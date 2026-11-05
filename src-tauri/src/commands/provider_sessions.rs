#![allow(non_snake_case)]

use agenthub_core::{
    platform::PlatformPaths,
    provider_sessions::{
        ProviderDetailRequest, ProviderListRequest, ProviderRenameRequest, ProviderSessionDetail,
        ProviderSessionRequest, ProviderSessionSummary,
    },
    services::use_cases::AgentHubUseCases,
};

#[tauri::command(rename = "antigravity.list")]
pub async fn listAntigravitySessions(
    request: Option<ProviderListRequest>,
) -> Result<Vec<ProviderSessionSummary>, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .antigravity()
            .list(request.unwrap_or_default())
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "antigravity.detail")]
pub async fn getAntigravitySession(
    request: ProviderDetailRequest,
) -> Result<ProviderSessionDetail, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .antigravity()
            .detail(request)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "antigravity.rename")]
pub async fn renameAntigravitySession(
    request: ProviderRenameRequest,
) -> Result<ProviderSessionSummary, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .antigravity()
            .rename(&request.session_key, &request.title)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "antigravity.archive")]
pub async fn archiveAntigravitySession(
    request: ProviderSessionRequest,
) -> Result<ProviderSessionSummary, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .antigravity()
            .archive(&request.session_key)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "antigravity.restore")]
pub async fn restoreAntigravitySession(
    request: ProviderSessionRequest,
) -> Result<ProviderSessionSummary, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .antigravity()
            .restore(&request.session_key)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "dsh.list")]
pub async fn listDshSessions(
    request: Option<ProviderListRequest>,
) -> Result<Vec<ProviderSessionSummary>, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .dsh()
            .list(request.unwrap_or_default())
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "dsh.detail")]
pub async fn getDshSession(
    request: ProviderDetailRequest,
) -> Result<ProviderSessionDetail, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .dsh()
            .detail(request)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "dsh.rename")]
pub async fn renameDshSession(
    request: ProviderRenameRequest,
) -> Result<ProviderSessionSummary, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .dsh()
            .rename(&request.session_key, &request.title)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "dsh.archive")]
pub async fn archiveDshSession(
    request: ProviderSessionRequest,
) -> Result<ProviderSessionSummary, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .dsh()
            .archive(&request.session_key)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "dsh.restore")]
pub async fn restoreDshSession(
    request: ProviderSessionRequest,
) -> Result<ProviderSessionSummary, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .dsh()
            .restore(&request.session_key)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "grokbot.list")]
pub async fn listGrokBotSessions(
    request: Option<ProviderListRequest>,
) -> Result<Vec<ProviderSessionSummary>, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .grokbot()
            .list(request.unwrap_or_default())
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "grokbot.detail")]
pub async fn getGrokBotSession(
    request: ProviderDetailRequest,
) -> Result<ProviderSessionDetail, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .grokbot()
            .detail(request)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}
