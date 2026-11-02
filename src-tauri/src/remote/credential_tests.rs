use super::*;
use std::future::Future;
use std::sync::mpsc;
use tokio::sync::oneshot;

const WATCHDOG: Duration = Duration::from_secs(10);
const QUEUE_TIMEOUT: Duration = Duration::from_millis(40);
const FIXTURE_REVISION: u64 = 731;
const FIXTURE_MACHINE_ID: &str = "m-fixture";
const CHANGED_MACHINE_ID: &str = "m-changed";
const FIXTURE_BASE: &str = "https://credential-fixture.example.invalid/agenthub/";
const CHANGED_BASE: &str = "https://changed-fixture.example.invalid/agenthub/";

fn fixture_connection() -> RemoteConnections {
    RemoteConnections {
        state: Arc::new(Mutex::new(ConnectionState {
            prefs: Preferences {
                target: MachineTarget::Remote,
                active_machine_id: Some(FIXTURE_MACHINE_ID.into()),
                machines: vec![RemoteMachineSummary {
                    id: FIXTURE_MACHINE_ID.into(),
                    name: "Fixture".into(),
                    base_url: FIXTURE_BASE.into(),
                }],
                base_url: None,
                revision: FIXTURE_REVISION,
            },
            ..Default::default()
        })),
        path: PathBuf::new(),
        http: Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .build()
            .expect("fixture HTTP client failed"),
        credentials: CredentialAccess::default(),
    }
}

struct ReleaseWork(Option<mpsc::Sender<()>>);

impl Drop for ReleaseWork {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(WATCHDOG, future)
        .await
        .expect("fixture watchdog elapsed")
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_transaction_keeps_connection_change_guard_until_work_finishes() {
    let connection = fixture_connection();
    let credentials = connection.credentials.clone();
    let guard = connection
        .begin_change(FIXTURE_REVISION)
        .expect("initial change was rejected");
    let (release, released) = mpsc::channel();
    let release = ReleaseWork(Some(release));
    let (started, start) = oneshot::channel();
    let (finished, finish) = oneshot::channel();
    let caller = tokio::spawn(async move {
        credentials
            .transaction(QUEUE_TIMEOUT, move || {
                {
                    let _guard = guard;
                    started.send(()).expect("start observer disappeared");
                    released
                        .recv_timeout(WATCHDOG)
                        .expect("fixture worker was not released");
                }
                let _ = finished.send(());
            })
            .await
    });

    bounded(start).await.expect("transaction did not start");
    caller.abort();
    assert!(bounded(caller)
        .await
        .expect_err("transaction caller was not cancelled")
        .is_cancelled());
    assert!(connection.state.lock().unwrap().changing);
    assert_eq!(
        connection
            .begin_change(FIXTURE_REVISION)
            .err()
            .expect("a change started while native work was still running"),
        "操作进行中，请稍后切换"
    );
    assert_eq!(
        connection
            .require_current_remote(FIXTURE_REVISION, FIXTURE_MACHINE_ID, FIXTURE_BASE)
            .expect_err("a remote request passed while the connection was changing"),
        "连接已变化，操作已取消"
    );

    drop(release);
    bounded(finish).await.expect("transaction did not finish");
    assert!(!connection.state.lock().unwrap().changing);
    connection
        .require_current_remote(FIXTURE_REVISION, FIXTURE_MACHINE_ID, FIXTURE_BASE)
        .expect("completed work left the connection unavailable");
    let next_guard = connection
        .begin_change(FIXTURE_REVISION)
        .expect("completed work retained its change guard");
    drop(next_guard);
}

#[test]
fn current_remote_rejects_changed_revision_target_base_and_change_in_progress() {
    let mutations: [fn(&mut ConnectionState); 6] = [
        |state| state.prefs.revision += 1,
        |state| state.prefs.target = MachineTarget::Local,
        |state| state.prefs.active_machine_id = Some(CHANGED_MACHINE_ID.into()),
        |state| state.prefs.machines[0].base_url = CHANGED_BASE.into(),
        |state| state.prefs.machines.clear(),
        |state| state.changing = true,
    ];

    for mutate in mutations {
        let connection = fixture_connection();
        connection
            .require_current_remote(FIXTURE_REVISION, FIXTURE_MACHINE_ID, FIXTURE_BASE)
            .expect("unchanged remote identity was rejected");
        mutate(&mut connection.state.lock().unwrap());
        let error = connection
            .require_current_remote(FIXTURE_REVISION, FIXTURE_MACHINE_ID, FIXTURE_BASE)
            .expect_err("changed remote identity was accepted");
        assert_eq!(error, "连接已变化，操作已取消");
        assert!(!error.contains(FIXTURE_BASE));
        assert!(!error.contains(CHANGED_BASE));
        assert!(!error.contains(&FIXTURE_REVISION.to_string()));
    }
}

#[test]
fn current_remote_rejects_stale_caller_identity_without_exposing_input() {
    let connection = fixture_connection();
    for (revision, machine_id, base) in [
        (FIXTURE_REVISION + 1, FIXTURE_MACHINE_ID, FIXTURE_BASE),
        (FIXTURE_REVISION, CHANGED_MACHINE_ID, FIXTURE_BASE),
        (FIXTURE_REVISION, FIXTURE_MACHINE_ID, CHANGED_BASE),
    ] {
        let error = connection
            .require_current_remote(revision, machine_id, base)
            .expect_err("stale remote request was accepted");
        assert_eq!(error, "连接已变化，操作已取消");
        assert!(!error.contains(base));
        assert!(!error.contains(machine_id));
        assert!(!error.contains(&revision.to_string()));
    }
}

#[test]
fn credential_errors_use_fixed_public_messages() {
    assert_eq!(
        credential_access_error(CredentialAccessError::TimedOut),
        "钥匙串未及时响应，请检查系统授权后重试"
    );
    assert_eq!(
        credential_access_error(CredentialAccessError::WorkerUnavailable),
        "钥匙串操作失败，请重试"
    );
}
