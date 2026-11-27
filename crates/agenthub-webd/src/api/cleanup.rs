use super::{ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::services::use_cases::AgentHubUseCases;

pub(crate) async fn hidden_threads_delete_dry_run(
    state: AppState,
    request: agenthub_core::archive::hidden_scan::HiddenScanRequest,
) -> ApiResponse {
    ok(AgentHubUseCases::new(state.platform())
        .cleanup()
        .scan_hidden(
            state.codex_paths(),
            crate::audit::ADMIN_ID.to_string(),
            request,
        )
        .await?)
}

pub(crate) async fn unified_cleanup_dry_run(state: AppState) -> ApiResponse {
    let paths = state.unified_cleanup_paths()?;
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform)
            .cleanup()
            .unified_dry_run(&paths)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn unified_cleanup_execute(
    state: AppState,
    request: agenthub_core::services::cleanup::UnifiedCleanupExecuteRequest,
) -> ApiResponse {
    let paths = state.unified_cleanup_paths()?;
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
            let (result, deleted_streams) = AgentHubUseCases::new(state.platform())
                .cleanup()
                .unified_execute_with_deleted_streams(&paths, request)?;
            let _ = state
                .db
                .prune_cursors_after_unified_cleanup(&deleted_streams);
            state.db.record_audit(
                Some(crate::audit::ADMIN_ID),
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
