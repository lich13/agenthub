use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use super::is_macos_network_volume_path;

#[derive(Debug, Clone)]
pub struct CodexPaths {
    pub home: PathBuf,
}

impl CodexPaths {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub(crate) fn contains_path(&self, path: &Path) -> bool {
        if is_macos_network_volume_path(&self.home) || is_macos_network_volume_path(path) {
            return false;
        }
        let Ok(home) = fs::canonicalize(&self.home) else {
            return false;
        };
        let Ok(candidate) = fs::canonicalize(path) else {
            return false;
        };
        candidate.starts_with(home)
    }

    pub fn state_db(&self) -> PathBuf {
        self.home.join("state_5.sqlite")
    }

    pub fn session_index(&self) -> PathBuf {
        self.home.join("session_index.jsonl")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.home.join("sessions")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedCodexPaths {
    pub configured_codex_home: Option<String>,
    pub home: PathBuf,
    pub logs_db: PathBuf,
    pub state_db: PathBuf,
    pub session_index: PathBuf,
    pub sessions_dir: PathBuf,
    pub configured_app_server_socket: Option<PathBuf>,
    pub app_server_socket: Option<PathBuf>,
    pub codex_home_source: String,
    pub logs_db_source: String,
    pub app_server_socket_source: Option<String>,
    pub discovery_warnings: Vec<String>,
}

impl ResolvedCodexPaths {
    pub fn codex_paths(&self) -> CodexPaths {
        CodexPaths::new(&self.home)
    }
}

#[derive(Debug, Clone)]
pub struct CodexPathDiscoveryOptions {
    pub env_codex_home: Option<PathBuf>,
    pub current_user_home: Option<PathBuf>,
    pub root_codex_home: PathBuf,
    pub ubuntu_codex_home: PathBuf,
    pub home_scan_root: PathBuf,
    pub fallback_codex_home: PathBuf,
    pub fallback_codex_home_source: &'static str,
}

impl Default for CodexPathDiscoveryOptions {
    fn default() -> Self {
        let current_user_home = dirs::home_dir();
        let (fallback_codex_home, fallback_codex_home_source) =
            default_fallback_codex_home(current_user_home.as_deref());
        let (root_codex_home, ubuntu_codex_home, home_scan_root) =
            default_linux_codex_discovery_paths(current_user_home.as_deref());
        Self {
            env_codex_home: env::var_os("CODEX_HOME").map(PathBuf::from),
            current_user_home,
            root_codex_home,
            ubuntu_codex_home,
            home_scan_root,
            fallback_codex_home,
            fallback_codex_home_source,
        }
    }
}

pub fn resolve_codex_paths(configured_home: &Path) -> ResolvedCodexPaths {
    resolve_codex_paths_with_options(configured_home, &CodexPathDiscoveryOptions::default())
}

pub fn resolve_codex_paths_with_options(
    configured_home: &Path,
    options: &CodexPathDiscoveryOptions,
) -> ResolvedCodexPaths {
    let configured_codex_home = configured_path_value(configured_home);
    let mut warnings = Vec::new();
    let configured_candidate = (!is_auto_path(configured_home)).then(|| {
        (
            configured_home.to_path_buf(),
            "configured",
            "configured Codex home is not valid",
        )
    });
    let mut candidates: Vec<(PathBuf, &'static str, &'static str)> = Vec::new();
    if let Some(candidate) = configured_candidate {
        candidates.push(candidate);
    }
    if let Some(path) = options
        .env_codex_home
        .as_deref()
        .filter(|path| !is_auto_path(path))
    {
        candidates.push((
            path.to_path_buf(),
            "env:CODEX_HOME",
            "CODEX_HOME is not a valid Codex home",
        ));
    }
    if let Some(path) = options.current_user_home.as_ref() {
        candidates.push((
            path.join(".codex"),
            "current_user",
            "current user ~/.codex is not a valid Codex home",
        ));
    }
    candidates.push((
        options.root_codex_home.clone(),
        "root",
        "/root/.codex is not a valid Codex home",
    ));
    candidates.push((
        options.ubuntu_codex_home.clone(),
        "home_ubuntu",
        "/home/ubuntu/.codex is not a valid Codex home",
    ));
    candidates.extend(
        scanned_home_codex_dirs(&options.home_scan_root)
            .into_iter()
            .map(|path| {
                (
                    path,
                    "home_scan",
                    "/home/*/.codex is not a valid Codex home",
                )
            }),
    );

    let mut selected: Option<(PathBuf, &'static str)> = None;
    for (path, source, invalid_message) in &candidates {
        if is_valid_codex_home(path) {
            selected = Some((path.clone(), *source));
            break;
        }
        if matches!(*source, "configured" | "env:CODEX_HOME" | "socket") {
            warnings.push(format!("{invalid_message}: {}", path.display()));
        }
    }

    let (home, codex_home_source) = selected.unwrap_or_else(|| {
        if !is_auto_path(configured_home) && !is_macos_network_volume_path(configured_home) {
            warnings.push(format!(
                "no valid Codex home discovered; using configured path {}",
                configured_home.display()
            ));
            (configured_home.to_path_buf(), "fallback_configured")
        } else {
            warnings.push(format!(
                "no valid Codex home discovered; using {}",
                options.fallback_codex_home.display()
            ));
            (
                options.fallback_codex_home.clone(),
                options.fallback_codex_home_source,
            )
        }
    });
    let codex_home_source = codex_home_source.to_string();
    ResolvedCodexPaths {
        configured_codex_home,
        logs_db: home.join("logs_2.sqlite"),
        state_db: home.join("state_5.sqlite"),
        session_index: home.join("session_index.jsonl"),
        sessions_dir: home.join("sessions"),
        configured_app_server_socket: None,
        app_server_socket: None,
        codex_home_source: codex_home_source.clone(),
        logs_db_source: codex_home_source,
        app_server_socket_source: None,
        discovery_warnings: warnings,
        home,
    }
}

fn default_fallback_codex_home(current_user_home: Option<&Path>) -> (PathBuf, &'static str) {
    #[cfg(target_os = "macos")]
    {
        let path = current_user_home
            .map(|home| home.join(".codex"))
            .unwrap_or_else(|| PathBuf::from(".codex"));
        (path, "fallback_current_user")
    }
    #[cfg(target_os = "windows")]
    {
        let path = current_user_home
            .map(|home| home.join(".codex"))
            .unwrap_or_else(|| PathBuf::from(".codex"));
        (path, "fallback_current_user")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = current_user_home;
        (PathBuf::from("/root/.codex"), "fallback_root")
    }
}

fn default_linux_codex_discovery_paths(
    current_user_home: Option<&Path>,
) -> (PathBuf, PathBuf, PathBuf) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let base = current_user_home
            .map(|home| home.join(".agenthub/non-linux-codex-discovery"))
            .unwrap_or_else(|| PathBuf::from(".agenthub/non-linux-codex-discovery"));
        (
            base.join("root/.codex"),
            base.join("home/ubuntu/.codex"),
            base.join("home"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = current_user_home;
        (
            PathBuf::from("/root/.codex"),
            PathBuf::from("/home/ubuntu/.codex"),
            PathBuf::from("/home"),
        )
    }
}

fn is_auto_path(path: &Path) -> bool {
    let value = path.to_string_lossy();
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto")
}

fn configured_path_value(path: &Path) -> Option<String> {
    let value = path.to_string_lossy();
    let trimmed = value.trim();
    (!trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto")).then(|| trimmed.to_string())
}

pub(crate) fn is_valid_codex_home(path: &Path) -> bool {
    if is_macos_network_volume_path(path) {
        return false;
    }
    path.is_dir()
        && [
            path.join("logs_2.sqlite"),
            path.join("state_5.sqlite"),
            path.join("session_index.jsonl"),
            path.join("sessions"),
            path.join("hooks.json"),
            path.join("app-server-control"),
        ]
        .iter()
        .any(|artifact| artifact.exists())
}

fn scanned_home_codex_dirs(home_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(home_root) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then(|| entry.path().join(".codex"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}
