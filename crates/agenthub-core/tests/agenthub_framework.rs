use agenthub_core::{
    config::Config,
    platform::{PlatformKind, PlatformPaths},
    probe::{ProbeEventInput, ProbeEventOutcome, ProbeRuntime},
};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{fs, path::PathBuf, time::SystemTime};

#[test]
fn default_config_uses_agenthub_runtime_names() {
    let config = Config::current_defaults().unwrap();

    let platform = PlatformPaths::current().unwrap();
    assert_eq!(config.paths.data_dir, platform.data_dir);
    assert_eq!(
        config.paths.db_path,
        platform.data_dir.join("agenthub.sqlite")
    );
    assert_eq!(config.paths.log_dir, platform.log_dir);
}

#[test]
fn linux_default_config_values_stay_cloud_compatible() {
    let config = Config::linux_defaults();

    assert_eq!(
        config.paths.data_dir,
        PathBuf::from("/var/lib/agenthub-webd")
    );
    assert_eq!(
        config.paths.db_path,
        PathBuf::from("/var/lib/agenthub-webd/agenthub.sqlite")
    );
    assert_eq!(
        config.paths.log_dir,
        PathBuf::from("/var/log/agenthub-webd")
    );
    assert_eq!(
        config.codex.workspace,
        PathBuf::from("/home/ubuntu/codex-workspace")
    );
}

#[test]
fn macos_default_config_uses_application_support_and_tauri_app_paths() {
    let home = temp_dir("agenthub-macos-default-home");
    fs::create_dir_all(&home).unwrap();
    let config = Config::for_platform_kind_with_home(PlatformKind::Macos, &home);

    assert_eq!(
        config.paths.data_dir,
        home.join("Library/Application Support/AgentHub")
    );
    assert_eq!(
        config.paths.db_path,
        home.join("Library/Application Support/AgentHub/agenthub.sqlite")
    );
    assert_eq!(config.paths.log_dir, home.join("Library/Logs/AgentHub"));
    assert_eq!(config.codex.home.to_string_lossy(), "auto");
    assert!(config.codex.workspace.starts_with(&home));
    assert!(!config
        .codex
        .workspace
        .to_string_lossy()
        .contains("/home/ubuntu"));
    assert!(!toml::to_string(&config).unwrap().contains("/opt/agenthub"));
    assert!(!toml::to_string(&config).unwrap().contains("/home/ubuntu"));

    fs::remove_dir_all(home).unwrap();
}

#[test]
fn platform_paths_cover_linux_and_macos() {
    let mac_home = temp_dir("agenthub-platform-macos-home");
    fs::create_dir_all(&mac_home).unwrap();

    assert_eq!(
        PlatformPaths::linux_server().data_dir,
        PathBuf::from("/var/lib/agenthub-webd")
    );
    let linux = PlatformPaths::linux_server();
    assert_eq!(
        linux.config_file,
        PathBuf::from("/etc/agenthub-webd/config.toml")
    );
    assert_eq!(linux.log_dir, PathBuf::from("/var/log/agenthub-webd"));
    assert_eq!(linux.service_name, "agenthub-webd");
    assert_eq!(linux.service_kind, "systemd");
    assert_eq!(
        linux.service_file,
        Some(PathBuf::from("/etc/systemd/system/agenthub-webd.service"))
    );
    assert_eq!(
        linux.daemon_binary(),
        PathBuf::from("/usr/local/bin/agenthub-webd")
    );
    let macos = PlatformPaths::for_kind_with_home(PlatformKind::Macos, &mac_home);
    assert_eq!(
        macos.data_dir,
        mac_home.join("Library/Application Support/AgentHub")
    );
    assert_eq!(
        macos.config_file,
        mac_home.join("Library/Application Support/AgentHub/config.toml")
    );
    assert_eq!(macos.log_dir, mac_home.join("Library/Logs/AgentHub"));
    assert_eq!(macos.service_file, None);
    assert_eq!(macos.service_name, "AgentHub.app");
    assert_eq!(macos.service_kind, "tauri");
    assert_eq!(
        macos.daemon_binary(),
        mac_home.join("Library/Application Support/AgentHub/bin/agenthub-webd")
    );
    fs::remove_dir_all(mac_home).unwrap();
}

