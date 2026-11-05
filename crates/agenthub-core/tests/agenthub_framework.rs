use agenthub_core::{
    config::Config,
    local::local_plugin_catalog,
    platform::{PlatformKind, PlatformPaths},
    probe::{ProbeActionPlanKind, ProbeEventInput, ProbeEventOutcome, ProbeRuntime},
    providers::{AgentProviderId, ProviderRegistry},
};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{fs, path::PathBuf, time::SystemTime};

#[test]
fn default_config_uses_agenthub_runtime_names() {
    let config = Config::default();

    let platform = PlatformPaths::current();
    assert_eq!(config.paths.data_dir, platform.data_dir);
    assert_eq!(
        config.paths.db_path,
        platform.data_dir.join("agenthub.sqlite")
    );
    assert_eq!(config.paths.log_dir, platform.log_dir);
}

#[test]
fn linux_default_config_values_stay_cloud_compatible() {
    let config = Config::for_platform_kind(PlatformKind::Linux);

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
fn platform_paths_cover_linux_macos_and_windows() {
    let mac_home = temp_dir("agenthub-platform-macos-home");
    fs::create_dir_all(&mac_home).unwrap();

    assert_eq!(
        PlatformPaths::for_kind(PlatformKind::Linux).data_dir,
        PathBuf::from("/var/lib/agenthub-webd")
    );
    let linux = PlatformPaths::for_kind(PlatformKind::Linux);
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
    assert_eq!(
        PlatformPaths::for_kind(PlatformKind::Windows).data_dir,
        PathBuf::from(r"%ProgramData%\AgentHub")
    );
    let windows = PlatformPaths::for_kind(PlatformKind::Windows);
    assert_eq!(
        windows.daemon_binary(),
        windows.data_dir.join("bin").join("agenthub-webd.exe")
    );
    fs::remove_dir_all(mac_home).unwrap();
}

#[test]
fn provider_registry_exposes_codex_grok_build_and_pi() {
    let registry = ProviderRegistry::default();
    let providers = registry.list();

    let codex = providers
        .iter()
        .find(|provider| provider.id == AgentProviderId::Codex)
        .unwrap();
    assert_eq!(codex.status, "ready");
    assert!(codex
        .capabilities
        .iter()
        .any(|capability| capability == "ready"));

    let grok = providers
        .iter()
        .find(|provider| provider.id == AgentProviderId::GrokBuild)
        .unwrap();
    assert_eq!(grok.status, "ready");
    assert!(grok
        .capabilities
        .iter()
        .any(|capability| capability == "readonly"));
    assert!(grok
        .capabilities
        .iter()
        .any(|capability| capability == "rename"));
    assert!(grok.safety.contains("native rename"));

    assert!(providers
        .iter()
        .any(|provider| provider.id == AgentProviderId::Cursor));
    assert!(providers
        .iter()
        .any(|provider| provider.id == AgentProviderId::Gemini));
}

#[test]
fn local_plugin_catalog_matches_existing_builtin_surface() {
    let plugins = local_plugin_catalog();
    let plugin_json = serde_json::to_value(&plugins).unwrap();

    assert_eq!(plugin_json.as_array().unwrap().len(), 4);
    assert_eq!(plugin_json[0]["id"], "codex");
    assert_eq!(plugin_json[0]["status"], "ready");
    assert_eq!(plugin_json[0]["kind"], "builtin");
    assert_eq!(plugin_json[0]["invocation_template"], "@Codex ");
    assert_eq!(plugin_json[1]["id"], "probe");
    assert_eq!(plugin_json[2]["id"], "grok_build");
    assert_eq!(plugin_json[2]["status"], "ready");
    assert_eq!(plugin_json[2]["invocation_template"], "@Grok Build ");
    assert_eq!(plugin_json[3]["id"], "system_ops");
}

#[tokio::test]
async fn probe_status_is_builtin_and_does_not_require_legacy_cli() {
    let root = temp_dir("agenthub-probe-status");
    let config = sandboxed_config(&root);
    seed_codex_logs_db(&config.codex.home.join("logs_2.sqlite"), &[]);
    let status = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux))
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
    let status = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux))
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

#[test]
fn probe_rejects_unsafe_thread_probe_ids() {
    assert!(agenthub_core::probe::safe_thread_probe_id("thread-abc"));
    assert!(!agenthub_core::probe::safe_thread_probe_id("thread one"));
    assert!(!agenthub_core::probe::safe_thread_probe_id("../thread"));
}

