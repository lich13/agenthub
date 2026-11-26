use super::*;
fn settings_changed(db: &PanelDb, before: &Config, after: &Config) -> Result<()> {
    db.with_transaction(|tx| super::settings_changed(tx, before, after))
}
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Database {
    db: PanelDb,
    root: PathBuf,
}
impl Database {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("agenthub-ci-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        Self {
            db: PanelDb::open_with_secret_box(
                root.join("panel.sqlite"),
                crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap(),
            root,
        }
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn account() -> Account {
    Account {
        id: 1,
        login: "fixture".into(),
    }
}
fn run(id: u64, status: &str, conclusion: Option<&str>, attempt: u64, time: i64) -> Run {
    Run {
        id,
        run_attempt: attempt,
        name: Some("CI".into()),
        head_branch: Some("main".into()),
        head_sha: "0123456789".into(),
        status: status.into(),
        conclusion: conclusion.map(str::to_owned),
        html_url: format!("https://github.com/fixture/repo/actions/runs/{id}"),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: jiff::Timestamp::from_millisecond(time).unwrap().to_string(),
        repository_id: 10,
        repository_name: "fixture/repo".into(),
        is_bot: false,
    }
}
#[test]
fn baseline_running_completion_restart_rerun_and_disable() {
    let db = Database::new();
    let mut state = Stored::default();
    let time = 1_800_000_000_000;
    reconcile(
        &mut state,
        account(),
        vec![
            run(1, "completed", Some("success"), 1, time - 1000),
            run(2, "in_progress", None, 1, time),
        ],
        true,
        true,
        time,
    );
    assert!(state.pending.is_empty());
    save(&db.db, &state).unwrap();
    let mut state = load(&db.db).unwrap();
    let complete = vec![
        run(1, "completed", Some("success"), 1, time - 1000),
        run(2, "completed", Some("failure"), 1, time + 1000),
    ];
    reconcile(
        &mut state,
        account(),
        complete.clone(),
        true,
        false,
        time + 2000,
    );
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].run.id, 2);
    save(&db.db, &state).unwrap();
    let key = state.pending[0].key();
    acknowledge(&db.db, std::slice::from_ref(&key)).unwrap();
    let mut state = load(&db.db).unwrap();
    reconcile(&mut state, account(), complete, true, false, time + 3000);
    assert!(state.pending.is_empty());
    reconcile(
        &mut state,
        account(),
        vec![run(2, "completed", Some("success"), 2, time + 4000)],
        true,
        false,
        time + 4000,
    );
    assert_eq!(state.pending.len(), 1);
    assert_ne!(key, state.pending[0].key());
    reconcile(
        &mut state,
        account(),
        vec![run(2, "in_progress", None, 3, time + 5000)],
        true,
        false,
        time + 5000,
    );
    assert!(state.pending.is_empty());
    reconcile(
        &mut state,
        account(),
        vec![run(2, "completed", Some("success"), 3, time + 6000)],
        false,
        false,
        time + 6000,
    );
    assert!(state.pending.is_empty());
    reconcile(
        &mut state,
        account(),
        vec![run(2, "completed", Some("success"), 3, time + 6000)],
        true,
        true,
        time + 7000,
    );
    assert!(state.pending.is_empty());
}
#[test]
fn initial_scan_skips_all_completed_runs() {
    let time = now();
    let mut state = Stored::default();
    reconcile(
        &mut state,
        account(),
        vec![
            run(1, "completed", Some("success"), 1, time - 1000),
            run(2, "completed", Some("success"), 1, time + 1000),
        ],
        true,
        true,
        time,
    );
    assert!(state.pending.is_empty());
}

#[test]
fn only_success_failure_timeout_notify_and_history_is_bounded() {
    let time = 1_800_000_000_000;
    let mut state = Stored {
        baseline_ms: time,
        ..Stored::default()
    };
    let runs = [
        "success",
        "failure",
        "timed_out",
        "cancelled",
        "skipped",
        "neutral",
        "action_required",
        "startup_failure",
    ]
    .iter()
    .enumerate()
    .map(|(i, c)| run(i as u64, "completed", Some(c), 1, time + 1000))
    .collect();
    reconcile(&mut state, account(), runs, true, false, time + 2000);
    assert_eq!(state.pending.len(), 3);
    let mut runs = (0..1100)
        .map(|i| run(i as u64, "completed", Some("success"), 1, time - 1000))
        .collect::<Vec<_>>();
    runs.push(run(9000, "in_progress", None, 1, time - 10000));
    reconcile(&mut state, account(), runs, true, true, time);
    assert_eq!(state.snapshot.runs.len(), 1101);
    assert!(state.snapshot.runs.iter().any(|r| r.id == 9000));
}
#[tokio::test]
async fn disabled_monitor_and_manual_refresh_only_read_cached_results() {
    let fixture = Database::new();
    let config = Config::current_defaults().unwrap();
    assert!(!enabled(&config));
    let mut monitor = Monitor::new().unwrap();
    assert!(monitor
        .tick(&fixture.db, &|| Ok(config.clone()))
        .await
        .unwrap()
        .is_empty());
    let before = fixture.db.get_setting(STATE).unwrap();
    assert!(before.is_none());
    for _ in 0..10 {
        assert!(monitor
            .tick(&fixture.db, &|| Ok(config.clone()))
            .await
            .unwrap()
            .is_empty());
    }
    assert_eq!(before, fixture.db.get_setting(STATE).unwrap());
    assert!(monitor.token.is_empty());
    let result = status(
        &fixture.db,
        &config,
        Request {
            refresh: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!result.refreshing);
    assert!(!result.enabled);
    assert_eq!(request_ms(&fixture.db).unwrap(), 0);
    fixture.db.set_setting(REFRESH, &now().to_string()).unwrap();
    assert!(monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            panic!("disabled CI must not read credentials even with an old refresh request")
        })
        .await
        .unwrap()
        .is_empty());
    assert!(
        !status(&fixture.db, &config, Request::default())
            .unwrap()
            .refreshing
    );
}

