use super::{http_update_platform, ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::{
    provider_sessions::{
        ProviderDetailRequest, ProviderListRequest, ProviderRenameRequest, ProviderSessionRequest,
    },
    services::use_cases::AgentHubUseCases,
};
use axum::{extract::State, http::HeaderMap, Json};

pub(crate) async fn antigravity_list(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderListRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .list(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_detail(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderDetailRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_rename(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderRenameRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_archive(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .archive(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_restore(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .antigravity()
            .restore(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_list(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderListRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .list(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_detail(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderDetailRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_rename(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderRenameRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_archive(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .archive(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_restore(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<ProviderSessionRequest>,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&http_update_platform())
            .dsh()
            .restore(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}
