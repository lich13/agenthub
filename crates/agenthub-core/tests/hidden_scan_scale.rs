use agenthub_core::{
    archive::hidden_scan::{self, HiddenScanMode, HiddenScanPhase, HiddenScanRequest},
    codex::CodexPaths,
};
use rusqlite::{params, Connection};
use std::{
    collections::BTreeSet,
    env, fs,
    io::{BufWriter, Write},
    mem::MaybeUninit,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "opt-in hidden scan scale baseline"]
async fn hidden_scan_scale_100() {
    run_hidden_scan_scale(100).await;
}

#[tokio::test]
#[ignore = "opt-in hidden scan scale baseline"]
async fn hidden_scan_scale_1000() {
    run_hidden_scan_scale(1_000).await;
}

#[tokio::test]
#[ignore = "opt-in hidden scan scale baseline"]
async fn hidden_scan_scale_5000() {
    run_hidden_scan_scale(5_000).await;
}

#[tokio::test]
#[ignore = "opt-in single large history memory and throughput check"]
async fn hidden_scan_scale_100m_history() {
    let fixture = HiddenScanFixture::new(1);
    let (history_bytes, payload_records) = fixture.write_large_history(100 * 1024 * 1024);
    run_hidden_scan_fixture(&fixture, 1, Some((history_bytes, payload_records))).await;
}

async fn run_hidden_scan_scale(hidden_count: usize) {
    let fixture = HiddenScanFixture::new(hidden_count);
    run_hidden_scan_fixture(&fixture, hidden_count, None).await;
}

