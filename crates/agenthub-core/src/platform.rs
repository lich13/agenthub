use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformPaths {
    pub kind: PlatformKind,
    pub data_dir: PathBuf,
    pub config_file: PathBuf,

    pub log_dir: PathBuf,
    pub service_name: String,
    pub service_kind: String,
    pub service_file: Option<PathBuf>,
}

impl PlatformPaths {
    pub fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::for_kind(PlatformKind::Macos)
        }
        #[cfg(target_os = "windows")]
        {
            Self::for_kind(PlatformKind::Windows)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self::for_kind(PlatformKind::Linux)
        }
    }

    pub fn desktop_current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::for_desktop_kind(PlatformKind::Macos)
        }
        #[cfg(target_os = "windows")]
        {
            Self::for_desktop_kind(PlatformKind::Windows)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self::for_desktop_kind(PlatformKind::Linux)
        }
    }

    pub fn for_kind(kind: PlatformKind) -> Self {
        Self::for_kind_with_home(kind, dirs::home_dir().unwrap_or_else(|| PathBuf::from("~")))
    }

    pub fn for_desktop_kind(kind: PlatformKind) -> Self {
        Self::for_desktop_kind_with_home(
            kind,
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("~")),
        )
    }

    pub fn for_kind_with_home(kind: PlatformKind, home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        match kind {
            PlatformKind::Linux => Self {
                kind,
                data_dir: PathBuf::from("/var/lib/agenthub-webd"),
                config_file: PathBuf::from("/etc/agenthub-webd/config.toml"),

                log_dir: PathBuf::from("/var/log/agenthub-webd"),
                service_name: "agenthub-webd".to_string(),
                service_kind: "systemd".to_string(),
                service_file: Some(PathBuf::from("/etc/systemd/system/agenthub-webd.service")),
            },
            PlatformKind::Macos => Self {
                kind,
                data_dir: home.join("Library/Application Support/AgentHub"),
                config_file: home.join("Library/Application Support/AgentHub/config.toml"),

                log_dir: home.join("Library/Logs/AgentHub"),
                service_name: "AgentHub.app".to_string(),
                service_kind: "tauri".to_string(),
                service_file: None,
            },
            PlatformKind::Windows => Self {
                kind,
                data_dir: PathBuf::from(r"%ProgramData%\AgentHub"),
                config_file: PathBuf::from(r"%ProgramData%\AgentHub\config.toml"),

                log_dir: PathBuf::from(r"%ProgramData%\AgentHub\logs"),
                service_name: "AgentHub".to_string(),
                service_kind: "windows_service".to_string(),
                service_file: None,
            },
        }
    }

    pub fn for_desktop_kind_with_home(kind: PlatformKind, home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        match kind {
            PlatformKind::Linux => {
                let data_dir = home.join(".local/share/AgentHub");
                Self {
                    kind,
                    config_file: home.join(".config/AgentHub/config.toml"),

                    log_dir: home.join(".local/state/AgentHub/logs"),
                    data_dir,
                    service_name: "AgentHub".to_string(),
                    service_kind: "tauri".to_string(),
                    service_file: None,
                }
            }
            PlatformKind::Macos => Self::for_kind_with_home(kind, home),
            PlatformKind::Windows => Self {
                kind,
                data_dir: home.join("AppData/Roaming/AgentHub"),
                config_file: home.join("AppData/Roaming/AgentHub/config.toml"),

                log_dir: home.join("AppData/Local/AgentHub/Logs"),
                service_name: "AgentHub".to_string(),
                service_kind: "tauri".to_string(),
                service_file: None,
            },
        }
    }

    pub fn daemon_binary(&self) -> PathBuf {
        let binary = match self.kind {
            PlatformKind::Windows => "agenthub-webd.exe",
            PlatformKind::Linux | PlatformKind::Macos => "agenthub-webd",
        };
        if self.kind == PlatformKind::Linux && self.service_kind == "systemd" {
            return PathBuf::from("/usr/local/bin").join(binary);
        }
        self.data_dir.join("bin").join(binary)
    }
}
