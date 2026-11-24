use super::*;

fn map_cortex_status(status: &str) -> &'static str {
    match status {
        "CORTEX_STEP_STATUS_RUNNING"
        | "CORTEX_STEP_STATUS_WAITING"
        | "CORTEX_STEP_STATUS_PENDING"
        | "CORTEX_STEP_STATUS_GENERATING"
        | "CORTEX_STEP_STATUS_QUEUED" => "running",
        "CORTEX_STEP_STATUS_ERROR"
        | "CORTEX_STEP_STATUS_CANCELED"
        | "CORTEX_STEP_STATUS_INTERRUPTED"
        | "CORTEX_STEP_STATUS_INVALID"
        | "CORTEX_STEP_STATUS_HALTED" => "failed",
        _ => "completed",
    }
}

fn extract_user_input_item_text(item: &Value) -> Option<String> {
    if let Some(text) = item["text"].as_str().filter(|t| !t.is_empty()) {
        return Some(text.to_owned());
    }
    let scope = &item["item"];
    if scope.is_null() {
        return None;
    }
    if let Some(raw) = scope.as_str().filter(|s| !s.trim().is_empty()) {
        return Some(format!("@{}", decode_file_uri(raw)));
    }
    if let Some(obj) = scope.as_object() {
        for key in ["label", "title", "name", "absolutePathUri", "uri", "path"] {
            if let Some(val) = obj
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                return Some(format!("@{}", decode_file_uri(val)));
            }
        }
        for nested in obj.values() {
            if let Some(inner) = nested.as_object() {
                for key in ["label", "title", "name", "absolutePathUri", "uri", "path"] {
                    if let Some(val) = inner
                        .get(key)
                        .and_then(Value::as_str)
                        .filter(|s| !s.trim().is_empty())
                    {
                        return Some(format!("@{}", decode_file_uri(val)));
                    }
                }
            }
        }
    }
    None
}

fn format_artifact_comment(entry: &Value) -> Option<String> {
    let basename = entry["artifactUri"]
        .as_str()
        .map(decode_file_uri)
        .and_then(|p| p.rsplit('/').find(|seg| !seg.is_empty()).map(str::to_owned))
        .unwrap_or_default();
    let comment = [
        &entry["comment"],
        &entry["commentText"],
        &entry["text"],
        &entry["fullFile"]["comment"],
        &entry["lineRange"]["comment"],
        &entry["selection"]["comment"],
    ]
    .into_iter()
    .find_map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()));
    let approval = entry["approvalStatus"].as_str().unwrap_or("");

    match (approval, !basename.is_empty(), comment) {
        ("ARTIFACT_APPROVAL_STATUS_APPROVED", true, Some(c)) => {
            Some(format!("批准文档（{basename}）：{c}"))
        }
        ("ARTIFACT_APPROVAL_STATUS_APPROVED", true, None) => Some(format!("批准文档：{basename}")),
        ("ARTIFACT_APPROVAL_STATUS_APPROVED", false, Some(c)) => Some(format!("批准文档：{c}")),
        ("ARTIFACT_APPROVAL_STATUS_APPROVED", false, None) => Some("批准文档".into()),
        (
            "ARTIFACT_APPROVAL_STATUS_REJECTED" | "ARTIFACT_APPROVAL_STATUS_CHANGES_REQUESTED",
            true,
            Some(c),
        ) => Some(format!("批注文档（{basename}）：{c}")),
        (
            "ARTIFACT_APPROVAL_STATUS_REJECTED" | "ARTIFACT_APPROVAL_STATUS_CHANGES_REQUESTED",
            true,
            None,
        ) => Some(format!("退回文档：{basename}")),
        (
            "ARTIFACT_APPROVAL_STATUS_REJECTED" | "ARTIFACT_APPROVAL_STATUS_CHANGES_REQUESTED",
            false,
            Some(c),
        ) => Some(format!("退回文档：{c}")),
        (
            "ARTIFACT_APPROVAL_STATUS_REJECTED" | "ARTIFACT_APPROVAL_STATUS_CHANGES_REQUESTED",
            false,
            None,
        ) => Some("退回文档".into()),
        (_, true, Some(c)) => Some(format!("批注文档（{basename}）：{c}")),
        (_, false, Some(c)) => Some(c.to_owned()),
        (_, true, None) => Some(format!("审阅文档：{basename}")),
        (_, false, None) => None,
    }
}

