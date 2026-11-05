use super::ApiErrorKind;
use super::{api_error, linux_update_platform, ok, ApiResponse};
use crate::{linux_adapter, state::AppState};
use agenthub_core::services::{
    cleanup::{CleanupExecuteRequest, CleanupTarget},
    use_cases::AgentHubUseCases,
};
use serde::Deserialize;

pub(crate) async fn archive_delete_dry_run(state: AppState) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .cleanup()
        .dry_run(CleanupTarget::Archived)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    ok(linux_adapter::execute_cleanup_plan(&state, &auth, plan)?)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ArchiveExecuteRequest {
    confirmed: bool,
    #[serde(default, alias = "expectedCount", alias = "expected_count")]
    expected_count: Option<u64>,
    #[serde(default)]
    candidates: Option<Vec<agenthub_core::archive::HiddenThreadSelection>>,
}

pub(crate) async fn archive_delete_execute(
    state: AppState,
    payload: ArchiveExecuteRequest,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .cleanup()
        .execute_confirmed(
            CleanupTarget::Archived,
            CleanupExecuteRequest {
                confirmed: payload.confirmed,
                expected_count: payload.expected_count,
                candidates: payload.candidates,
            },
        )
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    ok(linux_adapter::execute_cleanup_plan(&state, &auth, plan)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?)
}

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

pub(crate) async fn hidden_threads_delete_execute(
    state: AppState,
    payload: ArchiveExecuteRequest,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .cleanup()
        .execute_confirmed(
            CleanupTarget::Hidden,
            CleanupExecuteRequest {
                confirmed: payload.confirmed,
                expected_count: payload.expected_count,
                candidates: payload.candidates,
            },
        )
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        linux_adapter::execute_cleanup_plan(&state, &auth, plan)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_archive_delete_dry_run(state: AppState) -> ApiResponse {
    let dsh = state.dsh_paths();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&linux_update_platform())
            .cleanup()
            .dsh_archive_dry_run(&dsh)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn dsh_archive_delete_execute(
    state: AppState,
    request: agenthub_core::dsh::DshArchiveDeleteRequest,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let dsh = state.dsh_paths();
    ok(agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
        let result = AgentHubUseCases::new(&linux_update_platform())
            .cleanup()
            .dsh_archive_execute(&dsh, request)?;
        state.db.record_audit(Some(&auth.admin_id), "dsh_archives.delete.execute", Some("dsh_archives"), None, None,
            serde_json::json!({"deleted_threads": result.deleted_threads, "deleted_bytes": result.deleted_bytes}))?;
        Ok(result)
    }).await.map_err(anyhow::Error::from)??)
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
