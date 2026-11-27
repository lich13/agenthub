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
        repository_observed_ms: BTreeMap::from([(2, 10)]),
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
fn unrelated_repository_updates_do_not_advance_failed_repository_observations() {
    let mut observer = Observer::default();
    let mut state = snapshot();
    let time = state.runs[1]
        .updated_at
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond();
    state.runs[1].is_bot = true;
    state.last_scan_ms = time - 5000;
    state.repository_baselines_ms.insert(2, time - 10000);
    state.repository_observed_ms.insert(2, time - 5000);
    observer.observe(0, &state);
    state.bot_baseline_ms = time + 1000;
    state.last_scan_ms = time + 2000;
    state.repository_observed_ms.insert(3, time + 2000);
    assert!(observer.observe(0, &state).is_empty());
    state.runs[1].status = "completed".into();
    state.runs[1].conclusion = Some("success".into());
    state.repository_observed_ms.insert(2, time + 3000);
    state.last_scan_ms = time + 3000;
    assert!(observer.observe(0, &state).is_empty());
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

#[test]
fn source_baselines_cover_repository_readd_and_bot_toggle_missed_between_polls() {
    for bot in [false, true] {
        let mut observer = Observer::default();
        let mut state = snapshot();
        state.runs[1].is_bot = bot;
        observer.observe(0, &state);
        let finished = state.runs[1]
            .updated_at
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        if bot {
            state.bot_baseline_ms = finished + 1000;
        } else {
            state.repository_baselines_ms.insert(2, finished + 1000);
        }
        state.runs[1].status = "completed".into();
        state.runs[1].conclusion = Some("success".into());
        let mut unseen = run(3, "completed", Some("failure"), 1);
        unseen.is_bot = bot;
        state.runs.push(unseen);
        state.last_scan_ms += 1;
        assert!(observer.observe(0, &state).is_empty());
        state.runs[1].run_attempt += 1;
        state.runs[1].updated_at = jiff::Timestamp::from_millisecond(finished + 2000)
            .unwrap()
            .to_string();
        assert_eq!(observer.observe(0, &state).len(), 1);
        assert!(observer.observe(0, &state).is_empty());
    }
}

#[test]
fn observed_completion_in_baseline_second_notifies_but_unseen_history_does_not() {
    for bot in [false, true] {
        let mut observer = Observer::default();
        let mut state = snapshot();
        let second = state.runs[1]
            .updated_at
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        state.baseline_ms = second + 100;
        state.last_scan_ms = second + 400;
        state.repository_observed_ms.insert(2, second + 400);
        state.repository_baselines_ms.insert(2, second + 200);
        state.bot_baseline_ms = second + 300;
        for run in &mut state.runs {
            run.is_bot = bot;
        }
        assert!(observer.observe(0, &state).is_empty());
        state.last_scan_ms = second + 800;
        state.runs[1].status = "completed".into();
        state.runs[1].conclusion = Some("success".into());
        let mut unseen = run(3, "completed", Some("failure"), 1);
        unseen.is_bot = bot;
        state.runs.push(unseen);
        let events = observer.observe(0, &state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].run.id, 2);
        assert!(observer.observe(0, &state).is_empty());
        state.runs[1].run_attempt += 1;
        assert_eq!(observer.observe(0, &state).len(), 1);
    }
}

