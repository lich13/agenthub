use agenthub_core::config::Config;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static SANDBOX_COUNTER: AtomicUsize = AtomicUsize::new(0);
pub(crate) static CONFIG_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) struct AsyncEnvGuard {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl AsyncEnvGuard {
    pub(crate) async fn set_many(vars: &[(&'static str, &Path)]) -> Self {
        let guard = CONFIG_ENV_LOCK.lock().await;
        let mut previous = Vec::with_capacity(vars.len());
        for &(key, value) in vars {
            previous.push((key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        Self {
            _guard: guard,
            previous,
        }
    }

    pub(crate) async fn set_config(path: &Path) -> Self {
        Self::set_many(&[("AGENTHUB_CONFIG", path)]).await
    }

    pub(crate) async fn lock_read_only() -> tokio::sync::MutexGuard<'static, ()> {
        CONFIG_ENV_LOCK.lock().await
    }
}

impl Drop for AsyncEnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn write_executable_fixture(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, script).expect("write executable fixture");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("set executable fixture permissions");
    for attempt in 0..10 {
        match std::process::Command::new(path)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 9 => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(_) => break,
            Err(err) => panic!("probe executable fixture {}: {err}", path.display()),
        }
    }
}

pub(crate) fn sandboxed_test_config() -> (Config, PathBuf) {
    let idx = SANDBOX_COUNTER.fetch_add(1, Ordering::Relaxed);
    let sandbox_root = std::env::temp_dir().join(format!(
        "agenthub-test-cfg-sandbox-{}-{idx}",
        std::process::id()
    ));
    let codex_home = sandbox_root.join(".codex");
    let _ = fs::create_dir_all(&codex_home);
    let mut config = Config::default();
    config.codex.home = codex_home;
    config.codex.workspace = sandbox_root.join("workspace");
    config.paths.data_dir = sandbox_root.join("data");
    config.paths.db_path = sandbox_root.join("data/panel.sqlite");
    config.paths.log_dir = sandbox_root.join("logs");
    config.codex.bridge_enabled = false;
    (config, sandbox_root)
}

pub(super) fn source_line_count(relative: &str) -> usize {
    fs::read_to_string(src_path(relative))
        .expect("source file should be readable")
        .lines()
        .count()
}

fn src_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative)
}
