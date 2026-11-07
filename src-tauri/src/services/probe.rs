use crate::overview::DesktopState;
use agenthub_core::{
    probe::{ProbeRuntime, ProbeStatus},
    services::system::{require_capability, Capability},
};
use anyhow::Result;

pub async fn desktop_probe_status_with_state(state: &DesktopState) -> Result<ProbeStatus> {
    require_capability(state.platform(), Capability::Probe)?;
    let config = state.config();
    let mut status = ProbeRuntime::new(config, state.platform().clone())
        .status()
        .await?;
    if let Some(raw) = state.db.get_setting("probe_error_monitor_status")? {
        if let Ok(runtime) = serde_json::from_str::<
            agenthub_core::probe_error_monitor::ProbeErrorMonitorRuntimeStatus,
        >(&raw)
        {
            status.error_monitor_status = runtime.status;
            status.error_monitor_last_scan_at = Some(runtime.last_scan_at);
            status.error_monitor_last_error = runtime.last_error;
            status.error_monitor_incident_count = runtime.incident_count;
        }
    }
    status.provider_notifications = state.db.native_notification_status()?;
    Ok(status)
}

pub fn desktop_github_ci_status(
    state: &DesktopState,
    request: agenthub_core::github_ci::Request,
) -> std::result::Result<agenthub_core::github_ci::Snapshot, String> {
    let config = state.config();
    agenthub_core::services::use_cases::AgentHubUseCases::with_config_for_surface(
        &config,
        state.platform(),
        state.host_surface(),
    )
    .github_ci(&state.db, request)
    .map_err(|error| error.to_string())
}
