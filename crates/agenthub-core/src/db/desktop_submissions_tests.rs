use super::super::PanelDb;
use super::{
    DesktopLedger, Submission, MAX_BODY_BYTES, MAX_PENDING, MAX_RETAINED_BYTES, MAX_THREAD_PENDING,
};
use crate::{codex::desktop::references::NativeReference, crypto::SecretBox};
use base64::{engine::general_purpose, Engine as _};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    },
};

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-desktop-submissions-tests-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("panel.sqlite");
        Self { root, path }
    }

    fn ledger(&self) -> DesktopLedger {
        self.ledger_with_key(test_key(0x11))
    }

    fn ledger_with_key(&self, crypto: SecretBox) -> DesktopLedger {
        let db = PanelDb::open_with_secret_box(&self.path, crypto.clone()).unwrap();
        DesktopLedger::new(db, crypto)
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

fn test_key(byte: u8) -> SecretBox {
    let material = general_purpose::STANDARD.encode([byte; 32]);
    SecretBox::from_key_material(&material).unwrap()
}

fn source_id() -> String {
    "ab".repeat(32)
}

fn thread_id(number: u64) -> String {
    format!("10000000-0000-4000-8000-{number:012x}")
}

fn submission_id(number: u64) -> String {
    format!("20000000-0000-4000-8000-{number:012x}")
}

fn sample_body(value: &str) -> Value {
    json!({"text": value, "kind": "synthetic-fixture"})
}

fn record<'a>(records: &'a [Submission], id: &str) -> &'a Submission {
    records
        .iter()
        .find(|submission| submission.id == id)
        .unwrap_or_else(|| panic!("missing synthetic submission {id}"))
}

fn raw_count(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT count(*) FROM codex_desktop_submissions",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn encrypted_bodies_reject_wrong_keys_and_bind_ciphertext_to_record_identity() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(1);
    let first_id = submission_id(1);
    let second_id = submission_id(2);
    let first_body = sample_body("AGENTHUB_SYNTHETIC_SECRET_BODY_MARKER");

    ledger
        .enqueue(&source, &thread, &first_id, &first_body)
        .unwrap();
    ledger
        .enqueue(
            &source,
            &thread,
            &second_id,
            &sample_body("second synthetic body"),
        )
        .unwrap();

    let connection = fixture.connection();
    let (ciphertext, nonce): (Vec<u8>, Vec<u8>) = connection
        .query_row(
            "SELECT ciphertext,nonce FROM codex_desktop_submissions WHERE id=?1",
            [&first_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(nonce.len(), 12);
    assert!(!ciphertext
        .windows(b"AGENTHUB_SYNTHETIC_SECRET_BODY_MARKER".len())
        .any(|window| window == b"AGENTHUB_SYNTHETIC_SECRET_BODY_MARKER"));

    let wrong_key = fixture.ledger_with_key(test_key(0x22));
    assert!(wrong_key.list(&source, &thread).is_err());

    connection
        .execute(
            "UPDATE codex_desktop_submissions
             SET ciphertext=(SELECT ciphertext FROM codex_desktop_submissions WHERE id=?2),
                 nonce=(SELECT nonce FROM codex_desktop_submissions WHERE id=?2)
             WHERE id=?1",
            params![first_id, second_id],
        )
        .unwrap();
    assert!(ledger.list(&source, &thread).is_err());
}

#[test]
fn enqueue_is_idempotent_for_identical_identity_and_rejects_changed_content() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(2);
    let id = submission_id(10);
    let body = sample_body("same synthetic content");

    ledger.enqueue(&source, &thread, &id, &body).unwrap();
    ledger.enqueue(&source, &thread, &id, &body).unwrap();
    assert_eq!(raw_count(&fixture.connection()), 1);
    assert!(ledger
        .enqueue(
            &source,
            &thread,
            &id,
            &sample_body("changed synthetic content")
        )
        .is_err());
    assert_eq!(raw_count(&fixture.connection()), 1);
}

#[test]
fn only_the_first_queued_record_can_be_claimed_and_concurrent_claims_are_unique() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(3);
    let first_id = submission_id(20);
    let second_id = submission_id(21);
    ledger
        .enqueue(&source, &thread, &first_id, &sample_body("first"))
        .unwrap();
    ledger
        .enqueue(&source, &thread, &second_id, &sample_body("second"))
        .unwrap();

    let ordered = ledger.list(&source, &thread).unwrap();
    assert_eq!(ordered.len(), 2);
    let first = ordered[0].id.clone();
    let second = ordered[1].id.clone();
    assert!(!ledger.claim(&source, &thread, &second).unwrap());

    const WORKERS: usize = 12;
    let barrier = Arc::new(Barrier::new(WORKERS + 1));
    let wins = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for _ in 0..WORKERS {
        let worker_ledger = ledger.clone();
        let source = source.clone();
        let thread = thread.clone();
        let first = first.clone();
        let barrier = Arc::clone(&barrier);
        let wins = Arc::clone(&wins);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            if worker_ledger.claim(&source, &thread, &first).unwrap() {
                wins.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }

    assert_eq!(wins.load(Ordering::SeqCst), 1);
    assert!(!ledger.claim(&source, &thread, &second).unwrap());
}

#[test]
fn list_and_claim_keep_insert_order_when_timestamps_match_and_ids_sort_in_reverse() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(30);
    let inserted_first = submission_id(902);
    let inserted_second = submission_id(901);

    ledger
        .enqueue(
            &source,
            &thread,
            &inserted_first,
            &sample_body("inserted first"),
        )
        .unwrap();
    ledger
        .enqueue(
            &source,
            &thread,
            &inserted_second,
            &sample_body("inserted second"),
        )
        .unwrap();

    fixture
        .connection()
        .execute(
            "UPDATE codex_desktop_submissions SET created_at=123456,updated_at=123456
             WHERE id IN (?1,?2)",
            params![inserted_first, inserted_second],
        )
        .unwrap();

    let ordered = ledger.list(&source, &thread).unwrap();
    assert_eq!(ordered.len(), 2);
    assert_eq!(ordered[0].id, inserted_first);
    assert_eq!(ordered[1].id, inserted_second);
    assert!(!ledger.claim(&source, &thread, &inserted_second).unwrap());
    assert!(ledger.claim(&source, &thread, &inserted_first).unwrap());
}

