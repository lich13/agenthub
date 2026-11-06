use agenthub_core::{
    archive::{
        hidden_scan::{self, HiddenScanMode, HiddenScanPhase, HiddenScanRequest},
        HiddenThreadDeletePlan,
    },
    codex::CodexPaths,
};
use rusqlite::{params, Connection};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

const PAGE_SIZE: usize = 2;
static SCAN_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn hidden_scan_paginates_repeatably_and_keeps_its_candidate_snapshot() {
    let _guard = SCAN_TEST_LOCK.lock().await;
    let mut fixture = HiddenScanFixture::new(&[
        RolloutState::Complete,
        RolloutState::Complete,
        RolloutState::Complete,
        RolloutState::Complete,
        RolloutState::Complete,
    ]);
    let owner = format!("regression-owner-{}", Uuid::new_v4());
    let initial_ids = fixture.hidden_ids.iter().cloned().collect::<BTreeSet<_>>();

    let started = hidden_scan::request(
        fixture.paths(),
        owner.clone(),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(PAGE_SIZE),
            ..HiddenScanRequest::default()
        },
    )
    .await
    .expect("start hidden scan");
    let scan_id = started
        .scan
        .as_ref()
        .expect("start response includes scan progress")
        .id
        .clone();

    let first = poll_until_terminal(&fixture, &owner, &scan_id, 0, PAGE_SIZE).await;
    let first_progress = first.scan.as_ref().expect("first page progress");
    assert_eq!(first_progress.phase, HiddenScanPhase::Complete);
    assert_eq!(first_progress.processed, initial_ids.len());
    assert_eq!(first_progress.total, initial_ids.len() as u64);
    assert_eq!(first_progress.cursor, 0);
    assert_eq!(first_progress.next_cursor, PAGE_SIZE);
    assert!(first_progress.has_more);
    assert_eq!(first.candidates.len(), PAGE_SIZE);

    let first_ids = candidate_ids(&first);
    let repeated_first = poll_page(&fixture, &owner, &scan_id, 0, PAGE_SIZE).await;
    assert_eq!(candidate_ids(&repeated_first), first_ids);
    assert_eq!(
        repeated_first.scan.as_ref().unwrap().next_cursor,
        first_progress.next_cursor
    );

    let second_cursor = first_progress.next_cursor;
    let second = poll_page(&fixture, &owner, &scan_id, second_cursor, PAGE_SIZE).await;
    let second_progress = second.scan.as_ref().expect("second page progress");
    assert_eq!(second_progress.cursor, second_cursor);
    assert_eq!(second_progress.next_cursor, second_cursor + PAGE_SIZE);
    assert!(second_progress.has_more);
    assert_eq!(second.candidates.len(), PAGE_SIZE);
    let repeated_second = poll_page(&fixture, &owner, &scan_id, second_cursor, PAGE_SIZE).await;
    assert_eq!(candidate_ids(&repeated_second), candidate_ids(&second));

    let last_cursor = second_progress.next_cursor;
    let last = poll_page(&fixture, &owner, &scan_id, last_cursor, PAGE_SIZE).await;
    let last_progress = last.scan.as_ref().expect("last page progress");
    assert_eq!(last_progress.cursor, last_cursor);
    assert_eq!(last_progress.next_cursor, initial_ids.len());
    assert!(!last_progress.has_more);
    assert_eq!(last.candidates.len(), 1);

    let all_ids = first_ids
        .into_iter()
        .chain(candidate_ids(&second))
        .chain(candidate_ids(&last))
        .collect::<Vec<_>>();
    assert_eq!(
        all_ids.iter().cloned().collect::<BTreeSet<_>>(),
        initial_ids
    );
    assert_eq!(all_ids.len(), initial_ids.len());

    let late_id = fixture.add_hidden(RolloutState::Complete);
    let repeated_last = poll_page(&fixture, &owner, &scan_id, last_cursor, PAGE_SIZE).await;
    assert_eq!(candidate_ids(&repeated_last), candidate_ids(&last));
    assert_eq!(
        repeated_last.scan.as_ref().unwrap().processed,
        initial_ids.len()
    );
    assert_eq!(
        repeated_last.scan.as_ref().unwrap().total,
        initial_ids.len() as u64
    );
    assert!(!candidate_ids(&repeated_last).contains(&late_id));
}

