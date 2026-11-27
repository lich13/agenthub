//! Native Claude transcripts. Reading never starts the CLI or changes its configuration.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[allow(unused_imports)]
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
mod subagents;
#[cfg(test)]
mod tests;
pub(crate) use mutations::execute_claude_delete_batch;
pub(crate) use mutations::find_empty_inactive_sessions;
pub(crate) use mutations::DeletionContext;
pub use mutations::{execute_claude_delete, preview_claude_delete, rename_claude_session};
pub(crate) use reader::evict_idle_detail_cache;

#[derive(Debug, Clone)]
pub struct ClaudePaths {
    pub config: PathBuf,
    pub projects: PathBuf,
}
impl ClaudePaths {
    pub fn default_for_user() -> Result<Self> {
        let config = crate::platform::provider_root("CLAUDE_CONFIG_DIR", ".claude")?;
        Ok(Self {
            projects: config.join("projects"),
            config,
        })
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
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
    pub agent: Option<crate::codex::subagents::SubagentActivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagents: Option<crate::codex::subagents::SubagentCollection>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
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
    agent_names: HashMap<String, String>,
    timeline: crate::history_navigation::Timeline,
    id: String,
    title: String,
    cwd: String,
    /// Every working directory the session has recorded. A session that moves
    /// keeps the launch directory its native owner registered.
    cwds: Vec<String>,
    version: String,
    updated_at: Option<String>,
    events: Arc<Vec<Arc<ClaudeHistoryEvent>>>,
    message_count: usize,
    last_message: Option<String>,
    history: crate::jsonl_cache::HistoryJournal,
    issues: Vec<String>,
    has_fatal_issues: bool,
    has_partial_tail: bool,
    warnings: Vec<String>,
    last_turn_at: Option<i64>,
    last_terminal_at: Option<i64>,
    turn_open: bool,
    has_open_questions: bool,
    saw_sidechain: bool,
    saw_primary: bool,
    native_events: Vec<crate::native_probe::NativeTurnEvent>,
    prompts: HashMap<String, ClaudePrompt>,
    current_turn: Option<String>,
    record_count: u64,
    settled_count: u64,
}
fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
const MAX_LOCATED_SESSIONS: usize = 20_000;
type LocatedSessionKey = (PathBuf, String);
type LocatedSessionsCache = crate::lru::BoundedLruMap<LocatedSessionKey, PathBuf>;

static LOCATED_SESSIONS: std::sync::LazyLock<std::sync::Mutex<LocatedSessionsCache>> =
    std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(LocatedSessionsCache::new(MAX_LOCATED_SESSIONS))
    });
pub(super) fn evict_idle_located_sessions(max_idle: std::time::Duration) {
    if let Ok(mut cache) = LOCATED_SESSIONS.lock() {
        if cache.evict_idle(max_idle) {
            crate::note_cache_eviction();
        }
    }
}
fn key_for(root: &Path, path: &Path, _id: &str) -> String {
    let id = path.file_stem().unwrap_or_default().to_string_lossy();
    let key = format!(
        "claude:{}",
        crate::sha256_hex(format!(
            "{}\0{id}",
            path.strip_prefix(root).unwrap_or(path).to_string_lossy()
        ))
    );
    if let Ok(mut cache) = LOCATED_SESSIONS.lock() {
        // Keep the path scoped by its project root. The same relative
        // filename can exist in several machines or concurrent fixtures.
        cache.insert((root.to_path_buf(), key.clone()), path.into());
    }
    key
}

// A same-name JSONL owns its sibling directory, including when the main
// file is a link that explicit deletion must unlink without following it.
fn excluded_session_directory(path: &Path, depth: usize) -> bool {
    if matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("subagents" | "tool-results")
    ) {
        return true;
    }
    if depth < 2 {
        return false;
    }
    let Some(name) = path.file_name() else {
        return false;
    };
    let mut main_name = name.to_os_string();
    main_name.push(".jsonl");
    fs::symlink_metadata(path.with_file_name(main_name))
        .is_ok_and(|metadata| metadata.is_file() || metadata.file_type().is_symlink())
}

fn session_entry(entry: &walkdir::DirEntry) -> bool {
    !entry.file_type().is_dir() || !excluded_session_directory(entry.path(), entry.depth())
}

