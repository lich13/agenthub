//! Read-only projection of Grok Bot's account-scoped, bounded desktop cache.
use crate::{
    native_probe::{NativeProvider, NativeScan, NativeStreamSnapshot, NativeTurnEvent},
    provider_sessions::{
        self, ProviderDetailRequest, ProviderHistoryEvent, ProviderHistorySnapshot,
        ProviderListRequest, ProviderSessionDetail, ProviderSessionSummary,
    },
};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const PREFIX: &str = "sand.client.slice.";

#[derive(Debug, Clone)]
pub struct GrokBotPaths {
    pub persistence: PathBuf,
}
impl GrokBotPaths {
    pub fn default_for_user() -> Result<Self> {
        Ok(Self {
            persistence: crate::platform::current_home()?
                .join("Library/Application Support/Grok Bot/sand-client-persistence"),
        })
    }
}
fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let (mut bits, mut count, mut out) = (0u32, 0u32, String::new());
    for byte in bytes {
        bits = (bits << 8) | u32::from(*byte);
        count += 8;
        while count >= 5 {
            count -= 5;
            out.push(ALPHABET[((bits >> count) & 31) as usize] as char);
        }
    }
    if count > 0 {
        out.push(ALPHABET[((bits << (5 - count)) & 31) as usize] as char);
    }
    out
}
fn encode_scope(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-~_!()'*".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn slice_path(root: &Path, key: &str) -> PathBuf {
    let name = format!("{}.blob", base32(key.as_bytes()));
    root.join(if name.len() <= 240 {
        name
    } else {
        format!("{}.kblob", base32(&Sha256::digest(key)))
    })
}
fn read_slice(root: &Path, key: &str, schema: u64) -> Result<Option<Value>> {
    let path = slice_path(root, key);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(m) => ensure!(
            m.is_file() && !m.file_type().is_symlink(),
            "Grok Bot 缓存不是普通文件"
        ),
    }
    let text = fs::read_to_string(&path).context("无法读取 Grok Bot 缓存")?;
    let body = if path.extension().is_some_and(|s| s == "kblob") {
        let (header, body) = text.split_once('\n').context("Grok Bot 缓存格式错误")?;
        ensure!(
            serde_json::from_str::<String>(header)? == key,
            "Grok Bot 缓存身份不匹配"
        );
        body
    } else {
        &text
    };
    let mut envelope: Value = serde_json::from_str(body).context("Grok Bot 缓存格式错误")?;
    ensure!(
        envelope["schemaVersion"].as_u64() == Some(schema),
        "Grok Bot 缓存格式暂不支持"
    );
    let value = envelope
        .get_mut("value")
        .context("Grok Bot 缓存内容缺失")?
        .take();
    Ok(Some(value))
}
fn account(paths: &GrokBotPaths) -> Result<Option<String>> {
    if !paths.persistence.is_dir() {
        return Ok(None);
    }
    read_slice(
        &paths.persistence,
        &format!("{PREFIX}client-meta.account-slot"),
        1,
    )?
    .map(|value| {
        value
            .as_str()
            .filter(|s| !s.is_empty())
            .map(encode_scope)
            .context("Grok Bot 账户不可读取")
    })
    .transpose()
}
fn key(scope: &str, slice: &str) -> String {
    format!("{PREFIX}account.{scope}.{slice}")
}
fn roster(paths: &GrokBotPaths) -> Result<Option<(String, Value)>> {
    let Some(scope) = account(paths)? else {
        return Ok(None);
    };
    let value = read_slice(&paths.persistence, &key(&scope, "roster.last-roster"), 4)?
        .unwrap_or_else(|| serde_json::json!({"rows":[]}));
    Ok(Some((scope, value)))
}
fn roster_rows(roster: &Value) -> Result<&Vec<Value>> {
    roster["rows"].as_array().context("Grok Bot 列表格式错误")
}
fn session_key(scope: &str, id: &str) -> String {
    format!("{}:{id}", hex::encode(Sha256::digest(scope)))
}
fn locate(paths: &GrokBotPaths, session: &str) -> Result<(String, Value)> {
    let (scope, roster) = roster(paths)?.context("Grok Bot 当前账户不可用")?;
    let prefix = format!("{}:", hex::encode(Sha256::digest(&scope)));
    let id = session
        .strip_prefix(&prefix)
        .context("Grok Bot 线程不在当前账户中")?;
    let row = roster_rows(&roster)?
        .iter()
        .find(|row| row["id"].as_str() == Some(id))
        .context("Grok Bot 线程不在当前账户中")?;
    Ok((scope, row.clone()))
}
fn replica_key(scope: &str, id: &str) -> String {
    key(scope, &format!("transcript.replicas.{id}"))
}
fn text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.into();
    }
    if let Some(a) = value.as_array() {
        return a
            .iter()
            .map(text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    for field in [
        "content",
        "text",
        "question",
        "prompt",
        "instruction",
        "summary",
        "title",
        "label",
        "name",
    ] {
        if let Some(v) = value.get(field) {
            let s = text(v);
            if !s.is_empty() {
                return s;
            }
        }
    }
    String::new()
}
fn timestamp(ms: i64) -> Option<String> {
    jiff::Timestamp::from_millisecond(ms)
        .ok()
        .map(|ts| crate::timestamps::format_utc(ts, false))
}
fn row_title(row: &Value) -> String {
    row["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or(row["name"].as_str())
        .unwrap_or("Grok Bot")
        .into()
}
#[derive(Default)]
struct ReplicaInteractionSummary {
    has_pending_auth: bool,
    pending_reply_deadlines: Vec<Option<i64>>,
    seen_ids: std::collections::HashSet<String>,
}

static REPLICA_INTERACTION_CACHE: crate::read_cache::ReadCache<ReplicaInteractionSummary> =
    crate::read_cache::ReadCache::new(4 * 1024 * 1024)
        .with_entry_limit(512)
        .with_native_metadata();

pub(crate) fn evict_idle_cache(max_idle: std::time::Duration) {
    REPLICA_INTERACTION_CACHE.evict_idle(max_idle);
}

impl ReplicaInteractionSummary {
    fn from_replica(replica: &Value) -> Self {
        let mut out = Self::default();
        for entry in replica["entries"].as_array().into_iter().flatten() {
            for candidate in [
                entry["id"].as_str(),
                entry["requestId"].as_str(),
                entry["message"]["ask"]["requestId"].as_str(),
                entry["message"]["approval"]["requestId"].as_str(),
            ]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            {
                out.seen_ids.insert(candidate.to_owned());
            }
            if entry["timestampMs"].as_i64().is_none_or(|t| t <= 0) {
                continue;
            }
            if entry["kind"] != "send-message"
                || entry.get("respondedValue").is_some_and(|v| !v.is_null())
                || entry["widgetDismissed"] == true
                || entry.get("formResolution").is_some_and(|v| !v.is_null())
            {
                continue;
            }
            let message = &entry["message"];
            match message["type"].as_str() {
                Some("local-tool-permission")
                    if message["ask"]["status"] == "pending"
                        && message["ask"]["requestId"].as_str().is_some() =>
                {
                    out.has_pending_auth = true;
                }
                Some(
                    "auto-review-approval"
                    | "meeting-join-consent"
                    | "cookie-origin-approval"
                    | "virtual-card-approval",
                ) if message["approval"]["status"] == "pending"
                    && message["approval"]["requestId"].as_str().is_some() =>
                {
                    out.has_pending_auth = true;
                }
                Some("user-form")
                    if message["formRequest"].is_object() && entry["id"].as_str().is_some() =>
                {
                    let question = &message["phoneCallQuestion"];
                    if question.is_object() && question.get("closed").is_some() {
                        continue;
                    }
                    let deadline = if question.is_object() {
                        question["deadlineAtMs"].as_i64()
                    } else {
                        None
                    };
                    out.pending_reply_deadlines.push(deadline);
                }
                _ => {}
            }
        }
        out
    }

    fn weight(&self) -> u64 {
        (std::mem::size_of::<Self>()
            + self.pending_reply_deadlines.capacity() * std::mem::size_of::<Option<i64>>()
            + self
                .seen_ids
                .iter()
                .map(|s| std::mem::size_of::<String>() + s.capacity())
                .sum::<usize>()) as u64
    }
}

fn read_replica_interaction_summary(
    paths: &GrokBotPaths,
    scope: &str,
    id: &str,
    path: &Path,
) -> Result<Option<Arc<ReplicaInteractionSummary>>> {
    if !fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink()) {
        return Ok(None);
    }
    let k = replica_key(scope, id);
    REPLICA_INTERACTION_CACHE
        .read(
            path,
            |summary, _| summary.weight(),
            || {
                let value = read_slice(&paths.persistence, &k, 1)?;
                if let Some(value) = &value {
                    ensure!(value["entries"].is_array(), "Grok Bot 线程缓存格式错误");
                }
                Ok(value
                    .as_ref()
                    .map(ReplicaInteractionSummary::from_replica)
                    .unwrap_or_default())
            },
        )
        .map(Some)
}

fn derive_session_status(
    row: &Value,
    replica_summary: Option<&ReplicaInteractionSummary>,
) -> &'static str {
    if let Some(summary) = replica_summary {
        if summary.has_pending_auth {
            return "authorization";
        }
        let now_ms = jiff::Timestamp::now().as_millisecond();
        if summary
            .pending_reply_deadlines
            .iter()
            .any(|d| d.is_none_or(|deadline| deadline > now_ms))
        {
            return "reply_needed";
        }
    }
    let awaiting = &row["awaitingUserResponse"];
    if let (Some(request), Some(_ts)) = (
        awaiting["id"].as_str(),
        row["lastActivityAt"].as_i64().filter(|&t| t > 0),
    ) {
        if replica_summary.is_none_or(|s| !s.seen_ids.contains(request)) {
            return "reply_needed";
        }
    } else if replica_summary.is_none()
        && row
            .get("awaitingUserResponse")
            .is_some_and(|v| !v.is_null())
    {
        return "reply_needed";
    }
    "unknown"
}

