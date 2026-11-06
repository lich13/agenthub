use super::{ok, ApiResponse};
use crate::{linux_adapter, state::AppState};
use agenthub_core::{
    platform::{PlatformKind, PlatformPaths},
    services::{
        system::{require_capability_for_surface, Capability},
        updates::{self as update_service, UpdateAction},
    },
};
use anyhow::Result as AnyhowResult;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

pub(crate) async fn system_capabilities(state: AppState) -> ApiResponse {
    let platform = state.platform().clone();
    ok(
        agenthub_core::services::use_cases::AgentHubUseCases::with_config_for_surface(
            &state.config(),
            &platform,
            state.host_surface(),
        )
        .system()?
        .runtime_capabilities(),
    )
}

pub(crate) async fn system_version(_state: AppState) -> ApiResponse {
    ok(linux_version_info().await?)
}

pub(crate) async fn system_update_status(state: AppState) -> ApiResponse {
    let latest = github_latest_release("lich13", "agenthub").await.ok();
    let platform = state.platform().clone();
    ok(update_service::update_status_for_surface(
        &state.config(),
        &platform,
        state.host_surface(),
        latest.as_deref(),
        None,
    ))
}

async fn linux_version_info() -> AnyhowResult<agenthub_core::system::VersionInfo> {
    let inputs = agenthub_core::system::VersionInfoInputs {
        panel_latest: github_latest_release("lich13", "agenthub").await.ok(),
        codex_latest: npm_latest_version("@openai/codex").await.ok(),
    };
    agenthub_core::system::version_info_with_inputs(inputs).await
}

async fn github_latest_release(owner: &str, repo: &str) -> AnyhowResult<String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let release: Release = reqwest::Client::builder()
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

async fn npm_latest_version(package: &str) -> AnyhowResult<String> {
    #[derive(Deserialize)]
    struct DistTags {
        latest: String,
    }
    #[derive(Deserialize)]
    struct PackageInfo {
        #[serde(rename = "dist-tags")]
        dist_tags: DistTags,
    }
    let encoded = package.replace('/', "%2F");
    let url = format!("https://registry.npmjs.org/{encoded}");
    let package: PackageInfo = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?
        .get(url)
        .header("user-agent", "agenthub")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(package.dist_tags.latest)
}

pub(crate) async fn start_update_action(
    state: AppState,
    action: UpdateAction,
    audit_action: Option<&str>,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = state.platform().clone();
    require_capability_for_surface(
        &platform,
        state.host_surface(),
        match action {
            UpdateAction::Prune => Capability::PruneBackups,
            UpdateAction::Check | UpdateAction::Install => Capability::LinuxUpdateJob,
        },
    )?;
    let plan = linux_adapter::linux_update_action_plan(&state, &platform, action)?;
    let id = linux_adapter::start_update_action_plan(&state, &auth, plan, audit_action)?;
    ok(json!({"job_id": id}))
}

pub(crate) fn linux_update_platform() -> PlatformPaths {
    PlatformPaths::for_kind(PlatformKind::Linux)
}
