use agenthub_core::{
    config::Config,
    db::{NewProbeErrorIncident, NewProbeEvent, NotificationChannel, NotificationLease, PanelDb},
    native_probe::NativeProvider,
    platform::PlatformKind,
    services::settings::PROBE_BARK_DEVICE_KEY_SETTING,
};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};

const FIXTURE_TOKEN: &str = "fixture-bark-device-key";
const FIXTURE_BODY: &str = "独立通知队列的虚构正文，不得明文保存。";

struct Fixture {
    root: PathBuf,
    path: PathBuf,
    config: Config,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-notification-channels-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("codex/sessions")).unwrap();
        let path = root.join("panel.sqlite");
        let mut config = Config::for_platform_kind_with_home(PlatformKind::Linux, &root);
        config.codex.home = root.join("codex");
        config.codex.workspace = root.join("workspace");
        config.probe.enabled = true;
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = "https://bark.example.invalid".into();
        Self { root, path, config }
    }

    fn open(&self) -> PanelDb {
        PanelDb::open(&self.path).unwrap()
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }

    fn activated_ms(&self, channel: NotificationChannel) -> i64 {
        self.connection()
            .query_row(
                "SELECT activated_ms FROM probe_notification_channels WHERE channel=?1",
                [channel.as_str()],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn due_now(&self, channel: NotificationChannel, key: &str) {
        self.connection()
            .execute(
                "UPDATE probe_notification_deliveries SET next_ms=0 WHERE channel=?1 AND event_key=?2",
                params![channel.as_str(), key],
            )
            .unwrap();
    }

    fn row(&self, channel: NotificationChannel, key: &str) -> DeliveryRow {
        self.connection()
            .query_row(
                "SELECT status,attempts,ciphertext IS NOT NULL,nonce IS NOT NULL,next_ms,
                 chunk_index,chunk_attempts,first_attempt_ms,lease_until_ms
                 FROM probe_notification_deliveries WHERE channel=?1 AND event_key=?2",
                params![channel.as_str(), key],
                |row| {
                    Ok(DeliveryRow {
                        status: row.get(0)?,
                        attempts: row.get(1)?,
                        has_ciphertext: row.get(2)?,
                        has_nonce: row.get(3)?,
                        next_ms: row.get(4)?,
                        chunk_index: row.get(5)?,
                        chunk_attempts: row.get(6)?,
                        first_attempt_ms: row.get(7)?,
                        lease_until_ms: row.get(8)?,
                    })
                },
            )
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct DeliveryRow {
    status: String,
    attempts: u32,
    has_ciphertext: bool,
    has_nonce: bool,
    next_ms: i64,
    chunk_index: usize,
    chunk_attempts: usize,
    first_attempt_ms: Option<i64>,
    lease_until_ms: i64,
}

fn payload() -> Value {
    json!({"request":{"title":"通知 fixture","body":FIXTURE_BODY,"dedupe_key":"fixture-event"}})
}

fn http_failure(status: u16) -> Value {
    json!({"sent":false,"skipped":false,"http_status":status,"request_count":1,"reason":"http_status","retryable":true,"uncertain":false})
}

fn claim(db: &PanelDb, key: &str) -> NotificationLease {
    db.claim_notification_delivery(NotificationChannel::Bark, key)
        .unwrap()
        .expect("fixture delivery must be eligible")
}

fn finish(db: &PanelDb, lease: &NotificationLease, outcome: &Value) -> Value {
    db.finish_notification_delivery(lease, outcome)
        .unwrap()
        .expect("fixture lease must still own the delivery")
}

fn accepted() -> Value {
    json!({"sent":true,"skipped":false,"http_status":200,"retryable":false,"uncertain":false})
}

fn expire_lease(fixture: &Fixture, key: &str) {
    fixture
        .connection()
        .execute(
            "UPDATE probe_notification_deliveries SET lease_until_ms=0 WHERE event_key=?1",
            [key],
        )
        .unwrap();
}

fn stage(db: &PanelDb, fixture: &Fixture, channel: NotificationChannel, key: &str) {
    assert!(db
        .stage_notification_delivery(
            channel,
            key,
            fixture.activated_ms(NotificationChannel::Bark) + 1,
            &payload(),
        )
        .unwrap());
}

fn assert_no_bytes(path: &Path, text: &str) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("cannot read fixture database: {error}"),
    };
    assert!(!bytes
        .windows(text.len())
        .any(|window| window == text.as_bytes()));
}

#[test]
fn bark_enablement_controls_staging_and_preserves_the_initial_baseline() {
    for enabled in [false, true] {
        let mut fixture = Fixture::new();
        fixture.config.probe.notifications.enabled = enabled;
        let db = fixture.open();
        db.sync_notification_channels(&fixture.config).unwrap();
        assert_eq!(NotificationChannel::ALL, [NotificationChannel::Bark]);
        assert_eq!(NotificationChannel::Bark.enabled(&fixture.config), enabled);
        assert_eq!(fixture.activated_ms(NotificationChannel::Bark), 0);
        assert_eq!(
            db.stage_notification_delivery(
                NotificationChannel::Bark,
                "fixture-event",
                1,
                &payload(),
            )
            .unwrap(),
            enabled,
        );
        assert_eq!(
            db.claim_notification_delivery(NotificationChannel::Bark, "fixture-event")
                .unwrap()
                .is_some(),
            enabled,
        );
    }
}

#[test]
fn disabling_and_reenabling_bark_discards_pending_payloads_and_baselines_history() {
    let mut fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    stage(&db, &fixture, NotificationChannel::Bark, "old-event");
    fixture.config.probe.notifications.enabled = false;
    db.sync_notification_channels(&fixture.config).unwrap();
    let disabled = fixture.row(NotificationChannel::Bark, "old-event");
    assert_eq!(disabled.status, "skipped");
    assert!(!disabled.has_ciphertext && !disabled.has_nonce);
    assert!(db.due_notification_deliveries(100).unwrap().is_empty());
    fixture.config.probe.notifications.enabled = true;
    db.sync_notification_channels(&fixture.config).unwrap();
    let baseline = fixture.activated_ms(NotificationChannel::Bark);
    assert!(baseline > 0);
    assert!(!db
        .stage_notification_delivery(
            NotificationChannel::Bark,
            "disabled-period-event",
            baseline - 1,
            &payload(),
        )
        .unwrap());
    assert!(db
        .claim_notification_delivery(NotificationChannel::Bark, "old-event")
        .unwrap()
        .is_none());
    assert!(db
        .stage_notification_delivery(
            NotificationChannel::Bark,
            "after-reenable",
            baseline,
            &payload(),
        )
        .unwrap());
    db.sync_notification_channels(&fixture.config).unwrap();
    assert_eq!(fixture.activated_ms(NotificationChannel::Bark), baseline);
}

#[test]
fn concurrent_database_connections_claim_one_bark_delivery() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    for channel in NotificationChannel::ALL {
        stage(&db, &fixture, channel, "concurrent-event");
    }
    let barrier = Arc::new(Barrier::new(8));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let path = fixture.path.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let db = PanelDb::open(path).unwrap();
            let channel = NotificationChannel::Bark;
            barrier.wait();
            (
                channel,
                db.claim_notification_delivery(channel, "concurrent-event")
                    .unwrap(),
            )
        }));
    }
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    for channel in NotificationChannel::ALL {
        assert_eq!(
            outcomes
                .iter()
                .filter(|(actual, won)| *actual == channel && won.is_some())
                .count(),
            1
        );
        assert_eq!(fixture.row(channel, "concurrent-event").attempts, 1);
    }
}

