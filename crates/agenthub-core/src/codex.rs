use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub mod account_scope;
mod app_server_client;
mod assistant_question;
mod identity;
mod mutations;
mod name_client;
mod paths;
#[cfg(test)]
mod rollout_accuracy_tests;
mod rollout_events;
pub(crate) mod rollout_file;
mod session_index;
pub mod subagents;
#[cfg(test)]
mod tests;
mod thread_rows;
mod types;
mod user_input;

pub use app_server_client::CodexAppServerClient;
pub use assistant_question::assistant_feedback_request;
pub(crate) use identity::codex_task_identity_from_connection;
pub use identity::{codex_task_identity, CodexTaskIdentity};
pub use mutations::set_thread_archived;
pub use name_client::set_thread_title;
pub use paths::{
    resolve_codex_paths, resolve_codex_paths_with_options, CodexPathDiscoveryOptions, CodexPaths,
    ResolvedCodexPaths,
};
use rollout_events::{
    first_user_message_title, is_usable_thread_title, scan_rollout,
    should_repair_thread_title_from_local_metadata,
};
pub(crate) use rollout_events::{
    hidden_thread_metadata_category, rollout_has_confirmed_inactive_state,
    rollout_has_running_signal, ThreadVisibilityMetadata,
};
pub use rollout_events::{
    is_macos_network_volume_path, rollout_completion_last_agent_message,
    rollout_completion_last_agent_message_selection,
    rollout_completion_last_agent_message_with_source, rollout_hook_stop_message_selection,
    rollout_pending_feedback, rollout_request_user_input_state, thread_detail_page_from_summary,
    AssistantFeedback, RolloutMessageSelection, RolloutRequestUserInputState,
};
#[cfg(test)]
use rollout_events::{
    is_request_user_input, message_blocks_from_events, parse_message_event, RolloutScan,
};
use session_index::{read_session_index, SessionIndexEntry};
pub(crate) use thread_rows::coalesce_non_empty as coalesce_non_empty_columns;
pub use thread_rows::hidden_and_archived_thread_ids;
use thread_rows::read_thread_rows;
pub use types::{
    extract_proposed_plan_text, CodexMessage, MessageBlock, PendingElicitation, ThreadDetail,
    ThreadStatus, ThreadSummary, UserInputAnswer, UserInputOption, UserInputQuestion,
};
pub use user_input::{
    is_async_question_tool, is_question_tool, normalize_user_input_questions,
    rollout_async_questions, rollout_pending_async_questions_since, AsyncQuestionCall,
};

pub(crate) fn evict_idle_detail_cache(max_idle: std::time::Duration) {
    rollout_events::evict_idle_detail_cache(max_idle);
    rollout_events::evict_idle_scan_cache(max_idle);
    session_index::evict_idle_index_cache(max_idle);
    user_input::evict_idle_question_cache(max_idle);
    subagents::evict_idle(max_idle);
}

pub fn list_threads(
    paths: &CodexPaths,
    archived: bool,
    q: Option<&str>,
    limit: usize,
) -> Result<Vec<ThreadSummary>> {
    let mut rows = read_thread_rows(paths)?;
    rows.retain(|row| matches!(row.summary.status, ThreadStatus::Archived) == archived);
    let session_index = read_session_index(paths).unwrap_or_default();
    let canonical_home = (!is_macos_network_volume_path(&paths.home))
        .then(|| fs::canonicalize(&paths.home).ok())
        .flatten();
    let storage_canonical_home = canonical_home.as_deref().filter(|_| {
        fs::symlink_metadata(&paths.home)
            .map(|m| !m.file_type().is_symlink())
            .unwrap_or(false)
    });
    rows.retain_mut(|row| {
        enrich_local_thread_row_with_home(
            paths,
            canonical_home.as_deref(),
            storage_canonical_home,
            row,
            session_index.get(&row.summary.id),
            true,
        )
    });
    let mut rows = rows.into_iter().map(|row| row.summary).collect::<Vec<_>>();

    let needle = q.map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty());
    rows.retain(|row| {
        if let Some(needle) = &needle {
            let title = row.title.to_lowercase();
            let id = row.id.to_lowercase();
            let latest = row
                .latest_message
                .as_deref()
                .unwrap_or_default()
                .to_lowercase();
            let cwd = row.cwd.as_deref().unwrap_or_default().to_lowercase();
            title.contains(needle)
                || id.contains(needle)
                || latest.contains(needle)
                || cwd.contains(needle)
        } else {
            true
        }
    });
    ThreadSummary::sort_by_message_time(&mut rows);
    rows.truncate(limit.max(1));
    Ok(rows)
}

