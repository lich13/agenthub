use crate::overview::DesktopState;
use agenthub_core::config::Config;
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
    let job =
        native_update_job_plan_for_action(&state.config(), state.platform(), UpdateAction::Check)
            .map_err(|err| err.to_string())?;
    let execution = NativeUpdateJobExecution::start(&state, &job).map_err(|err| err.to_string())?;

    match check_update(&app, &state, execution.job_id()).await {
        Ok(status) => {
            execution.finish_success(&state);
            let job_id = execution.job_id().to_string();
            Ok(DesktopUpdateCheckResponse { job_id, status })
        }
        Err(err) => Err(execution.finish_error(&state, err)),
    }
}

pub(crate) async fn install_update_and_restart(
    app: AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> std::result::Result<DesktopUpdateInstallResponse, String> {
    let job =
        native_update_job_plan_for_action(&state.config(), state.platform(), UpdateAction::Install)
            .map_err(|err| err.to_string())?;
    let execution = NativeUpdateJobExecution::start(&state, &job).map_err(|err| err.to_string())?;

    match install_update(&app, &state, execution.job_id()).await {
        Ok(installed) => {
            execution.finish_success(&state);
            if installed {
                app.restart();
            }
            let job_id = execution.job_id().to_string();
            Ok(DesktopUpdateInstallResponse { job_id, installed })
        }
        Err(err) => Err(execution.finish_error(&state, err)),
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

#[derive(Debug)]
struct NativeUpdateJobPlan {
    spec: updates::MacosUpdaterJobSpec,
}

#[derive(Debug)]
struct NativeUpdateJobExecution {
    job_id: String,
}

impl NativeUpdateJobExecution {
    fn start(state: &DesktopState, plan: &NativeUpdateJobPlan) -> Result<Self> {
        let id = update_job_id(&plan.spec);
        state
            .db
            .create_job(&id, &plan.spec.kind, &plan.spec.title)?;
        state.db.append_job_output(&id, &plan.spec.initial_output)?;
        Ok(Self { job_id: id })
    }

    fn job_id(&self) -> &str {
        &self.job_id
    }

    fn finish_success(&self, state: &DesktopState) {
        let _ = state
            .db
            .finish_job(&self.job_id, "succeeded", Some(0), None);
    }

    fn finish_error(&self, state: &DesktopState, err: anyhow::Error) -> String {
        let message = err.to_string();
        let _ = state
            .db
            .append_job_output(&self.job_id, &format!("error: {message}\n"));
        let _ = state
            .db
            .finish_job(&self.job_id, "failed", None, Some(&message));
        message
    }
}

fn native_update_job_plan_for_action(
    config: &Config,
    platform: &PlatformPaths,
    action: UpdateAction,
) -> Result<NativeUpdateJobPlan> {
    let plan = updates::plan_update_action_for_surface(
        config,
        platform,
        agenthub_core::services::system::HostSurface::DesktopEmbeddedTauri,
        action,
    )?;
    let _native = plan
        .native
        .ok_or_else(|| anyhow::anyhow!("update action did not produce a native updater spec"))?;
    let spec = plan
        .macos_job
        .ok_or_else(|| anyhow::anyhow!("update action did not produce a macOS updater job spec"))?;
    Ok(NativeUpdateJobPlan { spec })
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
        let config = Config::for_platform_kind(PlatformKind::Macos);
        let platform = PlatformPaths::for_kind(PlatformKind::Macos);

        let check =
            native_update_job_plan_for_action(&config, &platform, UpdateAction::Check).unwrap();
        assert_eq!(check.spec.kind, "agenthub_update_check");
        assert!(update_job_id(&check.spec).starts_with("desktop-update-check-"));

        let install =
            native_update_job_plan_for_action(&config, &platform, UpdateAction::Install).unwrap();
        assert_eq!(install.spec.kind, "agenthub_update_install");
        assert!(update_job_id(&install.spec).starts_with("desktop-update-install-"));
    }

    #[test]
    fn recent_check_job_output_keeps_no_update_status_explicit() {
        let config = Config::for_platform_kind(PlatformKind::Macos);
        let platform = PlatformPaths::for_kind(PlatformKind::Macos);
        let job = job_record(
            "succeeded",
            "checking signed Tauri updater feed\nno signed app update available\n",
        );

        let status = updates::update_status_with_recent_check_job(
            &config,
            &platform,
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

    #[test]
    fn update_command_source_uses_core_recent_check_and_marker_helpers() {
        let command_source = include_str!("updates.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("updates production source");

        assert!(
            command_source.contains("updates::update_status_with_recent_check_job"),
            "Tauri commands should let core derive recent update check status"
        );
        assert!(
            command_source.contains(".macos_job"),
            "Tauri commands should use core action-plan job metadata for native updater jobs"
        );
        assert!(
            command_source.contains("updates::macos_updater_update_available_output")
                && command_source.contains("updates::macos_updater_no_update_output"),
            "Tauri commands should use core updater output marker helpers"
        );
        assert!(
            !command_source.contains("fn apply_recent_check_job")
                && !command_source.contains("fn signed_update_version_from_output"),
            "Tauri commands must not duplicate recent check status parsing"
        );
        assert!(
            !command_source.contains("macos_updater_job_spec(action)"),
            "Tauri commands must consume the core update action plan macos_job instead of rebuilding job metadata"
        );
        for forbidden in [concat!("UpdateAction", "::", "Prune"), "prune_backups"] {
            assert!(
                !command_source.contains(forbidden),
                "macOS Tauri update executor must not carry Linux prune semantics: {forbidden}"
            );
        }
    }

    #[test]
    fn native_update_job_state_is_owned_by_a_thin_executor() {
        let command_source = include_str!("updates.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("updates production source");

        assert!(
            command_source.contains("struct NativeUpdateJobExecution")
                && command_source.contains("NativeUpdateJobExecution::start")
                && command_source.contains(".finish_success(")
                && command_source.contains(".finish_error("),
            "Tauri updates should keep native updater job lifecycle conversion inside a thin executor"
        );
        for forbidden in [
            "fn native_update_id_action",
            "id_action",
            "let job_id = update_job_id(",
            "create_job(&job_id",
            "finish_job(&job_id",
        ] {
            assert!(
                !command_source.contains(forbidden),
                "Tauri updates must not scatter native updater job action/status conversion: {forbidden}"
            );
        }
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
