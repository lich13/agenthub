use super::{api_error, http_update_platform, ok, ApiResponse};
use crate::{auth::require_auth, linux_adapter, state::AppState};
use agenthub_core::services::use_cases::AgentHubUseCases;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use std::collections::HashMap;

pub(crate) async fn list_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    let limit = query.get("limit").and_then(|v| v.parse().ok());
    let platform = http_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .jobs()
        .list(limit)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, &err.to_string()))?;
    ok(linux_adapter::list_jobs_plan(&state, plan)?)
}

pub(crate) async fn job_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    let platform = http_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .jobs()
        .detail(&id)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, &err.to_string()))?;
    match linux_adapter::job_detail_plan(&state, plan)? {
        Some(job) => ok(job),
        None => Err(api_error(StatusCode::NOT_FOUND, "job not found")),
    }
}
