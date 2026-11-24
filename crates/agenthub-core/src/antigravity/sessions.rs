//! Antigravity App native session discovery, reading, and mutations.
//! Uses live Connect-RPC when the local Antigravity `language_server` is running,
//! and seamlessly falls back to `~/.gemini/antigravity` SQLite / prototext / transcript files offline.
use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    provider_sessions::{
        matches_query, paginate_provider_detail, ProviderDetailRequest, ProviderHistoryEvent,
        ProviderHistorySnapshot, ProviderListRequest, ProviderSessionDetail,
        ProviderSessionSummary,
    },
    services::sessions::{SessionBatchItem, SessionBatchItemResult, SessionOperation},
    session_delete::{self, Outcome},
    session_storage::{self, SessionStorageSize, StorageScope},
};

mod endpoint;
#[cfg(test)]
use endpoint::resolve_daemon_hub_endpoint;
pub(crate) use endpoint::{
    connect_endpoint, connect_rpc_post, fetch_live_trajectory_steps, ConnectRpcHttpError,
    LiveHistoryUnavailable, LiveRpc,
};
pub use endpoint::{
    discover_host_endpoints, evict_idle_endpoint_cache, get_cached_live_endpoint,
    record_endpoint_failure, record_verified_live_endpoint, DiscoveredAntigravityEndpoint,
};
#[cfg(test)]
pub(crate) use endpoint::{parse_loopback_listener_port, parse_proc_net_tcp_loopback_ports};

pub(super) mod annotations;
#[cfg(test)]
use annotations::read_raw_summary_annotations;
use annotations::{
    invalidate_proto_cache, parse_raw_summary, patch_raw_summary_annotations,
    read_annotation_overlay, write_annotation_overlay, ParsedRawSummary,
};
mod cli_meta;
pub(super) mod prompt;
pub use cli_meta::scan_offline_store;
pub(crate) use cli_meta::{
    build_offline_stream_snapshot, inspect_conv_db_metadata, load_store_auxiliary_index,
    transcript_has_subagent_reminder,
};

mod history;
pub(crate) use history::load_events_from_source;
#[cfg(test)]
use history::{extract_live_step_events, read_offline_transcript_events};
pub use history::{load_all_events, read_attachment, session_detail};

mod mutations;
pub(crate) use mutations::{
    descendant_ids, is_presence_locked_shared, resolve_target_store_and_endpoint,
};
pub use mutations::{
    execute_batch_item, execute_unified_cleanup_targets, live_item_is_actively_executing,
    live_item_is_failed, live_item_is_running, live_item_is_waiting, preview_batch,
    preview_batch_item, preview_unified_cleanup_candidates, rename_session, set_archived,
    UnifiedAntigravityStepOutcome,
};

const SERVICE_PREFIX: &str = "/exa.language_server_pb.LanguageServerService/";
const RESPONSE_LIMIT: usize = crate::resource_limits::ANTIGRAVITY_LOCAL_RPC_BYTES;
const MAX_TITLE_CHARS: usize = 200;

#[derive(Debug, Clone)]
pub struct AntigravityPaths {
    pub home: PathBuf,
    pub cli_home: Option<PathBuf>,
    /// Explicit endpoint mode used by isolated tests so tests never probe real host processes.
    pub rpc_mode: RpcMode,
}

#[derive(Debug, Clone)]
pub enum RpcMode {
    AutoDiscover,
    OfflineOnly,
    #[cfg(test)]
    Fixed {
        port: u16,
        token: String,
    },
}

impl AntigravityPaths {
    pub fn default_for_user() -> Result<Self> {
        let home = crate::platform::provider_root("ANTIGRAVITY_HOME", ".gemini/antigravity")?;
        let cli_home = match std::env::var_os("ANTIGRAVITY_CLI_HOME").filter(|v| !v.is_empty()) {
            Some(custom) => Some(PathBuf::from(custom)),
            None => Some(crate::platform::current_home()?.join(".gemini/antigravity-cli")),
        }
        .filter(|cli| cli != &home);
        Ok(Self {
            home,
            cli_home,
            rpc_mode: if cfg!(any(target_os = "macos", target_os = "linux")) {
                RpcMode::AutoDiscover
            } else {
                RpcMode::OfflineOnly
            },
        })
    }

    pub fn offline(home: PathBuf) -> Self {
        Self {
            home,
            cli_home: None,
            rpc_mode: RpcMode::OfflineOnly,
        }
    }

    pub fn offline_user_home(user_home: impl AsRef<Path>) -> Self {
        let base = user_home.as_ref();
        Self {
            home: base.join(".gemini/antigravity"),
            cli_home: Some(base.join(".gemini/antigravity-cli")),
            rpc_mode: RpcMode::OfflineOnly,
        }
    }

    #[cfg(test)]
    pub fn with_fixed_rpc(home: PathBuf, port: u16, token: impl Into<String>) -> Self {
        Self {
            home,
            cli_home: None,
            rpc_mode: RpcMode::Fixed {
                port,
                token: token.into(),
            },
        }
    }

    #[cfg(test)]
    pub fn with_fixed_rpc_and_cli(
        home: PathBuf,
        cli_home: PathBuf,
        port: u16,
        token: impl Into<String>,
    ) -> Self {
        Self {
            home,
            cli_home: Some(cli_home),
            rpc_mode: RpcMode::Fixed {
                port,
                token: token.into(),
            },
        }
    }

    pub(crate) fn primary_store(&self) -> Self {
        Self {
            home: self.home.clone(),
            cli_home: None,
            rpc_mode: self.rpc_mode.clone(),
        }
    }

    pub fn cli_store(&self) -> Option<Self> {
        self.cli_home
            .as_ref()
            .filter(|cli| *cli != &self.home)
            .map(|cli| Self::offline(cli.clone()))
    }

    pub(crate) fn cli_store_for_id(&self, id: &str) -> Option<Self> {
        let cli = self.cli_store()?;
        has_primary_session_storage(&cli, id).then_some(cli)
    }

    pub(crate) fn store_for_id(&self, id: &str, live: Option<&Value>) -> Self {
        if let Some(cli) = self.cli_store_for_id(id) {
            return cli;
        }
        if live
            .and_then(|v| v["trajectorySummaries"].get(id))
            .is_some()
            || has_primary_session_storage(self, id)
        {
            return self.primary_store();
        }
        self.primary_store()
    }

    pub(crate) fn other_store_for(&self, chosen_home: &Path) -> Option<Self> {
        if chosen_home == self.home {
            self.cli_store()
        } else {
            Some(self.primary_store())
        }
    }

    pub fn is_cli_only_session_default(id: &str) -> bool {
        if uuid::Uuid::parse_str(id).is_err() {
            return false;
        }
        let Ok(paths) = Self::default_for_user() else {
            return false;
        };
        paths.cli_store_for_id(id).is_some()
    }

    pub fn should_exclude_from_desktop_live_scan(
        &self,
        id: &str,
        extra_excluded: &BTreeSet<String>,
    ) -> bool {
        extra_excluded.contains(id) || self.cli_store_for_id(id).is_some()
    }

    pub fn collect_all_known_subagents(&self) -> BTreeSet<String> {
        let mut out = collect_store_known_subagents(&self.primary_store());
        if let Some(cli) = self.cli_store() {
            out.extend(collect_store_known_subagents(&cli));
        }
        out
    }

    pub fn available(&self) -> bool {
        self.home.is_dir()
            || self.cli_home.as_ref().is_some_and(|p| p.is_dir())
            || connect_endpoint(&self.primary_store()).is_ok_and(|endpoint| endpoint.is_some())
    }

    pub fn summaries_db(&self) -> PathBuf {
        self.home.join("conversation_summaries.db")
    }

    pub fn conversations_dir(&self) -> PathBuf {
        self.home.join("conversations")
    }

    pub fn annotations_dir(&self) -> PathBuf {
        self.home.join("annotations")
    }

    pub fn brain_dir(&self) -> PathBuf {
        self.home.join("brain")
    }

    pub fn presence_dir(&self) -> PathBuf {
        self.home.join("presence")
    }

    pub fn proto_cache(&self) -> PathBuf {
        self.home.join("agyhub_summaries_proto.pb")
    }

    pub fn canonical_path(&self, id: &str) -> PathBuf {
        let logs = self.brain_dir().join(id).join(".system_generated/logs");
        let full = logs.join("transcript_full.jsonl");
        if full.is_file() {
            return full;
        }
        let compact = logs.join("transcript.jsonl");
        if compact.is_file() {
            return compact;
        }
        let db = self.conversations_dir().join(format!("{id}.db"));
        if db.exists() {
            return db;
        }
        let pb = self.conversations_dir().join(format!("{id}.pb"));
        if pb.exists() {
            return pb;
        }
        let brain = self.brain_dir().join(id);
        if brain.exists() {
            return brain;
        }
        db
    }
}

pub fn valid_cascade_id(id: &str) -> Result<()> {
    ensure!(
        uuid::Uuid::parse_str(id).is_ok(),
        "无效的 Antigravity 会话 ID"
    );
    Ok(())
}

pub fn decode_file_uri(uri: &str) -> String {
    let raw = uri.trim();
    let stripped = raw.strip_prefix("file://").unwrap_or(raw);
    let bytes = stripped.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(hex);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn is_subtrajectory(id: &str, item: &Value) -> bool {
    let parent = item["parentConversationId"]
        .as_str()
        .or_else(|| item["trajectoryMetadata"]["parentConversationId"].as_str())
        .unwrap_or("");
    let depth = item["nestingDepth"]
        .as_i64()
        .or_else(|| item["trajectoryMetadata"]["nestingDepth"].as_i64())
        .unwrap_or(0);
    let root = item["rootConversationId"]
        .as_str()
        .or_else(|| item["trajectoryMetadata"]["rootConversationId"].as_str())
        .unwrap_or("");
    !parent.is_empty() || depth > 0 || (!root.is_empty() && root != id)
}

pub fn collect_subagents_by_root(
    summaries: &serde_json::Map<String, Value>,
) -> BTreeMap<String, BTreeMap<String, super::SubagentLiveness>> {
    let mut parents: BTreeMap<&str, &str> = BTreeMap::new();
    for (cid, item) in summaries {
        if let Some(pid) = item["parentConversationId"]
            .as_str()
            .or_else(|| item["trajectoryMetadata"]["parentConversationId"].as_str())
            .filter(|s| !s.is_empty())
        {
            parents.insert(cid.as_str(), pid);
        }
    }
    let mut by_root: BTreeMap<String, BTreeMap<String, super::SubagentLiveness>> = BTreeMap::new();
    for (cid, item) in summaries {
        if !is_subtrajectory(cid, item) {
            continue;
        }
        let liveness = super::SubagentLiveness {
            active: live_item_is_running(item),
            unsettled_background: false,
            last_modified_ms: crate::native_probe::timestamp(&item["lastModifiedTime"])
                .or_else(|| crate::native_probe::timestamp(&item["createdTime"])),
        };
        let mut target_roots = BTreeSet::new();
        if let Some(root) = item["rootConversationId"]
            .as_str()
            .or_else(|| item["trajectoryMetadata"]["rootConversationId"].as_str())
            .filter(|s| !s.is_empty() && *s != cid)
        {
            target_roots.insert(root.to_owned());
        }
        let mut curr = cid.as_str();
        for _ in 0..16 {
            let Some(&pid) = parents.get(curr) else {
                break;
            };
            target_roots.insert(pid.to_owned());
            curr = pid;
        }
        for root in target_roots {
            by_root
                .entry(root)
                .or_default()
                .insert(cid.clone(), liveness.clone());
        }
    }
    by_root
}

pub(crate) fn attach_storage_size(paths: &AntigravityPaths, summary: &mut ProviderSessionSummary) {
    if !paths.home.is_dir() {
        return;
    }
    let brain = paths.brain_dir().join(&summary.id);
    let primary = paths.conversations_dir().join(format!("{}.db", summary.id));
    let legacy_pb = paths.conversations_dir().join(format!("{}.pb", summary.id));
    let primary_file = if primary.is_file() {
        Some(primary)
    } else if legacy_pb.is_file() {
        Some(legacy_pb)
    } else {
        None
    };
    let size = if brain.is_dir() {
        session_storage::directory_size(
            &paths.home,
            &brain,
            primary_file.as_deref(),
            &format!("antigravity:{}", summary.id),
        )
    } else if let Some(file) = &primary_file {
        session_storage::file_size(&paths.home, file)
    } else {
        SessionStorageSize::unavailable(StorageScope::File)
    };
    summary.storage_size = Some(size);
}

pub(crate) fn has_primary_session_storage(store: &AntigravityPaths, id: &str) -> bool {
    if uuid::Uuid::parse_str(id).is_err() || !store.home.is_dir() {
        return false;
    }
    [
        store.conversations_dir().join(format!("{id}.db")),
        store.conversations_dir().join(format!("{id}.pb")),
        store.brain_dir().join(id),
    ]
    .into_iter()
    .any(|p| p.exists() && session_delete::within(&store.home, &p))
}

pub(crate) fn is_session_busy_status(status: &str) -> bool {
    matches!(status, "running" | "waiting")
}

pub(crate) fn offline_summary_row_status(
    status: &str,
    not_fully_idle: bool,
) -> Option<&'static str> {
    let trimmed = status.trim();
    if matches!(trimmed, "CASCADE_RUN_STATUS_WAITING" | "WAITING") {
        return Some("waiting");
    }
    if not_fully_idle
        || matches!(
            trimmed,
            "CASCADE_RUN_STATUS_RUNNING"
                | "CASCADE_RUN_STATUS_BUSY"
                | "CASCADE_RUN_STATUS_CANCELING"
                | "RUNNING"
                | "BUSY"
                | "CANCELING"
        )
    {
        return Some("running");
    }
    if matches!(trimmed, "CASCADE_RUN_STATUS_ERROR" | "ERROR" | "FAILED") {
        return Some("failed");
    }
    None
}

const OFFLINE_INFLIGHT_WINDOW_MS: i64 = 15 * 60 * 1000;
const OFFLINE_REPLY_NEEDED_WINDOW_MS: i64 = 6 * 3600 * 1000;

pub(crate) fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn parse_any_timestamp_ms(raw: &str) -> Option<i64> {
    cli_meta::parse_valid_timestamp_ms(raw)
}

fn file_mtime_ms(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

fn conv_db_modified_within(
    paths: &AntigravityPaths,
    id: &str,
    window_ms: i64,
    now_ms: i64,
) -> bool {
    let db = paths.conversations_dir().join(format!("{id}.db"));
    let wal = paths.conversations_dir().join(format!("{id}.db-wal"));
    [wal, db]
        .into_iter()
        .filter_map(|p| file_mtime_ms(&p))
        .any(|ms| now_ms.saturating_sub(ms) <= window_ms && ms <= now_ms + 60_000)
}

fn read_transcript_tail_rows(paths: &AntigravityPaths, id: &str) -> Option<(PathBuf, Vec<Value>)> {
    use std::io::{Seek, SeekFrom};
    let logs = paths.brain_dir().join(id).join(".system_generated/logs");
    let full = logs.join("transcript_full.jsonl");
    let compact = logs.join("transcript.jsonl");
    let chosen = if full.is_file() {
        full
    } else if compact.is_file() {
        compact
    } else {
        return None;
    };
    let mut file = fs::File::open(&chosen).ok()?;
    let len = file.metadata().ok()?.len();
    let tail_cap: u64 = 256 * 1024;
    let skip_partial = len > tail_cap;
    if skip_partial {
        let _ = file.seek(SeekFrom::Start(len - tail_cap));
    }
    let mut reader = BufReader::new(file);
    if skip_partial {
        let _ = prompt::drain_until_newline(&mut reader);
    }
    let mut rows = Vec::new();
    let _ = prompt::for_each_bounded_jsonl_row(&mut reader, |row| {
        rows.push(row);
        std::ops::ControlFlow::Continue(())
    });
    Some((chosen, rows))
}

pub(crate) fn extract_transcript_spawned_subagents(
    paths: &AntigravityPaths,
    id: &str,
) -> Vec<String> {
    let logs = paths.brain_dir().join(id).join(".system_generated/logs");
    let full = logs.join("transcript_full.jsonl");
    let compact = logs.join("transcript.jsonl");
    let chosen = if full.is_file() {
        full
    } else if compact.is_file() {
        compact
    } else {
        return Vec::new();
    };
    let Ok(file) = fs::File::open(&chosen) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let mut spawned = BTreeSet::new();
    let _ = prompt::for_each_bounded_jsonl_line(&mut reader, |line| {
        if line.windows(14).any(|w| w == b"conversationId")
            || line.windows(15).any(|w| w == b"conversation_id")
        {
            if let Ok(row) = serde_json::from_slice::<Value>(line) {
                if row["type"].as_str() != Some("SYSTEM_MESSAGE") {
                    super::collect_spawned_subagent_ids(&row, &mut spawned);
                }
            }
        }
        std::ops::ControlFlow::Continue(())
    });
    spawned.remove(id);
    spawned.into_iter().collect()
}

fn inspect_conv_db_steps_status(paths: &AntigravityPaths, id: &str) -> Option<&'static str> {
    let db_path = paths.conversations_dir().join(format!("{id}.db"));
    let mut f = fs::File::open(&db_path).ok()?;
    let mut header = [0u8; 16];
    if f.read_exact(&mut header).is_err() || &header != b"SQLite format 3\0" {
        return None;
    }
    drop(f);
    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let cols: BTreeSet<String> = conn
        .prepare("PRAGMA table_info(steps)")
        .ok()?
        .query_map([], |row| row.get::<_, String>(1))
        .ok()?
        .flatten()
        .collect();
    if !cols.contains("idx") || !cols.contains("step_type") || !cols.contains("status") {
        return None;
    }
    let mut stmt = conn
        .prepare("SELECT step_type, COALESCE(status, 0) FROM steps ORDER BY idx DESC LIMIT 64")
        .ok()?;
    let rows: Vec<(i64, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .ok()?
        .flatten()
        .collect();
    if rows.is_empty() {
        return None;
    }
    let mut saw_waiting = false;
    for (pos, &(step_type, status)) in rows.iter().enumerate() {
        // Cortex step status enum: 1 = PENDING, 2 = RUNNING, 7 = ERROR, 8 = GENERATING, 9 = WAITING, 11 = QUEUED
        if matches!(status, 1 | 2 | 8 | 11) {
            return Some("running");
        }
        if status == 9 {
            saw_waiting = true;
        }
        if pos == 0 && (status == 7 || step_type == 17) {
            return Some("failed");
        }
        if step_type == 14 || step_type == 101 {
            break;
        }
    }
    if saw_waiting {
        return Some("waiting");
    }
    if matches!(rows[0], (14 | 101 | 132, 3)) {
        return Some("between_steps");
    }
    Some("idle")
}

fn transcript_system_message_senders(row: &Value) -> Vec<String> {
    let mut senders = Vec::new();
    for direct in [
        row["sender"].as_str(),
        row["systemMessage"]["agentMessage"]["sender"].as_str(),
        row["systemMessage"]["sender"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        if !direct.is_empty() {
            senders.push(direct.to_owned());
        }
    }
    if let Some(content) = row["content"].as_str() {
        let mut rest = content;
        while let Some(pos) = rest.find("sender=") {
            rest = &rest[pos + "sender=".len()..];
            let end = rest
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '>' | '"' | '\'' | ',' | ';' | ']' | ')')
                })
                .unwrap_or(rest.len());
            let token = rest[..end].trim();
            if !token.is_empty() {
                senders.push(token.to_owned());
            }
            rest = &rest[end..];
        }
    }
    senders
}

