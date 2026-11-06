use super::*;
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
            db: PanelDb::open(root.join("panel.sqlite")).unwrap(),
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
        updated_at: chrono::DateTime::from_timestamp_millis(time)
            .unwrap()
            .to_rfc3339(),
        repository_id: 10,
        repository_name: "fixture/repo".into(),
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
    acknowledge(&db.db, &key).unwrap();
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
fn completion_during_initial_scan_is_new_but_older_history_is_not() {
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
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].run.id, 2);
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
    let mut runs = (0..MAX_RUNS + 100)
        .map(|i| run(i as u64, "completed", Some("success"), 1, time - 1000))
        .collect::<Vec<_>>();
    runs.push(run(9000, "in_progress", None, 1, time - 10000));
    reconcile(&mut state, account(), runs, true, true, time);
    assert_eq!(state.snapshot.runs.len(), MAX_RUNS + 1);
    assert!(state.snapshot.runs.iter().any(|r| r.id == 9000));
}
#[tokio::test]
async fn disabled_monitor_does_not_read_gh_or_write_on_idle_and_status_requests_refresh() {
    let fixture = Database::new();
    let config = Config::default();
    assert!(!enabled(&config));
    let mut monitor = Monitor::new().unwrap();
    assert!(!monitor.tick(&fixture.db, &config).await.unwrap());
    let before = fixture.db.get_setting(STATE).unwrap();
    assert!(before.is_none());
    for _ in 0..10 {
        assert!(!monitor.tick(&fixture.db, &config).await.unwrap());
    }
    assert_eq!(before, fixture.db.get_setting(STATE).unwrap());
    assert!(monitor.token.is_empty());
    let result = status(&fixture.db, &config, false).unwrap();
    assert!(result.refreshing);
    assert!(!result.enabled);
    assert!(request_ms(&fixture.db).unwrap() > 0);
}

#[tokio::test]
async fn restarting_during_rate_limit_waits_without_reading_credentials() {
    let fixture = Database::new();
    let mut config = Config::default();
    config.probe.notifications.notify_github_ci = true;
    let mut state = Stored::default();
    state.snapshot.retry_at_ms = Some(now() + 120000);
    save(&fixture.db, &state).unwrap();
    status(&fixture.db, &config, true).unwrap();
    let mut monitor = Monitor::new().unwrap();
    assert!(!monitor.tick(&fixture.db, &config).await.unwrap());
    assert!(!monitor.tick(&fixture.db, &config).await.unwrap());
    assert!(monitor.token.is_empty());
}

#[test]
fn saving_disable_and_reenable_resets_the_baseline_before_next_poll() {
    let fixture = Database::new();
    let mut active = Config::default();
    active.probe.notifications.notify_github_ci = true;
    let inactive = Config::default();
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
async fn reads_owned_private_and_fork_repositories_pages_caches_and_tracks_old_running_runs() {
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
    let path = "/repos/fixture/repo/actions/runs?per_page=100";
    server.reply(
        path,
        200,
        json!({"workflow_runs":[run(1,"completed",Some("success"),1,time-1)]}),
        "ETag: \"run-one\"\r\n",
    );
    server.set(
        "/repos/fixture/fork/actions/runs?per_page=100",
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
    state
        .snapshot
        .runs
        .push(run(2, "in_progress", None, 1, time));
    server.set(
        "/repos/fixture/repo/actions/runs/2",
        json!(run(2, "completed", Some("success"), 1, time + 2000)),
    );
    monitor.scan(&mut state, true, time + 3000).await.unwrap();
    assert_eq!(state.pending.len(), 1);
    assert_eq!(state.pending[0].run.id, 2);
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
        "/repos/fixture/repo/actions/runs?per_page=100",
        json!({"workflow_runs":[run(1,"in_progress",None,1,time)]}),
    );
    server.set(
        "/repos/fixture/fork/actions/runs?per_page=100",
        json!({"workflow_runs":[run(2,"in_progress",None,1,time)]}),
    );
    let mut state = Stored::default();
    monitor.scan(&mut state, true, time).await.unwrap();
    server.reply(
        "/repos/fixture/repo/actions/runs?per_page=100",
        429,
        json!({}),
        "Retry-After: 90\r\n",
    );
    assert!(monitor.scan(&mut state, true, now()).await.is_err());
    assert!(monitor.retry_at > now() + 80000);
    assert_eq!(state.snapshot.runs.len(), 2);
    server.set("/user", json!({"id":2,"login":"changed"}));
    server.set(
        "/repos/fixture/repo/actions/runs?per_page=100",
        json!({"workflow_runs":[run(1,"completed",Some("success"),1,time+1000)]}),
    );
    monitor.retry_at = 0;
    monitor.scan(&mut state, true, time + 2000).await.unwrap();
    assert_eq!(state.snapshot.account.unwrap().id, 2);
    assert!(state.pending.is_empty());
}
#[tokio::test]
async fn workflow_pagination_includes_old_running_and_rerun_attempts_and_errors_are_safe() {
    let server = Server::new().await;
    let mut monitor = server.monitor();
    let time = 1_800_000_000_000;
    let repo = Repository {
        id: 10,
        full_name: "fixture/repo".into(),
        archived: false,
    };
    let state = Stored {
        snapshot: Snapshot {
            last_scan_ms: time,
            ..Snapshot::default()
        },
        ..Stored::default()
    };
    server.reply(
        "/repos/fixture/repo/actions/runs?per_page=100",
        200,
        json!({"workflow_runs":[run(2,"completed",Some("success"),1,time-2000)]}),
        &format!("Link: <{}/runs-page2>; rel=\"next\"\r\n", server.url),
    );
    server.set(
        "/runs-page2",
        json!({"workflow_runs":[run(1,"in_progress",None,1,time-10000)]}),
    );
    let initial = monitor
        .repository_runs(&repo, &Stored::default())
        .await
        .unwrap();
    assert_eq!(initial.len(), 2);
    assert!(initial
        .iter()
        .any(|r| r.id == 1 && r.status == "in_progress"));
    server.set(
        "/runs-page2",
        json!({"workflow_runs":[run(1,"completed",Some("failure"),2,time+1000)]}),
    );
    assert_eq!(
        monitor.repository_runs(&repo, &state).await.unwrap().len(),
        2
    );
    server.reply(
        "/user",
        401,
        json!({"message":"fixture-token-never-echo"}),
        "",
    );
    let err = monitor
        .get::<Account>("/user")
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
    assert!(monitor.get::<Account>("/user").await.is_err());
    assert!(monitor.retry_at > now() + 100000);
}
