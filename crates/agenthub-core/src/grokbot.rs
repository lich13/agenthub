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
static SLICES: crate::read_cache::ReadCache<Arc<Value>> =
    crate::read_cache::ReadCache::new(crate::resource_limits::NATIVE_PROBE_CACHE_BYTES)
        .with_native_metadata();

#[derive(Debug, Clone)]
pub struct GrokBotPaths {
    pub persistence: PathBuf,
}
impl GrokBotPaths {
    pub fn default_for_user() -> Self {
        Self {
            persistence: dirs::home_dir()
                .unwrap_or_default()
                .join("Library/Application Support/Grok Bot/sand-client-persistence"),
        }
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
fn read_slice(root: &Path, key: &str, schema: u64) -> Result<Option<Arc<Value>>> {
    let path = slice_path(root, key);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(m) => ensure!(
            m.is_file() && !m.file_type().is_symlink(),
            "Grok Bot 缓存不是普通文件"
        ),
    }
    SLICES
        .read(
            &path,
            |_, size| size.saturating_mul(3),
            || {
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
                let mut envelope: Value =
                    serde_json::from_str(body).context("Grok Bot 缓存格式错误")?;
                ensure!(
                    envelope["schemaVersion"].as_u64() == Some(schema),
                    "Grok Bot 缓存格式暂不支持"
                );
                let value = envelope
                    .get_mut("value")
                    .context("Grok Bot 缓存内容缺失")?
                    .take();
                Ok(Arc::new(value))
            },
        )
        .map(Some)
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
fn roster(paths: &GrokBotPaths) -> Result<Option<(String, Arc<Value>)>> {
    let Some(scope) = account(paths)? else {
        return Ok(None);
    };
    let value = read_slice(&paths.persistence, &key(&scope, "roster.last-roster"), 4)?
        .unwrap_or_else(|| Arc::new(serde_json::json!({"rows":[]})));
    ensure!(value["rows"].is_array(), "Grok Bot 列表格式错误");
    Ok(Some((scope, value)))
}
fn session_key(scope: &str, id: &str) -> String {
    format!("{}:{id}", hex::encode(Sha256::digest(scope)))
}
fn locate(paths: &GrokBotPaths, session: &str) -> Result<(String, Value)> {
    let (scope, roster) = roster(paths)?.context("Grok Bot 当前账户不可用")?;
    let row = roster["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["id"]
                .as_str()
                .is_some_and(|id| session_key(&scope, id) == session)
        })
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
    chrono::DateTime::from_timestamp_millis(ms).map(|t| t.to_rfc3339())
}
fn summary(paths: &GrokBotPaths, scope: &str, row: &Value) -> Result<ProviderSessionSummary> {
    let id = row["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Grok Bot 线程身份缺失")?;
    let path = slice_path(&paths.persistence, &replica_key(scope, id));
    let available =
        fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink());
    let pending = row.get("awaitingUserResponse").filter(|v| !v.is_null());
    Ok(ProviderSessionSummary {
        storage_size: available
            .then(|| crate::session_storage::file_size(&paths.persistence, &path)),
        id: id.into(),
        session_key: session_key(scope, id),
        title: row["title"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .or(row["name"].as_str())
            .unwrap_or("Grok Bot")
            .into(),
        cwd: String::new(),
        path: if available { path } else { PathBuf::new() },
        archived: false,
        updated_at: row["lastActivityAt"]
            .as_i64()
            .or(row["updatedAt"].as_i64())
            .and_then(timestamp),
        message_count: 0,
        last_message: row["lastEntry"]["text"].as_str().map(str::to_owned),
        status: if pending.is_some() {
            "reply_needed"
        } else {
            "unknown"
        }
        .into(),
        format_version: if available {
            "近期缓存"
        } else {
            "未缓存"
        }
        .into(),
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
    let Some((scope, roster)) = roster(paths)? else {
        return Ok(vec![]);
    };
    let mut rows = roster["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["isRemovedFromSidebar"] != true)
        .map(|r| summary(paths, &scope, r))
        .collect::<Result<Vec<_>>>()?;
    rows.retain(|r| provider_sessions::matches_query(r, request.q.as_deref()));
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
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
                    || question["deadlineAtMs"]
                        .as_i64()
                        .is_some_and(|deadline| deadline <= chrono::Utc::now().timestamp_millis()))
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
pub fn load_all_events(
    paths: &GrokBotPaths,
    session: &str,
) -> Result<Arc<Vec<ProviderHistoryEvent>>> {
    Ok(snapshot(paths, session)?.events.clone())
}
fn snapshot(paths: &GrokBotPaths, session: &str) -> Result<Arc<ProviderHistorySnapshot>> {
    let (scope, row) = locate(paths, session)?;
    let id = row["id"].as_str().context("Grok Bot 线程身份缺失")?;
    let k = replica_key(&scope, id);
    let path = slice_path(&paths.persistence, &k);
    provider_sessions::HISTORY_CACHE.read(&path, provider_sessions::history_weight, || {
        let value = read_slice(&paths.persistence, &k, 1)?;
        if let Some(value) = &value {
            ensure!(value["entries"].is_array(), "Grok Bot 线程缓存格式错误");
        }
        Ok(ProviderHistorySnapshot::new(
            value.as_deref().map(events).unwrap_or_default(),
        ))
    })
}
pub fn detail(
    paths: &GrokBotPaths,
    request: &ProviderDetailRequest,
) -> Result<ProviderSessionDetail> {
    let mut summary = session_summary(paths, &request.session_key)?;
    let (scope, row) = locate(paths, &request.session_key)?;
    let replica = read_slice(&paths.persistence, &replica_key(&scope, &summary.id), 1)?;
    summary.status = pending_events(&row, replica.as_deref())
        .first()
        .map(|event| event.kind.as_str())
        .unwrap_or("unknown")
        .into();
    provider_sessions::paginate_provider_detail(
        summary,
        snapshot(paths, &request.session_key)?.as_ref(),
        request,
    )
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
        // A resolved interaction is stronger evidence than a lagging roster.
        let resolved = matching_entry.is_some_and(|e| {
            e["kind"] == "send-message"
                && (e.get("respondedValue").is_some_and(|v| !v.is_null())
                    || e["widgetDismissed"] == true
                    || e.get("formResolution").is_some_and(|v| !v.is_null())
                    || (e["message"]["type"] == "user-form"
                        && (e["message"]["phoneCallQuestion"].get("closed").is_some()
                            || e["message"]["phoneCallQuestion"]["deadlineAtMs"]
                                .as_i64()
                                .is_some_and(|deadline| {
                                    deadline <= chrono::Utc::now().timestamp_millis()
                                })))
                    || e["message"]["ask"]["status"]
                        .as_str()
                        .is_some_and(|s| s != "pending")
                    || e["message"]["approval"]["status"]
                        .as_str()
                        .is_some_and(|s| s != "pending"))
        });
        let covered = matching_entry.and_then(interaction).is_some();
        if !resolved && !covered && !pending.iter().any(|e| e.turn_id == request) {
            let ts = matching_entry
                .and_then(|e| e["timestampMs"].as_i64())
                .unwrap_or(ts);
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
pub fn scan(paths: &GrokBotPaths) -> Result<NativeScan> {
    let Some((scope, roster)) = roster(paths)? else {
        return Ok(NativeScan::default());
    };
    let mut scan = NativeScan::default();
    for row in roster["rows"].as_array().unwrap() {
        let Some(id) = row["id"].as_str() else {
            scan.errors += 1;
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
        let pending = pending_events(row, replica.as_deref());
        let count = pending.iter().map(|e| e.position).max().unwrap_or(0);
        scan.streams.push(NativeStreamSnapshot {
            provider: NativeProvider::GrokBot,
            session_key: session_key(&scope, id),
            id: id.into(),
            title: summary(paths, &scope, row)?.title,
            identity: session_key(&scope, id),
            record_count: count,
            settled_count: 0,
            events: pending,
        });
    }
    Ok(scan)
}
pub fn evict_idle(ttl: std::time::Duration) {
    SLICES.evict_idle(ttl)
}
#[cfg(test)]
mod tests;
