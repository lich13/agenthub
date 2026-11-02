use super::{api_error, http_update_platform, ok, ApiResponse};
use crate::{auth::require_auth, state::AppState};
use agenthub_core::{grok::GrokDeleteRequest, services::use_cases::AgentHubUseCases};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct GrokListQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct GrokDetailQuery {
    pub limit: Option<usize>,
    pub before: Option<String>,
}

pub(crate) async fn grok_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GrokListQuery>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .grok()
            .list(query.limit.unwrap_or(50), query.q.as_deref())
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn grok_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<GrokDetailQuery>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform()).grok().page(
            &id,
            query.limit,
            query.before.as_deref(),
        )
    })
    .await
    .map_err(anyhow::Error::from)??)
}

#[derive(Debug, Deserialize)]
pub(crate) struct GrokRenameRequest {
    pub id: String,
    pub title: String,
}

pub(crate) async fn grok_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GrokRenameRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(AgentHubUseCases::new(&http_update_platform())
        .grok()
        .rename(&request.id, &request.title)
        .await?)
}

pub(crate) async fn grok_delete_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .grok()
            .delete_preview(&id)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn grok_delete_execute(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GrokDeleteRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .grok()
            .delete_execute(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}
