use agenthub_core::{
    claude::{
        self,
        hooks::{self, ClaudeHooksAction, ClaudeHooksRequest},
        ClaudePaths,
    },
    config::Config,
    db::PanelDb,
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

struct Fixture {
    root: PathBuf,
    paths: ClaudePaths,
    transcript: PathBuf,
    config_path: PathBuf,
    config: Config,
    db: PanelDb,
    binary: PathBuf,
    requests: Arc<Mutex<Vec<Value>>>,
    reply_code: Arc<AtomicU16>,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
}

struct Monitor(Child);
impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("agenthub-claude-hook-{}", uuid::Uuid::new_v4()));
        let paths = ClaudePaths {
            config: root.join("claude"),
            projects: root.join("claude/projects"),
        };
        let transcript = paths.projects.join("fixture/session.jsonl");
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::create_dir_all(root.join("codex/sessions")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reply_code = Arc::new(AtomicU16::new(200));
        let response_code = reply_code.clone();
        let (captured, stopped) = (requests.clone(), stop.clone());
        let server = thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                // macOS can inherit the listener's nonblocking flag. Wait for
                // the request instead of dropping a just-accepted connection.
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
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= offset + 4 + length {
                            let payload: Value =
                                serde_json::from_slice(&bytes[offset + 4..offset + 4 + length])
                                    .unwrap();
                            captured.lock().unwrap().push(payload);
                            let code = response_code.load(Ordering::Relaxed);
                            let response = format!("HTTP/1.1 {code} Response\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{{\"code\":{code}}}");
                            let _ = stream.write_all(response.as_bytes());
                            break;
                        }
                    }
                }
            }
        });
        let mut config = Config::for_platform_kind(PlatformKind::Macos);
        config.codex.home = root.join("codex");
        config.paths.data_dir = root.join("data");
        config.paths.db_path = root.join("data/panel.sqlite");
        config.paths.log_dir = root.join("logs");
        config.security.secret_key = "7q9DCmCPyxnTrH3FhrV1sUJol1yqPgscQsBnR-mXA2E".into();
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = url;
        config.probe.notifications.notify_grok = false;
        config.probe.notifications.sound = Some("bell".into());
        config.probe.notifications.url = Some("https://example.invalid/thread".into());
        let config_path = root.join("config.toml");
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        let db = PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box().unwrap())
            .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        db.sync_notification_channels(&config).unwrap();
        db.stage_native_notifications(NativeProvider::Claude, &NativeScan::default(), &config)
            .unwrap();
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_agenthub-webd"))
            .canonicalize()
            .unwrap();
        let profile = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        assert_eq!(
            binary.parent(),
            Some(profile.as_path()),
            "must launch this build's CLI"
        );
        hooks::manage(
            &paths,
            &binary,
            &config_path,
            ClaudeHooksRequest {
                action: ClaudeHooksAction::Install,
                confirmed: true,
            },
        )
        .unwrap();
        Self {
            root,
            paths,
            transcript,
            config_path,
            config,
            db,
            binary,
            requests,
            reply_code,
            stop,
            server: Some(server),
        }
    }
    fn append(&self, value: Value) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.transcript)
            .unwrap();
        writeln!(file, "{value}").unwrap();
    }
    fn user(&self, uuid: &str, parent: Option<&str>, prompt: &str) {
        self.append(json!({"type":"user","uuid":uuid,"parentUuid":parent,"promptId":prompt,"sessionId":"session-fixture","version":"2.1.284","cwd":"/fixture/project","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"user","content":"Synthetic prompt"}}));
    }
    fn complete(&self) {
        self.append(json!({"type":"assistant","uuid":"answer-one","parentUuid":"message-one","sessionId":"session-fixture","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"assistant","content":"**完成**\n- 已核对 🙂","stop_reason":"end_turn"}}));
    }
    fn input(&self, event: &str, subtype: &str, prompt: &str) -> Value {
        json!({"session_id":"session-fixture","prompt_id":prompt,"transcript_path":self.transcript,"hook_event_name":event,"notification_type":subtype,"message":"**请确认**\n选择蓝色或绿色。","last_assistant_message":"**完成**\n- 已核对 🙂","error":"rate_limit","error_details":"private fixture error details"})
    }
    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .args(["--config", self.config_path.to_str().unwrap()])
            .env("CLAUDE_CONFIG_DIR", &self.paths.config)
            .env("GROK_BUILD_HOME", self.root.join("grok"));
        command
    }
    fn hook(&self, value: &Value) {
        let mut child = self
            .command()
            .arg("claude-hook")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(value.to_string().as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("private fixture"));
    }
    fn monitor(&self) -> Monitor {
        Monitor(
            self.command()
                .args(["probe", "monitor-errors"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn wait_count(&self, count: usize) {
        let start = Instant::now();
        while self.requests.lock().unwrap().len() < count
            && start.elapsed() < Duration::from_secs(10)
        {
            thread::sleep(Duration::from_millis(20));
        }
        let reasons = Connection::open(&self.config.paths.db_path).unwrap()
            .prepare("SELECT status, json_extract(outcome_json, '$.reason') FROM probe_notification_deliveries").unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(
            self.requests.lock().unwrap().len(),
            count,
            "delivery states: {reasons:?}"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn real_cli_hooks_and_polling_share_one_claim_per_prompt_category() {
    let f = Fixture::new();
    f.user("message-one", None, "prompt-one");
    f.complete();
    let context = claude::notification_context(&f.paths, &f.transcript).unwrap();
    f.db.stage_native_notifications_with_hooks(
        NativeProvider::Claude,
        &NativeScan {
            streams: vec![context.stream],
            errors: 0,
        },
        &f.config,
        true,
    )
    .unwrap();
    let monitor = f.monitor();
    let completion = f.input("Stop", "", "prompt-one");
    thread::scope(|scope| {
        scope.spawn(|| f.hook(&completion));
        scope.spawn(|| f.hook(&completion));
    });
    f.user("message-two", Some("answer-one"), "prompt-two");
    for event in [
        f.input("Notification", "permission_prompt", "prompt-one"),
        f.input("Notification", "elicitation_dialog", "prompt-one"),
        f.input("StopFailure", "", "prompt-two"),
    ] {
        f.hook(&event);
        f.hook(&event);
    }
    f.hook(&f.input("Notification", "idle_prompt", "prompt-one"));
    let context = claude::notification_context(&f.paths, &f.transcript).unwrap();
    f.db.stage_native_notifications_with_hooks(
        NativeProvider::Claude,
        &NativeScan {
            streams: vec![context.stream],
            errors: 0,
        },
        &f.config,
        true,
    )
    .unwrap();
    f.wait_count(4);
    thread::sleep(Duration::from_millis(250));
    drop(monitor);
    let requests = f.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let statuses: Vec<_> = requests
        .iter()
        .map(|v| v["title"].as_str().unwrap().split('｜').nth(1).unwrap())
        .collect();
    assert_eq!(statuses, vec!["完成", "需要授权", "需要回复", "失败"]);
    for request in requests.iter() {
        assert_eq!(request["group"], "AgentHub｜Claude Code");
        assert_eq!(request["sound"], "bell");
        assert_eq!(request["url"], "https://example.invalid/thread");
        let body = request["body"].as_str().unwrap();
        assert!(!body.contains("fixture") && !body.contains('*'));
        assert!(body.chars().count() <= 300);
    }
    assert_eq!(requests[1]["level"], "timeSensitive");
    assert_eq!(requests[2]["level"], "timeSensitive");
    assert_eq!(requests[0]["level"], "active");
}

#[test]
fn unflushed_prompt_is_encrypted_then_correlated_after_a_new_turn_without_uuid_guessing() {
    let f = Fixture::new();
    f.user("message-old", None, "prompt-old");
    f.hook(&f.input("Stop", "", "prompt-late"));
    assert!(f.requests.lock().unwrap().is_empty());
    let connection = Connection::open(f.db.path()).unwrap();
    let (status, encrypted): (String,bool) = connection.query_row("SELECT status,ciphertext IS NOT NULL FROM probe_notification_deliveries WHERE event_key LIKE 'claude-hook:%'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(status, "pending");
    assert!(encrypted);
    f.user("message-one", Some("message-old"), "prompt-late");
    f.complete();
    f.user("message-two", Some("answer-one"), "prompt-next");
    connection
        .execute("UPDATE probe_notification_deliveries SET next_ms=0", [])
        .unwrap();
    let monitor = f.monitor();
    f.wait_count(1);
    drop(monitor);
    f.hook(&f.input("Stop", "", "message-two"));
    assert_eq!(
        f.requests.lock().unwrap().len(),
        1,
        "message uuid must never become prompt identity"
    );
}

#[test]
fn hook_stdin_stall_is_bounded_and_disabling_notifications_prevents_delivery() {
    let f = Fixture::new();
    let start = Instant::now();
    let mut child = f
        .command()
        .arg("claude-hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _held_stdin = child.stdin.take().unwrap();
    assert!(child.wait_with_output().unwrap().stdout.is_empty());
    assert!(start.elapsed() < Duration::from_secs(8));
    let mut config = f.config.clone();
    config.probe.notifications.enabled = false;
    fs::write(&f.config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    f.user("message-one", None, "prompt-one");
    f.complete();
    f.hook(&f.input("Stop", "", "prompt-one"));
    assert!(f.requests.lock().unwrap().is_empty());
}

#[test]
fn idle_never_replays_a_baselined_terminal_record() {
    let f = Fixture::new();
    f.user("message-one", None, "prompt-one");
    f.complete();
    let context = claude::notification_context(&f.paths, &f.transcript).unwrap();
    let scan = NativeScan {
        streams: vec![context.stream],
        errors: 0,
    };
    let mut disabled = f.config.clone();
    disabled.probe.notifications.notify_claude = false;
    f.db.stage_native_notifications(NativeProvider::Claude, &scan, &disabled)
        .unwrap();
    f.db.stage_native_notifications(NativeProvider::Claude, &scan, &f.config)
        .unwrap();
    f.hook(&f.input("Notification", "idle_prompt", "prompt-one"));
    assert!(f.requests.lock().unwrap().is_empty());
}

#[test]
fn installing_hooks_preserves_an_existing_successful_native_delivery() {
    let f = Fixture::new();
    f.user("message-one", None, "prompt-one");
    f.complete();
    let context = claude::notification_context(&f.paths, &f.transcript).unwrap();
    f.db.stage_native_notifications(
        NativeProvider::Claude,
        &NativeScan {
            streams: vec![context.stream],
            errors: 0,
        },
        &f.config,
    )
    .unwrap();
    let delivery = f.db.pending_native_deliveries(10, None).unwrap().remove(0);
    assert!(f.db.claim_native_delivery(&delivery.event_key).unwrap());
    f.db.finish_native_delivery(
        &delivery,
        json!({"sent":true,"skipped":false,"status":"sent"}),
    )
    .unwrap();
    f.hook(&f.input("Stop", "", "prompt-one"));
    f.hook(&f.input("Notification", "idle_prompt", "prompt-one"));
    assert!(f.requests.lock().unwrap().is_empty());
}

#[test]
fn stale_failures_and_stops_without_terminal_evidence_do_not_send() {
    let f = Fixture::new();
    f.user("message-one", None, "prompt-one");
    f.user("message-two", Some("message-one"), "prompt-two");
    f.hook(&f.input("StopFailure", "", "prompt-one"));
    f.hook(&f.input("Stop", "", "prompt-two"));
    assert!(f.requests.lock().unwrap().is_empty());
    let conn = Connection::open(&f.config.paths.db_path).unwrap();
    let pending: i64 = conn.query_row("SELECT count(*) FROM probe_notification_deliveries WHERE status='pending' AND ciphertext IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(
        pending, 1,
        "unwritten terminal evidence stays in the bounded queue"
    );
}

#[test]
fn failure_retry_is_discarded_after_recovery_and_clears_queued_body() {
    let f = Fixture::new();
    f.user("message-one", None, "prompt-one");
    f.reply_code.store(503, Ordering::Relaxed);
    f.hook(&f.input("StopFailure", "", "prompt-one"));
    f.wait_count(1);
    f.append(json!({"type":"assistant","uuid":"resumed","parentUuid":"message-one","sessionId":"session-fixture","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"assistant","content":"Working again","stop_reason":null}}));
    f.reply_code.store(200, Ordering::Relaxed);
    let conn = Connection::open(&f.config.paths.db_path).unwrap();
    conn.execute(
        "UPDATE probe_notification_deliveries SET next_ms=0 WHERE status='pending'",
        [],
    )
    .unwrap();
    let monitor = f.monitor();
    let start = Instant::now();
    loop {
        let remaining: i64 = conn
            .query_row(
                "SELECT count(*) FROM probe_notification_deliveries WHERE ciphertext IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        if remaining == 0 {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "stale retry must reach a terminal outcome"
        );
        thread::sleep(Duration::from_millis(20));
    }
    drop(monitor);
    assert_eq!(
        f.requests.lock().unwrap().len(),
        1,
        "a recovered failure must not be retried"
    );
}

#[test]
fn same_turn_recovery_completion_and_cancellation_reject_late_failure_hooks() {
    for recovery in ["resume", "complete", "cancel"] {
        let f = Fixture::new();
        f.user("message-one", None, "prompt-one");
        f.append(json!({"type":"assistant","uuid":"error-one","parentUuid":"message-one","sessionId":"session-fixture","isApiErrorMessage":true,"timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"assistant","content":"API error fixture","stop_reason":"end_turn"}}));
        match recovery {
            "resume" => f.append(json!({"type":"assistant","uuid":"resumed","parentUuid":"error-one","sessionId":"session-fixture","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"assistant","content":"Working again","stop_reason":null}})),
            "complete" => f.append(json!({"type":"assistant","uuid":"done","parentUuid":"error-one","sessionId":"session-fixture","timestamp":chrono::Utc::now().to_rfc3339(),"message":{"role":"assistant","content":"Done","stop_reason":"end_turn"}})),
            _ => f.append(json!({"type":"system","subtype":"cancelled"})),
        }
        f.hook(&f.input("StopFailure", "", "prompt-one"));
        assert!(f.requests.lock().unwrap().is_empty(), "{recovery}");
        let conn = Connection::open(&f.config.paths.db_path).unwrap();
        let pending: i64 = conn
            .query_row(
                "SELECT count(*) FROM probe_notification_deliveries WHERE ciphertext IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0, "{recovery}");
    }
}

#[test]
fn polling_rechecks_failed_turns_but_delivers_valid_historical_completion() {
    for kind in ["failure", "completion"] {
        let f = Fixture::new();
        f.user("message-one", None, "prompt-one");
        if kind == "completion" {
            f.complete();
        } else {
            f.append(json!({"type":"result","subtype":"error_during_execution","is_error":true,"errors":["fixture failure"],"timestamp":chrono::Utc::now().to_rfc3339()}));
        }
        let context = claude::notification_context(&f.paths, &f.transcript).unwrap();
        f.db.stage_native_notifications(
            NativeProvider::Claude,
            &NativeScan {
                streams: vec![context.stream],
                errors: 0,
            },
            &f.config,
        )
        .unwrap();
        f.user(
            "message-two",
            Some(if kind == "completion" {
                "answer-one"
            } else {
                "message-one"
            }),
            "prompt-two",
        );
        let monitor = f.monitor();
        let start = Instant::now();
        loop {
            if !f.db.list_probe_events(10).unwrap().is_empty() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "native delivery did not settle"
            );
            thread::sleep(Duration::from_millis(20));
        }
        drop(monitor);
        let events = f.db.list_probe_events(10).unwrap();
        assert_eq!(
            events[0].payload["bark_status"],
            if kind == "completion" {
                "sent"
            } else {
                "skipped"
            }
        );
        assert_eq!(
            f.requests.lock().unwrap().len(),
            usize::from(kind == "completion")
        );
    }
}
