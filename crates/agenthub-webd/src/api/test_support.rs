use agenthub_core::config::Config;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static SANDBOX_COUNTER: AtomicUsize = AtomicUsize::new(0);

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
