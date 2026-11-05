use crate::state::AppState;
use agenthub_core::providers::ProviderRegistry;
#[cfg(test)]
use agenthub_core::services::updates::{self as update_service, UpdateAction};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::json;

mod claude;
mod cleanup;
mod grok;
mod jobs;
mod payload;
mod probe;
mod provider_sessions;
#[cfg(test)]
mod routes;
pub(crate) mod rpc_dispatch;
mod sessions;
mod system;
mod threads;

#[cfg(test)]
mod entry_contract_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod legacy_routes_tests;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use claude::{
    claude_delete_execute, claude_delete_preview, claude_detail, claude_list, claude_rename,
    ClaudeListQuery,
};
pub(crate) use cleanup::{
    archive_delete_dry_run, archive_delete_execute, dsh_archive_delete_dry_run,
    dsh_archive_delete_execute, hidden_threads_delete_dry_run, hidden_threads_delete_execute,
    unified_cleanup_dry_run, unified_cleanup_execute,
};
pub(crate) use grok::{
    grok_delete_execute, grok_delete_preview, grok_detail, grok_list, grok_rename, GrokListQuery,
};
pub(crate) use jobs::{job_detail, list_jobs};
#[cfg(test)]
pub(crate) use probe::probe_config_path;
pub(crate) use probe::{
    get_probe_events, get_probe_settings, get_probe_status, load_probe_threads,
    patch_probe_settings, spawn_probe_status_refresh, start_probe_action, ProbeEventsQuery,
    ProbeStatusQuery,
};
pub(crate) use provider_sessions::{
    antigravity_archive, antigravity_detail, antigravity_list, antigravity_rename,
    antigravity_restore, dsh_archive, dsh_detail, dsh_list, dsh_rename, dsh_restore,
};
#[cfg(test)]
pub(crate) use routes::router;
pub(crate) use system::{
    http_update_platform, start_update_action, system_capabilities, system_update_status,
    system_version,
};
pub(crate) use threads::{
    archive_thread, list_threads, rename_thread, restore_thread, thread_blocks, thread_detail,
};
type ApiResponse = Result<Response, ApiError>;

pub struct ApiError(Box<Response>);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        *self.0
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        api_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string())
    }
}

async fn list_providers(State(_state): State<AppState>, _headers: HeaderMap) -> ApiResponse {
    ok(ProviderRegistry::default().list())
}

async fn platform_overview(State(state): State<AppState>, _headers: HeaderMap) -> ApiResponse {
    ok(state.platform().clone())
}

fn ok<T: Serialize>(value: T) -> ApiResponse {
    Ok(Json(value).into_response())
}

fn api_error(status: StatusCode, message: &str) -> ApiError {
    ApiError(Box::new(
        (status, Json(json!({ "error": message }))).into_response(),
    ))
}