#[test]
fn dispatching_and_unknown_outcomes_block_later_claims_and_unknown_cannot_be_cancelled() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(4);
    let first_id = submission_id(30);
    let second_id = submission_id(31);
    ledger
        .enqueue(&source, &thread, &first_id, &sample_body("first"))
        .unwrap();
    ledger
        .enqueue(&source, &thread, &second_id, &sample_body("second"))
        .unwrap();

    let ordered = ledger.list(&source, &thread).unwrap();
    let first = ordered[0].id.clone();
    let second = ordered[1].id.clone();
    assert!(ledger.claim(&source, &thread, &first).unwrap());
    assert!(!ledger.claim(&source, &thread, &second).unwrap());
    assert!(!ledger.cancel(&source, &thread, &first).unwrap());

    ledger.outcome_unknown(&source, &thread, &first).unwrap();
    assert!(!ledger.claim(&source, &thread, &second).unwrap());
    assert!(!ledger.cancel(&source, &thread, &first).unwrap());
    assert_eq!(
        record(&ledger.list(&source, &thread).unwrap(), &first).status,
        "outcome_unknown"
    );
}

#[test]
fn restart_holds_queued_records_and_marks_dispatching_records_unknown() {
    let fixture = Fixture::new();
    let source = source_id();
    let thread = thread_id(5);
    let first_id = submission_id(40);
    let second_id = submission_id(41);
    let ledger = fixture.ledger();
    ledger
        .enqueue(&source, &thread, &first_id, &sample_body("dispatching"))
        .unwrap();
    ledger
        .enqueue(&source, &thread, &second_id, &sample_body("queued"))
        .unwrap();
    let first = ledger.list(&source, &thread).unwrap()[0].id.clone();
    assert!(ledger.claim(&source, &thread, &first).unwrap());
    drop(ledger);

    let restarted = fixture.ledger();
    restarted.recover(&source).unwrap();
    let records = restarted.list(&source, &thread).unwrap();
    assert_eq!(record(&records, &first).status, "outcome_unknown");
    let queued = records
        .iter()
        .find(|submission| submission.id != first)
        .unwrap();
    assert_eq!(queued.status, "held");
    assert!(!restarted.claim(&source, &thread, &queued.id).unwrap());
}