#[test]
fn completed_bark_claims_survive_reopen_without_replay() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    for channel in NotificationChannel::ALL {
        stage(&db, &fixture, channel, "restart-event");
    }
    let lease = claim(&db, "restart-event");
    finish(&db, &lease, &accepted());
    drop(db);

    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    assert!(db
        .claim_notification_delivery(NotificationChannel::Bark, "restart-event")
        .unwrap()
        .is_none());
    for channel in NotificationChannel::ALL {
        assert!(!db
            .stage_notification_delivery(
                channel,
                "restart-event",
                Utc::now().timestamp_millis(),
                &payload()
            )
            .unwrap());
        assert!(db
            .claim_notification_delivery(channel, "restart-event")
            .unwrap()
            .is_none());
        let row = fixture.row(channel, "restart-event");
        assert_eq!(row.status, "sent");
        assert_eq!(row.attempts, 1);
        assert!(!row.has_ciphertext && !row.has_nonce);
    }
    assert!(db.due_notification_deliveries(100).unwrap().is_empty());
}

#[test]
fn transient_failures_use_fifteen_then_sixty_seconds_and_stop_after_three_attempts() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    for status in [408, 429, 500, 502, 503, 504] {
        let key = format!("transient-{status}");
        stage(&db, &fixture, NotificationChannel::Bark, &key);
        for attempt in 1..=3 {
            let lease = claim(&db, &key);
            assert_eq!(lease.chunk_attempts, attempt);
            let before_finish = Utc::now().timestamp_millis();
            let outcome = finish(&db, &lease, &http_failure(status));
            let after_finish = Utc::now().timestamp_millis();
            let row = fixture.row(NotificationChannel::Bark, &key);
            assert_eq!(row.attempts as usize, attempt);
            assert_eq!(outcome["attempts"], attempt);
            assert_eq!(outcome["request_count"], attempt);
            assert_eq!(outcome["confirmed_chunks"], 0);
            assert!(db
                .claim_notification_delivery(NotificationChannel::Bark, &key)
                .unwrap()
                .is_none());
            assert!(db.due_notification_deliveries(100).unwrap().is_empty());
            if attempt < 3 {
                let base = if attempt == 1 { 15_000 } else { 60_000 };
                assert_eq!(row.status, "pending");
                assert_eq!(outcome["status"], "waiting_retry");
                assert_eq!(outcome["next_retry_ms"], row.next_ms);
                assert!(row.has_ciphertext && row.has_nonce);
                assert!(row.next_ms >= before_finish + base * 80 / 100);
                assert!(row.next_ms <= after_finish + base * 120 / 100);
                fixture.due_now(NotificationChannel::Bark, &key);
                let due = db.due_notification_deliveries(100).unwrap();
                assert_eq!(due.len(), 1);
                assert_eq!(due[0].event_key, key);
                assert_eq!(due[0].payload, payload());
            } else {
                assert_eq!(row.status, "failed");
                assert_eq!(outcome["status"], "failed");
                assert!(outcome["next_retry_ms"].is_null());
                assert!(!row.has_ciphertext && !row.has_nonce);
            }
            for private_field in ["retryable", "uncertain", "retry_after_ms"] {
                assert!(outcome.get(private_field).is_none());
            }
        }
    }
}

