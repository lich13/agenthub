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

pub use app_server_client::{set_thread_title, CodexAppServerClient};
pub use assistant_question::assistant_feedback_request;
pub(crate) use identity::codex_task_identity_from_connection;
pub use identity::{codex_task_identity, CodexTaskIdentity};
pub use mutations::set_thread_archived;
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
    rollout_has_running_signal, visit_search_blocks, ThreadVisibilityMetadata,
};
pub use rollout_events::{
    is_macos_network_volume_path, rollout_completion_last_agent_message,
    rollout_completion_last_agent_message_selection, rollout_hook_stop_message_selection,
    rollout_pending_feedback, rollout_request_user_input_state, thread_detail_page_from_summary,
    AssistantFeedback, RolloutMessageSelection, RolloutRequestUserInputState,
};
#[cfg(test)]
use rollout_events::{
    is_request_user_input, message_blocks_from_events, parse_message_event, RolloutScan,
};
pub use rollout_file::is_compressed as is_compressed_rollout_path;
use session_index::{read_session_index, SessionIndexEntry};
pub(crate) use thread_rows::coalesce_non_empty as coalesce_non_empty_columns;
use thread_rows::read_thread_rows_filtered;
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
    list_threads_inner(paths, archived, q, limit, true)
}

pub(crate) fn list_threads_without_storage(
    paths: &CodexPaths,
    archived: bool,
    q: Option<&str>,
    limit: usize,
) -> Result<Vec<ThreadSummary>> {
    list_threads_inner(paths, archived, q, limit, false)
}

fn list_threads_inner(
    paths: &CodexPaths,
    archived: bool,
    q: Option<&str>,
    limit: usize,
    include_storage: bool,
) -> Result<Vec<ThreadSummary>> {
    let rows = read_thread_rows_filtered(paths, Some(archived))?;
    let session_index = read_session_index(paths).unwrap_or_default();
    let canonical_home = (!is_macos_network_volume_path(&paths.home))
        .then(|| fs::canonicalize(&paths.home).ok())
        .flatten();
    let storage_canonical_home = canonical_home.as_deref().filter(|_| {
        fs::symlink_metadata(&paths.home)
            .map(|m| !m.file_type().is_symlink())
            .unwrap_or(false)
    });
    let mut enriched = Vec::with_capacity(rows.len());
    for mut row in rows {
        let index_entry = session_index.get(&row.summary.id);
        if let Some(storage_valid) = enrich_local_thread_row_with_home(
            paths,
            canonical_home.as_deref(),
            storage_canonical_home,
            &mut row,
            index_entry,
            true,
            false,
        ) {
            enriched.push((row.summary, storage_valid));
        }
    }

    let needle = q.map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty());
    if let Some(needle) = &needle {
        enriched.retain(|(row, _)| {
            crate::services::search::contains_folded_str(&row.title, needle)
                || crate::services::search::contains_folded_str(&row.id, needle)
                || row
                    .latest_message
                    .as_deref()
                    .is_some_and(|m| crate::services::search::contains_folded_str(m, needle))
                || row
                    .cwd
                    .as_deref()
                    .is_some_and(|c| crate::services::search::contains_folded_str(c, needle))
        });
    }
    enriched.sort_by_cached_key(|(row, _)| row.message_time_sort_key());
    enriched.truncate(limit.max(1));
    let mut workspace_root_memo: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for (row, _) in &mut enriched {
        row.workspace_root = row.cwd.as_deref().and_then(|cwd| {
            if let Some(cached) = workspace_root_memo.get(cwd) {
                return cached.clone();
            }
            let resolved = crate::workspace_root::resolve_workspace_root(cwd);
            workspace_root_memo.insert(cwd.to_owned(), resolved.clone());
            resolved
        });
    }
    if include_storage {
        use crate::session_storage::{self, SessionStorageSize, StorageScope};
        for (row, storage_valid) in &mut enriched {
            if let Some(path) = row.rollout_path.as_deref() {
                row.storage_size = Some(if *storage_valid {
                    session_storage::file_size_with_canonical_root(
                        &paths.home,
                        storage_canonical_home,
                        path,
                    )
                } else {
                    SessionStorageSize::unavailable(StorageScope::File)
                });
            }
        }
    }
    Ok(enriched.into_iter().map(|(row, _)| row).collect())
}

