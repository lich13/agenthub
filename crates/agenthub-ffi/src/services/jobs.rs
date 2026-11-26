use crate::overview::DesktopState;
use agenthub_core::services::use_cases::{AgentHubUseCases, JobResponse};
use anyhow::Result;

pub(crate) fn jobs_with_state(
    state: &DesktopState,
    limit: Option<u32>,
) -> Result<Vec<JobResponse>> {
    AgentHubUseCases::new(state.platform())
        .jobs()
        .list_from_db(&state.db, limit)
}

pub(crate) fn job_detail_with_state(
    state: &DesktopState,
    id: &str,
    include_output: Option<bool>,
) -> Result<Option<JobResponse>> {
    AgentHubUseCases::new(state.platform())
        .jobs()
        .detail_from_db(&state.db, id, include_output)
}