#[test]
fn permanent_failures_explicit_skips_and_unretryable_unknowns_stay_terminal() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let cases = [
        (
            "redirect",
            json!({"sent":false,"http_status":307,"retryable":false}),
            "failed",
        ),
        (
            "bad-request",
            json!({"sent":false,"http_status":400,"retryable":false}),
            "failed",
        ),
        (
            "unauthorized",
            json!({"sent":false,"http_status":401,"retryable":false}),
            "failed",
        ),
        (
            "explicit-skip",
            json!({"sent":false,"skipped":true,"retryable":true}),
            "skipped",
        ),
        (
            "unretryable-unknown",
            json!({"sent":false,"uncertain":true,"retryable":false}),
            "unknown",
        ),
    ];
    for (key, outcome, expected) in cases {
        stage(&db, &fixture, NotificationChannel::Bark, key);
        let lease = claim(&db, key);
        let result = finish(&db, &lease, &outcome);
        fixture.due_now(NotificationChannel::Bark, key);
        let row = fixture.row(NotificationChannel::Bark, key);
        assert_eq!(row.status, expected, "{key}");
        assert_eq!(result["status"], expected, "{key}");
        assert_eq!(row.attempts, 1);
        assert!(!row.has_ciphertext && !row.has_nonce);
        assert!(db
            .claim_notification_delivery(NotificationChannel::Bark, key)
            .unwrap()
            .is_none());
    }
    drop(db);
    assert!(fixture
        .open()
        .due_notification_deliveries(100)
        .unwrap()
        .is_empty());
}

