use crate::overview::DesktopState;
use agenthub_core::{
    local::{local_plugin_catalog, LocalPluginInfo},
    platform::PlatformPaths,
    services::{system::SystemCapabilitiesResponse, use_cases::AgentHubUseCases},
};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DesktopSystemVersion {
    pub panel_current: String,
    pub panel_latest: Option<String>,
    pub panel_update_available: Option<bool>,
    pub codex_current: Option<String>,
    pub codex_latest: Option<String>,
    pub codex_update_available: Option<bool>,
}

pub(crate) fn system_capabilities(state: &DesktopState) -> Result<SystemCapabilitiesResponse> {
    let config = state.config();
    Ok(
        AgentHubUseCases::with_config_for_surface(&config, state.platform(), state.host_surface())
            .system()?
            .runtime_capabilities(),
    )
}

pub(crate) fn system_version() -> DesktopSystemVersion {
    DesktopSystemVersion {
        panel_current: env!("CARGO_PKG_VERSION").to_string(),
        panel_latest: None,
        panel_update_available: None,
        codex_current: None,
        codex_latest: None,
        codex_update_available: None,
    }
}

pub(crate) fn providers() -> Vec<LocalPluginInfo> {
    local_plugin_catalog()
}

pub(crate) fn platform_overview(state: &DesktopState) -> PlatformPaths {
    state.platform().clone()
}
