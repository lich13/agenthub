use agenthub_core::{
    config::Config,
    db::{NotificationChannel, PanelDb},
    platform::PlatformKind,
    services::{
        commands::{is_retired_command, ALLOWED_RPC_COMMANDS},
        settings::{build_settings_view, ProbeSecretState, PROBE_BARK_DEVICE_KEY_SETTING},
    },
};
use rusqlite::{params, types::Value as SqlValue, Connection};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

const BARK_KEY: &[u8] = b"fixture-retained-bark-key";
const RETIRED_CIPHERTEXT: &[u8] = b"fixture-retired-pending-ciphertext-marker";
const RETIRED_DETAIL: &str = "fixture-retired-notification-detail";

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-notification-retirement-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        Self {
            path: root.join("panel.sqlite"),
            root,
        }
    }

    fn open(&self) -> PanelDb {
        PanelDb::open(&self.path).unwrap()
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

fn snapshot(connection: &Connection, sql: &str) -> Vec<Vec<SqlValue>> {
    let mut statement = connection.prepare(sql).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns).map(|column| row.get(column)).collect()
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn retirement_removes_only_gotify_and_preserves_bark_delivery_and_private_state() {
    let fixture = Fixture::new();
    let db = fixture.open();
    let mut config = Config::for_platform_kind_with_home(PlatformKind::Linux, &fixture.root);
    config.probe.notifications.enabled = true;
    db.sync_notification_channels(&config).unwrap();
    db.set_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING, BARK_KEY)
        .unwrap();
    db.set_secret_setting_bytes("probe_gotify_token", b"fixture-retired-gotify-token")
        .unwrap();
    let retired_secret = db.get_setting("probe_gotify_token").unwrap().unwrap();
    let api_key = db.rotate_admin_api_key().unwrap();
    db.set_setting("probe_error_monitor_cursor", "fixture-cursor:42")
        .unwrap();
    db.set_setting("probe_async_question_baseline_ms", "123456")
        .unwrap();
    db.set_setting(
        "fixture_unrelated_setting",
        "gotify is ordinary fixture text",
    )
    .unwrap();

    for status in [
        "pending",
        "delivering",
        "sent",
        "skipped",
        "failed",
        "unknown",
        "legacy",
    ] {
        let key = format!("fixture-bark-{status}");
        assert!(db
            .stage_notification_delivery(
                NotificationChannel::Bark,
                &key,
                123456,
                &json!({"request":{"title":"fixture","body":"retained encrypted Bark body"}}),
            )
            .unwrap());
        fixture.connection().execute(
            "UPDATE probe_notification_deliveries SET status=?1,attempts=2,next_ms=345678,created_ms=123456,updated_ms=234567,outcome_json=?2 WHERE event_key=?3",
            params![status, json!({"reason":"fixture-preserved"}).to_string(), key],
        ).unwrap();
    }
    let connection = fixture.connection();
    connection.execute_batch(
        "UPDATE probe_notification_channels SET activated_ms=123456 WHERE channel='bark';
         INSERT INTO probe_notification_channels VALUES('gotify',1,234567);
         INSERT INTO native_probe_providers VALUES('claude_code',1,123456,234567,1,0);
         INSERT INTO native_probe_streams VALUES('claude_code','fixture-stream','fixture-identity',37,234567);
         INSERT INTO probe_dedupe VALUES('fixture-completion','fixture-event',9999999999,123);",
    ).unwrap();
    connection.execute("INSERT INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at) VALUES('fixture-native-event','claude_code',?1,'sent',123,234)",
        [json!({"event_key":"fixture-native-event","provider":"claude_code","session_key":"fixture-stream","thread_id":"fixture-thread","title":"","stream_identity":"fixture-identity","event":{"position":37,"turn_id":"fixture-turn","kind":"completion","body":"","timestamp_ms":1}}).to_string()]).unwrap();
    for status in ["pending", "delivering", "sent", "failed", "unknown"] {
        connection.execute(
            "INSERT INTO probe_notification_deliveries(event_key,channel,ciphertext,nonce,status,outcome_json,attempts,created_ms,updated_ms,next_ms) VALUES(?1,'gotify',?2,?3,?4,?5,2,123,234,0)",
            params![format!("fixture-gotify-{status}"), RETIRED_CIPHERTEXT, b"fixture-nonce".as_slice(), status, json!({"reason":RETIRED_DETAIL}).to_string()],
        ).unwrap();
    }
    let retained_event = json!({
        "bark":{"sent":true,"request_count":1},
        "dedupe":{"namespace":"fixture-completion"},
        "notification_event_key":"fixture-bark-sent",
        "other":{"gotify":"fixture-unrelated-nested-value"},
        "provider":"codex"
    });
    let mut legacy_event = retained_event.clone();
    legacy_event["gotify"] = json!({"sent":false,"reason":RETIRED_DETAIL});
    for (id, payload) in [
        ("fixture-notification-event", legacy_event.to_string()),
        (
            "fixture-null-event",
            json!({"bark":{"sent":false},"gotify":null}).to_string(),
        ),
        ("fixture-malformed-event", "fixture-invalid-json".into()),
    ] {
        connection.execute(
            "INSERT INTO probe_events(id,kind,title,message,dedupe_key,source,payload_json,created_at) VALUES(?1,'completion','fixture title','fixture body',?1,'fixture',?2,123)",
            params![id,payload],
        ).unwrap();
    }
    for (id, kind, detail) in [
        ("fixture-retired-job", "probe_gotify_test", RETIRED_DETAIL),
        ("fixture-bark-job", "probe_bark_test", "fixture Bark output"),
        (
            "fixture-other-job",
            "fixture_other",
            "fixture unrelated output",
        ),
    ] {
        connection.execute(
            "INSERT INTO jobs(id,kind,status,title,started_at,output) VALUES(?1,?2,'succeeded','fixture job',123,?3)",
            params![id,kind,detail],
        ).unwrap();
    }
    for (action, detail) in [
        ("probe_gotify_test", RETIRED_DETAIL),
        ("probe.gotifyTest", RETIRED_DETAIL),
        ("probe_bark_test", "fixture Bark audit"),
        ("fixture.unrelated", "fixture unrelated audit"),
        ("probeXgotify", "fixture unrelated similarly named audit"),
    ] {
        connection
            .execute(
                "INSERT INTO audit_log(action,detail_json,created_at) VALUES(?1,?2,123)",
                params![action, json!({"detail":detail}).to_string()],
            )
            .unwrap();
    }
    let queries = [
        "SELECT * FROM probe_notification_channels WHERE channel='bark'",
        "SELECT * FROM probe_notification_deliveries WHERE channel='bark' ORDER BY event_key",
        "SELECT * FROM settings WHERE key<>'probe_gotify_token' ORDER BY key",
        "SELECT * FROM native_probe_providers ORDER BY provider",
        "SELECT * FROM native_probe_streams ORDER BY provider,session_key",
        "SELECT * FROM native_probe_deliveries ORDER BY event_key",
        "SELECT * FROM probe_dedupe ORDER BY namespace,dedupe_key",
        "SELECT * FROM jobs WHERE kind<>'probe_gotify_test' ORDER BY id",
        "SELECT * FROM audit_log WHERE action NOT GLOB 'probe_gotify_*' AND action<>'probe.gotifyTest' ORDER BY id",
        "SELECT id,kind,thread_id,title,message,dedupe_key,source,created_at,handled_at FROM probe_events ORDER BY id",
    ];
    let before: Vec<_> = queries
        .iter()
        .map(|sql| snapshot(&connection, sql))
        .collect();
    connection.pragma_update(None, "user_version", 0).unwrap();
    drop(connection);
    drop(db);

    for _ in 0..2 {
        let db = fixture.open();
        let connection = fixture.connection();
        for (sql, expected) in queries.iter().zip(&before) {
            assert_eq!(&snapshot(&connection, sql), expected, "{sql}");
        }
        assert!(db.get_setting("probe_gotify_token").unwrap().is_none());
        assert!(db
            .get_setting("gotify_retirement_pending")
            .unwrap()
            .is_none());
        assert_eq!(
            db.get_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING)
                .unwrap()
                .as_deref(),
            Some(BARK_KEY)
        );
        assert!(db.verify_admin_api_key(&api_key).unwrap());
        for sql in [
            "SELECT count(*) FROM probe_notification_channels WHERE channel='gotify'",
            "SELECT count(*) FROM probe_notification_deliveries WHERE channel='gotify'",
            "SELECT count(*) FROM jobs WHERE kind='probe_gotify_test'",
            "SELECT count(*) FROM audit_log WHERE action GLOB 'probe_gotify_*' OR action='probe.gotifyTest'",
        ] {
            assert_eq!(connection.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap(), 0, "{sql}");
        }
        for (id, expected) in [
            ("fixture-notification-event", retained_event.clone()),
            ("fixture-null-event", json!({"bark":{"sent":false}})),
        ] {
            let payload: String = connection
                .query_row(
                    "SELECT payload_json FROM probe_events WHERE id=?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(), expected);
        }
        let malformed: String = connection
            .query_row(
                "SELECT payload_json FROM probe_events WHERE id='fixture-malformed-event'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(malformed, "fixture-invalid-json");
        for path in [&fixture.path, &fixture.path.with_extension("sqlite-wal")] {
            let bytes = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("cannot read fixture database: {error}"),
            };
            for marker in [
                RETIRED_CIPHERTEXT,
                RETIRED_DETAIL.as_bytes(),
                retired_secret.as_bytes(),
            ] {
                assert!(!bytes.windows(marker.len()).any(|window| window == marker));
            }
        }
    }
}