#[tokio::test]
async fn refresh_during_rate_limit_finishes_without_reading_credentials() {
    let fixture = Database::new();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    let mut state = Stored::default();
    state.snapshot.last_scan_ms = now() - 1000;
    save(&fixture.db, &state).unwrap();
    let mut monitor = Monitor::new().unwrap();
    monitor.retry_at = now() + 120000;
    for _ in 0..2 {
        status(
            &fixture.db,
            &config,
            Request {
                refresh: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                panic!("credentials must not be read during cooldown")
            })
            .await
            .unwrap()
            .is_empty());
        let status = status(
            &fixture.db,
            &config,
            Request {
                refresh: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!status.refreshing);
        assert!(status.error.unwrap().contains("限流"));
    }
    assert!(monitor.token.is_empty());
}

#[test]
fn saving_disable_and_reenable_resets_the_baseline_before_next_poll() {
    let fixture = Database::new();
    let mut active = Config::current_defaults().unwrap();
    active.probe.notifications.notify_github_ci = true;
    let inactive = Config::current_defaults().unwrap();
    let time = now();
    let mut state = Stored::default();
    reconcile(
        &mut state,
        account(),
        vec![run(1, "in_progress", None, 1, time)],
        true,
        true,
        time,
    );
    reconcile(
        &mut state,
        account(),
        vec![run(1, "completed", Some("success"), 1, time + 1)],
        true,
        false,
        time + 1,
    );
    save(&fixture.db, &state).unwrap();
    let old_event = state.pending[0].clone();
    settings_changed(&fixture.db, &active, &inactive).unwrap();
    settings_changed(&fixture.db, &inactive, &active).unwrap();
    assert!(!event_current(&fixture.db, &active, &old_event).unwrap());
    let reset = load(&fixture.db).unwrap();
    assert_eq!(reset.baseline_ms, 0);
    assert!(!reset.snapshot.enabled);
    assert!(reset.pending.is_empty());
    assert!(request_ms(&fixture.db).unwrap() > 0);
}

#[derive(Clone)]
struct Reply {
    status: u16,
    body: serde_json::Value,
    headers: String,
}
struct Server {
    url: String,
    replies: Arc<Mutex<HashMap<String, Reply>>>,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let replies = Arc::new(Mutex::new(HashMap::<String, Reply>::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let routes = replies.clone();
        let observed = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let routes = routes.clone();
                let observed = observed.clone();
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut buf = [0; 2048];
                    loop {
                        let n = stream.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buf[..n]);
                        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8(bytes).unwrap();
                    let path = request.split_whitespace().nth(1).unwrap().to_owned();
                    observed.lock().unwrap().push(request);
                    let reply = routes.lock().unwrap().get(&path).cloned().unwrap_or(Reply {
                        status: 404,
                        body: json!({}),
                        headers: String::new(),
                    });
                    let body = reply.body.to_string();
                    let response=format!("HTTP/1.1 {} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",reply.status,body.len(),reply.headers,body);
                    stream.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        Self {
            url,
            replies,
            requests,
            task,
        }
    }
    fn set(&self, path: &str, body: serde_json::Value) {
        self.reply(path, 200, body, "");
    }
    fn reply(&self, path: &str, status: u16, body: serde_json::Value, headers: &str) {
        self.replies.lock().unwrap().insert(
            path.into(),
            Reply {
                status,
                body,
                headers: headers.into(),
            },
        );
    }
    fn monitor(&self) -> Monitor {
        let mut m = Monitor::new().unwrap();
        m.api = self.url.clone();
        m.token = "fixture-only".into();
        m
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[tokio::test]
async fn reads_owned_private_and_fork_repository_pages_with_conditional_requests() {
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let time = 1_800_000_000_000;
    server.set("/user", json!(account()));
    server.reply(
        "/user/repos?affiliation=owner&per_page=100",
        200,
        json!([{"id":10,"full_name":"fixture/repo","private":true}]),
        &format!("Link: <{}/repos-page2>; rel=\"next\"\r\n", server.url),
    );
    server.set(
        "/repos-page2",
        json!([{"id":11,"full_name":"fixture/fork","fork":true}]),
    );
    let path = "/repos/fixture/repo/actions/runs?per_page=10";
    server.reply(
        path,
        200,
        json!({"workflow_runs":[run(1,"completed",Some("success"),1,time-1)]}),
        "ETag: \"run-one\"\r\n",
    );
    server.set(
        "/repos/fixture/fork/actions/runs?per_page=10",
        json!({"workflow_runs":[]}),
    );
    let mut state = Stored::default();
    monitor.scan(&mut state, true, time).await.unwrap();
    state.snapshot.last_scan_ms = time;
    assert_eq!(state.snapshot.repositories.len(), 2);
    assert!(state.pending.is_empty());
    server.reply(path, 304, json!(null), "");
    monitor.scan(&mut state, true, time + 1000).await.unwrap();
    assert!(state.pending.is_empty());
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.to_lowercase().contains("if-none-match: \"run-one\"")));
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.contains("affiliation=owner"))
            .count(),
        1
    );
}
#[tokio::test]
async fn account_switch_rebaselines_and_rate_limit_keeps_unvisited_repositories() {
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let time = 1_800_000_000_000;
    server.set("/user", json!(account()));
    server.set(
        "/user/repos?affiliation=owner&per_page=100",
        json!([{"id":10,"full_name":"fixture/repo"},{"id":11,"full_name":"fixture/fork"}]),
    );
    server.set(
        "/repos/fixture/repo/actions/runs?per_page=10",
        json!({"workflow_runs":[run(1,"in_progress",None,1,time)]}),
    );
    server.set(
        "/repos/fixture/fork/actions/runs?per_page=10",
        json!({"workflow_runs":[run(2,"in_progress",None,1,time)]}),
    );
    let mut state = Stored::default();
    monitor.scan(&mut state, true, time).await.unwrap();
    server.reply(
        "/repos/fixture/repo/actions/runs?per_page=10",
        429,
        json!({}),
        "Retry-After: 90\r\n",
    );
    assert!(monitor.scan(&mut state, true, now()).await.is_err());
    assert!(monitor.retry_at > now() + 80000);
    assert_eq!(state.snapshot.runs.len(), 2);
    server.set("/user", json!({"id":2,"login":"changed"}));
    server.set(
        "/repos/fixture/repo/actions/runs?per_page=10",
        json!({"workflow_runs":[run(1,"completed",Some("success"),1,time+1000)]}),
    );
    monitor.retry_at = 0;
    monitor.scan(&mut state, true, time + 2000).await.unwrap();
    assert_eq!(state.snapshot.account.unwrap().id, 2);
    assert!(state.pending.is_empty());
}
#[tokio::test]
async fn workflow_reads_only_first_page_and_errors_are_safe() {
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let time = 1_800_000_000_000;
    let repo = Repository {
        id: 10,
        full_name: "fixture/repo".into(),
        archived: false,
    };
    server.reply(
        "/repos/fixture/repo/actions/runs?per_page=10",
        200,
        json!({"workflow_runs":[run(2,"completed",Some("success"),1,time-2000)]}),
        &format!("Link: <{}/runs-page2>; rel=\"next\"\r\n", server.url),
    );
    server.set(
        "/runs-page2",
        json!({"workflow_runs":[run(1,"in_progress",None,1,time-10000)]}),
    );
    let initial = monitor.repository_runs(&repo).await.unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].id, 2);
    server.set(
        "/runs-page2",
        json!({"workflow_runs":[run(1,"completed",Some("failure"),2,time+1000)]}),
    );
    assert_eq!(monitor.repository_runs(&repo).await.unwrap().len(), 1);
    assert!(!server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.contains("/runs-page2")));
    server.reply(
        "/user",
        401,
        json!({"message":"fixture-token-never-echo"}),
        "",
    );
    let err = monitor
        .get("/user", |raw| {
            Ok(CachedValue::Account(serde_json::from_value(raw)?))
        })
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(err, "gh 登录已失效");
    server.reply(
        "/user",
        403,
        json!({}),
        &format!(
            "x-ratelimit-remaining: 0\r\nx-ratelimit-reset: {}\r\n",
            now() / 1000 + 120
        ),
    );
    assert!(monitor
        .get("/user", |raw| Ok(CachedValue::Account(
            serde_json::from_value(raw)?
        )))
        .await
        .is_err());
    assert!(monitor.retry_at > now() + 100000);
}

