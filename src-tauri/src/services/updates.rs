use crate::overview::DesktopState;
use agenthub_core::platform::PlatformPaths;
use agenthub_core::services::updates::{self, UpdateAction, UpdateState, UpdateStatus};
use anyhow::Result;
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize)]
pub struct DesktopUpdateCheckResponse {
    pub job_id: String,
    pub status: UpdateStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesktopUpdateInstallResponse {
    pub job_id: String,
    pub installed: bool,
}

pub fn desktop_update_status_with_state(
    state: &DesktopState,
    latest_version: Option<&str>,
    last_error: Option<&str>,
) -> Result<UpdateStatus> {
    let config = state.config();
    let recent_check_job = if latest_version.is_none() && last_error.is_none() {
        state
            .db
            .list_jobs(25)?
            .into_iter()
            .find(|job| job.kind == "agenthub_update_check")
            .map(|job| state.db.job(&job.id))
            .transpose()?
            .flatten()
    } else {
        None
    };
    Ok(updates::update_status_with_recent_check_job_for_surface(
        &config,
        state.platform(),
        state.host_surface(),
        latest_version,
        last_error,
        recent_check_job.as_ref(),
    ))
}

pub(crate) async fn check_update_status(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> std::result::Result<DesktopUpdateCheckResponse, String> {
    let job = native_update_job_plan_for_action(state.platform(), UpdateAction::Check)
        .map_err(|err| err.to_string())?;
    let job_id = start_update_job(&state, &job).map_err(|err| err.to_string())?;

    match check_update(&app, &state, &job_id).await {
        Ok(status) => {
            let _ = state.db.finish_job(&job_id, "succeeded", Some(0), None);
            Ok(DesktopUpdateCheckResponse { job_id, status })
        }
        Err(err) => Err(finish_update_error(&state, &job_id, err)),
    }
}

pub(crate) async fn install_update_and_restart(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> std::result::Result<DesktopUpdateInstallResponse, String> {
    let job = native_update_job_plan_for_action(state.platform(), UpdateAction::Install)
        .map_err(|err| err.to_string())?;
    let job_id = start_update_job(&state, &job).map_err(|err| err.to_string())?;

    match install_update(&app, &state, &job_id).await {
        Ok(installed) => {
            let _ = state.db.finish_job(&job_id, "succeeded", Some(0), None);
            if installed {
                app.restart();
            }
            Ok(DesktopUpdateInstallResponse { job_id, installed })
        }
        Err(err) => Err(finish_update_error(&state, &job_id, err)),
    }
}

fn update_job_id(spec: &updates::MacosUpdaterJobSpec) -> String {
    let action = spec
        .kind
        .strip_prefix("agenthub_update_")
        .unwrap_or(spec.kind.as_str());
    format!(
        "desktop-update-{action}-{}-{}",
        chrono::Utc::now().timestamp_millis(),
        std::process::id()
    )
}

fn start_update_job(state: &DesktopState, spec: &updates::MacosUpdaterJobSpec) -> Result<String> {
    let id = update_job_id(spec);
    state.db.create_job(&id, &spec.kind, &spec.title)?;
    state.db.append_job_output(&id, &spec.initial_output)?;
    Ok(id)
}

fn finish_update_error(state: &DesktopState, job_id: &str, err: anyhow::Error) -> String {
    let message = err.to_string();
    let _ = state
        .db
        .append_job_output(job_id, &format!("error: {message}\n"));
    let _ = state.db.finish_job(job_id, "failed", None, Some(&message));
    message
}

fn native_update_job_plan_for_action(
    platform: &PlatformPaths,
    action: UpdateAction,
) -> Result<updates::MacosUpdaterJobSpec> {
    let plan = updates::plan_update_action_for_surface(
        platform,
        agenthub_core::services::system::HostSurface::DesktopEmbeddedTauri,
        action,
    )?;
    plan.macos_job
        .ok_or_else(|| anyhow::anyhow!("update action did not produce a macOS updater job spec"))
}

async fn check_update(app: &AppHandle, state: &DesktopState, job_id: &str) -> Result<UpdateStatus> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|err| anyhow::anyhow!("初始化更新器失败: {err}"))?;
    let mut status = desktop_update_status_with_state(state, None, None)?;
    match updater
        .check()
        .await
        .map_err(|err| anyhow::anyhow!("检查更新失败: {err}"))?
    {
        Some(update) => {
            state.db.append_job_output(
                job_id,
                &updates::macos_updater_update_available_output(&update.version),
            )?;
            status.latest_version = Some(update.version.clone());
            status.update_available =
                updates::update_available_for_versions(&status.current_version, &update.version);
            status.state = if status.update_available == Some(true) {
                UpdateState::Ready
            } else {
                UpdateState::Idle
            };
        }
        None => {
            state
                .db
                .append_job_output(job_id, updates::macos_updater_no_update_output())?;
            status.latest_version = Some(status.current_version.clone());
            status.update_available = Some(false);
            status.state = UpdateState::Idle;
        }
    }
    Ok(status)
}

async fn install_update(app: &AppHandle, state: &DesktopState, job_id: &str) -> Result<bool> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|err| anyhow::anyhow!("初始化更新器失败: {err}"))?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|err| anyhow::anyhow!("检查更新失败: {err}"))?
    else {
        state
            .db
            .append_job_output(job_id, updates::macos_updater_no_update_output())?;
        return Ok(false);
    };

    state.db.append_job_output(
        job_id,
        &format!("downloading signed app update {}\n", update.version),
    )?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|err| anyhow::anyhow!("下载更新失败: {err}"))?;
    state.db.append_job_output(
        job_id,
        &format!("installing signed app update {}\n", update.version),
    )?;
    update
        .install(bytes)
        .map_err(|err| anyhow::anyhow!("安装更新失败: {err}"))?;
    state
        .db
        .append_job_output(job_id, "signed app update installed\n")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenthub_core::{
        db::JobRecord,
        platform::{PlatformKind, PlatformPaths},
    };

    #[test]
    fn native_update_commands_are_validated_against_core_plan() {
        let platform = PlatformPaths::for_kind(PlatformKind::Macos);

        let check = native_update_job_plan_for_action(&platform, UpdateAction::Check).unwrap();
        assert_eq!(check.kind, "agenthub_update_check");
        assert!(update_job_id(&check).starts_with("desktop-update-check-"));

        let install = native_update_job_plan_for_action(&platform, UpdateAction::Install).unwrap();
        assert_eq!(install.kind, "agenthub_update_install");
        assert!(update_job_id(&install).starts_with("desktop-update-install-"));
    }

    #[test]
    fn recent_check_job_output_keeps_no_update_status_explicit() {
        let config = agenthub_core::config::Config::for_platform_kind(PlatformKind::Macos);
        let platform = PlatformPaths::for_kind(PlatformKind::Macos);
        let job = job_record(
            "succeeded",
            "checking signed Tauri updater feed\nno signed app update available\n",
        );

        let status = updates::update_status_with_recent_check_job_for_surface(
            &config,
            &platform,
            agenthub_core::services::system::HostSurface::default_for_platform(&platform),
            None,
            None,
            Some(&job),
        );

        assert_eq!(status.state, UpdateState::Idle);
        assert_eq!(
            status.latest_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(status.update_available, Some(false));
    }

    fn job_record(status: &str, output: &str) -> JobRecord {
        JobRecord {
            id: "job-1".to_string(),
            kind: "agenthub_update_check".to_string(),
            status: status.to_string(),
            title: "AgentHub app update check".to_string(),
            thread_id: None,
            turn_id: None,
            started_at: 1,
            finished_at: Some(2),
            exit_code: Some(0),
            output: output.to_string(),
            error: None,
        }
    }
}
