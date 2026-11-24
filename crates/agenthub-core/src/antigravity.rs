//! Pure projection of the native Antigravity App's read-only trajectory API.
pub mod sessions;
pub mod subagents;
mod trajectory;
use crate::native_probe::{self, NativeProvider, NativeStreamSnapshot, NativeTurnEvent};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use trajectory::evaluate_trajectory;

pub fn identity(id: &str, summary: &Value) -> Result<String> {
    ensure!(
        uuid::Uuid::parse_str(id).is_ok(),
        "invalid Antigravity identity"
    );
    let trajectory = summary["trajectoryId"]
        .as_str()
        .context("missing Antigravity trajectory identity")?;
    let created = summary["createdTime"]
        .as_str()
        .context("missing Antigravity creation time")?;
    let root = summary["trajectoryMetadata"]["rootConversationId"]
        .as_str()
        .context("missing Antigravity conversation origin")?;
    ensure!(
        !trajectory.is_empty() && native_probe::timestamp(&summary["createdTime"]).is_some(),
        "invalid Antigravity source identity"
    );
    ensure!(
        root.is_empty() || root == id,
        "Antigravity subagent is not a main conversation"
    );
    Ok(hex::encode(Sha256::digest(format!(
        "{id}\0{trajectory}\0{created}"
    ))))
}
fn safe_text(value: &Value) -> String {
    crate::security::redact_output(value.as_str().unwrap_or(""))
        .chars()
        .take(1200)
        .collect()
}
fn execution(step: &Value) -> &str {
    step["metadata"]["executionId"].as_str().unwrap_or("")
}
fn settled(step: &Value) -> bool {
    matches!(
        step["status"].as_str(),
        Some(
            "CORTEX_STEP_STATUS_DONE"
                | "CORTEX_STEP_STATUS_ERROR"
                | "CORTEX_STEP_STATUS_CANCELED"
                | "CORTEX_STEP_STATUS_CLEARED"
                | "CORTEX_STEP_STATUS_INTERRUPTED"
                | "CORTEX_STEP_STATUS_HALTED"
        )
    )
}
fn interaction(
    step: &Value,
    index: usize,
    fallback_timestamp_ms: Option<i64>,
) -> Result<Option<NativeTurnEvent>> {
    if step["status"] != "CORTEX_STEP_STATUS_WAITING" {
        return Ok(None);
    }
    let request = step["requestedInteraction"]
        .as_object()
        .filter(|r| !r.is_empty());
    let top_level_questions = (step["type"] == "CORTEX_STEP_TYPE_ASK_QUESTION")
        .then(|| step["askQuestion"]["questions"].as_array())
        .flatten();
    if request.is_none() && top_level_questions.is_none() {
        return Ok(None);
    }
    let (kind, body) = if let Some(questions) = request
        .and_then(|r| r.get("askQuestion"))
        .and_then(|v| v["questions"].as_array())
        .or(top_level_questions)
    {
        let texts = questions
            .iter()
            .filter_map(|q| q["question"].as_str())
            .filter(|q| !q.trim().is_empty())
            .collect::<Vec<_>>();
        let body = if texts.is_empty() {
            "需要回复，请打开 Antigravity 处理。".into()
        } else {
            texts.join("\n")
        };
        ("reply_needed", body)
    } else if request
        .is_some_and(|r| r.contains_key("elicitation") || r.contains_key("interaction"))
    {
        ("reply_needed", "需要回复，请打开 Antigravity 处理。".into())
    } else if request.is_some_and(|r| {
        [
            "permission",
            "filePermission",
            "approvalInteraction",
            "runCommand",
            "sendCommandInput",
            "readUrlContent",
            "openBrowserUrl",
            "confirmBrowserSetup",
            "openBrowserSetup",
            "runExtensionCode",
            "executeBrowserJavascript",
            "captureBrowserScreenshot",
            "clickBrowserPixel",
            "browserAction",
            "mcp",
            "deploy",
        ]
        .iter()
        .any(|k| r.contains_key(*k))
    }) {
        (
            "authorization",
            "操作需要授权，请打开 Antigravity 处理。".into(),
        )
    } else {
        anyhow::bail!("unsupported Antigravity interaction");
    };
    let execution = execution(step);
    ensure!(
        !execution.is_empty(),
        "missing Antigravity execution identity"
    );
    let proof_source = match request {
        Some(req) => serde_json::to_vec(req)?,
        None => serde_json::to_vec(&step["askQuestion"])?,
    };
    let proof = hex::encode(Sha256::digest(proof_source));
    let completed = step["completedInteractions"].as_array().map_or(0, Vec::len);
    Ok(Some(NativeTurnEvent {
        prompt_id: None,
        position: index as u64 + 1,
        turn_id: format!("{execution}:step:{index}:request:{completed}:{proof}"),
        kind: kind.into(),
        body: safe_text(&Value::String(body)),
        timestamp_ms: native_probe::timestamp(&step["metadata"]["createdAt"])
            .or_else(|| native_probe::timestamp(&step["metadata"]["startedAt"]))
            .or_else(|| native_probe::timestamp(&step["metadata"]["viewableAt"]))
            .or(fallback_timestamp_ms)
            .context("missing Antigravity interaction time")?,
    }))
}

