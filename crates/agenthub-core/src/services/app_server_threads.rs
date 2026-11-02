use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde_json::{json, Value};

use crate::codex::{self, MessageBlock, ThreadDetail, ThreadStatus, ThreadSummary};

pub fn merge_thread_summaries(
    fallback: Vec<ThreadSummary>,
    app_threads: Vec<ThreadSummary>,
) -> Vec<ThreadSummary> {
    let fallback_by_id: HashMap<String, ThreadSummary> = fallback
        .iter()
        .cloned()
        .map(|thread| (thread.id.clone(), thread))
        .collect();
    let mut rows: Vec<ThreadSummary> = app_threads
        .into_iter()
        .map(|mut row| {
            if let Some(fallback) = fallback_by_id.get(&row.id) {
                preserve_fallback_title(&mut row, fallback);
            }
            row
        })
        .collect();
    for thread in fallback {
        if !rows.iter().any(|row| row.id == thread.id) {
            rows.push(thread);
        }
    }
    rows
}

pub fn app_server_thread_summaries(
    value: &Value,
    fallback: &[ThreadSummary],
) -> Vec<ThreadSummary> {
    let fallback_by_id: HashMap<&str, &ThreadSummary> = fallback
        .iter()
        .map(|thread| (thread.id.as_str(), thread))
        .collect();
    value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("threads").and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|thread| app_server_thread_summary(thread, &fallback_by_id))
        .collect()
}

pub fn app_server_thread_summary(
    thread: &Value,
    fallback_by_id: &HashMap<&str, &ThreadSummary>,
) -> Option<ThreadSummary> {
    let id = thread.get("id").and_then(Value::as_str)?.to_string();
    let fallback = fallback_by_id.get(id.as_str()).copied();
    if fallback.is_none() && is_app_server_subagent_thread(thread) {
        return None;
    }
    let status = merge_app_thread_status(
        fallback,
        thread,
        fallback_has_pending_signal(fallback),
        fallback_has_running_signal(fallback) || app_thread_has_running_signal(thread),
        false,
    );
    let active_turn_id = merged_active_turn_id(fallback, thread, &status);
    let title = thread_title(thread)
        .filter(|title| {
            !is_placeholder_thread_title(title)
                || fallback.is_none_or(|thread| is_placeholder_thread_title(&thread.title))
        })
        .or_else(|| fallback.map(|thread| thread.title.clone()))
        .unwrap_or_else(|| "未命名线程".to_string());
    let mut summary = ThreadSummary {
        activity_updated_at: thread
            .get("updatedAt")
            .and_then(Value::as_i64)
            .and_then(timestamp_to_rfc3339)
            .or_else(|| {
                fallback.and_then(|row| {
                    row.activity_updated_at
                        .clone()
                        .or_else(|| row.updated_at.clone())
                })
            }),
        storage_size: fallback.and_then(|row| row.storage_size.clone()),
        id: id.clone(),
        title,
        status: status.clone(),
        updated_at: fallback.and_then(|thread| thread.updated_at.clone()),
        archived_at: thread
            .get("archivedAt")
            .or_else(|| thread.get("archived_at"))
            .and_then(Value::as_i64)
            .and_then(timestamp_to_rfc3339)
            .or_else(|| fallback.and_then(|thread| thread.archived_at.clone())),
        message_count: fallback.map(|thread| thread.message_count).unwrap_or(0),
        latest_message: thread
            .get("preview")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| fallback.and_then(|thread| thread.latest_message.clone())),
        cwd: thread
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| fallback.and_then(|thread| thread.cwd.clone())),
        model: thread
            .get("model")
            .or_else(|| thread.get("modelProvider"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| fallback.and_then(|thread| thread.model.clone())),
        rollout_path: resolve_preferred_rollout_path(
            app_thread_rollout_path(thread),
            fallback.and_then(|thread| thread.rollout_path.as_deref()),
        ),
        active_turn_id,
        active_job_id: fallback.and_then(|thread| thread.active_job_id.clone()),
        pending_elicitation: if matches!(status, ThreadStatus::ReplyNeeded | ThreadStatus::Archived)
        {
            fallback.and_then(|thread| thread.pending_elicitation.clone())
        } else {
            None
        },
        last_event_kind: fallback.and_then(|thread| thread.last_event_kind.clone()),
    };
    if fallback.is_none_or(|row| row.rollout_path != summary.rollout_path) {
        summary.storage_size = Some(crate::session_storage::SessionStorageSize::unavailable(
            crate::session_storage::StorageScope::File,
        ));
    }
    if !matches!(app_thread_state(thread), AppThreadState::Recoverable)
        && !thread_archived(thread)
        && summary.rollout_path.is_some()
    {
        let rollout_enriched = codex::enrich_thread_from_rollout(&mut summary).is_ok();
        let pending_signal = fallback_has_pending_signal(Some(&summary));
        let running_signal = fallback_has_running_signal(Some(&summary))
            || (rollout_enriched && matches!(summary.status, ThreadStatus::Running))
            || app_thread_has_running_signal(thread);
        let suppress_stale_running = rollout_enriched
            && rollout_suppresses_app_running(Some(&summary), thread, running_signal);
        summary.status = merge_app_thread_status(
            Some(&summary),
            thread,
            pending_signal,
            running_signal,
            suppress_stale_running,
        );
        summary.active_turn_id = merged_active_turn_id(Some(&summary), thread, &summary.status);
        if !matches!(
            summary.status,
            ThreadStatus::ReplyNeeded | ThreadStatus::Archived
        ) {
            summary.pending_elicitation = None;
        }
    }
    Some(summary)
}

