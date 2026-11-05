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
    use agenthub_core::crypto::SecretBox;
    use agenthub_core::services::{jobs as job_service, settings as settings_service};

    fn test_desktop_state() -> (tempfile::TempDir, DesktopState) {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::for_platform_kind_with_home(PlatformKind::Macos, temp.path());
        config.paths.data_dir = temp.path().join("data");
        config.paths.db_path = temp.path().join("panel.sqlite");
        config.paths.log_dir = temp.path().join("logs");
        config.codex.home = temp.path().join("codex-home");
        config.codex.workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&config.paths.data_dir).unwrap();
        std::fs::create_dir_all(&config.paths.log_dir).unwrap();
        std::fs::create_dir_all(&config.codex.home).unwrap();
        std::fs::create_dir_all(config.codex.home.join("sessions")).unwrap();
        std::fs::create_dir_all(&config.codex.workspace).unwrap();
        let db =
            PanelDb::open_with_secret_box(&config.paths.db_path, SecretBox::deterministic_dev())
                .unwrap();
        let state = DesktopState::new(
            config,
            db,
            PlatformPaths::for_kind_with_home(PlatformKind::Macos, temp.path()),
        );
        (temp, state)
    }

    #[test]
    fn desktop_thread_service_passes_archive_filter_to_codex_reader() {
        let source = include_str!("services/threads.rs");
        assert!(source.contains(
            "thread_service::load_thread_read_model_inputs(&paths, &plan, running_jobs)"
        ));
    }

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
    fn desktop_probe_save_settings_uses_shared_bark_device_key_constant() {
        let (_temp, state) = test_desktop_state();
        std::fs::create_dir_all(state.platform().config_file.parent().unwrap()).unwrap();
        let config = state.config();
        std::fs::write(
            &state.platform().config_file,
            toml::to_string(&config).unwrap(),
        )
        .unwrap();

        crate::services::settings::test_probe_save_settings_with_state(
            &state,
            settings_service::ProbeSettingsSaveRequest {
                notifications: Some(settings_service::ProbeNotificationsSavePatch {
                    device_key: Some("  shared-device-key  ".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            state
                .db
                .get_secret_setting_bytes(settings_service::PROBE_BARK_DEVICE_KEY_SETTING)
                .unwrap()
                .as_deref(),
            Some(b"shared-device-key".as_slice())
        );
        let hardcoded_setter = concat!("set_secret_setting_bytes(\"", "probe_bark_device_key\"");
        assert!(!include_str!("commands/settings.rs").contains(hardcoded_setter));
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
        let config = Config::for_platform_kind_with_home(PlatformKind::Macos, "/Users/example");
        let plan = agenthub_core::services::probe::plan_probe_action(
            &config,
            &platform,
            agenthub_core::services::probe::ProbeAction::BarkTest,
        )
        .unwrap();
        let command = plan.job.unwrap().command;

        assert!(
            command.contains(
                "'/Users/example/Library/Application Support/AgentHub/bin/agenthub-webd'"
            ),
            "{command}"
        );
        assert!(!command.contains("/opt/agenthub/bin/agenthub-webd"));
    }

    #[test]
    fn macos_default_config_does_not_use_linux_panel_update_precheck() {
        let config = Config::for_platform_kind_with_home(PlatformKind::Macos, "/Users/example");

        assert!(config
            .update
            .panel_update_command
            .contains("agenthub-webd-update"));
        assert!(!config.update.panel_precheck_command.contains("systemctl"));
        assert!(!config
            .update
            .panel_precheck_command
            .contains("127.0.0.1:15742/healthz"));
        assert!(!config
            .update
            .panel_precheck_command
            .contains("/opt/agenthub"));
        assert!(config
            .update
            .panel_precheck_command
            .contains("Library/Application Support/AgentHub"));
    }
}