fn inspect_transcript_tail_status(
    paths: &AntigravityPaths,
    id: &str,
    now_ms: i64,
    lock_held: bool,
) -> Option<&'static str> {
    let (transcript_path, rows) = read_transcript_tail_rows(paths, id)?;
    if rows.is_empty() {
        return None;
    }
    let last_idx = rows.iter().rposition(|row| {
        let st = row["type"].as_str().unwrap_or("");
        let src = row["source"].as_str().unwrap_or("");
        !matches!(st, "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "")
            && !(st == "SYSTEM_MESSAGE" && src == "SYSTEM_SDK")
            && !(prompt::is_transcript_user_row(row)
                && !prompt::transcript_row_has_user_payload(row))
    })?;
    let last = &rows[last_idx];
    let last_type = last["type"].as_str().unwrap_or("");
    let last_source = last["source"].as_str().unwrap_or("");
    let last_status = last["status"].as_str().unwrap_or("DONE");
    if lock_held {
        if last_status == "WAITING" {
            return Some("waiting");
        }
        if matches!(last_status, "RUNNING" | "PENDING" | "GENERATING" | "QUEUED") {
            return Some("running");
        }
    }
    if last_type == "ERROR_MESSAGE" || matches!(last_status, "ERROR" | "FAILED") {
        return Some("failed");
    }

    let turn_start = rows[..=last_idx]
        .iter()
        .rposition(|row| {
            prompt::is_transcript_user_row(row) && prompt::transcript_row_has_user_payload(row)
        })
        .unwrap_or(0);
    let turn_rows = &rows[turn_start..=last_idx];

    let mut open_tool_calls = 0usize;
    let mut open_waiting_tool = false;
    let mut pending_subagents: BTreeSet<String> = BTreeSet::new();
    for row in turn_rows {
        let rtype = row["type"].as_str().unwrap_or("");
        let rsource = row["source"].as_str().unwrap_or("");
        let rstatus = row["status"].as_str().unwrap_or("DONE");
        if matches!(rstatus, "CANCELED" | "INTERRUPTED" | "INVALID")
            || matches!(rtype, "FINISH" | "ERROR_MESSAGE")
        {
            open_tool_calls = 0;
            open_waiting_tool = false;
            if rtype != "FINISH" {
                continue;
            }
        }
        if rtype == "SYSTEM_MESSAGE" {
            if rsource != "SYSTEM_SDK" {
                for sender in transcript_system_message_senders(row) {
                    pending_subagents.remove(&sender);
                }
            }
            continue;
        }
        let mut row_spawned = BTreeSet::new();
        super::collect_spawned_subagent_ids(row, &mut row_spawned);
        row_spawned.remove(id);
        for cid in row_spawned {
            pending_subagents.insert(cid);
        }
        if rtype == "PLANNER_RESPONSE" {
            if let Some(calls) = row["tool_calls"].as_array() {
                for call in calls {
                    let cname = call["name"]
                        .as_str()
                        .or_else(|| call["tool_name"].as_str())
                        .unwrap_or("");
                    let args_val = call
                        .get("arguments")
                        .or_else(|| call.get("args"))
                        .map(super::parse_json_or_value)
                        .unwrap_or(Value::Null);
                    if cname == "manage_subagents" {
                        let action = args_val["Action"]
                            .as_str()
                            .or_else(|| args_val["action"].as_str())
                            .unwrap_or("");
                        if action == "kill_all" {
                            pending_subagents.clear();
                        } else if action == "kill" {
                            if let Some(ids) = args_val["ConversationIds"]
                                .as_array()
                                .or_else(|| args_val["conversationIds"].as_array())
                            {
                                for cid in ids.iter().filter_map(Value::as_str) {
                                    pending_subagents.remove(cid);
                                }
                            }
                        }
                    }
                    let has_inline_result = call
                        .get("output")
                        .or_else(|| call.get("result"))
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.trim().is_empty());
                    if !has_inline_result {
                        open_tool_calls = open_tool_calls.saturating_add(1);
                        if matches!(
                            cname,
                            "ask_question" | "default_api:ask_question" | "ask_permission"
                        ) {
                            open_waiting_tool = true;
                        }
                    }
                }
            }
        } else if !prompt::is_transcript_user_row(row)
            && !matches!(rtype, "FINISH" | "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "")
        {
            open_tool_calls = open_tool_calls.saturating_sub(1);
            if open_tool_calls == 0 {
                open_waiting_tool = false;
            }
        }
    }

    if lock_held {
        if open_waiting_tool {
            return Some("waiting");
        }
        for sub_id in &pending_subagents {
            if is_presence_locked_shared(paths, sub_id) {
                return Some("running");
            }
            let sub_node =
                evaluate_node_status_offline_with_lock(paths, sub_id, None, None, true, now_ms);
            if matches!(sub_node, "running" | "waiting") {
                return Some("running");
            }
            if last_type != "FINISH" {
                let sub_mtime = file_mtime_ms(&paths.canonical_path(sub_id));
                if sub_mtime.is_some_and(|ms| {
                    now_ms.saturating_sub(ms) < super::BACKGROUND_HANDOFF_GRACE_MS
                        && ms <= now_ms + 60_000
                }) {
                    return Some("running");
                }
            }
        }

        let unclosed_tool_step =
            !matches!(last_type, "PLANNER_RESPONSE" | "FINISH" | "ERROR_MESSAGE")
                && !matches!(last_status, "CANCELED" | "INTERRUPTED" | "INVALID");
        let inflight_tail = prompt::is_transcript_user_row(last)
            || (last_type == "SYSTEM_MESSAGE" && last_source != "SYSTEM_SDK")
            || open_tool_calls > 0
            || unclosed_tool_step;
        if inflight_tail {
            let fresh = match last["created_at"].as_str().and_then(parse_any_timestamp_ms) {
                Some(step_ms) => {
                    now_ms.saturating_sub(step_ms) <= OFFLINE_INFLIGHT_WINDOW_MS
                        && step_ms <= now_ms + 60_000
                }
                None => file_mtime_ms(&transcript_path)
                    .is_some_and(|ms| now_ms.saturating_sub(ms) <= 60_000 && ms <= now_ms + 60_000),
            };
            if fresh {
                return Some("running");
            }
        }
    }

    if last_type == "PLANNER_RESPONSE" && open_tool_calls == 0 {
        let fresh_reply = last["created_at"]
            .as_str()
            .and_then(parse_any_timestamp_ms)
            .is_some_and(|ms| {
                now_ms.saturating_sub(ms) <= OFFLINE_REPLY_NEEDED_WINDOW_MS && ms <= now_ms + 60_000
            });
        if fresh_reply {
            let reply_text = last["content"].as_str().unwrap_or("");
            let asks_question = crate::codex::assistant_feedback_request(reply_text).is_some();
            let asks_artifact_feedback = turn_rows.iter().any(|r| {
                super::step_requests_artifact_feedback(r)
                    || r["tool_calls"].as_array().is_some_and(|calls| {
                        calls.iter().any(|c| {
                            let args = c
                                .get("arguments")
                                .or_else(|| c.get("args"))
                                .map(super::parse_json_or_value)
                                .unwrap_or(Value::Null);
                            args["ArtifactMetadata"]["RequestFeedback"].as_bool() == Some(true)
                                || args["artifactMetadata"]["requestFeedback"].as_bool()
                                    == Some(true)
                        })
                    })
            });
            if asks_question || asks_artifact_feedback {
                return Some("reply_needed");
            }
        }
        return Some("settled_idle");
    }

    if last_type == "FINISH" {
        return Some("finished");
    }
    Some("idle")
}

pub(crate) fn evaluate_node_status_offline_with_lock(
    paths: &AntigravityPaths,
    id: &str,
    sqlite_status: Option<&str>,
    raw_summary: Option<&ParsedRawSummary>,
    inherited_lock: bool,
    now_ms: i64,
) -> &'static str {
    let db_step = inspect_conv_db_steps_status(paths, id);
    let conv_db_active_fresh = matches!(db_step, Some("running" | "waiting"))
        && conv_db_modified_within(paths, id, 60_000, now_ms);
    let lock_held = inherited_lock || is_presence_locked_shared(paths, id) || conv_db_active_fresh;
    let transcript = inspect_transcript_tail_status(paths, id, now_ms, lock_held);
    let node_settled =
        matches!(transcript, Some("finished" | "settled_idle")) || db_step == Some("idle");
    if !lock_held {
        if db_step == Some("failed")
            || transcript == Some("failed")
            || (!node_settled && raw_summary.is_some_and(ParsedRawSummary::is_termination_failure))
        {
            return "failed";
        }
        if transcript == Some("reply_needed") {
            return "reply_needed";
        }
        return "idle";
    }
    if db_step == Some("running") || transcript == Some("running") {
        return "running";
    }
    if db_step == Some("between_steps")
        && !matches!(transcript, Some("finished" | "settled_idle" | "failed"))
        && conv_db_modified_within(paths, id, OFFLINE_INFLIGHT_WINDOW_MS, now_ms)
    {
        return "running";
    }
    if db_step == Some("waiting")
        || transcript == Some("waiting")
        || (!node_settled && raw_summary.is_some_and(|r| !r.waiting_steps.is_empty()))
    {
        return "waiting";
    }
    if !node_settled {
        if sqlite_status == Some("running")
            || raw_summary.and_then(ParsedRawSummary::active_status) == Some("running")
        {
            return "running";
        }
        if sqlite_status == Some("waiting") {
            return "waiting";
        }
    }
    if db_step == Some("failed")
        || transcript == Some("failed")
        || (!node_settled
            && (sqlite_status == Some("failed")
                || raw_summary.is_some_and(ParsedRawSummary::is_termination_failure)))
    {
        return "failed";
    }
    if transcript == Some("reply_needed") {
        return "reply_needed";
    }
    "idle"
}

pub(crate) fn evaluate_node_status_offline(
    paths: &AntigravityPaths,
    id: &str,
    sqlite_status: Option<&str>,
    now_ms: i64,
) -> &'static str {
    evaluate_node_status_offline_with_lock(paths, id, sqlite_status, None, false, now_ms)
}

pub(crate) fn evaluate_offline_tree_status_with_lock(
    paths: &AntigravityPaths,
    root_id: &str,
    descendants: &[String],
    sqlite_statuses: &BTreeMap<String, &'static str>,
    raw_summaries: Option<&BTreeMap<String, ParsedRawSummary>>,
    inherited_lock: bool,
    now_ms: i64,
) -> &'static str {
    let tree_lock = inherited_lock || is_presence_locked_shared(paths, root_id);
    let root_status = evaluate_node_status_offline_with_lock(
        paths,
        root_id,
        sqlite_statuses.get(root_id).copied(),
        raw_summaries.and_then(|m| m.get(root_id)),
        tree_lock,
        now_ms,
    );
    if root_status == "running" {
        return "running";
    }
    let mut any_waiting = root_status == "waiting";
    for child in descendants {
        let child_status = evaluate_node_status_offline_with_lock(
            paths,
            child,
            sqlite_statuses.get(child).copied(),
            raw_summaries.and_then(|m| m.get(child)),
            tree_lock,
            now_ms,
        );
        if child_status == "running" {
            return "running";
        }
        if child_status == "waiting" {
            any_waiting = true;
        }
    }
    if any_waiting {
        "waiting"
    } else {
        root_status
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn make_summary_with_status(
    paths: &AntigravityPaths,
    id: String,
    title: String,
    cwd: String,
    archived: bool,
    updated_at: Option<String>,
    message_count: usize,
    last_message: Option<String>,
    raw_status: &str,
    format_version: &'static str,
    offline_tree_locked: bool,
) -> ProviderSessionSummary {
    let status = if archived { "archived" } else { raw_status };
    let busy = is_session_busy_status(status);
    let is_offline = format_version != "cascade-v2";
    let offline_busy = busy && is_offline;
    let offline_open_idle = is_offline && offline_tree_locked && !busy;
    let verb = if status == "waiting" {
        "会话正在等待处理，请先在 Antigravity 中处理或停止后再"
    } else {
        "会话正在运行，请先在 Antigravity 中停止后再"
    };
    let archive_verb = if archived { "恢复" } else { "归档" };
    let rename_block_reason = if offline_busy {
        Some(format!("{verb}改名"))
    } else if offline_open_idle {
        Some("会话当前已在 Antigravity 中打开，离线模式下无法安全改名".into())
    } else {
        None
    };
    let archive_block_reason = if offline_busy {
        Some(format!("{verb}{archive_verb}"))
    } else if offline_open_idle {
        Some(format!(
            "会话当前已在 Antigravity 中打开，离线模式下无法安全{archive_verb}"
        ))
    } else {
        None
    };
    let delete_block_reason = if busy {
        Some(format!("{verb}删除"))
    } else if offline_open_idle {
        Some("会话当前已在 Antigravity 中打开，离线模式下无法安全删除".into())
    } else {
        None
    };
    let workspace_root = crate::workspace_root::resolve_workspace_root(&cwd);
    ProviderSessionSummary {
        storage_size: None,
        session_key: id.clone(),
        path: paths.canonical_path(&id),
        id,
        title,
        cwd,
        workspace_root,
        archived,
        updated_at,
        message_count,
        last_message,
        status: status.into(),
        format_version: format_version.into(),
        can_rename: rename_block_reason.is_none(),
        rename_block_reason,
        can_archive: archive_block_reason.is_none(),
        archive_block_reason,
        can_delete: delete_block_reason.is_none(),
        delete_block_reason,
        read_error: None,
        read_warning: None,
    }
}

pub(crate) fn subagent_states_by_root(
    summaries: &serde_json::Map<String, Value>,
) -> BTreeMap<String, (bool, bool)> {
    let mut parents: BTreeMap<&str, &str> = BTreeMap::new();
    for (cid, item) in summaries {
        if let Some(pid) = item["parentConversationId"]
            .as_str()
            .or_else(|| item["trajectoryMetadata"]["parentConversationId"].as_str())
            .filter(|s| !s.is_empty())
        {
            parents.insert(cid.as_str(), pid);
        }
    }
    let mut out: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for (cid, item) in summaries {
        if !is_subtrajectory(cid, item) {
            continue;
        }
        let exec = live_item_is_actively_executing(item);
        let wait = live_item_is_waiting(item);
        if !exec && !wait {
            continue;
        }
        let mut roots = BTreeSet::new();
        if let Some(root) = item["rootConversationId"]
            .as_str()
            .or_else(|| item["trajectoryMetadata"]["rootConversationId"].as_str())
            .filter(|s| !s.is_empty() && *s != cid)
        {
            roots.insert(root.to_owned());
        }
        let mut curr = cid.as_str();
        for _ in 0..32 {
            let Some(&pid) = parents.get(curr) else {
                break;
            };
            roots.insert(pid.to_owned());
            curr = pid;
        }
        for root in roots {
            let entry = out.entry(root).or_insert((false, false));
            entry.0 |= exec;
            entry.1 |= wait;
        }
    }
    out
}

pub(crate) fn live_item_status(
    paths: &AntigravityPaths,
    id: &str,
    item: &Value,
    has_executing_subagents: bool,
    has_waiting_subagents: bool,
    now_ms: i64,
) -> &'static str {
    if live_item_is_actively_executing(item) || has_executing_subagents {
        return "running";
    }
    if live_item_is_waiting(item) || has_waiting_subagents {
        return "waiting";
    }
    if live_item_is_failed(item) {
        return "failed";
    }
    if item["status"].as_str() == Some("CASCADE_RUN_STATUS_IDLE") {
        let recent = item["lastModifiedTime"]
            .as_str()
            .or_else(|| item["createdTime"].as_str())
            .and_then(parse_any_timestamp_ms)
            .is_some_and(|ms| now_ms.saturating_sub(ms) <= OFFLINE_REPLY_NEEDED_WINDOW_MS);
        if recent {
            if let Some(tail) = inspect_transcript_tail_status(paths, id, now_ms, true) {
                if matches!(tail, "running" | "waiting" | "reply_needed") {
                    return tail;
                }
            }
        }
    }
    "idle"
}

fn summary_from_live_json(
    paths: &AntigravityPaths,
    id: &str,
    item: &Value,
    has_executing_subagents: bool,
    has_waiting_subagents: bool,
    now_ms: i64,
) -> Option<ProviderSessionSummary> {
    if uuid::Uuid::parse_str(id).is_err() || is_subtrajectory(id, item) {
        return None;
    }
    let overlay = read_annotation_overlay(paths, id);
    let ann_title = item["annotations"]["title"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or(overlay.title);
    let raw_summary_str = item["summary"].as_str().unwrap_or("");
    let fallback_summary = {
        let cleaned = prompt::extract_antigravity_user_request(raw_summary_str);
        (!cleaned.trim().is_empty()).then(|| cleaned.trim().to_owned())
    };
    let title = ann_title
        .or_else(|| prompt::clean_antigravity_auto_title(raw_summary_str))
        .unwrap_or_else(|| "未命名会话".into());
    let archived = item["annotations"]
        .get("archived")
        .and_then(Value::as_bool)
        .or(overlay.archived)
        .unwrap_or(false);
    let cwd = item["workspaces"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|ws| ws["workspaceFolderAbsoluteUri"].as_str())
        .or_else(|| {
            item["trajectoryMetadata"]["workspaceUris"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
        })
        .map(decode_file_uri)
        .unwrap_or_default();
    let updated_at = item["lastModifiedTime"]
        .as_str()
        .or_else(|| item["createdTime"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let step_count = item["stepCount"].as_u64().unwrap_or(0) as usize;
    let status = if archived {
        "archived"
    } else {
        live_item_status(
            paths,
            id,
            item,
            has_executing_subagents,
            has_waiting_subagents,
            now_ms,
        )
    };
    Some(make_summary_with_status(
        paths,
        id.to_owned(),
        title,
        cwd,
        archived,
        updated_at,
        step_count,
        fallback_summary,
        status,
        "cascade-v2",
        false,
    ))
}

fn summaries_table_columns(conn: &Connection) -> BTreeSet<String> {
    let Ok(mut stmt) = conn.prepare("PRAGMA table_info(conversation_summaries)") else {
        return BTreeSet::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return BTreeSet::new();
    };
    rows.flatten().collect()
}

pub(crate) fn has_conversation_parents_table(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'conversation_parents' LIMIT 1",
        [],
        |_| Ok(()),
    )
    .is_ok()
}

fn summarize_unindexed_transcript(
    paths: &AntigravityPaths,
    id: &str,
) -> (Option<String>, Option<String>, usize, Option<String>) {
    let logs = paths.brain_dir().join(id).join(".system_generated/logs");
    let chosen = [
        logs.join("transcript_full.jsonl"),
        logs.join("transcript.jsonl"),
    ]
    .into_iter()
    .find(|p| p.is_file());
    let Some(path) = chosen else {
        return (None, None, 0, None);
    };
    let Ok(file) = fs::File::open(&path) else {
        return (None, None, 0, None);
    };
    let mut reader = BufReader::new(file);
    let mut first_title: Option<String> = None;
    let mut last_preview: Option<String> = None;
    let mut last_created_at: Option<String> = None;
    let mut count = 0usize;
    let _ = prompt::for_each_bounded_jsonl_row(&mut reader, |row| {
        let st = row["type"].as_str().unwrap_or("");
        if matches!(st, "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "") {
            return std::ops::ControlFlow::Continue(());
        }
        count += 1;
        if let Some(ts) = row["created_at"].as_str().filter(|s| !s.is_empty()) {
            last_created_at = index_timestamp(ts);
        }
        if first_title.is_none() && prompt::is_transcript_user_row(&row) {
            if let Some(raw) = row["content"].as_str().filter(|s| !s.trim().is_empty()) {
                let cleaned = prompt::extract_antigravity_user_request(raw);
                let candidate = cleaned.lines().map(str::trim).find(|l| !l.is_empty());
                if let Some(line_text) = candidate {
                    first_title = Some(line_text.chars().take(80).collect());
                }
            }
        } else if st == "PLANNER_RESPONSE" {
            if let Some(reply) = row["content"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                last_preview = Some(reply.chars().take(160).collect());
            }
        }
        std::ops::ControlFlow::Continue(())
    });
    (first_title, last_preview, count, last_created_at)
}

pub(crate) fn collect_store_known_subagents(paths: &AntigravityPaths) -> BTreeSet<String> {
    let mut known = BTreeSet::new();
    if !paths.home.is_dir() {
        return known;
    }
    let db_path = paths.summaries_db();
    let conn = db_path
        .is_file()
        .then(|| {
            Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .ok()
        })
        .flatten();
    let aux = load_store_auxiliary_index(paths, conn.as_ref());
    for kids in aux.sidecar_children.values() {
        known.extend(kids.iter().cloned());
    }
    for (cid, entry) in &aux.conv_meta {
        if entry.is_internal || entry.parent_id.is_some() || entry.root_id.is_some() {
            known.insert(cid.clone());
        }
    }
    if let Some(conn) = &conn {
        if has_conversation_parents_table(conn) {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT conversation_id, COALESCE(parent_id, '') FROM conversation_parents",
            ) {
                if let Ok(rows) =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                {
                    for (cid, pid) in rows.flatten() {
                        if !pid.is_empty() && uuid::Uuid::parse_str(&cid).is_ok() {
                            known.insert(cid);
                        }
                    }
                }
            }
        }
        let cols = summaries_table_columns(conn);
        let raw_expr = if cols.contains("raw_summary") {
            "raw_summary"
        } else {
            "NULL"
        };
        let sql = format!(
            "SELECT conversation_id, COALESCE(parent_conversation_id, ''), COALESCE(nesting_depth, 0), {raw_expr} FROM conversation_summaries"
        );
        if let Ok(mut stmt) = conn.prepare(&sql) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3).ok().flatten(),
                ))
            }) {
                for (cid, pid, depth, raw) in rows.flatten() {
                    if !pid.is_empty()
                        || depth > 0
                        || raw
                            .as_deref()
                            .is_some_and(|b| parse_raw_summary(b).trajectory_meta.is_subagent(&cid))
                    {
                        known.insert(cid);
                    }
                }
            }
        }
    }
    known
}

