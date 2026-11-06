use super::*;
use agenthub_core::github_ci::{Account, Run};

fn snapshot() -> Snapshot {
    Snapshot {
        account: Some(Account {
            id: 1,
            login: "fixture".into(),
        }),
        enabled: true,
        baseline_ms: 1,
        last_scan_ms: 10,
        runs: vec![
            run(1, "completed", Some("success"), 1),
            run(2, "in_progress", None, 1),
        ],
        ..Default::default()
    }
}

fn run(id: u64, status: &str, conclusion: Option<&str>, attempt: u64) -> Run {
    serde_json::from_value(serde_json::json!({
        "id":id,"run_attempt":attempt,"name":"Build","head_sha":"0123456789","head_branch":"main",
        "status":status,"conclusion":conclusion,"html_url":"https://github.com/fixture/repo/actions/runs/1",
        "created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z",
        "repository_id":2,"repository_name":"fixture/repo"
    })).unwrap()
}

#[test]
fn first_observation_repeats_and_only_three_terminal_results() {
    for result in [
        "success",
        "failure",
        "timed_out",
        "cancelled",
        "skipped",
        "neutral",
    ] {
        let mut observer = Observer::default();
        let mut state = snapshot();
        assert!(observer.observe(0, &state).is_empty());
        state.last_scan_ms += 1;
        state.runs[1] = run(2, "completed", Some(result), 1);
        let events = observer.observe(0, &state);
        assert_eq!(
            events.len(),
            usize::from(matches!(result, "success" | "failure" | "timed_out"))
        );
        state.last_scan_ms += 1;
        assert!(observer.observe(0, &state).is_empty());
    }
}

#[test]
fn reruns_new_runs_and_scope_resets() {
    let mut observer = Observer::default();
    let mut state = snapshot();
    observer.observe(1, &state);
    state.last_scan_ms += 1;
    state.runs[0].run_attempt = 2;
    state.runs.push(run(3, "completed", Some("timed_out"), 1));
    assert_eq!(observer.observe(1, &state).len(), 2);
    assert!(observer.observe(2, &state).is_empty());
    state.account.as_mut().unwrap().id = 2;
    assert!(observer.observe(2, &state).is_empty());
    state.baseline_ms += 1;
    assert!(observer.observe(2, &state).is_empty());
    state.enabled = false;
    assert!(observer.observe(2, &state).is_empty());
    state.enabled = true;
    assert!(observer.observe(2, &state).is_empty());
    state.last_scan_ms += 1;
    state.runs[1] = run(2, "completed", Some("failure"), 1);
    assert_eq!(observer.observe(2, &state).len(), 1);
    state.last_scan_ms -= 2;
    assert!(observer.observe(2, &state).is_empty());
}

#[test]
fn presentation_includes_source_workflow_branch_and_distinct_result() {
    for (result, label) in [
        ("success", "成功"),
        ("failure", "失败"),
        ("timed_out", "超时"),
    ] {
        let (title, body) = run(1, "completed", Some(result), 1).notification_text("fixture-host");
        assert!(title.contains(label));
        assert!(title.contains("fixture/repo"));
        assert_eq!(body, "fixture-host\nBuild\nmain · 0123456");
    }
}
