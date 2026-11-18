#![allow(non_snake_case)]

use crate::{
    overview::DesktopState,
    services::{
        actions::DesktopActionResponse,
        settings::{
            self as settings_service, DesktopProbeEventsRequest, DesktopProbeEventsResponse,
            DesktopProbeSettings,
        },
    },
};
use agenthub_core::services::{probe as probe_service, settings::ProbeSettingsSaveRequest};
use anyhow::Result;

#[tauri::command(rename = "probe.settings.get")]
pub async fn getProbeSettings(
    state: tauri::State<'_, DesktopState>,
) -> Result<DesktopProbeSettings, String> {
    let state = state.inner().clone();
    super::blocking(move || settings_service::probe_settings_with_state(&state)).await
}

#[tauri::command(rename = "probe.settings.save")]
pub async fn saveProbeSettings(
    state: tauri::State<'_, DesktopState>,
    settings: ProbeSettingsSaveRequest,
) -> Result<DesktopProbeSettings, String> {
    let state = state.inner().clone();
    super::blocking(move || settings_service::probe_save_settings_with_state(&state, settings))
        .await
}

async fn probe_action(
    state: tauri::State<'_, DesktopState>,
    action: probe_service::ProbeAction,
) -> Result<DesktopActionResponse, String> {
    let state = state.inner().clone();
    super::blocking(move || settings_service::probe_action_with_state(&state, action)).await
}

#[tauri::command(rename = "probe.barkTest")]
pub async fn probeBarkTest(
    state: tauri::State<'_, DesktopState>,
) -> Result<DesktopActionResponse, String> {
    probe_action(state, probe_service::ProbeAction::BarkTest).await
}

#[tauri::command(rename = "probe.installHooks")]
pub async fn probeInstallHooks(
    state: tauri::State<'_, DesktopState>,
) -> Result<DesktopActionResponse, String> {
    probe_action(state, probe_service::ProbeAction::InstallHooks).await
}

#[tauri::command(rename = "probe.removeHooks")]
pub async fn probeRemoveHooks(
    state: tauri::State<'_, DesktopState>,
) -> Result<DesktopActionResponse, String> {
    probe_action(state, probe_service::ProbeAction::RemoveHooks).await
}

#[tauri::command(rename = "probe.events")]
pub async fn getProbeEvents(
    state: tauri::State<'_, DesktopState>,
    limit: Option<u32>,
) -> Result<DesktopProbeEventsResponse, String> {
    let state = state.inner().clone();
    super::blocking(move || {
        settings_service::probe_events_with_state(&state, DesktopProbeEventsRequest { limit })
    })
    .await
}

#[tauri::command(rename = "cleanup.hiddenDryRun")]
pub async fn dryRunHiddenThreadDelete(
    state: tauri::State<'_, DesktopState>,
    request: agenthub_core::archive::hidden_scan::HiddenScanRequest,
) -> Result<agenthub_core::archive::HiddenThreadDeletePlan, String> {
    agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
        .cleanup()
        .scan_hidden(state.codex_paths(), "desktop".into(), request)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command(rename = "claude.hooks")]
pub async fn claudeHooks(
    state: tauri::State<'_, DesktopState>,
    request: agenthub_core::claude::hooks::ClaudeHooksRequest,
    provider: Option<agenthub_core::claude::hooks::HookProvider>,
) -> Result<agenthub_core::claude::hooks::ClaudeHooksStatus, String> {
    let state = state.inner().clone();
    let provider = provider.unwrap_or_default();
    agenthub_core::read_tasks::spawn_blocking(move || {
        agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
            .probe()
            .provider_hooks(provider, request)
    })
    .await
    .map_err(|_| "Hooks 设置任务未完成".to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command(rename = "cleanup.unifiedDryRun")]
pub async fn dryRunUnifiedCleanup(
    state: tauri::State<'_, DesktopState>,
) -> Result<agenthub_core::services::cleanup::UnifiedCleanupPlan, String> {
    let state = state.inner().clone();
    agenthub_core::read_tasks::spawn_blocking(move || {
        let paths = agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(
            state.codex_paths(),
        );
        agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
            .cleanup()
            .unified_dry_run(&paths)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|_| "全平台清理预览任务未完成".to_string())?
}

#[tauri::command(rename = "cleanup.unifiedExecute")]
pub async fn startUnifiedCleanup(
    state: tauri::State<'_, DesktopState>,
    request: agenthub_core::services::cleanup::UnifiedCleanupExecuteRequest,
) -> Result<agenthub_core::services::cleanup::UnifiedCleanupResult, String> {
    let state = state.inner().clone();
    agenthub_core::read_tasks::spawn_blocking(move || {
        let paths = agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(
            state.codex_paths(),
        );
        agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
            .cleanup()
            .unified_execute(&paths, request)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|_| "全平台清理任务未完成".to_string())?
}
