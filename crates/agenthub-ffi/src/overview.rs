use agenthub_core::{
    codex::{resolve_codex_paths, CodexPaths},
    config::{Config, ConfigCache},
    db::PanelDb,
    jobs::JobRunner,
    platform::PlatformPaths,
};
use anyhow::{anyhow, Result};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
pub struct DesktopState {
    config: Arc<RwLock<Config>>,
    config_cache: Arc<ConfigCache>,
    pub db: PanelDb,
    pub jobs: JobRunner,
    platform: PlatformPaths,
    pub(crate) isolated: bool,
}

impl DesktopState {
    pub fn current() -> Result<Self> {
        let platform = PlatformPaths::desktop_current()?;
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
            config_cache: Arc::new(ConfigCache::default()),
            db,
            jobs,
            platform,
            isolated: false,
        }
    }

    pub fn config(&self) -> Config {
        self.config.read().expect("desktop config rwlock").clone()
    }

    pub fn refresh_config(&self) -> Result<Config> {
        let mut current = self.config.write().expect("desktop config rwlock");
        let loaded = self.config_cache.load(&self.platform.config_file)?;
        *current = loaded.clone();
        Ok(loaded)
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
}

fn default_local_desktop_workspace() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("agenthub-workspace"))
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
    use agenthub_core::platform::PlatformKind;

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
