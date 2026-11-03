pub mod antigravity;
pub mod archive;
pub mod claude;
pub mod codex;
pub mod config;
pub mod crypto;
pub mod db;
pub mod dsh;
pub mod grok;
pub mod history_navigation;
pub mod jobs;
pub mod jsonl_cache;
pub mod local;
pub mod platform;
pub mod probe;
pub mod probe_error_monitor;
pub mod providers;
mod read_cache;
pub mod read_tasks;
pub mod resource_limits;
mod scan_work;
pub mod security;
mod selected_codex;
pub mod services;
mod session_delete;
pub mod session_storage;
pub mod shell;
pub mod system;
pub mod update;

pub use config::Config;

pub mod native_probe;
pub mod provider_sessions;

pub mod user_message;

pub fn evict_idle_detail_caches(max_idle: std::time::Duration) {
    codex::evict_idle_detail_cache(max_idle);
    claude::evict_idle_detail_cache(max_idle);
    grok::evict_idle_detail_cache(max_idle);
    native_probe::evict_idle_cache(max_idle);
    dsh::notifications::evict_idle_cache(max_idle);
    dsh::sessions::evict_idle_fold_cache(max_idle);
    services::probe::evict_idle_live_provider_sessions_cache(max_idle);
    antigravity::sessions::evict_idle_endpoint_cache(max_idle);
}

#[cfg(all(test, unix))]
pub(crate) fn write_executable_fixture(path: &std::path::Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let body = if let Some(rest) = script.strip_prefix("#!/bin/sh\n") {
        format!("#!/bin/sh\nif [ \"${{AGENTHUB_TEST_EXEC_PROBE:-}}\" = \"1\" ]; then exit 0; fi\n{rest}")
    } else {
        script.to_string()
    };
    std::fs::write(path, body).expect("write executable fixture");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .expect("set executable fixture permissions");
    for attempt in 0..10 {
        match std::process::Command::new(path)
            .env("AGENTHUB_TEST_EXEC_PROBE", "1")
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
