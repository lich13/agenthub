mod activity;
mod message_builder;
mod pagination;

pub use activity::*;
use message_builder::{compact_chat_history, MessageBlockBuilder, PendingToolCall};
pub use pagination::*;

use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

const REQUEST_USER_INPUT_TAIL_MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutRequestUserInputState {
    Pending,
    Resolved,
    Missing,
}

use super::{
    extract_proposed_plan_text, session_index::SessionIndexEntry, CodexMessage, MessageBlock,
    PendingElicitation, ThreadDetail, ThreadSummary, UserInputAnswer,
};

pub fn message_blocks_from_events<'a>(
    events: impl IntoIterator<Item = &'a Value>,
) -> Vec<MessageBlock> {
    let mut block_builder = MessageBlockBuilder::default();
    let mut turn_context = None;
    for (index, value) in events.into_iter().enumerate() {
        let mut value = value.clone();
        normalize_canonical_turn(&mut value, &mut turn_context);
        block_builder.push_event(&value, index + 1);
    }
    block_builder.finish()
}

pub fn is_macos_network_volume_path(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        if path.starts_with("/Volumes") {
            return true;
        }
        let mut current = Some(path);
        while let Some(candidate) = current {
            if candidate.exists() {
                return fs::canonicalize(candidate)
                    .map(|canonical| canonical.starts_with("/Volumes"))
                    .unwrap_or(true);
            }
            current = candidate.parent();
        }
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

fn task_complete_has_last_agent_message(value: &Value) -> bool {
    let payload = value.get("payload").unwrap_or(value);
    value
        .get("last_agent_message")
        .or_else(|| payload.get("last_agent_message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|message| !message.is_empty())
}

fn latest_active_task_turn(active_tasks: &[Option<String>]) -> Option<String> {
    active_tasks.iter().rev().find_map(Clone::clone)
}

fn retire_active_turn_boundary(
    active_tasks: &mut Vec<Option<String>>,
    pending_tool_turns: &mut HashMap<String, Option<String>>,
    completed_turn_id: &str,
) -> bool {
    let Some(index) = active_tasks
        .iter()
        .position(|active| active.as_deref() == Some(completed_turn_id))
    else {
        pending_tool_turns
            .retain(|_, pending_turn| pending_turn.as_deref() != Some(completed_turn_id));
        return false;
    };

    let retired_turn_ids = active_tasks[..=index]
        .iter()
        .filter_map(Clone::clone)
        .collect::<HashSet<_>>();
    let retired_anonymous_turn = active_tasks[..=index].iter().any(Option::is_none);
    active_tasks.drain(..=index);
    pending_tool_turns.retain(|_, pending_turn| match pending_turn {
        Some(turn_id) => !retired_turn_ids.contains(turn_id),
        None => !retired_anonymous_turn,
    });
    true
}

fn is_turn_terminal_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "turn_completed" | "turn/completed" | "turn_aborted" | "turn/aborted"
    )
}

fn clear_anonymous_pending_tools(pending_tool_turns: &mut HashMap<String, Option<String>>) {
    pending_tool_turns.retain(|_, pending_turn| pending_turn.is_some());
}

pub(super) fn rollout_event_type(value: &Value) -> &str {
    let top_level = value.get("type").and_then(Value::as_str).unwrap_or("");
    let raw = if top_level == "event_msg" {
        value
            .pointer("/payload/type")
            .or_else(|| value.pointer("/payload/event_type"))
            .or_else(|| value.pointer("/payload/event/type"))
            .or_else(|| value.pointer("/payload/payload/type"))
            .and_then(Value::as_str)
            .unwrap_or(top_level)
    } else {
        top_level
    };
    match raw {
        "thread.started" => "thread_started",
        "TurnStarted" => "task_started",
        "turn.started" => "turn_started",
        "TurnComplete" | "TurnCompleted" | "turn_complete" | "turn/complete" | "turn.completed" => {
            "turn_completed"
        }
        "TurnAborted" | "turn.failed" | "error" => "turn_aborted",
        "item.started" => "item_started",
        "item.completed" => "item_completed",
        "item.plan.delta" => "item/plan/delta",
        "turn.plan.updated" => "turn/plan/updated",
        "RequestUserInput" | "requestUserInput" => "request_user_input",
        other => other,
    }
}

fn update_pending_tool_calls(
    value: &Value,
    pending_tool_turns: &mut HashMap<String, Option<String>>,
    active_turn_id: Option<&str>,
) {
    let Some(payload) = event_payload(value) else {
        return;
    };
    let payload_type = event_payload_type(value, payload);
    if is_tool_output_kind(payload_type) {
        if let Some(call_id) = payload_call_id(payload) {
            pending_tool_turns.remove(&call_id);
        }
        return;
    }
    if !is_tool_call_kind(payload_type) || is_request_user_input(value) {
        return;
    }
    let Some(call_id) = payload_call_id(payload) else {
        return;
    };
    if payload_status(payload)
        .as_deref()
        .is_some_and(is_finished_status)
    {
        return;
    }
    pending_tool_turns.insert(
        call_id,
        event_turn_id(value).or_else(|| active_turn_id.map(str::to_string)),
    );
}

fn plan_marker_for_event(
    marker: &Option<(Option<String>, Option<String>)>,
    value: &Value,
) -> Option<(Option<String>, Option<String>)> {
    let (turn_id, item_id) = marker.clone()?;
    let event_turn = event_turn_id(value);
    if turn_id.is_some() && event_turn.is_some() && turn_id != event_turn {
        return None;
    }
    Some((turn_id, item_id))
}

fn clears_pending_action(value: &Value, pending: Option<&PendingAction>) -> bool {
    let Some(pending) = pending else {
        return false;
    };
    if user_input_answer_matches(value, pending) {
        return true;
    }
    if function_output_matches(value, pending) {
        return true;
    }
    let Some(message) = parse_message_event(value) else {
        return false;
    };
    let role = message.role.as_str();
    if role == "user" {
        return true;
    }
    if role == "assistant" && !message.text.contains("<proposed_plan>") {
        return match pending {
            PendingAction::Plan { turn_id, .. } | PendingAction::Elicitation { turn_id, .. } => {
                event_turn_id(value).as_deref() != turn_id.as_deref()
            }
        };
    }
    false
}

fn user_input_answer_matches(value: &Value, pending: &PendingAction) -> bool {
    if !is_user_input_answer(value) {
        return false;
    }
    match pending {
        PendingAction::Elicitation {
            call_id: Some(expected),
            ..
        } => event_call_id(value).as_deref() == Some(expected),
        PendingAction::Elicitation { item_id, .. } => item_id
            .as_deref()
            .is_some_and(|expected| event_item_id(value).as_deref() == Some(expected)),
        PendingAction::Plan { item_id, .. } => item_id
            .as_deref()
            .is_some_and(|expected| event_item_id(value).as_deref() == Some(expected)),
    }
}

fn function_output_matches(value: &Value, pending: &PendingAction) -> bool {
    let payload = value.get("payload").unwrap_or(value);
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| value.get("type").and_then(Value::as_str));
    if payload_type != Some("function_call_output") {
        return false;
    }
    let call_id = payload
        .get("call_id")
        .or_else(|| payload.get("callId"))
        .and_then(Value::as_str);
    match pending {
        PendingAction::Elicitation {
            call_id: Some(expected),
            ..
        } => call_id == Some(expected),
        PendingAction::Elicitation { item_id, .. } => item_id
            .as_deref()
            .is_some_and(|expected| event_item_id(value).as_deref() == Some(expected)),
        PendingAction::Plan { item_id, .. } => item_id
            .as_deref()
            .is_some_and(|expected| event_item_id(value).as_deref() == Some(expected)),
    }
}

