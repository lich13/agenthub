use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn ci_delivery_rejects_changed_changing_and_closed_connections() {
    let (_root, connections) = remote_fixture();
    let revision = connections.view().revision;
    assert!(connections.is_current(revision));
    assert!(!connections.is_current(revision + 1));
    connections.state.lock().unwrap().changing = true;
    assert!(!connections.is_current(revision));
    connections.state.lock().unwrap().changing = false;
    connections.disconnect();
    assert!(!connections.is_current(revision));
}

fn remote_fixture() -> (tempfile::TempDir, RemoteConnections) {
    let root = tempfile::tempdir().unwrap();
    let connection = RemoteConnections::load(root.path()).unwrap();
    let prefs = Preferences {
        target: MachineTarget::Remote,
        active_machine_id: Some("fixture".into()),
        machines: vec![RemoteMachineSummary {
            id: "fixture".into(),
            name: "Fixture".into(),
            ssh_host: "fixture-host".into(),
            use_sudo: false,
        }],
        ..Default::default()
    };
    connection.commit(prefs, None).unwrap();
    (root, connection)
}

#[tokio::test]
async fn concurrent_recovery_shares_success_and_authentication_failure() {
    for succeed in [true, false] {
        let (_root, connection) = remote_fixture();
        let calls = AtomicUsize::new(0);
        let connect = |_| async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if succeed {
                Ok(ssh::fixture("reply"))
            } else {
                Err("authentication failed".into())
            }
        };
        let (first, second) = tokio::join!(
            connection.ensure_client_with(1, connect),
            connection.ensure_client_with(1, connect)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(connection.view().revision, 1);
        if succeed {
            assert!(Arc::ptr_eq(&first.unwrap(), &second.unwrap()));
        } else {
            assert_eq!(first.err(), second.err());
            assert!(connection.state.lock().unwrap().client.is_none());
        }
    }
}

#[tokio::test]
async fn late_handshake_cannot_restore_a_switched_or_closed_connection() {
    for close in [false, true] {
        let (_root, connection) = remote_fixture();
        let client = ssh::fixture("reply");
        let result = connection
            .ensure_client_with(1, |_| async {
                if close {
                    connection.disconnect();
                } else {
                    let _guard = connection.begin_change(1).unwrap();
                    connection.commit(Preferences::default(), None).unwrap();
                }
                Ok(client.clone())
            })
            .await;
        assert!(result.is_err());
        assert!(!client.is_connected());
        assert!(connection.state.lock().unwrap().client.is_none());
    }
}

#[tokio::test]
async fn failed_switch_does_not_leave_a_closed_recovery_attempt() {
    let (_root, connection) = remote_fixture();
    let guard = Mutex::new(None);
    let result = connection
        .ensure_client_with(1, |_| async {
            *guard.lock().unwrap() = Some(connection.begin_change(1).unwrap());
            Ok(ssh::fixture("reply"))
        })
        .await;
    assert!(result.is_err());
    drop(guard.lock().unwrap().take());
    assert!(connection
        .ensure_client_with(1, |_| async { Ok(ssh::fixture("reply")) })
        .await
        .unwrap()
        .is_connected());
}

#[tokio::test]
async fn reads_retry_once_and_writes_are_not_replayed() {
    for (command, expected_calls) in [("jobs.list", 2), ("threads.rename", 1)] {
        let (_root, connection) = remote_fixture();
        let calls = AtomicUsize::new(0);
        let result = connection
            .invoke_with(
                &RemoteInvokeRequest {
                    revision: 1,
                    command: command.into(),
                    args: json!({}),
                },
                |_| async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(ssh::fixture("disconnect"))
                },
            )
            .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        assert!(connection.state.lock().unwrap().client.is_none());
    }
    let (_root, connection) = remote_fixture();
    let calls = AtomicUsize::new(0);
    let result = connection
        .invoke_with(
            &RemoteInvokeRequest {
                revision: 1,
                command: "jobs.list".into(),
                args: json!({}),
            },
            |_| async {
                let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                Ok(ssh::fixture(if first { "disconnect" } else { "reply" }))
            },
        )
        .await
        .unwrap();
    assert_eq!(result, "fixture");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(connection.view().revision, 1);
}

#[tokio::test]
async fn business_errors_keep_the_channel_and_do_not_retry() {
    let (_root, connection) = remote_fixture();
    let calls = AtomicUsize::new(0);
    let result = connection
        .invoke_with(
            &RemoteInvokeRequest {
                revision: 1,
                command: "jobs.detail".into(),
                args: json!({}),
            },
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ssh::fixture("error"))
            },
        )
        .await;
    assert_eq!(result.unwrap_err(), "job not found");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(connection
        .state
        .lock()
        .unwrap()
        .client
        .as_ref()
        .unwrap()
        .is_connected());
}

#[test]
fn saved_ssh_preferences_and_local_write_guard() {
    let root = tempfile::tempdir().unwrap();
    let connection = RemoteConnections::load(root.path()).unwrap();
    let guard = connection.begin_local_write(0).unwrap();
    assert!(connection.begin_change(0).is_err());
    drop(guard);
    let mut prefs = Preferences::default();
    prefs.machines.push(RemoteMachineSummary {
        id: "fixture".into(),
        name: "Fixture".into(),
        ssh_host: "fixture-host".into(),
        use_sudo: true,
    });
    connection.commit(prefs, None).unwrap();
    let restored = RemoteConnections::load(root.path()).unwrap();
    assert_eq!(restored.view().machines[0].ssh_host, "fixture-host");
    assert!(restored.require_local(0).is_err());
}
