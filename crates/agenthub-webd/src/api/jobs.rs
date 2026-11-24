use super::ApiErrorKind;
use super::{api_error, linux_update_platform, ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::{
    db::JobRecord, platform::PlatformPaths, services::use_cases::AgentHubUseCases,
};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) async fn list_jobs(state: AppState, query: HashMap<String, String>) -> ApiResponse {
    let limit = query.get("limit").and_then(|v| v.parse().ok());
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .jobs()
        .list(limit)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    let jobs = state
        .db
        .list_jobs(plan)?
        .into_iter()
        .map(job_response_value)
        .collect::<anyhow::Result<Vec<_>>>()?;
    ok(jobs)
}

pub(crate) async fn job_detail(state: AppState, id: String, include_output: bool) -> ApiResponse {
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .jobs()
        .detail(&id)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    match state
        .db
        .job_with_output(&plan, include_output)?
        .map(job_response_value)
        .transpose()?
    {
        Some(job) => ok(job),
        None => Err(api_error(ApiErrorKind::NotFound, "job not found")),
    }
}

fn job_response_value(job: JobRecord) -> anyhow::Result<Value> {
    let response = AgentHubUseCases::new(&PlatformPaths::linux_server())
        .jobs()
        .response(job);
    Ok(serde_json::to_value(response)?)
}