fn explicit_event_turn_id(value: &Value) -> Option<String> {
    value
        .get("turn_id")
        .or_else(|| value.get("turnId"))
        .or_else(|| value.pointer("/payload/turn_id"))
        .or_else(|| value.pointer("/payload/turnId"))
        .or_else(|| value.pointer("/payload/event/turn_id"))
        .or_else(|| value.pointer("/payload/event/turnId"))
        .or_else(|| value.pointer("/payload/payload/turn_id"))
        .or_else(|| value.pointer("/payload/payload/turnId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn event_turn_id(value: &Value) -> Option<String> {
    explicit_event_turn_id(value).or_else(|| {
        value
            .pointer("/payload/internal_chat_message_metadata_passthrough/turn_id")
            .or_else(|| value.pointer("/internal_chat_message_metadata_passthrough/turn_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

// Model-response metadata may contain a different internal turn id. Only
// lifecycle records establish the canonical task context for subsequent items.
pub(super) fn normalize_canonical_turn(value: &mut Value, current: &mut Option<String>) {
    let kind = rollout_event_type(value);
    let explicit = explicit_event_turn_id(value);
    if matches!(
        kind,
        "turn_context" | "task_started" | "turn_started" | "turn/started"
    ) && explicit.is_some()
    {
        *current = explicit.clone();
    }
    let is_item = value.get("type").and_then(Value::as_str) == Some("response_item")
        || matches!(
            kind,
            "agent_message"
                | "user_message"
                | "assistant_message"
                | "request_user_input"
                | "item_started"
                | "item_completed"
        );
    if is_item {
        if let Some(turn) = explicit
            .or_else(|| current.clone())
            .or_else(|| event_turn_id(value))
        {
            value["turn_id"] = Value::String(turn);
        }
    }
}

fn is_internal_agent_message(value: &Value) -> bool {
    let payload = event_payload(value).unwrap_or(value);
    payload.get("type").and_then(Value::as_str) == Some("agent_message")
        && (payload.get("author").is_some() || payload.get("recipient").is_some())
}

fn is_final_assistant_message(value: &Value) -> bool {
    !is_internal_agent_message(value)
        && matches!(
            event_payload(value)
                .unwrap_or(value)
                .get("phase")
                .and_then(Value::as_str),
            Some("final" | "final_answer")
        )
}

fn event_item_id(value: &Value) -> Option<String> {
    value
        .get("item_id")
        .or_else(|| value.get("itemId"))
        .or_else(|| value.pointer("/item/id"))
        .or_else(|| value.pointer("/payload/id"))
        .or_else(|| value.pointer("/payload/item_id"))
        .or_else(|| value.pointer("/payload/itemId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn event_call_id(value: &Value) -> Option<String> {
    value
        .get("call_id")
        .or_else(|| value.get("callId"))
        .or_else(|| value.pointer("/payload/call_id"))
        .or_else(|| value.pointer("/payload/callId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn parse_message_event(value: &Value) -> Option<CodexMessage> {
    let mut message = parse_raw_message_event(value)?;
    if message.role == "user" {
        message.text = crate::user_message::summary_text(&message.text);
        if message.text.trim().is_empty() {
            return None;
        }
    }
    message.text = trim_text(&message.text, 4000);
    Some(message)
}

pub(super) fn parse_raw_message_event(value: &Value) -> Option<CodexMessage> {
    if is_internal_agent_message(value) {
        return None;
    }
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
    let payload = value
        .get("payload")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("item"));
    let payload_type = payload
        .and_then(|p| p.get("type"))
        .and_then(Value::as_str)
        .unwrap_or(event_type);
    if !matches!(
        payload_type,
        "message" | "agent_message" | "user_message" | "assistant_message"
    ) {
        return None;
    }

    let role = payload
        .and_then(|p| p.get("role"))
        .and_then(Value::as_str)
        .or_else(|| value.get("role").and_then(Value::as_str))
        .unwrap_or_else(|| {
            if payload_type == "user_message" {
                "user"
            } else {
                "assistant"
            }
        });

    let text = structured_text_raw(payload.unwrap_or(value)).unwrap_or_else(|| {
        let mut text = String::new();
        collect_text(value, &mut text);
        text
    });
    let text = trim_preserving_indentation(&text, usize::MAX).0;
    if text.is_empty() {
        return None;
    }
    if payload_type == "message" && !matches!(role, "user" | "assistant") {
        return None;
    }
    if is_internal_display_kind(payload_type)
        || if role == "user" {
            is_hidden_user_action_text(&text)
                || crate::user_message::presentation_text(&text)
                    .trim()
                    .is_empty()
        } else {
            is_internal_context_message_text(&text)
        }
    {
        return None;
    }
    Some(CodexMessage {
        role: role.to_string(),
        kind: payload_type.to_string(),
        text,
        created_at: value
            .get("timestamp")
            .or_else(|| value.get("created_at"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn plan_text_from_event(value: &Value) -> Option<String> {
    if rollout_event_type(value) == "item_completed"
        && value
            .get("item")
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("Plan")
    {
        return structured_text_raw(value.get("item")?)
            .map(|text| trim_text(&text, usize::MAX))
            .filter(|text| !text.is_empty());
    }
    let payload = value
        .get("payload")
        .or_else(|| value.get("item"))
        .unwrap_or(value);
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| value.get("type").and_then(Value::as_str));
    if !matches!(payload_type, Some("Plan" | "plan")) {
        return None;
    }
    structured_text_raw(payload)
        .or_else(|| structured_text_raw(value))
        .map(|text| trim_text(&text, usize::MAX))
        .filter(|text| !text.is_empty())
}

const MESSAGE_TEXT_LIMIT: usize = 12_000;
const TOOL_TEXT_LIMIT: usize = 8_000;
const TOOL_INPUT_LIMIT: usize = 4_000;
const TOOL_SUMMARY_LIMIT: usize = 220;
const CHAT_HISTORY_LIMIT: usize = 80;

fn parse_message_block(value: &Value, raw_index: usize) -> Option<MessageBlock> {
    if let Some(block) = pending_elicitation_block(value, raw_index) {
        return Some(block);
    }

    let payload = event_payload(value)?;
    let payload_type = event_payload_type(value, payload);
    if is_plan_delta_kind(payload_type) || is_user_input_answer(value) {
        return None;
    }
    if is_tool_call_kind(payload_type) || is_tool_output_kind(payload_type) {
        return None;
    }
    let role = payload
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_else(|| role_for_kind(payload_type));
    if payload_type == "message" && !matches!(role, "user" | "assistant") {
        return None;
    }
    if is_internal_display_kind(payload_type) {
        return None;
    }
    if payload_type != "message" && !is_action_display_kind(payload_type) {
        return None;
    }
    let user_message = (role == "user").then(|| {
        crate::user_message::parse_user_message(
            &block_id(value, raw_index),
            payload.get("content").unwrap_or(payload),
        )
    });
    let text = user_message
        .as_ref()
        .map(|message| message.text.clone())
        .or_else(|| structured_text(payload))
        .or_else(|| structured_text(value))
        .or_else(|| parse_message_event(value).map(|message| message.text));
    if text.as_deref().is_some_and(|text| {
        if role == "user" {
            is_hidden_user_action_text(text)
        } else {
            is_internal_context_message_text(text)
        }
    }) {
        return None;
    }
    if text.as_deref().unwrap_or("").trim().is_empty()
        && user_message.as_ref().is_none_or(|message| {
            message.attachments.is_empty() && message.context_blocks.is_empty()
        })
        && !is_action_display_kind(payload_type)
    {
        return None;
    }
    Some(MessageBlock {
        subagent: None,
        user_message,
        id: block_id(value, raw_index),
        role: role.to_string(),
        kind: normalize_kind(payload_type).to_string(),
        display_kind: display_kind_for_payload(payload_type).map(str::to_string),
        status: payload_status(payload),
        text,
        summary: None,
        input: None,
        truncated: Some(false),
        resolved: resolved_for_payload(payload_type, payload),
        answers: Vec::new(),
        plan_status: plan_status(payload),
        group_id: event_turn_id(value).or_else(|| payload_item_id(value, payload)),
        tool_name: tool_name(payload, payload_type),
        call_id: payload_call_id(payload),
        turn_id: event_turn_id(value),
        item_id: payload_item_id(value, payload),
        created_at: event_time(value),
        questions: Vec::new(),
        payload: None,
    })
}

fn is_plan_item_completed(value: &Value) -> bool {
    rollout_event_type(value) == "item_completed"
        && value
            .get("item")
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("Plan")
}

fn pending_elicitation_block(value: &Value, raw_index: usize) -> Option<MessageBlock> {
    let elicitation = parse_pending_elicitation(value)?;
    Some(MessageBlock {
        subagent: None,
        user_message: None,
        id: block_id(value, raw_index),
        role: "assistant".to_string(),
        kind: "request_user_input".to_string(),
        display_kind: Some("question".to_string()),
        status: Some("pending".to_string()),
        text: elicitation
            .questions
            .first()
            .map(|question| question.question.clone()),
        summary: elicitation
            .questions
            .first()
            .map(|question| tool_summary(&question.question)),
        input: None,
        truncated: Some(false),
        resolved: Some(false),
        answers: Vec::new(),
        plan_status: None,
        group_id: event_call_id(value).or_else(|| elicitation.item_id.clone()),
        tool_name: Some("request_user_input".to_string()),
        call_id: event_call_id(value),
        turn_id: elicitation.turn_id.clone(),
        item_id: elicitation.item_id.clone(),
        created_at: event_time(value),
        questions: elicitation.questions,
        payload: Some(value.clone()),
    })
}

fn request_user_input_hook_message(value: &Value) -> String {
    let Some(elicitation) = parse_pending_elicitation(value) else {
        return "Request user input".to_string();
    };
    elicitation
        .questions
        .iter()
        .enumerate()
        .map(|(question_index, question)| {
            let mut text = format!("问题 {}：{}", question_index + 1, question.question);
            for (option_index, option) in question.options.iter().enumerate() {
                text.push('\n');
                text.push_str(&format!("选项 {}：{}", option_index + 1, option.label));
                if let Some(description) = option.description.as_deref() {
                    if !description.trim().is_empty() {
                        text.push('\n');
                        text.push_str("说明：");
                        text.push_str(description.trim());
                    }
                }
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn plan_delta_block(value: &Value, raw_index: usize) -> Option<MessageBlock> {
    let payload = event_payload(value).unwrap_or(value);
    let payload_type = event_payload_type(value, payload);
    if !is_plan_delta_kind(payload_type) {
        return None;
    }
    let text = structured_text_raw(payload)
        .or_else(|| structured_text_raw(value))
        .or_else(|| {
            payload
                .get("delta")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            payload
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            payload
                .get("markdown")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let status = if is_final_plan_kind(payload_type) {
        payload_status(payload).unwrap_or_else(|| "pending".to_string())
    } else {
        "streaming".to_string()
    };
    Some(MessageBlock {
        subagent: None,
        user_message: None,
        id: plan_block_id(value, payload, raw_index),
        role: "assistant".to_string(),
        kind: "plan".to_string(),
        display_kind: Some("plan".to_string()),
        status: Some(status.clone()),
        text,
        summary: Some("Proposed Plan".to_string()),
        input: None,
        truncated: Some(false),
        resolved: Some(is_finished_status(&status)),
        answers: Vec::new(),
        plan_status: Some(status),
        group_id: plan_group_id(value, payload),
        tool_name: None,
        call_id: None,
        turn_id: event_turn_id(value)
            .or_else(|| {
                payload
                    .get("turnId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| {
                payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
        item_id: payload_item_id(value, payload),
        created_at: event_time(value),
        questions: Vec::new(),
        payload: None,
    })
}

fn user_input_answer_block(value: &Value, raw_index: usize) -> Option<MessageBlock> {
    if !is_user_input_answer(value) {
        return None;
    }
    let payload = value.get("payload").unwrap_or(value);
    let answers = parse_user_input_answers(payload);
    let call_id = event_call_id(value)
        .or_else(|| payload_call_id(payload))
        .or_else(|| payload_item_id(value, payload));
    Some(MessageBlock {
        subagent: None,
        user_message: None,
        id: block_id(value, raw_index),
        role: "assistant".to_string(),
        kind: "request_user_input_result".to_string(),
        display_kind: Some("question_result".to_string()),
        status: Some("completed".to_string()),
        text: Some(answer_summary(&answers)),
        summary: Some(answer_summary(&answers)),
        input: None,
        truncated: Some(false),
        resolved: Some(true),
        answers,
        plan_status: None,
        group_id: call_id.clone(),
        tool_name: Some("request_user_input".to_string()),
        call_id,
        turn_id: event_turn_id(value)
            .or_else(|| {
                payload
                    .get("turnId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| {
                payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
        item_id: payload_item_id(value, payload),
        created_at: event_time(value),
        questions: Vec::new(),
        payload: Some(value.clone()),
    })
}

fn user_input_output_block(
    value: &Value,
    payload: &Value,
    raw_index: usize,
    call_id: Option<String>,
) -> MessageBlock {
    let answers = parse_user_input_output_answers(payload);
    MessageBlock {
        subagent: None,
        user_message: None,
        id: block_id(value, raw_index),
        role: "assistant".to_string(),
        kind: "request_user_input_result".to_string(),
        display_kind: Some("question_result".to_string()),
        status: Some("completed".to_string()),
        text: Some(answer_summary(&answers)),
        summary: Some(answer_summary(&answers)),
        input: None,
        truncated: Some(false),
        resolved: Some(true),
        answers,
        plan_status: None,
        group_id: call_id.clone(),
        tool_name: Some("request_user_input".to_string()),
        call_id,
        turn_id: event_turn_id(value),
        item_id: payload_item_id(value, payload),
        created_at: event_time(value),
        questions: Vec::new(),
        payload: Some(value.clone()),
    }
}

fn tool_output_block(
    value: &Value,
    payload: &Value,
    payload_type: &str,
    raw_index: usize,
    pending: Option<PendingToolCall>,
) -> MessageBlock {
    let raw_text = structured_text_raw(payload)
        .or_else(|| structured_text_raw(value))
        .or_else(|| pretty_json(payload));
    let (text, truncated) = raw_text
        .as_deref()
        .map(|text| trim_text_with_limit(text, TOOL_TEXT_LIMIT))
        .unwrap_or_else(|| (String::new(), false));
    let call_id =
        payload_call_id(payload).or_else(|| pending.as_ref().and_then(|call| call.call_id.clone()));
    let input = pending
        .as_ref()
        .and_then(|call| call.input.clone())
        .or_else(|| tool_input_text(payload));
    let summary = if text.trim().is_empty() {
        input.as_deref().map(tool_summary)
    } else {
        Some(tool_summary(&text))
    };
    MessageBlock {
        subagent: pending.as_ref().and_then(|call| call.subagent.clone()),
        user_message: None,
        id: pending
            .as_ref()
            .map(|call| call.id.clone())
            .unwrap_or_else(|| block_id(value, raw_index)),
        role: "tool".to_string(),
        kind: normalize_kind(payload_type).to_string(),
        display_kind: Some("tool".to_string()),
        status: payload_status(payload)
            .or_else(|| pending.as_ref().and_then(|call| call.status.clone()))
            .or_else(|| Some("completed".to_string())),
        text: if text.is_empty() { None } else { Some(text) },
        summary,
        input,
        truncated: Some(truncated),
        resolved: Some(true),
        answers: Vec::new(),
        plan_status: None,
        group_id: call_id.clone(),
        tool_name: pending
            .as_ref()
            .and_then(|call| call.tool_name.clone())
            .or_else(|| tool_name(payload, payload_type)),
        call_id,
        turn_id: event_turn_id(value)
            .or_else(|| pending.as_ref().and_then(|call| call.turn_id.clone())),
        item_id: payload_item_id(value, payload)
            .or_else(|| pending.as_ref().and_then(|call| call.item_id.clone())),
        created_at: event_time(value)
            .or_else(|| pending.as_ref().and_then(|call| call.created_at.clone())),
        questions: Vec::new(),
        payload: None,
    }
}

fn parse_pending_elicitation(value: &Value) -> Option<PendingElicitation> {
    let payload = value.get("payload").unwrap_or(value);
    if !is_request_user_input(value) {
        return None;
    }
    let args = payload
        .get("arguments")
        .and_then(parse_arguments_value)
        .or_else(|| {
            payload
                .pointer("/input/arguments")
                .and_then(parse_arguments_value)
        })
        .or_else(|| payload.get("params").cloned())
        .or_else(|| value.get("params").cloned())
        .or_else(|| Some(payload.clone()))?;
    let questions = super::normalize_user_input_questions(&args)?;
    Some(PendingElicitation {
        turn_id: payload
            .get("turnId")
            .or_else(|| payload.get("turn_id"))
            .or_else(|| value.get("turnId"))
            .or_else(|| value.get("turn_id"))
            .or_else(|| value.pointer("/params/turnId"))
            .or_else(|| value.pointer("/params/turn_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| event_turn_id(value)),
        item_id: payload
            .get("itemId")
            .or_else(|| payload.get("item_id"))
            .or_else(|| value.get("itemId"))
            .or_else(|| value.get("item_id"))
            .or_else(|| value.pointer("/params/itemId"))
            .or_else(|| value.pointer("/params/item_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| event_call_id(value)),
        questions,
    })
}

fn parse_arguments_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(text).ok(),
        other => Some(other.clone()),
    }
}

fn update_turn_state(value: &Value, scan: &mut RolloutScan) {
    let event_type = rollout_event_type(value);
    let turn_id = event_turn_id(value);
    if matches!(event_type, "task_started" | "turn_started" | "turn/started") {
        scan.active_turn_id = turn_id.clone();
        scan.running = true;
    }
    if is_turn_terminal_event(event_type) && (turn_id.is_none() || scan.active_turn_id == turn_id) {
        scan.active_turn_id = None;
    }
}

fn block_id(value: &Value, raw_index: usize) -> String {
    value
        .pointer("/payload/id")
        .or_else(|| value.pointer("/item/id"))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("raw-{raw_index}"))
}

fn event_time(value: &Value) -> Option<String> {
    value
        .get("timestamp")
        .or_else(|| value.get("created_at"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn role_for_kind(kind: &str) -> &'static str {
    match kind {
        "userMessage" | "input_text" => "user",
        "function_call"
        | "function_call_output"
        | "custom_tool_call"
        | "custom_tool_call_output"
        | "tool_search_call"
        | "tool_search_output"
        | "web_search_call"
        | "web_search_end"
        | "commandExecution"
        | "mcpToolCall"
        | "dynamicToolCall"
        | "fileChange" => "tool",
        _ => "assistant",
    }
}

fn normalize_kind(kind: &str) -> &str {
    match kind {
        "Plan" => "plan",
        "PlanDelta" | "planDelta" | "plan_delta" | "item/plan/delta" => "plan",
        "turn/plan/updated" => "plan",
        other => other,
    }
}

fn is_plan_delta_kind(kind: &str) -> bool {
    let normalized = kind.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "plan" | "plandelta" | "plan_delta" | "item/plan/delta" | "turn/plan/updated"
    )
}

fn is_final_plan_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Plan" | "plan" | "turn/plan/updated" | "plan/updated" | "planUpdated"
    )
}

fn plan_block_id(value: &Value, payload: &Value, raw_index: usize) -> String {
    payload_item_id(value, payload)
        .or_else(|| event_turn_id(value).map(|turn| format!("plan-{turn}")))
        .unwrap_or_else(|| block_id(value, raw_index))
}

fn plan_group_key(block: &MessageBlock, fallback_index: usize) -> String {
    block
        .turn_id
        .as_deref()
        .map(|turn| format!("plan-turn-{turn}"))
        .or_else(|| {
            block
                .item_id
                .as_deref()
                .map(|item| format!("plan-item-{item}"))
        })
        .or_else(|| block.group_id.clone())
        .or_else(|| Some(format!("plan-{fallback_index}")))
        .unwrap()
}

fn plan_group_id(value: &Value, payload: &Value) -> Option<String> {
    event_turn_id(value)
        .or_else(|| {
            payload
                .get("turnId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            payload
                .get("turn_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|turn| format!("plan-turn-{turn}"))
        .or_else(|| payload_item_id(value, payload).map(|item| format!("plan-item-{item}")))
}

fn event_payload(value: &Value) -> Option<&Value> {
    value
        .get("payload")
        .or_else(|| value.get("item"))
        .or_else(|| value.get("message"))
}

fn event_payload_type<'a>(value: &'a Value, payload: &'a Value) -> &'a str {
    payload
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("event")
}

fn payload_status(payload: &Value) -> Option<String> {
    payload
        .get("status")
        .and_then(|status| status.get("type").or(Some(status)))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn plan_status(payload: &Value) -> Option<String> {
    payload_status(payload)
        .or_else(|| {
            payload
                .get("plan_status")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            payload
                .get("planStatus")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn display_kind_for_payload(kind: &str) -> Option<&'static str> {
    let normalized = normalize_kind(kind).to_ascii_lowercase();
    if normalized.contains("plan") {
        Some("plan")
    } else if normalized.contains("approval") || normalized.contains("permissions/request") {
        Some("approval")
    } else {
        None
    }
}

fn resolved_for_payload(kind: &str, payload: &Value) -> Option<bool> {
    let normalized = normalize_kind(kind).to_ascii_lowercase();
    if normalized.contains("plan") {
        return payload_status(payload)
            .as_deref()
            .map(is_finished_status)
            .or(Some(false));
    }
    None
}

fn payload_call_id(payload: &Value) -> Option<String> {
    payload
        .get("call_id")
        .or_else(|| payload.get("callId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn payload_item_id(value: &Value, payload: &Value) -> Option<String> {
    payload
        .get("id")
        .or_else(|| value.get("item_id"))
        .or_else(|| value.get("itemId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn is_tool_call_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_call" | "custom_tool_call" | "tool_search_call" | "web_search_call"
    )
}

fn is_tool_output_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_call_output"
            | "custom_tool_call_output"
            | "tool_search_output"
            | "web_search_end"
            | "web_search_output"
    )
}

fn is_action_display_kind(kind: &str) -> bool {
    let normalized = kind.to_ascii_lowercase();
    matches!(normalized.as_str(), "plan" | "plandelta" | "plan_delta")
        || normalized.contains("plan/delta")
        || normalized.contains("turn/plan/updated")
        || normalized.contains("approval")
        || normalized.contains("permissions/request")
}

fn is_internal_display_kind(kind: &str) -> bool {
    let normalized = kind.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "reasoning"
            | "reasoning_delta"
            | "agent_reasoning"
            | "internal"
            | "subagent"
            | "system"
            | "developer"
            | "session_meta"
    ) || normalized.contains("reasoning")
        || normalized.contains("internal")
        || normalized.contains("subagent")
}

fn is_internal_context_message_text(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    [
        "<environment_context>",
        "<permissions instructions>",
        "<app-context>",
        "<collaboration_mode>",
        "<skills_instructions>",
        "<plugins_instructions>",
        "<subagent_notification>",
        "<subagent_context>",
        "<codex_internal_context",
        "<goal_context>",
        "<additional_context>",
        "<user_instructions>",
        "<turn_aborted>",
        "<user_shell_command>",
        "<legacy_unified_exec_process_limit_warning>",
        "<legacy_apply_patch_exec_command_warning>",
        "<legacy_model_mismatch_warning>",
        "========= memory_summary begins =========",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn is_hidden_user_action_text(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    [
        "<turn_aborted>",
        "<user_shell_command>",
        "========= memory_summary begins =========",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn contains_proposed_plan(text: &str) -> bool {
    text.contains("<proposed_plan>")
}

fn rollout_metadata_title(payload: &Value) -> Option<String> {
    [
        "thread_title",
        "threadTitle",
        "title",
        "name",
        "session_title",
        "sessionTitle",
    ]
    .into_iter()
    .find_map(|field| {
        payload
            .get(field)
            .and_then(Value::as_str)
            .and_then(thread_title_candidate)
    })
}

pub(crate) fn first_user_message_title(text: &str) -> Option<String> {
    let value = trim_text(&crate::user_message::summary_text(text), 80);
    is_usable_thread_title(&value).then_some(value)
}

pub(crate) fn choose_initial_thread_title(
    title: Option<&str>,
    first_user_message: Option<&str>,
) -> String {
    title
        .and_then(thread_title_candidate)
        .or_else(|| first_user_message.and_then(first_user_message_title))
        .unwrap_or_else(|| "未命名线程".to_string())
}

pub(crate) fn should_repair_thread_title_from_local_metadata(
    db_title: Option<&str>,
    first_user_message: Option<&str>,
    index_entry: Option<&SessionIndexEntry>,
) -> bool {
    if db_title.and_then(thread_title_candidate).is_none() {
        return true;
    }
    if index_entry
        .and_then(SessionIndexEntry::title_candidate)
        .is_none()
    {
        return false;
    }
    let Some(db_title) = db_title.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let Some(first_user_message) = first_user_message
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    db_title == first_user_message
}

pub(crate) fn thread_title_candidate(text: &str) -> Option<String> {
    let value = trim_text(text, 80);
    (is_usable_thread_title(&value) && text.trim().chars().count() <= 120).then_some(value)
}

pub(crate) fn is_usable_thread_title(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "untitled" | "untitled thread" | "未命名线程" | "读取中" | "loading"
    ) || lower.starts_with("读取中")
        || lower.starts_with("loading")
        || lower.contains("<proposed_plan>")
        || looks_like_assistant_preview_title(trimmed)
        || is_internal_context_message_text(trimmed)
    {
        return false;
    }
    true
}

fn looks_like_assistant_preview_title(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    (lower.starts_with("assistant preview:") || lower.starts_with("assistant:"))
        && text.chars().count() > 40
}

fn is_subagent_session_meta(payload: &Value) -> bool {
    let source_value = payload
        .get("thread_source")
        .or_else(|| payload.get("threadSource"))
        .or_else(|| payload.get("source"))
        .or_else(|| payload.get("sourceKind"))
        .or_else(|| payload.get("source_kind"));
    let source = source_value.and_then(Value::as_str);
    is_subagent_metadata(
        source,
        payload
            .get("parent_thread_id")
            .or_else(|| payload.get("parentThreadId"))
            .and_then(Value::as_str),
        payload
            .get("agent_path")
            .or_else(|| payload.get("agentPath"))
            .and_then(Value::as_str),
        payload
            .get("agent_nickname")
            .or_else(|| payload.get("agentNickname"))
            .and_then(Value::as_str),
        payload
            .get("agent_role")
            .or_else(|| payload.get("agentRole"))
            .and_then(Value::as_str),
    ) || source_value.is_some_and(value_contains_subagent)
}

pub(crate) fn is_subagent_metadata(
    source: Option<&str>,
    parent_thread_id: Option<&str>,
    agent_path: Option<&str>,
    agent_nickname: Option<&str>,
    agent_role: Option<&str>,
) -> bool {
    non_empty(parent_thread_id).is_some()
        || non_empty(agent_path).is_some()
        || source.is_some_and(source_text_contains_subagent)
        || non_empty(agent_nickname).is_some()
        || non_empty(agent_role).is_some()
}

#[derive(Clone, Copy)]
pub(crate) struct ThreadVisibilityMetadata<'a> {
    pub(crate) thread_source: Option<&'a str>,
    pub(crate) source: Option<&'a str>,
    pub(crate) parent_thread_id: Option<&'a str>,
    pub(crate) agent_path: Option<&'a str>,
    pub(crate) agent_nickname: Option<&'a str>,
    pub(crate) agent_role: Option<&'a str>,
    pub(crate) has_user_event: Option<i64>,
    pub(crate) title: Option<&'a str>,
    pub(crate) first_user_message: Option<&'a str>,
    pub(crate) preview: Option<&'a str>,
}

pub(crate) fn is_hidden_thread_metadata(metadata: ThreadVisibilityMetadata<'_>) -> bool {
    hidden_thread_metadata_category(metadata).is_some()
}

pub(crate) fn hidden_thread_metadata_category(
    metadata: ThreadVisibilityMetadata<'_>,
) -> Option<String> {
    if is_internal_thread_metadata(metadata) {
        return Some("internal".to_string());
    }
    if is_subagent_metadata(
        metadata.thread_source,
        metadata.parent_thread_id,
        metadata.agent_path,
        metadata.agent_nickname,
        metadata.agent_role,
    ) || metadata.source.is_some_and(source_text_contains_subagent)
    {
        return Some("subagent".to_string());
    }
    None
}

pub(crate) fn is_internal_thread_metadata(metadata: ThreadVisibilityMetadata<'_>) -> bool {
    let source = metadata
        .source
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let thread_source = metadata
        .thread_source
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if source != "exec" || metadata.has_user_event.unwrap_or(0) != 0 {
        return false;
    }
    if !thread_source.is_empty() && thread_source != "user" {
        return false;
    }
    [
        metadata.title,
        metadata.first_user_message,
        metadata.preview,
    ]
    .into_iter()
    .flatten()
    .any(is_internal_thread_prompt_text)
}

fn is_internal_thread_prompt_text(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    if text.is_empty() {
        return false;
    }
    let readonly_probe = text.contains("只读验证")
        || text.contains("只读核查")
        || text.contains("不要修改文件")
        || text.contains("不改文件")
        || text.contains("read-only")
        || text.contains("readonly");
    let agent_probe = text.contains("spawn_agent")
        || text.contains("子代理")
        || text.contains("subagent")
        || text.contains("model_reasoning_effort=xhigh");
    if readonly_probe && agent_probe {
        return true;
    }

    let strong_subagent_instruction = text.contains("你是子代理")
        || text.contains("你是并行子代理")
        || text.contains("you are a subagent");
    let fixed_agent_config = text.contains("gpt-5.5")
        || text.contains("xhigh")
        || text.contains("model_reasoning_effort=xhigh");
    strong_subagent_instruction && fixed_agent_config
}

pub(crate) fn source_text_contains_subagent(value: &str) -> bool {
    value.to_ascii_lowercase().contains("subagent")
        || serde_json::from_str::<Value>(value)
            .ok()
            .as_ref()
            .is_some_and(value_contains_subagent)
}

pub(crate) fn thread_source_label(value: Option<&str>) -> String {
    let Some(value) = non_empty(value) else {
        return "unknown".to_string();
    };
    if source_text_contains_subagent(value) {
        return "subagent".to_string();
    }
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| {
            value
                .get("kind")
                .or_else(|| value.get("type"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| value.to_ascii_lowercase())
}

fn value_contains_subagent(value: &Value) -> bool {
    match value {
        Value::String(text) => text.to_ascii_lowercase().contains("subagent"),
        Value::Array(items) => items.iter().any(value_contains_subagent),
        Value::Object(map) => map.iter().any(|(key, value)| {
            key.to_ascii_lowercase().contains("subagent") || value_contains_subagent(value)
        }),
        _ => false,
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn tool_name(payload: &Value, kind: &str) -> Option<String> {
    payload
        .get("name")
        .or_else(|| payload.get("tool"))
        .or_else(|| payload.get("server"))
        .or_else(|| payload.get("method"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| match kind {
            "tool_search_call" | "tool_search_output" => Some("tool_search".to_string()),
            "web_search_call" | "web_search_end" | "web_search_output" => {
                Some("web_search".to_string())
            }
            _ => None,
        })
}

fn tool_input_text(payload: &Value) -> Option<String> {
    let value = payload
        .get("arguments")
        .or_else(|| payload.get("input"))
        .or_else(|| payload.get("params"))
        .or_else(|| payload.get("command"))?;
    let raw = match value {
        Value::String(text) => text.clone(),
        other => pretty_json(other)?,
    };
    let (text, _) = trim_preserving_indentation(&raw, TOOL_INPUT_LIMIT);
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn structured_text(value: &Value) -> Option<String> {
    structured_text_raw(value).map(|text| trim_text(&text, MESSAGE_TEXT_LIMIT))
}

fn structured_text_raw(value: &Value) -> Option<String> {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("output").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("aggregatedOutput").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(command) = value.get("command").and_then(Value::as_str) {
        return Some(command.to_string());
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .or_else(|| item.get("input_text"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

fn pretty_json(value: &Value) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

fn collect_text(value: &Value, out: &mut String) {
    match value {
        Value::String(s) => {
            if looks_like_message(s) {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(s);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_text(value, out);
            }
        }
        Value::Object(map) => {
            for key in [
                "payload", "item", "text", "message", "content", "summary", "output",
            ] {
                if let Some(value) = map.get(key) {
                    collect_text(value, out);
                }
            }
        }
        _ => {}
    }
}

fn looks_like_message(s: &str) -> bool {
    let s = s.trim();
    s.len() > 1
        && !s.starts_with("019")
        && !s.starts_with("call_")
        && !matches!(s, "assistant" | "user" | "system" | "function_call")
}

pub(crate) fn is_request_user_input(value: &Value) -> bool {
    super::user_input::question_tool(value).is_some()
}

fn is_user_input_answer(value: &Value) -> bool {
    let payload = value.get("payload").unwrap_or(value);
    [
        value.get("type").and_then(Value::as_str),
        value.get("name").and_then(Value::as_str),
        value.get("method").and_then(Value::as_str),
        payload.get("type").and_then(Value::as_str),
        payload.get("name").and_then(Value::as_str),
        payload.get("method").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .any(|name| {
        matches!(
            name,
            "UserInputAnswer" | "userInputAnswer" | "user_input_answer"
        )
    })
}

fn parse_user_input_answers(payload: &Value) -> Vec<UserInputAnswer> {
    let source = payload
        .get("answers")
        .or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("answers"))
        })
        .or_else(|| {
            payload
                .get("result")
                .and_then(|result| result.get("answers"))
        })
        .unwrap_or(payload);
    match source {
        Value::Object(map) => map
            .iter()
            .map(|(question_id, value)| UserInputAnswer {
                question_id: question_id.clone(),
                answers: answer_values(value),
                note: answer_note(value),
            })
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let question_id = item
                    .get("question_id")
                    .or_else(|| item.get("questionId"))
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("q{}", index + 1));
                UserInputAnswer {
                    question_id,
                    answers: answer_values(
                        item.get("answers")
                            .or_else(|| item.get("answer"))
                            .unwrap_or(item),
                    ),
                    note: answer_note(item),
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_user_input_output_answers(payload: &Value) -> Vec<UserInputAnswer> {
    payload
        .get("output")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .map(|value| parse_user_input_answers(&value))
        .filter(|answers| !answers.is_empty())
        .unwrap_or_else(|| parse_user_input_answers(payload))
}

fn answer_values(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Object(map) => map
                    .get("label")
                    .or_else(|| map.get("text"))
                    .or_else(|| map.get("value"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                _ => None,
            })
            .collect(),
        Value::Object(map) => map
            .get("answers")
            .or_else(|| map.get("answer"))
            .map(answer_values)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn answer_note(value: &Value) -> Option<String> {
    value
        .get("note")
        .or_else(|| value.get("user_note"))
        .or_else(|| value.get("userNote"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn answer_summary(answers: &[UserInputAnswer]) -> String {
    if answers.is_empty() {
        return "Questions answered".to_string();
    }
    let answered = answers
        .iter()
        .filter(|answer| !answer.answers.is_empty() || answer.note.is_some())
        .count();
    format!("{answered}/{} answered", answers.len())
}

fn is_finished_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(
        status.as_str(),
        "completed"
            | "complete"
            | "done"
            | "finished"
            | "success"
            | "succeeded"
            | "failed"
            | "error"
            | "cancelled"
            | "canceled"
            | "interrupted"
    )
}

fn is_running_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "pending" | "running" | "in_progress" | "inprogress" | "active"
    )
}

fn is_recoverable_terminal_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "failed" | "error" | "cancelled" | "canceled" | "interrupted"
    )
}

fn trim_text(text: &str, max: usize) -> String {
    trim_text_with_limit(text, max).0
}

fn trim_text_with_limit(text: &str, max: usize) -> (String, bool) {
    let compact = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if compact.chars().count() <= max {
        (compact, false)
    } else {
        (compact.chars().take(max).collect::<String>() + "...", true)
    }
}

fn trim_preserving_indentation(text: &str, max: usize) -> (String, bool) {
    let compact = text.trim().to_string();
    if compact.chars().count() <= max {
        (compact, false)
    } else {
        (compact.chars().take(max).collect::<String>() + "...", true)
    }
}

fn tool_summary(text: &str) -> String {
    let compact = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if compact.chars().count() <= TOOL_SUMMARY_LIMIT {
        compact.to_string()
    } else {
        compact.chars().take(TOOL_SUMMARY_LIMIT).collect::<String>() + "..."
    }
}
