use agenthub_core::{
    codex::{resolve_codex_paths, CodexPaths},
    config::Config,
    db::PanelDb,
    jobs::JobRunner,
    platform::{PlatformKind, PlatformPaths},
    services::system::HostSurface,
};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHubPaths {
    pub app_support_dir: PathBuf,
    pub config_file: PathBuf,
    pub database_file: PathBuf,
    pub log_dir: PathBuf,
    pub app_log_file: PathBuf,
}

#[derive(Clone)]
pub struct DesktopState {
    config: Arc<RwLock<Config>>,
    pub db: PanelDb,
    pub jobs: JobRunner,
    platform: PlatformPaths,
    host_surface: HostSurface,
}

impl DesktopState {
    pub fn current() -> Result<Self> {
        let platform = PlatformPaths::desktop_current();
        let mut config = load_desktop_config(&platform)?;
        std::fs::create_dir_all(&config.paths.data_dir)?;
        std::fs::create_dir_all(&config.paths.log_dir)?;
        if agenthub_core::codex::is_macos_network_volume_path(&config.codex.workspace) {
            config.codex.workspace = default_local_desktop_workspace()
                .ok_or_else(|| anyhow!("cannot resolve local desktop workspace"))?;
        }
        std::fs::create_dir_all(&config.codex.workspace)?;
        let db = open_panel_db(&config)?;
        Ok(Self::new(config, db, platform))
    }

    pub fn new(config: Config, db: PanelDb, platform: PlatformPaths) -> Self {
        let jobs = JobRunner::new(db.clone());
        Self {
            config: Arc::new(RwLock::new(config)),
            db,
            jobs,
            platform,
            host_surface: HostSurface::DesktopEmbeddedTauri,
        }
    }

    pub fn config(&self) -> Config {
        self.config.read().expect("desktop config rwlock").clone()
    }

    pub fn replace_config(&self, config: Config) {
        *self.config.write().expect("desktop config rwlock") = config;
    }

    pub fn resolved_codex_paths(&self) -> agenthub_core::codex::ResolvedCodexPaths {
        let config = self.config();
        resolve_codex_paths(&config.codex.home)
    }

    pub fn codex_paths(&self) -> CodexPaths {
        self.resolved_codex_paths().codex_paths()
    }

    pub fn platform(&self) -> &PlatformPaths {
        &self.platform
    }

    pub fn host_surface(&self) -> HostSurface {
        self.host_surface
    }
}

fn default_local_desktop_workspace() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("agenthub-workspace"))
}

pub fn agenthub_paths_for_home(home: impl Into<PathBuf>) -> AgentHubPaths {
    agenthub_paths_for_platform_home(PlatformKind::Macos, home)
}

pub fn agenthub_paths_for_platform_home(
    kind: PlatformKind,
    home: impl Into<PathBuf>,
) -> AgentHubPaths {
    let platform = PlatformPaths::for_desktop_kind_with_home(kind, home);
    agenthub_paths_for_platform(&platform)
}

fn agenthub_paths_for_platform(platform: &PlatformPaths) -> AgentHubPaths {
    AgentHubPaths {
        config_file: platform.config_file.clone(),
        database_file: platform.data_dir.join("agenthub.sqlite"),
        app_log_file: platform.log_dir.join("agenthub.log"),
        app_support_dir: platform.data_dir.clone(),
        log_dir: platform.log_dir.clone(),
    }
}

fn load_desktop_config(platform: &PlatformPaths) -> Result<Config> {
    Config::load_runtime(&platform.config_file)
}

pub(crate) fn open_panel_db(config: &Config) -> Result<PanelDb> {
    PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenthub_core::services::jobs as job_service;

    #[test]
    fn mac_style_paths_use_agenthub_application_support_and_logs() {
        let paths = agenthub_paths_for_home("/Users/example");

        assert_eq!(
            paths.app_support_dir,
            PathBuf::from("/Users/example/Library/Application Support/AgentHub")
        );
        assert_eq!(
            paths.log_dir,
            PathBuf::from("/Users/example/Library/Logs/AgentHub")
        );
        assert_eq!(
            paths.config_file,
            PathBuf::from("/Users/example/Library/Application Support/AgentHub/config.toml")
        );
        assert_eq!(
            paths.app_log_file,
            PathBuf::from("/Users/example/Library/Logs/AgentHub/agenthub.log")
        );
    }

    #[test]
    fn desktop_unsupported_action_is_explicitly_unavailable() {
        let response =
            job_service::action_unavailable("forkThread", "fork uses Codex app-server state");

        assert!(!response.available);
        assert!(!response.ok);
        assert_eq!(response.command, "forkThread");
        assert!(response
            .message
            .contains("fork uses Codex app-server state"));
    }

    #[test]
    fn fixed_probe_command_uses_platform_daemon_path() {
        let platform = PlatformPaths::for_kind_with_home(PlatformKind::Macos, "/Users/example");
        let plan = agenthub_core::services::probe::plan_probe_action(
            &platform,
            agenthub_core::services::probe::ProbeAction::BarkTest,
            &platform.config_file,
        )
        .unwrap();
        let command = plan.job.command;

        assert!(
            command.contains(
                "'/Users/example/Library/Application Support/AgentHub/bin/agenthub-webd'"
            ),
            "{command}"
        );
        assert!(!command.contains("/opt/agenthub/bin/agenthub-webd"));
    }
}
