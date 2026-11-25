use super::PanelDb;
use crate::crypto::SecretBox;
use rusqlite::Connection;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
};

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("agenthub-db-migration-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("panel.sqlite");
        Self { root, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn schema_three_reopen_preserves_ciphertext_queue_progress_and_business_state() {
    let fixture = Fixture::new();
    let key = SecretBox::random_key_material();
    let db =
        PanelDb::open_with_secret_box(&fixture.path, SecretBox::from_key_material(&key).unwrap())
            .unwrap();
    assert_eq!(
        db.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='thread_followups'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    db.conn.lock().unwrap().execute_batch("CREATE TABLE thread_followups(id TEXT PRIMARY KEY, message TEXT); INSERT INTO thread_followups VALUES('historic', 'preserved');").unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
        .unwrap();
    db.create_job("fixture-job", "probe_bark_test", "fixture")
        .unwrap();
    db.conn.lock().unwrap().execute_batch("INSERT INTO native_probe_deliveries(event_key,provider,event_json,ciphertext,nonce,status,created_at,updated_at) VALUES('fixture','grok','',X'01',X'02','pending',1,2); INSERT INTO probe_notification_deliveries(event_key,channel,status,created_ms,updated_ms,queue_version,chunk_index,attempts) VALUES('native:fixture','bark','pending',1,2,1,3,4);").unwrap();
    drop(db);
    for _ in 0..2 {
        let db = PanelDb::open_with_secret_box(
            &fixture.path,
            SecretBox::from_key_material(&key).unwrap(),
        )
        .unwrap();
        assert_eq!(
            db.get_secret_setting_bytes("probe_bark_device_key")
                .unwrap()
                .unwrap(),
            b"fixture-device"
        );
        assert!(db.job("fixture-job").unwrap().is_some());
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN (
                    'idx_native_probe_unfinished',
                    'idx_probe_events_notification_key',
                    'idx_probe_error_incidents_event',
                    'idx_audit_log_created_at',
                    'idx_jobs_started_at',
                    'idx_native_probe_deliveries_updated_at'
                )",
                [],
                |r| r.get::<_, usize>(0)
            )
            .unwrap(),
            6
        );
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row(
                "SELECT message FROM thread_followups WHERE id='historic'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "preserved"
        );
        assert_eq!(
            conn.query_row(
                "SELECT chunk_index,attempts FROM probe_notification_deliveries",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            )
            .unwrap(),
            (3, 4)
        );
        assert_eq!(
            conn.query_row("SELECT ciphertext FROM native_probe_deliveries", [], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .unwrap(),
            vec![1]
        );
    }
}
#[test]
fn old_schema_requires_intermediate_upgrade_without_changing_rows() {
    for version in 0..3 {
        let fixture = Fixture::new();
        let conn = Connection::open(&fixture.path).unwrap();
        conn.execute_batch("CREATE TABLE fixture_queue(payload TEXT); INSERT INTO fixture_queue VALUES('pending');").unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        drop(conn);
        let err = PanelDb::open_with_secret_box(
            &fixture.path,
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("v2.0.10"));
        let conn = Connection::open(&fixture.path).unwrap();
        assert_eq!(
            conn.query_row("SELECT payload FROM fixture_queue", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "pending"
        );
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
    }
}
#[test]
fn simultaneous_new_store_opens_share_current_schema() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(4));
    let workers = (0..4)
        .map(|_| {
            let path = fixture.path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                PanelDb::open_with_secret_box(
                    path,
                    crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
                )
                .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| worker.join())
        .collect::<Vec<_>>();
    for result in results {
        let db = result.unwrap();
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            conn.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            conn.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[test]
fn encrypted_upgrade_rolls_back_all_panel_rows_and_preserves_delivery_progress() {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use rusqlite::params;
    let fixture = Fixture::new();
    let db = PanelDb::open_with_secret_box(
        &fixture.path,
        SecretBox::from_key_material(&"19".repeat(32)).unwrap(),
    )
    .unwrap();
    let cipher = Aes256Gcm::new((&[0x19; 32]).into());
    let nonce = [3_u8; 12];
    let setting = cipher
        .encrypt(Nonce::from_slice(&nonce), b"fixture".as_slice())
        .unwrap();
    let old = serde_json::json!({"ciphertext": STANDARD.encode(setting), "nonce": STANDARD.encode(nonce)}).to_string();
    let channel_nonce = [4_u8; 12];
    let channel = cipher
        .encrypt(
            Nonce::from_slice(&channel_nonce),
            br#"{"chunks":["one","two"]}"#.as_slice(),
        )
        .unwrap();
    {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings(key,value,updated_at) VALUES('fixture',?1,7)",
            [&old],
        )
        .unwrap();
        conn.execute("INSERT INTO probe_notification_deliveries(event_key,channel,ciphertext,nonce,status,created_ms,updated_ms,chunk_index,lease_owner)
            VALUES('event','bark',X'00',?1,'delivering',8,9,1,'fixture-owner')", [channel_nonce.as_slice()]).unwrap();
    }

    assert!(db.bind_legacy_panel_records().is_err());
    assert_eq!(db.get_setting("fixture").unwrap().as_ref(), Some(&old));
    let check_state = || {
        db.conn.lock().unwrap().query_row("SELECT ciphertext,nonce,status,chunk_index,lease_owner,updated_ms FROM probe_notification_deliveries", [],
            |r| Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?))).unwrap()
    };
    assert_eq!(check_state().0, [0]);
    db.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE probe_notification_deliveries SET ciphertext=?1",
            params![channel],
        )
        .unwrap();
    db.bind_legacy_panel_records().unwrap();
    let converted = check_state();
    assert_eq!((converted.1.len(), converted.1[0]), (13, 2));
    assert_eq!(
        (&*converted.2, converted.3, &*converted.4, converted.5),
        ("delivering", 1, "fixture-owner", 9)
    );
    let (encrypted_setting, timestamp) =
        db.get_setting_with_updated_at("fixture").unwrap().unwrap();
    assert_ne!(encrypted_setting, old);
    assert_eq!(timestamp, 7);
    assert_eq!(
        db.get_secret_setting_bytes("fixture").unwrap().unwrap(),
        b"fixture"
    );
    assert!(db
        .crypto
        .decrypt(
            &["notification", "bark", "other"],
            &converted.0,
            &converted.1
        )
        .is_err());
    db.bind_legacy_panel_records().unwrap();
    assert_eq!(converted, check_state());
    assert_eq!(
        db.get_setting("fixture").unwrap().unwrap(),
        encrypted_setting
    );
}

#[test]
fn existing_schema_three_store_missing_performance_indexes_backfills_idempotently() {
    let fixture = Fixture::new();
    let key = SecretBox::random_key_material();
    let db =
        PanelDb::open_with_secret_box(&fixture.path, SecretBox::from_key_material(&key).unwrap())
            .unwrap();
    {
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(
            "DROP INDEX idx_audit_log_created_at;
             DROP INDEX idx_jobs_started_at;
             DROP INDEX idx_native_probe_deliveries_updated_at;",
        )
        .unwrap();
    }
    drop(db);

    let reopened =
        PanelDb::open_with_secret_box(&fixture.path, SecretBox::from_key_material(&key).unwrap())
            .unwrap();
    let conn = reopened.conn.lock().unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN (
                'idx_audit_log_created_at',
                'idx_jobs_started_at',
                'idx_native_probe_deliveries_updated_at'
            )",
            [],
            |r| r.get::<_, usize>(0),
        )
        .unwrap(),
        3
    );
}
