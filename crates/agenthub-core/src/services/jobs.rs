use crate::services::commands;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadStateAction {
    Archive,
    Restore,
    Rename { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadStateActionPlan {
    pub thread_id: String,
    pub action: ThreadStateAction,
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

pub fn thread_state_action_response(plan: &ThreadStateActionPlan) -> ActionResponse {
    match &plan.action {
        ThreadStateAction::Archive => action_ok(
            commands::THREADS_ARCHIVE,
            "thread archived in local Codex state",
            Some(plan.thread_id.clone()),
            None,
            None,
        ),
        ThreadStateAction::Restore => action_ok(
            commands::THREADS_RESTORE,
            "thread restored in local Codex state",
            Some(plan.thread_id.clone()),
            None,
            None,
        ),
        ThreadStateAction::Rename { name } => action_ok(
            commands::THREADS_RENAME,
            "thread renamed in local Codex state",
            Some(plan.thread_id.clone()),
            None,
            Some(json!({"name": name})),
        ),
    }
}

pub(crate) fn non_empty_owned(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(crate) fn required_command_thread_id(thread_id: &str) -> Result<String> {
    non_empty_owned(thread_id).ok_or_else(|| anyhow!("thread_id is required"))
}
