use crate::overview::DesktopState;
use agenthub_core::services::{system::SystemCapabilitiesResponse, use_cases::AgentHubUseCases};
use anyhow::Result;

pub(crate) fn system_capabilities(state: &DesktopState) -> Result<SystemCapabilitiesResponse> {
    let config = state.config();
    Ok(
        AgentHubUseCases::with_config_for_surface(&config, state.platform(), state.host_surface())
            .system()?
            .runtime_capabilities(),
    )
}
