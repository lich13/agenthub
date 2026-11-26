use crate::overview::DesktopState;
use agenthub_core::{
    probe::{ProbeRuntime, ProbeStatus},
    services::probe as probe_service,
};
use anyhow::Result;

pub async fn desktop_probe_status_with_state(state: &DesktopState) -> Result<ProbeStatus> {
    desktop_probe_status_with_options(state, false).await
}

pub async fn desktop_probe_status_with_options(
    state: &DesktopState,
    force_refresh: bool,
) -> Result<ProbeStatus> {
    let config = state.refresh_config()?;
    let mut status = ProbeRuntime::new(config, state.platform().clone())
        .status_with_options(force_refresh)
        .await?;
    probe_service::enrich_probe_status_from_db(&mut status, &state.db)?;
    Ok(status)
}

pub fn desktop_github_ci_status(
    state: &DesktopState,
    request: agenthub_core::github_ci::Request,
) -> std::result::Result<agenthub_core::github_ci::Snapshot, String> {
    let config = state.config();
    agenthub_core::services::use_cases::AgentHubUseCases::with_config(&config, state.platform())
        .github_ci(&state.db, request)
        .map_err(|error| error.to_string())
}
