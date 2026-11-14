use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

mod activity;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct GrokPaths {
    pub home: PathBuf,
}

impl GrokPaths {
    pub fn default_for_user() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            home: env::var_os("GROK_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".grok")),
        }
    }
    pub fn sessions(&self) -> PathBuf {
        self.home.join("sessions")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokSessionSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_eligibility: Option<GrokDeletionEligibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_size: Option<crate::session_storage::SessionStorageSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_path: Option<PathBuf>,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub path: PathBuf,
    pub updated_at: Option<String>,
    pub message_count: usize,
    pub last_message: Option<String>,
    pub status: String,
}

pub fn grok_handoff_path(session_dir: &Path) -> PathBuf {
    let candidates = [
        session_dir.join("chat_history.jsonl"),
        session_dir.join("updates.jsonl"),
        session_dir.join("events.jsonl"),
    ];
    for candidate in &candidates {
        if fs::metadata(candidate).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
            return candidate.clone();
        }
    }
    for candidate in &candidates {
        if candidate.is_file() {
            return candidate.clone();
        }
    }
    let summary = session_dir.join("summary.json");
    if summary.is_file() {
        return summary;
    }
    session_dir.to_path_buf()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokDeletionEligibility {
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokSessionDetail {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<crate::history_navigation::Timeline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<crate::jsonl_cache::HistoryMetadata>,
    #[serde(default)]
    pub total_events: usize,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub before_cursor: Option<String>,
    pub summary: GrokSessionSummary,
    pub events: Vec<Arc<GrokHistoryEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrokHistoryEvent {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_message: Option<crate::user_message::UserMessage>,
    pub timestamp: Option<String>,
    pub kind: String,
    pub text: Option<String>,
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

type SessionDirectories = std::collections::BTreeMap<String, Vec<PathBuf>>;

fn session_directories(root: &Path) -> Result<SessionDirectories> {
    let mut by_id = SessionDirectories::new();
    if !root.is_dir() {
        return Ok(by_id);
    }
    for workspace in fs::read_dir(root)? {
        let workspace = workspace?;
        if !workspace.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(workspace.path())? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(id) = entry.file_name().to_str() {
                    by_id.entry(id.to_owned()).or_default().push(entry.path());
                }
            }
        }
    }
    Ok(by_id)
}

fn list_grok_sessions_from_dirs(
    paths: &GrokPaths,
    by_id: &SessionDirectories,
    limit: usize,
    query: Option<&str>,
) -> Result<Vec<GrokSessionSummary>> {
    let needle = query
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_ascii_lowercase);
    let mut result = Vec::new();
    for dirs in by_id.values() {
        for dir in dirs {
            if let Ok(Some(summary)) =
                read_summary(dir).or_else(|_| Ok::<_, anyhow::Error>(Some(fallback_summary(dir))))
            {
                if needle.as_ref().is_some_and(|needle| {
                    !summary.title.to_ascii_lowercase().contains(needle)
                        && !summary.id.to_ascii_lowercase().contains(needle)
                        && !summary.cwd.to_ascii_lowercase().contains(needle)
                }) {
                    continue;
                }
                result.push(summary);
            }
        }
    }
    result.sort_by_cached_key(|session| {
        (std::cmp::Reverse(session.updated_at.as_deref().and_then(
            |time| chrono::DateTime::parse_from_rfc3339(time).ok(),
        )),)
    });
    result.truncate(limit.clamp(1, 200));
    let active = activity::Snapshot::capture_readonly(paths);
    for summary in &mut result {
        active.update_summary(summary);
    }
    Ok(result)
}

pub fn list_grok_sessions(
    paths: &GrokPaths,
    limit: usize,
    query: Option<&str>,
) -> Result<Vec<GrokSessionSummary>> {
    let root = paths.sessions();
    let mut items = list_grok_sessions_without_storage(paths, limit, query)?;
    for summary in &mut items {
        summary.storage_size = Some(crate::session_storage::directory_size(
            &root,
            &summary.path,
            None,
            &summary.id,
        ));
    }
    Ok(items)
}

pub(crate) fn list_grok_sessions_without_storage(
    paths: &GrokPaths,
    limit: usize,
    query: Option<&str>,
) -> Result<Vec<GrokSessionSummary>> {
    let root = paths.sessions();
    list_grok_sessions_from_dirs(paths, &session_directories(&root)?, limit, query)
}

pub(crate) fn probe_sessions(
    paths: &GrokPaths,
    limit: usize,
    candidate: impl Fn(&GrokSessionSummary) -> bool,
) -> Result<(Vec<GrokSessionSummary>, crate::native_probe::NativeScan)> {
    let root = paths.sessions().canonicalize()?;
    let mut by_id = session_directories(&root)?;
    let items = list_grok_sessions_from_dirs(paths, &by_id, limit, None)?;
    let selected: std::collections::BTreeSet<_> = items
        .iter()
        .filter(|s| candidate(s))
        .map(|s| s.id.as_str())
        .collect();
    by_id.retain(|id, _| selected.contains(id.as_str()));
    let scan = notification_snapshots_from_dirs(&root, by_id, None)?;
    Ok((items, scan))
}

type GrokProbeStamp = (
    u64,
    Option<std::time::SystemTime>,
    u64,
    Option<std::time::SystemTime>,
);

struct GrokProbeCacheEntry {
    session_dir: PathBuf,
    identity: String,
    stamp: GrokProbeStamp,
    title: String,
    snapshot: crate::native_probe::NativeStreamSnapshot,
    bytes: u64,
    last_accessed: std::time::Instant,
}

fn grok_probe_cache_stamp(
    event_path: &Path,
    updates_path: &Path,
) -> Result<(String, GrokProbeStamp)> {
    let event_meta = fs::symlink_metadata(event_path)?;
    let updates_meta = fs::symlink_metadata(updates_path)?;
    let identity = format!(
        "{}:{}",
        crate::native_probe::file_identity(event_path)?,
        crate::native_probe::file_identity(updates_path)?
    );
    let stamp = (
        event_meta.len(),
        event_meta.modified().ok(),
        updates_meta.len(),
        updates_meta.modified().ok(),
    );
    Ok((identity, stamp))
}