fn parse_json_or_value(raw: &Value) -> Value {
    match raw {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    }
}

fn step_tool_name(step: &Value) -> &str {
    step["generic"]["name"]
        .as_str()
        .or_else(|| step["generic"]["toolName"].as_str())
        .or_else(|| step["toolCall"]["name"].as_str())
        .or_else(|| step["metadata"]["toolCall"]["name"].as_str())
        .unwrap_or("")
}

fn step_tool_args(step: &Value) -> Value {
    for candidate in [
        &step["generic"]["arguments"],
        &step["generic"]["argumentsJson"],
        &step["generic"]["args"],
        &step["toolCall"]["arguments"],
        &step["toolCall"]["argumentsJson"],
        &step["toolCall"]["args"],
        &step["metadata"]["toolCall"]["arguments"],
        &step["metadata"]["toolCall"]["argumentsJson"],
    ] {
        if !candidate.is_null() {
            let parsed = parse_json_or_value(candidate);
            if !parsed.is_null() {
                return parsed;
            }
        }
    }
    Value::Null
}

fn step_is_daemon_task(step: &Value, args: &Value) -> bool {
    step["taskDetails"]["isDaemon"].as_bool() == Some(true)
        || step["runCommand"]["isDaemon"].as_bool() == Some(true)
        || step["schedule"]["isDaemon"].as_bool() == Some(true)
        || args["IsDaemon"].as_bool() == Some(true)
        || args["isDaemon"].as_bool() == Some(true)
}

pub(crate) fn collect_spawned_subagent_ids(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(cid) = map
                .get("conversationId")
                .or_else(|| map.get("conversation_id"))
                .and_then(Value::as_str)
            {
                if uuid::Uuid::parse_str(cid).is_ok() {
                    out.insert(cid.to_owned());
                }
            }
            for child in map.values() {
                collect_spawned_subagent_ids(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_spawned_subagent_ids(child, out);
            }
        }
        Value::String(raw) => {
            let mut offset = 0usize;
            while offset < raw.len() {
                let Some(rel) = raw[offset..].find(['{', '[']) else {
                    break;
                };
                let start = offset + rel;
                let mut stream =
                    serde_json::Deserializer::from_str(&raw[start..]).into_iter::<Value>();
                if let Some(Ok(parsed)) = stream.next() {
                    let consumed = stream.byte_offset().max(1);
                    collect_spawned_subagent_ids(&parsed, out);
                    offset = start + consumed;
                } else {
                    let step = raw[start..].chars().next().map_or(1, char::len_utf8);
                    offset = start + step;
                }
            }
        }
        _ => {}
    }
}

fn cascade_status_is_active(status: Option<&str>) -> bool {
    matches!(
        status,
        Some(
            "CASCADE_RUN_STATUS_RUNNING"
                | "CASCADE_RUN_STATUS_BUSY"
                | "CASCADE_RUN_STATUS_WAITING"
                | "CASCADE_RUN_STATUS_CANCELING"
        )
    )
}

pub const BACKGROUND_HANDOFF_GRACE_MS: i64 = 15_000;
pub const UNTRACKED_TASK_QUIESCENCE_MS: i64 = 60_000;
pub const ACTIVE_BACKGROUND_TASK_CAP_MS: i64 = 4 * 3_600 * 1_000;

