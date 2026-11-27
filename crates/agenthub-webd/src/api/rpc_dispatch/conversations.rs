use super::*;
use agenthub_core::{
    services::{
        search::SessionSearchRequest,
        sessions::{SessionBatchExecuteRequest, SessionBatchRequest},
        use_cases::AgentHubUseCases,
    },
    user_message::SessionAttachmentRequest,
};

pub(super) async fn handle_threads_subagent_detail(state: AppState, args: Value) -> ApiResponse {
    crate::api::threads::subagent_detail(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_sessions_attachment_read(state: AppState, args: Value) -> ApiResponse {
    let request: SessionAttachmentRequest = rpc_wrapped_payload(&args, &["request"])?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .sessions(state.codex_paths())?
            .attachment_read(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(super) async fn handle_sessions_search(state: AppState, args: Value) -> ApiResponse {
    let request: SessionSearchRequest = rpc_wrapped_payload(&args, &["request"])?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .search(state.codex_paths())?
            .search(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(super) async fn handle_sessions_bulk_preview(state: AppState, args: Value) -> ApiResponse {
    let request: SessionBatchRequest = rpc_wrapped_payload(&args, &["request"])?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .sessions(state.codex_paths())?
            .bulk_preview(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(super) async fn handle_sessions_bulk_execute(state: AppState, args: Value) -> ApiResponse {
    let request: SessionBatchExecuteRequest = rpc_wrapped_payload(&args, &["request"])?;
    let detail = serde_json::json!({
        "provider": request.provider,
        "operation": request.operation,
        "count": request.items.len(),
    });
    state.db.record_audit(
        Some(crate::state::ADMIN_ID),
        "sessions.bulkExecute.requested",
        Some("sessions"),
        None,
        detail,
    )?;
    let provider = request.provider;
    let operation = request.operation;
    let runtime = state.clone();
    let result = agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(runtime.platform())
            .sessions(runtime.codex_paths())?
            .bulk_execute(request)
    })
    .await
    .map_err(anyhow::Error::from)??;
    let _ = state
        .db
        .prune_cursors_after_batch_execute(provider, operation, &result);
    state.db.record_audit(
        Some(crate::state::ADMIN_ID),
        "sessions.bulkExecute.completed",
        Some("sessions"),
        None,
        serde_json::json!({
            "succeeded": result.items.iter().filter(|item| item.status == "succeeded").count(),
            "blocked": result.items.iter().filter(|item| item.status == "blocked").count(),
            "failed": result.items.iter().filter(|item| item.status == "failed").count(),
        }),
    )?;
    ok(result)
}

pub(super) async fn handle_threads_list(state: AppState, args: Value) -> ApiResponse {
    crate::api::list_threads(state, rpc_payload(&args)?).await
}

pub(super) async fn handle_threads_detail(state: AppState, args: Value) -> ApiResponse {
    thread_detail(
        state,
        rpc_required_string(&args, "id")?,
        rpc_nested_payload_or_empty(&args, "options")?,
    )
    .await
}

pub(super) async fn handle_threads_blocks(state: AppState, args: Value) -> ApiResponse {
    thread_blocks(
        state,
        rpc_required_string(&args, "id")?,
        rpc_nested_payload_or_empty(&args, "options")?,
    )
    .await
}

pub(super) async fn handle_threads_archive(state: AppState, args: Value) -> ApiResponse {
    archive_thread(state, rpc_required_string(&args, "threadId")?).await
}

pub(super) async fn handle_threads_restore(state: AppState, args: Value) -> ApiResponse {
    restore_thread(state, rpc_required_string(&args, "threadId")?).await
}

pub(super) async fn handle_threads_rename(state: AppState, args: Value) -> ApiResponse {
    rename_thread(state, rpc_payload(&args)?).await
}

pub(super) async fn handle_threads_account_scope_status(
    state: AppState,
    _args: Value,
) -> ApiResponse {
    crate::api::threads::account_scope_status(state).await
}

pub(super) async fn handle_threads_account_scope_clear(
    state: AppState,
    _args: Value,
) -> ApiResponse {
    crate::api::threads::account_scope_clear(state).await
}