pub fn local_thread_summary(paths: &CodexPaths, id: &str) -> Result<Option<ThreadSummary>> {
    let Some(mut row) = thread_rows::read_thread_rows_matching(paths, Some(id))?.pop() else {
        return Ok(None);
    };
    let index = read_session_index(paths).unwrap_or_default();
    let visible = enrich_local_thread_row(paths, &mut row, index.get(id));
    Ok(visible.then_some(row.summary))
}

/// Notification discovery reads headers only; it never scans every historical rollout.
pub fn notification_thread_headers(paths: &CodexPaths) -> Result<Vec<ThreadSummary>> {
    Ok(read_thread_rows(paths)?
        .into_iter()
        .map(|row| row.summary)
        .filter(|thread| !matches!(thread.status, ThreadStatus::Archived))
        .collect())
}

/// Recheck one notification source without loading other threads or history.
pub fn notification_thread_header(paths: &CodexPaths, id: &str) -> Result<Option<ThreadSummary>> {
    Ok(thread_rows::read_thread_rows_matching(paths, Some(id))?
        .into_iter()
        .map(|row| row.summary)
        .find(|thread| !matches!(thread.status, ThreadStatus::Archived)))
}

fn enrich_local_thread_row(
    paths: &CodexPaths,
    row: &mut thread_rows::LocalThreadRow,
    index_entry: Option<&SessionIndexEntry>,
) -> bool {
    let canonical_home = (!is_macos_network_volume_path(&paths.home))
        .then(|| fs::canonicalize(&paths.home).ok())
        .flatten();
    let storage_canonical_home = canonical_home.as_deref().filter(|_| {
        fs::symlink_metadata(&paths.home)
            .map(|m| !m.file_type().is_symlink())
            .unwrap_or(false)
    });
    enrich_local_thread_row_with_home(
        paths,
        canonical_home.as_deref(),
        storage_canonical_home,
        row,
        index_entry,
        true,
    )
}

fn enrich_local_thread_row_with_home(
    paths: &CodexPaths,
    canonical_home: Option<&Path>,
    storage_canonical_home: Option<&Path>,
    row: &mut thread_rows::LocalThreadRow,
    index_entry: Option<&SessionIndexEntry>,
    cwd_already_resolved: bool,
) -> bool {
    // Native updated_at also changes on metadata writes; only messages order the list.
    row.summary.updated_at = None;
    if row.summary.rollout_path.is_none() {
        row.summary.rollout_path = index_entry.and_then(|entry| entry.path.clone());
    }
    let is_archived = matches!(row.summary.status, ThreadStatus::Archived);
    if let Some(raw_path) = row.summary.rollout_path.take() {
        match resolve_existing_rollout_path_with_home(paths, canonical_home, &raw_path, is_archived)
        {
            Some(resolved) => row.summary.rollout_path = Some(resolved),
            None if !raw_path.as_os_str().is_empty()
                && !is_macos_network_volume_path(&paths.home)
                && !is_macos_network_volume_path(&raw_path)
                && path_has_ancestor_inside_canonical_home(canonical_home, &raw_path) =>
            {
                return false;
            }
            None => {}
        }
    }
    if should_repair_thread_title_from_local_metadata(
        row.db_title.as_deref(),
        row.first_user_message.as_deref(),
        index_entry,
    ) {
        row.summary.title = index_entry
            .and_then(SessionIndexEntry::title_candidate)
            .or_else(|| {
                row.first_user_message
                    .as_deref()
                    .and_then(first_user_message_title)
            })
            .unwrap_or_else(|| "未命名线程".to_string());
    }
    !enrich_thread_from_rollout_inner(
        &mut row.summary,
        Some(&paths.home),
        storage_canonical_home,
        cwd_already_resolved,
    )
    .unwrap_or(false)
}

