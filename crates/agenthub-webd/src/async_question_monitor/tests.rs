use super::*;
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Fixture {
    root: PathBuf,
    config: Config,
    db: PanelDb,
    requests: Arc<Mutex<Vec<Value>>>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new(status: StatusCode) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route(
                "/push",
                post(
                    move |State(requests): State<Arc<Mutex<Vec<Value>>>>,
                          Json(body): Json<Value>| async move {
                        requests.lock().unwrap().push(body);
                        (status, Json(json!({"code":status.as_u16()})))
                    },
                ),
            )
            .with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let root =
            std::env::temp_dir().join(format!("agenthub-async-questions-{}", uuid::Uuid::new_v4()));
        let home = root.join(".codex");
        fs::create_dir_all(home.join("sessions")).unwrap();
        let rollout = home.join("sessions/rollout.jsonl");
        fs::write(
            &rollout,
            format!(
                "{}\n{}\n",
                json!({"type":"session_meta","payload":{"id":"main"}}),
                json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-a"}})
            ),
        )
        .unwrap();
        let conn = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, updated_at INTEGER, archived_at INTEGER, archived INTEGER, rollout_path TEXT, cwd TEXT, model TEXT, source TEXT, thread_source TEXT, has_user_event INTEGER, first_user_message TEXT);").unwrap();
        conn.execute("INSERT INTO threads VALUES ('main','Question fixture',?1,NULL,0,?2,'/workspace','fixture','vscode','user',1,'fixture')", rusqlite::params![PanelDb::now(), rollout.to_string_lossy()]).unwrap();
        let mut config = Config::default();
        config.codex.home = home;
        config.probe.enabled = true;
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_codex = true;
        config.probe.notifications.notify_reply_needed = true;
        config.probe.notifications.server_url = url;
        let db = PanelDb::open_with_secret_box(
            root.join("panel.sqlite"),
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device-key")
            .unwrap();
        db.set_setting(
            "probe_async_question_baseline",
            &json!({"enabled":true,"since_ms":chrono::Utc::now().timestamp_millis()-60_000})
                .to_string(),
        )
        .unwrap();
        Self {
            root,
            config,
            db,
            requests,
            server,
        }
    }

    fn append(&self, value: Value) {
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(self.path())
                .unwrap(),
            "{value}"
        )
        .unwrap();
    }

    fn path(&self) -> PathBuf {
        self.config.codex.home.join("sessions/rollout.jsonl")
    }

    fn question(&self, call: &str) {
        self.append(json!({"type":"response_item","timestamp":chrono::Utc::now().to_rfc3339(),"payload":{"type":"function_call","name":"request_user_input_async","call_id":call,"arguments":json!({"questions":[{"title":"请确认测试结果。","options":["通过","失败"]},{"title":"请选择保留项。","options":["甲","乙"]}]}).to_string()}}));
        self.append(json!({"type":"response_item","payload":{"type":"function_call_output","call_id":call,"output":"{\"accepted\":true}"}}));
    }

    fn answer(&self, call: &str, index: usize) {
        let text = format!(
            "<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",
            json!([{"questionItemId":json!(["request_user_input_async",call,index]).to_string(),"question":"Fixture question","answer":"通过"}])
        );
        self.append(json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}}));
    }

    fn event(&self, call: &str) -> ProbeBuiltEvent {
        let paths = codex::resolve_codex_paths(&self.config.codex.home).codex_paths();
        let thread = codex::notification_thread_headers(&paths)
            .unwrap()
            .remove(0);
        let calls = codex::rollout_async_questions(&self.path()).unwrap();
        event_for(
            &self.config,
            &thread,
            calls.iter().find(|c| c.call_id == call).unwrap(),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn ongoing_question_is_delivered_once_across_hook_monitor_restart_and_ttl() {
    let fixture = Fixture::new(StatusCode::OK).await;
    fixture.question("call-a");
    fixture.append(json!({"type":"response_item","payload":{"type":"function_call","name":"exec","call_id":"work","arguments":"{}"}}));
    let event = fixture.event("call-a");
    let key = delivery_key(&event).unwrap();
    let load_config = || Ok(fixture.config.clone());
    let (hook, monitor) = tokio::join!(
        record_probe_event_with_bark(&load_config, &fixture.db, event.clone()),
        run(&fixture.config, &fixture.db)
    );
    hook.unwrap();
    monitor.unwrap();
    let payloads = fixture.requests.lock().unwrap().clone();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["title"], "Codex｜需要回复｜Question fixture");
    assert!(payloads[0]["body"]
        .as_str()
        .unwrap()
        .contains("请确认测试结果"));
    assert!(payloads[0]["body"]
        .as_str()
        .unwrap()
        .contains("请选择保留项"));
    assert!(payloads[0]["body"].as_str().unwrap().contains("失败"));
    fixture.answer("call-a", 0);
    let partial = prepare_event(&fixture.config, &fixture.db, event).unwrap();
    assert_eq!(delivery_key(&partial).as_deref(), Some(key.as_str()));
    assert!(!partial.bark_body.contains("请确认测试结果"));
    assert!(partial.bark_body.contains("请选择保留项"));
    fixture.append(json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-a","status":"completed"}}));
    rusqlite::Connection::open(fixture.root.join("panel.sqlite"))
        .unwrap()
        .execute("UPDATE probe_dedupe SET expires_at=0", [])
        .unwrap();
    let reopened = PanelDb::open_with_secret_box(
        fixture.root.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    run(&fixture.config, &reopened).await.unwrap();
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    fixture.answer("call-a", 1);
    assert!(codex::rollout_async_questions(&fixture.path()).unwrap()[0]
        .pending()
        .is_none());
}

#[tokio::test]
async fn each_call_has_its_own_identity_and_only_unanswered_questions_are_sent() {
    let fixture = Fixture::new(StatusCode::OK).await;
    fixture.question("call-a");
    fixture.question("call-b");
    fixture.answer("call-a", 0);
    run(&fixture.config, &fixture.db).await.unwrap();
    let payloads = fixture.requests.lock().unwrap();
    assert_eq!(payloads.len(), 2);
    assert!(!payloads[0]["body"]
        .as_str()
        .unwrap()
        .contains("请确认测试结果"));
    assert!(payloads[1]["body"]
        .as_str()
        .unwrap()
        .contains("请确认测试结果"));
}

#[tokio::test]
async fn first_enable_baselines_old_calls_and_rechecks_answers_before_sending() {
    let fixture = Fixture::new(StatusCode::OK).await;
    fixture.question("old-call");
    fixture
        .db
        .set_setting("probe_async_question_baseline", "{}")
        .unwrap();
    run(&fixture.config, &fixture.db).await.unwrap();
    assert!(fixture.requests.lock().unwrap().is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    fixture.question("new-call");
    let stale = fixture.event("new-call");
    fixture.answer("new-call", 0);
    fixture.answer("new-call", 1);
    let (_, outcome) =
        record_probe_event_with_bark(&|| Ok(fixture.config.clone()), &fixture.db, stale)
            .await
            .unwrap();
    assert_eq!(outcome.reason.as_deref(), Some("question_already_resolved"));
    run(&fixture.config, &fixture.db).await.unwrap();
    assert!(fixture.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn incomplete_tail_and_changed_native_identity_never_deliver() {
    let fixture = Fixture::new(StatusCode::OK).await;
    fixture.question("call-a");
    let stale = fixture.event("call-a");
    let complete = fs::read(fixture.path()).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.path())
        .unwrap()
        .write_all(b"{\"type\":")
        .unwrap();
    let (_, outcome) =
        record_probe_event_with_bark(&|| Ok(fixture.config.clone()), &fixture.db, stale.clone())
            .await
            .unwrap();
    assert_eq!(
        outcome.reason.as_deref(),
        Some("unconfirmed_question_transcript")
    );
    run(&fixture.config, &fixture.db).await.unwrap();
    assert!(fixture.requests.lock().unwrap().is_empty());
    fs::write(
        fixture.path(),
        String::from_utf8(complete)
            .unwrap()
            .replace("\"id\":\"main\"", "\"id\":\"other\""),
    )
    .unwrap();
    let (_, outcome) =
        record_probe_event_with_bark(&|| Ok(fixture.config.clone()), &fixture.db, stale)
            .await
            .unwrap();
    assert_eq!(
        outcome.reason.as_deref(),
        Some("question_transcript_identity_mismatch")
    );
    assert!(fixture.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn failed_delivery_is_persisted_without_claiming_success_or_retrying_on_restart() {
    let fixture = Fixture::new(StatusCode::INTERNAL_SERVER_ERROR).await;
    fixture.question("call-a");
    let key = delivery_key(&fixture.event("call-a")).unwrap();
    run(&fixture.config, &fixture.db).await.unwrap();
    let outcome: Value =
        serde_json::from_str(&fixture.db.get_setting(&key).unwrap().unwrap()).unwrap();
    assert_eq!(outcome["sent"], false);
    assert_eq!(outcome["http_status"], 500);
    assert_eq!(fixture.db.list_probe_events(10).unwrap().len(), 1);
    run(
        &fixture.config,
        &PanelDb::open_with_secret_box(
            fixture.root.join("panel.sqlite"),
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
}

#[test]
fn hook_accepts_async_title_string_options_and_free_text_questions() {
    let payload = json!({"hook_event_name":"PreToolUse","session_id":"main","turn_id":"turn-a","tool_name":"request_user_input_async","tool_use_id":"call-a","tool_input":{"questions":[{"title":"请确认。","options":["通过","失败"]},{"title":"请补充信息。"}]}});
    assert!(crate::validate_hook_request_user_input_payload(&payload).is_ok());
    let mut invalid = payload;
    invalid["tool_input"]["questions"][1]["title"] = json!("");
    assert!(crate::validate_hook_request_user_input_payload(&invalid).is_err());
}

#[tokio::test]
async fn startup_upgrade_is_idempotent_and_preserves_other_hooks_and_config() {
    let fixture = Fixture::new(StatusCode::OK).await;
    let user_hook =
        json!({"type":"command","command":"/usr/local/bin/example-user-hook","timeout":20});
    let stop = json!([{"matcher":"*","hooks":[user_hook.clone()]}]);
    let hook_path = fixture.config.codex.home.join("hooks.json");
    let config_path = fixture.config.codex.home.join("config.toml");
    fs::write(
        &config_path,
        "# User configuration\nmodel = 'example-model'\n",
    )
    .unwrap();
    fs::write(&hook_path,json!({"custom":true,"hooks":{"Stop":stop,"PreToolUse":[{"matcher":"^request_user_input$","custom":"keep","hooks":[user_hook.clone(),{"type":"command","command":"/usr/local/bin/agenthub-webd probe hook-request-user-input","timeout":9,"async":true}]}]}}).to_string()).unwrap();
    assert!(crate::upgrade_managed_question_hooks(&fixture.config).unwrap());
    let after = fs::read(&hook_path).unwrap();
    assert!(!crate::upgrade_managed_question_hooks(&fixture.config).unwrap());
    assert_eq!(fs::read(&hook_path).unwrap(), after);
    let root: Value = serde_json::from_slice(&after).unwrap();
    assert_eq!(root["hooks"]["Stop"], stop);
    assert_eq!(
        root["hooks"]["PreToolUse"][0]["matcher"],
        "^request_user_input$"
    );
    assert_eq!(root["hooks"]["PreToolUse"][0]["hooks"], json!([user_hook]));
    assert_eq!(
        root["hooks"]["PreToolUse"][1]["matcher"],
        agenthub_core::probe::PROBE_QUESTION_HOOK_MATCHER
    );
    assert_eq!(root["hooks"]["PreToolUse"][1]["custom"], "keep");
    assert!(root["hooks"]["PreToolUse"][1]["hooks"][0]
        .get("async")
        .is_none());
    assert_eq!(
        fs::read_to_string(config_path).unwrap(),
        "# User configuration\nmodel = 'example-model'\n"
    );
    assert_eq!(fs::read_dir(&fixture.config.codex.home).unwrap().count(), 4);
}