pub fn apply_app_server_thread_detail(detail: &mut ThreadDetail, value: &Value) {
    let Some(thread) = value.get("thread") else {
        return;
    };
    if let Some(title) = thread_title(thread) {
        if !is_placeholder_thread_title(&title)
            || is_placeholder_thread_title(&detail.summary.title)
        {
            detail.summary.title = title;
        }
    }
    if let Some(updated_at) = thread
        .get("updatedAt")
        .and_then(Value::as_i64)
        .and_then(timestamp_to_rfc3339)
    {
        detail.summary.activity_updated_at = Some(updated_at);
    }
    if let Some(cwd) = thread.get("cwd").and_then(Value::as_str) {
        detail.summary.cwd = Some(cwd.to_string());
    }
    if let Some(preview) = thread.get("preview").and_then(Value::as_str) {
        detail.summary.latest_message = Some(preview.to_string());
    }
    if let Some(model) = thread
        .get("model")
        .or_else(|| thread.get("modelProvider"))
        .and_then(Value::as_str)
    {
        detail.summary.model = Some(model.to_string());
    }
    detail.summary.rollout_path = resolve_preferred_rollout_path(
        app_thread_rollout_path(thread),
        detail.summary.rollout_path.as_deref(),
    );
    let mut rollout_enriched = false;
    if detail.summary.rollout_path.is_some() {
        rollout_enriched = codex::enrich_thread_from_rollout(&mut detail.summary).is_ok();
    }
    let pending_turn_id = detail_pending_turn_id(detail);
    let pending_signal = detail_has_pending_signal(detail) || pending_turn_id.as_deref().is_some();
    let running_signal = detail_has_running_signal(detail)
        || (rollout_enriched && matches!(detail.summary.status, ThreadStatus::Running))
        || app_thread_has_running_signal(thread);
    let suppress_stale_running = rollout_enriched
        && rollout_suppresses_app_running(Some(&detail.summary), thread, running_signal);
    let status = merge_app_thread_status(
        Some(&detail.summary),
        thread,
        pending_signal,
        running_signal,
        suppress_stale_running,
    );
    let active_turn_id = merged_active_turn_id(Some(&detail.summary), thread, &status)
        .or_else(|| detail_active_turn_id(detail))
        .or(pending_turn_id);
    detail.summary.active_turn_id = match status {
        ThreadStatus::Recent => None,
        _ => active_turn_id,
    };
    if !matches!(status, ThreadStatus::ReplyNeeded | ThreadStatus::Archived) {
        detail.summary.pending_elicitation = None;
    }
    detail.summary.status = status;
}

