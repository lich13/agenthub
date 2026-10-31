use super::*;
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read},
    time::SystemTime,
};

const MAX_BYTES: u64 = crate::resource_limits::CLAUDE_FILE_BYTES;

#[cfg(test)]
thread_local! { pub(super) static BODY_RECORDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Stamp {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
pub(super) fn stamp(path: &Path) -> Result<Stamp> {
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_file() && !m.file_type().is_symlink(),
        "Claude 会话必须是普通文件"
    );
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(Stamp {
        size: m.len(),
        modified: m.modified()?,
        #[cfg(unix)]
        identity: (m.dev(), m.ino(), m.ctime(), m.ctime_nsec()),
    })
}
#[derive(Default)]
struct ReaderState {
    parser: Parser,
    snapshot: Arc<Parsed>,
    issues: Vec<String>,
    main: Vec<String>,
    positions: HashMap<String, usize>,
    records: usize,
    rebuild: bool,
    journal: crate::jsonl_cache::HistoryJournal,
    weight: u64,
}
impl ReaderState {
    fn push(&mut self, line: &[u8]) -> Result<()> {
        #[cfg(test)]
        BODY_RECORDS.with(|count| count.set(count.get() + 1));
        ensure!(
            line.len() as u64 <= crate::resource_limits::CLAUDE_LINE_BYTES,
            "Claude 会话记录超出读取上限"
        );
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        ensure!(self.records < 200_000, "Claude 会话记录超出读取上限");
        let value = match serde_json::from_slice::<Value>(line) {
            Ok(value) if value.is_object() => value,
            _ => {
                if self.issues.is_empty() {
                    self.issues.push("会话含损坏记录，改名已禁用".into());
                }
                return Ok(());
            }
        };
        let uuid = field(&value, "uuid");
        if value.get("isSidechain").and_then(Value::as_bool) != Some(true)
            && matches!(field(&value, "type"), "user" | "assistant")
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
        self.parser = Parser::default();
        while input.read_until(b'\n', &mut line)? != 0 {
            if let Ok(value) = serde_json::from_slice::<Value>(&line) {
                if value.is_object() {
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

static DETAIL_CACHE: crate::jsonl_cache::JsonlCache<ReaderState> =
    crate::jsonl_cache::JsonlCache::new(crate::resource_limits::CLAUDE_DETAIL_CACHE_BYTES)
        .with_line_limit(crate::resource_limits::CLAUDE_LINE_BYTES);
static PROBE_CACHE: crate::read_cache::ReadCache<crate::native_probe::NativeStreamSnapshot> =
    crate::read_cache::ReadCache::new(crate::resource_limits::NATIVE_PROBE_CACHE_BYTES)
        .with_entry_limit(crate::resource_limits::CLAUDE_SUMMARY_ENTRIES)
        .with_native_metadata();

pub(crate) fn evict_idle_detail_cache(max_idle: std::time::Duration) {
    DETAIL_CACHE.evict_idle(max_idle);
    PROBE_CACHE.evict_idle(max_idle);
    super::activity::evict_idle_cache(max_idle);
}

pub(super) fn probe_snapshot(
    path: &Path,
    projects_root: &Path,
) -> Result<crate::native_probe::NativeStreamSnapshot> {
    PROBE_CACHE.read(
        path,
        |snapshot, _| {
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
            let meta = fs::symlink_metadata(path)?;
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Claude 会话必须是普通文件"
            );
            ensure!(meta.len() <= MAX_BYTES, "Claude 会话超出 512 MiB 读取上限");
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
            }
            let mut reader = BufReader::new(options.open(path)?);
            let mut state = ReaderState::default();
            let mut line = Vec::new();
            let mut offset = 0u64;
            loop {
                line.clear();
                let read_bytes = reader.read_until(b'\n', &mut line)?;
                if read_bytes == 0 {
                    break;
                }
                if !line.ends_with(b"\n") {
                    anyhow::bail!("Claude 会话尾行尚未写完");
                }
                offset += read_bytes as u64;
                state.push(&line)?;
            }
            if state.rebuild {
                state.rebuild(path, offset)?;
            }
            let parsed = state
                .parser
                .snapshot(path, state.issues, state.records as u64);
            ensure!(
                parsed.issues.is_empty(),
                "Claude notification transcript contains corrupt records"
            );
            Ok(crate::native_probe::NativeStreamSnapshot {
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
                events: parsed.native_events,
            })
        },
    )
}

pub(super) fn read(path: &Path) -> Result<Arc<Parsed>> {
    DETAIL_CACHE.read(
        path,
        |size| {
            ensure!(size <= MAX_BYTES, "Claude 会话超出 512 MiB 读取上限");
            Ok(0)
        },
        false,
        |state, line, _, _| state.push(line),
        |state, info| {
            if info.changed {
                let reset = state.rebuild;
                if state.rebuild {
                    state.rebuild(path, info.offset)?;
                }
                let mut issues = state.issues.clone();
                if info.partial {
                    issues.push("会话尾行尚未写完，请稍后刷新".into());
                }
                let mut parsed = state.parser.snapshot(path, issues, state.records as u64);
                let previous: HashMap<_, _> = state
                    .snapshot
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
                            .is_some_and(|old| !Arc::ptr_eq(old, event) && ***old != ***event)
                    })
                    .map(|event| event.id.clone())
                    .collect();
                state.journal.record_changes(&info, changed, reset);
                parsed.history = state.journal.clone();
                parsed.timeline = crate::history_navigation::update_timeline(
                    &state.snapshot.timeline,
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
                state.weight = parsed
                    .events
                    .iter()
                    .map(|event| {
                        512 + event.text.as_ref().map_or(0, String::len) as u64
                            + event.detail.as_ref().map_or(0, String::len) as u64
                            + event.result.as_ref().map_or(0, String::len) as u64
                            + event
                                .user_message
                                .as_ref()
                                .map_or(0, crate::user_message::UserMessage::weight)
                    })
                    .sum::<u64>()
                    + state.records as u64 * 256
                    + crate::history_navigation::weight(&parsed.timeline);
                state.snapshot = Arc::new(parsed);
            }
            Ok(state.snapshot.clone())
        },
        |state| state.weight,
    )
}

pub(super) fn summary(path: &Path) -> Result<Arc<Parsed>> {
    // A list may contain many more sources than the body cache can retain.
    // Keep only summary fields here, so revisiting an inactive list cannot
    // repeatedly evict and reparse every large transcript.
    static CACHE: crate::read_cache::ReadCache<Arc<Parsed>> =
        crate::read_cache::ReadCache::new(crate::resource_limits::CLAUDE_SUMMARY_CACHE_BYTES)
            .with_entry_limit(crate::resource_limits::CLAUDE_SUMMARY_ENTRIES)
            .with_native_metadata();
    CACHE.read(
        path,
        |parsed, _| {
            512 + (parsed.id.len()
                + parsed.title.len()
                + parsed.cwd.len()
                + parsed.version.len()
                + parsed.last_message.as_ref().map_or(0, String::len)
                + parsed.issues.iter().map(String::len).sum::<usize>()
                + parsed.warnings.iter().map(String::len).sum::<usize>()) as u64
        },
        || {
            let parsed = read(path)?;
            Ok(Arc::new(Parsed {
                id: parsed.id.clone(),
                title: parsed.title.clone(),
                cwd: parsed.cwd.clone(),
                version: parsed.version.clone(),
                updated_at: parsed.updated_at.clone(),
                message_count: parsed.message_count,
                last_message: parsed.last_message.clone(),
                issues: parsed.issues.clone(),
                warnings: parsed.warnings.clone(),
                last_turn_at: parsed.last_turn_at,
                turn_open: parsed.turn_open,
                ..Default::default()
            }))
        },
    )
}

fn bounded(text: &str) -> String {
    crate::security::redact_output(&text.chars().take(256 * 1024).collect::<String>())
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
        bounded(&serde_json::to_string_pretty(&clean(value)).unwrap_or_default())
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
fn millis(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| t.timestamp_millis())
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

#[derive(Default)]
struct Parser {
    parsed: Parsed,
    custom_title: Option<String>,
    auto_title: Option<String>,
    first_user: Option<String>,
    turn: Option<String>,
    calls: HashMap<String, usize>,
    pending: HashSet<String>,
    questions: HashMap<String, crate::native_probe::NativeTurnEvent>,
    permission_calls: HashMap<String, String>,
    seen: HashMap<String, String>,
    last_assistant_text: Option<String>,
    last_stop_reason: String,
    last_assistant_events: Vec<usize>,
    last_assistant_message_id: Option<String>,
}
impl Parser {
    fn push(&mut self, record: &Value, position: usize, allowed: Option<&HashSet<String>>) {
        let Self {
            mut parsed,
            custom_title,
            auto_title,
            first_user,
            turn,
            calls,
            pending,
            questions,
            permission_calls,
            mut seen,
            last_assistant_text,
            last_stop_reason,
            last_assistant_events,
            last_assistant_message_id,
        } = std::mem::take(self);
        for (position, record) in std::iter::once((position, record)) {
            if record.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let kind = field(record, "type");
            let uuid = field(record, "uuid");
            if matches!(kind, "user" | "assistant")
                && !uuid.is_empty()
                && allowed.is_some_and(|allowed| !allowed.contains(uuid))
            {
                continue;
            }
            if alias_conflict(record, &["sessionId", "session_id"]) {
                warning(&mut parsed, "会话包含不同的身份字段，已按原生会话身份读取");
            }
            if alias_conflict(record, &["version", "claude_code_version"]) {
                warning(&mut parsed, "会话包含不同的版本字段，已按原生记录版本读取");
            }
            let session = alias_field(record, &["sessionId", "session_id"]);
            if !session.is_empty() {
                if parsed.id.is_empty() {
                    parsed.id = session.into();
                } else if parsed.id != session {
                    issue(&mut parsed, "会话身份冲突，改名已禁用");
                    continue;
                }
            }
            if !field(record, "cwd").is_empty() {
                parsed.cwd = field(record, "cwd").into();
            }
            let version = alias_field(record, &["version", "claude_code_version"]);
            if !version.is_empty() {
                parsed.version = version.into();
            }
            let key = if uuid.is_empty() {
                format!("record-{}", position + 1)
            } else {
                uuid.into()
            };
            if !uuid.is_empty() {
                let hash = digest(record.to_string());
                if let Some(prior) = seen.insert(uuid.into(), hash.clone()) {
                    if prior != hash {
                        issue(&mut parsed, "记录 ID 冲突，改名已禁用");
                    }
                    continue;
                }
            }
            *self = Self {
                parsed,
                custom_title,
                auto_title,
                first_user,
                turn,
                calls,
                pending,
                questions,
                permission_calls,
                seen,
                last_assistant_text,
                last_stop_reason,
                last_assistant_events,
                last_assistant_message_id,
            };

            self.read_kind(record, position, key, kind, uuid);
            return;
        }

        *self = Self {
            parsed,
            custom_title,
            auto_title,
            first_user,
            turn,
            calls,
            pending,
            questions,
            permission_calls,
            seen,
            last_assistant_text,
            last_stop_reason,
            last_assistant_events,
            last_assistant_message_id,
        };
    }
    fn read_kind(&mut self, record: &Value, position: usize, key: String, kind: &str, uuid: &str) {
        for key in std::iter::once(key) {
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
                    let mut e = event(
                        key,
                        record,
                        "permission_response",
                        Some("权限回复".into()),
                        &self.turn,
                    );
                    e.detail = Some(printable(record));
                    self.parsed.events.push(Arc::new(e));
                }
                "summary" => {
                    self.last_assistant_events.clear();
                    self.parsed.events.push(Arc::new(event(
                        key,
                        record,
                        "compaction",
                        Some(bounded(field(record, "summary"))),
                        &self.turn,
                    )));
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
                | "api-request-blob" | "api-request" | "agent-setting" | "continued-in" => {}
                _ => {
                    let mut e = event(
                        key,
                        record,
                        "unknown",
                        Some(format!("未识别的活动：{kind}")),
                        &self.turn,
                    );
                    e.detail = Some(printable(record));
                    self.parsed.events.push(Arc::new(e));
                    warning(&mut self.parsed, "存在新格式活动，仅显示兼容提示");
                }
            }
        }
    }
    fn read_message(
        &mut self,
        record: &Value,
        position: usize,
        key: String,
        kind: &str,
        uuid: &str,
    ) {
        for key in std::iter::once(key) {
            let Some(message) = record.get("message").filter(|m| m.is_object()) else {
                issue(&mut self.parsed, "消息格式未知，改名已禁用");
                continue;
            };
            let Some(content) = message.get("content") else {
                issue(&mut self.parsed, "消息正文缺失，改名已禁用");
                continue;
            };
            if !content.is_array() && !content.is_string() {
                issue(&mut self.parsed, "消息正文格式未知，改名已禁用");
                let mut e = event(
                    key,
                    record,
                    "unknown",
                    Some("未知消息正文".into()),
                    &self.turn,
                );
                e.detail = Some(printable(content));
                self.parsed.events.push(Arc::new(e));
                continue;
            }
            let parts = content.as_array().cloned().unwrap_or_else(|| {
                vec![serde_json::json!({"type":"text","text":content.as_str().unwrap_or("")})]
            });
            let has_user_content = kind == "user"
                && parts
                    .iter()
                    .any(|p| matches!(field(p, "type"), "text" | "image" | "document"));
            if has_user_content {
                let (user, compact) = super::presentation::user_content(&key, record, &parts);
                let automatic = record["isMeta"] == true
                    || compact
                    || (user.text.trim().is_empty() && user.attachments.is_empty());
                if automatic {
                    let body = user
                        .context_blocks
                        .iter()
                        .map(|block| block.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let (role, title) = if body.trim_start().starts_with("<local-command-") {
                        ("local_command", "命令输出")
                    } else if body.trim_start().starts_with("<task-notification>") {
                        ("task_notification", "任务通知")
                    } else {
                        ("native_context", "上下文")
                    };
                    let mut e = event(
                        key.clone(),
                        record,
                        if compact { "compaction" } else { "system" },
                        Some(if compact { "压缩摘要" } else { title }.into()),
                        &self.turn,
                    );
                    e.role = Some(role.into());
                    e.detail = Some(bounded(&body));
                    e.user_message = Some(user);
                    if !body.trim().is_empty() {
                        self.parsed.events.push(Arc::new(e));
                    }
                } else {
                    // Keep earlier terminal events so an incremental polling cursor
                    // can catch up after several turns between scans.
                    let prompt = field(record, "promptId");
                    if !prompt.is_empty() {
                        let entry = self.parsed.prompts.entry(prompt.into()).or_insert_with(|| {
                            ClaudePrompt {
                                turn_id: key.clone(),
                                timestamp_ms: millis(field(record, "timestamp")).unwrap_or(0),
                                ambiguous: false,
                                cancelled: false,
                                last_activity_position: position as u64 + 1,
                                last_activity_ms: millis(field(record, "timestamp")),
                                last_api_error_position: None,
                            }
                        });
                        entry.ambiguous |= entry.turn_id != key;
                    }
                    self.turn = Some(key.clone());
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
                    if uuid.is_empty() {
                        issue(&mut self.parsed, "原生消息身份缺失，通知和改名已禁用");
                    }
                    if self.first_user.is_none() {
                        let title = crate::user_message::summary_text(&user.text);
                        if !title.trim().is_empty() {
                            self.first_user = Some(title.chars().take(120).collect::<String>());
                        }
                    }
                    let mut e = event(
                        key.clone(),
                        record,
                        "user_message",
                        Some(bounded(&user.text)),
                        &self.turn,
                    );
                    e.user_message = Some(user);
                    self.parsed.events.push(Arc::new(e));
                }
            }
            if kind == "user" {
                if let Some(plan) = record
                    .get("planContent")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    self.parsed.events.push(Arc::new(event(
                        format!("{key}:plan"),
                        record,
                        "plan",
                        Some(bounded(plan)),
                        &self.turn,
                    )));
                }
            }
            let intermediate = kind == "assistant"
                && (field(message, "stop_reason") == "tool_use"
                    || parts.iter().any(|part| field(part, "type") == "tool_use"));
            let api_error = record["isApiErrorMessage"] == true;
            if kind == "assistant"
                || parts.iter().any(|p| {
                    field(p, "type") == "tool_result"
                        && self.pending.contains(field(p, "tool_use_id"))
                })
            {
                self.execution_activity(record, position, api_error);
            }
            if kind == "assistant" {
                // Native transcripts can split one API message into several
                // records. Only an explicit shared message ID links their text.
                let message_id = field(message, "id");
                if message_id.is_empty()
                    || self.last_assistant_message_id.as_deref() != Some(message_id)
                {
                    self.last_assistant_events.clear();
                }
                self.last_assistant_message_id =
                    (!message_id.is_empty() && message_id.len() <= 512).then(|| message_id.into());
                if intermediate {
                    for &index in &self.last_assistant_events {
                        Arc::make_mut(&mut self.parsed.events[index]).assistant_phase =
                            Some(ClaudeAssistantPhase::Intermediate);
                    }
                }
            }
            for (index, part) in parts.iter().enumerate() {
                let mut e = event(format!("{key}:{index}"), record, "", None, &self.turn);
                match field(part, "type") {
                    "text" if kind == "assistant" => {
                        e.kind = "assistant_message".into();
                        e.text = Some(bounded(field(part, "text")));
                        e.assistant_phase =
                            intermediate.then_some(ClaudeAssistantPhase::Intermediate);
                        self.last_assistant_events.push(self.parsed.events.len());
                    }
                    "thinking" if kind == "assistant" => {
                        e.kind = "thinking".into();
                        e.text = Some(bounded(field(part, "thinking")));
                    }
                    "tool_reference" => {
                        let name = if field(part, "tool_name").is_empty() {
                            field(part, "name")
                        } else {
                            field(part, "tool_name")
                        };
                        e.kind = "tool_reference".into();
                        e.text = Some(if name.is_empty() {
                            "工具引用".into()
                        } else {
                            name.into()
                        });
                        e.role = (!name.is_empty()).then(|| name.into());
                        e.detail = Some(printable(part));
                        if name.is_empty() {
                            warning(&mut self.parsed, "工具引用缺少工具名称，已保留为警告");
                        }
                    }
                    "tool_search_tool_result" => {
                        e.kind = "tool_reference".into();
                        e.text = Some("工具引用结果".into());
                        e.detail = part.get("content").map(printable);
                    }
                    "redacted_thinking" => {
                        continue;
                    }
                    "tool_use" if kind == "assistant" => {
                        let call = field(part, "id");
                        let name = field(part, "name");
                        e.kind = "tool_call".into();
                        e.text = Some(name.into());
                        e.role = Some(name.into());
                        e.call_id = Some(call.into());
                        e.status = Some("in_progress".into());
                        e.detail = part.get("input").map(printable);
                        if call.is_empty() {
                            issue(&mut self.parsed, "工具调用身份缺失，改名已禁用");
                        }
                        self.pending.insert(call.into());
                        self.calls.insert(call.into(), self.parsed.events.len());
                        if name == "AskUserQuestion" && !call.is_empty() {
                            if let (Some(turn), Some(ts), Some(qs)) = (
                                &self.turn,
                                millis(field(record, "timestamp")),
                                part.pointer("/input/questions").and_then(Value::as_array),
                            ) {
                                let body = qs
                                    .iter()
                                    .filter_map(|q| {
                                        let question = field(q, "question");
                                        if question.is_empty() {
                                            return None;
                                        }
                                        let options = q
                                            .get("options")
                                            .and_then(Value::as_array)
                                            .map(|opts| {
                                                opts.iter()
                                                    .filter_map(|o| {
                                                        o.as_str().or_else(|| {
                                                            o.get("label").and_then(Value::as_str)
                                                        })
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join(" / ")
                                            })
                                            .unwrap_or_default();
                                        Some(format!("{question}\n{options}"))
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n\n");
                                if !body.trim().is_empty() {
                                    self.questions.insert(
                                        call.into(),
                                        crate::native_probe::NativeTurnEvent {
                                            prompt_id: prompt_for_turn(&self.parsed, turn),
                                            position: position as u64 + 1,
                                            turn_id: format!("{turn}:{call}"),
                                            kind: "reply_needed".into(),
                                            body: bounded(&body),
                                            timestamp_ms: ts,
                                        },
                                    );
                                }
                            }
                        }
                        if name == "ExitPlanMode" {
                            if let Some(plan) = part
                                .pointer("/input/plan")
                                .and_then(Value::as_str)
                                .filter(|p| !p.trim().is_empty())
                            {
                                e.kind = "plan".into();
                                e.text = Some(bounded(plan));
                                e.detail = None;
                            }
                        }
                    }
                    "tool_result" => {
                        let call = field(part, "tool_use_id");
                        self.pending.remove(call);
                        self.questions.remove(call);
                        self.settle_permission(call);
                        let result = part
                            .get("content")
                            .map(|c| {
                                if c.is_array() {
                                    content_text(c)
                                } else {
                                    printable(c)
                                }
                            })
                            .unwrap_or_default();
                        let status = if part.get("is_error").and_then(Value::as_bool) == Some(true)
                        {
                            "failed"
                        } else {
                            "completed"
                        };
                        if let Some(index) = self.calls.get(call) {
                            let prior = Arc::make_mut(&mut self.parsed.events[*index]);
                            prior.status = Some(status.into());
                            prior.result = Some(result);
                            continue;
                        }
                        e.kind = "tool_result".into();
                        e.text = Some("工具结果".into());
                        e.call_id = Some(call.into());
                        e.status = Some(status.into());
                        e.result = Some(result);
                    }
                    "text" | "image" | "document" if kind == "user" => continue,
                    _ => {
                        e.kind = "unknown".into();
                        e.text = Some("未知内容块".into());
                        e.detail = Some(printable(part));
                        warning(&mut self.parsed, "存在未知内容块，已保留为警告");
                    }
                }
                self.parsed.events.push(Arc::new(e));
            }
            if kind == "assistant" && !api_error {
                let text = text_content_only(content);
                if !text.trim().is_empty() {
                    self.last_assistant_text = Some(text);
                }
                self.last_stop_reason = field(message, "stop_reason").into();
                if !terminal_stop_reason(&self.last_stop_reason) || !self.pending.is_empty() {
                    self.parsed.turn_open = self.turn.is_some();
                    self.parsed
                        .native_events
                        .retain(|event| Some(&event.turn_id) != self.turn.as_ref());
                }
            }
            let assistant_terminal = terminal_stop_reason(field(message, "stop_reason"));
            if kind == "assistant"
                && !api_error
                && assistant_terminal
                && self.pending.is_empty()
                && self.questions.is_empty()
            {
                self.mark_final_answer();
                finish(
                    &mut self.parsed,
                    &self.turn,
                    position,
                    record,
                    "completion",
                    self.last_assistant_text.clone().unwrap_or_default(),
                );
            }
        }
    }
    fn read_result(&mut self, record: &Value, position: usize) {
        for _ in std::iter::once(()) {
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
                && (self.last_stop_reason.is_empty()
                    || terminal_stop_reason(&self.last_stop_reason))
                && record.get("is_error").and_then(Value::as_bool) == Some(false);
            if subtype == "success"
                && stop_reason.is_some_and(|reason| !terminal_stop_reason(reason))
            {
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
                for call in &self.pending {
                    if let Some(index) = self.calls.get(call) {
                        Arc::make_mut(&mut self.parsed.events[*index]).status =
                            Some("cancelled".into());
                    }
                }
                self.pending.clear();
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
                self.parsed.events.push(Arc::new(failure));
                finish(
                    &mut self.parsed,
                    &self.turn,
                    position,
                    record,
                    "failure",
                    record
                        .get("errors")
                        .map(printable)
                        .unwrap_or_else(|| subtype.into()),
                );
            }
        }
    }
    fn read_system(&mut self, record: &Value, position: usize, key: String) {
        for key in std::iter::once(key) {
            let subtype = field(record, "subtype");
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
                self.parsed
                    .native_events
                    .retain(|event| Some(&event.turn_id) != self.turn.as_ref());
                self.turn = None;
                self.last_stop_reason.clear();
                self.last_assistant_events.clear();
                self.last_assistant_text = None;
                self.questions.clear();
                for call in &self.pending {
                    if let Some(index) = self.calls.get(call) {
                        Arc::make_mut(&mut self.parsed.events[*index]).status =
                            Some("cancelled".into());
                    }
                }
                self.pending.clear();
            }
            if subtype == "init" {
                // Identity/cwd/version were extracted above. Startup
                // configuration is metadata, not a conversation activity.
                continue;
            }
            if subtype == "compact_boundary" {
                self.last_assistant_events.clear();
                self.parsed.events.push(Arc::new(event(
                    key,
                    record,
                    "compaction",
                    Some(content_text(record.get("content").unwrap_or(&Value::Null))),
                    &self.turn,
                )));
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
                self.parsed.events.push(Arc::new(e));
                if !known {
                    warning(&mut self.parsed, "存在新格式系统活动，仅显示兼容提示");
                }
            }
        }
    }
    fn read_permission_request(&mut self, record: &Value, position: usize, key: String) {
        for key in std::iter::once(key) {
            let call = field(record, "tool_use_id");
            let request = field(record, "request_id");
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
            self.parsed.events.push(Arc::new(e));
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
                                body: bounded(&body),
                                timestamp_ms: ts,
                            },
                        );
                    }
                }
            }
        }
    }
    fn mark_final_answer(&mut self) {
        // Presentation only: never infer the phase from text or change native
        // notification terminal events and their deduplication identities.
        for &index in &self.last_assistant_events {
            let event = &mut self.parsed.events[index];
            if event.turn_id == self.turn
                && event.assistant_phase != Some(ClaudeAssistantPhase::Intermediate)
            {
                Arc::make_mut(event).assistant_phase = Some(ClaudeAssistantPhase::Final);
            }
        }
    }
    fn clear_final_answer(&mut self) {
        for &index in &self.last_assistant_events {
            let event = &mut self.parsed.events[index];
            if event.assistant_phase == Some(ClaudeAssistantPhase::Final) {
                Arc::make_mut(event).assistant_phase = None;
            }
        }
    }
    fn settle_permission(&mut self, call: &str) {
        for event in self
            .parsed
            .events
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
        for key in std::iter::once(key) {
            let name = if field(record, "tool_name").is_empty() {
                field(record, "name")
            } else {
                field(record, "tool_name")
            };
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
            self.parsed.events.push(Arc::new(e));
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
    }
    fn read_auxiliary(&mut self, record: &Value, key: String, kind: &str) {
        for key in std::iter::once(key) {
            if kind == "attachment"
                && record.pointer("/attachment/type").and_then(Value::as_str)
                    == Some("plan_file_reference")
            {
                if let Some(plan) = record
                    .pointer("/attachment/planContent")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    self.parsed.events.push(Arc::new(event(
                        key,
                        record,
                        "plan",
                        Some(bounded(plan)),
                        &self.turn,
                    )));
                    continue;
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
            e.detail = Some(printable(record));
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
                let mut user =
                    crate::user_message::parse_user_message(&e.id, &Value::String(String::new()));
                user.context_blocks
                    .push(crate::user_message::UserContextBlock {
                        kind: if instructions {
                            crate::user_message::UserContextKind::Instructions
                        } else {
                            crate::user_message::UserContextKind::Runtime
                        },
                        text: bounded(&body),
                    });
                e.user_message = Some(user);
            }
            self.parsed.events.push(Arc::new(e));
        }
    }
    fn snapshot(&self, path: &Path, issues: Vec<String>, record_count: u64) -> Parsed {
        let mut parsed = self.parsed.clone();
        parsed.current_turn = self.turn.clone();
        for message in issues {
            issue(&mut parsed, &message);
        }
        parsed.record_count = record_count;
        parsed.settled_count = record_count;
        if parsed.id.is_empty() {
            parsed.id = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
            if !parsed.events.is_empty() {
                issue(&mut parsed, "未确认原生会话 ID");
            }
        }
        parsed.title = self
            .custom_title
            .clone()
            .or_else(|| self.auto_title.clone())
            .or_else(|| self.first_user.clone())
            .unwrap_or_else(|| parsed.id.clone());
        if parsed.version.is_empty() {
            parsed.version = "unknown".into();
        }
        if parsed.version != "unknown" && !parsed.version.starts_with("2.") {
            issue(&mut parsed, "此 Claude 版本尚未验证，改名已禁用");
        }
        for q in self.questions.values() {
            parsed.native_events.push(q.clone());
        }
        parsed.native_events.sort_by_key(|e| e.position);
        let messages: Vec<_> = parsed
            .events
            .iter()
            .filter(|event| matches!(event.kind.as_str(), "user_message" | "assistant_message"))
            .collect();
        parsed.message_count = messages.len();
        parsed.updated_at = messages
            .iter()
            .rev()
            .find(|event| {
                event
                    .text
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty())
                    || event
                        .user_message
                        .as_ref()
                        .is_some_and(|message| !message.attachments.is_empty())
            })
            .and_then(|event| event.timestamp.clone());
        parsed.last_message = messages
            .last()
            .and_then(|event| event.text.as_ref())
            .map(|text| text.chars().take(240).collect());
        parsed
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

fn finish(
    parsed: &mut Parsed,
    turn: &Option<String>,
    position: usize,
    record: &Value,
    kind: &str,
    body: String,
) {
    parsed.turn_open = false;
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
            body,
            timestamp_ms: ts,
        });
}
