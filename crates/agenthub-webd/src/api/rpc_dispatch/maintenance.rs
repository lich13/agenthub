use super::*;

pub(super) async fn handle_github_ci_status(state: AppState, args: Value) -> ApiResponse {
    let config = state.config();
    crate::api::ok(
        agenthub_core::services::use_cases::AgentHubUseCases::with_config(
            &config,
            state.platform(),
        )
        .github_ci(&state.db, rpc_payload(&args)?)?,
    )
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
        ProbeEventsQuery::default()
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
    unified_cleanup_dry_run(state).await
}

pub(super) async fn handle_cleanup_unified_execute(state: AppState, args: Value) -> ApiResponse {
    unified_cleanup_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
}

pub(super) async fn handle_cleanup_hidden_dry_run(state: AppState, args: Value) -> ApiResponse {
    hidden_threads_delete_dry_run(state, rpc_wrapped_payload(&args, &["request"])?).await
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
        JobsListQuery::default()
    } else {
        rpc_payload(&args)?
    };
    list_jobs(state, query).await
}

pub(super) async fn handle_jobs_detail(state: AppState, args: Value) -> ApiResponse {
    job_detail(
        state,
        rpc_required_string(&args, "id")?,
        args.get("includeOutput")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    )
    .await
}