struct GrokProbeCache {
    entries: std::collections::VecDeque<GrokProbeCacheEntry>,
    bytes: u64,
}
static GROK_PROBE_CACHE: std::sync::Mutex<GrokProbeCache> = std::sync::Mutex::new(GrokProbeCache {
    entries: std::collections::VecDeque::new(),
    bytes: 0,
});

#[cfg(test)]
thread_local! { static PROBE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

pub(crate) fn notification_snapshots(paths: &GrokPaths) -> Result<crate::native_probe::NativeScan> {
    if !paths.sessions().exists() {
        return Ok(crate::native_probe::NativeScan::default());
    }
    let root = paths.sessions().canonicalize()?;
    notification_snapshots_from_dirs(&root, session_directories(&root)?, None)
}

pub(crate) fn notification_for_event(
    paths: &GrokPaths,
    id: &str,
    event: &crate::native_probe::NativeTurnEvent,
) -> Result<crate::native_probe::NativeScan> {
    let root = match paths.sessions().canonicalize() {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error.into()),
    };
    let by_id = SessionDirectories::from([(id.to_owned(), locate_sessions(paths, id)?)]);
    notification_snapshots_from_dirs(&root, by_id, Some(event.position))
}

fn notification_snapshots_from_dirs(
    root: &Path,
    by_id: SessionDirectories,
    current_position: Option<u64>,
) -> Result<crate::native_probe::NativeScan> {
    use crate::native_probe::{
        grok_events, read_jsonl, NativeProvider, NativeScan, NativeStreamSnapshot,
    };
    let mut scan = NativeScan::default();
    for (id, dirs) in by_id {
        let read = (|| -> Result<Option<NativeStreamSnapshot>> {
            if dirs.is_empty() {
                return Ok(None);
            }
            uuid::Uuid::parse_str(&id).context("invalid Grok session ID")?;
            ensure!(dirs.len() == 1, "ambiguous Grok session ID");
            let raw_dir = &dirs[0];
            ensure!(
                !fs::symlink_metadata(raw_dir)?.file_type().is_symlink(),
                "Grok session must not be a symlink"
            );
            let canonical_dir = raw_dir.canonicalize()?;
            ensure!(
                canonical_dir.parent().and_then(Path::parent) == Some(root),
                "Grok session escaped sessions root"
            );
            let summary = read_summary_metadata(&canonical_dir)?
                .context("Grok session summary unavailable")?;
            let event_path = canonical_dir.join("events.jsonl");
            let updates_path = canonical_dir.join("updates.jsonl");
            let (identity, stamp) = grok_probe_cache_stamp(&event_path, &updates_path)?;
            let current_events = if let Some(position) = current_position {
                let events = read_jsonl(&event_path)?;
                if !event_position_current(&events, position) {
                    return Ok(None);
                }
                Some(events)
            } else {
                None
            };
            let title =
                crate::native_probe::notification_title(&summary.title, &summary.id, &summary.cwd);
            if let Ok(mut cache) = GROK_PROBE_CACHE.lock() {
                if let Some(pos) = cache.entries.iter().position(|e| {
                    e.session_dir == canonical_dir
                        && e.identity == identity
                        && e.stamp == stamp
                        && e.title == title
                }) {
                    let mut hit = cache.entries.remove(pos).expect("grok probe cache entry");
                    hit.last_accessed = std::time::Instant::now();
                    let out = hit.snapshot.clone();
                    cache.entries.push_back(hit);
                    return Ok(Some(out));
                }
            }
            #[cfg(test)]
            PROBE_READS.with(|c| c.set(c.get() + 1));
            let events = match current_events {
                Some(events) => events,
                None => read_jsonl(&event_path)?,
            };
            let updates = read_jsonl(&updates_path)?;
            let (turns, settled_count) = grok_events(&id, &events, &updates);
            let snapshot = NativeStreamSnapshot {
                provider: NativeProvider::Grok,
                session_key: id.clone(),
                id,
                title: title.clone(),
                identity: identity.clone(),
                record_count: events.len() as u64,
                settled_count,
                events: turns.into(),
            };
            let (after_identity, after_stamp) = grok_probe_cache_stamp(&event_path, &updates_path)?;
            if after_identity == identity && after_stamp == stamp {
                let bytes = 512
                    + (snapshot.id.len()
                        + snapshot.title.len()
                        + snapshot.identity.len()
                        + snapshot
                            .events
                            .iter()
                            .map(|e| 256 + e.turn_id.len() + e.body.len())
                            .sum::<usize>()) as u64;
                if bytes <= crate::resource_limits::NATIVE_PROBE_CACHE_BYTES {
                    if let Ok(mut cache) = GROK_PROBE_CACHE.lock() {
                        if let Some(pos) = cache
                            .entries
                            .iter()
                            .position(|e| e.session_dir == canonical_dir)
                        {
                            let old = cache.entries.remove(pos).expect("located cache entry");
                            cache.bytes -= old.bytes;
                        }
                        while cache.entries.len() >= crate::resource_limits::CLAUDE_SUMMARY_ENTRIES
                            || cache.bytes + bytes
                                > crate::resource_limits::NATIVE_PROBE_CACHE_BYTES
                        {
                            if let Some(old) = cache.entries.pop_front() {
                                cache.bytes -= old.bytes;
                            } else {
                                break;
                            }
                        }
                        cache.bytes += bytes;
                        cache.entries.push_back(GrokProbeCacheEntry {
                            session_dir: canonical_dir,
                            identity,
                            stamp,
                            title,
                            snapshot: snapshot.clone(),
                            bytes,
                            last_accessed: std::time::Instant::now(),
                        });
                    }
                }
            }
            Ok(Some(snapshot))
        })();
        match read {
            Ok(Some(stream)) => scan.streams.push(stream),
            Ok(None) => {}
            Err(_) => scan.errors += 1,
        }
    }
    Ok(scan)
}

