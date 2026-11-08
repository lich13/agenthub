#![cfg(target_os = "macos")]
use agenthub_core::{
    config::Config,
    db::PanelDb,
    dsh::{notifications, DshPaths},
    native_probe::{NativeProvider, NativeScan},
    platform::PlatformKind,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
struct Monitor(Child);
impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Fixture {
    root: PathBuf,
    paths: DshPaths,
    config: Config,
    db: PanelDb,
    requests: Arc<Mutex<Vec<Value>>>,
    code: Arc<AtomicU16>,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("agenthub-dsh-cli-{}", uuid::Uuid::new_v4()));
        let paths = DshPaths {
            home: root.join("dsh"),
        };
        fs::create_dir_all(paths.home.join("sessions/project")).unwrap();
        fs::create_dir_all(root.join("codex/sessions")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let code = Arc::new(AtomicU16::new(200));
        let stop = Arc::new(AtomicBool::new(false));
        let (captured, response_code, stopped) = (requests.clone(), code.clone(), stop.clone());
        let server = thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                while let Ok(n) = stream.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(offset) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..offset]);
                        let length = header
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|s| s.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() < offset + 4 + length {
                            continue;
                        }
                        if let Ok(value) =
                            serde_json::from_slice(&bytes[offset + 4..offset + 4 + length])
                        {
                            captured.lock().unwrap().push(value);
                        }
                        let c = response_code.load(Ordering::Relaxed);
                        let body = format!("{{\"code\":{c}}}");
                        let reply=format!("HTTP/1.1 {c} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                        let _ = stream.write_all(reply.as_bytes());
                        break;
                    }
                }
            }
        });
        let mut config = Config::for_platform_kind_with_home(PlatformKind::Macos, &root);
        config.codex.home = root.join("codex");
        config.paths.db_path = root.join("data/panel.sqlite");
        config.paths.data_dir = root.join("data");
        config.paths.log_dir = root.join("logs");
        config.security.secret_key = "7q9DCmCPyxnTrH3FhrV1sUJol1yqPgscQsBnR-mXA2E".into();
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = url;
        config.probe.notifications.notify_codex = false;
        config.probe.notifications.notify_claude = false;
        config.probe.notifications.notify_grok = false;
        config.probe.notifications.notify_dsh = true;
        config.probe.notifications.notify_dsh_completion = true;
        config.probe.notifications.notify_dsh_failure = true;
        config.probe.notifications.notify_dsh_reply_needed = true;
        config.probe.notifications.notify_dsh_authorization = true;
        fs::write(root.join("config.toml"), toml::to_string(&config).unwrap()).unwrap();
        let db = PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box().unwrap())
            .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        db.sync_notification_channels(&config).unwrap();
        db.stage_native_notifications(NativeProvider::Dsh, &NativeScan::default(), &config)
            .unwrap();
        Self {
            root,
            paths,
            config,
            db,
            requests,
            code,
            stop,
            server: Some(server),
        }
    }
    fn session(&self, id: &str, kind: &str) {
        let dir = self.paths.home.join("sessions/project").join(id);
        fs::create_dir_all(&dir).unwrap();
        let now = chrono::Utc::now().timestamp_millis() + 100;
        let mut lines = vec![
            json!({"type":"session","version":4,"id":id,"isSeeded":false}),
            json!({"type":"turn/start","seq":0,"time":now,"data":{"turn":1}}),
        ];
        match kind{
            "completion"|"failure"=>{lines.push(json!({"type":"assistant/message","seq":1,"time":now,"data":{"turn":1,"message":{"content":[{"type":"text","text":"Fixture finished"}]}}}));lines.push(json!({"type":"turn/end","seq":2,"time":now+1,"data":{"turn":1,"reason":{"kind":if kind=="completion"{"completed"}else{"error"}}}}));},
            "authorization"=>lines.push(json!({"type":"approval/asked","seq":1,"time":now,"data":{"id":"approval","toolName":"fixture-tool"}})),
            "reply_needed"=>lines.push(json!({"type":"tool/call","seq":1,"time":now,"data":{"turn":1,"name":"ask_user_question","callId":"question","arguments":{"questions":[{"question":"Fixture choice?"}]}}})),_=>panic!("unknown fixture")
        }
        fs::write(
            dir.join("session.v4.jsonl"),
            lines.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
    }
    fn stage(&self) {
        let scan = notifications::scan(&self.paths).unwrap();
        assert_eq!(scan.errors, 0);
        self.db
            .stage_native_notifications(NativeProvider::Dsh, &scan, &self.config)
            .unwrap();
    }
    fn monitor(&self) -> Monitor {
        Monitor(
            Command::new(env!("CARGO_BIN_EXE_agenthub-webd"))
                .args([
                    "--config",
                    self.root.join("config.toml").to_str().unwrap(),
                    "probe",
                    "monitor-errors",
                ])
                .env("DSH_HOME", &self.paths.home)
                .env("CLAUDE_CONFIG_DIR", self.root.join("claude"))
                .env("GROK_BUILD_HOME", self.root.join("grok"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn wait(&self, predicate: impl Fn() -> bool) {
        let start = Instant::now();
        while !predicate() && start.elapsed() < Duration::from_secs(12) {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(predicate(), "fixture monitor did not reach expected state");
    }
    fn append(&self, id: &str, row: Value) {
        let path = self
            .paths
            .home
            .join("sessions/project")
            .join(id)
            .join("session.v4.jsonl");
        writeln!(
            fs::OpenOptions::new().append(true).open(path).unwrap(),
            "{row}"
        )
        .unwrap();
    }
    fn status(&self, expected: &str) -> bool {
        Connection::open(self.db.path())
            .unwrap()
            .query_row(
                "SELECT count(*) FROM probe_notification_deliveries WHERE coalesce(json_extract(outcome_json,'$.status'),status)=?1",
                [expected],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.server.take() {
            t.join().unwrap();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn built_cli_delivers_four_categories_once_and_preserves_claims_after_restart() {
    let f = Fixture::new();
    for kind in ["completion", "failure", "reply_needed", "authorization"] {
        f.session(kind, kind);
    }
    f.stage();
    let monitor = f.monitor();
    f.wait(|| {
        Connection::open(f.db.path())
            .unwrap()
            .query_row(
                "SELECT count(*) FROM native_probe_deliveries WHERE status='sent'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            == 4
    });
    drop(monitor);
    let bodies = f.requests.lock().unwrap();
    assert!(bodies
        .iter()
        .all(|r| r["title"].as_str().unwrap().contains("DSH")));
    drop(bodies);
    assert!(f.db.pending_native_deliveries(20, None).unwrap().is_empty());
    Connection::open(f.db.path()).unwrap().execute(
        "UPDATE native_probe_providers SET scanned_at='fixture-before-restart' WHERE provider='dsh'", [],
    ).unwrap();
    let monitor = f.monitor();
    f.wait(|| {
        Connection::open(f.db.path())
            .unwrap()
            .query_row(
                "SELECT count(*) FROM native_probe_providers WHERE provider='dsh' AND scanned_at<>'fixture-before-restart'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            == 1
    });
    drop(monitor);
    assert_eq!(f.requests.lock().unwrap().len(), 4);
}
#[test]
fn built_cli_rechecks_answer_and_recovered_failure_before_retry() {
    for kind in ["failure", "reply_needed", "authorization"] {
        let f = Fixture::new();
        f.code.store(503, Ordering::Relaxed);
        f.session("fixture", kind);
        f.stage();
        let monitor = f.monitor();
        f.wait(|| f.status("waiting_retry"));
        drop(monitor);
        assert_eq!(f.requests.lock().unwrap().len(), 1);
        let (seq, event, data) = match kind {
            "failure" => (3, "turn/start", json!({"turn":2})),
            "reply_needed" => (
                2,
                "tool/result",
                json!({"message":{"toolCallId":"question","content":[]}}),
            ),
            _ => (
                2,
                "approval/decided",
                json!({"id":"approval","outcome":"allowed-once"}),
            ),
        };
        f.append("fixture",json!({"type":event,"seq":seq,"time":chrono::Utc::now().timestamp_millis(),"data":data}));
        Connection::open(f.db.path())
            .unwrap()
            .execute("UPDATE probe_notification_deliveries SET next_ms=0", [])
            .unwrap();
        f.code.store(200, Ordering::Relaxed);
        let monitor = f.monitor();
        f.wait(|| f.status("skipped"));
        drop(monitor);
        assert_eq!(f.requests.lock().unwrap().len(), 1);
    }
}