pub fn local_thread_summary(paths: &CodexPaths, id: &str) -> Result<Option<ThreadSummary>> {
    let Some(mut row) = thread_rows::read_thread_rows_matching(paths, Some(id))?.pop() else {
        return Ok(None);
    };
    let index = read_session_index(paths).unwrap_or_default();
    let visible = enrich_local_thread_row(paths, &mut row, index.get(id));
    Ok(visible.then_some(row.summary))
}

fn resolve_missing_notification_rollout_path(
    paths: &CodexPaths,
    canonical_home: &mut Option<Option<PathBuf>>,
    summary: &mut ThreadSummary,
) {
    let Some(raw_path) = summary.rollout_path.as_deref() else {
        return;
    };
    if raw_path.is_file() {
        return;
    }
    let home = canonical_home
        .get_or_insert_with(|| {
            (!is_macos_network_volume_path(&paths.home))
                .then(|| fs::canonicalize(&paths.home).ok())
                .flatten()
        })
        .as_deref();
    if let Some(resolved) = resolve_existing_rollout_path_with_home(paths, home, raw_path, false) {
        summary.rollout_path = Some(resolved);
    }
}

/// Batch notification header + task identity read on a single SQLite connection.
pub fn notification_thread_headers_with_identities(
    paths: &CodexPaths,
) -> Result<Vec<(ThreadSummary, CodexTaskIdentity)>> {
    let mut rows = thread_rows::read_notification_thread_headers_with_identities(paths)?;
    let mut canonical_home = None;
    for (summary, _) in &mut rows {
        resolve_missing_notification_rollout_path(paths, &mut canonical_home, summary);
    }
    Ok(rows)
}

/// Recheck one notification source without loading other threads or history.
pub fn notification_thread_header(paths: &CodexPaths, id: &str) -> Result<Option<ThreadSummary>> {
    let mut canonical_home = None;
    Ok(thread_rows::read_thread_rows_matching(paths, Some(id))?
        .into_iter()
        .map(|row| row.summary)
        .find(|thread| !matches!(thread.status, ThreadStatus::Archived))
        .map(|mut summary| {
            resolve_missing_notification_rollout_path(paths, &mut canonical_home, &mut summary);
            summary
        }))
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
    let visible = enrich_local_thread_row_with_home(
        paths,
        canonical_home.as_deref(),
        storage_canonical_home,
        row,
        index_entry,
        true,
        true,
    )
    .is_some();
    if visible {
        row.summary.workspace_root = row
            .summary
            .cwd
            .as_deref()
            .and_then(crate::workspace_root::resolve_workspace_root);
    }
    visible
}

fn enrich_local_thread_row_with_home(
    paths: &CodexPaths,
    canonical_home: Option<&Path>,
    storage_canonical_home: Option<&Path>,
    row: &mut thread_rows::LocalThreadRow,
    index_entry: Option<&SessionIndexEntry>,
    cwd_already_resolved: bool,
    compute_storage_immediately: bool,
) -> Option<bool> {
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
                return None;
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
    let (is_subagent, storage_valid) = enrich_thread_from_rollout_inner(
        &mut row.summary,
        Some(&paths.home),
        storage_canonical_home,
        cwd_already_resolved,
        compute_storage_immediately,
    )
    .unwrap_or((false, false));
    (!is_subagent).then_some(storage_valid)
}

pub(super) fn resolve_existing_rollout_path_with_home(
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
    enrich_thread_from_rollout_inner(row, None, None, false, false).map(|(sub, _)| sub)
}

fn enrich_thread_from_rollout_inner(
    row: &mut ThreadSummary,
    root: Option<&Path>,
    canonical_root: Option<&Path>,
    cwd_already_resolved: bool,
    compute_storage_immediately: bool,
) -> Result<(bool, bool)> {
    let Some(path) = &row.rollout_path else {
        return Ok((false, false));
    };
    let scan = scan_rollout(path, 80)?;
    let storage_valid = !scan.identity_conflict && scan.session_id.as_deref() == Some(&row.id);
    if compute_storage_immediately {
        if let Some(root) = root {
            use crate::session_storage::{self, SessionStorageSize, StorageScope};
            row.storage_size = Some(if storage_valid {
                session_storage::file_size_with_canonical_root(root, canonical_root, path)
            } else {
                SessionStorageSize::unavailable(StorageScope::File)
            });
        }
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
    Ok((scan.is_subagent, storage_valid))
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
    let is_default_home = dirs::home_dir()
        .is_some_and(|home| crate::platform::same_canonical_dir(root, &home.join(".codex")));
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