fn event_position_current(events: &[Value], position: u64) -> bool {
    position as usize <= events.len()
        && !events[position as usize..]
            .iter()
            .any(|r| matches!(r["type"].as_str(), Some("turn_started" | "turn_ended")))
}

pub(crate) fn grok_session_summary(paths: &GrokPaths, id: &str) -> Result<GrokSessionSummary> {
    resolve_session(paths, id)
}

pub fn grok_session_detail(
    paths: &GrokPaths,
    id: &str,
    cwd: Option<&str>,
) -> Result<GrokSessionDetail> {
    grok_session_page(paths, id, cwd, None, None, None)
}

pub(crate) fn grok_session_detail_without_storage(
    paths: &GrokPaths,
    id: &str,
) -> Result<GrokSessionDetail> {
    grok_session_page_with_storage(paths, id, None, None, None, None, false)
}

pub fn grok_session_page(
    paths: &GrokPaths,
    id: &str,
    cwd: Option<&str>,
    limit: Option<usize>,
    before: Option<&str>,
    since: Option<&str>,
) -> Result<GrokSessionDetail> {
    grok_session_page_with_storage(paths, id, cwd, limit, before, since, true)
}

fn grok_session_page_with_storage(
    paths: &GrokPaths,
    id: &str,
    cwd: Option<&str>,
    limit: Option<usize>,
    before: Option<&str>,
    since: Option<&str>,
    include_storage: bool,
) -> Result<GrokSessionDetail> {
    let mut summary = resolve_session(paths, id)?;
    ensure!(
        cwd.is_none_or(|cwd| summary.cwd == cwd),
        "Grok workspace identity changed"
    );
    if include_storage {
        summary.storage_size = Some(crate::session_storage::directory_size(
            &paths.sessions(),
            &summary.path,
            None,
            &summary.id,
        ));
    }
    let path = summary.path.clone();
    with_session_history(&path, |state, exists| {
        summary.updated_at = state.last_message_at.clone();
        let source = &state.events.events;
        let total_events = source.len();
        let (start, end) = match crate::history_navigation::seek_window(
            source.iter().map(|event| event.id.as_str()),
            before,
            limit.unwrap_or(160).clamp(1, 500),
        )? {
            Some(window) => window,
            None => {
                let end = match before {
                    Some(id) => source
                        .iter()
                        .position(|event| event.id == id)
                        .context("历史位置已变化，请刷新会话")?,
                    None => total_events,
                };
                (
                    limit.map_or(0, |n| end.saturating_sub(n.clamp(1, 500))),
                    end,
                )
            }
        };
        let mut events = source[start..end].to_vec();
        for event in &mut events {
            if event
                .user_message
                .as_ref()
                .is_some_and(|message| !message.attachments.is_empty())
            {
                if let Some(message) = Arc::make_mut(event).user_message.as_mut() {
                    message.refresh_files(Some(&summary.cwd));
                }
            }
        }
        Ok(GrokSessionDetail {
            timeline: before.is_none().then(|| state.timeline.clone()),
            after_cursor: crate::history_navigation::after_cursor(
                end,
                total_events,
                source
                    .get(end.saturating_sub(1))
                    .map(|event| event.id.as_str()),
            ),
            summary,
            events,
            total_events,
            has_more: start > 0,
            before_cursor: (start > 0).then(|| source[start].id.clone()),
            history: exists.then(|| state.journal.metadata(since)),
        })
    })
}

static GROK_DETAIL_CACHE: crate::jsonl_cache::JsonlCache<GrokReader> =
    crate::jsonl_cache::JsonlCache::new(crate::resource_limits::GROK_DETAIL_CACHE_BYTES);
static GROK_LAST_MESSAGE_CACHE: crate::read_cache::ReadCache<Option<String>> =
    crate::read_cache::ReadCache::new(crate::resource_limits::GROK_ACTIVITY_CACHE_BYTES)
        .with_entry_limit(crate::resource_limits::CLAUDE_SUMMARY_ENTRIES)
        .with_native_metadata();

pub(crate) fn evict_idle_detail_cache(max_idle: Duration) {
    GROK_DETAIL_CACHE.evict_idle(max_idle);
    GROK_LAST_MESSAGE_CACHE.evict_idle(max_idle);
    activity::evict_idle_cache(max_idle);
    if let Ok(mut cache) = GROK_PROBE_CACHE.lock() {
        let now = std::time::Instant::now();
        let mut removed = 0;
        cache.entries.retain(|entry| {
            let keep = now.saturating_duration_since(entry.last_accessed) < max_idle;
            if !keep {
                removed += entry.bytes;
            }
            keep
        });
        cache.bytes -= removed;
    }
}

fn with_session_history<R>(
    directory: &Path,
    project: impl FnOnce(&GrokReader, bool) -> Result<R>,
) -> Result<R> {
    let updates = directory.join("updates.jsonl");
    let native = !updates.exists();
    let path = if native {
        directory.join("chat_history.jsonl")
    } else {
        updates
    };
    if !path.exists() {
        return project(&GrokReader::default(), false);
    }
    GROK_DETAIL_CACHE.read(
        &path,
        |_| Ok(0),
        true,
        |state, line, _, start| {
            if let Ok(value) = serde_json::from_slice::<Value>(line) {
                if native {
                    state.push_native(value, start);
                } else {
                    state.push(value, start);
                }
            }
            Ok(())
        },
        |state, info| {
            if info.changed {
                #[cfg(test)]
                HISTORY_READS.with(|reads| reads.set(reads.get() + 1));
                if !native
                    && !state.reconciled_initial_native_turns
                    && !state.events.events.is_empty()
                {
                    state.reconciled_initial_native_turns = true;
                    state.prepend_missing_initial_native_users(directory);
                }
                let dirty = std::mem::take(&mut state.events.dirty);
                state.journal.record(&info, dirty.iter().cloned().collect());
                state.timeline = crate::history_navigation::update_timeline(
                    &state.timeline,
                    state
                        .events
                        .events
                        .iter()
                        .enumerate()
                        .filter(|(_, event)| event.kind == "user_message_chunk")
                        .map(|(index, event)| {
                            (
                                event.id.as_str(),
                                event.user_message.as_ref(),
                                event.text.as_deref(),
                                index < state.previous_count && !dirty.contains(&event.id),
                            )
                        }),
                );
                state.previous_count = state.events.events.len();
                state.weight = state
                    .events
                    .events
                    .iter()
                    .map(|event| {
                        256 + event
                            .user_message
                            .as_ref()
                            .map_or(0, crate::user_message::UserMessage::weight)
                            + event.text.as_ref().map_or(0, String::len) as u64
                            + event.detail.as_ref().map_or(0, String::len) as u64
                    })
                    .sum::<u64>()
                    + crate::history_navigation::weight(&state.timeline);
            }
            project(state, true)
        },
        |state| state.weight,
    )
}

