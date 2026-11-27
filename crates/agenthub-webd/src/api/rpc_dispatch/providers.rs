use super::*;
use agenthub_core::{
    claude::{ClaudeDeleteRequest, ClaudeDetailRequest},
    grok::GrokDeleteRequest,
    native_probe::NativeProvider,
    platform::PlatformPaths,
    provider_sessions::{
        ProviderDetailRequest, ProviderListRequest, ProviderRenameRequest, ProviderSessionRequest,
    },
    services::use_cases::{
        AgentHubUseCases, ClaudeRenameRequest, GrokRenameRequest, PageWindowOptions,
    },
};

async fn run_blocking_provider<T, F>(state: AppState, work: F) -> ApiResponse
where
    T: serde::Serialize + Send + 'static,
    F: FnOnce(&PlatformPaths) -> anyhow::Result<T> + Send + 'static,
{
    let platform = state.platform().clone();
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || work(&platform))
            .await
            .map_err(anyhow::Error::from)??,
    )
}

fn parse_provider_list_args(args: &Value) -> Result<ProviderListRequest, super::super::ApiError> {
    if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
        Ok(Default::default())
    } else {
        rpc_wrapped_payload(args, &["request"])
    }
}

pub(super) async fn handle_grok_list(state: AppState, args: Value) -> ApiResponse {
    let query = if args.is_null() {
        QueryLimitRequest::default()
    } else {
        rpc_payload(&args)?
    };
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .grok()?
            .list(query.limit.unwrap_or(100), query.q.as_deref())
    })
    .await
}

pub(super) async fn handle_grok_detail(state: AppState, args: Value) -> ApiResponse {
    let id = rpc_required_string(&args, "id")?;
    let query: PageWindowOptions = rpc_payload(&args)?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .grok()?
            .page(&id, query.limit, query.before.as_deref())
    })
    .await
}

pub(super) async fn handle_grok_rename(state: AppState, args: Value) -> ApiResponse {
    let request: GrokRenameRequest = rpc_payload(&args)?;
    ok(AgentHubUseCases::new(state.platform())
        .grok()?
        .rename(&request.id, &request.title)
        .await?)
}

pub(super) async fn handle_grok_delete_preview(state: AppState, args: Value) -> ApiResponse {
    let id = rpc_required_string(&args, "id")?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p).grok()?.delete_preview(&id)
    })
    .await
}

pub(super) async fn handle_grok_delete_execute(state: AppState, args: Value) -> ApiResponse {
    let request: GrokDeleteRequest = rpc_wrapped_payload(&args, &["request"])?;
    let platform = state.platform().clone();
    let db = state.db.clone();
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
            let result = AgentHubUseCases::new(&platform)
                .grok()?
                .delete_execute(request)?;
            if result.deleted && result.residuals.is_empty() {
                let _ = db.delete_native_probe_stream_cursors(
                    NativeProvider::Grok,
                    std::slice::from_ref(&result.id),
                );
            }
            Ok(result)
        })
        .await
        .map_err(anyhow::Error::from)??,
    )
}

pub(super) async fn handle_claude_list(state: AppState, args: Value) -> ApiResponse {
    let query = if args.is_null() {
        QueryLimitRequest::default()
    } else {
        rpc_payload(&args)?
    };
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .claude()?
            .list(query.limit.unwrap_or(100), query.q.as_deref())
    })
    .await
}

pub(super) async fn handle_claude_detail(state: AppState, args: Value) -> ApiResponse {
    let request: ClaudeDetailRequest = rpc_payload(&args)?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p).claude()?.detail(request)
    })
    .await
}

pub(super) async fn handle_claude_rename(state: AppState, args: Value) -> ApiResponse {
    let request: ClaudeRenameRequest = rpc_payload(&args)?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .claude()?
            .rename(&request.session_key, &request.title)
    })
    .await
}

pub(super) async fn handle_claude_delete_preview(state: AppState, args: Value) -> ApiResponse {
    let session_key = rpc_required_string(&args, "sessionKey")?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .claude()?
            .delete_preview(&session_key)
    })
    .await
}

pub(super) async fn handle_claude_delete_execute(state: AppState, args: Value) -> ApiResponse {
    let request: ClaudeDeleteRequest = rpc_wrapped_payload(&args, &["request"])?;
    let platform = state.platform().clone();
    let db = state.db.clone();
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
            let result = AgentHubUseCases::new(&platform)
                .claude()?
                .delete_execute(request)?;
            if result.deleted && result.residuals.is_empty() {
                let _ = db.delete_native_probe_stream_cursors(
                    NativeProvider::Claude,
                    std::slice::from_ref(&result.session_key),
                );
            }
            Ok(result)
        })
        .await
        .map_err(anyhow::Error::from)??,
    )
}

pub(super) async fn handle_antigravity_list(state: AppState, args: Value) -> ApiResponse {
    let request = parse_provider_list_args(&args)?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p).antigravity()?.list(request)
    })
    .await
}

pub(super) async fn handle_antigravity_detail(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderDetailRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p).antigravity()?.detail(request)
    })
    .await
}

pub(super) async fn handle_antigravity_rename(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderRenameRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .antigravity()?
            .rename(&request.session_key, &request.title)
    })
    .await
}

pub(super) async fn handle_antigravity_archive(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderSessionRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .antigravity()?
            .archive(&request.session_key)
    })
    .await
}

pub(super) async fn handle_antigravity_restore(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderSessionRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .antigravity()?
            .restore(&request.session_key)
    })
    .await
}

pub(super) async fn handle_dsh_list(state: AppState, args: Value) -> ApiResponse {
    let request = parse_provider_list_args(&args)?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p).dsh()?.list(request)
    })
    .await
}

pub(super) async fn handle_dsh_detail(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderDetailRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p).dsh()?.detail(request)
    })
    .await
}

pub(super) async fn handle_dsh_rename(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderRenameRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .dsh()?
            .rename(&request.session_key, &request.title)
    })
    .await
}

pub(super) async fn handle_dsh_archive(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderSessionRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .dsh()?
            .archive(&request.session_key)
    })
    .await
}

pub(super) async fn handle_dsh_restore(state: AppState, args: Value) -> ApiResponse {
    let request: ProviderSessionRequest = rpc_wrapped_payload(&args, &["request"])?;
    run_blocking_provider(state, move |p| {
        AgentHubUseCases::new(p)
            .dsh()?
            .restore(&request.session_key)
    })
    .await
}
