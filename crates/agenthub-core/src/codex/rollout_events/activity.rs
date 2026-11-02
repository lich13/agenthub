//! Mechanical extraction of the existing Codex activity path.
use super::*;

#[derive(Default, Clone)]
pub(crate) struct RolloutScan {
    pub(crate) session_id: Option<String>,
    pub(crate) identity_conflict: bool,
    pub(crate) message_count: usize,
    pub(crate) latest_message: Option<String>,
    pub(crate) last_message_at: Option<String>,
    pub(crate) reply_needed: bool,
    pub(crate) recoverable: bool,
    pub(crate) running: bool,
    pub(crate) cwd: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) first_user_message_title: Option<String>,
    pub(crate) active_turn_id: Option<String>,
    pub(crate) pending_elicitation: Option<PendingElicitation>,
    pub(crate) last_event_kind: Option<String>,
    pub(crate) is_subagent: bool,
}

#[derive(Debug, Clone)]
pub(super) enum PendingAction {
    Plan {
        turn_id: Option<String>,
        item_id: Option<String>,
    },
    Elicitation {
        turn_id: Option<String>,
        item_id: Option<String>,
        call_id: Option<String>,
        elicitation: PendingElicitation,
    },
}

#[derive(Debug, Clone)]
struct PendingHookStopAction {
    action: PendingAction,
    message: String,
    source: &'static str,
    turn_id: Option<String>,
    line: usize,
}

#[derive(Debug, Clone)]
pub struct RolloutMessageSelection {
    pub message: String,
    pub source: String,
    pub strategy: String,
    pub selected_turn_id: Option<String>,
    pub selected_line: Option<usize>,
    pub candidate_count: usize,
}

