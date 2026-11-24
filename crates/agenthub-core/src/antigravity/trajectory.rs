use super::*;

struct TurnWindow<'a> {
    id: &'a str,
    summary_trajectory_id: &'a str,
    steps: &'a [Value],
    step_offset: usize,
    local_turn_start: usize,
    latest: &'a str,
    cascade_active: bool,
    unsettled_background: bool,
    count: u64,
    ctx: &'a TrajectoryLivenessContext,
}

struct Skipped {
    reason: &'static str,
    hold_cursor: bool,
}

impl TurnWindow<'_> {
    fn event_for_executor(
        &self,
        meta: &Value,
        exec: &str,
        group: &[(usize, &Value)],
    ) -> Result<NativeTurnEvent, Skipped> {
        let Self {
            id,
            summary_trajectory_id,
            steps,
            step_offset,
            latest,
            ..
        } = *self;
        let end = meta["lastStepIdx"].as_u64();
        let reason = meta["terminationReason"].as_str().unwrap_or("");
        if group.is_empty() {
            return Err(Skipped {
                reason: "no_steps_in_window",
                hold_cursor: false,
            });
        }
        if reason.is_empty() || reason == "EXECUTOR_TERMINATION_REASON_UNSPECIFIED" {
            return Err(Skipped {
                reason: "termination_reason_unspecified",
                hold_cursor: true,
            });
        }
        if meta["proceededWithAutoContinue"] == true {
            return Err(Skipped {
                reason: "proceeded_with_auto_continue",
                hold_cursor: exec == self.latest,
            });
        }
        if group.iter().any(|(_, step)| !settled(step)) {
            return Err(Skipped {
                reason: "group_has_unsettled_step",
                hold_cursor: exec == self.latest,
            });
        }
        let Some((last_global_index, last)) = group.last() else {
            return Err(Skipped {
                reason: "no_steps_in_window",
                hold_cursor: false,
            });
        };
        if end != Some(*last_global_index as u64) {
            return Err(Skipped {
                reason: "end_mismatch_last_global_index",
                hold_cursor: exec == self.latest,
            });
        }
        let local_last_idx = last_global_index.saturating_sub(step_offset);
        let local_after = local_last_idx.saturating_add(1);
        let next_user_input_idx = steps
            .iter()
            .enumerate()
            .skip(local_after)
            .find(|(_, s)| {
                !step_is_foreign(id, summary_trajectory_id, s)
                    && s["type"] == "CORTEX_STEP_TYPE_USER_INPUT"
            })
            .map(|(idx, _)| idx);
        if next_user_input_idx.is_none() && exec != latest {
            return Err(Skipped {
                reason: "not_latest_executor_in_turn",
                hold_cursor: false,
            });
        }
        let terminal_no_tool_call = (last["type"] == "CORTEX_STEP_TYPE_PLANNER_RESPONSE"
            && last["plannerResponse"]["toolCalls"]
                .as_array()
                .is_none_or(Vec::is_empty))
            || last["type"] == "CORTEX_STEP_TYPE_FINISH";
        if reason == "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL" && !terminal_no_tool_call {
            return Err(Skipped {
                reason: "no_tool_call_on_non_terminal_step",
                hold_cursor: exec == self.latest,
            });
        }
        let kind = match reason {
            "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL" => "completion",
            "EXECUTOR_TERMINATION_REASON_TERMINAL_STEP_TYPE"
                if last["type"] == "CORTEX_STEP_TYPE_FINISH" =>
            {
                "completion"
            }
            "EXECUTOR_TERMINATION_REASON_ERROR"
            | "EXECUTOR_TERMINATION_REASON_MAX_INVOCATIONS"
            | "EXECUTOR_TERMINATION_REASON_MAX_FORCED_INVOCATIONS"
            | "EXECUTOR_TERMINATION_REASON_MAX_TOKEN_BUDGET_EXCEEDED" => "failure",
            _ => {
                return Err(Skipped {
                    reason: "non_notifiable_termination_reason",
                    hold_cursor: false,
                });
            }
        };

        self.verify_terminal_turn(
            kind,
            exec,
            local_after,
            next_user_input_idx,
            *last_global_index as u64,
        )?;
        let timestamp = native_probe::timestamp(&last["metadata"]["completedAt"])
            .or_else(|| native_probe::timestamp(&last["metadata"]["finishedGeneratingAt"]));
        let Some(timestamp_ms) = timestamp else {
            return Err(Skipped {
                reason: "missing_timestamp",
                hold_cursor: false,
            });
        };

        let (kind, body) =
            self.event_content(kind, group, local_last_idx, next_user_input_idx.is_some());
        Ok(NativeTurnEvent {
            prompt_id: None,
            position: *last_global_index as u64 + 1,
            turn_id: exec.into(),
            kind: kind.into(),
            body,
            timestamp_ms,
        })
    }

    fn verify_terminal_turn(
        &self,
        kind: &str,
        exec: &str,
        local_after: usize,
        next_user_input_idx: Option<usize>,
        last_global_index: u64,
    ) -> Result<(), Skipped> {
        let Self {
            id,
            summary_trajectory_id,
            steps,
            ctx,
            cascade_active,
            unsettled_background,
            count,
            ..
        } = *self;
        if let Some(next_ui_idx) = next_user_input_idx {
            if kind != "completion" {
                return Err(Skipped {
                    reason: "prior_turn_non_completion",
                    hold_cursor: false,
                });
            }
            if steps[local_after..next_ui_idx].iter().any(|s| {
                !step_is_foreign(id, summary_trajectory_id, s)
                    && s["type"] == "CORTEX_STEP_TYPE_SYSTEM_MESSAGE"
            }) {
                return Err(Skipped {
                    reason: "intervening_system_message",
                    hold_cursor: false,
                });
            }
            if steps[local_after..next_ui_idx].iter().any(|s| {
                !step_is_foreign(id, summary_trajectory_id, s)
                    && !execution(s).is_empty()
                    && execution(s) != exec
            }) {
                return Err(Skipped {
                    reason: "intervening_other_executor",
                    hold_cursor: false,
                });
            }
            if has_unsettled_background_work(id, summary_trajectory_id, &steps[..next_ui_idx], ctx)
            {
                return Err(Skipped {
                    reason: "prior_turn_has_pending_work",
                    hold_cursor: true,
                });
            }
        } else if kind == "completion" {
            if (last_global_index + 1) < count {
                return Err(Skipped {
                    reason: "trailing_steps_after_last_global_index",
                    hold_cursor: true,
                });
            }
            if cascade_active {
                return Err(Skipped {
                    reason: "cascade_active",
                    hold_cursor: true,
                });
            }
            if unsettled_background {
                return Err(Skipped {
                    reason: "unsettled_background",
                    hold_cursor: true,
                });
            }
        }
        // User cancellation or a new execution invalidates a previous failure.
        if kind == "failure"
            && (steps.iter().skip(local_after).any(|s| {
                s["type"] == "CORTEX_STEP_TYPE_USER_INPUT"
                    || s["type"] == "CORTEX_STEP_TYPE_SYSTEM_MESSAGE"
                    || (!execution(s).is_empty() && execution(s) != exec)
            }) || cascade_active)
        {
            return Err(Skipped {
                reason: "superseded_failure",
                hold_cursor: true,
            });
        }

        Ok(())
    }
    fn event_content(
        &self,
        mut kind: &'static str,
        group: &[(usize, &Value)],
        local_last_idx: usize,
        prior_turn: bool,
    ) -> (&'static str, String) {
        let Self {
            id,
            summary_trajectory_id,
            steps,
            local_turn_start,
            ..
        } = *self;
        let last_planner_text = group
            .iter()
            .rev()
            .find(|(_, step)| step["type"] == "CORTEX_STEP_TYPE_PLANNER_RESPONSE")
            .and_then(|(_, step)| step["plannerResponse"]["response"].as_str())
            .unwrap_or("");
        let exec_turn_start = if prior_turn {
            steps[..=local_last_idx]
                .iter()
                .enumerate()
                .rev()
                .find(|(_, step)| {
                    !step_is_foreign(id, summary_trajectory_id, step)
                        && step["type"] == "CORTEX_STEP_TYPE_USER_INPUT"
                })
                .map_or(0, |(idx, _)| idx + 1)
        } else {
            local_turn_start
        };
        let turn_slice = if exec_turn_start <= local_last_idx && local_last_idx < steps.len() {
            &steps[exec_turn_start..=local_last_idx]
        } else {
            &[]
        };
        let artifact_feedback_step = turn_slice.iter().rev().find(|step| {
            !step_is_foreign(id, summary_trajectory_id, step)
                && settled(step)
                && step_requests_artifact_feedback(step)
        });
        let detected_question = if kind == "completion" {
            crate::codex::assistant_feedback_request(last_planner_text)
        } else {
            None
        };
        if kind == "completion" && (artifact_feedback_step.is_some() || detected_question.is_some())
        {
            kind = "reply_needed";
        }
        let body = if kind == "failure" {
            "主回合未能完成，请打开 Antigravity 查看详情。".into()
        } else if let Some(question_text) = detected_question.as_deref() {
            safe_text(&Value::String(question_text.to_owned()))
        } else if !last_planner_text.trim().is_empty() {
            safe_text(&Value::String(last_planner_text.to_owned()))
        } else if let Some(summary_text) =
            artifact_feedback_step.and_then(step_artifact_feedback_summary)
        {
            safe_text(&Value::String(summary_text.to_owned()))
        } else if kind == "reply_needed" {
            "需要回复或确认计划，请打开 Antigravity 处理。".into()
        } else {
            String::new()
        };

        (kind, body)
    }
}