fn main_session_path(root: &Path, path: &Path) -> bool {
    crate::session_delete::within(root, path)
        && path
            .ancestors()
            .skip(1)
            .take_while(|parent| *parent != root)
            .all(|parent| {
                parent.strip_prefix(root).is_ok_and(|relative| {
                    !excluded_session_directory(parent, relative.components().count())
                })
            })
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
        .filter_entry(session_entry)
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
            has_fatal_issues: true,
            ..Default::default()
        })
    })
}
fn delete_file_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                meta.is_file() || meta.file_type().is_symlink(),
                "Claude 会话不是普通文件或符号链接"
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
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
        .and_then(|mut cache| cache.get(&(root.clone(), key.to_owned())))
    {
        if main_session_path(&root, &path) {
            delete_file_exists(&path)?;
            return Ok(path);
        }
    }
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(session_entry)
    {
        let entry = entry?;
        if (entry.file_type().is_file() || entry.file_type().is_symlink())
            && entry.path().extension().is_some_and(|ext| ext == "jsonl")
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
        .and_then(|mut cache| cache.get(&(root.clone(), key.to_owned())));
    if let Some(path) = located {
        // Revalidate every native identity, including the ancestor chain. The
        // lookup stores only paths discovered by our own bounded directory scan.
        let safe = main_session_path(&root, &path)
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
    // The key is derived from the path alone, so only the located file is parsed.
    for path in files(paths)? {
        if key_for(&root, &path, "") == key {
            let parsed = read_or_warning(&path, false);
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
    let live_streaming_tail = !parsed.has_fatal_issues
        && parsed.has_partial_tail
        && (matches!(assessment.native_status(), Some("busy" | "waiting"))
            || (owner == activity::Ownership::Open && parsed.turn_open));
    let fatal_reason =
        (!parsed.issues.is_empty() && !live_streaming_tail).then(|| parsed.issues.join("；"));
    let warning_reason = (!parsed.warnings.is_empty()).then(|| parsed.warnings.join("；"));
    let reason = if let Some(reason) = (!parsed.issues.is_empty()).then(|| parsed.issues.join("；"))
    {
        Some(reason)
    } else {
        assessment.block_reason()
    };
    let status = if !parsed.issues.is_empty() && !live_streaming_tail {
        "unknown"
    } else if let Some(native) = assessment.native_status() {
        // The verified owner publishes the live state: main loop or delegated
        // agents/workflows active, a prompt waiting for the user, or idle.
        match native {
            "busy" => {
                if is_native_busy_causally_stale(parsed, &assessment) {
                    if parsed.has_open_questions {
                        "waiting"
                    } else {
                        "recent"
                    }
                } else {
                    "running"
                }
            }
            "waiting" => "waiting",
            _ if parsed.has_open_questions => "waiting",
            _ => "recent",
        }
    } else if parsed.has_open_questions {
        "waiting"
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
    let read_error = fatal_reason;
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
        workspace_root: crate::workspace_root::resolve_workspace_root(&parsed.cwd),
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
        if is_agent_filename(&path) {
            continue;
        }
        let parsed = read_or_warning(&path, true);
        if parsed.saw_sidechain && !parsed.saw_primary {
            continue;
        }
        let s = summary_with_root(&canonical_projects, &path, &parsed, &snapshot)?;
        if needle.is_empty()
            || [&s.title, &s.id, &s.cwd]
                .iter()
                .any(|v| crate::services::search::contains_folded_str(v, &needle))
        {
            result.push(s);
        }
    }
    result.sort_by_cached_key(|session| {
        (std::cmp::Reverse(crate::timestamps::sort_key_nanos(
            session.updated_at.as_deref(),
        )),)
    });
    result.truncate(limit.clamp(1, 500));
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
    if let Some(agent_id) = &request.agent_id {
        return subagents::detail(paths, request, agent_id);
    }
    let (path, parsed) = resolve(paths, &request.session_key)?;
    let page = crate::history_navigation::paginate_window(
        &parsed.events,
        |event| event.id.as_str(),
        request.before.as_deref(),
        request.limit,
        false,
    )?;
    let mut events: Vec<_> = parsed.events[page.start..page.end]
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
        agent: None,
        subagents: Some(subagents::collection(paths, &path, &parsed)),
        timeline: request.before.is_none().then(|| parsed.timeline.clone()),
        after_cursor: page.after_cursor,
        history: Some(parsed.history.metadata(None)),
        summary,
        events,
        total_events: page.total,
        has_more: page.has_more,
        before_cursor: page.before_cursor,
    })
}
/// Visit the same newest-first pages as search without constructing UI detail.
pub(crate) fn visit_search_events(
    paths: &ClaudePaths,
    key: &str,
    max_pages: usize,
    mut visit: impl FnMut(&ClaudeHistoryEvent) -> Result<bool>,
) -> Result<()> {
    let (_, parsed) = resolve(paths, key)?;
    for page in parsed.events.rchunks(500).take(max_pages) {
        for event in page {
            let mut refreshed;
            let event = if event.user_message.is_some() {
                refreshed = (**event).clone();
                refreshed
                    .user_message
                    .as_mut()
                    .unwrap()
                    .refresh_files(Some(&parsed.cwd));
                &refreshed
            } else {
                event.as_ref()
            };
            if !visit(event)? {
                return Ok(());
            }
        }
    }
    Ok(())
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
        if is_agent_filename(&path) {
            continue;
        }
        match reader::probe_snapshot_if_primary(&path, &canonical_root) {
            Ok(Some(snapshot)) => scan.streams.push(snapshot),
            Ok(None) => {}
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
    let parsed = reader::read_probe_snapshot(path)?;
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
        events: parsed.native_events.clone().into(),
    };
    ensure!(
        before == reader::stamp(path)?,
        "Claude notification source changed during read"
    );
    let assessment_parsed = parsed.to_summary_parsed();
    let assessment = activity::Snapshot::capture_readonly(paths).assess(&assessment_parsed, path);
    let native_busy = assessment
        .native_status()
        .is_some_and(|status| status == "busy")
        && !is_native_busy_causally_stale(&assessment_parsed, &assessment);
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

fn is_agent_filename(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("agent-"))
}

fn is_native_busy_causally_stale(parsed: &Parsed, assessment: &activity::Assessment) -> bool {
    if parsed.turn_open {
        return false;
    }
    let settled_ms = parsed
        .last_terminal_at
        .or_else(|| parsed.updated_at.as_deref().and_then(reader::millis))
        .or(parsed.last_turn_at);
    assessment
        .native_status_updated_at()
        .zip(settled_ms)
        .is_some_and(|(status_ms, terminal_ms)| terminal_ms > status_ms)
}
fn prompt_for_turn(parsed: &Parsed, turn: &str) -> Option<String> {
    parsed
        .prompts
        .iter()
        .find(|(_, p)| !p.ambiguous && p.turn_id == turn)
        .map(|(id, _)| id.clone())
}
