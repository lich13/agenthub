use super::*;
use crate::send_bark_notification;
use agenthub_core::{
    db::NewProbeEvent,
    native_probe::{NativeProvider, NativeTurnEvent},
    platform::PlatformKind,
};
use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Method, Request, StatusCode},
    response::Response,
    Router,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const TOKEN: &str = "fixture-bark-device-key";
const BODY: &str = "这是虚构完成正文。\n中文、换行与标点应完整到达。";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct Reply {
    status: StatusCode,
    body: Vec<u8>,
    delay: Duration,
    headers: Vec<(String, String)>,
}

impl Reply {
    fn json(status: StatusCode, value: Value) -> Self {
        Self {
            status,
            body: serde_json::to_vec(&value).unwrap(),
            delay: Duration::ZERO,
            headers: Vec::new(),
        }
    }

    fn bark() -> Self {
        Self::json(StatusCode::OK, json!({"code":200}))
    }

    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Clone)]
struct CapturedRequest {
    method: Method,
    uri: String,
    body: Value,
}

#[derive(Default)]
struct RequestGate {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

struct ServerState {
    reply: Mutex<Reply>,
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<CapturedRequest>>,
    request_gate: Mutex<Option<Arc<RequestGate>>>,
}

struct LoopbackServer {
    url: String,
    state: Arc<ServerState>,
    task: tokio::task::JoinHandle<()>,
}

async fn capture_request(
    State(state): State<Arc<ServerState>>,
    request: Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 2 * 1024 * 1024).await.unwrap();
    state.requests.lock().unwrap().push(CapturedRequest {
        method: parts.method,
        uri: parts.uri.to_string(),
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    });
    let request_gate = state.request_gate.lock().unwrap().take();
    if let Some(gate) = request_gate {
        gate.entered.notify_one();
        gate.release.notified().await;
    }
    let reply = state
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or_else(|| state.reply.lock().unwrap().clone());
    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }
    let mut response = Response::builder()
        .status(reply.status)
        .header("Content-Type", "application/json");
    for (name, value) in reply.headers {
        response = response.header(name, value);
    }
    response.body(Body::from(reply.body)).unwrap()
}

impl LoopbackServer {
    async fn new(reply: Reply) -> Self {
        let state = Arc::new(ServerState {
            reply: Mutex::new(reply),
            replies: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
            request_gate: Mutex::new(None),
        });
        let router = Router::new()
            .fallback(capture_request)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, state, task }
    }

    fn reply(&self, reply: Reply) {
        *self.state.reply.lock().unwrap() = reply;
    }

    fn replies(&self, replies: Vec<Reply>) {
        *self.state.replies.lock().unwrap() = replies.into();
    }

    fn gate_next_request(&self) -> Arc<RequestGate> {
        let gate = Arc::new(RequestGate::default());
        *self.state.request_gate.lock().unwrap() = Some(gate.clone());
        gate
    }

    fn requests(&self) -> Vec<CapturedRequest> {
        self.state.requests.lock().unwrap().clone()
    }

    fn count(&self) -> usize {
        self.state.requests.lock().unwrap().len()
    }
}

impl Drop for LoopbackServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Fixture {
    root: PathBuf,
    rollout: PathBuf,
    config: Config,
    db: PanelDb,
    bark: LoopbackServer,
}

impl Fixture {
    async fn new() -> Self {
        let bark = LoopbackServer::new(Reply::bark()).await;
        let root = std::env::temp_dir().join(format!(
            "agenthub-channel-delivery-{}",
            uuid::Uuid::new_v4()
        ));
        let home = root.join("codex");
        fs::create_dir_all(home.join("sessions")).unwrap();
        let rollout = home.join("sessions/rollout.jsonl");
        let records = [
            json!({"type":"session_meta","payload":{"id":"fixture-main"}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"fixture-turn"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":BODY}]}}),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"fixture-turn","status":"completed"}}),
        ];
        fs::write(
            &rollout,
            records
                .iter()
                .map(|value| format!("{value}\n"))
                .collect::<String>(),
        )
        .unwrap();
        fs::write(home.join("session_index.jsonl"), "").unwrap();
        let connection = Connection::open(home.join("state_5.sqlite")).unwrap();
        connection.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY, name TEXT, title TEXT, source TEXT, thread_source TEXT,
                agent_path TEXT, rollout_path TEXT, updated_at INTEGER, archived_at INTEGER,
                archived INTEGER, cwd TEXT, model TEXT, has_user_event INTEGER, first_user_message TEXT
             );",
        ).unwrap();
        connection.execute(
            "INSERT INTO threads VALUES ('fixture-main','Fixture task','Fixture task','vscode','user',NULL,?1,?2,NULL,0,'/workspace/fixture','fixture',1,'fixture question')",
            params![rollout.to_str().unwrap(), PanelDb::now()],
        ).unwrap();
        drop(connection);
        let mut config = Config::for_platform_kind_with_home(PlatformKind::Linux, &root);
        config.codex.home = home;
        config.codex.workspace = root.join("workspace");
        config.probe.enabled = true;
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_codex = true;
        config.probe.notifications.notify_completion = true;
        config.probe.notifications.notify_grok = true;
        config.probe.notifications.notify_grok_completion = true;
        config.probe.notifications.notify_claude = true;
        config.probe.notifications.notify_claude_reply_needed = true;
        config.probe.notifications.server_url = bark.url.clone();
        let db = PanelDb::open_with_secret_box(
            root.join("panel.sqlite"),
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.set_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING, TOKEN.as_bytes())
            .unwrap();
        db.sync_notification_channels(&config).unwrap();
        Self {
            root,
            rollout,
            config,
            db,
            bark,
        }
    }

    fn connection(&self) -> Connection {
        Connection::open(self.db.path()).unwrap()
    }

