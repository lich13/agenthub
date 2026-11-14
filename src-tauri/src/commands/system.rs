#![allow(non_snake_case)]

use crate::{overview::DesktopState, services::system as system_service};

#[tauri::command(rename = "system.capabilities")]
pub async fn getSystemCapabilities(
    state: tauri::State<'_, DesktopState>,
) -> Result<agenthub_core::services::system::SystemCapabilitiesResponse, String> {
    system_service::system_capabilities(&state).map_err(|err| err.to_string())
}
