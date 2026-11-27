use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Linux,
    Macos,
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

impl PlatformKind {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

pub(crate) fn current_home() -> anyhow::Result<PathBuf> {
    dirs::home_dir()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| anyhow::anyhow!("cannot resolve user home directory"))
}

pub(crate) fn provider_root(variable: &str, relative: &str) -> anyhow::Result<PathBuf> {
    match std::env::var_os(variable).filter(|value| !value.is_empty()) {
        Some(root) => Ok(PathBuf::from(root)),
        None => Ok(current_home()?.join(relative)),
    }
}

impl PlatformPaths {
    pub fn current() -> anyhow::Result<Self> {
        match PlatformKind::current() {
            PlatformKind::Linux => Ok(Self::linux_server()),
            PlatformKind::Macos => Self::desktop_current(),
        }
    }

    pub fn desktop_current() -> anyhow::Result<Self> {
        Ok(Self::macos(current_home()?))
    }

    pub fn linux_server() -> Self {
        Self {
            kind: PlatformKind::Linux,
            data_dir: PathBuf::from("/var/lib/agenthub-webd"),
            config_file: PathBuf::from("/etc/agenthub-webd/config.toml"),

            log_dir: PathBuf::from("/var/log/agenthub-webd"),
            service_name: "agenthub-webd".to_string(),
            service_kind: "systemd".to_string(),
            service_file: Some(PathBuf::from("/etc/systemd/system/agenthub-webd.service")),
        }
    }

    pub fn macos(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            kind: PlatformKind::Macos,
            data_dir: home.join("Library/Application Support/AgentHub"),
            config_file: home.join("Library/Application Support/AgentHub/config.toml"),

            log_dir: home.join("Library/Logs/AgentHub"),
            service_name: "AgentHub.app".to_string(),
            // Protocol 2 service kind consumed by older Apps.
            service_kind: "tauri".to_string(),
            service_file: None,
        }
    }

    pub fn daemon_binary(&self) -> PathBuf {
        let binary = "agenthub-webd";
        if self.kind == PlatformKind::Linux && self.service_kind == "systemd" {
            return PathBuf::from("/usr/local/bin").join(binary);
        }
        self.data_dir.join("bin").join(binary)
    }
}

pub(crate) fn same_canonical_dir(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || matches!(
            (std::fs::canonicalize(left), std::fs::canonicalize(right)),
            (Ok(left), Ok(right)) if left == right
        )
}

pub(crate) fn resolve_kernel_exe(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        let mut buf = [0_u8; 4096];
        // SAFETY: `buf` is writable for `buf.len()` bytes and `pid` is validated to fit `i32`.
        let count = unsafe {
            libc::proc_pidpath(
                pid.try_into().ok()?,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
            )
        };
        if count <= 0 {
            return None;
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(count as usize);
        std::str::from_utf8(&buf[..end]).ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}
