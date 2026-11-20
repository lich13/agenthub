use super::*;

pub(super) async fn handle_threads_subagent_detail(state: AppState, args: Value) -> ApiResponse {
    crate::api::threads::subagent_detail(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_sessions_attachment_read(state: AppState, args: Value) -> ApiResponse {
    crate::api::sessions::attachment_read(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_sessions_search(state: AppState, args: Value) -> ApiResponse {
    crate::api::sessions::search(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_sessions_bulk_preview(state: AppState, args: Value) -> ApiResponse {
    crate::api::sessions::bulk_preview(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_sessions_bulk_execute(state: AppState, args: Value) -> ApiResponse {
    crate::api::sessions::bulk_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
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
    rename_thread(
        state,
        rpc_required_string(&args, "threadId")?,
        rpc_payload(&args)?,
    )
    .await
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