fn summary(paths: &GrokBotPaths, scope: &str, row: &Value) -> Result<ProviderSessionSummary> {
    summary_inner(paths, scope, row, true)
}

fn summary_inner(
    paths: &GrokBotPaths,
    scope: &str,
    row: &Value,
    include_storage: bool,
) -> Result<ProviderSessionSummary> {
    let id = row["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Grok Bot 线程身份缺失")?;
    let path = slice_path(&paths.persistence, &replica_key(scope, id));
    let available =
        fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink());
    let replica_summary = if available {
        read_replica_interaction_summary(paths, scope, id, &path)
            .ok()
            .flatten()
    } else {
        None
    };
    let status = derive_session_status(row, replica_summary.as_deref());
    Ok(ProviderSessionSummary {
        storage_size: (include_storage && available)
            .then(|| crate::session_storage::file_size(&paths.persistence, &path)),
        id: id.into(),
        session_key: session_key(scope, id),
        title: row_title(row),
        cwd: String::new(),
        workspace_root: None,
        path: if available { path } else { PathBuf::new() },
        archived: false,
        updated_at: row["lastActivityAt"]
            .as_i64()
            .or(row["updatedAt"].as_i64())
            .and_then(timestamp),
        message_count: 0,
        last_message: row["lastEntry"]["text"].as_str().map(str::to_owned),
        status: status.into(),
        format_version: String::new(),
        can_rename: false,
        rename_block_reason: None,
        can_archive: false,
        archive_block_reason: None,
        can_delete: false,
        delete_block_reason: None,
        read_error: None,
        read_warning: None,
    })
}
pub fn list(
    paths: &GrokBotPaths,
    request: &ProviderListRequest,
) -> Result<Vec<ProviderSessionSummary>> {
    list_inner(paths, request, true)
}

