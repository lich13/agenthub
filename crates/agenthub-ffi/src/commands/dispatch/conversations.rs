use super::*;

pub(super) async fn threads_list(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ThreadsListArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            thread_service::threads_with_state(
                &state,
                ThreadListRequest {
                    archived: args.archived,
                    query: args.q,
                    limit: args.limit,
                },
            )
        })
        .await,
    )
}

pub(super) async fn threads_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ThreadDetailArgs = parse_args(args_json)?;
    let opts = args.options.unwrap_or_default();
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            thread_service::thread_detail_with_state(
                &state,
                ThreadDetailRequest {
                    id: args.id,
                    limit: opts.limit,
                    before: opts.before,
                    full: opts.full,
                },
            )
        })
        .await,
    )
}

pub(super) async fn threads_subagent_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<SubagentDetailRequest> = parse_args(args_json)?;
    let paths = ctx.state.codex_paths();
    let platform = ctx.state.platform().clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(&platform)
                .threads()
                .subagent_detail(&paths, &args.request)
        })
        .await,
    )
}

pub(super) async fn threads_blocks(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ThreadDetailArgs = parse_args(args_json)?;
    let opts = args.options.unwrap_or_default();
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            thread_service::thread_blocks_with_state(
                &state,
                ThreadBlocksRequest {
                    id: args.id,
                    limit: opts.limit,
                    before: opts.before,
                },
            )
        })
        .await,
    )
}

pub(super) async fn threads_archive(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ThreadIdArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            thread_service::archive_thread_with_state(
                &state,
                DesktopThreadIdRequest {
                    thread_id: args.thread_id,
                },
            )
        })
        .await,
    )
}

pub(super) async fn threads_restore(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ThreadIdArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            thread_service::restore_thread_with_state(
                &state,
                DesktopThreadIdRequest {
                    thread_id: args.thread_id,
                },
            )
        })
        .await,
    )
}

pub(super) async fn threads_rename(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ThreadRenameArgs = parse_args(args_json)?;
    encode(
        thread_service::rename_thread_with_state(
            ctx.state,
            DesktopRenameThreadRequest {
                thread_id: args.thread_id,
                name: args.name,
            },
        )
        .await
        .map_err(|e| e.to_string()),
    )
}

pub(super) async fn threads_account_scope_status(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let paths = ctx.state.codex_paths();
    let platform = ctx.state.platform().clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(&platform)
                .threads()
                .account_scope_status(&paths)
        })
        .await,
    )
}

pub(super) async fn threads_account_scope_clear(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let paths = ctx.state.codex_paths();
    let platform = ctx.state.platform().clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(&platform)
                .threads()
                .account_scope_clear(&paths)
        })
        .await,
    )
}

pub(super) async fn sessions_search(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<SessionSearchRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .search(state.codex_paths())?
                .search(args.request)
        })
        .await,
    )
}

pub(super) async fn sessions_attachment_read(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<SessionAttachmentRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .sessions(state.codex_paths())?
                .attachment_read(args.request)
        })
        .await,
    )
}

pub(super) async fn sessions_bulk_preview(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<SessionBatchRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .sessions(state.codex_paths())?
                .bulk_preview(args.request)
        })
        .await,
    )
}

pub(super) async fn sessions_bulk_execute(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<SessionBatchExecuteRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        tokio::task::spawn_blocking(move || {
            AgentHubUseCases::new(state.platform())
                .sessions(state.codex_paths())?
                .bulk_execute(args.request)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string()),
    )
}

pub(super) async fn plans_save(
    _ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<PlanSaveRequest> = parse_args(args_json)?;
    encode(crate::commands::plans::savePlanMarkdown(args.request).await)
}
