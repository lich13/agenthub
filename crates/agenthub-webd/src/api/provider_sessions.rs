use super::{linux_update_platform, ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::{
    provider_sessions::{
        ProviderDetailRequest, ProviderListRequest, ProviderRenameRequest, ProviderSessionRequest,
    },
    services::use_cases::AgentHubUseCases,
};

pub(crate) async fn antigravity_list(
    _state: AppState,
    request: ProviderListRequest,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .antigravity()
            .list(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_detail(
    _state: AppState,
    request: ProviderDetailRequest,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .antigravity()
            .detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_rename(
    _state: AppState,
    request: ProviderRenameRequest,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .antigravity()
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_archive(
    _state: AppState,
    request: ProviderSessionRequest,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .antigravity()
            .archive(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn antigravity_restore(
    _state: AppState,
    request: ProviderSessionRequest,
) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .antigravity()
            .restore(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_list(_state: AppState, request: ProviderListRequest) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .dsh()
            .list(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_detail(_state: AppState, request: ProviderDetailRequest) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .dsh()
            .detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_rename(_state: AppState, request: ProviderRenameRequest) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .dsh()
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_archive(_state: AppState, request: ProviderSessionRequest) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .dsh()
            .archive(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_restore(_state: AppState, request: ProviderSessionRequest) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .dsh()
            .restore(&request.session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}