#[test]
fn uncertain_attempts_retry_within_budget_and_end_unknown() {
    for reason in ["timeout", "network", "response_invalid"] {
        let fixture = Fixture::new();
        let db = fixture.open();
        db.sync_notification_channels(&fixture.config).unwrap();
        stage(&db, &fixture, NotificationChannel::Bark, "uncertain-event");
        for attempt in 1..=3 {
            let lease = claim(&db, "uncertain-event");
            let outcome = finish(
                &db,
                &lease,
                &json!({
                    "sent":false,"skipped":false,"retryable":true,"uncertain":true,"reason":reason
                }),
            );
            assert_eq!(
                outcome["status"],
                if attempt < 3 {
                    "waiting_retry"
                } else {
                    "unknown"
                }
            );
            fixture.due_now(NotificationChannel::Bark, "uncertain-event");
        }
        let row = fixture.row(NotificationChannel::Bark, "uncertain-event");
        assert_eq!(row.attempts, 3);
        assert!(!row.has_ciphertext && !row.has_nonce);
        drop(db);
        assert!(fixture
            .open()
            .due_notification_deliveries(100)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn uncertainty_survives_later_rejections_and_resets_only_after_chunk_acceptance() {
    for accept_retry in [false, true] {
        let fixture = Fixture::new();
        let db = fixture.open();
        db.sync_notification_channels(&fixture.config).unwrap();
        let key = "uncertainty-history";
        let mut queued = payload();
        queued["chunks"] = if accept_retry {
            json!(["first", "second"])
        } else {
            json!(["first"])
        };
        assert!(db
            .stage_notification_delivery(
                NotificationChannel::Bark,
                key,
                fixture.activated_ms(NotificationChannel::Bark) + 1,
                &queued
            )
            .unwrap());
        let first = claim(&db, key);
        finish(
            &db,
            &first,
            &json!({"sent":false,"retryable":true,"uncertain":true,"reason":"timeout"}),
        );
        fixture.due_now(NotificationChannel::Bark, key);
        let second = claim(&db, key);
        assert!(second.uncertain);
        if accept_retry {
            finish(&db, &second, &accepted());
            let next_chunk = claim(&db, key);
            assert_eq!(next_chunk.chunk_index, 1);
            assert!(!next_chunk.uncertain);
            let result = finish(
                &db,
                &next_chunk,
                &json!({"sent":false,"retryable":false,"uncertain":false,"http_status":401}),
            );
            assert_eq!(result["status"], "failed");
            assert_eq!(result["confirmed_chunks"], 1);
        } else {
            finish(&db, &second, &http_failure(503));
            fixture.due_now(NotificationChannel::Bark, key);
            let third = claim(&db, key);
            assert!(third.uncertain);
            assert_eq!(finish(&db, &third, &http_failure(503))["status"], "unknown");
        }
    }
}

#[test]
fn retry_after_is_a_floor_and_cannot_extend_the_ten_minute_deadline() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    for (key, retry_after_ms, expected) in [
        ("server-delay", 120_000, "waiting_retry"),
        ("server-delay-exceeds-deadline", 600_001, "failed"),
    ] {
        stage(&db, &fixture, NotificationChannel::Bark, key);
        let lease = claim(&db, key);
        let before = Utc::now().timestamp_millis();
        let mut rejection = http_failure(429);
        rejection["retry_after_ms"] = json!(retry_after_ms);
        let outcome = finish(&db, &lease, &rejection);
        let after = Utc::now().timestamp_millis();
        assert_eq!(outcome["status"], expected);
        if expected == "waiting_retry" {
            let next = outcome["next_retry_ms"].as_i64().unwrap();
            assert!((before + retry_after_ms..=after + retry_after_ms).contains(&next));
            assert!(next < lease.deadline_ms);
        } else {
            assert!(outcome["next_retry_ms"].is_null());
            let row = fixture.row(NotificationChannel::Bark, key);
            assert!(!row.has_ciphertext && !row.has_nonce);
        }
    }
}

#[test]
fn lifetime_starts_at_first_claim_and_expired_pending_work_is_never_sent() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "lifetime-event";
    stage(&db, &fixture, NotificationChannel::Bark, key);
    fixture
        .connection()
        .execute(
            "UPDATE probe_notification_deliveries SET created_ms=?1 WHERE event_key=?2",
            params![Utc::now().timestamp_millis() - 3_600_000, key],
        )
        .unwrap();
    assert!(fixture
        .row(NotificationChannel::Bark, key)
        .first_attempt_ms
        .is_none());
    let before = Utc::now().timestamp_millis();
    let lease = claim(&db, key);
    let after = Utc::now().timestamp_millis();
    let first = fixture
        .row(NotificationChannel::Bark, key)
        .first_attempt_ms
        .unwrap();
    assert!((before..=after).contains(&first));
    assert_eq!(lease.deadline_ms, first + 600_000);
    finish(&db, &lease, &http_failure(503));
    fixture.connection().execute(
        "UPDATE probe_notification_deliveries SET first_attempt_ms=?1,next_ms=0 WHERE event_key=?2",
        params![Utc::now().timestamp_millis() - 600_000, key],
    ).unwrap();
    assert!(db
        .claim_notification_delivery(NotificationChannel::Bark, key)
        .unwrap()
        .is_none());
    assert!(db.due_notification_deliveries(100).unwrap().is_empty());
    let row = fixture.row(NotificationChannel::Bark, key);
    assert_eq!(row.status, "failed");
    assert_eq!(row.attempts, 1);
    assert!(!row.has_ciphertext && !row.has_nonce);
    assert_eq!(
        db.notification_delivery_outcome(NotificationChannel::Bark, key)
            .unwrap()
            .unwrap()["reason"],
        "retry_deadline_exceeded"
    );
}

#[test]
fn retry_is_not_scheduled_when_its_backoff_would_cross_the_deadline() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "nearly-expired-event";
    stage(&db, &fixture, NotificationChannel::Bark, key);
    fixture
        .connection()
        .execute(
            "UPDATE probe_notification_deliveries SET first_attempt_ms=?1 WHERE event_key=?2",
            params![Utc::now().timestamp_millis() - 590_000, key],
        )
        .unwrap();
    let lease = claim(&db, key);
    let outcome = finish(&db, &lease, &http_failure(503));
    assert_eq!(outcome["status"], "failed");
    assert!(outcome["next_retry_ms"].is_null());
    assert!(db.due_notification_deliveries(100).unwrap().is_empty());
}

