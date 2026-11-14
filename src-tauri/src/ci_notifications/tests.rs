use super::*;
use agenthub_core::github_ci::{Account, Run};

fn snapshot() -> Snapshot {
    Snapshot {
        account: Some(Account {
            id: 1,
            login: "fixture".into(),
        }),
        enabled: true,
        scanned_repository_ids: BTreeSet::from([2]),
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

#[test]
fn filtered_observation_skips_bots_and_unselected_repos_without_retrigger_on_unfilter() {
    let mut observer = Observer::default();
    let mut state = snapshot();
    state.monitored_repositories = vec!["fixture/repo".into()];
    state.filter_bots = true;
    assert!(observer.observe(0, &state).is_empty());

    state.last_scan_ms += 1;
    let mut bot_run = run(2, "completed", Some("failure"), 1);
    bot_run.is_bot = true;
    state.runs[1] = bot_run;
    state.monitored_repositories = vec!["fixture/repo".into()];
    state.filter_bots = true;
    assert!(observer.observe(0, &state).is_empty());

    // Disabling bot filter on the next tick must not re-fire the already-completed bot run.
    state.last_scan_ms += 1;
    state.filter_bots = false;
    assert!(observer.observe(0, &state).is_empty());

    // A new attempt on an unselected repository is tracked without emitting an event.
    state.last_scan_ms += 1;
    state.runs[1].is_bot = false;
    state.runs[1].run_attempt = 2;
    state.monitored_repositories = vec!["fixture/other".into()];
    state.filter_bots = true;
    assert!(observer.observe(0, &state).is_empty());
}

#[test]
fn new_readded_and_empty_repositories_get_independent_baselines() {
    let mut observer = Observer::default();
    let mut state = snapshot();
    observer.observe(0, &state);
    let mut added = run(5, "completed", Some("success"), 1);
    added.repository_id = 3;
    added.repository_name = "fixture/new".into();
    state.runs.push(added);
    state.scanned_repository_ids.extend([3, 4]); // Repo 4 was scanned successfully but is empty.
    state.last_scan_ms += 1;
    assert!(observer.observe(0, &state).is_empty());
    state.runs[2].run_attempt = 2;
    state.runs[2].status = "in_progress".into();
    state.runs[2].conclusion = None;
    state.last_scan_ms += 1;
    assert!(observer.observe(0, &state).is_empty());
    state.runs[2].status = "completed".into();
    state.runs[2].conclusion = Some("success".into());
    let mut first = run(6, "completed", Some("failure"), 1);
    first.repository_id = 4;
    state.runs.push(first);
    state.last_scan_ms += 1;
    assert_eq!(observer.observe(0, &state).len(), 2);
    // Config changes may prune the source set before last_scan_ms advances.
    state.scanned_repository_ids.remove(&3);
    state.runs.retain(|run| run.repository_id != 3);
    observer.observe(0, &state);
    let mut readded = run(7, "completed", Some("success"), 1);
    readded.repository_id = 3;
    state.runs.push(readded);
    state.scanned_repository_ids.insert(3);
    state.last_scan_ms += 1;
    assert!(observer.observe(0, &state).is_empty());
}
