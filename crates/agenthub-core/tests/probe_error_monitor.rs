use agenthub_core::{
    crypto::SecretBox,
    db::{NewProbeErrorIncident, PanelDb},
    probe_error_monitor::{classify_codex_turn_error, scan_codex_turn_errors, CodexTurnErrorClass},
};
use rusqlite::{params, Connection};
use std::{fs, path::PathBuf, sync::Arc, thread};

const THREAD_ID: &str = "00000000-0000-4000-8000-000000000104";
const TURN_ID: &str = "00000000-0000-4000-8000-000000000105";
const CAPACITY_BODY: &str = "session_loop{thread_id=00000000-0000-4000-8000-000000000104}:submission_dispatch{otel.name=\"op.dispatch.user_input\" submission.id=\"00000000-0000-4000-8000-000000000105\" codex.op=\"user_input\"}:turn{otel.name=\"session_task.turn\" thread.id=00000000-0000-4000-8000-000000000104 turn.id=00000000-0000-4000-8000-000000000105 model=gpt-5.6-sol codex.turn.reasoning_effort=ultra}:session_task.run:run_turn: Turn error: Selected model is at capacity. Please try a different model.";

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "agenthub-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn create_logs_db(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            ts_nanos INTEGER NOT NULL,
            level TEXT NOT NULL,
            target TEXT NOT NULL,
            feedback_log_body TEXT,
            module_path TEXT,
            file TEXT,
            line INTEGER,
            thread_id TEXT,
            process_uuid TEXT,
            estimated_bytes INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_logs_ts ON logs(ts DESC, ts_nanos DESC, id DESC);",
    )
    .unwrap();
    conn
}

fn insert_log(conn: &Connection, ts: i64, target: &str, thread_id: Option<&str>, body: &str) {
    conn.execute(
        "INSERT INTO logs(ts, ts_nanos, level, target, feedback_log_body, thread_id)
         VALUES(?1, 1, 'INFO', ?2, ?3, ?4)",
        params![ts, target, body, thread_id],
    )
    .unwrap();
}

