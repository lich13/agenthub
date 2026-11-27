use agenthub_core::{
    codex::{resolve_codex_paths, CodexPaths},
    config::{Config, ConfigCache},
    db::PanelDb,
    jobs::JobRunner,
    platform::PlatformPaths,
};
use anyhow::Result;
use std::sync::{Arc, RwLock};

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
        let config = load_desktop_config(&platform)?;
        std::fs::create_dir_all(&config.paths.data_dir)?;
        std::fs::create_dir_all(&config.paths.log_dir)?;
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

fn load_desktop_config(platform: &PlatformPaths) -> Result<Config> {
    Config::load_runtime(&platform.config_file)
}

pub(crate) fn open_panel_db(config: &Config) -> Result<PanelDb> {
    PanelDb::open_with_secret_box(&config.paths.db_path, config.secret_box()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_probe_command_uses_platform_daemon_path() {
        let platform = PlatformPaths::macos("/Users/example");
        let plan = agenthub_core::services::probe::plan_probe_action(
            agenthub_core::services::probe::ProbeAction::BarkTest,
        )
        .unwrap();
        let process = agenthub_core::services::probe::probe_process_command(
            &platform,
            &platform.config_file,
            plan.action,
        );

        assert_eq!(
            process.program.to_str(),
            Some("/Users/example/Library/Application Support/AgentHub/bin/agenthub-webd")
        );
        assert_ne!(
            process.program.to_str(),
            Some("/opt/agenthub/bin/agenthub-webd")
        );
    }
}
