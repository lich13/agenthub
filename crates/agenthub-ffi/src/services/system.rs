use crate::overview::DesktopState;
use agenthub_core::services::{system::SystemCapabilitiesResponse, use_cases::AgentHubUseCases};
use anyhow::Result;

pub(crate) fn system_capabilities(state: &DesktopState) -> Result<SystemCapabilitiesResponse> {
    Ok(AgentHubUseCases::new(state.platform())
        .system()
        .runtime_capabilities())
}
