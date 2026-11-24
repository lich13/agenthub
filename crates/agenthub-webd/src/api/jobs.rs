use super::{api_error, ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::{
    db::JobRecord, platform::PlatformPaths, services::use_cases::AgentHubUseCases,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct JobsListQuery {
    pub(crate) limit: Option<u32>,
}

pub(crate) async fn list_jobs(state: AppState, query: JobsListQuery) -> ApiResponse {
    let plan = AgentHubUseCases::new(state.platform())
        .jobs()
        .list(query.limit)?;
    let platform = state.platform().clone();
    let jobs = state
        .db
        .list_jobs(plan)?
        .into_iter()
        .map(|job| job_response_value(&platform, job))
        .collect::<anyhow::Result<Vec<_>>>()?;
    ok(jobs)
}

pub(crate) async fn job_detail(state: AppState, id: String, include_output: bool) -> ApiResponse {
    let plan = AgentHubUseCases::new(state.platform()).jobs().detail(&id)?;
    let platform = state.platform().clone();
    match state
        .db
        .job_with_output(&plan, include_output)?
        .map(|job| job_response_value(&platform, job))
        .transpose()?
    {
        Some(job) => ok(job),
        None => Err(api_error("job not found")),
    }
}

fn job_response_value(platform: &PlatformPaths, job: JobRecord) -> anyhow::Result<Value> {
    let response = AgentHubUseCases::new(platform).jobs().response(job);
    Ok(serde_json::to_value(response)?)
}