#[derive(Default)]
pub(super) struct ScanState {
    scan: RolloutScan,
    pending_action: Option<PendingAction>,
    async_questions: super::super::user_input::AsyncQuestionTracker,
    current_plan_marker: Option<(Option<String>, Option<String>)>,
    last_task_status: Option<String>,
    active_tasks: Vec<Option<String>>,
    pending_tool_turns: HashMap<String, Option<String>>,
    turn_context: Option<String>,
}
impl ScanState {
    pub(super) fn push(&mut self, mut value: Value) {
        let Self {
            mut scan,
            mut pending_action,
            mut async_questions,
            mut current_plan_marker,
            mut last_task_status,
            mut active_tasks,
            mut pending_tool_turns,
            mut turn_context,
        } = std::mem::take(self);
        normalize_canonical_turn(&mut value, &mut turn_context);
        async_questions.push(&value, 0);
        let metadata = value
            .get("session_meta")
            .and_then(|v| v.get("payload"))
            .or_else(|| (value["type"] == "session_meta").then(|| &value["payload"]));
        if let Some(id) = metadata
            .and_then(|m| m["id"].as_str())
            .filter(|id| !id.is_empty())
        {
            scan.identity_conflict |= scan.session_id.as_deref().is_some_and(|old| old != id);
            scan.session_id = Some(id.to_owned());
        }
        if let Some(payload) = value.get("session_meta").and_then(|v| v.get("payload")) {
            scan.is_subagent |= is_subagent_session_meta(payload);
            scan.cwd = payload
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or(scan.cwd);
            scan.model = payload
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or(scan.model);
            if scan.title.is_none() {
                scan.title = rollout_metadata_title(payload);
            }
        }
        let event_type = rollout_event_type(&value);
        if !event_type.is_empty() {
            scan.last_event_kind = Some(event_type.to_string());
        }
        update_turn_state(&value, &mut scan);
        if is_turn_terminal_event(event_type) {
            let completed_turn_id = event_turn_id(&value);
            if let Some(turn_id) = &completed_turn_id {
                if retire_active_turn_boundary(&mut active_tasks, &mut pending_tool_turns, turn_id)
                {
                    scan.active_turn_id = latest_active_task_turn(&active_tasks);
                }
            } else {
                active_tasks.clear();
                pending_tool_turns.clear();
                scan.active_turn_id = None;
            }
            if scan.active_turn_id.is_none() {
                scan.recoverable = false;
            }
        }
        if matches!(event_type, "task_started" | "turn_started" | "turn/started") {
            if event_type == "task_started" {
                scan.recoverable = false;
            }
            if let Some(turn_id) = event_turn_id(&value) {
                scan.active_turn_id = Some(turn_id.clone());
                if !active_tasks
                    .iter()
                    .any(|active| active.as_deref() == Some(turn_id.as_str()))
                {
                    active_tasks.push(Some(turn_id));
                }
            } else if event_type == "task_started" {
                active_tasks.push(None);
            }
            scan.running = true;
        }
        if event_type == "item_completed"
            && value
                .get("item")
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                == Some("Plan")
        {
            current_plan_marker = Some((event_turn_id(&value), event_item_id(&value)));
        }
        if event_type == "task_complete" {
            let completed_turn_id = event_turn_id(&value);
            if let Some(turn_id) = completed_turn_id.as_deref() {
                if retire_active_turn_boundary(&mut active_tasks, &mut pending_tool_turns, turn_id)
                {
                    scan.active_turn_id = latest_active_task_turn(&active_tasks);
                }
            } else {
                if let Some(index) = active_tasks.iter().rposition(Option::is_none) {
                    active_tasks.remove(index);
                    scan.active_turn_id = latest_active_task_turn(&active_tasks);
                }
            }
            let payload = value.get("payload").unwrap_or(&value);
            last_task_status = value
                .get("status")
                .and_then(Value::as_str)
                .or_else(|| payload.get("status").and_then(Value::as_str))
                .or_else(|| value.get("turn_status").and_then(Value::as_str))
                .or_else(|| payload.get("turn_status").and_then(Value::as_str))
                .map(str::to_string);
            let last_agent_null = value
                .get("last_agent_message")
                .or_else(|| payload.get("last_agent_message"))
                .map(Value::is_null)
                .unwrap_or(false);
            let last_agent_message = value
                .get("last_agent_message")
                .or_else(|| payload.get("last_agent_message"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty());
            if completed_turn_id.is_none()
                && last_agent_message.is_some()
                && scan.active_turn_id.is_none()
            {
                clear_anonymous_pending_tools(&mut pending_tool_turns);
            }
            if last_agent_null {
                if let Some((turn_id, item_id)) =
                    plan_marker_for_event(&current_plan_marker, &value)
                {
                    pending_action = Some(PendingAction::Plan { turn_id, item_id });
                }
            }
            if last_agent_message.is_some() {
                scan.recoverable = false;
            } else if last_agent_null
                && last_task_status
                    .as_deref()
                    .is_some_and(is_recoverable_terminal_status)
            {
                scan.recoverable = true;
            }
            current_plan_marker = None;
        }
        if is_request_user_input(&value)
            && !super::super::user_input::question_tool(&value)
                .is_some_and(super::super::is_async_question_tool)
        {
            if let Some(elicitation) = parse_pending_elicitation(&value) {
                pending_action = Some(PendingAction::Elicitation {
                    turn_id: elicitation.turn_id.clone(),
                    item_id: elicitation.item_id.clone(),
                    call_id: event_call_id(&value),
                    elicitation,
                });
            }
        }
        update_pending_tool_calls(
            &value,
            &mut pending_tool_turns,
            scan.active_turn_id.as_deref(),
        );
        if clears_pending_action(&value, pending_action.as_ref()) {
            pending_action = None;
        }
        if let Some(message) = parse_message_event(&value) {
            scan.message_count += 1;
            scan.last_message_at = message.created_at;
            let text = trim_text(&message.text, 500);
            if message.role == "user" && scan.first_user_message_title.is_none() {
                scan.first_user_message_title = first_user_message_title(&text);
            }
            if !text.is_empty() {
                if text.contains("<proposed_plan>") {
                    let (turn_id, item_id) = plan_marker_for_event(&current_plan_marker, &value)
                        .unwrap_or_else(|| (event_turn_id(&value), event_item_id(&value)));
                    pending_action = Some(PendingAction::Plan { turn_id, item_id });
                    scan.latest_message = Some(
                        extract_proposed_plan_text(&text)
                            .map(|plan| trim_text(&plan, 500))
                            .unwrap_or_else(|| text.clone()),
                    );
                } else {
                    scan.recoverable = false;
                    scan.latest_message = Some(text);
                }
            }
        }

        *self = Self {
            scan,
            pending_action,
            async_questions,
            current_plan_marker,
            last_task_status,
            active_tasks,
            pending_tool_turns,
            turn_context,
        };
    }
    pub(super) fn summary(&self) -> RolloutScan {
        let mut scan = self.scan.clone();
        let pending_action = self.pending_action.clone();
        let async_questions = &self.async_questions;
        let pending_tool_turns = &self.pending_tool_turns;
        let active_tasks = &self.active_tasks;
        let last_task_status = &self.last_task_status;
        if let Some(pending) = pending_action {
            scan.reply_needed = true;
            scan.recoverable = false;
            if let PendingAction::Elicitation { elicitation, .. } = pending {
                scan.pending_elicitation = Some(elicitation);
            }
        }
        if let Some(call) = async_questions.pending().next() {
            scan.reply_needed = true;
            scan.pending_elicitation = call.pending();
        }
        if scan.active_turn_id.is_none() {
            scan.active_turn_id = pending_tool_turns.values().find_map(Clone::clone);
        }
        scan.running = scan.active_turn_id.is_some()
            || !active_tasks.is_empty()
            || !pending_tool_turns.is_empty()
            || matches!(
                last_task_status.as_deref(),
                Some("running" | "in_progress" | "inProgress")
            );
        scan
    }
}
pub(crate) fn scan_rollout(path: &Path, _max_messages: usize) -> Result<RolloutScan> {
    if crate::codex::rollout_file::is_compressed(path) {
        use std::io::{BufRead, BufReader, Read};
        let file = fs::File::open(path)?;
        let decoder = zstd::stream::read::Decoder::new(file)?;
        let mut reader = BufReader::new(decoder.take(64 * 1024 * 1024));
        let mut line = Vec::new();
        let mut state = ScanState::default();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if let Ok(value) = serde_json::from_slice::<Value>(&line) {
                state.push(value);
            }
        }
        return Ok(state.summary());
    }
    static CACHE: crate::jsonl_cache::JsonlCache<ScanState> =
        crate::jsonl_cache::JsonlCache::new(crate::resource_limits::CODEX_SCAN_CACHE_BYTES);
    CACHE.read(
        path,
        |_| Ok(0),
        true,
        |state, line, _, _| {
            if let Ok(value) = serde_json::from_slice::<Value>(line) {
                state.push(value);
            }
            Ok(())
        },
        |state, _| Ok(state.summary()),
        |state| {
            8192 + state.scan.latest_message.as_ref().map_or(0, String::len) as u64
                + state.async_questions.calls.len() as u64 * 4096
                + state.pending_tool_turns.len() as u64 * 256
        },
    )
}

