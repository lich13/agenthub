//! Native Claude transcripts. Reading never starts the CLI or changes its configuration.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

mod activity;
pub mod hooks;
mod mutations;
mod presentation;
mod reader;
#[cfg(test)]
mod tests;
pub(crate) use mutations::execute_claude_delete_batch;
pub(crate) use mutations::find_empty_inactive_sessions;
pub use mutations::{execute_claude_delete, preview_claude_delete, rename_claude_session};
pub(crate) use reader::evict_idle_detail_cache;

#[derive(Debug, Clone)]
pub struct ClaudePaths {
    pub config: PathBuf,
    pub projects: PathBuf,
}
impl ClaudePaths {
    pub fn default_for_user() -> Self {
        let config = env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"));
        Self {
            projects: config.join("projects"),
            config,
        }
    }
    pub fn available(&self) -> bool {
        self.projects.is_dir()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSessionSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_size: Option<crate::session_storage::SessionStorageSize>,
    pub id: String,
    pub session_key: String,
    pub title: String,
    pub cwd: String,
    pub path: PathBuf,
    pub updated_at: Option<String>,
    pub message_count: usize,
    pub last_message: Option<String>,
    pub status: String,
    pub format_version: String,
    pub can_rename: bool,
    pub rename_block_reason: Option<String>,
    pub can_delete: bool,
    pub delete_block_reason: Option<String>,
    pub read_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_warning: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeHistoryEvent {
    pub id: String,
    pub timestamp: Option<String>,
    pub kind: String,
    pub role: Option<String>,
    pub text: Option<String>,
    pub call_id: Option<String>,
    pub turn_id: Option<String>,
    pub status: Option<String>,
    pub detail: Option<String>,
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_message: Option<crate::user_message::UserMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_phase: Option<ClaudeAssistantPhase>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeAssistantPhase {
    Intermediate,
    Final,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSessionDetail {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<crate::history_navigation::Timeline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<crate::jsonl_cache::HistoryMetadata>,
    pub summary: ClaudeSessionSummary,
    pub events: Vec<ClaudeHistoryEvent>,
    pub total_events: usize,
    pub has_more: bool,
    pub before_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaudeDetailRequest {
    pub session_key: String,
    pub limit: Option<usize>,
    pub before: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDeletePreview {
    pub session_key: String,
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    pub fingerprint: String,
    pub file_count: usize,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeOwnershipProcess {
    pub pid: u32,
    pub role: String,
    pub registration_verified: bool,
    pub mismatched_fields: Vec<String>,
}

/// Local diagnostics deliberately exclude transcript content, paths and argv.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeOwnershipDiagnostics {
    pub ownership: String,
    pub reason_codes: Vec<String>,
    pub processes: Vec<ClaudeOwnershipProcess>,
}

pub fn claude_ownership_diagnostics(
    paths: &ClaudePaths,
    key: &str,
) -> Result<ClaudeOwnershipDiagnostics> {
    let (path, parsed) = resolve(paths, key)
        .map_err(|_| anyhow::anyhow!("Claude 会话定位或内容无法核验 [evidence_conflict]"))?;
    Ok(
        mutations::deletion_assessment(&parsed, &path, &activity::Snapshot::capture(paths))
            .map_err(|_| anyhow::anyhow!("Claude 专属内容无法核验 [evidence_conflict]"))?
            .diagnostics(),
    )
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaudeDeleteRequest {
    pub session_key: String,
    pub confirmed: bool,
    #[serde(default)]
    pub fingerprint: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDeleteResult {
    pub session_key: String,
    pub deleted: bool,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub residuals: Vec<String>,
}

#[derive(Clone, Default)]
struct Parsed {
    timeline: crate::history_navigation::Timeline,
    id: String,
    title: String,
    cwd: String,
    /// Every working directory the session has recorded. A session that moves
    /// keeps the launch directory its native owner registered.
    cwds: Vec<String>,
    version: String,
    updated_at: Option<String>,
    events: Vec<Arc<ClaudeHistoryEvent>>,
    message_count: usize,
    last_message: Option<String>,
    history: crate::jsonl_cache::HistoryJournal,
    issues: Vec<String>,
    warnings: Vec<String>,
    last_turn_at: Option<i64>,
    turn_open: bool,
    native_events: Vec<crate::native_probe::NativeTurnEvent>,
    prompts: HashMap<String, ClaudePrompt>,
    current_turn: Option<String>,
    record_count: u64,
    settled_count: u64,
}
fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn digest(value: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(value.as_ref()))
}
type LocatedSessions = HashMap<(PathBuf, String), PathBuf>;
static LOCATED_SESSIONS: std::sync::LazyLock<std::sync::Mutex<LocatedSessions>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
fn key_for(root: &Path, path: &Path, _id: &str) -> String {
    let id = path.file_stem().unwrap_or_default().to_string_lossy();
    let key = format!(
        "claude:{}",
        digest(format!(
            "{}\0{id}",
            path.strip_prefix(root).unwrap_or(path).to_string_lossy()
        ))
    );
    if let Ok(mut cache) = LOCATED_SESSIONS.lock() {
        let cache_key = (root.to_path_buf(), key.clone());
        if cache.len() >= 20_000 && !cache.contains_key(&cache_key) {
            cache.clear();
        }
        // Keep the path scoped by its project root. The same relative
        // filename can exist in several machines or concurrent fixtures.
        cache.insert(cache_key, path.into());
    }
    key
}

fn files(paths: &ClaudePaths) -> Result<Vec<PathBuf>> {
    if !paths.projects.exists() {
        return Ok(vec![]);
    }
    ensure!(
        !fs::symlink_metadata(&paths.projects)?
            .file_type()
            .is_symlink(),
        "Claude 会话根不能是符号链接"
    );
    let root = paths.projects.canonicalize()?;
    ensure!(root.is_dir(), "Claude 会话根不是目录");
    let mut result = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_entry(|e| !matches!(e.file_name().to_str(), Some("subagents" | "tool-results")))
    {
        let entry = entry.context("无法扫描 Claude 会话")?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|x| x == "jsonl") {
            ensure!(result.len() < 20_000, "Claude 会话数量超出读取上限");
            result.push(entry.into_path());
        }
    }
    result.sort();
    Ok(result)
}
fn read_or_warning(path: &Path, summary_only: bool) -> Arc<Parsed> {
    let result = if summary_only {
        reader::summary(path)
    } else {
        reader::read(path)
    };
    result.unwrap_or_else(|_| {
        let id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Arc::new(Parsed {
            title: id.clone(),
            id,
            version: "unknown".into(),
            issues: vec!["会话无法读取或超出读取上限".into()],
            ..Default::default()
        })
    })
}
fn locate(paths: &ClaudePaths, key: &str) -> Result<PathBuf> {
    ensure!(
        key.starts_with("claude:")
            && key.len() == 71
            && key[7..].bytes().all(|c| c.is_ascii_hexdigit()),
        "无效的 Claude 会话定位键"
    );
    let root = paths
        .projects
        .canonicalize()
        .unwrap_or_else(|_| paths.projects.clone());
    if let Some(path) = LOCATED_SESSIONS
        .lock()
        .ok()
        .and_then(|cache| cache.get(&(root.clone(), key.to_owned())).cloned())
    {
        if crate::session_delete::within(&root, &path) {
            return Ok(path);
        }
    }
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !matches!(e.file_name().to_str(), Some("subagents" | "tool-results")))
    {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "jsonl")
            && key_for(&root, entry.path(), "") == key
        {
            return Ok(entry.into_path());
        }
    }
    anyhow::bail!("Claude 会话定位键不存在，请刷新列表")
}

fn resolve(paths: &ClaudePaths, key: &str) -> Result<(PathBuf, Arc<Parsed>)> {
    ensure!(
        key.starts_with("claude:")
            && key.len() == 71
            && key[7..].bytes().all(|c| c.is_ascii_hexdigit()),
        "无效的 Claude 会话定位键"
    );
    ensure!(
        !fs::symlink_metadata(&paths.projects)?
            .file_type()
            .is_symlink(),
        "Claude 会话根不能是符号链接"
    );
    let root = paths
        .projects
        .canonicalize()
        .context("未发现 Claude 会话")?;
    let located = LOCATED_SESSIONS
        .lock()
        .ok()
        .and_then(|cache| cache.get(&(root.clone(), key.to_owned())).cloned());
    if let Some(path) = located {
        // Revalidate every native identity, including the ancestor chain. The
        // lookup stores only paths discovered by our own bounded directory scan.
        let safe = path.starts_with(&root)
            && path
                .ancestors()
                .take_while(|parent| *parent != root)
                .all(|parent| {
                    fs::symlink_metadata(parent).is_ok_and(|m| !m.file_type().is_symlink())
                });
        if safe {
            if let Ok(parsed) = reader::read(&path) {
                if key_for(&root, &path, &parsed.id) == key {
                    return Ok((path, parsed));
                }
            }
        }
    }
    for path in files(paths)? {
        let parsed = read_or_warning(&path, true);
        if key_for(&root, &path, &parsed.id) == key {
            let parsed = read_or_warning(&path, false);
            ensure!(
                key_for(&root, &path, &parsed.id) == key,
                "Claude 会话身份已变化，请刷新列表"
            );
            return Ok((path, parsed));
        }
    }
    anyhow::bail!("Claude 会话已变化或不存在，请刷新列表")
}
fn summary(
    paths: &ClaudePaths,
    path: &Path,
    parsed: &Parsed,
    snapshot: &activity::Snapshot,
) -> Result<ClaudeSessionSummary> {
    let canonical_projects = paths.projects.canonicalize()?;
    summary_with_root(&canonical_projects, path, parsed, snapshot)
}
fn summary_with_root(
    canonical_projects: &Path,
    path: &Path,
    parsed: &Parsed,
    snapshot: &activity::Snapshot,
) -> Result<ClaudeSessionSummary> {
    let assessment = snapshot.assess(parsed, path);
    let owner = assessment.ownership;
    let fatal_reason = (!parsed.issues.is_empty()).then(|| parsed.issues.join("；"));
    let warning_reason = (!parsed.warnings.is_empty()).then(|| parsed.warnings.join("；"));
    let reason = if let Some(reason) = fatal_reason.clone() {
        Some(reason)
    } else {
        assessment.block_reason()
    };
    let status = if !parsed.issues.is_empty() {
        "unknown"
    } else if let Some(native) = assessment.native_status() {
        // The verified owner publishes the live state: main loop or delegated
        // agents/workflows active, a prompt waiting for the user, or idle.
        match native {
            "busy" => "running",
            "waiting" => "waiting",
            _ => "recent",
        }
    } else if !parsed.turn_open {
        "recent"
    } else if owner == activity::Ownership::Open {
        "running"
    } else {
        "unknown"
    };
    /*
     * Fatal parse errors remain the read error and force an unknown status.
     * Compatibility warnings are shown separately without blocking mutations,
     * while notification scanning may use their terminal evidence.
     */
    let read_error = (!parsed.issues.is_empty()).then(|| parsed.issues.join("；"));
    let read_warning = warning_reason;
    let rename_reason = reason
        .clone()
        .or_else(|| {
            (!parsed.version.starts_with("2.") || parsed.cwd.is_empty())
                .then(|| "缺少改名所需的工作目录或版本".into())
        })
        .or_else(|| {
            mutations::check_rename_path_with_root(canonical_projects, path)
                .err()
                .map(|e| e.to_string())
        });
    Ok(ClaudeSessionSummary {
        storage_size: None,
        id: parsed.id.clone(),
        session_key: key_for(canonical_projects, path, &parsed.id),
        title: parsed.title.clone(),
        cwd: parsed.cwd.clone(),
        path: path.to_owned(),
        updated_at: parsed.updated_at.clone(),
        message_count: parsed.message_count,
        last_message: parsed.last_message.clone(),
        status: status.into(),
        format_version: parsed.version.clone(),
        can_rename: rename_reason.is_none(),
        can_delete: true,
        rename_block_reason: rename_reason,
        delete_block_reason: None,
        read_error,
        read_warning,
    })
}
pub fn list_claude_sessions(
    paths: &ClaudePaths,
    limit: usize,
    query: Option<&str>,
) -> Result<Vec<ClaudeSessionSummary>> {
    let mut items = list_claude_sessions_without_storage(paths, limit, query)?;
    for item in &mut items {
        attach_storage_size(paths, item);
    }
    Ok(items)
}

pub(crate) fn list_claude_sessions_without_storage(
    paths: &ClaudePaths,
    limit: usize,
    query: Option<&str>,
) -> Result<Vec<ClaudeSessionSummary>> {
    let snapshot = activity::Snapshot::capture_readonly(paths);
    let needle = query.unwrap_or("").trim().to_lowercase();
    let files = files(paths)?;
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let canonical_projects = paths.projects.canonicalize()?;
    let mut result = Vec::new();
    for path in files {
        let parsed = read_or_warning(&path, true);
        let s = summary_with_root(&canonical_projects, &path, &parsed, &snapshot)?;
        if [&s.title, &s.id, &s.cwd]
            .iter()
            .any(|v| v.to_lowercase().contains(&needle))
        {
            result.push(s);
        }
    }
    result.sort_by_cached_key(|session| {
        (std::cmp::Reverse(session.updated_at.as_deref().and_then(
            |time| chrono::DateTime::parse_from_rfc3339(time).ok(),
        )),)
    });
    result.truncate(limit.clamp(1, 200));
    Ok(result)
}

fn attach_storage_size(paths: &ClaudePaths, summary: &mut ClaudeSessionSummary) {
    use crate::session_storage::{self, StorageScope, StorageStatus};
    let mut size = session_storage::file_size(&paths.projects, &summary.path);
    if summary.read_error.is_some() {
        summary.storage_size = Some(session_storage::SessionStorageSize::unavailable(
            StorageScope::File,
        ));
        return;
    }
    // A same-ID directory belongs to this transcript only when the native ID,
    // native filename and project all agree. Duplicate IDs in other files do not inherit it.
    if summary.path.file_stem().and_then(|s| s.to_str()) == Some(&summary.id) {
        let directory = summary.path.with_extension("");
        match fs::symlink_metadata(&directory) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
                size = session_storage::directory_size(
                    &paths.projects,
                    &directory,
                    Some(&summary.path),
                    &summary.session_key,
                );
            }
            Ok(_) => {
                size.scope = StorageScope::Directory;
                size.status = StorageStatus::Partial;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                size.scope = StorageScope::Directory;
                size.status = StorageStatus::Partial;
            }
        }
    }
    summary.storage_size = Some(size);
}
pub fn claude_session_summary(paths: &ClaudePaths, key: &str) -> Result<ClaudeSessionSummary> {
    let mut summary = claude_session_summary_without_storage(paths, key)?;
    attach_storage_size(paths, &mut summary);
    Ok(summary)
}

pub(crate) fn claude_session_summary_without_storage(
    paths: &ClaudePaths,
    key: &str,
) -> Result<ClaudeSessionSummary> {
    let (path, parsed) = resolve(paths, key)?;
    summary(
        paths,
        &path,
        &parsed,
        &activity::Snapshot::capture_readonly(paths),
    )
}
pub fn claude_session_detail(
    paths: &ClaudePaths,
    request: &ClaudeDetailRequest,
) -> Result<ClaudeSessionDetail> {
    let mut detail = claude_session_detail_without_storage(paths, request)?;
    attach_storage_size(paths, &mut detail.summary);
    Ok(detail)
}

pub(crate) fn claude_session_detail_without_storage(
    paths: &ClaudePaths,
    request: &ClaudeDetailRequest,
) -> Result<ClaudeSessionDetail> {
    let (path, parsed) = resolve(paths, &request.session_key)?;
    let limit = request.limit.unwrap_or(160).clamp(1, 500);
    let (start, end) = match crate::history_navigation::seek_window(
        parsed.events.iter().map(|event| event.id.as_str()),
        request.before.as_deref(),
        limit,
    )? {
        Some(window) => window,
        None => {
            let end = match &request.before {
                None => parsed.events.len(),
                Some(id) => parsed
                    .events
                    .iter()
                    .position(|event| &event.id == id)
                    .context("历史位置已变化，请刷新会话")?,
            };
            (end.saturating_sub(limit), end)
        }
    };
    let mut events: Vec<_> = parsed.events[start..end]
        .iter()
        .map(|event| (**event).clone())
        .collect();
    for event in &mut events {
        if let Some(user) = &mut event.user_message {
            user.refresh_files(Some(&parsed.cwd));
        }
    }
    let summary = summary(
        paths,
        &path,
        &parsed,
        &activity::Snapshot::capture_readonly(paths),
    )?;
    Ok(ClaudeSessionDetail {
        timeline: request.before.is_none().then(|| parsed.timeline.clone()),
        after_cursor: crate::history_navigation::after_cursor(
            end,
            parsed.events.len(),
            parsed
                .events
                .get(end.saturating_sub(1))
                .map(|event| event.id.as_str()),
        ),
        history: Some(parsed.history.metadata(None)),
        summary,
        events,
        total_events: parsed.events.len(),
        has_more: start > 0,
        before_cursor: (start > 0).then(|| parsed.events[start].id.clone()),
    })
}
pub fn read_attachment(
    paths: &ClaudePaths,
    request: &crate::user_message::SessionAttachmentRequest,
) -> Result<crate::user_message::SessionAttachmentResponse> {
    let (path, parsed) = resolve(paths, &request.session_key)?;
    let before = reader::stamp(&path)?;
    let mut user = parsed
        .events
        .iter()
        .filter_map(|e| e.user_message.as_ref())
        .find(|m| m.id == request.message_id)
        .cloned()
        .context("Claude 附件消息已变化或不存在")?;
    user.refresh_files(Some(&parsed.cwd));
    let result = user.read_attachment(&request.attachment_id)?;
    ensure!(
        before == reader::stamp(&path)?,
        "Claude 会话已变化，请刷新后重试"
    );
    Ok(result)
}
pub(crate) fn notification_snapshots(
    paths: &ClaudePaths,
) -> Result<crate::native_probe::NativeScan> {
    use crate::native_probe::NativeScan;
    let mut scan = NativeScan::default();
    let files = files(paths)?;
    if files.is_empty() {
        return Ok(scan);
    }
    let canonical_root = paths.projects.canonicalize()?;
    for path in files {
        match reader::probe_snapshot(&path, &canonical_root) {
            Ok(snapshot) => scan.streams.push(snapshot),
            Err(_) => scan.errors += 1,
        }
    }
    Ok(scan)
}

pub(crate) fn probe_snapshot_for_path(
    paths: &ClaudePaths,
    path: &Path,
) -> Result<crate::native_probe::NativeStreamSnapshot> {
    let canonical_root = paths.projects.canonicalize()?;
    reader::probe_snapshot(path, &canonical_root)
}

#[derive(Debug, Clone)]
pub struct ClaudePrompt {
    pub turn_id: String,
    pub timestamp_ms: i64,
    pub ambiguous: bool,
    pub cancelled: bool,
    pub last_activity_position: u64,
    pub last_activity_ms: Option<i64>,
    pub last_api_error_position: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeNotificationState {
    Current,
    Pending,
    Obsolete,
}
#[derive(Debug, Clone)]
pub struct ClaudeNotificationContext {
    pub stream: crate::native_probe::NativeStreamSnapshot,
    pub prompts: HashMap<String, ClaudePrompt>,
    pub path: PathBuf,
    pub project: String,
    pub current_turn: Option<String>,
    /// Verified native owner still reports delegated agents or workflows.
    pub native_busy: bool,
}
impl ClaudeNotificationContext {
    /// A terminal turn is not the thread's end while the verified native owner
    /// still reports delegated work that will wake the session. Polling uses
    /// this; a Stop hook carries its own in-flight background list instead.
    pub fn completion_deferred(&self, turn_id: &str) -> bool {
        self.native_busy && self.current_turn.as_deref() == Some(turn_id)
    }
    /// The prompt's pending request is a native question, so a permission
    /// notification for it must claim the same category as the transcript.
    pub fn pending_question(
        &self,
        prompt_id: &str,
    ) -> Option<&crate::native_probe::NativeTurnEvent> {
        if prompt_id.is_empty() {
            return None;
        }
        let mut question = None;
        let mut permission = false;
        for event in self
            .stream
            .events
            .iter()
            .filter(|event| event.prompt_id.as_deref() == Some(prompt_id))
        {
            match crate::native_probe::event_category(event) {
                "reply_needed" => question = question.or(Some(event)),
                "authorization" => permission = true,
                _ => {}
            }
        }
        question.filter(|_| !permission)
    }

    /// A hook is terminal evidence for its own prompt, not for the whole process.
    /// Successful historical turns remain deliverable; failures must still be current.
    pub fn hook_state(
        &self,
        prompt_id: &str,
        kind: &str,
        received_ms: i64,
        observed_records: Option<u64>,
    ) -> ClaudeNotificationState {
        use ClaudeNotificationState::*;
        let Some(prompt) = self.prompts.get(prompt_id).filter(|p| !p.ambiguous) else {
            return Pending;
        };
        if prompt.cancelled {
            return Obsolete;
        }
        if !matches!(kind, "completion" | "failure") {
            return Current;
        }
        let terminal = self.stream.events.iter().find(|e| {
            e.turn_id == prompt.turn_id && matches!(e.kind.as_str(), "completion" | "failure")
        });
        if kind == "completion" {
            if let Some(event) = terminal {
                return if event.kind == "completion" {
                    Current
                } else {
                    Obsolete
                };
            }
            return if self.current_turn.as_ref() != Some(&prompt.turn_id)
                || prompt.last_activity_ms.is_some_and(|ms| ms > received_ms)
            {
                Obsolete
            } else {
                Pending
            };
        }
        if self.current_turn.as_ref() != Some(&prompt.turn_id)
            || terminal.is_some_and(|e| e.kind == "completion")
            || observed_records.is_some_and(|n| prompt.last_activity_position > n)
            || prompt.last_activity_ms.is_some_and(|ms| ms > received_ms)
            || prompt
                .last_api_error_position
                .is_some_and(|n| prompt.last_activity_position > n)
        {
            Obsolete
        } else {
            Current
        }
    }
}
pub fn notification_context_for_key(
    paths: &ClaudePaths,
    key: &str,
) -> Result<ClaudeNotificationContext> {
    let (path, _) = resolve(paths, key)?;
    notification_context(paths, &path)
}
/// Targeted read for hook correlation and delivery verification; no full-provider scan.
pub fn notification_context(paths: &ClaudePaths, path: &Path) -> Result<ClaudeNotificationContext> {
    let root = paths.projects.canonicalize()?;
    let relative = path
        .strip_prefix(&paths.projects)
        .or_else(|_| path.strip_prefix(&root))
        .context("Claude notification path is outside session storage")?;
    ensure!(
        relative.components().count() == 2 && path.extension().is_some_and(|e| e == "jsonl"),
        "Claude notification source must be a main transcript"
    );
    let canonical_path = root.join(relative);
    let path = canonical_path.as_path();
    let mut component = root.clone();
    for part in relative.components() {
        component.push(part);
        ensure!(
            !fs::symlink_metadata(&component)?.file_type().is_symlink(),
            "Claude notification source contains a link"
        );
    }
    let before = reader::stamp(path)?;
    let parsed = reader::read(path)?;
    ensure!(
        parsed.issues.is_empty(),
        "Claude notification identity is unavailable"
    );
    let stream = crate::native_probe::NativeStreamSnapshot {
        provider: crate::native_probe::NativeProvider::Claude,
        session_key: key_for(&root, path, &parsed.id),
        id: parsed.id.clone(),
        title: crate::native_probe::notification_title(&parsed.title, &parsed.id, &parsed.cwd),
        identity: crate::native_probe::file_identity(path)?,
        record_count: parsed.record_count,
        settled_count: parsed.settled_count,
        events: parsed.native_events.clone(),
    };
    ensure!(
        before == reader::stamp(path)?,
        "Claude notification source changed during read"
    );
    let native_busy = activity::Snapshot::capture_readonly(paths)
        .assess(&parsed, path)
        .native_status()
        .is_some_and(|status| status == "busy");
    Ok(ClaudeNotificationContext {
        stream,
        prompts: parsed.prompts.clone(),
        current_turn: parsed.current_turn.clone(),
        native_busy,
        path: path.into(),
        project: Path::new(&parsed.cwd)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
    })
}
fn prompt_for_turn(parsed: &Parsed, turn: &str) -> Option<String> {
    parsed
        .prompts
        .iter()
        .find(|(_, p)| !p.ambiguous && p.turn_id == turn)
        .map(|(id, _)| id.clone())
}