    fn event_ms(&self) -> i64 {
        self.connection()
            .query_row(
                "SELECT activated_ms + 1 FROM probe_notification_channels WHERE channel='bark'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn request(&self) -> ProbeBarkRequest {
        ProbeBarkRequest {
            url: None,
            presentation: None,
            title: "完成 · 中文 fixture".into(),
            body: BODY.into(),
            dedupe_key: "fixture-event".into(),
        }
    }

    fn event(&self) -> ProbeBuiltEvent {
        ProbeBuiltEvent {
            kind: "completion".into(),
            event_type: "task_complete".into(),
            thread_id: Some("fixture-main".into()),
            turn_id: Some("fixture-turn".into()),
            title: "Fixture task".into(),
            message: "Fixture completion".into(),
            bark_title: "完成 · 中文 fixture".into(),
            bark_body: BODY.into(),
            dedupe_namespace: "fixture-completion".into(),
            dedupe_key: "fixture-event".into(),
            ttl_seconds: 3_600,
            source: "fixture-hook".into(),
            payload: json!({"body_source":"assistant_final","feedback_completed_at_ms":self.event_ms()}),
            suppression_reason: None,
        }
    }

    fn native_delivery(&self, provider: NativeProvider, kind: &str) -> NativeDelivery {
        NativeDelivery {
            event_key: format!("fixture-{}-{kind}", provider.as_str()),
            provider,
            session_key: "fixture-native-session".into(),
            thread_id: "fixture-native-thread".into(),
            title: "原生测试线程".into(),
            event: NativeTurnEvent {
                prompt_id: None,
                position: 3,
                turn_id: "fixture-native-turn".into(),
                kind: kind.into(),
                body: BODY.into(),
                timestamp_ms: self.event_ms(),
            },
        }
    }

    fn due_now(&self, key: &str) {
        self.connection()
            .execute(
                "UPDATE probe_notification_deliveries SET next_ms=0 WHERE event_key=?1",
                [key],
            )
            .unwrap();
    }

    fn status(&self, channel: NotificationChannel, key: &str) -> String {
        self.connection().query_row(
            "SELECT status FROM probe_notification_deliveries WHERE channel=?1 AND event_key=?2",
            params![channel.as_str(), key],
            |row| row.get(0),
        ).unwrap()
    }

    fn assert_payload_cleared(&self, channel: NotificationChannel, key: &str) {
        let cleared = self.connection().query_row(
            "SELECT ciphertext IS NULL AND nonce IS NULL FROM probe_notification_deliveries WHERE channel=?1 AND event_key=?2",
            params![channel.as_str(), key],
            |row| row.get::<_, bool>(0),
        ).unwrap();
        assert!(cleared);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn bark_posts_the_complete_chinese_body_without_exposing_the_key_in_outcomes() {
    let fixture = Fixture::new().await;
    let request = fixture.request();
    let outcome =
        send_bark_notification(&fixture.config, TOKEN.as_bytes(), &request, REQUEST_TIMEOUT)
            .await
            .unwrap();
    assert!(outcome.sent && !outcome.skipped);
    assert_eq!(outcome.http_status, Some(200));
    assert_eq!(outcome.request_count, 1);
    let requests = fixture.bark.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::POST);
    assert_eq!(requests[0].uri, "/push");
    assert_eq!(requests[0].body["device_key"], TOKEN);
    assert_eq!(requests[0].body["body"], BODY);
    assert_eq!(requests[0].body["title"], request.title);
    let serialized = serde_json::to_string(&outcome).unwrap();
    assert!(!serialized.contains(TOKEN));
    assert!(!serialized.contains(BODY));
}

#[tokio::test]
async fn bark_respects_enablement_secret_and_legacy_dedupe_before_sending() {
    for suppression in ["disabled", "missing-secret", "legacy-dedupe"] {
        let mut fixture = Fixture::new().await;
        if suppression == "disabled" {
            fixture.config.probe.notifications.enabled = false;
        }
        if suppression == "missing-secret" {
            fixture
                .db
                .remove_secret_setting(PROBE_BARK_DEVICE_KEY_SETTING)
                .unwrap();
        }
        let bark = codex(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &fixture.event(),
            suppression != "legacy-dedupe",
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(bark.skipped && !bark.sent, "{suppression}");
        assert_eq!(fixture.bark.count(), 0, "{suppression}");
    }
}

#[tokio::test]
async fn concurrent_codex_entrances_send_bark_once_and_reopening_does_not_replay() {
    let fixture = Fixture::new().await;
    let event = fixture.event();
    let load_config = || Ok(fixture.config.clone());
    let (first, second) = tokio::join!(
        codex(&load_config, &fixture.db, &event, true, REQUEST_TIMEOUT),
        codex(&load_config, &fixture.db, &event, true, REQUEST_TIMEOUT),
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert!(outcomes.iter().any(|outcome| outcome.sent));
    assert!(outcomes.iter().all(|outcome| outcome.sent
        || (!outcome.skipped && outcome.status.as_deref() == Some("retrying"))));
    assert_eq!(fixture.bark.count(), 1);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let duplicate = codex(
        &|| Ok(fixture.config.clone()),
        &reopened,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(duplicate.sent || duplicate.skipped);
    retry_pending(&fixture.config, &reopened).await.unwrap();
    assert_eq!(fixture.bark.count(), 1);
}

#[tokio::test]
async fn an_initial_lease_claimed_elsewhere_stays_retrying_at_repeated_entrances() {
    let fixture = Fixture::new().await;
    let event = fixture.event();
    let key = codex_key(&event);
    let candidate = queued_candidate(
        &fixture.config,
        &fixture.db,
        Source::Codex {
            original_body: None,
            event: Box::new(event.clone()),
            fingerprint: codex_fingerprint(&fixture.config, &event),
        },
        fixture.request(),
        fixture.event_ms(),
    )
    .unwrap();
    assert!(fixture
        .db
        .stage_notification_delivery(
            NotificationChannel::Bark,
            &key,
            candidate.event_ms,
            &serde_json::to_value(&candidate).unwrap(),
        )
        .unwrap());
    let other_entrance = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let lease = other_entrance
        .claim_notification_delivery(NotificationChannel::Bark, &key)
        .unwrap()
        .unwrap();
    assert_eq!((lease.chunk_index, lease.chunk_attempts), (0, 1));
    let stored = fixture
        .db
        .notification_delivery_outcome(NotificationChannel::Bark, &key)
        .unwrap()
        .expect("the first in-flight attempt must expose its durable status");
    let outcomes = [
        serde_json::from_value::<ProbeBarkOutcome>(stored).unwrap(),
        process(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &key,
            NotificationChannel::Bark,
            &candidate,
            REQUEST_TIMEOUT,
            false,
        )
        .await
        .unwrap(),
        codex(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &event,
            true,
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap(),
    ];
    for outcome in outcomes {
        assert_eq!(outcome.status.as_deref(), Some("retrying"));
        assert!(!outcome.sent && !outcome.skipped);
        assert_eq!(outcome.attempts, 1);
        assert_eq!(outcome.request_count, 1);
        assert_eq!(outcome.chunk_count, 1);
        assert_eq!(outcome.confirmed_chunks, 0);
        assert!(outcome.next_retry_ms.is_none());
    }
    assert_eq!(
        fixture.status(NotificationChannel::Bark, &key),
        "delivering"
    );
    assert_eq!(fixture.bark.count(), 0);
}

#[tokio::test]
async fn bark_reenable_baseline_skips_old_codex_and_native_events() {
    let mut fixture = Fixture::new().await;
    fixture.config.probe.notifications.enabled = false;
    fixture
        .db
        .sync_notification_channels(&fixture.config)
        .unwrap();
    fixture.config.probe.notifications.enabled = true;
    fixture
        .db
        .sync_notification_channels(&fixture.config)
        .unwrap();
    let mut event = fixture.event();
    event.payload["feedback_completed_at_ms"] = json!(fixture.event_ms() - 60_000);
    let bark = codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(bark.skipped && !bark.sent);
    let mut delivery = fixture.native_delivery(NativeProvider::Grok, "completion");
    delivery.event.timestamp_ms = fixture.event_ms() - 60_000;
    let bark = native(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &delivery,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(bark.skipped && !bark.sent);
    assert_eq!(fixture.bark.count(), 0);
}

#[tokio::test]
async fn bark_native_events_require_live_claude_evidence_and_do_not_replay() {
    let _env_guard = crate::api::test_support::AsyncEnvGuard::lock_read_only().await;
    for (provider, kind) in [
        (NativeProvider::Grok, "completion"),
        (NativeProvider::Claude, "reply_needed"),
    ] {
        let fixture = Fixture::new().await;
        let delivery = fixture.native_delivery(provider, kind);
        // Direct delivery only; do not scan or discover any real native sessions.
        let bark = native(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &delivery,
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        if provider == NativeProvider::Claude {
            assert!(!bark.sent && !bark.skipped);
            assert_eq!(bark.status.as_deref(), Some("waiting_retry"));
            assert_eq!(
                fixture.bark.count(),
                0,
                "unverified synthetic identities must not send"
            );
            continue;
        }
        assert!(bark.sent);
        let requests = fixture.bark.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body["body"], BODY);
        assert!(requests[0].body["title"].as_str().unwrap().contains(
            if provider == NativeProvider::Claude {
                "Claude Code"
            } else {
                "Grok"
            }
        ));
        let reopened = PanelDb::open_with_secret_box(
            fixture.db.path(),
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let duplicate = native(
            &|| Ok(fixture.config.clone()),
            &reopened,
            &delivery,
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(duplicate.sent || duplicate.skipped);
        assert_eq!(fixture.bark.count(), 1);
    }
}

#[tokio::test]
async fn invalid_bark_acknowledgement_is_retried_and_recovers_after_restart() {
    let fixture = Fixture::new().await;
    let mut invalid = Reply::bark();
    invalid.body = b"fixture-invalid-json".to_vec();
    fixture.bark.reply(invalid);
    let event = fixture.event();
    let key = codex_key(&event);
    let outcome = codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(!outcome.sent && !outcome.skipped);
    assert_eq!(outcome.reason.as_deref(), Some("response_decode"));
    assert_eq!(outcome.status.as_deref(), Some("waiting_retry"));
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "pending");
    fixture.bark.reply(Reply::bark());
    fixture.due_now(&key);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    retry_pending(&fixture.config, &reopened).await.unwrap();
    assert_eq!(fixture.bark.count(), 2);
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "sent");
    fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    let requests = fixture.bark.requests();
    assert_eq!(requests[0].body["id"], requests[1].body["id"]);
}

#[tokio::test]
async fn explicit_transient_failures_are_retried_at_most_three_times_across_restart() {
    for status in [408, 429, 500, 502, 503, 504] {
        let fixture = Fixture::new().await;
        fixture.bark.reply(Reply::json(
            StatusCode::from_u16(status).unwrap(),
            json!({"error":"fixture temporary rejection"}),
        ));
        let event = fixture.event();
        assert!(codex_fingerprint(&fixture.config, &event).is_some());
        let key = codex_key(&event);
        let first = codex(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &event,
            true,
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(!first.sent && !first.skipped);
        assert_eq!(first.http_status, Some(status));
        assert_eq!(fixture.status(NotificationChannel::Bark, &key), "pending");
        retry_pending(&fixture.config, &fixture.db).await.unwrap();
        assert_eq!(fixture.bark.count(), 1);
        for attempt in 2..=3 {
            fixture.due_now(&key);
            let reopened = PanelDb::open_with_secret_box(
                fixture.db.path(),
                agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap();
            retry_pending(&fixture.config, &reopened).await.unwrap();
            assert_eq!(fixture.bark.count(), attempt);
        }
        assert_eq!(fixture.status(NotificationChannel::Bark, &key), "failed");
        fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
        fixture.due_now(&key);
        retry_pending(&fixture.config, &fixture.db).await.unwrap();
        let duplicate = codex(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &event,
            true,
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(!duplicate.sent);
        assert_eq!(duplicate.status.as_deref(), Some("failed"));
        assert_eq!(fixture.bark.count(), 3);
        let requests = fixture.bark.requests();
        let id = requests[0].body["id"].as_str().unwrap();
        assert!(!id.is_empty());
        assert!(requests.iter().all(|request| request.body["id"] == id));
        for private in [TOKEN, BODY, "fixture-main", "fixture-event"] {
            assert!(!id.contains(private));
        }
    }
}

#[tokio::test]
async fn a_successful_bark_retry_updates_the_existing_business_event() {
    let fixture = Fixture::new().await;
    fixture.bark.reply(Reply::json(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error":"fixture rejection"}),
    ));
    let event = fixture.event();
    let key = codex_key(&event);
    let bark = codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(!bark.sent && !bark.skipped);
    fixture
        .db
        .record_probe_event(NewProbeEvent {
            kind: "completion",
            thread_id: Some("fixture-main"),
            title: Some("Fixture task"),
            message: Some("Fixture completion"),
            dedupe_key: Some("fixture-event"),
            source: "fixture-hook",
            payload: json!({"notification_event_key":key,"bark":bark}),
        })
        .unwrap();
    fixture.bark.reply(Reply::bark());
    fixture.due_now(&key);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    retry_pending(&fixture.config, &reopened).await.unwrap();
    assert_eq!(fixture.bark.count(), 2);
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "sent");
    fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    let (count, payload) = fixture
        .connection()
        .query_row(
            "SELECT count(*),payload_json FROM probe_events WHERE dedupe_key='fixture-event'",
            [],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(count, 1);
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["bark"]["sent"], true);
    assert!(payload.get("gotify").is_none());
    assert!(!payload.to_string().contains(TOKEN));
}

#[tokio::test]
async fn retry_rechecks_source_identity_fingerprint_provider_event_and_channel_switches() {
    for change in [
        "transcript",
        "rewritten",
        "replaced",
        "identity",
        "provider",
        "event",
        "channel",
        "target",
        "secret",
    ] {
        let mut fixture = Fixture::new().await;
        fixture.bark.reply(Reply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"fixture rejection"}),
        ));
        let event = fixture.event();
        let key = codex_key(&event);
        let outcome = codex(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &event,
            true,
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(!outcome.sent);
        fixture.due_now(&key);
        match change {
            "transcript" => writeln!(fs::OpenOptions::new().append(true).open(&fixture.rollout).unwrap(), "{}", json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"next-fixture-turn"}})).unwrap(),
            "rewritten" => fs::write(&fixture.rollout, "{}\n").unwrap(),
            "replaced" => {
                let bytes = fs::read(&fixture.rollout).unwrap();
                fs::rename(&fixture.rollout, fixture.rollout.with_extension("previous")).unwrap();
                fs::write(&fixture.rollout, bytes).unwrap();
            }
            "target" => fixture.config.probe.notifications.server_url = format!("{}/different-target", fixture.bark.url),
            "secret" => fixture.db.set_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING, b"fixture-replacement-device-key").unwrap(),
            "identity" => {
                Connection::open(fixture.config.codex.home.join("state_5.sqlite")).unwrap().execute("DELETE FROM threads WHERE id='fixture-main'", []).unwrap();
            }
            "provider" => fixture.config.probe.notifications.notify_codex = false,
            "event" => fixture.config.probe.notifications.notify_completion = false,
            "channel" => fixture.config.probe.notifications.enabled = false,
            _ => unreachable!(),
        }
        fixture.bark.reply(Reply::bark());
        retry_pending(&fixture.config, &fixture.db).await.unwrap();
        assert_eq!(fixture.bark.count(), 1, "{change}");
        assert_eq!(
            fixture.status(NotificationChannel::Bark, &key),
            "skipped",
            "{change}"
        );
        fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    }
}

#[tokio::test]
async fn retry_batch_reloads_configuration_after_an_earlier_request_has_started() {
    for change in ["target", "channel", "provider", "event"] {
        let fixture = Fixture::new().await;
        let replacement = LoopbackServer::new(Reply::bark()).await;
        fixture.bark.reply(Reply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"fixture rejection"}),
        ));
        let mut keys = Vec::new();
        for dedupe_key in ["fixture-first-event", "fixture-second-event"] {
            let mut event = fixture.event();
            event.dedupe_key = dedupe_key.into();
            let key = codex_key(&event);
            let outcome = codex(
                &|| Ok(fixture.config.clone()),
                &fixture.db,
                &event,
                true,
                REQUEST_TIMEOUT,
            )
            .await
            .unwrap();
            assert_eq!(outcome.status.as_deref(), Some("waiting_retry"));
            fixture.due_now(&key);
            keys.push(key);
        }
        assert_eq!(fixture.bark.count(), 2);
        fixture.bark.reply(Reply::bark());
        let gate = fixture.bark.gate_next_request();
        let live_config = Mutex::new(fixture.config.clone());
        let load_config = || Ok(live_config.lock().unwrap().clone());
        let update_config = async {
            tokio::time::timeout(REQUEST_TIMEOUT, gate.entered.notified())
                .await
                .expect("the first queued retry must reach the original endpoint");
            {
                let mut config = live_config.lock().unwrap();
                match change {
                    "target" => config.probe.notifications.server_url = replacement.url.clone(),
                    "channel" => config.probe.notifications.enabled = false,
                    "provider" => config.probe.notifications.notify_codex = false,
                    "event" => config.probe.notifications.notify_completion = false,
                    _ => unreachable!(),
                }
            }
            gate.release.notify_one();
        };
        let (result, ()) = tokio::join!(
            retry_pending_with_config(&fixture.db, &load_config),
            update_config
        );
        result.unwrap();
        assert_eq!(fixture.bark.count(), 3, "{change}");
        assert_eq!(replacement.count(), 0, "{change}");
        let mut statuses: Vec<_> = keys
            .iter()
            .map(|key| fixture.status(NotificationChannel::Bark, key))
            .collect();
        statuses.sort();
        assert_eq!(statuses, ["sent", "skipped"], "{change}");
    }
}