async fn run_hidden_scan_fixture(
    fixture: &HiddenScanFixture,
    hidden_count: usize,
    large_history: Option<(u64, usize)>,
) {
    let started = Instant::now();

    let owner = format!("hidden-scan-scale-{}", Uuid::new_v4());
    let first = hidden_scan::request(
        fixture.paths(),
        owner.clone(),
        HiddenScanRequest {
            mode: Some(HiddenScanMode::Start),
            limit: Some(100),
            ..HiddenScanRequest::default()
        },
    )
    .await
    .expect("start synthetic hidden scan");
    let scan_id = first
        .scan
        .as_ref()
        .expect("start response includes scan progress")
        .id
        .clone();

    let mut plan = first;
    let mut cursor = 0;
    let mut candidates = Vec::with_capacity(hidden_count);
    let deadline = Instant::now() + Duration::from_secs(10 * 60);
    loop {
        let progress = plan.scan.as_ref().expect("scan response includes progress");
        assert_ne!(
            progress.phase,
            HiddenScanPhase::Failed,
            "synthetic scan failed: {:?}",
            progress.error
        );
        assert_ne!(
            progress.phase,
            HiddenScanPhase::Stopped,
            "synthetic scan stopped unexpectedly"
        );

        if progress.next_cursor > cursor {
            assert_eq!(progress.cursor, cursor);
            candidates.extend(plan.candidates.iter().cloned());
            cursor = progress.next_cursor;
        }
        if progress.phase == HiddenScanPhase::Complete && !progress.has_more {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "synthetic scan did not finish in time"
        );
        let previous_cursor = cursor;
        plan = hidden_scan::request(
            fixture.paths(),
            owner.clone(),
            HiddenScanRequest {
                mode: Some(HiddenScanMode::Poll),
                scan_id: Some(scan_id.clone()),
                cursor: Some(cursor),
                limit: Some(100),
            },
        )
        .await
        .expect("poll synthetic hidden scan");
        let next_progress = plan.scan.as_ref().expect("poll response includes progress");
        if next_progress.next_cursor == previous_cursor
            && !matches!(
                next_progress.phase,
                HiddenScanPhase::Complete | HiddenScanPhase::Stopped | HiddenScanPhase::Failed
            )
        {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    let elapsed = started.elapsed();
    let peak_rss_kib = peak_rss_kib();

    assert_eq!(plan.hidden_threads, hidden_count as u64);
    assert_eq!(plan.total_threads, (hidden_count + 1) as u64);
    assert_eq!(plan.visible_threads, 1);
    assert_eq!(plan.archived_threads, 0);
    assert_eq!(candidates.len(), hidden_count);
    assert_eq!(plan.scan.as_ref().unwrap().processed, hidden_count);
    assert_eq!(plan.scan.as_ref().unwrap().total, hidden_count as u64);
    assert_eq!(
        plan.source_counts.get("subagent"),
        Some(&(hidden_count as u64))
    );
    assert!(candidates
        .iter()
        .all(|candidate| candidate.allowed && candidate.fingerprint.is_some()));

    let expected_ids = fixture.hidden_ids.iter().cloned().collect::<BTreeSet<_>>();
    let candidate_ids = candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(candidate_ids, expected_ids);

    let peak_rss = peak_rss_kib
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    if let Some((history_bytes, payload_records)) = large_history {
        println!(
            "HIDDEN_SCAN_HISTORY hidden_threads={hidden_count} history_bytes={history_bytes} payload_records={payload_records} mode=streaming elapsed_ms={:.3} peak_rss_kib={peak_rss}",
            elapsed.as_secs_f64() * 1_000.0
        );
    } else {
        println!(
            "HIDDEN_SCAN_SCALE hidden_threads={hidden_count} mode=streaming elapsed_ms={:.3} peak_rss_kib={peak_rss}",
            elapsed.as_secs_f64() * 1_000.0
        );
    }
}

struct HiddenScanFixture {
    root: PathBuf,
    hidden_ids: Vec<String>,
}

impl HiddenScanFixture {
    fn new(hidden_count: usize) -> Self {
        let root = env::temp_dir().join(format!("agenthub-hidden-scan-scale-{}", Uuid::new_v4()));
        fs::create_dir(&root).expect("create anonymous hidden scan fixture");
        let mut fixture = Self {
            root,
            hidden_ids: Vec::with_capacity(hidden_count),
        };
        fixture.populate(hidden_count);
        fixture
    }

    fn paths(&self) -> CodexPaths {
        CodexPaths::new(&self.root)
    }

    fn write_large_history(&self, target_bytes: u64) -> (u64, usize) {
        const PAYLOAD_BYTES: usize = 64 * 1024;
        const PREFIX: &[u8] = br#"{"type":"synthetic_payload","payload":""#;
        const SUFFIX: &[u8] = b"\"}\n";

        let id = self
            .hidden_ids
            .first()
            .expect("large history candidate exists");
        let path = self
            .root
            .join("sessions")
            .join(format!("rollout-{id}.jsonl"));
        let file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .expect("open synthetic large rollout");
        let mut writer = BufWriter::new(file);

        let header =
            serde_json::json!({ "type": "session_meta", "payload": { "id": id } }).to_string();
        let turn_id = format!("turn-{id}");
        let turn_started =
            serde_json::json!({ "type": "turn_started", "turn_id": turn_id }).to_string();
        let task_complete = serde_json::json!({
            "type": "event_msg",
            "payload": { "type": "task_complete", "turn_id": turn_id }
        })
        .to_string();
        for line in [&header, &turn_started] {
            writer
                .write_all(line.as_bytes())
                .expect("write rollout preamble");
            writer.write_all(b"\n").expect("finish rollout preamble");
        }

        let payload = vec![b'x'; PAYLOAD_BYTES];
        let record_bytes = (PREFIX.len() + payload.len() + SUFFIX.len()) as u64;
        let fixed_bytes = (header.len() + turn_started.len() + task_complete.len() + 3) as u64;
        let remaining_bytes = target_bytes.saturating_sub(fixed_bytes);
        let payload_records = remaining_bytes.div_ceil(record_bytes) as usize;
        for _ in 0..payload_records {
            writer
                .write_all(PREFIX)
                .expect("write rollout payload prefix");
            writer.write_all(&payload).expect("write rollout payload");
            writer
                .write_all(SUFFIX)
                .expect("finish rollout payload record");
        }
        writer
            .write_all(task_complete.as_bytes())
            .expect("write rollout terminal event");
        writer
            .write_all(b"\n")
            .expect("finish rollout terminal event");
        writer.flush().expect("flush synthetic large rollout");

        let history_bytes = fs::metadata(path)
            .expect("read synthetic large rollout metadata")
            .len();
        assert!(
            history_bytes >= target_bytes,
            "large history should meet the requested byte size"
        );
        (history_bytes, payload_records)
    }

    fn populate(&mut self, hidden_count: usize) {
        let sessions = self.root.join("sessions");
        fs::create_dir_all(&sessions).expect("create fixture sessions directory");

        let mut conn = Connection::open(self.root.join("state_5.sqlite"))
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

        let tx = conn.transaction().expect("start fixture transaction");
        let mut insert = tx
            .prepare_cached(
                "INSERT INTO threads(
                    id, rollout_path, created_at, updated_at, source, thread_source,
                    parent_thread_id, agent_path, agent_nickname, agent_role,
                    has_user_event, first_user_message, preview, cwd, title,
                    archived, archived_at
                ) VALUES(?1, ?2, 1, ?3, ?4, ?5, ?6, NULL, NULL, NULL, ?7, ?8, ?8, ?9, ?10, 0, NULL)",
            )
            .expect("prepare fixture thread insert");
        let index_file = fs::File::create(self.root.join("session_index.jsonl"))
            .expect("create fixture session index");
        let mut index = BufWriter::new(index_file);

        let parent_id = Uuid::new_v4().to_string();
        let parent_rollout = sessions.join(format!("rollout-{parent_id}.jsonl"));
        write_completed_rollout(&parent_rollout, &parent_id);
        write_session_index_entry(&mut index, &parent_id, &parent_rollout);
        insert_fixture_thread(
            &mut insert,
            FixtureThreadRow {
                id: &parent_id,
                rollout: &parent_rollout,
                updated_at: 0,
                source: "codex",
                thread_source: "user",
                parent_thread_id: None,
                has_user_event: 1,
                first_user_message: "Synthetic visible parent",
                title: "Synthetic visible parent",
            },
        );

        for number in 0..hidden_count {
            let id = Uuid::new_v4().to_string();
            let rollout = sessions.join(format!("rollout-{id}.jsonl"));
            write_completed_rollout(&rollout, &id);
            write_session_index_entry(&mut index, &id, &rollout);
            insert_fixture_thread(
                &mut insert,
                FixtureThreadRow {
                    id: &id,
                    rollout: &rollout,
                    updated_at: number as i64 + 1,
                    source: "codex",
                    thread_source: "subagent",
                    parent_thread_id: Some(&parent_id),
                    has_user_event: 0,
                    first_user_message: "Synthetic subagent fixture history",
                    title: &format!("Synthetic hidden thread {number:05}"),
                },
            );
            self.hidden_ids.push(id);
        }

        drop(insert);
        index.flush().expect("flush fixture session index");
        tx.commit().expect("commit fixture state database");
    }
}

impl Drop for HiddenScanFixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("HIDDEN_SCAN_SCALE_CLEANUP_FAILED kind={:?}", error.kind());
        }
    }
}

