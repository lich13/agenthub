use crate::overview::DesktopState;
use agenthub_core::services::use_cases::{AgentHubUseCases, JobResponse};
use anyhow::Result;

pub(crate) fn jobs_with_state(
    state: &DesktopState,
    limit: Option<u32>,
) -> Result<Vec<JobResponse>> {
    let use_cases = AgentHubUseCases::new(state.platform());
    let plan = use_cases.jobs().list(limit)?;
    Ok(state
        .db
        .list_jobs(plan)?
        .into_iter()
        .map(|job| use_cases.jobs().response(job))
        .collect())
}

pub(crate) fn job_detail_with_state(
    state: &DesktopState,
    id: &str,
    include_output: Option<bool>,
) -> Result<Option<JobResponse>> {
    let use_cases = AgentHubUseCases::new(state.platform());
    let plan = use_cases.jobs().detail(id)?;
    Ok(state
        .db
        .job_with_output(&plan, include_output.unwrap_or(true))?
        .map(|job| use_cases.jobs().response(job)))
}
