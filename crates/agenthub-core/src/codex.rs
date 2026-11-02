use anyhow::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

mod app_server_client;
mod assistant_question;
#[cfg(unix)]
pub mod desktop;
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
mod test_support;
#[cfg(test)]
mod tests;
mod thread_rows;
mod types;
mod user_input;

pub use app_server_client::CodexAppServerClient;
pub use assistant_question::assistant_feedback_request;
pub use identity::{codex_task_identity, CodexTaskIdentity};
pub use mutations::{db_integrity, set_thread_archived};
pub use name_client::set_thread_title;
pub use paths::{
    resolve_codex_paths, resolve_codex_paths_with_options, CodexPathDiscoveryOptions, CodexPaths,
    ResolvedCodexPaths,
};
pub(crate) use rollout_events::{
    evict_idle_detail_cache, hidden_thread_metadata_category, rollout_has_confirmed_inactive_state,
    rollout_has_running_signal, ThreadVisibilityMetadata,
};
use rollout_events::{
    first_user_message_title, is_usable_thread_title, scan_rollout,
    should_repair_thread_title_from_local_metadata,
};
pub use rollout_events::{
    is_macos_network_volume_path, message_blocks_from_events,
    rollout_completion_last_agent_message, rollout_completion_last_agent_message_selection,
    rollout_completion_last_agent_message_with_source, rollout_has_completed_turn,
    rollout_hook_stop_message, rollout_hook_stop_message_selection,
    rollout_hook_stop_message_with_source, rollout_latest_assistant_message,
    rollout_pending_feedback, rollout_request_user_input_state, thread_detail_from_summary,
    thread_detail_page_from_summary, window_thread_detail, AssistantFeedback,
    RolloutMessageSelection, RolloutRequestUserInputState,
};
#[cfg(test)]
use rollout_events::{is_request_user_input, parse_message_event, RolloutScan};
use session_index::{read_session_index, SessionIndexEntry};
use thread_rows::read_thread_rows;
pub use thread_rows::{archived_thread_ids, hidden_thread_ids, thread_source_counts};
pub use types::{
    extract_proposed_plan_text, CodexMessage, MessageBlock, PendingElicitation, ThreadDetail,
    ThreadStatus, ThreadSummary, UserInputAnswer, UserInputOption, UserInputQuestion,
};
pub use user_input::{
    is_async_question_tool, is_question_tool, normalize_user_input_questions,
    rollout_async_questions, AsyncQuestionCall,
};