pub(crate) fn list_without_storage(
    paths: &GrokBotPaths,
    request: &ProviderListRequest,
) -> Result<Vec<ProviderSessionSummary>> {
    list_inner(paths, request, false)
}

fn list_inner(
    paths: &GrokBotPaths,
    request: &ProviderListRequest,
    include_storage: bool,
) -> Result<Vec<ProviderSessionSummary>> {
    let Some((scope, roster)) = roster(paths)? else {
        return Ok(vec![]);
    };
    let mut rows = roster_rows(&roster)?
        .iter()
        .filter(|r| r["isRemovedFromSidebar"] != true)
        .map(|r| summary_inner(paths, &scope, r, include_storage))
        .collect::<Result<Vec<_>>>()?;
    rows.retain(|r| provider_sessions::matches_query(r, request.q.as_deref()));
    rows.sort_by_cached_key(|r| {
        (
            std::cmp::Reverse(crate::timestamps::sort_key_nanos(r.updated_at.as_deref())),
            r.id.clone(),
        )
    });
    rows.truncate(request.limit.unwrap_or(256));
    Ok(rows)
}
pub fn session_summary(paths: &GrokBotPaths, session: &str) -> Result<ProviderSessionSummary> {
    let (scope, row) = locate(paths, session)?;
    summary(paths, &scope, &row)
}
fn interaction(entry: &Value) -> Option<(&'static str, String, String)> {
    if entry["kind"] != "send-message"
        || entry.get("respondedValue").is_some_and(|v| !v.is_null())
        || entry["widgetDismissed"] == true
        || entry.get("formResolution").is_some_and(|v| !v.is_null())
    {
        return None;
    }
    let message = &entry["message"];
    match message["type"].as_str()? {
        "local-tool-permission" if message["ask"]["status"] == "pending" => Some((
            "authorization",
            message["ask"]["requestId"].as_str()?.into(),
            text(&message["ask"]),
        )),
        "auto-review-approval"
        | "meeting-join-consent"
        | "cookie-origin-approval"
        | "virtual-card-approval"
            if message["approval"]["status"] == "pending" =>
        {
            Some((
                "authorization",
                message["approval"]["requestId"].as_str()?.into(),
                text(&message["approval"]),
            ))
        }
        "user-form" if message["formRequest"].is_object() => {
            let question = &message["phoneCallQuestion"];
            if question.is_object()
                && (question.get("closed").is_some()
                    || question["deadlineAtMs"].as_i64().is_some_and(|deadline| {
                        deadline <= jiff::Timestamp::now().as_millisecond()
                    }))
            {
                return None;
            }
            Some((
                "reply_needed",
                entry["id"].as_str()?.into(),
                text(&message["formRequest"]),
            ))
        }
        _ => None,
    }
}
fn events(replica: &Value) -> Vec<ProviderHistoryEvent> {
    replica["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry["id"].as_str()?.to_owned();
            let mut event = ProviderHistoryEvent {
                id: id.clone(),
                timestamp: entry["timestampMs"].as_i64().and_then(timestamp),
                kind: "system".into(),
                role: None,
                text: None,
                call_id: None,
                turn_id: entry["requestId"].as_str().map(str::to_owned),
                status: None,
                detail: None,
                result: None,
                user_message: None,
                subagent: None,
            };
            let content = match entry["kind"].as_str()? {
                "message" => {
                    let role = entry["role"].as_str()?;
                    if !matches!(role, "user" | "assistant") {
                        return None;
                    }
                    event.kind = format!("{role}_message");
                    event.role = Some(role.into());
                    if role == "user" {
                        let mut parts = vec![serde_json::json!({"type":"text", "text":entry["content"]})];
                        for image in entry["images"].as_array().into_iter().flatten() {
                            parts.push(serde_json::json!({"type":"image_url", "image_url":image["url"]}));
                        }
                        event.user_message = Some(crate::user_message::parse_user_message(&id, &Value::Array(parts)));
                    }
                    text(&entry["content"])
                }
                "send-message" => {
                    event.kind = "assistant_message".into();
                    event.role = Some("assistant".into());
                    let msg = &entry["message"];
                    match msg["type"].as_str() {
                        Some("text") => text(&msg["content"]),
                        Some("attachment") => {
                            let name = msg["file_name"].as_str().unwrap_or("附件").replace('[', "\\[").replace(']', "\\]");
                            let path = msg["source_path"].as_str().filter(|s| s.starts_with('/')).or(msg["url"].as_str());
                            path.map_or(name.clone(), |path| format!("[{name}](<{}>)", path.replace('>', "%3E")))
                        },
                        _ => {
                            event.kind = "interaction".into();
                            event.role = None;
                            event.detail = Some(text(if msg["type"] == "widget" { &msg["widget"] } else if msg["type"] == "user-form" { &msg["formRequest"] } else { msg }));
                            event.result = entry.get("respondedValue").map(text).filter(|s| !s.is_empty());
                            event.status = interaction(entry).map(|(kind, _, _)| kind.into());
                            if let Some(result) = &event.result {
                                event.detail = Some(format!("{}\n\n{result}", event.detail.as_deref().unwrap_or_default()));
                            }
                            let value = text(msg);
                            if value.is_empty() {
                                if msg["type"] == "widget" {
                                    "交互卡片".into()
                                } else {
                                    "交互记录".into()
                                }
                            } else {
                                value
                            }
                        }
                    }
                }
                "tool-call" => {
                    event.kind = "tool_call".into();
                    event.call_id = Some(entry["callId"].as_str().unwrap_or(&id).into());
                    event.status = entry["status"].as_str().map(str::to_owned);
                    event.detail = entry.get("input").map(|v| {
                        if v.is_string() {
                            text(v)
                        } else {
                            v.to_string()
                        }
                    });
                    event.result = entry.get("result").map(|value| if value.is_string() { text(value) } else { value.to_string() });
                    let summary = text(&entry["summary"]);
                    if summary.is_empty() { text(&entry["name"]) } else { summary }
                }
                "user-attachment" => {
                    event.kind = "user_message".into();
                    event.role = Some("user".into());
                    let name = entry["file_name"].as_str().unwrap_or("附件");
                    event.user_message = Some(crate::user_message::parse_user_message(
                        &id,
                        &serde_json::json!([{"type":if entry["width"].is_number() { "local_image" } else { "resource_link" },"path":entry["file_path"]}]),
                    ));
                    name.into()
                }
                "notice" | "event" | "feedback" | "voice-call" | "mcp-app" => text(entry),
                _ => return None,
            };
            event.text = Some(content);
            Some(event)
        })
        .collect()
}
pub(crate) const GROKBOT_VERIFY_ATTEMPTS: usize = 4;