#[tokio::test]
async fn first_delivery_reads_live_configuration_after_claiming_before_http() {
    for change in ["target", "channel", "provider", "event"] {
        let fixture = Fixture::new().await;
        let replacement = LoopbackServer::new(Reply::bark()).await;
        let event = fixture.event();
        let key = codex_key(&event);
        let load_config = || {
            let mut config = fixture.config.clone();
            let claimed = fixture
                .db
                .notification_delivery_outcome(NotificationChannel::Bark, &key)?
                .is_some_and(|outcome| outcome["status"] == "retrying");
            if claimed {
                match change {
                    "target" => config.probe.notifications.server_url = replacement.url.clone(),
                    "channel" => config.probe.notifications.enabled = false,
                    "provider" => config.probe.notifications.notify_codex = false,
                    "event" => config.probe.notifications.notify_completion = false,
                    _ => unreachable!(),
                }
            }
            Ok(config)
        };
        let outcome = codex(&load_config, &fixture.db, &event, true, REQUEST_TIMEOUT)
            .await
            .unwrap();
        assert!(outcome.skipped && !outcome.sent, "{change}");
        assert_eq!(fixture.bark.count(), 0, "{change}");
        assert_eq!(replacement.count(), 0, "{change}");
        fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    }
}

#[tokio::test]
async fn retry_reloads_configuration_after_source_validation_before_sending() {
    for change in ["target", "channel", "provider", "event", "home"] {
        let fixture = Fixture::new().await;
        let replacement = LoopbackServer::new(Reply::bark()).await;
        let event = fixture.event();
        let key = codex_key(&event);
        let candidate = queued_candidate(
            &fixture.config,
            &fixture.db,
            Source::Codex {
                original_body: None,
                event: Box::new(event.clone()),
                fingerprint: codex_fingerprint(&fixture.config, &event),
            },
            fixture.request(),
            fixture.event_ms(),
        )
        .unwrap();
        assert!(current(&fixture.config, &fixture.db, &candidate).await);
        assert!(fixture
            .db
            .stage_notification_delivery(
                NotificationChannel::Bark,
                &key,
                candidate.event_ms,
                &serde_json::to_value(&candidate).unwrap(),
            )
            .unwrap());
        let mut changed_config = fixture.config.clone();
        match change {
            "target" => changed_config.probe.notifications.server_url = replacement.url.clone(),
            "channel" => changed_config.probe.notifications.enabled = false,
            "provider" => changed_config.probe.notifications.notify_codex = false,
            "event" => changed_config.probe.notifications.notify_completion = false,
            "home" => changed_config.codex.home = fixture.root.join("replacement-codex"),
            _ => unreachable!(),
        }
        let initial_config = Mutex::new(Some(fixture.config.clone()));
        let load_config = || {
            Ok(initial_config
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| changed_config.clone()))
        };
        let outcome = process(
            &load_config,
            &fixture.db,
            &key,
            NotificationChannel::Bark,
            &candidate,
            REQUEST_TIMEOUT,
            true,
        )
        .await
        .unwrap();
        assert!(outcome.skipped && !outcome.sent, "{change}");
        assert_eq!(outcome.status.as_deref(), Some("skipped"), "{change}");
        assert_eq!(fixture.bark.count(), 0, "{change}");
        assert_eq!(replacement.count(), 0, "{change}");
        assert_eq!(fixture.status(NotificationChannel::Bark, &key), "skipped");
        fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    }
}

