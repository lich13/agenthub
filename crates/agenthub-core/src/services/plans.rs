//! Desktop-only Markdown export; never accepted by the server dispatcher.
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanSaveRequest {
    pub filename: String,
    pub markdown: String,
}

#[derive(Serialize)]
pub struct PlanSaveResult {
    pub filename: String,
}