#[derive(Debug)]
pub(crate) struct GrokBotAccountChanged;

impl std::fmt::Display for GrokBotAccountChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Grok Bot 当前账户已切换，请重新搜索")
    }
}

impl std::error::Error for GrokBotAccountChanged {}

pub(crate) struct GrokBotSearchRoster {
    scope: Option<String>,
    prefix: String,
    account_stamp: Option<crate::file_stamp::FileStamp>,
    roster_stamp: Option<crate::file_stamp::FileStamp>,
    roster_shortcut_valid: bool,
    pub(crate) thread_ids: std::collections::HashSet<String>,
}

type GrokBotStampPair = (
    Option<crate::file_stamp::FileStamp>,
    Option<crate::file_stamp::FileStamp>,
);

enum VerifiedGrokBotRoster {
    NoAccount {
        account_stamp: Option<crate::file_stamp::FileStamp>,
    },
    Active {
        scope: String,
        roster_val: Value,
        account_stamp: Option<crate::file_stamp::FileStamp>,
        roster_stamp: Option<crate::file_stamp::FileStamp>,
    },
}

#[cfg(test)]
type GrokBotTestHookSlot = std::cell::RefCell<Option<Box<dyn FnMut()>>>;

#[cfg(test)]
thread_local! {
    static TEST_MID_ROSTER_READ_HOOK: GrokBotTestHookSlot = std::cell::RefCell::new(None);
    static TEST_MID_SEARCH_LOAD_HOOK: GrokBotTestHookSlot = std::cell::RefCell::new(None);
}