#[cfg(test)]
thread_local! { static HISTORY_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(Default)]
struct GrokReader {
    last_message_at: Option<String>,
    previous_count: usize,
    reconciled_initial_native_turns: bool,
    timeline: crate::history_navigation::Timeline,
    events: GrokEvents,
    journal: crate::jsonl_cache::HistoryJournal,
    weight: u64,
}
#[derive(Default)]
struct GrokEvents {
    events: Vec<Arc<GrokHistoryEvent>>,
    tool_indices: HashMap<String, usize>,
    user_parts: Vec<Value>,
    user_native_key: Option<String>,
    user_start: u64,
    seen_user_updates: HashSet<String>,
    dirty: std::collections::BTreeSet<String>,
}
impl GrokReader {
    fn prepend_missing_initial_native_users(&mut self, directory: &Path) {
        if self
            .events
            .events
            .first()
            .is_none_or(|first| first.kind == "user_message_chunk")
        {
            return;
        }
        let chat_path = directory.join("chat_history.jsonl");
        let Ok(meta) = fs::symlink_metadata(&chat_path) else {
            return;
        };
        if !meta.is_file() || meta.file_type().is_symlink() {
            return;
        }
        use std::io::{BufRead, BufReader, Read};
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let Ok(file) = options.open(&chat_path) else {
            return;
        };
        let mut reader = BufReader::new(file.take(1024 * 1024));
        let mut line = Vec::new();
        let mut offset = 0u64;
        let mut prepended = Vec::new();
        for _ in 0..64 {
            line.clear();
            let Ok(read) = reader.read_until(b'\n', &mut line) else {
                break;
            };
            if read == 0 {
                break;
            }
            let start = offset;
            offset += read as u64;
            let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            if value.get("synthetic_reason").and_then(Value::as_str) == Some("system_reminder") {
                continue;
            }
            match value.get("type").and_then(Value::as_str).unwrap_or("") {
                "system" => continue,
                "user" => {
                    let raw_content = value.get("content").cloned().unwrap_or(Value::Null);
                    let parts = match raw_content {
                        Value::String(text) => {
                            vec![serde_json::json!({"type": "text", "text": text})]
                        }
                        Value::Array(arr) => arr,
                        other => vec![other],
                    };
                    let id = format!("grok:init:{start}");
                    let message =
                        crate::user_message::parse_user_message(&id, &Value::Array(parts));
                    if message.text.trim().is_empty() && message.attachments.is_empty() {
                        continue;
                    }
                    if self.last_message_at.is_none() {
                        self.last_message_at = value
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                    prepended.push(Arc::new(GrokHistoryEvent {
                        id: id.clone(),
                        user_message: Some(message.clone()),
                        timestamp: value
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        kind: "user_message_chunk".into(),
                        text: Some(message.text),
                        method: None,
                        call_id: None,
                        status: None,
                        detail: None,
                    }));
                    self.events.dirty.insert(id);
                }
                _ => break,
            }
        }
        if !prepended.is_empty() {
            let shift = prepended.len();
            for idx in self.events.tool_indices.values_mut() {
                *idx += shift;
            }
            prepended.append(&mut self.events.events);
            self.events.events = prepended;
        }
    }

    fn push_native(&mut self, value: Value, start: u64) {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if value.get("synthetic_reason").and_then(Value::as_str) == Some("system_reminder") {
            return;
        }
        let content = value.get("content").cloned().unwrap_or(Value::Null);
        let text = match &content {
            Value::String(text) => text.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter(|part| {
                    part.get("type")
                        .and_then(Value::as_str)
                        .is_none_or(|kind| kind == "text")
                })
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        match kind {
            "user" => {
                self.events.user_parts.clear();
                let content = if let Value::String(text) = content { serde_json::json!({"type":"text","text":text}) } else { content };
                self.push(serde_json::json!({"timestamp":value.get("timestamp"),"sessionUpdate":"user_message_chunk","messageId":format!("native:{start}"),"content":content}), start);
            }
            "assistant" => {
                if !text.is_empty() {
                    self.push(serde_json::json!({"timestamp":value.get("timestamp"),"sessionUpdate":"agent_message_chunk","messageId":format!("native:{start}"),"content":{"text":text}}), start);
                }
                for (index, call) in value.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
                    self.push(serde_json::json!({"timestamp":value.get("timestamp"),"sessionUpdate":"tool_call","toolCallId":id,"title":call.get("name").or_else(||call.pointer("/function/name")),"status":"pending",
                        "content":[{"content":{"text":call.get("arguments").or_else(||call.pointer("/function/arguments")).map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).unwrap_or_default()}}]}),start);
                    if let Some(event) = self.events.events.last_mut() {
                        if event.call_id.as_deref() == Some(id) { Arc::make_mut(event).id = format!("grok:{start}:tool:{index}"); }
                    }
                }
            }
            "tool_result" => self.push(serde_json::json!({"timestamp":value.get("timestamp"),"sessionUpdate":"tool_call_update","toolCallId":value.get("tool_call_id"),"status":"completed","content":[{"content":{"text":text}}]}), start),
            _ => {},
        }
    }