#[test]
fn accepted_records_clear_encrypted_body_and_cannot_be_cancelled() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(6);
    let id = submission_id(50);
    ledger
        .enqueue(
            &source,
            &thread,
            &id,
            &sample_body("accepted synthetic body"),
        )
        .unwrap();
    assert!(ledger.claim(&source, &thread, &id).unwrap());
    assert!(ledger
        .accepted(&source, &thread, &id, "turn-fixture-accepted")
        .unwrap());
    assert!(!ledger.cancel(&source, &thread, &id).unwrap());
    assert!(ledger.list(&source, &thread).unwrap().is_empty());

    let connection = fixture.connection();
    let (status, turn_id, ciphertext, nonce) = connection
        .query_row(
            "SELECT status,turn_id,ciphertext,nonce
                 FROM codex_desktop_submissions WHERE id=?1",
            [&id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(status, "accepted");
    assert_eq!(turn_id.as_deref(), Some("turn-fixture-accepted"));
    assert!(ciphertext.is_none());
    assert!(nonce.is_none());
}

#[test]
fn hold_and_resume_control_unsent_records_and_only_unsent_records_can_be_cancelled() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(7);
    let first_id = submission_id(60);
    let second_id = submission_id(61);
    ledger
        .enqueue(&source, &thread, &first_id, &sample_body("first"))
        .unwrap();
    ledger
        .enqueue(&source, &thread, &second_id, &sample_body("second"))
        .unwrap();

    let ordered = ledger.list(&source, &thread).unwrap();
    let first = ordered[0].id.clone();
    let second = ordered[1].id.clone();
    ledger.hold(&source, &thread).unwrap();
    for submission in ledger.list(&source, &thread).unwrap() {
        assert_eq!(submission.status, "held");
    }
    assert!(!ledger.claim(&source, &thread, &first).unwrap());
    assert!(ledger.cancel(&source, &thread, &first).unwrap());

    ledger.resume(&source, &thread).unwrap();
    assert_eq!(
        record(&ledger.list(&source, &thread).unwrap(), &second).status,
        "queued"
    );
    assert!(ledger.claim(&source, &thread, &second).unwrap());
    assert!(!ledger.cancel(&source, &thread, &second).unwrap());
    ledger.not_sent(&source, &thread, &second).unwrap();
    assert_eq!(
        record(&ledger.list(&source, &thread).unwrap(), &second).status,
        "held"
    );
    assert!(ledger.cancel(&source, &thread, &second).unwrap());
    assert!(ledger.list(&source, &thread).unwrap().is_empty());
}

#[test]
fn hold_queued_only_changes_the_matching_queued_record() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let queued_thread = thread_id(16);
    let dispatching_thread = thread_id(17);
    let held_thread = thread_id(18);
    let unknown_thread = thread_id(19);
    let accepted_thread = thread_id(20);
    let queued_id = submission_id(160);
    let dispatching_id = submission_id(161);
    let held_id = submission_id(162);
    let unknown_id = submission_id(163);
    let accepted_id = submission_id(164);

    for (thread, id) in [
        (&queued_thread, &queued_id),
        (&dispatching_thread, &dispatching_id),
        (&held_thread, &held_id),
        (&unknown_thread, &unknown_id),
        (&accepted_thread, &accepted_id),
    ] {
        ledger
            .enqueue(
                &source,
                thread,
                id,
                &sample_body("synthetic status fixture"),
            )
            .unwrap();
    }
    assert!(ledger
        .claim_immediate(&source, &dispatching_thread, &dispatching_id)
        .unwrap());
    ledger.hold(&source, &held_thread).unwrap();
    assert!(ledger
        .claim_immediate(&source, &unknown_thread, &unknown_id)
        .unwrap());
    ledger
        .outcome_unknown(&source, &unknown_thread, &unknown_id)
        .unwrap();
    assert!(ledger
        .claim_immediate(&source, &accepted_thread, &accepted_id)
        .unwrap());
    assert!(ledger
        .accepted(
            &source,
            &accepted_thread,
            &accepted_id,
            "turn-fixture-accepted"
        )
        .unwrap());

    ledger
        .hold_queued(&source, &thread_id(99), &queued_id)
        .unwrap();
    assert_eq!(
        record(&ledger.list(&source, &queued_thread).unwrap(), &queued_id).status,
        "queued"
    );
    ledger
        .hold_queued(&source, &queued_thread, &queued_id)
        .unwrap();
    ledger
        .hold_queued(&source, &dispatching_thread, &dispatching_id)
        .unwrap();
    ledger.hold_queued(&source, &held_thread, &held_id).unwrap();
    ledger
        .hold_queued(&source, &unknown_thread, &unknown_id)
        .unwrap();
    ledger
        .hold_queued(&source, &accepted_thread, &accepted_id)
        .unwrap();

    assert_eq!(
        record(&ledger.list(&source, &queued_thread).unwrap(), &queued_id).status,
        "held"
    );
    assert_eq!(
        record(
            &ledger.list(&source, &dispatching_thread).unwrap(),
            &dispatching_id
        )
        .status,
        "dispatching"
    );
    assert_eq!(
        record(&ledger.list(&source, &held_thread).unwrap(), &held_id).status,
        "held"
    );
    assert_eq!(
        record(&ledger.list(&source, &unknown_thread).unwrap(), &unknown_id).status,
        "outcome_unknown"
    );
    assert!(ledger.list(&source, &accepted_thread).unwrap().is_empty());

    let accepted_status: String = fixture
        .connection()
        .query_row(
            "SELECT status FROM codex_desktop_submissions WHERE id=?1",
            [&accepted_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(accepted_status, "accepted");
}

#[test]
fn resume_selected_requeues_only_the_listed_held_records() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(21);
    let first_id = submission_id(170);
    let second_id = submission_id(171);
    let third_id = submission_id(172);
    for id in [&first_id, &second_id, &third_id] {
        ledger
            .enqueue(
                &source,
                &thread,
                id,
                &sample_body("synthetic selected-resume fixture"),
            )
            .unwrap();
    }
    ledger.hold(&source, &thread).unwrap();

    ledger
        .resume_selected(&source, &thread, std::slice::from_ref(&second_id))
        .unwrap();
    let records = ledger.list(&source, &thread).unwrap();
    assert_eq!(record(&records, &first_id).status, "held");
    assert_eq!(record(&records, &second_id).status, "queued");
    assert_eq!(record(&records, &third_id).status, "held");
}