#[test]
fn expired_lease_is_recovered_after_restart_and_late_results_are_fenced() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "interrupted-event";
    stage(&db, &fixture, NotificationChannel::Bark, key);
    let before_claim = Utc::now().timestamp_millis();
    let old_lease = claim(&db, key);
    let after_claim = Utc::now().timestamp_millis();
    let row = fixture.row(NotificationChannel::Bark, key);
    assert!((before_claim + 30_000..=after_claim + 30_000).contains(&row.lease_until_ms));
    let mut foreign = old_lease.clone();
    foreign.owner = "fixture-other-owner".into();
    assert!(db
        .finish_notification_delivery(&foreign, &accepted())
        .unwrap()
        .is_none());
    assert_eq!(
        fixture.row(NotificationChannel::Bark, key).status,
        "delivering"
    );
    expire_lease(&fixture, key);
    assert!(db
        .finish_notification_delivery(&old_lease, &accepted())
        .unwrap()
        .is_none());
    drop(db);

    let db = fixture.open();
    let before_recovery = Utc::now().timestamp_millis();
    assert!(db.due_notification_deliveries(100).unwrap().is_empty());
    let after_recovery = Utc::now().timestamp_millis();
    let row = fixture.row(NotificationChannel::Bark, key);
    assert_eq!(row.status, "pending");
    assert_eq!(row.attempts, 1);
    assert!(row.has_ciphertext && row.has_nonce);
    assert!((before_recovery + 12_000..=after_recovery + 18_000).contains(&row.next_ms));
    assert!(db
        .claim_notification_delivery(NotificationChannel::Bark, key)
        .unwrap()
        .is_none());
    fixture.due_now(NotificationChannel::Bark, key);
    let new_lease = claim(&db, key);
    assert_ne!(new_lease.owner, old_lease.owner);
    assert_eq!(new_lease.chunk_index, old_lease.chunk_index);
    assert_eq!(new_lease.chunk_attempts, 2);
    assert_eq!(new_lease.deadline_ms, old_lease.deadline_ms);
    assert!(db
        .finish_notification_delivery(&old_lease, &accepted())
        .unwrap()
        .is_none());
    assert_eq!(
        fixture.row(NotificationChannel::Bark, key).status,
        "delivering"
    );
    assert_eq!(finish(&db, &new_lease, &accepted())["status"], "sent");
    assert!(db
        .finish_notification_delivery(&new_lease, &accepted())
        .unwrap()
        .is_none());
    assert!(db.due_notification_deliveries(100).unwrap().is_empty());
}

#[test]
fn only_an_active_owned_delivery_lease_has_remaining_time() {
    for invalidation in [
        "foreign-owner",
        "expired-lease",
        "expired-deadline",
        "completed",
        "disabled",
    ] {
        let mut fixture = Fixture::new();
        let db = fixture.open();
        db.sync_notification_channels(&fixture.config).unwrap();
        let key = "fixture-lease-remaining";
        stage(&db, &fixture, NotificationChannel::Bark, key);
        let mut lease = claim(&db, key);
        let remaining = db
            .notification_delivery_lease_remaining_ms(&lease)
            .unwrap()
            .expect("a newly claimed lease must still be usable");
        assert!((1..=30_000).contains(&remaining));
        match invalidation {
            "foreign-owner" => lease.owner = "fixture-other-owner".into(),
            "expired-lease" => expire_lease(&fixture, key),
            "expired-deadline" => {
                fixture.connection().execute(
                    "UPDATE probe_notification_deliveries SET first_attempt_ms=?1 WHERE event_key=?2",
                    params![Utc::now().timestamp_millis() - 600_000, key],
                ).unwrap();
            }
            "completed" => {
                assert_eq!(finish(&db, &lease, &accepted())["status"], "sent");
            }
            "disabled" => {
                fixture.config.probe.notifications.enabled = false;
                db.sync_notification_channels(&fixture.config).unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            db.notification_delivery_lease_remaining_ms(&lease).unwrap(),
            None,
            "{invalidation}"
        );
    }
}

#[test]
fn repeated_lease_expiry_consumes_attempts_and_keeps_the_final_result_unknown() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "repeated-interruption";
    stage(&db, &fixture, NotificationChannel::Bark, key);
    for attempt in 1..=3 {
        let lease = claim(&db, key);
        assert_eq!(lease.chunk_attempts, attempt);
        expire_lease(&fixture, key);
        assert!(db.due_notification_deliveries(100).unwrap().is_empty());
        let row = fixture.row(NotificationChannel::Bark, key);
        assert_eq!(row.status, if attempt < 3 { "pending" } else { "unknown" });
        assert_eq!(row.attempts as usize, attempt);
        fixture.due_now(NotificationChannel::Bark, key);
    }
    let row = fixture.row(NotificationChannel::Bark, key);
    assert!(!row.has_ciphertext && !row.has_nonce);
    assert!(db
        .claim_notification_delivery(NotificationChannel::Bark, key)
        .unwrap()
        .is_none());
}

