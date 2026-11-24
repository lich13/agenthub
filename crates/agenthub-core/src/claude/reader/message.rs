use super::*;

impl Parser {
    pub(super) fn read_message(
        &mut self,
        record: &Value,
        position: usize,
        key: String,
        kind: &str,
        uuid: &str,
    ) {
        let Some(message) = record.get("message").filter(|m| m.is_object()) else {
            issue(&mut self.parsed, "消息格式未知，改名已禁用");
            return;
        };
        let Some(content) = message.get("content") else {
            issue(&mut self.parsed, "消息正文缺失，改名已禁用");
            return;
        };
        if !content.is_array() && !content.is_string() {
            issue(&mut self.parsed, "消息正文格式未知，改名已禁用");
            let mut e = event(
                key,
                record,
                "unknown",
                Some("未知消息正文".into()),
                &self.turn,
            );
            e.detail = Some(printable(content));
            self.parsed.events.push(Arc::new(e));
            return;
        }
        let single;
        let parts = if let Some(parts) = content.as_array() {
            parts.as_slice()
        } else {
            single = [serde_json::json!({"type":"text","text":content})];
            &single
        };
        let has_user_content = kind == "user"
            && parts
                .iter()
                .any(|p| matches!(field(p, "type"), "text" | "image" | "document"));
        if has_user_content {
            self.read_user_content(&key, record, parts, position, uuid);
        }
        if kind == "user" {
            if let Some(plan) = record
                .get("planContent")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                self.parsed.events.push(Arc::new(event(
                    format!("{key}:plan"),
                    record,
                    "plan",
                    Some(bounded(plan)),
                    &self.turn,
                )));
            }
        }
        let intermediate = kind == "assistant"
            && (field(message, "stop_reason") == "tool_use"
                || parts.iter().any(|part| field(part, "type") == "tool_use"));
        let api_error = record["isApiErrorMessage"] == true;
        if kind == "assistant"
            || parts.iter().any(|p| {
                field(p, "type") == "tool_result" && self.pending.contains(field(p, "tool_use_id"))
            })
        {
            self.execution_activity(record, position, api_error);
        }
        if kind == "assistant" {
            // Native transcripts can split one API message into several
            // records. Only an explicit shared message ID links their text.
            let message_id = field(message, "id");
            if message_id.is_empty()
                || self.last_assistant_message_id.as_deref() != Some(message_id)
            {
                self.last_assistant_events.clear();
            }
            self.last_assistant_message_id =
                (!message_id.is_empty() && message_id.len() <= 512).then(|| message_id.into());
            if intermediate {
                for &index in &self.last_assistant_events {
                    Arc::make_mut(&mut self.parsed.events[index]).assistant_phase =
                        Some(ClaudeAssistantPhase::Intermediate);
                }
            }
        }
        self.read_content_parts(record, position, &key, kind, parts, intermediate);
        if kind == "assistant" && !api_error {
            let text = text_content_only(content);
            if !text.trim().is_empty() {
                self.last_assistant_text = Some(text);
            }
            self.last_stop_reason = field(message, "stop_reason").into();
            if !terminal_stop_reason(&self.last_stop_reason) || !self.pending.is_empty() {
                self.parsed.turn_open = self.turn.is_some();
                self.parsed
                    .native_events
                    .retain(|event| Some(&event.turn_id) != self.turn.as_ref());
            }
        }
        let assistant_terminal = terminal_stop_reason(field(message, "stop_reason"));
        if kind == "assistant"
            && !api_error
            && assistant_terminal
            && self.pending.is_empty()
            && self.questions.is_empty()
        {
            self.mark_final_answer();
            finish(
                &mut self.parsed,
                &self.turn,
                position,
                record,
                "completion",
                self.last_assistant_text.clone().unwrap_or_default(),
            );
        }
    }

    fn read_user_content(
        &mut self,
        key: &str,
        record: &Value,
        parts: &[Value],
        position: usize,
        uuid: &str,
    ) {
        let (user, compact) = crate::claude::presentation::user_content(key, record, parts);
        let automatic = record["isMeta"] == true
            || compact
            || (user.text.trim().is_empty() && user.attachments.is_empty());
        if automatic {
            // A wake-up carries its own native prompt identity, so its
            // continuation turn is a fresh completion instead of a
            // duplicate of the prompt it continues.
            if !compact
                && record["isMeta"] != true
                && !uuid.is_empty()
                && !field(record, "promptId").is_empty()
                && record
                    .pointer("/origin/kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "human")
            {
                self.begin_turn(key, record, position, field(record, "promptId"));
            }
            let body = user
                .context_blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let (role, title) = if body.trim_start().starts_with("<local-command-") {
                ("local_command", "命令输出")
            } else if body.trim_start().starts_with("<task-notification>") {
                ("task_notification", "任务通知")
            } else {
                ("native_context", "上下文")
            };
            let mut e = event(
                key.to_owned(),
                record,
                if compact { "compaction" } else { "system" },
                Some(if compact { "压缩摘要" } else { title }.into()),
                &self.turn,
            );
            e.role = Some(role.into());
            e.detail = Some(bounded(&body));
            e.user_message = Some(user);
            if !body.trim().is_empty() {
                self.parsed.events.push(Arc::new(e));
            }
        } else if user
            .text
            .trim_start()
            .starts_with("[Request interrupted by user")
        {
            self.clear_final_answer();
            for prompt in self.parsed.prompts.values_mut() {
                if Some(&prompt.turn_id) == self.turn.as_ref() {
                    prompt.cancelled = true;
                }
            }
            self.parsed.turn_open = false;
            if let Some(ts) = millis(field(record, "timestamp")) {
                self.parsed.last_terminal_at =
                    Some(self.parsed.last_terminal_at.map_or(ts, |prev| prev.max(ts)));
            }
            self.parsed
                .native_events
                .retain(|event| Some(&event.turn_id) != self.turn.as_ref());
            self.turn = None;
            self.last_stop_reason.clear();
            self.last_assistant_events.clear();
            self.last_assistant_text = None;
            self.questions.clear();
            for call in &self.pending {
                if let Some(index) = self.calls.get(call) {
                    Arc::make_mut(&mut self.parsed.events[*index]).status =
                        Some("cancelled".into());
                }
            }
            self.pending.clear();
            let mut e = event(
                key.to_owned(),
                record,
                "user_message",
                Some(bounded(&user.text)),
                &self.turn,
            );
            e.user_message = Some(user);
            self.parsed.events.push(Arc::new(e));
        } else {
            // Keep earlier terminal events so an incremental polling cursor
            // can catch up after several turns between scans.
            self.begin_turn(key, record, position, field(record, "promptId"));
            if uuid.is_empty() {
                issue(&mut self.parsed, "原生消息身份缺失，通知和改名已禁用");
            }
            if self.first_user.is_none() {
                let title = crate::user_message::summary_text(&user.text);
                if !title.trim().is_empty() {
                    self.first_user = Some(title.chars().take(120).collect::<String>());
                }
            }
            let mut e = event(
                key.to_owned(),
                record,
                "user_message",
                Some(bounded(&user.text)),
                &self.turn,
            );
            e.user_message = Some(user);
            self.parsed.events.push(Arc::new(e));
        }
    }

    fn read_content_parts(
        &mut self,
        record: &Value,
        position: usize,
        key: &str,
        kind: &str,
        parts: &[Value],
        intermediate: bool,
    ) {
        for (index, part) in parts.iter().enumerate() {
            let mut e = event(format!("{key}:{index}"), record, "", None, &self.turn);
            match field(part, "type") {
                "text" if kind == "assistant" => {
                    e.kind = "assistant_message".into();
                    e.text = Some(bounded(field(part, "text")));
                    e.assistant_phase = intermediate.then_some(ClaudeAssistantPhase::Intermediate);
                    self.last_assistant_events.push(self.parsed.events.len());
                }
                "thinking" if kind == "assistant" => {
                    e.kind = "thinking".into();
                    e.text = Some(bounded(field(part, "thinking")));
                }
                "tool_reference" => {
                    let name = if field(part, "tool_name").is_empty() {
                        field(part, "name")
                    } else {
                        field(part, "tool_name")
                    };
                    e.kind = "tool_reference".into();
                    e.text = Some(if name.is_empty() {
                        "工具引用".into()
                    } else {
                        name.into()
                    });
                    e.role = (!name.is_empty()).then(|| name.into());
                    e.detail = Some(printable(part));
                    if name.is_empty() {
                        warning(&mut self.parsed, "工具引用缺少工具名称，已保留为警告");
                    }
                }
                "tool_search_tool_result" => {
                    e.kind = "tool_reference".into();
                    e.text = Some("工具引用结果".into());
                    e.detail = part.get("content").map(printable);
                }
                "redacted_thinking" => {
                    continue;
                }
                "tool_use" if kind == "assistant" => {
                    self.read_tool_use(record, position, part, &mut e)
                }
                "tool_result" => {
                    let call = field(part, "tool_use_id");
                    self.pending.remove(call);
                    self.questions.remove(call);
                    self.settle_permission(call);
                    let result = part
                        .get("content")
                        .map(|c| {
                            if c.is_array() {
                                content_text(c)
                            } else {
                                printable(c)
                            }
                        })
                        .unwrap_or_default();
                    let status = if part.get("is_error").and_then(Value::as_bool) == Some(true) {
                        "failed"
                    } else {
                        "completed"
                    };
                    if let Some(index) = self.calls.get(call) {
                        if !self.context.accepts_sidechain()
                            && self.parsed.events[*index].role.as_deref() == Some("Agent")
                        {
                            if let Some(result) = record.get("toolUseResult") {
                                let agent_id = field(result, "agentId");
                                let name = field(result, "description");
                                if !agent_id.is_empty() && !name.trim().is_empty() {
                                    self.parsed
                                        .agent_names
                                        .insert(agent_id.into(), bounded(name));
                                }
                            }
                        }
                        let prior = Arc::make_mut(&mut self.parsed.events[*index]);
                        prior.status = Some(status.into());
                        prior.result = Some(result);
                        continue;
                    }
                    e.kind = "tool_result".into();
                    e.text = Some("工具结果".into());
                    e.call_id = Some(call.into());
                    e.status = Some(status.into());
                    e.result = Some(result);
                }
                "text" | "image" | "document" if kind == "user" => continue,
                _ => {
                    e.kind = "unknown".into();
                    e.text = Some("未知内容块".into());
                    e.detail = Some(printable(part));
                    warning(&mut self.parsed, "存在未知内容块，已保留为警告");
                }
            }
            self.parsed.events.push(Arc::new(e));
        }
    }

    fn read_tool_use(
        &mut self,
        record: &Value,
        position: usize,
        part: &Value,
        e: &mut ClaudeHistoryEvent,
    ) {
        let call = field(part, "id");
        let name = field(part, "name");
        e.kind = "tool_call".into();
        e.text = Some(name.into());
        e.role = Some(name.into());
        e.call_id = Some(call.into());
        e.status = Some("in_progress".into());
        e.detail = part.get("input").map(printable);
        if call.is_empty() {
            issue(&mut self.parsed, "工具调用身份缺失，改名已禁用");
        }
        self.pending.insert(call.into());
        self.calls.insert(call.into(), self.parsed.events.len());
        if name == "AskUserQuestion" && !call.is_empty() {
            if let (Some(turn), Some(ts), Some(qs)) = (
                &self.turn,
                millis(field(record, "timestamp")),
                part.pointer("/input/questions").and_then(Value::as_array),
            ) {
                let body = qs
                    .iter()
                    .filter_map(|q| {
                        let question = field(q, "question");
                        if question.is_empty() {
                            return None;
                        }
                        let options = q
                            .get("options")
                            .and_then(Value::as_array)
                            .map(|opts| {
                                opts.iter()
                                    .filter_map(|o| {
                                        o.as_str()
                                            .or_else(|| o.get("label").and_then(Value::as_str))
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" / ")
                            })
                            .unwrap_or_default();
                        Some(format!("{question}\n{options}"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if !body.trim().is_empty() {
                    self.questions.insert(
                        call.into(),
                        crate::native_probe::NativeTurnEvent {
                            prompt_id: prompt_for_turn(&self.parsed, turn),
                            position: position as u64 + 1,
                            turn_id: format!("{turn}:{call}"),
                            kind: "reply_needed".into(),
                            body: crate::security::redact_output(&bounded(&body)),
                            timestamp_ms: ts,
                        },
                    );
                }
            }
        }
        if name == "ExitPlanMode" {
            if let Some(plan) = part
                .pointer("/input/plan")
                .and_then(Value::as_str)
                .filter(|p| !p.trim().is_empty())
            {
                e.kind = "plan".into();
                e.text = Some(bounded(plan));
                e.detail = None;
            }
        }
    }
}