#[test]
fn codex_turn_error_capacity_fixture_extracts_exact_identity_and_classification() {
    let root = temp_dir("turn-error-capacity");
    let logs = root.join("logs_2.sqlite");
    let conn = create_logs_db(&logs);
    insert_log(
        &conn,
        100,
        "codex_core::session::turn",
        Some(THREAD_ID),
        "baseline",
    );
    let baseline = scan_codex_turn_errors(&logs, None, 100).unwrap();
    assert!(baseline.baseline_only);

    insert_log(
        &conn,
        101,
        "codex_core::session::turn",
        Some(THREAD_ID),
        CAPACITY_BODY,
    );
    let scan = scan_codex_turn_errors(&logs, Some(&baseline.cursor), 100).unwrap();

    assert!(!scan.baseline_only);
    assert_eq!(scan.rows_seen, 1);
    assert_eq!(scan.incidents.len(), 1);
    let incident = &scan.incidents[0];
    assert_eq!(incident.thread_id, THREAD_ID);
    assert_eq!(incident.turn_id, TURN_ID);
    assert_eq!(
        incident.classification,
        CodexTurnErrorClass::ServerOverloaded
    );
    assert_eq!(
        incident.summary,
        "Selected model is at capacity. Please try a different model."
    );
    assert_eq!(incident.error_sha256.len(), 64);
    assert_eq!(incident.incident_key.len(), 64);
    assert!(!incident.summary.contains("session_loop"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_turn_error_ignores_tools_quotes_and_missing_thread_identity() {
    let root = temp_dir("turn-error-negative");
    let logs = root.join("logs_2.sqlite");
    let conn = create_logs_db(&logs);
    insert_log(
        &conn,
        100,
        "codex_core::session::turn",
        Some(THREAD_ID),
        "baseline",
    );
    let baseline = scan_codex_turn_errors(&logs, None, 100).unwrap();

    for (offset, target) in [
        "codex_core::tools::mcp",
        "codex_core::tools::shell",
        "codex_core::plugins",
        "codex_http_client::transport",
    ]
    .into_iter()
    .enumerate()
    {
        insert_log(
            &conn,
            101 + offset as i64,
            target,
            Some(THREAD_ID),
            CAPACITY_BODY,
        );
    }
    insert_log(
        &conn,
        105,
        "codex_core::session::turn",
        Some(THREAD_ID),
        "diagnostic quoted text: Turn error: Selected model is at capacity.",
    );
    insert_log(&conn, 106, "codex_core::session::turn", None, CAPACITY_BODY);
    let scan = scan_codex_turn_errors(&logs, Some(&baseline.cursor), 100).unwrap();

    assert_eq!(scan.rows_seen, 6);
    assert!(scan.incidents.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_turn_error_redacts_sensitive_summary_before_hashing_or_storage() {
    let root = temp_dir("turn-error-redaction");
    let logs = root.join("logs_2.sqlite");
    let conn = create_logs_db(&logs);
    insert_log(
        &conn,
        100,
        "codex_core::session::turn",
        Some(THREAD_ID),
        "baseline",
    );
    let baseline = scan_codex_turn_errors(&logs, None, 100).unwrap();
    let body = CAPACITY_BODY.replace(
        "Selected model is at capacity. Please try a different model.",
        "OPENAI_API_KEY=sk-secret-token",
    );
    insert_log(
        &conn,
        101,
        "codex_core::session::turn",
        Some(THREAD_ID),
        &body,
    );

    let scan = scan_codex_turn_errors(&logs, Some(&baseline.cursor), 100).unwrap();

    assert_eq!(scan.incidents.len(), 1);
    assert_eq!(scan.incidents[0].summary, "[redacted sensitive line]");
    assert!(!scan.incidents[0].summary.contains("sk-secret"));
    assert_eq!(scan.incidents[0].error_sha256.len(), 64);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_turn_error_classifies_supported_terminal_error_families() {
    for (summary, expected) in [
        (
            "Selected model is at capacity. Please try a different model.",
            CodexTurnErrorClass::ServerOverloaded,
        ),
        (
            "unexpected status 429 Too Many Requests: rate limit exceeded",
            CodexTurnErrorClass::RateLimited,
        ),
        (
            "unexpected status 503 Service Unavailable",
            CodexTurnErrorClass::ServerError,
        ),
        (
            "stream disconnected before completion",
            CodexTurnErrorClass::TransportError,
        ),
        (
            "401 Unauthorized: authentication failed",
            CodexTurnErrorClass::AuthenticationError,
        ),
        (
            "request blocked by safety policy",
            CodexTurnErrorClass::PolicyError,
        ),
        (
            "400 Bad Request: unsupported parameter",
            CodexTurnErrorClass::InvalidRequest,
        ),
        (
            "provider returned an unrecognized terminal failure",
            CodexTurnErrorClass::Unknown,
        ),
    ] {
        assert_eq!(classify_codex_turn_error(summary), expected, "{summary}");
    }
}

#[test]
fn probe_error_monitor_uses_bounded_composite_cursor_batches() {
    let root = temp_dir("turn-error-batches");
    let logs = root.join("logs_2.sqlite");
    let conn = create_logs_db(&logs);
    insert_log(
        &conn,
        100,
        "codex_core::session::turn",
        Some(THREAD_ID),
        "baseline",
    );
    let baseline = scan_codex_turn_errors(&logs, None, 100).unwrap();
    for ts in [101, 101, 101] {
        insert_log(
            &conn,
            ts,
            "codex_core::session::turn",
            Some(THREAD_ID),
            CAPACITY_BODY,
        );
    }

    let first = scan_codex_turn_errors(&logs, Some(&baseline.cursor), 2).unwrap();
    assert_eq!(first.rows_seen, 2);
    assert_eq!(first.incidents.len(), 2);
    let second = scan_codex_turn_errors(&logs, Some(&first.cursor), 2).unwrap();
    assert_eq!(second.rows_seen, 1);
    assert_eq!(second.incidents.len(), 1);
    let exhausted = scan_codex_turn_errors(&logs, Some(&second.cursor), 2).unwrap();
    assert_eq!(exhausted.rows_seen, 0);
    assert!(exhausted.incidents.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_error_monitor_first_scan_and_replaced_database_only_establish_baseline() {
    let root = temp_dir("turn-error-baseline");
    let logs = root.join("logs_2.sqlite");
    let conn = create_logs_db(&logs);
    insert_log(
        &conn,
        100,
        "codex_core::session::turn",
        Some(THREAD_ID),
        CAPACITY_BODY,
    );
    let baseline = scan_codex_turn_errors(&logs, None, 100).unwrap();
    assert!(baseline.baseline_only);
    assert!(baseline.incidents.is_empty());
    drop(conn);

    fs::remove_file(&logs).unwrap();
    let replacement = create_logs_db(&logs);
    insert_log(
        &replacement,
        200,
        "codex_core::session::turn",
        Some(THREAD_ID),
        CAPACITY_BODY,
    );
    let replacement_baseline = scan_codex_turn_errors(&logs, None, 100).unwrap();
    let mut reused_identity_cursor = baseline.cursor.clone();
    reused_identity_cursor.database_identity =
        replacement_baseline.cursor.database_identity.clone();
    let replaced = scan_codex_turn_errors(&logs, Some(&reused_identity_cursor), 100).unwrap();
    assert!(replaced.baseline_only);
    assert!(replaced.incidents.is_empty());
    assert_eq!(
        replaced.cursor.database_identity,
        replacement_baseline.cursor.database_identity
    );
    assert_eq!(
        (
            replaced.cursor.ts,
            replaced.cursor.ts_nanos,
            replaced.cursor.id
        ),
        (200, 1, 1)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_error_monitor_incident_claim_is_permanent_and_deduplicated() {
    let root = temp_dir("turn-error-dedupe");
    let panel = root.join("agenthub.sqlite");
    let db = PanelDb::open_with_secret_box(
        &panel,
        SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let incident = NewProbeErrorIncident {
        incident_key: "incident-key".to_string(),
        source_ts: 100,
        source_ts_nanos: 1,
        source_row_id: 9,
        thread_id: THREAD_ID.to_string(),
        turn_id: TURN_ID.to_string(),
        classification: "server_overloaded".to_string(),
        error_sha256: "a".repeat(64),
        error_summary: "Selected model is at capacity.".to_string(),
    };

    assert!(db.claim_probe_error_incident(&incident).unwrap());
    assert!(!db.claim_probe_error_incident(&incident).unwrap());
    assert_eq!(db.probe_error_incident_count().unwrap(), 1);
    let stored = db
        .get_probe_error_incident("incident-key")
        .unwrap()
        .unwrap();
    assert_eq!(stored.thread_id, THREAD_ID);
    assert_eq!(stored.turn_id, TURN_ID);
    assert_eq!(stored.error_summary, "Selected model is at capacity.");
    assert_eq!(db.list_pending_probe_error_deliveries(10).unwrap().len(), 1);
    db.update_probe_error_incident_delivery("incident-key", Some("event-id"), "sent")
        .unwrap();
    assert!(db
        .list_pending_probe_error_deliveries(10)
        .unwrap()
        .is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_error_monitor_concurrent_database_claim_records_one_incident() {
    let root = temp_dir("turn-error-concurrent-claim");
    let panel = root.join("agenthub.sqlite");
    drop(
        PanelDb::open_with_secret_box(
            &panel,
            SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap(),
    );
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let panel = panel.clone();
        let barrier = barrier.clone();
        workers.push(thread::spawn(move || {
            let db = PanelDb::open_with_secret_box(
                &panel,
                SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap();
            let incident = NewProbeErrorIncident {
                incident_key: "concurrent-key".to_string(),
                source_ts: 100,
                source_ts_nanos: 1,
                source_row_id: 9,
                thread_id: THREAD_ID.to_string(),
                turn_id: TURN_ID.to_string(),
                classification: "server_overloaded".to_string(),
                error_sha256: "c".repeat(64),
                error_summary: "capacity".to_string(),
            };
            barrier.wait();
            db.claim_probe_error_incident(&incident).unwrap()
        }));
    }
    barrier.wait();
    let claimed = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|claimed| *claimed)
        .count();
    assert_eq!(claimed, 1);
    let db = PanelDb::open_with_secret_box(
        &panel,
        SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    assert_eq!(db.probe_error_incident_count().unwrap(), 1);
    fs::remove_dir_all(root).unwrap();
}