#[tokio::test]
async fn timeout_retries_only_the_unconfirmed_request_after_restart() {
    let fixture = Fixture::new().await;
    let mut delayed = Reply::bark();
    delayed.delay = Duration::from_secs(1);
    fixture.bark.reply(delayed);
    let event = fixture.event();
    let key = codex_key(&event);
    let outcome = codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        Duration::from_millis(100),
    )
    .await
    .unwrap();
    assert!(!outcome.sent && !outcome.skipped);
    assert_eq!(outcome.reason.as_deref(), Some("timeout"));
    assert_eq!(outcome.status.as_deref(), Some("waiting_retry"));
    assert!(outcome.http_status.is_none());
    assert_eq!(outcome.request_count, 1);
    assert_eq!(fixture.bark.count(), 1);
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "pending");
    fixture.bark.reply(Reply::bark());
    fixture.due_now(&key);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    retry_pending(&fixture.config, &reopened).await.unwrap();
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "sent");
    fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    let requests = fixture.bark.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body["id"], requests[1].body["id"]);
    codex(
        &|| Ok(fixture.config.clone()),
        &reopened,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert_eq!(fixture.bark.count(), 2);
}

#[tokio::test]
async fn permanent_http_and_apns_rejections_never_enter_retry_queue() {
    let mut cases: Vec<Reply> = [400, 401, 403, 404, 413, 425, 501, 599]
        .into_iter()
        .map(|status| Reply::json(StatusCode::from_u16(status).unwrap(), json!({"code":200})))
        .collect();
    for reason in ["BadDeviceToken", "Unregistered", "DeviceTokenNotForTopic"] {
        cases.push(Reply::json(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"code":500,"message":format!("{reason} {TOKEN}")}),
        ));
    }
    for reply in cases {
        let fixture = Fixture::new().await;
        fixture.bark.reply(reply);
        let event = fixture.event();
        let key = codex_key(&event);
        let outcome = codex(
            &|| Ok(fixture.config.clone()),
            &fixture.db,
            &event,
            true,
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(!outcome.sent && !outcome.skipped);
        assert_eq!(outcome.status.as_deref(), Some("failed"));
        assert_eq!(fixture.status(NotificationChannel::Bark, &key), "failed");
        fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
        let rendered = serde_json::to_string(&outcome).unwrap();
        assert!(!rendered.contains(TOKEN));
        assert!(!rendered.contains(&fixture.bark.url));
        fixture.due_now(&key);
        let reopened = PanelDb::open_with_secret_box(
            fixture.db.path(),
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        retry_pending(&fixture.config, &reopened).await.unwrap();
        assert_eq!(fixture.bark.count(), 1);
    }
}

#[tokio::test]
async fn bark_requires_both_a_successful_http_status_and_code_200() {
    for (status, body, sent, retryable, uncertain) in [
        (200, json!({"code":200}), true, false, false),
        (202, json!({"code":200}), true, false, false),
        (200, json!({"code":429}), false, true, false),
        (200, json!({"code":400}), false, false, false),
        (
            200,
            json!({"message":"fixture missing acknowledgement"}),
            false,
            true,
            true,
        ),
        (400, json!({"code":200}), false, false, false),
        (503, json!({"code":200}), false, true, false),
    ] {
        let fixture = Fixture::new().await;
        fixture
            .bark
            .reply(Reply::json(StatusCode::from_u16(status).unwrap(), body));
        let outcome = send_bark_notification(
            &fixture.config,
            TOKEN.as_bytes(),
            &fixture.request(),
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert_eq!(
            (outcome.sent, outcome.retryable, outcome.uncertain),
            (sent, retryable, uncertain)
        );
        assert_eq!(outcome.http_status, Some(status));
        assert_eq!(fixture.bark.count(), 1);
    }
}

#[tokio::test]
async fn redirects_are_rejected_without_forwarding_the_device_key() {
    let fixture = Fixture::new().await;
    let destination = LoopbackServer::new(Reply::bark()).await;
    fixture.bark.reply(
        Reply::json(StatusCode::TEMPORARY_REDIRECT, json!({"code":200}))
            .header("Location", &format!("{}/push", destination.url)),
    );
    let outcome = send_bark_notification(
        &fixture.config,
        TOKEN.as_bytes(),
        &fixture.request(),
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(!outcome.sent && !outcome.retryable);
    assert_eq!(outcome.reason.as_deref(), Some("redirect_rejected"));
    assert_eq!(fixture.bark.count(), 1);
    assert_eq!(destination.count(), 0);
    assert!(!serde_json::to_string(&outcome)
        .unwrap()
        .contains(&destination.url));
}

#[tokio::test]
async fn retry_after_headers_reach_the_persistent_schedule() {
    let fixture = Fixture::new().await;
    fixture.bark.reply(
        Reply::json(StatusCode::TOO_MANY_REQUESTS, json!({"code":429}))
            .header("Retry-After", "120"),
    );
    let before = chrono::Utc::now().timestamp_millis();
    let outcome = codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &fixture.event(),
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    let after = chrono::Utc::now().timestamp_millis();
    assert_eq!(outcome.status.as_deref(), Some("waiting_retry"));
    assert!((before + 120_000..=after + 120_000).contains(&outcome.next_retry_ms.unwrap()));
    retry_pending(&fixture.config, &fixture.db).await.unwrap();
    assert_eq!(fixture.bark.count(), 1);

    let date = (chrono::Utc::now() + chrono::Duration::seconds(120)).to_rfc2822();
    fixture.bark.reply(
        Reply::json(StatusCode::SERVICE_UNAVAILABLE, json!({"code":503}))
            .header("Retry-After", &date),
    );
    let outcome = send_bark_notification(
        &fixture.config,
        TOKEN.as_bytes(),
        &fixture.request(),
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!((118_000..=120_000).contains(&outcome.retry_after_ms.unwrap()));
}

#[tokio::test]
async fn incomplete_http_response_is_classified_without_leaking_transport_details() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut fixture = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.config.probe.notifications.server_url =
        format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 512\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}").await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let outcome = send_bark_notification(
        &fixture.config,
        TOKEN.as_bytes(),
        &fixture.request(),
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    task.await.unwrap();
    assert!(!outcome.sent && outcome.retryable && outcome.uncertain);
    assert_eq!(outcome.reason.as_deref(), Some("response_read"));
    let rendered = serde_json::to_string(&outcome).unwrap();
    assert!(!rendered.contains(TOKEN));
    assert!(!rendered.contains(&fixture.config.probe.notifications.server_url));
}

#[tokio::test]
async fn refused_loopback_connection_is_retryable_and_has_only_a_safe_category() {
    let mut fixture = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    for path in ["", "certificate", "ssl", "dns", "certificate/ssl/dns"] {
        fixture.config.probe.notifications.server_url = format!("http://{address}/{path}");
        let outcome = send_bark_notification(
            &fixture.config,
            TOKEN.as_bytes(),
            &fixture.request(),
            REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(
            !outcome.sent && outcome.retryable && !outcome.uncertain,
            "{path}"
        );
        assert_eq!(outcome.reason.as_deref(), Some("connection"), "{path}");
        assert_eq!(
            outcome.error_category.as_deref(),
            Some("connection"),
            "{path}"
        );
        let rendered = serde_json::to_string(&outcome).unwrap();
        assert!(!rendered.contains(TOKEN));
        assert!(!rendered.contains("127.0.0.1"));
        assert!(!rendered.contains(&fixture.config.probe.notifications.server_url));
    }
}

#[tokio::test]
async fn invalid_acknowledgements_exhaust_to_unknown_and_do_not_replay() {
    let fixture = Fixture::new().await;
    let mut invalid = Reply::bark();
    invalid.body = b"fixture invalid acknowledgement".to_vec();
    fixture.bark.reply(invalid);
    let event = fixture.event();
    let key = codex_key(&event);
    codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    for attempt in 2..=3 {
        fixture.due_now(&key);
        retry_pending(&fixture.config, &fixture.db).await.unwrap();
        assert_eq!(fixture.bark.count(), attempt);
    }
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "unknown");
    fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    fixture.bark.reply(Reply::bark());
    fixture.due_now(&key);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    retry_pending(&fixture.config, &reopened).await.unwrap();
    codex(
        &|| Ok(fixture.config.clone()),
        &reopened,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert_eq!(fixture.bark.count(), 3);
}

#[tokio::test]
async fn segmented_delivery_resumes_after_restart_without_repeating_confirmed_chunks() {
    let fixture = Fixture::new().await;
    fixture.bark.replies(vec![
        Reply::bark(),
        Reply::json(StatusCode::SERVICE_UNAVAILABLE, json!({"code":503})),
    ]);
    let mut event = fixture.event();
    event.bark_body = format!(
        "{}\n{}\n{}",
        "甲".repeat(900),
        "乙".repeat(900),
        "丙".repeat(900)
    );
    let key = codex_key(&event);
    // Simulate an already-encrypted pre-upgrade payload, including its saved chunks.
    let mut candidate = queued_candidate(
        &fixture.config,
        &fixture.db,
        Source::Codex {
            original_body: None,
            event: Box::new(event.clone()),
            fingerprint: codex_fingerprint(&fixture.config, &event),
        },
        fixture.request(),
        fixture.event_ms(),
    )
    .unwrap();
    candidate.request.body = event.bark_body.clone();
    candidate.chunks =
        crate::bark_body_chunks(&event.bark_body, crate::PROBE_BARK_BODY_CHUNK_BYTES);
    let first = deliver(
        &|| Ok(fixture.config.clone()),
        &fixture.config,
        &fixture.db,
        &key,
        NotificationChannel::Bark,
        &candidate,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert_eq!(first.status.as_deref(), Some("waiting_retry"));
    assert!(first.chunk_count >= 3);
    assert_eq!(first.confirmed_chunks, 1);
    assert_eq!(fixture.bark.count(), 2);
    fixture.due_now(&key);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    retry_pending(&fixture.config, &reopened).await.unwrap();
    let outcome = reopened
        .notification_delivery_outcome(NotificationChannel::Bark, &key)
        .unwrap()
        .unwrap();
    assert_eq!(outcome["status"], "sent");
    assert_eq!(outcome["confirmed_chunks"], first.chunk_count);
    assert_eq!(outcome["request_count"], first.chunk_count + 1);
    let requests = fixture.bark.requests();
    assert_eq!(requests.len(), first.chunk_count + 1);
    assert_ne!(requests[0].body["id"], requests[1].body["id"]);
    assert_eq!(requests[1].body["id"], requests[2].body["id"]);
    assert_eq!(requests[1].body["body"], requests[2].body["body"]);
    let confirmed: Vec<_> = requests
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 1)
        .map(|(_, request)| request)
        .collect();
    let reconstructed: String = confirmed
        .iter()
        .enumerate()
        .map(|(index, request)| {
            let prefix = format!("第 {}/{} 段\n\n", index + 1, first.chunk_count);
            request.body["body"]
                .as_str()
                .unwrap()
                .strip_prefix(&prefix)
                .unwrap()
        })
        .collect();
    assert_eq!(reconstructed, event.bark_body);
    let ids: std::collections::HashSet<_> = confirmed
        .iter()
        .map(|request| request.body["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), first.chunk_count);
    fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
}

#[tokio::test]
async fn unrelated_append_only_records_do_not_cancel_a_valid_retry() {
    let fixture = Fixture::new().await;
    fixture.bark.reply(Reply::json(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"code":503}),
    ));
    let event = fixture.event();
    let key = codex_key(&event);
    codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    writeln!(
        fs::OpenOptions::new()
            .append(true)
            .open(&fixture.rollout)
            .unwrap(),
        "{}",
        json!({"type":"event_msg","payload":{"type":"token_count","turn_id":"fixture-turn"}})
    )
    .unwrap();
    fixture.bark.reply(Reply::bark());
    fixture.due_now(&key);
    retry_pending(&fixture.config, &fixture.db).await.unwrap();
    assert_eq!(fixture.bark.count(), 2);
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "sent");
}

#[tokio::test]
async fn repeated_entrance_keeps_the_original_encrypted_message_and_identifier() {
    let fixture = Fixture::new().await;
    fixture.bark.reply(Reply::json(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"code":503}),
    ));
    let mut event = fixture.event();
    let key = codex_key(&event);
    codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    event.bark_body = "fixture replacement body must not overwrite the queue".into();
    event.bark_title = "fixture replacement title".into();
    fixture.bark.reply(Reply::bark());
    fixture.due_now(&key);
    codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert_eq!(
        fixture.bark.count(),
        1,
        "duplicate entrances leave retries to the monitor"
    );
    retry_pending(&fixture.config, &fixture.db).await.unwrap();
    let requests = fixture.bark.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body["body"], BODY);
    assert_eq!(requests[0].body, requests[1].body);
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "sent");
}

