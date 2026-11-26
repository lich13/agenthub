mod message;

use super::*;
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read},
};

const MAX_BYTES: u64 = crate::resource_limits::CLAUDE_FILE_BYTES;

#[cfg(test)]
thread_local! { pub(super) static BODY_RECORDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

pub(super) type Stamp = crate::file_stamp::FileStamp;
pub(super) fn stamp(path: &Path) -> Result<Stamp> {
    Stamp::read_regular_nofollow(path, "Claude 会话必须是普通文件")
}
#[derive(Clone, Default)]
pub(super) enum ReadContext {
    #[default]
    Main,
    Subagent {
        parent_id: String,
        agent_id: String,
        prefix: String,
    },
}
impl ReadContext {
    fn accepts_sidechain(&self) -> bool {
        matches!(self, Self::Subagent { .. })
    }
    fn validate(&self, record: &Value) -> Result<()> {
        if let Self::Subagent {
            parent_id,
            agent_id,
            ..
        } = self
        {
            let dialogue = matches!(field(record, "type"), "user" | "assistant");
            ensure!(
                !dialogue || record.get("isSidechain").and_then(Value::as_bool) == Some(true),
                "子智能体记录分支不匹配"
            );
            for (key, expected) in [("sessionId", parent_id), ("agentId", agent_id)] {
                ensure!(
                    (!dialogue && record.get(key).is_none()) || field(record, key) == expected,
                    "子智能体记录身份不匹配"
                );
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct ReaderState {
    parser: Parser,
    snapshot: Arc<Parsed>,
    issues: Vec<String>,
    main: Vec<String>,
    positions: HashMap<String, usize>,
    records: usize,
    retained_records: usize,
    rebuild: bool,
    journal: crate::jsonl_cache::HistoryJournal,
    weight: u64,
}
impl ReaderState {
    fn push(&mut self, line: &[u8], context: &ReadContext) -> Result<()> {
        #[cfg(test)]
        BODY_RECORDS.with(|count| count.set(count.get() + 1));
        ensure!(
            line.len() as u64 <= crate::resource_limits::CLAUDE_LINE_BYTES,
            "Claude 会话记录超出读取上限"
        );
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        let value = match serde_json::from_slice::<Value>(line) {
            Ok(value) if value.is_object() => value,
            _ => {
                if self.issues.is_empty() {
                    self.issues.push("会话含损坏记录，改名已禁用".into());
                }
                return Ok(());
            }
        };
        let kind = field(&value, "type");
        let discarded = is_discarded_telemetry_kind(kind);
        if !discarded {
            ensure!(
                self.retained_records < 200_000,
                "Claude 会话记录超出读取上限"
            );
        }
        context.validate(&value)?;
        if self.records == 0 {
            self.parser.context = context.clone();
        }
        let uuid = field(&value, "uuid");
        if (context.accepts_sidechain()
            || value.get("isSidechain").and_then(Value::as_bool) != Some(true))
            && matches!(kind, "user" | "assistant")
            && !uuid.is_empty()
            && !self.positions.contains_key(uuid)
        {
            if let Some(parent) = self.positions.get(field(&value, "parentUuid")).copied() {
                if parent + 1 < self.main.len() {
                    self.main.truncate(parent + 1);
                    self.positions.retain(|_, index| *index <= parent);
                    self.rebuild = true;
                }
            }
            self.positions.insert(uuid.into(), self.main.len());
            self.main.push(uuid.into());
        }
        if !self.rebuild {
            self.parser.push(&value, self.records, None);
        }
        self.records += 1;
        if !discarded {
            self.retained_records += 1;
        }
        Ok(())
    }
    fn rebuild(&mut self, path: &Path, offset: u64) -> Result<()> {
        let allowed: HashSet<_> = self.main.iter().cloned().collect();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut input = BufReader::new(options.open(path)?.take(offset));
        let mut line = Vec::new();
        let mut position = 0;
        self.parser = Parser {
            context: self.parser.context.clone(),
            summary_only: self.parser.summary_only,
            retain_probe_events: self.parser.retain_probe_events,
            ..Default::default()
        };
        while input.read_until(b'\n', &mut line)? != 0 {
            if let Ok(value) = serde_json::from_slice::<Value>(&line) {
                if value.is_object() {
                    self.parser.context.validate(&value)?;
                    self.parser.push(&value, position, Some(&allowed));
                    position += 1;
                }
            }
            line.clear();
        }
        self.rebuild = false;
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct ClaudeProbeSnapshot {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) cwd: String,
    pub(super) cwds: Vec<String>,
    pub(super) version: String,
    pub(super) updated_at: Option<String>,
    pub(super) message_count: usize,
    pub(super) last_message: Option<String>,
    pub(super) issues: Vec<String>,
    pub(super) warnings: Vec<String>,
    pub(super) last_turn_at: Option<i64>,
    pub(super) last_terminal_at: Option<i64>,
    pub(super) turn_open: bool,
    pub(super) has_open_questions: bool,
    pub(super) saw_sidechain: bool,
    pub(super) saw_primary: bool,
    pub(super) native_events: Vec<crate::native_probe::NativeTurnEvent>,
    pub(super) prompts: HashMap<String, ClaudePrompt>,
    pub(super) current_turn: Option<String>,
    pub(super) record_count: u64,
    pub(super) settled_count: u64,
}

impl ClaudeProbeSnapshot {
    fn from_parsed(parsed: &Parsed) -> Self {
        Self {
            id: parsed.id.clone(),
            title: parsed.title.clone(),
            cwd: parsed.cwd.clone(),
            cwds: parsed.cwds.clone(),
            version: parsed.version.clone(),
            updated_at: parsed.updated_at.clone(),
            message_count: parsed.message_count,
            last_message: parsed.last_message.clone(),
            issues: parsed.issues.clone(),
            warnings: parsed.warnings.clone(),
            last_turn_at: parsed.last_turn_at,
            last_terminal_at: parsed.last_terminal_at,
            turn_open: parsed.turn_open,
            has_open_questions: parsed.has_open_questions,
            saw_sidechain: parsed.saw_sidechain,
            saw_primary: parsed.saw_primary,
            native_events: parsed.native_events.clone(),
            prompts: parsed.prompts.clone(),
            current_turn: parsed.current_turn.clone(),
            record_count: parsed.record_count,
            settled_count: parsed.settled_count,
        }
    }

    pub(super) fn to_summary_parsed(&self) -> Parsed {
        Parsed {
            id: self.id.clone(),
            title: self.title.clone(),
            cwd: self.cwd.clone(),
            cwds: self.cwds.clone(),
            version: self.version.clone(),
            updated_at: self.updated_at.clone(),
            message_count: self.message_count,
            last_message: self.last_message.clone(),
            issues: self.issues.clone(),
            warnings: self.warnings.clone(),
            last_turn_at: self.last_turn_at,
            last_terminal_at: self.last_terminal_at,
            turn_open: self.turn_open,
            has_open_questions: self.has_open_questions,
            saw_sidechain: self.saw_sidechain,
            saw_primary: self.saw_primary,
            ..Default::default()
        }
    }
}

struct ProbeReaderState {
    inner: ReaderState,
    snapshot: Option<Arc<ClaudeProbeSnapshot>>,
}

impl Default for ProbeReaderState {
    fn default() -> Self {
        Self {
            inner: ReaderState {
                parser: Parser {
                    summary_only: true,
                    retain_probe_events: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            snapshot: None,
        }
    }
}

static DETAIL_CACHE: crate::jsonl_cache::JsonlCache<ReaderState> =
    crate::jsonl_cache::JsonlCache::new(crate::resource_limits::CLAUDE_DETAIL_CACHE_BYTES)
        .with_line_limit(crate::resource_limits::CLAUDE_LINE_BYTES);
static PROBE_DETAIL_CACHE: crate::jsonl_cache::JsonlCache<ProbeReaderState> =
    crate::jsonl_cache::JsonlCache::new(crate::resource_limits::NATIVE_PROBE_CACHE_BYTES)
        .with_line_limit(crate::resource_limits::CLAUDE_LINE_BYTES);
static PROBE_CACHE: crate::read_cache::ReadCache<(
    bool,
    crate::native_probe::NativeStreamSnapshot,
)> = crate::read_cache::ReadCache::new(crate::resource_limits::NATIVE_PROBE_CACHE_BYTES)
    .with_entry_limit(crate::resource_limits::CLAUDE_SUMMARY_ENTRIES)
    .with_native_metadata();

static SUMMARY_CACHE: crate::read_cache::ReadCache<Parsed> =
    crate::read_cache::ReadCache::new(crate::resource_limits::CLAUDE_SUMMARY_CACHE_BYTES)
        .with_entry_limit(usize::MAX)
        .with_native_metadata();

pub(crate) fn evict_idle_detail_cache(max_idle: std::time::Duration) {
    DETAIL_CACHE.evict_idle(max_idle);
    PROBE_DETAIL_CACHE.evict_idle(max_idle);
    PROBE_CACHE.evict_idle(max_idle);
    SUMMARY_CACHE.evict_idle(max_idle);
    super::activity::evict_idle_cache(max_idle);
    super::evict_idle_located_sessions(max_idle);
}

pub(super) fn read_probe_snapshot(path: &Path) -> Result<Arc<ClaudeProbeSnapshot>> {
    if DETAIL_CACHE.has_entry(path, "") {
        let parsed = read(path)?;
        return Ok(Arc::new(ClaudeProbeSnapshot::from_parsed(&parsed)));
    }
    PROBE_DETAIL_CACHE.read(
        path,
        |size| {
            ensure!(size <= MAX_BYTES, "Claude 会话超出 512 MiB 读取上限");
            Ok(0)
        },
        false,
        |state, line, _, _| state.inner.push(line, &ReadContext::Main),
        |state, info| {
            if info.changed || state.snapshot.is_none() {
                if state.inner.rebuild {
                    state.inner.rebuild(path, info.offset)?;
                }
                let mut issues = state.inner.issues.clone();
                if info.partial {
                    issues.push("会话尾行尚未写完，请稍后刷新".into());
                }
                let parsed = state
                    .inner
                    .parser
                    .snapshot(path, issues, state.inner.records as u64);
                let snap = ClaudeProbeSnapshot::from_parsed(&parsed);
                state.inner.weight = 512
                    + (snap.id.capacity()
                        + snap.title.capacity()
                        + snap.cwd.capacity()
                        + snap.cwds.iter().map(String::capacity).sum::<usize>()
                        + snap.issues.iter().map(String::capacity).sum::<usize>()
                        + snap
                            .native_events
                            .iter()
                            .map(|e| 160 + e.turn_id.capacity() + e.body.capacity())
                            .sum::<usize>()
                        + snap
                            .prompts
                            .keys()
                            .map(|k| 96 + k.capacity())
                            .sum::<usize>()) as u64
                    + state.inner.retained_records as u64 * 96;
                state.snapshot = Some(Arc::new(snap));
            }
            Ok(state.snapshot.as_ref().expect("probe snapshot").clone())
        },
        |state| state.inner.weight,
    )
}

fn probe_snapshot_entry(
    path: &Path,
    projects_root: &Path,
) -> Result<Arc<(bool, crate::native_probe::NativeStreamSnapshot)>> {
    PROBE_CACHE.read(
        path,
        |(_, snapshot), _| {
            512 + (snapshot.id.len()
                + snapshot.session_key.len()
                + snapshot.title.len()
                + snapshot.identity.len()
                + snapshot
                    .events
                    .iter()
                    .map(|e| 256 + e.turn_id.len() + e.body.len())
                    .sum::<usize>()) as u64
        },
        || {
            let parsed = read_probe_snapshot(path)?;
            let sidechain_only = parsed.saw_sidechain && !parsed.saw_primary;
            if sidechain_only {
                return Ok((
                    true,
                    crate::native_probe::NativeStreamSnapshot {
                        provider: crate::native_probe::NativeProvider::Claude,
                        session_key: String::new(),
                        id: String::new(),
                        title: String::new(),
                        identity: String::new(),
                        record_count: 0,
                        settled_count: 0,
                        events: Arc::default(),
                    },
                ));
            }
            ensure!(
                parsed.issues.is_empty(),
                "Claude notification transcript contains corrupt records"
            );
            Ok((
                false,
                crate::native_probe::NativeStreamSnapshot {
                    provider: crate::native_probe::NativeProvider::Claude,
                    session_key: super::key_for(projects_root, path, &parsed.id),
                    id: parsed.id.clone(),
                    title: crate::native_probe::notification_title(
                        &parsed.title,
                        &parsed.id,
                        &parsed.cwd,
                    ),
                    identity: crate::native_probe::file_identity(path)?,
                    record_count: parsed.record_count,
                    settled_count: parsed.settled_count,
                    events: parsed.native_events.clone().into(),
                },
            ))
        },
    )
}

pub(super) fn probe_snapshot_if_primary(
    path: &Path,
    projects_root: &Path,
) -> Result<Option<crate::native_probe::NativeStreamSnapshot>> {
    let entry = probe_snapshot_entry(path, projects_root)?;
    if entry.0 {
        Ok(None)
    } else {
        Ok(Some(entry.1.clone()))
    }
}

pub(super) fn probe_snapshot(
    path: &Path,
    projects_root: &Path,
) -> Result<crate::native_probe::NativeStreamSnapshot> {
    let entry = probe_snapshot_entry(path, projects_root)?;
    if entry.0 {
        let parsed = read_probe_snapshot(path)?;
        ensure!(
            parsed.issues.is_empty(),
            "Claude notification transcript contains corrupt records"
        );
        return Ok(crate::native_probe::NativeStreamSnapshot {
            provider: crate::native_probe::NativeProvider::Claude,
            session_key: super::key_for(projects_root, path, &parsed.id),
            id: parsed.id.clone(),
            title: crate::native_probe::notification_title(&parsed.title, &parsed.id, &parsed.cwd),
            identity: crate::native_probe::file_identity(path)?,
            record_count: parsed.record_count,
            settled_count: parsed.settled_count,
            events: parsed.native_events.clone().into(),
        });
    }
    Ok(entry.1.clone())
}

pub(super) fn read(path: &Path) -> Result<Arc<Parsed>> {
    read_context(path, &ReadContext::Main)
}

pub(super) fn read_context(path: &Path, context: &ReadContext) -> Result<Arc<Parsed>> {
    let scope = match context {
        ReadContext::Main => String::new(),
        ReadContext::Subagent { prefix, .. } => format!("subagent:{prefix}"),
    };
    DETAIL_CACHE.read_scoped(
        (path, &scope),
        |size| {
            ensure!(size <= MAX_BYTES, "Claude 会话超出 512 MiB 读取上限");
            Ok(0)
        },
        false,
        |state, line, _, _| state.push(line, context),
        |state, info| {
            if info.changed {
                let old_snapshot = std::mem::take(&mut state.snapshot);
                let reset = state.rebuild;
                if state.rebuild {
                    state.rebuild(path, info.offset)?;
                }
                let mut issues = state.issues.clone();
                if info.partial {
                    issues.push("会话尾行尚未写完，请稍后刷新".into());
                }
                let mut parsed = state.parser.snapshot(path, issues, state.records as u64);
                let previous: HashMap<_, _> = old_snapshot
                    .events
                    .iter()
                    .map(|event| (&event.id, event))
                    .collect();
                let changed = parsed
                    .events
                    .iter()
                    .filter(|event| {
                        previous
                            .get(&event.id)
                            .is_some_and(|old| !Arc::ptr_eq(old, event) && **old != **event)
                    })
                    .map(|event| event.id.clone())
                    .collect();
                state.journal.record_changes(&info, changed, reset);
                parsed.history = state.journal.clone();
                let previous_timeline = match context {
                    ReadContext::Main => {
                        std::borrow::Cow::Borrowed(old_snapshot.timeline.as_slice())
                    }
                    ReadContext::Subagent { prefix, .. } => std::borrow::Cow::Owned(
                        old_snapshot
                            .timeline
                            .iter()
                            .cloned()
                            .map(|mut entry| {
                                entry.id = entry
                                    .id
                                    .strip_prefix(prefix)
                                    .expect("scoped child timeline")
                                    .into();
                                entry
                            })
                            .collect::<Vec<_>>(),
                    ),
                };
                parsed.timeline = crate::history_navigation::update_timeline(
                    &previous_timeline,
                    parsed
                        .events
                        .iter()
                        .filter(|event| event.kind == "user_message")
                        .map(|event| {
                            (
                                event.id.as_str(),
                                event.user_message.as_ref(),
                                event.text.as_deref(),
                                previous
                                    .get(&event.id)
                                    .is_some_and(|old| Arc::ptr_eq(old, event)),
                            )
                        }),
                );
                if let ReadContext::Subagent { prefix, .. } = context {
                    // Keep the scoped rail in the same budgeted snapshot; unchanged
                    // child polls only clone its Arc, never the full timeline.
                    parsed.timeline = Arc::new(
                        parsed
                            .timeline
                            .iter()
                            .cloned()
                            .map(|mut entry| {
                                entry.id = format!("{prefix}{}", entry.id);
                                entry
                            })
                            .collect(),
                    );
                }
                state.weight = parsed
                    .events
                    .iter()
                    .map(|event| {
                        (512 + event.id.capacity()
                            + event.kind.capacity()
                            + event.timestamp.as_ref().map_or(0, String::capacity)
                            + event.text.as_ref().map_or(0, String::capacity)
                            + event.turn_id.as_ref().map_or(0, String::capacity)
                            + event.role.as_ref().map_or(0, String::capacity)
                            + event.call_id.as_ref().map_or(0, String::capacity)
                            + event.status.as_ref().map_or(0, String::capacity)
                            + event.detail.as_ref().map_or(0, String::capacity)
                            + event.result.as_ref().map_or(0, String::capacity))
                            as u64
                            + event
                                .user_message
                                .as_ref()
                                .map_or(0, crate::user_message::UserMessage::weight)
                    })
                    .sum::<u64>()
                    + (parsed.events.capacity() * std::mem::size_of::<Arc<ClaudeHistoryEvent>>())
                        as u64
                    + state.retained_records as u64 * 128
                    + parsed
                        .agent_names
                        .iter()
                        .map(|(id, name)| (id.capacity() + name.capacity() + 32) as u64)
                        .sum::<u64>()
                    + parsed
                        .native_events
                        .iter()
                        .map(|e| (160 + e.turn_id.capacity() + e.body.capacity()) as u64)
                        .sum::<u64>()
                    + parsed
                        .prompts
                        .keys()
                        .map(|k| (96 + k.capacity()) as u64)
                        .sum::<u64>()
                    + (parsed.id.capacity()
                        + parsed.title.capacity()
                        + parsed.cwd.capacity()
                        + parsed.cwds.iter().map(String::capacity).sum::<usize>()
                        + parsed.issues.iter().map(String::capacity).sum::<usize>()
                        + parsed.warnings.iter().map(String::capacity).sum::<usize>())
                        as u64
                    + crate::history_navigation::weight(&parsed.timeline);
                state.snapshot = Arc::new(parsed);
            }
            Ok(state.snapshot.clone())
        },
        |state| state.weight,
    )
}

fn read_summary_uncached(path: &Path) -> Result<Parsed> {
    let mut state = ReaderState {
        parser: Parser {
            summary_only: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let (offset, partial) = crate::jsonl_cache::scan_once(
        path,
        MAX_BYTES,
        Some(crate::resource_limits::CLAUDE_LINE_BYTES),
        false,
        |line, _, _| state.push(line, &ReadContext::Main),
    )?;
    if state.rebuild {
        state.rebuild(path, offset)?;
    }
    let mut issues = state.issues;
    if partial {
        issues.push("会话尾行尚未写完，请稍后刷新".into());
    }
    let mut parsed = state.parser.snapshot(path, issues, state.records as u64);
    parsed.events = Arc::default();
    parsed.native_events.clear();
    parsed.prompts.clear();
    parsed.agent_names.clear();
    Ok(parsed)
}

pub(super) fn summary(path: &Path) -> Result<Arc<Parsed>> {
    // A list may contain many more sources than the body cache can retain.
    // Keep only summary fields here, and never insert cold list items into DETAIL_CACHE.
    SUMMARY_CACHE.read(
        path,
        |parsed, _| {
            512 + (parsed.id.capacity()
                + parsed.title.capacity()
                + parsed.cwd.capacity()
                + parsed.cwds.iter().map(String::capacity).sum::<usize>()
                + parsed.version.capacity()
                + parsed.updated_at.as_ref().map_or(0, String::capacity)
                + parsed.last_message.as_ref().map_or(0, String::capacity)
                + parsed.issues.iter().map(String::capacity).sum::<usize>()
                + parsed.warnings.iter().map(String::capacity).sum::<usize>())
                as u64
        },
        || {
            if DETAIL_CACHE.has_entry(path, "") {
                let parsed = read(path)?;
                return Ok(Parsed {
                    id: parsed.id.clone(),
                    title: parsed.title.clone(),
                    cwd: parsed.cwd.clone(),
                    cwds: parsed.cwds.clone(),
                    version: parsed.version.clone(),
                    updated_at: parsed.updated_at.clone(),
                    message_count: parsed.message_count,
                    last_message: parsed.last_message.clone(),
                    issues: parsed.issues.clone(),
                    warnings: parsed.warnings.clone(),
                    last_turn_at: parsed.last_turn_at,
                    last_terminal_at: parsed.last_terminal_at,
                    turn_open: parsed.turn_open,
                    has_open_questions: parsed.has_open_questions,
                    saw_sidechain: parsed.saw_sidechain,
                    saw_primary: parsed.saw_primary,
                    ..Default::default()
                });
            }
            if PROBE_DETAIL_CACHE.has_entry(path, "") {
                let snap = read_probe_snapshot(path)?;
                return Ok(snap.to_summary_parsed());
            }
            read_summary_uncached(path)
        },
    )
}

fn bounded(text: &str) -> String {
    text.chars().take(256 * 1024).collect()
}
fn printable(value: &Value) -> String {
    fn clean(v: &Value) -> Value {
        match v {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .filter(|(k, _)| {
                        !matches!(k.as_str(), "data" | "image_url" | "base64" | "signature")
                    })
                    .map(|(k, v)| (k.clone(), clean(v)))
                    .collect(),
            ),
            Value::Array(rows) => Value::Array(rows.iter().map(clean).collect()),
            Value::String(s) if s.starts_with("data:image/") => Value::String("[图片]".into()),
            _ => v.clone(),
        }
    }
    if let Some(s) = value.as_str() {
        bounded(s)
    } else {
        bounded(&format!("{:#}", clean(value)))
    }
}
fn content_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return bounded(s);
    }
    content
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| match field(p, "type") {
                    "text" => Some(field(p, "text").to_owned()),
                    "tool_reference" => {
                        let name = if field(p, "tool_name").is_empty() {
                            field(p, "name")
                        } else {
                            field(p, "tool_name")
                        };
                        (!name.is_empty()).then(|| format!("工具引用：{name}"))
                    }
                    "tool_search_tool_result" => p
                        .get("content")
                        .map(content_text)
                        .or_else(|| p.get("tool_references").map(content_text))
                        .filter(|text| !text.is_empty()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .map(|s| bounded(&s))
        .unwrap_or_default()
}
fn text_content_only(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return bounded(s);
    }
    content
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter(|p| field(p, "type") == "text")
                .map(|p| field(p, "text"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .map(|s| bounded(&s))
        .unwrap_or_default()
}
pub(super) fn millis(value: &str) -> Option<i64> {
    (value)
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond())
}
fn terminal_stop_reason(value: &str) -> bool {
    matches!(value, "end_turn" | "stop_sequence" | "max_tokens")
}
fn event(
    id: String,
    record: &Value,
    kind: &str,
    text: Option<String>,
    turn: &Option<String>,
) -> ClaudeHistoryEvent {
    ClaudeHistoryEvent {
        id,
        timestamp: record
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned),
        kind: kind.into(),
        text,
        turn_id: turn.clone(),
        role: None,
        call_id: None,
        status: None,
        detail: None,
        result: None,
        user_message: None,
        assistant_phase: None,
    }
}
fn issue(parsed: &mut Parsed, message: &str) {
    if !parsed.issues.iter().any(|i| i == message) {
        parsed.issues.push(message.into());
    }
}
fn warning(parsed: &mut Parsed, message: &str) {
    if !parsed.warnings.iter().any(|i| i == message) {
        parsed.warnings.push(message.into());
    }
}
fn alias_field<'a>(value: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("")
}
fn alias_conflict(record: &Value, keys: &[&str]) -> bool {
    let mut value = None;
    for key in keys {
        let Some(candidate) = record
            .get(*key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if let Some(prior) = value {
            if prior != candidate {
                return true;
            }
        } else {
            value = Some(candidate);
        }
    }
    false
}

fn is_discarded_telemetry_kind(kind: &str) -> bool {
    matches!(
        kind,
        "progress"
            | "last-prompt"
            | "agent-name"
            | "tag"
            | "pr-link"
            | "saved_hook_context"
            | "mode"
            | "permission-mode"
            | "api-request-shape"
            | "api-request-blob"
            | "api-request"
            | "agent-setting"
            | "continued-in"
            | "tools-used"
    )
}

fn compact_file_history_detail(record: &Value) -> String {
    let container = record
        .get("snapshot")
        .or_else(|| record.get("delta"))
        .unwrap_or(record);
    let mut paths: Vec<&str> = Vec::new();
    if let Some(backups) = container
        .get("trackedFileBackups")
        .and_then(Value::as_object)
    {
        paths.extend(backups.keys().map(String::as_str));
    } else if let Some(files) = container.get("files") {
        if let Some(arr) = files.as_array() {
            for item in arr {
                if let Some(s) = item
                    .as_str()
                    .or_else(|| item.get("path").and_then(Value::as_str))
                {
                    if !s.is_empty() {
                        paths.push(s);
                    }
                }
            }
        } else if let Some(obj) = files.as_object() {
            paths.extend(obj.keys().map(String::as_str));
        }
    }
    let msg_id = field(container, "messageId");
    let msg_id = if msg_id.is_empty() {
        field(record, "messageId")
    } else {
        msg_id
    };
    if !paths.is_empty() {
        let total = paths.len();
        let preview: Vec<&str> = paths.into_iter().take(24).collect();
        let mut out = if msg_id.is_empty() {
            format!(
                "文件历史快照（共 {total} 个文件）：\n{}",
                preview.join("\n")
            )
        } else {
            format!(
                "文件历史快照（messageId: {msg_id}，共 {total} 个文件）：\n{}",
                preview.join("\n")
            )
        };
        if total > 24 {
            out.push_str(&format!("\n… 其余 {} 个文件已省略", total - 24));
        }
        return out;
    }
    if !msg_id.is_empty() {
        return format!("文件历史快照（messageId: {msg_id}）");
    }
    "文件历史快照".into()
}

#[derive(Default)]
struct Parser {
    context: ReadContext,
    summary_only: bool,
    retain_probe_events: bool,
    saw_any_event: bool,
    summary_message_count: usize,
    summary_updated_at: Option<String>,
    summary_last_message: Option<String>,
    parsed: Parsed,
    custom_title: Option<String>,
    auto_title: Option<String>,
    first_user: Option<String>,
    turn: Option<String>,
    calls: HashMap<String, usize>,
    pending: HashSet<String>,
    questions: HashMap<String, crate::native_probe::NativeTurnEvent>,
    permission_calls: HashMap<String, String>,
    seen: HashMap<String, [u8; 32]>,
    last_assistant_text: Option<String>,
    last_stop_reason: String,
    last_assistant_events: Vec<usize>,
    last_assistant_message_id: Option<String>,
}
struct Sha256JsonWriter<'a>(&'a mut sha2::Sha256);

impl std::io::Write for Sha256JsonWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest;
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn canonical_record_digest(record: &Value) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    let _ = serde_json::to_writer(Sha256JsonWriter(&mut hasher), record);
    hasher.finalize().into()
}

impl Parser {
    fn push_event(&mut self, e: ClaudeHistoryEvent) {
        self.saw_any_event = true;
        if matches!(e.kind.as_str(), "user_message" | "assistant_message") {
            self.summary_message_count += 1;
            self.summary_last_message = e.text.as_ref().map(|t| t.chars().take(240).collect());
            if e.text.as_ref().is_some_and(|t| !t.trim().is_empty())
                || e.user_message
                    .as_ref()
                    .is_some_and(|m| !m.attachments.is_empty())
            {
                self.summary_updated_at = e.timestamp.clone();
            }
        }
        if !self.summary_only {
            Arc::make_mut(&mut self.parsed.events).push(Arc::new(e));
        }
    }

    fn push(&mut self, record: &Value, position: usize, allowed: Option<&HashSet<String>>) {
        let is_sidechain_record = record.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || !field(record, "agentId").is_empty()
            || field(record, "type") == "fork-context-ref";
        if is_sidechain_record {
            self.parsed.saw_sidechain = true;
        } else if matches!(
            field(record, "type"),
            "user" | "assistant" | "system" | "result" | "custom-title" | "ai-title"
        ) {
            self.parsed.saw_primary = true;
        }
        if !self.context.accepts_sidechain()
            && record.get("isSidechain").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        let kind = field(record, "type");
        let uuid = field(record, "uuid");
        if matches!(kind, "user" | "assistant")
            && !uuid.is_empty()
            && allowed.is_some_and(|allowed| !allowed.contains(uuid))
        {
            return;
        }
        if alias_conflict(record, &["sessionId", "session_id"]) {
            warning(
                &mut self.parsed,
                "会话包含不同的身份字段，已按原生会话身份读取",
            );
        }
        if alias_conflict(record, &["version", "claude_code_version"]) {
            warning(
                &mut self.parsed,
                "会话包含不同的版本字段，已按原生记录版本读取",
            );
        }
        let session = alias_field(record, &["sessionId", "session_id"]);
        if !session.is_empty() {
            if self.parsed.id.is_empty() {
                self.parsed.id = session.into();
            } else if self.parsed.id != session {
                issue(&mut self.parsed, "会话身份冲突，改名已禁用");
                return;
            }
        }
        let cwd = field(record, "cwd");
        if !cwd.is_empty() && self.parsed.cwd != cwd {
            self.parsed.cwd = cwd.into();
            if self.parsed.cwds.len() < 32 && !self.parsed.cwds.iter().any(|seen| seen == cwd) {
                self.parsed.cwds.push(cwd.into());
            }
        }
        let version = alias_field(record, &["version", "claude_code_version"]);
        if !version.is_empty() {
            self.parsed.version = version.into();
        }
        if is_discarded_telemetry_kind(kind) {
            return;
        }
        let key = if uuid.is_empty() {
            format!("record-{}", position + 1)
        } else {
            uuid.into()
        };
        if !uuid.is_empty() {
            let hash = canonical_record_digest(record);
            if let Some(prior) = self.seen.insert(uuid.into(), hash) {
                if prior != hash {
                    issue(&mut self.parsed, "记录 ID 冲突，改名已禁用");
                }
                return;
            }
        }
        self.read_kind(record, position, key, kind, uuid);
    }
    fn read_kind(&mut self, record: &Value, position: usize, key: String, kind: &str, uuid: &str) {
        match kind {
            "custom-title" => {
                self.custom_title = record
                    .get("customTitle")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(bounded)
            }
            "ai-title" => {
                self.auto_title = record
                    .get("aiTitle")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(bounded)
            }
            "user" | "assistant" => {
                self.read_message(record, position, key, kind, uuid);
            }
            "result" => {
                self.read_result(record, position);
            }
            "system" => {
                self.read_system(record, position, key);
            }
            "permission_request" => {
                self.read_permission_request(record, position, key);
            }
            "permission_response" => {
                if let Some(call) = self.permission_calls.remove(field(record, "request_id")) {
                    self.questions.remove(&call);
                    self.settle_permission(&call);
                }
                if self.summary_only {
                    self.saw_any_event = true;
                } else {
                    let mut e = event(
                        key,
                        record,
                        "permission_response",
                        Some("权限回复".into()),
                        &self.turn,
                    );
                    e.detail = Some(printable(record));
                    self.push_event(e);
                }
            }
            "summary" => {
                self.last_assistant_events.clear();
                if self.summary_only {
                    self.saw_any_event = true;
                } else {
                    self.push_event(event(
                        key,
                        record,
                        "compaction",
                        Some(bounded(field(record, "summary"))),
                        &self.turn,
                    ));
                }
            }
            "tool_reference" | "tool_search_tool_result" => {
                self.read_tool_reference(record, key, kind);
            }
            "file-history-snapshot"
            | "file-history-delta"
            | "queue-operation"
            | "attachment"
            | "atis-latch"
            | "cost-state" => {
                self.read_auxiliary(record, key, kind);
            }
            "progress" | "last-prompt" | "agent-name" | "tag" | "pr-link"
            | "saved_hook_context" | "mode" | "permission-mode" | "api-request-shape"
            | "api-request-blob" | "api-request" | "agent-setting" | "continued-in"
            | "tools-used" => {}
            _ => {
                if self.summary_only {
                    self.saw_any_event = true;
                } else {
                    let mut e = event(
                        key,
                        record,
                        "unknown",
                        Some(format!("未识别的活动：{kind}")),
                        &self.turn,
                    );
                    e.detail = Some(printable(record));
                    self.push_event(e);
                }
                warning(&mut self.parsed, "存在新格式活动，仅显示兼容提示");
            }
        }
    }
    /// A human prompt or a native wake-up record starts a new notification
    /// turn. Both register their own native prompt identity, while only the
    /// human form becomes a message, title or timeline marker.
    fn begin_turn(&mut self, key: &str, record: &Value, position: usize, prompt: &str) {
        if !prompt.is_empty() {
            let entry = self
                .parsed
                .prompts
                .entry(prompt.into())
                .or_insert_with(|| ClaudePrompt {
                    turn_id: key.to_owned(),
                    timestamp_ms: millis(field(record, "timestamp")).unwrap_or(0),
                    ambiguous: false,
                    cancelled: false,
                    last_activity_position: position as u64 + 1,
                    last_activity_ms: millis(field(record, "timestamp")),
                    last_api_error_position: None,
                });
            entry.ambiguous |= entry.turn_id != key;
        }
        self.turn = Some(key.to_owned());
        self.parsed
            .native_events
            .retain(|event| event.kind != "failure");
        self.parsed.turn_open = true;
        self.parsed.last_turn_at = millis(field(record, "timestamp"));
        self.pending.clear();
        self.questions.clear();
        self.permission_calls.clear();
        self.calls.clear();
        self.last_assistant_text = None;
        self.last_stop_reason.clear();
        self.last_assistant_events.clear();
    }
    fn read_result(&mut self, record: &Value, position: usize) {
        let subtype = field(record, "subtype");
        let is_failure = record.get("is_error").and_then(Value::as_bool) == Some(true)
            && matches!(
                subtype,
                "error_during_execution"
                    | "error_max_turns"
                    | "error_max_budget_usd"
                    | "error_max_structured_output_retries"
            );
        if is_failure || subtype == "success" {
            self.execution_activity(record, position, is_failure);
        }
        let stop_reason = record.get("stop_reason").and_then(Value::as_str);
        let explicit_terminal = stop_reason.is_some_and(terminal_stop_reason);
        let inferred_terminal = stop_reason.is_none()
            && (self.last_stop_reason.is_empty() || terminal_stop_reason(&self.last_stop_reason))
            && record.get("is_error").and_then(Value::as_bool) == Some(false);
        if subtype == "success" && stop_reason.is_some_and(|reason| !terminal_stop_reason(reason)) {
            self.clear_final_answer();
            self.parsed.native_events.retain(|event| {
                event.kind != "completion" || Some(&event.turn_id) != self.turn.as_ref()
            });
            self.parsed.turn_open = self.turn.is_some();
        }
        if subtype == "success"
            && record.get("is_error").and_then(Value::as_bool) != Some(true)
            && self.pending.is_empty()
            && self.questions.is_empty()
            && (explicit_terminal || inferred_terminal)
        {
            let result = bounded(field(record, "result"));
            if result.trim().is_empty()
                || self.last_assistant_text.as_deref().map(bounded).as_deref()
                    == Some(result.as_str())
            {
                self.mark_final_answer();
            }
            finish(
                &mut self.parsed,
                &self.turn,
                position,
                record,
                "completion",
                if result.trim().is_empty() {
                    self.last_assistant_text.clone().unwrap_or_default()
                } else {
                    result
                },
            );
        } else if is_failure {
            self.questions.clear();
            if !self.summary_only {
                let events = Arc::make_mut(&mut self.parsed.events);
                for call in &self.pending {
                    if let Some(&index) = self.calls.get(call) {
                        Arc::make_mut(&mut events[index]).status = Some("cancelled".into());
                    }
                }
            }
            self.pending.clear();
            if self.summary_only {
                self.saw_any_event = true;
            } else {
                let body = record
                    .get("errors")
                    .map(printable)
                    .unwrap_or_else(|| subtype.into());
                let mut failure = event(
                    format!("record-{}:failure", position + 1),
                    record,
                    "failure",
                    Some(bounded(&body)),
                    &self.turn,
                );
                failure.status = Some("failed".into());
                self.push_event(failure);
            }
            finish(
                &mut self.parsed,
                &self.turn,
                position,
                record,
                "failure",
                if self.summary_only && !self.retain_probe_events {
                    String::new()
                } else {
                    record
                        .get("errors")
                        .map(printable)
                        .unwrap_or_else(|| subtype.into())
                },
            );
        }
    }
    fn read_system(&mut self, record: &Value, position: usize, key: String) {
        let subtype = field(record, "subtype");
        if subtype == "turn_duration" {
            let keep_open = native_pending_background(record) || !self.questions.is_empty();
            if keep_open {
                // The turn ended while native agents, workflows, or user questions
                // are still pending; this terminal turn is not the thread's end.
                if let Some(turn) = self.turn.clone() {
                    self.parsed
                        .native_events
                        .retain(|event| !(event.kind == "completion" && event.turn_id == turn));
                }
                self.parsed.turn_open = true;
            } else {
                self.parsed.turn_open = false;
                if let Some(ts) = millis(field(record, "timestamp")) {
                    self.parsed.last_terminal_at =
                        Some(self.parsed.last_terminal_at.map_or(ts, |prev| prev.max(ts)));
                }
            }
        }
        if subtype == "api_error" {
            self.execution_activity(record, position, true);
        }
        if matches!(subtype, "interrupted" | "cancelled" | "canceled") {
            self.clear_final_answer();
            for prompt in self.parsed.prompts.values_mut() {
                if Some(&prompt.turn_id) == self.turn.as_ref() {
                    prompt.cancelled = true;
                }
            }
            self.parsed.turn_open = false;
            if let Some(ts) = millis(field(record, "timestamp")) {
                self.parsed.last_terminal_at =
                    Some(self.parsed.last_terminal_at.map_or(ts, |prev| prev.max(ts)));
            }
            self.parsed
                .native_events
                .retain(|event| Some(&event.turn_id) != self.turn.as_ref());
            self.turn = None;
            self.last_stop_reason.clear();
            self.last_assistant_events.clear();
            self.last_assistant_text = None;
            self.questions.clear();
            if !self.summary_only {
                let events = Arc::make_mut(&mut self.parsed.events);
                for call in &self.pending {
                    if let Some(&index) = self.calls.get(call) {
                        Arc::make_mut(&mut events[index]).status = Some("cancelled".into());
                    }
                }
            }
            self.pending.clear();
        }
        if subtype == "init" {
            // Identity/cwd/version were extracted above. Startup
            // configuration is metadata, not a conversation activity.
            return;
        }
        if subtype == "compact_boundary" {
            self.last_assistant_events.clear();
            if self.summary_only {
                self.saw_any_event = true;
            } else {
                self.push_event(event(
                    key,
                    record,
                    "compaction",
                    Some(content_text(record.get("content").unwrap_or(&Value::Null))),
                    &self.turn,
                ));
            }
        } else {
            let known = matches!(
                subtype,
                "init"
                    | "turn_duration"
                    | "away_summary"
                    | "stop_hook_summary"
                    | "interrupted"
                    | "cancelled"
                    | "canceled"
                    | "api_error"
                    | "local_command"
            );
            if self.summary_only {
                self.saw_any_event = true;
            } else {
                let mut e = event(
                    key,
                    record,
                    if known { "system" } else { "unknown" },
                    Some(format!("系统活动：{subtype}")),
                    &self.turn,
                );
                e.detail = Some(printable(record));
                e.role = Some(subtype.into());
                if subtype == "api_error" {
                    e.status = Some("failed".into());
                }
                self.push_event(e);
            }
            if !known {
                warning(&mut self.parsed, "存在新格式系统活动，仅显示兼容提示");
            }
        }
    }
    fn read_permission_request(&mut self, record: &Value, position: usize, key: String) {
        let call = field(record, "tool_use_id");
        let request = field(record, "request_id");
        if self.summary_only {
            self.saw_any_event = true;
        } else {
            let mut e = event(
                key,
                record,
                "permission_request",
                Some("权限请求".into()),
                &self.turn,
            );
            e.detail = Some(printable(record));
            e.call_id = (!call.is_empty()).then(|| call.into());
            e.status = (!request.is_empty()
                && self.pending.contains(call)
                && field(record, "agent_id").is_empty()
                && alias_field(record, &["sessionId", "session_id"]) == self.parsed.id)
                .then(|| "in_progress".into());
            self.push_event(e);
        }
        // Only a pending primary call with explicit transcript identity
        // proves this is our permission request, not a remote subagent's.
        if !request.is_empty()
            && self.pending.contains(call)
            && field(record, "agent_id").is_empty()
            && alias_field(record, &["sessionId", "session_id"]) == self.parsed.id
        {
            if let (Some(turn), Some(ts)) = (&self.turn, millis(field(record, "timestamp"))) {
                let body = format!(
                    "{}\n{}",
                    field(record, "description"),
                    field(record, "tool_name")
                );
                if !body.trim().is_empty() {
                    self.permission_calls.insert(request.into(), call.into());
                    self.questions.entry(call.into()).or_insert(
                        crate::native_probe::NativeTurnEvent {
                            prompt_id: prompt_for_turn(&self.parsed, turn),
                            position: position as u64 + 1,
                            turn_id: format!("{turn}:permission:{request}"),
                            kind: "authorization".into(),
                            body: crate::security::redact_output(&bounded(&body)),
                            timestamp_ms: ts,
                        },
                    );
                }
            }
        }
    }
    fn mark_final_answer(&mut self) {
        if self.summary_only || self.last_assistant_events.is_empty() {
            return;
        }
        // Presentation only: never infer the phase from text or change native
        // notification terminal events and their deduplication identities.
        let events = Arc::make_mut(&mut self.parsed.events);
        for &index in &self.last_assistant_events {
            let event = &mut events[index];
            if event.turn_id == self.turn
                && event.assistant_phase != Some(ClaudeAssistantPhase::Intermediate)
            {
                Arc::make_mut(event).assistant_phase = Some(ClaudeAssistantPhase::Final);
            }
        }
    }
    fn clear_final_answer(&mut self) {
        if self.summary_only || self.last_assistant_events.is_empty() {
            return;
        }
        let events = Arc::make_mut(&mut self.parsed.events);
        for &index in &self.last_assistant_events {
            let event = &mut events[index];
            if event.assistant_phase == Some(ClaudeAssistantPhase::Final) {
                Arc::make_mut(event).assistant_phase = None;
            }
        }
    }
    fn settle_permission(&mut self, call: &str) {
        if self.summary_only {
            return;
        }
        let events = Arc::make_mut(&mut self.parsed.events);
        for event in events
            .iter_mut()
            .rev()
            .take_while(|e| e.turn_id == self.turn)
        {
            if event.kind == "permission_request" && event.call_id.as_deref() == Some(call) {
                Arc::make_mut(event).status = Some("completed".into());
            }
        }
    }
    fn read_tool_reference(&mut self, record: &Value, key: String, kind: &str) {
        let name = if field(record, "tool_name").is_empty() {
            field(record, "name")
        } else {
            field(record, "tool_name")
        };
        if self.summary_only {
            self.saw_any_event = true;
        } else {
            let mut e = event(
                key,
                record,
                "tool_reference",
                Some(if name.is_empty() {
                    if kind == "tool_search_tool_result" {
                        "工具引用结果".into()
                    } else {
                        "工具引用".into()
                    }
                } else {
                    name.into()
                }),
                &self.turn,
            );
            e.role = (!name.is_empty()).then(|| name.into());
            e.detail = Some(printable(record));
            self.push_event(e);
        }
        let nested_reference = ["content", "tool_references"].iter().any(|key| {
            record
                .get(*key)
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        field(part, "type") == "tool_reference"
                            && !alias_field(part, &["tool_name", "name"]).is_empty()
                    })
                })
        });
        if name.is_empty() && !nested_reference {
            warning(&mut self.parsed, "工具引用缺少工具名称，已保留为警告");
        }
    }
    fn read_auxiliary(&mut self, record: &Value, key: String, kind: &str) {
        if self.summary_only {
            self.saw_any_event = true;
            return;
        }
        if kind == "attachment"
            && record.pointer("/attachment/type").and_then(Value::as_str)
                == Some("plan_file_reference")
        {
            if let Some(plan) = record
                .pointer("/attachment/planContent")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                self.push_event(event(key, record, "plan", Some(bounded(plan)), &self.turn));
                return;
            }
        }
        let mut e = event(
            key,
            record,
            kind,
            Some(
                match kind {
                    "file-history-snapshot" | "file-history-delta" => "文件历史快照",
                    "queue-operation" => "队列活动",
                    "atis-latch" => "会话设置",
                    "cost-state" => "用量记录",
                    _ => "附件",
                }
                .into(),
            ),
            &self.turn,
        );
        if kind == "attachment" {
            let attachment = &record["attachment"];
            e.role = Some("native_context".into());
            e.text = Some(
                match field(attachment, "type") {
                    "instructions" => "指令",
                    "async_hook_response" => "Hook 输出",
                    "queued_command" => "队列活动",
                    "total_tokens_reminder" | "silent_turn_reminder" | "task_reminder" => {
                        "系统提醒"
                    }
                    _ => "上下文",
                }
                .into(),
            );
            let instructions = field(attachment, "type") == "instructions";
            let body = if instructions {
                attachment["files"]
                    .as_array()
                    .map(|files| {
                        files
                            .iter()
                            .filter_map(|file| file["content"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    })
                    .unwrap_or_default()
            } else {
                printable(attachment)
            };
            let bounded_body = bounded(&body);
            e.detail = Some(if bounded_body.trim().is_empty() {
                printable(attachment)
            } else {
                bounded_body.clone()
            });
            let mut user =
                crate::user_message::parse_user_message(&e.id, &Value::String(String::new()));
            user.context_blocks
                .push(crate::user_message::UserContextBlock {
                    kind: if instructions {
                        crate::user_message::UserContextKind::Instructions
                    } else {
                        crate::user_message::UserContextKind::Runtime
                    },
                    text: bounded_body,
                });
            e.user_message = Some(user);
        } else if matches!(kind, "file-history-snapshot" | "file-history-delta") {
            e.detail = Some(compact_file_history_detail(record));
        } else {
            e.detail = Some(printable(record));
        }
        self.push_event(e);
    }
    fn snapshot(&self, path: &Path, extra_issues: Vec<String>, record_count: u64) -> Parsed {
        let mut issues = self.parsed.issues.clone();
        for message in extra_issues {
            if !issues.iter().any(|i| i == &message) {
                issues.push(message);
            }
        }
        let mut id = self.parsed.id.clone();
        if id.is_empty() {
            id = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
            if self.saw_any_event && !issues.iter().any(|i| i == "未确认原生会话 ID") {
                issues.push("未确认原生会话 ID".into());
            }
        }
        let title = self
            .custom_title
            .clone()
            .or_else(|| self.auto_title.clone())
            .or_else(|| self.first_user.clone())
            .unwrap_or_else(|| id.clone());
        let version = if self.parsed.version.is_empty() {
            "unknown".into()
        } else {
            self.parsed.version.clone()
        };
        if version != "unknown"
            && !version.starts_with("2.")
            && !issues
                .iter()
                .any(|i| i == "此 Claude 版本尚未验证，改名已禁用")
        {
            issues.push("此 Claude 版本尚未验证，改名已禁用".into());
        }
        let keep_probe = !self.summary_only || self.retain_probe_events;
        let native_events = if !keep_probe {
            Vec::new()
        } else {
            let mut evs = self.parsed.native_events.clone();
            evs.extend(self.questions.values().cloned());
            evs.sort_by_key(|e| e.position);
            evs
        };
        Parsed {
            agent_names: if self.summary_only {
                HashMap::new()
            } else {
                self.parsed.agent_names.clone()
            },
            timeline: Arc::default(),
            id,
            title,
            cwd: self.parsed.cwd.clone(),
            cwds: self.parsed.cwds.clone(),
            version,
            updated_at: self.summary_updated_at.clone(),
            events: Arc::clone(&self.parsed.events),
            message_count: self.summary_message_count,
            last_message: self.summary_last_message.clone(),
            history: Default::default(),
            issues,
            warnings: self.parsed.warnings.clone(),
            last_turn_at: self.parsed.last_turn_at,
            last_terminal_at: self.parsed.last_terminal_at,
            turn_open: self.parsed.turn_open,
            has_open_questions: !self.questions.is_empty(),
            saw_sidechain: self.parsed.saw_sidechain,
            saw_primary: self.parsed.saw_primary,
            native_events,
            prompts: if !keep_probe {
                HashMap::new()
            } else {
                self.parsed.prompts.clone()
            },
            current_turn: self.turn.clone(),
            record_count,
            settled_count: record_count,
        }
    }

    fn execution_activity(&mut self, record: &Value, position: usize, api_error: bool) {
        for prompt in self.parsed.prompts.values_mut() {
            if Some(&prompt.turn_id) == self.turn.as_ref() {
                if api_error {
                    prompt.last_api_error_position = Some(position as u64 + 1);
                } else {
                    prompt.last_activity_position = position as u64 + 1;
                    prompt.last_activity_ms = millis(field(record, "timestamp"));
                }
            }
        }
        if api_error {
            self.parsed
                .native_events
                .retain(|event| Some(&event.turn_id) != self.turn.as_ref());
            self.parsed.turn_open = self.turn.is_some();
        }
    }
}

fn native_pending_background(record: &Value) -> bool {
    ["pendingBackgroundAgentCount", "pendingWorkflowCount"]
        .iter()
        .any(|key| {
            record
                .get(*key)
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0)
        })
}

fn finish(
    parsed: &mut Parsed,
    turn: &Option<String>,
    position: usize,
    record: &Value,
    kind: &str,
    body: String,
) {
    parsed.turn_open = false;
    if let Some(ts) = millis(field(record, "timestamp")) {
        parsed.last_terminal_at = Some(parsed.last_terminal_at.map_or(ts, |prev| prev.max(ts)));
    }
    if body.trim().is_empty() {
        return;
    }
    let (Some(turn), Some(ts)) = (turn, millis(field(record, "timestamp"))) else {
        return;
    };
    if parsed
        .native_events
        .iter()
        .any(|e| e.turn_id == *turn && e.kind == kind)
    {
        return;
    }
    parsed.native_events.retain(|e| e.turn_id != *turn);
    parsed
        .native_events
        .push(crate::native_probe::NativeTurnEvent {
            prompt_id: prompt_for_turn(parsed, turn),
            position: position as u64 + 1,
            turn_id: turn.clone(),
            kind: kind.into(),
            body: crate::security::redact_output(&body),
            timestamp_ms: ts,
        });
}