#[tokio::test]
async fn refresh_during_scan_is_consumed_and_unchanged_polls_do_not_write() {
    let fixture = Database::new();
    let server = Server::new().await;
    server.set("/user", json!(account()));
    server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
    let mut monitor = server.monitor();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    status(
        &fixture.db,
        &config,
        Request {
            refresh: true,
            ..Default::default()
        },
    )
    .unwrap();
    let during = now() + 1;
    monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            fixture
                .db
                .set_setting(REFRESH, &during.to_string())
                .unwrap();
            Ok("fixture-only".into())
        })
        .await
        .unwrap();
    let stored = fixture.db.get_setting(STATE).unwrap();
    assert!(
        !status(
            &fixture.db,
            &config,
            Request {
                refresh: false,
                ..Default::default()
            }
        )
        .unwrap()
        .refreshing
    );
    assert_eq!(load(&fixture.db).unwrap().request_ms, during);
    monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            panic!("idle must not scan")
        })
        .await
        .unwrap();
    assert_eq!(stored, fixture.db.get_setting(STATE).unwrap());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn settings_reset_during_scan_is_preserved() {
    let fixture = Database::new();
    let server = Server::new().await;
    server.set("/user", json!(account()));
    server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
    let mut monitor = server.monitor();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    let mut state = Stored::default();
    state.snapshot.enabled = true;
    state.baseline_ms = now() - 1000;
    save(&fixture.db, &state).unwrap();
    let events = monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            settings_changed(&fixture.db, &config, &Config::current_defaults().unwrap()).unwrap();
            Ok("fixture-only".into())
        })
        .await
        .unwrap();
    assert!(events.is_empty());
    let state = load(&fixture.db).unwrap();
    assert_eq!(state.baseline_ms, 0);
    assert!(!state.snapshot.enabled);
    assert!(state.snapshot.account.is_none());
}

#[tokio::test]
async fn repository_refresh_removes_only_obsolete_run_cache_entries() {
    let server = Server::new().await;
    server.set("/user", json!(account()));
    server.set(
        "/user/repos?affiliation=owner&per_page=100",
        json!([{"id":10,"full_name":"fixture/repo"}]),
    );
    server.set(
        "/repos/fixture/repo/actions/runs?per_page=10",
        json!({"workflow_runs":[]}),
    );
    let mut monitor = server.monitor();
    let mut state = Stored::default();
    monitor.scan(&mut state, false, now()).await.unwrap();
    assert_eq!(monitor.cache.len(), 3);
    server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
    monitor
        .scan(&mut state, false, now() + 600001)
        .await
        .unwrap();
    assert_eq!(monitor.cache.len(), 2);
    assert!(monitor
        .cache
        .keys()
        .all(|url| !url.contains("/repos/fixture/")));
}

#[test]
fn batch_acknowledgement_preserves_unprocessed_events_and_old_state_is_readable() {
    let fixture = Database::new();
    let time = now();
    let events = (1..=3)
        .map(|id| Event {
            account: account(),
            observed_ms: time,
            run: run(id, "completed", Some("success"), 1, time),
        })
        .collect::<Vec<_>>();
    fixture
        .db
        .set_setting(
            STATE,
            &json!({"snapshot":{"retry_at_ms":time+60000},"pending":events}).to_string(),
        )
        .unwrap();
    acknowledge(&fixture.db, &[events[0].key(), events[1].key()]).unwrap();
    let state = load(&fixture.db).unwrap();
    assert_eq!(state.pending, vec![events[2].clone()]);
    assert!(!fixture
        .db
        .get_setting(STATE)
        .unwrap()
        .unwrap()
        .contains("retry_at_ms"));
    acknowledge(&fixture.db, &[events[0].key()]).unwrap();
    assert_eq!(load(&fixture.db).unwrap().pending, state.pending);
}

#[test]
fn cached_status_never_requests_scan_and_exposes_monitor_baseline() {
    let fixture = Database::new();
    let config = Config::current_defaults().unwrap();
    let cached = Request {
        refresh: true,
        cached_only: true,
    };
    let empty = status(&fixture.db, &config, cached.clone()).unwrap();
    assert_eq!(empty.last_scan_ms, 0);
    assert!(!empty.refreshing);
    assert!(fixture.db.get_setting(REFRESH).unwrap().is_none());
    let state = Stored {
        baseline_ms: 123,
        ..Default::default()
    };
    save(&fixture.db, &state).unwrap();
    assert_eq!(
        status(&fixture.db, &config, cached).unwrap().baseline_ms,
        123
    );
    assert!(fixture.db.get_setting(REFRESH).unwrap().is_none());
}

