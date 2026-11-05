use super::{ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::services::{
    search::SessionSearchRequest,
    sessions::{SessionBatchExecuteRequest, SessionBatchRequest},
    use_cases::AgentHubUseCases,
};
use axum::{extract::State, http::HeaderMap, Json};

pub(crate) async fn search(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<SessionSearchRequest>,
) -> ApiResponse {
    let result = agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .search(state.codex_paths())
            .search(request)
    })
    .await
    .map_err(anyhow::Error::from)??;
    ok(result)
}

pub(crate) async fn attachment_read(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<agenthub_core::user_message::SessionAttachmentRequest>,
) -> ApiResponse {
    let result = agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .sessions(state.codex_paths())
            .attachment_read(request)
    })
    .await
    .map_err(anyhow::Error::from)??;
    ok(result)
}

pub(crate) async fn bulk_preview(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<SessionBatchRequest>,
) -> ApiResponse {
    let result = agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .sessions(state.codex_paths())
            .bulk_preview(request)
    })
    .await
    .map_err(anyhow::Error::from)??;
    ok(result)
}

pub(crate) async fn bulk_execute(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Json(request): Json<SessionBatchExecuteRequest>,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let detail = serde_json::json!({"provider": request.provider, "operation": request.operation, "count": request.items.len()});
    state.db.record_audit(
        Some(&auth.admin_id),
        "sessions.bulkExecute.requested",
        Some("sessions"),
        None,
        None,
        detail,
    )?;
    let runtime = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        AgentHubUseCases::new(runtime.platform())
            .sessions(runtime.codex_paths())
            .bulk_execute(request)
    })
    .await
    .map_err(anyhow::Error::from)??;
    state.db.record_audit(Some(&auth.admin_id), "sessions.bulkExecute.completed", Some("sessions"), None, None,
        serde_json::json!({"succeeded": result.items.iter().filter(|item| item.status == "succeeded").count(), "blocked": result.items.iter().filter(|item| item.status == "blocked").count(), "failed": result.items.iter().filter(|item| item.status == "failed").count()}))?;
    ok(result)
}