fn describe_candidate(
    meta: &Value,
    actual_last: Option<u64>,
    db_cursor: Option<u64>,
    skip_reason: Option<&'static str>,
) -> Value {
    let position = actual_last.map(|idx| idx + 1);
    serde_json::json!({
        "executionId": meta["executionId"],
        "lastStepIdx": meta["lastStepIdx"].as_u64(),
        "actualLastStepIdx": actual_last,
        "position": position,
        "terminationReason": meta["terminationReason"].as_str().unwrap_or(""),
        "status": if skip_reason.is_none() { "emitted" } else { "skipped" },
        "skipReason": skip_reason,
        "cursorComparison": match (position, db_cursor) {
            (Some(pos), Some(cur)) if pos < cur => "position < cursor (already behind cursor)",
            (Some(pos), Some(cur)) if pos == cur => "position == cursor (at cursor boundary)",
            (Some(_), Some(_)) => "position > cursor (eligible)",
            _ => "no_db_cursor",
        },
    })
}

pub(super) fn evaluate_trajectory(
    id: &str,
    summary: &Value,
    response: &Value,
    ctx: &TrajectoryLivenessContext,
    db_cursor: Option<u64>,
    want_diag: bool,
) -> Result<(NativeStreamSnapshot, Value)> {
    let identity = identity(id, summary)?;
    let trajectory = &response["trajectory"];
    ensure!(
        trajectory["cascadeId"] == id && trajectory["trajectoryId"] == summary["trajectoryId"],
        "Antigravity source identity changed"
    );
    let step_offset = response["stepOffset"].as_u64().unwrap_or(0) as usize;
    let steps = trajectory["steps"]
        .as_array()
        .context("missing Antigravity steps")?;
    let total_steps = step_offset.saturating_add(steps.len()) as u64;
    let count = response["numTotalSteps"].as_u64().unwrap_or(total_steps);
    ensure!(
        count == total_steps && count <= 200_000,
        "incomplete Antigravity trajectory"
    );
    let executors = trajectory["executorMetadatas"]
        .as_array()
        .context("missing Antigravity execution state")?;
    let latest = executors
        .last()
        .and_then(|e| e["executionId"].as_str())
        .unwrap_or("");
    let summary_trajectory_id = summary["trajectoryId"].as_str().unwrap_or("");
    let summary_status_active = cascade_status_is_active(summary["status"].as_str());
    let response_status_active = cascade_status_is_active(response["status"].as_str());
    let subagents_any_active = ctx.subagents.values().any(|s| s.active);
    let not_fully_idle = summary["notFullyIdle"].as_bool() == Some(true);
    let has_running_subagents = summary["hasRunningSubagents"].as_bool() == Some(true);
    let waiting_steps_non_empty = summary["waitingSteps"]
        .as_array()
        .is_some_and(|arr| !arr.is_empty());
    let cascade_active = ctx.has_active_subagents
        || subagents_any_active
        || summary_status_active
        || response_status_active
        || not_fully_idle
        || has_running_subagents
        || waiting_steps_non_empty;
    let unsettled_background = has_unsettled_background_work(id, summary_trajectory_id, steps, ctx);
    let summary_modified_ms = native_probe::timestamp(&summary["lastModifiedTime"])
        .or_else(|| native_probe::timestamp(&summary["createdTime"]));
    let local_turn_start = steps
        .iter()
        .enumerate()
        .rev()
        .find(|(_, step)| {
            !step_is_foreign(id, summary_trajectory_id, step)
                && step["type"] == "CORTEX_STEP_TYPE_USER_INPUT"
        })
        .map_or(0, |(idx, _)| idx + 1);
    let mut by_execution: BTreeMap<&str, Vec<(usize, &Value)>> = BTreeMap::new();
    let mut events = Vec::new();
    let mut settled_count = count;
    for (local_index, step) in steps.iter().enumerate() {
        let global_index = step_offset + local_index;
        if step_is_foreign(id, summary_trajectory_id, step) {
            continue; // Forked / inherited steps belong to the original conversation.
        }
        let exec = execution(step);
        if !exec.is_empty() {
            by_execution
                .entry(exec)
                .or_default()
                .push((global_index, step));
        }
        if !settled(step) {
            settled_count = settled_count.min(global_index as u64);
        }
        if let Some(event) = interaction(step, global_index, summary_modified_ms)? {
            events.push(event);
        }
    }
    let known_executors: BTreeSet<&str> = executors
        .iter()
        .filter_map(|e| e["executionId"].as_str())
        .collect();
    for (exec, group) in &by_execution {
        if !known_executors.contains(exec)
            && group.iter().all(|(_, step)| settled(step))
            && group
                .last()
                .is_some_and(|(idx, _)| *idx >= step_offset + local_turn_start)
        {
            settled_count = settled_count.min(group[0].0 as u64);
        }
    }
    let mut candidates = if want_diag {
        Vec::with_capacity(executors.len())
    } else {
        Vec::new()
    };
    let window = TurnWindow {
        id,
        summary_trajectory_id,
        steps,
        step_offset,
        local_turn_start,
        latest,
        cascade_active,
        unsettled_background,
        count,
        ctx,
    };
    for meta in executors {
        let exec = meta["executionId"]
            .as_str()
            .context("missing Antigravity execution identity")?;
        let group = by_execution
            .get(exec)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let skip_reason = match window.event_for_executor(meta, exec, group) {
            Ok(event) => {
                events.push(event);
                None
            }
            Err(skip) => {
                if skip.hold_cursor {
                    if let Some((first, _)) = group.first() {
                        settled_count = settled_count.min(*first as u64);
                    }
                }
                Some(skip.reason)
            }
        };
        if want_diag {
            candidates.push(describe_candidate(
                meta,
                group.last().map(|(idx, _)| *idx as u64),
                db_cursor,
                skip_reason,
            ));
        }
    }
    events.sort_by_key(|e| e.position);
    if events.len() > 1024 {
        events.drain(..events.len() - 1024);
    }
    let raw_title = summary["annotations"]["title"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            summary["summary"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("");
    let summary_cwd = summary["workspaces"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|ws| ws["workspaceFolderAbsoluteUri"].as_str())
        .or_else(|| {
            summary["trajectoryMetadata"]["workspaceUris"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
        })
        .map(sessions::decode_file_uri)
        .unwrap_or_default();
    let resolved_title = native_probe::notification_title(raw_title, id, &summary_cwd);
    let snapshot = NativeStreamSnapshot {
        provider: NativeProvider::Antigravity,
        session_key: id.into(),
        id: id.into(),
        title: safe_text(&Value::String(resolved_title)),
        identity,
        record_count: count,
        settled_count,
        events: events.into(),
    };
    if !want_diag {
        return Ok((snapshot, Value::Null));
    }
    let pending = collect_turn_pending_work(id, summary_trajectory_id, steps);
    let executor_tail: Vec<Value> = executors
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(|meta| {
            serde_json::json!({
                "executionId": meta["executionId"],
                "lastStepIdx": meta["lastStepIdx"],
                "terminationReason": meta["terminationReason"],
                "proceededWithAutoContinue": meta["proceededWithAutoContinue"],
            })
        })
        .collect();
    let diagnosis = serde_json::json!({
        "cascadeId": id,
        "trajectoryId": summary["trajectoryId"],
        "stepOffset": step_offset,
        "recordCount": snapshot.record_count,
        "settledCount": snapshot.settled_count,
        "cursor": db_cursor,
        "executorMetadatasTail": executor_tail,
        "cascadeActive": {
            "active": cascade_active,
            "hasActiveSubagents": ctx.has_active_subagents,
            "subagentsAnyActive": subagents_any_active,
            "summaryStatus": summary["status"],
            "summaryStatusActive": summary_status_active,
            "responseStatus": response["status"],
            "responseStatusActive": response_status_active,
            "notFullyIdle": not_fully_idle,
            "hasRunningSubagents": has_running_subagents,
            "waitingStepsNonEmpty": waiting_steps_non_empty,
        },
        "unsettledBackground": {
            "unsettled": unsettled_background,
            "nowMs": ctx.now_ms,
            "lastRootStepMs": pending.last_root_step_ms,
            "hasActiveChildProcesses": ctx.has_active_child_processes,
            "pendingTasksCount": pending.pending_tasks.len(),
            "pendingSubagentsCount": pending.pending_subagents.len(),
        },
        "candidates": candidates,
    });
    Ok((snapshot, diagnosis))
}
