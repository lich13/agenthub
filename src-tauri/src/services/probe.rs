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

pub async fn desktop_github_ci_status(
    state: &DesktopState,
    request: agenthub_core::github_ci::Request,
) -> std::result::Result<agenthub_core::github_ci::Snapshot, String> {
    let config = state.config();
    let snapshot = agenthub_core::services::use_cases::AgentHubUseCases::with_config_for_surface(
        &config,
        state.platform(),
        state.host_surface(),
    )
    .github_ci(&state.db, request.clone())
    .map_err(|error| error.to_string())?;
    if config.probe.monitor_enabled() {
        *state.github_ci.lock().await = None;
    }
    if !request.cached_only && !config.probe.monitor_enabled() && snapshot.refreshing {
        let mut monitor = state.github_ci.lock().await;
        let config = state.config();
        if config.probe.monitor_enabled() {
            *monitor = None;
            return agenthub_core::github_ci::status(&state.db, &config, Default::default())
                .map_err(|error| error.to_string());
        }
        if monitor.is_none() {
            *monitor =
                Some(agenthub_core::github_ci::Monitor::new().map_err(|error| error.to_string())?);
        }
        monitor
            .as_mut()
            .expect("initialized CI monitor")
            .tick(&state.db, &config)
            .await
            .map_err(|error| error.to_string())?;
        let config = state.config();
        if config.probe.monitor_enabled() {
            *monitor = None;
        }
        return agenthub_core::github_ci::status(&state.db, &config, Default::default())
            .map_err(|error| error.to_string());
    }
    Ok(snapshot)
}

#[cfg(test)]
mod ci_tests {
    use super::*;
    use agenthub_core::{
        config::Config,
        db::PanelDb,
        platform::{PlatformKind, PlatformPaths},
    };
    use serde_json::json;

    #[tokio::test]
    async fn cached_disabled_status_needs_no_monitor_and_ci_only_uses_the_daemon() {
        let mut config = Config::default();
        config.probe.error_monitor.enabled = false;
        config.probe.notifications.enabled = false;
        config.probe.notifications.notify_github_ci = false;
        let db = PanelDb::open(":memory:").unwrap();
        db.set_setting(
            "github_ci_state",
            &json!({"snapshot":{"last_scan_ms":1}}).to_string(),
        )
        .unwrap();
        let state = DesktopState::new(
            config.clone(),
            db,
            PlatformPaths::for_kind(PlatformKind::Macos),
        );
        let result = desktop_github_ci_status(
            &state,
            agenthub_core::github_ci::Request {
                refresh: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!result.refreshing);
        assert!(state.github_ci.lock().await.is_none());
        *state.github_ci.lock().await = Some(agenthub_core::github_ci::Monitor::new().unwrap());
        config.probe.enabled = true;
        config.probe.notifications.notify_github_ci = true;
        assert!(config.probe.monitor_enabled());
        state.replace_config(config);
        let result = desktop_github_ci_status(
            &state,
            agenthub_core::github_ci::Request {
                refresh: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(result.refreshing);
        assert!(state.github_ci.lock().await.is_none());
    }
    #[tokio::test]
    async fn cached_only_never_initializes_a_desktop_monitor() {
        let state = DesktopState::new(
            Config::default(),
            PanelDb::open(":memory:").unwrap(),
            PlatformPaths::for_kind(PlatformKind::Macos),
        );
        let snapshot = desktop_github_ci_status(
            &state,
            agenthub_core::github_ci::Request {
                refresh: true,
                cached_only: true,
            },
        )
        .await
        .unwrap();
        assert!(!snapshot.refreshing);
        assert!(state.github_ci.lock().await.is_none());
        assert!(state.db.get_setting("github_ci_refresh").unwrap().is_none());
    }
}