#[tokio::test]
async fn bot_runs_and_repository_filters_suppress_pending_events_without_storm_on_toggle() {
    let fixture = Database::new();
    let server = Server::new().await;
    let time = 1_800_000_000_000;
    let repo = Repository {
        id: 10,
        full_name: "fixture/repo".into(),
        archived: false,
    };
    let path = "/repos/fixture/repo/actions/runs?per_page=10";
    server.reply(
        path,
        200,
        json!({
            "workflow_runs": [
                {
                    "id": 1,
                    "run_attempt": 1,
                    "name": "CI",
                    "head_branch": "dependabot/cargo/serde-1.0",
                    "head_sha": "0123456789",
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": "https://github.com/fixture/repo/actions/runs/1",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": jiff::Timestamp::from_millisecond(time + 1000).unwrap().to_string(),
                    "actor": { "login": "dependabot[bot]", "type": "Bot" }
                },
                {
                    "id": 2,
                    "run_attempt": 1,
                    "name": "CI",
                    "head_branch": "main",
                    "head_sha": "abcdef0123",
                    "status": "completed",
                    "conclusion": "failure",
                    "html_url": "https://github.com/fixture/repo/actions/runs/2",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": jiff::Timestamp::from_millisecond(time + 1000).unwrap().to_string(),
                    "actor": { "login": "alice", "type": "User" }
                }
            ]
        }),
        "ETag: \"bot-runs\"\r\n",
    );
    let mut monitor = server.monitor();
    let first = monitor.repository_runs(&repo).await.unwrap();
    assert_eq!(first.len(), 2);
    assert!(first[0].is_bot);
    assert!(!first[1].is_bot);

    // 304 Not Modified preserves raw JSON and re-derives is_bot accurately.
    server.reply(path, 304, json!(null), "");
    let cached = monitor.repository_runs(&repo).await.unwrap();
    assert!(cached[0].is_bot);
    assert!(!cached[1].is_bot);

    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    assert!(config.probe.notifications.github_ci_filter_bots);

    let mut state = Stored {
        baseline_ms: time,
        snapshot: Snapshot {
            enabled: true,
            account: Some(account()),
            ..Default::default()
        },
        ..Default::default()
    };
    reconcile_with_config(
        &mut state,
        account(),
        cached.clone(),
        &config,
        true,
        false,
        time + 2000,
    );
    // Only the non-bot run is queued in pending, while both remain in snapshot.runs.
    assert_eq!(state.snapshot.runs.len(), 2);
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].run.id, 2);

    // Turning off filter_bots on subsequent poll must not trigger a historical storm for run 1
    // because run 1 was already recorded as completed in state.snapshot.runs.
    config.probe.notifications.github_ci_filter_bots = false;
    reconcile_with_config(
        &mut state,
        account(),
        cached,
        &config,
        true,
        false,
        time + 3000,
    );
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].run.id, 2);

    // Selecting another repository prunes pending events and cached runs via settings_changed without resetting baseline.
    save(&fixture.db, &state).unwrap();
    let mut filtered_config = config.clone();
    filtered_config.probe.notifications.github_ci_repositories = vec!["fixture/other".into()];
    settings_changed(&fixture.db, &config, &filtered_config).unwrap();
    assert!(!event_current(&fixture.db, &filtered_config, &state.pending[0]).unwrap());
    let updated = load(&fixture.db).unwrap();
    assert_eq!(updated.baseline_ms, time);
    assert!(updated.pending.is_empty());
    assert!(updated.snapshot.runs.is_empty());
    assert!(request_ms(&fixture.db).unwrap() > 0);
}