#[cfg(test)]
fn run_test_hook(slot: &'static std::thread::LocalKey<GrokBotTestHookSlot>) {
    let hook = slot.with(|cell| cell.borrow_mut().take());
    if let Some(mut f) = hook {
        f();
        slot.with(|cell| *cell.borrow_mut() = Some(f));
    }
}

fn account_slot_stamp(paths: &GrokBotPaths) -> Option<crate::file_stamp::FileStamp> {
    crate::file_stamp::FileStamp::of_nofollow(&slice_path(
        &paths.persistence,
        &format!("{PREFIX}client-meta.account-slot"),
    ))
}

fn account_and_roster_stamps(paths: &GrokBotPaths, scope: &str) -> GrokBotStampPair {
    let roster_path = slice_path(&paths.persistence, &key(scope, "roster.last-roster"));
    (
        account_slot_stamp(paths),
        crate::file_stamp::FileStamp::of_nofollow(&roster_path),
    )
}

fn read_verified_roster(paths: &GrokBotPaths) -> Result<VerifiedGrokBotRoster> {
    for _ in 0..GROKBOT_VERIFY_ATTEMPTS {
        let acc_before = account_slot_stamp(paths);
        let Some(scope) = account(paths)? else {
            #[cfg(test)]
            run_test_hook(&TEST_MID_ROSTER_READ_HOOK);
            if account_slot_stamp(paths) == acc_before && account(paths)?.is_none() {
                return Ok(VerifiedGrokBotRoster::NoAccount {
                    account_stamp: acc_before,
                });
            }
            continue;
        };
        let roster_path = slice_path(&paths.persistence, &key(&scope, "roster.last-roster"));
        let ros_before = crate::file_stamp::FileStamp::of_nofollow(&roster_path);
        let roster_val = read_slice(&paths.persistence, &key(&scope, "roster.last-roster"), 4)?
            .unwrap_or_else(|| serde_json::json!({"rows":[]}));
        #[cfg(test)]
        run_test_hook(&TEST_MID_ROSTER_READ_HOOK);
        let after = account_and_roster_stamps(paths, &scope);
        if after == (acc_before, ros_before) && account(paths)?.as_deref() == Some(scope.as_str()) {
            return Ok(VerifiedGrokBotRoster::Active {
                scope,
                roster_val,
                account_stamp: after.0,
                roster_stamp: after.1,
            });
        }
    }
    anyhow::bail!("Grok Bot 账户或会话列表正在变更，请稍后重试")
}

