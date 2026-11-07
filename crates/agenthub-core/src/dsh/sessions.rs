//! DeepSeek Harness (DSH) native session listing, reading, and 5-action mutations.
//! Backed by `$DSH_HOME/storages/workspace.json` (v2), `storages/session_projcache`,
//! and `sessions/<project>/<session>/session.v{1..4}.jsonl[.zstd]`.
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    directory, dsh_command, file_stamp, read_bounded, registry, regular_file, remove_cache,
    remove_dsh_cache_strict, remove_dsh_session_strict, remove_references, save_registry,
    standard_layout, valid_id, DshPaths, HEADER_LIMIT,
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

const LOG_FILE_LIMIT: u64 = 128 * 1024 * 1024;
const PROJCACHE_LIMIT: u64 = 4 * 1024 * 1024;
pub const MAX_DSH_TITLE_BYTES: usize = 80;

#[derive(Debug, Clone)]
struct DiscoveredSession {
    id: String,
    project_key: String,
    dir: PathBuf,
    log_path: PathBuf,
    version: u32,
    compressed: bool,
    header: Value,
    parent_subagent_of: Option<String>,
}

impl DiscoveredSession {
    fn is_subagent(&self) -> bool {
        self.parent_subagent_of.is_some()
            || self.header["origin"].as_str() == Some("subagent")
            || self.header["delegationDepth"].as_u64().unwrap_or(0) > 0
    }

    fn is_internal_no_cwd(&self) -> bool {
        self.project_key == "_no-cwd" && self.header.get("cwd").and_then(Value::as_str).is_none()
    }
}

fn check_host_dsh_stopped_for(paths: &DshPaths) -> Result<()> {
    let default_home = DshPaths::default_for_user().home;
    let same_as_default = paths.home == default_home
        || (paths.home.canonicalize().ok().is_some()
            && paths.home.canonicalize().ok() == default_home.canonicalize().ok());
    if !same_as_default {
        return Ok(());
    }
    let output = std::process::Command::new("ps")
        .args(["-axo", "command="])
        .output()
        .context("无法核验 DeepSeek Harness 运行状态")?;
    ensure!(
        output.status.success(),
        "无法核验 DeepSeek Harness 运行状态"
    );
    let text = std::str::from_utf8(&output.stdout).context("无法核验 DeepSeek Harness 运行状态")?;
    ensure!(
        !text.lines().any(dsh_command),
        "DeepSeek Harness 正在运行，请先退出后再执行归档、恢复或删除操作"
    );
    Ok(())
}

struct ExclusiveSessionLease {
    file: File,
}

