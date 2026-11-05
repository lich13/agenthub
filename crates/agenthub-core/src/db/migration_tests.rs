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
        let err = PanelDb::open(&fixture.path).err().unwrap();
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
                PanelDb::open(path).unwrap()
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