#[tokio::test]
async fn subset_repository_filter_polls_only_selected_repos_and_baselines_newly_added_repos() {
    let fixture = Database::new();
    let server = Server::new().await;
    let time = 1_800_000_000_000;
    server.set("/user", json!(account()));
    server.set(
        "/user/repos?affiliation=owner&per_page=100",
        json!([
            {"id": 10, "full_name": "fixture/repo", "archived": false},
            {"id": 11, "full_name": "fixture/other", "archived": false}
        ]),
    );
    let repo_runs_path = "/repos/fixture/repo/actions/runs?per_page=10";
    let other_runs_path = "/repos/fixture/other/actions/runs?per_page=10";
    server.set(
        repo_runs_path,
        json!({"workflow_runs": [run(1, "completed", Some("success"), 1, time - 1000)]}),
    );
    let mut other_completed = run(20, "completed", Some("failure"), 1, time + 500);
    other_completed.repository_id = 11;
    other_completed.repository_name = "fixture/other".into();
    let mut other_running = run(21, "in_progress", None, 1, time + 500);
    other_running.repository_id = 11;
    other_running.repository_name = "fixture/other".into();
    server.set(
        other_runs_path,
        json!({"workflow_runs": [other_completed, other_running]}),
    );

    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    config.probe.notifications.github_ci_repositories = vec!["FIXTURE/REPO".into()];

    let mut monitor = server.monitor();
    let mut state = Stored::default();
    monitor
        .scan_with_config(&mut state, &config, true, time)
        .await
        .unwrap();

    // Full catalog of repositories is retained for UI selection, but only fixture/repo was polled for runs.
    assert_eq!(state.snapshot.repositories.len(), 2);
    assert_eq!(
        state.snapshot.repository_baselines_ms,
        BTreeMap::from([(10, time)])
    );
    assert_eq!(state.snapshot.runs.len(), 1);
    assert_eq!(state.snapshot.runs[0].repository_name, "fixture/repo");
    assert!(state.pending.is_empty());
    assert!(!server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.contains(other_runs_path)));

    // Expand monitored repositories to include fixture/other.
    save(&fixture.db, &state).unwrap();
    let mut expanded_config = config.clone();
    expanded_config.probe.notifications.github_ci_repositories =
        vec!["fixture/repo".into(), "fixture/other".into()];
    settings_changed(&fixture.db, &config, &expanded_config).unwrap();
    assert!(request_ms(&fixture.db).unwrap() > 0);

    // If the first poll of newly added fixture/other fails, it must NOT be marked as baselined yet.
    state = load(&fixture.db).unwrap();
    server.reply(
        other_runs_path,
        500,
        json!({"message": "temporary error"}),
        "",
    );
    assert!(monitor
        .scan_with_config(&mut state, &expanded_config, true, time + 1000)
        .await
        .is_err());
    save(&fixture.db, &state).unwrap();

    assert!(!state.snapshot.repository_baselines_ms.contains_key(&11));

    // First successful scan of newly added fixture/other must baseline its already-completed run (id 20) without firing a notification.
    server.set(
        other_runs_path,
        json!({"workflow_runs": [other_completed, other_running]}),
    );
    state = load(&fixture.db).unwrap();
    monitor
        .scan_with_config(&mut state, &expanded_config, true, time + 2000)
        .await
        .unwrap();
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.contains(other_runs_path)));
    assert_eq!(state.snapshot.runs.len(), 3);
    assert_eq!(
        state.snapshot.repository_baselines_ms,
        BTreeMap::from([(10, time), (11, time + 2000)])
    );
    assert!(state.pending.is_empty());

    // When the in-progress run (id 21) on fixture/other finishes on the next poll, it notifies normally.
    let mut other_finished = run(21, "completed", Some("success"), 1, time + 3000);
    other_finished.repository_id = 11;
    other_finished.repository_name = "fixture/other".into();
    server.set(other_runs_path, json!({"workflow_runs": [other_finished]}));
    monitor
        .scan_with_config(&mut state, &expanded_config, true, time + 4000)
        .await
        .unwrap();
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].run.id, 21);
    assert_eq!(state.pending[0].run.repository_id, 11);

    // Acknowledge the notification, then deselect and re-select fixture/other via settings_changed;
    // re-adding fixture/other must not re-fire a duplicate notification for historical run 21.
    save(&fixture.db, &state).unwrap();
    let ack_key = state.pending[0].key();
    acknowledge(&fixture.db, &[ack_key]).unwrap();
    settings_changed(&fixture.db, &expanded_config, &config).unwrap();
    settings_changed(&fixture.db, &config, &expanded_config).unwrap();
    state = load(&fixture.db).unwrap();
    monitor
        .scan_with_config(&mut state, &expanded_config, true, time + 5000)
        .await
        .unwrap();
    assert!(state.pending.is_empty());
    save(&fixture.db, &state).unwrap();
    let snapshot = status(
        &fixture.db,
        &expanded_config,
        Request {
            cached_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        snapshot.repository_baselines_ms,
        BTreeMap::from([(10, time), (11, time + 5000)])
    );
}

#[tokio::test]
async fn a_failed_repository_cannot_borrow_another_repositories_observation_after_bot_cutoff() {
    for code in [500, 429] {
        let fixture = Database::new();
        let server = Server::new().await;
        let mut monitor = server.monitor();
        let mut state = Stored::default();
        let mut config = Config::current_defaults().unwrap();
        config.probe.notifications.notify_github_ci = true;
        let time = 1_800_000_000_000;
        server.set("/user", json!(account()));
        server.set(
            "/user/repos?affiliation=owner&per_page=100",
            json!([
                {"id":11,"full_name":"fixture/other"},{"id":10,"full_name":"fixture/repo"}
            ]),
        );
        let good = "/repos/fixture/other/actions/runs?per_page=10";
        let failed = "/repos/fixture/repo/actions/runs?per_page=10";
        let mut bot = serde_json::to_value(run(1, "in_progress", None, 1, time)).unwrap();
        bot["actor"] = json!({"type":"Bot"});
        server.set(good, json!({"workflow_runs":[]}));
        server.set(failed, json!({"workflow_runs":[bot.clone()]}));
        monitor
            .scan_with_config(&mut state, &config, true, time)
            .await
            .unwrap();
        state.snapshot.bot_baseline_ms = time + 2000;
        config.probe.notifications.github_ci_filter_bots = false;
        server.reply(
            failed,
            code,
            json!({}),
            if code == 429 {
                "Retry-After: 1\r\n"
            } else {
                ""
            },
        );
        assert!(monitor
            .scan_with_config(&mut state, &config, true, time + 3000)
            .await
            .is_err());
        state.snapshot.last_scan_ms = time + 3000;
        assert_eq!(state.snapshot.repository_observed_ms[&10], time);
        assert_eq!(state.snapshot.repository_observed_ms[&11], time + 3000);
        save(&fixture.db, &state).unwrap();
        state = load(&fixture.db).unwrap();
        monitor = server.monitor();
        bot["status"] = "completed".into();
        bot["conclusion"] = "success".into();
        bot["updated_at"] = json!(jiff::Timestamp::from_millisecond(time + 1000)
            .unwrap()
            .to_string());
        server.set(failed, json!({"workflow_runs":[bot]}));
        monitor
            .scan_with_config(&mut state, &config, true, time + 4000)
            .await
            .unwrap();
        assert!(state.pending.is_empty());
        assert_eq!(state.snapshot.repository_observed_ms[&10], time + 4000);
    }
}

#[tokio::test]
async fn direct_config_bot_toggles_reset_each_cutoff_even_when_repository_reads_fail() {
    let fixture = Database::new();
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    let config_path = fixture.root.join("config.toml");
    server.set("/user", json!(account()));
    server.set(
        "/user/repos?affiliation=owner&per_page=100",
        json!([{"id":10,"full_name":"fixture/repo"}]),
    );
    let path = "/repos/fixture/repo/actions/runs?per_page=10";
    server.set(path, json!({"workflow_runs":[]}));
    for filtered in [true, false, true, false] {
        config.probe.notifications.github_ci_filter_bots = filtered;
        std::fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
        if !filtered {
            let mut state = load(&fixture.db).unwrap();
            state.snapshot.bot_baseline_ms = 1;
            save(&fixture.db, &state).unwrap();
            server.reply(path, 500, json!({}), "");
        }
        monitor.next_poll_ms = 0;
        monitor.repository_due_ms.insert(10, 0);
        let before = now();
        monitor
            .tick_with_token(&fixture.db, &|| Config::load(&config_path), async {
                Ok("fixture-only".into())
            })
            .await
            .unwrap();
        let state = load(&fixture.db).unwrap();
        assert_eq!(state.snapshot.filter_bots, filtered);
        if !filtered {
            assert!(state.snapshot.bot_baseline_ms >= before);
            assert!(state.snapshot.error.is_some());
        }
    }
}

