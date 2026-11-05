use super::{linux_update_platform, ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::services::use_cases::AgentHubUseCases;

pub(crate) async fn hidden_threads_delete_dry_run(
    state: AppState,
    request: agenthub_core::archive::hidden_scan::HiddenScanRequest,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = linux_update_platform();
    ok(AgentHubUseCases::new(&platform)
        .cleanup()
        .scan_hidden(state.codex_paths(), auth.admin_id.to_string(), request)
        .await?)
}

pub(crate) async fn unified_cleanup_dry_run(
    state: AppState,
    request: agenthub_core::services::cleanup::UnifiedCleanupDryRunRequest,
) -> ApiResponse {
    let paths = state.unified_cleanup_paths();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .cleanup()
            .unified_dry_run(&paths, request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn unified_cleanup_execute(
    state: AppState,
    request: agenthub_core::services::cleanup::UnifiedCleanupExecuteRequest,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let paths = state.unified_cleanup_paths();
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
            let result = AgentHubUseCases::new(&linux_update_platform())
                .cleanup()
                .unified_execute(&paths, request)?;
            state.db.record_audit(
                Some(&auth.admin_id),
                "unified_cleanup.execute",
                Some("unified_cleanup"),
                None,
                None,
                serde_json::json!({
                    "deleted_count": result.deleted_count,
                    "deleted_bytes": result.deleted_bytes,
                    "failed_count": result.failed_count,
                    "stopped_early": result.stopped_early,
                }),
            )?;
            Ok(result)
        })
        .await
        .map_err(anyhow::Error::from)??,
    )
}
