use crate::overview::DesktopState;
use agenthub_core::services::use_cases::{AgentHubUseCases, JobResponse};
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopJobsRequest {
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopJobDetailRequest {
    pub id: String,
    pub include_output: Option<bool>,
}

pub(crate) fn jobs_with_state(
    state: &DesktopState,
    request: DesktopJobsRequest,
) -> Result<Vec<JobResponse>> {
    let use_cases = AgentHubUseCases::new(state.platform());
    let plan = use_cases.jobs().list(request.limit)?;
    Ok(state
        .db
        .list_jobs(plan)?
        .into_iter()
        .map(|job| use_cases.jobs().response(job))
        .collect())
}

pub(crate) fn job_detail_with_state(
    state: &DesktopState,
    request: DesktopJobDetailRequest,
) -> Result<Option<JobResponse>> {
    let use_cases = AgentHubUseCases::new(state.platform());
    let plan = use_cases.jobs().detail(&request.id)?;
    Ok(state
        .db
        .job_with_output(&plan, request.include_output.unwrap_or(true))?
        .map(|job| use_cases.jobs().response(job)))
}
