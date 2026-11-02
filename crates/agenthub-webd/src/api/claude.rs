use super::{api_error, http_update_platform, ok, ApiResponse};
use crate::{auth::require_auth, state::AppState};
use agenthub_core::{claude::ClaudeDeleteRequest, services::use_cases::AgentHubUseCases};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ClaudeListQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

pub(crate) async fn claude_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ClaudeListQuery>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .claude()
            .list(query.limit.unwrap_or(100), query.q.as_deref())
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn claude_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<agenthub_core::claude::ClaudeDetailRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .claude()
            .detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeRenameRequest {
    pub session_key: String,
    pub title: String,
}

pub(crate) async fn claude_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ClaudeRenameRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .claude()
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn claude_delete_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_key): Path<String>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .claude()
            .delete_preview(&session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn claude_delete_execute(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ClaudeDeleteRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .claude()
            .delete_execute(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}