fn resolve_existing_rollout_path(
    paths: &CodexPaths,
    path: &Path,
    archived: bool,
) -> Option<PathBuf> {
    let canonical_home = (!is_macos_network_volume_path(&paths.home))
        .then(|| fs::canonicalize(&paths.home).ok())
        .flatten();
    resolve_existing_rollout_path_with_home(paths, canonical_home.as_deref(), path, archived)
}

fn resolve_existing_rollout_path_with_home(
    paths: &CodexPaths,
    canonical_home: Option<&Path>,
    path: &Path,
    archived: bool,
) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    let home = canonical_home?;
    let plain = rollout_file::plain_path(path);
    if paths.contains_path_with_canonical_home(home, &plain) {
        return Some(plain);
    }
    if path != plain && paths.contains_path_with_canonical_home(home, path) {
        return Some(path.to_path_buf());
    }
    let compressed = rollout_file::compressed_path(path);
    if compressed != path && paths.contains_path_with_canonical_home(home, &compressed) {
        return Some(compressed);
    }
    if archived && path_has_ancestor_inside_canonical_home(Some(home), path) {
        if let Some(name) = path.file_name() {
            let relocated = paths.home.join("archived_sessions").join(name);
            let rel_plain = rollout_file::plain_path(&relocated);
            if paths.contains_path_with_canonical_home(home, &rel_plain) {
                return Some(rel_plain);
            }
            if relocated != rel_plain && paths.contains_path_with_canonical_home(home, &relocated) {
                return Some(relocated);
            }
            let rel_compressed = rollout_file::compressed_path(&relocated);
            if rel_compressed != relocated
                && paths.contains_path_with_canonical_home(home, &rel_compressed)
            {
                return Some(rel_compressed);
            }
        }
    }
    None
}

fn path_has_ancestor_inside_canonical_home(canonical_home: Option<&Path>, path: &Path) -> bool {
    if path.as_os_str().is_empty() || is_macos_network_volume_path(path) {
        return false;
    }
    let Some(home) = canonical_home else {
        return false;
    };
    let mut ancestor = path.parent();
    while let Some(candidate) = ancestor {
        if candidate.as_os_str().is_empty() {
            return false;
        }
        if candidate.exists() {
            let Ok(existing) = fs::canonicalize(candidate) else {
                return false;
            };
            return existing.starts_with(home);
        }
        ancestor = candidate.parent();
    }
    false
}

pub fn enrich_thread_from_rollout(row: &mut ThreadSummary) -> Result<bool> {
    enrich_thread_from_rollout_inner(row, None, None, false)
}

fn enrich_thread_from_rollout_inner(
    row: &mut ThreadSummary,
    root: Option<&Path>,
    canonical_root: Option<&Path>,
    cwd_already_resolved: bool,
) -> Result<bool> {
    let Some(path) = &row.rollout_path else {
        return Ok(false);
    };
    let scan = scan_rollout(path, 80)?;
    if let Some(root) = root {
        use crate::session_storage::{self, SessionStorageSize, StorageScope};
        row.storage_size = Some(
            if !scan.identity_conflict && scan.session_id.as_deref() == Some(&row.id) {
                session_storage::file_size_with_canonical_root(root, canonical_root, path)
            } else {
                SessionStorageSize::unavailable(StorageScope::File)
            },
        );
    }
    row.message_count = scan.message_count;
    row.latest_message = scan.latest_message;
    if row.activity_updated_at.is_none() {
        row.activity_updated_at = row.updated_at.clone();
    }
    row.updated_at = scan.last_message_at;
    let mut active_turn_id = scan.active_turn_id;
    if !matches!(row.status, ThreadStatus::Archived) {
        if scan.recoverable {
            row.status = ThreadStatus::Recoverable;
        } else if scan.reply_needed {
            row.status = ThreadStatus::ReplyNeeded;
        } else if scan.running {
            if codex_rollout_turn_can_be_live(root, path) {
                row.status = ThreadStatus::Running;
            } else {
                row.status = ThreadStatus::Recoverable;
                active_turn_id = None;
            }
        } else if matches!(
            row.status,
            ThreadStatus::Running | ThreadStatus::ReplyNeeded | ThreadStatus::Recoverable
        ) {
            row.status = ThreadStatus::Recent;
        }
    }
    let mut cwd_changed = false;
    if let Some(scan_cwd) = scan.cwd {
        if row.cwd.as_deref() != Some(scan_cwd.as_str()) {
            row.cwd = Some(scan_cwd);
            cwd_changed = true;
        }
    }
    if scan.model.is_some() {
        row.model = scan.model;
    }
    if cwd_changed || (!cwd_already_resolved && row.workspace_root.is_none()) {
        row.workspace_root = row
            .cwd
            .as_deref()
            .and_then(crate::workspace_root::resolve_workspace_root);
    }
    if !is_usable_thread_title(&row.title) {
        row.title = scan
            .title
            .or(scan.first_user_message_title)
            .unwrap_or_else(|| "未命名线程".to_string());
    }
    row.active_turn_id = active_turn_id;
    row.pending_elicitation = scan.pending_elicitation;
    row.last_event_kind = scan.last_event_kind;
    Ok(scan.is_subagent)
}