#[tokio::test]
async fn hidden_scan_rejects_other_owners_and_sources_and_stops_for_its_owner() {
    let _guard = SCAN_TEST_LOCK.lock().await;
    let fixture = HiddenScanFixture::new(&[RolloutState::Complete]);
    let other_source = HiddenScanFixture::new(&[RolloutState::Complete]);
    let owner = format!("authorized-owner-{}", Uuid::new_v4());
    let scan_id = hidden_scan::request(
        fixture.paths(),
        owner.clone(),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(PAGE_SIZE),
            ..HiddenScanRequest::default()
        },
    )
    .await
    .expect("start owner-scoped scan")
    .scan
    .expect("start response includes scan progress")
    .id;

    assert!(
        hidden_scan::request(
            fixture.paths(),
            format!("unauthorized-owner-{}", Uuid::new_v4()),
            poll_request(&scan_id, 0, PAGE_SIZE),
        )
        .await
        .is_err(),
        "another owner cannot poll the scan"
    );
    assert!(
        hidden_scan::request(
            fixture.paths(),
            format!("unauthorized-owner-{}", Uuid::new_v4()),
            cancel_request(&scan_id, PAGE_SIZE),
        )
        .await
        .is_err(),
        "another owner cannot stop the scan"
    );
    assert!(
        hidden_scan::request(
            other_source.paths(),
            owner.clone(),
            poll_request(&scan_id, 0, PAGE_SIZE),
        )
        .await
        .is_err(),
        "another source cannot poll the scan"
    );
    assert!(
        hidden_scan::request(
            other_source.paths(),
            owner.clone(),
            cancel_request(&scan_id, PAGE_SIZE),
        )
        .await
        .is_err(),
        "another source cannot stop the scan"
    );

    let owned_poll = hidden_scan::request(
        fixture.paths(),
        owner.clone(),
        poll_request(&scan_id, 0, PAGE_SIZE),
    )
    .await
    .expect("authorized owner can still poll after rejected requests");
    assert!(!matches!(
        owned_poll.scan.as_ref().unwrap().phase,
        HiddenScanPhase::Stopping | HiddenScanPhase::Stopped
    ));

    let stopped = hidden_scan::request(
        fixture.paths(),
        owner.clone(),
        cancel_request(&scan_id, PAGE_SIZE),
    )
    .await
    .expect("authorized owner can stop the scan");
    assert!(matches!(
        stopped.scan.as_ref().unwrap().phase,
        HiddenScanPhase::Stopping | HiddenScanPhase::Stopped
    ));
    let stopped = poll_until_terminal(&fixture, &owner, &scan_id, 0, PAGE_SIZE).await;
    assert_eq!(
        stopped.scan.as_ref().unwrap().phase,
        HiddenScanPhase::Stopped
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hidden_scan_rejects_a_second_active_scan_for_the_same_source() {
    let _guard = SCAN_TEST_LOCK.lock().await;
    let fixture = HiddenScanFixture::new(&[RolloutState::Complete]);
    let owner = format!("same-source-owner-{}", Uuid::new_v4());
    let first = hidden_scan::request(
        fixture.paths(),
        owner.clone(),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(PAGE_SIZE),
            ..HiddenScanRequest::default()
        },
    )
    .await
    .expect("start first scan for source");
    let scan_id = first
        .scan
        .expect("start response includes scan progress")
        .id;

    // On a current-thread executor the newly spawned worker cannot run until
    // this test yields, so the source lease is still held by the first Start.
    let second = hidden_scan::request(
        fixture.paths(),
        format!("different-owner-{}", Uuid::new_v4()),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(PAGE_SIZE),
            ..HiddenScanRequest::default()
        },
    )
    .await;
    assert!(second.is_err(), "one source cannot have two active scans");

    hidden_scan::request(
        fixture.paths(),
        owner.clone(),
        cancel_request(&scan_id, PAGE_SIZE),
    )
    .await
    .expect("cancel first same-source scan");
    let stopped = poll_until_terminal(&fixture, &owner, &scan_id, 0, PAGE_SIZE).await;
    assert_eq!(
        stopped.scan.as_ref().unwrap().phase,
        HiddenScanPhase::Stopped
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hidden_scan_limits_active_contexts_to_two() {
    let _guard = SCAN_TEST_LOCK.lock().await;
    let first_fixture = HiddenScanFixture::new(&[RolloutState::Complete]);
    let second_fixture = HiddenScanFixture::new(&[RolloutState::Complete]);
    let third_fixture = HiddenScanFixture::new(&[RolloutState::Complete]);
    let first_owner = format!("slot-one-owner-{}", Uuid::new_v4());
    let second_owner = format!("slot-two-owner-{}", Uuid::new_v4());

    let first = hidden_scan::request(
        first_fixture.paths(),
        first_owner.clone(),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(PAGE_SIZE),
            ..HiddenScanRequest::default()
        },
    )
    .await
    .expect("start first independent scan");
    let first_id = first.scan.expect("first scan progress").id;
    let second = hidden_scan::request(
        second_fixture.paths(),
        second_owner.clone(),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(PAGE_SIZE),
            ..HiddenScanRequest::default()
        },
    )
    .await
    .expect("start second independent scan");
    let second_id = second.scan.expect("second scan progress").id;

    // Both permits are reserved synchronously. No worker can finish and be
    // evicted until this current-thread test yields to the runtime.
    let third = hidden_scan::request(
        third_fixture.paths(),
        format!("slot-three-owner-{}", Uuid::new_v4()),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(PAGE_SIZE),
            ..HiddenScanRequest::default()
        },
    )
    .await;
    assert!(third.is_err(), "only two active scan contexts are allowed");

    hidden_scan::request(
        first_fixture.paths(),
        first_owner.clone(),
        cancel_request(&first_id, PAGE_SIZE),
    )
    .await
    .expect("cancel first scan");
    hidden_scan::request(
        second_fixture.paths(),
        second_owner.clone(),
        cancel_request(&second_id, PAGE_SIZE),
    )
    .await
    .expect("cancel second scan");

    let first_stopped =
        poll_until_terminal(&first_fixture, &first_owner, &first_id, 0, PAGE_SIZE).await;
    let second_stopped =
        poll_until_terminal(&second_fixture, &second_owner, &second_id, 0, PAGE_SIZE).await;
    assert_eq!(
        first_stopped.scan.as_ref().unwrap().phase,
        HiddenScanPhase::Stopped
    );
    assert_eq!(
        second_stopped.scan.as_ref().unwrap().phase,
        HiddenScanPhase::Stopped
    );
}

