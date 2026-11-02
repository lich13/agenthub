use super::{api_error, http_update_platform, ok, ApiResponse};
use crate::{auth::require_auth, state::AppState};
use agenthub_core::{
    provider_sessions::{
        ProviderDetailRequest, ProviderListRequest, ProviderRenameRequest, ProviderSessionRequest,
    },
    services::use_cases::AgentHubUseCases,
};
use axum::{extract::State, http::HeaderMap, Json};

pub(crate) async fn antigravity_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderListRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .list(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderDetailRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderRenameRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .archive(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .restore(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderListRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .list(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderDetailRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderRenameRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .archive(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    require_auth(&headers, &state).map_err(|s| api_error(s, "unauthorized"))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .restore(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}