#[tokio::test]
async fn concurrent_retry_workers_claim_one_http_attempt() {
    let fixture = Fixture::new().await;
    fixture.bark.reply(Reply::json(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"code":503}),
    ));
    let event = fixture.event();
    let key = codex_key(&event);
    codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    fixture.bark.reply(Reply::bark());
    fixture.due_now(&key);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let (first, second) = tokio::join!(
        retry_pending(&fixture.config, &fixture.db),
        retry_pending(&fixture.config, &reopened)
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(fixture.bark.count(), 2);
    assert_eq!(fixture.status(NotificationChannel::Bark, &key), "sent");
}

#[tokio::test]
async fn a_real_pending_retry_contains_no_token_or_plaintext_message_in_the_database() {
    let fixture = Fixture::new().await;
    fixture.bark.reply(Reply::json(
        StatusCode::TOO_MANY_REQUESTS,
        json!({"error":TOKEN}),
    ));
    let event = fixture.event();
    let key = codex_key(&event);
    let outcome = codex(
        &|| Ok(fixture.config.clone()),
        &fixture.db,
        &event,
        true,
        REQUEST_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(!serde_json::to_string(&outcome).unwrap().contains(TOKEN));
    assert!(!serde_json::to_string(&fixture.config)
        .unwrap()
        .contains(TOKEN));
    fixture.due_now(&key);
    let queued = fixture.db.due_notification_deliveries(100).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].payload["request"]["body"], BODY);
    assert!(!queued[0].payload.to_string().contains(TOKEN));
    for path in [
        fixture.db.path().to_path_buf(),
        fixture.db.path().with_extension("sqlite-wal"),
    ] {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("cannot read fixture database: {error}"),
        };
        for secret in [TOKEN, BODY] {
            assert!(!bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()));
        }
    }
}

