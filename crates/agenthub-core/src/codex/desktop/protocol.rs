//! Read-only source comparison: builds 20586 and 20770 share this method table.
//! Build numbers are diagnostic metadata, not compatibility gates. This fixed
//! allowlist is not negotiation. Unknown protocol versions fail closed; never probe a new
//! mutation or fall back to an independent turn executor.
pub(super) const STREAM_VERSION: u64 = 11;
pub(super) const QUEUE_VERSION: u64 = 2;

pub(super) fn request_wait(method: &str) -> std::time::Duration {
    use crate::resource_limits::{
        CODEX_DESKTOP_CONNECT_WAIT, CODEX_DESKTOP_START_WAIT, CODEX_DESKTOP_STEER_WAIT,
    };
    match method {
        "thread-follower-start-turn" => CODEX_DESKTOP_START_WAIT,
        "thread-follower-steer-turn" => CODEX_DESKTOP_STEER_WAIT,
        _ => CODEX_DESKTOP_CONNECT_WAIT,
    }
}

/// Build 20586 distinguishes broker/pre-handler errors from exceptions thrown
/// by a handler. A handler exception does not prove that a turn was not started.
pub(super) fn response_error(native: Option<&str>) -> super::DesktopError {
    use super::DesktopError::*;
    match native {
        Some("no-client-found") => OwnerUnavailable,
        Some(
            "request-version-mismatch"
            | "no-handler-for-request"
            | "unsupported-version"
            | "method-not-found",
        ) => UnsupportedProtocol,
        Some("timeout" | "request-timeout") => Timeout,
        Some("ipc-connection-reset" | "client-disconnected" | "server-closed") => Disconnected,
        _ => Rejected,
    }
}

#[derive(Clone, Copy)]
pub(super) enum Method {
    Initialize,
    DiscoverOwner,
    Follow,
    Start,
    Steer,
    Interrupt,
    Settings,
    CommandDecision,
    FileDecision,
    Permissions,
    Answers,
    Elicitation,
}

impl Method {
    pub const ALL: [Self; 12] = [
        Self::Initialize,
        Self::DiscoverOwner,
        Self::Follow,
        Self::Start,
        Self::Steer,
        Self::Interrupt,
        Self::Settings,
        Self::CommandDecision,
        Self::FileDecision,
        Self::Permissions,
        Self::Answers,
        Self::Elicitation,
    ];
    pub const fn wire(self) -> (&'static str, u64) {
        match self {
            Self::Initialize => ("initialize", 0),
            Self::DiscoverOwner => ("thread-owner-discovery", 1),
            Self::Follow => ("thread-stream-following-changed", 1),
            Self::Start => ("thread-follower-start-turn", 2),
            Self::Steer => ("thread-follower-steer-turn", 1),
            Self::Interrupt => ("thread-follower-interrupt-turn", 4),
            Self::Settings => ("thread-follower-update-thread-settings", 2),
            Self::CommandDecision => ("thread-follower-command-approval-decision", 1),
            Self::FileDecision => ("thread-follower-file-approval-decision", 1),
            Self::Permissions => ("thread-follower-permissions-request-approval-response", 1),
            Self::Answers => ("thread-follower-submit-user-input", 1),
            Self::Elicitation => ("thread-follower-submit-mcp-server-elicitation-response", 1),
        }
    }
}
