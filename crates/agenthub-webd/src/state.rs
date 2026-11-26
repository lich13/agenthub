use agenthub_core::{
    codex::{resolve_codex_paths, CodexAppServerClient, CodexPaths, ResolvedCodexPaths},
    config::{Config, ConfigCache},
    db::PanelDb,
    jobs::JobRunner,
    platform::PlatformPaths,
};
use serde_json::Value;
use std::sync::{Arc, Mutex, RwLock};

#[cfg(test)]
pub(crate) struct TestSandboxDir(pub(crate) std::path::PathBuf);

#[cfg(test)]
impl Drop for TestSandboxDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
pub(crate) struct AppState {
    config: Arc<RwLock<Config>>,
    config_cache: Arc<ConfigCache>,
    platform: PlatformPaths,
    pub(crate) db: PanelDb,
    pub(crate) jobs: JobRunner,
    pub(crate) app_server_client: CodexAppServerClient,
    pub(crate) probe_status_cache: Arc<Mutex<ProbeStatusCache>>,
    #[cfg(test)]
    test_sandbox: Option<Arc<TestSandboxDir>>,
}

impl AppState {
    pub(crate) fn new(config: Config, db: PanelDb) -> Self {
        let jobs = JobRunner::new(db.clone());
        Self {
            config: Arc::new(RwLock::new(config)),
            config_cache: Arc::new(ConfigCache::default()),
            platform: PlatformPaths::linux_server(),
            db,
            jobs,
            app_server_client: CodexAppServerClient::new(),
            probe_status_cache: Arc::new(Mutex::new(ProbeStatusCache::default())),
            #[cfg(test)]
            test_sandbox: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_sandbox(mut self, root: std::path::PathBuf) -> Self {
        self.test_sandbox = Some(Arc::new(TestSandboxDir(root)));
        self
    }

    pub(crate) fn is_test_sandbox(&self) -> bool {
        #[cfg(test)]
        {
            self.test_sandbox.is_some()
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    pub(crate) fn platform(&self) -> &PlatformPaths {
        &self.platform
    }

    pub(crate) fn config(&self) -> Config {
        self.config.read().expect("config rwlock").clone()
    }

    pub(crate) fn with_config_path(mut self, path: std::path::PathBuf) -> Self {
        self.platform.config_file = path;
        self
    }

    pub(crate) fn config_path(&self) -> &std::path::Path {
        &self.platform.config_file
    }

    pub(crate) fn refresh_config(&self) -> anyhow::Result<Config> {
        let mut current = self.config.write().expect("config rwlock");
        let loaded = self.config_cache.load(self.config_path())?;
        *current = loaded.clone();
        Ok(loaded)
    }

    pub(crate) fn replace_config(&self, config: Config) {
        *self.config.write().expect("config rwlock") = config;
    }

    pub(crate) fn resolved_codex_paths(&self) -> ResolvedCodexPaths {
        let config = self.config();
        resolve_codex_paths(&config.codex.home)
    }

    pub(crate) fn codex_paths(&self) -> CodexPaths {
        self.resolved_codex_paths().codex_paths()
    }

    pub(crate) fn unified_cleanup_paths(
        &self,
    ) -> anyhow::Result<agenthub_core::services::cleanup::UnifiedCleanupPaths> {
        let codex = self.codex_paths();
        #[cfg(test)]
        if let Some(sandbox) = &self.test_sandbox {
            let claude_cfg = sandbox.0.join(".claude");
            return Ok(agenthub_core::services::cleanup::UnifiedCleanupPaths {
                codex,
                antigravity:
                    agenthub_core::antigravity::sessions::AntigravityPaths::offline_user_home(
                        &sandbox.0,
                    ),
                dsh: agenthub_core::dsh::DshPaths {
                    home: sandbox.0.join(".dsh"),
                },
                claude: agenthub_core::claude::ClaudePaths {
                    projects: claude_cfg.join("projects"),
                    config: claude_cfg,
                },
                grok: agenthub_core::grok::GrokPaths {
                    home: sandbox.0.join(".grok"),
                },
            });
        }
        agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(codex)
    }
}

#[derive(Debug, Default)]
pub(crate) struct ProbeStatusCache {
    pub(crate) snapshot: Option<CachedProbeStatus>,
    pub(crate) refreshing: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedProbeStatus {
    pub(crate) value: Value,
    pub(crate) refreshed_at_unix: i64,
}