pub(crate) fn list_offline_summaries(
    paths: &AntigravityPaths,
    filter_id: Option<&str>,
) -> Result<Vec<ProviderSessionSummary>> {
    if !paths.home.is_dir() {
        return Ok(Vec::new());
    }
    let now_ms = now_epoch_ms();
    let mut by_id: BTreeMap<String, ProviderSessionSummary> = BTreeMap::new();
    let mut indexed_ids: BTreeSet<String> = BTreeSet::new();
    let mut known_subagents: BTreeSet<String> = BTreeSet::new();
    let mut children_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut sqlite_statuses: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut raw_summaries: BTreeMap<String, ParsedRawSummary> = BTreeMap::new();
    let mut node_timestamps_ms: BTreeMap<String, i64> = BTreeMap::new();
    let mut conv_db_metas: BTreeMap<String, cli_meta::ConvDbMetadata> = BTreeMap::new();
    let mut had_any_db_summary = false;

    let add_child_edge =
        |children: &mut BTreeMap<String, Vec<String>>, parent: &str, child: &str| {
            if !parent.is_empty()
                && parent != child
                && uuid::Uuid::parse_str(parent).is_ok()
                && uuid::Uuid::parse_str(child).is_ok()
            {
                let list = children.entry(parent.to_owned()).or_default();
                if !list.iter().any(|x| x == child) {
                    list.push(child.to_owned());
                }
            }
        };

    type OfflineRow = (
        String,
        String,
        String,
        i64,
        String,
        String,
        String,
        i64,
        String,
        bool,
    );
    let mut root_rows: Vec<OfflineRow> = Vec::new();

    let db_path = paths.summaries_db();
    let open_conn = db_path
        .is_file()
        .then(|| {
            Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .ok()
        })
        .flatten();
    let aux = load_store_auxiliary_index(paths, open_conn.as_ref());

    for (pid, kids) in &aux.sidecar_children {
        for cid in kids {
            known_subagents.insert(cid.clone());
            add_child_edge(&mut children_map, pid, cid);
        }
    }
    for (cid, entry) in &aux.conv_meta {
        if entry.is_internal {
            known_subagents.insert(cid.clone());
        }
        if let Some(pid) = entry.parent_id.as_deref().or(entry.root_id.as_deref()) {
            known_subagents.insert(cid.clone());
            add_child_edge(&mut children_map, pid, cid);
        }
    }

    if let Some(conn) = &open_conn {
        if has_conversation_parents_table(conn) {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT conversation_id, COALESCE(parent_id, '') FROM conversation_parents",
            ) {
                if let Ok(rows) = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                }) {
                    for (cid, pid) in rows.flatten() {
                        if !pid.is_empty() && uuid::Uuid::parse_str(&cid).is_ok() {
                            indexed_ids.insert(cid.clone());
                            known_subagents.insert(cid.clone());
                            add_child_edge(&mut children_map, &pid, &cid);
                        }
                    }
                }
            }
        }

        let cols = summaries_table_columns(conn);
        let status_expr = if cols.contains("status") {
            "COALESCE(status, '')"
        } else {
            "''"
        };
        let idle_expr = if cols.contains("not_fully_idle") {
            "not_fully_idle"
        } else {
            "0"
        };
        let raw_expr = if cols.contains("raw_summary") {
            "raw_summary"
        } else {
            "NULL"
        };
        let sql = format!(
            "SELECT conversation_id, COALESCE(title, ''), COALESCE(preview, ''), \
             COALESCE(step_count, 0), COALESCE(last_modified_time, ''), \
             COALESCE(workspace_uris, ''), COALESCE(parent_conversation_id, ''), \
             COALESCE(nesting_depth, 0), {status_expr}, {idle_expr}, {raw_expr} \
             FROM conversation_summaries"
        );
        if let Ok(mut stmt) = conn.prepare(&sql) {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, ValueOrString>(9).map(|v| v.0).unwrap_or(false),
                    row.get::<_, Option<Vec<u8>>>(10).ok().flatten(),
                ))
            }) {
                for (
                    id,
                    db_title,
                    preview,
                    steps,
                    modified,
                    ws_raw,
                    parent,
                    depth,
                    status_str,
                    not_fully_idle,
                    raw_blob,
                ) in rows.flatten()
                {
                    if uuid::Uuid::parse_str(&id).is_err() {
                        continue;
                    }
                    indexed_ids.insert(id.clone());
                    let parsed_raw = raw_blob.as_deref().map(parse_raw_summary);
                    if let Some(ts_ms) = parse_any_timestamp_ms(&modified)
                        .or_else(|| parsed_raw.as_ref().and_then(|r| r.created_time_ms))
                        .or_else(|| aux.fts_rows.get(&id).and_then(|f| f.last_modified_ms))
                    {
                        node_timestamps_ms.insert(id.clone(), ts_ms);
                    }
                    let st =
                        offline_summary_row_status(&status_str, not_fully_idle).or_else(|| {
                            parsed_raw
                                .as_ref()
                                .and_then(ParsedRawSummary::active_status)
                        });
                    if let Some(st) = st {
                        sqlite_statuses.insert(id.clone(), st);
                    }
                    let raw_is_sub = parsed_raw
                        .as_ref()
                        .is_some_and(|r| r.trajectory_meta.is_subagent(&id));
                    let raw_parent = parsed_raw
                        .as_ref()
                        .and_then(|r| r.trajectory_meta.effective_parent(&id))
                        .map(str::to_owned);
                    if let Some(pr) = parsed_raw {
                        raw_summaries.insert(id.clone(), pr);
                    }
                    if !parent.is_empty() || depth > 0 || raw_is_sub {
                        known_subagents.insert(id.clone());
                        if !parent.is_empty() {
                            add_child_edge(&mut children_map, &parent, &id);
                        } else if let Some(pid) = raw_parent.as_deref() {
                            add_child_edge(&mut children_map, pid, &id);
                        }
                        continue;
                    }
                    if has_primary_session_storage(paths, &id) {
                        had_any_db_summary = true;
                    }
                    root_rows.push((
                        id,
                        db_title,
                        preview,
                        steps,
                        modified,
                        ws_raw,
                        parent,
                        depth,
                        status_str,
                        not_fully_idle,
                    ));
                }
            }
        }
    }

    let mut has_recorded_parent: BTreeSet<String> = children_map
        .values()
        .flat_map(|kids| kids.iter().cloned())
        .collect();
    let mut locked_ids: BTreeSet<String> = BTreeSet::new();
    let mut unindexed_ids: BTreeSet<String> = BTreeSet::new();
    if paths.conversations_dir().is_dir() {
        if let Ok(entries) = fs::read_dir(paths.conversations_dir()) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if !entry_path.is_file() || !session_delete::within(&paths.home, &entry_path) {
                    continue;
                }
                let Some(ext) = entry_path.extension().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !matches!(ext, "db" | "pb") {
                    continue;
                }
                let Some(id) = entry_path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if uuid::Uuid::parse_str(id).is_ok()
                    && !indexed_ids.contains(id)
                    && !known_subagents.contains(id)
                {
                    unindexed_ids.insert(id.to_owned());
                }
            }
        }
    }
    if paths.presence_dir().is_dir() {
        if let Ok(entries) = fs::read_dir(paths.presence_dir()) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.extension().and_then(|s| s.to_str()) != Some("lock") {
                    continue;
                }
                let Some(id) = entry_path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if uuid::Uuid::parse_str(id).is_ok() && is_presence_locked_shared(paths, id) {
                    locked_ids.insert(id.to_owned());
                    if !indexed_ids.contains(id)
                        && !known_subagents.contains(id)
                        && has_primary_session_storage(paths, id)
                    {
                        unindexed_ids.insert(id.to_owned());
                    }
                }
            }
        }
    }
    if !had_any_db_summary && paths.brain_dir().is_dir() {
        if let Ok(entries) = fs::read_dir(paths.brain_dir()) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                let id = entry.file_name().to_string_lossy().into_owned();
                if uuid::Uuid::parse_str(&id).is_ok()
                    && !indexed_ids.contains(&id)
                    && !known_subagents.contains(&id)
                    && session_delete::within(&paths.home, &entry_path)
                {
                    unindexed_ids.insert(id);
                }
            }
        }
    }

    // Inspect .db metadata for unindexed sessions (and any indexed root whose .db might mark it a subagent)
    let mut unresolved_unindexed = false;
    for uid in &unindexed_ids {
        if let Some(meta) = inspect_conv_db_metadata(paths, uid) {
            if meta.is_subagent {
                known_subagents.insert(uid.clone());
                if !meta.parent_ids.is_empty() {
                    for pid in &meta.parent_ids {
                        has_recorded_parent.insert(uid.clone());
                        add_child_edge(&mut children_map, pid, uid);
                    }
                } else if let Some(rid) = meta.root_id.as_deref() {
                    has_recorded_parent.insert(uid.clone());
                    add_child_edge(&mut children_map, rid, uid);
                }
            }
            conv_db_metas.insert(uid.clone(), meta);
        } else {
            unresolved_unindexed = true;
        }
        if !known_subagents.contains(uid) && transcript_has_subagent_reminder(paths, uid) {
            known_subagents.insert(uid.clone());
        }
    }

    let has_locked_unparented_subagent = locked_ids
        .iter()
        .any(|id| known_subagents.contains(id) && !has_recorded_parent.contains(id));
    if unresolved_unindexed || has_locked_unparented_subagent {
        for pid in indexed_ids.iter().chain(unindexed_ids.iter()) {
            for cid in extract_transcript_spawned_subagents(paths, pid) {
                known_subagents.insert(cid.clone());
                has_recorded_parent.insert(cid.clone());
                add_child_edge(&mut children_map, pid, &cid);
            }
        }
    } else {
        let mut scan_pids: BTreeSet<&str> = locked_ids
            .intersection(&indexed_ids)
            .map(String::as_str)
            .collect();
        if let Some(target) = filter_id {
            scan_pids.insert(target);
        }
        for pid in scan_pids {
            for cid in extract_transcript_spawned_subagents(paths, pid) {
                known_subagents.insert(cid.clone());
                has_recorded_parent.insert(cid.clone());
                add_child_edge(&mut children_map, pid, &cid);
            }
        }
    }

    type HistoryPromptFallbackMap = BTreeMap<String, (Option<String>, Option<String>)>;
    let mut history_loaded: Option<HistoryPromptFallbackMap> = None;
    let mut get_history_fallback = |cid: &str| -> (Option<String>, Option<String>) {
        let map =
            history_loaded.get_or_insert_with(|| cli_meta::load_history_jsonl_fallbacks(paths));
        map.get(cid).cloned().unwrap_or((None, None))
    };

    for (id, db_title, preview, steps, modified, ws_raw, _, _, _, _) in root_rows {
        if known_subagents.contains(&id) || !has_primary_session_storage(paths, &id) {
            continue;
        }
        if filter_id.is_some_and(|target| target != id) {
            continue;
        }
        let raw_parsed = raw_summaries.get(&id);
        let fts_row = aux.fts_rows.get(&id);
        let conv_meta_entry = aux.conv_meta.get(&id);
        let overlay = read_annotation_overlay(paths, &id);
        let archived = overlay
            .archived
            .or_else(|| raw_parsed.and_then(|r| r.annotations.archived))
            .or_else(|| fts_row.and_then(|f| f.archived))
            .unwrap_or(false);

        let mut cwd = serde_json::from_str::<Vec<String>>(&ws_raw)
            .ok()
            .and_then(|uris| uris.into_iter().next())
            .map(|uri| decode_file_uri(&uri))
            .filter(|s| !s.is_empty())
            .or_else(|| {
                raw_parsed
                    .and_then(|r| {
                        r.workspaces
                            .first()
                            .or_else(|| r.trajectory_meta.workspace_uris.first())
                    })
                    .map(|u| decode_file_uri(u))
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| fts_row.and_then(|f| f.workspace.clone()))
            .or_else(|| aux.last_conv_cwd.get(&id).cloned())
            .or_else(|| conv_meta_entry.and_then(|m| m.cwd.clone()))
            .unwrap_or_default();
        let mut db_meta_entry = conv_db_metas.get(&id).cloned();
        if cwd.is_empty() || steps <= 0 {
            if db_meta_entry.is_none() {
                if let Some(m) = inspect_conv_db_metadata(paths, &id) {
                    if m.is_subagent {
                        continue;
                    }
                    db_meta_entry = Some(m);
                }
            }
            if cwd.is_empty() {
                cwd = db_meta_entry
                    .as_ref()
                    .and_then(|m| m.cwd.clone())
                    .or_else(|| cli_meta::read_conv_meta_file(paths, &id).1)
                    .or_else(|| get_history_fallback(&id).0)
                    .unwrap_or_default();
            }
        }

        let cleaned_preview = prompt::extract_antigravity_user_request(&preview);
        let title = overlay
            .title
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                raw_parsed
                    .and_then(|r| r.annotations.title.clone())
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| prompt::clean_antigravity_auto_title(&db_title))
            .or_else(|| fts_row.and_then(|f| f.title.clone()))
            .or_else(|| conv_meta_entry.and_then(|m| m.title.clone()))
            .or_else(|| {
                cleaned_preview
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .map(|l| l.chars().take(80).collect())
            })
            .or_else(|| cli_meta::read_conv_meta_file(paths, &id).0)
            .or_else(|| get_history_fallback(&id).1)
            .unwrap_or_else(|| "未命名会话".into());

        let descendants = descendant_ids(&children_map, &id);
        let raw_status = if archived {
            "archived"
        } else {
            evaluate_offline_tree_status_with_lock(
                paths,
                &id,
                &descendants,
                &sqlite_statuses,
                Some(&raw_summaries),
                false,
                now_ms,
            )
        };

        let updated_at = index_timestamp(&modified).or_else(|| {
            let mut best_ms = node_timestamps_ms.get(&id).copied().or_else(|| {
                file_mtime_ms(&paths.canonical_path(&id)).and_then(cli_meta::valid_epoch_ms)
            });
            for desc in &descendants {
                if let Some(dms) = node_timestamps_ms.get(desc).copied() {
                    best_ms = Some(best_ms.map_or(dms, |prev| prev.max(dms)));
                }
            }
            best_ms.and_then(cli_meta::format_ms_utc)
        });
        let effective_steps = if steps > 0 {
            steps as usize
        } else {
            fts_row
                .and_then(|f| f.step_count)
                .or_else(|| {
                    db_meta_entry
                        .as_ref()
                        .map(|m| m.step_count)
                        .filter(|&c| c > 0)
                })
                .unwrap_or(0)
        };

        let tree_locked =
            locked_ids.contains(&id) || descendants.iter().any(|d| locked_ids.contains(d));
        let summary = make_summary_with_status(
            paths,
            id.clone(),
            title,
            cwd,
            archived,
            updated_at,
            effective_steps,
            (!cleaned_preview.trim().is_empty()).then_some(cleaned_preview),
            raw_status,
            "sqlite-offline",
            tree_locked,
        );
        by_id.insert(id, summary);
    }

    for id in unindexed_ids {
        if known_subagents.contains(&id)
            || by_id.contains_key(&id)
            || filter_id.is_some_and(|target| target != id)
        {
            continue;
        }
        let overlay = read_annotation_overlay(paths, &id);
        let conv_meta_entry = aux.conv_meta.get(&id);
        let db_meta_entry = conv_db_metas.get(&id);
        let (meta_file_title, meta_file_cwd) = cli_meta::read_conv_meta_file(paths, &id);
        let archived = overlay.archived.unwrap_or(false);
        let (first_title, last_preview, step_count, transcript_ts) =
            summarize_unindexed_transcript(paths, &id);
        let conv_db = paths.conversations_dir().join(format!("{id}.db"));
        let conv_pb = paths.conversations_dir().join(format!("{id}.pb"));
        let format_version = if conv_db.is_file() {
            "sqlite-offline"
        } else if conv_pb.is_file() {
            "pb-offline"
        } else {
            "brain-offline"
        };
        let updated_at = transcript_ts.or_else(|| {
            file_mtime_ms(&paths.canonical_path(&id)).and_then(cli_meta::format_ms_utc)
        });
        let descendants = descendant_ids(&children_map, &id);
        let raw_status = if archived {
            "archived"
        } else {
            evaluate_offline_tree_status_with_lock(
                paths,
                &id,
                &descendants,
                &sqlite_statuses,
                Some(&raw_summaries),
                false,
                now_ms,
            )
        };
        let mut cwd = db_meta_entry
            .and_then(|m| m.cwd.clone())
            .or(meta_file_cwd)
            .or_else(|| aux.last_conv_cwd.get(&id).cloned())
            .or_else(|| conv_meta_entry.and_then(|m| m.cwd.clone()))
            .unwrap_or_default();
        if cwd.is_empty() {
            cwd = get_history_fallback(&id).0.unwrap_or_default();
        }
        let default_title = if format_version == "brain-offline" {
            id.clone()
        } else {
            "未命名会话".into()
        };
        let title = overlay
            .title
            .filter(|s| !s.trim().is_empty())
            .or_else(|| conv_meta_entry.and_then(|m| m.title.clone()))
            .or(meta_file_title)
            .or(first_title)
            .or_else(|| get_history_fallback(&id).1)
            .unwrap_or(default_title);
        let effective_steps = if step_count > 0 {
            step_count
        } else {
            db_meta_entry.map_or(0, |m| m.step_count)
        };
        let tree_locked =
            locked_ids.contains(&id) || descendants.iter().any(|d| locked_ids.contains(d));
        let summary = make_summary_with_status(
            paths,
            id.clone(),
            title,
            cwd,
            archived,
            updated_at,
            effective_steps,
            last_preview,
            raw_status,
            format_version,
            tree_locked,
        );
        by_id.insert(id, summary);
    }

    Ok(by_id.into_values().collect())
}

/// The index stores `2026-01-02 03:04:05.678+00:00`; the live service reports
/// RFC 3339. One form keeps merged lists ordered and displayable.
fn index_timestamp(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with("0001-") || raw.starts_with("0000-") {
        return None;
    }
    let parsed = raw
        .parse::<jiff::Timestamp>()
        .or_else(|_| jiff::Timestamp::strptime("%Y-%m-%d %H:%M:%S%.f%:z", raw))
        .ok();
    match parsed {
        Some(ts) if ts.as_millisecond() > 86_400_000 => {
            Some(crate::timestamps::format_utc(ts, true))
        }
        Some(_) => None,
        None => cli_meta::parse_valid_timestamp_ms(raw).and_then(cli_meta::format_ms_utc),
    }
}

pub(crate) struct ValueOrString(pub(crate) bool);
impl rusqlite::types::FromSql for ValueOrString {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match value {
            rusqlite::types::ValueRef::Integer(i) => Ok(Self(i != 0)),
            rusqlite::types::ValueRef::Text(bytes) => {
                let s = std::str::from_utf8(bytes).unwrap_or("");
                Ok(Self(s == "1" || s.eq_ignore_ascii_case("true")))
            }
            _ => Ok(Self(false)),
        }
    }
}

fn list_all_summaries(paths: &AntigravityPaths) -> Result<Vec<ProviderSessionSummary>> {
    list_summaries_filtered(paths, None)
}

fn list_summaries_filtered(
    paths: &AntigravityPaths,
    filter_id: Option<&str>,
) -> Result<Vec<ProviderSessionSummary>> {
    if let Some(id) = filter_id {
        if let Some(cli) = paths.cli_store_for_id(id) {
            return list_offline_summaries(&cli, Some(id));
        }
    }
    let primary = paths.primary_store();
    let endpoint = connect_endpoint(&primary)?;
    list_summaries_from_source(
        paths,
        filter_id,
        endpoint.as_ref().map(|(_, payload)| payload),
    )
}

