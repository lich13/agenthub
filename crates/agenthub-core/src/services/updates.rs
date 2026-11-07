use crate::{
    config::Config,
    db::JobRecord,
    platform::{PlatformKind, PlatformPaths},
    services::commands,
    services::system::{require_capability_for_surface, Capability, HostSurface},
    system::{compare_semver, extract_semver},
    update::{self, analyze_job_failure, JobFailureCategory},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateExecutionMethod {
    LinuxSystemdJob,
    MacosTauriUpdater,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    Idle,
    Checking,
    Ready,
    Installing,
    Succeeded,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateAction {
    Check,
    Install,
    Prune,
}

impl UpdateAction {
    pub fn as_rpc_action(self) -> &'static str {
        match self {
            Self::Check => commands::UPDATES_CHECK,
            Self::Install => commands::UPDATES_INSTALL,
            Self::Prune => commands::UPDATES_PRUNE,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateFailureCategory {
    ReleaseMissing,
    DownloadSha256Mismatch,
    SystemdFailure,
    PermissionDeniedSudo,
    CodexAuthFailure,
    SqliteIntegrityFailure,
    ReadOnlyFileSystem,
    NetworkTlsEof,
    CodexLocalStateUnavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: Option<bool>,
    pub channel: String,
    pub method: UpdateExecutionMethod,
    pub state: UpdateState,
    pub failure_category: Option<UpdateFailureCategory>,
    pub recommended_action: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateJobPlan {
    pub action: UpdateAction,
    pub method: UpdateExecutionMethod,
    pub platform: PlatformKind,
    pub exclusive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxUpdateJobSpec {
    pub kind: String,
    pub title: String,
    pub command: String,
    pub exclusive_group: Option<String>,
}

impl LinuxUpdateJobSpec {
    pub fn process_command(&self) -> Result<crate::jobs::ProcessCommand> {
        match self.kind.as_str() {
            "agenthub_update_check" => update::panel_process(true),
            "agenthub_update_install" => update::panel_process(false),
            "agenthub_update_prune" => Ok(crate::jobs::ProcessCommand::new(
                "python3",
                ["-c", update::PANEL_PRUNE_SCRIPT],
            )),
            _ => anyhow::bail!("unsupported managed update command"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MacosUpdaterJobSpec {
    pub kind: String,
    pub title: String,
    pub initial_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateActionPlan {
    pub required_capability: Capability,
    pub action: UpdateAction,
    pub method: UpdateExecutionMethod,
    pub platform: PlatformKind,
    pub linux_job: Option<LinuxUpdateJobSpec>,
    pub macos_job: Option<MacosUpdaterJobSpec>,
}

pub const MACOS_UPDATER_CHECKING_OUTPUT: &str = "checking signed Tauri updater feed\n";
pub const MACOS_UPDATER_NO_UPDATE_OUTPUT: &str = "no signed app update available\n";
const MACOS_UPDATER_AVAILABLE_PREFIX: &str = "signed app update available ";

pub fn update_status_for_surface(
    _config: &Config,
    platform: &PlatformPaths,
    host_surface: HostSurface,
    latest_version: Option<&str>,
    last_error: Option<&str>,
) -> UpdateStatus {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let normalized_latest = latest_version.and_then(non_empty_string);
    let update_available = normalized_latest
        .and_then(|latest| update_available_for_versions(&current_version, latest));
    let failure_category = last_error.and_then(|error| {
        analyze_job_failure("agenthub_update", "", Some(error), Some(1))
            .map(|analysis| analysis.category.into())
    });
    let state = if failure_category.is_some() {
        UpdateState::Failed
    } else {
        UpdateState::Idle
    };
    let method = execution_method_for_surface(platform.kind, host_surface);
    let capabilities = capabilities_for_surface(platform.kind, host_surface);
    let recommended_action = recommended_action(platform.kind, method, update_available);

    UpdateStatus {
        current_version,
        latest_version: normalized_latest.map(ToString::to_string),
        update_available,
        channel: "stable".to_string(),
        method,
        state,
        failure_category,
        recommended_action,
        capabilities,
    }
}

pub fn update_status_with_recent_check_job_for_surface(
    config: &Config,
    platform: &PlatformPaths,
    host_surface: HostSurface,
    latest_version: Option<&str>,
    last_error: Option<&str>,
    recent_check_job: Option<&JobRecord>,
) -> UpdateStatus {
    let mut status =
        update_status_for_surface(config, platform, host_surface, latest_version, last_error);
    if latest_version.is_some() || last_error.is_some() || platform.kind != PlatformKind::Macos {
        return status;
    }
    if let Some(job) = recent_check_job {
        derive_macos_recent_check_status(&mut status, job, config, platform, host_surface);
    }
    status
}

pub fn update_job_plan_for_surface(
    platform: &PlatformPaths,
    host_surface: HostSurface,
    action: UpdateAction,
) -> UpdateJobPlan {
    UpdateJobPlan {
        action,
        method: execution_method_for_surface(platform.kind, host_surface),
        platform: platform.kind,
        exclusive: platform.kind == PlatformKind::Linux
            && host_surface == HostSurface::LinuxServerApi,
    }
}

pub fn plan_update_action(
    config: &Config,
    platform: &PlatformPaths,
    action: UpdateAction,
) -> Result<UpdateActionPlan> {
    plan_update_action_for_surface(
        config,
        platform,
        HostSurface::default_for_platform(platform),
        action,
    )
}

pub fn plan_update_action_for_surface(
    config: &Config,
    platform: &PlatformPaths,
    host_surface: HostSurface,
    action: UpdateAction,
) -> Result<UpdateActionPlan> {
    let job_plan = update_job_plan_for_surface(platform, host_surface, action);
    let required_capability =
        update_action_capability_for_surface(platform.kind, host_surface, action);
    require_capability_for_surface(platform, host_surface, required_capability)?;
    let (linux_job, macos_job) = match job_plan.method {
        UpdateExecutionMethod::LinuxSystemdJob => {
            (Some(linux_update_job_spec(config, job_plan)?), None)
        }
        UpdateExecutionMethod::MacosTauriUpdater => (None, Some(macos_updater_job_spec(action)?)),
        UpdateExecutionMethod::Unsupported => {
            anyhow::bail!("updates are unavailable on this platform")
        }
    };
    Ok(UpdateActionPlan {
        required_capability,
        action,
        method: job_plan.method,
        platform: job_plan.platform,
        linux_job,
        macos_job,
    })
}

pub fn macos_updater_job_spec(action: UpdateAction) -> Result<MacosUpdaterJobSpec> {
    let (kind, title) = match action {
        UpdateAction::Check => ("agenthub_update_check", "AgentHub app update check"),
        UpdateAction::Install => ("agenthub_update_install", "AgentHub app update install"),
        UpdateAction::Prune => anyhow::bail!("native updater does not support backup prune"),
    };
    Ok(MacosUpdaterJobSpec {
        kind: kind.to_string(),
        title: title.to_string(),
        initial_output: MACOS_UPDATER_CHECKING_OUTPUT.to_string(),
    })
}

pub fn macos_updater_update_available_output(version: &str) -> String {
    format!("{MACOS_UPDATER_AVAILABLE_PREFIX}{}\n", version.trim())
}

pub fn macos_updater_no_update_output() -> &'static str {
    MACOS_UPDATER_NO_UPDATE_OUTPUT
}

pub fn linux_update_job_spec(_config: &Config, plan: UpdateJobPlan) -> Result<LinuxUpdateJobSpec> {
    if plan.method != UpdateExecutionMethod::LinuxSystemdJob {
        anyhow::bail!("only Linux WebUI can start server update jobs");
    }
    if plan.platform != PlatformKind::Linux {
        anyhow::bail!("only Linux WebUI can start server update jobs");
    }
    let exclusive_group = plan.exclusive.then(|| "agenthub-webd-update".to_string());
    let spec = match plan.action {
        UpdateAction::Check => LinuxUpdateJobSpec {
            kind: "agenthub_update_check".to_string(),
            title: "AgentHub update precheck".to_string(),
            command: update::PANEL_PRECHECK_DEFAULT.into(),
            exclusive_group,
        },
        UpdateAction::Install => LinuxUpdateJobSpec {
            kind: "agenthub_update_install".to_string(),
            title: "AgentHub update install".to_string(),
            command: update::PANEL_UPDATE_DEFAULT.into(),
            exclusive_group,
        },
        UpdateAction::Prune => LinuxUpdateJobSpec {
            kind: "agenthub_update_prune".to_string(),
            title: "AgentHub update backup prune".to_string(),
            command: update::panel_prune_command(),
            exclusive_group,
        },
    };
    spec.process_command()?;
    Ok(spec)
}

fn derive_macos_recent_check_status(
    status: &mut UpdateStatus,
    job: &JobRecord,
    config: &Config,
    platform: &PlatformPaths,
    host_surface: HostSurface,
) {
    if job.kind != "agenthub_update_check" {
        return;
    }
    if job.status == "failed" {
        status.state = UpdateState::Failed;
        return;
    }
    if job.status == "running" {
        status.state = UpdateState::Checking;
        return;
    }
    if let Some(version) = macos_signed_update_version_from_output(&job.output) {
        *status = update_status_for_surface(config, platform, host_surface, Some(&version), None);
        status.state = if status.update_available == Some(true) {
            UpdateState::Ready
        } else {
            UpdateState::Idle
        };
        return;
    }
    if job
        .output
        .contains(MACOS_UPDATER_NO_UPDATE_OUTPUT.trim_end())
    {
        status.latest_version = Some(status.current_version.clone());
        status.update_available = Some(false);
        status.state = UpdateState::Idle;
    }
}

fn macos_signed_update_version_from_output(output: &str) -> Option<String> {
    output.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix(MACOS_UPDATER_AVAILABLE_PREFIX)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn non_empty_string(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn update_action_capability_for_surface(
    platform: PlatformKind,
    host_surface: HostSurface,
    action: UpdateAction,
) -> Capability {
    if host_surface == HostSurface::DesktopEmbeddedTauri {
        return match action {
            UpdateAction::Check | UpdateAction::Install => Capability::AppUpdater,
            UpdateAction::Prune => Capability::PruneBackups,
        };
    }
    match (platform, action) {
        (PlatformKind::Linux, UpdateAction::Prune) => Capability::PruneBackups,
        (PlatformKind::Linux, UpdateAction::Check | UpdateAction::Install) => {
            Capability::LinuxUpdateJob
        }
        (PlatformKind::Macos, UpdateAction::Check | UpdateAction::Install) => {
            Capability::AppUpdater
        }
        (PlatformKind::Macos, UpdateAction::Prune) => Capability::PruneBackups,
        (PlatformKind::Windows, UpdateAction::Check | UpdateAction::Install) => {
            Capability::AppUpdater
        }
        (PlatformKind::Windows, UpdateAction::Prune) => Capability::PruneBackups,
    }
}

pub fn update_available_for_versions(current_version: &str, latest_version: &str) -> Option<bool> {
    Some(
        compare_semver(
            extract_semver(latest_version)?.as_str(),
            extract_semver(current_version)?.as_str(),
        )?
        .is_gt(),
    )
}

fn execution_method_for_surface(
    platform: PlatformKind,
    host_surface: HostSurface,
) -> UpdateExecutionMethod {
    match (platform, host_surface) {
        (PlatformKind::Linux, HostSurface::LinuxServerApi) => {
            UpdateExecutionMethod::LinuxSystemdJob
        }
        (PlatformKind::Linux | PlatformKind::Macos, HostSurface::DesktopEmbeddedTauri) => {
            UpdateExecutionMethod::MacosTauriUpdater
        }
        _ => UpdateExecutionMethod::Unsupported,
    }
}

fn capabilities_for_surface(platform: PlatformKind, host_surface: HostSurface) -> Vec<String> {
    match (platform, host_surface) {
        (PlatformKind::Linux, HostSurface::LinuxServerApi) => [
            "check",
            "confirm_install",
            "job_history",
            "sha256_verification",
            "systemd_health_check",
            "rollback",
            "prune_backups",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        (PlatformKind::Linux | PlatformKind::Macos, HostSurface::DesktopEmbeddedTauri) => [
            "check",
            "confirm_install",
            "job_history",
            "signature_verification",
            "restart_after_install",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        _ => Vec::new(),
    }
}

fn recommended_action(
    platform: PlatformKind,
    method: UpdateExecutionMethod,
    update_available: Option<bool>,
) -> String {
    match (platform, method, update_available) {
        (PlatformKind::Linux, UpdateExecutionMethod::LinuxSystemdJob, Some(true)) => {
            "Confirm install to start the Linux server update job after precheck.".to_string()
        }
        (PlatformKind::Linux, UpdateExecutionMethod::LinuxSystemdJob, _) => {
            "Run the fixed Linux update precheck before installing a new release.".to_string()
        }
        (PlatformKind::Macos, UpdateExecutionMethod::MacosTauriUpdater, Some(true)) => {
            "Confirm install in the Tauri updater after signature verification.".to_string()
        }
        (PlatformKind::Macos, UpdateExecutionMethod::MacosTauriUpdater, _) => {
            "Use the Tauri updater to check signed app releases.".to_string()
        }
        (PlatformKind::Linux, UpdateExecutionMethod::MacosTauriUpdater, Some(true)) => {
            "Confirm install in the Tauri updater after signature verification.".to_string()
        }
        (PlatformKind::Linux, UpdateExecutionMethod::MacosTauriUpdater, _) => {
            "Use the Tauri updater to check signed app releases.".to_string()
        }
        _ => "Updates are unavailable on this platform in the current release.".to_string(),
    }
}

impl From<JobFailureCategory> for UpdateFailureCategory {
    fn from(value: JobFailureCategory) -> Self {
        match value {
            JobFailureCategory::ReleaseMissing => Self::ReleaseMissing,
            JobFailureCategory::DownloadSha256Mismatch => Self::DownloadSha256Mismatch,
            JobFailureCategory::SystemdFailure => Self::SystemdFailure,
            JobFailureCategory::PermissionDeniedSudo => Self::PermissionDeniedSudo,
            JobFailureCategory::CodexAuthFailure => Self::CodexAuthFailure,
            JobFailureCategory::SqliteIntegrityFailure => Self::SqliteIntegrityFailure,
            JobFailureCategory::ReadOnlyFileSystem => Self::ReadOnlyFileSystem,
            JobFailureCategory::NetworkTlsEof => Self::NetworkTlsEof,
            JobFailureCategory::CodexLocalStateUnavailable => Self::CodexLocalStateUnavailable,
            JobFailureCategory::Unknown => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_update_action, UpdateAction, UpdateExecutionMethod};
    use crate::{
        config::Config,
        platform::{PlatformKind, PlatformPaths},
        services::system::Capability,
    };

    #[test]
    fn linux_update_action_plan_includes_capability_and_job_spec() {
        let config = Config::for_platform_kind(PlatformKind::Linux);
        let platform = PlatformPaths::for_kind(PlatformKind::Linux);

        let plan = plan_update_action(&config, &platform, UpdateAction::Install)
            .expect("Linux should allow update shell jobs");

        assert_eq!(plan.required_capability, Capability::LinuxUpdateJob);
        assert_eq!(plan.method, UpdateExecutionMethod::LinuxSystemdJob);
        let job = plan
            .linux_job
            .expect("Linux update action should create a shell job spec");
        assert_eq!(job.kind, "agenthub_update_install");
        assert_eq!(job.exclusive_group.as_deref(), Some("agenthub-webd-update"));
        assert!(job.command.contains("agenthub-webd-update"));
    }

    #[test]
    fn macos_update_action_plan_uses_native_updater_and_rejects_prune() {
        let config = Config::for_platform_kind(PlatformKind::Macos);
        let platform = PlatformPaths::for_kind(PlatformKind::Macos);

        let plan = plan_update_action(&config, &platform, UpdateAction::Install)
            .expect("macOS should plan native updater installs");

        assert_eq!(plan.required_capability, Capability::AppUpdater);
        assert_eq!(plan.method, UpdateExecutionMethod::MacosTauriUpdater);
        assert!(plan.linux_job.is_none());
        assert_eq!(
            plan.macos_job.as_ref().unwrap().kind,
            "agenthub_update_install"
        );
        assert_eq!(
            plan.macos_job.as_ref().unwrap().initial_output,
            super::MACOS_UPDATER_CHECKING_OUTPUT
        );

        let err = plan_update_action(&config, &platform, UpdateAction::Prune)
            .expect_err("macOS should not plan Linux backup pruning");
        let message = err.to_string();
        assert!(message.contains("prune_backups is unavailable on macos"));
        assert!(!message.contains("systemd"));
        assert!(!message.contains("Nginx"));
        assert!(!message.contains("sudo"));
        assert!(!message.contains("/opt/agenthub"));
    }

    #[test]
    fn windows_update_actions_fail_before_any_host_specific_plan() {
        let config = Config::for_platform_kind(PlatformKind::Windows);
        let platform = PlatformPaths::for_kind(PlatformKind::Windows);

        let err = plan_update_action(&config, &platform, UpdateAction::Check)
            .expect_err("Windows update check is not supported in this release");
        let message = err.to_string();
        assert!(message.contains("app_updater is unavailable on windows"));
        assert!(!message.contains("systemd"));
        assert!(!message.contains("Nginx"));
        assert!(!message.contains("sudo"));
        assert!(!message.contains("/opt/agenthub"));
    }
}