#[test]
fn dismiss_unknown_clears_only_the_matching_uncertain_record() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(22);
    let uncertain_id = submission_id(180);
    let queued_id = submission_id(181);
    ledger
        .enqueue(
            &source,
            &thread,
            &uncertain_id,
            &sample_body("synthetic uncertain fixture"),
        )
        .unwrap();
    ledger
        .enqueue(
            &source,
            &thread,
            &queued_id,
            &sample_body("synthetic queued fixture"),
        )
        .unwrap();
    assert!(ledger
        .claim_immediate(&source, &thread, &uncertain_id)
        .unwrap());
    ledger
        .outcome_unknown(&source, &thread, &uncertain_id)
        .unwrap();

    assert!(ledger
        .dismiss_unknown(&source, &thread, &uncertain_id)
        .unwrap());
    assert!(!ledger
        .dismiss_unknown(&source, &thread, &uncertain_id)
        .unwrap());
    assert!(!ledger
        .dismiss_unknown(&source, &thread, &queued_id)
        .unwrap());
    let records = ledger.list(&source, &thread).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, queued_id);
    assert_eq!(records[0].status, "queued");

    let (status, ciphertext, nonce): (String, Option<Vec<u8>>, Option<Vec<u8>>) = fixture
        .connection()
        .query_row(
            "SELECT status,ciphertext,nonce FROM codex_desktop_submissions WHERE id=?1",
            [&uncertain_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "dismissed");
    assert!(ciphertext.is_none());
    assert!(nonce.is_none());
}

#[test]
fn encrypted_reference_body_is_recovered_and_held_after_restart() {
    let fixture = Fixture::new();
    let source = source_id();
    let thread = thread_id(23);
    let id = submission_id(190);
    let marker = "AGENTHUB_SYNTHETIC_REFERENCE_FIXTURE";
    let path = fixture.root.join(format!("{marker}.txt"));
    fs::write(&path, b"synthetic referenced file contents").unwrap();
    let reference = NativeReference::selected_file(path).unwrap();
    let body = json!({
        "kind":"queue",
        "text":"synthetic message",
        "references":[serde_json::to_value(&reference).unwrap()]
    });
    let plaintext = serde_json::to_vec(&body).unwrap();
    assert!(plaintext
        .windows(marker.len())
        .any(|window| window == marker.as_bytes()));

    let ledger = fixture.ledger();
    ledger.enqueue(&source, &thread, &id, &body).unwrap();
    let ciphertext: Vec<u8> = fixture
        .connection()
        .query_row(
            "SELECT ciphertext FROM codex_desktop_submissions WHERE id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!ciphertext
        .windows(marker.len())
        .any(|window| window == marker.as_bytes()));
    drop(ledger);

    let restarted = fixture.ledger();
    restarted.recover(&source).unwrap();
    let records = restarted.list(&source, &thread).unwrap();
    let restored = record(&records, &id);
    assert_eq!(restored.status, "held");
    let references: Vec<NativeReference> =
        serde_json::from_value(restored.body.as_ref().unwrap()["references"].clone()).unwrap();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].kind(), "file");
    references[0]
        .validate()
        .expect("recovered fixture reference retains its file identity");
}

