use super::PanelDb;
use crate::crypto::SecretBox;
use rusqlite::{hooks::AuthAction, hooks::Authorization, Connection};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier, Mutex},
};

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-db-migration-tests-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("panel.sqlite");
        Self { root, path }
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn user_version(connection: &Connection) -> i64 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn native_payload_upgrade_rekeys_only_notification_data_and_preserves_retry_progress() {
    use crate::{
        db::{NativeDelivery, NotificationChannel},
        native_probe::{NativeProvider, NativeTurnEvent},
    };
    use serde_json::json;
    for (index, old_key) in SecretBox::legacy_development_keys()
        .into_iter()
        .chain([SecretBox::from_key_material(&SecretBox::random_key_material()).unwrap()])
        .enumerate()
    {
        let fixture = Fixture::new();
        let old = PanelDb::open_with_secret_box(&fixture.path, old_key.clone()).unwrap();
        old.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        let mut config = crate::config::Config::default();
        config.probe.notifications.enabled = true;
        old.sync_notification_channels(&config).unwrap();
        let payload = json!({"chunks":["confirmed", "remaining"],"fixture":"encrypted retry"});
        old.stage_notification_delivery(
            NotificationChannel::Bark,
            "native:fixture-pending",
            chrono::Utc::now().timestamp_millis(),
            &payload,
        )
        .unwrap();
        let delivery = NativeDelivery {
            event_key: "fixture-pending".into(),
            provider: NativeProvider::Antigravity,
            session_key: "fixture-session".into(),
            thread_id: "fixture-session".into(),
            title: "fixture-title".into(),
            event: NativeTurnEvent {
                prompt_id: None,
                position: 5,
                turn_id: "fixture-turn".into(),
                kind: "completion".into(),
                body: format!("{}legacy-private-body-tail", "x".repeat(1000)),
                timestamp_ms: 1,
            },
        };
        let conn = fixture.connection();
        conn.execute("INSERT INTO native_probe_streams VALUES('antigravity','fixture-session','fixture-identity',5,1)", []).unwrap();
        conn.execute("INSERT INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at) VALUES(?1,'antigravity',?2,'pending',1,2)", rusqlite::params![delivery.event_key,serde_json::to_string(&delivery).unwrap()]).unwrap();
        conn.execute_batch("UPDATE probe_notification_deliveries SET attempts=2,chunk_index=1,chunk_attempts=1,first_attempt_ms=unixepoch()*1000,lease_owner='fixture-owner',lease_until_ms=7;
            PRAGMA user_version=2;").unwrap();
        if index == 1 {
            conn.execute_batch("UPDATE native_probe_deliveries SET status='delivering';
                UPDATE probe_notification_deliveries SET outcome_json='{\"status\":\"waiting_retry\",\"sent\":false}';").unwrap();
        }
        conn.execute("INSERT INTO codex_desktop_submissions(id,source_id,thread_id,status,ciphertext,nonce,created_at,updated_at) VALUES('fixture-independent','local','fixture-thread','held',?1,?2,1,1)",
            rusqlite::params![b"independent-keychain-ciphertext".as_slice(), b"independent-keychain-nonce".as_slice()]).unwrap();
        let before: String = old.get_setting("probe_bark_device_key").unwrap().unwrap();
        drop(conn);
        drop(old);
        let target = if index == 2 {
            old_key
        } else {
            SecretBox::from_key_material(&SecretBox::random_key_material()).unwrap()
        };
        let upgraded = PanelDb::open_with_secret_box(&fixture.path, target.clone()).unwrap();
        for path in [&fixture.path, &fixture.path.with_extension("sqlite-wal")] {
            if let Ok(bytes) = fs::read(path) {
                assert!(!bytes
                    .windows(b"legacy-private-body-tail".len())
                    .any(|window| window == b"legacy-private-body-tail"));
            }
        }
        assert_eq!(
            upgraded
                .get_secret_setting_bytes("probe_bark_device_key")
                .unwrap()
                .unwrap(),
            b"fixture-device"
        );
        if index == 2 {
            assert_eq!(
                upgraded
                    .get_setting("probe_bark_device_key")
                    .unwrap()
                    .unwrap(),
                before
            );
        }
        if index != 1 {
            assert_eq!(
                upgraded.pending_native_deliveries(10).unwrap()[0]
                    .event
                    .body,
                delivery.event.body
            );
        }
        assert_eq!(
            upgraded.due_notification_deliveries(10).unwrap()[0].payload,
            payload
        );
        let conn = fixture.connection();
        assert_eq!(
            conn.query_row(
                "SELECT ciphertext,nonce FROM codex_desktop_submissions",
                [],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            )
            .unwrap(),
            (
                b"independent-keychain-ciphertext".to_vec(),
                b"independent-keychain-nonce".to_vec()
            )
        );
        let progress: (i64,i64,i64,String,i64) = conn.query_row("SELECT attempts,chunk_index,chunk_attempts,lease_owner,lease_until_ms FROM probe_notification_deliveries", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap();
        assert_eq!(progress, (2, 1, 1, "fixture-owner".into(), 7));
        let raw: String = conn
            .query_row(
                "SELECT event_json FROM native_probe_deliveries",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!raw.contains("legacy-private-body"));
        assert_eq!(user_version(&conn), 3);
        drop(conn);
        drop(upgraded);
        let reopened = PanelDb::open_with_secret_box(&fixture.path, target).unwrap();
        if index == 1 {
            reopened.recover_interrupted_native_deliveries().unwrap();
            assert!(reopened.pending_native_deliveries(10).unwrap().is_empty());
            assert_eq!(
                fixture
                    .connection()
                    .query_row("SELECT status FROM native_probe_deliveries", [], |row| {
                        row.get::<_, String>(0)
                    })
                    .unwrap(),
                "queued"
            );
        } else {
            assert_eq!(
                reopened.pending_native_deliveries(10).unwrap()[0]
                    .event
                    .body,
                delivery.event.body
            );
            reopened.claim_native_delivery(&delivery.event_key).unwrap();
            reopened
                .finish_native_delivery(&delivery, json!({"sent":true,"status":"sent"}))
                .unwrap();
        }
        assert_eq!(fixture.connection().query_row("SELECT count(*) FROM native_probe_deliveries WHERE ciphertext IS NOT NULL OR nonce IS NOT NULL",[],|row|row.get::<_,usize>(0)).unwrap(),0);
        // The allowed UI excerpt is short; the complete legacy body is gone.
        assert!(reopened.list_probe_events(10).unwrap().len() == 1);
    }
}

#[test]
fn failed_native_migration_rolls_back_encryption_and_keeps_original_queue() {
    let fixture = Fixture::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
        .unwrap();
    let secret = db.get_setting("probe_bark_device_key").unwrap().unwrap();
    fixture.connection().execute_batch("INSERT INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at) VALUES('fixture','grok','invalid-json','pending',1,2); PRAGMA user_version=2;").unwrap();
    drop(db);
    let key = SecretBox::from_key_material(&SecretBox::random_key_material()).unwrap();
    assert!(PanelDb::open_with_secret_box(&fixture.path, key).is_err());
    let conn = fixture.connection();
    assert_eq!(user_version(&conn), 2);
    assert_eq!(
        conn.query_row(
            "SELECT value FROM settings WHERE key='probe_bark_device_key'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        secret
    );
    assert_eq!(
        conn.query_row("SELECT event_json FROM native_probe_deliveries", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "invalid-json"
    );
}

#[test]
fn a_completed_upgrade_skips_retirement_cleanup_and_event_scans_on_reopen() {
    let fixture = Fixture::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    db.set_setting("probe_error_monitor_cursor", "preserve-fixture-cursor")
        .unwrap();
    drop(db);

    let legacy = fixture.connection();
    legacy
        .execute_batch(
            "PRAGMA user_version=0;
             CREATE TABLE admins(id TEXT PRIMARY KEY, username TEXT, password_hash TEXT);
             INSERT INTO admins VALUES('fixture-admin','fixture-user','fixture-password-hash');
             INSERT INTO settings(key,value,updated_at) VALUES('cookie_secure','retired-fixture',1),
                 ('probe_logs_db_last_maintain','retired-fixture',1);",
        )
        .unwrap();
    drop(legacy);

    let upgraded = PanelDb::open(&fixture.path).unwrap();
    assert_eq!(
        upgraded
            .get_setting("probe_error_monitor_cursor")
            .unwrap()
            .as_deref(),
        Some("preserve-fixture-cursor")
    );
    drop(upgraded);
    let connection = fixture.connection();
    assert_eq!(user_version(&connection), 3);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='admins'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    drop(connection);

    let connection = fixture.connection();
    connection
        .execute_batch(
            "INSERT INTO settings(key,value,updated_at)
                 VALUES('probe_logs_db_last_maintain','protected-fixture',2);
             CREATE TRIGGER reject_repeated_retirement_cleanup
             BEFORE DELETE ON settings
             WHEN OLD.key='probe_logs_db_last_maintain'
             BEGIN SELECT RAISE(ABORT,'retirement cleanup repeated'); END;",
        )
        .unwrap();
    drop(connection);

    let connection = fixture.connection();
    connection.authorizer(Some(
        |context: rusqlite::hooks::AuthContext<'_>| match context.action {
            AuthAction::Read {
                table_name: "probe_events",
                ..
            } => Authorization::Deny,
            _ => Authorization::Allow,
        },
    ));
    let db = PanelDb {
        path: fixture.path.clone(),
        conn: Arc::new(Mutex::new(connection)),
        crypto: SecretBox::deterministic_dev(),
    };
    db.migrate().unwrap();
    drop(db);

    let reopened = PanelDb::open(&fixture.path).unwrap();
    assert_eq!(
        reopened
            .get_setting("probe_logs_db_last_maintain")
            .unwrap()
            .as_deref(),
        Some("protected-fixture")
    );
    assert_eq!(
        reopened
            .get_setting("probe_error_monitor_cursor")
            .unwrap()
            .as_deref(),
        Some("preserve-fixture-cursor")
    );
    drop(reopened);
    assert_eq!(user_version(&fixture.connection()), 3);
}

#[test]
fn concurrent_opens_serialize_a_legacy_upgrade_and_preserve_business_state() {
    let fixture = Fixture::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    db.set_setting("probe_error_monitor_cursor", "concurrent-fixture-cursor")
        .unwrap();
    drop(db);

    let legacy = fixture.connection();
    legacy
        .execute_batch(
            "DROP TABLE probe_notification_deliveries;
             DROP TABLE probe_notification_channels;
             DELETE FROM settings WHERE key='probe_channel_migration_v1';
             INSERT INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at)
                 VALUES('fixture-terminal-claim','claude_code','{\"event_key\":\"fixture-terminal-claim\",\"provider\":\"claude_code\",\"session_key\":\"fixture\",\"thread_id\":\"fixture\",\"title\":\"fixture\",\"event\":{\"position\":1,\"turn_id\":\"turn\",\"kind\":\"completion\",\"body\":\"fixture\",\"timestamp_ms\":1}}','sent',1,2);
             PRAGMA user_version=0;",
        )
        .unwrap();
    drop(legacy);

    const WORKERS: usize = 8;
    let barrier = Arc::new(Barrier::new(WORKERS + 1));
    let mut workers = Vec::new();
    for _ in 0..WORKERS {
        let path = fixture.path.clone();
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            PanelDb::open(path).unwrap()
        }));
    }
    barrier.wait();
    for worker in workers {
        drop(worker.join().unwrap());
    }

    let connection = fixture.connection();
    assert_eq!(user_version(&connection), 3);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM probe_notification_deliveries
                 WHERE event_key='native:fixture-terminal-claim' AND channel='bark' AND status='sent'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT value FROM settings WHERE key='probe_error_monitor_cursor'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "concurrent-fixture-cursor"
    );
}