pub fn app_server_detail_from_read(value: &Value) -> Option<ThreadDetail> {
    let thread = value.get("thread")?;
    let fallback_by_id = HashMap::new();
    let mut summary = app_server_thread_summary(thread, &fallback_by_id)?;
    if summary.rollout_path.is_some() {
        let _ = codex::enrich_thread_from_rollout(&mut summary);
    }
    let mut detail = codex::thread_detail_from_summary(summary).ok()?;
    if detail.blocks.is_empty() {
        let events = app_server_thread_item_events(thread);
        if !events.is_empty() {
            detail.raw_event_count = events.len();
            detail.blocks = codex::message_blocks_from_events(events.iter());
            detail.timeline = Some(crate::history_navigation::timeline(
                detail
                    .blocks
                    .iter()
                    .filter(|block| block.role == "user")
                    .map(|block| {
                        (
                            block.id.as_str(),
                            block.user_message.as_ref(),
                            block.text.as_deref(),
                        )
                    }),
            ));
            detail.total_blocks = detail.blocks.len();
            detail.has_more_blocks = false;
            detail.before_cursor = None;
        }
    }
    apply_app_server_thread_detail(&mut detail, value);
    Some(detail)
}

pub fn thread_title(thread: &Value) -> Option<String> {
    ["name", "title"].into_iter().find_map(|field| {
        thread
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn preserve_fallback_title(row: &mut ThreadSummary, fallback: &ThreadSummary) {
    if is_placeholder_thread_title(&row.title) && !is_placeholder_thread_title(&fallback.title) {
        row.title = fallback.title.clone();
    }
    if matches!(fallback.status, ThreadStatus::Archived) {
        row.status = ThreadStatus::Archived;
        row.archived_at = fallback.archived_at.clone();
    }
}

fn is_placeholder_thread_title(title: &str) -> bool {
    let value = title.trim();
    value.is_empty() || matches!(value, "未命名线程" | "Untitled thread" | "Untitled")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppThreadState {
    Active,
    Recoverable,
    Idle,
    NotLoaded,
    Unknown,
}

fn merge_app_thread_status(
    fallback: Option<&ThreadSummary>,
    thread: &Value,
    pending_signal: bool,
    running_signal: bool,
    suppress_stale_running: bool,
) -> ThreadStatus {
    if fallback.is_some_and(|thread| matches!(thread.status, ThreadStatus::Archived))
        || thread_archived(thread)
    {
        return ThreadStatus::Archived;
    }
    if fallback.is_some_and(|thread| matches!(thread.status, ThreadStatus::Recoverable)) {
        return ThreadStatus::Recoverable;
    }
    match app_thread_state(thread) {
        AppThreadState::Active => {
            if suppress_stale_running && !pending_signal {
                fallback_stable_status(fallback)
            } else {
                ThreadStatus::Running
            }
        }
        AppThreadState::Recoverable => ThreadStatus::Recoverable,
        AppThreadState::Idle => {
            if running_signal && !suppress_stale_running {
                ThreadStatus::Running
            } else if pending_signal {
                ThreadStatus::ReplyNeeded
            } else {
                ThreadStatus::Recent
            }
        }
        AppThreadState::NotLoaded => {
            if running_signal && !suppress_stale_running {
                ThreadStatus::Running
            } else if pending_signal {
                ThreadStatus::ReplyNeeded
            } else if fallback.is_some_and(|summary| {
                fallback_has_clearable_stale_status(summary)
                    && app_thread_has_fallback_rollout_path(thread)
            }) {
                ThreadStatus::Recent
            } else {
                fallback_stable_status(fallback)
            }
        }
        AppThreadState::Unknown => {
            if running_signal {
                ThreadStatus::Running
            } else if pending_signal {
                ThreadStatus::ReplyNeeded
            } else if fallback.is_some_and(fallback_has_clearable_stale_status) {
                ThreadStatus::Recent
            } else {
                fallback_stable_status(fallback)
            }
        }
    }
}

fn fallback_stable_status(fallback: Option<&ThreadSummary>) -> ThreadStatus {
    match fallback.map(|thread| &thread.status) {
        Some(ThreadStatus::Archived) => ThreadStatus::Archived,
        Some(ThreadStatus::Recoverable) => ThreadStatus::Recoverable,
        Some(ThreadStatus::Running | ThreadStatus::ReplyNeeded) | None => ThreadStatus::Recent,
        Some(status) => status.clone(),
    }
}

fn fallback_has_clearable_stale_status(summary: &ThreadSummary) -> bool {
    matches!(
        summary.status,
        ThreadStatus::Running | ThreadStatus::ReplyNeeded
    ) && summary.rollout_path.is_some()
        && summary.active_turn_id.is_none()
        && summary.active_job_id.is_none()
        && summary.pending_elicitation.is_none()
}

fn rollout_suppresses_app_running(
    fallback: Option<&ThreadSummary>,
    thread: &Value,
    running_signal: bool,
) -> bool {
    if !running_signal {
        return false;
    }
    let Some(summary) = fallback else {
        return false;
    };
    if !matches!(summary.status, ThreadStatus::Recent)
        || summary.active_turn_id.is_some()
        || summary.active_job_id.is_some()
        || summary.pending_elicitation.is_some()
    {
        return false;
    }
    let Some(path) = summary.rollout_path.as_deref() else {
        return false;
    };
    let Some(active_turn_id) = app_thread_active_turn_id(thread) else {
        return false;
    };
    codex::rollout_has_completed_turn(path, Some(active_turn_id.as_str())).unwrap_or(false)
}

fn merged_active_turn_id(
    fallback: Option<&ThreadSummary>,
    thread: &Value,
    status: &ThreadStatus,
) -> Option<String> {
    let active_turn_id = app_thread_active_turn_id(thread)
        .or_else(|| fallback.and_then(|thread| thread.active_turn_id.clone()));
    match status {
        ThreadStatus::Running
        | ThreadStatus::ReplyNeeded
        | ThreadStatus::Recoverable
        | ThreadStatus::Archived => active_turn_id,
        ThreadStatus::Recent => None,
    }
}

fn fallback_has_pending_signal(fallback: Option<&ThreadSummary>) -> bool {
    let Some(summary) = fallback else {
        return false;
    };
    summary.active_turn_id.is_some() && summary.pending_elicitation.is_some()
}

fn fallback_has_running_signal(fallback: Option<&ThreadSummary>) -> bool {
    let Some(summary) = fallback else {
        return false;
    };
    summary.active_job_id.is_some()
}

fn detail_has_pending_signal(detail: &ThreadDetail) -> bool {
    if fallback_has_pending_signal(Some(&detail.summary)) {
        return true;
    }
    let Some(active_turn_id) = detail.summary.active_turn_id.as_deref() else {
        return false;
    };
    detail.blocks.iter().any(|block| {
        block.turn_id.as_deref() == Some(active_turn_id) && block_has_pending_signal(block)
    })
}

fn detail_pending_turn_id(detail: &ThreadDetail) -> Option<String> {
    let mut pending: Option<&MessageBlock> = None;
    for block in &detail.blocks {
        if block_has_pending_signal(block) {
            pending = Some(block);
            continue;
        }
        if pending.is_some_and(|pending| block_clears_pending_signal(block, pending)) {
            pending = None;
        }
    }
    pending.and_then(|block| block.turn_id.clone())
}

fn block_clears_pending_signal(block: &MessageBlock, pending: &MessageBlock) -> bool {
    if block_has_pending_signal(block) {
        return false;
    }
    if let Some(expected) = pending.call_id.as_deref() {
        if block.call_id.as_deref() == Some(expected) {
            return true;
        }
    }
    if let Some(expected) = pending.item_id.as_deref() {
        if block.item_id.as_deref() == Some(expected) {
            return true;
        }
    }
    matches!(block.role.as_str(), "user" | "assistant" | "tool")
}

fn detail_has_running_signal(detail: &ThreadDetail) -> bool {
    fallback_has_running_signal(Some(&detail.summary))
        || detail.blocks.iter().any(block_has_running_signal)
}

fn detail_active_turn_id(detail: &ThreadDetail) -> Option<String> {
    detail
        .blocks
        .iter()
        .rev()
        .find(|block| block_has_running_signal(block))
        .and_then(|block| block.turn_id.clone())
}

fn block_has_pending_signal(block: &MessageBlock) -> bool {
    let kind = block.kind.as_str();
    if kind == "request_user_input" {
        return !block.questions.is_empty();
    }
    if kind == "plan" {
        return block
            .text
            .as_deref()
            .is_some_and(|text| text.contains("<proposed_plan>"));
    }
    kind.contains("approval")
        && block.status.as_deref().is_none_or(|status| {
            matches!(status, "pending" | "running" | "in_progress" | "inProgress")
        })
}

fn block_has_running_signal(block: &MessageBlock) -> bool {
    if block_has_pending_signal(block) {
        return false;
    }
    let status = block.status.as_deref().unwrap_or_default();
    let running_status = matches!(
        status,
        "pending" | "running" | "in_progress" | "inProgress" | "active"
    );
    running_status
        && (block.role == "tool"
            || block.kind.contains("function_call")
            || block.kind.contains("tool")
            || block.kind.contains("command"))
}

fn app_thread_state(thread: &Value) -> AppThreadState {
    match app_thread_status_text(thread) {
        Some("notLoaded" | "not_loaded") => AppThreadState::NotLoaded,
        Some("active" | "running" | "in_progress" | "inProgress" | "generating") => {
            AppThreadState::Active
        }
        Some("systemError" | "system_error" | "recoverable" | "error") => {
            AppThreadState::Recoverable
        }
        Some(
            "idle" | "recent" | "inactive" | "completed" | "complete" | "done" | "finished"
            | "stopped" | "canceled" | "cancelled" | "interrupted" | "success" | "succeeded",
        ) => AppThreadState::Idle,
        Some(_) | None => AppThreadState::Unknown,
    }
}

fn app_thread_status_text(thread: &Value) -> Option<&str> {
    thread
        .get("status")
        .and_then(|status| status.get("type").or(Some(status)))
        .and_then(Value::as_str)
}

fn app_thread_active_turn_id(thread: &Value) -> Option<String> {
    thread
        .pointer("/status/turnId")
        .or_else(|| thread.pointer("/status/turn_id"))
        .or_else(|| thread.get("activeTurnId"))
        .or_else(|| thread.get("active_turn_id"))
        .or_else(|| thread.get("turnId"))
        .or_else(|| thread.get("turn_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| app_thread_turns_active_turn_id(thread))
}

fn app_thread_has_running_signal(thread: &Value) -> bool {
    app_thread_active_turn_id(thread).is_some()
}

fn app_thread_has_fallback_rollout_path(thread: &Value) -> bool {
    app_thread_rollout_path(thread).is_some()
}

fn app_thread_turns_active_turn_id(thread: &Value) -> Option<String> {
    thread
        .get("turns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .rev()
        .find_map(|turn| {
            let id = turn
                .get("id")
                .or_else(|| turn.get("turnId"))
                .or_else(|| turn.get("turn_id"))
                .and_then(Value::as_str)?;
            if app_thread_state(turn) == AppThreadState::Active || turn_has_running_item(turn) {
                Some(id.to_string())
            } else {
                None
            }
        })
}

fn app_server_thread_item_events(thread: &Value) -> Vec<Value> {
    thread
        .get("turns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|turn| {
            let turn_id = turn
                .get("id")
                .or_else(|| turn.get("turnId"))
                .or_else(|| turn.get("turn_id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            turn.get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(move |item| app_server_item_events(turn_id.as_deref(), item))
        })
        .collect()
}

fn app_server_item_events(turn_id: Option<&str>, item: &Value) -> Vec<Value> {
    let Some(payload) = normalize_app_server_item_payload(item) else {
        return Vec::new();
    };
    let item_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut event = json!({
        "type": "response_item",
        "payload": payload,
    });
    if let Some(turn_id) = turn_id {
        event["turn_id"] = Value::String(turn_id.to_string());
    }
    if item_type.eq_ignore_ascii_case("plan") {
        let mut marker = json!({
            "type": "item_completed",
            "item": { "type": "Plan" },
        });
        if let Some(turn_id) = turn_id {
            marker["turn_id"] = Value::String(turn_id.to_string());
        }
        if let Some(item_id) = item
            .get("id")
            .or_else(|| item.get("itemId"))
            .or_else(|| item.get("item_id"))
            .and_then(Value::as_str)
        {
            marker["item"]["id"] = Value::String(item_id.to_string());
        }
        vec![marker, event]
    } else {
        vec![event]
    }
}

fn normalize_app_server_item_payload(item: &Value) -> Option<Value> {
    let item_type = item.get("type").and_then(Value::as_str)?;
    match item_type {
        "agentMessage" | "assistantMessage" => item_text(item).map(|text| {
            json!({
                "type": "message",
                "role": "assistant",
                "id": item_id(item),
                "content": [{ "text": text }]
            })
        }),
        "userMessage" => item_text(item).map(|text| {
            json!({
                "type": "message",
                "role": "user",
                "id": item_id(item),
                "content": [{ "text": text }]
            })
        }),
        "plan" | "Plan" => item_text(item).map(|text| {
            json!({
                "type": "Plan",
                "id": item_id(item),
                "text": text,
                "status": item.get("status").cloned().unwrap_or(Value::Null)
            })
        }),
        "requestUserInput" | "request_user_input" | "toolRequestUserInput" => {
            let questions = item
                .get("questions")
                .or_else(|| item.pointer("/params/questions"))
                .cloned()
                .unwrap_or(Value::Null);
            Some(json!({
                "type": "function_call",
                "name": "request_user_input",
                "id": item_id(item),
                "call_id": item_id(item),
                "turn_id": item
                    .get("turnId")
                    .or_else(|| item.get("turn_id"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "arguments": { "questions": questions },
                "status": item.get("status").cloned().unwrap_or(Value::Null)
            }))
        }
        _ => Some(item.clone()),
    }
}

fn item_text(item: &Value) -> Option<String> {
    item.get("text")
        .or_else(|| item.get("message"))
        .or_else(|| item.get("content"))
        .or_else(|| item.get("aggregatedText"))
        .and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Array(items) => {
                let text = items
                    .iter()
                    .filter_map(|item| {
                        item.get("text")
                            .or_else(|| item.get("input_text"))
                            .and_then(Value::as_str)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.trim().is_empty()).then_some(text)
            }
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
}

fn item_id(item: &Value) -> Option<String> {
    item.get("id")
        .or_else(|| item.get("itemId"))
        .or_else(|| item.get("item_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn turn_has_running_item(turn: &Value) -> bool {
    turn.get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|item| {
            item.get("status")
                .and_then(|status| status.get("type").or(Some(status)))
                .and_then(Value::as_str)
                .is_some_and(|status| {
                    matches!(
                        status,
                        "active" | "running" | "in_progress" | "inProgress" | "pending"
                    )
                })
        })
}

fn app_thread_rollout_path(thread: &Value) -> Option<PathBuf> {
    thread
        .get("path")
        .or_else(|| thread.get("rollout_path"))
        .or_else(|| thread.get("rolloutPath"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .filter(|path| {
            path.ends_with(".jsonl")
                || path.ends_with(".jsonl.zst")
                || path.ends_with(".zst")
                || path.contains("rollout-")
        })
        .map(PathBuf::from)
}

fn existing_rollout_variant(path: &Path) -> Option<PathBuf> {
    [
        crate::codex::rollout_file::plain_path(path),
        path.to_path_buf(),
        crate::codex::rollout_file::compressed_path(path),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

fn resolve_preferred_rollout_path(
    app_path: Option<PathBuf>,
    fallback: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(existing) = app_path.as_deref().and_then(existing_rollout_variant) {
        return Some(existing);
    }
    if let Some(existing) = fallback.and_then(existing_rollout_variant) {
        return Some(existing);
    }
    app_path.or_else(|| fallback.map(Path::to_path_buf))
}

fn is_app_server_subagent_thread(thread: &Value) -> bool {
    has_non_empty_string(thread, &["parentThreadId", "parent_thread_id"])
        || has_non_empty_string(thread, &["agentPath", "agent_path"])
        || has_non_empty_string(thread, &["agentNickname", "agent_nickname"])
        || has_non_empty_string(thread, &["agentRole", "agent_role"])
        || field_contains_subagent(
            thread,
            &["sourceKind", "source_kind", "threadSource", "thread_source"],
        )
        || source_value_contains_subagent(thread.get("source"))
}

fn has_non_empty_string(value: &Value, fields: &[&str]) -> bool {
    fields.iter().any(|field| {
        value
            .get(*field)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    })
}

fn field_contains_subagent(value: &Value, fields: &[&str]) -> bool {
    fields.iter().any(|field| {
        value
            .get(*field)
            .and_then(Value::as_str)
            .is_some_and(|text| text.to_ascii_lowercase().contains("subagent"))
    })
}

fn source_value_contains_subagent(source: Option<&Value>) -> bool {
    match source {
        Some(Value::String(text)) => text.to_ascii_lowercase().contains("subagent"),
        Some(Value::Array(items)) => items
            .iter()
            .any(|item| source_value_contains_subagent(Some(item))),
        Some(Value::Object(map)) => map.iter().any(|(key, value)| {
            key.to_ascii_lowercase().contains("subagent")
                || source_value_contains_subagent(Some(value))
        }),
        _ => false,
    }
}

fn thread_archived(thread: &Value) -> bool {
    thread
        .get("archived")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || thread
            .get("archivedAt")
            .or_else(|| thread.get("archived_at"))
            .is_some_and(|value| !value.is_null())
        || thread
            .get("path")
            .and_then(Value::as_str)
            .map(|path| path.contains("/archived/") || path.contains("/archived_sessions/"))
            .unwrap_or(false)
}

fn timestamp_to_rfc3339(value: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(value, 0).map(|dt| dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rollout_path_resolution_prefers_existing_compressed_or_relocated_handoff_file() {
        let dir =
            std::env::temp_dir().join(format!("agenthub-codex-handoff-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("sessions")).unwrap();
        fs::create_dir_all(dir.join("archived_sessions")).unwrap();

        let stale_plain = dir.join("sessions/rollout-2026-10-29-demo.jsonl");
        let existing_zst = dir.join("sessions/rollout-2026-10-29-demo.jsonl.zst");
        fs::write(&existing_zst, b"compressed").unwrap();
        assert_eq!(
            resolve_preferred_rollout_path(Some(stale_plain.clone()), None),
            Some(existing_zst.clone())
        );
        fs::remove_file(&existing_zst).unwrap();

        let relocated = dir.join("archived_sessions/rollout-2026-10-29-demo.jsonl");
        fs::write(&relocated, b"{}\n").unwrap();
        assert_eq!(
            resolve_preferred_rollout_path(Some(stale_plain), Some(&relocated)),
            Some(relocated)
        );

        fs::remove_dir_all(dir).unwrap();
    }
}