pub(crate) fn prepare_search(
    paths: &GrokBotPaths,
    thread_session_key: Option<&str>,
    max_sessions: usize,
) -> Result<(Vec<ProviderSessionSummary>, GrokBotSearchRoster)> {
    if let Some(session) = thread_session_key {
        let VerifiedGrokBotRoster::Active {
            scope,
            roster_val,
            account_stamp,
            roster_stamp,
        } = read_verified_roster(paths)?
        else {
            anyhow::bail!("Grok Bot 当前账户不可用");
        };
        let prefix = format!("{}:", hex::encode(Sha256::digest(&scope)));
        let id = session
            .strip_prefix(&prefix)
            .context("Grok Bot 线程不在当前账户中")?;
        let row = roster_rows(&roster_val)?
            .iter()
            .find(|row| row["id"].as_str() == Some(id))
            .context("Grok Bot 线程不在当前账户中")?;
        let sum = summary_inner(paths, &scope, row, false)?;
        let mut thread_ids = std::collections::HashSet::with_capacity(1);
        thread_ids.insert(sum.id.clone());
        return Ok((
            vec![sum],
            GrokBotSearchRoster {
                scope: Some(scope),
                prefix,
                account_stamp,
                roster_stamp,
                roster_shortcut_valid: true,
                thread_ids,
            },
        ));
    }
    let (scope, roster_val, account_stamp, roster_stamp) = match read_verified_roster(paths)? {
        VerifiedGrokBotRoster::NoAccount { account_stamp } => {
            return Ok((
                vec![],
                GrokBotSearchRoster {
                    scope: None,
                    prefix: String::new(),
                    account_stamp,
                    roster_stamp: None,
                    roster_shortcut_valid: false,
                    thread_ids: std::collections::HashSet::new(),
                },
            ));
        }
        VerifiedGrokBotRoster::Active {
            scope,
            roster_val,
            account_stamp,
            roster_stamp,
        } => (scope, roster_val, account_stamp, roster_stamp),
    };
    let mut summaries = roster_rows(&roster_val)?
        .iter()
        .filter(|r| r["isRemovedFromSidebar"] != true)
        .map(|r| summary_inner(paths, &scope, r, false))
        .collect::<Result<Vec<_>>>()?;
    summaries.sort_by_cached_key(|r| {
        (
            std::cmp::Reverse(crate::timestamps::sort_key_nanos(r.updated_at.as_deref())),
            r.id.clone(),
        )
    });
    summaries.truncate(max_sessions);
    let thread_ids = summaries.iter().map(|s| s.id.clone()).collect();
    let prefix = format!("{}:", hex::encode(Sha256::digest(&scope)));
    Ok((
        summaries,
        GrokBotSearchRoster {
            scope: Some(scope),
            prefix,
            account_stamp,
            roster_stamp,
            roster_shortcut_valid: true,
            thread_ids,
        },
    ))
}

pub(crate) fn verify_search_scope_unchanged(
    paths: &GrokBotPaths,
    state: &GrokBotSearchRoster,
) -> Result<bool> {
    if account_slot_stamp(paths) != state.account_stamp {
        return Ok(false);
    }
    if account(paths)?.as_deref() != state.scope.as_deref() {
        return Ok(false);
    }
    Ok(account_slot_stamp(paths) == state.account_stamp)
}

pub(crate) fn load_all_events_for_search(
    paths: &GrokBotPaths,
    state: &mut GrokBotSearchRoster,
    session: &str,
) -> Result<Arc<Vec<ProviderHistoryEvent>>> {
    #[cfg(test)]
    run_test_hook(&TEST_MID_SEARCH_LOAD_HOOK);
    let Some(scope) = state.scope.as_deref() else {
        return Err(GrokBotAccountChanged.into());
    };
    if state.roster_shortcut_valid {
        let current_stamps = account_and_roster_stamps(paths, scope);
        if current_stamps == (state.account_stamp, state.roster_stamp) {
            if let Some(id) = session.strip_prefix(&state.prefix) {
                if state.thread_ids.contains(id) {
                    let res = snapshot_for_id(paths, scope, id).map(|snap| snap.events.clone());
                    if !verify_search_scope_unchanged(paths, state)? {
                        return Err(GrokBotAccountChanged.into());
                    }
                    return res;
                }
            }
        }
        state.roster_shortcut_valid = false;
    }
    if !verify_search_scope_unchanged(paths, state)? {
        return Err(GrokBotAccountChanged.into());
    }
    let res = load_all_events(paths, session);
    if !verify_search_scope_unchanged(paths, state)? {
        return Err(GrokBotAccountChanged.into());
    }
    res
}

