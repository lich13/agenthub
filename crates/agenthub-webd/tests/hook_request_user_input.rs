use agenthub_core::{config::Config, db::PanelDb, platform::PlatformKind};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn hook_request_user_input_accepts_official_pre_tool_use_stdin_with_empty_stdout() {
    let (root, config_path, config) = test_config("valid");
    write_config(&config_path, &config);
    let transcript = write_pending_rollout(&config, "turn-hook", "call-hook");
    let payload = pre_tool_use_payload(Some(&transcript));
    let db = PanelDb::open(&config.paths.db_path).unwrap();

    let started = Instant::now();
    let output = run_hook(&config_path, payload.to_string().as_bytes());
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(elapsed < Duration::from_secs(2), "elapsed: {elapsed:?}");
    let events = wait_for_event_count(&db, 1, Duration::from_secs(4));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "reply-needed");
    assert_eq!(events[0].thread_id.as_deref(), Some("thread-hook"));
    assert_eq!(events[0].payload["turn_id"], "turn-hook");
    assert_eq!(events[0].payload["call_id"], "call-hook");
    assert_eq!(events[0].payload["body_source"], "request_user_input");
    assert_eq!(events[0].payload["scan_source"], "pre-tool-use-hook");
    assert_eq!(
        events[0].payload["transcript_path"],
        transcript.to_string_lossy().as_ref()
    );
    assert_eq!(
        events[0].payload["question_confirmation_strategy"],
        "rollout_unresolved_after_grace"
    );
    assert_eq!(events[0].payload["question_confirmation_delay_ms"], 1000);
    assert_eq!(events[0].payload["question_confirmed_pending"], true);
    assert_eq!(events[0].title.as_deref(), Some("thread-hook"));
    assert_eq!(events[0].payload["bark"]["title"], "等待回复：thread-hook");
    assert!(events[0].payload["body_summary"]
        .as_str()
        .is_some_and(|body| body.contains("Choose a mode?") && body.contains("Safe mode")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_uses_current_local_thread_title_for_bark() {
    let (root, config_path, config) = test_config("local-title");
    seed_local_thread_title(
        &config,
        "thread-hook",
        "请为示例图书馆设计一份周末活动安排。",
        "示例图书馆周末活动安排",
    );
    write_config(&config_path, &config);
    let transcript = write_pending_rollout(&config, "turn-hook", "call-hook");
    let db = PanelDb::open(&config.paths.db_path).unwrap();

    let output = run_hook(
        &config_path,
        pre_tool_use_payload(Some(&transcript))
            .to_string()
            .as_bytes(),
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let events = wait_for_event_count(&db, 1, Duration::from_secs(4));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title.as_deref(), Some("示例图书馆周末活动安排"));
    assert_eq!(events[0].payload["thread_title"], "示例图书馆周末活动安排");
    assert_eq!(
        events[0].payload["bark"]["title"],
        "等待回复：示例图书馆周末活动安排"
    );
    assert_eq!(events[0].payload["call_id"], "call-hook");
    assert!(events[0].payload["body_summary"]
        .as_str()
        .is_some_and(|body| body.contains("Choose a mode?") && body.contains("Safe mode")));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_prefers_explicit_payload_thread_title() {
    let (root, config_path, config) = test_config("explicit-title");
    seed_local_thread_title(
        &config,
        "thread-hook",
        "Local first message",
        "Local generated title",
    );
    write_config(&config_path, &config);
    let transcript = write_pending_rollout(&config, "turn-hook", "call-hook");
    let mut payload = pre_tool_use_payload(Some(&transcript));
    payload["thread_title"] = json!("Explicit hook title");
    let db = PanelDb::open(&config.paths.db_path).unwrap();

    let output = run_hook(&config_path, payload.to_string().as_bytes());

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let events = wait_for_event_count(&db, 1, Duration::from_secs(4));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title.as_deref(), Some("Explicit hook title"));
    assert_eq!(events[0].payload["thread_title"], "Explicit hook title");
    assert_eq!(
        events[0].payload["bark"]["title"],
        "等待回复：Explicit hook title"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_allows_tool_but_suppresses_unconfirmed_task_identity() {
    let (root, config_path, config) = test_config("invalid-local-title-state");
    fs::write(
        config.codex.home.join("state_5.sqlite"),
        b"not a sqlite database",
    )
    .unwrap();
    write_config(&config_path, &config);
    let transcript = write_pending_rollout(&config, "turn-hook", "call-hook");
    let db = PanelDb::open(&config.paths.db_path).unwrap();

    let output = run_hook(
        &config_path,
        pre_tool_use_payload(Some(&transcript))
            .to_string()
            .as_bytes(),
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    std::thread::sleep(Duration::from_millis(1200));
    assert!(db.list_probe_events(10).unwrap().is_empty());
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    for table in ["probe_events", "probe_dedupe"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_memory_context_is_suppressed_with_empty_stdout() {
    let (root, config_path, mut config) = test_config("memory-suppressed");
    let memory_root = config.codex.home.join("memories");
    fs::create_dir_all(&memory_root).unwrap();
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    write_config(&config_path, &config);
    let db =
        PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box().unwrap()).unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"test-device-key")
        .unwrap();
    let mut payload = pre_tool_use_payload(None);
    payload["cwd"] = json!(memory_root);
    payload["tool_input"] = json!("must not be parsed for internal memory work");

    let output = run_hook(&config_path, payload.to_string().as_bytes());

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(db.list_probe_events(10).unwrap().is_empty());
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_invalid_stdin_fails_open_without_stdout_or_recording() {
    let (root, config_path, config) = test_config("invalid");
    write_config(&config_path, &config);

    let invalid_payloads = vec![
        b"not-json".to_vec(),
        json!({
            "hook_event_name": "PreToolUse",
            "session_id": "thread-hook",
            "turn_id": "turn-hook",
            "tool_name": "exec_command",
            "tool_use_id": "call-hook",
            "tool_input": {"questions": []}
        })
        .to_string()
        .into_bytes(),
    ];
    for payload in invalid_payloads {
        let output = run_hook(&config_path, &payload);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("probe_hook_request_user_input_failed"));
        assert!(!stderr.contains("not-json"));
    }

    let db = PanelDb::open(&config.paths.db_path).unwrap();
    assert!(db.list_probe_events(10).unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_resolved_screenshot_calls_have_zero_side_effects() {
    let (root, config_path, mut config) = test_config("resolved-screenshot-calls");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = format!("http://{}", listener.local_addr().unwrap());
    write_config(&config_path, &config);
    let db =
        PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box().unwrap()).unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"test-device-key")
        .unwrap();
    let turn_id = "00000000-0000-4000-8000-000000000100";
    let call_ids = [
        "call_ewbJTMQy5Y9jfFvGrL4cQRhd",
        "call_BlwC14PE47BjSEAbt54FOdPo",
        "call_J93MFun82lrB2G97ZWg5bMUu",
        "call_C9kAFgmLeNwVTFAZOMU9GecB",
    ];
    let transcript = write_resolved_rollout(&config, turn_id, &call_ids);

    for call_id in call_ids {
        let mut payload = pre_tool_use_payload(Some(&transcript));
        payload["session_id"] = json!("00000000-0000-4000-8000-000000000107");
        payload["turn_id"] = json!(turn_id);
        payload["tool_use_id"] = json!(call_id);
        let output = run_hook(&config_path, payload.to_string().as_bytes());
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }

    thread::sleep(Duration::from_millis(1_800));
    assert!(db.list_probe_events(10).unwrap().is_empty());
    assert_probe_side_effect_counts(&db, 0, 0);
    let mut byte = [0_u8; 1];
    match listener.accept() {
        Ok((mut stream, _)) => panic!("unexpected Bark request: {:?}", stream.read(&mut byte)),
        Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock),
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_missing_or_invalid_transcript_has_zero_side_effects() {
    let (root, config_path, config) = test_config("missing-transcript");
    write_config(&config_path, &config);
    let db = PanelDb::open(&config.paths.db_path).unwrap();
    let missing = root.join("missing-rollout.jsonl");

    for transcript in [None, Some(missing.as_path())] {
        let output = run_hook(
            &config_path,
            pre_tool_use_payload(transcript).to_string().as_bytes(),
        );
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }

    thread::sleep(Duration::from_millis(1_800));
    assert!(db.list_probe_events(10).unwrap().is_empty());
    assert_probe_side_effect_counts(&db, 0, 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_answer_during_confirmation_window_is_not_recorded() {
    let (root, config_path, config) = test_config("answered-during-window");
    write_config(&config_path, &config);
    let db = PanelDb::open(&config.paths.db_path).unwrap();
    let transcript = write_pending_rollout(&config, "turn-hook", "call-hook");

    let output = run_hook(
        &config_path,
        pre_tool_use_payload(Some(&transcript))
            .to_string()
            .as_bytes(),
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    // Write the resolution well before the fixed one-second confirmation grace period.
    thread::sleep(Duration::from_millis(50));
    append_rollout_event(
        &transcript,
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-hook","output":"{\"answers\":{\"mode\":{\"answers\":[\"Safe mode\"]}}}","internal_chat_message_metadata_passthrough":{"turn_id":"turn-hook"}}}),
    );

    thread::sleep(Duration::from_millis(1_300));
    assert!(db.list_probe_events(10).unwrap().is_empty());
    assert_probe_side_effect_counts(&db, 0, 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_duplicate_pending_schedules_record_once() {
    let (root, config_path, config) = test_config("duplicate-pending");
    write_config(&config_path, &config);
    let db = PanelDb::open(&config.paths.db_path).unwrap();
    let transcript = write_pending_rollout(&config, "turn-hook", "call-hook");
    let payload = pre_tool_use_payload(Some(&transcript));

    for _ in 0..2 {
        let output = run_hook(&config_path, payload.to_string().as_bytes());
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }

    let events = wait_for_event_count(&db, 1, Duration::from_secs(4));
    thread::sleep(Duration::from_millis(300));
    assert_eq!(events.len(), 1);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
    assert_probe_side_effect_counts(&db, 1, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hook_request_user_input_bark_timeout_is_bounded_to_three_seconds() {
    let (root, config_path, mut config) = test_config("timeout");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        if let Ok((_stream, _)) = listener.accept() {
            thread::sleep(Duration::from_secs(8));
        }
    });
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = format!("http://{address}");
    write_config(&config_path, &config);
    let db =
        PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box().unwrap()).unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"test-device-key")
        .unwrap();
    let transcript = write_pending_rollout(&config, "turn-hook", "call-hook");

    let started = Instant::now();
    let output = run_hook(
        &config_path,
        pre_tool_use_payload(Some(&transcript))
            .to_string()
            .as_bytes(),
    );
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(elapsed < Duration::from_secs(2), "elapsed: {elapsed:?}");
    let events = wait_for_event_count(&db, 1, Duration::from_secs(6));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["bark"]["reason"], "timeout");
    assert_eq!(events[0].payload["bark"]["request_count"], 1);

    fs::remove_dir_all(root).unwrap();
}

fn pre_tool_use_payload(transcript_path: Option<&Path>) -> serde_json::Value {
    json!({
        "cwd": "/tmp",
        "hook_event_name": "PreToolUse",
        "model": "gpt-test",
        "permission_mode": "default",
        "session_id": "thread-hook",
        "tool_input": {
            "questions": [{
                "id": "mode",
                "header": "Mode",
                "question": "Choose a mode?",
                "options": [
                    {"label": "Safe mode", "description": "Use guarded execution"},
                    {"label": "Fast mode", "description": "Use direct execution"}
                ]
            }]
        },
        "tool_name": "request_user_input",
        "tool_use_id": "call-hook",
        "transcript_path": transcript_path,
        "turn_id": "turn-hook"
    })
}

fn write_pending_rollout(config: &Config, turn_id: &str, call_id: &str) -> PathBuf {
    let state_db = config.codex.home.join("state_5.sqlite");
    if !state_db.exists() {
        let conn = Connection::open(&state_db).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, first_user_message TEXT, source TEXT, thread_source TEXT, updated_at INTEGER, rollout_path TEXT);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads(id, title, first_user_message, source, thread_source, updated_at, rollout_path) VALUES('thread-hook', 'thread-hook', 'thread-hook', 'vscode', 'user', 1, NULL)",
            [],
        )
        .unwrap();
    }
    let path = config.codex.home.join("sessions/pending-rollout.jsonl");
    fs::write(
        &path,
        json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "request_user_input",
                "arguments": "{\"questions\":[{\"id\":\"mode\",\"question\":\"Choose a mode?\",\"options\":[{\"label\":\"Safe mode\"}]}]}",
                "call_id": call_id,
                "internal_chat_message_metadata_passthrough": {"turn_id": turn_id}
            }
        })
        .to_string(),
    )
    .unwrap();
    path
}

fn write_resolved_rollout(config: &Config, turn_id: &str, call_ids: &[&str]) -> PathBuf {
    let path = config.codex.home.join("sessions/resolved-rollout.jsonl");
    let mut events = Vec::new();
    for call_id in call_ids {
        events.push(json!({"type":"response_item","payload":{"type":"function_call","name":"request_user_input","arguments":"{\"questions\":[{\"id\":\"unused\",\"question\":\"占位\",\"options\":[{\"label\":\"继续（推荐）\"}]}]}","call_id":call_id,"internal_chat_message_metadata_passthrough":{"turn_id":turn_id}}}));
        events.push(json!({"type":"response_item","payload":{"type":"function_call_output","call_id":call_id,"output":"request_user_input is unavailable in Default mode","internal_chat_message_metadata_passthrough":{"turn_id":turn_id}}}));
    }
    fs::write(
        &path,
        events
            .into_iter()
            .map(|event| event.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    path
}

fn append_rollout_event(path: &Path, event: serde_json::Value) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{}", event).unwrap();
}

fn wait_for_event_count(
    db: &PanelDb,
    expected: usize,
    timeout: Duration,
) -> Vec<agenthub_core::db::ProbeEvent> {
    let started = Instant::now();
    loop {
        let events = db.list_probe_events(10).unwrap();
        if events.len() >= expected {
            return events;
        }
        assert!(
            started.elapsed() < timeout,
            "timed out waiting for {expected} events"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn assert_probe_side_effect_counts(db: &PanelDb, dedupe: i64, marker: i64) {
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    let dedupe_count = conn
        .query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    let marker_count = conn
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key LIKE 'probe_passive_sent_marker:%'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(dedupe_count, dedupe);
    assert_eq!(marker_count, marker);
}

fn cargo_webd_binary() -> PathBuf {
    let binary = std::env::var_os("CARGO_BIN_EXE_agenthub-webd")
        .map(PathBuf::from)
        .or_else(|| option_env!("CARGO_BIN_EXE_agenthub-webd").map(PathBuf::from))
        .expect("Cargo must provide CARGO_BIN_EXE_agenthub-webd for this integration test");
    assert!(binary.is_absolute(), "Cargo binary path must be absolute");
    let binary = binary
        .canonicalize()
        .expect("Cargo binary path must resolve to an existing build artifact");
    assert!(binary.is_file(), "Cargo binary path must point to a file");

    let test_binary = std::env::current_exe()
        .expect("current integration test executable must be available")
        .canonicalize()
        .expect("current integration test executable must exist");
    // Cargo places test executables in the profile's `deps` directory or, with
    // the per-package layout, deeper below the same profile directory.
    assert!(
        binary.file_name().is_some_and(
            |name| name == format!("agenthub-webd{}", std::env::consts::EXE_SUFFIX).as_str()
        ) && binary
            .parent()
            .is_some_and(|profile| test_binary.starts_with(profile)),
        "Cargo binary must belong to the same build directory and profile as this integration test"
    );
    binary
}

fn run_hook(config_path: &Path, stdin: &[u8]) -> Output {
    let mut child = Command::new(cargo_webd_binary())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "probe",
            "hook-request-user-input",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn test_config(name: &str) -> (PathBuf, PathBuf, Config) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("agenthub-hook-request-{name}-{unique}"));
    fs::create_dir_all(root.join(".codex/sessions")).unwrap();
    let mut config = Config::for_platform_kind(PlatformKind::Macos);
    config.codex.home = root.join(".codex");
    config.paths.data_dir = root.join("data");
    config.paths.db_path = root.join("data/agenthub.sqlite");
    config.paths.log_dir = root.join("logs");
    config.security.secret_key = "7q9DCmCPyxnTrH3FhrV1sUJol1yqPgscQsBnR-mXA2E".to_string();
    config.probe.notifications.enabled = false;
    let config_path = root.join("config.toml");
    (root, config_path, config)
}

fn write_config(path: &Path, config: &Config) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, toml::to_string_pretty(config).unwrap()).unwrap();
}

fn seed_local_thread_title(
    config: &Config,
    thread_id: &str,
    first_user_message: &str,
    current_title: &str,
) {
    let state_db = config.codex.home.join("state_5.sqlite");
    let conn = Connection::open(state_db).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT,
            first_user_message TEXT,
            source TEXT,
            thread_source TEXT,
            updated_at INTEGER,
            rollout_path TEXT
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, title, first_user_message, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, ?3, 'vscode', 'user', ?4, NULL)",
        params![
            thread_id,
            first_user_message,
            first_user_message,
            chrono::Utc::now().timestamp_millis()
        ],
    )
    .unwrap();
    fs::write(
        config.codex.home.join("session_index.jsonl"),
        json!({
            "id": thread_id,
            "thread_name": current_title,
            "updated_at": "2026-07-11T00:00:00Z"
        })
        .to_string(),
    )
    .unwrap();
}
