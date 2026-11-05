use crate::overview::DesktopState;
use agenthub_core::{
    db::JobRecord, services::use_cases::AgentHubUseCases, update::JobFailureAnalysis,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopJobResponse {
    #[serde(flatten)]
    pub job: JobRecord,
    pub failure_analysis: Option<JobFailureAnalysis>,
    pub analysis: Option<String>,
    pub explanation: Option<String>,
}

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
) -> Result<Vec<DesktopJobResponse>> {
    let use_cases = AgentHubUseCases::new(state.platform());
    let plan = use_cases.jobs().list(request.limit)?;
    Ok(state
        .db
        .list_jobs(plan.limit)?
        .into_iter()
        .map(|job| use_cases.jobs().response(job).into())
        .collect())
}

pub(crate) fn job_detail_with_state(
    state: &DesktopState,
    request: DesktopJobDetailRequest,
) -> Result<Option<DesktopJobResponse>> {
    let use_cases = AgentHubUseCases::new(state.platform());
    let plan = use_cases.jobs().detail(&request.id)?;
    Ok(state
        .db
        .job_with_output(&plan.job_id, request.include_output.unwrap_or(true))?
        .map(|job| use_cases.jobs().response(job).into()))
}

impl From<agenthub_core::services::use_cases::JobResponse> for DesktopJobResponse {
    fn from(value: agenthub_core::services::use_cases::JobResponse) -> Self {
        Self {
            job: value.job,
            failure_analysis: value.failure_analysis,
            analysis: value.analysis,
            explanation: value.explanation,
        }
    }
}
