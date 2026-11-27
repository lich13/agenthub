use super::*;

pub(super) async fn handle_github_ci_status(state: AppState, args: Value) -> ApiResponse {
    let config = state.config();
    let snapshot = agenthub_core::services::use_cases::AgentHubUseCases::with_config(
        &config,
        state.platform(),
    )
    .github_ci(&state.db, rpc_payload(&args)?)?;
    if snapshot.refreshing {
        state.notify_github_ci();
    }
    crate::api::ok(snapshot)
}

pub(super) async fn handle_probe_status(state: AppState, args: Value) -> ApiResponse {
    get_probe_status(state, rpc_payload(&args)?).await
}

pub(super) async fn handle_probe_settings_get(state: AppState, _args: Value) -> ApiResponse {
    get_probe_settings(state).await
}

pub(super) async fn handle_probe_settings_save(state: AppState, args: Value) -> ApiResponse {
    patch_probe_settings(state, rpc_wrapped_payload(&args, &["settings"])?).await
}

pub(super) async fn handle_probe_events(state: AppState, args: Value) -> ApiResponse {
    let query = if args.is_null() {
        LimitU32Request::default()
    } else {
        rpc_payload(&args)?
    };
    get_probe_events(state, query).await
}

pub(super) async fn handle_probe_bark_test(state: AppState, _args: Value) -> ApiResponse {
    start_probe_action(state, probe_service::ProbeAction::BarkTest).await
}

pub(super) async fn handle_probe_install_hooks(state: AppState, _args: Value) -> ApiResponse {
    start_probe_action(state, probe_service::ProbeAction::InstallHooks).await
}

pub(super) async fn handle_probe_remove_hooks(state: AppState, _args: Value) -> ApiResponse {
    start_probe_action(state, probe_service::ProbeAction::RemoveHooks).await
}

pub(super) async fn handle_cleanup_unified_dry_run(state: AppState, _args: Value) -> ApiResponse {
    let paths = state.unified_cleanup_paths()?;
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        agenthub_core::services::use_cases::AgentHubUseCases::new(&platform)
            .cleanup()
            .unified_dry_run(&paths)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(super) async fn handle_cleanup_unified_execute(state: AppState, args: Value) -> ApiResponse {
    let request: agenthub_core::services::cleanup::UnifiedCleanupExecuteRequest =
        rpc_wrapped_payload(&args, &["request"])?;
    let paths = state.unified_cleanup_paths()?;
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
            let (result, deleted_streams) =
                agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
                    .cleanup()
                    .unified_execute_with_deleted_streams(&paths, request)?;
            let _ = state
                .db
                .prune_cursors_after_unified_cleanup(&deleted_streams);
            state.db.record_audit(
                Some(crate::state::ADMIN_ID),
                "unified_cleanup.execute",
                Some("unified_cleanup"),
                None,
                serde_json::json!({
                    "deleted_count": result.deleted_count,
                    "deleted_bytes": result.deleted_bytes,
                    "failed_count": result.failed_count,
                }),
            )?;
            Ok(result)
        })
        .await
        .map_err(anyhow::Error::from)??,
    )
}

pub(super) async fn handle_cleanup_hidden_dry_run(state: AppState, args: Value) -> ApiResponse {
    let request: agenthub_core::archive::hidden_scan::HiddenScanRequest =
        rpc_wrapped_payload(&args, &["request"])?;
    ok(
        agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
            .cleanup()
            .scan_hidden(
                state.codex_paths(),
                crate::state::ADMIN_ID.to_string(),
                request,
            )
            .await?,
    )
}

pub(super) async fn handle_updates_status(state: AppState, _args: Value) -> ApiResponse {
    system_update_status(state).await
}

pub(super) async fn handle_updates_check(state: AppState, _args: Value) -> ApiResponse {
    start_update_action(state, UpdateAction::Check, None).await
}

pub(super) async fn handle_updates_install(state: AppState, _args: Value) -> ApiResponse {
    start_update_action(
        state,
        UpdateAction::Install,
        Some("agenthub.update.install_started"),
    )
    .await
}

pub(super) async fn handle_updates_prune(state: AppState, _args: Value) -> ApiResponse {
    start_update_action(
        state,
        UpdateAction::Prune,
        Some("agenthub.update.prune_started"),
    )
    .await
}

pub(super) async fn handle_system_capabilities(state: AppState, _args: Value) -> ApiResponse {
    system_capabilities(state).await
}

pub(super) async fn handle_jobs_list(state: AppState, args: Value) -> ApiResponse {
    let query = if args.is_null() {
        LimitU32Request::default()
    } else {
        rpc_payload(&args)?
    };
    crate::api::ok(
        agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
            .jobs()
            .list_from_db(&state.db, query.limit)?,
    )
}

pub(super) async fn handle_jobs_detail(state: AppState, args: Value) -> ApiResponse {
    let req: JobDetailRequest = rpc_payload(&args)?;
    match agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
        .jobs()
        .detail_from_db(&state.db, &req.id, req.include_output)?
    {
        Some(job) => crate::api::ok(job),
        None => Err(api_error("job not found")),
    }
}
