use super::*;
use agenthub_core::services::threads::{self as core_thread_service, ThreadsQuery};

pub(super) async fn threads_list(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ThreadsListArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            let plan = AgentHubUseCases::new(state.platform())
                .threads()
                .list_read(ThreadsQuery {
                    archived: args.archived,
                    q: args.q,
                    limit: args.limit,
                })?;
            core_thread_service::load_thread_read_model_inputs(&state.codex_paths(), &plan)
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
            let plan = AgentHubUseCases::new(state.platform())
                .threads()
                .detail_read(core_thread_service::ThreadDetailRequest {
                    id: args.id,
                    limit: opts.limit,
                    full: opts.full,
                    before: opts.before,
                })?;
            core_thread_service::read_window_thread_detail(&state.codex_paths(), &plan)
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
            let plan = AgentHubUseCases::new(state.platform()).threads().blocks(
                &args.id,
                opts.limit,
                opts.before,
            )?;
            core_thread_service::read_thread_blocks_page(&state.codex_paths(), &plan)
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
            thread_service::archive_thread_with_state(&state, &args.thread_id)
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
            thread_service::restore_thread_with_state(&state, &args.thread_id)
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
        thread_service::rename_thread_with_state(ctx.state, args.thread_id, args.name)
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
            let provider = args.request.provider;
            let operation = args.request.operation;
            let result = AgentHubUseCases::new(state.platform())
                .sessions(state.codex_paths())?
                .bulk_execute(args.request)?;
            let _ = state
                .db
                .prune_cursors_after_batch_execute(provider, operation, &result);
            Ok::<_, anyhow::Error>(result)
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
