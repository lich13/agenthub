//! Mechanical extraction of the existing Codex message builder path.
use super::*;

#[derive(Default, Clone)]
pub(super) struct MessageBlockBuilder {
    pub(super) blocks: Vec<Arc<MessageBlock>>,
    pub(super) block_positions: HashMap<String, usize>,
    pub(super) pending_tools: HashMap<String, PendingToolCall>,
    pub(super) suppressed_call_ids: HashSet<String>,
    pub(super) async_questions: super::super::user_input::AsyncQuestionTracker,
    pub(super) current_plan_marker: Option<(Option<String>, Option<String>)>,
    pub(super) plan_block_indexes: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub(super) struct PendingToolCall {
    pub(super) subagent: Option<super::super::subagents::SubagentActivity>,
    pub(super) raw_index: usize,
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) status: Option<String>,
    pub(super) tool_name: Option<String>,
    pub(super) call_id: Option<String>,
    pub(super) turn_id: Option<String>,
    pub(super) item_id: Option<String>,
    pub(super) created_at: Option<String>,
    pub(super) input: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) payload: Option<Value>,
}

impl MessageBlockBuilder {
    pub(super) fn push_event(&mut self, value: &Value, raw_index: usize) {
        let previous_len = self.blocks.len();
        self.async_questions.push(value, raw_index);
        self.push_event_inner(value, raw_index);
        for block in &self.blocks[previous_len..] {
            self.block_positions
                .entry(block.id.clone())
                .or_insert(raw_index);
        }
    }

    fn push_event_inner(&mut self, value: &Value, raw_index: usize) {
        if let Some(block) = super::super::subagents::native_activity_block(value) {
            self.blocks.push(Arc::new(block));
            return;
        }
        if is_internal_agent_message(value) {
            return;
        }
        let event_type = rollout_event_type(value);
        if event_type == "task_complete" || is_turn_terminal_event(event_type) {
            self.clear_pending_tools_for_turn(event_turn_id(value).as_deref());
        }
        if event_type == "task_complete" && task_complete_has_last_agent_message(value) {
            self.resolve_pending_plans_for_external_progress(event_turn_id(value).as_deref());
        }

        if is_plan_item_completed(value) {
            self.current_plan_marker = Some((event_turn_id(value), event_item_id(value)));
            return;
        }

        if let Some(block) = plan_delta_block(value, raw_index) {
            self.push_plan_block(block);
            return;
        }

        if let Some(block) = user_input_answer_block(value, raw_index) {
            self.push_answer_block(block);
            return;
        }

        if let Some(block) = pending_elicitation_block(value, raw_index) {
            if let Some(call_id) = &block.call_id {
                self.suppressed_call_ids.insert(call_id.clone());
            }
            self.blocks.push(Arc::new(block));
            return;
        }

        let Some(payload) = event_payload(value) else {
            return;
        };
        let payload_type = event_payload_type(value, payload);

        if is_tool_call_kind(payload_type) {
            if let Some(call_id) = payload_call_id(payload) {
                self.pending_tools.insert(
                    call_id.clone(),
                    PendingToolCall::from_payload(value, payload, payload_type, raw_index, call_id),
                );
            }
            return;
        }

        if is_tool_output_kind(payload_type) {
            let call_id = payload_call_id(payload);
            if call_id.as_ref().is_some_and(|id| {
                self.async_questions
                    .calls
                    .iter()
                    .any(|call| &call.call_id == id)
            }) {
                return;
            }
            if call_id
                .as_ref()
                .is_some_and(|id| self.suppressed_call_ids.remove(id))
            {
                self.push_answer_block(user_input_output_block(value, payload, raw_index, call_id));
                return;
            }
            let pending = call_id
                .as_ref()
                .and_then(|id| self.pending_tools.remove(id));
            if let Some(call) = &pending {
                self.block_positions.insert(call.id.clone(), call.raw_index);
            }
            self.blocks.push(Arc::new(tool_output_block(
                value,
                payload,
                payload_type,
                raw_index,
                pending,
            )));
            return;
        }

        if let Some(mut block) = parse_message_block(value, raw_index) {
            if block.user_message.as_ref().is_some_and(|message| {
                self.blocks.iter().any(|existing| {
                    existing
                        .user_message
                        .as_ref()
                        .is_some_and(|previous| previous.id == message.id)
                })
            }) {
                return;
            }
            if block.role == "assistant"
                && block.text.as_deref().is_some_and(contains_proposed_plan)
            {
                block.kind = "plan".to_string();
                block.status = block.status.or_else(|| Some("pending".to_string()));
                block.display_kind = Some("plan".to_string());
                block.plan_status = block.status.clone();
                block.resolved = Some(false);
                if let Some((turn_id, item_id)) =
                    plan_marker_for_event(&self.current_plan_marker, value)
                {
                    block.turn_id = block.turn_id.or(turn_id);
                    block.item_id = block.item_id.or(item_id);
                }
                block.group_id = Some(plan_group_key(&block, raw_index));
                self.current_plan_marker = None;
                self.push_plan_block(block);
                return;
            }
            if matches!(block.role.as_str(), "user" | "assistant" | "tool") {
                self.resolve_pending_plans();
            }
            self.blocks.push(Arc::new(block));
        }
    }