#[tokio::test]
async fn hidden_scan_locates_corrupt_and_nonterminal_candidates_without_content_checks() {
    let _guard = SCAN_TEST_LOCK.lock().await;
    let fixture = HiddenScanFixture::new(&[
        RolloutState::Complete,
        RolloutState::Corrupt,
        RolloutState::NonTerminal,
    ]);
    let owner = format!("candidate-guard-owner-{}", Uuid::new_v4());
    let plan = scan_all(&fixture, &owner, PAGE_SIZE).await;
    let candidates = plan
        .candidates
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect::<HashMap<_, _>>();

    assert_eq!(plan.scan.as_ref().unwrap().phase, HiddenScanPhase::Complete);
    assert_eq!(plan.hidden_threads, 3);
    let complete = candidates
        .get(fixture.hidden_ids[0].as_str())
        .expect("complete candidate is present");
    assert!(complete.allowed);
    assert!(complete.fingerprint.is_some());

    let corrupt = candidates
        .get(fixture.hidden_ids[1].as_str())
        .expect("corrupt candidate is present");
    assert!(corrupt.allowed);
    assert!(corrupt.fingerprint.is_some());
    assert!(corrupt.reason.is_none());

    let nonterminal = candidates
        .get(fixture.hidden_ids[2].as_str())
        .expect("nonterminal candidate is present");
    assert!(nonterminal.allowed);
    assert!(nonterminal.fingerprint.is_some());
    assert!(nonterminal.reason.is_none());
}

