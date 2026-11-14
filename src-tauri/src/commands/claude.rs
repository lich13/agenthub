#![allow(non_snake_case)]

use agenthub_core::{
    claude::{ClaudeDeletePreview, ClaudeDeleteRequest, ClaudeDeleteResult},
    platform::PlatformPaths,
    services::use_cases::AgentHubUseCases,
};

#[tauri::command(rename = "claude.ownershipDiagnostics")]
pub async fn claudeOwnershipDiagnostics(
    session_key: String,
) -> Result<agenthub_core::claude::ClaudeOwnershipDiagnostics, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .claude()
            .ownership_diagnostics(&session_key)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "claude.list")]
pub async fn listClaudeSessions(
    q: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<agenthub_core::claude::ClaudeSessionSummary>, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .claude()
            .list(limit.unwrap_or(100), q.as_deref())
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "claude.detail")]
pub async fn getClaudeSession(
    session_key: String,
    agent_id: Option<String>,
    limit: Option<usize>,
    before: Option<String>,
) -> Result<agenthub_core::claude::ClaudeSessionDetail, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .claude()
            .detail(agenthub_core::claude::ClaudeDetailRequest {
                agent_id,
                session_key,
                limit,
                before,
            })
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "claude.rename")]
pub async fn renameClaudeSession(
    session_key: String,
    title: String,
) -> Result<agenthub_core::claude::ClaudeSessionSummary, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .claude()
            .rename(&session_key, &title)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "claude.deletePreview")]
pub async fn previewClaudeSessionDelete(
    session_key: String,
) -> Result<ClaudeDeletePreview, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .claude()
            .delete_preview(&session_key)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "claude.deleteExecute")]
pub async fn deleteClaudeSession(
    request: ClaudeDeleteRequest,
) -> Result<ClaudeDeleteResult, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .claude()
            .delete_execute(request)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}
