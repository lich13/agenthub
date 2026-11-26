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
        if let Some(payload) = metadata {
            scan.is_subagent |= is_subagent_session_meta(payload);
            scan.cwd = payload
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or(scan.cwd);
            scan.model = payload
                .get("model")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or(scan.model);
            if scan.title.is_none() {
                scan.title = rollout_metadata_title(payload);
            }
        }
        if value["type"] == "turn_context" {
            if let Some(payload) = value.get("payload") {
                if let Some(cwd) = payload
                    .get("cwd")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    scan.cwd = Some(cwd.to_string());
                }
                if let Some(model) = payload
                    .get("model")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    scan.model = Some(model.to_string());
                }
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
        let _ = update_pending_tool_calls(
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
static SCAN_CACHE: crate::jsonl_cache::JsonlCache<ScanState> =
    crate::jsonl_cache::JsonlCache::new(crate::resource_limits::CODEX_SCAN_CACHE_BYTES)
        .with_entry_limit(crate::resource_limits::CODEX_SCAN_ENTRIES);

pub(crate) fn evict_idle_scan_cache(max_idle: std::time::Duration) {
    SCAN_CACHE.evict_idle(max_idle);
}

pub(crate) fn scan_rollout(path: &Path, max_messages: usize) -> Result<RolloutScan> {
    scan_rollout_with_cache(&SCAN_CACHE, path, max_messages)
}

fn scan_rollout_with_cache(
    cache: &crate::jsonl_cache::JsonlCache<ScanState>,
    path: &Path,
    _max_messages: usize,
) -> Result<RolloutScan> {
    crate::codex::rollout_file::read_cached(
        cache,
        path,
        true,
        |state, line, _, _| {
            if let Ok(value) = serde_json::from_slice::<Value>(line) {
                state.push(value);
            }
            Ok(())
        },
        |state, _| Ok(state.summary()),
        |state| {
            let opt_cap = |s: &Option<String>| s.as_ref().map_or(0, String::capacity) as u64;
            let pending_action_bytes = match &state.pending_action {
                None => 0,
                Some(PendingAction::Plan { turn_id, item_id }) => {
                    opt_cap(turn_id) + opt_cap(item_id)
                }
                Some(PendingAction::Elicitation {
                    turn_id,
                    item_id,
                    call_id,
                    elicitation,
                }) => {
                    opt_cap(turn_id)
                        + opt_cap(item_id)
                        + opt_cap(call_id)
                        + opt_cap(&elicitation.turn_id)
                        + opt_cap(&elicitation.item_id)
                        + (elicitation.questions.capacity()
                            * std::mem::size_of::<crate::codex::UserInputQuestion>())
                            as u64
                        + elicitation
                            .questions
                            .iter()
                            .map(crate::codex::UserInputQuestion::heap_bytes)
                            .sum::<u64>()
                }
            };
            let plan_marker_bytes = state
                .current_plan_marker
                .as_ref()
                .map_or(0, |(t, i)| opt_cap(t) + opt_cap(i));
            std::mem::size_of::<ScanState>() as u64
                + 128
                + opt_cap(&state.scan.session_id)
                + opt_cap(&state.scan.latest_message)
                + opt_cap(&state.scan.last_message_at)
                + opt_cap(&state.scan.cwd)
                + opt_cap(&state.scan.model)
                + opt_cap(&state.scan.title)
                + opt_cap(&state.scan.first_user_message_title)
                + opt_cap(&state.scan.active_turn_id)
                + opt_cap(&state.scan.last_event_kind)
                + opt_cap(&state.last_task_status)
                + opt_cap(&state.turn_context)
                + pending_action_bytes
                + plan_marker_bytes
                + state.async_questions.heap_bytes()
                + state.pending_tool_turns.capacity() as u64 * 96
                + state.active_tasks.capacity() as u64 * 32
        },
    )
}

pub fn rollout_hook_stop_message_selection(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<RolloutMessageSelection>> {
    select_rollout_message(path, turn_id)
}

fn select_rollout_message(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<RolloutMessageSelection>> {
    let read = || {
        let reader = crate::codex::rollout_file::open_reader(path)?;
        select_rollout_message_inner(std::io::BufReader::new(reader), turn_id)
    };
    read().with_context(|| format!("read rollout {}", path.display()))
}

fn select_rollout_message_inner(
    mut input: impl std::io::BufRead,
    turn_id: Option<&str>,
) -> Result<Option<RolloutMessageSelection>> {
    let mut latest_assistant = None;
    let mut latest_unresolved_action: Option<PendingHookStopAction> = None;
    let mut latest_task_complete = None;
    let mut candidate_count = 0usize;
    let mut current_turn_id: Option<String> = None;
    let mut async_questions = super::super::user_input::AsyncQuestionTracker::default();
    let mut line = String::new();
    let mut line_number = 0;
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        line_number += 1;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(&line) else {
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
        return Ok(call.pending().map(|pending| RolloutMessageSelection {
            message: crate::services::probe::format_probe_pending_elicitation(&pending),
            source: "request_user_input".to_string(),
            strategy: "unresolved_async_question".to_string(),
            selected_turn_id: Some(call.turn_id.clone()),
            selected_line: Some(call.line),
            candidate_count: candidate_count + 1,
        }));
    }
    let selection = latest_unresolved_action
        .map(|pending| RolloutMessageSelection {
            message: pending.message,
            source: pending.source.to_string(),
            strategy: pending.source.to_string(),
            selected_turn_id: pending.turn_id,
            selected_line: Some(pending.line),
            candidate_count: 0,
        })
        .or(latest_task_complete)
        .or(latest_assistant);
    Ok(selection.map(|mut selection| {
        selection.candidate_count = candidate_count;
        selection
    }))
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
        rollout_completion_last_agent_message_selection(path, turn_id)?
            .map(|selection| selection.message),
    )
}

pub fn rollout_completion_last_agent_message_selection(
    path: &Path,
    turn_id: Option<&str>,
) -> Result<Option<RolloutMessageSelection>> {
    select_rollout_message(path, turn_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantFeedback {
    pub turn_id: String,
    pub body: String,
    pub completed_at_ms: i64,
}

#[derive(Default)]
struct PendingFeedbackScanState {
    current_turn: Option<String>,
    final_reply: Option<String>,
    completed: Option<(String, i64)>,
    pending: HashMap<String, Option<String>>,
}

impl PendingFeedbackScanState {
    fn feed_line(&mut self, raw_line: &[u8]) -> bool {
        let raw_line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if raw_line.iter().all(u8::is_ascii_whitespace) {
            return true;
        }
        let Ok(mut value) = serde_json::from_slice::<Value>(raw_line) else {
            return false;
        };
        let previous_turn = self.current_turn.clone();
        normalize_canonical_turn(&mut value, &mut self.current_turn);
        let kind = rollout_event_type(&value);
        if previous_turn != self.current_turn
            || matches!(kind, "task_started" | "turn_started" | "turn/started")
        {
            self.final_reply = None;
            self.completed = None;
            self.pending.clear();
        }
        if let Some(message) = parse_message_event(&value) {
            if message.role == "user" {
                self.final_reply = None;
                self.completed = None;
            } else if message.role == "assistant" && is_final_assistant_message(&value) {
                if self.final_reply.as_deref() != Some(&message.text) {
                    self.completed = None;
                }
                self.final_reply = Some(message.text);
            } else if message.role == "assistant" {
                self.completed = None;
            }
        }
        let pending_changed =
            update_pending_tool_calls(&value, &mut self.pending, self.current_turn.as_deref());
        let interactive_request = kind.contains("request_user_input")
            || kind.contains("elicitation")
            || contains_ascii_case_insensitive(raw_line, b"request_user_input")
            || contains_ascii_case_insensitive(raw_line, b"elicitation")
            || (contains_ascii_case_insensitive(raw_line, b"\\u")
                && value
                    .get("payload")
                    .map(|payload| payload.to_string())
                    .is_some_and(|payload| {
                        contains_ascii_case_insensitive(payload.as_bytes(), b"request_user_input")
                            || contains_ascii_case_insensitive(payload.as_bytes(), b"elicitation")
                    }));
        if pending_changed
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
            self.completed = None;
        }
        if matches!(kind, "turn_aborted" | "turn/aborted" | "turn_error") {
            self.final_reply = None;
            self.completed = None;
        }
        if matches!(kind, "task_complete" | "turn_completed" | "turn/completed") {
            let payload = value.get("payload").unwrap_or(&value);
            if let Some(message) = payload
                .get("last_agent_message")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                self.final_reply = Some(message.to_string());
            }
            if payload
                .get("status")
                .or_else(|| payload.pointer("/turn/status"))
                .and_then(Value::as_str)
                .is_some_and(|status| !matches!(status, "completed" | "complete" | "success"))
            {
                self.final_reply = None;
                self.completed = None;
                return true;
            }
            let turn = event_turn_id(&value).or_else(|| self.current_turn.clone());
            let timestamp = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<jiff::Timestamp>().ok())
                .map(|time| time.as_millisecond());
            self.completed = turn.zip(timestamp);
        }
        true
    }

    fn finish(self) -> Option<AssistantFeedback> {
        if !self.pending.is_empty() {
            return None;
        }
        let (turn_id, completed_at_ms) = self.completed?;
        let body = self
            .final_reply
            .as_deref()
            .and_then(super::super::assistant_feedback_request)?;
        Some(AssistantFeedback {
            turn_id,
            body,
            completed_at_ms,
        })
    }
}

/// Only a complete, latest terminal turn can wait for ordinary-language feedback.
/// Bounded tail reads also make polling large sessions inexpensive.
pub fn rollout_pending_feedback(path: &Path) -> Result<Option<AssistantFeedback>> {
    if crate::codex::rollout_file::is_compressed(path) {
        let metadata = fs::metadata(path)?;
        let mut reader = std::io::BufReader::new(crate::codex::rollout_file::open_reader(path)?);
        let mut state = PendingFeedbackScanState::default();
        let mut buf = Vec::new();
        let mut saw_any = false;
        loop {
            buf.clear();
            let read = std::io::BufRead::read_until(&mut reader, b'\n', &mut buf)?;
            if read == 0 {
                break;
            }
            saw_any = true;
            let Some(line) = buf.strip_suffix(b"\n") else {
                return Ok(None);
            };
            if !state.feed_line(line) {
                return Ok(None);
            }
        }
        if !saw_any {
            return Ok(None);
        }
        let after = fs::metadata(path)?;
        if metadata.len() != after.len() || metadata.modified()? != after.modified()? {
            return Ok(None);
        }
        return Ok(state.finish());
    }

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
    let mut state = PendingFeedbackScanState::default();
    for raw_line in bytes.split(|&b| b == b'\n') {
        if !state.feed_line(raw_line) {
            return Ok(None);
        }
    }
    let after = fs::metadata(path)?;
    if metadata.len() != after.len() || metadata.modified()? != after.modified()? {
        return Ok(None);
    }
    Ok(state.finish())
}

enum RequestUserInputLineStep {
    Continue,
    ReturnAsync,
    ReturnResolved,
}

fn step_request_user_input_line(
    line: &str,
    turn_id: &str,
    call_id: &str,
    found_call: &mut bool,
    turn_context: &mut Option<String>,
) -> RequestUserInputLineStep {
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return RequestUserInputLineStep::Continue;
    };
    normalize_canonical_turn(&mut value, turn_context);
    if !*found_call {
        if is_request_user_input(&value)
            && event_turn_id(&value).as_deref() == Some(turn_id)
            && event_call_id(&value).as_deref() == Some(call_id)
        {
            if super::super::user_input::question_tool(&value)
                .is_some_and(super::super::is_async_question_tool)
            {
                return RequestUserInputLineStep::ReturnAsync;
            }
            *found_call = true;
        }
        return RequestUserInputLineStep::Continue;
    }
    if request_user_input_resolution_matches(&value, turn_id, call_id) {
        return RequestUserInputLineStep::ReturnResolved;
    }
    RequestUserInputLineStep::Continue
}

