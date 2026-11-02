#![allow(non_snake_case)]

use crate::{overview::DesktopState, services::system as system_service};

#[tauri::command(rename = "system.capabilities")]
pub async fn getSystemCapabilities(
    state: tauri::State<'_, DesktopState>,
) -> Result<agenthub_core::services::system::SystemCapabilitiesResponse, String> {
    system_service::system_capabilities(&state).map_err(|err| err.to_string())
}

#[tauri::command(rename = "system.version")]
pub fn getSystemVersion() -> Result<system_service::DesktopSystemVersion, String> {
    Ok(system_service::system_version())
}

#[tauri::command(rename = "system.providers")]
pub fn listProviders() -> Result<Vec<agenthub_core::local::LocalPluginInfo>, String> {
    Ok(system_service::providers())
}

#[tauri::command(rename = "system.platform")]
pub async fn getPlatformOverview(
    state: tauri::State<'_, DesktopState>,
) -> Result<agenthub_core::platform::PlatformPaths, String> {
    Ok(system_service::platform_overview(&state))
}
