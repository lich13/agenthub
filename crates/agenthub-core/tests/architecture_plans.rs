use agenthub_core::{
    codex::{ThreadStatus, ThreadSummary},
    services::{
        probe::{probe_status_snapshot_view, ProbeSnapshotStatus},
        threads::thread_list_read_model,
    },
};
use serde_json::json;

#[test]
fn core_thread_read_model_preserves_native_status_and_filters_archived() {
    let view = thread_list_read_model(
        vec![
            thread(
                "running",
                ThreadStatus::Running,
                Some("2026-06-24T10:00:00Z"),
            ),
            thread("idle", ThreadStatus::Recent, Some("2026-06-24T11:00:00Z")),
            thread("archived", ThreadStatus::Archived, None),
        ],
        Default::default(),
    );
    assert_eq!(view.len(), 2);
    assert_eq!(view[0].id, "idle");
    assert_eq!(view[1].status, ThreadStatus::Running);
    assert!(view.iter().all(|row| row.active_job_id.is_none()));
}

#[test]
fn core_probe_views_own_snapshot_metadata() {
    let snapshot = probe_status_snapshot_view(
        json!({"label": "Probe"}),
        42,
        true,
        ProbeSnapshotStatus::Cached,
    );
    assert_eq!(snapshot["snapshot_age_seconds"], 42);
    assert_eq!(snapshot["is_refreshing"], true);
    assert_eq!(snapshot["snapshot_status"], "cached");
}

fn thread(id: &str, status: ThreadStatus, updated_at: Option<&str>) -> ThreadSummary {
    ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: id.to_string(),
        title: format!("Thread {id}"),
        status,
        updated_at: updated_at.map(str::to_string),
        archived_at: None,
        message_count: 1,
        latest_message: None,
        cwd: None,
        model: None,
        rollout_path: None,
        active_turn_id: None,
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: None,
    }
}