pub fn rollout_latest_assistant_message(path: &Path) -> Result<Option<String>> {
    let scan = scan_rollout(path, 1)?;
    Ok(scan.latest_message)
}

pub fn rollout_hook_stop_message(path: &Path, turn_id: Option<&str>) -> Result<Option<String>> {
    Ok(rollout_hook_stop_message_with_source(path, turn_id)?.map(|(message, _)| message))
}

pub fn rollout_hook_stop_message_with_source(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<(String, String)>> {
    Ok(rollout_hook_stop_message_selection(path, turn_id)?
        .map(|selection| (selection.message, selection.source)))
}

pub fn rollout_hook_stop_message_selection(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<RolloutMessageSelection>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read rollout {}", path.display()))?;
    Ok(select_rollout_message_inner(&text, turn_id))
}

fn select_rollout_message_inner(
    text: &str,
    turn_id: Option<&str>,
) -> Option<RolloutMessageSelection> {
    let mut latest_assistant = None;
    let mut latest_unresolved_action: Option<PendingHookStopAction> = None;
    let mut latest_task_complete = None;
    let mut candidate_count = 0usize;
    let mut current_turn_id: Option<String> = None;
    let mut async_questions = super::super::user_input::AsyncQuestionTracker::default();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        normalize_canonical_turn(&mut value, &mut current_turn_id);
        async_questions.push(&value, line_number);
        let effective_turn_id = event_turn_id(&value);
        let matches_turn_scope = turn_id
            .map(|expected_turn_id| effective_turn_id.as_deref() == Some(expected_turn_id))
            .unwrap_or(true);
        if !matches_turn_scope {
            continue;
        }
        let mut event_added_candidate = false;
        if let Some(plan) = plan_text_from_event(&value) {
            let (action_turn_id, action_item_id) = plan_marker_for_event(&None, &value)
                .unwrap_or_else(|| (event_turn_id(&value), event_item_id(&value)));
            latest_unresolved_action = Some(PendingHookStopAction {
                action: PendingAction::Plan {
                    turn_id: action_turn_id.clone(),
                    item_id: action_item_id,
                },
                message: plan,
                source: "proposed_plan",
                turn_id: action_turn_id.or_else(|| effective_turn_id.clone()),
                line: line_number,
            });
            event_added_candidate = true;
        }
        if let Some(elicitation) = parse_pending_elicitation(&value).filter(|_| {
            !super::super::user_input::question_tool(&value)
                .is_some_and(super::super::is_async_question_tool)
        }) {
            let elicitation_turn_id = elicitation.turn_id.clone();
            latest_unresolved_action = Some(PendingHookStopAction {
                action: PendingAction::Elicitation {
                    turn_id: elicitation_turn_id.clone(),
                    item_id: elicitation.item_id.clone(),
                    call_id: event_call_id(&value),
                    elicitation,
                },
                message: request_user_input_hook_message(&value),
                source: "request_user_input",
                turn_id: elicitation_turn_id.or_else(|| effective_turn_id.clone()),
                line: line_number,
            });
            event_added_candidate = true;
        }
        if let Some(message) = parse_raw_message_event(&value) {
            if message.role == "assistant" && !message.text.trim().is_empty() {
                let source = if extract_proposed_plan_text(&message.text).is_some() {
                    "proposed_plan"
                } else {
                    "last_assistant_message"
                };
                let is_plan = source == "proposed_plan";
                if is_plan {
                    let pending_plan = PendingHookStopAction {
                        action: PendingAction::Plan {
                            turn_id: event_turn_id(&value),
                            item_id: event_item_id(&value),
                        },
                        message: extract_proposed_plan_text(&message.text).unwrap_or(message.text),
                        source: "proposed_plan",
                        turn_id: effective_turn_id.clone(),
                        line: line_number,
                    };
                    if should_replace_pending_hook_stop_action(
                        latest_unresolved_action.as_ref(),
                        &pending_plan,
                    ) {
                        latest_unresolved_action = Some(pending_plan);
                    }
                    event_added_candidate = true;
                } else if is_final_assistant_message(&value) {
                    latest_assistant = Some(RolloutMessageSelection {
                        message: message.text,
                        source: source.to_string(),
                        strategy: source.to_string(),
                        selected_turn_id: effective_turn_id.clone(),
                        selected_line: Some(line_number),
                        candidate_count: 0,
                    });
                    event_added_candidate = true;
                }
            }
        }
        if latest_unresolved_action
            .as_ref()
            .is_some_and(|pending| clears_pending_action(&value, Some(&pending.action)))
        {
            latest_unresolved_action = None;
        }
        if rollout_event_type(&value) != "task_complete" {
            if event_added_candidate {
                candidate_count += 1;
            }
            continue;
        }
        let payload = value.get("payload").unwrap_or(&value);
        let last_agent_message = value
            .get("last_agent_message")
            .or_else(|| payload.get("last_agent_message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
            .map(str::to_string);
        if last_agent_message.is_some() {
            latest_task_complete = last_agent_message.map(|message| RolloutMessageSelection {
                message,
                source: "task_complete.last_agent_message".to_string(),
                strategy: "task_complete.last_agent_message".to_string(),
                selected_turn_id: effective_turn_id.clone(),
                selected_line: Some(line_number),
                candidate_count: 0,
            });
            event_added_candidate = true;
        }
        if event_added_candidate {
            candidate_count += 1;
        }
    }
    if let Some(call) = async_questions
        .pending()
        .find(|call| turn_id.is_none_or(|id| id == call.turn_id))
    {
        return Some(RolloutMessageSelection {
            message: crate::services::probe::format_probe_pending_elicitation(&call.pending()?),
            source: "request_user_input".to_string(),
            strategy: "unresolved_async_question".to_string(),
            selected_turn_id: Some(call.turn_id.clone()),
            selected_line: Some(call.line),
            candidate_count: candidate_count + 1,
        });
    }
    let mut selection = latest_unresolved_action
        .map(|pending| RolloutMessageSelection {
            message: pending.message,
            source: pending.source.to_string(),
            strategy: pending.source.to_string(),
            selected_turn_id: pending.turn_id,
            selected_line: Some(pending.line),
            candidate_count: 0,
        })
        .or(latest_task_complete)
        .or(latest_assistant)?;
    selection.candidate_count = candidate_count;
    Some(selection)
}

fn should_replace_pending_hook_stop_action(
    current: Option<&PendingHookStopAction>,
    next: &PendingHookStopAction,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    match (&current.action, &next.action) {
        (
            PendingAction::Plan {
                turn_id: current_turn,
                item_id: current_item,
            },
            PendingAction::Plan {
                turn_id: next_turn,
                item_id: next_item,
            },
        ) if current_turn == next_turn && (next_item.is_none() || current_item == next_item) => {
            next.message.len() > current.message.len()
        }
        _ => true,
    }
}

pub fn rollout_completion_last_agent_message(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<String>> {
    Ok(
        rollout_completion_last_agent_message_with_source(path, turn_id)?
            .map(|(message, _)| message),
    )
}

pub fn rollout_completion_last_agent_message_with_source(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<(String, String)>> {
    Ok(
        rollout_completion_last_agent_message_selection(path, turn_id)?
            .map(|selection| (selection.message, selection.source)),
    )
}

pub fn rollout_completion_last_agent_message_selection(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<RolloutMessageSelection>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read rollout {}", path.display()))?;
    Ok(select_rollout_message_inner(&text, turn_id))
}

pub fn rollout_has_completed_turn(path: &Path, turn_id: Option<&str>) -> Result<bool> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read rollout {}", path.display()))?;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let event_type = rollout_event_type(&value);
        if !matches!(
            event_type,
            "task_complete" | "turn_completed" | "turn/completed"
        ) {
            continue;
        }
        if let Some(expected_turn_id) = turn_id {
            let event_turn = event_turn_id(&value);
            if event_turn.as_deref() != Some(expected_turn_id) {
                continue;
            }
        }
        if event_type != "task_complete" || task_complete_has_last_agent_message(&value) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantFeedback {
    pub turn_id: String,
    pub body: String,
    pub completed_at_ms: i64,
}

/// Only a complete, latest terminal turn can wait for ordinary-language feedback.
/// Bounded tail reads also make polling large sessions inexpensive.
pub fn rollout_pending_feedback(path: &Path) -> Result<Option<AssistantFeedback>> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let size = metadata.len();
    let start = size.saturating_sub(2 * 1024 * 1024);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if !bytes.ends_with(b"\n") {
        return Ok(None);
    }
    if start > 0 {
        let Some(end) = bytes.iter().position(|b| *b == b'\n') else {
            return Ok(None);
        };
        bytes.drain(..=end);
    }
    let mut current_turn = None;
    let mut final_reply: Option<String> = None;
    let mut completed = None;
    let mut pending = HashMap::new();
    for line in String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            return Ok(None);
        };
        let previous_turn = current_turn.clone();
        normalize_canonical_turn(&mut value, &mut current_turn);
        let kind = rollout_event_type(&value);
        if previous_turn != current_turn
            || matches!(kind, "task_started" | "turn_started" | "turn/started")
        {
            final_reply = None;
            completed = None;
            pending.clear();
        }
        if let Some(message) = parse_message_event(&value) {
            if message.role == "user" {
                final_reply = None;
                completed = None;
            } else if message.role == "assistant" && is_final_assistant_message(&value) {
                if final_reply.as_deref() != Some(&message.text) {
                    completed = None;
                }
                final_reply = Some(message.text);
            } else if message.role == "assistant" {
                completed = None;
            }
        }
        let before = pending.clone();
        update_pending_tool_calls(&value, &mut pending, current_turn.as_deref());
        let interactive_request = kind.contains("request_user_input")
            || kind.contains("elicitation")
            || value
                .get("payload")
                .map(|payload| payload.to_string().to_ascii_lowercase())
                .is_some_and(|payload| {
                    payload.contains("request_user_input") || payload.contains("elicitation")
                });
        if before != pending
            || interactive_request
            || matches!(
                kind,
                "function_call"
                    | "function_call_output"
                    | "custom_tool_call"
                    | "custom_tool_call_output"
                    | "tool_call"
                    | "tool_result"
            )
        {
            completed = None;
        }
        if matches!(kind, "turn_aborted" | "turn/aborted" | "turn_error") {
            final_reply = None;
            completed = None;
        }
        if matches!(kind, "task_complete" | "turn_completed" | "turn/completed") {
            let payload = value.get("payload").unwrap_or(&value);
            if let Some(message) = payload
                .get("last_agent_message")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                final_reply = Some(message.to_string());
            }
            if payload
                .get("status")
                .or_else(|| payload.pointer("/turn/status"))
                .and_then(Value::as_str)
                .is_some_and(|status| !matches!(status, "completed" | "complete" | "success"))
            {
                final_reply = None;
                completed = None;
                continue;
            }
            let turn = event_turn_id(&value).or_else(|| current_turn.clone());
            let timestamp = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|time| time.timestamp_millis());
            completed = turn.zip(timestamp);
        }
    }
    let after = std::fs::metadata(path)?;
    if metadata.len() != after.len() || metadata.modified()? != after.modified()? {
        return Ok(None);
    }
    if !pending.is_empty() {
        return Ok(None);
    }
    let Some((turn_id, completed_at_ms)) = completed else {
        return Ok(None);
    };
    let Some(body) = final_reply
        .as_deref()
        .and_then(super::super::assistant_feedback_request)
    else {
        return Ok(None);
    };
    Ok(Some(AssistantFeedback {
        turn_id,
        body,
        completed_at_ms,
    }))
}