impl Drop for ExclusiveSessionLease {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            unsafe {
                let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

fn try_acquire_session_lease(session_dir: &Path) -> Result<Option<ExclusiveSessionLease>> {
    let lock_path = session_dir.join("session.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(&lock_path)
        .context("无法打开 DSH 会话锁文件")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::io::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if matches!(
                err.raw_os_error(),
                Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
            ) {
                return Ok(None);
            }
            return Err(err).context("无法校验 DSH 会话锁状态");
        }
        // Verify inode stability after acquiring flock (matches DSH SessionWriteLease)
        let fd_meta = file.metadata()?;
        let path_meta = fs::symlink_metadata(&lock_path)?;
        if (fd_meta.dev(), fd_meta.ino()) != (path_meta.dev(), path_meta.ino()) {
            unsafe {
                let _ = libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
            return Ok(None);
        }
    }
    Ok(Some(ExclusiveSessionLease { file }))
}

fn is_session_locked_readonly(session_dir: &Path) -> bool {
    let lock_path = session_dir.join("session.lock");
    if !lock_path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;
        let Ok(file) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
        else {
            return false;
        };
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            unsafe {
                let _ = libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
            false
        } else {
            let err = std::io::Error::last_os_error();
            matches!(
                err.raw_os_error(),
                Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
            )
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn pick_generation(session_dir: &Path) -> Result<Option<(u32, bool, PathBuf)>> {
    let mut generations = Vec::new();
    for entry in fs::read_dir(session_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(version) = name
            .strip_prefix("session.v")
            .and_then(|n| n.split('.').next())
            .and_then(|v| v.parse::<u32>().ok())
        {
            if name == format!("session.v{version}.jsonl")
                || name == format!("session.v{version}.jsonl.zstd")
            {
                generations.push((version, name.ends_with(".zstd"), entry.path()));
            }
        }
    }
    generations.sort_by_key(|g| (g.0, !g.1));
    Ok(generations.pop().filter(|(v, _, _)| (1..=4).contains(v)))
}

fn read_header_json(path: &Path, compressed: bool, expected_id: &str) -> Result<Value> {
    let file = regular_file(path)?;
    let reader: Box<dyn Read> = if compressed {
        Box::new(zstd::stream::read::Decoder::new(file)?)
    } else {
        Box::new(file)
    };
    let mut line = Vec::new();
    BufReader::new(reader.take(HEADER_LIMIT + 1)).read_until(b'\n', &mut line)?;
    ensure!(
        !line.is_empty() && line.len() as u64 <= HEADER_LIMIT,
        "DSH 会话头不完整或过大"
    );
    let value: Value = serde_json::from_slice(&line).context("DSH 会话头损坏")?;
    ensure!(
        value["type"] == "session" && value["id"].as_str() == Some(expected_id),
        "不匹配的 DSH 会话头身份"
    );
    Ok(value)
}

fn discover_all_sessions(paths: &DshPaths) -> Result<Vec<DiscoveredSession>> {
    ensure!(paths.home.is_absolute(), "DSH_HOME 必须是绝对路径");
    if !directory(&paths.home)? {
        return Ok(Vec::new());
    }
    standard_layout(paths)?;
    let sessions_root = paths.home.join("sessions");
    if !directory(&sessions_root)? {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for project in fs::read_dir(&sessions_root)? {
        let project = project?;
        let ptype = project.file_type()?;
        if ptype.is_symlink() || !ptype.is_dir() {
            continue;
        }
        let project_key = project.file_name().to_string_lossy().into_owned();
        if project_key == "resume-pins" {
            continue;
        }
        for entry in fs::read_dir(project.path())? {
            let entry = entry?;
            let etype = entry.file_type()?;
            if etype.is_symlink() || !etype.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if valid_id(&id).is_err() {
                continue;
            }
            if let Ok(Some(session)) = discover_session(entry.path(), project_key.clone(), &id) {
                out.push(session);
            }
        }
    }
    Ok(out)
}

fn discover_session(
    dir: PathBuf,
    project_key: String,
    id: &str,
) -> Result<Option<DiscoveredSession>> {
    let Some((version, compressed, log_path)) = pick_generation(&dir)? else {
        return Ok(None);
    };
    let header = read_header_json(&log_path, compressed, id)?;
    let parent_subagent_of = if header["origin"].as_str() == Some("subagent") {
        header["parentSession"]
            .as_str()
            .filter(|p| valid_id(p).is_ok())
            .map(str::to_owned)
    } else {
        None
    };
    Ok(Some(DiscoveredSession {
        id: id.into(),
        project_key,
        dir,
        log_path,
        version,
        compressed,
        header,
        parent_subagent_of,
    }))
}

fn decode_log_lines(
    path: &Path,
    compressed: bool,
    on_line: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    decode_log_lines_checked(path, compressed, false, on_line).map(|_| ())
}

// Append requires a complete source. Readers may still show already completed
// frames while DSH is writing its next frame.
fn decode_log_lines_checked(
    path: &Path,
    compressed: bool,
    for_append: bool,
    mut on_line: impl FnMut(&[u8]) -> Result<()>,
) -> Result<bool> {
    let file = regular_file(path)?;
    let mut bytes = Vec::new();
    file.take(LOG_FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= LOG_FILE_LIMIT,
        "DSH 会话日志超出读取上限"
    );

    let mut needs_newline = false;
    if compressed {
        let mut consumed = 0usize;
        let mut decoded_total = 0usize;
        while consumed < bytes.len() {
            let tail = &bytes[consumed..];
            let magic = [0x28, 0xb5, 0x2f, 0xfd];
            if tail.len() < 4 || tail[..4] != magic {
                ensure!(!for_append, "DSH Zstandard 尾帧不完整，已拒绝改名");
                break;
            }
            let size = match zstd::zstd_safe::find_frame_compressed_size(tail) {
                Ok(size) => size,
                Err(error) if zstd::zstd_safe::get_error_name(error) == "Src size is incorrect" => {
                    ensure!(!for_append, "DSH Zstandard 尾帧不完整，已拒绝改名");
                    break;
                }
                Err(_) => bail!("DSH Zstandard 帧损坏"),
            };
            ensure!(size > 0 && size <= tail.len(), "DSH Zstandard 帧长度无效");
            let mut decoded = Vec::new();
            zstd::stream::read::Decoder::new(&tail[..size])?
                .take(LOG_FILE_LIMIT + 1)
                .read_to_end(&mut decoded)?;
            decoded_total = decoded_total.saturating_add(decoded.len());
            ensure!(
                decoded_total as u64 <= LOG_FILE_LIMIT,
                "DSH 解压后会话日志超出读取上限"
            );
            for raw in decoded.split(|&b| b == b'\n') {
                if !raw.iter().all(u8::is_ascii_whitespace) {
                    on_line(raw)?;
                }
            }
            if let Some(last) = decoded.last() {
                needs_newline = *last != b'\n';
            }
            consumed += size;
        }
    } else {
        needs_newline = bytes.last().is_some_and(|last| *last != b'\n');
        for raw in bytes.split(|&b| b == b'\n') {
            if !raw.iter().all(u8::is_ascii_whitespace) {
                on_line(raw)?;
            }
        }
    }
    Ok(needs_newline)
}

fn epoch_ms_to_rfc3339(ms: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms).map(|dt| dt.to_rfc3339())
}

fn value_timestamp_rfc3339(val: &Value) -> Option<String> {
    if let Some(ms) = val.as_i64() {
        return epoch_ms_to_rfc3339(ms);
    }
    if let Some(s) = val.as_str().filter(|s| !s.trim().is_empty()) {
        return Some(s.to_owned());
    }
    None
}

#[derive(Default, Clone)]
struct FoldedMetadata {
    title: Option<String>,
    first_prompt: Option<String>,
    last_message: Option<String>,
    cwd: Option<String>,
    updated_at_ms: Option<i64>,
    message_count: usize,
    open_turn: bool,
    last_seq: Option<u64>,
}

static DSH_FOLD_CACHE: crate::read_cache::ReadCache<FoldedMetadata> =
    crate::read_cache::ReadCache::new(crate::resource_limits::DSH_SUMMARY_CACHE_BYTES)
        .with_entry_limit(crate::resource_limits::DSH_SUMMARY_ENTRIES)
        .with_native_metadata();

pub(crate) fn evict_idle_fold_cache(max_idle: std::time::Duration) {
    DSH_FOLD_CACHE.evict_idle(max_idle);
}

fn extract_content_plain_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_owned();
    }
    if let Some(arr) = content.as_array() {
        let mut chunks = Vec::new();
        for item in arr {
            if item["type"].as_str() == Some("text") {
                if let Some(t) = item["text"].as_str().filter(|s| !s.is_empty()) {
                    chunks.push(t);
                }
            }
        }
        return chunks.join("\n");
    }
    String::new()
}

fn try_read_projcache(paths: &DshPaths, session: &DiscoveredSession) -> Option<FoldedMetadata> {
    let cache_path = paths
        .home
        .join("storages/session_projcache/sessions")
        .join(format!("{}.json", session.id));
    let cache_meta = fs::symlink_metadata(&cache_path).ok()?;
    if !cache_meta.is_file() || cache_meta.file_type().is_symlink() {
        return None;
    }
    let log_meta = fs::symlink_metadata(&session.log_path).ok()?;
    if cache_meta.modified().ok()? < log_meta.modified().ok()? {
        return None;
    }
    let bytes = read_bounded(&cache_path, PROJCACHE_LIMIT).ok()?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    let ver = json["version"].as_u64()?;
    if !(3..=7).contains(&ver) {
        return None;
    }
    let record = json.get("record")?;
    if record["identity"]["createdAt"] != session.header["createdAt"] {
        return None;
    }
    let rows = record.get("rows")?;
    let title = rows["title"]["val"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let cwd = rows["workingDirectory"]["val"]
        .as_str()
        .or_else(|| record["identity"]["cwd"].as_str())
        .map(str::to_owned);
    let updated_at_ms = rows["sessionListMetadata"]["val"]["lastPromptAt"]
        .as_i64()
        .or_else(|| session.header["createdAt"].as_i64());
    let outline = rows["turnOutline"]["val"].as_array();
    let message_count = outline.map_or(0, |arr| arr.len() * 2);
    let last_message = outline
        .and_then(|arr| arr.last())
        .and_then(|item| {
            item["prompt"]
                .as_str()
                .or_else(|| item["title"].as_str())
                .or_else(|| item["summary"].as_str())
        })
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned);
    Some(FoldedMetadata {
        title,
        first_prompt: last_message.clone(),
        last_message,
        cwd,
        updated_at_ms,
        message_count,
        open_turn: false,
        last_seq: None,
    })
}

fn fold_log_metadata(session: &DiscoveredSession) -> Result<FoldedMetadata> {
    DSH_FOLD_CACHE.read(
        &session.log_path,
        |meta, _| {
            (std::mem::size_of::<FoldedMetadata>()
                + [
                    &meta.title,
                    &meta.first_prompt,
                    &meta.last_message,
                    &meta.cwd,
                ]
                .iter()
                .map(|value| value.as_ref().map_or(0, String::capacity))
                .sum::<usize>()) as u64
        },
        || fold_log_metadata_uncached(session),
    )
}

fn fold_log_metadata_uncached(session: &DiscoveredSession) -> Result<FoldedMetadata> {
    let inherited_count = if session.header["isSeeded"] == true {
        session.header["inheritedEventCount"].as_u64().unwrap_or(0)
    } else {
        0
    };
    let mut meta = FoldedMetadata {
        cwd: session.header["cwd"].as_str().map(str::to_owned),
        updated_at_ms: session.header["createdAt"].as_i64(),
        ..Default::default()
    };
    let mut line_index = 0usize;
    decode_log_lines(&session.log_path, session.compressed, |line| {
        if line_index == 0 {
            line_index += 1;
            return Ok(());
        }
        line_index += 1;
        let Ok(row) = serde_json::from_slice::<Value>(line) else {
            return Ok(());
        };
        let seq = row["seq"].as_u64();
        if let Some(seq) = seq {
            meta.last_seq = Some(seq);
        }
        let is_inherited = seq.is_some_and(|s| s < inherited_count);
        if let Some(ts) = row["time"].as_i64() {
            meta.updated_at_ms = Some(meta.updated_at_ms.map_or(ts, |prev| prev.max(ts)));
        }
        let kind = row["type"].as_str().unwrap_or("");
        let data = &row["data"];
        match kind {
            "session/title" => {
                if let Some(t) = data["title"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    meta.title = Some(t.to_owned());
                }
            }
            "working-directory/change" => {
                if let Some(c) = data["cwd"].as_str().filter(|s| !s.is_empty()) {
                    meta.cwd = Some(c.to_owned());
                }
            }
            "session/end-seed" => {
                meta.open_turn = false;
            }
            "turn/start" if !is_inherited => {
                meta.open_turn = true;
            }
            "turn/end" if !is_inherited => {
                meta.open_turn = false;
            }
            "user/message" => {
                let src = data["source"]["kind"].as_str().unwrap_or("user");
                if src == "user" {
                    let txt = extract_content_plain_text(&data["content"]);
                    let trimmed = txt.trim();
                    if !trimmed.is_empty() {
                        meta.message_count += 1;
                        if meta.first_prompt.is_none() {
                            meta.first_prompt = Some(trimmed.chars().take(120).collect());
                        }
                        meta.last_message = Some(trimmed.chars().take(200).collect());
                    }
                }
            }
            "assistant/message" => {
                let txt = extract_content_plain_text(&data["message"]["content"]);
                let trimmed = txt.trim();
                if !trimmed.is_empty() {
                    meta.message_count += 1;
                    meta.last_message = Some(trimmed.chars().take(200).collect());
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(meta)
}

fn build_summary(
    paths: &DshPaths,
    session: &DiscoveredSession,
    archived_set: &BTreeSet<String>,
) -> ProviderSessionSummary {
    let locked = is_session_locked_readonly(&session.dir);
    let folded = if locked {
        fold_log_metadata(session).unwrap_or_default()
    } else {
        try_read_projcache(paths, session)
            .or_else(|| fold_log_metadata(session).ok())
            .unwrap_or_default()
    };
    let archived = archived_set.contains(&session.id);
    let running = !archived && locked && folded.open_turn;
    let title = folded
        .title
        .or_else(|| {
            folded
                .first_prompt
                .as_deref()
                .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(60).collect())
        })
        .unwrap_or_else(|| "未命名会话".into());
    let cwd = folded
        .cwd
        .or_else(|| session.header["cwd"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let updated_at = folded
        .updated_at_ms
        .and_then(epoch_ms_to_rfc3339)
        .or_else(|| {
            fs::symlink_metadata(&session.log_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
        });
    let status = if archived {
        "archived"
    } else if running {
        "running"
    } else {
        "idle"
    };
    let can_rename = session.version == 4 && !locked;
    let rename_block_reason = if session.version != 4 {
        Some("旧版 DSH 会话格式（v1-v3）为只读，不支持改名".into())
    } else if locked {
        Some("该会话正在 DeepSeek Harness 中打开，请先关闭或切换会话后再改名".into())
    } else {
        None
    };
    let storage_size = if session.dir.is_dir() {
        Some(session_storage::directory_size(
            &paths.home,
            &session.dir,
            Some(&session.log_path),
            &format!("dsh:{}", session.id),
        ))
    } else {
        Some(SessionStorageSize::unavailable(StorageScope::Directory))
    };

    ProviderSessionSummary {
        storage_size,
        id: session.id.clone(),
        session_key: session.id.clone(),
        title,
        cwd,
        path: session.log_path.clone(),
        archived,
        updated_at,
        message_count: folded.message_count,
        last_message: folded.last_message,
        status: status.into(),
        format_version: format!("dsh-v{}", session.version),
        can_rename,
        rename_block_reason,
        can_archive: true,
        archive_block_reason: None,
        can_delete: !locked,
        delete_block_reason: locked
            .then(|| "该会话正在 DeepSeek Harness 中运行，请先退出后再删除".into()),
        read_error: None,
        read_warning: None,
    }
}

pub fn list_sessions(
    paths: &DshPaths,
    request: &ProviderListRequest,
) -> Result<Vec<ProviderSessionSummary>> {
    let reg = registry(paths)?;
    let archived_set = reg.map(|r| r.archived).unwrap_or_default();
    let discovered = discover_all_sessions(paths)?;
    let mut items: Vec<ProviderSessionSummary> = discovered
        .iter()
        .filter(|s| !s.is_subagent() && !s.is_internal_no_cwd())
        .map(|s| build_summary(paths, s, &archived_set))
        .filter(|item| match request.archived {
            Some(flag) => item.archived == flag,
            None => true,
        })
        .filter(|item| matches_query(item, request.q.as_deref()))
        .collect();

    items.sort_by(|a, b| {
        b.updated_at
            .as_deref()
            .unwrap_or("")
            .cmp(a.updated_at.as_deref().unwrap_or(""))
            .then_with(|| a.id.cmp(&b.id))
    });
    let limit = request.limit.unwrap_or(200).clamp(1, 1000);
    items.truncate(limit);
    Ok(items)
}

fn find_root_session(paths: &DshPaths, id: &str) -> Result<DiscoveredSession> {
    valid_id(id)?;
    ensure!(paths.home.is_absolute(), "DSH_HOME 必须是绝对路径");
    if directory(&paths.home)? {
        standard_layout(paths)?;
        let root = paths.home.join("sessions");
        if directory(&root)? {
            for project in fs::read_dir(root)? {
                let project = project?;
                let kind = project.file_type()?;
                let key = project.file_name().to_string_lossy().into_owned();
                if !kind.is_dir() || kind.is_symlink() || key == "resume-pins" {
                    continue;
                }
                let dir = project.path().join(id);
                let Ok(metadata) = fs::symlink_metadata(&dir) else {
                    continue;
                };
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    continue;
                }
                if let Ok(Some(session)) = discover_session(dir, key, id) {
                    if !session.is_subagent() {
                        return Ok(session);
                    }
                }
            }
        }
    }
    bail!("未找到 DeepSeek Harness 会话：{id}")
}

pub fn session_summary(paths: &DshPaths, id: &str) -> Result<ProviderSessionSummary> {
    let reg = registry(paths)?;
    let archived_set = reg.map(|r| r.archived).unwrap_or_default();
    let found = find_root_session(paths, id)?;
    Ok(build_summary(paths, &found, &archived_set))
}

fn resolve_dsh_attachment_parts(paths: &DshPaths, content: &Value) -> Value {
    let Some(arr) = content.as_array() else {
        return content.clone();
    };
    let mut out = Vec::new();
    for part in arr {
        let ptype = part["type"].as_str().unwrap_or("");
        if ptype == "image" || ptype == "attachment" || ptype == "file" {
            let candidate_path = part["path"].as_str().map(PathBuf::from).or_else(|| {
                part["sha256"].as_str().and_then(|hash| {
                    if hash.len() >= 4 {
                        let p1 = paths
                            .home
                            .join("cache/attachments/request-images")
                            .join(&hash[..2])
                            .join(hash);
                        if p1.is_file() {
                            return Some(p1);
                        }
                        let p2 = paths.home.join("attachments").join(hash);
                        if p2.is_file() {
                            return Some(p2);
                        }
                    }
                    None
                })
            });
            if let Some(resolved) = candidate_path.filter(|p| p.is_file()) {
                let pstr = resolved.to_string_lossy().into_owned();
                out.push(json!({"type": "text", "text": format!("<image path=\"{pstr}\">")}));
                out.push(json!({
                    "type": "image",
                    "url": pstr,
                    "mimeType": part["mimeType"].as_str().unwrap_or("image/png")
                }));
                out.push(json!({"type": "text", "text": "</image>"}));
                continue;
            }
        }
        out.push(part.clone());
    }
    Value::Array(out)
}

pub fn load_all_events(
    paths: &DshPaths,
    id: &str,
) -> Result<std::sync::Arc<Vec<ProviderHistoryEvent>>> {
    let session = find_root_session(paths, id)?;
    Ok(cached_events(paths, &session)?.events.clone())
}

fn cached_events(
    paths: &DshPaths,
    session: &DiscoveredSession,
) -> Result<std::sync::Arc<ProviderHistorySnapshot>> {
    crate::provider_sessions::HISTORY_CACHE.read(
        &session.log_path,
        crate::provider_sessions::history_weight,
        || parse_events(paths, session).map(ProviderHistorySnapshot::new),
    )
}

fn parse_events(
    paths: &DshPaths,
    session: &DiscoveredSession,
) -> Result<Vec<ProviderHistoryEvent>> {
    let mut events: Vec<ProviderHistoryEvent> = Vec::new();
    let mut open_tool_indices: Vec<usize> = Vec::new();
    let mut open_tool_by_id: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut line_index = 0usize;
    let mut current_turn: Option<String> = None;

    decode_log_lines(&session.log_path, session.compressed, |line| {
        if line_index == 0 {
            line_index += 1;
            return Ok(());
        }
        let idx = line_index;
        line_index += 1;
        let Ok(row) = serde_json::from_slice::<Value>(line) else {
            return Ok(());
        };
        let seq = row["seq"].as_u64().unwrap_or(idx as u64);
        let timestamp = value_timestamp_rfc3339(&row["time"]);
        let kind = row["type"].as_str().unwrap_or("");
        let data = &row["data"];
        if let Some(t) = data["turn"].as_u64() {
            current_turn = Some(format!("turn-{t}"));
        }

        match kind {
            "turn/start" => {
                if let Some(t) = data["turn"].as_u64() {
                    current_turn = Some(format!("turn-{t}"));
                }
            }
            "turn/end" | "session/end-seed" => {
                let reason = data["reason"]["kind"].as_str().unwrap_or("completed");
                let settled = if kind == "session/end-seed" || reason == "completed" {
                    "completed"
                } else {
                    "failed"
                };
                open_tool_by_id.clear();
                for call_idx in open_tool_indices.drain(..) {
                    if let Some(ev) = events.get_mut(call_idx) {
                        if ev.status.as_deref() == Some("running") {
                            ev.status = Some(settled.into());
                        }
                    }
                }
            }
            "user/message" => {
                let source_kind = data["source"]["kind"].as_str().unwrap_or("user");
                if !matches!(source_kind, "user" | "user-question-reply") {
                    return Ok(());
                }
                let event_id = format!("seq-{seq}-user");
                let normalized_content = resolve_dsh_attachment_parts(paths, &data["content"]);
                let user_msg =
                    crate::user_message::parse_user_message(&event_id, &normalized_content);
                if user_msg.text.trim().is_empty() && user_msg.attachments.is_empty() {
                    return Ok(());
                }
                let mut ev = make_dsh_event(
                    event_id,
                    timestamp,
                    "user_message",
                    "user",
                    Some(user_msg.text.clone()),
                    current_turn.clone(),
                    "completed",
                );
                ev.user_message = Some(user_msg);
                events.push(ev);
            }
            "assistant/message" => {
                let msg = &data["message"];
                if let Some(reasoning) = msg["reasoning_content"]
                    .as_str()
                    .or_else(|| msg["reasoningContent"].as_str())
                    .filter(|s| !s.trim().is_empty())
                {
                    events.push(make_dsh_event(
                        format!("seq-{seq}-thinking"),
                        timestamp.clone(),
                        "thinking",
                        "assistant",
                        Some(reasoning.to_owned()),
                        current_turn.clone(),
                        "completed",
                    ));
                }
                if let Some(blocks) = msg["content"].as_array() {
                    let mut text_parts = Vec::new();
                    for (bi, block) in blocks.iter().enumerate() {
                        match block["type"].as_str().unwrap_or("") {
                            "thinking" | "reasoning" => {
                                if let Some(t) = block["thinking"]
                                    .as_str()
                                    .or_else(|| block["text"].as_str())
                                    .filter(|s| !s.trim().is_empty())
                                {
                                    events.push(make_dsh_event(
                                        format!("seq-{seq}-thinking-{bi}"),
                                        timestamp.clone(),
                                        "thinking",
                                        "assistant",
                                        Some(t.to_owned()),
                                        current_turn.clone(),
                                        "completed",
                                    ));
                                }
                            }
                            "text" => {
                                if let Some(t) = block["text"].as_str().filter(|s| !s.is_empty()) {
                                    text_parts.push(t);
                                }
                            }
                            _ => {}
                        }
                    }
                    let joined = text_parts.join("\n");
                    if !joined.trim().is_empty() {
                        events.push(make_dsh_event(
                            format!("seq-{seq}-assistant"),
                            timestamp,
                            "assistant_message",
                            "assistant",
                            Some(joined),
                            current_turn.clone(),
                            "completed",
                        ));
                    }
                } else if let Some(text) = msg["content"].as_str().filter(|s| !s.trim().is_empty())
                {
                    events.push(make_dsh_event(
                        format!("seq-{seq}-assistant"),
                        timestamp,
                        "assistant_message",
                        "assistant",
                        Some(text.to_owned()),
                        current_turn.clone(),
                        "completed",
                    ));
                }
            }
            "tool/call" => {
                let name = data["name"].as_str().unwrap_or("tool");
                let call_id = data["callId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let args = if let Some(s) = data["arguments"].as_str() {
                    s.to_owned()
                } else if !data["arguments"].is_null() {
                    serde_json::to_string_pretty(&data["arguments"]).unwrap_or_default()
                } else {
                    String::new()
                };
                let call_idx = events.len();
                open_tool_indices.push(call_idx);
                if let Some(ref cid) = call_id {
                    open_tool_by_id.insert(cid.clone(), call_idx);
                }
                let mut ev = make_dsh_event(
                    format!("seq-{seq}-toolcall"),
                    timestamp,
                    "tool_call",
                    name,
                    Some(name.to_owned()),
                    current_turn.clone(),
                    "running",
                );
                ev.call_id = call_id;
                ev.detail = (!args.trim().is_empty()).then_some(args);
                events.push(ev);
            }
            "tool/result" => {
                let call_id = data["callId"]
                    .as_str()
                    .or_else(|| data["message"]["toolCallId"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let name = data["name"]
                    .as_str()
                    .or_else(|| data["message"]["name"].as_str())
                    .unwrap_or("tool");
                let output = extract_content_plain_text(&data["message"]["content"]);
                let is_error = data["isError"].as_bool() == Some(true)
                    || data["message"]["isError"].as_bool() == Some(true)
                    || data.get("error").is_some_and(|e| !e.is_null());
                let status_str = if is_error { "failed" } else { "completed" };
                if let Some(ref cid) = call_id {
                    if let Some(call_idx) = open_tool_by_id.remove(cid) {
                        if let Some(ev) = events.get_mut(call_idx) {
                            ev.status = Some(status_str.into());
                        }
                    }
                }
                let result_text = if !output.trim().is_empty() {
                    Some(output)
                } else {
                    data["error"]["message"].as_str().map(str::to_owned)
                };
                let mut ev = make_dsh_event(
                    format!("seq-{seq}-toolresult"),
                    timestamp,
                    "tool_result",
                    name,
                    Some(name.to_owned()),
                    current_turn.clone(),
                    status_str,
                );
                ev.call_id = call_id;
                ev.result = result_text;
                events.push(ev);
            }
            "command/run" => {
                if let Some(cmd) = data["command"]
                    .as_str()
                    .or_else(|| data["name"].as_str())
                    .filter(|s| !s.trim().is_empty())
                {
                    events.push(make_dsh_event(
                        format!("seq-{seq}-command"),
                        timestamp,
                        "system",
                        "command",
                        Some(format!("/{cmd}")),
                        current_turn.clone(),
                        "completed",
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(events)
}

fn make_dsh_event(
    id: String,
    timestamp: Option<String>,
    kind: &str,
    role: &str,
    text: Option<String>,
    turn_id: Option<String>,
    status: &str,
) -> ProviderHistoryEvent {
    ProviderHistoryEvent {
        id,
        timestamp,
        kind: kind.into(),
        role: Some(role.into()),
        text,
        call_id: None,
        turn_id,
        status: Some(status.into()),
        detail: None,
        result: None,
        user_message: None,
    }
}

pub fn session_detail(
    paths: &DshPaths,
    request: &ProviderDetailRequest,
) -> Result<ProviderSessionDetail> {
    let session = find_root_session(paths, &request.session_key)?;
    let archived = registry(paths)?.map(|r| r.archived).unwrap_or_default();
    let summary = build_summary(paths, &session, &archived);
    let events = cached_events(paths, &session)?;
    paginate_provider_detail(summary, &events, request)
}

pub fn read_attachment(
    paths: &DshPaths,
    request: &crate::user_message::SessionAttachmentRequest,
) -> Result<crate::user_message::SessionAttachmentResponse> {
    let session = find_root_session(paths, &request.session_key)?;
    let archived = registry(paths)?.map(|r| r.archived).unwrap_or_default();
    let summary = build_summary(paths, &session, &archived);
    let events = cached_events(paths, &session)?;
    let mut message = events
        .events
        .iter()
        .filter_map(|event| event.user_message.as_ref())
        .find(|msg| msg.id == request.message_id)
        .cloned()
        .context("会话消息已变化或不存在，请刷新会话")?;
    message.refresh_files((!summary.cwd.is_empty()).then_some(summary.cwd.as_str()));
    message.read_attachment(&request.attachment_id)
}

/// Exact `@deepseek-ai/dsh-session-title` normalization:
/// strips ANSI escape sequences, C0/C1 controls, zero-width/bidi controls,
/// collapses whitespace runs to single ASCII space, and truncates to 80 UTF-8 bytes.
pub fn normalize_dsh_title(raw: &str) -> Result<String> {
    let mut stripped = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\x1b' {
            i += 1;
            if i < chars.len() && chars[i] == '[' {
                i += 1;
                while i < chars.len() && !('@'..='~').contains(&chars[i]) {
                    i += 1;
                }
                i += 1;
                continue;
            } else if i < chars.len() && chars[i] == ']' {
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\x07' {
                        i += 1;
                        break;
                    }
                    if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            continue;
        }
        let code = ch as u32;
        let is_zero_width_or_bidi = matches!(
            code,
            0x200B
                | 0x200E
                | 0x200F
                | 0x202A..=0x202E
                | 0x2060..=0x2064
                | 0x2066..=0x206F
                | 0xFEFF
        );
        if is_zero_width_or_bidi {
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            stripped.push(' ');
        } else if !ch.is_control() {
            stripped.push(ch);
        }
        i += 1;
    }

    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut utf8_len = 0usize;
    let mut truncated = String::with_capacity(collapsed.len().min(MAX_DSH_TITLE_BYTES));
    for ch in collapsed.chars() {
        let clen = ch.len_utf8();
        if utf8_len + clen > MAX_DSH_TITLE_BYTES {
            break;
        }
        utf8_len += clen;
        truncated.push(ch);
    }
    let final_title = truncated.trim().to_owned();
    ensure!(!final_title.is_empty(), "会话名称不能为空");
    Ok(final_title)
}

pub fn rename_session(
    paths: &DshPaths,
    id: &str,
    raw_title: &str,
) -> Result<ProviderSessionSummary> {
    let title = normalize_dsh_title(raw_title)?;
    let session = find_root_session(paths, id)?;
    ensure!(
        session.version == 4,
        "旧版 DSH 会话格式（v1-v3）为只读，不支持改名"
    );
    let Some(_lease) = try_acquire_session_lease(&session.dir)? else {
        bail!("该会话正在 DeepSeek Harness 中打开，请先关闭或切换会话后再改名");
    };

    // Re-read log under exclusive lease to obtain exact next monotonic seq
    let mut expected_next_seq = 0u64;
    let mut saw_header = false;
    let needs_newline =
        decode_log_lines_checked(&session.log_path, session.compressed, true, |line| {
            let row: Value =
                serde_json::from_slice(line).context("DSH 会话日志记录损坏，已拒绝改名")?;
            if !saw_header {
                ensure!(
                    row["type"] == "session"
                        && row["version"] == 4
                        && row["id"].as_str() == Some(id),
                    "DSH 会话头校验失败"
                );
                saw_header = true;
                return Ok(());
            }
            let seq = row["seq"].as_u64().context("DSH 事件缺少序号")?;
            ensure!(seq == expected_next_seq, "DSH 事件序号不连续，已拒绝改名");
            expected_next_seq = seq.checked_add(1).context("DSH 事件序号溢出")?;
            Ok(())
        })?;
    ensure!(saw_header, "DSH 会话日志缺少头部记录");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    // Strict invariant in @deepseek-ai/dsh-session-title:
    // (messageSeqs.length === 0) === (source.kind === "user")
    let event = json!({
        "type": "session/title",
        "seq": expected_next_seq,
        "time": now_ms,
        "data": {
            "title": title,
            "messageSeqs": [],
            "source": { "kind": "user" }
        }
    });
    let mut event_line = Vec::new();
    if needs_newline {
        event_line.push(b'\n');
    }
    serde_json::to_writer(&mut event_line, &event)?;
    event_line.push(b'\n');

    let payload_to_append = if session.compressed {
        let mut frame = Vec::new();
        let mut encoder = zstd::stream::write::Encoder::new(&mut frame, 3)?;
        encoder.include_checksum(true)?;
        encoder.write_all(&event_line)?;
        encoder.finish()?;
        frame
    } else {
        event_line
    };

    let mut log_file = OpenOptions::new()
        .append(true)
        .open(&session.log_path)
        .context("无法写入 DSH 会话日志")?;
    log_file.write_all(&payload_to_append)?;
    log_file.sync_all()?;
    drop(log_file);

    // Invalidate cold projection cache so both AgentHub and DSH fold the new title event
    let mut cache_outcome = Outcome::default();
    let _ = remove_cache(paths, id, &mut cache_outcome);

    let archived = registry(paths)?
        .map(|index| index.archived)
        .unwrap_or_default();
    Ok(build_summary(paths, &session, &archived))
}

fn ensure_registered_in_workspace(value: &mut Value, id: &str, cwd: &str) {
    let default_ws = value
        .pointer("/global/defaultWorkspaceId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(workspaces) = value
        .pointer_mut("/tables/workspaces")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let already_present = workspaces.values().any(|ws| {
        ws["sessionIds"]
            .as_array()
            .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(id)))
    });
    if already_present {
        return;
    }
    let matching_key = workspaces
        .iter()
        .find(|(_, ws)| !cwd.is_empty() && ws["path"].as_str() == Some(cwd))
        .map(|(k, _)| k.clone())
        .or_else(|| default_ws.filter(|k| workspaces.contains_key(k)))
        .or_else(|| workspaces.keys().next().cloned());

    if let Some(ws_key) = matching_key {
        if let Some(arr) = workspaces
            .get_mut(&ws_key)
            .and_then(|ws| ws.get_mut("sessionIds"))
            .and_then(Value::as_array_mut)
        {
            arr.push(Value::String(id.to_owned()));
        }
    } else {
        let ws_id = "workspace-default".to_owned();
        let now = chrono::Utc::now().to_rfc3339();
        workspaces.insert(
            ws_id.clone(),
            json!({
                "path": if cwd.is_empty() { "/" } else { cwd },
                "title": Path::new(cwd).file_name().and_then(|s| s.to_str()).unwrap_or("Workspace"),
                "sessionIds": [id],
                "createdAt": now,
                "updatedAt": now
            }),
        );
        if let Some(arr) = value
            .pointer_mut("/global/workspaceIds")
            .and_then(Value::as_array_mut)
        {
            if !arr.iter().any(|v| v.as_str() == Some(&ws_id)) {
                arr.push(Value::String(ws_id));
            }
        }
    }
}

pub fn set_archived(paths: &DshPaths, id: &str, archived: bool) -> Result<ProviderSessionSummary> {
    set_archived_checked(paths, id, archived, || check_host_dsh_stopped_for(paths))
}

pub fn set_archived_checked(
    paths: &DshPaths,
    id: &str,
    archived: bool,
    check_stopped: impl Fn() -> Result<()>,
) -> Result<ProviderSessionSummary> {
    valid_id(id)?;
    check_stopped()?;
    let root = find_root_session(paths, id)?;
    let cwd = root.header["cwd"].as_str().unwrap_or("");
    let index = registry(paths)?;
    let mut archived_set = index
        .as_ref()
        .map(|index| index.archived.clone())
        .unwrap_or_default();
    if let Some(index) = index {
        let mut value = index.value.clone();
        if archived {
            ensure_registered_in_workspace(&mut value, id, cwd);
            if let Some(arr) = value
                .pointer_mut("/global/archivedSessionIds")
                .and_then(Value::as_array_mut)
            {
                if !arr.iter().any(|v| v.as_str() == Some(id)) {
                    arr.push(Value::String(id.to_owned()));
                }
            }
            // Native @deepseek-ai/dsh-workspace rule: archiving unpins in the same write
            if let Some(pins) = value
                .pointer_mut("/global/pinnedSessionIds")
                .and_then(Value::as_array_mut)
            {
                pins.retain(|v| v.as_str() != Some(id));
            }
        } else if let Some(arr) = value
            .pointer_mut("/global/archivedSessionIds")
            .and_then(Value::as_array_mut)
        {
            arr.retain(|v| v.as_str() != Some(id));
        }
        save_registry(&index, &value, &check_stopped)?;
    } else if archived {
        let storages = paths.home.join("storages");
        fs::create_dir_all(&storages)?;
        let ws_path = storages.join("workspace.json");
        let now = chrono::Utc::now().to_rfc3339();
        let initial = json!({
            "unit": { "name": "workspace", "version": 2 },
            "global": {
                "initialized": true,
                "workspaceIds": ["workspace-default"],
                "defaultWorkspaceId": "workspace-default",
                "archivedSessionIds": [id],
                "pinnedSessionIds": []
            },
            "tables": {
                "workspaces": {
                    "workspace-default": {
                        "path": if cwd.is_empty() { "/" } else { cwd },
                        "title": "Workspace",
                        "sessionIds": [id],
                        "createdAt": now,
                        "updatedAt": now
                    }
                }
            }
        });
        let tmp = storages.join(format!(".agenthub-index-{}", uuid::Uuid::new_v4()));
        {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            serde_json::to_writer(&mut file, &initial)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        check_stopped()?;
        fs::rename(&tmp, &ws_path)?;
    }

    if archived {
        archived_set.insert(id.to_owned());
    } else {
        archived_set.remove(id);
    }
    Ok(build_summary(paths, &root, &archived_set))
}

fn find_subagent_descendants(all: &[DiscoveredSession], root_id: &str) -> Vec<String> {
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in all {
        if let Some(parent) = &s.parent_subagent_of {
            children
                .entry(parent.clone())
                .or_default()
                .push(s.id.clone());
        }
    }
    let mut out = BTreeSet::new();
    let mut stack = vec![root_id.to_owned()];
    while let Some(curr) = stack.pop() {
        if let Some(kids) = children.get(&curr) {
            for kid in kids {
                if kid != root_id && out.insert(kid.clone()) {
                    stack.push(kid.clone());
                }
            }
        }
    }
    out.into_iter().collect()
}

fn collect_owned_paths_for_ids(
    paths: &DshPaths,
    all: &[DiscoveredSession],
    ids: &[String],
) -> Vec<PathBuf> {
    let id_set: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let mut target_paths = Vec::new();
    for s in all {
        if id_set.contains(s.id.as_str()) && session_delete::within(&paths.home, &s.dir) {
            target_paths.push(s.dir.clone());
        }
    }
    let proj_dir = paths.home.join("storages/session_projcache/sessions");
    for id in ids {
        let p = proj_dir.join(format!("{id}.json"));
        if p.exists() && session_delete::within(&paths.home, &p) {
            target_paths.push(p);
        }
    }
    target_paths
}

pub fn preview_batch_item(
    paths: &DshPaths,
    key: &str,
    operation: SessionOperation,
    include_descendants: bool,
) -> Result<SessionBatchItem> {
    preview_batch_item_checked(paths, key, operation, include_descendants, || {
        check_host_dsh_stopped_for(paths)
    })
}

fn record_remaining_dsh_residuals(
    paths: &DshPaths,
    all: &[DiscoveredSession],
    remaining_ids: &[String],
    outcome: &mut Outcome,
) {
    for path in collect_owned_paths_for_ids(paths, all, remaining_ids) {
        if path.exists() {
            outcome.residuals.push(path.display().to_string());
        }
    }
}

pub fn preview_batch_item_checked(
    paths: &DshPaths,
    key: &str,
    operation: SessionOperation,
    include_descendants: bool,
    check_stopped: impl Fn() -> Result<()>,
) -> Result<SessionBatchItem> {
    check_stopped()?;
    let archived = registry(paths)?
        .map(|index| index.archived)
        .unwrap_or_default();
    let root = find_root_session(paths, key)?;
    let summary = build_summary(paths, &root, &archived);
    let all = if operation == SessionOperation::Delete {
        discover_all_sessions(paths)?
    } else {
        vec![root]
    };
    if operation == SessionOperation::Delete && !summary.can_delete {
        bail!(
            "{}",
            summary
                .delete_block_reason
                .as_deref()
                .unwrap_or("该会话正在运行，无法删除")
        );
    }
    let all_descendants = if operation == SessionOperation::Delete {
        find_subagent_descendants(&all, &summary.id)
    } else {
        Vec::new()
    };
    if operation == SessionOperation::Delete {
        for child_id in &all_descendants {
            for child in all.iter().filter(|s| &s.id == child_id) {
                ensure!(
                    !is_session_locked_readonly(&child.dir),
                    "子会话正在 DeepSeek Harness 中运行，请先退出后再删除"
                );
            }
        }
    }
    let descendants = if operation == SessionOperation::Delete && include_descendants {
        all_descendants
    } else {
        Vec::new()
    };
    let mut all_ids = vec![summary.id.clone()];
    all_ids.extend(descendants.iter().cloned());
    let mut target_paths = collect_owned_paths_for_ids(paths, &all, &all_ids);
    if target_paths.is_empty() {
        target_paths.push(summary.path.clone());
    }
    let (_, bytes) = session_delete::size(&target_paths);
    let stamp = file_stamp(&summary.path).ok();
    let fingerprint = hex::encode(Sha256::digest(format!(
        "dsh:{}:{}:{}:{}",
        summary.id,
        summary.archived,
        bytes,
        stamp.map_or(0, |s| s.len)
    )));
    Ok(SessionBatchItem {
        session_key: summary.session_key,
        id: summary.id,
        title: summary.title,
        paths: target_paths,
        bytes,
        allowed: true,
        reason: None,
        fingerprint: Some(fingerprint),
        descendants,
        warnings: Vec::new(),
    })
}

pub fn execute_batch_item(
    paths: &DshPaths,
    key: &str,
    operation: SessionOperation,
    descendants: &[String],
    include_descendants: bool,
) -> Result<SessionBatchItemResult> {
    execute_batch_item_checked(
        paths,
        key,
        operation,
        descendants,
        include_descendants,
        || check_host_dsh_stopped_for(paths),
    )
}

pub fn execute_batch_item_checked(
    paths: &DshPaths,
    key: &str,
    operation: SessionOperation,
    descendants: &[String],
    include_descendants: bool,
    check_stopped: impl Fn() -> Result<()>,
) -> Result<SessionBatchItemResult> {
    valid_id(key)?;
    match operation {
        SessionOperation::Archive => {
            set_archived_checked(paths, key, true, check_stopped)?;
            Ok(SessionBatchItemResult {
                session_key: key.to_owned(),
                status: "succeeded".into(),
                message: None,
                deleted_ids: Vec::new(),
                residuals: Vec::new(),
            })
        }
        SessionOperation::Restore => {
            set_archived_checked(paths, key, false, check_stopped)?;
            Ok(SessionBatchItemResult {
                session_key: key.to_owned(),
                status: "succeeded".into(),
                message: None,
                deleted_ids: Vec::new(),
                residuals: Vec::new(),
            })
        }
        SessionOperation::Delete => {
            check_stopped()?;
            let index = registry(paths)?;
            let all = discover_all_sessions(paths)?;
            let root = all
                .iter()
                .find(|s| s.id == key && !s.is_subagent())
                .with_context(|| format!("未找到 DeepSeek Harness 会话：{key}"))?;
            let verified_descendants = find_subagent_descendants(&all, key);
            let verified_set: BTreeSet<&str> =
                verified_descendants.iter().map(String::as_str).collect();
            for child in descendants {
                valid_id(child)?;
                ensure!(
                    include_descendants && verified_set.contains(child.as_str()),
                    "子会话 {child} 不属于当前主会话，拒绝删除"
                );
            }
            ensure!(
                !is_session_locked_readonly(&root.dir),
                "该会话正在 DeepSeek Harness 中运行，请先退出后再删除"
            );
            for child_id in descendants {
                for child in all.iter().filter(|s| &s.id == child_id) {
                    ensure!(
                        !is_session_locked_readonly(&child.dir),
                        "子会话正在 DeepSeek Harness 中运行，请先退出后再删除"
                    );
                }
            }

            let selected: BTreeSet<&str> = descendants.iter().map(String::as_str).collect();
            let mut ordered_ids: Vec<String> = verified_descendants
                .into_iter()
                .rev()
                .filter(|id| selected.contains(id.as_str()))
                .collect();
            ordered_ids.push(key.to_owned());

            let mut leases = Vec::new();
            for id in &ordered_ids {
                for s in all.iter().filter(|s| &s.id == id) {
                    let Some(lease) = try_acquire_session_lease(&s.dir)? else {
                        bail!("会话或子会话正在 DeepSeek Harness 中运行，请先退出后再删除");
                    };
                    leases.push(lease);
                }
            }

            let mut outcome = Outcome::default();
            let mut deleted_ids = Vec::new();
            let mut parent_project_dirs = BTreeSet::new();
            let pins_dir = paths.home.join("sessions/resume-pins");
            for (idx, id) in ordered_ids.iter().enumerate() {
                if !remove_dsh_cache_strict(paths, id, &mut outcome) {
                    record_remaining_dsh_residuals(paths, &all, &ordered_ids[idx..], &mut outcome);
                    break;
                }
                let mut dir_failed = false;
                for s in all.iter().filter(|s| &s.id == id) {
                    if let Some(p) = s.dir.parent() {
                        parent_project_dirs.insert(p.to_path_buf());
                    }
                    if !remove_dsh_session_strict(&s.dir, &mut outcome) {
                        dir_failed = true;
                        break;
                    }
                }
                if dir_failed {
                    record_remaining_dsh_residuals(paths, &all, &ordered_ids[idx..], &mut outcome);
                    break;
                }
                if pins_dir.is_dir() {
                    if let Ok(entries) = fs::read_dir(&pins_dir) {
                        let mut pin_failed = false;
                        for entry in entries.flatten() {
                            let fname = entry.file_name().to_string_lossy().into_owned();
                            let matches = fname == *id || fname.starts_with(&format!("{id}."));
                            if matches && !outcome.remove_strict(&entry.path()) {
                                pin_failed = true;
                                break;
                            }
                        }
                        if pin_failed {
                            record_remaining_dsh_residuals(
                                paths,
                                &all,
                                &ordered_ids[idx + 1..],
                                &mut outcome,
                            );
                            break;
                        }
                    }
                }
                deleted_ids.push(id.clone());
            }
            drop(leases);

            for pdir in parent_project_dirs {
                if pdir.is_dir()
                    && fs::read_dir(&pdir)
                        .ok()
                        .is_some_and(|mut it| it.next().is_none())
                {
                    let _ = fs::remove_dir(&pdir);
                }
            }

            if !deleted_ids.is_empty() {
                if let Some(reg) = index {
                    let deleted_set: BTreeSet<String> = deleted_ids.iter().cloned().collect();
                    let mut value = reg.value.clone();
                    remove_references(&mut value, &deleted_set);
                    if let Err(err) = save_registry(&reg, &value, &check_stopped) {
                        outcome.residuals.push(format!("工作区索引：{err}"));
                    }
                }
            }
            outcome.finish();
            Ok(SessionBatchItemResult {
                session_key: key.to_owned(),
                status: if outcome.residuals.is_empty() {
                    "succeeded"
                } else {
                    "failed"
                }
                .into(),
                message: (!outcome.residuals.is_empty())
                    .then(|| "部分 DSH 会话未完成删除，已停止后续删除".into()),
                deleted_ids,
                residuals: outcome.residuals,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_handles_plain_compressed_and_excludes_subagents_and_unsafe_paths() {
        let home =
            std::env::temp_dir().join(format!("agenthub-dsh-locate-{}", uuid::Uuid::new_v4()));
        let paths = DshPaths { home: home.clone() };
        for (id, compressed, subagent) in [
            ("plain", false, false),
            ("compressed", true, false),
            ("child", false, true),
        ] {
            let dir = home.join("sessions/project").join(id);
            fs::create_dir_all(&dir).unwrap();
            let header = json!({"type":"session","version":4,"id":id,"cwd":"/workspace/fixture","origin":if subagent {"subagent"} else {"user"}});
            if compressed {
                write_zstd_frames(&dir.join("session.v4.jsonl.zstd"), &[header]);
            } else {
                fs::write(dir.join("session.v4.jsonl"), format!("{header}\n")).unwrap();
            }
        }
        for id in ["plain", "compressed"] {
            let found = find_root_session(&paths, id).unwrap();
            assert_eq!(found.id, id);
            assert_eq!(found.compressed, id == "compressed");
            let preview =
                preview_batch_item_checked(&paths, id, SessionOperation::Archive, true, || Ok(()))
                    .unwrap();
            assert!(preview.descendants.is_empty());
            assert!(
                set_archived_checked(&paths, id, true, || Ok(()))
                    .unwrap()
                    .archived
            );
            assert!(
                !set_archived_checked(&paths, id, false, || Ok(()))
                    .unwrap()
                    .archived
            );
            assert_eq!(
                rename_session(&paths, id, "Updated title").unwrap().title,
                "Updated title"
            );
        }
        for id in ["child", "missing", "../plain", "/plain"] {
            assert!(find_root_session(&paths, id).is_err(), "{id}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = home.join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(
                outside.join("session.v4.jsonl"),
                "{\"type\":\"session\",\"version\":4,\"id\":\"linked\"}\n",
            )
            .unwrap();
            symlink(&outside, home.join("sessions/project/linked")).unwrap();
            assert!(find_root_session(&paths, "linked").is_err());
            let external_project = home.join("external-project");
            fs::create_dir_all(external_project.join("linked-project")).unwrap();
            fs::write(
                external_project.join("linked-project/session.v4.jsonl"),
                "{\"type\":\"session\",\"version\":4,\"id\":\"linked-project\"}\n",
            )
            .unwrap();
            symlink(&external_project, home.join("sessions/project-link")).unwrap();
            assert!(find_root_session(&paths, "linked-project").is_err());
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn delete_keeps_children_created_after_confirmation() {
        let home =
            std::env::temp_dir().join(format!("agenthub-dsh-fixed-{}", uuid::Uuid::new_v4()));
        let paths = DshPaths { home: home.clone() };
        for selected_child in [false, true] {
            let root = if selected_child {
                "root-with-child"
            } else {
                "root-only"
            };
            let child = format!("{root}-selected");
            let late = format!("{root}-late");
            let write = |id: &str, parent: Option<&str>| {
                let dir = home.join("sessions/fixture").join(id);
                fs::create_dir_all(&dir).unwrap();
                let mut header = json!({"type":"session","version":4,"id":id,"createdAt":1,"cwd":"/workspace/fixture"});
                if let Some(parent) = parent {
                    header["origin"] = json!("subagent");
                    header["parentSession"] = json!(parent);
                }
                fs::write(dir.join("session.v4.jsonl"), format!("{header}\n")).unwrap();
            };
            write(root, None);
            if selected_child {
                write(&child, Some(root));
            }
            let preview =
                preview_batch_item_checked(&paths, root, SessionOperation::Delete, true, || Ok(()))
                    .unwrap();
            assert_eq!(preview.descendants.len(), usize::from(selected_child));
            write(&late, Some(root));
            let result = execute_batch_item_checked(
                &paths,
                root,
                SessionOperation::Delete,
                &preview.descendants,
                true,
                || Ok(()),
            )
            .unwrap();
            assert_eq!(result.status, "succeeded");
            assert_eq!(result.deleted_ids.len(), 1 + usize::from(selected_child));
            assert!(home.join("sessions/fixture").join(late).exists());
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn rename_delimits_complete_tails_and_leaves_incomplete_sources_unchanged() {
        let home = std::env::temp_dir().join(format!("agenthub-dsh-tail-{}", uuid::Uuid::new_v4()));
        let paths = DshPaths { home: home.clone() };
        for compressed in [false, true] {
            for tail in ["complete", "partial-json", "partial-frame"] {
                if tail == "partial-frame" && !compressed {
                    continue;
                }
                let id = format!("session-{compressed}-{tail}");
                let dir = home.join("sessions/fixture").join(&id);
                fs::create_dir_all(&dir).unwrap();
                let path = dir.join(if compressed {
                    "session.v4.jsonl.zstd"
                } else {
                    "session.v4.jsonl"
                });
                let header = json!({"type":"session","version":4,"id":id,"createdAt":1,"cwd":"/workspace/fixture"});
                let event =
                    json!({"type":"session/title","seq":0,"time":2,"data":{"title":"original"}});
                let raw = format!(
                    "{header}\n{event}{}",
                    if tail == "partial-json" {
                        "\n{\"seq\":"
                    } else {
                        ""
                    }
                );
                let mut bytes = if compressed {
                    zstd::stream::encode_all(raw.as_bytes(), 3).unwrap()
                } else {
                    raw.into_bytes()
                };
                if tail == "partial-frame" {
                    let frame = zstd::stream::encode_all(b"\n{\"seq\":1}".as_slice(), 3).unwrap();
                    bytes.extend_from_slice(&frame[..frame.len() - 1]);
                }
                fs::write(&path, &bytes).unwrap();
                let result = rename_session(&paths, &id, "renamed");
                if tail == "complete" {
                    assert_eq!(result.unwrap().title, "renamed");
                    let bytes = fs::read(&path).unwrap();
                    let decoded = if compressed {
                        zstd::stream::decode_all(bytes.as_slice()).unwrap()
                    } else {
                        bytes
                    };
                    let rows: Vec<Value> = decoded
                        .split(|b| *b == b'\n')
                        .filter(|line| !line.is_empty())
                        .map(|line| serde_json::from_slice(line).unwrap())
                        .collect();
                    assert_eq!(rows.len(), 3);
                    assert_eq!(rows[2]["seq"], 1);
                } else {
                    assert!(result.is_err());
                    assert_eq!(fs::read(&path).unwrap(), bytes);
                }
            }
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cached_history_reuses_snapshots_and_refreshes_changed_files_and_attachments() {
        let home =
            std::env::temp_dir().join(format!("agenthub-dsh-cache-{}", uuid::Uuid::new_v4()));
        let dir = home.join("sessions/fixture/session-cache");
        fs::create_dir_all(&dir).unwrap();
        let paths = DshPaths { home: home.clone() };
        let path = dir.join("session.v4.jsonl");
        let attachment = home.join("fixture.png");
        let mut image = vec![0u8; 33];
        image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        image[12..16].copy_from_slice(b"IHDR");
        fs::write(&attachment, &image).unwrap();
        let header = json!({"type":"session","version":4,"id":"session-cache","createdAt":1,"cwd":"/workspace/fixture"});
        let event = json!({"type":"user/message","seq":0,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"fixture"},{"type":"image","path":attachment}]}});
        let raw = format!("{header}\n{event}\n");
        fs::write(&path, &raw).unwrap();
        let first = load_all_events(&paths, "session-cache").unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &load_all_events(&paths, "session-cache").unwrap()
        ));
        let request = ProviderDetailRequest {
            session_key: "session-cache".into(),
            limit: Some(1),
            before: None,
        };
        let detail = session_detail(&paths, &request).unwrap();
        let warm = session_detail(&paths, &request).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            detail.timeline.as_ref().unwrap(),
            warm.timeline.as_ref().unwrap()
        ));
        let before = detail.events[0].user_message.clone().unwrap();
        image.push(1);
        fs::write(&attachment, &image).unwrap();
        let after = session_detail(&paths, &request).unwrap().events[0]
            .user_message
            .clone()
            .unwrap();
        assert_ne!(before.attachments[0].id, after.attachments[0].id);
        assert!(read_attachment(
            &paths,
            &crate::user_message::SessionAttachmentRequest {
                root_thread_id: None,
                provider: crate::services::sessions::SessionProvider::Dsh,
                session_key: "session-cache".into(),
                message_id: after.id,
                attachment_id: after.attachments[0].id.clone()
            }
        )
        .is_ok());
        let replacement = dir.join("replacement");
        fs::write(&replacement, raw.replace("\"fixture\"", "\"updated\"")).unwrap();
        fs::rename(replacement, &path).unwrap();
        let changed = load_all_events(&paths, "session-cache").unwrap();
        assert!(!std::sync::Arc::ptr_eq(&first, &changed));
        assert!(changed[0].text.as_deref().unwrap().contains("updated"));
        assert_eq!(
            session_detail(&paths, &request).unwrap().timeline.unwrap()[0].title,
            "updated"
        );
        fs::write(&path, format!("{header}\n")).unwrap();
        assert!(load_all_events(&paths, "session-cache").unwrap().is_empty());
        fs::remove_dir_all(home).unwrap();
    }

    fn write_zstd_frames(path: &Path, lines: &[Value]) {
        let mut all_frames = Vec::new();
        for row in lines {
            let mut raw = serde_json::to_vec(row).unwrap();
            raw.push(b'\n');
            let mut encoder = zstd::stream::write::Encoder::new(&mut all_frames, 3).unwrap();
            encoder.include_checksum(true).unwrap();
            encoder.write_all(&raw).unwrap();
            encoder.finish().unwrap();
        }
        fs::write(path, all_frames).unwrap();
    }

    #[test]
    fn dsh_title_normalizer_enforces_80_utf8_bytes_and_strips_controls() {
        let dirty = "  \x1b[32mHello\x1b[0m\u{200B}   世界\n\tDeepSeek  ";
        assert_eq!(normalize_dsh_title(dirty).unwrap(), "Hello 世界 DeepSeek");

        // 30 Chinese characters = 90 UTF-8 bytes -> must truncate cleanly at 26 chars (78 bytes)
        let long_cn = "深".repeat(30);
        let norm = normalize_dsh_title(&long_cn).unwrap();
        assert!(norm.len() <= MAX_DSH_TITLE_BYTES);
        assert_eq!(norm.len(), 78);
        assert_eq!(norm.chars().count(), 26);
    }

    #[test]
    fn dsh_zstd_sessions_support_list_detail_rename_archive_restore_and_subagent_delete() {
        let home = std::env::temp_dir().join(format!("agenthub-dsh-sess-{}", uuid::Uuid::new_v4()));
        let proj = home.join("sessions/--workspace-demo--");
        let root_id = "session-11111111";
        let sub_id = "session-22222222";
        let fork_id = "session-33333333";
        fs::create_dir_all(proj.join(root_id)).unwrap();
        fs::create_dir_all(proj.join(sub_id)).unwrap();
        fs::create_dir_all(proj.join(fork_id)).unwrap();
        fs::create_dir_all(home.join("storages/session_projcache/sessions")).unwrap();

        fs::write(
            home.join("storages/workspace.json"),
            serde_json::to_vec(&json!({
                "unit": { "name": "workspace", "version": 2 },
                "global": {
                    "initialized": true,
                    "workspaceIds": ["ws-1"],
                    "defaultWorkspaceId": "ws-1",
                    "archivedSessionIds": [],
                    "pinnedSessionIds": [root_id]
                },
                "tables": {
                    "workspaces": {
                        "ws-1": {
                            "path": "/workspace/demo",
                            "title": "Demo",
                            "sessionIds": [root_id, fork_id]
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        write_zstd_frames(
            &proj.join(root_id).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":root_id,"createdAt":1793000000000i64,"cwd":"/workspace/demo","isSeeded":false}),
                json!({"type":"turn/start","seq":0,"time":1793000001000i64,"data":{"turn":1}}),
                json!({"type":"user/message","seq":1,"time":1793000001000i64,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"请帮我检查 DSH 存储结构"}]}}),
                json!({"type":"tool/call","seq":2,"time":1793000002000i64,"data":{"turn":1,"callId":"call-1","name":"bash","arguments":{"command":"ls -la"}}}),
                json!({"type":"tool/result","seq":3,"time":1793000003000i64,"data":{"turn":1,"callId":"call-1","message":{"toolCallId":"call-1","content":[{"type":"text","text":"workspace.json"}]}}}),
                json!({"type":"assistant/message","seq":4,"time":1793000004000i64,"data":{"turn":1,"message":{"reasoning_content":"先看返回结果","content":[{"type":"text","text":"检查完成，索引为 v2 格式。"}]}}}),
                json!({"type":"turn/end","seq":5,"time":1793000005000i64,"data":{"turn":1,"reason":{"kind":"completed"}}}),
            ],
        );
        // Subagent session (must NOT appear in top-level list, MUST be deleted with root_id)
        write_zstd_frames(
            &proj.join(sub_id).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":sub_id,"createdAt":1793000002000i64,"cwd":"/workspace/demo","origin":"subagent","parentSession":root_id,"delegationDepth":1}),
            ],
        );
        // Forked user session (MUST appear in top-level list, must NOT be deleted with root_id)
        write_zstd_frames(
            &proj.join(fork_id).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":fork_id,"createdAt":1793000006000i64,"cwd":"/workspace/demo","isSeeded":true,"inheritedEventCount":0}),
                json!({"type":"session/title","seq":0,"time":1793000006000i64,"data":{"title":"分支会话","messageSeqs":[],"source":{"kind":"user"}}}),
            ],
        );

        let paths = DshPaths { home: home.clone() };

        // 1. List active sessions: root_id + fork_id (sub_id excluded!)
        let list = list_sessions(
            &paths,
            &ProviderListRequest {
                archived: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|s| s.id == root_id));
        assert!(list
            .iter()
            .any(|s| s.id == fork_id && s.title == "分支会话"));

        // 2. Detail reading
        let detail = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: root_id.into(),
                limit: None,
                before: None,
            },
        )
        .unwrap();
        assert_eq!(detail.events.len(), 5);
        assert_eq!(detail.events[0].kind, "user_message");
        assert_eq!(detail.events[1].kind, "tool_call");
        assert_eq!(detail.events[2].kind, "tool_result");
        assert_eq!(detail.events[3].kind, "thinking");
        assert_eq!(detail.events[4].kind, "assistant_message");

        // 3. Rename appends valid checksummed Zstd frame with seq=6 and strict user invariant
        let renamed = rename_session(&paths, root_id, "DSH 完美改名测试").unwrap();
        assert_eq!(renamed.title, "DSH 完美改名测试");
        // Verify dsh::notifications strict reader still parses the multi-frame zstd log cleanly!
        let mut last_event: Option<Value> = None;
        decode_log_lines(
            &proj.join(root_id).join("session.v4.jsonl.zstd"),
            true,
            |line| {
                last_event = Some(serde_json::from_slice(line).unwrap());
                Ok(())
            },
        )
        .unwrap();
        let ev = last_event.unwrap();
        assert_eq!(ev["type"], "session/title");
        assert_eq!(ev["seq"], 6);
        assert_eq!(ev["data"]["messageSeqs"], json!([]));
        assert_eq!(ev["data"]["source"]["kind"], "user");

        // 4. Archive unpins root_id and moves it to archived view; running DSH guard blocks when active
        assert!(set_archived_checked(&paths, root_id, true, || bail!("DSH running")).is_err());
        let archived = set_archived_checked(&paths, root_id, true, || Ok(())).unwrap();
        assert!(archived.archived);
        let ws_after_archive: Value =
            serde_json::from_slice(&fs::read(home.join("storages/workspace.json")).unwrap())
                .unwrap();
        assert_eq!(
            ws_after_archive["global"]["archivedSessionIds"],
            json!([root_id])
        );
        assert_eq!(ws_after_archive["global"]["pinnedSessionIds"], json!([]));

        let restored = set_archived_checked(&paths, root_id, false, || Ok(())).unwrap();
        assert!(!restored.archived);

        // 5. Delete removes root_id + sub_id, updates workspace.json, and leaves fork_id intact!
        let preview =
            preview_batch_item_checked(&paths, root_id, SessionOperation::Delete, true, || Ok(()))
                .unwrap();
        assert_eq!(preview.descendants, vec![sub_id.to_string()]);
        let del = execute_batch_item_checked(
            &paths,
            root_id,
            SessionOperation::Delete,
            &preview.descendants,
            true,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(del.status, "succeeded");
        assert!(!proj.join(root_id).exists());
        assert!(!proj.join(sub_id).exists());
        assert!(proj.join(fork_id).exists());

        let remaining = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, fork_id);

        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn dsh_delete_rejects_forged_cross_root_subagents_and_locked_descendants() {
        let home =
            std::env::temp_dir().join(format!("agenthub-dsh-guard-{}", uuid::Uuid::new_v4()));
        let proj = home.join("sessions/--workspace-demo--");
        let root_a = "session-aaaa1111";
        let sub_a = "session-aaaa2222";
        let root_b = "session-bbbb1111";
        let sub_b = "session-bbbb2222";
        for id in [root_a, sub_a, root_b, sub_b] {
            fs::create_dir_all(proj.join(id)).unwrap();
        }
        fs::create_dir_all(home.join("storages/session_projcache/sessions")).unwrap();
        fs::write(
            home.join("storages/workspace.json"),
            serde_json::to_vec(&json!({
                "unit": { "name": "workspace", "version": 2 },
                "global": {
                    "initialized": true,
                    "workspaceIds": ["ws-1"],
                    "defaultWorkspaceId": "ws-1",
                    "archivedSessionIds": [],
                    "pinnedSessionIds": []
                },
                "tables": {
                    "workspaces": {
                        "ws-1": {
                            "path": "/workspace/demo",
                            "title": "Demo",
                            "sessionIds": [root_a, root_b]
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        write_zstd_frames(
            &proj.join(root_a).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":root_a,"createdAt":1,"cwd":"/workspace/demo","isSeeded":false}),
            ],
        );
        write_zstd_frames(
            &proj.join(sub_a).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":sub_a,"createdAt":2,"cwd":"/workspace/demo","origin":"subagent","parentSession":root_a,"delegationDepth":1}),
            ],
        );
        write_zstd_frames(
            &proj.join(root_b).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":root_b,"createdAt":3,"cwd":"/workspace/demo","isSeeded":false}),
            ],
        );
        write_zstd_frames(
            &proj.join(sub_b).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":sub_b,"createdAt":4,"cwd":"/workspace/demo","origin":"subagent","parentSession":root_b,"delegationDepth":1}),
            ],
        );

        let paths = DshPaths { home: home.clone() };

        // 1. Forged root session ID in descendants is rejected
        assert!(execute_batch_item_checked(
            &paths,
            root_a,
            SessionOperation::Delete,
            &[root_b.to_string()],
            true,
            || Ok(()),
        )
        .is_err());
        assert!(proj.join(root_b).exists());

        // 2. Cross-root subagent ID (is_subagent == true, but parent is root_b) is rejected
        assert!(execute_batch_item_checked(
            &paths,
            root_a,
            SessionOperation::Delete,
            &[sub_b.to_string()],
            true,
            || Ok(()),
        )
        .is_err());
        assert!(proj.join(sub_b).exists());

        // 3. Passing descendants when include_descendants == false is rejected
        assert!(execute_batch_item_checked(
            &paths,
            root_a,
            SessionOperation::Delete,
            &[sub_a.to_string()],
            false,
            || Ok(()),
        )
        .is_err());

        // 4. Locked descendant blocks both preview and execute before any deletion
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let lock_file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(proj.join(sub_a).join("session.lock"))
                .unwrap();
            assert_eq!(
                unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            assert!(preview_batch_item_checked(
                &paths,
                root_a,
                SessionOperation::Delete,
                true,
                || Ok(()),
            )
            .is_err());
            assert!(execute_batch_item_checked(
                &paths,
                root_a,
                SessionOperation::Delete,
                &[sub_a.to_string()],
                true,
                || Ok(()),
            )
            .is_err());
            assert!(proj.join(root_a).exists());
            assert!(proj.join(sub_a).exists());
            unsafe {
                let _ = libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN);
            }
        }

        // 5. Partial filesystem failure stops on sub_a before touching root_a or workspace.json
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Pre-create session.lock so lease acquisition succeeds while directory unlink fails
            fs::write(proj.join(sub_a).join("session.lock"), b"").unwrap();
            fs::set_permissions(proj.join(sub_a), fs::Permissions::from_mode(0o500)).unwrap();
            let partial = execute_batch_item_checked(
                &paths,
                root_a,
                SessionOperation::Delete,
                &[sub_a.to_string()],
                true,
                || Ok(()),
            )
            .unwrap();
            assert_eq!(partial.status, "failed");
            assert!(partial.deleted_ids.is_empty());
            assert!(!partial.residuals.is_empty());
            assert!(proj.join(root_a).exists());
            let ws_mid: Value =
                serde_json::from_slice(&fs::read(home.join("storages/workspace.json")).unwrap())
                    .unwrap();
            assert_eq!(
                ws_mid["tables"]["workspaces"]["ws-1"]["sessionIds"],
                json!([root_a, root_b])
            );

            fs::set_permissions(proj.join(sub_a), fs::Permissions::from_mode(0o700)).unwrap();
            let retried = execute_batch_item_checked(
                &paths,
                root_a,
                SessionOperation::Delete,
                &[sub_a.to_string()],
                true,
                || Ok(()),
            )
            .unwrap();
            assert_eq!(retried.status, "succeeded");
            assert!(!proj.join(root_a).exists());
            assert!(!proj.join(sub_a).exists());
            assert!(proj.join(root_b).exists());
            assert!(proj.join(sub_b).exists());
        }

        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn pick_generation_deterministically_prefers_highest_version_and_plain_jsonl_over_zstd() {
        let dir = std::env::temp_dir().join(format!("agenthub-dsh-gen-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let zstd_v4 = dir.join("session.v4.jsonl.zstd");
        let plain_v3 = dir.join("session.v3.jsonl");
        fs::write(&zstd_v4, b"zstd").unwrap();
        fs::write(&plain_v3, b"{}\n").unwrap();
        let picked_zstd = pick_generation(&dir).unwrap().unwrap();
        assert_eq!(picked_zstd, (4, true, zstd_v4));

        let plain_v4 = dir.join("session.v4.jsonl");
        fs::write(&plain_v4, b"{}\n").unwrap();
        let picked_plain = pick_generation(&dir).unwrap().unwrap();
        assert_eq!(picked_plain, (4, false, plain_v4));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn locked_completed_and_seeded_sessions_report_idle_and_settle_unclosed_tool_calls() {
        let home = std::env::temp_dir().join(format!("agenthub-dsh-lock-{}", uuid::Uuid::new_v4()));
        let proj = home.join("sessions/--workspace-demo--");
        let completed_id = "session-completed-locked";
        let active_id = "session-active-locked";
        let seeded_id = "session-seeded-locked";
        for id in [completed_id, active_id, seeded_id] {
            fs::create_dir_all(proj.join(id)).unwrap();
        }
        fs::create_dir_all(home.join("storages/session_projcache/sessions")).unwrap();
        fs::write(
            home.join("storages/workspace.json"),
            serde_json::to_vec(&json!({
                "unit": { "name": "workspace", "version": 2 },
                "global": {
                    "initialized": true,
                    "workspaceIds": ["ws-1"],
                    "defaultWorkspaceId": "ws-1",
                    "archivedSessionIds": [],
                    "pinnedSessionIds": []
                },
                "tables": {
                    "workspaces": {
                        "ws-1": {
                            "path": "/workspace/demo",
                            "title": "Demo",
                            "sessionIds": [completed_id, active_id, seeded_id]
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        write_zstd_frames(
            &proj.join(completed_id).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":completed_id,"createdAt":10,"cwd":"/workspace/demo","isSeeded":false}),
                json!({"type":"turn/start","seq":0,"time":11,"data":{"turn":1}}),
                json!({"type":"user/message","seq":1,"time":12,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"分析对话目录文件结构"}]}}),
                json!({"type":"tool/call","seq":2,"time":13,"data":{"turn":1,"callId":"call-orphan","name":"bash","arguments":{"command":"ls"}}}),
                json!({"type":"turn/end","seq":3,"time":14,"data":{"turn":1,"reason":{"kind":"completed"}}}),
            ],
        );
        write_zstd_frames(
            &proj.join(active_id).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":active_id,"createdAt":20,"cwd":"/workspace/demo","isSeeded":false}),
                json!({"type":"turn/start","seq":0,"time":21,"data":{"turn":1}}),
                json!({"type":"user/message","seq":1,"time":22,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"仍在执行中"}]}}),
            ],
        );
        write_zstd_frames(
            &proj.join(seeded_id).join("session.v4.jsonl.zstd"),
            &[
                json!({"type":"session","version":4,"id":seeded_id,"createdAt":30,"cwd":"/workspace/demo","isSeeded":true,"inheritedEventCount":2}),
                json!({"type":"turn/start","seq":0,"time":31,"data":{"turn":1}}),
                json!({"type":"user/message","seq":1,"time":32,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"继承自父会话的未闭合轮次"}]}}),
            ],
        );

        let paths = DshPaths { home: home.clone() };

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let mut locks = Vec::new();
            for id in [completed_id, active_id, seeded_id] {
                let f = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(proj.join(id).join("session.lock"))
                    .unwrap();
                assert_eq!(
                    unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                    0
                );
                locks.push(f);
            }

            let list = list_sessions(
                &paths,
                &ProviderListRequest {
                    archived: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
            let completed = list.iter().find(|s| s.id == completed_id).unwrap();
            assert_eq!(completed.status, "idle");
            assert!(!completed.can_rename);
            assert!(!completed.can_delete);

            let active = list.iter().find(|s| s.id == active_id).unwrap();
            assert_eq!(active.status, "running");

            let seeded = list.iter().find(|s| s.id == seeded_id).unwrap();
            assert_eq!(seeded.status, "idle");

            let detail = session_detail(
                &paths,
                &ProviderDetailRequest {
                    session_key: completed_id.into(),
                    limit: None,
                    before: None,
                },
            )
            .unwrap();
            let tool_ev = detail
                .events
                .iter()
                .find(|e| e.kind == "tool_call")
                .unwrap();
            assert_eq!(tool_ev.status.as_deref(), Some("completed"));

            drop(locks);
        }

        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn dsh_fold_cache_hits_and_invalidates_on_file_change_and_eviction() {
        let home = std::env::temp_dir().join(format!("agenthub-dsh-fold-{}", uuid::Uuid::new_v4()));
        let proj = home.join("sessions").join("proj-cache");
        let id = "11111111-2222-4333-8444-555555555555";
        fs::create_dir_all(proj.join(id)).unwrap();
        let log_path = proj.join(id).join("session.v4.jsonl.zstd");
        write_zstd_frames(
            &log_path,
            &[
                json!({"type":"session","version":4,"id":id,"createdAt":10,"cwd":"/workspace/cache"}),
                json!({"type":"turn/start","seq":0,"time":11,"data":{"turn":1}}),
                json!({"type":"user/message","seq":1,"time":12,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"初始标题"}]}}),
                json!({"type":"assistant/message","seq":2,"time":13,"data":{"turn":1,"message":{"content":[{"type":"text","text":"第一次回复"}]}}}),
                json!({"type":"turn/end","seq":3,"time":14,"data":{"turn":1,"reason":{"kind":"completed"}}}),
            ],
        );

        let paths = DshPaths { home: home.clone() };
        let first = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].last_message.as_deref(), Some("第一次回复"));

        // Second call hits cache and returns identical summary
        let second = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(second[0].last_message.as_deref(), Some("第一次回复"));

        // Modifying the file changes size/mtime and invalidates the cached fold entry
        write_zstd_frames(
            &log_path,
            &[
                json!({"type":"session","version":4,"id":id,"createdAt":10,"cwd":"/workspace/cache"}),
                json!({"type":"turn/start","seq":0,"time":11,"data":{"turn":1}}),
                json!({"type":"user/message","seq":1,"time":12,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"初始标题"}]}}),
                json!({"type":"assistant/message","seq":2,"time":13,"data":{"turn":1,"message":{"content":[{"type":"text","text":"更新后的长回复内容"}]}}}),
                json!({"type":"turn/end","seq":3,"time":15,"data":{"turn":1,"reason":{"kind":"completed"}}}),
            ],
        );
        let third = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(third[0].last_message.as_deref(), Some("更新后的长回复内容"));

        evict_idle_fold_cache(std::time::Duration::ZERO);
        let reloaded = list_sessions(&paths, &ProviderListRequest::default()).unwrap();
        assert_eq!(reloaded[0].last_message, third[0].last_message);

        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn load_all_events_settles_multiple_open_tools_across_turns_in_linear_pass() {
        let home =
            std::env::temp_dir().join(format!("agenthub-dsh-tools-{}", uuid::Uuid::new_v4()));
        let proj = home.join("sessions").join("proj-tools");
        let id = "22222222-3333-4444-8555-666666666666";
        fs::create_dir_all(proj.join(id)).unwrap();
        let log_path = proj.join(id).join("session.v4.jsonl.zstd");
        write_zstd_frames(
            &log_path,
            &[
                json!({"type":"session","version":4,"id":id,"createdAt":10,"cwd":"/workspace/tools"}),
                json!({"type":"turn/start","seq":0,"time":11,"data":{"turn":1}}),
                json!({"type":"tool/call","seq":1,"time":12,"data":{"callId":"call-a","name":"bash","arguments":{"cmd":"ls"}}}),
                json!({"type":"tool/call","seq":2,"time":13,"data":{"callId":"call-b","name":"read","arguments":{"path":"a.txt"}}}),
                json!({"type":"tool/result","seq":3,"time":14,"data":{"callId":"call-a","name":"bash","message":{"content":[{"type":"text","text":"ok"}]}}}),
                json!({"type":"turn/end","seq":4,"time":15,"data":{"turn":1,"reason":{"kind":"aborted"}}}),
                json!({"type":"turn/start","seq":5,"time":16,"data":{"turn":2}}),
                json!({"type":"tool/call","seq":6,"time":17,"data":{"callId":"call-c","name":"bash","arguments":{"cmd":"pwd"}}}),
                json!({"type":"turn/end","seq":7,"time":18,"data":{"turn":2,"reason":{"kind":"completed"}}}),
            ],
        );

        let paths = DshPaths { home: home.clone() };
        let detail = session_detail(
            &paths,
            &ProviderDetailRequest {
                session_key: id.into(),
                limit: None,
                before: None,
            },
        )
        .unwrap();
        let tools: Vec<_> = detail
            .events
            .iter()
            .filter(|e| e.kind == "tool_call")
            .collect();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].call_id.as_deref(), Some("call-a"));
        assert_eq!(tools[0].status.as_deref(), Some("completed"));
        assert_eq!(tools[1].call_id.as_deref(), Some("call-b"));
        assert_eq!(tools[1].status.as_deref(), Some("failed"));
        assert_eq!(tools[2].call_id.as_deref(), Some("call-c"));
        assert_eq!(tools[2].status.as_deref(), Some("completed"));

        fs::remove_dir_all(home).unwrap();
    }
}
