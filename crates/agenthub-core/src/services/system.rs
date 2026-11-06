use crate::{
    config::Config,
    platform::{PlatformKind, PlatformPaths},
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

pub const API_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCapabilitiesResponse {
    pub api_version: u32,
    pub host_surface: HostSurface,
    pub capabilities: SystemCapabilities,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostSurface {
    LinuxServerApi,
    DesktopEmbeddedTauri,
}

impl HostSurface {
    pub const ALL: &'static [HostSurface] = &[
        HostSurface::LinuxServerApi,
        HostSurface::DesktopEmbeddedTauri,
    ];

    pub fn default_for_platform(platform: &PlatformPaths) -> Self {
        match platform.kind {
            PlatformKind::Linux => Self::LinuxServerApi,
            PlatformKind::Macos | PlatformKind::Windows => Self::DesktopEmbeddedTauri,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LinuxServerApi => "linux_server_api",
            Self::DesktopEmbeddedTauri => "desktop_embedded_tauri",
        }
    }
}

impl fmt::Display for HostSurface {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for HostSurface {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().replace('-', "_").as_str() {
            "linux_server_api" => Ok(Self::LinuxServerApi),
            "desktop_embedded_tauri" => Ok(Self::DesktopEmbeddedTauri),
            other => Err(format!("unsupported host surface: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Threads,
    ThreadSubagents,
    CodexDesktop,
    Jobs,
    Probe,
    Settings,
    JobHistory,
    AppUpdater,
    ThreadCleanup,
    CodexDeleteTree,
    ThreadArchiveActions,
    Systemd,
    LinuxUpdateJob,
    PruneBackups,
}

impl Capability {
    pub const ALL: &'static [Capability] = &[
        Capability::Threads,
        Capability::ThreadSubagents,
        Capability::CodexDesktop,
        Capability::Jobs,
        Capability::Probe,
        Capability::Settings,
        Capability::JobHistory,
        Capability::AppUpdater,
        Capability::ThreadCleanup,
        Capability::CodexDeleteTree,
        Capability::ThreadArchiveActions,
        Capability::Systemd,
        Capability::LinuxUpdateJob,
        Capability::PruneBackups,
    ];

    pub fn all() -> &'static [Capability] {
        Self::ALL
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Threads => "threads",
            Self::ThreadSubagents => "thread_subagents",
            Self::CodexDesktop => "codex_desktop",
            Self::Jobs => "jobs",
            Self::Probe => "probe",
            Self::Settings => "settings",
            Self::JobHistory => "job_history",
            Self::AppUpdater => "app_updater",
            Self::ThreadCleanup => "thread_cleanup",
            Self::CodexDeleteTree => "codex_delete_tree",
            Self::ThreadArchiveActions => "thread_archive_actions",
            Self::Systemd => "systemd",
            Self::LinuxUpdateJob => "linux_update_job",
            Self::PruneBackups => "prune_backups",
        }
    }

    pub fn is_supported_on(self, platform: &PlatformPaths) -> bool {
        self.is_supported_on_surface(platform, HostSurface::default_for_platform(platform))
    }

    pub fn is_supported_on_surface(self, platform: &PlatformPaths, surface: HostSurface) -> bool {
        let shared_core = matches!(platform.kind, PlatformKind::Linux | PlatformKind::Macos);
        let linux_server_api =
            surface == HostSurface::LinuxServerApi && matches!(platform.kind, PlatformKind::Linux);
        let desktop_embedded = surface == HostSurface::DesktopEmbeddedTauri && shared_core;
        match self {
            Self::CodexDesktop => {
                surface == HostSurface::DesktopEmbeddedTauri && platform.kind == PlatformKind::Macos
            }
            Self::Threads
            | Self::ThreadSubagents
            | Self::Jobs
            | Self::Probe
            | Self::Settings
            | Self::JobHistory
            | Self::ThreadCleanup
            | Self::CodexDeleteTree
            | Self::ThreadArchiveActions => linux_server_api || desktop_embedded,
            Self::AppUpdater => linux_server_api || desktop_embedded,
            Self::Systemd | Self::LinuxUpdateJob | Self::PruneBackups => linux_server_api,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemCapabilities {
    pub threads: bool,
    #[serde(default)]
    pub thread_subagents: bool,
    #[serde(default)]
    pub codex_desktop: bool,
    pub jobs: bool,
    pub probe: bool,
    pub settings: bool,
    pub job_history: bool,
    pub app_updater: bool,
    pub thread_cleanup: bool,
    #[serde(default)]
    pub hidden_cleanup_scan: bool,
    #[serde(default)]
    pub dsh_archive_cleanup: bool,
    #[serde(default)]
    pub antigravity_notifications: bool,
    #[serde(default)]
    pub dsh_notifications: bool,
    #[serde(default)]
    pub antigravity_sessions: bool,
    #[serde(default)]
    pub grokbot_sessions: bool,
    #[serde(default)]
    pub github_ci: bool,
    #[serde(default)]
    pub grokbot_notifications: bool,
    #[serde(default)]
    pub dsh_sessions: bool,
    #[serde(default)]
    pub codex_delete_tree: bool,
    pub thread_archive_actions: bool,
    pub systemd: bool,
    pub linux_update_job: bool,
    pub prune_backups: bool,
}

pub fn require_capability(platform: &PlatformPaths, capability: Capability) -> Result<()> {
    require_capability_for_surface(
        platform,
        HostSurface::default_for_platform(platform),
        capability,
    )
}

pub fn require_capability_for_surface(
    platform: &PlatformPaths,
    host_surface: HostSurface,
    capability: Capability,
) -> Result<()> {
    if capability.is_supported_on_surface(platform, host_surface) {
        return Ok(());
    }
    bail!(
        "{} is unavailable on {} {}",
        capability.as_str(),
        platform_kind_label(platform.kind),
        host_surface.as_str()
    )
}

pub fn system_capabilities(config: &Config, platform: &PlatformPaths) -> SystemCapabilities {
    system_capabilities_for_surface(
        config,
        platform,
        HostSurface::default_for_platform(platform),
    )
}

pub fn system_capabilities_for_surface(
    _config: &Config,
    platform: &PlatformPaths,
    host_surface: HostSurface,
) -> SystemCapabilities {
    SystemCapabilities {
        threads: Capability::Threads.is_supported_on_surface(platform, host_surface),
        thread_subagents: Capability::ThreadSubagents
            .is_supported_on_surface(platform, host_surface),
        codex_desktop: Capability::CodexDesktop.is_supported_on_surface(platform, host_surface),
        jobs: Capability::Jobs.is_supported_on_surface(platform, host_surface),
        probe: Capability::Probe.is_supported_on_surface(platform, host_surface),
        settings: Capability::Settings.is_supported_on_surface(platform, host_surface),
        job_history: Capability::JobHistory.is_supported_on_surface(platform, host_surface),
        app_updater: Capability::AppUpdater.is_supported_on_surface(platform, host_surface),
        thread_cleanup: Capability::ThreadCleanup.is_supported_on_surface(platform, host_surface),
        hidden_cleanup_scan: Capability::ThreadCleanup
            .is_supported_on_surface(platform, host_surface),
        dsh_archive_cleanup: Capability::ThreadCleanup
            .is_supported_on_surface(platform, host_surface),
        antigravity_notifications: platform.kind == PlatformKind::Macos
            && host_surface == HostSurface::DesktopEmbeddedTauri,
        dsh_notifications: platform.kind == PlatformKind::Macos
            && host_surface == HostSurface::DesktopEmbeddedTauri,
        github_ci: Capability::Probe.is_supported_on_surface(platform, host_surface),
        grokbot_sessions: platform.kind == PlatformKind::Macos
            && host_surface == HostSurface::DesktopEmbeddedTauri,
        grokbot_notifications: platform.kind == PlatformKind::Macos
            && host_surface == HostSurface::DesktopEmbeddedTauri,
        antigravity_sessions: Capability::Threads.is_supported_on_surface(platform, host_surface),
        dsh_sessions: Capability::Threads.is_supported_on_surface(platform, host_surface),
        codex_delete_tree: Capability::CodexDeleteTree
            .is_supported_on_surface(platform, host_surface),
        thread_archive_actions: Capability::ThreadArchiveActions
            .is_supported_on_surface(platform, host_surface),
        systemd: Capability::Systemd.is_supported_on_surface(platform, host_surface),
        linux_update_job: Capability::LinuxUpdateJob
            .is_supported_on_surface(platform, host_surface),
        prune_backups: Capability::PruneBackups.is_supported_on_surface(platform, host_surface),
    }
}

fn platform_kind_label(kind: PlatformKind) -> &'static str {
    match kind {
        PlatformKind::Linux => "linux",
        PlatformKind::Macos => "macos",
        PlatformKind::Windows => "windows",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        require_capability, system_capabilities, system_capabilities_for_surface, Capability,
        HostSurface,
    };
    use crate::{
        config::Config,
        platform::{PlatformKind, PlatformPaths},
    };

    #[test]
    fn api_surface_has_no_web_authentication_capabilities() {
        let config = Config::for_platform_kind(PlatformKind::Linux);
        let platform = PlatformPaths::for_kind(PlatformKind::Linux);
        let capabilities =
            system_capabilities_for_surface(&config, &platform, HostSurface::LinuxServerApi);
        assert!(capabilities.threads && capabilities.linux_update_job);
        let json = serde_json::to_value(capabilities).unwrap();
        for key in [
            "web_auth",
            "csrf",
            "turnstile",
            "admin_password",
            "security_settings",
        ] {
            assert!(json.get(key).is_none());
        }
        assert!("linux_server_webui".parse::<HostSurface>().is_err());
    }

    #[test]
    fn local_maintenance_capabilities_are_shared_by_linux_and_macos_only() {
        let linux = PlatformPaths::for_kind(crate::platform::PlatformKind::Linux);
        let macos = PlatformPaths::for_kind(crate::platform::PlatformKind::Macos);
        let windows = PlatformPaths::for_kind(crate::platform::PlatformKind::Windows);

        for capability in [Capability::ThreadCleanup, Capability::ThreadArchiveActions] {
            assert!(require_capability(&linux, capability).is_ok());
            assert!(require_capability(&macos, capability).is_ok());
            assert!(require_capability(&windows, capability).is_err());
        }

        assert!(require_capability(&macos, Capability::LinuxUpdateJob).is_err());
        assert!(require_capability(&windows, Capability::ThreadCleanup).is_err());
        assert!(require_capability(&windows, Capability::ThreadArchiveActions).is_err());
    }

    #[test]
    fn capability_matrix_matches_neutral_capability_gate() {
        let config = Config::for_platform_kind(crate::platform::PlatformKind::Windows);
        let platform = PlatformPaths::for_kind(crate::platform::PlatformKind::Windows);
        let matrix = system_capabilities(&config, &platform);

        assert!(!matrix.settings);
        assert!(require_capability(&platform, Capability::Settings).is_err());
        assert!(!matrix.thread_cleanup);
        assert!(!matrix.thread_archive_actions);
    }
}
