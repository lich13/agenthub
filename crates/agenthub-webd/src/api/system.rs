use super::{ok, ApiResponse};
use crate::{linux_adapter, state::AppState};
use agenthub_core::services::{
    system::{require_capability, Capability},
    updates::{self as update_service, UpdateAction},
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
    let (latest, net_error) = if state.is_test_sandbox() {
        (None, None)
    } else {
        match github_latest_release("lich13", "agenthub").await {
            Ok(tag) => (Some(tag), None),
            Err(err) => (
                None,
                Some(agenthub_core::security::redact_output(&err.to_string())),
            ),
        }
    };
    let job_error = state
        .db
        .latest_job_by_kinds(
            &[
                "agenthub_update_check",
                "agenthub_update_install",
                "agenthub_update_prune",
            ],
            false,
        )
        .ok()
        .flatten()
        .filter(|job| job.status == "failed")
        .and_then(|job| {
            let full = state.db.job(&job.id).ok().flatten().unwrap_or(job);
            full.error
                .filter(|s| !s.trim().is_empty())
                .or_else(|| (!full.output.trim().is_empty()).then_some(full.output))
        });
    let last_error = job_error.or(net_error);
    ok(update_service::update_status(
        state.platform(),
        latest.as_deref(),
        last_error.as_deref(),
    ))
}

fn github_release_client() -> AnyhowResult<reqwest::Client> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(cached) = CLIENT.get() {
        return Ok(cached.clone());
    }
    let built = agenthub_core::http_client_builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    Ok(CLIENT.get_or_init(|| built).clone())
}

async fn github_latest_release(owner: &str, repo: &str) -> AnyhowResult<String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let release: Release = github_release_client()?
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
    let platform = state.platform().clone();
    require_capability(
        &platform,
        match action {
            UpdateAction::Prune => Capability::PruneBackups,
            UpdateAction::Check | UpdateAction::Install => Capability::LinuxUpdateJob,
        },
    )?;
    let plan = update_service::plan_update_action(&platform, action)?;
    let id = linux_adapter::start_update_action_plan(&state, plan, audit_action)?;
    ok(json!({"job_id": id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::sandboxed_test_config;
    use agenthub_core::{crypto::SecretBox, db::PanelDb};
    use serde_json::Value;

    #[tokio::test]
    async fn system_update_status_surfaces_failed_update_job_past_twenty_five_unrelated_jobs() {
        let (config, root) = sandboxed_test_config();
        let db = PanelDb::open_with_secret_box(
            root.join("panel.sqlite"),
            SecretBox::from_key_material(&"22".repeat(32)).unwrap(),
        )
        .unwrap();
        db.create_job(
            "upd-fail-1",
            "agenthub_update_install",
            "Install webd update",
        )
        .unwrap();
        db.finish_job(
            "upd-fail-1",
            "failed",
            Some(1),
            Some("minisign signature verification failed"),
        )
        .unwrap();

        for idx in 0..30 {
            let id = format!("other-{idx:02}");
            db.create_job(&id, "probe_bark_test", "Bark test").unwrap();
            db.finish_job(&id, "succeeded", Some(0), None).unwrap();
        }

        let state = AppState::new(config, db.clone()).with_test_sandbox(root.clone());
        let raw = system_update_status(state.clone()).await.unwrap();
        let resp: Value = serde_json::from_str(raw.get()).unwrap();
        assert_eq!(resp["state"], "failed");
        assert!(resp["failure_category"].as_str().is_some());

        // A newer succeeded update job supersedes the older failed update job.
        db.create_job("upd-ok-2", "agenthub_update_install", "Install webd update")
            .unwrap();
        db.finish_job("upd-ok-2", "succeeded", Some(0), None)
            .unwrap();
        let cleared_raw = system_update_status(state).await.unwrap();
        let cleared: Value = serde_json::from_str(cleared_raw.get()).unwrap();
        assert_ne!(cleared["state"], "failed");
        let _ = std::fs::remove_dir_all(root);
    }
}