    fn push(&mut self, value: Value, line_start: u64) {
        let GrokEvents {
            mut events,
            mut tool_indices,
            mut user_parts,
            mut user_native_key,
            mut user_start,
            mut seen_user_updates,
            mut dirty,
        } = std::mem::take(&mut self.events);
        for value in std::iter::once(value) {
            let params = value.get("params").unwrap_or(&value);
            let update = params.get("update").unwrap_or(params);
            let kind = update
                .get("sessionUpdate")
                .and_then(Value::as_str)
                .unwrap_or("event")
                .to_string();
            if kind != "user_message_chunk" {
                user_parts.clear();
            }
            if !matches!(
                kind.as_str(),
                "user_message_chunk"
                    | "agent_message_chunk"
                    | "tool_call"
                    | "tool_call_update"
                    | "plan"
            ) {
                continue;
            }
            let text = update
                .get("content")
                .and_then(|v| v.get("text"))
                .and_then(Value::as_str)
                .or_else(|| update.get("text").and_then(Value::as_str))
                .or_else(|| update.get("title").and_then(Value::as_str))
                .map(ToString::to_string);
            let call_id = update
                .get("toolCallId")
                .and_then(Value::as_str)
                .map(str::to_string);
            let status = update
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string);
            let detail = update
                .get("content")
                .and_then(Value::as_array)
                .map(|content| {
                    content
                        .iter()
                        .filter_map(|item| item.pointer("/content/text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|text| !text.is_empty());
            if kind == "user_message_chunk" {
                if let Some(id) = update.get("updateId").and_then(Value::as_str) {
                    if !seen_user_updates.insert(id.to_owned()) {
                        continue;
                    }
                }
                let native_key = update
                    .get("messageId")
                    .or_else(|| update.get("promptId"))
                    .or_else(|| params.get("prompt_id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let same = !user_parts.is_empty()
                    && events
                        .last()
                        .is_some_and(|event: &Arc<GrokHistoryEvent>| event.kind == kind)
                    && user_native_key == native_key;
                if !same {
                    user_parts.clear();
                    user_start = line_start;
                    user_native_key = native_key;
                }
                let content = update.get("content").cloned().unwrap_or_else(
                    || serde_json::json!({"type":"text", "text":text.clone().unwrap_or_default()}),
                );
                match content {
                    Value::Array(parts) => user_parts.extend(parts),
                    other => user_parts.push(other),
                }
                let message = crate::user_message::parse_user_message(
                    &format!("grok:{user_start}"),
                    &Value::Array(user_parts.clone()),
                );
                if !message.text.trim().is_empty() || !message.attachments.is_empty() {
                    self.last_message_at = value
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                if same {
                    let previous = Arc::make_mut(events.last_mut().unwrap());
                    dirty.insert(previous.id.clone());
                    previous.text = Some(message.text.clone());
                    previous.user_message = Some(message);
                } else {
                    events.push(Arc::new(GrokHistoryEvent {
                        id: format!("grok:{line_start}"),
                        user_message: Some(message.clone()),
                        timestamp: value
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        kind,
                        text: Some(message.text),
                        method: value
                            .get("method")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        call_id,
                        status,
                        detail,
                    }));
                }
                continue;
            }
            user_parts.clear();
            if kind.starts_with("tool_") {
                if let Some(index) = call_id.as_ref().and_then(|id| tool_indices.get(id)) {
                    let previous = Arc::make_mut(&mut events[*index]);
                    dirty.insert(previous.id.clone());
                    if text.is_some() {
                        previous.text = text;
                    }
                    if status.is_some() {
                        previous.status = status;
                    }
                    if detail.is_some() {
                        previous.detail = detail;
                    }
                    continue;
                }
                if let Some(id) = &call_id {
                    tool_indices.insert(id.clone(), events.len());
                }
            }
            if kind == "agent_message_chunk"
                && text.as_ref().is_some_and(|text| !text.trim().is_empty())
            {
                self.last_message_at = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            if kind.ends_with("message_chunk") && update.get("messageId").is_none() {
                if let Some(previous) = events
                    .last_mut()
                    .filter(|event: &&mut Arc<GrokHistoryEvent>| event.kind == kind)
                {
                    let previous = Arc::make_mut(previous);
                    dirty.insert(previous.id.clone());
                    previous
                        .text
                        .get_or_insert_with(String::new)
                        .push_str(text.as_deref().unwrap_or_default());
                    continue;
                }
            }
            events.push(Arc::new(GrokHistoryEvent {
                id: format!("grok:{line_start}"),
                user_message: None,
                timestamp: value.get("timestamp").map(|value| {
                    value
                        .as_str()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| value.to_string())
                }),
                kind,
                text,
                method: value
                    .get("method")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                call_id,
                status,
                detail,
            }));
        }
        self.events = GrokEvents {
            events,
            tool_indices,
            user_parts,
            user_native_key,
            user_start,
            seen_user_updates,
            dirty,
        };
    }
}

#[cfg(test)]
fn read_history(path: &Path) -> Result<Vec<GrokHistoryEvent>> {
    let cache = crate::jsonl_cache::JsonlCache::<GrokReader>::new(192 * 1024 * 1024);
    cache.read(
        path,
        |_| Ok(0),
        true,
        |state, line, _, start| {
            if let Ok(value) = serde_json::from_slice::<Value>(line) {
                state.push(value, start);
            }
            Ok(())
        },
        |state, _| {
            Ok(state
                .events
                .events
                .iter()
                .map(|event| (**event).clone())
                .collect())
        },
        |_| 0,
    )
}

fn read_last_message_at(directory: &Path) -> Result<Option<String>> {
    let updates = directory.join("updates.jsonl");
    let native = !updates.exists();
    let path = if native {
        directory.join("chat_history.jsonl")
    } else {
        updates
    };
    if !path.exists() {
        return Ok(None);
    }
    GROK_LAST_MESSAGE_CACHE.read(
        &path,
        |val, _| 64 + val.as_ref().map_or(0, String::len) as u64,
        || scan_grok_last_message_at(directory, &path, native),
    )
}

fn scan_grok_last_message_at(
    directory: &Path,
    path: &Path,
    native: bool,
) -> Result<Option<String>> {
    use std::io::{BufRead, BufReader};
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "Grok history must be a regular file"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut reader = BufReader::new(options.open(path)?);
    let mut line = Vec::new();
    let mut offset = 0u64;
    let mut state = GrokReader::default();
    loop {
        line.clear();
        let read_bytes = reader.read_until(b'\n', &mut line)?;
        if read_bytes == 0 {
            break;
        }
        let start = offset;
        offset += read_bytes as u64;
        if let Ok(value) = serde_json::from_slice::<Value>(&line) {
            if native {
                state.push_native(value, start);
            } else {
                state.push(value, start);
            }
        }
    }
    if !native && !state.events.events.is_empty() {
        state.prepend_missing_initial_native_users(directory);
    }
    Ok(state.last_message_at)
}

fn read_summary(path: &Path) -> Result<Option<GrokSessionSummary>> {
    let mut summary = read_summary_metadata(path)?;
    if let Some(summary) = &mut summary {
        summary.updated_at = read_last_message_at(path).unwrap_or_default();
    }
    Ok(summary)
}

fn read_summary_metadata(path: &Path) -> Result<Option<GrokSessionSummary>> {
    let file = path.join("summary.json");
    if !file.is_file() {
        return Ok(Some(fallback_summary(path)));
    }
    ensure!(
        !fs::symlink_metadata(&file)?.file_type().is_symlink(),
        "Grok summary must not be a symlink"
    );
    ensure!(
        fs::metadata(&file)?.len() <= 1024 * 1024,
        "Grok summary exceeds size limit"
    );
    static CACHE: crate::read_cache::ReadCache<Value> =
        crate::read_cache::ReadCache::new(crate::resource_limits::GROK_ACTIVITY_CACHE_BYTES);
    let value = CACHE.read(
        &file,
        |_, size| size.saturating_mul(4),
        || Ok(serde_json::from_str::<Value>(&fs::read_to_string(&file)?)?),
    )?;
    let id = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_string();
    let info = value.get("info").unwrap_or(&Value::Null);
    ensure!(
        info.get("id").and_then(Value::as_str) == Some(&id),
        "Grok session identity mismatch"
    );
    let cwd = info
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    ensure!(
        Path::new(&cwd).is_absolute(),
        "Grok session has no absolute workspace"
    );
    let title = value
        .get("generated_title")
        .and_then(Value::as_str)
        .or_else(|| value.get("last_turn_summary").and_then(Value::as_str))
        .unwrap_or("未命名会话")
        .trim()
        .to_string();
    let message_count = value
        .get("num_messages")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    Ok(Some(GrokSessionSummary {
        deletion_eligibility: None,
        storage_size: None,
        handoff_path: Some(grok_handoff_path(path)),
        id,
        title,
        cwd,
        path: path.to_path_buf(),
        updated_at: None,
        message_count,
        last_message: value
            .get("last_turn_summary")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        status: "recent".to_string(),
    }))
}

fn resolve_session(paths: &GrokPaths, id: &str) -> Result<GrokSessionSummary> {
    resolve_session_with_activity(paths, id, &activity::Snapshot::capture_readonly(paths))
}

fn resolve_session_with_activity(
    paths: &GrokPaths,
    id: &str,
    active: &activity::Snapshot,
) -> Result<GrokSessionSummary> {
    uuid::Uuid::parse_str(id).context("invalid Grok session ID")?;
    let root = paths
        .sessions()
        .canonicalize()
        .context("Grok sessions unavailable")?;
    let mut found = None;
    for workspace in fs::read_dir(&root)? {
        let workspace = workspace?;
        if !workspace.file_type()?.is_dir() {
            continue;
        }
        let path = workspace.path().join(id);
        if !path.try_exists()? {
            continue;
        }
        ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "Grok session must not be a symlink"
        );
        let canonical = path.canonicalize()?;
        ensure!(
            canonical.parent().and_then(Path::parent) == Some(root.as_path()),
            "Grok session escaped sessions root"
        );
        if let Some(mut summary) = read_summary(&canonical)? {
            ensure!(found.is_none(), "ambiguous Grok session ID");
            active.update_summary(&mut summary);
            found = Some(summary);
        }
    }
    found.context("Grok session not found")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokDeletePreview {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub fingerprint: String,
    pub file_count: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokDeleteRequest {
    pub id: String,
    pub confirmed: bool,
    #[serde(default)]
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokDeleteResult {
    pub id: String,
    pub deleted: bool,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub residuals: Vec<String>,
}

fn fallback_summary(path: &Path) -> GrokSessionSummary {
    let id = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    GrokSessionSummary {
        id: id.clone(),
        title: id,
        cwd: String::new(),
        handoff_path: Some(grok_handoff_path(path)),
        path: path.into(),
        updated_at: None,
        message_count: 0,
        last_message: None,
        status: "unknown".into(),
        deletion_eligibility: Some(GrokDeletionEligibility {
            allowed: true,
            reason: None,
        }),
        storage_size: None,
    }
}

fn session_workspaces(paths: &GrokPaths) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(paths.sessions()) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut workspaces = Vec::new();
    for workspace in entries {
        let workspace = workspace?;
        if workspace.file_type()?.is_dir() {
            workspaces.push(workspace.path());
        }
    }
    workspaces.sort();
    Ok(workspaces)
}

fn locate_in_workspaces(workspaces: &[PathBuf], id: &str) -> Result<Vec<PathBuf>> {
    crate::session_delete::valid_id(id)?;
    let mut result = Vec::new();
    for workspace in workspaces {
        let path = workspace.join(id);
        match fs::symlink_metadata(&path) {
            Ok(_) => result.push(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(result)
}

fn locate_sessions(paths: &GrokPaths, id: &str) -> Result<Vec<PathBuf>> {
    crate::session_delete::valid_id(id)?;
    locate_in_workspaces(&session_workspaces(paths)?, id)
}

pub fn preview_grok_delete(paths: &GrokPaths, id: &str) -> Result<GrokDeletePreview> {
    DeletionContext::new(paths).preview(id)
}

pub(crate) struct DeletionContext<'a> {
    paths: &'a GrokPaths,
}
impl<'a> DeletionContext<'a> {
    pub(crate) fn new(paths: &'a GrokPaths) -> Self {
        Self { paths }
    }
    pub(crate) fn summary(&self, id: &str) -> Result<GrokSessionSummary> {
        let paths = locate_sessions(self.paths, id)?;
        let path = paths.first().context("Grok session not found")?;
        Ok(read_summary(path)
            .ok()
            .flatten()
            .unwrap_or_else(|| fallback_summary(path)))
    }
    pub(crate) fn preview(&self, id: &str) -> Result<GrokDeletePreview> {
        let paths = locate_sessions(self.paths, id)?;
        let (file_count, bytes) = crate::session_delete::size(&paths);
        Ok(GrokDeletePreview {
            id: id.into(),
            title: id.into(),
            path: paths.first().cloned().unwrap_or_default(),
            fingerprint: String::new(),
            file_count,
            bytes,
        })
    }
}

pub(crate) fn grok_fingerprint(located: &[PathBuf]) -> Result<String> {
    let mut sorted = located.to_vec();
    sorted.sort();
    let mut hash = Sha256::new();
    hash.update(b"grok:v1\0");
    for root in &sorted {
        let mut entries = Vec::new();
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .follow_root_links(false)
        {
            let entry = entry?;
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .into_owned();
            entries.push((rel, entry.into_path()));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        hash.update(b"root\0");
        for (rel, path) in entries {
            let meta = fs::symlink_metadata(&path)?;
            hash.update(rel.as_bytes());
            hash.update(b"\0");
            if meta.file_type().is_symlink() {
                hash.update(b"L");
                if let Ok(target) = fs::read_link(&path) {
                    hash.update(target.to_string_lossy().as_bytes());
                }
            } else if meta.is_dir() {
                hash.update(b"D");
            } else {
                hash.update(b"F");
                hash.update(meta.len().to_le_bytes());
                if let Ok(modified) = meta.modified() {
                    if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                        hash.update(dur.as_secs().to_le_bytes());
                        hash.update(dur.subsec_nanos().to_le_bytes());
                    }
                }
                if meta.len() <= 64 * 1024 {
                    if let Ok(bytes) = fs::read(&path) {
                        hash.update(&bytes);
                    }
                }
            }
            hash.update(b"\n");
        }
    }
    Ok(hex::encode(hash.finalize()))
}

pub(crate) fn find_empty_inactive_sessions(
    paths: &GrokPaths,
) -> Result<Vec<(GrokSessionSummary, String, u64)>> {
    let root = paths.sessions();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let snapshot = activity::Snapshot::capture(paths);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for workspace in fs::read_dir(&root).with_context(|| format!("read {}", root.display()))? {
        let workspace = workspace?;
        if !workspace.file_type()?.is_dir() {
            continue;
        }
        for session in fs::read_dir(workspace.path())? {
            let session = session?;
            if !session.file_type()?.is_dir() {
                continue;
            }
            let session_path = session.path();
            if !crate::session_delete::within(&root, &session_path) {
                continue;
            }
            let Some(mut summary) = read_summary(&session_path).ok().flatten() else {
                continue;
            };
            if crate::session_delete::valid_id(&summary.id).is_err()
                || !seen.insert(summary.id.clone())
            {
                continue;
            }
            snapshot.update_summary(&mut summary);
            if summary.message_count != 0
                || summary.status != "recent"
                || snapshot.ownership(&summary) != activity::Ownership::Inactive
            {
                continue;
            }
            let Ok(located) = locate_sessions(paths, &summary.id) else {
                continue;
            };
            if located.is_empty()
                || located.iter().any(|p| {
                    !crate::session_delete::within(&root, p) || {
                        let s = read_summary(p)
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| fallback_summary(p));
                        snapshot.ownership(&s) != activity::Ownership::Inactive
                    }
                })
            {
                continue;
            }
            if grok_session_detail_without_storage(paths, &summary.id)
                .map(|detail| !detail.events.is_empty())
                .unwrap_or(true)
            {
                continue;
            }
            let Ok(fp) = grok_fingerprint(&located) else {
                continue;
            };
            let (_, bytes) = crate::session_delete::size(&located);
            out.push((summary, fp, bytes));
        }
    }
    Ok(out)
}

pub fn execute_grok_delete(
    paths: &GrokPaths,
    request: GrokDeleteRequest,
) -> Result<GrokDeleteResult> {
    execute_grok_delete_batch(paths, vec![request]).remove(0)
}

pub(crate) fn execute_grok_delete_batch(
    paths: &GrokPaths,
    requests: Vec<GrokDeleteRequest>,
) -> Vec<Result<GrokDeleteResult>> {
    let workspaces = session_workspaces(paths);
    let sessions_root = paths.sessions();
    let mut pending: Vec<_> = requests
        .into_iter()
        .map(|request| -> Result<_> {
            ensure!(request.confirmed, "Grok deletion requires confirmation");
            crate::session_delete::valid_id(&request.id)?;
            let workspaces = workspaces.as_ref().map_err(|e| anyhow::anyhow!("{e:#}"))?;
            let located = locate_in_workspaces(workspaces, &request.id)?;
            for path in &located {
                ensure!(
                    crate::session_delete::within(&sessions_root, path),
                    "Grok 会话路径越界，拒绝删除"
                );
            }
            let expected_fp = request.fingerprint.trim();
            if !expected_fp.is_empty() {
                let snapshot = activity::Snapshot::capture(paths);
                for path in &located {
                    let summary = read_summary(path)
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| fallback_summary(path));
                    ensure!(
                        snapshot.ownership(&summary) != activity::Ownership::Open,
                        "Grok 会话正在运行，请先退出后再删除"
                    );
                }
                ensure!(!located.is_empty(), "Grok 会话目录已不存在，请重新预览");
                ensure!(
                    grok_fingerprint(&located)? == expected_fp,
                    "Grok 会话已变化，请重新预览"
                );
            }
            let mut outcome = crate::session_delete::Outcome::default();
            for (idx, path) in located.iter().enumerate() {
                if !outcome.remove_strict(path) {
                    outcome.residuals.extend(
                        located[idx + 1..]
                            .iter()
                            .filter(|p| fs::symlink_metadata(p).is_ok())
                            .map(|p| p.display().to_string()),
                    );
                    break;
                }
            }
            Ok((request.id, outcome))
        })
        .collect();
    let ids: std::collections::BTreeSet<_> = pending
        .iter()
        .filter_map(|item| item.as_ref().ok())
        .filter(|(_, outcome)| outcome.residuals.is_empty())
        .map(|(id, _)| id.clone())
        .collect();
    if !ids.is_empty() {
        let index = paths.home.join("active_sessions.json");
        let result = crate::session_delete::edit_json(&index, |value| match value {
            Value::Array(rows) => rows.retain(|row| !crate::session_delete::has_id(row, &ids)),
            Value::Object(rows) => rows
                .retain(|key, row| !ids.contains(key) && !crate::session_delete::has_id(row, &ids)),
            _ => {}
        });
        if let Err(error) = result {
            for (id, outcome) in pending.iter_mut().filter_map(|item| item.as_mut().ok()) {
                if ids.contains(id) {
                    outcome
                        .residuals
                        .push(format!("{}: {error}", index.display()));
                }
            }
        }
    }
    pending
        .into_iter()
        .map(|item| {
            item.map(|(id, mut outcome)| {
                outcome.finish();
                GrokDeleteResult {
                    id,
                    deleted: outcome.residuals.is_empty(),
                    bytes: outcome.bytes,
                    residuals: outcome.residuals,
                }
            })
        })
        .collect()
}

pub async fn rename_grok_session(
    paths: &GrokPaths,
    id: &str,
    title: &str,
) -> Result<GrokSessionSummary> {
    let session = resolve_session(paths, id)?;
    let title = title.trim();
    ensure!(
        !title.is_empty() && title.chars().count() <= 200 && !title.chars().any(char::is_control),
        "Grok title must contain 1-200 characters without control characters"
    );
    ensure_grok_storage_writable(&session.path, "persist the native session title")?;
    let executable = env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .map(|path| path.join("grok"))
        .chain(
            dirs::home_dir()
                .into_iter()
                .flat_map(|home| [home.join(".local/bin/grok"), home.join(".grok/bin/grok")]),
        )
        .find(|path| path.is_file())
        .context("Grok executable unavailable")?;
    rename_native(&executable, paths, &session, title, Duration::from_secs(10)).await?;
    let updated = resolve_session(paths, id)?;
    ensure!(
        updated.title == title,
        "Grok rename returned without persisting the requested title"
    );
    Ok(updated)
}

fn ensure_grok_storage_writable(directory: &Path, operation: &str) -> Result<()> {
    let marker = directory.join(format!(".agenthub-write-check-{}", uuid::Uuid::new_v4()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .with_context(|| grok_storage_write_error(directory, operation))?;
    drop(file);
    fs::remove_file(&marker).with_context(|| {
        format!(
            "{}; temporary marker remains at {}",
            grok_storage_write_error(directory, operation),
            marker.display()
        )
    })?;
    Ok(())
}

fn grok_storage_write_error(path: &Path, operation: &str) -> String {
    format!(
        "cannot {operation} at {}. Grok session storage is not writable from AgentHub; if systemd reports a read-only filesystem or permission denial, add this exact session root to ReadWritePaths, run systemctl daemon-reload, and restart agenthub-webd",
        path.display()
    )
}

async fn rename_native(
    executable: &Path,
    paths: &GrokPaths,
    session: &GrokSessionSummary,
    title: &str,
    timeout: Duration,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let mut child = tokio::process::Command::new(executable)
        .args(["agent", "stdio"])
        .env("GROK_HOME", &paths.home)
        .current_dir(&session.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("start Grok agent stdio")?;
    let mut input = child.stdin.take().context("Grok stdin unavailable")?;
    let output = child.stdout.take().context("Grok stdout unavailable")?;
    let mut output = tokio::io::BufReader::new(output.take(1024 * 1024));
    let exchange = async {
        for (id, method, params) in [
            (
                1,
                "initialize",
                serde_json::json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"agenthub","version":env!("CARGO_PKG_VERSION")}}),
            ),
            (
                2,
                "_x.ai/session/rename",
                serde_json::json!({"sessionId":session.id,"cwd":session.cwd,"title":title}),
            ),
        ] {
            let mut request = serde_json::to_vec(
                &serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            )?;
            request.push(b'\n');
            input.write_all(&request).await?;
            input.flush().await?;
            loop {
                let mut line = String::new();
                ensure!(
                    output.read_line(&mut line).await? > 0,
                    "Grok protocol ended before response"
                );
                let value: Value = serde_json::from_str(&line).context("invalid Grok response")?;
                if value.get("id").and_then(Value::as_u64) != Some(id) {
                    continue;
                }
                if value.get("error").is_some() {
                    bail!("Grok rejected {method}; no local fallback was applied");
                }
                ensure!(value.get("result").is_some(), "Grok response has no result");
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    let result = tokio::time::timeout(timeout, exchange)
        .await
        .context("Grok rename timed out; result unknown, refresh before retrying");
    drop(input);
    let _ = child.kill().await;
    let _ = child.wait().await;
    result?
}