#[tokio::test]
async fn due_repositories_use_five_or_sixty_seconds_and_reuse_credentials_and_304_summaries() {
    let fixture = Database::new();
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    config.probe.notifications.github_ci_filter_bots = true;
    let time = now();
    server.set("/user", json!(account()));
    server.set(
        "/user/repos?affiliation=owner&per_page=100",
        json!([
            {"id":10,"full_name":"fixture/repo"}, {"id":11,"full_name":"fixture/idle"},
            {"id":12,"full_name":"fixture/bot"}
        ]),
    );
    let path = "/repos/fixture/repo/actions/runs?per_page=10";
    server.reply(
        path,
        200,
        json!({"workflow_runs":[run(1,"in_progress",None,1,time)]}),
        "ETag: \"runs\"\r\n",
    );
    server.set(
        "/repos/fixture/idle/actions/runs?per_page=10",
        json!({"workflow_runs":[]}),
    );
    let mut bot = serde_json::to_value(run(3, "in_progress", None, 1, time)).unwrap();
    bot["actor"] = json!({"type":"Bot"});
    server.set(
        "/repos/fixture/bot/actions/runs?per_page=10",
        json!({"workflow_runs":[bot]}),
    );
    monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            Ok("fixture-only".into())
        })
        .await
        .unwrap();
    let due = monitor.repository_due_ms[&10];
    assert_eq!(monitor.repository_due_ms[&11] - due, 55_000);
    assert_eq!(monitor.repository_due_ms[&12] - due, 55_000);
    let stored = fixture.db.get_setting(STATE).unwrap();
    let sql = rusqlite::Connection::open(fixture.root.join("panel.sqlite")).unwrap();
    sql.execute_batch("CREATE TRIGGER no_idle_ci_writes BEFORE UPDATE ON settings WHEN NEW.key='github_ci_state' BEGIN SELECT RAISE(ABORT,'unchanged poll wrote state'); END;").unwrap();
    server.reply(path, 304, json!(null), "");
    monitor.repository_due_ms.insert(10, time - 1);
    monitor.next_poll_ms = time - 1;
    monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            panic!("fast poll must reuse gh credentials")
        })
        .await
        .unwrap();
    assert_eq!(stored, fixture.db.get_setting(STATE).unwrap());
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 6);
    assert!(requests
        .last()
        .unwrap()
        .to_lowercase()
        .contains("if-none-match: \"runs\""));
    sql.execute_batch("DROP TRIGGER no_idle_ci_writes").unwrap();
    // In-memory deadlines, including the account check, expire independently.
    monitor.credentials_due_ms = time - 1;
    monitor.next_poll_ms = time - 1;
    monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            Ok("fixture-only".into())
        })
        .await
        .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 7);
    let snapshot = status(
        &fixture.db,
        &config,
        Request {
            cached_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(snapshot.filter_bots);
    assert_eq!(
        snapshot.scanned_repository_ids,
        BTreeSet::from([10, 11, 12])
    );
}

#[tokio::test]
async fn failed_account_refresh_waits_for_credentials_and_does_not_use_stale_identity() {
    let fixture = Database::new();
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    server.set("/user", json!(account()));
    server.set(
        "/user/repos?affiliation=owner&per_page=100",
        json!([{"id":10,"full_name":"fixture/repo"}]),
    );
    server.set(
        "/repos/fixture/repo/actions/runs?per_page=10",
        json!({"workflow_runs":[run(1,"in_progress",None,1,now())]}),
    );
    monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            Ok("fixture-only".into())
        })
        .await
        .unwrap();
    monitor.credentials_due_ms = 0;
    monitor.next_poll_ms = 0;
    monitor.repository_due_ms.insert(10, 0);
    server.reply("/user", 401, json!({}), "");
    assert!(monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            Ok("fixture-only".into())
        })
        .await
        .unwrap()
        .is_empty());
    assert_eq!(monitor.credentials_due_ms, 0);
    assert!(monitor.next_poll_ms >= now() + 59_000);
    let count = server.requests.lock().unwrap().len();
    monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            panic!("account retry is not yet due")
        })
        .await
        .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), count);
    assert!(load(&fixture.db)
        .unwrap()
        .snapshot
        .error
        .unwrap()
        .contains("登录已失效"));
}

#[tokio::test]
async fn credential_failure_after_restart_and_short_account_limit_retry_verify_account_again() {
    for limited in [false, true] {
        let fixture = Database::new();
        let server = Server::new().await;
        let mut config = Config::current_defaults().unwrap();
        config.probe.notifications.notify_github_ci = true;
        let mut state = Stored::default();
        reconcile(
            &mut state,
            account(),
            vec![run(1, "in_progress", None, 1, now())],
            true,
            true,
            now(),
        );
        save(&fixture.db, &state).unwrap();
        let mut monitor = server.monitor();
        monitor.token.clear();
        server.reply("/user", 429, json!({}), "Retry-After: 1\r\n");
        monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                if limited {
                    Ok("fixture-only".into())
                } else {
                    anyhow::bail!("fixture missing credential")
                }
            })
            .await
            .unwrap();
        assert_eq!(monitor.credentials_due_ms, 0);
        if limited {
            assert_eq!(monitor.next_poll_ms, monitor.retry_at);
        }
        let count = server.requests.lock().unwrap().len();
        monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                panic!("cooldown")
            })
            .await
            .unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), count);
        monitor.next_poll_ms = 0;
        monitor.retry_at = 0;
        server.set("/user", json!(account()));
        server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
        monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                Ok("fixture-recovered".into())
            })
            .await
            .unwrap();
        assert!(load(&fixture.db).unwrap().snapshot.error.is_none());
        let requests = server.requests.lock().unwrap();
        assert!(requests[count].starts_with("GET /user "));
        assert!(!requests[count].contains("Bearer \r"));
    }
}

