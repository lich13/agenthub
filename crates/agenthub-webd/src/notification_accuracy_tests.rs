use super::*;
use rusqlite::Connection;

pub(super) struct TestTaskIdentity(PathBuf);

impl Drop for TestTaskIdentity {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn seed_main_task_identities(home: &Path, ids: &[&str]) {
    fs::create_dir_all(home).unwrap();
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, source TEXT, thread_source TEXT, agent_path TEXT);").unwrap();
    for id in ids {
        db.execute(
            "INSERT INTO threads VALUES (?1, 'vscode', 'user', NULL)",
            [id],
        )
        .unwrap();
    }
}

pub(super) fn test_notification_config(ids: &[&str]) -> (Config, TestTaskIdentity) {
    let root =
        std::env::temp_dir().join(format!("agenthub-test-identity-{}", uuid::Uuid::new_v4()));
    let mut config = Config::default();
    config.codex.home = root.join(".codex");
    seed_main_task_identities(&config.codex.home, ids);
    (config, TestTaskIdentity(root))
}

struct Fixture {
    root: PathBuf,
    config: Config,
    transcript: PathBuf,
}

impl Fixture {
    fn new(events: &[Value]) -> Self {
        let root =
            std::env::temp_dir().join(format!("agenthub-notification-{}", uuid::Uuid::new_v4()));
        let home = root.join(".codex");
        fs::create_dir_all(home.join("sessions")).unwrap();
        let transcript = home.join("sessions/rollout.jsonl");
        fs::write(
            &transcript,
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        fs::write(home.join("session_index.jsonl"), "").unwrap();
        let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, name TEXT, title TEXT, source TEXT, thread_source TEXT, agent_path TEXT, rollout_path TEXT, updated_at INTEGER);
            INSERT INTO threads VALUES ('main', 'Current native title', 'Long original first message', 'vscode', 'user', NULL, NULL, 1);
            INSERT INTO threads VALUES ('child', 'Child title', 'Child title', 'vscode', 'subagent', '/root/child', NULL, 1);").unwrap();
        conn.execute(
            "UPDATE threads SET rollout_path = ?1",
            [transcript.to_str().unwrap()],
        )
        .unwrap();
        let mut config = Config::default();
        config.codex.home = home;
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_completion = true;
        config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
        Self {
            root,
            config,
            transcript,
        }
    }