#[tokio::test]
async fn retired_gotify_rpc_is_unavailable_and_cannot_enqueue() {
    let fixture = Fixture::new().await;
    let state = crate::state::AppState::new(fixture.config.clone(), fixture.db.clone());
    let error = crate::api::rpc_dispatch::rpc_dispatch(state, "probe.gotifyTest".into(), json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.kind, crate::api::ApiErrorKind::NotFound);
    assert_eq!(fixture.bark.count(), 0);
    assert_eq!(
        fixture
            .connection()
            .query_row("SELECT count(*) FROM jobs", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        fixture
            .connection()
            .query_row(
                "SELECT count(*) FROM probe_notification_deliveries",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

fn ci_event(fixture: &Fixture) -> agenthub_core::github_ci::Event {
    serde_json::from_value(json!({
        "observed_ms": Utc::now().timestamp_millis(),
        "account": {"id": 1, "login": "fixture"},
        "run": {
            "id": 42,
            "run_attempt": 1,
            "name": "CI",
            "head_branch": "main",
            "head_sha": "0123456789",
            "status": "completed",
            "conclusion": "success",
            "html_url": "https://example.invalid/actions/runs/42?attempt=1",
            "repository_id": 10,
            "repository_name": "fixture/repo",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:01:00Z"
        },
        "fixture_ms": fixture.event_ms()
    }))
    .unwrap()
}

fn store_ci(fixture: &Fixture, event: &agenthub_core::github_ci::Event) {
    fixture
        .db
        .set_setting(
            "github_ci_state",
            &json!({
                "snapshot": {"enabled": true, "account": event.account, "runs": [event.run]},
                "baseline_ms": event.observed_ms - 1000
            })
            .to_string(),
        )
        .unwrap();
}

#[tokio::test]
async fn github_ci_delivery_uses_run_link_and_deduplicates_after_restart() {
    let mut fixture = Fixture::new().await;
    fixture.config.probe.notifications.notify_github_ci = true;
    let event = ci_event(&fixture);
    store_ci(&fixture, &event);
    github_ci(&|| Ok(fixture.config.clone()), &fixture.db, event.clone())
        .await
        .unwrap();
    assert_eq!(fixture.bark.count(), 1);
    assert_eq!(fixture.bark.requests()[0].body["url"], event.run.html_url);
    let reopened = PanelDb::open_with_secret_box(
        fixture.db.path(),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    github_ci(&|| Ok(fixture.config.clone()), &reopened, event.clone())
        .await
        .unwrap();
    assert_eq!(fixture.bark.count(), 1);
    fixture.assert_payload_cleared(NotificationChannel::Bark, &event.key());
}
#[tokio::test]
async fn github_ci_retry_stops_after_disable_account_switch_or_new_attempt() {
    for change in ["disable", "account", "attempt", "reenable"] {
        let mut fixture = Fixture::new().await;
        fixture.config.probe.notifications.notify_github_ci = true;
        fixture.bark.reply(Reply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":503}),
        ));
        let event = ci_event(&fixture);
        store_ci(&fixture, &event);
        let key = event.key();
        github_ci(&|| Ok(fixture.config.clone()), &fixture.db, event.clone())
            .await
            .unwrap();
        assert_eq!(fixture.bark.count(), 1);
        fixture.due_now(&key);
        let mut next = event.clone();
        match change {
            "disable" => fixture.config.probe.notifications.notify_github_ci = false,
            "account" => next.account.id = 2,
            "attempt" => next.run.run_attempt = 2,
            "reenable" => next.observed_ms += 5000,
            _ => unreachable!(),
        }
        store_ci(&fixture, &next);
        retry_pending_with_config(&fixture.db, &|| Ok(fixture.config.clone()))
            .await
            .unwrap();
        assert_eq!(fixture.bark.count(), 1, "{change}");
        fixture.assert_payload_cleared(NotificationChannel::Bark, &key);
    }
}
#[test]
fn older_bark_payload_without_run_url_still_deserializes() {
    let request: ProbeBarkRequest =
        serde_json::from_value(json!({"title":"fixture","body":"fixture","dedupe_key":"fixture"}))
            .unwrap();
    assert!(request.url.is_none());
}

#[tokio::test]
async fn github_ci_batch_acknowledges_success_before_an_enqueue_failure() {
    let mut fixture = Fixture::new().await;
    fixture.config.probe.notifications.notify_github_ci = true;
    let first = ci_event(&fixture);
    let mut second = first.clone();
    second.run.id += 1;
    let events = vec![first.clone(), second.clone()];
    fixture
        .db
        .set_setting(
            "github_ci_state",
            &json!({
                "snapshot": {"enabled": true, "account": first.account, "runs": [first.run, second.run]},
                "baseline_ms": first.observed_ms - 1000,
                "pending": events
            })
            .to_string(),
        )
        .unwrap();
    fixture
        .connection()
        .execute_batch(&format!(
            "CREATE TRIGGER fail_ci_enqueue BEFORE INSERT ON probe_notification_deliveries
             WHEN NEW.event_key='{}'
             BEGIN SELECT RAISE(FAIL, 'fixture enqueue failure'); END;",
            second.key()
        ))
        .unwrap();
    assert!(
        crate::github_ci_monitor::deliver_events(&fixture.db, events, &|| Ok(fixture
            .config
            .clone()))
        .await
        .is_err()
    );
    let raw = fixture.db.get_setting("github_ci_state").unwrap().unwrap();
    let state: Value = serde_json::from_str(&raw).unwrap();
    let pending: Vec<agenthub_core::github_ci::Event> =
        serde_json::from_value(state["pending"].clone()).unwrap();
    assert_eq!(pending, vec![second]);
    assert_eq!(fixture.bark.count(), 1);
    fixture
        .connection()
        .execute_batch("DROP TRIGGER fail_ci_enqueue")
        .unwrap();
    crate::github_ci_monitor::deliver_events(&fixture.db, pending, &|| Ok(fixture.config.clone()))
        .await
        .unwrap();
    assert_eq!(fixture.bark.count(), 2);
    let raw = fixture.db.get_setting("github_ci_state").unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap()["pending"],
        json!([])
    );
}