async fn scan_all(
    fixture: &HiddenScanFixture,
    owner: &str,
    limit: usize,
) -> HiddenThreadDeletePlan {
    let started = hidden_scan::request(
        fixture.paths(),
        owner.to_owned(),
        HiddenScanRequest {
            mode: HiddenScanMode::Start,
            limit: Some(limit),
            ..HiddenScanRequest::default()
        },
    )
    .await
    .expect("start hidden scan");
    let scan_id = started
        .scan
        .expect("start response includes scan progress")
        .id;

    let mut cursor = 0;
    let mut candidates = Vec::new();
    loop {
        let page = poll_until_available_or_terminal(fixture, owner, &scan_id, cursor, limit).await;
        let (phase, next_cursor, has_more) = {
            let progress = page.scan.as_ref().expect("poll response includes progress");
            assert_ne!(
                progress.phase,
                HiddenScanPhase::Failed,
                "hidden scan failed: {:?}",
                progress.error
            );
            assert_ne!(
                progress.phase,
                HiddenScanPhase::Stopped,
                "hidden scan was stopped unexpectedly"
            );
            (progress.phase, progress.next_cursor, progress.has_more)
        };
        candidates.extend(page.candidates.iter().cloned());
        cursor = next_cursor;
        if phase == HiddenScanPhase::Complete && !has_more {
            let mut complete = page;
            complete.candidates = candidates;
            return complete;
        }
    }
}