    fn push_plan_block(&mut self, mut block: MessageBlock) {
        let key = plan_group_key(&block, self.blocks.len());
        block.group_id = Some(key.clone());
        block.display_kind = Some("plan".to_string());
        block.plan_status = block.plan_status.or_else(|| block.status.clone());
        if let Some(index) = self.plan_block_indexes.get(&key).copied() {
            let existing = Arc::make_mut(&mut self.blocks[index]);
            if block.status.as_deref() == Some("streaming") {
                let next_text = block.text.unwrap_or_default();
                if !next_text.trim().is_empty() {
                    let current = existing.text.get_or_insert_with(String::new);
                    current.push_str(&next_text);
                }
                existing.status = Some("streaming".to_string());
                existing.plan_status = Some("streaming".to_string());
                existing.resolved = Some(false);
            } else {
                if block
                    .text
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty())
                {
                    existing.text = block.text;
                }
                existing.status = block.status.or_else(|| Some("pending".to_string()));
                existing.plan_status = block.plan_status.or_else(|| existing.status.clone());
                existing.resolved = block.resolved.or(Some(false));
                existing.item_id = block.item_id.or_else(|| existing.item_id.clone());
                existing.turn_id = block.turn_id.or_else(|| existing.turn_id.clone());
                existing.created_at = block.created_at.or_else(|| existing.created_at.clone());
                existing.kind = "plan".to_string();
            }
            return;
        }
        self.plan_block_indexes.insert(key, self.blocks.len());
        self.blocks.push(Arc::new(block));
    }

    fn resolve_pending_plans(&mut self) {
        for block in &mut self.blocks {
            if block.kind == "plan" && block.resolved != Some(true) {
                let block = Arc::make_mut(block);
                block.status = Some("completed".to_string());
                block.plan_status = Some("completed".to_string());
                block.resolved = Some(true);
            }
        }
    }

    fn resolve_pending_plans_for_external_progress(&mut self, progress_turn_id: Option<&str>) {
        let Some(progress_turn_id) = progress_turn_id else {
            return;
        };
        for block in &mut self.blocks {
            if block.kind == "plan"
                && block.resolved != Some(true)
                && block
                    .turn_id
                    .as_deref()
                    .is_some_and(|turn_id| turn_id != progress_turn_id)
            {
                let block = Arc::make_mut(block);
                block.status = Some("completed".to_string());
                block.plan_status = Some("completed".to_string());
                block.resolved = Some(true);
            }
        }
    }

    fn push_answer_block(&mut self, block: MessageBlock) {
        let key = block
            .call_id
            .as_deref()
            .or(block.item_id.as_deref())
            .map(str::to_string);
        if let Some(key) = key {
            if let Some(index) = self.blocks.iter().rposition(|candidate| {
                candidate.call_id.as_deref() == Some(key.as_str())
                    || candidate.item_id.as_deref() == Some(key.as_str())
            }) {
                let pending = Arc::make_mut(&mut self.blocks[index]);
                if pending.kind == "request_user_input" {
                    pending.kind = "request_user_input_result".to_string();
                    pending.status = Some("completed".to_string());
                    pending.display_kind = Some("question_result".to_string());
                    pending.resolved = Some(true);
                    pending.answers = block.answers.clone();
                    pending.summary = Some(answer_summary(&block.answers));
                    pending.text = block.text;
                    return;
                }
            }
        }
        self.blocks.push(Arc::new(block));
    }

    fn clear_pending_tools_for_turn(&mut self, turn_id: Option<&str>) {
        match turn_id {
            Some(turn_id) => self.pending_tools.retain(|_, call| {
                call.turn_id
                    .as_deref()
                    .is_some_and(|value| value != turn_id)
            }),
            None => self.pending_tools.clear(),
        }
    }

    pub(super) fn finish(self) -> Vec<MessageBlock> {
        compact_chat_history(self.finish_uncompacted(), CHAT_HISTORY_LIMIT)
            .into_iter()
            .map(Arc::unwrap_or_clone)
            .collect()
    }

    pub(super) fn finish_uncompacted(mut self) -> Vec<Arc<MessageBlock>> {
        for block in &mut self.blocks {
            if let Some(call) = self.async_questions.calls.iter().find(|call| {
                block.call_id.as_deref() == Some(&call.call_id)
                    && block.turn_id.as_deref() == Some(&call.turn_id)
            }) {
                let block = Arc::make_mut(block);
                let pending = call.pending();
                block.resolved = Some(pending.is_none());
                block.status = Some(
                    if pending.is_some() {
                        "pending"
                    } else {
                        "completed"
                    }
                    .to_string(),
                );
                if let Some(pending) = pending {
                    block.questions = pending.questions;
                }
            }
        }
        let mut pending = self.pending_tools.into_values().collect::<Vec<_>>();
        pending.sort_by_key(|call| call.raw_index);
        for call in pending {
            self.block_positions.insert(call.id.clone(), call.raw_index);
            self.blocks.push(Arc::new(call.into_running_block()));
        }
        self.blocks.sort_by_key(|block| {
            self.block_positions
                .get(&block.id)
                .copied()
                .unwrap_or(usize::MAX)
        });
        super::super::subagents::merge_creation_events(&mut self.blocks);
        self.blocks
    }
}