pub fn list_threads(
    paths: &CodexPaths,
    archived: bool,
    q: Option<&str>,
    limit: usize,
) -> Result<Vec<ThreadSummary>> {
    let mut rows = read_thread_rows(paths)?;
    let session_index = read_session_index(paths).unwrap_or_default();
    rows.retain_mut(|row| enrich_local_thread_row(paths, row, session_index.get(&row.summary.id)));
    let mut rows = rows.into_iter().map(|row| row.summary).collect::<Vec<_>>();

    let needle = q
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty());
    rows.retain(|row| {
        if matches!(row.status, ThreadStatus::Archived) != archived {
            return false;
        }
        if let Some(needle) = &needle {
            let title = row.title.to_ascii_lowercase();
            let id = row.id.to_ascii_lowercase();
            let latest = row
                .latest_message
                .clone()
                .unwrap_or_default()
                .to_ascii_lowercase();
            title.contains(needle) || id.contains(needle) || latest.contains(needle)
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

fn enrich_local_thread_row(
    paths: &CodexPaths,
    row: &mut thread_rows::LocalThreadRow,
    index_entry: Option<&SessionIndexEntry>,
) -> bool {
    // Native updated_at also changes on metadata writes; only messages order the list.
    row.summary.updated_at = None;
    if row.summary.rollout_path.is_none() {
        row.summary.rollout_path = index_entry.and_then(|entry| entry.path.clone());
    }
    let is_archived = matches!(row.summary.status, ThreadStatus::Archived);
    if row
        .summary
        .rollout_path
        .as_deref()
        .is_some_and(|path| missing_rollout_inside_codex_home(paths, path, is_archived))
    {
        return false;
    }
    row.summary.rollout_path = row
        .summary
        .rollout_path
        .take()
        .and_then(|path| resolve_existing_rollout_path(paths, &path, is_archived));
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
    !enrich_thread_from_rollout_with_storage(&mut row.summary, Some(&paths.home)).unwrap_or(false)
}

fn resolve_existing_rollout_path(
    paths: &CodexPaths,
    path: &Path,
    archived: bool,
) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    for candidate in [
        rollout_file::plain_path(path),
        path.to_path_buf(),
        rollout_file::compressed_path(path),
    ] {
        if paths.contains_path(&candidate) {
            return Some(candidate);
        }
    }
    if archived && path_has_ancestor_inside_codex_home(paths, path) {
        if let Some(name) = path.file_name() {
            let relocated = paths.home.join("archived_sessions").join(name);
            for candidate in [
                rollout_file::plain_path(&relocated),
                relocated.clone(),
                rollout_file::compressed_path(&relocated),
            ] {
                if paths.contains_path(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn path_has_ancestor_inside_codex_home(paths: &CodexPaths, path: &Path) -> bool {
    if path.as_os_str().is_empty()
        || is_macos_network_volume_path(&paths.home)
        || is_macos_network_volume_path(path)
    {
        return false;
    }
    let Ok(home) = fs::canonicalize(&paths.home) else {
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

fn missing_rollout_inside_codex_home(paths: &CodexPaths, path: &Path, archived: bool) -> bool {
    if path.as_os_str().is_empty()
        || resolve_existing_rollout_path(paths, path, archived).is_some()
        || is_macos_network_volume_path(&paths.home)
        || is_macos_network_volume_path(path)
    {
        return false;
    }
    path_has_ancestor_inside_codex_home(paths, path)
}

pub fn enrich_thread_from_rollout(row: &mut ThreadSummary) -> Result<bool> {
    enrich_thread_from_rollout_with_storage(row, None)
}

fn enrich_thread_from_rollout_with_storage(
    row: &mut ThreadSummary,
    root: Option<&Path>,
) -> Result<bool> {
    let Some(path) = &row.rollout_path else {
        return Ok(false);
    };
    let scan = scan_rollout(path, 80)?;
    if let Some(root) = root {
        use crate::session_storage::{self, SessionStorageSize, StorageScope};
        row.storage_size = Some(
            if !scan.identity_conflict && scan.session_id.as_deref() == Some(&row.id) {
                session_storage::file_size(root, path)
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
    if !matches!(row.status, ThreadStatus::Archived) {
        if scan.recoverable {
            row.status = ThreadStatus::Recoverable;
        } else if scan.reply_needed {
            row.status = ThreadStatus::ReplyNeeded;
        } else if scan.running {
            row.status = ThreadStatus::Running;
        } else if matches!(
            row.status,
            ThreadStatus::Running | ThreadStatus::ReplyNeeded | ThreadStatus::Recoverable
        ) {
            row.status = ThreadStatus::Recent;
        }
    }
    row.cwd = scan.cwd;
    row.model = scan.model;
    if !is_usable_thread_title(&row.title) {
        row.title = scan
            .title
            .or(scan.first_user_message_title)
            .unwrap_or_else(|| "未命名线程".to_string());
    }
    row.active_turn_id = scan.active_turn_id;
    row.pending_elicitation = scan.pending_elicitation;
    row.last_event_kind = scan.last_event_kind;
    Ok(scan.is_subagent)
}

pub fn thread_detail(paths: &CodexPaths, id: &str) -> Result<Option<ThreadDetail>> {
    thread_detail_page(paths, id, None, None, None)
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
