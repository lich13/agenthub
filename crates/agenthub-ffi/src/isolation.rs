use crate::overview::DesktopState;
use agenthub_core::{
    config::Config,
    db::PanelDb,
    platform::PlatformPaths,
    probe_error_monitor::{
        ProbeErrorMonitorLaunchAgentStatus, PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL,
    },
};
use anyhow::{anyhow, Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const ISOLATED_DATA_DIR_ENV: &str = "AGENTHUB_ISOLATED_DATA_DIR";
pub const KEY_SERVICE_PRODUCTION: &str = "com.lich13.agenthub.codex-desktop";
pub const KEY_SERVICE_PREVIEW: &str = "com.lich13.agenthub.codex-desktop.preview";

static ISOLATED_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn is_isolated_mode() -> bool {
    ISOLATED_ACTIVE.load(Ordering::Acquire)
}

pub fn set_isolated_mode(isolated: bool) {
    ISOLATED_ACTIVE.store(isolated, Ordering::Release);
}

pub fn resolve_isolated_root(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit.filter(|p| !p.as_os_str().is_empty()) {
        return Some(path.to_path_buf());
    }
    std::env::var_os(ISOLATED_DATA_DIR_ENV)
        .filter(|val| !val.is_empty())
        .map(PathBuf::from)
}

pub fn init_desktop_state(isolated_root: Option<&Path>) -> Result<(DesktopState, bool)> {
    let Some(root) = resolve_isolated_root(isolated_root) else {
        set_isolated_mode(false);
        return Ok((DesktopState::current()?, false));
    };
    set_isolated_mode(true);
    let state = build_isolated_desktop_state(&root)?;
    Ok((state, true))
}

pub fn build_isolated_desktop_state(isolated_root: &Path) -> Result<DesktopState> {
    create_dir_0700(isolated_root)?;
    let log_dir = isolated_root.join("logs");
    let workspace_dir = isolated_root.join("workspace");
    create_dir_0700(&log_dir)?;
    create_dir_0700(&workspace_dir)?;

    let config_file = isolated_root.join("config.toml");
    let production_platform = PlatformPaths::desktop_current();

    if !config_file.exists() {
        let mut seed = if production_platform.config_file.is_file() {
            Config::load(&production_platform.config_file).unwrap_or_default()
        } else {
            Config::default()
        };
        seed.security.secret_key.clear();
        seed.paths.data_dir = isolated_root.to_path_buf();
        seed.paths.db_path = isolated_root.join("agenthub.sqlite");
        seed.paths.log_dir = log_dir.clone();
        seed.codex.workspace = workspace_dir.clone();
        let serialized = toml::to_string(&seed).context("serialize isolated config.toml")?;
        write_atomic_0600(&config_file, serialized.as_bytes())?;
    }

    // Seed remote-connection.json read-only once if the user already has saved SSH hosts.
    let isolated_remote = isolated_root.join("remote-connection.json");
    let production_remote = production_platform.data_dir.join("remote-connection.json");
    if !isolated_remote.exists() && production_remote.is_file() {
        if let Ok(bytes) = fs::read(&production_remote) {
            let _ = write_atomic_0600(&isolated_remote, &bytes);
        }
    }

    let mut platform = production_platform;
    platform.data_dir = isolated_root.to_path_buf();
    platform.config_file = config_file;
    platform.log_dir = log_dir;

    let mut config = Config::load_runtime(&platform.config_file)?;
    config.paths.data_dir = isolated_root.to_path_buf();
    config.paths.db_path = isolated_root.join("agenthub.sqlite");
    config.paths.log_dir = platform.log_dir.clone();
    if agenthub_core::codex::is_macos_network_volume_path(&config.codex.workspace) {
        config.codex.workspace = workspace_dir;
    }
    fs::create_dir_all(&config.codex.workspace)?;
    let db = PanelDb::open_with_secret_box(
        &config.paths.db_path,
        config
            .secret_box()
            .map_err(|err| anyhow!("isolated secret box failed: {err}"))?,
    )?;
    Ok(DesktopState::new(config, db, platform))
}

fn create_dir_0700(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_atomic_0600(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("target path missing parent directory"))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".isolated-cfg-")
        .tempfile_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(target)
        .map_err(|err| anyhow!("persist isolated file failed: {}", err.error))?;
    Ok(())
}

pub fn skipped_launch_agent_status(
    config: &Config,
    platform: &PlatformPaths,
) -> ProbeErrorMonitorLaunchAgentStatus {
    ProbeErrorMonitorLaunchAgentStatus {
        supported: false,
        enabled: config.probe.monitor_enabled(),
        loaded: false,
        changed: false,
        label: PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL.to_string(),
        plist_path: None,
        helper_path: platform.daemon_binary(),
        config_path: platform.config_file.clone(),
    }
}
