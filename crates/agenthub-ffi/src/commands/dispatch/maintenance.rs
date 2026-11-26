use super::*;

pub(super) async fn system_capabilities(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    encode(Ok(AgentHubUseCases::new(ctx.state.platform())
        .system()
        .runtime_capabilities()))
}

pub(super) async fn jobs_list(ctx: DispatchContext<'_>, args_json: &str) -> Result<String, String> {
    let args: LimitU32Request = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .jobs()
                .list_from_db(&state.db, args.limit)
        })
        .await,
    )
}

pub(super) async fn jobs_detail(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: JobDetailRequest = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            AgentHubUseCases::new(state.platform())
                .jobs()
                .detail_from_db(&state.db, &args.id, args.include_output)
        })
        .await,
    )
}

pub(super) async fn probe_status(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ProbeStatusRequest = parse_args(args_json)?;
    encode(
        probe_service::desktop_probe_status_with_options(ctx.state, args.refresh.unwrap_or(false))
            .await
            .map_err(|e| e.to_string()),
    )
}

pub(super) async fn github_ci_status(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let request: agenthub_core::github_ci::Request = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        agenthub_core::read_tasks::spawn_blocking(move || {
            probe_service::desktop_github_ci_status(&state, request)
        })
        .await
        .map_err(|e| e.to_string())?,
    )
}

pub(super) async fn probe_settings_get(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || settings_service::probe_settings_with_state(&state))
            .await,
    )
}

pub(super) async fn probe_settings_save(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: ProbeSettingsSaveArgs = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            settings_service::probe_save_settings_with_state(&state, args.settings)
        })
        .await,
    )
}

pub(super) async fn probe_events(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: LimitU32Request = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            settings_service::probe_events_with_state(&state, args.limit)
        })
        .await,
    )
}

pub(super) async fn probe_bark_test(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            settings_service::probe_action_with_state(&state, ProbeAction::BarkTest)
        })
        .await,
    )
}

pub(super) async fn probe_install_hooks(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            settings_service::probe_action_with_state(&state, ProbeAction::InstallHooks)
        })
        .await,
    )
}

pub(super) async fn probe_remove_hooks(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            settings_service::probe_action_with_state(&state, ProbeAction::RemoveHooks)
        })
        .await,
    )
}

pub(super) async fn updates_status(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let state = ctx.state.clone();
    encode(
        crate::commands::blocking(move || {
            update_service::desktop_update_status_with_state(&state, None, None)
        })
        .await,
    )
}

pub(super) async fn updates_check(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    encode(update_service::check_update_status(ctx.state).await)
}

pub(super) async fn updates_install(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    encode(update_service::install_update_and_restart(ctx.state, ctx.host, ctx.isolated).await)
}

pub(super) async fn cleanup_unified_dry_run(
    ctx: DispatchContext<'_>,
    _args_json: &str,
) -> Result<String, String> {
    let state = ctx.state.clone();
    encode(
        agenthub_core::read_tasks::spawn_blocking(move || {
            let paths = agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(
                state.codex_paths(),
            )
            .map_err(|error| error.to_string())?;
            AgentHubUseCases::new(state.platform())
                .cleanup()
                .unified_dry_run(&paths)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|_| "全平台清理预览任务未完成".to_string())?,
    )
}

pub(super) async fn cleanup_unified_execute(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<UnifiedCleanupExecuteRequest> = parse_args(args_json)?;
    let state = ctx.state.clone();
    encode(
        agenthub_core::read_tasks::spawn_blocking(move || {
            let paths = agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(
                state.codex_paths(),
            )
            .map_err(|error| error.to_string())?;
            let (result, deleted_streams) = AgentHubUseCases::new(state.platform())
                .cleanup()
                .unified_execute_with_deleted_streams(&paths, args.request)
                .map_err(|e| e.to_string())?;
            let _ = state
                .db
                .prune_cursors_after_unified_cleanup(&deleted_streams);
            Ok(result)
        })
        .await
        .map_err(|_| "全平台清理任务未完成".to_string())?,
    )
}

pub(super) async fn cleanup_hidden_dry_run(
    ctx: DispatchContext<'_>,
    args_json: &str,
) -> Result<String, String> {
    let args: WrappedRequest<HiddenScanRequest> = parse_args(args_json)?;
    encode(
        AgentHubUseCases::new(ctx.state.platform())
            .cleanup()
            .scan_hidden(ctx.state.codex_paths(), "desktop".into(), args.request)
            .await
            .map_err(|e| e.to_string()),
    )
}
