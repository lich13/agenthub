#![allow(non_snake_case)]

use crate::{overview::DesktopState, services::probe as probe_service};
use agenthub_core::probe::ProbeStatus;

#[tauri::command(rename = "probe.status")]
pub async fn getProbeStatus(
    state: tauri::State<'_, DesktopState>,
) -> std::result::Result<ProbeStatus, String> {
    probe_service::desktop_probe_status_with_state(&state)
        .await
        .map_err(|err| err.to_string())
}