pub fn load_all_events(
    paths: &GrokBotPaths,
    session: &str,
) -> Result<Arc<Vec<ProviderHistoryEvent>>> {
    Ok(snapshot(paths, session)?.events.clone())
}
fn snapshot(paths: &GrokBotPaths, session: &str) -> Result<Arc<ProviderHistorySnapshot>> {
    let (scope, row) = locate(paths, session)?;
    let id = row["id"].as_str().context("Grok Bot 线程身份缺失")?;
    snapshot_for_id(paths, &scope, id)
}
fn snapshot_for_id(
    paths: &GrokBotPaths,
    scope: &str,
    id: &str,
) -> Result<Arc<ProviderHistorySnapshot>> {
    let k = replica_key(scope, id);
    let path = slice_path(&paths.persistence, &k);
    provider_sessions::HISTORY_CACHE.read(&path, provider_sessions::history_weight, || {
        let value = read_slice(&paths.persistence, &k, 1)?;
        if let Some(value) = &value {
            ensure!(value["entries"].is_array(), "Grok Bot 线程缓存格式错误");
        }
        Ok(ProviderHistorySnapshot::new(
            value.as_ref().map(events).unwrap_or_default(),
        ))
    })
}
pub fn detail(
    paths: &GrokBotPaths,
    request: &ProviderDetailRequest,
) -> Result<ProviderSessionDetail> {
    anyhow::ensure!(request.agent_id.is_none(), "Grok Bot 不支持子智能体详情");
    let (scope, row) = locate(paths, &request.session_key)?;
    let mut summary = summary(paths, &scope, &row)?;
    let replica_path = slice_path(&paths.persistence, &replica_key(&scope, &summary.id));
    let replica_summary =
        read_replica_interaction_summary(paths, &scope, &summary.id, &replica_path)?;
    summary.status = derive_session_status(&row, replica_summary.as_deref()).into();
    let snap = snapshot_for_id(paths, &scope, &summary.id)?;
    provider_sessions::paginate_provider_detail(summary, snap.as_ref(), request)
}
fn pending_events(row: &Value, replica: Option<&Value>) -> Vec<NativeTurnEvent> {
    let mut pending = Vec::new();
    for entry in replica
        .and_then(|v| v["entries"].as_array())
        .into_iter()
        .flatten()
    {
        if let Some((kind, request, body)) = interaction(entry) {
            if let Some(ts) = entry["timestampMs"].as_i64().filter(|t| *t > 0) {
                pending.push(NativeTurnEvent {
                    prompt_id: Some(request.clone()),
                    position: ts as u64,
                    turn_id: request,
                    kind: kind.into(),
                    body: if body.is_empty() {
                        if kind == "authorization" {
                            "需要授权，请打开 Grok Bot 处理。".into()
                        } else {
                            "需要回复，请打开 Grok Bot 处理。".into()
                        }
                    } else {
                        body
                    },
                    timestamp_ms: ts,
                });
            }
        }
    }
    let awaiting = &row["awaitingUserResponse"];
    if let (Some(request), Some(ts)) = (
        awaiting["id"].as_str(),
        row["lastActivityAt"].as_i64().filter(|t| *t > 0),
    ) {
        let matching_entry = replica
            .and_then(|v| v["entries"].as_array())
            .and_then(|entries| {
                entries.iter().find(|e| {
                    e["id"] == request
                        || e["requestId"] == request
                        || e["message"]["ask"]["requestId"] == request
                        || e["message"]["approval"]["requestId"] == request
                })
            });
        if matching_entry.is_none() && !pending.iter().any(|e| e.turn_id == request) {
            pending.push(NativeTurnEvent {
                prompt_id: Some(request.into()),
                position: ts as u64,
                turn_id: request.into(),
                kind: "reply_needed".into(),
                body: text(awaiting),
                timestamp_ms: ts,
            });
        }
    }
    pending
}
fn completion_from_entry(entry: &Value, fallback_ts: Option<i64>) -> Option<NativeTurnEvent> {
    if entry["kind"].as_str() != Some("send-message")
        || entry["draftSendState"].as_str() == Some("failed")
    {
        return None;
    }
    let id = entry["id"].as_str().filter(|s| !s.trim().is_empty())?;
    let ts = entry["timestampMs"]
        .as_i64()
        .or(fallback_ts)
        .filter(|&t| t > 0)?;
    let body = if entry["message"].is_object() {
        let msg = &entry["message"];
        match msg["type"].as_str() {
            Some("text") => {
                let s = text(&msg["content"]);
                if s.trim().is_empty() {
                    text(&entry["text"])
                } else {
                    s
                }
            }
            Some("attachment") => msg["file_name"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| msg["alt"].as_str().filter(|s| !s.trim().is_empty()))
                .unwrap_or("附件")
                .to_owned(),
            _ => return None,
        }
    } else if entry.get("respondedValue").is_some_and(|v| !v.is_null())
        || entry["widgetDismissed"] == true
        || entry.get("formResolution").is_some_and(|v| !v.is_null())
    {
        return None;
    } else {
        text(&entry["text"])
    };
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(NativeTurnEvent {
        prompt_id: None,
        position: ts as u64,
        turn_id: id.to_owned(),
        kind: "completion".into(),
        body: crate::security::redact_output(trimmed)
            .chars()
            .take(1200)
            .collect(),
        timestamp_ms: ts,
    })
}

