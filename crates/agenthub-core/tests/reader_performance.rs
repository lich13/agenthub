//! Synthetic reader benchmark. Run the ignored matrix with `--nocapture`.
//!
//! "Cold" means a fresh process/application cache; the OS page cache is retained.
//! Fixture creation, append writes, and process startup are outside the timers.
//! Read and JSON serialization are measured separately. No native user data is read.
//! Codex/Claude request 160 rows; the existing Grok detail API is not paginated.
//! Set AGENTHUB_READER_PAGED=1 for the new Codex (120) and Grok (160) page APIs.
//! Set AGENTHUB_READER_TMPDIR to keep fixtures under a caller-owned temp directory.
//! Defaults: five cold, ten warm (after one warmup), and five append samples.
//! Override only with AGENTHUB_READER_COLD_RUNS / WARM_RUNS / APPEND_RUNS.

#![allow(unexpected_cfgs)]

use agenthub_core::{
    claude::{
        claude_session_detail, list_claude_sessions, ClaudeDetailRequest, ClaudePaths,
        ClaudeSessionSummary,
    },
    codex::{
        thread_detail_from_summary, window_thread_detail, ThreadDetail, ThreadStatus, ThreadSummary,
    },
    grok::{grok_session_detail, GrokHistoryEvent, GrokPaths, GrokSessionDetail},
};
#[cfg(not(reader_legacy_api))]
use agenthub_core::{codex::thread_detail_page_from_summary, grok::grok_session_page};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(not(reader_legacy_api))]
use std::collections::HashSet;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

const SESSION_ID: &str = "00000000-0000-4000-8000-000000000001";
const WORKSPACE: &str = "/workspace/reader-fixture";
const TIMESTAMP: &str = "2025-01-01T00:00:00Z";
const PAGE_LIMIT: usize = 160;
const CODEX_PAGE_LIMIT: usize = 120;
const CLAUDE_LIST_SESSION_COUNT: usize = 70;
const CLAUDE_LIST_TEXT_BYTES: usize = 16 * 1024;
const SAMPLE_PREFIX: &str = "READER_PERFORMANCE_SAMPLE ";
const MIB: u64 = 1024 * 1024;

struct Fixture {
    root: PathBuf,
    provider: String,
    turns: usize,
}

impl Fixture {
    fn new(provider: &str, target_bytes: u64) -> Self {
        Self::with_text_bytes(provider, target_bytes, 64 * 1024)
    }

