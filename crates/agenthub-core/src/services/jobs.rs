use crate::{
    platform::PlatformPaths,
    services::{
        commands,
        system::{require_capability, Capability},
    },
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionResponse {
    pub ok: bool,
    pub available: bool,
    pub command: String,
    pub message: String,
    pub thread_id: Option<String>,
    pub job_id: Option<String>,
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRenameRequest {
    #[serde(alias = "thread_id")]
    pub thread_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStateActionPlan {
    pub required_capability: Capability,
    pub command: String,
    pub thread_id: String,
    pub archived: Option<bool>,
    pub name: Option<String>,
}

pub fn plan_thread_archive_with_capability(
    platform: &PlatformPaths,
    thread_id: &str,
) -> Result<ThreadStateActionPlan> {
    plan_thread_archive_state_action(platform, thread_id, true)
}

pub fn plan_thread_restore_with_capability(
    platform: &PlatformPaths,
    thread_id: &str,
) -> Result<ThreadStateActionPlan> {
    plan_thread_archive_state_action(platform, thread_id, false)
}

pub fn plan_thread_rename_with_capability(
    platform: &PlatformPaths,
    request: ThreadRenameRequest,
) -> Result<ThreadStateActionPlan> {
    require_capability(platform, Capability::ThreadArchiveActions)?;
    let name = non_empty_owned(&request.name).ok_or_else(|| anyhow!("name cannot be empty"))?;
    Ok(ThreadStateActionPlan {
        required_capability: Capability::ThreadArchiveActions,
        command: commands::THREADS_RENAME.to_string(),
        thread_id: required_command_thread_id(Some(&request.thread_id), None)?,
        archived: None,
        name: Some(name),
    })
}

pub fn action_ok(
    command: &str,
    message: &str,
    thread_id: Option<String>,
    job_id: Option<String>,
    data: Option<Value>,
) -> ActionResponse {
    ActionResponse {
        ok: true,
        available: true,
        command: command.to_string(),
        message: message.to_string(),
        thread_id,
        job_id,
        data,
    }
}

pub fn action_unavailable(command: &str, message: &str) -> ActionResponse {
    ActionResponse {
        ok: false,
        available: false,
        command: command.to_string(),
        message: message.to_string(),
        thread_id: None,
        job_id: None,
        data: None,
    }
}

pub fn archive_thread_response(thread_id: String, archived: bool) -> ActionResponse {
    let (command, message) = if archived {
        (
            commands::THREADS_ARCHIVE,
            "thread archived in local Codex state",
        )
    } else {
        (
            commands::THREADS_RESTORE,
            "thread restored in local Codex state",
        )
    };
    action_ok(command, message, Some(thread_id), None, None)
}

pub fn rename_thread_response(thread_id: String, name: &str) -> Result<ActionResponse> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("name cannot be empty"));
    }
    Ok(action_ok(
        commands::THREADS_RENAME,
        "thread renamed in local Codex state",
        Some(thread_id),
        None,
        Some(json!({"name": name})),
    ))
}

pub fn thread_state_action_response(plan: &ThreadStateActionPlan) -> Result<ActionResponse> {
    match plan.command.as_str() {
        commands::THREADS_ARCHIVE => Ok(archive_thread_response(plan.thread_id.clone(), true)),
        commands::THREADS_RESTORE => Ok(archive_thread_response(plan.thread_id.clone(), false)),
        commands::THREADS_RENAME => rename_thread_response(
            plan.thread_id.clone(),
            plan.name.as_deref().unwrap_or_default(),
        ),
        _ => Err(anyhow!("unsupported thread state action: {}", plan.command)),
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn non_empty_owned(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn required_command_thread_id(
    request_thread_id: Option<&str>,
    message_thread_id: Option<&str>,
) -> Result<String> {
    non_empty(request_thread_id)
        .or_else(|| non_empty(message_thread_id))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("thread_id is required"))
}

fn plan_thread_archive_state_action(
    platform: &PlatformPaths,
    thread_id: &str,
    archived: bool,
) -> Result<ThreadStateActionPlan> {
    require_capability(platform, Capability::ThreadArchiveActions)?;
    Ok(ThreadStateActionPlan {
        required_capability: Capability::ThreadArchiveActions,
        command: if archived {
            commands::THREADS_ARCHIVE
        } else {
            commands::THREADS_RESTORE
        }
        .to_string(),
        thread_id: required_command_thread_id(Some(thread_id), None)?,
        archived: Some(archived),
        name: None,
    })
}
