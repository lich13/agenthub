use agenthub_core::services::jobs as job_service;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopActionResponse {
    pub ok: bool,
    pub available: bool,
    pub command: String,
    pub message: String,
    pub thread_id: Option<String>,
    pub job_id: Option<String>,
    pub data: Option<Value>,
}

impl From<job_service::ActionResponse> for DesktopActionResponse {
    fn from(value: job_service::ActionResponse) -> Self {
        Self {
            ok: value.ok,
            available: value.available,
            command: value.command,
            message: value.message,
            thread_id: value.thread_id,
            job_id: value.job_id,
            data: value.data,
        }
    }
}