    fn with_text_bytes(provider: &str, target_bytes: u64, text_bytes: usize) -> Self {
        let parent = env::var_os("AGENTHUB_READER_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir);
        fs::create_dir_all(&parent).expect("create synthetic fixture parent");
        let root = parent.join(format!(
            "agenthub-reader-performance-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("create synthetic fixture root");
        let root = fs::canonicalize(root).expect("canonicalize synthetic fixture root");
        let mut fixture = Self {
            root,
            provider: provider.into(),
            turns: 0,
        };
        let file = fixture.history();
        fs::create_dir_all(file.parent().unwrap()).expect("create fixture directories");
        if provider == "grok" {
            fs::write(
                file.parent().unwrap().join("summary.json"),
                json!({"info":{"id":SESSION_ID,"cwd":WORKSPACE},
                    "generated_title":"Synthetic reader fixture","num_messages":0})
                .to_string(),
            )
            .expect("write synthetic Grok summary");
            fs::write(fixture.root.join("active_sessions.json"), "{}")
                .expect("write empty synthetic registrations");
        }
        let mut output = BufWriter::new(File::create(file).expect("create fixture history"));
        let header = if provider == "codex" {
            format!(
                "{}\n",
                json!({"type":"session_meta","timestamp":TIMESTAMP,
                "payload":{"id":SESSION_ID,"cwd":WORKSPACE,"source":"cli"}})
            )
        } else {
            String::new()
        };
        output
            .write_all(header.as_bytes())
            .expect("write fixture header");
        let mut written = header.len() as u64;
        loop {
            let rows = turn_bytes(provider, fixture.turns, text_bytes);
            if written + rows.len() as u64 > target_bytes {
                break;
            }
            output.write_all(&rows).expect("write synthetic turn");
            written += rows.len() as u64;
            fixture.turns += 1;
        }
        assert!(
            fixture.turns > 0,
            "fixture must contain complete mixed turns"
        );
        let remaining = (target_bytes - written) as usize;
        if remaining > 0 {
            let mut padding = vec![b' '; remaining];
            padding[remaining - 1] = b'\n';
            output
                .write_all(&padding)
                .expect("pad fixture to exact size");
        }
        output.flush().expect("flush fixture history");
        output.get_ref().sync_data().expect("sync fixture history");
        assert_eq!(fs::metadata(fixture.history()).unwrap().len(), target_bytes);
        fixture
    }

    fn history(&self) -> PathBuf {
        history_path(&self.root, &self.provider)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.root).is_err() {
            eprintln!(
                "READER_PERFORMANCE_CLEANUP_FAILED provider={}",
                self.provider
            );
        }
    }
}

struct ClaudeListFixture {
    root: PathBuf,
    session_count: usize,
    file_bytes: u64,
}

impl ClaudeListFixture {
    fn new(session_count: usize, text_bytes: usize) -> Self {
        let parent = env::var_os("AGENTHUB_READER_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir);
        fs::create_dir_all(&parent).expect("create synthetic fixture parent");
        let root = parent.join(format!(
            "agenthub-reader-claude-list-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("create synthetic Claude list root");
        let session_root = root.join("projects/workspace-list-fixture");
        fs::create_dir_all(&session_root).expect("create synthetic Claude project");
        let mut file_bytes = 0;
        for index in 0..session_count {
            let id = uuid::Uuid::new_v4().to_string();
            let path = session_root.join(format!("{id}.jsonl"));
            let bytes = claude_list_session_bytes(&id, index, text_bytes);
            fs::write(path, &bytes).expect("write synthetic Claude transcript");
            file_bytes += bytes.len() as u64;
        }
        Self {
            root,
            session_count,
            file_bytes,
        }
    }
}

impl Drop for ClaudeListFixture {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.root).is_err() {
            eprintln!("READER_PERFORMANCE_CLEANUP_FAILED provider=claude-list");
        }
    }
}

fn claude_list_session_bytes(session_id: &str, turn: usize, text_bytes: usize) -> Vec<u8> {
    let minute = turn / 60;
    let second = turn % 60;
    let timestamp = format!("2025-01-01T00:{minute:02}:{second:02}Z");
    String::from_utf8(turn_bytes("claude", turn, text_bytes))
        .expect("synthetic Claude transcript UTF-8")
        .replace(SESSION_ID, session_id)
        .replace(TIMESTAMP, &timestamp)
        .into_bytes()
}

fn history_path(root: &Path, provider: &str) -> PathBuf {
    match provider {
        "codex" => root.join("rollout.jsonl"),
        "claude" => root.join(format!("projects/workspace/{SESSION_ID}.jsonl")),
        "grok" => root.join(format!("sessions/workspace/{SESSION_ID}/updates.jsonl")),
        _ => panic!("unsupported synthetic provider"),
    }
}

fn turn_bytes(provider: &str, turn: usize, text_bytes: usize) -> Vec<u8> {
    let marker = format!("fixture-turn-{turn:08}");
    let text = format!(
        "{marker} {}",
        "synthetic example text. ".repeat(text_bytes / 24)
    );
    let call = format!("fixture-call-{turn:08}");
    let turn_id = format!("fixture-turn-id-{turn:08}");
    let rows = match provider {
        "codex" => vec![
            json!({"type":"event_msg","timestamp":TIMESTAMP,
                "payload":{"type":"task_started","turn_id":turn_id}}),
            json!({"type":"response_item","timestamp":TIMESTAMP,
                "payload":{"type":"message","role":"user","turn_id":turn_id,
                    "content":[{"type":"input_text","text":text}]}}),
            json!({"type":"response_item","timestamp":TIMESTAMP,
                "payload":{"type":"function_call","name":"fixture_tool","call_id":call,
                    "arguments":"{\"query\":\"example.invalid\"}","turn_id":turn_id}}),
            json!({"type":"response_item","timestamp":TIMESTAMP,
                "payload":{"type":"function_call_output","call_id":call,"output":text,
                    "turn_id":turn_id}}),
            json!({"type":"response_item","timestamp":TIMESTAMP,
                "payload":{"type":"message","role":"assistant","turn_id":turn_id,
                    "content":[{"type":"output_text","text":text}]}}),
            json!({"type":"event_msg","timestamp":TIMESTAMP,
                "payload":{"type":"task_complete","turn_id":turn_id}}),
        ],
        "claude" => {
            let ids: Vec<_> = (0..4).map(|n| format!("fixture-{turn:08}-{n}")).collect();
            let mut rows = vec![
                json!({"type":"user","message":{"role":"user","content":text}}),
                json!({"type":"assistant","message":{"role":"assistant","content":[
                    {"type":"tool_use","id":call,"name":"fixture_tool",
                        "input":{"query":"example.invalid"}}],"stop_reason":"tool_use"}}),
                json!({"type":"user","message":{"role":"user","content":[
                    {"type":"tool_result","tool_use_id":call,"content":text}]}}),
                json!({"type":"assistant","message":{"role":"assistant","content":[
                    {"type":"text","text":text}],"stop_reason":"end_turn"}}),
            ];
            for (index, row) in rows.iter_mut().enumerate() {
                row["uuid"] = json!(ids[index]);
                row["parentUuid"] = if index > 0 {
                    json!(ids[index - 1])
                } else if turn > 0 {
                    json!(format!("fixture-{:08}-3", turn - 1))
                } else {
                    Value::Null
                };
                row["sessionId"] = json!(SESSION_ID);
                row["cwd"] = json!(WORKSPACE);
                row["version"] = json!("2.1.284");
                row["timestamp"] = json!(TIMESTAMP);
            }
            rows
        }
        "grok" => vec![
            json!({"timestamp":TIMESTAMP,"method":"session/update","params":{"update":{
                "sessionUpdate":"user_message_chunk","messageId":turn_id,
                "updateId":format!("fixture-user-{turn:08}"),
                "content":{"type":"text","text":text}}}}),
            json!({"timestamp":TIMESTAMP,"method":"session/update","params":{"update":{
                "sessionUpdate":"tool_call","toolCallId":call,"title":"fixture_tool",
                "status":"in_progress"}}}),
            json!({"timestamp":TIMESTAMP,"method":"session/update","params":{"update":{
                "sessionUpdate":"tool_call_update","toolCallId":call,"status":"completed",
                "content":[{"type":"content","content":{"type":"text","text":text}}]}}}),
            json!({"timestamp":TIMESTAMP,"method":"session/update","params":{"update":{
                "sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}}}),
        ],
        _ => panic!("unsupported synthetic provider"),
    };
    let mut bytes = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut bytes, &row).expect("serialize synthetic record");
        bytes.push(b'\n');
    }
    bytes
}

fn codex_summary(root: &Path) -> ThreadSummary {
    ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: SESSION_ID.into(),
        title: "Synthetic reader fixture".into(),
        status: ThreadStatus::Recent,
        updated_at: Some(TIMESTAMP.into()),
        archived_at: None,
        message_count: 0,
        latest_message: None,
        cwd: Some(WORKSPACE.into()),
        model: None,
        rollout_path: Some(history_path(root, "codex")),
        active_turn_id: None,
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: None,
    }
}

fn claude_request(before: Option<&str>) -> ClaudeDetailRequest {
    // The native fixture has a deterministic relative path and session identity.
    let key = hex::encode(Sha256::digest(format!(
        "workspace/{SESSION_ID}.jsonl\0{SESSION_ID}"
    )));
    ClaudeDetailRequest {
        agent_id: None,
        session_key: format!("claude:{key}"),
        limit: Some(PAGE_LIMIT),
        before: before.map(str::to_owned),
    }
}

#[derive(Default)]
struct ByteCounter(u64);
impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 += bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClaudeListSummaryCounts {
    sessions: usize,
    messages: usize,
    read_errors: usize,
    read_warnings: usize,
    can_rename: usize,
    can_delete: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct Sample {
    #[serde(alias = "read_ms")]
    wall_ms: f64,
    #[serde(default)]
    process_cpu_ms: f64,
    serialize_ms: f64,
    response_bytes: u64,
    file_bytes: u64,
    returned_rows: usize,
    total_rows: usize,
    raw_records: Option<usize>,
    tool_rows: usize,
    user_rows: usize,
    assistant_rows: usize,
    last_turn_visible: bool,
    cursor: Option<String>,
    #[serde(default)]
    peak_rss_kib: u64,
    #[serde(default)]
    row_id_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    list_summary_counts: Option<ClaudeListSummaryCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_schema: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary_sha256: Option<String>,
    #[serde(skip)]
    row_ids: Vec<String>,
}

fn process_usage() -> (u64, u64) {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
            let usage = unsafe { usage.assume_init() };
            let timeval_us =
                |time: libc::timeval| (time.tv_sec as u64) * 1_000_000 + time.tv_usec as u64;
            let cpu_us = timeval_us(usage.ru_utime) + timeval_us(usage.ru_stime);
            let rss = usage.ru_maxrss as u64;
            let rss_kib = if cfg!(target_os = "macos") {
                rss / 1024
            } else {
                rss
            };
            return (cpu_us, rss_kib);
        }
    }
    (0, 0)
}

fn serialized<T: Serialize>(value: &T) -> (f64, u64) {
    let start = Instant::now();
    let mut bytes = ByteCounter::default();
    serde_json::to_writer(&mut bytes, value).expect("serialize reader response");
    (start.elapsed().as_secs_f64() * 1000.0, bytes.0)
}

fn peak_rss_kib() -> u64 {
    process_usage().1
}

fn sample(
    root: &Path,
    provider: &str,
    last_turn: usize,
    before: Option<&str>,
    paged: bool,
) -> Sample {
    let marker = format!("fixture-turn-{last_turn:08}");
    let (cpu_before_us, _) = process_usage();
    let start = Instant::now();
    let mut result = match provider {
        "codex" => {
            let detail = if paged {
                read_codex_page(codex_summary(root), before)
            } else {
                let parsed = thread_detail_from_summary(codex_summary(root))
                    .unwrap_or_else(|_| panic!("synthetic Codex detail read failed"));
                window_thread_detail(parsed, Some(PAGE_LIMIT), before)
            };
            let read_ms = start.elapsed().as_secs_f64() * 1000.0;
            let (serialize_ms, response_bytes) = serialized(&detail);
            Sample {
                wall_ms: read_ms,
                process_cpu_ms: 0.0,
                serialize_ms,
                response_bytes,
                file_bytes: 0,
                returned_rows: detail.blocks.len(),
                total_rows: detail.total_blocks,
                raw_records: Some(detail.raw_event_count),
                tool_rows: detail.blocks.iter().filter(|b| b.role == "tool").count(),
                user_rows: detail.blocks.iter().filter(|b| b.role == "user").count(),
                assistant_rows: detail
                    .blocks
                    .iter()
                    .filter(|b| b.role == "assistant")
                    .count(),
                last_turn_visible: detail
                    .blocks
                    .iter()
                    .any(|b| b.text.as_deref().is_some_and(|s| s.contains(&marker))),
                cursor: detail.before_cursor,
                peak_rss_kib: 0,
                row_id_sha256: String::new(),
                list_summary_counts: None,
                response_schema: None,
                summary_sha256: None,
                row_ids: detail.blocks.iter().map(|b| b.id.clone()).collect(),
            }
        }
        "claude" => {
            let detail = claude_session_detail(
                &ClaudePaths {
                    config: root.into(),
                    projects: root.join("projects"),
                },
                &claude_request(before),
            )
            .unwrap_or_else(|_| panic!("synthetic Claude detail read failed"));
            let read_ms = start.elapsed().as_secs_f64() * 1000.0;
            assert!(
                detail.summary.read_error.is_none(),
                "synthetic Claude parse error"
            );
            assert!(
                detail.summary.read_warning.is_none(),
                "synthetic Claude parse warning"
            );
            let (serialize_ms, response_bytes) = serialized(&detail);
            Sample {
                wall_ms: read_ms,
                process_cpu_ms: 0.0,
                serialize_ms,
                response_bytes,
                file_bytes: 0,
                returned_rows: detail.events.len(),
                total_rows: detail.total_events,
                raw_records: None,
                tool_rows: detail
                    .events
                    .iter()
                    .filter(|e| e.kind.starts_with("tool_"))
                    .count(),
                user_rows: detail
                    .events
                    .iter()
                    .filter(|e| e.kind == "user_message")
                    .count(),
                assistant_rows: detail
                    .events
                    .iter()
                    .filter(|e| e.kind == "assistant_message")
                    .count(),
                last_turn_visible: detail
                    .events
                    .iter()
                    .any(|e| e.text.as_deref().is_some_and(|s| s.contains(&marker))),
                cursor: detail.before_cursor,
                peak_rss_kib: 0,
                row_id_sha256: String::new(),
                list_summary_counts: None,
                response_schema: None,
                summary_sha256: None,
                row_ids: detail.events.iter().map(|e| e.id.clone()).collect(),
            }
        }
        "grok" => {
            let paths = GrokPaths { home: root.into() };
            let detail = if paged {
                read_grok_page(&paths, before)
            } else {
                grok_session_detail(&paths, SESSION_ID, Some(WORKSPACE))
            }
            .unwrap_or_else(|_| panic!("synthetic Grok detail read failed"));
            let read_ms = start.elapsed().as_secs_f64() * 1000.0;
            let (serialize_ms, response_bytes) = serialized(&detail);
            Sample {
                wall_ms: read_ms,
                process_cpu_ms: 0.0,
                serialize_ms,
                response_bytes,
                file_bytes: 0,
                returned_rows: detail.events.len(),
                total_rows: grok_total_events(&detail),
                raw_records: None,
                tool_rows: detail
                    .events
                    .iter()
                    .filter(|e| e.kind.starts_with("tool_"))
                    .count(),
                user_rows: detail
                    .events
                    .iter()
                    .filter(|e| e.kind == "user_message_chunk")
                    .count(),
                assistant_rows: detail
                    .events
                    .iter()
                    .filter(|e| e.kind == "agent_message_chunk")
                    .count(),
                last_turn_visible: detail
                    .events
                    .iter()
                    .any(|e| e.text.as_deref().is_some_and(|s| s.contains(&marker))),
                cursor: grok_before_cursor(&detail),
                peak_rss_kib: 0,
                row_id_sha256: String::new(),
                list_summary_counts: None,
                response_schema: None,
                summary_sha256: None,
                row_ids: grok_detail_row_ids(&detail),
            }
        }
        _ => panic!("unsupported synthetic provider"),
    };
    result.file_bytes = fs::metadata(history_path(root, provider)).unwrap().len();
    result.peak_rss_kib = peak_rss_kib();
    assert!(
        result.returned_rows > 0 && result.tool_rows > 0,
        "mixed history missing"
    );
    if before.is_none() {
        assert!(
            result.user_rows > 0 && result.assistant_rows > 0,
            "visible messages missing"
        );
        assert!(result.last_turn_visible, "latest synthetic turn missing");
    }
    let (cpu_after_us, _) = process_usage();
    result.process_cpu_ms = cpu_after_us.saturating_sub(cpu_before_us) as f64 / 1000.0;
    let mut row_ids = Sha256::new();
    for id in &result.row_ids {
        row_ids.update((id.len() as u64).to_le_bytes());
        row_ids.update(id.as_bytes());
    }
    result.row_id_sha256 = hex::encode(row_ids.finalize());
    result
}

fn claude_list_summary_sha256(summaries: &[ClaudeSessionSummary]) -> String {
    let mut digest = Sha256::new();
    for summary in summaries {
        let path = summary.path.to_string_lossy();
        for value in [
            summary.id.as_str(),
            summary.session_key.as_str(),
            summary.title.as_str(),
            summary.cwd.as_str(),
            path.as_ref(),
            summary.updated_at.as_deref().unwrap_or(""),
            summary.last_message.as_deref().unwrap_or(""),
            summary.status.as_str(),
            summary.format_version.as_str(),
            summary.rename_block_reason.as_deref().unwrap_or(""),
            summary.delete_block_reason.as_deref().unwrap_or(""),
            summary.read_error.as_deref().unwrap_or(""),
            summary.read_warning.as_deref().unwrap_or(""),
        ] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
        digest.update((summary.message_count as u64).to_le_bytes());
        digest.update([summary.can_rename as u8, summary.can_delete as u8]);
        digest.update(
            serde_json::to_vec(&summary.storage_size).expect("serialize Claude storage summary"),
        );
    }
    hex::encode(digest.finalize())
}

fn claude_list_sample(root: &Path, expected_sessions: usize, file_bytes: u64) -> Sample {
    let paths = ClaudePaths {
        config: root.to_path_buf(),
        projects: root.join("projects"),
    };
    let (cpu_before_us, _) = process_usage();
    let start = Instant::now();
    let summaries = list_claude_sessions(&paths, 200, None)
        .unwrap_or_else(|_| panic!("synthetic Claude list failed"));
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        summaries.len(),
        expected_sessions,
        "Claude list omitted fixture sessions"
    );
    let (serialize_ms, response_bytes) = serialized(&summaries);
    let (cpu_after_us, _) = process_usage();
    let peak_rss_kib = peak_rss_kib();
    let summary_counts = ClaudeListSummaryCounts {
        sessions: summaries.len(),
        messages: summaries.iter().map(|summary| summary.message_count).sum(),
        read_errors: summaries
            .iter()
            .filter(|summary| summary.read_error.is_some())
            .count(),
        read_warnings: summaries
            .iter()
            .filter(|summary| summary.read_warning.is_some())
            .count(),
        can_rename: summaries
            .iter()
            .filter(|summary| summary.can_rename)
            .count(),
        can_delete: summaries
            .iter()
            .filter(|summary| summary.can_delete)
            .count(),
    };
    assert_eq!(
        summary_counts.read_errors, 0,
        "Claude list fixture has errors"
    );
    assert_eq!(
        summary_counts.read_warnings, 0,
        "Claude list fixture has warnings"
    );
    let mut response_schema: Vec<String> = serde_json::to_value(&summaries[0])
        .expect("serialize Claude summary schema")
        .as_object()
        .expect("Claude summary is an object")
        .keys()
        .cloned()
        .collect();
    response_schema.sort();
    let last_turn_visible = summaries.iter().all(|summary| {
        summary
            .last_message
            .as_deref()
            .is_some_and(|message| message.contains("fixture-turn-"))
    });
    assert!(
        last_turn_visible,
        "Claude list omitted latest fixture messages"
    );
    let mut result = Sample {
        wall_ms,
        process_cpu_ms: cpu_after_us.saturating_sub(cpu_before_us) as f64 / 1000.0,
        serialize_ms,
        response_bytes,
        file_bytes,
        returned_rows: summaries.len(),
        total_rows: summaries.len(),
        raw_records: None,
        tool_rows: 0,
        user_rows: 0,
        assistant_rows: 0,
        last_turn_visible,
        cursor: None,
        peak_rss_kib,
        row_id_sha256: String::new(),
        list_summary_counts: Some(summary_counts),
        response_schema: Some(response_schema),
        summary_sha256: Some(claude_list_summary_sha256(&summaries)),
        row_ids: summaries
            .iter()
            .map(|summary| summary.session_key.clone())
            .collect(),
    };
    let mut row_ids = Sha256::new();
    for id in &result.row_ids {
        row_ids.update((id.len() as u64).to_le_bytes());
        row_ids.update(id.as_bytes());
    }
    result.row_id_sha256 = hex::encode(row_ids.finalize());
    result
}

#[cfg(reader_legacy_api)]
fn grok_total_events(detail: &GrokSessionDetail) -> usize {
    detail.events.len()
}

#[cfg(not(reader_legacy_api))]
fn grok_total_events(detail: &GrokSessionDetail) -> usize {
    detail.total_events
}

#[cfg(reader_legacy_api)]
fn grok_before_cursor(_: &GrokSessionDetail) -> Option<String> {
    None
}

#[cfg(not(reader_legacy_api))]
fn grok_before_cursor(detail: &GrokSessionDetail) -> Option<String> {
    detail.before_cursor.clone()
}

fn grok_event_row_ids<'a>(events: impl IntoIterator<Item = &'a GrokHistoryEvent>) -> Vec<String> {
    events
        .into_iter()
        .enumerate()
        .map(|(index, event)| {
            let mut hasher = Sha256::new();
            hasher.update((index as u64).to_le_bytes());
            for value in [
                event.timestamp.as_deref().unwrap_or(""),
                event.kind.as_str(),
                event.text.as_deref().unwrap_or(""),
                event.method.as_deref().unwrap_or(""),
                event.call_id.as_deref().unwrap_or(""),
                event.status.as_deref().unwrap_or(""),
                event.detail.as_deref().unwrap_or(""),
            ] {
                hasher.update((value.len() as u64).to_le_bytes());
                hasher.update(value.as_bytes());
            }
            hex::encode(hasher.finalize())
        })
        .collect()
}

#[cfg(reader_legacy_api)]
fn grok_detail_row_ids(detail: &GrokSessionDetail) -> Vec<String> {
    grok_event_row_ids(detail.events.iter())
}

#[cfg(not(reader_legacy_api))]
fn grok_detail_row_ids(detail: &GrokSessionDetail) -> Vec<String> {
    grok_event_row_ids(detail.events.iter().map(|event| event.as_ref()))
}

#[cfg(not(reader_legacy_api))]
fn read_codex_page(summary: ThreadSummary, before: Option<&str>) -> ThreadDetail {
    thread_detail_page_from_summary(summary, true, Some(CODEX_PAGE_LIMIT), before, None)
        .unwrap_or_else(|_| panic!("synthetic Codex page read failed"))
}

#[cfg(reader_legacy_api)]
fn read_codex_page(_: ThreadSummary, _: Option<&str>) -> ThreadDetail {
    panic!("paged API is unavailable in legacy benchmark binary")
}

#[cfg(not(reader_legacy_api))]
fn read_grok_page(paths: &GrokPaths, before: Option<&str>) -> anyhow::Result<GrokSessionDetail> {
    grok_session_page(
        paths,
        SESSION_ID,
        Some(WORKSPACE),
        Some(PAGE_LIMIT),
        before,
        None,
    )
}

#[cfg(reader_legacy_api)]
fn read_grok_page(_: &GrokPaths, _: Option<&str>) -> anyhow::Result<GrokSessionDetail> {
    panic!("paged API is unavailable in legacy benchmark binary")
}

fn append_turn(root: &Path, provider: &str, turn: usize) {
    let bytes = turn_bytes(provider, turn, 1024);
    let mut file = OpenOptions::new()
        .append(true)
        .open(history_path(root, provider))
        .expect("open synthetic append file");
    file.write_all(&bytes).expect("append synthetic turn");
    file.sync_data().expect("sync synthetic append");
}

#[test]
#[cfg(not(reader_legacy_api))]
fn synthetic_readers_preserve_latest_tools_pagination_and_append() {
    for provider in ["codex", "claude", "grok"] {
        for paged in [false, true] {
            if paged && provider == "claude" {
                continue;
            }
            let fixture = Fixture::with_text_bytes(provider, MIB / 2, 1024);
            let first = sample(&fixture.root, provider, fixture.turns - 1, None, paged);
            let unchanged = sample(&fixture.root, provider, fixture.turns - 1, None, paged);
            assert_eq!(first.returned_rows, unchanged.returned_rows);
            assert_eq!(first.total_rows, unchanged.total_rows);
            assert_eq!(first.row_ids, unchanged.row_ids);
            if paged {
                assert!(first.cursor.is_some(), "paged fixture must continue");
            }
            let first_total_rows = first.total_rows;
            let first_file_bytes = first.file_bytes;
            let mut seen = HashSet::new();
            let mut fetched = 0;
            let mut page = first;
            let mut page_count = 0;
            loop {
                page_count += 1;
                assert!(page_count <= 10_000, "pagination did not terminate");
                assert!(page.returned_rows > 0, "pagination returned an empty page");
                fetched += page.returned_rows;
                for id in &page.row_ids {
                    assert!(seen.insert(id.clone()), "duplicate row across pages");
                }
                let Some(cursor) = page.cursor.clone() else {
                    break;
                };
                page = sample(
                    &fixture.root,
                    provider,
                    fixture.turns - 1,
                    Some(&cursor),
                    paged,
                );
            }
            assert_eq!(fetched, page.total_rows, "pagination omitted rows");
            append_turn(&fixture.root, provider, fixture.turns);
            let appended = sample(&fixture.root, provider, fixture.turns, None, paged);
            assert!(appended.total_rows > first_total_rows);
            assert!(appended.file_bytes > first_file_bytes);
        }
    }
}

#[test]
#[ignore = "internal shared-fixture pagination integrity check"]
fn reader_performance_verify_fixture() {
    let root = PathBuf::from(env::var_os("AGENTHUB_READER_FIXTURE").expect("fixture path"));
    let provider = env::var("AGENTHUB_READER_PROVIDER").expect("synthetic provider");
    let turns: usize = env::var("AGENTHUB_READER_TURNS")
        .expect("synthetic turn count")
        .parse()
        .expect("valid synthetic turn count");
    let paged = env::var_os("AGENTHUB_READER_PAGED").is_some();
    let first = sample(&root, &provider, turns - 1, None, paged);
    if paged {
        let cursor = first
            .cursor
            .as_deref()
            .expect("paged fixture must continue");
        let older = sample(&root, &provider, turns - 1, Some(cursor), paged);
        assert_eq!(
            first.total_rows, older.total_rows,
            "pagination total changed"
        );
        assert!(
            older.row_ids.iter().all(|id| !first.row_ids.contains(id)),
            "adjacent fixture pages overlap"
        );
        assert!(older.returned_rows > 0, "older page is empty");
    }
}

#[test]
#[ignore = "internal isolated-process worker for the opt-in performance matrix"]
fn reader_performance_worker() {
    let Some(root) = env::var_os("AGENTHUB_READER_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let provider = env::var("AGENTHUB_READER_PROVIDER").expect("synthetic provider");
    let mode = env::var("AGENTHUB_READER_MODE").expect("sample mode");
    let turns: usize = env::var("AGENTHUB_READER_TURNS").unwrap().parse().unwrap();
    let runs: usize = env::var("AGENTHUB_READER_RUNS").unwrap().parse().unwrap();
    let paged = env::var_os("AGENTHUB_READER_PAGED").is_some();
    if mode != "cold" {
        let _ = sample(&root, &provider, turns - 1, None, paged);
    }
    for run in 0..runs {
        let last_turn = if mode == "append" {
            append_turn(&root, &provider, turns + run);
            turns + run
        } else {
            turns - 1
        };
        let value = sample(&root, &provider, last_turn, None, paged);
        println!("{SAMPLE_PREFIX}{}", serde_json::to_string(&value).unwrap());
    }
}

#[test]
#[ignore = "internal isolated-process worker for the Claude list cache comparison"]
fn reader_performance_claude_list_worker() {
    let Some(root) = env::var_os("AGENTHUB_READER_CLAUDE_LIST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let session_count: usize = env::var("AGENTHUB_READER_CLAUDE_LIST_SESSIONS")
        .expect("synthetic Claude session count")
        .parse()
        .expect("valid Claude session count");
    let file_bytes: u64 = env::var("AGENTHUB_READER_CLAUDE_LIST_FILE_BYTES")
        .expect("synthetic Claude fixture bytes")
        .parse()
        .expect("valid Claude fixture byte count");
    let mode = env::var("AGENTHUB_READER_MODE").expect("sample mode");
    if mode == "warm" {
        let _ = claude_list_sample(&root, session_count, file_bytes);
    } else {
        assert_eq!(mode, "cold", "unsupported Claude list sample mode");
    }
    let value = claude_list_sample(&root, session_count, file_bytes);
    println!("{SAMPLE_PREFIX}{}", serde_json::to_string(&value).unwrap());
}

fn worker_at(
    executable: &Path,
    fixture: &Fixture,
    mode: &str,
    runs: usize,
    paged: bool,
) -> Vec<Sample> {
    let mut command = Command::new(executable);
    command
        .args([
            "--ignored",
            "--exact",
            "reader_performance_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AGENTHUB_READER_FIXTURE", &fixture.root)
        .env("AGENTHUB_READER_PROVIDER", &fixture.provider)
        .env("AGENTHUB_READER_MODE", mode)
        .env("AGENTHUB_READER_TURNS", fixture.turns.to_string())
        .env("AGENTHUB_READER_RUNS", runs.to_string());
    if paged {
        command.env("AGENTHUB_READER_PAGED", "1");
    } else {
        command.env_remove("AGENTHUB_READER_PAGED");
    }
    let output = command.output().expect("start isolated benchmark worker");
    assert!(
        output.status.success(),
        "isolated {mode} worker failed; exit={:?}",
        output.status.code()
    );
    let stdout = String::from_utf8(output.stdout).expect("benchmark worker UTF-8");
    let samples: Vec<Sample> = stdout
        .lines()
        .filter_map(|line| {
            line.find(SAMPLE_PREFIX).map(|index| {
                serde_json::from_str(&line[index + SAMPLE_PREFIX.len()..])
                    .expect("worker sample JSON")
            })
        })
        .collect();
    assert_eq!(samples.len(), runs, "worker sample count");
    samples
}

fn worker(fixture: &Fixture, mode: &str, runs: usize) -> Vec<Sample> {
    worker_at(
        &env::current_exe().expect("current benchmark executable"),
        fixture,
        mode,
        runs,
        env::var_os("AGENTHUB_READER_PAGED").is_some(),
    )
}

fn verify_binary_fixture(executable: &Path, fixture: &Fixture) -> Sample {
    let samples = worker_at(executable, fixture, "cold", 1, false);
    let sample = samples.into_iter().next().expect("fixture sample");
    assert!(sample.tool_rows > 0, "fixture tool events missing");
    assert!(sample.user_rows > 0, "fixture user messages missing");
    assert!(
        sample.assistant_rows > 0,
        "fixture assistant messages missing"
    );
    assert!(sample.last_turn_visible, "fixture latest turn missing");
    assert_eq!(
        sample.file_bytes,
        fs::metadata(fixture.history()).unwrap().len()
    );
    sample
}

fn claude_list_worker_at(executable: &Path, fixture: &ClaudeListFixture, mode: &str) -> Sample {
    let mut command = Command::new(executable);
    command
        .args([
            "--ignored",
            "--exact",
            "reader_performance_claude_list_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AGENTHUB_READER_CLAUDE_LIST_ROOT", &fixture.root)
        .env(
            "AGENTHUB_READER_CLAUDE_LIST_SESSIONS",
            fixture.session_count.to_string(),
        )
        .env(
            "AGENTHUB_READER_CLAUDE_LIST_FILE_BYTES",
            fixture.file_bytes.to_string(),
        )
        .env("AGENTHUB_READER_MODE", mode)
        .env_remove("AGENTHUB_READER_PAGED");
    let output = command.output().expect("start isolated Claude list worker");
    assert!(
        output.status.success(),
        "isolated Claude list {mode} worker failed; exit={:?}",
        output.status.code()
    );
    let stdout = String::from_utf8(output.stdout).expect("Claude list worker UTF-8");
    let samples: Vec<Sample> = stdout
        .lines()
        .filter_map(|line| {
            line.find(SAMPLE_PREFIX).map(|index| {
                serde_json::from_str(&line[index + SAMPLE_PREFIX.len()..])
                    .expect("Claude list worker sample JSON")
            })
        })
        .collect();
    assert_eq!(samples.len(), 1, "Claude list worker sample count");
    samples
        .into_iter()
        .next()
        .expect("Claude list worker sample")
}

fn csv_filter(name: &str) -> Option<Vec<String>> {
    env::var(name).ok().map(|value| {
        let values: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect();
        assert!(!values.is_empty(), "{name} filter selected no values");
        values
    })
}

fn selected(filter: &Option<Vec<String>>, value: &str) -> bool {
    filter
        .as_ref()
        .is_none_or(|values| values.iter().any(|candidate| candidate == value))
}

fn run_count(suffix: &str, default: usize) -> usize {
    env::var(format!("AGENTHUB_READER_{suffix}_RUNS"))
        .ok()
        .map(|value| value.parse::<usize>().expect("positive sample count"))
        .unwrap_or(default)
        .max(1)
}

fn abba_run_count(suffix: &str, minimum: usize, maximum: usize) -> usize {
    let value = env::var(format!("AGENTHUB_READER_{suffix}_RUNS"))
        .ok()
        .map(|value| value.parse::<usize>().expect("positive ABBA sample count"))
        .unwrap_or(minimum);
    assert!(
        (minimum..=maximum).contains(&value),
        "{suffix} sample count must be between {minimum} and {maximum}"
    );
    value
}

fn distribution(values: impl Iterator<Item = f64>) -> Value {
    let mut values: Vec<_> = values.collect();
    values.sort_by(f64::total_cmp);
    let p95 = (values.len() * 95).div_ceil(100).saturating_sub(1);
    json!({
        "mean":values.iter().sum::<f64>() / values.len() as f64,
        "median":values[values.len() / 2],
        "p95":values[p95]
    })
}

fn report(provider: &str, mib: u64, mode: &str, samples: &[Sample]) {
    println!(
        "READER_PERFORMANCE {}",
        json!({
            "provider":provider,"fixture_mib":mib,"mode":mode,"samples":samples.len(),
            "reader_api":if env::var_os("AGENTHUB_READER_PAGED").is_some() { "paged" } else { "legacy" },
            "cold_definition":"fresh_process_os_page_cache_retained",
            "read_wall_ms":distribution(samples.iter().map(|s| s.wall_ms)),
            "process_cpu_ms":distribution(samples.iter().map(|s| s.process_cpu_ms)),
            "serialize_ms":distribution(samples.iter().map(|s| s.serialize_ms)),
            "response_bytes":distribution(samples.iter().map(|s| s.response_bytes as f64)),
            "peak_rss_kib":distribution(samples.iter().map(|s| s.peak_rss_kib as f64)),
            "first":samples.first(),"last":samples.last(),
        })
    );
}

#[test]
#[ignore = "generates exact 10/100/300 MiB synthetic histories; opt in explicitly"]
fn reader_performance_matrix() {
    let paged = env::var_os("AGENTHUB_READER_PAGED").is_some();
    let providers = csv_filter("AGENTHUB_READER_PROVIDER_FILTER");
    let sizes = csv_filter("AGENTHUB_READER_SIZE_FILTER");
    let modes = csv_filter("AGENTHUB_READER_MODE_FILTER")
        .unwrap_or_else(|| ["cold", "warm", "append"].map(str::to_owned).to_vec());
    for provider in providers.iter().flatten() {
        assert!(["codex", "claude", "grok"].contains(&provider.as_str()));
    }
    for size in sizes.iter().flatten() {
        assert!(["10", "100", "300"].contains(&size.as_str()));
    }
    assert!(modes
        .iter()
        .all(|mode| ["cold", "warm", "append"].contains(&mode.as_str())));
    assert!(
        !paged
            || providers
                .as_ref()
                .is_none_or(|values| !values.iter().any(|provider| provider == "claude")),
        "paged history is available only for Codex and Grok"
    );
    for provider in ["codex", "claude", "grok"] {
        if paged && provider == "claude" {
            continue;
        }
        if !selected(&providers, provider) {
            continue;
        }
        for mib in [10, 100, 300] {
            if !selected(&sizes, &mib.to_string()) {
                continue;
            }
            let fixture = Fixture::new(provider, mib * MIB);
            if paged {
                let executable = env::current_exe().expect("current benchmark executable");
                let mut command = Command::new(executable);
                command
                    .args([
                        "--ignored",
                        "--exact",
                        "reader_performance_verify_fixture",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("AGENTHUB_READER_FIXTURE", &fixture.root)
                    .env("AGENTHUB_READER_PROVIDER", provider)
                    .env("AGENTHUB_READER_TURNS", fixture.turns.to_string())
                    .env("AGENTHUB_READER_PAGED", "1");
                let output = command.output().expect("verify paged synthetic fixture");
                assert!(
                    output.status.success(),
                    "paged fixture integrity check failed; exit={:?}; stderr={}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            println!(
                "READER_PERFORMANCE_FIXTURE {}",
                json!({
                    "provider":provider,"fixture_mib":mib,"turns":fixture.turns,
                    "file_bytes":mib * MIB,
                    "page_limit":if paged && provider == "codex" { Some(CODEX_PAGE_LIMIT) }
                        else if paged || provider != "grok" { Some(PAGE_LIMIT) } else { None },
                })
            );
            if modes.iter().any(|mode| mode == "cold") {
                let mut cold = Vec::new();
                for _ in 0..run_count("COLD", 5) {
                    cold.extend(worker(&fixture, "cold", 1));
                }
                report(provider, mib, "cold", &cold);
            }
            if modes.iter().any(|mode| mode == "warm") {
                let mut warm = Vec::new();
                for _ in 0..run_count("WARM", 10) {
                    warm.extend(worker(&fixture, "warm", 1));
                }
                report(provider, mib, "warm", &warm);
            }
            if modes.iter().any(|mode| mode == "append") {
                let append = worker(&fixture, "append", run_count("APPEND", 5));
                report(provider, mib, "append", &append);
            }
        }
    }
}

#[test]
#[ignore = "runs a bounded old/new ABBA comparison on identical fixtures"]
#[cfg(not(reader_legacy_api))]
fn reader_performance_abba() {
    let old_binary = PathBuf::from(
        env::var_os("AGENTHUB_READER_OLD_BINARY").expect("legacy harness binary path"),
    );
    assert!(old_binary.is_file(), "legacy harness binary is missing");
    let new_binary = env::current_exe().expect("current benchmark executable");
    let providers = csv_filter("AGENTHUB_READER_PROVIDER_FILTER");
    let sizes = csv_filter("AGENTHUB_READER_SIZE_FILTER");
    let modes = csv_filter("AGENTHUB_READER_MODE_FILTER")
        .unwrap_or_else(|| ["cold", "warm"].map(str::to_owned).to_vec());
    for provider in providers.iter().flatten() {
        assert!(["codex", "claude", "grok"].contains(&provider.as_str()));
    }
    for size in sizes.iter().flatten() {
        assert!(["10", "100", "300"].contains(&size.as_str()));
    }
    assert!(modes
        .iter()
        .all(|mode| ["cold", "warm"].contains(&mode.as_str())));
    for provider in ["codex", "grok", "claude"] {
        if !selected(&providers, provider) {
            continue;
        }
        let candidates: &[u64] = match provider {
            "codex" => &[10, 100, 300],
            "grok" => &[10, 100, 300],
            "claude" => &[10, 100, 300],
            _ => unreachable!(),
        };
        for mib in candidates {
            if !selected(&sizes, &mib.to_string()) {
                continue;
            }
            let fixture = Fixture::new(provider, *mib * MIB);
            let old_fixture = verify_binary_fixture(&old_binary, &fixture);
            let new_fixture = verify_binary_fixture(&new_binary, &fixture);
            assert_eq!(
                old_fixture.returned_rows, new_fixture.returned_rows,
                "old/new result row counts differ on shared fixture"
            );
            assert_eq!(
                old_fixture.total_rows, new_fixture.total_rows,
                "old/new total row counts differ on shared fixture"
            );
            assert_eq!(
                old_fixture.tool_rows, new_fixture.tool_rows,
                "old/new tool event counts differ on shared fixture"
            );
            assert_eq!(
                old_fixture.user_rows, new_fixture.user_rows,
                "old/new user message counts differ on shared fixture"
            );
            assert_eq!(
                old_fixture.assistant_rows, new_fixture.assistant_rows,
                "old/new assistant message counts differ on shared fixture"
            );
            assert_eq!(
                old_fixture.row_id_sha256, new_fixture.row_id_sha256,
                "old/new row identities differ on shared fixture"
            );
            println!(
                "READER_PERFORMANCE_ABBA_FIXTURE {}",
                json!({"provider":provider,"fixture_mib":mib,"file_bytes":mib * MIB,
                    "turns":fixture.turns})
            );
            for (mode, minimum, batch) in [
                ("cold", abba_run_count("ABBA_COLD", 3, 15), 1),
                ("warm", abba_run_count("ABBA_WARM", 5, 25), 1),
            ] {
                if !modes.iter().any(|selected| selected == mode) {
                    continue;
                }
                let mut old_samples = Vec::new();
                let mut new_samples = Vec::new();
                let schedule = ["old", "new", "new", "old"];
                while old_samples.len() < minimum || new_samples.len() < minimum {
                    for version in schedule {
                        let samples = if version == "old" {
                            &mut old_samples
                        } else {
                            &mut new_samples
                        };
                        if samples.len() >= minimum {
                            continue;
                        }
                        let executable = if version == "old" {
                            &old_binary
                        } else {
                            &new_binary
                        };
                        let count = (minimum - samples.len()).min(batch);
                        samples.extend(worker_at(executable, &fixture, mode, count, false));
                    }
                }
                println!(
                    "READER_PERFORMANCE_ABBA {}",
                    json!({"provider":provider,"fixture_mib":mib,"mode":mode,
                        "order":"ABBA repeated","old_reader_api":"legacy",
                        "new_reader_api":"legacy","old_samples":old_samples,
                        "new_samples":new_samples})
                );
            }
        }
    }
}

#[cfg(not(reader_legacy_api))]
#[test]
#[ignore = "runs a bounded old/new ABBA comparison for Claude's summary cache"]
fn reader_performance_claude_list_abba() {
    let old_binary = PathBuf::from(
        env::var_os("AGENTHUB_READER_OLD_BINARY").expect("legacy harness binary path"),
    );
    assert!(old_binary.is_file(), "legacy harness binary is missing");
    let new_binary = env::current_exe().expect("current benchmark executable");
    let providers = csv_filter("AGENTHUB_READER_PROVIDER_FILTER");
    assert!(
        selected(&providers, "claude"),
        "Claude list benchmark requires the claude provider filter"
    );
    assert!(
        csv_filter("AGENTHUB_READER_SIZE_FILTER").is_none(),
        "size filters do not apply to the Claude list fixture"
    );
    let modes = csv_filter("AGENTHUB_READER_MODE_FILTER")
        .unwrap_or_else(|| ["cold", "warm"].map(str::to_owned).to_vec());
    assert!(modes
        .iter()
        .all(|mode| ["cold", "warm"].contains(&mode.as_str())));
    let fixture = ClaudeListFixture::new(CLAUDE_LIST_SESSION_COUNT, CLAUDE_LIST_TEXT_BYTES);
    println!(
        "READER_PERFORMANCE_CLAUDE_LIST_FIXTURE {}",
        json!({"sessions":fixture.session_count,"file_bytes":fixture.file_bytes,
            "text_bytes_per_message":CLAUDE_LIST_TEXT_BYTES,"body_cache_entries":64})
    );
    for mode in ["cold", "warm"] {
        if !modes.iter().any(|selected| selected == mode) {
            continue;
        }
        let minimum = if mode == "cold" {
            abba_run_count("ABBA_COLD", 3, 15)
        } else {
            abba_run_count("ABBA_WARM", 5, 25)
        };
        let mut old_samples = Vec::new();
        let mut new_samples = Vec::new();
        let schedule = ["old", "new", "new", "old"];
        while old_samples.len() < minimum || new_samples.len() < minimum {
            for version in schedule {
                let samples = if version == "old" {
                    &mut old_samples
                } else {
                    &mut new_samples
                };
                if samples.len() >= minimum {
                    continue;
                }
                let executable = if version == "old" {
                    &old_binary
                } else {
                    &new_binary
                };
                samples.push(claude_list_worker_at(executable, &fixture, mode));
            }
        }
        println!(
            "READER_PERFORMANCE_CLAUDE_LIST_ABBA {}",
            json!({"provider":"claude","mode":mode,"order":"ABBA repeated",
                "old_samples":old_samples,"new_samples":new_samples})
        );
        let old = old_samples.first().expect("old Claude list sample");
        let new = new_samples.first().expect("new Claude list sample");
        assert_eq!(old.returned_rows, fixture.session_count);
        assert_eq!(new.returned_rows, fixture.session_count);
        assert_eq!(old.returned_rows, new.returned_rows);
        assert_eq!(old.total_rows, new.total_rows);
        assert_eq!(old.row_id_sha256, new.row_id_sha256);
        assert_eq!(old.list_summary_counts, new.list_summary_counts);
        assert_eq!(old.response_schema, new.response_schema);
        assert_eq!(old.summary_sha256, new.summary_sha256);
        assert_eq!(old.response_bytes, new.response_bytes);
        assert!(old.last_turn_visible && new.last_turn_visible);
    }
}