pub(super) fn compact_chat_history(
    blocks: Vec<Arc<MessageBlock>>,
    max_chat_messages: usize,
) -> Vec<Arc<MessageBlock>> {
    let chat_indexes = blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| is_chat_history_block(block).then_some(index))
        .collect::<Vec<_>>();
    if chat_indexes.len() <= max_chat_messages {
        return blocks;
    }

    let hidden = chat_indexes.len().saturating_sub(max_chat_messages);
    let keep_start = chat_indexes.len().saturating_sub(max_chat_messages);
    let keep_chat = chat_indexes[keep_start..]
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let hide_chat = chat_indexes[..keep_start]
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let collapsed = MessageBlock {
        subagent: None,
        user_message: None,
        id: "chat-history-collapsed".to_string(),
        role: "tool".to_string(),
        kind: "chat_history_collapsed".to_string(),
        display_kind: Some("history_group".to_string()),
        status: Some("completed".to_string()),
        text: Some(format!("{hidden} 条历史对话已折叠")),
        summary: Some(format!("{hidden} 条历史对话已折叠")),
        input: None,
        truncated: Some(false),
        resolved: Some(true),
        answers: Vec::new(),
        plan_status: None,
        group_id: Some("chat_history".to_string()),
        tool_name: Some("chat_history".to_string()),
        call_id: None,
        turn_id: None,
        item_id: None,
        created_at: None,
        questions: Vec::new(),
        payload: None,
    };

    let mut compacted = Vec::with_capacity(blocks.len().saturating_sub(hidden).saturating_add(1));
    let mut inserted = false;
    let mut segment: Option<usize> = None;
    let mut segment_count = 0;
    for (index, block) in blocks.into_iter().enumerate() {
        if hide_chat.contains(&index) && !keep_chat.contains(&index) {
            let position = *segment.get_or_insert_with(|| {
                let mut marker = collapsed.clone();
                if inserted {
                    marker.id = format!("chat-history-collapsed:{}", block.id);
                }
                compacted.push(Arc::new(marker));
                inserted = true;
                segment_count = 0;
                compacted.len() - 1
            });
            segment_count += 1;
            let summary = Some(format!("{segment_count} 条历史对话已折叠"));
            let marker = Arc::make_mut(&mut compacted[position]);
            marker.text = summary.clone();
            marker.summary = summary;
            continue;
        }
        segment = None;
        compacted.push(block);
    }
    compacted
}

fn is_chat_history_block(block: &MessageBlock) -> bool {
    if !matches!(block.role.as_str(), "user" | "assistant") {
        return false;
    }
    if !block.questions.is_empty() {
        return false;
    }
    if block.status.as_deref().is_some_and(is_running_status) {
        return false;
    }
    let kind = block.kind.trim().to_ascii_lowercase();
    !matches!(
        kind.as_str(),
        "reasoning"
            | "agent_reasoning"
            | "session_meta"
            | "request_user_input"
            | "requestuserinput"
    ) && !kind.contains("plan")
        && !kind.contains("approval")
        && !kind.contains("tool")
        && !kind.contains("function_call")
        && !kind.contains("command")
}

impl PendingToolCall {
    fn from_payload(
        value: &Value,
        payload: &Value,
        payload_type: &str,
        raw_index: usize,
        call_id: String,
    ) -> Self {
        let input = tool_input_text(payload);
        let summary = input.as_deref().map(tool_summary);
        let name = tool_name(payload, payload_type);
        let subagent = name
            .as_deref()
            .and_then(|name| super::super::subagents::creation_activity(name, payload));
        Self {
            subagent,
            raw_index,
            id: block_id(value, raw_index),
            kind: normalize_kind(payload_type).to_string(),
            status: payload_status(payload),
            tool_name: name,
            call_id: Some(call_id),
            turn_id: event_turn_id(value),
            item_id: payload_item_id(value, payload),
            created_at: event_time(value),
            input,
            summary,
            payload: None,
        }
    }

    fn into_running_block(self) -> MessageBlock {
        MessageBlock {
            subagent: self.subagent,
            user_message: None,
            id: self.id,
            role: "tool".to_string(),
            kind: self.kind,
            display_kind: Some("tool".to_string()),
            status: self.status.or_else(|| Some("running".to_string())),
            text: None,
            summary: self.summary,
            input: self.input,
            truncated: Some(false),
            resolved: Some(false),
            answers: Vec::new(),
            plan_status: None,
            group_id: self.call_id.clone(),
            tool_name: self.tool_name,
            call_id: self.call_id,
            turn_id: self.turn_id,
            item_id: self.item_id,
            created_at: self.created_at,
            questions: Vec::new(),
            payload: self.payload,
        }
    }
}