#[tokio::test]
async fn failed_repository_catalog_waits_and_manual_refresh_recovers() {
    for limited in [false, true] {
        let fixture = Database::new();
        let server = Server::new().await;
        let mut monitor = server.monitor();
        let mut config = Config::current_defaults().unwrap();
        config.probe.notifications.notify_github_ci = true;
        server.set("/user", json!(account()));
        server.reply(
            "/user/repos?affiliation=owner&per_page=100",
            if limited { 429 } else { 500 },
            json!({}),
            if limited { "Retry-After: 1\r\n" } else { "" },
        );
        monitor.repository_due_ms.insert(10, 0);
        monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                Ok("fixture-only".into())
            })
            .await
            .unwrap();
        if limited {
            assert_eq!(monitor.next_poll_ms, monitor.retry_at);
        } else {
            assert!(monitor.next_poll_ms >= now() + 59_000);
        }
        let count = server.requests.lock().unwrap().len();
        monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                panic!("no tight loop")
            })
            .await
            .unwrap();
        assert_eq!(count, server.requests.lock().unwrap().len());
        assert!(load(&fixture.db).unwrap().snapshot.error.is_some());
        monitor.retry_at = 0;
        fixture
            .db
            .set_setting(REFRESH, &(now() + 1).to_string())
            .unwrap();
        server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
        monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                Ok("fixture-only".into())
            })
            .await
            .unwrap();
        assert!(load(&fixture.db).unwrap().snapshot.error.is_none());
    }
}

#[tokio::test]
async fn repository_and_bot_settings_saved_during_scan_cannot_be_overwritten() {
    for bot_change in [false, true] {
        let fixture = Database::new();
        let server = Server::new().await;
        let mut monitor = server.monitor();
        let mut config = Config::current_defaults().unwrap();
        config.probe.notifications.notify_github_ci = true;
        server.set("/user", json!(account()));
        server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
        let mut after = config.clone();
        if bot_change {
            after.probe.notifications.github_ci_filter_bots = false;
        } else {
            after.probe.notifications.github_ci_repositories = vec!["fixture/selected".into()];
        }
        monitor
            .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
                settings_changed(&fixture.db, &config, &after).unwrap();
                Ok("fixture-only".into())
            })
            .await
            .unwrap();
        let current = load(&fixture.db).unwrap();
        assert_eq!(
            current.snapshot.monitored_repositories,
            after.probe.notifications.github_ci_repositories
        );
        assert_eq!(
            current.snapshot.filter_bots,
            after.probe.notifications.github_ci_filter_bots
        );
        assert!(current.snapshot.account.is_none());
        assert!(request_ms(&fixture.db).unwrap() > current.request_ms);
        assert_eq!(monitor.active, None);
        monitor
            .tick_with_token(&fixture.db, &|| Ok(after.clone()), async {
                Ok("fixture-only".into())
            })
            .await
            .unwrap();
        assert_eq!(
            load(&fixture.db).unwrap().request_ms,
            request_ms(&fixture.db).unwrap()
        );
    }
}

#[test]
fn source_bot_cutoff_survives_restart_and_suppresses_unseen_history() {
    let fixture = Database::new();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    let time = now() - 10000;
    let mut state = Stored::default();
    reconcile(
        &mut state,
        account(),
        vec![run(1, "in_progress", None, 1, time)],
        true,
        true,
        time,
    );
    save(&fixture.db, &state).unwrap();
    let mut after = config.clone();
    after.probe.notifications.github_ci_filter_bots = false;
    settings_changed(&fixture.db, &config, &after).unwrap();
    let mut restored = load(&fixture.db).unwrap();
    let cutoff = restored.snapshot.bot_baseline_ms;
    let mut old = run(2, "completed", Some("success"), 1, cutoff - 1);
    old.is_bot = true;
    let mut new = run(3, "completed", Some("success"), 1, cutoff + 1);
    new.is_bot = true;
    reconcile_with_config(
        &mut restored,
        account(),
        vec![old, new],
        &after,
        true,
        false,
        cutoff + 2,
    );
    assert_eq!(
        restored
            .pending
            .iter()
            .map(|e| e.run.id)
            .collect::<Vec<_>>(),
        vec![3]
    );
}

#[tokio::test]
async fn acknowledgement_during_scan_is_not_reinserted() {
    let fixture = Database::new();
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    let time = now() - 2000;
    let mut state = Stored::default();
    reconcile(
        &mut state,
        account(),
        vec![run(1, "in_progress", None, 1, time)],
        true,
        true,
        time,
    );
    reconcile(
        &mut state,
        account(),
        vec![run(1, "completed", Some("success"), 1, time + 1000)],
        true,
        false,
        time + 1000,
    );
    let key = state.pending[0].key();
    server.set("/user", json!(account()));
    server.set(
        "/user/repos?affiliation=owner&per_page=100",
        json!([{"id":10,"full_name":"fixture/repo"}]),
    );
    server.set(
        "/repos/fixture/repo/actions/runs?per_page=10",
        json!({"workflow_runs":state.snapshot.runs}),
    );
    save(&fixture.db, &state).unwrap();
    assert!(monitor
        .tick_with_token(&fixture.db, &|| Ok(config.clone()), async {
            acknowledge(&fixture.db, &[key]).unwrap();
            Ok("fixture-only".into())
        })
        .await
        .unwrap()
        .is_empty());
    assert!(load(&fixture.db).unwrap().pending.is_empty());
}

