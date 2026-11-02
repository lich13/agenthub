#![allow(non_snake_case)]
use crate::overview::DesktopState;
use agenthub_core::services::{
    search::{SessionSearchRequest, SessionSearchResponse},
    sessions::{
        SessionBatchExecuteRequest, SessionBatchPreview, SessionBatchRequest, SessionBatchResult,
    },
    use_cases::AgentHubUseCases,
};

#[tauri::command(rename = "sessions.search")]
pub async fn searchSessions(
    state: tauri::State<'_, DesktopState>,
    request: SessionSearchRequest,
) -> Result<SessionSearchResponse, String> {
    let state = state.inner().clone();
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .search(state.codex_paths())
            .search(request)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command(rename = "sessions.attachmentRead")]
pub async fn readSessionAttachment(
    state: tauri::State<'_, DesktopState>,
    request: agenthub_core::user_message::SessionAttachmentRequest,
) -> Result<agenthub_core::user_message::SessionAttachmentResponse, String> {
    let state = state.inner().clone();
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .sessions(state.codex_paths())
            .attachment_read(request)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command(rename = "sessions.bulkPreview")]
pub async fn previewSessionBatch(
    state: tauri::State<'_, DesktopState>,
    request: SessionBatchRequest,
) -> Result<SessionBatchPreview, String> {
    let state = state.inner().clone();
    agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .sessions(state.codex_paths())
            .bulk_preview(request)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command(rename = "sessions.bulkExecute")]
pub async fn executeSessionBatch(
    state: tauri::State<'_, DesktopState>,
    request: SessionBatchExecuteRequest,
) -> Result<SessionBatchResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .sessions(state.codex_paths())
            .bulk_execute(request)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