pub(crate) fn list_summaries_from_source(
    paths: &AntigravityPaths,
    filter_id: Option<&str>,
    payload: Option<&Value>,
) -> Result<Vec<ProviderSessionSummary>> {
    let primary = paths.primary_store();
    let cli = paths.cli_store();
    let cli_subagents = cli
        .as_ref()
        .map(collect_store_known_subagents)
        .unwrap_or_default();
    let primary_subagents = if payload.is_some() {
        collect_store_known_subagents(&primary)
    } else {
        BTreeSet::new()
    };
    let now_ms = now_epoch_ms();
    let mut items = if let Some(payload) = payload {
        let map = payload["trajectorySummaries"].as_object();
        let sub_states = map.map(subagent_states_by_root).unwrap_or_default();
        let mut items = map
            .into_iter()
            .flatten()
            .filter(|(id, _)| {
                filter_id.is_none_or(|target| target == id.as_str())
                    && paths.cli_store_for_id(id).is_none()
                    && !cli_subagents.contains(id.as_str())
                    && !primary_subagents.contains(id.as_str())
            })
            .filter_map(|(id, item)| {
                let (exec_subs, wait_subs) = sub_states.get(id).copied().unwrap_or((false, false));
                summary_from_live_json(&primary, id, item, exec_subs, wait_subs, now_ms)
            })
            .collect::<Vec<_>>();
        // The live service reports only conversations it has loaded this run.
        // The on-disk index lists every conversation, so add the roots the
        // service has not loaded yet; a live entry always wins.
        let loaded = map
            .map(|map| map.keys().map(String::as_str).collect::<BTreeSet<_>>())
            .unwrap_or_default();
        items.extend(
            list_offline_summaries(&primary, filter_id)
                .unwrap_or_default()
                .into_iter()
                // An index row whose native store is gone was deleted through
                // the service and must not reappear from the index.
                .filter(|item| {
                    !loaded.contains(item.id.as_str())
                        && paths.cli_store_for_id(&item.id).is_none()
                        && !cli_subagents.contains(&item.id)
                        && has_primary_session_storage(&primary, &item.id)
                }),
        );
        items
    } else {
        list_offline_summaries(&primary, filter_id)?
            .into_iter()
            .filter(|item| {
                paths.cli_store_for_id(&item.id).is_none() && !cli_subagents.contains(&item.id)
            })
            .collect()
    };
    if let Some(cli) = cli {
        let seen: BTreeSet<String> = items.iter().map(|i| i.id.clone()).collect();
        items.extend(
            list_offline_summaries(&cli, filter_id)
                .unwrap_or_default()
                .into_iter()
                .filter(|item| !seen.contains(&item.id) && !primary_subagents.contains(&item.id)),
        );
    }
    items.sort_by(|a, b| {
        b.updated_at
            .as_deref()
            .unwrap_or("")
            .cmp(a.updated_at.as_deref().unwrap_or(""))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(items)
}

pub fn list_sessions(
    paths: &AntigravityPaths,
    request: &ProviderListRequest,
) -> Result<Vec<ProviderSessionSummary>> {
    let mut items = list_sessions_without_storage(paths, request)?;
    for item in &mut items {
        let store = paths.store_for_id(&item.id, None);
        attach_storage_size(&store, item);
    }
    Ok(items)
}

pub(crate) fn list_sessions_without_storage(
    paths: &AntigravityPaths,
    request: &ProviderListRequest,
) -> Result<Vec<ProviderSessionSummary>> {
    let limit = request.limit.unwrap_or(200).clamp(1, 1000);
    let all = list_all_summaries(paths)?;
    Ok(all
        .into_iter()
        .filter(|item| match request.archived {
            Some(flag) => item.archived == flag,
            None => true,
        })
        .filter(|item| matches_query(item, request.q.as_deref()))
        .take(limit)
        .collect())
}

pub fn session_summary(paths: &AntigravityPaths, key: &str) -> Result<ProviderSessionSummary> {
    let mut summary = session_summary_without_storage(paths, key)?;
    let store = paths.store_for_id(&summary.id, None);
    attach_storage_size(&store, &mut summary);
    Ok(summary)
}

pub(crate) fn session_summary_without_storage(
    paths: &AntigravityPaths,
    key: &str,
) -> Result<ProviderSessionSummary> {
    valid_cascade_id(key)?;
    list_summaries_filtered(paths, Some(key))?
        .into_iter()
        .next()
        .with_context(|| format!("未找到 Antigravity 会话：{key}"))
}

pub(crate) fn probe_live_stream_snapshot(
    paths: &AntigravityPaths,
    id: &str,
    now_ms: i64,
) -> Option<crate::native_probe::NativeStreamSnapshot> {
    valid_cascade_id(id).ok()?;
    if let Some(cli) = paths.cli_store_for_id(id) {
        let summary = list_offline_summaries(&cli, Some(id))
            .ok()?
            .into_iter()
            .next()?;
        return build_offline_stream_snapshot(&cli, &summary);
    }
    let primary = paths.primary_store();
    let Some((rpc, all_live)) = connect_endpoint(&primary).ok().flatten() else {
        let summary = list_offline_summaries(&primary, Some(id))
            .ok()?
            .into_iter()
            .next()?;
        return build_offline_stream_snapshot(&primary, &summary);
    };
    let Some(summary) = all_live["trajectorySummaries"].get(id) else {
        let offline = list_offline_summaries(&primary, Some(id))
            .ok()?
            .into_iter()
            .next()?;
        return build_offline_stream_snapshot(&primary, &offline);
    };
    let (mut resp, steps) = match fetch_live_trajectory_steps(&rpc, id) {
        Ok(pair) => pair,
        Err(err) if err.is::<LiveHistoryUnavailable>() => {
            let offline = list_offline_summaries(&primary, Some(id))
                .ok()?
                .into_iter()
                .next()?;
            return build_offline_stream_snapshot(&primary, &offline);
        }
        Err(_) => return None,
    };
    resp["trajectory"]["steps"] = Value::Array(steps);
    let subagents = all_live["trajectorySummaries"]
        .as_object()
        .and_then(|m| collect_subagents_by_root(m).remove(id))
        .unwrap_or_default();
    let ctx = super::TrajectoryLivenessContext {
        has_active_subagents: subagents.values().any(|s| s.active),
        has_active_child_processes: false,
        now_ms: Some(now_ms),
        subagents,
        tasks: BTreeMap::new(),
    };
    super::snapshot_with_context(id, summary, &resp, &ctx).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn batch_preview_reads_live_list_once_and_preserves_per_item_results() {
        let root = env::temp_dir().join(format!("agenthub-ag-batch-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("conversations")).unwrap();
        let ids: Vec<_> = (0..3).map(|_| uuid::Uuid::new_v4().to_string()).collect();
        let mut summaries = serde_json::Map::new();
        for (n, id) in ids.iter().enumerate() {
            fs::write(
                root.join("conversations").join(format!("{id}.db")),
                b"fixture",
            )
            .unwrap();
            summaries.insert(id.clone(),json!({"summary":"Fixture","stepCount":2,"status":if n==1 {"CASCADE_RUN_STATUS_RUNNING"} else {"CASCADE_RUN_STATUS_IDLE"},"lastModifiedTime":"2026-01-01T00:00:00Z"}));
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.contains("GetAllCascadeTrajectories"));
            let mut len = 0;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        len = value.trim().parse().unwrap();
                    }
                }
            }
            reader.read_exact(&mut vec![0; len]).unwrap();
            let body = json!({"trajectorySummaries":summaries}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        });
        let paths = AntigravityPaths::with_fixed_rpc(root.clone(), port, "fixture-token");
        let result = preview_batch(&paths, &ids, SessionOperation::Delete, true);
        worker.join().unwrap();
        assert!(result[0].is_ok());
        assert!(result[1].is_err());
        assert!(result[2].is_ok());
        assert_eq!(result[0].as_ref().unwrap().id, ids[0]);
        let offline = AntigravityPaths::offline(root.clone());
        // Existing offline fixture behavior is exercised by the mutation tests.
        assert!(preview_batch(
            &offline,
            &["invalid".into()],
            SessionOperation::Delete,
            true
        )[0]
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn offline_sqlite_pbtxt_and_transcript_support_all_five_actions() {
        let root = env::temp_dir().join(format!("agenthub-ag-offline-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("conversations")).unwrap();
        fs::create_dir_all(root.join("annotations")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let sub_id = uuid::Uuid::new_v4().to_string();
        let logs_dir = root.join("brain").join(&id).join(".system_generated/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::create_dir_all(root.join("brain").join(&sub_id)).unwrap();
        fs::write(
            root.join("conversations").join(format!("{id}.db")),
            b"sqlite-placeholder",
        )
        .unwrap();
        fs::write(
            root.join("conversations").join(format!("{sub_id}.db")),
            b"sub-placeholder",
        )
        .unwrap();
        fs::write(root.join("agyhub_summaries_proto.pb"), b"cached").unwrap();
        fs::write(
            logs_dir.join("transcript_full.jsonl"),
            concat!(
                r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-28T01:00:00Z","content":"重构前端界面"}"#,
                "\n",
                r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-10-28T01:00:05Z","thinking":"先分析样式系统","tool_calls":[{"name":"view_file","arguments":{"Path":"/tmp/demo.css"}}],"content":"已完成样式重构。"}"#,
                "\n"
            ),
        )
        .unwrap();

        let conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT,
                preview TEXT,
                step_count INTEGER,
                last_modified_time TEXT,
                workspace_uris TEXT,
                parent_conversation_id TEXT,
                nesting_depth INTEGER
            );
            CREATE TABLE conversations_fts (
                conversation_id TEXT,
                chunk_index INTEGER,
                archived TEXT,
                title TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries VALUES (?1, '初始标题', '已完成样式重构', 2, '2026-10-28T01:00:05Z', '[\"file:///workspace/demo\"]', '', 0)",
            params![id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries VALUES (?1, '子代理', 'sub', 1, '2026-10-28T01:00:04Z', '', ?2, 1)",
            params![sub_id, id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversations_fts VALUES (?1, -1, '0', '初始标题')",
            params![id],
        )
        .unwrap();
        drop(conn);

        let paths = AntigravityPaths::offline(root.clone());
        let active = list_sessions(
            &paths,
            &ProviderListRequest {
                archived: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id);
        assert_eq!(active[0].title, "初始标题");
        assert_eq!(active[0].cwd, "/workspace/demo");

        // Detail & timeline
        let detail = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: id.clone(),
                agent_id: None,
                limit: Some(50),
                before: None,
            },
        )
        .unwrap();
        assert_eq!(detail.events.len(), 4);
        assert_eq!(detail.events[0].kind, "user_message");
        assert_eq!(detail.events[1].kind, "thinking");
        assert_eq!(detail.events[2].kind, "tool_call");
        assert_eq!(detail.events[3].kind, "assistant_message");
        assert_eq!(detail.timeline.as_ref().unwrap().len(), 1);
        let cached = load_all_events(&paths, &id).unwrap();
        assert!(Arc::ptr_eq(&cached, &load_all_events(&paths, &id).unwrap()));
        let transcript = logs_dir.join("transcript_full.jsonl");
        let original = fs::read_to_string(&transcript).unwrap();
        fs::write(
            &transcript,
            original.replace("重构前端界面", "更新前端界面"),
        )
        .unwrap();
        let refreshed = load_all_events(&paths, &id).unwrap();
        assert!(!Arc::ptr_eq(&cached, &refreshed));
        assert_eq!(refreshed[0].text.as_deref(), Some("更新前端界面"));

        // Rename (with non-ASCII UTF-8 octal prototext round-trip)
        let renamed = rename_session(&paths, &id, "ChatGPT 风格新标题").unwrap();
        assert_eq!(renamed.title, "ChatGPT 风格新标题");
        assert!(!paths.proto_cache().exists());

        // Archive & Restore
        let archived_item = set_archived(&paths, &id, true).unwrap();
        assert!(archived_item.archived);
        assert_eq!(
            list_sessions(
                &paths,
                &ProviderListRequest {
                    archived: Some(false),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            0
        );
        assert_eq!(
            list_sessions(
                &paths,
                &ProviderListRequest {
                    archived: Some(true),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            1
        );

        let restored = set_archived(&paths, &id, false).unwrap();
        assert!(!restored.archived);

        // Permanent delete (including subagent descendant)
        let preview = preview_batch_item(&paths, &id, SessionOperation::Delete, true).unwrap();
        assert!(preview.allowed);
        assert_eq!(preview.descendants, vec![sub_id.clone()]);
        let del = execute_batch_item(
            &paths,
            &id,
            SessionOperation::Delete,
            &preview.descendants,
            true,
        )
        .unwrap();
        assert_eq!(del.status, "succeeded");
        assert!(del.residuals.is_empty());
        assert!(!root.join("brain").join(&id).exists());
        assert!(!root.join("brain").join(&sub_id).exists());
        assert!(!root
            .join("annotations")
            .join(format!("{id}.pbtxt"))
            .exists());
        assert!(list_sessions(&paths, &ProviderListRequest::default())
            .unwrap()
            .is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_connect_rpc_mode_handles_list_detail_rename_archive_and_delete_sweep() {
        let root = env::temp_dir().join(format!("agenthub-ag-live-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("annotations")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        // Simulate an orphaned .pbtxt left on disk that post-RPC sweep must clean up
        fs::write(
            root.join("annotations").join(format!("{id}.pbtxt")),
            "title: \"orphan\"\n",
        )
        .unwrap();

        let state = Arc::new(Mutex::new((
            "初始会话".to_string(),
            false,
            false,
            false,
            0_u8,
        )));
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server_reads = reads.clone();
        let server_state = state.clone();
        let server_id = id.clone();
        let handle = std::thread::spawn(move || {
            while !server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let json_body: Value = serde_json::from_slice(&body).unwrap_or_default();
                let mut guard = server_state.lock().unwrap();
                let resp = if req_line.contains("GetAllCascadeTrajectories") {
                    if guard.2 {
                        json!({ "trajectorySummaries": {} })
                    } else {
                        json!({
                            "trajectorySummaries": {
                                (&server_id): {
                                    "summary": format!("默认摘要 {}", guard.4),
                                    "stepCount": 2,
                                    "lastModifiedTime": "2026-10-28T02:00:00Z",
                                    "status": if guard.3 { "CASCADE_RUN_STATUS_BUSY" } else { "CASCADE_RUN_STATUS_IDLE" },
                                    "annotations": {
                                        "title": guard.0,
                                        "archived": guard.1
                                    },
                                    "workspaces": [{ "workspaceFolderAbsoluteUri": "file:///tmp/proj" }]
                                }
                            }
                        })
                    }
                } else if req_line.contains("GetCascadeTrajectory ") {
                    server_reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if guard.4 == 5 {
                        json!({"trajectory":{"cascadeId":server_id}})
                    } else if guard.4 == 6 {
                        json!({})
                    } else if guard.4 == 7 {
                        json!({"numTotalSteps":1,"trajectory":{"cascadeId":server_id}})
                    } else {
                        json!({
                            "numTotalSteps": if [1, 3, 4].contains(&guard.4) { 3 } else { 2 },
                            "trajectory": {
                                "cascadeId": if guard.4 == 2 { "fixture-mismatched" } else { &server_id },
                                "steps": [
                                    {
                                        "type": "CORTEX_STEP_TYPE_USER_INPUT",
                                        "status": "CORTEX_STEP_STATUS_DONE",
                                        "userInput": { "items": [{ "text": "你好 Antigravity" }] }
                                    },
                                    {
                                        "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                                        "status": "CORTEX_STEP_STATUS_DONE",
                                        "plannerResponse": { "response": "你好！我是 Antigravity。" }
                                    }
                                ]
                            }
                        })
                    }
                } else if req_line.contains("GetCascadeTrajectorySteps") {
                    assert_eq!(json_body["stepOffset"], 2);
                    match guard.4 {
                        3 => json!({"steps": [
                            {"type":"CORTEX_STEP_TYPE_USER_INPUT", "userInput":{"items":[{"text":"追加请求"}]}},
                            {"type":"CORTEX_STEP_TYPE_PLANNER_RESPONSE", "plannerResponse":{"response":"追加回复"}}
                        ]}),
                        4 => json!({"steps":"invalid"}),
                        _ => json!({"steps":[]}),
                    }
                } else if req_line.contains("UpdateConversationAnnotations") {
                    if let Some(t) = json_body["annotations"]["title"].as_str() {
                        guard.0 = t.to_owned();
                    }
                    if let Some(a) = json_body["annotations"]["archived"].as_bool() {
                        guard.1 = a;
                    }
                    json!({})
                } else if req_line.contains("DeleteCascadeTrajectory") {
                    guard.2 = true;
                    json!({})
                } else {
                    json!({})
                };
                drop(guard);
                let bytes = serde_json::to_vec(&resp).unwrap();
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&bytes);
                if req_line.contains("DeleteCascadeTrajectory") {
                    // One more GetAllCascadeTrajectories check may follow
                }
            }
        });

        let paths = AntigravityPaths::with_fixed_rpc(root.clone(), port, "test_token_123");
        let list = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "初始会话");

        let detail = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: id.clone(),
                agent_id: None,
                limit: None,
                before: None,
            },
        )
        .unwrap();
        assert_eq!(detail.events.len(), 2);
        let cached = load_all_events(&paths, &id).unwrap();
        assert!(Arc::ptr_eq(&cached, &load_all_events(&paths, &id).unwrap()));
        assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 1);
        state.lock().unwrap().3 = true;
        load_all_events(&paths, &id).unwrap();
        load_all_events(&paths, &id).unwrap();
        assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 3);
        state.lock().unwrap().3 = false;
        load_all_events(&paths, &id).unwrap();
        load_all_events(&paths, &id).unwrap();
        assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 4);
        let other_owner =
            AntigravityPaths::with_fixed_rpc(root.clone(), port, "fixture-other-owner");
        load_all_events(&other_owner, &id).unwrap();
        assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 4);

        // Incomplete pages and a mismatched identity must never populate the live cache.
        for invalid in [1, 2, 4, 7] {
            state.lock().unwrap().4 = invalid;
            let before = reads.load(std::sync::atomic::Ordering::Relaxed);
            assert!(load_all_events(&paths, &id).is_err());
            assert!(load_all_events(&paths, &id).is_err());
            assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), before + 2);
        }
        for empty in [5, 6] {
            state.lock().unwrap().4 = empty;
            assert!(load_all_events(&paths, &id).unwrap().is_empty());
        }
        state.lock().unwrap().4 = 3;
        assert_eq!(load_all_events(&paths, &id).unwrap().len(), 4);
        state.lock().unwrap().4 = 0;

        let renamed = rename_session(&paths, &id, "RPC 改名成功").unwrap();
        assert_eq!(renamed.title, "RPC 改名成功");

        let archived = set_archived(&paths, &id, true).unwrap();
        assert!(archived.archived);

        let del = execute_batch_item(&paths, &id, SessionOperation::Delete, &[], true).unwrap();
        assert_eq!(del.status, "succeeded");
        assert!(!root
            .join("annotations")
            .join(format!("{id}.pbtxt"))
            .exists());
        drop(paths);
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", port));
        let _ = handle.join();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_loopback_listener_port_accepts_loopback_only() {
        assert_eq!(
            parse_loopback_listener_port("n127.0.0.1:51234"),
            Some(51234)
        );
        assert_eq!(parse_loopback_listener_port("n0.0.0.0:51234"), None);
        assert_eq!(parse_loopback_listener_port("n192.0.2.10:51234"), None);
        assert_eq!(parse_loopback_listener_port("n[::1]:51234"), None);
    }

    #[test]
    fn delete_rejects_forged_cross_root_descendants_locked_targets_and_halts_on_partial_failure() {
        let root = env::temp_dir().join(format!("agenthub-ag-guard-{}", uuid::Uuid::new_v4()));
        for dir in ["conversations", "annotations", "presence"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let root_a = uuid::Uuid::new_v4().to_string();
        let sub_a = uuid::Uuid::new_v4().to_string();
        let root_b = uuid::Uuid::new_v4().to_string();
        let sub_b = uuid::Uuid::new_v4().to_string();
        for id in [&root_a, &sub_a, &root_b, &sub_b] {
            fs::create_dir_all(root.join("brain").join(id)).unwrap();
            fs::write(root.join("brain").join(id).join("note.md"), b"fixture").unwrap();
            fs::write(
                root.join("conversations").join(format!("{id}.db")),
                b"sqlite-fixture",
            )
            .unwrap();
        }
        let conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT,
                preview TEXT,
                step_count INTEGER,
                last_modified_time TEXT,
                workspace_uris TEXT,
                parent_conversation_id TEXT,
                nesting_depth INTEGER
            );
            CREATE TABLE conversations_fts (
                conversation_id TEXT,
                chunk_index INTEGER,
                archived TEXT,
                title TEXT
            );",
        )
        .unwrap();
        for (cid, title, parent, depth) in [
            (&root_a, "主会话 A", "", 0),
            (&sub_a, "子代理 A", root_a.as_str(), 1),
            (&root_b, "主会话 B", "", 0),
            (&sub_b, "子代理 B", root_b.as_str(), 1),
        ] {
            conn.execute(
                "INSERT INTO conversation_summaries VALUES (?1, ?2, 'p', 1, '2026-10-28T01:00:00Z', '', ?3, ?4)",
                params![cid, title, parent, depth],
            )
            .unwrap();
        }
        drop(conn);

        let paths = AntigravityPaths::offline(root.clone());
        let confirmed =
            preview_batch_item(&paths, &root_a, SessionOperation::Delete, true).unwrap();
        assert_eq!(confirmed.descendants, vec![sub_a.clone()]);
        let late = uuid::Uuid::new_v4().to_string();
        fs::create_dir_all(root.join("brain").join(&late)).unwrap();
        Connection::open(paths.summaries_db()).unwrap().execute(
            "INSERT INTO conversation_summaries VALUES (?1,'late','p',1,'2026-10-28T01:00:00Z','',?2,1)",
            params![late,root_a]).unwrap();

        // 1. Forged other-root UUID in descendants is rejected
        assert!(execute_batch_item(
            &paths,
            &root_a,
            SessionOperation::Delete,
            std::slice::from_ref(&root_b),
            true,
        )
        .is_err());
        assert!(root
            .join("conversations")
            .join(format!("{root_b}.db"))
            .exists());

        // 2. Cross-root subagent UUID in descendants is rejected
        assert!(execute_batch_item(
            &paths,
            &root_a,
            SessionOperation::Delete,
            std::slice::from_ref(&sub_b),
            true,
        )
        .is_err());
        assert!(root
            .join("conversations")
            .join(format!("{sub_b}.db"))
            .exists());

        // 3. Descendants supplied when include_descendants == false is rejected
        assert!(execute_batch_item(
            &paths,
            &root_a,
            SessionOperation::Delete,
            std::slice::from_ref(&sub_a),
            false,
        )
        .is_err());

        // 4. Presence flock on descendant blocks preview & execute
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let lock_path = root.join("presence").join(format!("{sub_a}.lock"));
            let lock_file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            assert!(preview_batch_item(&paths, &root_a, SessionOperation::Delete, true).is_err());
            assert!(execute_batch_item(
                &paths,
                &root_a,
                SessionOperation::Delete,
                std::slice::from_ref(&sub_a),
                true
            )
            .is_err());
            assert!(root
                .join("conversations")
                .join(format!("{root_a}.db"))
                .exists());
            assert!(root
                .join("conversations")
                .join(format!("{sub_a}.db"))
                .exists());
            unsafe {
                let _ = libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN);
            }
        }

        // 5. Partial filesystem failure on sub_a halts immediately before touching root_a
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let sub_brain = root.join("brain").join(&sub_a);
            fs::set_permissions(&sub_brain, fs::Permissions::from_mode(0o500)).unwrap();
            let partial = execute_batch_item(
                &paths,
                &root_a,
                SessionOperation::Delete,
                std::slice::from_ref(&sub_a),
                true,
            )
            .unwrap();
            assert_eq!(partial.status, "failed");
            assert!(partial.deleted_ids.is_empty());
            assert!(!partial.residuals.is_empty());
            assert!(root
                .join("conversations")
                .join(format!("{root_a}.db"))
                .exists());
            assert!(root.join("brain").join(&root_a).exists());
            // root_a remains visible in list_sessions for retry!
            assert!(list_sessions(&paths, &ProviderListRequest::default())
                .unwrap()
                .iter()
                .any(|s| s.id == root_a));

            fs::set_permissions(&sub_brain, fs::Permissions::from_mode(0o700)).unwrap();
            let retried = execute_batch_item(
                &paths,
                &root_a,
                SessionOperation::Delete,
                std::slice::from_ref(&sub_a),
                true,
            )
            .unwrap();
            assert_eq!(retried.status, "succeeded");
            assert!(!root.join("brain").join(&root_a).exists());
            assert!(!root.join("brain").join(&sub_a).exists());
            assert!(root.join("brain").join(&late).exists());
            assert!(root.join("brain").join(&root_b).exists());
            assert!(root.join("brain").join(&sub_b).exists());
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_list_adds_indexed_roots_the_service_has_not_loaded() {
        let root = env::temp_dir().join(format!("agenthub-ag-merge-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("conversations")).unwrap();
        let loaded = uuid::Uuid::new_v4().to_string();
        let unloaded = uuid::Uuid::new_v4().to_string();
        let child = uuid::Uuid::new_v4().to_string();
        let removed = uuid::Uuid::new_v4().to_string();
        for id in [&loaded, &unloaded, &child] {
            fs::write(
                root.join("conversations").join(format!("{id}.db")),
                b"sqlite-fixture",
            )
            .unwrap();
        }
        let paths = AntigravityPaths::offline(root.clone());
        let conn = Connection::open(paths.summaries_db()).unwrap();
        conn.execute_batch("CREATE TABLE conversation_summaries(conversation_id TEXT PRIMARY KEY,title TEXT,preview TEXT,step_count INTEGER,last_modified_time TEXT,workspace_uris TEXT,parent_conversation_id TEXT,nesting_depth INTEGER);").unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries VALUES \
             (?1,'Indexed loaded','',2,'2026-01-02 03:04:05.5+00:00','',NULL,0),\
             (?2,'Indexed only','',7,'2026-01-03 03:04:05.678901+00:00','',NULL,0),\
             (?3,'Child','',1,'2026-01-04 00:00:00+00:00','',?2,1),\
             (?4,'Removed','',1,'2026-01-05 00:00:00+00:00','',NULL,0)",
            params![loaded, unloaded, child, removed],
        )
        .unwrap();
        drop(conn);
        let mut summaries = serde_json::Map::new();
        summaries.insert(
            loaded.clone(),
            json!({"summary":"Live loaded","stepCount":3,"status":"CASCADE_RUN_STATUS_RUNNING","lastModifiedTime":"2026-01-02T03:04:05Z"}),
        );
        let payload = json!({ "trajectorySummaries": summaries });

        let items = list_summaries_from_source(&paths, None, Some(&payload)).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            [unloaded.as_str(), loaded.as_str()]
        );
        assert_eq!(items[0].title, "Indexed only");
        assert_eq!(items[0].status, "idle");
        assert_eq!(items[0].message_count, 7);
        assert_eq!(
            items[0].updated_at.as_deref(),
            Some("2026-01-03T03:04:05.678901Z")
        );
        // The service's own entry wins over the index row for the same ID.
        assert_eq!(items[1].title, "Live loaded");
        assert_eq!(items[1].status, "running");

        let only = list_summaries_from_source(&paths, Some(&unloaded), Some(&payload)).unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].id, unloaded);
        assert!(
            list_summaries_from_source(&paths, Some(&child), Some(&payload))
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_with_no_confirmed_descendants_keeps_new_children() {
        let root = env::temp_dir().join(format!("agenthub-ag-fixed-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let child = uuid::Uuid::new_v4().to_string();
        fs::create_dir_all(root.join("brain").join(&id)).unwrap();
        let paths = AntigravityPaths::offline(root.clone());
        let preview = preview_batch_item(&paths, &id, SessionOperation::Delete, true).unwrap();
        assert!(preview.descendants.is_empty());
        let conn = Connection::open(paths.summaries_db()).unwrap();
        conn.execute_batch("CREATE TABLE conversation_summaries(conversation_id TEXT PRIMARY KEY,title TEXT,preview TEXT,step_count INTEGER,last_modified_time TEXT,workspace_uris TEXT,parent_conversation_id TEXT,nesting_depth INTEGER);
            CREATE TABLE conversations_fts(conversation_id TEXT,chunk_index INTEGER,archived TEXT,title TEXT);").unwrap();
        conn.execute("INSERT INTO conversation_summaries VALUES (?1,'root','',0,'','',NULL,0),(?2,'child','',0,'','',?1,1)", params![id,child]).unwrap();
        fs::create_dir_all(root.join("brain").join(&child)).unwrap();
        let result = execute_batch_item(
            &paths,
            &id,
            SessionOperation::Delete,
            &preview.descendants,
            true,
        )
        .unwrap();
        assert_eq!(result.deleted_ids, vec![id]);
        assert!(root.join("brain").join(child).exists());
        drop(conn);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn online_delete_blocks_running_subtrajectory_and_halts_on_subagent_rpc_error() {
        let root = env::temp_dir().join(format!("agenthub-ag-rpc-guard-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("conversations")).unwrap();
        let root_id = uuid::Uuid::new_v4().to_string();
        let sub_id = uuid::Uuid::new_v4().to_string();
        for id in [&root_id, &sub_id] {
            fs::create_dir_all(root.join("brain").join(id)).unwrap();
            fs::write(
                root.join("conversations").join(format!("{id}.db")),
                b"sqlite-fixture",
            )
            .unwrap();
        }

        // (sub_busy, fail_sub_delete)
        let mode = Arc::new(Mutex::new((true, true)));
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_mode = mode.clone();
        let s_root = root_id.clone();
        let s_sub = sub_id.clone();
        let handle = std::thread::spawn(move || {
            while !server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let json_body: Value = serde_json::from_slice(&body).unwrap_or_default();
                let (sub_busy, fail_sub_delete) = *server_mode.lock().unwrap();
                let (status_code, resp) = if req_line.contains("GetAllCascadeTrajectories") {
                    let exclude_sub = json_body["excludeSubtrajectories"]
                        .as_bool()
                        .unwrap_or(true);
                    let mut summaries = serde_json::Map::new();
                    summaries.insert(
                        s_root.clone(),
                        json!({
                            "summary": "主会话",
                            "stepCount": 2,
                            "status": "CASCADE_RUN_STATUS_IDLE"
                        }),
                    );
                    if !exclude_sub {
                        summaries.insert(
                            s_sub.clone(),
                            json!({
                                "summary": "子代理",
                                "stepCount": 1,
                                "parentConversationId": s_root,
                                "trajectoryMetadata": { "rootConversationId": s_root, "nestingDepth": 1 },
                                "status": if sub_busy { "CASCADE_RUN_STATUS_BUSY" } else { "CASCADE_RUN_STATUS_IDLE" }
                            }),
                        );
                    }
                    (200, json!({ "trajectorySummaries": summaries }))
                } else if req_line.contains("DeleteCascadeTrajectory") {
                    if json_body["cascadeId"].as_str() == Some(s_sub.as_str()) && fail_sub_delete {
                        (500, json!({ "message": "subagent busy in runtime" }))
                    } else {
                        (200, json!({}))
                    }
                } else {
                    (200, json!({}))
                };
                let bytes = serde_json::to_vec(&resp).unwrap();
                let status_text = if status_code == 200 {
                    "200 OK"
                } else {
                    "500 Internal Server Error"
                };
                let header = format!(
                    "HTTP/1.1 {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&bytes);
            }
        });

        let paths = AntigravityPaths::with_fixed_rpc(root.clone(), port, "test_token_123");

        // 1. Subagent BUSY blocks both preview and execute even though root summary is IDLE
        assert!(preview_batch_item(&paths, &root_id, SessionOperation::Delete, true).is_err());
        assert!(execute_batch_item(
            &paths,
            &root_id,
            SessionOperation::Delete,
            std::slice::from_ref(&sub_id),
            true
        )
        .is_err());
        assert!(root
            .join("conversations")
            .join(format!("{sub_id}.db"))
            .exists());

        // 2. Subagent IDLE, but DeleteCascadeTrajectory on sub_id returns HTTP 500 -> must NOT delete disk files!
        *mode.lock().unwrap() = (false, true);
        let failed = execute_batch_item(
            &paths,
            &root_id,
            SessionOperation::Delete,
            std::slice::from_ref(&sub_id),
            true,
        )
        .unwrap();
        assert_eq!(failed.status, "failed");
        assert!(failed.deleted_ids.is_empty());
        assert!(!failed.residuals.is_empty());
        assert!(root
            .join("conversations")
            .join(format!("{sub_id}.db"))
            .exists());
        assert!(root.join("brain").join(&sub_id).exists());
        assert!(root
            .join("conversations")
            .join(format!("{root_id}.db"))
            .exists());
        assert!(root.join("brain").join(&root_id).exists());

        // 3. Once RPC succeeds, both sub_id and root_id are cleanly deleted
        *mode.lock().unwrap() = (false, false);
        let ok = execute_batch_item(
            &paths,
            &root_id,
            SessionOperation::Delete,
            std::slice::from_ref(&sub_id),
            true,
        )
        .unwrap();
        assert_eq!(ok.status, "succeeded");
        assert!(ok.residuals.is_empty());
        assert!(!root
            .join("conversations")
            .join(format!("{sub_id}.db"))
            .exists());
        assert!(!root
            .join("conversations")
            .join(format!("{root_id}.db"))
            .exists());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", port));
        let _ = handle.join();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canonical_path_prefers_plain_transcript_handoff_files_over_sqlite_db() {
        let root =
            std::env::temp_dir().join(format!("agenthub-ag-handoff-{}", uuid::Uuid::new_v4()));
        let id = "11111111-1111-4111-8111-111111111111";
        let paths = AntigravityPaths::offline(root.clone());
        let logs = paths.brain_dir().join(id).join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        fs::create_dir_all(paths.conversations_dir()).unwrap();

        let db = paths.conversations_dir().join(format!("{id}.db"));
        fs::write(&db, b"sqlite").unwrap();
        assert_eq!(paths.canonical_path(id), db);

        let compact = logs.join("transcript.jsonl");
        fs::write(&compact, b"{}\n").unwrap();
        assert_eq!(paths.canonical_path(id), compact);

        let full = logs.join("transcript_full.jsonl");
        fs::write(&full, b"{}\n").unwrap();
        assert_eq!(paths.canonical_path(id), full);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extract_live_step_events_captures_artifact_approvals_and_item_mentions() {
        let id = "11111111-1111-4111-8111-111111111111";
        let steps = vec![
            json!({
                "type": "CORTEX_STEP_TYPE_USER_INPUT",
                "status": "CORTEX_STEP_STATUS_DONE",
                "metadata": { "executionId": "exec-1", "createdAt": "2026-10-29T01:00:00Z" },
                "userInput": {
                    "items": [
                        { "item": { "file": { "absolutePathUri": "file:///workspace/demo/plan.md" } } },
                        { "text": " 按这个推进" }
                    ]
                }
            }),
            json!({
                "type": "CORTEX_STEP_TYPE_USER_INPUT",
                "status": "CORTEX_STEP_STATUS_DONE",
                "metadata": { "executionId": "exec-2", "createdAt": "2026-10-29T01:05:00Z" },
                "userInput": {
                    "artifactComments": [
                        {
                            "artifactUri": "file:///workspace/demo/implementation_plan.md",
                            "approvalStatus": "ARTIFACT_APPROVAL_STATUS_APPROVED",
                            "fullFile": {}
                        }
                    ]
                }
            }),
        ];
        let events = extract_live_step_events(id, &steps);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "user_message");
        assert!(events[0]
            .text
            .as_deref()
            .unwrap_or("")
            .contains("@/workspace/demo/plan.md"));
        assert_eq!(events[1].kind, "user_message");
        assert_eq!(
            events[1].text.as_deref(),
            Some("批准文档：implementation_plan.md")
        );
    }

    #[test]
    fn idle_open_session_with_presence_lock_reports_idle_deletes_online_and_blocks_offline() {
        let root = env::temp_dir().join(format!("agenthub-ag-presence-{}", uuid::Uuid::new_v4()));
        for dir in ["conversations", "presence"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let id = uuid::Uuid::new_v4().to_string();
        let sub_id = uuid::Uuid::new_v4().to_string();
        let logs = root.join("brain").join(&id).join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            root.join("conversations").join(format!("{id}.db")),
            b"sqlite-fixture",
        )
        .unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-29T01:00:00Z","content":"已完成会话"}"#,
        )
        .unwrap();

        #[cfg(unix)]
        let _held_lock = {
            use std::os::unix::io::AsRawFd;
            let lock_path = root.join("presence").join(format!("{id}.lock"));
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            file
        };

        // 1. Offline list/detail reports "idle" even when presence/<id>.lock is held,
        //    while disabling offline mutations and reporting explicit open-in-Antigravity block reasons.
        let offline_paths = AntigravityPaths::offline(root.clone());
        let offline_list = list_sessions(&offline_paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(offline_list.len(), 1);
        assert_eq!(offline_list[0].status, "idle");
        #[cfg(unix)]
        {
            assert!(!offline_list[0].can_rename && offline_list[0].rename_block_reason.is_some());
            assert!(!offline_list[0].can_archive && offline_list[0].archive_block_reason.is_some());
            assert!(!offline_list[0].can_delete);
            assert!(offline_list[0]
                .delete_block_reason
                .as_deref()
                .unwrap_or("")
                .contains("已在 Antigravity 中打开"));
        }

        #[cfg(unix)]
        {
            let err = preview_batch_item(&offline_paths, &id, SessionOperation::Delete, true)
                .unwrap_err()
                .to_string();
            assert!(err.contains("会话当前已在 Antigravity 中打开"), "{err}");
            assert!(
                execute_batch_item(&offline_paths, &id, SessionOperation::Delete, &[], true)
                    .is_err()
            );
            assert!(root.join("conversations").join(format!("{id}.db")).exists());
        }

        // 2. Online mode marks root "running" iff root or a subagent is active in RPC;
        //    once RPC is IDLE, open presence lock does NOT mark it "running" and online delete succeeds.
        let sub_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let deleted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let s_root = id.clone();
        let s_sub = sub_id.clone();
        let s_sub_running = sub_running.clone();
        let s_deleted = deleted.clone();
        let handle = std::thread::spawn(move || {
            while !server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let resp = if req_line.contains("GetAllCascadeTrajectories") {
                    if s_deleted.load(std::sync::atomic::Ordering::Relaxed) {
                        json!({ "trajectorySummaries": {} })
                    } else {
                        let sub_status = if s_sub_running.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            "CASCADE_RUN_STATUS_RUNNING"
                        } else {
                            "CASCADE_RUN_STATUS_IDLE"
                        };
                        json!({
                            "trajectorySummaries": {
                                (&s_root): {
                                    "summary": "打开但已完成的会话",
                                    "stepCount": 4,
                                    "status": "CASCADE_RUN_STATUS_IDLE"
                                },
                                (&s_sub): {
                                    "summary": "子代理",
                                    "stepCount": 2,
                                    "parentConversationId": s_root,
                                    "status": sub_status
                                }
                            }
                        })
                    }
                } else if req_line.contains("DeleteCascadeTrajectory") {
                    s_deleted.store(true, std::sync::atomic::Ordering::Relaxed);
                    json!({})
                } else {
                    json!({})
                };
                let bytes = serde_json::to_vec(&resp).unwrap();
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&bytes);
            }
        });

        let online_paths = AntigravityPaths::with_fixed_rpc(root.clone(), port, "tok");
        let while_sub_active =
            list_sessions(&online_paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(while_sub_active[0].status, "running");

        sub_running.store(false, std::sync::atomic::Ordering::Relaxed);
        let once_idle = list_sessions(&online_paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(once_idle[0].status, "idle");

        let preview =
            preview_batch_item(&online_paths, &id, SessionOperation::Delete, true).unwrap();
        assert!(preview.allowed);
        let res =
            execute_batch_item(&online_paths, &id, SessionOperation::Delete, &[], true).unwrap();
        assert_eq!(res.status, "succeeded");
        assert!(!root.join("conversations").join(format!("{id}.db")).exists());

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", port));
        let _ = handle.join();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn offline_and_live_multi_step_turns_pair_generic_tool_outputs_and_unify_turn_ids() {
        let root = env::temp_dir().join(format!("agenthub-ag-fold-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let logs = root.join("brain").join(&id).join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            concat!(
                r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-29T01:00:00Z","content":"跑检查并提交"}"#,
                "\n",
                r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-10-29T01:00:02Z","thinking":"先编辑再运行命令","content":"正在应用修改并运行测试","tool_calls":[{"name":"edit_file","arguments":{"Path":"/workspace/demo.rs"}},{"name":"run_command","arguments":{"CommandLine":"cargo test"}}]}"#,
                "\n",
                r#"{"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","created_at":"2026-10-29T01:00:03Z","content":"Created At: 2026-10-29T01:00:03Z Completed At: 2026-10-29T01:00:03Z Made 1 replacement in /workspace/demo.rs."}"#,
                "\n",
                r#"{"step_index":3,"source":"MODEL","type":"GENERIC","status":"DONE","created_at":"2026-10-29T01:00:05Z","content":"Created At: 2026-10-29T01:00:03Z Completed At: 2026-10-29T01:00:05Z Command exited with code 0.\n\ntest result: ok."}"#,
                "\n",
                r#"{"step_index":4,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-10-29T01:00:06Z","content":"Background task finished"}"#,
                "\n",
                r#"{"step_index":5,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-10-29T01:00:08Z","thinking":"确认通过","content":"全部检查已通过。"}"#,
                "\n"
            ),
        )
        .unwrap();

        let paths = AntigravityPaths::offline(root.clone());
        let events = read_offline_transcript_events(&paths, &id).unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "user_message",
                "thinking",
                "thinking",
                "tool_call",
                "tool_call",
                "tool_result",
                "tool_result",
                "thinking",
                "assistant_message"
            ]
        );
        assert_eq!(events[3].call_id, events[5].call_id);
        assert_eq!(events[4].call_id, events[6].call_id);
        for ev in &events {
            assert_eq!(ev.turn_id.as_deref(), Some("turn-1"));
        }

        // Live multi-executor steps within one user turn also share turn-1 and fold intermediate commentary
        let live_steps = vec![
            json!({
                "type": "CORTEX_STEP_TYPE_USER_INPUT",
                "status": "CORTEX_STEP_STATUS_DONE",
                "metadata": { "executionId": "exec-a" },
                "userInput": { "items": [{ "text": "继续推进" }] }
            }),
            json!({
                "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                "status": "CORTEX_STEP_STATUS_DONE",
                "metadata": { "executionId": "exec-a" },
                "plannerResponse": {
                    "thinking": "派发任务",
                    "response": "等待任务完成",
                    "toolCalls": [{ "id": "call-1", "name": "run_command", "argumentsJson": "{}" }]
                }
            }),
            json!({
                "type": "CORTEX_STEP_TYPE_SYSTEM_MESSAGE",
                "status": "CORTEX_STEP_STATUS_DONE",
                "metadata": { "executionId": "exec-b" }
            }),
            json!({
                "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                "status": "CORTEX_STEP_STATUS_DONE",
                "metadata": { "executionId": "exec-b" },
                "plannerResponse": {
                    "thinking": "收到唤醒",
                    "response": "任务已完成。"
                }
            }),
        ];
        let live_events = extract_live_step_events(&id, &live_steps);
        let live_kinds: Vec<&str> = live_events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(
            live_kinds,
            vec![
                "user_message",
                "thinking",
                "thinking",
                "tool_call",
                "thinking",
                "assistant_message"
            ]
        );
        for ev in &live_events {
            assert_eq!(ev.turn_id.as_deref(), Some("turn-1"));
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_conversation_summaries_db_aborts_delete_preview_and_execute() {
        let root = env::temp_dir().join(format!("agenthub-ag-corrupt-db-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("conversations")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let logs = root.join("brain").join(&id).join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            root.join("conversations").join(format!("{id}.db")),
            b"sqlite-fixture",
        )
        .unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-29T01:00:00Z","content":"测试损坏索引"}"#,
        )
        .unwrap();
        fs::write(root.join("conversation_summaries.db"), b"not-a-sqlite-db").unwrap();

        let paths = AntigravityPaths::offline(root.clone());
        assert!(preview_batch_item(&paths, &id, SessionOperation::Delete, true).is_err());
        assert!(execute_batch_item(&paths, &id, SessionOperation::Delete, &[], true).is_err());
        assert!(root.join("conversations").join(format!("{id}.db")).exists());
        assert!(root.join("brain").join(&id).exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_history_uses_offline_only_for_unavailable_or_empty_content() {
        fn serve(responses: Vec<Value>) -> (u16, std::thread::JoinHandle<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let handle = std::thread::spawn(move || {
                for body in responses {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let mut len = 0;
                    loop {
                        line.clear();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                        if let Some((key, value)) = line.split_once(':') {
                            if key.eq_ignore_ascii_case("content-length") {
                                len = value.trim().parse().unwrap();
                            }
                        }
                    }
                    let mut request = vec![0; len];
                    reader.read_exact(&mut request).unwrap();
                    let body = body.to_string();
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
                }
            });
            (port, handle)
        }
        let root = env::temp_dir().join(format!(
            "agenthub-offline-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        let id = uuid::Uuid::new_v4().to_string();
        let paths = AntigravityPaths::offline(root.clone());
        let logs = paths.brain_dir().join(&id).join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(logs.join("transcript_full.jsonl"), r#"{"step_index":0,"type":"USER_INPUT","source":"USER_EXPLICIT","status":"DONE","content":"offline fixture"}"#).unwrap();
        let user = json!({"type":"CORTEX_STEP_TYPE_USER_INPUT","userInput":{"items":[{"text":"live fixture"}]}});
        let cases = [
            (
                vec![json!({"trajectory":{"cascadeId":id,"steps":[]},"numTotalSteps":0})],
                true,
            ),
            (
                vec![
                    json!({"trajectory":{"cascadeId":id,"steps":[{"type":"CORTEX_STEP_TYPE_SYSTEM_MESSAGE"}]},"numTotalSteps":1}),
                ],
                true,
            ),
            (
                vec![
                    json!({"trajectory":{"cascadeId":id,"steps":[user.clone()]},"numTotalSteps":2}),
                ],
                true,
            ),
            (
                vec![json!({"trajectory":{"cascadeId":"different","steps":[]}})],
                false,
            ),
            (
                vec![json!({"trajectory":{"cascadeId":id,"steps":"broken"}})],
                false,
            ),
            (
                vec![
                    json!({"trajectory":{"cascadeId":id,"steps":[user]},"numTotalSteps":2}),
                    json!({"cascadeId":id,"steps":[]}),
                ],
                false,
            ),
        ];
        for (responses, fallback) in cases {
            let (port, handle) = serve(responses);
            let result = load_events_from_source(
                &paths,
                &id,
                Some(&(
                    LiveRpc {
                        port,
                        token: "fixture".into(),
                    },
                    json!({}),
                )),
            );
            handle.join().unwrap();
            if fallback {
                assert_eq!(
                    result.unwrap().events[0].text.as_deref(),
                    Some("offline fixture")
                );
            } else {
                assert!(result.is_err());
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn endpoint_cache_skips_negative_cache_on_transient_rpc_error_and_evicts_on_idle() {
        evict_idle_endpoint_cache(std::time::Duration::ZERO);
        assert_eq!(get_cached_live_endpoint(), None);

        let sample = DiscoveredAntigravityEndpoint {
            pid: 99_999,
            ports: vec![51_234],
            token: "fixture-csrf".into(),
        };
        record_verified_live_endpoint(99_999, 51_234, "fixture-csrf".into());
        assert_eq!(get_cached_live_endpoint(), Some(Ok(sample)));

        // Transient RPC failure while candidate processes exist clears positive cache without negative-caching Err(())
        record_endpoint_failure(true);
        assert_eq!(get_cached_live_endpoint(), None);

        // Failure when no candidate process exists records a 5s negative cache (Some(Err(())))
        record_endpoint_failure(false);
        assert_eq!(get_cached_live_endpoint(), Some(Err(())));

        // Idle eviction clears the cache
        evict_idle_endpoint_cache(std::time::Duration::ZERO);
        assert_eq!(get_cached_live_endpoint(), None);
    }

    fn encode_proto_test_len(field_num: u64, payload: &[u8], out: &mut Vec<u8>) {
        let mut tag = (field_num << 3) | 2;
        while tag >= 0x80 {
            out.push(((tag as u8) & 0x7f) | 0x80);
            tag >>= 7;
        }
        out.push(tag as u8);
        let mut len = payload.len() as u64;
        while len >= 0x80 {
            out.push(((len as u8) & 0x7f) | 0x80);
            len >>= 7;
        }
        out.push(len as u8);
        out.extend_from_slice(payload);
    }

    fn build_sample_raw_summary(title: &str, tag_value: &str) -> Vec<u8> {
        let mut ann = Vec::new();
        encode_proto_test_len(1, title.as_bytes(), &mut ann);
        // field 3 = repeated string tags (must never be overwritten by archived!)
        encode_proto_test_len(3, tag_value.as_bytes(), &mut ann);
        // field 7 = last_user_view_time (Timestamp: seconds=1700000000)
        encode_proto_test_len(7, &[0x08, 0x80, 0x94, 0xeb, 0xaa, 0x06], &mut ann);
        let mut top = Vec::new();
        // field 1 = summary
        encode_proto_test_len(1, b"fallback-summary", &mut top);
        // field 2 = step_count = 4
        top.extend_from_slice(&[0x10, 0x04]);
        // field 15 = annotations
        encode_proto_test_len(15, &ann, &mut top);
        top
    }

    #[test]
    fn raw_summary_protobuf_patch_fts5_integer_archive_and_metadata_cache_sync() {
        let raw = build_sample_raw_summary("原始 CLI 标题", "keep-tag");
        assert!(patch_raw_summary_annotations(&[], Some("x"), None, 10).is_none());
        assert!(patch_raw_summary_annotations(&[0x7a, 0xff], Some("x"), None, 10).is_none());
        // Empty field 15 (`[0x7a, 0x00]`) observed on untitled CLI sessions must patch cleanly
        let mut empty_f15 = Vec::new();
        encode_proto_test_len(1, b"untitled-summary", &mut empty_f15);
        empty_f15.extend_from_slice(&[0x7a, 0x00]);
        let patched_empty =
            patch_raw_summary_annotations(&empty_f15, Some("首轮改名"), Some(true), 123).unwrap();
        let decoded_empty = read_raw_summary_annotations(&patched_empty);
        assert_eq!(decoded_empty.title.as_deref(), Some("首轮改名"));
        assert_eq!(decoded_empty.archived, Some(true));

        let root = env::temp_dir().join(format!("agenthub-ag-cli-fts5-{}", uuid::Uuid::new_v4()));
        for dir in ["conversations", "annotations", "cache"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let id = uuid::Uuid::new_v4().to_string();
        fs::write(
            root.join("conversations").join(format!("{id}.db")),
            b"sqlite-placeholder",
        )
        .unwrap();
        for name in annotations::PROTO_CACHE_FILES {
            fs::write(root.join(name), b"cached-proto").unwrap();
        }
        let meta_path = root.join("cache/conversation_metadata.json");
        fs::write(
            &meta_path,
            serde_json::to_vec_pretty(&json!({
                "conversations": {
                    (&id): {
                        "summary": "原始 CLI 标题",
                        "is_internal": false,
                        "last_modified_time": "2026-10-29T00:00:00Z"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                preview TEXT NOT NULL DEFAULT '',
                step_count INTEGER NOT NULL DEFAULT 0,
                last_modified_time TEXT NOT NULL DEFAULT '',
                workspace_uris TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'CASCADE_RUN_STATUS_IDLE',
                parent_conversation_id TEXT NOT NULL DEFAULT '',
                nesting_depth INTEGER NOT NULL DEFAULT 0,
                not_fully_idle NUMERIC NOT NULL DEFAULT 0,
                raw_summary BLOB
            );
            CREATE VIRTUAL TABLE conversations_fts USING fts5(
                conversation_id UNINDEXED,
                chunk_index UNINDEXED,
                archived UNINDEXED,
                title
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries \
             (conversation_id, title, preview, step_count, last_modified_time, workspace_uris, raw_summary) \
             VALUES (?1, '原始 CLI 标题', '预览', 4, '2026-10-29T01:00:00Z', '[\"file:///workspace/cli\"]', ?2)",
            params![id, raw],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversations_fts (conversation_id, chunk_index, archived, title) VALUES (?1, -1, 0, '原始 CLI 标题')",
            params![id],
        )
        .unwrap();
        drop(conn);

        let paths = AntigravityPaths::offline(root.clone());
        let renamed = rename_session(&paths, &id, "全新 CLI 标题").unwrap();
        assert_eq!(renamed.title, "全新 CLI 标题");
        for name in annotations::PROTO_CACHE_FILES {
            assert!(!root.join(name).exists(), "{name} must be invalidated");
        }
        let meta_after: Value = serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
        assert_eq!(
            meta_after["conversations"][&id]["summary"].as_str(),
            Some("全新 CLI 标题")
        );
        assert_eq!(meta_after["conversations"][&id]["is_internal"], false);

        let archived = set_archived(&paths, &id, true).unwrap();
        assert!(archived.archived);

        // Verify FTS5 integer `WHERE chunk_index = -1 AND archived = 1` matches (exact `agy` CLI query)
        let conn = Connection::open(paths.summaries_db()).unwrap();
        let fts_hit: i64 = conn
            .query_row(
                "SELECT count(*) FROM conversations_fts WHERE chunk_index = -1 AND archived = 1 AND conversation_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_hit, 1);

        // Verify raw_summary field 15 carries updated title + archived=true while preserving tags (field 3) and view time (field 7)
        let patched_blob: Vec<u8> = conn
            .query_row(
                "SELECT raw_summary FROM conversation_summaries WHERE conversation_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        let decoded_ann = read_raw_summary_annotations(&patched_blob);
        assert_eq!(decoded_ann.title.as_deref(), Some("全新 CLI 标题"));
        assert_eq!(decoded_ann.archived, Some(true));
        assert!(
            patched_blob
                .windows(b"keep-tag".len())
                .any(|w| w == b"keep-tag"),
            "field 3 tags must remain intact in raw_summary"
        );
        assert!(
            patched_blob
                .windows(b"fallback-summary".len())
                .any(|w| w == b"fallback-summary"),
            "top-level summary field must remain intact"
        );
        drop(conn);

        // Even if .pbtxt is removed, raw_summary alone preserves title and archived state!
        fs::remove_file(paths.annotations_dir().join(format!("{id}.pbtxt"))).unwrap();
        let from_raw = session_summary(&paths, &id).unwrap();
        assert_eq!(from_raw.title, "全新 CLI 标题");
        assert!(from_raw.archived);

        let restored = set_archived(&paths, &id, false).unwrap();
        assert!(!restored.archived);
        let conn = Connection::open(paths.summaries_db()).unwrap();
        let fts_after_restore: i64 = conn
            .query_row(
                "SELECT count(*) FROM conversations_fts WHERE chunk_index = -1 AND archived = 1 AND conversation_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_after_restore, 0);
        drop(conn);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dual_store_excludes_ghost_rows_routes_cli_offline_and_cleans_cli_shared_files() {
        let base = env::temp_dir().join(format!("agenthub-ag-dual-{}", uuid::Uuid::new_v4()));
        let app_home = base.join(".gemini/antigravity");
        let cli_home = base.join(".gemini/antigravity-cli");
        for dir in ["conversations", "annotations", "brain", "presence"] {
            fs::create_dir_all(app_home.join(dir)).unwrap();
            fs::create_dir_all(cli_home.join(dir)).unwrap();
        }
        fs::create_dir_all(cli_home.join("cache")).unwrap();
        fs::create_dir_all(cli_home.join("implicit")).unwrap();

        let app_id = uuid::Uuid::new_v4().to_string();
        let cli_id = uuid::Uuid::new_v4().to_string();
        let cli_sub = uuid::Uuid::new_v4().to_string();
        let cli_keep = uuid::Uuid::new_v4().to_string();
        let orphan_id = uuid::Uuid::new_v4().to_string();
        let implicit_id = uuid::Uuid::new_v4().to_string();

        // App primary storage
        fs::write(
            app_home.join("conversations").join(format!("{app_id}.db")),
            b"app-db",
        )
        .unwrap();
        // CLI primary storage + transcript
        for id in [&cli_id, &cli_sub, &cli_keep] {
            fs::write(
                cli_home.join("conversations").join(format!("{id}.db")),
                b"cli-db-bytes",
            )
            .unwrap();
        }
        let cli_logs = cli_home
            .join("brain")
            .join(&cli_id)
            .join(".system_generated/logs");
        fs::create_dir_all(&cli_logs).unwrap();
        fs::write(
            cli_logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-30T01:00:00Z","content":"管理 agy cli 会话"}"#,
        )
        .unwrap();
        // Orphaned .pbtxt / .lock and implicit/*.pb must NEVER count as primary session storage
        fs::write(
            cli_home
                .join("annotations")
                .join(format!("{orphan_id}.pbtxt")),
            "title: \"orphan\"\n",
        )
        .unwrap();
        fs::write(
            cli_home.join("presence").join(format!("{orphan_id}.lock")),
            b"",
        )
        .unwrap();
        fs::write(
            cli_home.join("implicit").join(format!("{implicit_id}.pb")),
            b"workspace-implicit",
        )
        .unwrap();

        // Cross-index ghost rows: app DB contains cli_id ghost; cli DB contains app_id, orphan_id, implicit_id ghosts, plus conversation_parents
        let app_conn = Connection::open(app_home.join("conversation_summaries.db")).unwrap();
        app_conn
            .execute_batch(
                "CREATE TABLE conversation_summaries (
                    conversation_id TEXT PRIMARY KEY,
                    title TEXT, preview TEXT, step_count INTEGER,
                    last_modified_time TEXT, workspace_uris TEXT,
                    parent_conversation_id TEXT, nesting_depth INTEGER
                );
                CREATE TABLE conversations_fts (
                    conversation_id TEXT, chunk_index INTEGER, archived INTEGER, title TEXT
                );",
            )
            .unwrap();
        app_conn
            .execute(
                "INSERT INTO conversation_summaries VALUES (?1,'Ghost CLI in App','',2,'2026-10-30T01:00:00Z','', '', 0)",
                params![cli_id],
            )
            .unwrap();
        drop(app_conn);

        let cli_conn = Connection::open(cli_home.join("conversation_summaries.db")).unwrap();
        cli_conn
            .execute_batch(
                "CREATE TABLE conversation_summaries (
                    conversation_id TEXT PRIMARY KEY,
                    title TEXT, preview TEXT, step_count INTEGER,
                    last_modified_time TEXT, workspace_uris TEXT,
                    status TEXT, parent_conversation_id TEXT,
                    nesting_depth INTEGER, not_fully_idle NUMERIC
                );
                CREATE VIRTUAL TABLE conversations_fts USING fts5(
                    conversation_id UNINDEXED, chunk_index UNINDEXED, archived UNINDEXED, title
                );
                CREATE TABLE conversation_parents (
                    conversation_id TEXT PRIMARY KEY, parent_id TEXT NOT NULL
                ) WITHOUT ROWID;",
            )
            .unwrap();
        for (cid, title, ts) in [
            (&cli_id, "CLI 主会话", "2026-10-30T02:00:00Z"),
            (&cli_sub, "CLI 子代理", "2026-10-30T01:59:00Z"),
            (&cli_keep, "CLI 保留会话", "2026-10-30T00:30:00Z"),
            (&app_id, "Ghost App in CLI", "2026-10-30T03:00:00Z"),
            (&orphan_id, "Orphan Row", "2026-10-30T04:00:00Z"),
            (&implicit_id, "Implicit Row", "2026-10-30T05:00:00Z"),
        ] {
            cli_conn
                .execute(
                    "INSERT INTO conversation_summaries VALUES (?1, ?2, 'p', 1, ?3, '[\"file:///workspace/cli\"]', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0)",
                    params![cid, title, ts],
                )
                .unwrap();
            cli_conn
                .execute(
                    "INSERT INTO conversations_fts VALUES (?1, -1, 0, ?2)",
                    params![cid, title],
                )
                .unwrap();
        }
        // cli_sub has empty parent_conversation_id in summaries, linked ONLY via conversation_parents!
        cli_conn
            .execute(
                "INSERT INTO conversation_parents VALUES (?1, ?2)",
                params![cli_sub, cli_id],
            )
            .unwrap();
        drop(cli_conn);

        // Seed CLI shared files: history.jsonl (including entry without conversationId), conversation_metadata.json, last_conversations.json
        let history_path = cli_home.join("history.jsonl");
        fs::write(
            &history_path,
            format!(
                "{}\n{}\n{}\n",
                json!({"display": "/help", "timestamp": 1, "workspace": "/workspace/cli"}),
                json!({"conversationId": cli_id, "display": "delete me", "timestamp": 2, "workspace": "/workspace/cli"}),
                json!({"conversationId": cli_keep, "display": "keep me", "timestamp": 3, "workspace": "/workspace/cli"}),
            ),
        )
        .unwrap();
        let meta_path = cli_home.join("cache/conversation_metadata.json");
        fs::write(
            &meta_path,
            serde_json::to_vec(&json!({
                "conversations": {
                    (&cli_id): { "summary": "CLI 主会话" },
                    (&cli_sub): { "summary": "CLI 子代理" },
                    (&cli_keep): { "summary": "CLI 保留会话" }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let last_path = cli_home.join("cache/last_conversations.json");
        fs::write(
            &last_path,
            serde_json::to_vec(&json!({
                "/workspace/cli": cli_id,
                "/workspace/keep": cli_keep
            }))
            .unwrap(),
        )
        .unwrap();

        // Live desktop RPC serves ONLY app_id and must NEVER receive RPC mutations/reads for cli_id!
        let rpc_bad_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server_bad_calls = rpc_bad_calls.clone();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let s_app = app_id.clone();
        let handle = std::thread::spawn(move || {
            while !server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                if server_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let resp = if req_line.contains("GetAllCascadeTrajectories") {
                    json!({
                        "trajectorySummaries": {
                            (&s_app): {
                                "summary": "桌面在线会话",
                                "stepCount": 3,
                                "lastModifiedTime": "2026-10-30T03:00:00Z",
                                "status": "CASCADE_RUN_STATUS_IDLE"
                            }
                        }
                    })
                } else {
                    server_bad_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    json!({})
                };
                let bytes = serde_json::to_vec(&resp).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .unwrap();
                stream.write_all(&bytes).unwrap();
            }
        });

        let paths = AntigravityPaths::with_fixed_rpc_and_cli(
            app_home.clone(),
            cli_home.clone(),
            port,
            "tok",
        );
        let list = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        let listed_ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            listed_ids,
            vec![app_id.as_str(), cli_id.as_str(), cli_keep.as_str()]
        );
        // Storage size for CLI sessions resolves from cli_home (never Unavailable from app_home)!
        let cli_summary = list.iter().find(|s| s.id == cli_id).unwrap();
        assert!(cli_summary.path.starts_with(&cli_home));
        assert_ne!(
            cli_summary.storage_size.as_ref().map(|s| s.status),
            Some(session_storage::StorageStatus::Unavailable)
        );
        let cli_keep_summary = list.iter().find(|s| s.id == cli_keep).unwrap();
        assert_eq!(
            cli_keep_summary.storage_size.as_ref().and_then(|s| s.bytes),
            Some(b"cli-db-bytes".len() as u64)
        );

        // Reading, renaming, archiving, and deleting cli_id never invokes desktop RPC methods!
        let detail = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: cli_id.clone(),
                agent_id: None,
                limit: Some(20),
                before: None,
            },
        )
        .unwrap();
        assert_eq!(detail.events.len(), 1);
        assert_eq!(detail.events[0].text.as_deref(), Some("管理 agy cli 会话"));
        let renamed = rename_session(&paths, &cli_id, "CLI 已改名").unwrap();
        assert_eq!(renamed.title, "CLI 已改名");
        let archived = set_archived(&paths, &cli_id, true).unwrap();
        assert!(archived.archived);
        assert_eq!(rpc_bad_calls.load(std::sync::atomic::Ordering::Relaxed), 0);

        let preview = preview_batch_item(&paths, &cli_id, SessionOperation::Delete, true).unwrap();
        assert_eq!(preview.descendants, vec![cli_sub.clone()]);
        let del = execute_batch_item(
            &paths,
            &cli_id,
            SessionOperation::Delete,
            &preview.descendants,
            true,
        )
        .unwrap();
        assert_eq!(del.status, "succeeded");
        assert!(del.residuals.is_empty());
        assert_eq!(rpc_bad_calls.load(std::sync::atomic::Ordering::Relaxed), 0);

        // Verify implicit/*.pb untouched, conversation_parents & cross-store ghost row removed, and shared JSON/JSONL cleaned!
        assert!(cli_home
            .join("implicit")
            .join(format!("{implicit_id}.pb"))
            .exists());
        let hist_after = fs::read_to_string(&history_path).unwrap();
        assert!(hist_after.contains("\"/help\""));
        assert!(!hist_after.contains(&cli_id));
        assert!(hist_after.contains(&cli_keep));
        let meta_after: Value = serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
        assert!(meta_after["conversations"].get(&cli_id).is_none());
        assert!(meta_after["conversations"].get(&cli_sub).is_none());
        assert!(meta_after["conversations"].get(&cli_keep).is_some());
        let last_after: Value = serde_json::from_slice(&fs::read(&last_path).unwrap()).unwrap();
        assert!(last_after.get("/workspace/cli").is_none());
        assert_eq!(
            last_after["/workspace/keep"].as_str(),
            Some(cli_keep.as_str())
        );

        let app_conn = Connection::open(app_home.join("conversation_summaries.db")).unwrap();
        let ghost_left: i64 = app_conn
            .query_row(
                "SELECT count(*) FROM conversation_summaries WHERE conversation_id = ?1",
                params![cli_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            ghost_left, 0,
            "cross-store ghost row in app DB must be cleaned"
        );
        drop(app_conn);

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", port));
        let _ = handle.join();
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cli_active_subagent_with_lock_reports_running_and_blocks_all_offline_mutations() {
        use std::os::unix::io::AsRawFd;
        let root = env::temp_dir().join(format!("agenthub-ag-cli-lock-{}", uuid::Uuid::new_v4()));
        for dir in ["conversations", "annotations", "presence", "daemon"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let root_id = uuid::Uuid::new_v4().to_string();
        let sub_id = uuid::Uuid::new_v4().to_string();
        let crashed_id = uuid::Uuid::new_v4().to_string();
        let idle_open_id = uuid::Uuid::new_v4().to_string();
        let deleted_brain_only_id = uuid::Uuid::new_v4().to_string();
        for id in [&root_id, &sub_id, &crashed_id, &idle_open_id] {
            fs::write(
                root.join("conversations").join(format!("{id}.db")),
                b"cli-sqlite",
            )
            .unwrap();
        }
        fs::create_dir_all(root.join("brain").join(&deleted_brain_only_id)).unwrap();
        let conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT, preview TEXT, step_count INTEGER,
                last_modified_time TEXT, workspace_uris TEXT,
                status TEXT, parent_conversation_id TEXT,
                nesting_depth INTEGER, not_fully_idle NUMERIC
            );",
        )
        .unwrap();
        // root_id is IDLE in SQLite, sub_id is RUNNING in SQLite, crashed_id is RUNNING without flock, idle_open_id is IDLE with flock
        conn.execute(
            "INSERT INTO conversation_summaries VALUES \
             (?1, 'CLI 运行中父会话', '', 2, '2026-10-30T02:00:00Z', '', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0), \
             (?2, 'CLI 运行中子会话', '', 1, '2026-10-30T02:00:01Z', '', 'CASCADE_RUN_STATUS_RUNNING', ?1, 1, 1), \
             (?3, 'CLI 已崩溃会话', '', 1, '2026-10-30T01:00:00Z', '', 'CASCADE_RUN_STATUS_RUNNING', '', 0, 1), \
             (?4, 'CLI 终端打开中空闲会话', '', 1, '2026-10-30T01:30:00Z', '', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0)",
            params![root_id, sub_id, crashed_id, idle_open_id],
        )
        .unwrap();
        drop(conn);

        let sub_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("presence").join(format!("{sub_id}.lock")))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(sub_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let idle_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("presence").join(format!("{idle_open_id}.lock")))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(idle_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );

        let paths = AntigravityPaths::offline(root.clone());
        let list = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        let active_item = list.iter().find(|s| s.id == root_id).unwrap();
        assert_eq!(active_item.status, "running");
        assert!(!active_item.can_rename && active_item.rename_block_reason.is_some());
        assert!(!active_item.can_archive && active_item.archive_block_reason.is_some());
        assert!(!active_item.can_delete && active_item.delete_block_reason.is_some());

        let idle_open_item = list.iter().find(|s| s.id == idle_open_id).unwrap();
        assert_eq!(
            idle_open_item.status, "idle",
            "idle open CLI session must stay idle rather than running"
        );
        assert!(!idle_open_item.can_rename && idle_open_item.rename_block_reason.is_some());
        assert!(!idle_open_item.can_archive && idle_open_item.archive_block_reason.is_some());
        assert!(!idle_open_item.can_delete);
        assert!(idle_open_item
            .delete_block_reason
            .as_deref()
            .unwrap_or("")
            .contains("已在 Antigravity 中打开"));
        let rename_err = rename_session(&paths, &idle_open_id, "不可改名")
            .unwrap_err()
            .to_string();
        assert!(
            rename_err.contains("会话当前已在 Antigravity 中打开"),
            "{rename_err}"
        );
        let archive_err = set_archived(&paths, &idle_open_id, true)
            .unwrap_err()
            .to_string();
        assert!(
            archive_err.contains("会话当前已在 Antigravity 中打开"),
            "{archive_err}"
        );
        let delete_err = preview_batch_item(&paths, &idle_open_id, SessionOperation::Delete, true)
            .unwrap_err()
            .to_string();
        assert!(
            delete_err.contains("会话当前已在 Antigravity 中打开"),
            "{delete_err}"
        );

        let crashed_item = list.iter().find(|s| s.id == crashed_id).unwrap();
        assert_eq!(
            crashed_item.status, "idle",
            "SQLite RUNNING without live flock must not stick in running state"
        );
        assert!(crashed_item.can_rename && crashed_item.can_archive && crashed_item.can_delete);

        assert!(
            session_summary(&paths, &deleted_brain_only_id).is_err(),
            "orphaned brain/<id> must not be resurrected by filter_id when DB has valid summaries"
        );

        assert!(rename_session(&paths, &root_id, "不可改名").is_err());
        assert!(set_archived(&paths, &root_id, true).is_err());
        assert!(preview_batch_item(&paths, &root_id, SessionOperation::Archive, false).is_err());
        assert!(preview_batch_item(&paths, &root_id, SessionOperation::Delete, true).is_err());
        assert!(rename_session(&paths, &idle_open_id, "不可改名").is_err());
        assert!(set_archived(&paths, &idle_open_id, true).is_err());
        assert!(preview_batch_item(&paths, &idle_open_id, SessionOperation::Delete, true).is_err());

        // Also test daemon/hub_*.json endpoint discovery with stale PID files coexisting!
        let uid = unsafe { libc::getuid() };
        let daemon_dir = root.join("daemon");
        fs::write(
            daemon_dir.join("hub_50001.json"),
            serde_json::to_vec(&json!({
                "pid": 11_111,
                "httpPort": 50001,
                "httpsPort": 50000,
                "csrfToken": "stale_token_111"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            daemon_dir.join("hub_50002.json"),
            serde_json::to_vec(&json!({
                "pid": 22_222,
                "httpPort": 50002,
                "httpsPort": 50003,
                "csrfToken": "live_token_222"
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            resolve_daemon_hub_endpoint(
                std::slice::from_ref(&daemon_dir),
                22_222,
                uid,
                &[50002, 50003],
            ),
            Some((vec![50002], "live_token_222".into()))
        );
        assert_eq!(
            resolve_daemon_hub_endpoint(std::slice::from_ref(&daemon_dir), 22_222, uid, &[59999]),
            None,
            "hub_*.json port not in live process loopback listeners must be rejected"
        );

        unsafe {
            let _ = libc::flock(sub_lock.as_raw_fd(), libc::LOCK_UN);
            let _ = libc::flock(idle_lock.as_raw_fd(), libc::LOCK_UN);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cli_root_lock_inheritance_trailing_prose_json_15col_fts_object_rename_and_offline_probe_events(
    ) {
        use std::os::unix::io::AsRawFd;
        let root = env::temp_dir().join(format!("agenthub-ag-cli-full-{}", uuid::Uuid::new_v4()));
        for dir in ["conversations", "annotations", "presence", "cache"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let root_id = uuid::Uuid::new_v4().to_string();
        let child_id = uuid::Uuid::new_v4().to_string();
        let internal_id = uuid::Uuid::new_v4().to_string();
        let err_id = uuid::Uuid::new_v4().to_string();
        let ask_id = uuid::Uuid::new_v4().to_string();

        // 1. Streaming JSON extraction when invoke_subagent result has trailing prose
        let mut parsed_ids = BTreeSet::new();
        crate::antigravity::collect_spawned_subagent_ids(
            &json!(format!(
                "Created subagent:\n{{\"conversationId\":\"{child_id}\"}}\nSubagent returned 3 findings."
            )),
            &mut parsed_ids,
        );
        assert!(parsed_ids.contains(&child_id));

        for cid in [&root_id, &child_id, &internal_id, &err_id, &ask_id] {
            fs::write(
                root.join("conversations").join(format!("{cid}.db")),
                b"placeholder",
            )
            .unwrap();
            fs::create_dir_all(root.join("brain").join(cid).join(".system_generated/logs"))
                .unwrap();
        }
        // Sidecar subagent registration: brain/<root>/.system_generated/subagents/<child>.json
        let sub_sidecar = root
            .join("brain")
            .join(&root_id)
            .join(".system_generated/subagents");
        fs::create_dir_all(&sub_sidecar).unwrap();
        fs::write(
            sub_sidecar.join(format!("{child_id}.json")),
            serde_json::to_vec(
                &json!({"title": "Worker Child", "state": "SUBAGENT_STATE_RUNNING"}),
            )
            .unwrap(),
        )
        .unwrap();

        let now_iso = crate::timestamps::format_utc(jiff::Timestamp::now(), true);
        // Root transcript settled at PLANNER_RESPONSE, while child transcript has active GENERATING step (with lock ONLY on root!)
        fs::write(
            root.join("brain")
                .join(&root_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n",
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":now_iso,"content":"启动多代理检查"}),
                json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"已派发子代理"}),
            ),
        )
        .unwrap();
        fs::write(
            root.join("brain")
                .join(&child_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n",
                json!({"step_index":0,"source":"MODEL","type":"PLANNER_RESPONSE","status":"GENERATING","created_at":now_iso,"content":"分析中"}),
            ),
        )
        .unwrap();
        // Error session ending in ERROR_MESSAGE (status: "DONE") + SYSTEM_SDK hook context after it
        fs::write(
            root.join("brain")
                .join(&err_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":now_iso,"content":"运行任务"}),
                json!({"step_index":1,"source":"MODEL","type":"ERROR_MESSAGE","status":"DONE","created_at":now_iso,"error":"Quota exceeded"}),
                json!({"step_index":2,"source":"SYSTEM_SDK","type":"SYSTEM_MESSAGE","status":"DONE","created_at":now_iso,"content":"hook"}),
            ),
        )
        .unwrap();
        // Mid-turn ask_question session
        fs::write(
            root.join("brain")
                .join(&ask_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n",
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":now_iso,"content":"部署配置"}),
                json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"tool_calls":[{"id":"q1","name":"ask_question","args":{"questions":[{"question":"是否覆盖线上环境？"}]}}]}),
            ),
        )
        .unwrap();

        // conversation_metadata.json with Go SummaryStruct object & internal session
        let meta_path = root.join("cache/conversation_metadata.json");
        fs::write(
            &meta_path,
            serde_json::to_vec(&json!({
                "conversations": {
                    (&root_id): {
                        "summary": {
                            "Title": "CLI 根任务",
                            "WorkspaceURIs": ["file:///workspace/orca-demo"],
                            "Internal": false
                        }
                    },
                    (&internal_id): {
                        "is_internal": true,
                        "summary": { "Title": "Internal Memory", "Internal": true }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversation_summaries (
                conversation_id TEXT PRIMARY KEY,
                title TEXT, preview TEXT, step_count INTEGER,
                last_modified_time TEXT, workspace_uris TEXT,
                status TEXT, parent_conversation_id TEXT,
                nesting_depth INTEGER, not_fully_idle NUMERIC
            );
            CREATE TABLE conversations_fts (
                conversation_id TEXT, chunk_index INTEGER,
                chunk_first_step INTEGER, chunk_last_step INTEGER,
                last_modified_time INTEGER, step_count INTEGER,
                last_step_created_at INTEGER, archived INTEGER,
                user_step_byte_offsets TEXT, agent_step_byte_offsets TEXT,
                title TEXT, workspace TEXT, project TEXT,
                user_body TEXT, agent_body TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries VALUES \
             (?1, '', '', 0, '0001-01-01 00:00:00+00:00', '', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0), \
             (?2, 'Internal', '', 1, ?5, '', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0), \
             (?3, 'Error Run', '', 2, ?5, '[\"file:///workspace/orca-demo\"]', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0), \
             (?4, 'Ask Run', '', 2, ?5, '[\"file:///workspace/orca-demo\"]', 'CASCADE_RUN_STATUS_IDLE', '', 0, 0)",
            params![root_id, internal_id, err_id, ask_id, now_iso],
        )
        .unwrap();
        drop(conn);

        // Hold lock ONLY on root_id and ask_id
        let hold = |cid: &str| {
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(root.join("presence").join(format!("{cid}.lock")))
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            f
        };
        let root_lock = hold(&root_id);
        let ask_lock = hold(&ask_id);

        let paths = AntigravityPaths::offline(root.clone());
        let list = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        // child_id and internal_id must both be excluded from main list!
        assert!(!list.iter().any(|s| s.id == child_id));
        assert!(!list.iter().any(|s| s.id == internal_id));

        let root_sum = list.iter().find(|s| s.id == root_id).unwrap();
        assert_eq!(
            root_sum.status, "running",
            "child must inherit root_id presence lock"
        );
        assert_eq!(root_sum.title, "CLI 根任务");
        assert_eq!(root_sum.cwd, "/workspace/orca-demo");
        assert!(
            !root_sum
                .updated_at
                .as_deref()
                .unwrap_or("")
                .starts_with("0001-"),
            "0001-01-01 zero time must be rejected"
        );

        let err_sum = list.iter().find(|s| s.id == err_id).unwrap();
        assert_eq!(err_sum.status, "failed");

        let ask_sum = list.iter().find(|s| s.id == ask_id).unwrap();
        assert_eq!(ask_sum.status, "waiting");

        // Offline NativeStreamSnapshot: running root holds settled_count < record_count with 0 events;
        // waiting ask_id emits reply_needed; failed err_id emits failure; child subagent is suppressed!
        let scan_all = scan_offline_store(&paths, None).unwrap();
        let snap_root = scan_all.streams.iter().find(|s| s.id == root_id).unwrap();
        assert!(snap_root.settled_count < snap_root.record_count);
        assert!(snap_root.events.is_empty());
        assert!(scan_offline_store(&paths, Some(&child_id))
            .unwrap()
            .streams
            .is_empty());

        let snap_ask = scan_all.streams.iter().find(|s| s.id == ask_id).unwrap();
        assert_eq!(snap_ask.events.len(), 1);
        assert_eq!(snap_ask.events[0].kind, "reply_needed");
        assert!(snap_ask.events[0].body.contains("是否覆盖线上环境"));

        let snap_err = scan_all.streams.iter().find(|s| s.id == err_id).unwrap();
        assert_eq!(snap_err.events.len(), 1);
        assert_eq!(snap_err.events[0].kind, "failure");
        assert!(snap_err.events[0].body.contains("Quota exceeded"));

        // Finish child + unlock root -> root emits completion, rename preserves Go SummaryStruct Object, archive upserts 15-col FTS!
        fs::write(
            root.join("brain")
                .join(&child_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n",
                json!({"step_index":0,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"分析完成"}),
            ),
        )
        .unwrap();
        unsafe {
            let _ = libc::flock(root_lock.as_raw_fd(), libc::LOCK_UN);
            let _ = libc::flock(ask_lock.as_raw_fd(), libc::LOCK_UN);
        }
        drop(root_lock);
        drop(ask_lock);

        let snap_done = scan_offline_store(&paths, Some(&root_id)).unwrap();
        assert_eq!(snap_done.streams[0].identity, snap_root.identity);
        assert_eq!(snap_done.streams[0].events.len(), 1);
        assert_eq!(snap_done.streams[0].events[0].kind, "completion");

        // Crashed/killed mid-tool session (PLANNER_RESPONSE with unsettled tool_calls & no lock)
        // must NEVER emit a false "completion" Bark event, and L2 subagent with both root_conversation_id
        // and direct parent L1 must keep unique parent L1!
        let mid_crash_id = uuid::Uuid::new_v4().to_string();
        let l2_id = uuid::Uuid::new_v4().to_string();
        for cid in [&mid_crash_id, &l2_id] {
            fs::write(
                root.join("conversations").join(format!("{cid}.db")),
                b"placeholder",
            )
            .unwrap();
            fs::create_dir_all(root.join("brain").join(cid).join(".system_generated/logs"))
                .unwrap();
        }
        fs::write(
            root.join("brain")
                .join(&mid_crash_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n",
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":now_iso,"content":"跑全量检查"}),
                json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"先执行测试命令","tool_calls":[{"id":"c1","name":"run_command","args":{"CommandLine":"cargo test"}}]}),
            ),
        )
        .unwrap();
        let snap_mid_crash = scan_offline_store(&paths, Some(&mid_crash_id)).unwrap();
        assert_eq!(snap_mid_crash.streams.len(), 1);
        assert!(
            snap_mid_crash.streams[0].events.is_empty(),
            "mid-tool crashed session must not emit false completion"
        );

        let child_sidecar = root
            .join("brain")
            .join(&child_id)
            .join(".system_generated/subagents");
        fs::create_dir_all(&child_sidecar).unwrap();
        fs::write(
            child_sidecar.join(format!("{l2_id}.json")),
            serde_json::to_vec(&json!({"title": "Nested L2", "state": "SUBAGENT_STATE_DONE"}))
                .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("brain")
                .join(&l2_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n",
                json!({"step_index":0,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"L2 子代理完成"}),
            ),
        )
        .unwrap();
        fs::write(
            &meta_path,
            serde_json::to_vec(&json!({
                "conversations": {
                    (&root_id): {
                        "summary": {
                            "Title": "CLI 根任务",
                            "WorkspaceURIs": ["file:///workspace/orca-demo"],
                            "Internal": false
                        }
                    },
                    (&l2_id): {
                        "root_conversation_id": (&root_id),
                        "summary": { "Title": "Nested L2", "RootConversationID": (&root_id) }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let l2_detail = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: root_id.clone(),
                agent_id: Some(l2_id.clone()),
                limit: Some(20),
                before: None,
            },
        )
        .expect("L2 subagent with root_conversation_id + direct L1 parent must resolve uniquely");
        assert_eq!(l2_detail.events.len(), 1);

        let renamed = rename_session(&paths, &root_id, "CLI 新标题").unwrap();
        assert_eq!(renamed.title, "CLI 新标题");
        let meta_after: Value = serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
        assert!(meta_after["conversations"][&root_id]["summary"].is_object());
        assert_eq!(
            meta_after["conversations"][&root_id]["summary"]["Title"].as_str(),
            Some("CLI 新标题")
        );
        assert_eq!(
            meta_after["conversations"][&root_id]["summary"]["WorkspaceURIs"][0].as_str(),
            Some("file:///workspace/orca-demo")
        );

        let archived = set_archived(&paths, &root_id, true).unwrap();
        assert!(archived.archived);
        let check_conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        let (fts_arch, fts_sc, fts_lm): (i64, i64, i64) = check_conn
            .query_row(
                "SELECT archived, step_count, last_modified_time FROM conversations_fts WHERE conversation_id = ?1 AND chunk_index = -1",
                params![root_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(fts_arch, 1);
        assert!(fts_sc >= 1);
        assert!(fts_lm > 1_000_000_000_000_000_000);
        drop(check_conn);

        fs::remove_dir_all(root).unwrap();
    }

    fn pb_len_field(field_no: u64, payload: &[u8]) -> Vec<u8> {
        fn push_varint(mut v: u64, out: &mut Vec<u8>) {
            loop {
                let mut byte = (v & 0x7f) as u8;
                v >>= 7;
                if v != 0 {
                    byte |= 0x80;
                }
                out.push(byte);
                if v == 0 {
                    break;
                }
            }
        }
        let mut out = Vec::with_capacity(payload.len() + 10);
        push_varint((field_no << 3) | 2, &mut out);
        push_varint(payload.len() as u64, &mut out);
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn prompt_purification_across_jsonl_sqlite_db_and_bark_summary() {
        let root = env::temp_dir().join(format!("agenthub-ag-purify-{}", uuid::Uuid::new_v4()));
        for d in ["conversations", "brain", "presence"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let now_iso = "2026-11-24T12:00:00Z";

        let jsonl_id = uuid::Uuid::new_v4().to_string();
        fs::write(
            root.join("conversations").join(format!("{jsonl_id}.db")),
            b"placeholder",
        )
        .unwrap();
        let img_path = root.join("brain").join(&jsonl_id).join("screen.png");
        let logs_dir = root
            .join("brain")
            .join(&jsonl_id)
            .join(".system_generated/logs");
        fs::create_dir_all(&logs_dir).unwrap();
        fs::write(&img_path, b"\x89PNG\r\n\x1a\nfixture").unwrap();
        let wrapped_prompt = concat!(
            "<USER_REQUEST>\n",
            "/boost /plan 根治提示词泄漏\n",
            "</USER_REQUEST>\n",
            "<ADDITIONAL_METADATA>\n",
            "<ORCHESTRATOR>SECRET_ORCHESTRATOR_PROMPT</ORCHESTRATOR>\n",
            "<PLAN>SECRET_PLAN_PROMPT</PLAN>\n",
            "</ADDITIONAL_METADATA>"
        );
        fs::write(
            logs_dir.join("transcript_full.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                json!({
                    "step_index": "0",
                    "source": "USER",
                    "type": "REQUEST",
                    "status": "DONE",
                    "created_at": now_iso,
                    "content": wrapped_prompt,
                    "media": [{"uri": format!("file://{}", img_path.display()), "mime_type": "image/png"}]
                }),
                json!({
                    "step_index": "1",
                    "source": "MODEL",
                    "type": "PLANNER_RESPONSE",
                    "status": "DONE",
                    "created_at": now_iso,
                    "tool_calls": [{"id": "call-live", "name": "run_command", "arguments": {"CommandLine": "cargo test"}}]
                }),
                json!({
                    "step_index": "2",
                    "source": "USER_EXPLICIT",
                    "type": "USER_INPUT",
                    "status": "DONE",
                    "created_at": now_iso,
                    "content": "<USER_SETTINGS_CHANGE>model=gemini-3.1-pro</USER_SETTINGS_CHANGE>"
                }),
            ),
        )
        .unwrap();

        let hist_id = uuid::Uuid::new_v4().to_string();
        fs::write(
            root.join("conversations").join(format!("{hist_id}.db")),
            b"placeholder",
        )
        .unwrap();
        fs::create_dir_all(
            root.join("brain")
                .join(&hist_id)
                .join(".system_generated/logs"),
        )
        .unwrap();
        fs::write(
            root.join("brain")
                .join(&hist_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n",
                json!({"step_index":0,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":now_iso,"content":"Ready"}),
                json!({"step_index":1,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":now_iso,"content":"<USER_SETTINGS_CHANGE>model=claude-opus-4.6</USER_SETTINGS_CHANGE>"}),
            ),
        )
        .unwrap();
        fs::write(
            root.join("history.jsonl"),
            format!(
                "{}\n{}\n{}\n",
                json!({"conversationId": hist_id, "display": "/boost /plan 深度瘦身老项目", "cwd": "/workspace/demo"}),
                json!({"conversationId": hist_id, "display": "/compact"}),
                json!({"conversationId": hist_id, "display": "/clear"}),
            ),
        )
        .unwrap();

        let offline = AntigravityPaths::offline(root.clone());
        let listed = list_sessions(&offline, &ProviderListRequest::default()).unwrap();
        assert_eq!(
            listed.iter().find(|s| s.id == jsonl_id).unwrap().title,
            "/boost /plan 根治提示词泄漏"
        );
        assert_eq!(
            listed.iter().find(|s| s.id == hist_id).unwrap().title,
            "/boost /plan 深度瘦身老项目"
        );
        let hist_scan = scan_offline_store(&offline, Some(&hist_id)).unwrap();
        assert_eq!(hist_scan.streams.len(), 1);
        assert_eq!(hist_scan.streams[0].events.len(), 1);
        assert_eq!(hist_scan.streams[0].events[0].kind, "completion");
        assert_eq!(hist_scan.streams[0].events[0].body, "Ready");

        let jsonl_detail = session_detail(
            &offline,
            &ProviderDetailRequest {
                session_key: jsonl_id.clone(),
                limit: Some(20),
                ..Default::default()
            },
        )
        .unwrap();
        let user_evs: Vec<_> = jsonl_detail
            .events
            .iter()
            .filter(|e| e.kind == "user_message")
            .collect();
        assert_eq!(user_evs.len(), 1);
        let um = user_evs[0].user_message.as_ref().unwrap();
        assert_eq!(um.text, "/boost /plan 根治提示词泄漏");
        assert!(um.context_blocks.is_empty());
        assert_eq!(um.attachments.len(), 1);
        assert_eq!(jsonl_detail.timeline.as_ref().map(|t| t.len()), Some(1));
        assert_eq!(
            jsonl_detail
                .events
                .iter()
                .find(|e| e.kind == "tool_call")
                .unwrap()
                .status
                .as_deref(),
            None
        );
        let searchable = crate::user_message::searchable_text(&um.text);
        assert!(!searchable.contains("SECRET_ORCHESTRATOR_PROMPT"));
        assert!(!searchable.contains("ADDITIONAL_METADATA"));

        let db_done_id = uuid::Uuid::new_v4().to_string();
        let db_err_id = uuid::Uuid::new_v4().to_string();
        let mut f19 = Vec::new();
        f19.extend(pb_len_field(1, &pb_len_field(1, b"@boost rewritten")));
        f19.extend(pb_len_field(
            2,
            "<USER_REQUEST>\n只展示我的真实输入\n</USER_REQUEST>\n<ADDITIONAL_METADATA>meta</ADDITIONAL_METADATA>".as_bytes(),
        ));
        f19.extend(pb_len_field(3, b"EXPANDED_WORKFLOW_SYSTEM_PROMPT"));
        for sec in ["identity", "artifacts", "slash_commands", "user_rules"] {
            f19.extend(pb_len_field(12, sec.as_bytes()));
        }
        let step14_blob = pb_len_field(19, &f19);

        let mut f20 = Vec::new();
        f20.extend(pb_len_field(1, "旧回退回复".as_bytes()));
        f20.extend(pb_len_field(3, b"INTERNAL_THINKING_TRACE"));
        f20.extend(pb_len_field(6, br#"{"tool":"secret_internal_tool"}"#));
        f20.extend(pb_len_field(8, "最终纯净回复正文".as_bytes()));
        let step15_blob = pb_len_field(20, &f20);

        let mut err_inner = Vec::new();
        err_inner.extend(pb_len_field(1, "内部原始错误".as_bytes()));
        err_inner.extend(pb_len_field(2, "模型配额已达上限".as_bytes()));
        err_inner.extend(pb_len_field(3, b"goroutine 42 internal stack trace"));
        let step17_blob = pb_len_field(24, &pb_len_field(3, &err_inner));

        for (cid, step2_type, step2_status, step2_blob) in [
            (&db_done_id, 15_i64, 3_i64, &step15_blob),
            (&db_err_id, 17_i64, 7_i64, &step17_blob),
        ] {
            let conn =
                Connection::open(root.join("conversations").join(format!("{cid}.db"))).unwrap();
            conn.execute_batch(
                "CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER, status INTEGER, step_payload BLOB);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO steps VALUES (0, 14, 3, ?1), (1, ?2, ?3, ?4)",
                params![step14_blob, step2_type, step2_status, step2_blob],
            )
            .unwrap();
        }

        let db_detail = session_detail(
            &offline,
            &ProviderDetailRequest {
                session_key: db_done_id.clone(),
                limit: Some(20),
                ..Default::default()
            },
        )
        .unwrap();
        let db_um = db_detail
            .events
            .iter()
            .find_map(|e| e.user_message.as_ref())
            .unwrap();
        assert_eq!(db_um.text, "只展示我的真实输入");
        assert!(db_um.context_blocks.is_empty());
        for leaked in [
            "identity",
            "slash_commands",
            "EXPANDED_WORKFLOW_SYSTEM_PROMPT",
            "@boost rewritten",
        ] {
            assert!(!db_um.text.contains(leaked));
        }
        assert!(db_detail
            .events
            .iter()
            .any(|e| e.kind == "thinking" && e.text.as_deref() == Some("INTERNAL_THINKING_TRACE")));
        assert!(db_detail.events.iter().any(
            |e| e.kind == "assistant_message" && e.text.as_deref() == Some("最终纯净回复正文")
        ));

        let err_detail = session_detail(
            &offline,
            &ProviderDetailRequest {
                session_key: db_err_id.clone(),
                limit: Some(20),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            err_detail
                .events
                .iter()
                .find(|e| e.role.as_deref() == Some("error_message"))
                .unwrap()
                .result
                .as_deref(),
            Some("模型配额已达上限")
        );

        let bark_done = scan_offline_store(&offline, Some(&db_done_id)).unwrap();
        assert_eq!(bark_done.streams[0].events[0].kind, "completion");
        assert!(bark_done.streams[0].events[0]
            .body
            .contains("最终纯净回复正文"));
        assert!(!bark_done.streams[0].events[0]
            .body
            .contains("INTERNAL_THINKING_TRACE"));
        assert!(!bark_done.streams[0].events[0]
            .body
            .contains("secret_internal_tool"));

        let bark_err = scan_offline_store(&offline, Some(&db_err_id)).unwrap();
        assert_eq!(bark_err.streams[0].events[0].kind, "failure");
        assert!(bark_err.streams[0].events[0]
            .body
            .contains("模型配额已达上限"));
        assert!(!bark_err.streams[0].events[0].body.contains("goroutine 42"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_user_response_priority_beyond_32_pages_and_subagent_role_delegation_purification() {
        let root = env::temp_dir().join(format!("agenthub-ag-live-sub-{}", uuid::Uuid::new_v4()));
        for d in ["conversations", "brain", "presence"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let live_id = uuid::Uuid::new_v4().to_string();
        fs::write(
            root.join("conversations").join(format!("{live_id}.db")),
            b"live-db",
        )
        .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let live_id_srv = live_id.clone();
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown_srv = shutdown.clone();
        let srv = std::thread::spawn(move || loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            if shutdown_srv.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut req_line = String::new();
            if reader.read_line(&mut req_line).unwrap_or(0) == 0 {
                break;
            }
            let mut len = 0usize;
            let mut hdr = String::new();
            loop {
                hdr.clear();
                if reader.read_line(&mut hdr).unwrap_or(0) == 0 || hdr == "\r\n" {
                    break;
                }
                if let Some((k, v)) = hdr.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            let mut body = vec![0u8; len];
            let _ = reader.read_exact(&mut body);
            let payload = if req_line.contains("GetAllCascadeTrajectories") {
                json!({
                    "trajectorySummaries": {
                        (&live_id_srv): {
                            "summary": "Live 35-page session",
                            "stepCount": 35,
                            "status": "CASCADE_RUN_STATUS_IDLE",
                            "lastModifiedTime": "2026-11-24T12:00:35Z"
                        }
                    }
                })
            } else if req_line.contains("GetCascadeTrajectory ") {
                json!({
                    "numTotalSteps": 35,
                    "trajectory": {
                        "cascadeId": (&live_id_srv),
                        "steps": [{
                            "type": "CORTEX_STEP_TYPE_USER_INPUT",
                            "status": "CORTEX_STEP_STATUS_DONE",
                            "userInput": {
                                "items": [{"text": "@boost @plan expanded-item"}],
                                "userResponse": "<USER_REQUEST>\n/boost /plan 优先读取原始用户输入\n</USER_REQUEST>\n<ADDITIONAL_METADATA>meta</ADDITIONAL_METADATA>"
                            }
                        }]
                    }
                })
            } else {
                let req_json: Value = serde_json::from_slice(&body).unwrap_or_default();
                let offset = req_json["stepOffset"].as_u64().unwrap_or(0) as usize;
                let step = if offset == 34 {
                    json!({
                        "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                        "status": "CORTEX_STEP_STATUS_DONE",
                        "plannerResponse": {"modifiedResponse": "第35页收尾回复"}
                    })
                } else {
                    json!({
                        "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                        "status": "CORTEX_STEP_STATUS_DONE",
                        "plannerResponse": {"thinking": format!("step-{offset}")}
                    })
                };
                json!({"cascadeId": (&live_id_srv), "steps": [step]})
            };
            let resp_bytes = serde_json::to_vec(&payload).unwrap();
            write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    resp_bytes.len()
                )
                .unwrap();
            stream.write_all(&resp_bytes).unwrap();
        });

        let online = AntigravityPaths::with_fixed_rpc(root.clone(), port, "fixture-token");
        let live_detail = session_detail(
            &online,
            &ProviderDetailRequest {
                session_key: live_id.clone(),
                limit: Some(80),
                ..Default::default()
            },
        )
        .unwrap();
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(("127.0.0.1", port));
        srv.join().unwrap();
        let live_um = live_detail
            .events
            .iter()
            .find_map(|e| e.user_message.as_ref())
            .unwrap();
        assert_eq!(live_um.text, "/boost /plan 优先读取原始用户输入");
        assert!(live_um.context_blocks.is_empty());
        assert!(
            live_detail
                .events
                .iter()
                .any(|e| e.text.as_deref() == Some("第35页收尾回复")),
            "live trajectory pagination must not truncate at 32 pages"
        );

        // Subagent sidecar subagentDescriptor.role + agent_name + <original_task> purification + no fake user bubble.
        let root_id = uuid::Uuid::new_v4().to_string();
        let child_sidecar_id = uuid::Uuid::new_v4().to_string();
        let child_db_id = uuid::Uuid::new_v4().to_string();
        let child_self_delegation_id = uuid::Uuid::new_v4().to_string();
        for cid in [
            &root_id,
            &child_sidecar_id,
            &child_db_id,
            &child_self_delegation_id,
        ] {
            fs::write(
                root.join("conversations").join(format!("{cid}.db")),
                b"placeholder",
            )
            .unwrap();
            fs::create_dir_all(root.join("brain").join(cid).join(".system_generated/logs"))
                .unwrap();
        }
        let sidecar_dir = root
            .join("brain")
            .join(&root_id)
            .join(".system_generated/subagents");
        fs::create_dir_all(&sidecar_dir).unwrap();
        fs::write(
            sidecar_dir.join(format!("{child_sidecar_id}.json")),
            serde_json::to_vec(&json!({
                "subagentDescriptor": {"role": "代码审查员", "typeName": "browser_subagent"},
                "state": "SUBAGENT_STATE_DONE"
            }))
            .unwrap(),
        )
        .unwrap();
        let raw_delegation = concat!(
            "<original_task>\n",
            "**Task**: 核验 FFI 与 SQLite 边界\n",
            "</original_task>\n",
            "<prior_attempt>Updated <original_task>...</original_task> handling</prior_attempt>\n",
            "<instructions>confidential worker prompt</instructions>"
        );
        fs::write(
            root.join("brain")
                .join(&root_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n{}\n{}\n{}\n",
                json!({
                    "step_index": 0,
                    "source": "USER_EXPLICIT",
                    "type": "USER_INPUT",
                    "status": "DONE",
                    "created_at": "2026-11-24T12:00:00Z",
                    "content": "<USER_REQUEST>\n检查子代理\n<environment_context>字面量环境说明</environment_context>\n</USER_REQUEST>\n<ADDITIONAL_METADATA>ts=1</ADDITIONAL_METADATA>"
                }),
                json!({
                    "step_index": 1,
                    "source": "MODEL",
                    "type": "PLANNER_RESPONSE",
                    "status": "DONE",
                    "created_at": "2026-11-24T12:00:01Z",
                    "content": "全部检查已完成。",
                    "tool_calls": [
                        {"id": "c-sidecar", "name": "invoke_subagent", "arguments": {"Role": "代码审查员", "TypeName": "internal_worker", "Task": raw_delegation}},
                        {"id": "c-db", "name": "invoke_subagent", "arguments": {"Task": "<original_task>\n**Task**: 检索历史会话\n</original_task>"}},
                        {"id": "c-creating", "name": "invoke_subagent", "arguments": {"Role": "测试验证员", "TypeName": "raw_type", "Task": "<original_task>\n**Task**: 跑门禁检查\n</original_task>"}}
                    ]
                }),
                json!({"step_index": 2, "type": "GENERIC", "status": "DONE", "tool_call_id": "c-sidecar", "content": format!("Created the following subagent:\n{{\"conversationId\":\"{child_sidecar_id}\"}}")}),
                json!({"step_index": 3, "type": "GENERIC", "status": "DONE", "tool_call_id": "c-db", "content": format!("Created the following subagent:\n{{\"conversationId\":\"{child_db_id}\"}}")}),
                json!({"step_index": 4, "source": "USER_EXPLICIT", "type": "USER_INPUT", "status": "DONE", "created_at": "2026-11-24T12:00:02Z", "content": "<USER_SETTINGS_CHANGE>model=gemini-3.1-pro</USER_SETTINGS_CHANGE>"}),
            ),
        )
        .unwrap();
        for cid in [&child_sidecar_id, &child_db_id] {
            fs::write(
                root.join("brain")
                    .join(cid)
                    .join(".system_generated/logs/transcript_full.jsonl"),
                format!(
                    "{}\n{}\n",
                    json!({"step_index": 0, "type": "SYSTEM_MESSAGE", "status": "DONE", "content": raw_delegation}),
                    json!({"step_index": 1, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "content": "子代理检查通过"}),
                ),
            )
            .unwrap();
        }
        fs::write(
            root.join("brain")
                .join(&child_self_delegation_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            format!(
                "{}\n{}\n",
                json!({
                    "step_index": 0,
                    "type": "SYSTEM_MESSAGE",
                    "status": "DONE",
                    "content": "The following is a <SYSTEM_MESSAGE> not actually sent by the user.\n<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-24T12:00:01Z sender=root priority=HIGH content=<original_task>\n**Task**: 从子转录恢复委派\n</original_task>\n<prior_attempt>Fixed </original_task> edge case</prior_attempt>\n</SYSTEM_MESSAGE>"
                }),
                json!({"step_index": 1, "source": "MODEL", "type": "PLANNER_RESPONSE", "status": "DONE", "content": "独立恢复完成"}),
            ),
        )
        .unwrap();
        let s_conn = Connection::open(root.join("conversation_summaries.db")).unwrap();
        s_conn
            .execute_batch(
                "CREATE TABLE conversation_summaries (
                    conversation_id TEXT PRIMARY KEY,
                    title TEXT,
                    agent_name TEXT,
                    preview TEXT,
                    step_count INTEGER,
                    last_modified_time TEXT,
                    workspace_uris TEXT,
                    parent_conversation_id TEXT,
                    nesting_depth INTEGER
                );
                CREATE TABLE conversations_fts (
                    conversation_id TEXT,
                    chunk_index INTEGER,
                    archived INTEGER,
                    title TEXT
                );",
            )
            .unwrap();
        s_conn
            .execute(
                "INSERT INTO conversation_summaries VALUES \
                 (?1, '<USER_REQUEST>', '', '<USER_REQUEST>\n从预览恢复主标题\n</USER_REQUEST>', 3, '2026-11-24T12:00:00Z', '', '', 0), \
                 (?2, '<original_task>', '', '子代理检查通过', 2, '2026-11-24T12:00:01Z', '', ?1, 1), \
                 (?3, '<original_task>', 'SQLite检索员', '检索完成', 2, '2026-11-24T12:00:01Z', '', ?1, 1), \
                 (?4, '<original_task>', '', '[Message] timestamp=2026-11-24T12:00:01Z sender=root priority=HIGH content=从子转录恢复委派', 2, '2026-11-24T12:00:01Z', '', ?1, 1)",
                params![root_id, child_sidecar_id, child_db_id, child_self_delegation_id],
            )
            .unwrap();
        s_conn
            .execute(
                "INSERT INTO conversations_fts VALUES (?1, -1, 0, '<USER_REQUEST>')",
                params![root_id],
            )
            .unwrap();
        drop(s_conn);

        let offline = AntigravityPaths::offline(root.clone());
        let root_sum = session_summary(&offline, &root_id).unwrap();
        assert_eq!(root_sum.title, "从预览恢复主标题");
        assert_eq!(root_sum.status, "idle");
        let root_detail = session_detail(
            &offline,
            &ProviderDetailRequest {
                session_key: root_id.clone(),
                limit: Some(20),
                ..Default::default()
            },
        )
        .unwrap();
        let tl = root_detail.timeline.as_ref().unwrap();
        assert_eq!(tl.len(), 1);
        assert!(
            tl[0].title.contains("字面量环境说明"),
            "Antigravity Rust timeline must preserve literal <environment_context> text"
        );
        let subs = root_detail.subagents.as_ref().unwrap();
        let a_sidecar = subs
            .agents
            .iter()
            .find(|a| a.agent_id.as_deref() == Some(child_sidecar_id.as_str()))
            .unwrap();
        assert_eq!(a_sidecar.name, "代码审查员");
        assert_eq!(
            a_sidecar.delegation.as_deref(),
            Some("核验 FFI 与 SQLite 边界")
        );
        let a_db = subs
            .agents
            .iter()
            .find(|a| a.agent_id.as_deref() == Some(child_db_id.as_str()))
            .unwrap();
        assert_eq!(a_db.name, "SQLite检索员");
        assert_eq!(a_db.delegation.as_deref(), Some("检索历史会话"));
        let a_self = subs
            .agents
            .iter()
            .find(|a| a.agent_id.as_deref() == Some(child_self_delegation_id.as_str()))
            .unwrap();
        assert_eq!(a_self.name, "从子转录恢复委派");
        assert_eq!(a_self.delegation.as_deref(), Some("从子转录恢复委派"));
        let inline_unresolved = root_detail
            .events
            .iter()
            .filter_map(|e| e.subagent.as_ref())
            .find(|a| a.agent_id.is_none())
            .unwrap();
        assert_eq!(inline_unresolved.role.as_deref(), Some("测试验证员"));
        assert_eq!(inline_unresolved.delegation.as_deref(), Some("跑门禁检查"));

        let sub_drawer = session_detail(
            &offline,
            &ProviderDetailRequest {
                session_key: root_id.clone(),
                agent_id: Some(child_sidecar_id),
                limit: Some(20),
                before: None,
            },
        )
        .unwrap();
        assert!(
            sub_drawer.events.iter().all(|e| e.kind != "user_message"),
            "subagent drawer starting with SYSTEM_MESSAGE must not synthesize a fake user bubble"
        );
        assert_eq!(
            sub_drawer
                .agent
                .as_ref()
                .and_then(|a| a.delegation.as_deref()),
            Some("核验 FFI 与 SQLite 边界")
        );

        let fallback_drawer = session_detail(
            &offline,
            &ProviderDetailRequest {
                session_key: root_id,
                agent_id: Some(child_self_delegation_id),
                limit: Some(20),
                before: None,
            },
        )
        .unwrap();
        assert_eq!(
            fallback_drawer
                .agent
                .as_ref()
                .and_then(|a| a.delegation.as_deref()),
            Some("从子转录恢复委派")
        );

        fs::remove_dir_all(root).unwrap();
    }
}