#[tokio::test]
async fn probe_status_is_builtin_and_does_not_require_legacy_cli() {
    let root = temp_dir("agenthub-probe-status");
    let config = sandboxed_config(&root);
    seed_codex_logs_db(&config.codex.home.join("logs_2.sqlite"), &[]);
    let status = ProbeRuntime::new(config, PlatformPaths::linux_server())
        .status()
        .await
        .unwrap();

    assert_eq!(status.label, "Probe");
    assert!(status.enabled);
    assert!(status.available);
    assert_eq!(status.flavor, "builtin");
    assert_eq!(status.service_name, "agenthub-webd");
    assert!(status.binary_path.is_none());
    assert_eq!(status.lifecycle_status, "managed");
    assert_eq!(status.doctor_status, "ready");
    assert_eq!(
        status.config_path,
        PathBuf::from("/etc/agenthub-webd/config.toml")
    );
    assert_eq!(status.recent_event_count, 0);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn probe_status_exposes_hook_and_bark_config_without_device_key() {
    let root = temp_dir("agenthub-probe-status-bark-config");
    let mut config = sandboxed_config(&root);
    seed_codex_logs_db(&config.codex.home.join("logs_2.sqlite"), &[]);
    config.probe.hooks.manage_stop_hook = true;
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_completion = true;
    config.probe.notifications.notify_reply_needed = false;
    config.probe.notifications.notify_recoverable = true;
    config.probe.notifications.server_url = "https://api.day.app/custom".to_string();
    let status = ProbeRuntime::new(config, PlatformPaths::linux_server())
        .status()
        .await
        .unwrap();
    let status_json = serde_json::to_value(&status).unwrap();

    assert_eq!(status_json["hook_stop_enabled"], true);
    assert_eq!(status_json["hooks_enabled"], true);
    assert_eq!(status_json["bark_status"], "configured");
    assert_eq!(status_json["bark_enabled"], true);
    assert_eq!(status_json["bark_server_url"], "https://api.day.app/custom");
    assert_eq!(status_json["bark_notify_completion"], true);
    assert_eq!(status_json["bark_notify_reply_needed"], false);
    assert_eq!(status_json["bark_notify_recoverable"], true);
    assert!(status_json.get("device_key").is_none());
    assert!(serde_json::to_string(&status_json)
        .unwrap()
        .contains("api.day.app"));
    assert!(!serde_json::to_string(&status_json)
        .unwrap()
        .contains("secret"));
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn probe_status_and_hooks_expose_builtin_runtime_state() {
    let root = temp_dir("agenthub-probe-hook-managed");
    let mut config = sandboxed_config(&root);
    fs::write(
        config.codex.home.join("hooks.json"),
        json!({
            "hooks": {
                "Stop": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "/usr/local/bin/agenthub-webd --config /etc/agenthub-webd/config.toml probe hook-stop"
                    }]
                }],
                "PreToolUse": [{
                    "matcher": agenthub_core::probe::PROBE_QUESTION_HOOK_MATCHER,
                    "hooks": [{
                        "type": "command",
                        "command": "/usr/local/bin/agenthub-webd --config /etc/agenthub-webd/config.toml probe hook-request-user-input",
                        "timeout": 5
                    }]
                }]
            }
        })
        .to_string(),
    )
    .unwrap();
    config.probe.notifications.enabled = true;
    let runtime = ProbeRuntime::new(config, PlatformPaths::linux_server());

    let status = runtime.status().await.unwrap();
    assert_eq!(status.lifecycle_status, "managed");
    assert_eq!(status.doctor_status, "ready");
    assert_eq!(status.service_name, "agenthub-webd");
    assert_eq!(status.service_kind, "systemd");
    assert!(status.hooks_enabled);
    assert!(status.bark_enabled);
    assert_eq!(status.hook_status, "managed");
    assert!(status.hook_command.contains("/usr/local/bin/agenthub-webd"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn macos_probe_hook_command_uses_launchd_paths_and_quotes_application_support() {
    let home = temp_dir("agenthub-macos-hook-home");
    fs::create_dir_all(&home).unwrap();
    let config = Config::for_platform_kind_with_home(PlatformKind::Macos, &home);
    let paths = PlatformPaths::for_kind_with_home(PlatformKind::Macos, &home);
    let runtime = ProbeRuntime::new(config, paths);

    let hook = runtime.hook_status();

    assert!(hook.hook_command.contains("probe hook-stop"));
    assert!(hook.hook_command.contains("'"));
    assert!(hook.hook_command.contains(
        &home
            .join("Library/Application Support/AgentHub/config.toml")
            .display()
            .to_string()
    ));
    assert!(!hook.hook_command.contains("/opt/agenthub"));
    assert!(!hook.hook_command.contains("systemctl"));

    fs::remove_dir_all(home).unwrap();
}

#[test]
fn capabilities_response_has_no_status_collection_or_private_paths() {
    let paths = PlatformPaths::macos("/fixture/home");
    let response = agenthub_core::services::use_cases::AgentHubUseCases::new(&paths)
        .system()
        .runtime_capabilities();
    let json = serde_json::to_value(response).unwrap();
    assert_eq!(json["host_surface"], "desktop_embedded_tauri");
    assert_eq!(json["capabilities"]["thread_cleanup"], true);
    assert_eq!(json["api_version"], 2);
    assert_eq!(json.as_object().unwrap().len(), 3);
    assert!(json["capabilities"].get("status").is_none());
}

#[test]
fn probe_event_model_dedupes_hook_stop_and_completion_without_desktop_control() {
    let root = temp_dir("agenthub-probe-event-model");
    let config = sandboxed_config(&root);
    let kind = PlatformPaths::linux_server().kind;

    let hook = ProbeRuntime::build_event_for(
        &config,
        kind,
        ProbeEventInput::hook_stop(Some("thread-a"), Some("turn-1"), "hook-stop"),
    );
    assert_eq!(hook.kind, "hook-stop");
    assert_eq!(hook.thread_id.as_deref(), Some("thread-a"));
    assert_eq!(hook.dedupe_namespace, "probe_event");
    assert!(hook
        .dedupe_key
        .starts_with("hook-stop:thread-a:turn-1:hook_stop:"));
    assert!(hook.payload["bark"].get("body").is_none());
    assert_eq!(
        hook.payload["bark"]["body_sha256"],
        hook.payload["body_sha256"]
    );
    assert_eq!(hook.ttl_seconds, 300);
    assert_eq!(hook.payload["notify_completion"], false);
    assert!(hook.payload.get("auto_reply").is_none());
    assert!(hook.payload.get("hidden_desktop_control").is_none());

    let duplicate_hook = ProbeRuntime::build_event_for(
        &config,
        kind,
        ProbeEventInput::hook_stop(Some("thread-a"), Some("turn-1"), "hook-stop"),
    );
    assert_eq!(hook.dedupe_key, duplicate_hook.dedupe_key);

    let completion = ProbeRuntime::build_event_for(
        &config,
        kind,
        ProbeEventInput::notify_completion(Some("thread-a"), Some("turn-1")),
    );
    assert_eq!(completion.kind, "completion");
    assert!(completion
        .dedupe_key
        .starts_with("completion:thread-a:turn-1:completion:"));
    assert!(completion.payload["bark"].get("body").is_none());
    assert_eq!(completion.payload["notify_completion"], true);
    assert!(completion.payload.get("auto_reply").is_none());

    let recorded = ProbeEventOutcome::from_claim(&completion, true);
    assert!(recorded.recorded);
    assert_eq!(recorded.dedupe_key, completion.dedupe_key);
    let duplicate = ProbeEventOutcome::from_claim(&completion, false);
    assert!(!duplicate.recorded);
    let _ = fs::remove_dir_all(root);
}

fn sandboxed_config(root: &std::path::Path) -> Config {
    let codex_home = root.join(".codex");
    fs::create_dir_all(&codex_home).unwrap();
    let mut config = Config::current_defaults().unwrap();
    config.codex.home = codex_home;
    config.codex.workspace = root.join("workspace");
    config.paths.data_dir = root.join("data");
    config.paths.db_path = root.join("agenthub.sqlite");
    config.paths.log_dir = root.join("logs");

    config
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{label}-{unique}"))
}

fn seed_codex_logs_db(path: &std::path::Path, timestamps: &[i64]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            ts_nanos INTEGER NOT NULL,
            level TEXT NOT NULL,
            target TEXT NOT NULL,
            feedback_log_body TEXT,
            module_path TEXT,
            file TEXT,
            line INTEGER,
            thread_id TEXT,
            process_uuid TEXT,
            estimated_bytes INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_logs_ts ON logs(ts DESC, ts_nanos DESC, id DESC);
        "#,
    )
    .unwrap();
    for ts in timestamps {
        conn.execute(
            "INSERT INTO logs(ts, ts_nanos, level, target, estimated_bytes) VALUES(?1, 0, 'INFO', 'test', 1)",
            params![ts],
        )
        .unwrap();
    }
}