    fn payload(&self, id: &str) -> Value {
        json!({"session_id":id,"turn_id":"root-turn","transcript_path":self.transcript,"last_assistant_message":"Unconfirmed summary"})
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn hook_stop_accuracy_never_falls_back_to_unconfirmed_stdin() {
    assert!(
        hook_stop_last_assistant_message(None, Some("turn-a"), Some("Working on it"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn hook_stop_accuracy_keeps_control_payload_suppression_without_transcript() {
    let selection =
        hook_stop_last_assistant_message(None, Some("turn-a"), Some(r#"{"suggestions":[]}"#))
            .unwrap()
            .unwrap();
    let event =
        probe_runtime(&Config::default()).build_event(ProbeEventInput::hook_stop_with_context(
            None,
            Some("turn-a"),
            None,
            None,
            Some(&selection.message),
            "hook-stop",
        ));
    assert_eq!(
        event.suppression_reason.as_deref(),
        Some("internal_control_payload")
    );
}

#[test]
fn notify_completion_accuracy_requires_verified_final_body() {
    let fixture = Fixture::new(&[]);
    let input =
        notify_completion_context(&fixture.config, Some(&fixture.payload("main")), None, None)
            .unwrap();
    let event = probe_runtime(&fixture.config).build_event(input);
    assert_eq!(
        event.suppression_reason.as_deref(),
        Some("unconfirmed_final_body")
    );
}

#[test]
fn notification_accuracy_unreadable_identity_never_allows_notifications_or_recovery() {
    let fixture = Fixture::new(&[]);
    fs::write(
        fixture.config.codex.home.join("state_5.sqlite"),
        "invalid sqlite",
    )
    .unwrap();
    assert_eq!(
        task_notification_suppression_reason(&fixture.config, Some("main")),
        Some("unconfirmed_task_identity")
    );
}

#[tokio::test]
async fn hook_stop_accuracy_fixture_turns_deliver_only_exact_terminal_answers() {
    let events: Vec<Value> =
        include_str!("../../agenthub-core/tests/fixtures/notification-turns.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    for boundary in [2, 7] {
        let terminal = &events[boundary];
        let turn = terminal["payload"]["turn_id"].as_str().unwrap();
        let expected = terminal["payload"]["last_agent_message"].as_str().unwrap();
        let mut fixture = Fixture::new(&events[..=boundary]);
        let server = super::tests::TestHttpServer::start_n(1, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}");
        fixture.config.probe.notifications.server_url = server.url();
        let db = PanelDb::open(fixture.root.join("panel.sqlite")).unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-key")
            .unwrap();
        let payload =
            json!({"session_id":"main","turn_id":turn,"transcript_path":fixture.transcript});
        let input = hook_stop_event_input(&fixture.config, Some(&payload), None, None, "hook-stop")
            .unwrap();
        let event = probe_runtime(&fixture.config).build_event(input);
        assert_eq!(event.payload["body_selected_turn_id"], turn);
        let (outcome, bark) =
            record_probe_event_with_bark(&|| Ok(fixture.config.clone()), &db, event)
                .await
                .unwrap();
        assert!(outcome.recorded && bark.sent);
        let request = server.request();
        let capture: Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            capture["body"],
            expected
                .replace("[sample](https://example.com/sample)", "sample")
                .replace("\n\n- ", "\n")
                .replace("\n- ", "\n")
        );
    }
}

#[tokio::test]
async fn hook_stop_accuracy_fixture_final_body_reaches_bark_with_current_title() {
    let final_body = "Updated the sample document and verified the result.";
    let mut fixture = Fixture::new(&[
        json!({"type":"turn_context","payload":{"turn_id":"root-turn"}}),
        json!({"type":"event_msg","payload":{"type":"agent_message","phase":"commentary","message":"I will review the file."}}),
        json!({"type":"response_item","payload":{"type":"agent_message","author":"/root/audit","recipient":"/root","content":[{"text":"Message Type: FINAL_ANSWER\nInternal report"}]}}),
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","internal_chat_message_metadata_passthrough":{"turn_id":"internal-model-turn"},"content":[{"text":final_body}]}}),
    ]);
    let server = super::tests::TestHttpServer::start_n(1, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}");
    fixture.config.probe.notifications.server_url = server.url();
    let db = PanelDb::open(fixture.root.join("panel.sqlite")).unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"test-only-key")
        .unwrap();
    let input = hook_stop_event_input(
        &fixture.config,
        Some(&fixture.payload("main")),
        None,
        None,
        "hook-stop",
    )
    .unwrap();
    let event = probe_runtime(&fixture.config).build_event(input);
    let (outcome, bark) =
        record_probe_event_with_bark(&|| Ok(fixture.config.clone()), &db, event.clone())
            .await
            .unwrap();
    assert!(outcome.recorded && bark.sent);
    let raw = server.request();
    let payload: Value = serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(payload["body"], final_body);
    assert_eq!(payload["title"], "Codex｜完成｜Current native title");
    assert_eq!(event.payload["body_selected_turn_id"], "root-turn");
    let (_, duplicate) = record_probe_event_with_bark(&|| Ok(fixture.config.clone()), &db, event)
        .await
        .unwrap();
    assert_eq!(duplicate.request_count, 0);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
}

#[tokio::test]
async fn hook_stop_accuracy_internal_unknown_and_unconfirmed_body_have_zero_side_effects() {
    let fixture = Fixture::new(&[
        json!({"type":"turn_context","payload":{"turn_id":"root-turn"}}),
        json!({"type":"response_item","payload":{"type":"agent_message","author":"/root/audit","recipient":"/root","content":[{"text":"Message Type: FINAL_ANSWER\nInternal report"}]}}),
    ]);
    let db = PanelDb::open(fixture.root.join("panel.sqlite")).unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"test-only-key")
        .unwrap();
    for (id, reason) in [
        ("main", "unconfirmed_final_body"),
        ("child", "internal_task"),
        ("missing", "unconfirmed_task_identity"),
    ] {
        let input = hook_stop_event_input(
            &fixture.config,
            Some(&fixture.payload(id)),
            None,
            None,
            "hook-stop",
        )
        .unwrap();
        let event = probe_runtime(&fixture.config).build_event(input);
        let (outcome, bark) =
            record_probe_event_with_bark(&|| Ok(fixture.config.clone()), &db, event)
                .await
                .unwrap();
        assert!(!outcome.recorded && !outcome.duplicate);
        assert_eq!(bark.request_count, 0);
        assert_eq!(bark.reason.as_deref(), Some(reason));
    }
    let conn = Connection::open(db.path()).unwrap();
    for table in ["probe_events", "probe_dedupe"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
