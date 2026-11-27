#[cfg(test)]
use agenthub_core::services::updates::{self as update_service, UpdateAction};
use serde::Serialize;
use serde_json::value::RawValue;

mod payload;
mod probe;
pub(crate) mod rpc_dispatch;
mod system;
mod threads;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use probe::{
    get_probe_events, get_probe_settings, get_probe_status, patch_probe_settings,
    spawn_probe_status_refresh, start_probe_action,
};
pub(crate) use system::{start_update_action, system_capabilities, system_update_status};
pub(crate) use threads::{
    archive_thread, list_threads, rename_thread, restore_thread, thread_blocks, thread_detail,
};
type ApiResponse = Result<Box<RawValue>, ApiError>;
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
    serde_json::value::to_raw_value(&value).map_err(|error| api_error(error.to_string()))
}
fn api_error(message: impl Into<String>) -> ApiError {
    ApiError {
        message: message.into(),
    }
}
