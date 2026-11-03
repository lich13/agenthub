use agenthub_core::{
    codex::{resolve_codex_paths, CodexAppServerClient, CodexPaths, ResolvedCodexPaths},
    config::Config,
    db::PanelDb,
    jobs::JobRunner,
    platform::PlatformPaths,
    services::system::HostSurface,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

#[cfg(test)]
pub(crate) struct TestSandboxDir(pub(crate) std::path::PathBuf);

#[cfg(test)]
impl Drop for TestSandboxDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
pub struct AppState {
    config: Arc<RwLock<Config>>,
    host_surface: HostSurface,
    platform: PlatformPaths,
    pub db: PanelDb,
    pub jobs: JobRunner,
    pub app_server_client: CodexAppServerClient,
    pub auth_limiter: Arc<Mutex<AuthLimiter>>,
    pub probe_status_cache: Arc<Mutex<ProbeStatusCache>>,
    #[cfg(test)]
    test_sandbox: Option<Arc<TestSandboxDir>>,
}

impl AppState {
    pub fn new(config: Config, db: PanelDb) -> Self {
        Self::new_for_surface(config, db, HostSurface::LinuxServerApi)
    }

    pub fn new_for_surface(config: Config, db: PanelDb, host_surface: HostSurface) -> Self {
        Self::new_for_surface_with_app_server_client(
            config,
            db,
            host_surface,
            CodexAppServerClient::new(),
        )
    }

    fn new_for_surface_with_app_server_client(
        config: Config,
        db: PanelDb,
        host_surface: HostSurface,
        app_server_client: CodexAppServerClient,
    ) -> Self {
        let jobs = JobRunner::new(db.clone());
        let auth_rate_limit = config.security.auth_rate_limit_per_minute;
        let platform = match host_surface {
            HostSurface::LinuxServerApi => {
                PlatformPaths::for_kind(agenthub_core::platform::PlatformKind::Linux)
            }
            HostSurface::DesktopEmbeddedTauri => PlatformPaths::current(),
        };
        Self {
            config: Arc::new(RwLock::new(config)),
            host_surface,
            platform,
            db,
            jobs,
            app_server_client,
            auth_limiter: Arc::new(Mutex::new(AuthLimiter::new(auth_rate_limit))),
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

    pub fn host_surface(&self) -> HostSurface {
        self.host_surface
    }

    pub fn platform(&self) -> &PlatformPaths {
        &self.platform
    }

    pub fn config(&self) -> Config {
        self.config.read().expect("config rwlock").clone()
    }

    pub fn replace_config(&self, config: Config) {
        *self.config.write().expect("config rwlock") = config;
    }

    pub fn resolved_codex_paths(&self) -> ResolvedCodexPaths {
        let config = self.config();
        resolve_codex_paths(&config.codex.home)
    }

    pub fn codex_paths(&self) -> CodexPaths {
        self.resolved_codex_paths().codex_paths()
    }

    pub fn dsh_paths(&self) -> agenthub_core::dsh::DshPaths {
        #[cfg(test)]
        if let Some(sandbox) = &self.test_sandbox {
            return agenthub_core::dsh::DshPaths {
                home: sandbox.0.join(".dsh"),
            };
        }
        agenthub_core::dsh::DshPaths::default_for_user()
    }

    pub fn unified_cleanup_paths(&self) -> agenthub_core::services::cleanup::UnifiedCleanupPaths {
        let codex = self.codex_paths();
        #[cfg(test)]
        if let Some(sandbox) = &self.test_sandbox {
            let claude_cfg = sandbox.0.join(".claude");
            return agenthub_core::services::cleanup::UnifiedCleanupPaths {
                codex,
                antigravity: agenthub_core::antigravity::sessions::AntigravityPaths::offline(
                    sandbox.0.join(".gemini/antigravity"),
                ),
                dsh: self.dsh_paths(),
                claude: agenthub_core::claude::ClaudePaths {
                    projects: claude_cfg.join("projects"),
                    config: claude_cfg,
                },
                grok: agenthub_core::grok::GrokPaths {
                    home: sandbox.0.join(".grok"),
                },
            };
        }
        agenthub_core::services::cleanup::UnifiedCleanupPaths::default_for_codex(codex)
    }
}

#[derive(Debug, Default)]
pub struct ProbeStatusCache {
    pub snapshot: Option<CachedProbeStatus>,
    pub refreshing: bool,
}

#[derive(Debug, Clone)]
pub struct CachedProbeStatus {
    pub value: Value,
    pub refreshed_at_unix: i64,
}

pub struct AuthLimiter {
    max_per_minute: u32,
    attempts: HashMap<String, Vec<Instant>>,
}

impl AuthLimiter {
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            max_per_minute,
            attempts: HashMap::new(),
        }
    }

    pub fn check(&mut self, key: &str) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(60);
        self.attempts.retain(|_, times| {
            times.retain(|instant| now.duration_since(*instant) < window);
            !times.is_empty()
        });
        if self.attempts.len() >= 4096 && !self.attempts.contains_key(key) {
            return false;
        }
        let attempts = self.attempts.entry(key.to_string()).or_default();
        attempts.retain(|instant| now.duration_since(*instant) < window);
        if attempts.len() >= self.max_per_minute as usize {
            return false;
        }
        attempts.push(now);
        true
    }
}