#[test]
fn an_interrupted_retirement_marker_is_finished_idempotently() {
    let fixture = Fixture::new();
    let db = fixture.open();
    db.set_setting("gotify_retirement_pending", "true").unwrap();
    db.set_setting("fixture_keep", "unchanged").unwrap();
    fixture
        .connection()
        .pragma_update(None, "user_version", 0)
        .unwrap();
    drop(db);
    for _ in 0..2 {
        let db = fixture.open();
        assert!(db
            .get_setting("gotify_retirement_pending")
            .unwrap()
            .is_none());
        assert_eq!(
            db.get_setting("fixture_keep").unwrap().as_deref(),
            Some("unchanged")
        );
    }
}

#[test]
fn old_nested_quoted_and_inline_toml_remove_gotify_preserving_other_values_and_comments() {
    for text in [
        "# fixture header\n[probe.notifications]\nenabled = true # fixture Bark comment\nserver_url = 'https://bark.example.invalid'\ngroup = 'fixture group'\n[probe.notifications.gotify]\nenabled = true\nserver_url = 'https://notify.example.invalid'\npriority = 7\n",
        "# fixture header\n['probe'.'notifications']\nenabled = true # fixture Bark comment\nserver_url = 'https://bark.example.invalid'\ngroup = 'fixture group'\n['probe'.'notifications'.'gotify']\nenabled = true\npriority = 7\n",
        "# fixture header\n[probe]\nnotifications = { enabled = true, server_url = 'https://bark.example.invalid', group = 'fixture group', gotify = { enabled = true, priority = 7 } } # fixture Bark comment\n",
        "# fixture header\nprobe = { enabled = true, notifications = { enabled = true, server_url = 'https://bark.example.invalid', group = 'fixture group', gotify = { enabled = true, priority = 7 } } } # fixture Bark comment\n",
    ] {
        let fixture = Fixture::new();
        let path = fixture.root.join("config.toml");
        let defaults = Config::for_platform_kind_with_home(PlatformKind::Linux, &fixture.root);
        let mut base = toml::Value::try_from(defaults).unwrap();
        base.as_table_mut().unwrap().remove("probe");
        let base = toml::to_string_pretty(&base).unwrap();
        let text = format!("{text}\n{base}\n# fixture other comment\n[fixture]\nvalue = 'gotify remains ordinary text'\n");
        let mut expected: toml::Value = toml::from_str(&text).unwrap();
        expected["probe"]["notifications"].as_table_mut().unwrap().remove("gotify");
        fs::write(&path, &text).unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.probe.notifications.enabled);
        assert_eq!(config.probe.notifications.server_url, "https://bark.example.invalid");
        assert_eq!(config.probe.notifications.group, "fixture group");
        let migrated = fs::read_to_string(&path).unwrap();
        assert_eq!(toml::from_str::<toml::Value>(&migrated).unwrap(), expected);
        for comment in ["# fixture header", "# fixture Bark comment", "# fixture other comment"] {
            assert!(migrated.contains(comment));
        }
        Config::load(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), migrated);
    }
}

#[test]
fn retired_channel_has_no_serialized_settings_or_executable_command() {
    let fixture = Fixture::new();
    let config = Config::for_platform_kind_with_home(PlatformKind::Linux, &fixture.root);
    let view =
        serde_json::to_value(build_settings_view(&config, ProbeSecretState::Configured)).unwrap();
    assert!(view.get("gotify").is_none());
    assert_eq!(view["notifications"]["device_key_configured"], true);
    assert!(view["notifications"]["device_key"].is_null());
    assert!(serde_json::from_value::<NotificationChannel>(json!("gotify")).is_err());
    assert!(is_retired_command("probe.gotifyTest"));
    assert!(!ALLOWED_RPC_COMMANDS.contains(&"probe.gotifyTest"));
    assert!(ALLOWED_RPC_COMMANDS.contains(&"probe.barkTest"));
    let contract: Value =
        serde_json::from_str(include_str!("../../../contracts/agenthub-contract.json")).unwrap();
    assert!(contract["retiredActions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "probe.gotifyTest"));
    assert!(!contract["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["id"] == "probe.gotifyTest"));
}