#[test]
fn disabling_bark_fences_an_inflight_lease_and_clears_its_pending_body() {
    let mut fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "disabled-inflight";
    stage(&db, &fixture, NotificationChannel::Bark, key);
    let lease = claim(&db, key);
    fixture.config.probe.notifications.enabled = false;
    db.sync_notification_channels(&fixture.config).unwrap();
    let row = fixture.row(NotificationChannel::Bark, key);
    assert_eq!(row.status, "skipped");
    assert!(!row.has_ciphertext && !row.has_nonce);
    assert!(db
        .finish_notification_delivery(&lease, &accepted())
        .unwrap()
        .is_none());
    fixture.config.probe.notifications.enabled = true;
    db.sync_notification_channels(&fixture.config).unwrap();
    assert!(db
        .claim_notification_delivery(NotificationChannel::Bark, key)
        .unwrap()
        .is_none());
}

#[test]
fn interrupted_chunk_recovery_preserves_the_visible_delivery_progress() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "interrupted-chunk-progress";
    let mut queued = payload();
    queued["chunks"] = json!(["fixture first", "fixture second", "fixture third"]);
    assert!(db
        .stage_notification_delivery(
            NotificationChannel::Bark,
            key,
            fixture.activated_ms(NotificationChannel::Bark) + 1,
            &queued,
        )
        .unwrap());
    let first = claim(&db, key);
    let first_outcome = finish(&db, &first, &accepted());
    db.record_probe_event(NewProbeEvent {
        kind: "completion",
        thread_id: Some("fixture-progress-thread"),
        title: Some("Fixture progress"),
        message: Some("Fixture summary"),
        dedupe_key: Some(key),
        source: "fixture-source",
        payload: json!({"notification_event_key":key,"bark":first_outcome}),
    })
    .unwrap();
    let interrupted = claim(&db, key);
    assert_eq!(interrupted.chunk_index, 1);
    expire_lease(&fixture, key);
    drop(db);

    let db = fixture.open();
    assert!(db.due_notification_deliveries(100).unwrap().is_empty());
    let outcome = db
        .notification_delivery_outcome(NotificationChannel::Bark, key)
        .unwrap()
        .unwrap();
    assert_eq!(outcome["status"], "waiting_retry");
    assert_eq!(outcome["chunk_count"], 3);
    assert_eq!(outcome["confirmed_chunks"], 1);
    assert_eq!(outcome["attempts"], 1);
    assert_eq!(outcome["request_count"], 2);
    assert!(outcome["next_retry_ms"].is_i64());
    let event: String = fixture
        .connection()
        .query_row(
            "SELECT payload_json FROM probe_events WHERE dedupe_key=?1",
            [key],
            |row| row.get(0),
        )
        .unwrap();
    let event: Value = serde_json::from_str(&event).unwrap();
    assert_eq!(event["bark"], outcome);
    fixture.due_now(NotificationChannel::Bark, key);
    let resumed = claim(&db, key);
    assert_eq!(resumed.chunk_index, 1);
    assert_eq!(resumed.chunk_attempts, 2);
    assert_eq!(resumed.attempts, 3);
}

