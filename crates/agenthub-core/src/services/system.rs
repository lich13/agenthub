use crate::platform::{PlatformKind, PlatformPaths};
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
    // Protocol 2 wire name used by v2.1.28 and later.
    #[serde(rename = "desktop_embedded_tauri")]
    DesktopEmbedded,
}

impl HostSurface {
    pub const ALL: &'static [HostSurface] =
        &[HostSurface::LinuxServerApi, HostSurface::DesktopEmbedded];

    pub fn default_for_platform(platform: &PlatformPaths) -> Self {
        match platform.kind {
            PlatformKind::Linux => Self::LinuxServerApi,
            PlatformKind::Macos => Self::DesktopEmbedded,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LinuxServerApi => "linux_server_api",
            Self::DesktopEmbedded => "desktop_embedded_tauri",
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
            "desktop_embedded_tauri" => Ok(Self::DesktopEmbedded),
            other => Err(format!("unsupported host surface: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    AntigravityNotifications,
    DshNotifications,
    GrokbotSessions,
    GrokbotNotifications,
    Systemd,
    LinuxUpdateJob,
    PruneBackups,
}

impl Capability {
    pub fn all() -> &'static [Self] {
        &[
            Self::AntigravityNotifications,
            Self::DshNotifications,
            Self::GrokbotSessions,
            Self::GrokbotNotifications,
            Self::Systemd,
            Self::LinuxUpdateJob,
            Self::PruneBackups,
        ]
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AntigravityNotifications => "antigravity_notifications",
            Self::DshNotifications => "dsh_notifications",
            Self::GrokbotSessions => "grokbot_sessions",
            Self::GrokbotNotifications => "grokbot_notifications",
            Self::Systemd => "systemd",
            Self::LinuxUpdateJob => "linux_update_job",
            Self::PruneBackups => "prune_backups",
        }
    }
    pub fn is_supported_on_surface(self, surface: HostSurface) -> bool {
        match self {
            Self::AntigravityNotifications => true,
            Self::Systemd | Self::LinuxUpdateJob | Self::PruneBackups => {
                surface == HostSurface::LinuxServerApi
            }
            Self::DshNotifications | Self::GrokbotSessions | Self::GrokbotNotifications => {
                surface == HostSurface::DesktopEmbedded
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemCapabilities {
    #[serde(default)]
    pub claude_subagents: bool,
    #[serde(default)]
    pub antigravity_subagents: bool,
    pub threads: bool,
    #[serde(default)]
    pub thread_subagents: bool,
    pub jobs: bool,
    pub probe: bool,
    pub settings: bool,
    pub job_history: bool,
    pub app_updater: bool,
    pub thread_cleanup: bool,
    #[serde(default)]
    pub hidden_cleanup_scan: bool,
    #[serde(default)]
    pub codex_account_scope: bool,
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
    pub thread_archive_actions: bool,
    pub systemd: bool,
    pub linux_update_job: bool,
    pub prune_backups: bool,
}

pub fn require_capability(platform: &PlatformPaths, capability: Capability) -> Result<()> {
    let host_surface = HostSurface::default_for_platform(platform);
    if capability.is_supported_on_surface(host_surface) {
        return Ok(());
    }
    bail!(
        "{} is unavailable on {} {}",
        capability.as_str(),
        platform_kind_label(platform.kind),
        host_surface.as_str()
    )
}

pub fn system_capabilities_for_surface(host_surface: HostSurface) -> SystemCapabilities {
    // Protocol 2 keeps the common fields for installed clients.
    SystemCapabilities {
        claude_subagents: true,
        antigravity_subagents: true,
        threads: true,
        thread_subagents: true,
        jobs: true,
        probe: true,
        settings: true,
        job_history: true,
        app_updater: true,
        thread_cleanup: true,
        hidden_cleanup_scan: true,
        codex_account_scope: true,
        antigravity_notifications: Capability::AntigravityNotifications
            .is_supported_on_surface(host_surface),
        dsh_notifications: Capability::DshNotifications.is_supported_on_surface(host_surface),
        antigravity_sessions: true,
        grokbot_sessions: Capability::GrokbotSessions.is_supported_on_surface(host_surface),
        github_ci: true,
        grokbot_notifications: Capability::GrokbotNotifications
            .is_supported_on_surface(host_surface),
        dsh_sessions: true,
        thread_archive_actions: true,
        systemd: Capability::Systemd.is_supported_on_surface(host_surface),
        linux_update_job: Capability::LinuxUpdateJob.is_supported_on_surface(host_surface),
        prune_backups: Capability::PruneBackups.is_supported_on_surface(host_surface),
    }
}

fn platform_kind_label(kind: PlatformKind) -> &'static str {
    match kind {
        PlatformKind::Linux => "linux",
        PlatformKind::Macos => "macos",
    }
}

#[cfg(test)]
mod tests {
    use super::{system_capabilities_for_surface, HostSurface};

    #[test]
    fn api_surface_has_no_web_authentication_capabilities() {
        let capabilities = system_capabilities_for_surface(HostSurface::LinuxServerApi);
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
}