#[test]
fn oversized_bodies_and_per_thread_pending_limit_are_enforced() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(8);

    let oversized = json!({"text": "x".repeat(MAX_BODY_BYTES)});
    assert!(ledger
        .enqueue(&source, &thread, &submission_id(100), &oversized)
        .is_err());
    assert_eq!(raw_count(&fixture.connection()), 0);

    for index in 0..MAX_THREAD_PENDING as u64 {
        ledger
            .enqueue(
                &source,
                &thread,
                &submission_id(101 + index),
                &sample_body("capacity fixture"),
            )
            .unwrap();
    }
    assert!(ledger
        .enqueue(
            &source,
            &thread,
            &submission_id(200),
            &sample_body("per-thread overflow"),
        )
        .is_err());
    assert_eq!(
        ledger.list(&source, &thread).unwrap().len(),
        MAX_THREAD_PENDING as usize
    );
}

#[test]
fn global_pending_limit_is_enforced_across_threads() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let threads = MAX_PENDING as usize / MAX_THREAD_PENDING as usize;
    let mut serial = 1_u64;

    for thread_number in 1..=threads as u64 {
        let thread = thread_id(thread_number);
        for _ in 0..MAX_THREAD_PENDING {
            ledger
                .enqueue(
                    &source,
                    &thread,
                    &submission_id(serial),
                    &sample_body("global capacity fixture"),
                )
                .unwrap();
            serial += 1;
        }
    }
    assert_eq!(raw_count(&fixture.connection()), MAX_PENDING);

    assert!(ledger
        .enqueue(
            &source,
            &thread_id(threads as u64 + 1),
            &submission_id(serial),
            &sample_body("global overflow"),
        )
        .is_err());
    assert_eq!(raw_count(&fixture.connection()), MAX_PENDING);
}

fn near_max_body(source: &str, thread: &str, id: &str) -> Value {
    let body_with_empty_payload = json!({"payload": ""});
    let envelope = json!({
        "source": source,
        "thread": thread,
        "id": id,
        "body": body_with_empty_payload,
    });
    let target_plaintext_len = MAX_BODY_BYTES - 64;
    let empty_len = serde_json::to_vec(&envelope).unwrap().len();
    let payload_len = target_plaintext_len - empty_len;
    json!({"payload": "x".repeat(payload_len)})
}

#[test]
fn retained_ciphertext_byte_limit_is_enforced() {
    let fixture = Fixture::new();
    let ledger = fixture.ledger();
    let source = source_id();
    let thread = thread_id(9);
    let mut next = 300_u64;
    let first_id = submission_id(next);
    let first_body = near_max_body(&source, &thread, &first_id);
    let envelope_len = serde_json::to_vec(&json!({
        "source": source,
        "thread": thread,
        "id": first_id,
        "body": first_body,
    }))
    .unwrap()
    .len();
    assert_eq!(envelope_len, MAX_BODY_BYTES - 64);

    for _ in 0..32 {
        let id = submission_id(next);
        let body = near_max_body(&source, &thread, &id);
        ledger.enqueue(&source, &thread, &id, &body).unwrap();
        next += 1;
    }

    let connection = fixture.connection();
    let (count, bytes): (i64, i64) = connection
        .query_row(
            "SELECT count(*),sum(length(ciphertext)) FROM codex_desktop_submissions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 32);
    assert!(bytes <= MAX_RETAINED_BYTES);
    assert!(bytes + (envelope_len as i64) + 16 > MAX_RETAINED_BYTES);

    assert!(ledger
        .enqueue(
            &source,
            &thread,
            &submission_id(next),
            &near_max_body(&source, &thread, &submission_id(next)),
        )
        .is_err());
    assert_eq!(raw_count(&connection), 32);
}
