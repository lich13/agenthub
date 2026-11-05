use super::ApiErrorKind;
use super::{api_error, linux_update_platform, ok, ApiResponse};
use crate::{linux_adapter, state::AppState};
use agenthub_core::services::use_cases::AgentHubUseCases;
use std::collections::HashMap;

pub(crate) async fn list_jobs(state: AppState, query: HashMap<String, String>) -> ApiResponse {
    let limit = query.get("limit").and_then(|v| v.parse().ok());
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .jobs()
        .list(limit)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    ok(linux_adapter::list_jobs_plan(&state, plan)?)
}

pub(crate) async fn job_detail(state: AppState, id: String, include_output: bool) -> ApiResponse {
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .jobs()
        .detail(&id)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    match linux_adapter::job_detail_plan(&state, plan, include_output)? {
        Some(job) => ok(job),
        None => Err(api_error(ApiErrorKind::NotFound, "job not found")),
    }
}