#[test]
fn probe_action_plans_use_agenthub_commands_and_confirmation_ids() {
    let root = temp_dir("agenthub-probe-action-plans");
    let config = sandboxed_config(&root);
    let runtime = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux));

    let hook_plan = runtime
        .plan_action(ProbeActionPlanKind::InstallHooks)
        .unwrap();
    assert!(hook_plan.plan_id.starts_with("probe-hooks-"));
    let hook_command_step = hook_plan
        .steps
        .iter()
        .find(|step| step.contains("probe hook-stop"))
        .expect("hook command step");
    assert!(hook_command_step
        .contains("agenthub-webd --config /etc/agenthub-webd/config.toml probe hook-stop"));
    assert!(!hook_plan
        .steps
        .iter()
        .any(|step| step.contains("codex-sentinel")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_diagnostics_lifecycle_and_hook_status_expose_builtin_runtime_boundaries() {
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
    let runtime = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux));

    let lifecycle = runtime.lifecycle_status();
    assert_eq!(lifecycle.status, "managed");
    assert_eq!(lifecycle.service_name, "agenthub-webd");
    assert_eq!(lifecycle.service_kind, "systemd");
    assert!(lifecycle.hooks_enabled);
    assert!(lifecycle.notifications_enabled);
    assert_eq!(
        lifecycle.next_actions,
        vec!["probe-hook-ready".to_string(),]
    );

    let hook = runtime.hook_status();
    assert_eq!(hook.status, "managed");
    assert!(hook.hook_command.contains("/usr/local/bin/agenthub-webd"));
    assert!(hook.supported_events.contains(&"hook-stop".to_string()));
    assert!(hook
        .supported_events
        .contains(&"notify-completion".to_string()));
    assert_eq!(hook.dedupe_namespace, "probe_event");

    let diagnostics = runtime.diagnostics();
    assert_eq!(diagnostics.doctor_status, "ready");
    assert_eq!(diagnostics.lifecycle_status, "managed");
    assert!(diagnostics
        .managed_boundaries
        .iter()
        .any(|boundary| boundary.contains("不执行自动回复")));
    assert_eq!(
        diagnostics.effective_constants["hidden_desktop_control"],
        false
    );
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
    let root = temp_dir("agenthub-capabilities-sandbox");
    let config = sandboxed_config(&root);
    let paths = PlatformPaths::for_kind(PlatformKind::Macos);
    let response =
        agenthub_core::services::use_cases::AgentHubUseCases::with_config(&config, &paths)
            .system()
            .unwrap()
            .runtime_capabilities();
    let json = serde_json::to_value(response).unwrap();
    assert_eq!(json["host_surface"], "desktop_embedded_tauri");
    assert_eq!(json["capabilities"]["thread_cleanup"], true);
    assert_eq!(json["api_version"], 2);
    assert_eq!(json.as_object().unwrap().len(), 3);
    assert!(json["capabilities"].get("status").is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn probe_event_model_dedupes_hook_stop_and_completion_without_desktop_control() {
    let root = temp_dir("agenthub-probe-event-model");
    let runtime = ProbeRuntime::new(
        sandboxed_config(&root),
        PlatformPaths::for_kind(PlatformKind::Linux),
    );

    let hook = runtime.build_event(ProbeEventInput::hook_stop(
        Some("thread-a"),
        Some("turn-1"),
        "hook-stop",
    ));
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
    assert_eq!(hook.payload["auto_reply"], false);
    assert_eq!(hook.payload["hidden_desktop_control"], false);

    let duplicate_hook = runtime.build_event(ProbeEventInput::hook_stop(
        Some("thread-a"),
        Some("turn-1"),
        "hook-stop",
    ));
    assert_eq!(hook.dedupe_key, duplicate_hook.dedupe_key);

    let completion = runtime.build_event(ProbeEventInput::notify_completion(
        Some("thread-a"),
        Some("turn-1"),
    ));
    assert_eq!(completion.kind, "completion");
    assert!(completion
        .dedupe_key
        .starts_with("completion:thread-a:turn-1:completion:"));
    assert!(completion.payload["bark"].get("body").is_none());
    assert_eq!(completion.payload["notify_completion"], true);
    assert_eq!(completion.payload["auto_reply"], false);

    let recorded = ProbeEventOutcome::from_claim(&completion, true);
    assert!(recorded.recorded);
    assert_eq!(recorded.dedupe_key, completion.dedupe_key);
    let duplicate = ProbeEventOutcome::from_claim(&completion, false);
    assert!(!duplicate.recorded);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn probe_bark_test_plan_redacts_device_key_and_keeps_payload_minimal() {
    let root = temp_dir("agenthub-bark-test-plan");
    let mut config = sandboxed_config(&root);
    config.probe.notifications.enabled = true;
    config.probe.notifications.group = "AgentHub Ops".to_string();
    config.probe.notifications.sound = Some("alarm".to_string());
    config.probe.notifications.url = Some("https://example.com/click".to_string());
    let runtime = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux));

    let plan = runtime.bark_test_plan(true);
    let rendered = serde_json::to_string(&plan).unwrap();

    assert_eq!(plan.kind, "bark-test");
    assert_eq!(plan.payload["configured"], true);
    assert_eq!(plan.payload["device_key"], "[configured]");
    assert_eq!(plan.payload["bark_payload"]["title"], "AgentHub 推送测试");
    assert_eq!(
        plan.payload["bark_payload"]["body"],
        agenthub_core::probe::bark_test_body()
    );
    assert!(plan.payload["bark_payload"].get("device_key").is_none());
    assert!(plan.payload["bark_payload"].get("sound").is_none());
    assert!(plan.payload["bark_payload"].get("group").is_none());
    assert!(plan.payload["bark_payload"].get("url").is_none());
    assert!(!rendered.contains("secret"));
    assert!(!rendered.contains("alarm"));
    assert!(!rendered.contains("example.com"));
    let _ = fs::remove_dir_all(root);
}

fn sandboxed_config(root: &std::path::Path) -> Config {
    let codex_home = root.join(".codex");
    fs::create_dir_all(&codex_home).unwrap();
    let mut config = Config::default();
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