struct FixtureThreadRow<'a> {
    id: &'a str,
    rollout: &'a Path,
    updated_at: i64,
    source: &'a str,
    thread_source: &'a str,
    parent_thread_id: Option<&'a str>,
    has_user_event: i64,
    first_user_message: &'a str,
    title: &'a str,
}

fn insert_fixture_thread(insert: &mut rusqlite::CachedStatement<'_>, row: FixtureThreadRow<'_>) {
    insert
        .execute(params![
            row.id,
            row.rollout.to_string_lossy().as_ref(),
            row.updated_at,
            row.source,
            row.thread_source,
            row.parent_thread_id,
            row.has_user_event,
            row.first_user_message,
            "/synthetic/agenthub-hidden-scan-fixture",
            row.title,
        ])
        .expect("insert fixture thread row");
}

fn write_completed_rollout(path: &Path, id: &str) {
    let turn_id = format!("turn-{id}");
    let contents = [
        serde_json::json!({ "type": "session_meta", "payload": { "id": id } }).to_string(),
        serde_json::json!({ "type": "turn_started", "turn_id": turn_id }).to_string(),
        serde_json::json!({
            "type": "event_msg",
            "payload": { "type": "task_complete", "turn_id": turn_id }
        })
        .to_string(),
    ]
    .join("\n");
    fs::write(path, contents).expect("write synthetic completed rollout");
}

fn write_session_index_entry(writer: &mut BufWriter<fs::File>, id: &str, rollout: &Path) {
    serde_json::to_writer(
        &mut *writer,
        &serde_json::json!({
            "id": id,
            "path": rollout.to_string_lossy(),
        }),
    )
    .expect("write synthetic session index entry");
    writer
        .write_all(b"\n")
        .expect("finish synthetic session index entry");
}

fn peak_rss_kib() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage = MaybeUninit::<libc::rusage>::uninit();
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
            let rss = unsafe { usage.assume_init().ru_maxrss as u64 };
            return Some(if cfg!(target_os = "macos") {
                rss / 1_024
            } else {
                rss
            });
        }
    }
    None
}
