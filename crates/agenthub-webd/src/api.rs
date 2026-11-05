use crate::state::AppState;
use agenthub_core::providers::ProviderRegistry;
#[cfg(test)]
use agenthub_core::services::updates::{self as update_service, UpdateAction};
use serde::Serialize;
use serde_json::Value;

mod claude;
mod cleanup;
mod grok;
mod jobs;
mod payload;
mod probe;
mod provider_sessions;
pub(crate) mod rpc_dispatch;
mod sessions;
mod system;
mod threads;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use claude::{
    claude_delete_execute, claude_delete_preview, claude_detail, claude_list, claude_rename,
    ClaudeListQuery,
};
pub(crate) use cleanup::{
    hidden_threads_delete_dry_run, unified_cleanup_dry_run, unified_cleanup_execute,
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
pub(crate) use system::{
    linux_update_platform, start_update_action, system_capabilities, system_update_status,
    system_version,
};
pub(crate) use threads::{
    archive_thread, list_threads, rename_thread, restore_thread, thread_blocks, thread_detail,
};
type ApiResponse = Result<Value, ApiError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiErrorKind {
    InvalidRequest,
    NotFound,
    Conflict,
    OperationFailed,
}
#[derive(Debug)]
pub(crate) struct ApiError {
    #[cfg_attr(not(test), allow(dead_code))]
    // Protocol 2 exposes the message; direct tests also assert the category.
    pub kind: ApiErrorKind,
    pub message: String,
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        api_error(ApiErrorKind::OperationFailed, &error.to_string())
    }
}
async fn list_providers(_state: AppState) -> ApiResponse {
    ok(ProviderRegistry::default().list())
}
async fn platform_overview(state: AppState) -> ApiResponse {
    ok(state.platform().clone())
}
fn ok<T: Serialize>(value: T) -> ApiResponse {
    serde_json::to_value(value)
        .map_err(|error| api_error(ApiErrorKind::OperationFailed, &error.to_string()))
}
fn api_error(kind: ApiErrorKind, message: &str) -> ApiError {
    ApiError {
        kind,
        message: message.into(),
    }
}
