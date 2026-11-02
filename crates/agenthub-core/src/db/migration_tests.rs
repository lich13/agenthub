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
    assert_eq!(user_version(&connection), 2);
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
    assert_eq!(user_version(&fixture.connection()), 2);
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
                 VALUES('fixture-terminal-claim','claude_code','{}','sent',1,2);
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
    assert_eq!(user_version(&connection), 2);
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
    assert_eq!(user_version(&fixture.connection()), 2);
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
    assert_eq!(user_version(&connection), 2);
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