fn resolve_async_question_call_state(
    path: &Path,
    turn_id: &str,
    call_id: &str,
) -> Result<RolloutRequestUserInputState> {
    Ok(super::super::rollout_async_questions(path)?
        .iter()
        .find(|call| call.turn_id == turn_id && call.call_id == call_id)
        .map(|call| {
            if call.pending().is_some() {
                RolloutRequestUserInputState::Pending
            } else {
                RolloutRequestUserInputState::Resolved
            }
        })
        .unwrap_or(RolloutRequestUserInputState::Missing))
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

    let mut found_call = false;
    let mut turn_context = None;
    if crate::codex::rollout_file::is_compressed(path) {
        let Ok(reader) = crate::codex::rollout_file::open_reader(path) else {
            return Ok(RolloutRequestUserInputState::Missing);
        };
        let mut reader = std::io::BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::BufRead::read_line(&mut reader, &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => return Ok(RolloutRequestUserInputState::Missing),
            }
            match step_request_user_input_line(
                &line,
                turn_id,
                call_id,
                &mut found_call,
                &mut turn_context,
            ) {
                RequestUserInputLineStep::Continue => {}
                RequestUserInputLineStep::ReturnAsync => {
                    return resolve_async_question_call_state(path, turn_id, call_id);
                }
                RequestUserInputLineStep::ReturnResolved => {
                    return Ok(RolloutRequestUserInputState::Resolved);
                }
            }
        }
    } else {
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

        for line in String::from_utf8_lossy(&tail).lines() {
            match step_request_user_input_line(
                line,
                turn_id,
                call_id,
                &mut found_call,
                &mut turn_context,
            ) {
                RequestUserInputLineStep::Continue => {}
                RequestUserInputLineStep::ReturnAsync => {
                    return resolve_async_question_call_state(path, turn_id, call_id);
                }
                RequestUserInputLineStep::ReturnResolved => {
                    return Ok(RolloutRequestUserInputState::Resolved);
                }
            }
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

fn contains_ascii_case_insensitive(haystack: &[u8], needle_lower: &[u8]) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.len() < needle_lower.len() {
        return false;
    }
    let first = needle_lower[0];
    let first_upper = first.to_ascii_uppercase();
    let max_start = haystack.len() - needle_lower.len();
    for i in 0..=max_start {
        let b = haystack[i];
        if (b == first || b == first_upper)
            && haystack[i + 1..i + needle_lower.len()].eq_ignore_ascii_case(&needle_lower[1..])
        {
            return true;
        }
    }
    false
}

/// Archive/restore retain explicit native terminal evidence.
pub(crate) fn rollout_has_confirmed_inactive_state(path: &Path) -> Result<bool> {
    let scan = scan_rollout(path, 1)?;
    if scan.running || scan.active_turn_id.is_some() || scan.reply_needed {
        return Ok(false);
    }
    let mut reader = std::io::BufReader::new(crate::codex::rollout_file::open_reader(path)?);
    let mut line = String::new();
    let mut terminal = false;
    let mut turn_context = None;
    loop {
        line.clear();
        if std::io::BufRead::read_line(&mut reader, &mut line)? == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let mut value: Value = serde_json::from_str(&line).context("线程记录损坏或尚未写入完整")?;
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

#[cfg(test)]
mod streaming_tests {
    use super::*;

    #[test]
    fn streaming_selection_preserves_lines_scope_and_propagates_late_read_errors() {
        let rows = [
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"old"}}).to_string(),
            serde_json::json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"old","last_agent_message":"old answer"}}).to_string(),
            String::from("invalid record"), String::new(),
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"new"}}).to_string(),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"new answer"}]}}).to_string(),
        ].join("\r\n");
        let selection = select_rollout_message_inner(rows.as_bytes(), Some("new"))
            .unwrap()
            .unwrap();
        assert_eq!(selection.message, "new answer");
        assert_eq!(selection.selected_line, Some(6));
        assert_eq!(selection.candidate_count, 1);
        assert_eq!(
            select_rollout_message_inner(rows.as_bytes(), Some("old"))
                .unwrap()
                .unwrap()
                .message,
            "old answer"
        );
        let invalid = [rows.as_bytes(), b"\n\xff"].concat();
        assert!(select_rollout_message_inner(invalid.as_slice(), None).is_err());
        struct FailedRead;
        impl std::io::Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("fixture read failed"))
            }
        }
        let broken = std::io::Read::chain(rows.as_bytes(), FailedRead);
        assert!(select_rollout_message_inner(std::io::BufReader::new(broken), None).is_err());
    }

    #[test]
    fn idle_cleanup_releases_plain_and_compressed_summary_cache() {
        let root =
            std::env::temp_dir().join(format!("agenthub-scan-idle-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let plain = root.join("history.jsonl");
        let compressed = root.join("history.jsonl.zst");
        let bytes = b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn\"}}\n";
        fs::write(&plain, bytes).unwrap();
        fs::write(
            &compressed,
            zstd::stream::encode_all(&bytes[..], 1).unwrap(),
        )
        .unwrap();
        for path in [&plain, &compressed] {
            assert!(scan_rollout(path, 80).unwrap().running);
            assert!(SCAN_CACHE.cached_weight(path) > 0);
        }
        crate::codex::evict_idle_detail_cache(std::time::Duration::ZERO);
        for path in [&plain, &compressed] {
            assert_eq!(SCAN_CACHE.cached_weight(path), 0);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn confirmed_inactive_state_rejects_non_json_unicode_whitespace_prefix() {
        let root =
            std::env::temp_dir().join(format!("agenthub-inactive-nbsp-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let valid = root.join("valid.jsonl");
        let nbsp_corrupt = root.join("nbsp-corrupt.jsonl");
        let bom_corrupt = root.join("bom-corrupt.jsonl");

        fs::write(
            &valid,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\"}}\n",
                "   \t\r\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-1\",\"status\":\"completed\"}}\n"
            ),
        )
        .unwrap();
        assert!(rollout_has_confirmed_inactive_state(&valid).unwrap());

        let running_only = root.join("running.jsonl");
        fs::write(
            &running_only,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\n"
            ),
        )
        .unwrap();
        assert!(!rollout_has_confirmed_inactive_state(&running_only).unwrap());

        // Without an open task_started (or after a completed prior turn), scan_rollout reports
        // running=false; serde_json::from_str(&line) must still reject leading U+00A0 / U+FEFF
        // on a corrupt task_complete line rather than stripping non-JSON Unicode whitespace.
        fs::write(
            &nbsp_corrupt,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\"}}\n",
                "\u{00a0}{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-1\",\"status\":\"completed\"}}\n"
            ),
        )
        .unwrap();
        let err = rollout_has_confirmed_inactive_state(&nbsp_corrupt).unwrap_err();
        assert!(
            err.to_string().contains("线程记录损坏或尚未写入完整"),
            "unexpected error: {err:#}"
        );

        fs::write(
            &bom_corrupt,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\"}}\n",
                "\u{feff}{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-1\",\"status\":\"completed\"}}\n"
            ),
        )
        .unwrap();
        assert!(rollout_has_confirmed_inactive_state(&bom_corrupt).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_cache_counts_nested_async_question_heap_and_evicts_over_budget() {
        let root = std::env::temp_dir().join(format!(
            "agenthub-scan-question-budget-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();

        let make_question_rollout = |path: &Path, seed: &str, payload_kb: usize| {
            let large_body = "问".repeat((payload_kb * 1024) / 3);
            let large_desc = "选".repeat((payload_kb * 512) / 3);
            let turn_id = format!("turn-{seed}-{}", "t".repeat(128));
            let call_id = format!("call-{seed}-{}", "c".repeat(128));
            let answer_envelope = format!(
                "<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",
                serde_json::json!([{
                    "questionItemId": serde_json::json!(["request_user_input_async", call_id, 0]).to_string(),
                    "question": "first",
                    "answer": "选A"
                }])
            );
            let rows = [
                serde_json::json!({"type":"session_meta","payload":{"id":format!("sess-{seed}")}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":turn_id}}),
                serde_json::json!({
                    "type":"response_item",
                    "turn_id":turn_id,
                    "payload":{
                        "type":"function_call",
                        "name":"request_user_input_async",
                        "call_id":call_id,
                        "arguments":serde_json::json!({
                            "questions":[
                                {
                                    "id":format!("qid-{seed}-1"),
                                    "header":format!("Header-{seed}"),
                                    "question":large_body,
                                    "options":[
                                        {"label":"选A","description":large_desc},
                                        {"label":"选B","description":"备用选项"}
                                    ]
                                },
                                {
                                    "id":format!("qid-{seed}-2"),
                                    "header":"第二题",
                                    "question":"仍待回复的问题？",
                                    "options":[{"label":"继续"}]
                                }
                            ]
                        }).to_string()
                    }
                }),
                serde_json::json!({
                    "type":"response_item",
                    "turn_id":turn_id,
                    "payload":{"type":"message","role":"user","content":[{"type":"input_text","text":answer_envelope}]}
                }),
            ];
            let content = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
            fs::write(path, content).unwrap();
        };

        // Budget = 32 KiB: each rollout has only 1 async question call (~18 KiB of nested strings),
        // which under the old fixed 256B/call estimate would look like ~1 KiB and never evict.
        let cache =
            crate::jsonl_cache::JsonlCache::<ScanState>::new(32 * 1024).with_entry_limit(64);
        let first = root.join("first.jsonl");
        let second = root.join("second.jsonl");
        let oversize = root.join("oversize.jsonl");
        make_question_rollout(&first, "a", 12);
        make_question_rollout(&second, "b", 12);
        make_question_rollout(&oversize, "huge", 40);

        let first_scan = scan_rollout_with_cache(&cache, &first, 80).unwrap();
        assert!(first_scan.reply_needed);
        let first_weight = cache.cached_weight(&first);
        assert!(
            first_weight > 16 * 1024,
            "nested AsyncQuestionTracker heap must be counted in SCAN_CACHE weight, got {first_weight}"
        );

        let second_scan = scan_rollout_with_cache(&cache, &second, 80).unwrap();
        assert!(second_scan.reply_needed);
        assert_eq!(
            cache.cached_weight(&first),
            0,
            "older rollout must be LRU-evicted once nested question strings exceed budget"
        );
        assert!(cache.cached_weight(&second) > 16 * 1024);

        let huge_scan = scan_rollout_with_cache(&cache, &oversize, 80).unwrap();
        assert!(huge_scan.reply_needed);
        assert_eq!(
            cache.cached_weight(&oversize),
            0,
            "single oversize question rollout must still read accurately without staying cached"
        );

        fs::remove_dir_all(root).unwrap();
    }
}