pub fn rollout_request_user_input_state(
    path: &Path,
    turn_id: &str,
    call_id: &str,
) -> Result<RolloutRequestUserInputState> {
    let turn_id = turn_id.trim();
    let call_id = call_id.trim();
    if turn_id.is_empty() || call_id.is_empty() {
        return Ok(RolloutRequestUserInputState::Missing);
    }

    let Ok(mut file) = File::open(path) else {
        return Ok(RolloutRequestUserInputState::Missing);
    };
    let Ok(size) = file.metadata().map(|metadata| metadata.len()) else {
        return Ok(RolloutRequestUserInputState::Missing);
    };
    let start = size.saturating_sub(REQUEST_USER_INPUT_TAIL_MAX_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Ok(RolloutRequestUserInputState::Missing);
    }
    let mut tail =
        Vec::with_capacity((size - start).min(REQUEST_USER_INPUT_TAIL_MAX_BYTES) as usize);
    if file.read_to_end(&mut tail).is_err() {
        return Ok(RolloutRequestUserInputState::Missing);
    }
    if start > 0 {
        let Some(first_newline) = tail.iter().position(|byte| *byte == b'\n') else {
            return Ok(RolloutRequestUserInputState::Missing);
        };
        tail.drain(..=first_newline);
    }

    let mut found_call = false;
    let mut turn_context = None;
    for line in String::from_utf8_lossy(&tail).lines() {
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        normalize_canonical_turn(&mut value, &mut turn_context);
        if !found_call {
            if is_request_user_input(&value)
                && event_turn_id(&value).as_deref() == Some(turn_id)
                && event_call_id(&value).as_deref() == Some(call_id)
            {
                if super::super::user_input::question_tool(&value)
                    .is_some_and(super::super::is_async_question_tool)
                {
                    return Ok(super::super::rollout_async_questions(path)?
                        .iter()
                        .find(|call| call.turn_id == turn_id && call.call_id == call_id)
                        .map(|call| {
                            if call.pending().is_some() {
                                RolloutRequestUserInputState::Pending
                            } else {
                                RolloutRequestUserInputState::Resolved
                            }
                        })
                        .unwrap_or(RolloutRequestUserInputState::Missing));
                }
                found_call = true;
            }
            continue;
        }
        if request_user_input_resolution_matches(&value, turn_id, call_id) {
            return Ok(RolloutRequestUserInputState::Resolved);
        }
    }

    if !found_call {
        if let Some(call) = super::super::rollout_async_questions(path)
            .unwrap_or_default()
            .iter()
            .find(|call| call.turn_id == turn_id && call.call_id == call_id)
        {
            return Ok(if call.pending().is_some() {
                RolloutRequestUserInputState::Pending
            } else {
                RolloutRequestUserInputState::Resolved
            });
        }
    }
    Ok(if found_call {
        RolloutRequestUserInputState::Pending
    } else {
        RolloutRequestUserInputState::Missing
    })
}

