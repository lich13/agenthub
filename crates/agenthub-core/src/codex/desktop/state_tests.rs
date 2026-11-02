use super::{DesktopError, DesktopEvent, NativeState, RuntimeState};
use serde_json::{json, Value};

const THREAD_ID: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_THREAD_ID: &str = "22222222-2222-4222-8222-222222222222";

fn state_for(thread_id: &str) -> Value {
    json!({
        "id": thread_id,
        "hostId": "local",
        "parentThreadId": null,
        "archived": false,
        "source": {},
        "threadSource": {},
        "threadRuntimeStatus": {"type": "idle"},
        "turns": [],
        "requests": []
    })
}

fn replica_with(thread_id: &str, state: Value, revision: u64) -> NativeState {
    let mut replica = NativeState::new(thread_id).expect("fixture thread ID is valid");
    replica
        .apply(DesktopEvent::Snapshot { revision, state })
        .expect("fixture snapshot is accepted");
    replica
}

fn assert_invalidated(replica: &NativeState) {
    assert_eq!(replica.revision(), None);
    assert_eq!(replica.runtime(), RuntimeState::Unavailable);
    assert_eq!(replica.check_writable(), Err(DesktopError::OutOfSync));
}

#[test]
fn canonical_history_is_used_when_legacy_turns_is_empty() {
    let mut state = state_for(THREAD_ID);
    state["threadRuntimeStatus"] = json!({"type": "active"});
    state["turnHistory"] = json!({
        "kind": "canonical",
        "history": {
            "entitiesByKey": {
                "fixture-turn-key": {
                    "turnId": "fixture-turn-canonical",
                    "status": "inProgress",
                    "items": [{
                        "type": "userMessage",
                        "clientId": "fixture-submission-canonical"
                    }]
                }
            }
        }
    });
    state["turns"] = json!([]);

    let replica = replica_with(THREAD_ID, state, 7);

    assert_eq!(replica.turns().count(), 1);
    assert_eq!(replica.active_turn_id().unwrap(), "fixture-turn-canonical");
    assert!(replica.contains_submission("fixture-submission-canonical"));
    assert!(!replica.contains_submission("fixture-submission-absent"));
}

#[test]
fn legacy_turns_are_used_when_canonical_history_is_absent() {
    let mut state = state_for(THREAD_ID);
    state["threadRuntimeStatus"] = json!({"type": "active"});
    state["turns"] = json!([{
        "id": "fixture-turn-legacy",
        "status": "inProgress",
        "items": [{
            "type": "userMessage",
            "clientId": "fixture-submission-legacy"
        }]
    }]);

    let replica = replica_with(THREAD_ID, state, 3);

    assert_eq!(replica.turns().count(), 1);
    assert_eq!(replica.active_turn_id().unwrap(), "fixture-turn-legacy");
    assert!(replica.contains_submission("fixture-submission-legacy"));
}

#[test]
fn snapshot_for_a_different_thread_invalidates_the_replica() {
    let mut replica = NativeState::new(THREAD_ID).expect("fixture thread ID is valid");

    assert_eq!(
        replica.apply(DesktopEvent::Snapshot {
            revision: 4,
            state: state_for(OTHER_THREAD_ID),
        }),
        Err(DesktopError::InvalidIdentity)
    );
    assert_invalidated(&replica);
}

#[test]
fn patch_base_revision_gap_invalidates_the_replica() {
    let mut replica = replica_with(THREAD_ID, state_for(THREAD_ID), 10);

    assert_eq!(
        replica.apply(DesktopEvent::Patches {
            revision: 13,
            change: json!({
                "baseRevision": 11,
                "patches": [{"op": "replace", "path": ["title"], "value": "fixture"}]
            }),
        }),
        Err(DesktopError::OutOfSync)
    );
    assert_invalidated(&replica);
}

#[test]
fn immer_array_paths_insert_remove_and_replace_items() {
    let mut state = state_for(THREAD_ID);
    state["items"] = json!(["alpha", "beta", "gamma"]);
    let mut replica = replica_with(THREAD_ID, state, 2);

    replica
        .apply(DesktopEvent::Patches {
            revision: 8,
            change: json!({
                "baseRevision": 2,
                "patches": [
                    {"op": "add", "path": ["items", 1], "value": "inserted"},
                    {"op": "remove", "path": ["items", 0]},
                    {"op": "replace", "path": ["items", 2], "value": "replaced"}
                ]
            }),
        })
        .expect("array patches use Immer index paths");

    assert_eq!(replica.revision(), Some(8));
    assert_eq!(
        replica.value()["items"],
        json!(["inserted", "beta", "replaced"])
    );
}

