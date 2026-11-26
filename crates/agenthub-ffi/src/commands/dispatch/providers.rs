use super::*;

pub(super) async fn claude_hooks(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ClaudeHooksArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    let provider = args.provider.unwrap_or_default();
    encode(
        agenthub_core::read_tasks::spawn_blocking(move || {
            AgentHubUseCases::new(state.platform())
                .probe()
                .provider_hooks(provider, args.request)
        })
        .await
        .map_err(|_| "Hooks 设置任务未完成".to_string())?
        .map_err(|e| e.to_string()),
    )
}

pub(super) async fn grok_list(ctx: DispatchContext<'_>, args_json: &str) -> Result<String, String> {
    let args: QueryLimitRequest = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .grok()?
                .list(args.limit.unwrap_or(100), args.q.as_deref())
        })
        .await,
    )
}

pub(super) async fn grok_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: GrokDetailArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform()).grok()?.page(
                &args.id,
                args.limit,
                args.before.as_deref(),
            )
        })
        .await,
    )
}

pub(super) async fn grok_rename(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: GrokRenameArgs = parse_args(args_json)?;
    encode(
        AgentHubUseCases::new(ctx.state.platform())
            .grok()
            .map_err(|error| error.to_string())?
            .rename(&args.id, &args.title)
            .await
            .map_err(|e| e.to_string()),
    )
}

pub(super) async fn grok_delete_preview(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: IdOnlyArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .grok()?
                .delete_preview(&args.id)
        })
        .await,
    )
}

pub(super) async fn grok_delete_execute(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<GrokDeleteRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            let result = AgentHubUseCases::new(state.platform())
                .grok()?
                .delete_execute(args.request)?;
            if result.deleted && result.residuals.is_empty() {
                let _ = state.db.delete_native_probe_stream_cursors(
                    agenthub_core::native_probe::NativeProvider::Grok,
                    std::slice::from_ref(&result.id),
                );
            }
            Ok(result)
        })
        .await,
    )
}

pub(super) async fn claude_list(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: QueryLimitRequest = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .claude()?
                .list(args.limit.unwrap_or(100), args.q.as_deref())
        })
        .await,
    )
}

pub(super) async fn claude_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let request: ClaudeDetailRequest = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .claude()?
                .detail(request)
        })
        .await,
    )
}

pub(super) async fn claude_ownership_diagnostics(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: SessionKeyOnlyArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .claude()?
                .ownership_diagnostics(&args.session_key)
        })
        .await,
    )
}

pub(super) async fn claude_rename(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ClaudeRenameArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .claude()?
                .rename(&args.session_key, &args.title)
        })
        .await,
    )
}

pub(super) async fn claude_delete_preview(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: SessionKeyOnlyArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .claude()?
                .delete_preview(&args.session_key)
        })
        .await,
    )
}

pub(super) async fn claude_delete_execute(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ClaudeDeleteRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            let result = AgentHubUseCases::new(state.platform())
                .claude()?
                .delete_execute(args.request)?;
            if result.deleted && result.residuals.is_empty() {
                let _ = state.db.delete_native_probe_stream_cursors(
                    agenthub_core::native_probe::NativeProvider::Claude,
                    std::slice::from_ref(&result.session_key),
                );
            }
            Ok(result)
        })
        .await,
    )
}

pub(super) async fn antigravity_list(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: OptionalWrappedRequest<ProviderListRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .antigravity()?
                .list(args.request.unwrap_or_default())
        })
        .await,
    )
}

pub(super) async fn antigravity_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderDetailRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .antigravity()?
                .detail(args.request)
        })
        .await,
    )
}

pub(super) async fn antigravity_rename(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderRenameRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .antigravity()?
                .rename(&args.request.session_key, &args.request.title)
        })
        .await,
    )
}

pub(super) async fn antigravity_archive(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .antigravity()?
                .archive(&args.request.session_key)
        })
        .await,
    )
}

pub(super) async fn antigravity_restore(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .antigravity()?
                .restore(&args.request.session_key)
        })
        .await,
    )
}

pub(super) async fn dsh_list(ctx: DispatchContext<'_>, args_json: &str) -> Result<String, String> {
    let args: OptionalWrappedRequest<ProviderListRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .dsh()?
                .list(args.request.unwrap_or_default())
        })
        .await,
    )
}

pub(super) async fn dsh_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderDetailRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .dsh()?
                .detail(args.request)
        })
        .await,
    )
}

pub(super) async fn dsh_rename(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderRenameRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .dsh()?
                .rename(&args.request.session_key, &args.request.title)
        })
        .await,
    )
}

pub(super) async fn dsh_archive(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .dsh()?
                .archive(&args.request.session_key)
        })
        .await,
    )
}

pub(super) async fn dsh_restore(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderSessionRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .dsh()?
                .restore(&args.request.session_key)
        })
        .await,
    )
}

pub(super) async fn grokbot_list(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: OptionalWrappedRequest<ProviderListRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .grokbot()?
                .list(args.request.unwrap_or_default())
        })
        .await,
    )
}

pub(super) async fn grokbot_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<ProviderDetailRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .grokbot()?
                .detail(args.request)
        })
        .await,
    )
}
