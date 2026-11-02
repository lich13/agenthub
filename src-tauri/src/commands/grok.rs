#![allow(non_snake_case)]

use agenthub_core::{
    grok::{GrokDeletePreview, GrokDeleteRequest, GrokDeleteResult},
    platform::PlatformPaths,
    services::use_cases::AgentHubUseCases,
};

#[tauri::command(rename = "grok.list")]
pub async fn listGrokSessions(
    q: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<agenthub_core::grok::GrokSessionSummary>, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .grok()
            .list(limit.unwrap_or(100), q.as_deref())
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}
#[tauri::command(rename = "grok.detail")]
pub async fn getGrokSession(
    id: String,
    limit: Option<usize>,
    before: Option<String>,
) -> Result<agenthub_core::grok::GrokSessionDetail, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .grok()
            .page(&id, limit, before.as_deref())
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}
#[tauri::command(rename = "grok.rename")]
pub async fn renameGrokSession(
    id: String,
    title: String,
) -> Result<agenthub_core::grok::GrokSessionSummary, String> {
    AgentHubUseCases::new(&PlatformPaths::desktop_current())
        .grok()
        .rename(&id, &title)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command(rename = "grok.deletePreview")]
pub async fn previewGrokSessionDelete(id: String) -> Result<GrokDeletePreview, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .grok()
            .delete_preview(&id)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command(rename = "grok.deleteExecute")]
pub async fn deleteGrokSession(request: GrokDeleteRequest) -> Result<GrokDeleteResult, String> {
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&PlatformPaths::desktop_current())
            .grok()
            .delete_execute(request)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}
