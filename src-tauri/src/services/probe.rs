use crate::overview::DesktopState;
use agenthub_core::{
    probe::{ProbeRuntime, ProbeStatus},
    services::{probe as probe_service, use_cases::AgentHubUseCases},
};
use anyhow::Result;

pub async fn desktop_probe_status_with_state(state: &DesktopState) -> Result<ProbeStatus> {
    let config = state.config();
    let limit = config.probe.recent_limit.clamp(1, 200);
    let facade_status = AgentHubUseCases::with_config(&config, state.platform())
        .probe()?
        .status()?
        .status;
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
    let recent_event_count = state
        .db
        .list_probe_events(limit as u32)
        .ok()
        .map(|events| events.len());
    Ok(probe_service::probe_status_with_runtime_read_model(
        status,
        facade_status,
        recent_event_count,
    ))
}