#[test]
fn expiration_and_channel_disable_clear_native_pending_counts() {
    for provider in [NativeProvider::Grok, NativeProvider::Claude] {
        for (action, expected) in [
            ("deadline", "failed"),
            ("uncertain-deadline", "unknown"),
            ("disabled", "skipped"),
            ("disabled-inflight", "skipped"),
        ] {
            let mut fixture = Fixture::new();
            let db = fixture.open();
            db.sync_notification_channels(&fixture.config).unwrap();
            let native_key = "fixture-native-terminal";
            let key = format!("native:{native_key}");
            fixture.connection().execute(
                "INSERT INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at)
                 VALUES(?1,?2,'{}','queued',1,1)",
                params![native_key, provider.as_str()],
            ).unwrap();
            stage(&db, &fixture, NotificationChannel::Bark, &key);
            let lease = claim(&db, &key);
            if action != "disabled-inflight" {
                let mut rejection = http_failure(503);
                rejection["uncertain"] = json!(action == "uncertain-deadline");
                finish(&db, &lease, &rejection);
            }
            let before = db.native_notification_status().unwrap();
            let before = before
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["provider"] == provider.as_str())
                .unwrap();
            assert_eq!(before["pending_deliveries"], 1);
            if action.starts_with("disabled") {
                fixture.config.probe.notifications.enabled = false;
                db.sync_notification_channels(&fixture.config).unwrap();
            } else {
                fixture.connection().execute(
                    "UPDATE probe_notification_deliveries SET first_attempt_ms=?1,next_ms=0 WHERE event_key=?2",
                    params![Utc::now().timestamp_millis() - 600_000, key],
                ).unwrap();
                assert!(db.due_notification_deliveries(100).unwrap().is_empty());
            }
            assert_eq!(
                fixture.row(NotificationChannel::Bark, &key).status,
                expected
            );
            let native_status: String = fixture
                .connection()
                .query_row(
                    "SELECT status FROM native_probe_deliveries WHERE event_key=?1",
                    [native_key],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(native_status, expected, "{action}");
            let after = db.native_notification_status().unwrap();
            let after = after
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["provider"] == provider.as_str())
                .unwrap();
            assert_eq!(after["pending_deliveries"], 0, "{action}");
            assert_eq!(
                after["failed_deliveries"],
                usize::from(expected == "failed")
            );
        }
    }
}

#[test]
fn a_successful_retry_updates_the_linked_error_incident_and_existing_event() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "fixture-error-delivery";
    stage(&db, &fixture, NotificationChannel::Bark, key);
    let first = claim(&db, key);
    let rejected = finish(&db, &first, &http_failure(503));
    assert_eq!(rejected["status"], "waiting_retry");
    db.record_probe_event(NewProbeEvent {
        kind: "error",
        thread_id: Some("fixture-error-thread"),
        title: Some("Fixture failure"),
        message: Some("Fixture error summary"),
        dedupe_key: Some(key),
        source: "fixture-error-monitor",
        payload: json!({"notification_event_key":key,"bark":rejected}),
    })
    .unwrap();
    let event_id = db.probe_event_id_by_dedupe_key(key).unwrap().unwrap();
    let incident_key = "fixture-retry-incident";
    assert!(db
        .claim_probe_error_incident(&NewProbeErrorIncident {
            incident_key: incident_key.into(),
            source_ts: 1,
            source_ts_nanos: 0,
            source_row_id: 1,
            thread_id: "fixture-error-thread".into(),
            turn_id: "fixture-error-turn".into(),
            classification: "capacity".into(),
            error_sha256: "0".repeat(64),
            error_summary: "Fixture error summary".into(),
        })
        .unwrap());
    db.update_probe_error_incident_delivery(incident_key, Some(&event_id), "waiting_retry")
        .unwrap();
    fixture.due_now(NotificationChannel::Bark, key);
    drop(db);

    let db = fixture.open();
    let retry = claim(&db, key);
    assert_eq!(
        db.get_probe_error_incident(incident_key)
            .unwrap()
            .unwrap()
            .bark_status,
        "retrying"
    );
    let outcome = finish(&db, &retry, &accepted());
    assert_eq!(outcome["status"], "sent");
    let incident = db.get_probe_error_incident(incident_key).unwrap().unwrap();
    assert_eq!(incident.bark_status, "sent");
    assert_eq!(incident.event_id.as_deref(), Some(event_id.as_str()));
    let (count, stored): (u32, String) = fixture
        .connection()
        .query_row(
            "SELECT count(*),payload_json FROM probe_events WHERE id=?1",
            [event_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 1);
    let stored: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["bark"], outcome);
}