fn make_history_event(
    id: String,
    timestamp: Option<String>,
    kind: &str,
    role: &str,
    text: Option<String>,
    turn_id: Option<String>,
    status: Option<&str>,
) -> ProviderHistoryEvent {
    ProviderHistoryEvent {
        id,
        timestamp,
        kind: kind.into(),
        role: Some(role.into()),
        text,
        call_id: None,
        turn_id,
        status: status.map(str::to_owned),
        detail: None,
        result: None,
        user_message: None,
        subagent: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn make_tool_event(
    id: String,
    timestamp: Option<String>,
    kind: &str,
    name: &str,
    call_id: Option<String>,
    turn_id: Option<String>,
    status: Option<&str>,
    detail: Option<String>,
    result: Option<String>,
) -> ProviderHistoryEvent {
    let mut ev = make_history_event(
        id,
        timestamp,
        kind,
        name,
        Some(name.to_owned()),
        turn_id,
        status,
    );
    ev.call_id = call_id;
    ev.detail = detail;
    ev.result = result;
    ev
}

fn is_subagent_coordination_tool(name: &str) -> bool {
    matches!(
        name,
        "invoke_subagent" | "send_message" | "manage_subagents"
    )
}

pub(super) fn extract_live_step_events(id: &str, steps: &[Value]) -> Vec<ProviderHistoryEvent> {
    let mut has_later_planner = vec![false; steps.len()];
    let mut has_later_settled_step = vec![false; steps.len()];
    let mut resolved_call_ids = BTreeSet::new();
    let mut seen_planner_in_turn = false;
    let mut seen_settled_after = false;
    for (idx, step) in steps.iter().enumerate().rev() {
        let source = &step["metadata"]["sourceTrajectoryStepInfo"];
        if source["cascadeId"]
            .as_str()
            .is_some_and(|orig| !orig.is_empty() && orig != id)
        {
            continue;
        }
        has_later_settled_step[idx] = seen_settled_after;
        let step_type = step["type"].as_str().unwrap_or("");
        let step_status = map_cortex_status(step["status"].as_str().unwrap_or(""));
        if step_status != "running"
            && !matches!(
                step_type,
                "CORTEX_STEP_TYPE_EPHEMERAL_MESSAGE"
                    | "CORTEX_STEP_TYPE_CHECKPOINT"
                    | "CORTEX_STEP_TYPE_SYSTEM_MESSAGE"
                    | ""
            )
        {
            seen_settled_after = true;
        }
        if !matches!(
            step_type,
            "CORTEX_STEP_TYPE_USER_INPUT"
                | "CORTEX_STEP_TYPE_PLANNER_RESPONSE"
                | "CORTEX_STEP_TYPE_FINISH"
                | "CORTEX_STEP_TYPE_EPHEMERAL_MESSAGE"
                | "CORTEX_STEP_TYPE_CHECKPOINT"
                | "CORTEX_STEP_TYPE_SYSTEM_MESSAGE"
                | ""
        ) {
            if let Some(cid) = step["metadata"]["toolCall"]["id"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                resolved_call_ids.insert(cid.to_owned());
            }
        }
        match step_type {
            "CORTEX_STEP_TYPE_USER_INPUT" => seen_planner_in_turn = false,
            "CORTEX_STEP_TYPE_PLANNER_RESPONSE" => {
                has_later_planner[idx] = seen_planner_in_turn;
                seen_planner_in_turn = true;
            }
            _ => {}
        }
    }

    let mut events = Vec::new();
    let mut turn_seq = 0usize;
    let mut pending_subagent_calls: BTreeMap<String, usize> = BTreeMap::new();
    for (index, step) in steps.iter().enumerate() {
        let source = &step["metadata"]["sourceTrajectoryStepInfo"];
        if source["cascadeId"]
            .as_str()
            .is_some_and(|orig| !orig.is_empty() && orig != id)
        {
            continue;
        }
        let step_type = step["type"].as_str().unwrap_or("");
        let timestamp = step["metadata"]["createdAt"]
            .as_str()
            .or_else(|| step["metadata"]["completedAt"].as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let status = map_cortex_status(step["status"].as_str().unwrap_or(""));

        match step_type {
            "CORTEX_STEP_TYPE_USER_INPUT" => {
                let mut parts = Vec::new();
                if let Some(items) = step["userInput"]["items"].as_array() {
                    for item in items {
                        if let Some(text) = extract_user_input_item_text(item) {
                            parts.push(json!({"type": "text", "text": text}));
                        }
                    }
                }
                if parts.is_empty() {
                    if let Some(text) = step["userInput"]["userResponse"]
                        .as_str()
                        .or_else(|| step["userInput"]["query"].as_str())
                        .filter(|t| !t.trim().is_empty())
                    {
                        parts.push(json!({"type": "text", "text": text}));
                    }
                }
                if let Some(comments) = step["userInput"]["artifactComments"].as_array() {
                    let rendered = comments
                        .iter()
                        .filter_map(format_artifact_comment)
                        .collect::<Vec<_>>();
                    if !rendered.is_empty() {
                        let prefix = if parts.is_empty() { "" } else { "\n" };
                        parts.push(json!({
                            "type": "text",
                            "text": format!("{prefix}{}", rendered.join("\n"))
                        }));
                    }
                }
                if let Some(media) = step["userInput"]["media"].as_array() {
                    for item in media {
                        if let Some(uri) =
                            item["uri"].as_str().or_else(|| item["inlineData"].as_str())
                        {
                            let path = decode_file_uri(uri);
                            parts.push(
                                json!({"type": "text", "text": format!("<image path=\"{path}\">")}),
                            );
                            parts.push(json!({
                                "type": "image",
                                "url": path,
                                "mimeType": item["mimeType"].as_str().unwrap_or("image/png")
                            }));
                            parts.push(json!({"type": "text", "text": "</image>"}));
                        }
                    }
                }
                if parts.is_empty() {
                    continue;
                }
                let event_id = format!("step-{index}-user");
                let user_msg =
                    crate::user_message::parse_user_message(&event_id, &Value::Array(parts));
                if user_msg.text.trim().is_empty() && user_msg.attachments.is_empty() {
                    continue;
                }
                turn_seq = turn_seq.saturating_add(1);
                let mut ev = make_history_event(
                    event_id,
                    timestamp,
                    "user_message",
                    "user",
                    Some(user_msg.text.clone()),
                    Some(format!("turn-{turn_seq}")),
                    Some(status),
                );
                ev.user_message = Some(user_msg);
                events.push(ev);
            }
            "CORTEX_STEP_TYPE_PLANNER_RESPONSE" => {
                let turn_id = Some(format!("turn-{turn_seq}"));
                let pr = &step["plannerResponse"];
                if let Some(thinking) = pr["thinking"].as_str().filter(|s| !s.trim().is_empty()) {
                    events.push(make_history_event(
                        format!("step-{index}-thinking"),
                        timestamp.clone(),
                        "thinking",
                        "assistant",
                        Some(thinking.to_owned()),
                        turn_id.clone(),
                        Some(status),
                    ));
                }
                let reply = pr["modifiedResponse"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| pr["response"].as_str().filter(|s| !s.trim().is_empty()));
                let is_intermediate = has_later_planner[index];
                if is_intermediate {
                    if let Some(text) = reply {
                        events.push(make_history_event(
                            format!("step-{index}-commentary"),
                            timestamp.clone(),
                            "thinking",
                            "assistant",
                            Some(text.to_owned()),
                            turn_id.clone(),
                            Some(status),
                        ));
                    }
                }
                if let Some(calls) = pr["toolCalls"].as_array() {
                    let has_final_reply = !is_intermediate && reply.is_some();
                    for (ci, call) in calls.iter().enumerate() {
                        let name = call["name"].as_str().unwrap_or("tool");
                        let args = if call["argumentsJson"].is_string() {
                            call["argumentsJson"].as_str().unwrap_or("").to_owned()
                        } else if !call["arguments"].is_null() {
                            format!("{:#}", call["arguments"])
                        } else {
                            String::new()
                        };
                        let call_id = call["id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("step-{index}-call-{ci}"));
                        let call_status = if !resolved_call_ids.contains(&call_id)
                            && !has_later_settled_step[index]
                            && !has_final_reply
                            && status != "failed"
                        {
                            None
                        } else {
                            Some(status)
                        };
                        let call_idx = events.len();
                        if is_subagent_coordination_tool(name) {
                            pending_subagent_calls.insert(call_id.clone(), call_idx);
                        }
                        events.push(make_tool_event(
                            format!("step-{index}-toolcall-{ci}"),
                            timestamp.clone(),
                            "tool_call",
                            name,
                            Some(call_id),
                            turn_id.clone(),
                            call_status,
                            (!args.trim().is_empty()).then_some(args),
                            None,
                        ));
                    }
                }
                if !is_intermediate {
                    if let Some(text) = reply {
                        events.push(make_history_event(
                            format!("step-{index}-assistant"),
                            timestamp,
                            "assistant_message",
                            "assistant",
                            Some(text.to_owned()),
                            turn_id,
                            Some(status),
                        ));
                    }
                }
            }
            "CORTEX_STEP_TYPE_FINISH"
            | "CORTEX_STEP_TYPE_EPHEMERAL_MESSAGE"
            | "CORTEX_STEP_TYPE_CHECKPOINT"
            | "CORTEX_STEP_TYPE_SYSTEM_MESSAGE"
            | "" => {}
            other => {
                let turn_id = Some(format!("turn-{turn_seq}"));
                let tool_name = step["metadata"]["toolCall"]["name"]
                    .as_str()
                    .or_else(|| step["generic"]["name"].as_str())
                    .or_else(|| step["generic"]["toolName"].as_str())
                    .or_else(|| step["toolCall"]["name"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .or_else(|| {
                        (!step["invokeSubagent"].is_null()).then(|| "invoke_subagent".into())
                    })
                    .unwrap_or_else(|| {
                        other
                            .strip_prefix("CORTEX_STEP_TYPE_")
                            .unwrap_or(other)
                            .to_ascii_lowercase()
                    });
                let call_id = step["metadata"]["toolCall"]["id"]
                    .as_str()
                    .or_else(|| step["toolCall"]["id"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let (detail, result) = summarize_cortex_tool_step(step);
                if detail.is_none() && result.is_none() && call_id.is_none() && status != "running"
                {
                    continue;
                }
                if let Some(cid) = call_id.as_deref() {
                    if let Some(call_idx) = pending_subagent_calls.remove(cid) {
                        if let Some(call_ev) = events.get_mut(call_idx) {
                            if call_ev.detail.is_none() {
                                call_ev.detail = detail;
                            }
                            if result.is_some() {
                                call_ev.result = result;
                            }
                            call_ev.status = Some(status.to_owned());
                            continue;
                        }
                    }
                }
                let kind = if call_id.is_some() {
                    "tool_result"
                } else {
                    "tool_call"
                };
                events.push(make_tool_event(
                    format!("step-{index}-tool"),
                    timestamp,
                    kind,
                    &tool_name,
                    call_id,
                    turn_id,
                    Some(status),
                    detail,
                    result,
                ));
            }
        }
    }
    events
}

fn summarize_cortex_tool_step(step: &Value) -> (Option<String>, Option<String>) {
    if let Some(cmd) = step.get("runCommand").and_then(Value::as_object) {
        let command_line = cmd
            .get("commandLine")
            .or_else(|| cmd.get("command"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let output = cmd
            .get("combinedOutput")
            .and_then(|v| v.as_str().or_else(|| v["full"].as_str()))
            .or_else(|| cmd.get("stdout").and_then(Value::as_str))
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned);
        return (command_line, output);
    }
    if let Some(vf) = step.get("viewFile").and_then(Value::as_object) {
        let uri = vf
            .get("absolutePathUri")
            .and_then(Value::as_str)
            .map(decode_file_uri);
        let content = vf
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned);
        return (uri, content);
    }
    if let Some(ca) = step.get("codeAction").and_then(Value::as_object) {
        let spec = ca
            .get("actionSpec")
            .map(|v| format!("{v:#}"))
            .filter(|s| !s.trim().is_empty());
        let res = ca
            .get("actionResult")
            .map(|v| format!("{v:#}"))
            .filter(|s| !s.trim().is_empty());
        return (spec, res);
    }
    if let Some(gen_obj) = step.get("generic").and_then(Value::as_object) {
        let detail = [
            gen_obj.get("argumentsJson"),
            gen_obj.get("arguments"),
            gen_obj.get("args"),
            step["metadata"]["toolCall"].get("argumentsJson"),
        ]
        .into_iter()
        .flatten()
        .find_map(|v| {
            if v.is_null() {
                None
            } else if let Some(s) = v.as_str() {
                (!s.trim().is_empty()).then(|| s.to_owned())
            } else {
                let formatted = format!("{v:#}");
                (!formatted.trim().is_empty() && formatted != "{}" && formatted != "null")
                    .then_some(formatted)
            }
        });
        let result = [gen_obj.get("result"), gen_obj.get("output")]
            .into_iter()
            .flatten()
            .find_map(|v| {
                if v.is_null() {
                    None
                } else if let Some(s) = v.as_str() {
                    (!s.trim().is_empty()).then(|| s.to_owned())
                } else {
                    let formatted = format!("{v:#}");
                    (!formatted.trim().is_empty() && formatted != "{}" && formatted != "null")
                        .then_some(formatted)
                }
            });
        return (detail, result);
    }
    let detail = step["metadata"]["toolCall"]["argumentsJson"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned);
    let result = step
        .as_object()
        .and_then(|obj| {
            obj.iter()
                .find(|(k, _)| {
                    !matches!(k.as_str(), "type" | "status" | "metadata" | "permissions")
                })
                .map(|(_, v)| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{v:#}"))
                })
        })
        .filter(|s| !s.trim().is_empty() && s != "{}" && s != "null");
    (detail, result)
}

pub(super) fn read_offline_transcript_events(
    paths: &AntigravityPaths,
    id: &str,
) -> Result<Vec<ProviderHistoryEvent>> {
    let logs_dir = paths.brain_dir().join(id).join(".system_generated/logs");
    let full = logs_dir.join("transcript_full.jsonl");
    let compact = logs_dir.join("transcript.jsonl");
    let chosen = if full.is_file() {
        Some(full)
    } else if compact.is_file() {
        Some(compact)
    } else {
        None
    };

    if let Some(path) = chosen {
        let file = fs::File::open(&path)?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        let mut rows = Vec::new();
        loop {
            line.clear();
            let read = (&mut reader)
                .take(TRANSCRIPT_LINE_LIMIT as u64 + 1)
                .read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            if line.len() > TRANSCRIPT_LINE_LIMIT || line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            if let Ok(row) = serde_json::from_slice::<Value>(&line) {
                rows.push(row);
            }
        }

        let row_count = rows.len();
        let mut has_later_planner = vec![false; row_count];
        let mut has_later_settled_row = vec![false; row_count];
        let mut seen_planner_in_turn = false;
        let mut seen_settled_after = false;
        for (pos, row) in rows.iter().enumerate().rev() {
            has_later_settled_row[pos] = seen_settled_after;
            let st = row["type"].as_str().unwrap_or("");
            let src = row["source"].as_str().unwrap_or("");
            let sr = row["status"].as_str().unwrap_or("DONE");
            if !matches!(
                sr,
                "RUNNING" | "WAITING" | "PENDING" | "GENERATING" | "QUEUED"
            ) && !matches!(
                st,
                "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "SYSTEM_MESSAGE" | ""
            ) {
                seen_settled_after = true;
            }
            if st == "USER_INPUT" || src == "USER_EXPLICIT" {
                seen_planner_in_turn = false;
            } else if st == "PLANNER_RESPONSE" {
                has_later_planner[pos] = seen_planner_in_turn;
                seen_planner_in_turn = true;
            }
        }

        let mut events: Vec<ProviderHistoryEvent> = Vec::new();
        let mut turn_seq = 0usize;
        let mut pending_calls: VecDeque<(String, String, usize)> = VecDeque::new();
        for (idx, row) in rows.into_iter().enumerate() {
            let step_index = row["step_index"].as_u64().unwrap_or(idx as u64);
            let step_type = row["type"].as_str().unwrap_or("");
            let source = row["source"].as_str().unwrap_or("");
            let timestamp = row["created_at"].as_str().map(str::to_owned);
            let status_raw = row["status"].as_str().unwrap_or("DONE");
            let status = if step_type == "ERROR_MESSAGE" {
                "failed"
            } else {
                match status_raw {
                    "RUNNING" | "WAITING" | "PENDING" | "GENERATING" | "QUEUED"
                        if has_later_settled_row[idx] =>
                    {
                        "completed"
                    }
                    "RUNNING" | "WAITING" | "PENDING" | "GENERATING" | "QUEUED" => "running",
                    "ERROR" | "FAILED" | "CANCELED" | "INVALID" | "INTERRUPTED" => "failed",
                    _ => "completed",
                }
            };

            if step_type == "USER_INPUT" || source == "USER_EXPLICIT" {
                let text = row["content"].as_str().unwrap_or("");
                let mut parts = Vec::new();
                if !text.trim().is_empty() {
                    parts.push(json!({"type": "text", "text": text}));
                }
                if let Some(media) = row["media"].as_array() {
                    for item in media {
                        if let Some(uri) = item["uri"].as_str() {
                            let decoded = decode_file_uri(uri);
                            parts.push(
                                json!({"type": "text", "text": format!("<image path=\"{decoded}\">")}),
                            );
                            parts.push(json!({
                                "type": "image",
                                "url": decoded,
                                "mimeType": item["mime_type"].as_str().unwrap_or("image/png")
                            }));
                            parts.push(json!({"type": "text", "text": "</image>"}));
                        }
                    }
                }
                if parts.is_empty() {
                    continue;
                }
                turn_seq = turn_seq.saturating_add(1);
                for (_, _, call_idx) in pending_calls.drain(..) {
                    if let Some(ev) = events.get_mut(call_idx) {
                        ev.status.get_or_insert_with(|| "completed".into());
                    }
                }
                let turn_id = Some(format!("turn-{turn_seq}"));
                let event_id = format!("transcript-{step_index}-user");
                let user_msg =
                    crate::user_message::parse_user_message(&event_id, &Value::Array(parts));
                let mut ev = make_history_event(
                    event_id,
                    timestamp,
                    "user_message",
                    "user",
                    Some(user_msg.text.clone()),
                    turn_id,
                    Some(status),
                );
                ev.user_message = Some(user_msg);
                events.push(ev);
            } else if step_type == "PLANNER_RESPONSE" {
                let turn_id = Some(format!("turn-{turn_seq}"));
                if let Some(thinking) = row["thinking"].as_str().filter(|s| !s.trim().is_empty()) {
                    events.push(make_history_event(
                        format!("transcript-{step_index}-thinking"),
                        timestamp.clone(),
                        "thinking",
                        "assistant",
                        Some(thinking.to_owned()),
                        turn_id.clone(),
                        Some(status),
                    ));
                }
                let is_intermediate = has_later_planner[idx];
                if is_intermediate {
                    if let Some(content) = row["content"].as_str().filter(|s| !s.trim().is_empty())
                    {
                        events.push(make_history_event(
                            format!("transcript-{step_index}-commentary"),
                            timestamp.clone(),
                            "thinking",
                            "assistant",
                            Some(content.to_owned()),
                            turn_id.clone(),
                            Some(status),
                        ));
                    }
                }
                let has_tool_calls = row["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| !calls.is_empty());
                let has_final_reply = !is_intermediate
                    && !has_tool_calls
                    && row["content"]
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty());
                if let Some(calls) = row["tool_calls"].as_array() {
                    for (ci, call) in calls.iter().enumerate() {
                        let name = call["name"]
                            .as_str()
                            .or_else(|| call["tool_name"].as_str())
                            .unwrap_or("tool");
                        let args = call
                            .get("arguments")
                            .or_else(|| call.get("args"))
                            .map(|v| {
                                v.as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| format!("{v:#}"))
                            })
                            .filter(|s| !s.trim().is_empty());
                        let result = call
                            .get("output")
                            .or_else(|| call.get("result"))
                            .and_then(Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned);
                        let call_id = call["id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("transcript-{step_index}-call-{ci}"));
                        let call_status = if result.is_none()
                            && !has_later_settled_row[idx]
                            && !has_final_reply
                            && status != "failed"
                        {
                            None
                        } else {
                            Some(status)
                        };
                        let call_idx = events.len();
                        if result.is_none() {
                            pending_calls.push_back((call_id.clone(), name.to_owned(), call_idx));
                        }
                        events.push(make_tool_event(
                            format!("transcript-{step_index}-tool-{ci}"),
                            timestamp.clone(),
                            "tool_call",
                            name,
                            Some(call_id),
                            turn_id.clone(),
                            call_status,
                            args,
                            result,
                        ));
                    }
                }
                if !is_intermediate {
                    if let Some(content) = row["content"].as_str().filter(|s| !s.trim().is_empty())
                    {
                        events.push(make_history_event(
                            format!("transcript-{step_index}-assistant"),
                            timestamp,
                            "assistant_message",
                            "assistant",
                            Some(content.to_owned()),
                            turn_id,
                            Some(status),
                        ));
                    }
                }
            } else if step_type == "FINISH" {
                for (_, _, call_idx) in pending_calls.drain(..) {
                    if let Some(ev) = events.get_mut(call_idx) {
                        ev.status.get_or_insert_with(|| "completed".into());
                    }
                }
                continue;
            } else if step_type == "ERROR_MESSAGE" {
                for (_, _, call_idx) in pending_calls.drain(..) {
                    if let Some(ev) = events.get_mut(call_idx) {
                        ev.status.get_or_insert_with(|| "failed".into());
                    }
                }
                let err_text = row["content"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| row["error"].as_str().filter(|s| !s.trim().is_empty()))
                    .unwrap_or("执行出错")
                    .to_owned();
                events.push(make_tool_event(
                    format!("transcript-{step_index}-error"),
                    timestamp,
                    "tool_call",
                    "error_message",
                    None,
                    Some(format!("turn-{turn_seq}")),
                    Some("failed"),
                    None,
                    Some(err_text),
                ));
            } else if step_type == "SYSTEM_MESSAGE" {
                if source == "SYSTEM" {
                    for (_, _, call_idx) in pending_calls.drain(..) {
                        if let Some(ev) = events.get_mut(call_idx) {
                            ev.status.get_or_insert_with(|| "completed".into());
                        }
                    }
                }
                continue;
            } else if matches!(step_type, "CHECKPOINT" | "EPHEMERAL_MESSAGE" | "") {
                continue;
            } else {
                let turn_id = Some(format!("turn-{turn_seq}"));
                let content = row["content"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| row["error"].as_str().filter(|s| !s.trim().is_empty()))
                    .map(str::to_owned);
                if let Some((call_id, tool_name, call_idx)) = pending_calls.pop_front() {
                    if let Some(call_ev) = events.get_mut(call_idx) {
                        call_ev.status.get_or_insert_with(|| status.to_owned());
                        if is_subagent_coordination_tool(&tool_name) {
                            if content.is_some() {
                                call_ev.result = content;
                            }
                            call_ev.status = Some(status.to_owned());
                            continue;
                        }
                    }
                    events.push(make_tool_event(
                        format!("transcript-{step_index}-toolresult"),
                        timestamp,
                        "tool_result",
                        &tool_name,
                        Some(call_id),
                        turn_id,
                        Some(status),
                        None,
                        content,
                    ));
                } else if let Some(content) = content {
                    let name = step_type.to_ascii_lowercase();
                    events.push(make_tool_event(
                        format!("transcript-{step_index}-step"),
                        timestamp,
                        "tool_call",
                        &name,
                        None,
                        turn_id,
                        Some(status),
                        None,
                        Some(content),
                    ));
                }
            }
        }
        if !events.is_empty() {
            return Ok(events);
        }
    }

    // Secondary offline fallback: extract step_payload fields 19 (user_input), 20 (planner_response), 24 (error_message)
    let conv_db = paths.conversations_dir().join(format!("{id}.db"));
    if conv_db.is_file() {
        if let Ok(conn) = Connection::open_with_flags(
            &conv_db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            if let Ok(mut stmt) =
                conn.prepare("SELECT idx, step_type, step_payload FROM steps ORDER BY idx ASC")
            {
                if let Ok(rows) = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                }) {
                    let mut events = Vec::new();
                    for (idx, step_type, blob) in rows.flatten() {
                        let target_field = match step_type {
                            14 => Some(19),
                            15 => Some(20),
                            17 => Some(24),
                            _ => None,
                        };
                        let Some(field_no) = target_field else {
                            continue;
                        };
                        let mut strings =
                            super::annotations::extract_step_payload_field_strings(&blob, field_no);
                        if strings.is_empty() {
                            strings = extract_protobuf_utf8_strings(&blob);
                        }
                        let joined = strings.join("\n").trim().to_owned();
                        if joined.is_empty() {
                            continue;
                        }
                        if step_type == 14 {
                            let event_id = format!("db-{idx}-user");
                            let user_msg = crate::user_message::parse_user_message(
                                &event_id,
                                &Value::String(joined.clone()),
                            );
                            let mut ev = make_history_event(
                                event_id,
                                None,
                                "user_message",
                                "user",
                                Some(user_msg.text.clone()),
                                None,
                                Some("completed"),
                            );
                            ev.user_message = Some(user_msg);
                            events.push(ev);
                        } else if step_type == 15 {
                            events.push(make_history_event(
                                format!("db-{idx}-assistant"),
                                None,
                                "assistant_message",
                                "assistant",
                                Some(joined),
                                None,
                                Some("completed"),
                            ));
                        } else if step_type == 17 {
                            events.push(make_tool_event(
                                format!("db-{idx}-error"),
                                None,
                                "tool_call",
                                "error_message",
                                None,
                                None,
                                Some("failed"),
                                None,
                                Some(joined),
                            ));
                        }
                    }
                    return Ok(events);
                }
            }
        }
    }

    Ok(Vec::new())
}

fn extract_protobuf_utf8_strings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    collect_proto_strings(bytes, 0, &mut out);
    out
}

fn read_varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    let mut shift = 0u32;
    while *pos < bytes.len() && shift < 64 {
        let b = bytes[*pos];
        *pos += 1;
        result |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
    }
    None
}

fn collect_proto_strings(bytes: &[u8], depth: usize, out: &mut Vec<String>) {
    if depth > 4 {
        return;
    }
    let mut pos = 0usize;
    while pos < bytes.len() {
        let Some(tag) = read_varint(bytes, &mut pos) else {
            return;
        };
        let wire = tag & 0x07;
        match wire {
            0 => {
                if read_varint(bytes, &mut pos).is_none() {
                    return;
                }
            }
            1 => {
                if pos + 8 > bytes.len() {
                    return;
                }
                pos += 8;
            }
            5 => {
                if pos + 4 > bytes.len() {
                    return;
                }
                pos += 4;
            }
            2 => {
                let Some(len) = read_varint(bytes, &mut pos) else {
                    return;
                };
                let len = len as usize;
                if pos + len > bytes.len() {
                    return;
                }
                let slice = &bytes[pos..pos + len];
                pos += len;
                if let Ok(text) = std::str::from_utf8(slice) {
                    let trimmed = text.trim();
                    if trimmed.len() >= 2
                        && !trimmed
                            .chars()
                            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                        && uuid::Uuid::parse_str(trimmed).is_err()
                    {
                        out.push(trimmed.to_owned());
                        continue;
                    }
                }
                collect_proto_strings(slice, depth + 1, out);
            }
            _ => return,
        }
    }
}

pub fn load_all_events(
    paths: &AntigravityPaths,
    id: &str,
) -> Result<std::sync::Arc<Vec<ProviderHistoryEvent>>> {
    valid_cascade_id(id)?;
    if let Some(cli) = paths.cli_store_for_id(id) {
        return Ok(load_events_from_source(&cli, id, None)?.events.clone());
    }
    let primary = paths.primary_store();
    let endpoint = connect_endpoint(&primary)?;
    Ok(load_events_from_source(&primary, id, endpoint.as_ref())?
        .events
        .clone())
}

#[derive(Debug)]
struct LiveHistoryUnavailable;
impl std::fmt::Display for LiveHistoryUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("原生历史服务不可达或暂无在线内容")
    }
}
impl std::error::Error for LiveHistoryUnavailable {}

fn read_live_events(rpc: &LiveRpc, id: &str) -> Result<Vec<ProviderHistoryEvent>> {
    let mut resp = connect_rpc_post(
        rpc.port,
        &rpc.token,
        "GetCascadeTrajectory",
        &json!({"cascadeId": id}),
    )
    .map_err(|error| {
        if service_unreachable(&error) {
            anyhow::Error::new(LiveHistoryUnavailable)
        } else {
            error
        }
    })?;
    if resp.is_object() && resp.get("trajectory").is_none() {
        return Err(LiveHistoryUnavailable.into());
    }
    ensure!(
        resp["trajectory"]["cascadeId"].as_str() == Some(id),
        "原生历史身份不匹配"
    );
    let mut steps = match resp["trajectory"].get_mut("steps") {
        Some(value) => value
            .as_array_mut()
            .map(std::mem::take)
            .context("原生历史步骤格式错误")?,
        None => Vec::new(),
    };
    let total = match resp.get("numTotalSteps") {
        Some(value) => value.as_u64().context("原生历史总数格式错误")? as usize,
        None => steps.len(),
    };
    if steps.is_empty() && total == 0 {
        return Err(LiveHistoryUnavailable.into());
    }
    ensure!(!steps.is_empty(), "原生历史读取不完整，请重试");
    while steps.len() < total {
        let mut more = connect_rpc_post(
            rpc.port,
            &rpc.token,
            "GetCascadeTrajectorySteps",
            &json!({"cascadeId": id, "stepOffset": steps.len()}),
        )
        .map_err(|error| {
            if service_unreachable(&error) {
                anyhow::Error::new(LiveHistoryUnavailable)
            } else {
                error
            }
        })?;
        if let Some(cascade_id) = more.get("cascadeId") {
            ensure!(cascade_id.as_str() == Some(id), "原生历史身份不匹配");
        }
        let batch = more["steps"]
            .as_array_mut()
            .context("原生历史分页格式错误")?;
        ensure!(!batch.is_empty(), "原生历史读取不完整，请重试");
        steps.append(batch);
    }
    let events = extract_live_step_events(id, &steps);
    if events.is_empty() {
        return Err(LiveHistoryUnavailable.into());
    }
    Ok(events)
}

pub(crate) fn load_events_from_source(
    paths: &AntigravityPaths,
    id: &str,
    endpoint: Option<&(LiveRpc, Value)>,
) -> Result<std::sync::Arc<ProviderHistorySnapshot>> {
    use crate::provider_sessions::{history_weight, HISTORY_CACHE};
    if let Some((rpc, payload)) = endpoint {
        let summaries = payload["trajectorySummaries"].as_object();
        let item = &payload["trajectorySummaries"][id];
        let active =
            summaries.is_some_and(|map| super::subagent_states_by_root(map).contains_key(id));
        let reusable =
            item["status"] == "CASCADE_RUN_STATUS_IDLE" && !active && !live_item_is_running(item);
        let revision = reusable.then(|| {
            Sha256::digest(serde_json::to_vec(&json!([id, item])).expect("JSON value")).to_vec()
        });
        let events = HISTORY_CACHE.read_revision(
            &paths.brain_dir().join(id).join("live-history"),
            revision,
            history_weight,
            || read_live_events(rpc, id).map(ProviderHistorySnapshot::new),
        );
        match events {
            Ok(events) => return Ok(events),
            Err(error) if error.is::<LiveHistoryUnavailable>() => {}
            Err(error) => return Err(error),
        }
    }
    let logs = paths.brain_dir().join(id).join(".system_generated/logs");
    let db = paths.conversations_dir().join(format!("{id}.db"));
    HISTORY_CACHE.read_sources(
        &paths.brain_dir().join(id).join("offline-history"),
        &[
            logs.join("transcript_full.jsonl"),
            logs.join("transcript.jsonl"),
            db.clone(),
            db.with_extension("db-wal"),
        ],
        history_weight,
        || read_offline_transcript_events(paths, id).map(ProviderHistorySnapshot::new),
    )
}

struct LoadedSessionSource {
    summary: ProviderSessionSummary,
    events: std::sync::Arc<ProviderHistorySnapshot>,
    store: AntigravityPaths,
    endpoint: Option<(LiveRpc, Value)>,
}

fn summary_and_events(paths: &AntigravityPaths, id: &str) -> Result<LoadedSessionSource> {
    valid_cascade_id(id)?;
    if let Some(cli) = paths.cli_store_for_id(id) {
        let summary = list_summaries_from_source(&cli, Some(id), None)?
            .into_iter()
            .next()
            .context("未找到 Antigravity 会话")?;
        let events = load_events_from_source(&cli, id, None)?;
        return Ok(LoadedSessionSource {
            summary,
            events,
            store: cli,
            endpoint: None,
        });
    }
    let primary = paths.primary_store();
    let endpoint = connect_endpoint(&primary)?;
    let summary =
        list_summaries_from_source(paths, Some(id), endpoint.as_ref().map(|(_, value)| value))?
            .into_iter()
            .next()
            .context("未找到 Antigravity 会话")?;
    let store = paths.store_for_id(id, endpoint.as_ref().map(|(_, value)| value));
    let store_ep = (store.home == primary.home).then_some(endpoint).flatten();
    let events = load_events_from_source(&store, id, store_ep.as_ref())?;
    Ok(LoadedSessionSource {
        summary,
        events,
        store,
        endpoint: store_ep,
    })
}

pub fn session_detail(
    paths: &AntigravityPaths,
    request: &ProviderDetailRequest,
) -> Result<ProviderSessionDetail> {
    if let Some(agent_id) = request.agent_id.as_deref() {
        return crate::antigravity::subagents::subagent_detail(paths, request, agent_id);
    }
    let mut loaded = summary_and_events(paths, &request.session_key)?;
    attach_storage_size(&loaded.store, &mut loaded.summary);
    let mut detail = paginate_provider_detail(loaded.summary, &loaded.events, request)?;
    crate::antigravity::subagents::enrich_session_detail(
        &loaded.store,
        loaded.endpoint.as_ref(),
        &request.session_key,
        &loaded.events.events,
        &mut detail,
    );
    Ok(detail)
}

pub fn read_attachment(
    paths: &AntigravityPaths,
    request: &crate::user_message::SessionAttachmentRequest,
) -> Result<crate::user_message::SessionAttachmentResponse> {
    let loaded = summary_and_events(paths, &request.session_key)?;
    let mut message = loaded
        .events
        .events
        .iter()
        .filter_map(|event| event.user_message.as_ref())
        .find(|msg| msg.id == request.message_id)
        .cloned()
        .context("会话消息已变化或不存在，请刷新会话")?;
    message.refresh_files((!loaded.summary.cwd.is_empty()).then_some(loaded.summary.cwd.as_str()));
    message.read_attachment(&request.attachment_id)
}