fn is_interaction_entry(entry: &Value) -> bool {
    entry["kind"].as_str() == Some("send-message")
        && matches!(
            entry["message"]["type"].as_str(),
            Some(
                "local-tool-permission"
                    | "auto-review-approval"
                    | "meeting-join-consent"
                    | "cookie-origin-approval"
                    | "virtual-card-approval"
                    | "user-form"
            )
        )
}

fn latest_completion_event(
    row: &Value,
    replica: Option<&Value>,
    pending: &[NativeTurnEvent],
) -> (u64, Option<NativeTurnEvent>) {
    let mut latest: Option<NativeTurnEvent> = None;
    let mut latest_interaction_ms: Option<i64> = None;
    for entry in replica
        .and_then(|v| v["entries"].as_array())
        .into_iter()
        .flatten()
    {
        if is_interaction_entry(entry) {
            if let Some(ts) = entry["timestampMs"].as_i64().filter(|&t| t > 0) {
                latest_interaction_ms = Some(latest_interaction_ms.map_or(ts, |prev| prev.max(ts)));
            }
        }
        if let Some(candidate) = completion_from_entry(entry, None) {
            if latest
                .as_ref()
                .is_none_or(|prev| candidate.timestamp_ms >= prev.timestamp_ms)
            {
                latest = Some(candidate);
            }
        }
    }
    if let Some(roster_last) =
        completion_from_entry(&row["lastEntry"], row["lastActivityAt"].as_i64())
    {
        if latest
            .as_ref()
            .is_none_or(|prev| roster_last.timestamp_ms > prev.timestamp_ms)
        {
            latest = Some(roster_last);
        }
    }
    let Some(candidate) = latest else {
        return (0, None);
    };
    let settled_count = candidate.position;
    if pending
        .iter()
        .any(|p| p.timestamp_ms >= candidate.timestamp_ms)
        || latest_interaction_ms.is_some_and(|ts| ts >= candidate.timestamp_ms)
    {
        return (settled_count, None);
    }
    (settled_count, Some(candidate))
}

pub fn scan(paths: &GrokBotPaths) -> Result<NativeScan> {
    scan_selected(paths, None)
}

pub fn scan_selected(paths: &GrokBotPaths, selected: Option<&str>) -> Result<NativeScan> {
    let Some((scope, roster)) = roster(paths)? else {
        return Ok(NativeScan::default());
    };
    let mut scan = NativeScan::default();
    for row in roster_rows(&roster)? {
        let Some(id) = row["id"].as_str() else {
            scan.errors += 1;
            continue;
        };
        if selected.is_some_and(|selected| selected != id) {
            continue;
        };
        let replica = match read_slice(&paths.persistence, &replica_key(&scope, id), 1) {
            Ok(v) => v,
            Err(_) => {
                scan.errors += 1;
                continue;
            }
        };
        if replica
            .as_ref()
            .is_some_and(|value| !value["entries"].is_array())
        {
            scan.errors += 1;
            continue;
        }
        let mut events = pending_events(row, replica.as_ref());
        let (settled_count, completion) = latest_completion_event(row, replica.as_ref(), &events);
        if let Some(comp) = completion {
            events.push(comp);
        }
        events.sort_by_key(|e| e.position);
        let count = events
            .iter()
            .map(|e| e.position)
            .max()
            .unwrap_or(settled_count)
            .max(settled_count);
        let session_key = session_key(&scope, id);
        scan.streams.push(NativeStreamSnapshot {
            provider: NativeProvider::GrokBot,
            session_key: session_key.clone(),
            id: id.into(),
            title: row_title(row),
            identity: session_key,
            record_count: count,
            settled_count,
            events: events.into(),
        });
    }
    Ok(scan)
}
#[cfg(test)]
mod tests;
