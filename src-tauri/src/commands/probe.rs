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

#[tauri::command(rename = "github.ci.status", rename_all = "snake_case")]
pub async fn getGithubCiStatus(
    state: tauri::State<'_, DesktopState>,
    refresh: Option<bool>,
) -> Result<agenthub_core::github_ci::Snapshot, String> {
    probe_service::desktop_github_ci_status(
        &state,
        agenthub_core::github_ci::Request {
            refresh: refresh.unwrap_or(false),
            cached_only: false,
        },
    )
    .await
}
