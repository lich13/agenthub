use agenthub_core::{
    codex::{set_thread_archived, set_thread_title},
    services::{
        jobs::{self as job_service, ActionResponse},
        use_cases::AgentHubUseCases,
    },
};
use anyhow::Result;

use crate::overview::DesktopState;

pub(crate) fn archive_thread_with_state(
    state: &DesktopState,
    thread_id: &str,
) -> Result<ActionResponse> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .archive(thread_id)?;
    set_thread_archived(&state.codex_paths(), &plan.thread_id, true)?;
    job_service::thread_state_action_response(&plan)
}

pub(crate) fn restore_thread_with_state(
    state: &DesktopState,
    thread_id: &str,
) -> Result<ActionResponse> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .restore(thread_id)?;
    set_thread_archived(&state.codex_paths(), &plan.thread_id, false)?;
    job_service::thread_state_action_response(&plan)
}

pub(crate) async fn rename_thread_with_state(
    state: &DesktopState,
    thread_id: String,
    name: String,
) -> Result<ActionResponse> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .rename(job_service::ThreadRenameRequest { thread_id, name })?;
    let name = plan.name.as_deref().unwrap_or_default();
    set_thread_title(&state.codex_paths(), &plan.thread_id, name).await?;
    job_service::thread_state_action_response(&plan)
}