#[cfg(test)]
thread_local! {
    static TEST_CODEX_HOST_ALIVE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

static CODEX_HOST_PROCESS_CACHE: std::sync::Mutex<Option<(bool, std::time::Instant)>> =
    std::sync::Mutex::new(None);

fn codex_rollout_turn_can_be_live(root: Option<&Path>, rollout_path: &Path) -> bool {
    #[cfg(test)]
    if let Some(override_alive) = TEST_CODEX_HOST_ALIVE.with(std::cell::Cell::get) {
        return override_alive;
    }
    let Some(root) = root else {
        return true;
    };
    let is_default_home =
        dirs::home_dir().is_some_and(|home| same_canonical_dir(root, &home.join(".codex")));
    if !is_default_home {
        return true;
    }
    if !has_live_codex_host_process() {
        return false;
    }
    if let Ok(modified) = fs::metadata(rollout_path).and_then(|m| m.modified()) {
        if let Ok(elapsed) = modified.elapsed() {
            if elapsed > std::time::Duration::from_secs(6 * 3600) {
                return false;
            }
        }
    }
    true
}

fn same_canonical_dir(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}

fn has_live_codex_host_process() -> bool {
    if let Ok(guard) = CODEX_HOST_PROCESS_CACHE.lock() {
        if let Some((alive, ts)) = *guard {
            if ts.elapsed() < std::time::Duration::from_secs(2) {
                return alive;
            }
        }
    }
    let alive = probe_live_codex_host_process();
    if let Ok(mut guard) = CODEX_HOST_PROCESS_CACHE.lock() {
        *guard = Some((alive, std::time::Instant::now()));
    }
    alive
}

fn probe_live_codex_host_process() -> bool {
    #[cfg(unix)]
    {
        let current_uid = unsafe { libc::geteuid() };
        let Ok(output) = std::process::Command::new("/bin/ps")
            .env("LC_ALL", "C")
            .args(["-axo", "uid=,stat=,comm="])
            .output()
        else {
            return true;
        };
        if !output.status.success() {
            return true;
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut fields = line.split_whitespace();
            let Some(uid) = fields.next().and_then(|v| v.parse::<u32>().ok()) else {
                continue;
            };
            if uid != current_uid {
                continue;
            }
            let Some(stat) = fields.next() else {
                continue;
            };
            if stat.starts_with('Z') {
                continue;
            }
            let comm = fields.collect::<Vec<_>>().join(" ");
            let base = Path::new(comm.trim())
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(comm.trim());
            if matches!(
                base,
                "codex"
                    | "codex-aarch64-apple-darwin"
                    | "codex-x86_64-apple-darwin"
                    | "codex-x86_64-unknown-linux-gnu"
                    | "codex-aarch64-unknown-linux-gnu"
                    | "Codex"
                    | "Codex Helper"
            ) {
                return true;
            }
        }
        false
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn thread_detail_page(
    paths: &CodexPaths,
    id: &str,
    limit: Option<usize>,
    before: Option<&str>,
    since: Option<&str>,
) -> Result<Option<ThreadDetail>> {
    let Some(summary) = local_thread_summary(paths, id)? else {
        return Ok(None);
    };
    rollout_events::thread_detail_page_from_summary(summary, true, limit, before, since).map(Some)
}
