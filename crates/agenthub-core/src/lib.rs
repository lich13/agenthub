pub mod antigravity;
pub mod archive;
pub mod claude;
pub mod codex;
pub mod config;
pub mod crypto;
pub mod db;
pub mod dsh;
pub mod grok;
pub mod grokbot;
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

/// Share the updater's ring provider across core and daemon HTTP clients.
pub fn http_client_builder() -> reqwest::ClientBuilder {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
}

pub mod native_probe;
pub mod provider_sessions;

pub mod user_message;

pub fn evict_idle_detail_caches(max_idle: std::time::Duration) {
    codex::evict_idle_detail_cache(max_idle);
    claude::evict_idle_detail_cache(max_idle);
    grok::evict_idle_detail_cache(max_idle);
    grokbot::evict_idle(max_idle);
    native_probe::evict_idle_cache(max_idle);
    dsh::notifications::evict_idle_cache(max_idle);
    dsh::sessions::evict_idle_fold_cache(max_idle);
    provider_sessions::HISTORY_CACHE.evict_idle(max_idle);
    services::probe::evict_idle_live_provider_sessions_cache(max_idle);
    antigravity::sessions::evict_idle_endpoint_cache(max_idle);
}

#[cfg(all(test, unix))]
pub(crate) fn write_executable_fixture(path: &std::path::Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let rest = script
        .strip_prefix("#!/bin/sh\n")
        .expect("executable fixture script must start with #!/bin/sh");
    let body = format!(
        "#!/bin/sh\nif [ \"${{AGENTHUB_TEST_EXEC_PROBE:-}}\" = \"1\" ]; then exit 0; fi\n{rest}"
    );
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

#[cfg(all(test, unix))]
mod fixture_tests {
    #[test]
    #[should_panic(expected = "executable fixture script must start with #!/bin/sh")]
    fn write_executable_fixture_rejects_non_sh_shebang() {
        let path =
            std::env::temp_dir().join(format!("agenthub-non-sh-fixture-{}", uuid::Uuid::new_v4()));
        super::write_executable_fixture(&path, "#!/usr/bin/env node\nprocess.exit(0);\n");
    }
}

pub mod github_ci;
