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
pub(crate) use jobs::{job_detail, list_jobs, JobsListQuery};
pub(crate) use probe::{
    get_probe_events, get_probe_settings, get_probe_status, load_probe_threads,
    patch_probe_settings, spawn_probe_status_refresh, start_probe_action, ProbeEventsQuery,
};
pub(crate) use provider_sessions::{
    antigravity_archive, antigravity_detail, antigravity_list, antigravity_rename,
    antigravity_restore, dsh_archive, dsh_detail, dsh_list, dsh_rename, dsh_restore,
};
pub(crate) use system::{start_update_action, system_capabilities, system_update_status};
pub(crate) use threads::{
    archive_thread, list_threads, rename_thread, restore_thread, thread_blocks, thread_detail,
};
type ApiResponse = Result<Value, ApiError>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiError {
    pub message: String,
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        api_error(error.to_string())
    }
}
fn ok<T: Serialize>(value: T) -> ApiResponse {
    serde_json::to_value(value).map_err(|error| api_error(error.to_string()))
}
fn api_error(message: impl Into<String>) -> ApiError {
    ApiError {
        message: message.into(),
    }
}