#[derive(Debug, Clone, Default)]
pub struct SubagentLiveness {
    pub active: bool,
    pub unsettled_background: bool,
    pub last_modified_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskLiveness {
    pub active: bool,
    pub last_activity_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct TrajectoryLivenessContext {
    pub has_active_subagents: bool,
    pub has_active_child_processes: bool,
    pub now_ms: Option<i64>,
    pub subagents: BTreeMap<String, SubagentLiveness>,
    pub tasks: BTreeMap<String, TaskLiveness>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedCommandSpec {
    pub spawn_ms: i64,
    pub command_line: String,
}

#[derive(Debug, Clone, Default)]
struct PendingTaskRecord {
    spawn_ms: Option<i64>,
    expected_deadline_ms: Option<i64>,
    log_uri: Option<String>,
    command_line: Option<String>,
    step_running: bool,
}

fn step_activity_timestamp_ms(step: &Value) -> Option<i64> {
    native_probe::timestamp(&step["metadata"]["completedAt"])
        .or_else(|| native_probe::timestamp(&step["metadata"]["finishedGeneratingAt"]))
        .or_else(|| native_probe::timestamp(&step["metadata"]["createdAt"]))
}

fn step_is_foreign(id: &str, summary_trajectory_id: &str, step: &Value) -> bool {
    let source = &step["metadata"]["sourceTrajectoryStepInfo"];
    source["trajectoryId"]
        .as_str()
        .is_some_and(|orig| !summary_trajectory_id.is_empty() && orig != summary_trajectory_id)
        || source["cascadeId"]
            .as_str()
            .is_some_and(|orig| !orig.is_empty() && orig != id)
}

pub struct TurnPendingWork {
    last_root_step_ms: Option<i64>,
    pending_tasks: BTreeMap<String, PendingTaskRecord>,
    pending_subagents: BTreeMap<String, Option<i64>>,
}

fn system_message_sender(step: &Value) -> Option<&str> {
    step["systemMessage"]["agentMessage"]["sender"]
        .as_str()
        .or_else(|| step["systemMessage"]["sender"].as_str())
        .filter(|s| !s.is_empty())
}

pub fn collect_turn_pending_work(
    id: &str,
    summary_trajectory_id: &str,
    steps: &[Value],
) -> TurnPendingWork {
    let last_user_input_idx = steps
        .iter()
        .enumerate()
        .rev()
        .find(|(_, step)| {
            !step_is_foreign(id, summary_trajectory_id, step)
                && step["type"] == "CORTEX_STEP_TYPE_USER_INPUT"
        })
        .map(|(idx, _)| idx);

    let parent_sender = if last_user_input_idx.is_none() {
        steps
            .iter()
            .filter(|step| !step_is_foreign(id, summary_trajectory_id, step))
            .take_while(|step| {
                step["taskDetails"]["id"].as_str().is_none_or(str::is_empty)
                    && step_tool_name(step) != "invoke_subagent"
                    && step["invokeSubagent"].is_null()
            })
            .find_map(|step| {
                (step["type"] == "CORTEX_STEP_TYPE_SYSTEM_MESSAGE")
                    .then(|| system_message_sender(step))
                    .flatten()
            })
    } else {
        None
    };

    let turn_start = if let Some(idx) = last_user_input_idx {
        idx + 1
    } else if let Some(parent) = parent_sender {
        steps
            .iter()
            .enumerate()
            .rev()
            .find(|(_, step)| {
                !step_is_foreign(id, summary_trajectory_id, step)
                    && step["type"] == "CORTEX_STEP_TYPE_SYSTEM_MESSAGE"
                    && system_message_sender(step) == Some(parent)
            })
            .map_or(0, |(idx, _)| idx + 1)
    } else {
        0
    };

    let mut last_root_step_ms: Option<i64> = None;
    let mut pending_tasks: BTreeMap<String, PendingTaskRecord> = BTreeMap::new();
    let mut pending_subagents: BTreeMap<String, Option<i64>> = BTreeMap::new();

    for step in &steps[turn_start..] {
        if step_is_foreign(id, summary_trajectory_id, step) {
            continue;
        }
        let step_ms = step_activity_timestamp_ms(step);
        if let Some(ms) = step_ms {
            last_root_step_ms = Some(last_root_step_ms.map_or(ms, |prev| prev.max(ms)));
        }
        if step["type"] == "CORTEX_STEP_TYPE_SYSTEM_MESSAGE" {
            if let Some(sender) = system_message_sender(step) {
                pending_tasks.remove(sender);
                pending_subagents.remove(sender);
            }
            continue;
        }
        let tool_name = step_tool_name(step);
        let args = step_tool_args(step);
        if let Some(task_id) = step["taskDetails"]["id"].as_str().filter(|s| !s.is_empty()) {
            if !step_is_daemon_task(step, &args) {
                let duration_secs = args["DurationSeconds"]
                    .as_i64()
                    .or_else(|| args["durationSeconds"].as_i64())
                    .or_else(|| step["schedule"]["durationSeconds"].as_i64())
                    .filter(|&d| d > 0);
                let spawn_ms = native_probe::timestamp(&step["taskDetails"]["movedToBackgroundAt"])
                    .or(step_ms);
                let expected_deadline_ms = match (spawn_ms, duration_secs) {
                    (Some(start), Some(secs)) => {
                        Some(start.saturating_add(secs.saturating_mul(1000)))
                    }
                    _ => None,
                };
                let log_uri = step["taskDetails"]["logUri"]
                    .as_str()
                    .or_else(|| step["taskDetails"]["logAbsoluteUri"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let command_line = args["CommandLine"]
                    .as_str()
                    .or_else(|| args["commandLine"].as_str())
                    .or_else(|| step["runCommand"]["commandLine"].as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                pending_tasks.insert(
                    task_id.to_owned(),
                    PendingTaskRecord {
                        spawn_ms,
                        expected_deadline_ms,
                        log_uri,
                        command_line,
                        step_running: !settled(step),
                    },
                );
            }
        }
        if tool_name == "manage_task" && settled(step) {
            let action = args["Action"]
                .as_str()
                .or_else(|| args["action"].as_str())
                .unwrap_or("");
            if action == "kill" {
                if let Some(tid) = args["TaskId"].as_str().or_else(|| args["taskId"].as_str()) {
                    pending_tasks.remove(tid);
                }
            }
        } else if (tool_name == "invoke_subagent" || !step["invokeSubagent"].is_null())
            && settled(step)
        {
            let mut spawned = BTreeSet::new();
            collect_spawned_subagent_ids(&step["generic"]["result"], &mut spawned);
            collect_spawned_subagent_ids(&step["generic"]["output"], &mut spawned);
            collect_spawned_subagent_ids(&step["invokeSubagent"], &mut spawned);
            collect_spawned_subagent_ids(&step["metadata"]["toolCall"], &mut spawned);
            for cid in spawned {
                pending_subagents.insert(cid, step_ms);
            }
        } else if tool_name == "send_message" && settled(step) {
            if let Some(recipient) = args["Recipient"]
                .as_str()
                .or_else(|| args["recipient"].as_str())
                .filter(|cid| {
                    uuid::Uuid::parse_str(cid).is_ok() && *cid != id && Some(*cid) != parent_sender
                })
            {
                pending_subagents.insert(recipient.to_owned(), step_ms);
            }
        } else if tool_name == "manage_subagents" && settled(step) {
            let action = args["Action"]
                .as_str()
                .or_else(|| args["action"].as_str())
                .unwrap_or("");
            if action == "kill_all" {
                pending_subagents.clear();
            } else if action == "kill" {
                if let Some(ids) = args["ConversationIds"]
                    .as_array()
                    .or_else(|| args["conversationIds"].as_array())
                {
                    for cid in ids.iter().filter_map(Value::as_str) {
                        pending_subagents.remove(cid);
                    }
                }
            }
        }
    }

    TurnPendingWork {
        last_root_step_ms,
        pending_tasks,
        pending_subagents,
    }
}

pub fn collect_pending_subagent_ids(
    id: &str,
    summary_trajectory_id: &str,
    steps: &[Value],
) -> BTreeSet<String> {
    collect_turn_pending_work(id, summary_trajectory_id, steps)
        .pending_subagents
        .into_keys()
        .collect()
}

pub fn collect_pending_task_logs(
    id: &str,
    summary_trajectory_id: &str,
    steps: &[Value],
) -> BTreeMap<String, String> {
    let work = collect_turn_pending_work(id, summary_trajectory_id, steps);
    work.pending_tasks
        .into_iter()
        .filter_map(|(task_id, rec)| rec.log_uri.map(|uri| (task_id, uri)))
        .collect()
}

pub fn collect_untracked_task_commands(
    id: &str,
    summary_trajectory_id: &str,
    steps: &[Value],
    tracked_tasks: &BTreeMap<String, TaskLiveness>,
) -> Vec<UntrackedCommandSpec> {
    let work = collect_turn_pending_work(id, summary_trajectory_id, steps);
    let mut specs = Vec::new();
    for (task_id, rec) in &work.pending_tasks {
        if tracked_tasks.get(task_id).is_some_and(|t| t.active)
            || rec.expected_deadline_ms.is_some()
        {
            continue;
        }
        let Some(spawn_ms) = rec.spawn_ms.or(work.last_root_step_ms).filter(|&ms| ms > 0) else {
            continue;
        };
        let Some(command_line) = rec.command_line.as_ref().filter(|cmd| !cmd.is_empty()) else {
            continue;
        };
        specs.push(UntrackedCommandSpec {
            spawn_ms,
            command_line: command_line.clone(),
        });
    }
    specs
}

pub(crate) fn step_requests_artifact_feedback(step: &Value) -> bool {
    let meta = &step["codeAction"]["artifactMetadata"];
    if meta["requestFeedback"].as_bool() == Some(true)
        || meta["RequestFeedback"].as_bool() == Some(true)
    {
        return true;
    }
    let args = step_tool_args(step);
    args["ArtifactMetadata"]["RequestFeedback"].as_bool() == Some(true)
        || args["artifactMetadata"]["requestFeedback"].as_bool() == Some(true)
}

fn step_artifact_feedback_summary(step: &Value) -> Option<&str> {
    step["codeAction"]["artifactMetadata"]["summary"]
        .as_str()
        .or_else(|| step["codeAction"]["artifactMetadata"]["Summary"].as_str())
        .filter(|s| !s.trim().is_empty())
}

pub fn has_unsettled_background_work(
    id: &str,
    summary_trajectory_id: &str,
    steps: &[Value],
    ctx: &TrajectoryLivenessContext,
) -> bool {
    collect_turn_pending_work(id, summary_trajectory_id, steps).is_unsettled(
        id,
        &ctx.subagents,
        &ctx.tasks,
        ctx.has_active_child_processes,
        ctx.now_ms,
    )
}

impl TurnPendingWork {
    /// Evaluate after discovering descendants, without retaining their raw steps.
    pub fn is_unsettled(
        &self,
        id: &str,
        subagents: &BTreeMap<String, SubagentLiveness>,
        tasks: &BTreeMap<String, TaskLiveness>,
        has_active_child_processes: bool,
        now_ms: Option<i64>,
    ) -> bool {
        let Self {
            last_root_step_ms,
            pending_tasks,
            pending_subagents,
        } = self;

        if pending_tasks.is_empty() && pending_subagents.is_empty() {
            return false;
        }
        let Some(now_ms) = now_ms else {
            return true;
        };

        let max_subagent_modified_ms = subagents
            .iter()
            .filter(|(k, _)| k.as_str() != id)
            .filter_map(|(_, s)| s.last_modified_ms)
            .max();

        for (cid, spawn_ms) in pending_subagents {
            let sub = subagents.get(cid);
            if sub.is_some_and(|s| s.active || s.unsettled_background) {
                return true;
            }
            let anchor_ms = [
                *last_root_step_ms,
                *spawn_ms,
                sub.and_then(|s| s.last_modified_ms),
                max_subagent_modified_ms,
            ]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(now_ms);
            if now_ms.saturating_sub(anchor_ms) < BACKGROUND_HANDOFF_GRACE_MS {
                return true;
            }
        }

        for (task_id, rec) in pending_tasks {
            let task = tasks.get(task_id);
            let anchor_ms = [
                *last_root_step_ms,
                rec.spawn_ms,
                rec.expected_deadline_ms,
                task.and_then(|t| t.last_activity_ms),
            ]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(now_ms);
            let grace = if rec.step_running || task.is_some_and(|t| t.active) {
                ACTIVE_BACKGROUND_TASK_CAP_MS
            } else if (task.is_some() || rec.expected_deadline_ms.is_some())
                && !has_active_child_processes
            {
                BACKGROUND_HANDOFF_GRACE_MS
            } else {
                UNTRACKED_TASK_QUIESCENCE_MS
            };
            if now_ms.saturating_sub(anchor_ms) < grace {
                return true;
            }
        }

        false
    }
}

pub fn snapshot(id: &str, summary: &Value, response: &Value) -> Result<NativeStreamSnapshot> {
    snapshot_with_context(id, summary, response, &TrajectoryLivenessContext::default())
}

pub fn snapshot_with_context(
    id: &str,
    summary: &Value,
    response: &Value,
    ctx: &TrajectoryLivenessContext,
) -> Result<NativeStreamSnapshot> {
    Ok(evaluate_trajectory(id, summary, response, ctx, None, false)?.0)
}

pub fn diagnose_trajectory(
    id: &str,
    summary: &Value,
    response: &Value,
    ctx: &TrajectoryLivenessContext,
    db_cursor: Option<u64>,
) -> Result<Value> {
    Ok(evaluate_trajectory(id, summary, response, ctx, db_cursor, true)?.1)
}

#[cfg(test)]
mod tests;
