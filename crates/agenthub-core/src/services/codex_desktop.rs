//! Local macOS desktop contracts. These are intentionally absent from the
//! authenticated Linux RPC allowlist; remote sources remain read-only here.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopDiagnosticsRequest {
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDiagnostic {
    pub timestamp: i64,
    pub source: String,
    pub method: String,
    pub version: Option<u64>,
    pub code: Option<i64>,
    pub category: String,
    pub hint: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDiagnostics {
    pub records: Vec<DesktopDiagnostic>,
    pub desktop_build: Option<String>,
    pub bundled_cli_version: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopThreadRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_owner: Option<bool>,
    pub revision: u64,
    pub thread_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopSessionRequest {
    pub revision: u64,
    pub session_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopActionRequest {
    pub revision: u64,
    pub session_id: String,
    pub action: DesktopAction,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DesktopAction {
    Send {
        message_id: String,
        text: String,
        references: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reference_spans: Vec<DesktopReferenceSpan>,
    },
    Steer {
        message_id: String,
        text: String,
        references: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reference_spans: Vec<DesktopReferenceSpan>,
        turn_id: String,
    },
    Stop {
        turn_id: String,
    },
    CancelQueued {
        message_id: String,
    },
    DismissUnknown {
        message_id: String,
    },
    ResumeQueue,
    Settings {
        model: String,
        reasoning_effort: Option<String>,
        mode: DesktopMode,
    },
    Decide {
        request_key: String,
        allow: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision: Option<String>,
    },
    Answer {
        request_key: String,
        answers: Vec<DesktopAnswer>,
    },
    Elicitation {
        request_key: String,
        accept: bool,
        content: Option<Value>,
    },
}

/// UTF-8 offsets in the user's text, never a native path or native wire object.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopReferenceSpan {
    pub handle: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesktopMode {
    Default,
    Plan,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopAnswer {
    pub question_id: String,
    pub answers: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_build: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<DesktopGoal>,
    pub session_id: String,
    pub thread_id: String,
    pub revision: u64,
    pub connected: bool,
    pub writable: bool,
    pub runtime: String,
    pub active_turn_id: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub mode: Option<DesktopMode>,
    pub requests: Vec<DesktopPendingRequest>,
    pub queue: Vec<DesktopQueuedMessage>,
    pub error: Option<String>,
}

/// Read-only native goal projection. The retired AgentHub Goal commands and
/// scheduler remain unavailable; mutations belong to the native desktop.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopGoal {
    pub objective: String,
    pub status: String,
    pub truncated: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopQueuedMessage {
    pub id: String,
    pub status: String,
    pub kind: String,
    pub preview: String,
    pub created_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPendingRequest {
    pub key: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub questions: Vec<DesktopQuestion>,
    pub form_schema: Option<Value>,
    pub can_accept: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<DesktopReview>,
}

/// Read-only approval presentation. Paths here are never accepted as inputs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopReview {
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub reason: Option<String>,
    pub changes: Vec<DesktopFileReview>,
    pub permissions: Option<DesktopPermissionsReview>,
    pub extra: Option<String>,
    pub truncated: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopFileReview {
    pub path: String,
    pub kind: String,
    pub diff: String,
    pub truncated: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPermissionsReview {
    pub network: Option<String>,
    pub file_system: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub secret: bool,
    pub options: Vec<DesktopQuestionOption>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopCatalogRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_refetch: Option<bool>,
    pub revision: u64,
    pub session_id: Option<String>,
    pub project_handle: Option<String>,
    pub kind: String,
    pub query: String,
    pub cursor: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCatalog {
    pub items: Vec<DesktopCatalogItem>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCatalogItem {
    pub handle: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopChooseRequest {
    pub revision: u64,
    pub session_id: Option<String>,
    pub kind: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopChosenItem {
    pub handle: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopChoice {
    pub item: Option<DesktopChosenItem>,
}

/// Native attachment receiver. No client path, URL, bytes or method names.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopAttachmentRequest {
    pub revision: u64,
    pub session_id: String,
    pub action: DesktopAttachmentAction,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DesktopAttachmentAction {
    Bind {
        bounds: DesktopAttachmentBounds,
        handles: Vec<String>,
    },
    Bounds {
        binding_id: String,
        bounds: DesktopAttachmentBounds,
        handles: Vec<String>,
    },
    Paste {
        binding_id: String,
    },
    Release {
        binding_id: String,
        handles: Vec<String>,
    },
    Unbind {
        binding_id: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopAttachmentBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAttachmentEvent {
    pub binding_id: String,
    pub items: Vec<DesktopChosenItem>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAttachmentResult {
    pub binding_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paste_disposition: Option<DesktopPasteDisposition>,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DesktopPasteDisposition {
    Attachments,
    Text,
    Unsupported,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopCreateRequest {
    pub revision: u64,
    pub project_handle: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCreatedThread {
    pub thread_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DesktopAck {
    pub ok: bool,
}