#[test]
fn unknown_patch_operation_is_rejected_and_invalidates_the_replica() {
    let mut replica = replica_with(THREAD_ID, state_for(THREAD_ID), 1);

    assert_eq!(
        replica.apply(DesktopEvent::Patches {
            revision: 2,
            change: json!({
                "baseRevision": 1,
                "patches": [{"op": "copy", "path": ["title"], "value": "fixture"}]
            }),
        }),
        Err(DesktopError::UnsupportedProtocol)
    );
    assert_invalidated(&replica);
}

#[test]
fn a_later_patch_failure_discards_the_partially_applied_replica() {
    let mut state = state_for(THREAD_ID);
    state["items"] = json!(["before"]);
    let mut replica = replica_with(THREAD_ID, state, 5);

    assert_eq!(
        replica.apply(DesktopEvent::Patches {
            revision: 6,
            change: json!({
                "baseRevision": 5,
                "patches": [
                    {"op": "replace", "path": ["items", 0], "value": "partially-applied"},
                    {"op": "replace", "path": ["missing"], "value": "invalid"}
                ]
            }),
        }),
        Err(DesktopError::OutOfSync)
    );
    assert_invalidated(&replica);
    assert_eq!(replica.value(), &Value::Null);
}

#[test]
fn subthreads_remote_threads_and_archived_threads_are_read_only() {
    let mut subthread = state_for(THREAD_ID);
    subthread["parentThreadId"] = json!(OTHER_THREAD_ID);

    let mut source_subagent = state_for(THREAD_ID);
    source_subagent["source"]["subagent"] = json!({"taskName": "fixture-child"});

    let mut thread_source_subagent = state_for(THREAD_ID);
    thread_source_subagent["threadSource"]["subagent"] = json!({"taskName": "fixture-child"});

    let mut remote = state_for(THREAD_ID);
    remote["hostId"] = json!("fixture-remote-host");

    let mut archived = state_for(THREAD_ID);
    archived["archived"] = json!(true);

    for (name, state) in [
        ("parent-linked subthread", subthread),
        ("source subagent", source_subagent),
        ("thread-source subagent", thread_source_subagent),
        ("remote thread", remote),
        ("archived thread", archived),
    ] {
        let replica = replica_with(THREAD_ID, state, 1);
        assert_eq!(
            replica.check_writable(),
            Err(DesktopError::ReadOnly),
            "{name} must stay read-only"
        );
    }
}

#[test]
fn native_queue_and_pending_requests_prevent_idle() {
    let mut queued = replica_with(THREAD_ID, state_for(THREAD_ID), 1);
    assert_eq!(queued.check_idle(), Ok(()));

    queued
        .apply(DesktopEvent::Queue {
            messages: vec![json!({"id": "fixture-queued-message"})],
        })
        .unwrap();
    assert_eq!(queued.check_idle(), Err(DesktopError::Busy));

    queued
        .apply(DesktopEvent::Queue {
            messages: Vec::new(),
        })
        .unwrap();
    assert_eq!(queued.check_idle(), Ok(()));

    let mut state_with_request = state_for(THREAD_ID);
    state_with_request["requests"] = json!([{
        "id": "fixture-pending-request",
        "method": "fixture/question",
        "params": {"threadId": THREAD_ID}
    }]);
    let pending = replica_with(THREAD_ID, state_with_request, 1);
    assert_eq!(pending.check_idle(), Err(DesktopError::Busy));
}

#[test]
fn pending_request_requires_matching_identity_and_must_not_be_expired() {
    let mut state = state_for(THREAD_ID);
    state["requests"] = json!([
        {
            "id": "fixture-current-request",
            "method": "fixture/question",
            "params": {"threadId": THREAD_ID}
        },
        {
            "id": "fixture-other-thread-request",
            "method": "fixture/question",
            "params": {"threadId": OTHER_THREAD_ID}
        }
    ]);
    let replica = replica_with(THREAD_ID, state, 9);

    assert_eq!(
        replica
            .pending_request(&json!("fixture-current-request"), "fixture/question")
            .unwrap()["params"]["threadId"],
        THREAD_ID
    );
    assert_eq!(
        replica
            .pending_request(&json!("fixture-current-request"), "fixture/other-method")
            .unwrap_err(),
        DesktopError::RequestExpired
    );
    assert_eq!(
        replica
            .pending_request(&json!("fixture-expired-request"), "fixture/question")
            .unwrap_err(),
        DesktopError::RequestExpired
    );
    assert_eq!(
        replica
            .pending_request(&json!("fixture-other-thread-request"), "fixture/question")
            .unwrap_err(),
        DesktopError::InvalidIdentity
    );
}
