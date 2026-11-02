//! Read-only native provider terminal-turn detection. No CLI, hooks or extensions are started.
use crate::{config::Config, security::redact_output};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader},
    path::Path,
};

pub(crate) fn notification_title(title: &str, id: &str, cwd: &str) -> String {
    if !title.trim().is_empty()
        && title != id
        && !matches!(title.trim(), "未命名会话" | "未命名线程")
    {
        title.into()
    } else {
        Path::new(cwd)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NativeProvider {
    Grok,
    #[serde(rename = "claude_code")]
    Claude,
    Antigravity,
    Dsh,
}
impl NativeProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grok => "grok",
            Self::Claude => "claude_code",
            Self::Antigravity => "antigravity",
            Self::Dsh => "dsh",
        }
    }
    pub fn local_only(self) -> bool {
        matches!(self, Self::Antigravity | Self::Dsh)
    }
    pub fn enabled(self, config: &Config) -> bool {
        config.probe.enabled
            && config.probe.notifications.enabled
            && match self {
                Self::Grok => config.probe.notifications.notify_grok,
                Self::Claude => config.probe.notifications.notify_claude,
                Self::Antigravity => config.probe.notifications.notify_antigravity,
                Self::Dsh => config.probe.notifications.notify_dsh,
            }
    }
    pub fn event_enabled(self, config: &Config, kind: &str) -> bool {
        match (self, kind) {
            (Self::Claude, "completion") => config.probe.notifications.notify_claude_completion,
            (Self::Claude, "failure") => config.probe.notifications.notify_claude_failure,
            (Self::Claude, "reply_needed" | "authorization") => {
                config.probe.notifications.notify_claude_reply_needed
            }
            (Self::Grok, "completion") => config.probe.notifications.notify_grok_completion,
            (Self::Grok, "failure") => config.probe.notifications.notify_grok_failure,
            (Self::Grok, "reply_needed" | "authorization") => {
                config.probe.notifications.notify_grok_reply_needed
            }
            (Self::Antigravity, "completion") => {
                config.probe.notifications.notify_antigravity_completion
            }
            (Self::Antigravity, "failure") => config.probe.notifications.notify_antigravity_failure,
            (Self::Antigravity, "reply_needed") => {
                config.probe.notifications.notify_antigravity_reply_needed
            }
            (Self::Antigravity, "authorization") => {
                config.probe.notifications.notify_antigravity_authorization
            }
            (Self::Dsh, "completion") => config.probe.notifications.notify_dsh_completion,
            (Self::Dsh, "failure") => config.probe.notifications.notify_dsh_failure,
            (Self::Dsh, "reply_needed") => config.probe.notifications.notify_dsh_reply_needed,
            (Self::Dsh, "authorization") => config.probe.notifications.notify_dsh_authorization,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeTurnEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
    pub position: u64,
    pub turn_id: String,
    pub kind: String,
    pub body: String,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeStreamSnapshot {
    pub provider: NativeProvider,
    pub session_key: String,
    pub id: String,
    pub title: String,
    pub identity: String,
    pub record_count: u64,
    pub settled_count: u64,
    pub events: Vec<NativeTurnEvent>,
}
impl NativeStreamSnapshot {
    pub fn key(&self) -> String {
        hex::encode(Sha256::digest(format!(
            "{}\0{}\0{}",
            self.provider.as_str(),
            self.session_key,
            self.identity
        )))
    }
    pub fn prompt_event_key(&self, prompt: &str, kind: &str, instance: &str) -> String {
        hex::encode(Sha256::digest(format!(
            "claude-prompt\0{instance}\0{}\0{prompt}\0{kind}",
            self.key()
        )))
    }
    pub fn event_key(&self, event: &NativeTurnEvent) -> String {
        hex::encode(Sha256::digest(format!(
            "{}\0{}\0{}",
            self.key(),
            event.turn_id,
            event.kind
        )))
    }
}

#[derive(Debug, Clone, Default)]
pub struct NativeScan {
    pub streams: Vec<NativeStreamSnapshot>,
    pub errors: usize,
}

pub fn event_category(event: &NativeTurnEvent) -> &str {
    if event.kind == "reply_needed" && event.turn_id.contains(":permission:") {
        "authorization"
    } else {
        &event.kind
    }
}
pub fn same_event(current: &NativeTurnEvent, queued: &NativeTurnEvent) -> bool {
    current.position == queued.position
        && current.turn_id == queued.turn_id
        && event_category(current) == event_category(queued)
        && current.body == queued.body
        && current.timestamp_ms == queued.timestamp_ms
        && queued
            .prompt_id
            .as_ref()
            .is_none_or(|p| current.prompt_id.as_ref() == Some(p))
}

pub fn file_identity(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "notification source must be a regular file"
    );
    let canonical = path.canonicalize()?;
    let mut digest = Sha256::new();
    digest.update(canonical.as_os_str().as_encoded_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        digest.update(metadata.dev().to_le_bytes());
        digest.update(metadata.ino().to_le_bytes());
    }
    Ok(hex::encode(digest.finalize()))
}

pub fn retry_event_is_current(provider: NativeProvider, id: &str, event: &NativeTurnEvent) -> bool {
    match provider {
        NativeProvider::Grok => crate::grok::notification_event_current(id, event.position),
        NativeProvider::Claude => true, // Claude snapshots retain terminal events on the valid branch.
        NativeProvider::Antigravity | NativeProvider::Dsh => true, // Re-read by the local source verifier.
    }
}

pub fn scan(provider: NativeProvider) -> Result<NativeScan> {
    match provider {
        NativeProvider::Claude => {
            crate::claude::notification_snapshots(&crate::claude::ClaudePaths::default_for_user())
        }
        NativeProvider::Grok => {
            crate::grok::notification_snapshots(&crate::grok::GrokPaths::default_for_user())
        }
        NativeProvider::Dsh => {
            crate::dsh::notifications::scan(&crate::dsh::DshPaths::default_for_user())
        }
        NativeProvider::Antigravity => {
            anyhow::bail!("Antigravity requires the local native transport")
        }
    }
}

static NATIVE_PROBE_CACHE: crate::read_cache::ReadCache<Vec<Value>> =
    crate::read_cache::ReadCache::new(crate::resource_limits::NATIVE_PROBE_CACHE_BYTES);

pub(crate) fn evict_idle_cache(max_idle: std::time::Duration) {
    NATIVE_PROBE_CACHE.evict_idle(max_idle);
}

pub(crate) fn read_jsonl(path: &Path) -> Result<Vec<Value>> {
    NATIVE_PROBE_CACHE.read(
        path,
        |_, size| size.saturating_mul(5),
        || {
            file_identity(path)?;
            ensure!(
                fs::metadata(path)?.len() <= 128 * 1024 * 1024,
                "notification source exceeds size limit"
            );
            let mut reader = BufReader::new(fs::File::open(path)?);
            let mut records = Vec::new();
            let mut line = Vec::new();
            loop {
                line.clear();
                if reader.read_until(b'\n', &mut line)? == 0 {
                    break;
                }
                // Native writers append JSONL. Never consume a partial record.
                if !line.ends_with(b"\n") {
                    break;
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                ensure!(
                    line.len() <= 8 * 1024 * 1024 && records.len() < 200_000,
                    "notification source exceeds record limit"
                );
                records.push(
                    serde_json::from_slice(&line).context("invalid native notification record")?,
                );
            }
            Ok(records)
        },
    )
}

pub fn timestamp(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .map(|n| {
            if n.abs() < 100_000_000_000 {
                n.saturating_mul(1000)
            } else {
                n
            }
        })
        .or_else(|| {
            value
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|ts| ts.timestamp_millis())
        })
}

/// Completion needs both a native main-turn ending and its matching prompt.
/// The event stream alone excludes tool, background, retry and cancel events.
pub(crate) fn grok_events(
    id: &str,
    events: &[Value],
    updates: &[Value],
) -> (Vec<NativeTurnEvent>, u64) {
    let mut start: Option<&Value> = None;
    let mut result = Vec::new();
    let mut settled = events.len() as u64;
    for (position, event) in events.iter().enumerate() {
        match event.get("type").and_then(Value::as_str) {
            Some("turn_started") => {
                start = Some(event);
            }
            Some("turn_ended") => {
                let Some(begin) = start.take() else {
                    continue;
                };
                if begin.get("session_id").and_then(Value::as_str) != Some(id)
                    || begin.get("session_relationship").and_then(Value::as_str) != Some("primary")
                {
                    continue;
                }
                let outcome = event
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !matches!(outcome, "completed" | "error") {
                    continue;
                }
                let (Some(start_ms), Some(end_ms)) = (
                    begin.get("ts").and_then(timestamp),
                    event.get("ts").and_then(timestamp),
                ) else {
                    continue;
                };
                if end_ms < start_ms
                    || begin
                        .get("schema_version")
                        .and_then(Value::as_str)
                        .is_some_and(|v| v != "1.0")
                {
                    continue;
                }
                let next_start = events[position + 1..]
                    .iter()
                    .find(|row| row.get("type").and_then(Value::as_str) == Some("turn_started"))
                    .and_then(|row| row.get("ts"))
                    .and_then(timestamp);
                let rows = updates
                    .iter()
                    .filter(|row| {
                        row.pointer("/params/sessionId").and_then(Value::as_str) == Some(id)
                            && row
                                .pointer("/params/_meta/agentTimestampMs")
                                .or_else(|| row.get("timestamp"))
                                .and_then(timestamp)
                                .is_some_and(|ts| {
                                    ts >= start_ms
                                        && ts <= end_ms.saturating_add(2_000)
                                        && next_start.is_none_or(|next| ts < next)
                                })
                    })
                    .collect::<Vec<_>>();
                let endings = rows
                    .iter()
                    .filter(|row| {
                        row.pointer("/params/update/sessionUpdate")
                            .and_then(Value::as_str)
                            == Some("turn_completed")
                    })
                    .collect::<Vec<_>>();
                // Multiple distinct terminal prompts cannot be safely correlated
                // using timestamps alone; wait for an unambiguous native snapshot.
                let prompt_ids = endings
                    .iter()
                    .filter_map(|row| {
                        row.pointer("/params/update/prompt_id")
                            .and_then(Value::as_str)
                    })
                    .collect::<HashSet<_>>();
                let Some(ending) = endings.last().filter(|_| prompt_ids.len() == 1) else {
                    settled = settled.min(position as u64);
                    continue;
                };
                let stop = ending
                    .pointer("/params/update/stop_reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if stop == "cancelled" {
                    continue;
                }
                let Some(prompt) = ending
                    .pointer("/params/update/prompt_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                else {
                    settled = settled.min(position as u64);
                    continue;
                };
                let mut kind = match (outcome, stop) {
                    ("completed", "end_turn") => "completion",
                    ("error", "error") => "failure",
                    _ => continue,
                };
                let mut body = String::new();
                let mut seen_chunks = HashSet::new();
                for row in &rows {
                    if row
                        .pointer("/params/update/sessionUpdate")
                        .and_then(Value::as_str)
                        == Some("retry_state")
                    {
                        body.clear();
                        continue;
                    }
                    if row
                        .pointer("/params/_meta/promptId")
                        .and_then(Value::as_str)
                        != Some(prompt)
                    {
                        continue;
                    }
                    let update = &row["params"]["update"];
                    match update.get("sessionUpdate").and_then(Value::as_str) {
                        Some("tool_call") => body.clear(),
                        Some("agent_message_chunk") => {
                            if let Some(chunk) =
                                row.pointer("/params/_meta/chunkId").and_then(Value::as_str)
                            {
                                if !seen_chunks.insert(chunk) {
                                    continue;
                                }
                            }
                            if update.pointer("/content/type").and_then(Value::as_str)
                                == Some("text")
                            {
                                if let Some(text) =
                                    update.pointer("/content/text").and_then(Value::as_str)
                                {
                                    body.push_str(text);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if kind == "failure" {
                    body = "Grok 主回合已明确终止失败，请打开线程查看详情。".into();
                }
                if body.trim().is_empty() {
                    settled = settled.min(position as u64);
                    continue;
                }
                if kind == "completion" && crate::codex::assistant_feedback_request(&body).is_some()
                {
                    kind = "reply_needed";
                }
                result.push(NativeTurnEvent {
                    prompt_id: None,
                    position: position as u64 + 1,
                    turn_id: format!(
                        "{}:{prompt}",
                        begin.get("turn_number").unwrap_or(&Value::Null)
                    ),
                    kind: kind.into(),
                    body: redact_output(&body),
                    timestamp_ms: end_ms,
                });
            }
            _ => {}
        }
    }
    (result, settled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_subject_uses_project_when_native_title_is_missing() {
        for title in ["", "fixture-id", "未命名会话", "未命名线程"] {
            assert_eq!(
                notification_title(title, "fixture-id", "/fixture/project"),
                "project"
            );
        }
        assert_eq!(
            notification_title("Named session", "fixture-id", "/fixture/project"),
            "Named session"
        );
    }
    use serde_json::json;
    fn grok_fixture(outcome: &str, stop: &str, relationship: &str) -> (Vec<Value>, Vec<Value>) {
        let id = "session";
        let ts = 1_767_225_600_000i64;
        let events = vec![
            json!({"type":"turn_started","ts":ts,"session_id":id,"session_relationship":relationship,"turn_number":2}),
            json!({"type":"turn_ended","ts":ts+500,"outcome":outcome}),
        ];
        let updates = vec![
            json!({"timestamp":ts+10,"params":{"sessionId":id,"update":{"sessionUpdate":"retry_state","type":"retrying"}}}),
            json!({"timestamp":ts+20,"params":{"sessionId":id,"_meta":{"promptId":"p","chunkId":"c"},"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"final answer"}}}}),
            json!({"timestamp":ts+500,"params":{"sessionId":id,"update":{"sessionUpdate":"turn_completed","prompt_id":"p","stop_reason":stop}}}),
        ];
        (events, updates)
    }
    #[test]
    fn grok_requires_main_terminal_prompt_and_excludes_cancel_background_or_tool_errors() {
        for (outcome, stop, rel, expected) in [
            ("completed", "end_turn", "primary", 1),
            ("completed", "end_turn", "child", 0),
            ("cancelled", "cancelled", "primary", 0),
            ("failed", "cancelled", "primary", 0),
            ("error", "error", "primary", 1),
            ("completed", "tool_error", "primary", 0),
        ] {
            let (events, updates) = grok_fixture(outcome, stop, rel);
            let (turns, _) = grok_events("session", &events, &updates);
            assert_eq!(turns.len(), expected, "{outcome}/{stop}/{rel}");
            assert!(grok_events("different", &events, &updates).0.is_empty());
        }
        let (events, mut updates) = grok_fixture("completed", "end_turn", "primary");
        updates.pop();
        let (turns, cursor) = grok_events("session", &events, &updates);
        assert!(turns.is_empty());
        assert_eq!(cursor, 1, "unsettled cross-file record must be retried");
    }
    #[test]
    fn native_jsonl_keeps_half_line_pending_and_rejects_complete_corruption() {
        let p = std::env::temp_dir().join(format!("native-probe-{}.jsonl", uuid::Uuid::new_v4()));
        fs::write(&p, b"{\"ok\":true}\n{\"partial\":").unwrap();
        assert_eq!(read_jsonl(&p).unwrap().len(), 1);
        fs::write(&p, b"{\"ok\":true}\nbroken\n").unwrap();
        assert!(read_jsonl(&p).is_err());
        fs::remove_file(p).unwrap();
    }

    #[test]
    fn grok_adjacent_turns_and_retry_keep_only_the_matching_final_answer() {
        let (mut events, mut updates) = grok_fixture("completed", "end_turn", "primary");
        let ts = events[0]["ts"].as_i64().unwrap();
        updates.insert(0, json!({"timestamp":ts+1,"params":{"sessionId":"session","_meta":{"promptId":"p","chunkId":"discard"},"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"discarded retry"}}}}));
        events.extend([
            json!({"type":"turn_started","ts":ts+600,"session_id":"session","session_relationship":"primary","turn_number":3}),
            json!({"type":"turn_ended","ts":ts+800,"outcome":"completed"}),
        ]);
        updates.extend([
            json!({"timestamp":ts/1000,"params":{"sessionId":"session","_meta":{"agentTimestampMs":ts+700,"promptId":"next","chunkId":"next-c"},"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"next answer"}}}}),
            json!({"timestamp":ts/1000,"params":{"sessionId":"session","_meta":{"agentTimestampMs":ts+800},"update":{"sessionUpdate":"turn_completed","prompt_id":"next","stop_reason":"end_turn"}}}),
        ]);
        let (turns, cursor) = grok_events("session", &events, &updates);
        assert_eq!(cursor, 4);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].body, "final answer");
        assert_eq!(turns[1].body, "next answer");
        assert_ne!(turns[0].turn_id, turns[1].turn_id);

        updates.insert(3, json!({"timestamp":ts+499,"params":{"sessionId":"session","update":{"sessionUpdate":"turn_completed","prompt_id":"ambiguous","stop_reason":"end_turn"}}}));
        let (turns, cursor) = grok_events("session", &events, &updates);
        assert_eq!(cursor, 1);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].body, "next answer");
    }

    #[test]
    fn grok_detects_reply_needed_questions_and_honors_grok_reply_needed_switch() {
        let (events, mut updates) = grok_fixture("completed", "end_turn", "primary");
        updates[1]["params"]["update"]["content"]["text"] =
            json!("构建准备就绪。请问是否现在继续执行数据库迁移？");
        let (turns, cursor) = grok_events("session", &events, &updates);
        assert_eq!(cursor, 2);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].kind, "reply_needed");

        // Rhetorical tail with earlier keywords stays completion instead of false-positive reply_needed.
        updates[1]["params"]["update"]["content"]["text"] =
            json!("已检查是否还有遗留项。为什么不值得庆贺呢？");
        let (turns_rhetorical, _) = grok_events("session", &events, &updates);
        assert_eq!(turns_rhetorical[0].kind, "completion");

        let mut cfg = Config::default();
        assert!(NativeProvider::Grok.event_enabled(&cfg, "reply_needed"));
        cfg.probe.notifications.notify_grok_reply_needed = false;
        assert!(!NativeProvider::Grok.event_enabled(&cfg, "reply_needed"));
    }
}
