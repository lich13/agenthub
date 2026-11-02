use agenthub_core::db::{NewProbeEvent, PanelDb};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

struct TestDatabase {
    directory: PathBuf,
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("agenthub-api-upgrade-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("agenthub.sqlite");
        Self { directory, path }
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn contains_bytes(path: &Path, value: &str) -> bool {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => panic!("read {}: {error}", path.display()),
    };
    bytes
        .windows(value.len())
        .any(|window| window == value.as_bytes())
}

#[test]
fn api_keys_persist_as_digests_and_rotation_and_revocation_survive_reopen() {
    let fixture = TestDatabase::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    assert!(!db.verify_admin_api_key("").unwrap());
    assert!(!db.verify_admin_api_key("nhk_unconfigured_fixture").unwrap());
    let first = db.rotate_admin_api_key().unwrap();
    assert!(first.starts_with("nhk_"));
    assert_eq!(first.len(), 68);
    assert!(db.verify_admin_api_key(&first).unwrap());
    assert!(!db.verify_admin_api_key("nhk_incorrect_fixture").unwrap());
    let digest = db.get_setting("admin_api_key_sha256").unwrap().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(!digest.contains(&first));
    drop(db);

    let db = PanelDb::open(&fixture.path).unwrap();
    assert!(db.verify_admin_api_key(&first).unwrap());
    let second = db.rotate_admin_api_key().unwrap();
    assert_ne!(first, second);
    assert!(!db.verify_admin_api_key(&first).unwrap());
    assert!(db.verify_admin_api_key(&second).unwrap());
    drop(db);

    let db = PanelDb::open(&fixture.path).unwrap();
    assert!(!db.verify_admin_api_key(&first).unwrap());
    assert!(db.verify_admin_api_key(&second).unwrap());
    db.revoke_admin_api_key().unwrap();
    drop(db);
    let db = PanelDb::open(&fixture.path).unwrap();
    assert!(!db.verify_admin_api_key(&second).unwrap());
    assert!(!db.verify_admin_api_key("").unwrap());
    drop(db);

    for path in [&fixture.path, &fixture.path.with_extension("sqlite-wal")] {
        assert!(!contains_bytes(path, &first));
        assert!(!contains_bytes(path, &second));
    }
    let connection = Connection::open(&fixture.path).unwrap();
    let audit: String = connection
        .query_row(
            "SELECT group_concat(action || detail_json, '|') FROM audit_log",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(audit.matches("api_key.rotate").count(), 2);
    assert_eq!(audit.matches("api_key.revoke").count(), 1);
    assert!(!audit.contains(&first));
    assert!(!audit.contains(&second));
}

#[test]
fn upgrade_erases_retired_auth_pages_and_wal_but_preserves_encrypted_business_state() {
    let fixture = TestDatabase::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    let bark_key = b"fixture-bark-secret-preserved";
    db.set_secret_setting_bytes("probe_bark_device_key", bark_key)
        .unwrap();
    let ciphertext = db.get_setting("probe_bark_device_key").unwrap().unwrap();
    assert!(!ciphertext.contains("fixture-bark-secret-preserved"));
    db.set_setting("probe_error_monitor_cursor", r#"{"row_id":42}"#)
        .unwrap();
    let event = db
        .record_probe_event(NewProbeEvent {
            kind: "reply-needed",
            thread_id: Some("fixture-thread"),
            title: Some("确认事项"),
            message: Some("保留通知"),
            dedupe_key: Some("fixture-dedupe"),
            source: "fixture-hook",
            payload: json!({"turn_id":"fixture-turn"}),
        })
        .unwrap();
    assert!(db
        .claim_probe_dedupe("reply_needed", "fixture-dedupe", 3600)
        .unwrap());
    db.record_audit(
        Some("fixture-admin"),
        "threads.rename",
        Some("thread"),
        Some("fixture-thread"),
        Some("192.0.2.5"),
        json!({"name":"保留业务审计"}),
    )
    .unwrap();
    drop(db);

    let legacy = Connection::open(&fixture.path).unwrap();
    legacy.pragma_update(None, "user_version", 0).unwrap();
    legacy.execute_batch(
        "PRAGMA secure_delete=OFF; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
         CREATE TABLE admins(id TEXT PRIMARY KEY, username TEXT, password_hash TEXT);
         CREATE TABLE sessions(id TEXT PRIMARY KEY, admin_id TEXT, token_hash TEXT, csrf_token TEXT);
         CREATE TABLE turnstile_attempts(id TEXT PRIMARY KEY, token_hash TEXT);",
    ).unwrap();
    let password_marker = "retired-password-fixture-marker";
    let session_marker = "retired-session-fixture-marker";
    let turnstile_marker = "retired-turnstile-fixture-marker";
    let wal_marker = "retired-wal-session-fixture-marker";
    legacy
        .execute(
            "INSERT INTO admins VALUES('fixture-admin','fixture-user',?1)",
            [password_marker.repeat(1000)],
        )
        .unwrap();
    legacy.execute("INSERT INTO sessions VALUES('fixture-session','fixture-admin',?1,'retired-csrf-fixture')",
        [session_marker.repeat(1000)]).unwrap();
    legacy
        .execute(
            "INSERT INTO turnstile_attempts VALUES('fixture-attempt',?1)",
            [turnstile_marker.repeat(1000)],
        )
        .unwrap();
    for key in [
        "turnstile_enabled",
        "turnstile_required",
        "turnstile_site_key",
        "turnstile_secret_key",
        "turnstile_expected_hostname",
        "turnstile_expected_action",
        "session_ttl_seconds",
        "cookie_secure",
        "login_rate_limit_per_minute",
    ] {
        legacy
            .execute(
                "INSERT INTO settings(key,value,updated_at) VALUES(?1,?2,1)",
                params![key, format!("retired-setting-fixture-{key}")],
            )
            .unwrap();
    }
    legacy
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    legacy.execute("INSERT INTO sessions VALUES('fixture-wal-session','fixture-admin',?1,'retired-wal-csrf-fixture')",
        [wal_marker.repeat(1000)]).unwrap();
    let wal_path = fixture.path.with_extension("sqlite-wal");
    assert!(contains_bytes(&fixture.path, password_marker));
    assert!(contains_bytes(&fixture.path, session_marker));
    assert!(contains_bytes(&wal_path, wal_marker));

    let db = PanelDb::open(&fixture.path).unwrap();
    for table in ["admins", "sessions", "turnstile_attempts"] {
        let exists: bool = legacy
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists, "retired table {table}");
    }
    let retired_settings: usize = legacy.query_row(
        "SELECT count(*) FROM settings WHERE key GLOB 'turnstile_*' OR key IN ('session_ttl_seconds','cookie_secure','login_rate_limit_per_minute')",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(retired_settings, 0);
    for marker in [
        password_marker,
        session_marker,
        turnstile_marker,
        wal_marker,
        "retired-setting-fixture-",
        "retired-csrf-fixture",
    ] {
        assert!(
            !contains_bytes(&fixture.path, marker),
            "retired bytes in database: {marker}"
        );
        assert!(
            !contains_bytes(&wal_path, marker),
            "retired bytes in WAL: {marker}"
        );
    }
    assert_eq!(
        legacy
            .query_row("PRAGMA freelist_count", [], |row| row.get::<_, usize>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.get_setting("probe_bark_device_key").unwrap().unwrap(),
        ciphertext
    );
    assert_eq!(
        db.get_secret_setting_bytes("probe_bark_device_key")
            .unwrap()
            .unwrap(),
        bark_key
    );
    assert_eq!(
        db.get_setting("probe_error_monitor_cursor")
            .unwrap()
            .as_deref(),
        Some(r#"{"row_id":42}"#)
    );
    assert_eq!(db.list_probe_events(10).unwrap(), vec![event.clone()]);
    assert!(!db
        .claim_probe_dedupe("reply_needed", "fixture-dedupe", 3600)
        .unwrap());
    let audit: (String, String, String) = legacy
        .query_row(
            "SELECT admin_id,action,detail_json FROM audit_log WHERE action='threads.rename'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        audit,
        (
            "fixture-admin".into(),
            "threads.rename".into(),
            json!({"name":"保留业务审计"}).to_string()
        )
    );
    drop(db);
    drop(legacy);

    let reopened = PanelDb::open(&fixture.path).unwrap();
    assert_eq!(
        reopened
            .get_setting("probe_bark_device_key")
            .unwrap()
            .unwrap(),
        ciphertext
    );
    assert_eq!(reopened.list_probe_events(10).unwrap(), vec![event]);
}

#[test]
fn upgrade_removes_retired_cookie_settings_even_without_legacy_auth_tables() {
    for key in ["cookie_secure", "login_rate_limit_per_minute"] {
        let fixture = TestDatabase::new();
        let db = PanelDb::open(&fixture.path).unwrap();
        db.set_setting(key, "retired-cookie-setting-fixture")
            .unwrap();
        db.set_setting("probe_error_monitor_cursor", "keep-fixture")
            .unwrap();
        drop(db);
        fixture
            .connection()
            .pragma_update(None, "user_version", 0)
            .unwrap();
        let upgraded = PanelDb::open(&fixture.path).unwrap();
        assert!(
            upgraded.get_setting(key).unwrap().is_none(),
            "retired setting {key}"
        );
        assert_eq!(
            upgraded
                .get_setting("probe_error_monitor_cursor")
                .unwrap()
                .as_deref(),
            Some("keep-fixture")
        );
        drop(upgraded);
        assert!(!contains_bytes(
            &fixture.path,
            "retired-cookie-setting-fixture"
        ));
    }
}

#[test]
fn pending_web_retirement_resumes_erasure_after_legacy_tables_are_gone() {
    let fixture = TestDatabase::new();
    let db = PanelDb::open(&fixture.path).unwrap();
    db.set_setting("probe_error_monitor_cursor", "keep-fixture")
        .unwrap();
    drop(db);

    let marker = "retired-pending-credential-fixture";
    let legacy = Connection::open(&fixture.path).unwrap();
    legacy
        .execute_batch(
            "PRAGMA user_version=0;
             PRAGMA secure_delete=OFF;
             CREATE TABLE sessions(id TEXT PRIMARY KEY, token_hash TEXT);",
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO sessions VALUES('fixture-session',?1)",
            [marker.repeat(1000)],
        )
        .unwrap();
    legacy
        .execute_batch(
            "PRAGMA wal_checkpoint(TRUNCATE);
             BEGIN IMMEDIATE;
             INSERT INTO settings(key,value,updated_at) VALUES('web_retirement_pending','true',1);
             DROP TABLE sessions;
             COMMIT;
             PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .unwrap();
    let retired_tables: usize = legacy
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('admins','sessions','turnstile_attempts')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retired_tables, 0);
    assert!(
        legacy
            .query_row("PRAGMA freelist_count", [], |row| row.get::<_, usize>(0))
            .unwrap()
            > 0
    );
    drop(legacy);
    assert!(contains_bytes(&fixture.path, marker));

    let resumed = PanelDb::open(&fixture.path).unwrap();
    assert!(resumed
        .get_setting("web_retirement_pending")
        .unwrap()
        .is_none());
    assert_eq!(
        resumed
            .get_setting("probe_error_monitor_cursor")
            .unwrap()
            .as_deref(),
        Some("keep-fixture")
    );
    assert!(!contains_bytes(&fixture.path, marker));
    assert!(!contains_bytes(
        &fixture.path.with_extension("sqlite-wal"),
        marker
    ));
    drop(resumed);

    let reopened = PanelDb::open(&fixture.path).unwrap();
    assert!(reopened
        .get_setting("web_retirement_pending")
        .unwrap()
        .is_none());
}