pub(super) fn request_user_input_resolution_matches(
    value: &Value,
    turn_id: &str,
    call_id: &str,
) -> bool {
    let event_type = rollout_event_type(value);
    if (event_type == "task_complete" || is_turn_terminal_event(event_type))
        && event_turn_id(value).as_deref() == Some(turn_id)
    {
        return true;
    }

    let event_call = event_call_id(value);
    let call_matches = event_call.as_deref() == Some(call_id);
    let turn_matches = event_turn_id(value)
        .as_deref()
        .is_none_or(|event_turn| event_turn == turn_id);
    if !call_matches || !turn_matches {
        return false;
    }
    if is_user_input_answer(value) {
        return true;
    }
    let payload = value.get("payload").unwrap_or(value);
    payload.get("type").and_then(Value::as_str) == Some("function_call_output")
}

/// Archive/restore retain explicit native terminal evidence.
pub(crate) fn rollout_has_confirmed_inactive_state(path: &Path) -> Result<bool> {
    let scan = scan_rollout(path, 1)?;
    if scan.running || scan.active_turn_id.is_some() || scan.reply_needed {
        return Ok(false);
    }
    let text = fs::read_to_string(path)?;
    let mut terminal = false;
    let mut turn_context = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut value: Value = serde_json::from_str(line).context("线程记录损坏或尚未写入完整")?;
        normalize_canonical_turn(&mut value, &mut turn_context);
        let kind = rollout_event_type(&value);
        if is_turn_terminal_event(kind) || kind == "task_complete" {
            terminal = true;
        }
        if matches!(kind, "task_started" | "turn_started" | "turn/started") {
            terminal = false;
        }
    }
    Ok(terminal)
}

pub(crate) fn rollout_has_running_signal(path: &Path) -> Result<bool> {
    let scan = scan_rollout(path, 80)?;
    Ok(scan.running)
}