#[test]
fn a_failed_retirement_keeps_version_zero_and_resumes_without_losing_business_data() {
    let fixture = Fixture::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    db.set_setting("probe_error_monitor_cursor", "resume-fixture-cursor")
        .unwrap();
    db.set_setting("probe_legacy_import", "legacy-fixture")
        .unwrap();
    drop(db);

    let legacy = fixture.connection();
    legacy
        .execute_batch(
            "PRAGMA user_version=0;
             CREATE TABLE admins(id TEXT PRIMARY KEY, username TEXT, password_hash TEXT);
             INSERT INTO admins VALUES('fixture-admin','fixture-user','fixture-password-hash');
             INSERT INTO settings(key,value,updated_at) VALUES('cookie_secure','retired-fixture',1);
             CREATE TRIGGER fail_channel_migration_marker
             BEFORE DELETE ON settings WHEN OLD.key='probe_legacy_import'
             BEGIN SELECT RAISE(ABORT,'fixture migration failure'); END;",
        )
        .unwrap();
    drop(legacy);

    let error = match PanelDb::open(&fixture.path) {
        Ok(db) => {
            drop(db);
            panic!("fixture trigger must fail the legacy migration")
        }
        Err(error) => error,
    };
    assert!(error.to_string().contains("fixture migration failure"));
    let failed = fixture.connection();
    assert_eq!(user_version(&failed), 0);
    assert_eq!(
        failed
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='admins'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        failed
            .query_row(
                "SELECT count(*) FROM settings WHERE key='cookie_secure'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        failed
            .query_row(
                "SELECT value FROM settings WHERE key='probe_error_monitor_cursor'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "resume-fixture-cursor"
    );
    assert_eq!(
        failed
            .query_row(
                "SELECT value FROM settings WHERE key='probe_legacy_import'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "legacy-fixture"
    );
    drop(failed);

    let connection = fixture.connection();
    connection
        .execute_batch("DROP TRIGGER fail_channel_migration_marker;")
        .unwrap();
    drop(connection);

    let resumed = PanelDb::open(&fixture.path).unwrap();
    assert!(resumed
        .get_setting("probe_legacy_import")
        .unwrap()
        .is_none());
    assert_eq!(
        resumed
            .get_setting("probe_error_monitor_cursor")
            .unwrap()
            .as_deref(),
        Some("resume-fixture-cursor")
    );
    drop(resumed);
    assert_eq!(user_version(&fixture.connection()), 3);
}

#[test]
fn schema_one_upgrade_adds_the_desktop_ledger_without_repeating_retirement_cleanup() {
    let fixture = Fixture::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    db.set_setting("probe_error_monitor_cursor", "schema-one-fixture-cursor")
        .unwrap();
    drop(db);

    let schema_one = fixture.connection();
    schema_one
        .execute_batch(
            "DROP TABLE codex_desktop_submissions;
             INSERT INTO settings(key,value,updated_at)
                 VALUES('probe_logs_db_last_maintain','protected-schema-one-fixture',3);
             CREATE TRIGGER reject_schema_one_retirement_cleanup
             BEFORE DELETE ON settings
             WHEN OLD.key='probe_logs_db_last_maintain'
             BEGIN SELECT RAISE(ABORT,'schema-one retirement cleanup repeated'); END;
             PRAGMA user_version=1;",
        )
        .unwrap();
    drop(schema_one);

    let upgraded = PanelDb::open(&fixture.path).unwrap();
    assert_eq!(
        upgraded
            .get_setting("probe_error_monitor_cursor")
            .unwrap()
            .as_deref(),
        Some("schema-one-fixture-cursor")
    );
    drop(upgraded);

    let connection = fixture.connection();
    assert_eq!(user_version(&connection), 3);
    assert_eq!(
        connection
            .query_row(
                "SELECT value FROM settings WHERE key='probe_logs_db_last_maintain'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "protected-schema-one-fixture"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type='table' AND name='codex_desktop_submissions'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type='trigger' AND name='reject_schema_one_retirement_cleanup'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}
