#![allow(non_snake_case)]

use crate::{
    overview::DesktopState,
    services::updates::{
        self as update_service, DesktopUpdateCheckResponse, DesktopUpdateInstallResponse,
    },
};
use agenthub_core::services::updates::UpdateStatus;
use tauri::AppHandle;

#[tauri::command(rename = "updates.status")]
pub fn getUpdateStatus(
    state: tauri::State<'_, DesktopState>,
) -> std::result::Result<UpdateStatus, String> {
    update_service::desktop_update_status_with_state(&state, None, None)
        .map_err(|err| err.to_string())
}

#[tauri::command(rename = "updates.check")]
pub async fn updatesCheck(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> std::result::Result<DesktopUpdateCheckResponse, String> {
    update_service::check_update_status(app, state).await
}

#[tauri::command(rename = "updates.install")]
pub async fn updatesInstall(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> std::result::Result<DesktopUpdateInstallResponse, String> {
    update_service::install_update_and_restart(app, state).await
}
