use std::collections::HashSet;

use agenthub_core::{
    codex::{ThreadDetail, ThreadStatus, ThreadSummary},
    db::JobRecord,
    platform::{PlatformKind, PlatformPaths},
    services::{
        probe::{probe_status_snapshot_view, ProbeSnapshotStatus},
        threads::{thread_detail_read_model, thread_list_read_model, ThreadReadModelInputs},
    },
};
use serde_json::json;

#[test]
fn core_thread_read_model_merges_running_jobs_without_autosubmit_effects() {
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);
    let known_running_job = running_job("job-running", "known-thread", Some("turn-running"), 20);
    let new_running_job = running_job("job-new", "new-running-thread", None, 30);

    let view = thread_list_read_model(
        &platform,
        ThreadReadModelInputs {
            threads: vec![
                thread(
                    "known-thread",
                    ThreadStatus::Recent,
                    Some("2026-06-24T10:00:00Z"),
                ),
                thread(
                    "idle-thread",
                    ThreadStatus::Recent,
                    Some("2026-06-24T11:00:00Z"),
                ),
                thread(
                    "hidden-thread",
                    ThreadStatus::Recent,
                    Some("2026-06-24T12:00:00Z"),
                ),
                thread(
                    "archived-thread",
                    ThreadStatus::Archived,
                    Some("2026-06-24T09:00:00Z"),
                ),
            ],
            running_jobs: vec![known_running_job, new_running_job],
            hidden_thread_ids: HashSet::from(["hidden-thread".to_string()]),
            archived_thread_ids: HashSet::from(["archived-thread".to_string()]),
        },
        agenthub_core::services::threads::ThreadsQuery {
            archived: Some(false),
            q: None,
            limit: Some(20),
        },
    )
    .expect("thread read model should be planned in core");

    let ids = view
        .threads
        .iter()
        .map(|thread| thread.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&"new-running-thread"));
    assert!(ids.contains(&"known-thread"));
    assert!(ids.contains(&"idle-thread"));
    assert!(!ids.contains(&"archived-thread"));
    assert_eq!(
        view.threads
            .iter()
            .find(|thread| thread.id == "known-thread")
            .map(|thread| &thread.status),
        Some(&ThreadStatus::Running)
    );
    assert!(serde_json::to_value(&view)
        .unwrap()
        .get("autosubmitEffects")
        .is_none());

    let idle_view = thread_list_read_model(
        &platform,
        ThreadReadModelInputs {
            threads: vec![thread(
                "idle-thread",
                ThreadStatus::Recent,
                Some("2026-06-24T11:00:00Z"),
            )],
            running_jobs: vec![],
            hidden_thread_ids: HashSet::new(),
            archived_thread_ids: HashSet::new(),
        },
        agenthub_core::services::threads::ThreadsQuery {
            archived: Some(false),
            q: None,
            limit: Some(20),
        },
    )
    .expect("idle read model must remain read-only");

    assert!(serde_json::to_value(&idle_view)
        .unwrap()
        .get("autosubmitEffects")
        .is_none());
}

#[test]
fn core_thread_detail_read_model_returns_updated_detail_without_autosubmit_effect() {
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);
    let detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: thread(
            "idle-thread",
            ThreadStatus::Recent,
            Some("2026-06-24T11:00:00Z"),
        ),
        messages: vec![],
        blocks: vec![],
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };

    let view = thread_detail_read_model(&platform, detail, None)
        .expect("core should own detail read-model");

    assert_eq!(view.detail.summary.status, ThreadStatus::Recent);
    assert!(serde_json::to_value(&view)
        .unwrap()
        .get("autosubmitEffects")
        .is_none());
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

fn running_job(id: &str, thread_id: &str, turn_id: Option<&str>, started_at: i64) -> JobRecord {
    JobRecord {
        id: id.to_string(),
        kind: "codex".to_string(),
        status: "running".to_string(),
        title: format!("Job {id}"),
        thread_id: Some(thread_id.to_string()),
        turn_id: turn_id.map(str::to_string),
        started_at,
        finished_at: None,
        exit_code: None,
        output: String::new(),
        error: None,
    }
}