async fn poll_until_terminal(
    fixture: &HiddenScanFixture,
    owner: &str,
    scan_id: &str,
    cursor: usize,
    limit: usize,
) -> HiddenThreadDeletePlan {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let page = poll_page(fixture, owner, scan_id, cursor, limit).await;
        let phase = page
            .scan
            .as_ref()
            .expect("poll response includes progress")
            .phase;
        if matches!(
            phase,
            HiddenScanPhase::Complete | HiddenScanPhase::Stopped | HiddenScanPhase::Failed
        ) {
            return page;
        }
        assert!(
            Instant::now() < deadline,
            "hidden scan did not finish in time"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn poll_until_available_or_terminal(
    fixture: &HiddenScanFixture,
    owner: &str,
    scan_id: &str,
    cursor: usize,
    limit: usize,
) -> HiddenThreadDeletePlan {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let page = poll_page(fixture, owner, scan_id, cursor, limit).await;
        let progress = page.scan.as_ref().expect("poll response includes progress");
        if progress.next_cursor > cursor
            || matches!(
                progress.phase,
                HiddenScanPhase::Complete | HiddenScanPhase::Stopped | HiddenScanPhase::Failed
            )
        {
            return page;
        }
        assert!(
            Instant::now() < deadline,
            "hidden scan made no progress in time"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn poll_page(
    fixture: &HiddenScanFixture,
    owner: &str,
    scan_id: &str,
    cursor: usize,
    limit: usize,
) -> HiddenThreadDeletePlan {
    hidden_scan::request(
        fixture.paths(),
        owner.to_owned(),
        poll_request(scan_id, cursor, limit),
    )
    .await
    .expect("poll hidden scan page")
}

fn poll_request(scan_id: &str, cursor: usize, limit: usize) -> HiddenScanRequest {
    HiddenScanRequest {
        mode: HiddenScanMode::Poll,
        scan_id: Some(scan_id.to_owned()),
        cursor: Some(cursor),
        limit: Some(limit),
    }
}

fn cancel_request(scan_id: &str, limit: usize) -> HiddenScanRequest {
    HiddenScanRequest {
        mode: HiddenScanMode::Cancel,
        scan_id: Some(scan_id.to_owned()),
        limit: Some(limit),
        ..HiddenScanRequest::default()
    }
}

fn candidate_ids(plan: &HiddenThreadDeletePlan) -> Vec<String> {
    plan.candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect()
}

#[derive(Clone, Copy)]
enum RolloutState {
    Complete,
    Corrupt,
    NonTerminal,
}

struct HiddenScanFixture {
    root: PathBuf,
    parent_id: String,
    hidden_ids: Vec<String>,
}

impl HiddenScanFixture {
    fn new(states: &[RolloutState]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-hidden-scan-regression-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("sessions")).expect("create anonymous fixture directories");
        let parent_id = Uuid::new_v4().to_string();
        let mut fixture = Self {
            root,
            parent_id: parent_id.clone(),
            hidden_ids: Vec::new(),
        };
        fixture.create_schema();
        fixture.add_thread(
            &parent_id,
            "user",
            None,
            1,
            "Synthetic visible parent",
            RolloutState::Complete,
        );
        for (number, state) in states.iter().copied().enumerate() {
            fixture.add_hidden_at(state, number as i64 + 2);
        }
        fixture
    }

    fn paths(&self) -> CodexPaths {
        CodexPaths::new(&self.root)
    }

    fn add_hidden(&mut self, state: RolloutState) -> String {
        let updated_at = self.hidden_ids.len() as i64 + 10_000;
        self.add_hidden_at(state, updated_at)
    }

    fn add_hidden_at(&mut self, state: RolloutState, updated_at: i64) -> String {
        let id = Uuid::new_v4().to_string();
        self.add_thread(
            &id,
            "subagent",
            Some(&self.parent_id),
            updated_at,
            &format!("Synthetic hidden candidate {updated_at}"),
            state,
        );
        self.hidden_ids.push(id.clone());
        id
    }

    fn create_schema(&self) {
        let conn = Connection::open(self.root.join("state_5.sqlite"))
            .expect("create fixture state database");
        conn.execute_batch(
            "CREATE TABLE threads(
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                thread_source TEXT NOT NULL,
                parent_thread_id TEXT,
                agent_path TEXT,
                agent_nickname TEXT,
                agent_role TEXT,
                has_user_event INTEGER NOT NULL,
                first_user_message TEXT NOT NULL,
                preview TEXT NOT NULL,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0,
                archived_at INTEGER
            );
            CREATE TABLE thread_dynamic_tools(thread_id TEXT NOT NULL);
            CREATE TABLE thread_spawn_edges(parent_thread_id TEXT NOT NULL, child_thread_id TEXT NOT NULL);
            CREATE TABLE agent_job_items(id TEXT PRIMARY KEY, assigned_thread_id TEXT, status TEXT NOT NULL);",
        )
        .expect("create fixture tables");
    }

    fn add_thread(
        &self,
        id: &str,
        thread_source: &str,
        parent_thread_id: Option<&str>,
        updated_at: i64,
        title: &str,
        state: RolloutState,
    ) {
        let rollout = self
            .root
            .join("sessions")
            .join(format!("rollout-{id}.jsonl"));
        write_rollout(&rollout, id, state);

        let conn = Connection::open(self.root.join("state_5.sqlite"))
            .expect("open fixture state database");
        conn.execute(
            "INSERT INTO threads(
                id, rollout_path, created_at, updated_at, source, thread_source,
                parent_thread_id, agent_path, agent_nickname, agent_role,
                has_user_event, first_user_message, preview, cwd, title,
                archived, archived_at
            ) VALUES(?1, ?2, 1, ?3, 'codex', ?4, ?5, NULL, NULL, NULL, ?6, ?7, ?7, ?8, ?9, 0, NULL)",
            params![
                id,
                rollout.to_string_lossy().as_ref(),
                updated_at,
                thread_source,
                parent_thread_id,
                if thread_source == "user" { 1_i64 } else { 0_i64 },
                if thread_source == "user" {
                    "Synthetic visible parent"
                } else {
                    "Synthetic subagent fixture history"
                },
                "/synthetic/agenthub-hidden-scan-fixture",
                title,
            ],
        )
        .expect("insert fixture thread row");

        let index = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("session_index.jsonl"))
            .expect("open fixture session index");
        let mut writer = std::io::BufWriter::new(index);
        serde_json::to_writer(
            &mut writer,
            &serde_json::json!({ "id": id, "path": rollout.to_string_lossy() }),
        )
        .expect("write fixture index entry");
        writer.write_all(b"\n").expect("finish fixture index entry");
        writer.flush().expect("flush fixture index entry");
    }
}

impl Drop for HiddenScanFixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!(
                "HIDDEN_SCAN_REGRESSION_CLEANUP_FAILED kind={:?}",
                error.kind()
            );
        }
    }
}

fn write_rollout(path: &Path, id: &str, state: RolloutState) {
    let turn_id = format!("turn-{id}");
    let header = serde_json::json!({ "type": "session_meta", "payload": { "id": id } });
    let started = serde_json::json!({ "type": "turn_started", "turn_id": turn_id });
    let complete = serde_json::json!({
        "type": "event_msg",
        "payload": { "type": "task_complete", "turn_id": turn_id }
    });
    let contents = match state {
        RolloutState::Complete => [
            header.to_string(),
            started.to_string(),
            complete.to_string(),
        ]
        .join("\n"),
        RolloutState::Corrupt => [header.to_string(), "{malformed".to_owned()].join("\n"),
        RolloutState::NonTerminal => [header.to_string(), started.to_string()].join("\n"),
    };
    fs::write(path, contents).expect("write synthetic rollout fixture");
}
