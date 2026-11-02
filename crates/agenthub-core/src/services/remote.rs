//! Desktop connection DTOs. Credentials are input-only and deliberately not Debug/Serialize.
pub mod credentials;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineTarget {
    #[default]
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMachineSummary {
    pub id: String,
    pub name: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConnectionView {
    pub target: MachineTarget,
    pub revision: u64,
    pub base_url: Option<String>,
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_machine_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub machines: Vec<RemoteMachineSummary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteConnectionCredentials {
    pub revision: u64,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteRevisionRequest {
    pub revision: u64,
    #[serde(default)]
    pub machine_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteSelectionRequest {
    pub revision: u64,
    pub target: MachineTarget,
    #[serde(default)]
    pub machine_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteInvokeRequest {
    pub revision: u64,
    pub command: String,
    pub args: Value,
}

pub type RemoteInvokeResponse = Value;
