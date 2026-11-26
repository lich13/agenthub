use super::*;

pub(super) async fn handle_grok_list(state: AppState, args: Value) -> ApiResponse {
    let query = if args.is_null() {
        QueryLimitRequest::default()
    } else {
        rpc_payload(&args)?
    };
    grok_list(state, query).await
}

pub(super) async fn handle_grok_detail(state: AppState, args: Value) -> ApiResponse {
    let id = rpc_required_string(&args, "id")?;
    let query: crate::api::grok::GrokDetailQuery = rpc_payload(&args)?;
    grok_detail(state, id, query).await
}

pub(super) async fn handle_grok_rename(state: AppState, args: Value) -> ApiResponse {
    grok_rename(state, rpc_payload(&args)?).await
}

pub(super) async fn handle_grok_delete_preview(state: AppState, args: Value) -> ApiResponse {
    grok_delete_preview(state, rpc_required_string(&args, "id")?).await
}

pub(super) async fn handle_grok_delete_execute(state: AppState, args: Value) -> ApiResponse {
    grok_delete_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_claude_list(state: AppState, args: Value) -> ApiResponse {
    let query = if args.is_null() {
        QueryLimitRequest::default()
    } else {
        rpc_payload(&args)?
    };
    claude_list(state, query).await
}

pub(super) async fn handle_claude_detail(state: AppState, args: Value) -> ApiResponse {
    claude_detail(state, rpc_payload(&args)?).await
}

pub(super) async fn handle_claude_rename(state: AppState, args: Value) -> ApiResponse {
    claude_rename(state, rpc_payload(&args)?).await
}

pub(super) async fn handle_claude_delete_preview(state: AppState, args: Value) -> ApiResponse {
    let session_key = rpc_required_string(&args, "sessionKey")?;
    claude_delete_preview(state, session_key).await
}

pub(super) async fn handle_claude_delete_execute(state: AppState, args: Value) -> ApiResponse {
    claude_delete_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_antigravity_list(state: AppState, args: Value) -> ApiResponse {
    let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
        Default::default()
    } else {
        rpc_wrapped_payload(&args, &["request"])?
    };
    antigravity_list(state, request).await
}

pub(super) async fn handle_antigravity_detail(state: AppState, args: Value) -> ApiResponse {
    antigravity_detail(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_antigravity_rename(state: AppState, args: Value) -> ApiResponse {
    antigravity_rename(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_antigravity_archive(state: AppState, args: Value) -> ApiResponse {
    antigravity_archive(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_antigravity_restore(state: AppState, args: Value) -> ApiResponse {
    antigravity_restore(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_dsh_list(state: AppState, args: Value) -> ApiResponse {
    let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
        Default::default()
    } else {
        rpc_wrapped_payload(&args, &["request"])?
    };
    dsh_list(state, request).await
}

pub(super) async fn handle_dsh_detail(state: AppState, args: Value) -> ApiResponse {
    dsh_detail(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_dsh_rename(state: AppState, args: Value) -> ApiResponse {
    dsh_rename(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_dsh_archive(state: AppState, args: Value) -> ApiResponse {
    dsh_archive(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_dsh_restore(state: AppState, args: Value) -> ApiResponse {
    dsh_restore(state, rpc_wrapped_payload(&args, &["request"])?).await
}