#[test]
fn whole_second_completions_use_observations_from_the_current_source_baseline() {
    for bot in [false, true] {
        let second = 1_800_000_000_000;
        let mut config = Config::current_defaults().unwrap();
        config.probe.notifications.github_ci_filter_bots = false;
        let mut state = Stored::default();
        let mut running = run(1, "in_progress", None, 1, second);
        running.is_bot = bot;
        reconcile_with_config(
            &mut state,
            account(),
            vec![running.clone()],
            &config,
            true,
            true,
            second + 100,
        );
        state
            .snapshot
            .repository_baselines_ms
            .insert(10, second + 200);
        state.snapshot.bot_baseline_ms = second + 300;
        state.snapshot.last_scan_ms = second + 400;
        state
            .snapshot
            .repository_observed_ms
            .insert(10, second + 400);
        let mut finished = running.clone();
        finished.status = "completed".into();
        finished.conclusion = Some("success".into());
        let mut unseen = finished.clone();
        unseen.id = 2;
        reconcile_with_config(
            &mut state,
            account(),
            vec![finished.clone(), unseen],
            &config,
            true,
            false,
            second + 800,
        );
        assert_eq!(state.pending.len(), 1);
        assert_eq!(state.pending[0].run.id, 1);
        state.pending.clear();
        reconcile_with_config(
            &mut state,
            account(),
            vec![finished.clone()],
            &config,
            true,
            false,
            second + 900,
        );
        assert!(state.pending.is_empty());
        // A bot toggle within the completion second keeps an observed attempt;
        // a repository re-add still discards the old observation.
        state.snapshot.runs = vec![running];
        if bot {
            state.snapshot.bot_baseline_ms = second + 600;
        } else {
            state
                .snapshot
                .repository_baselines_ms
                .insert(10, second + 600);
        }
        reconcile_with_config(
            &mut state,
            account(),
            vec![finished],
            &config,
            true,
            false,
            second + 950,
        );
        assert_eq!(state.pending.len(), usize::from(bot));
    }
}

#[tokio::test]
async fn config_file_and_state_save_during_scan_leave_refresh_pending_for_latest_settings() {
    use crate::services::settings::{apply_probe_settings_save_plan, ProbeSettingsSavePlan};
    for notifications in [
        json!({"notify_github_ci": false}),
        json!({"github_ci_repositories": ["fixture/selected"]}),
        json!({"github_ci_filter_bots": false}),
    ] {
        let fixture = Database::new();
        let server = Server::new().await;
        let mut monitor = server.monitor();
        let mut config = Config::current_defaults().unwrap();
        config.probe.notifications.notify_github_ci = true;
        config.paths.db_path = fixture.db.path().to_path_buf();
        let path = fixture.root.join("config.toml");
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        server.set("/user", json!(account()));
        server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
        let load_config = || Config::load(&path);
        let events = monitor
            .tick_with_token(&fixture.db, &load_config, async {
                apply_probe_settings_save_plan(
                    &path,
                    &fixture.db,
                    ProbeSettingsSavePlan {
                        config_patch: serde_json::from_value(
                            json!({"probe": {"notifications": notifications}}),
                        )
                        .unwrap(),
                        secret_write: Some(crate::services::settings::SecretSettingWritePlan {
                            setting_key: "fixture-secret".into(),
                            secret_value: "fixture-only".into(),
                        }),
                    },
                )
                .unwrap();
                Ok("fixture-only".into())
            })
            .await
            .unwrap();
        assert!(events.is_empty());
        let saved = load(&fixture.db).unwrap();
        assert!(
            saved.snapshot.account.is_none(),
            "obsolete scan must not commit"
        );
        assert_eq!(monitor.active, None);
        assert_eq!(
            fixture
                .db
                .get_secret_setting_bytes("fixture-secret")
                .unwrap()
                .unwrap(),
            b"fixture-only"
        );
        // No in-memory Config replacement is needed: a separate monitor reads the saved file.
        let after = load_config().unwrap();
        if enabled(&after) {
            assert!(request_ms(&fixture.db).unwrap() > saved.request_ms);
        }
        monitor
            .tick_with_token(&fixture.db, &load_config, async {
                Ok("fixture-only".into())
            })
            .await
            .unwrap();
        let current = load(&fixture.db).unwrap();
        assert_eq!(current.snapshot.enabled, enabled(&after));
        assert_eq!(
            current.snapshot.monitored_repositories,
            after.probe.notifications.github_ci_repositories
        );
        assert_eq!(
            current.snapshot.filter_bots,
            after.probe.notifications.github_ci_filter_bots
        );
        assert_eq!(current.request_ms, request_ms(&fixture.db).unwrap());
    }
}

#[tokio::test]
async fn current_config_is_checked_even_when_database_settings_are_unchanged() {
    let fixture = Database::new();
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let mut config = Config::current_defaults().unwrap();
    config.probe.notifications.notify_github_ci = true;
    let current_config = Mutex::new(config);
    let load_config = || Ok(current_config.lock().unwrap().clone());
    server.set("/user", json!(account()));
    server.set("/user/repos?affiliation=owner&per_page=100", json!([]));
    monitor
        .tick_with_token(&fixture.db, &load_config, async {
            current_config
                .lock()
                .unwrap()
                .probe
                .notifications
                .notify_github_ci = false;
            Ok("fixture-only".into())
        })
        .await
        .unwrap();
    assert!(load(&fixture.db).unwrap().snapshot.account.is_none());
    assert_eq!(monitor.active, None);
}

#[test]
fn bot_toggle_same_second_requires_the_same_observed_unfinished_attempt() {
    let second = 1_800_000_000_000;
    for (old_status, attempt, finished_ms, expected) in [
        ("in_progress", 1, second, 1),
        ("in_progress", 1, second - 1000, 0),
        ("in_progress", 2, second, 0),
        ("completed", 1, second, 0),
    ] {
        let mut config = Config::current_defaults().unwrap();
        config.probe.notifications.github_ci_filter_bots = false;
        let mut state = Stored::default();
        let mut old = run(
            1,
            old_status,
            (old_status == "completed").then_some("success"),
            1,
            second - 5000,
        );
        old.is_bot = true;
        reconcile_with_config(
            &mut state,
            account(),
            vec![old],
            &config,
            true,
            true,
            second - 10000,
        );
        state.snapshot.last_scan_ms = second - 5000;
        state
            .snapshot
            .repository_observed_ms
            .insert(10, second - 5000);
        state.snapshot.bot_baseline_ms = second + 450;
        let mut finished = run(1, "completed", Some("success"), attempt, finished_ms);
        finished.is_bot = true;
        let mut unseen = finished.clone();
        unseen.id = 2;
        reconcile_with_config(
            &mut state,
            account(),
            vec![finished, unseen],
            &config,
            true,
            false,
            second + 1000,
        );
        assert_eq!(state.pending.len(), expected);
        if expected == 1 {
            assert_eq!(state.pending[0].run.id, 1);
        }
    }
}