#[test]
fn chunks_resume_at_the_first_unconfirmed_part_with_independent_attempt_budgets() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let key = "chunked-event";
    let mut queued = payload();
    queued["chunks"] = json!(["fixture first", "fixture second", "fixture third"]);
    assert!(db
        .stage_notification_delivery(
            NotificationChannel::Bark,
            key,
            fixture.activated_ms(NotificationChannel::Bark) + 1,
            &queued
        )
        .unwrap());
    let first = claim(&db, key);
    assert_eq!(
        (first.chunk_index, first.chunk_count, first.chunk_attempts),
        (0, 3, 1)
    );
    let accepted_first = finish(&db, &first, &accepted());
    assert_eq!(accepted_first["status"], "waiting_retry");
    assert_eq!(accepted_first["confirmed_chunks"], 1);
    assert_eq!(
        fixture.row(NotificationChannel::Bark, key).chunk_attempts,
        0
    );
    drop(db);

    let db = fixture.open();
    assert_eq!(
        db.due_notification_deliveries(100).unwrap()[0].payload,
        queued
    );
    let second = claim(&db, key);
    assert_eq!(
        (second.chunk_index, second.chunk_attempts, second.attempts),
        (1, 1, 2)
    );
    let rejected = finish(&db, &second, &http_failure(503));
    assert_eq!(rejected["confirmed_chunks"], 1);
    assert_eq!(rejected["request_count"], 2);
    fixture.due_now(NotificationChannel::Bark, key);
    let second_retry = claim(&db, key);
    assert_eq!(
        (
            second_retry.chunk_index,
            second_retry.chunk_attempts,
            second_retry.attempts
        ),
        (1, 2, 3)
    );
    assert_eq!(
        finish(&db, &second_retry, &accepted())["confirmed_chunks"],
        2
    );
    let third = claim(&db, key);
    assert_eq!(
        (third.chunk_index, third.chunk_attempts, third.attempts),
        (2, 1, 4)
    );
    let complete = finish(&db, &third, &accepted());
    assert_eq!(complete["status"], "sent");
    assert_eq!(complete["confirmed_chunks"], 3);
    assert_eq!(complete["request_count"], 4);
    assert_eq!(complete["attempts"], 1);
    let row = fixture.row(NotificationChannel::Bark, key);
    assert_eq!(row.chunk_index, 3);
    assert!(!row.has_ciphertext && !row.has_nonce);
    drop(db);
    assert!(fixture
        .open()
        .due_notification_deliveries(100)
        .unwrap()
        .is_empty());
}

#[test]
fn retry_batches_are_bounded_to_ten_and_instance_identity_survives_reopen() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    let instance = db.notification_instance_id().unwrap();
    for index in 0..12 {
        stage(
            &db,
            &fixture,
            NotificationChannel::Bark,
            &format!("bounded-{index:02}"),
        );
    }
    assert_eq!(db.due_notification_deliveries(100).unwrap().len(), 10);
    assert_eq!(db.due_notification_deliveries(2).unwrap().len(), 2);
    drop(db);
    assert_eq!(fixture.open().notification_instance_id().unwrap(), instance);
    let other = Fixture::new();
    assert_ne!(other.open().notification_instance_id().unwrap(), instance);
}

#[test]
fn secret_and_pending_body_are_encrypted_and_status_never_returns_the_token() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.sync_notification_channels(&fixture.config).unwrap();
    db.set_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING, FIXTURE_TOKEN.as_bytes())
        .unwrap();
    stage(&db, &fixture, NotificationChannel::Bark, "private-event");
    let secret_setting = db
        .get_setting(PROBE_BARK_DEVICE_KEY_SETTING)
        .unwrap()
        .unwrap();
    assert!(!secret_setting.contains(FIXTURE_TOKEN));
    assert!(serde_json::from_str::<Value>(&secret_setting).unwrap()["ciphertext"].is_string());
    let due = db.due_notification_deliveries(100).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].payload, payload());
    assert!(!due[0].payload.to_string().contains(FIXTURE_TOKEN));
    assert!(!db
        .notification_channel_status()
        .unwrap()
        .to_string()
        .contains(FIXTURE_TOKEN));
    for path in [&fixture.path, &fixture.path.with_extension("sqlite-wal")] {
        assert_no_bytes(path, FIXTURE_TOKEN);
        assert_no_bytes(path, FIXTURE_BODY);
    }
    drop(db);
    let db = fixture.open();
    assert_eq!(
        db.get_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING)
            .unwrap()
            .as_deref(),
        Some(FIXTURE_TOKEN.as_bytes())
    );
    assert_eq!(
        db.due_notification_deliveries(100).unwrap()[0].payload,
        payload()
    );
    db.remove_secret_setting(PROBE_BARK_DEVICE_KEY_SETTING)
        .unwrap();
    assert!(db
        .get_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING)
        .unwrap()
        .is_none());
}
