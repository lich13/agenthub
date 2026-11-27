use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub mod account_scope;
mod app_server_client;
mod assistant_question;
mod identity;
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

pub use crate::selected_codex::set_thread_archived;
pub use app_server_client::{set_thread_title, CodexAppServerClient};
pub use assistant_question::assistant_feedback_request;
pub(crate) use identity::codex_task_identity_from_connection;
pub use identity::{codex_task_identity, CodexTaskIdentity};
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
        Some(paths),
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
    paths: Option<&CodexPaths>,
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
        if let Some(paths) = paths {
            use crate::session_storage::{self, SessionStorageSize, StorageScope};
            row.storage_size = Some(if storage_valid {
                session_storage::file_size_with_canonical_root(&paths.home, canonical_root, path)
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
            if codex_rollout_turn_can_be_live(paths, &row.id, path) {
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

type CodexHostSnapshot = (
    bool,
    std::collections::HashSet<(u64, u64)>,
    std::time::Instant,
);

static CODEX_HOST_PROCESS_CACHE: std::sync::Mutex<Option<(u32, CodexHostSnapshot)>> =
    std::sync::Mutex::new(None);

fn is_managed_codex_home(root: &Path) -> bool {
    root.file_name().and_then(|s| s.to_str()) == Some(".codex")
        || root.join("thread-writer-locks").is_dir()
        || root.join("auth.json").is_file()
        || root.join("version.json").is_file()
        || dirs::home_dir()
            .is_some_and(|home| crate::platform::same_canonical_dir(root, &home.join(".codex")))
}

fn codex_rollout_turn_can_be_live(
    paths: Option<&CodexPaths>,
    id: &str,
    rollout_path: &Path,
) -> bool {
    #[cfg(test)]
    if let Some(override_alive) = TEST_CODEX_HOST_ALIVE.with(std::cell::Cell::get) {
        return override_alive;
    }
    let Some(paths) = paths else {
        return true;
    };
    if crate::selected_codex::tree::probe_writer_locked(paths, id).unwrap_or(false) {
        return true;
    }
    if !is_managed_codex_home(&paths.home) {
        return true;
    }
    #[cfg(unix)]
    let (has_worker, open_inodes) = {
        use std::os::unix::fs::MetadataExt;
        let target_uid = fs::symlink_metadata(rollout_path)
            .or_else(|_| fs::symlink_metadata(&paths.home))
            .map(|m| m.uid())
            .unwrap_or_else(|_| unsafe { libc::geteuid() });
        let (has_worker, open_inodes) = codex_host_snapshot_for_uid(target_uid);
        if let Ok(meta) = fs::metadata(rollout_path) {
            if open_inodes.contains(&(meta.dev(), meta.ino())) {
                return true;
            }
        }
        (has_worker, open_inodes)
    };
    #[cfg(not(unix))]
    let (has_worker, _open_inodes) = (true, std::collections::HashSet::<(u64, u64)>::new());

    let _ = open_inodes;
    if !has_worker {
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

#[cfg(unix)]
fn codex_host_snapshot_for_uid(target_uid: u32) -> (bool, std::collections::HashSet<(u64, u64)>) {
    if let Ok(guard) = CODEX_HOST_PROCESS_CACHE.lock() {
        if let Some((cached_uid, (alive, ref inodes, ts))) = *guard {
            if cached_uid == target_uid && ts.elapsed() < std::time::Duration::from_secs(2) {
                return (alive, inodes.clone());
            }
        }
    }
    let (alive, inodes) = probe_live_codex_host_for_uid(target_uid);
    if let Ok(mut guard) = CODEX_HOST_PROCESS_CACHE.lock() {
        *guard = Some((
            target_uid,
            (alive, inodes.clone(), std::time::Instant::now()),
        ));
    }
    (alive, inodes)
}

pub(crate) fn is_codex_binary_name(base: &str) -> bool {
    matches!(base, "codex" | "Codex" | "Codex Helper")
        || base.starts_with("codex-aarch64-")
        || base.starts_with("codex-x86_64-")
}

pub(crate) fn is_pure_codex_managed_daemon_args(args: &[String]) -> bool {
    let has_app_server = args.iter().skip(1).any(|a| a == "app-server");
    let has_managed_daemon = args.iter().skip(1).any(|a| a == "--managed-daemon");
    has_app_server && has_managed_daemon
}

pub(crate) fn classify_codex_process(
    exe: Option<&Path>,
    fallback_comm: &str,
    args: &[String],
) -> (bool, bool) {
    let exe_base = exe.and_then(|p| p.file_name()).and_then(|n| n.to_str());
    let arg0_base = args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .and_then(|n| n.to_str());
    let comm_base = Path::new(fallback_comm.trim())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(fallback_comm.trim());

    let is_node_wrapper = matches!(exe_base.or(arg0_base).unwrap_or(comm_base), "node" | "bun")
        && args
            .iter()
            .skip(1)
            .any(|a| a.contains("/@openai/codex") || a.ends_with("/bin/codex.js"));

    let is_codex = is_node_wrapper
        || exe_base.is_some_and(is_codex_binary_name)
        || arg0_base.is_some_and(is_codex_binary_name)
        || is_codex_binary_name(comm_base)
        || comm_base.starts_with("codex-x86_64-")
        || comm_base.starts_with("codex-aarch64-");
    if !is_codex {
        return (false, false);
    }
    let is_worker = !is_pure_codex_managed_daemon_args(args);
    (true, is_worker)
}

#[cfg(unix)]
fn probe_live_codex_host_for_uid(target_uid: u32) -> (bool, std::collections::HashSet<(u64, u64)>) {
    let mut inodes = std::collections::HashSet::new();
    let Ok(output) = std::process::Command::new("/bin/ps")
        .env("LC_ALL", "C")
        .args(["-ww", "-axo", "pid=,uid=,stat=,comm=,args="])
        .output()
    else {
        return (true, inodes);
    };
    if !output.status.success() {
        return (true, inodes);
    }
    let mut has_worker = false;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        let Some(uid) = fields.next().and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        if uid != target_uid {
            continue;
        }
        let Some(stat) = fields.next() else {
            continue;
        };
        if stat.starts_with('Z') {
            continue;
        }
        let Some(comm) = fields.next() else {
            continue;
        };
        let ps_args: Vec<String> = fields.map(str::to_owned).collect();
        if !comm.contains("codex")
            && !comm.contains("Codex")
            && !matches!(comm, "node" | "bun")
            && !ps_args
                .iter()
                .any(|a| a.contains("codex") || a.contains("Codex"))
        {
            continue;
        }
        let exe = crate::platform::resolve_kernel_exe(pid);
        let args = crate::platform::resolve_kernel_args(pid).unwrap_or(ps_args);
        let (is_codex, is_worker) = classify_codex_process(exe.as_deref(), comm, &args);
        if !is_codex {
            continue;
        }
        if is_worker {
            has_worker = true;
        }
        if let Some((_, pid_inodes)) = crate::platform::collect_kernel_open_vnodes(pid) {
            inodes.extend(pid_inodes);
        }
    }
    (has_worker, inodes)
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