#[test]
fn bot_toggle_same_second_keeps_only_the_observed_attempt_with_a_current_repository() {
    for (prior_status, attempt, seconds_before, readded, expected) in [
        ("in_progress", 1, 0, false, 1),
        ("in_progress", 1, 1, false, 0),
        ("in_progress", 2, 0, false, 0),
        ("completed", 1, 0, false, 0),
        ("in_progress", 1, 0, true, 0),
    ] {
        let mut observer = Observer::default();
        let mut state = snapshot();
        state.runs = vec![run(
            2,
            prior_status,
            (prior_status == "completed").then_some("cancelled"),
            1,
        )];
        state.runs[0].is_bot = true;
        let second = state.runs[0]
            .updated_at
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        state.last_scan_ms = second - 5000;
        state.repository_observed_ms.insert(2, second - 5000);
        assert!(observer.observe(0, &state).is_empty());
        state.bot_baseline_ms = second + 450;
        if readded {
            state.repository_baselines_ms.insert(2, second + 400);
        }
        state.runs[0].status = "completed".into();
        state.runs[0].conclusion = Some("success".into());
        state.runs[0].run_attempt = attempt;
        state.runs[0].updated_at =
            jiff::Timestamp::from_millisecond(second - seconds_before * 1000)
                .unwrap()
                .to_string();
        let mut unseen = state.runs[0].clone();
        unseen.id = 3;
        state.runs.push(unseen);
        state.last_scan_ms = second + 1000;
        let events = observer.observe(0, &state);
        assert_eq!(events.len(), expected);
        if expected == 1 {
            assert_eq!(events[0].run.id, 2);
        }
        assert!(observer.observe(0, &state).is_empty());
    }
}

#[test]
fn stale_idle_snapshot_on_app_open_notifies_only_subsequent_completed_run() {
    let mut observer = Observer::default();
    let idle_time = 1_800_000_000_000_i64;
    let mut state = Snapshot {
        account: Some(Account {
            id: 1,
            login: "fixture".into(),
        }),
        enabled: true,
        scanned_repository_ids: BTreeSet::from([2]),
        baseline_ms: idle_time - 60_000,
        last_scan_ms: idle_time,
        repository_baselines_ms: BTreeMap::from([(2, idle_time - 60_000)]),
        repository_observed_ms: BTreeMap::from([(2, idle_time)]),
        runs: vec![run(1, "completed", Some("success"), 1)],
        ..Default::default()
    };
    // App opens 5 minutes after the last state write because idle polls skipped rewriting unchanged state.
    assert!(observer.observe(0, &state).is_empty());
    assert!(observer.observe(0, &state).is_empty());

    // A new workflow run completes later and advances last_scan_ms / repository_observed_ms.
    let finish_ms = idle_time + 330_000;
    let mut newly_completed = run(3, "completed", Some("failure"), 1);
    newly_completed.updated_at = jiff::Timestamp::from_millisecond(finish_ms)
        .unwrap()
        .to_string();
    state.runs.insert(0, newly_completed);
    state.repository_observed_ms.insert(2, finish_ms);
    state.last_scan_ms = finish_ms;

    let events = observer.observe(0, &state);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].run.id, 3);
    assert!(observer.observe(0, &state).is_empty());
}

#[test]
fn observed_in_progress_run_finishing_after_unchanged_polls_still_notifies() {
    let mut observer = Observer::default();
    let start_ms = 1_800_000_000_000_i64;
    let mut in_flight = run(2, "in_progress", None, 1);
    in_flight.updated_at = jiff::Timestamp::from_millisecond(start_ms)
        .unwrap()
        .to_string();
    let mut state = Snapshot {
        account: Some(Account {
            id: 1,
            login: "fixture".into(),
        }),
        enabled: true,
        scanned_repository_ids: BTreeSet::from([2]),
        baseline_ms: start_ms - 10_000,
        last_scan_ms: start_ms,
        repository_baselines_ms: BTreeMap::from([(2, start_ms - 10_000)]),
        repository_observed_ms: BTreeMap::from([(2, start_ms)]),
        runs: vec![in_flight],
        ..Default::default()
    };
    assert!(observer.observe(0, &state).is_empty());
    // Unchanged in-progress polls keep last_scan_ms untouched.
    assert!(observer.observe(0, &state).is_empty());

    let done_ms = start_ms + 45_000;
    state.runs[0].status = "completed".into();
    state.runs[0].conclusion = Some("success".into());
    state.runs[0].updated_at = jiff::Timestamp::from_millisecond(done_ms)
        .unwrap()
        .to_string();
    state.repository_observed_ms.insert(2, done_ms);
    state.last_scan_ms = done_ms;

    let events = observer.observe(0, &state);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].run.id, 2);
    assert!(observer.observe(0, &state).is_empty());
}
