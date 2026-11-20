use super::{ok, ApiResponse};
use crate::{linux_adapter, state::AppState};
use agenthub_core::{
    platform::PlatformPaths,
    services::{
        system::{require_capability, Capability},
        updates::{self as update_service, UpdateAction},
    },
};
use anyhow::Result as AnyhowResult;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

pub(crate) async fn system_capabilities(state: AppState) -> ApiResponse {
    ok(
        agenthub_core::services::use_cases::AgentHubUseCases::new(state.platform())
            .system()
            .runtime_capabilities(),
    )
}

pub(crate) async fn system_update_status(state: AppState) -> ApiResponse {
    let latest = github_latest_release("lich13", "agenthub").await.ok();
    ok(update_service::update_status(
        state.platform(),
        latest.as_deref(),
        None,
    ))
}

async fn github_latest_release(owner: &str, repo: &str) -> AnyhowResult<String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let release: Release = agenthub_core::http_client_builder()
        .timeout(Duration::from_secs(8))
        .build()?
        .get(url)
        .header("user-agent", "agenthub")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(release.tag_name)
}

pub(crate) async fn start_update_action(
    state: AppState,
    action: UpdateAction,
    audit_action: Option<&str>,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = state.platform().clone();
    require_capability(
        &platform,
        match action {
            UpdateAction::Prune => Capability::PruneBackups,
            UpdateAction::Check | UpdateAction::Install => Capability::LinuxUpdateJob,
        },
    )?;
    let plan = linux_adapter::linux_update_action_plan(&platform, action)?;
    let id = linux_adapter::start_update_action_plan(&state, &auth, plan, audit_action)?;
    ok(json!({"job_id": id}))
}

pub(crate) fn linux_update_platform() -> PlatformPaths {
    PlatformPaths::linux_server()
}
