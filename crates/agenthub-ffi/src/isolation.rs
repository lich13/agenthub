use crate::overview::DesktopState;
use agenthub_core::{config::Config, db::PanelDb, platform::PlatformPaths};
use anyhow::{anyhow, Context, Result};
use std::{fs, io::Write, path::Path};
use zeroize::{Zeroize, Zeroizing};

pub fn init_desktop_state(isolated_root: Option<&Path>) -> Result<DesktopState> {
    match isolated_root {
        Some(root) => build_isolated_desktop_state(root),
        None => DesktopState::current(),
    }
}

pub fn build_isolated_desktop_state(isolated_root: &Path) -> Result<DesktopState> {
    create_dir_0700(isolated_root)?;
    let log_dir = isolated_root.join("logs");
    create_dir_0700(&log_dir)?;

    let config_file = isolated_root.join("config.toml");
    let production_platform = PlatformPaths::desktop_current()?;

    if !config_file.exists() {
        let mut seed = Config::load(&production_platform.config_file)?;
        seed.security.secret_key.zeroize();
        seed.paths.data_dir = isolated_root.to_path_buf();
        seed.paths.db_path = isolated_root.join("agenthub.sqlite");
        seed.paths.log_dir = log_dir.clone();
        seed.codex.workspace = isolated_root.join("workspace");
        let serialized = seed
            .to_toml_string()
            .context("serialize isolated config.toml")?;
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

    let env_file = isolated_root.join("env");
    if !env_file.exists() {
        let key = Zeroizing::new(agenthub_core::crypto::SecretBox::random_key_material());
        let contents = Zeroizing::new(format!("AGENTHUB_SECRET_KEY={}\n", key.as_str()));
        write_atomic_0600(&env_file, contents.as_bytes())?;
    }

    let mut config = Config::load_runtime(&platform.config_file)?;
    config.paths.data_dir = isolated_root.to_path_buf();
    config.paths.db_path = isolated_root.join("agenthub.sqlite");
    config.paths.log_dir = platform.log_dir.clone();
    let db = PanelDb::open_with_secret_box(
        &config.paths.db_path,
        config
            .secret_box()
            .map_err(|err| anyhow!("isolated secret box failed: {err}"))?,
    )?;
    let mut state = DesktopState::new(config, db, platform);
    state.isolated = true;
    Ok(state)
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
