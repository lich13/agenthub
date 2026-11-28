use super::*;
use rusqlite::{params, Connection};
use std::io::{BufRead, BufReader, Read, Write};
use std::time::SystemTime;

#[test]
fn disabled_error_monitor_retains_timestamp_until_status_changes() {
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let mut status = ProbeErrorMonitorRuntimeStatus {
        status: "disabled".into(),
        last_scan_at: 1,
        last_error: None,
        rows_seen: 0,
        new_incidents: 0,
        incident_count: 0,
        cursor: None,
    };
    store_probe_error_monitor_status(&db, status.clone()).unwrap();
    status.last_scan_at = 2;
    store_probe_error_monitor_status(&db, status.clone()).unwrap();
    let saved: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
        &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved.last_scan_at, 1);
    status.incident_count = 1;
    store_probe_error_monitor_status(&db, status.clone()).unwrap();
    let saved: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
        &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved, status);
    status.status = "watching".into();
    for timestamp in [3, 4] {
        status.last_scan_at = timestamp;
        store_probe_error_monitor_status(&db, status.clone()).unwrap();
        let saved: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
            &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved, status);
    }
}

#[test]
fn idle_ok_error_monitor_heartbeat_throttles_unchanged_db_writes_for_300s() {
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let mut status = ProbeErrorMonitorRuntimeStatus {
        status: "ok".into(),
        last_scan_at: 1_000,
        last_error: None,
        rows_seen: 0,
        new_incidents: 0,
        incident_count: 2,
        cursor: Some(agenthub_core::probe_error_monitor::ProbeErrorCursor {
            database_identity: String::new(),
            ts: 100,
            ts_nanos: 0,
            id: 42,
        }),
    };
    store_probe_error_monitor_status(&db, status.clone()).unwrap();
    status.last_scan_at = 1_015;
    store_probe_error_monitor_status(&db, status.clone()).unwrap();
    let saved_early: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
        &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved_early.last_scan_at, 1_000);

    status.last_scan_at = 1_300;
    store_probe_error_monitor_status(&db, status.clone()).unwrap();
    let saved_after_window: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
        &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved_after_window.last_scan_at, 1_300);

    status.last_scan_at = 1_305;
    status.cursor = Some(agenthub_core::probe_error_monitor::ProbeErrorCursor {
        database_identity: String::new(),
        ts: 100,
        ts_nanos: 0,
        id: 43,
    });
    store_probe_error_monitor_status(&db, status.clone()).unwrap();
    let saved_on_cursor_advance: ProbeErrorMonitorRuntimeStatus = serde_json::from_str(
        &db.get_setting(PROBE_ERROR_MONITOR_STATUS_SETTING)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved_on_cursor_advance.last_scan_at, 1_305);
    assert_eq!(saved_on_cursor_advance.cursor.unwrap().id, 43);
}

#[test]
fn hook_request_user_input_cli_subcommand_is_internal_and_argument_free() {
    assert!(Cli::try_parse_from(["agenthub-webd", "probe", "hook-request-user-input"]).is_ok());
    assert!(
        Cli::try_parse_from(["agenthub-webd", "probe", "hook-request-user-input-confirm"]).is_ok()
    );
    assert!(Cli::try_parse_from([
        "agenthub-webd",
        "probe",
        "hook-request-user-input",
        "--thread-id",
        "thread-a"
    ])
    .is_err());
}

#[test]
fn legacy_import_is_hidden_and_retired() {
    use clap::CommandFactory;
    let mut command = Cli::command();
    let mut help = Vec::new();
    command
        .find_subcommand_mut("probe")
        .unwrap()
        .write_long_help(&mut help)
        .unwrap();
    assert!(!String::from_utf8(help).unwrap().contains("legacy-import"));
}

#[test]
fn codex_config_patch_enables_features_hooks_and_preserves_existing_values() {
    let updated = ensure_codex_hooks_feature(
        r#"
model = "gpt-5"

[features]
foo = true
hooks = false
"#,
    )
    .unwrap();

    assert!(updated.contains("hooks = true"));
    assert!(updated.contains("foo = true"));
    assert!(updated.contains("model = \"gpt-5\""));
}

#[test]
fn codex_config_patch_creates_features_table_when_missing() {
    let updated = ensure_codex_hooks_feature("model = \"gpt-5\"\n").unwrap();

    assert!(updated.contains("[features]"));
    assert!(updated.contains("hooks = true"));
}

const COMMENTED_CODEX_CONFIG: &str = r#"# 個人 Codex 設定
model = "gpt-5" # inline note
approval_policy = "on-request"

[features]
# keep this line
zeta = true
hooks = false # was off
alpha = true

[projects."/workspace/fixture"]
trust_level = "trusted"
"#;

#[test]
fn codex_config_patch_keeps_comments_key_order_and_untouched_bytes() {
    assert_eq!(
        ensure_codex_hooks_feature(COMMENTED_CODEX_CONFIG).unwrap(),
        COMMENTED_CODEX_CONFIG.replace("hooks = false # was off", "hooks = true # was off")
    );

    // A missing table is appended, also when existing tables are interleaved.
    let without_features = r#"# header
model = "gpt-5"   # spaced

[projects."/workspace/first"]
trust_level = "trusted"

[notice]
hide = true

[projects."/workspace/second"]
trust_level = "trusted"
"#;
    assert_eq!(
        ensure_codex_hooks_feature(without_features).unwrap(),
        format!("{without_features}\n[features]\nhooks = true\n")
    );
}

#[test]
fn codex_hook_state_prune_removes_only_stale_entries() {
    let original = r#"# header
model = "gpt-5"

[features]
hooks = true # managed

[hooks.state."/tmp/.codex/hooks.json:stop:0:0"]
last_error = "old agenthub hook"

# third-party state stays
[hooks.state."/tmp/.codex/custom.json:session_start:0:0"]
last_error = "kept"

[hooks.state."/tmp/.codex/hooks.json:pre_tool_use:0:0"]
last_error = "old agenthub request hook"

[profiles.fixture] # trailing table
model = "gpt-5"
"#;
    let expected = r#"# header
model = "gpt-5"

[features]
hooks = true # managed

# third-party state stays
[hooks.state."/tmp/.codex/custom.json:session_start:0:0"]
last_error = "kept"

[profiles.fixture] # trailing table
model = "gpt-5"
"#;
    assert_eq!(prune_codex_hook_state_toml(original).unwrap(), expected);
    assert_eq!(ensure_codex_hooks_feature(original).unwrap(), expected);
}

#[test]
fn codex_config_without_changes_is_returned_byte_for_byte() {
    let original = "# header\r\n[features]\nhooks=true   # already on\n\tzeta = 1\n\n[hooks.state.\"/tmp/.codex/custom.json:stop:0:0\"]\nlast_error = 'kept'\n# trailing\n";
    assert_eq!(ensure_codex_hooks_feature(original).unwrap(), original);
    assert_eq!(prune_codex_hook_state_toml(original).unwrap(), original);
}

async fn install_probe_hooks_for_test(config: &Config, dry_run: bool) -> Result<Value> {
    install_probe_hooks_with_repair(config, None, dry_run, |platform, enabled| {
        Ok(json!({
            "supported": true,
            "enabled": enabled,
            "loaded": enabled,
            "changed": false,
            "label": agenthub_core::probe_error_monitor::PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL,
            "plist_path": Value::Null,
            "helper_path": platform.daemon_binary(),
            "config_path": &platform.config_file,
        }))
    })
    .await
}

#[tokio::test]
async fn installed_hook_commands_keep_the_explicit_config_path() {
    let dir = temp_test_dir("agenthub-hooks-custom-config");
    let home = dir.join("codex");
    fs::create_dir_all(home.join("sessions")).unwrap();
    let config = test_config(home.clone());
    let path = dir.join("custom config.toml");
    let output = install_probe_hooks_with_repair(&config, Some(&path), false, |platform, _| {
        assert_eq!(platform.config_file, path);
        Ok(json!({"changed":false}))
    })
    .await
    .unwrap();
    let expected = shell_quote(&path.display().to_string(), QuoteStyle::Minimal);
    for key in ["hook_command", "request_user_input_hook_command"] {
        assert!(output[key]
            .as_str()
            .unwrap()
            .contains(&format!("--config {expected} ")));
    }
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn install_probe_hooks_writes_hooks_json_and_codex_features_hooks() {
    let dir = temp_test_dir("agenthub-hooks-install");
    let codex_home = dir.join(".codex");
    fs::create_dir_all(&codex_home).unwrap();
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    fs::write(codex_home.join("config.toml"), "model = \"gpt-5\"\n").unwrap();
    let config = test_config(codex_home.clone());

    let result = install_probe_hooks_for_test(&config, false).await.unwrap();

    assert_eq!(result["ok"], true);
    assert_eq!(result["hooks_json_changed"], true);
    assert_eq!(result["codex_config_changed"], true);
    assert!(result["reload_result"].is_null());
    let hooks_json: Value =
        serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
    assert!(hooks_json.to_string().contains("probe hook-stop"));
    let pre_tool_use = hooks_json["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre_tool_use.len(), 1);
    assert_eq!(
        pre_tool_use[0]["matcher"],
        "^(functions\\.)?request_user_input(_async)?$"
    );
    let request_hook = &pre_tool_use[0]["hooks"][0];
    assert!(request_hook["command"]
        .as_str()
        .unwrap()
        .contains("probe hook-request-user-input"));
    assert_eq!(request_hook["timeout"], 5);
    assert!(request_hook.get("async").is_none());
    let codex_config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(codex_config.contains("[features]"));
    assert!(codex_config.contains("hooks = true"));

    let second = install_probe_hooks_for_test(&config, false).await.unwrap();
    assert_eq!(second["changed"], false);
    assert_eq!(
        fs::read(codex_home.join("hooks.json")).unwrap(),
        serde_json::to_vec_pretty(&hooks_json).unwrap()
    );

    let third_party = "/usr/local/bin/custom-codex-hook --event stop";
    let mut with_custom = hooks_json.clone();
    with_custom["hooks"]["Stop"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "matcher": "*",
            "hooks": [{"type": "command", "command": third_party}]
        }));
    fs::write(
        codex_home.join("hooks.json"),
        serde_json::to_vec_pretty(&with_custom).unwrap(),
    )
    .unwrap();
    let probe_cfg_path = dir.join("config.toml");
    fs::write(&probe_cfg_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let removed = remove_probe_hooks(&config, Some(&probe_cfg_path), false)
        .await
        .unwrap();
    assert_eq!(removed["ok"], true);
    assert_eq!(removed["hooks_json_changed"], true);
    assert_eq!(removed["probe_config_changed"], true);
    let after_remove: Value =
        serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
    assert!(!after_remove.to_string().contains("probe hook-stop"));
    assert!(!after_remove
        .to_string()
        .contains("probe hook-request-user-input"));
    assert!(after_remove.to_string().contains(third_party));
    let reloaded = Config::load_runtime(&probe_cfg_path).unwrap();
    assert!(!reloaded.probe.hooks.manage_stop_hook);

    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn install_probe_hooks_replaces_stale_agenthub_hooks_and_hook_state() {
    let dir = temp_test_dir("agenthub-hooks-replace-stale");
    let codex_home = dir.join(".codex");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("hooks.json"),
        serde_json::to_vec_pretty(&json!({
            "hooks": {
                "Stop": [
                    {"hooks": []},
                    {
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/opt/fixture/bin/agenthub-webd --config /opt/fixture/config.toml probe hook-stop"
                        }]
                    },
                    {
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/usr/local/bin/third-party-hook"
                        }]
                    }
                ],
                "PreToolUse": [
                    {
                        "matcher": "^request_user_input$",
                        "hooks": [{
                            "type": "command",
                            "command": "/opt/fixture/bin/agenthub-webd --config /opt/fixture/config.toml probe hook-request-user-input",
                            "timeout": 9,
                            "async": true
                        }]
                    },
                    {
                        "matcher": "^request_user_input$",
                        "hooks": [{
                            "type": "command",
                            "command": "/usr/local/bin/third-party-request-hook"
                        }]
                    }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"
model = "gpt-5"

[features]
hooks = true

[hooks.state."/tmp/.codex/hooks.json:stop:0:0"]
last_error = "old empty group"

[hooks.state."/tmp/.codex/hooks.json:stop:1:0"]
last_error = "old agenthub hook"

[hooks.state."/tmp/.codex/hooks.json:pre_tool_use:0:0"]
last_error = "old agenthub request hook"
"#,
    )
    .unwrap();
    let config = test_config(codex_home.clone());

    let result = install_probe_hooks_for_test(&config, false).await.unwrap();

    assert_eq!(result["hooks_json_changed"], true);
    assert_eq!(result["codex_config_changed"], true);
    let hooks_json: Value =
        serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
    let stop = hooks_json["hooks"]["Stop"].as_array().unwrap();
    let commands = stop
        .iter()
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
        .filter_map(|hook| hook["command"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        commands
            .iter()
            .filter(|command| command.contains("probe hook-stop"))
            .count(),
        1
    );
    assert!(commands
        .iter()
        .any(|command| command.contains("agenthub-webd")));
    assert!(!commands
        .iter()
        .any(|command| command.starts_with("/opt/fixture/")));
    assert!(commands.contains(&"/usr/local/bin/third-party-hook"));
    assert!(stop.iter().all(|group| group["hooks"]
        .as_array()
        .is_some_and(|hooks| !hooks.is_empty())));
    let pre_tool_use = hooks_json["hooks"]["PreToolUse"].as_array().unwrap();
    let request_commands = pre_tool_use
        .iter()
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
        .filter_map(|hook| hook["command"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        request_commands
            .iter()
            .filter(|command| command.contains("probe hook-request-user-input"))
            .count(),
        1
    );
    assert!(request_commands
        .iter()
        .any(|command| command.contains("agenthub-webd")));
    assert!(request_commands.contains(&"/usr/local/bin/third-party-request-hook"));
    let managed_request_hook = pre_tool_use
        .iter()
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
        .find(|hook| {
            hook["command"]
                .as_str()
                .is_some_and(|command| command.contains("agenthub-webd"))
        })
        .unwrap();
    assert_eq!(managed_request_hook["timeout"], 5);
    assert!(managed_request_hook.get("async").is_none());
    let codex_config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(!codex_config.contains("[hooks.state."));

    fs::remove_dir_all(&dir).unwrap();
}

const STALE_CODEX_HOOK_STATE: &str =
    "\n[hooks.state.\"/tmp/.codex/hooks.json:stop:0:0\"]\nlast_error = \"old agenthub hook\"\n";

/// Both files exist and both installing and removing the hooks would rewrite them.
fn seed_stale_probe_hook_files(codex_home: &Path) -> (Vec<u8>, String) {
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    let hooks = serde_json::to_vec_pretty(&json!({
        "hooks": {
            "Stop": [{
                "matcher": "*",
                "hooks": [{
                    "type": "command",
                    "command": "/opt/fixture/bin/agenthub-webd --config /opt/fixture/config.toml probe hook-stop"
                }]
            }]
        }
    }))
    .unwrap();
    let codex_config = format!("{COMMENTED_CODEX_CONFIG}{STALE_CODEX_HOOK_STATE}");
    fs::write(codex_home.join("hooks.json"), &hooks).unwrap();
    fs::write(codex_home.join("config.toml"), &codex_config).unwrap();
    (hooks, codex_config)
}

fn probe_backup_files(codex_home: &Path) -> Vec<String> {
    fs::read_dir(codex_home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("agenthub-probe-bak"))
        .collect()
}

#[tokio::test]
async fn probe_hook_runs_that_copy_nothing_report_no_backup_paths() {
    let dir = temp_test_dir("agenthub-hooks-no-backup");
    let codex_home = dir.join(".codex");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    let config = test_config(codex_home.clone());
    let assert_no_backup = |result: &Value, case: &str| {
        assert!(result["backup_path"].is_null(), "{case}");
        assert!(result["codex_config_backup_path"].is_null(), "{case}");
        assert!(probe_backup_files(&codex_home).is_empty(), "{case}");
    };

    let preview = install_probe_hooks_for_test(&config, true).await.unwrap();
    assert_eq!(preview["changed"], true);
    assert_no_backup(&preview, "dry run without files");
    assert!(!codex_home.join("hooks.json").exists());
    assert!(!codex_home.join("config.toml").exists());

    let created = install_probe_hooks_for_test(&config, false).await.unwrap();
    assert_eq!(created["hooks_json_changed"], true);
    assert_eq!(created["codex_config_changed"], true);
    assert_no_backup(&created, "newly created files");
    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml")).unwrap(),
        "[features]\nhooks = true\n"
    );

    let unchanged = install_probe_hooks_for_test(&config, false).await.unwrap();
    assert_eq!(unchanged["changed"], false);
    assert_no_backup(&unchanged, "unchanged install");

    let (hooks, codex_config) = seed_stale_probe_hook_files(&codex_home);
    for (case, result) in [
        (
            "install dry run",
            install_probe_hooks_for_test(&config, true).await.unwrap(),
        ),
        (
            "remove dry run",
            remove_probe_hooks(&config, None, true).await.unwrap(),
        ),
    ] {
        assert_eq!(result["hooks_json_changed"], true, "{case}");
        assert_eq!(result["codex_config_changed"], true, "{case}");
        assert_no_backup(&result, case);
    }
    assert_eq!(fs::read(codex_home.join("hooks.json")).unwrap(), hooks);
    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml")).unwrap(),
        codex_config
    );

    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn probe_hook_changes_back_up_the_previous_files_and_keep_unrelated_config_text() {
    let dir = temp_test_dir("agenthub-hooks-backup-copy");
    let codex_home = dir.join(".codex");
    let (hooks, codex_config) = seed_stale_probe_hook_files(&codex_home);
    let config = test_config(codex_home.clone());
    let enabled_config =
        COMMENTED_CODEX_CONFIG.replace("hooks = false # was off", "hooks = true # was off");

    let installed = install_probe_hooks_for_test(&config, false).await.unwrap();
    assert_eq!(
        fs::read(installed["backup_path"].as_str().unwrap()).unwrap(),
        hooks
    );
    assert_eq!(
        fs::read_to_string(installed["codex_config_backup_path"].as_str().unwrap()).unwrap(),
        codex_config
    );
    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml")).unwrap(),
        enabled_config
    );

    let hooks = fs::read(codex_home.join("hooks.json")).unwrap();
    let codex_config = format!("{enabled_config}{STALE_CODEX_HOOK_STATE}");
    fs::write(codex_home.join("config.toml"), &codex_config).unwrap();
    let removed = remove_probe_hooks(&config, None, false).await.unwrap();
    assert_eq!(
        fs::read(removed["backup_path"].as_str().unwrap()).unwrap(),
        hooks
    );
    assert_eq!(
        fs::read_to_string(removed["codex_config_backup_path"].as_str().unwrap()).unwrap(),
        codex_config
    );
    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml")).unwrap(),
        enabled_config
    );

    let unchanged = remove_probe_hooks(&config, None, false).await.unwrap();
    assert_eq!(unchanged["changed"], false);
    assert!(unchanged["backup_path"].is_null());
    assert!(unchanged["codex_config_backup_path"].is_null());

    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_stop_records_probe_event_but_returns_codex_stop_json_and_redacted_bark_state() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-a"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_completion = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();

    let event = build_probe_event(
        &config,
        ProbeEventInput::hook_stop_with_context(
            Some("thread-a"),
            Some("turn-1"),
            None,
            None,
            None,
            "hook-stop",
        ),
    );
    let result = handle_built_probe_event(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();

    assert_eq!(
        result.stdout,
        json!({"continue": true, "suppressOutput": false})
    );
    assert!(result.outcome.recorded);
    assert!(!result.bark.sent);
    assert!(result.bark.skipped);
    assert_eq!(result.bark.reason.as_deref(), Some("device_key_missing"));
    assert!(!result.bark.device_key_configured);
    let output = serde_json::to_string(&result).unwrap();
    assert!(!output.contains("device-key"));
    assert!(!output.contains("secret"));

    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "hook-stop");
    assert_eq!(events[0].payload["bark"]["reason"], "device_key_missing");
    assert!(events[0].payload["bark"].get("device_key").is_none());
}

#[tokio::test]
async fn probe_control_payload_suppression_skips_dedupe_db_and_bark() {
    let dir = temp_test_dir("agenthub-control-payload-suppression");
    fs::create_dir_all(&dir).unwrap();
    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-extra-key"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_completion = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();

    for (index, body) in [r#"{"suggestions":[]}"#, r#"{"exclude":[]}"#]
        .into_iter()
        .enumerate()
    {
        let event = build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some(&format!("thread-control-{index}")),
                Some(&format!("turn-control-{index}")),
                Some(&format!("thread-control-{index}")),
                None,
                Some(body),
                "hook-stop",
            ),
        );
        let (outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
            .await
            .unwrap();

        assert!(!outcome.recorded);
        assert!(!outcome.duplicate);
        assert!(bark.skipped);
        assert_eq!(bark.reason.as_deref(), Some("internal_control_payload"));
        assert_eq!(bark.request_count, 0);
    }

    let conn = Connection::open(db.path()).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM probe_events", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );

    let normal_event = build_probe_event(
        &config,
        ProbeEventInput::hook_stop_with_context(
            Some("thread-extra-key"),
            Some("turn-extra-key"),
            Some("thread-extra-key"),
            None,
            Some(r#"{"suggestions":[],"message":"normal JSON"}"#),
            "hook-stop",
        ),
    );
    let (normal_outcome, normal_bark) =
        record_probe_event_with_bark(&|| Ok(config.clone()), &db, normal_event)
            .await
            .unwrap();
    assert!(normal_outcome.recorded);
    assert!(!normal_outcome.duplicate);
    assert_eq!(normal_bark.request_count, 1);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );

    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_stop_suppresses_internal_memory_consolidation_before_body_dedupe_db_and_bark() {
    let dir = temp_test_dir("agenthub-memory-hook-stop-suppression");
    let codex_home = dir.join("custom-codex-home");
    let memory_root = codex_home.join("memories");
    let normalized_cwd = memory_root.join("nested").join("..");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    fs::create_dir_all(memory_root.join("nested")).unwrap();
    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let mut config = test_config(codex_home);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_completion = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let memory_body = "MEMORY_CONSOLIDATION_BODY_MUST_NOT_BE_READ_OR_STORED";

    let event_input = hook_stop_event_input(
        &config,
        Some(&json!({
            "cwd": normalized_cwd,
            "hook_event_name": "Stop",
            "ephemeral": true,
            "session_id": "00000000-0000-4000-8000-000000000106",
            "turn_id": "turn-memory",
            "transcript_path": null,
            "last_assistant_message": memory_body,
        })),
        None,
        None,
        "hook-stop",
    )
    .unwrap();
    let event = build_probe_event(&config, event_input);

    assert_eq!(
        event.suppression_reason.as_deref(),
        Some("internal_memory_consolidation")
    );
    assert!(event.payload["last_assistant_message"].is_null());
    assert!(!event.bark_body.contains(memory_body));

    let result = handle_built_probe_event(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();
    assert_eq!(
        result.stdout,
        json!({"continue": true, "suppressOutput": false})
    );
    assert!(!result.outcome.recorded);
    assert!(!result.outcome.duplicate);
    assert_eq!(
        result.bark.reason.as_deref(),
        Some("internal_memory_consolidation")
    );
    assert_eq!(result.bark.request_count, 0);

    let conn = Connection::open(db.path()).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM probe_events", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hook_stop_memory_identity_is_exact_and_fails_open_for_counterexamples() {
    let dir = temp_test_dir("agenthub-memory-hook-stop-counterexamples");
    let codex_home = dir.join(".codex");
    let memory_root = codex_home.join("memories");
    let ordinary_cwd = dir.join("workspace");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    fs::create_dir_all(&memory_root).unwrap();
    fs::create_dir_all(&ordinary_cwd).unwrap();
    let transcript = dir.join("rollout.jsonl");
    fs::write(
        &transcript,
        json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "turn_id": "turn-user",
                "last_agent_message": "用户线程完成"
            }
        })
        .to_string(),
    )
    .unwrap();
    let config = test_config(codex_home);

    let payloads = [
        json!({
            "cwd": ordinary_cwd,
            "session_id": "ordinary-empty-transcript",
            "turn_id": "turn-user",
            "transcript_path": null,
            "last_assistant_message": "正文提及 MEMORY.md、memory_summary.md 和记忆整理"
        }),
        json!({
            "cwd": memory_root,
            "session_id": "memory-with-transcript",
            "turn_id": "turn-user",
            "transcript_path": transcript,
            "last_assistant_message": "显式用户线程"
        }),
        json!({
            "session_id": "missing-cwd",
            "turn_id": "turn-user",
            "transcript_path": null,
            "last_assistant_message": "普通用户线程"
        }),
        json!({
            "cwd": dir.join("does-not-exist"),
            "session_id": "invalid-cwd",
            "turn_id": "turn-user",
            "transcript_path": null,
            "last_assistant_message": "普通用户线程"
        }),
        json!({
            "cwd": memory_root,
            "session_id": "invalid-transcript-field",
            "turn_id": "turn-user",
            "transcript_path": 42,
            "last_assistant_message": "普通用户线程"
        }),
    ];

    for payload in payloads {
        let event_input =
            hook_stop_event_input(&config, Some(&payload), None, None, "hook-stop").unwrap();
        let event = build_probe_event(&config, event_input);
        assert_ne!(
            event.suppression_reason.as_deref(),
            Some("internal_memory_consolidation"),
            "payload should fail open: {payload}"
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hook_request_user_input_memory_context_with_transcript_or_invalid_field_fails_open() {
    let dir = temp_test_dir("agenthub-memory-pre-tool-use-counterexamples");
    let codex_home = dir.join(".codex");
    let memory_root = codex_home.join("memories");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    fs::create_dir_all(&memory_root).unwrap();
    let config = test_config(codex_home);
    notification_accuracy_tests::seed_main_task_identities(
        &config.codex.home,
        &["thread-user-question"],
    );

    for transcript_path in [json!("/tmp/user-rollout.jsonl"), json!(42)] {
        let event_input = hook_request_user_input_event_input(
            &config,
            json!({
                "cwd": memory_root,
                "hook_event_name": "PreToolUse",
                "session_id": "thread-user-question",
                "turn_id": "turn-user-question",
                "tool_name": "request_user_input",
                "tool_use_id": "call-user-question",
                "tool_input": {
                    "questions": [{
                        "id": "mode",
                        "question": "Choose a mode?",
                        "options": [{"label": "Continue"}]
                    }]
                },
                "transcript_path": transcript_path
            }),
        )
        .unwrap();
        let event = build_probe_event(&config, event_input);
        assert_ne!(
            event.suppression_reason.as_deref(),
            Some("internal_memory_consolidation")
        );
        assert!(event.bark_body.contains("Choose a mode?"));
    }
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_request_user_input_suppresses_memory_context_without_db_dedupe_or_bark() {
    let dir = temp_test_dir("agenthub-memory-pre-tool-use-suppression");
    let codex_home = dir.join(".codex");
    let memory_root = codex_home.join("memories");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    fs::create_dir_all(&memory_root).unwrap();
    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let mut config = test_config(codex_home);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let event_input = hook_request_user_input_event_input(
        &config,
        json!({
            "cwd": memory_root,
            "hook_event_name": "PreToolUse",
            "session_id": "thread-memory-question",
            "turn_id": "turn-memory-question",
            "tool_name": "request_user_input",
            "tool_use_id": "call-memory-question",
            "tool_input": {
                "questions": [{
                    "id": "memory",
                    "question": "Internal memory question?",
                    "options": [{"label": "Continue"}]
                }]
            },
            "transcript_path": null
        }),
    )
    .unwrap();
    let event = build_probe_event(&config, event_input);

    assert_eq!(
        event.suppression_reason.as_deref(),
        Some("internal_memory_consolidation")
    );
    let (outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();
    assert!(!outcome.recorded);
    assert!(!outcome.duplicate);
    assert_eq!(
        bark.reason.as_deref(),
        Some("internal_memory_consolidation")
    );
    assert_eq!(bark.request_count, 0);
    assert!(db.list_probe_events(10).unwrap().is_empty());
    let conn = Connection::open(db.path()).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_stop_records_stdin_compatible_context_fields() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["session-stdin"]);
    config.probe.notifications.enabled = false;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let event = build_probe_event(
        &config,
        ProbeEventInput::hook_stop_with_context(
            None,
            Some("turn-stdin"),
            Some("session-stdin"),
            Some("/tmp/transcript.jsonl"),
            Some("assistant body"),
            "hook-stop",
        ),
    );

    let result = handle_built_probe_event(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();

    assert_eq!(
        result.stdout,
        json!({"continue": true, "suppressOutput": false})
    );
    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].thread_id.as_deref(), Some("session-stdin"));
    assert_eq!(events[0].payload["session_id"], "session-stdin");
    assert_eq!(events[0].payload["turn_id"], "turn-stdin");
    assert_eq!(
        events[0].payload["transcript_path"],
        "/tmp/transcript.jsonl"
    );
    assert_eq!(
        events[0].payload["last_assistant_message"]["summary"],
        "assistant body"
    );
    assert_eq!(
        events[0].payload["last_assistant_message"]["classification"],
        "completion"
    );
    assert_eq!(events[0].payload["event_type"], "completion");
    assert_eq!(events[0].payload["raw_kind"], "hook-stop");
    assert_eq!(events[0].payload["dedupe"]["namespace"], "probe_event");
    assert_eq!(events[0].payload["dedupe"]["claimed"], true);
    assert_eq!(events[0].payload["dedupe"]["duplicate"], false);
    assert_eq!(events[0].kind, "completion");
}

#[tokio::test]
async fn hook_stop_uses_transcript_latest_assistant_when_stdin_omits_body() {
    let (mut config, _identity) = notification_accuracy_tests::test_notification_config(&[
        "thread-transcript",
        "session-transcript",
    ]);
    config.probe.notifications.enabled = false;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let dir = temp_test_dir("agenthub-hook-transcript-summary");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    fs::write(
        &transcript,
        [
            json!({"type":"response_item","turn_id":"turn-transcript","payload":{"type":"message","role":"assistant","content":[{"text":"first answer"}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-transcript","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"text":"final answer"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let selection = hook_stop_last_assistant_message(
        Some(transcript.to_string_lossy().as_ref()),
        Some("turn-transcript"),
        None,
    )
    .unwrap()
    .unwrap();
    let event = build_probe_event(
        &config,
        ProbeEventInput::hook_stop_with_context(
            Some("thread-transcript"),
            Some("turn-transcript"),
            Some("session-transcript"),
            Some(transcript.to_string_lossy().as_ref()),
            Some(&selection.message),
            "hook-stop",
        )
        .with_body_source(Some(&selection.source)),
    );

    handle_built_probe_event(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();

    let events = db.list_probe_events(10).unwrap();
    assert_eq!(
        events[0].payload["last_assistant_message"]["summary"],
        "final answer"
    );
    assert_eq!(
        events[0].payload["last_assistant_message"]["classification"],
        "completion"
    );
    assert_eq!(events[0].payload["body_summary"], "final answer");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_stop_uses_local_thread_title_for_bark_title_when_payload_omits_title() {
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let dir = temp_test_dir("agenthub-hook-local-title");
    let codex_home = dir.join(".codex");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    let transcript = codex_home.join("sessions").join("rollout-title.jsonl");
    fs::write(
        &transcript,
        json!({"type":"response_item","turn_id":"turn-title","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"text":"完整最终回复"}]}})
            .to_string(),
    )
    .unwrap();
    let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT,
            source TEXT,
            thread_source TEXT,
            updated_at INTEGER,
            rollout_path TEXT
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
        params![
            "thread-title",
            "真实 HookStop 标题",
            jiff::Timestamp::now().as_millisecond(),
            transcript.to_string_lossy().as_ref()
        ],
    )
    .unwrap();
    fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
    let mut config = test_config(codex_home);
    config.probe.notifications.enabled = false;

    let selection = hook_stop_last_assistant_message(
        Some(transcript.to_string_lossy().as_ref()),
        Some("turn-title"),
        None,
    )
    .unwrap()
    .unwrap();
    let event_input = ProbeEventInput::hook_stop_with_context(
        Some("thread-title"),
        Some("turn-title"),
        Some("thread-title"),
        Some(transcript.to_string_lossy().as_ref()),
        Some(&selection.message),
        "hook-stop",
    )
    .with_body_source(Some(&selection.source))
    .with_thread_title(
        local_thread_title(&config, "thread-title")
            .unwrap()
            .as_deref(),
    );
    let event = build_probe_event(&config, event_input);

    handle_built_probe_event(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();

    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events[0].title.as_deref(), Some("真实 HookStop 标题"));
    assert_eq!(events[0].payload["thread_title"], "真实 HookStop 标题");
    assert_eq!(
        events[0].payload["bark"]["title"],
        "线程正常完成：真实 HookStop 标题"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_stop_dedupe_skips_duplicate_bark_without_leaking_device_key() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-a"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();

    let first = handle_built_probe_event(
        &|| Ok(config.clone()),
        &db,
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-a"),
                Some("turn-1"),
                None,
                None,
                None,
                "hook-stop",
            ),
        ),
    )
    .await
    .unwrap();
    let duplicate = handle_built_probe_event(
        &|| Ok(config.clone()),
        &db,
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-a"),
                Some("turn-1"),
                None,
                None,
                None,
                "hook-stop",
            ),
        ),
    )
    .await
    .unwrap();

    assert_eq!(first.bark.reason.as_deref(), Some("connection"));
    assert_eq!(
        duplicate.stdout,
        json!({"continue": true, "suppressOutput": false})
    );
    assert!(!duplicate.outcome.recorded);
    assert!(!duplicate.bark.sent);
    assert!(duplicate.bark.skipped);
    assert_eq!(duplicate.bark.reason.as_deref(), Some("dedupe"));
    assert!(duplicate.bark.device_key_configured);
    assert!(duplicate.bark.dedupe_hit);
    assert!(!serde_json::to_string(&duplicate)
        .unwrap()
        .contains("super-secret-device"));

    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["bark"]["device_key_configured"], true);
    assert!(events[0].payload["bark"].get("device_key").is_none());
}

#[tokio::test]
async fn passive_reply_needed_scan_uses_sent_marker_while_hook_window_stays_short() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-passive", "thread-hook"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();

    let passive_input = || {
        ProbeEventInput::hook_stop_with_context(
            Some("thread-passive"),
            Some("turn-passive"),
            Some("thread-passive"),
            None,
            Some("等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-passive\nTurn ID：turn-passive\n时间：2026-06-16 12:00:00 北京时间\n状态说明：这一轮正在等待用户选择，不是异常停止。\n\n待选择内容：\n问题 1：继续吗？\n选项 1：继续"),
            "reply-needed",
        )
        .with_body_source(Some("request_user_input"))
        .with_passive_scan_source()
    };

    let first = record_probe_event_with_bark(
        &|| Ok(config.clone()),
        &db,
        build_probe_event(&config, passive_input()),
    )
    .await
    .unwrap();
    let duplicate = record_probe_event_with_bark(
        &|| Ok(config.clone()),
        &db,
        build_probe_event(&config, passive_input()),
    )
    .await
    .unwrap();

    assert!(first.0.recorded);
    assert!(!duplicate.0.recorded);
    assert_eq!(duplicate.1.reason.as_deref(), Some("sent_marker"));
    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["dedupe_ttl_seconds"], 6 * 60 * 60);
    assert_eq!(events[0].payload["scan_source"], "passive-scan");

    let hook = build_probe_event(&config, ProbeEventInput::hook_stop_with_context(
        Some("thread-hook"),
        Some("turn-hook"),
        Some("thread-hook"),
        None,
        Some("等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-hook\nTurn ID：turn-hook"),
        "reply-needed",
    ));

    assert_eq!(
        hook.ttl_seconds,
        agenthub_core::probe::PROBE_EVENT_TTL_SECONDS
    );
    assert_eq!(
        hook.payload["dedupe_ttl_seconds"],
        agenthub_core::probe::PROBE_EVENT_TTL_SECONDS
    );
    assert_eq!(hook.payload["scan_source"], "hook-stop");
}

#[tokio::test]
async fn notify_completion_uses_completion_bark_switch() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-a"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_completion = false;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let event = build_probe_event(
        &config,
        ProbeEventInput::notify_completion(Some("thread-a"), Some("turn-a")),
    );

    let (_outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();

    assert!(bark.skipped);
    assert_eq!(bark.reason.as_deref(), Some("event_switch_disabled"));
    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events[0].kind, "completion");
    assert_eq!(events[0].payload["bark"]["relevant_switch_enabled"], false);
}

#[tokio::test]
async fn notify_completion_uses_complete_last_agent_message_from_rollout_without_storing_body() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-complete"]);
    config.probe.notifications.enabled = false;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let dir = temp_test_dir("agenthub-notify-completion-rollout");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    let final_message = format!(
        "最终反馈第一行\n{}\nAuthorization: Bearer secret-token\n末尾唯一完整反馈",
        "完整正文".repeat(900)
    );
    fs::write(
        &transcript,
        [
            json!({"type":"response_item","turn_id":"turn-complete","payload":{"type":"message","role":"assistant","content":[{"text":"short assistant fallback"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-complete","last_agent_message":final_message}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let context = notify_completion_context(
        &config,
        Some(&json!({
            "thread_id": "thread-complete",
            "turn_id": "turn-complete",
            "transcript_path": transcript,
        })),
        None,
        None,
    )
    .unwrap();
    let event = build_probe_event(&config, context);
    let (_outcome, _bark) =
        record_probe_event_with_bark(&|| Ok(config.clone()), &db, event.clone())
            .await
            .unwrap();

    assert_eq!(event.kind, "completion");
    assert!(event.bark_body.contains("最终反馈第一行"));
    assert!(event.bark_body.contains("完整正文完整正文"));
    assert!(event.bark_body.contains("末尾唯一完整反馈"));
    assert!(!event.bark_body.contains("secret-token"));
    assert!(!event.bark_body.contains("[truncated]"));
    assert!(!event.bark_body.contains("最后反馈："));
    assert!(event.payload["bark"].get("body").is_none());
    assert_eq!(
        event.payload["body_source"],
        "task_complete.last_agent_message"
    );
    let stored = db.list_probe_events(10).unwrap();
    let stored_json = serde_json::to_string(&stored[0]).unwrap();
    assert!(!stored_json.contains("末尾唯一完整反馈"));
    assert!(!stored_json.contains("secret-token"));
    assert_eq!(
        stored[0].payload["body_summary"],
        event.payload["body_summary"]
    );
    assert_eq!(
        stored[0].payload["body_sha256"],
        event.payload["body_sha256"]
    );
    assert!(stored[0].payload["bark"].get("body").is_none());

    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn notify_completion_prefers_transcript_final_message_over_stale_stdin_message() {
    let (config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-complete"]);
    let dir = temp_test_dir("agenthub-notify-completion-stale-stdin");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    fs::write(
        &transcript,
        [
            json!({"type":"response_item","turn_id":"turn-complete","payload":{"type":"message","role":"assistant","content":[{"text":"上一条 assistant fallback"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-complete","last_agent_message":"最后一条 final message"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let context = notify_completion_context(
        &config,
        Some(&json!({
            "thread_id": "thread-complete",
            "turn_id": "turn-complete",
            "transcript_path": transcript,
            "last_assistant_message": "上一条旧消息"
        })),
        None,
        None,
    )
    .unwrap();
    let event = build_probe_event(&config, context);

    assert!(event.bark_body.contains("最后一条 final message"));
    assert!(!event.bark_body.contains("上一条旧消息"));
    assert_eq!(
        event.payload["body_source"],
        "task_complete.last_agent_message"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn notify_completion_waits_for_later_final_message_and_records_selection_diagnostics() {
    let (config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-live"]);
    let dir = temp_test_dir("agenthub-notify-completion-waits-final");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    fs::write(
        &transcript,
        [
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"上一轮完成正文"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}).to_string(),
            json!({"type":"turn_context","payload":{"turn_id":"turn-live","summary":"auto","cwd":"/tmp"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let transcript_for_writer = transcript.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        let final_message = "已接着完成。\n最终完成正文唯一标记";
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&transcript_for_writer)
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":final_message}],"phase":"final_answer"}})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-live","last_agent_message":final_message}})
        )
        .unwrap();
    });

    let context = notify_completion_context(
        &config,
        Some(&json!({
            "thread_id": "thread-live",
            "turn_id": "turn-live",
            "transcript_path": transcript,
            "last_assistant_message": "过早 stdin 正文"
        })),
        None,
        None,
    )
    .unwrap();
    writer.join().unwrap();
    let event = build_probe_event(&config, context);

    assert!(event.bark_body.contains("最终完成正文唯一标记"));
    assert!(!event.bark_body.contains("过早 stdin 正文"));
    assert!(!event.bark_body.contains("上一轮完成正文"));
    assert_eq!(
        event.payload["body_source"],
        "task_complete.last_agent_message"
    );
    assert_eq!(event.payload["body_selected_turn_id"], "turn-live");
    assert!(event.payload["body_selected_line"].as_u64().unwrap() >= 4);
    assert_eq!(event.payload["transcript_stabilized"], true);
    assert!(
        event.payload["transcript_size_after"].as_u64().unwrap()
            > event.payload["transcript_size_before"].as_u64().unwrap()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn notify_completion_uses_full_raw_assistant_message_when_task_complete_body_is_missing() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-complete"]);
    config.probe.notifications.enabled = false;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let dir = temp_test_dir("agenthub-notify-completion-raw-assistant");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    let final_message = format!(
        "助手完整回复开头\n{}\n助手完整回复末尾唯一标记",
        "没有完成字段也不能截断".repeat(900)
    );
    fs::write(
        &transcript,
        [
            json!({"type":"response_item","turn_id":"turn-complete","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"text":final_message}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-complete","last_agent_message":null}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let context = notify_completion_context(
        &config,
        Some(&json!({
            "thread_id": "thread-complete",
            "turn_id": "turn-complete",
            "transcript_path": transcript,
        })),
        None,
        None,
    )
    .unwrap();
    let event = build_probe_event(&config, context);
    let (_outcome, _bark) =
        record_probe_event_with_bark(&|| Ok(config.clone()), &db, event.clone())
            .await
            .unwrap();

    assert_eq!(event.kind, "completion");
    assert_eq!(event.payload["body_source"], "last_assistant_message");
    assert!(event.bark_body.contains("助手完整回复开头"));
    assert!(event.bark_body.contains("助手完整回复末尾唯一标记"));
    assert!(!event.bark_body.contains("[truncated]"));
    assert!(event.payload["body_length"].as_u64().unwrap() > 4000);
    let stored = db.list_probe_events(10).unwrap();
    assert!(!serde_json::to_string(&stored[0])
        .unwrap()
        .contains("助手完整回复末尾唯一标记"));

    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_stop_uses_cli_turn_id_when_payload_turn_id_is_missing() {
    let (config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-cli"]);
    let dir = temp_test_dir("agenthub-hook-cli-turn-id");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    fs::write(
        &transcript,
        [
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"上一轮完成正文"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-cli"}}).to_string(),
            json!({"type":"turn_context","payload":{"turn_id":"turn-cli","summary":"auto","cwd":"/tmp"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-cli","last_agent_message":"CLI turn 最终正文"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let event_input = hook_stop_event_input(
        &config,
        Some(&json!({
            "thread_id": "thread-cli",
            "transcript_path": transcript,
            "last_assistant_message": "payload 早期正文"
        })),
        Some("thread-cli"),
        Some("turn-cli"),
        "hook-stop",
    )
    .unwrap();
    let event = build_probe_event(&config, event_input);

    assert!(event.bark_body.contains("CLI turn 最终正文"));
    assert!(!event.bark_body.contains("上一轮完成正文"));
    assert!(!event.bark_body.contains("payload 早期正文"));
    assert_eq!(event.payload["turn_id"], "turn-cli");
    assert_eq!(event.payload["body_selected_turn_id"], "turn-cli");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn notify_completion_context_keeps_unresolved_plan_as_reply_needed() {
    let (config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-plan"]);
    let dir = temp_test_dir("agenthub-notify-completion-plan-pending");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    fs::write(
        &transcript,
        [
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 待确认计划\n- 检查\n</proposed_plan>"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":"计划等待确认。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let context = notify_completion_context(
        &config,
        Some(&json!({
            "thread_id": "thread-plan",
            "turn_id": "turn-plan",
            "transcript_path": transcript,
        })),
        None,
        None,
    )
    .unwrap();
    let event = build_probe_event(&config, context);

    assert_eq!(event.kind, "reply-needed");
    assert_eq!(event.event_type, "reply_needed");
    assert_eq!(event.payload["body_source"], "proposed_plan");
    assert_eq!(event.bark_body, "# 待确认计划\n- 检查");

    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn notify_completion_context_keeps_unresolved_question_as_reply_needed() {
    let (config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-choice"]);
    let dir = temp_test_dir("agenthub-notify-completion-question-pending");
    fs::create_dir_all(&dir).unwrap();
    let transcript = dir.join("rollout.jsonl");
    fs::write(
        &transcript,
        [
            json!({"type":"RequestUserInput","turn_id":"turn-choice","item_id":"choice-1","questions":[{"id":"choice","question":"继续吗？","options":[{"label":"继续"},{"label":"停止"}]}]}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-choice","last_agent_message":"问题等待选择。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let context = notify_completion_context(
        &config,
        Some(&json!({
            "thread_id": "thread-choice",
            "turn_id": "turn-choice",
            "transcript_path": transcript,
        })),
        None,
        None,
    )
    .unwrap();
    let event = build_probe_event(&config, context);

    assert_eq!(event.kind, "reply-needed");
    assert_eq!(event.event_type, "reply_needed");
    assert_eq!(event.payload["body_source"], "request_user_input");
    assert!(event.bark_body.contains("问题 1：继续吗？"));
    assert!(event.bark_body.contains("选项 1：继续"));
    assert!(event.bark_body.contains("选项 2：停止"));

    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hook_stop_prefers_full_rollout_plan_over_short_stdin_summary() {
    let dir = temp_test_dir("agenthub-hook-stop-full-plan");
    fs::create_dir_all(&dir).unwrap();
    let mut config = test_config(dir.join(".codex"));
    config.probe.notifications.enabled = false;
    let transcript = dir.join("rollout.jsonl");
    let full_plan = format!("# 完整计划\n{}\n末尾唯一完整计划", "计划正文".repeat(1200));
    fs::write(
        &transcript,
        [
            json!({"type":"item_completed","thread_id":"thread-plan","turn_id":"turn-plan","item":{"type":"Plan","id":"plan-1","text":full_plan}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n短摘要\n</proposed_plan>"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":null}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let selection = hook_stop_last_assistant_message(
        transcript.to_str(),
        Some("turn-plan"),
        Some("<proposed_plan>\n短摘要\n</proposed_plan>"),
    )
    .unwrap()
    .unwrap();
    let event = build_probe_event(
        &config,
        ProbeEventInput::hook_stop_with_context(
            Some("thread-plan"),
            Some("turn-plan"),
            Some("thread-plan"),
            transcript.to_str(),
            Some(&selection.message),
            "hook-stop",
        )
        .with_body_source(Some(&selection.source)),
    );

    assert_eq!(event.kind, "reply-needed");
    assert_eq!(event.event_type, "reply_needed");
    assert!(event.bark_body.contains("# 完整计划"));
    assert!(event.bark_body.contains("末尾唯一完整计划"));
    assert!(!event.bark_body.contains("短摘要"));
    assert!(!event.bark_body.contains("[truncated]"));
    assert!(event.payload["body_length"].as_u64().unwrap() > 4000);
    assert_eq!(event.payload["body_source"], "proposed_plan");

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn passive_reply_needed_body_formats_questions_and_extracts_plan_text() {
    let thread = agenthub_core::codex::ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: "thread-question".to_string(),
        title: "问题线程".to_string(),
        status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
        updated_at: None,
        archived_at: None,
        message_count: 1,
        latest_message: Some("fallback should not be used".to_string()),
        cwd: None,
        workspace_root: None,
        model: None,
        rollout_path: None,
        active_turn_id: Some("turn-question".to_string()),
        active_job_id: None,
        pending_elicitation: Some(agenthub_core::codex::PendingElicitation {
            turn_id: Some("turn-question".to_string()),
            item_id: Some("call-question".to_string()),
            questions: vec![agenthub_core::codex::UserInputQuestion {
                id: "q1".to_string(),
                header: Some("Mode".to_string()),
                question: "怎么继续？".to_string(),
                options: vec![
                    agenthub_core::codex::UserInputOption {
                        label: "直接执行".to_string(),
                        description: Some("按当前计划继续".to_string()),
                    },
                    agenthub_core::codex::UserInputOption {
                        label: "先调整".to_string(),
                        description: Some("补充约束后再执行".to_string()),
                    },
                ],
            }],
        }),
        last_event_kind: None,
    };

    let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
    let body = plan.body;
    let source = plan.body_source;

    let body = body.expect("request_user_input body");
    assert_eq!(source.as_deref(), Some("request_user_input"));
    assert!(!body.contains("等待用户选择"));
    assert!(!body.contains("Call ID："));
    assert!(!body.contains("Turn ID："));
    assert!(!body.contains("待选择内容"));
    assert!(body.contains("问题 1：怎么继续？"));
    assert!(body.contains("选项 1：直接执行"));
    assert!(body.contains("说明：按当前计划继续"));
    assert!(body.contains("选项 2：先调整"));
    assert!(!body.contains("fallback should not be used"));

    let (sandbox_cfg, sandbox_root) = crate::api::test_support::sandboxed_test_config();
    let event = build_probe_event(
        &sandbox_cfg,
        ProbeEventInput::hook_stop_with_context(
            Some("thread-question"),
            Some("turn-question"),
            Some("thread-question"),
            None,
            Some(&body),
            "reply-needed",
        )
        .with_thread_title(Some("问题线程"))
        .with_body_source(source.as_deref()),
    );
    assert_eq!(event.bark_title, "等待回复：问题线程");
    let _ = fs::remove_dir_all(sandbox_root);
    assert_eq!(event.bark_body, body);
    assert!(!event.bark_body.contains("thread-question"));

    let plan_thread = agenthub_core::codex::ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        pending_elicitation: None,
        latest_message: Some(
            "<proposed_plan>\n# 修复计划\n- 等待确认\n</proposed_plan>".to_string(),
        ),
        ..thread
    };

    let plan = probe_service::probe_passive_thread_notification_plan(&plan_thread, "reply-needed");
    let body = plan.body;
    let source = plan.body_source;

    let body = body.expect("plan body");
    assert_eq!(source.as_deref(), Some("proposed_plan"));
    assert_eq!(body, "# 修复计划\n- 等待确认");
    assert!(!body.contains("<proposed_plan>"));
    assert!(!body.contains("</proposed_plan>"));
}

#[test]
fn passive_reply_needed_body_suppresses_plan_after_later_reply_or_completion() {
    let dir = temp_test_dir("agenthub-plan-suppressed-after-reply");
    fs::create_dir_all(&dir).unwrap();
    let rollout = dir.join("rollout-plan-replied.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"批准，继续。"}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"正在执行计划。"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-work","last_agent_message":"已完成。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let thread = agenthub_core::codex::ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: "thread-plan-replied".to_string(),
        title: "计划已回复线程".to_string(),
        status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
        updated_at: None,
        archived_at: None,
        message_count: 4,
        latest_message: Some("<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>".to_string()),
        cwd: None,
        workspace_root: None,
        model: None,
        rollout_path: Some(rollout),
        active_turn_id: Some("turn-plan".to_string()),
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: Some("task_complete".to_string()),
    };

    let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
    let body = plan.body;
    let source = plan.body_source;

    assert_eq!(body, None);
    assert_eq!(source, None);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn passive_reply_needed_scan_rejects_old_reply_needed_threads() {
    let thread = agenthub_core::codex::ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: "thread-old-plan".to_string(),
        title: "旧计划线程".to_string(),
        status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
        updated_at: Some(
            (jiff::Timestamp::now()
                - jiff::SignedDuration::from_secs(
                    probe_service::PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS + 60,
                ))
            .to_string(),
        ),
        archived_at: None,
        message_count: 1,
        latest_message: Some("<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>".to_string()),
        cwd: None,
        workspace_root: None,
        model: None,
        rollout_path: None,
        active_turn_id: Some("turn-old".to_string()),
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: Some("task_complete".to_string()),
    };

    let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
    assert!(!plan.fresh);
}

#[tokio::test]
async fn passive_reply_needed_plan_dedupe_key_changes_with_plan_hash_and_no_ttl_resend() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-plan"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let dir = temp_test_dir("agenthub-plan-ttl-marker");
    fs::create_dir_all(&dir).unwrap();
    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();

    let body_one = agenthub_core::codex::extract_proposed_plan_text(
        "<proposed_plan>\n# 第一版\n- A\n</proposed_plan>",
    )
    .unwrap();
    let body_two = agenthub_core::codex::extract_proposed_plan_text(
        "<proposed_plan>\n# 第二版\n- B\n</proposed_plan>",
    )
    .unwrap();
    let make_event = |body: &str| {
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-plan"),
                Some("turn-plan"),
                Some("thread-plan"),
                None,
                Some(body),
                "reply-needed",
            )
            .with_thread_title(Some("真实计划线程"))
            .with_body_source(Some("proposed_plan"))
            .with_passive_scan_source(),
        )
    };

    let first_event = make_event(&body_one);
    let first_key = first_event.dedupe_key.clone();
    let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, first_event)
        .await
        .unwrap();
    let duplicate =
        record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event(&body_one))
            .await
            .unwrap();
    let changed_event = make_event(&body_two);
    let changed_key = changed_event.dedupe_key.clone();
    let changed = record_probe_event_with_bark(&|| Ok(config.clone()), &db, changed_event)
        .await
        .unwrap();

    assert!(first.0.recorded);
    assert_eq!(duplicate.1.reason.as_deref(), Some("sent_marker"));
    assert!(!duplicate.0.recorded);
    assert!(changed.0.recorded);
    assert_ne!(first_key, changed_key);
    assert!(first_key.contains("thread-plan"));
    assert!(first_key.contains("turn-plan"));
    assert_ne!(
        first_key,
        "reply-needed:thread-plan:turn-plan:reply_needed:turn:turn-plan"
    );
    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn passive_reply_needed_plan_does_not_resend_after_ttl_expires() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-plan-ttl"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let dir = temp_test_dir("agenthub-plan-ttl-marker-single");
    fs::create_dir_all(&dir).unwrap();
    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let body = agenthub_core::codex::extract_proposed_plan_text(
        "<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>",
    )
    .unwrap();
    let make_event = || {
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-plan-ttl"),
                Some("turn-plan-ttl"),
                Some("thread-plan-ttl"),
                None,
                Some(&body),
                "reply-needed",
            )
            .with_thread_title(Some("旧计划 TTL 线程"))
            .with_body_source(Some("proposed_plan"))
            .with_passive_scan_source(),
        )
    };

    let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
        .await
        .unwrap();
    Connection::open(db.path())
        .unwrap()
        .execute("DELETE FROM probe_dedupe", [])
        .unwrap();
    let after_ttl = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
        .await
        .unwrap();

    assert!(first.0.recorded);
    assert_eq!(after_ttl.1.reason.as_deref(), Some("sent_marker"));
    assert!(!after_ttl.0.recorded);
    assert!(!after_ttl.1.sent);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn passive_request_user_input_event_does_not_resend_after_ttl_expires() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-question-ttl"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let dir = temp_test_dir("agenthub-input-ttl-marker");
    fs::create_dir_all(&dir).unwrap();
    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let body = "等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-question\nTurn ID：turn-question\n时间：2026-06-16 12:00:00 北京时间\n状态说明：这一轮正在等待用户选择，不是异常停止。\n\n待选择内容：\n问题 1：Continue?\n选项 1：继续";
    let make_event = || {
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-question-ttl"),
                Some("turn-question"),
                Some("thread-question-ttl"),
                None,
                Some(body),
                "reply-needed",
            )
            .with_thread_title(Some("问题 TTL 线程"))
            .with_body_source(Some("request_user_input"))
            .with_passive_scan_source(),
        )
    };

    let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
        .await
        .unwrap();
    Connection::open(db.path())
        .unwrap()
        .execute("DELETE FROM probe_dedupe", [])
        .unwrap();
    let after_ttl = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
        .await
        .unwrap();

    assert!(first.0.recorded);
    assert_eq!(after_ttl.1.reason.as_deref(), Some("sent_marker"));
    assert!(!after_ttl.0.recorded);
    assert!(!after_ttl.1.sent);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn pre_tool_use_request_user_input_event_does_not_resend_after_ttl_expires() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-pre-tool-question"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let dir = temp_test_dir("agenthub-pre-tool-use-input-marker");
    fs::create_dir_all(&dir).unwrap();
    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let body = "问题 1：Continue?\n选项 1：继续\n选项 2：停止";
    let make_event = || {
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-pre-tool-question"),
                Some("turn-pre-tool-question"),
                Some("thread-pre-tool-question"),
                None,
                Some(body),
                "reply-needed",
            )
            .with_thread_title(Some("PreToolUse 问题"))
            .with_body_source(Some("request_user_input"))
            .with_pre_tool_use_scan_source()
            .with_call_id(Some("call-pre-tool-question")),
        )
    };

    let first = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
        .await
        .unwrap();
    Connection::open(db.path())
        .unwrap()
        .execute("DELETE FROM probe_dedupe", [])
        .unwrap();
    let after_ttl = record_probe_event_with_bark(&|| Ok(config.clone()), &db, make_event())
        .await
        .unwrap();

    assert!(first.0.recorded);
    assert_eq!(after_ttl.1.reason.as_deref(), Some("sent_marker"));
    assert!(!after_ttl.0.recorded);
    assert!(!after_ttl.1.sent);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn passive_request_user_input_marker_ignores_scan_time_changes() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-question-time"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let make_body = |time: &str| {
        format!(
            "等待用户选择：Plan Mode 已请求用户选择后继续。\n\nCall ID：call-question\nTurn ID：turn-question\n时间：{time}\n状态说明：这一轮正在等待用户选择，不是异常停止。\n\n待选择内容：\n问题 1：Continue?\n选项 1：继续"
        )
    };
    let make_event = |body: String| {
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-question-time"),
                Some("turn-question"),
                Some("thread-question-time"),
                None,
                Some(&body),
                "reply-needed",
            )
            .with_thread_title(Some("问题时间线程"))
            .with_body_source(Some("request_user_input"))
            .with_passive_scan_source(),
        )
    };

    let first = record_probe_event_with_bark(
        &|| Ok(config.clone()),
        &db,
        make_event(make_body("2026-06-16 12:00:00 北京时间")),
    )
    .await
    .unwrap();
    let second = record_probe_event_with_bark(
        &|| Ok(config.clone()),
        &db,
        make_event(make_body("2026-06-16 12:05:00 北京时间")),
    )
    .await
    .unwrap();

    assert!(first.0.recorded);
    assert_eq!(second.1.reason.as_deref(), Some("sent_marker"));
    assert!(!second.0.recorded);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
}

#[tokio::test]
async fn send_bark_posts_lite_payload_and_reports_non_success_response_code() {
    let server = TestHttpServer::start_n(
        1,
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 39\r\n\r\n{\"code\":400,\"message\":\"bad bark token\"}",
    );
    let (mut config, sandbox_root) = crate::api::test_support::sandboxed_test_config();
    config.probe.notifications.server_url = server.url();
    config.probe.notifications.group = "Probe Group".to_string();
    config.probe.notifications.sound = Some("bell".to_string());
    config.probe.notifications.url = Some("https://panel.example.com/agenthub/".to_string());

    let request = ProbeBarkRequest {
        url: None,
        presentation: None,
        title: "AgentHub 推送测试".to_string(),
        body: "来自本机的测试通知。".to_string(),
        dedupe_key: "hook-stop:thread-a:turn-1".to_string(),
    };
    let result = bark::send_chunk(
        &config,
        b"device key/with spaces",
        &request,
        &request.body,
        0,
        1,
        "fixture-request",
        1,
        std::time::Duration::from_secs(2),
    )
    .await;

    assert!(!result.sent);
    assert!(!result.skipped);
    assert_eq!(result.reason.as_deref(), Some("bark_response_code"));
    assert_eq!(result.http_status, Some(200));
    assert!(result.device_key_configured);
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("device key"));
    let raw = server.request();
    assert!(raw.starts_with("POST /push "));
    let body = raw.split("\r\n\r\n").nth(1).unwrap();
    let payload: Value = serde_json::from_str(body).unwrap();
    assert_eq!(payload["device_key"], "device key/with spaces");
    assert_eq!(payload["title"], "AgentHub 推送测试");
    assert_eq!(payload["body"], "来自本机的测试通知。");
    assert_eq!(payload.as_object().unwrap().len(), 4);
    assert!(payload["id"].is_string());
    assert!(payload.get("group").is_none());
    assert!(payload.get("sound").is_none());
    assert!(payload.get("url").is_none());
    let _ = fs::remove_dir_all(sandbox_root);
}

#[tokio::test]
async fn bark_delivery_sends_one_bounded_summary_without_ids_or_chunks() {
    let server = TestHttpServer::start_n(
        20,
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
    );
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-long"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_completion = true;
    config.probe.notifications.server_url = server.url();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"device-key")
        .unwrap();
    let full_body = format!("开头\n{}\n末尾唯一完整反馈", "完整正文".repeat(900));
    let event = build_probe_event(
        &config,
        ProbeEventInput::notify_completion_with_context(
            Some("thread-long"),
            Some("turn-long"),
            Some("thread-long"),
            None,
            Some(&full_body),
            Some("task_complete.last_agent_message"),
        )
        .with_thread_title(Some("真实长正文线程")),
    );
    assert_eq!(event.bark_title, "线程正常完成：真实长正文线程");
    assert!(event.payload["body_truncated"].as_bool().unwrap());
    assert!(!event.payload["body_summary"]
        .as_str()
        .unwrap()
        .contains("末尾唯一完整反馈"));

    let (_outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();

    assert!(bark.sent);
    let requests = server.requests(bark.request_count);
    assert_eq!(requests.len(), bark.request_count);
    assert_eq!(bark.request_count, 1);
    assert_eq!(bark.chunk_count, 1);
    let payload: Value =
        serde_json::from_str(requests[0].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(payload["title"], "Codex｜完成｜真实长正文线程");
    let summary = payload["body"].as_str().unwrap();
    assert!(summary.starts_with("开头\n"));
    assert!(summary.ends_with('…'));
    assert_eq!(summary.chars().count(), 300);
    assert!(!summary.contains("末尾唯一完整反馈"));
    assert_eq!(payload["group"], "AgentHub｜Codex");
    assert_eq!(payload["level"], "active");
    let stored = db.list_probe_events(10).unwrap();
    let stored_json = serde_json::to_string(&stored[0]).unwrap();
    assert!(!stored_json.contains("末尾唯一完整反馈"));
    assert!(stored[0].payload["bark"].get("body").is_none());
}

#[tokio::test]
async fn record_probe_event_stores_body_metadata_but_not_full_bark_body_or_tokens() {
    let server = TestHttpServer::start_n(
        20,
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
    );
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-safe-store"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_completion = true;
    config.probe.notifications.server_url = server.url();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"secret-device-token")
        .unwrap();
    let full_body = format!(
        "开头\nAuthorization: Bearer secret-token\n{}\n<proposed_plan>不要存这个标签</proposed_plan>\n末尾唯一完整反馈",
        "完整正文".repeat(900)
    );
    let event = build_probe_event(
        &config,
        ProbeEventInput::notify_completion_with_context(
            Some("thread-safe-store"),
            Some("turn-safe-store"),
            Some("thread-safe-store"),
            None,
            Some(&full_body),
            Some("task_complete.last_agent_message"),
        )
        .with_thread_title(Some("安全存储线程")),
    );

    let (_outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();
    assert!(bark.sent);
    let _requests = server.requests(bark.request_count);

    let stored = db.list_probe_events(10).unwrap();
    assert_eq!(stored.len(), 1);
    let payload = &stored[0].payload;
    assert!(payload["body_summary"].as_str().unwrap().contains("开头"));
    assert!(payload["body_sha256"]
        .as_str()
        .is_some_and(|value| value.len() == 64));
    assert_eq!(payload["body_length"], full_body.len() as u64);
    assert_eq!(payload["bark"]["body_length"], full_body.len() as u64);
    assert_eq!(payload["bark"]["body_sha256"], payload["body_sha256"]);
    assert!(payload["bark"]["chunk_count"].as_u64().is_some());
    assert!(payload["bark"]["request_count"].as_u64().is_some());
    assert!(payload["bark"].get("body").is_none());
    assert!(payload.get("bark_body").is_none() || payload["bark_body"].is_null());
    let stored_json = serde_json::to_string(&stored[0]).unwrap();
    assert!(!stored_json.contains("末尾唯一完整反馈"));
    assert!(!stored_json.contains("secret-token"));
    assert!(!stored_json.contains("secret-device-token"));
    assert!(!stored_json.contains("<proposed_plan>"));
    assert!(!stored_json.contains("</proposed_plan>"));
    assert!(!stored_json.contains("/push"));
}

#[tokio::test]
async fn passive_reply_needed_plan_dedupe_key_includes_thread_turn_item_and_plan_hash() {
    let server = TestHttpServer::start_n(
        1,
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
    );
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-plan-stable"]);
    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = server.url();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();
    let raw_plan = "<proposed_plan>\n# 稳定计划\n- A\n</proposed_plan>";
    let body = agenthub_core::codex::extract_proposed_plan_text(raw_plan).unwrap();
    let event = build_probe_event(
        &config,
        ProbeEventInput::hook_stop_with_context(
            Some("thread-plan-stable"),
            Some("turn-plan-stable"),
            Some("thread-plan-stable"),
            None,
            Some(&body),
            "reply-needed",
        )
        .with_thread_title(Some("稳定计划线程"))
        .with_body_source(Some("proposed_plan"))
        .with_passive_scan_source(),
    );
    let mut event = event;
    event.payload["item_id"] = json!("item-plan-stable");

    let (outcome, bark) = record_probe_event_with_bark(&|| Ok(config.clone()), &db, event)
        .await
        .unwrap();
    assert!(bark.sent);
    let _raw = server.request();
    let dedupe_key = outcome.dedupe_key;

    assert!(dedupe_key.contains("thread-plan-stable"));
    assert!(dedupe_key.contains("turn-plan-stable"));
    assert!(dedupe_key.contains("item-plan-stable"));
    assert!(dedupe_key.contains("plan_hash"));
    assert_ne!(
        dedupe_key,
        "reply-needed:thread-plan-stable:turn-plan-stable:reply_needed:turn:turn-plan-stable"
    );
    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn passive_reply_needed_body_suppresses_plan_after_tool_output_assistant_or_turn_completed() {
    for (name, tail) in [
        (
            "tool-output",
            vec![
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"function_call_output","call_id":"call-plan","output":"完成选择"}}),
            ],
        ),
        (
            "assistant-progress",
            vec![
                json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"继续执行计划。"}]}}),
            ],
        ),
        (
            "turn-completed",
            vec![json!({"type":"turn_completed","turn_id":"turn-plan"})],
        ),
    ] {
        let dir = temp_test_dir(&format!("agenthub-plan-suppressed-{name}"));
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout.jsonl");
        let mut lines = vec![
            json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>"}]}}),
        ];
        lines.extend(tail);
        fs::write(
            &rollout,
            lines
                .into_iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let thread = agenthub_core::codex::ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: format!("thread-{name}"),
            title: "计划应被抑制线程".to_string(),
            status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
            updated_at: None,
            archived_at: None,
            message_count: 2,
            latest_message: Some(
                "<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>".to_string(),
            ),
            cwd: None,
            workspace_root: None,
            model: None,
            rollout_path: Some(rollout),
            active_turn_id: Some("turn-plan".to_string()),
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: None,
        };

        let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
        let body = plan.body;
        let source = plan.body_source;

        assert_eq!(body, None, "{name}");
        assert_eq!(source, None, "{name}");
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn passive_reply_needed_fallback_suppresses_old_plan_after_later_completion() {
    let dir = temp_test_dir("agenthub-fallback-old-plan-completed");
    fs::create_dir_all(&dir).unwrap();
    let rollout = dir.join("rollout.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧 fallback 计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"继续"}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-work","payload":{"type":"message","role":"assistant","content":[{"text":"执行完成。"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-work","last_agent_message":"执行完成。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let thread = agenthub_core::codex::ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: "thread-fallback-old-plan".to_string(),
        title: "fallback 旧计划线程".to_string(),
        status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
        updated_at: None,
        archived_at: None,
        message_count: 4,
        latest_message: None,
        cwd: None,
        workspace_root: None,
        model: None,
        rollout_path: Some(rollout),
        active_turn_id: None,
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: Some("task_complete".to_string()),
    };

    let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
    let body = plan.body;
    let source = plan.body_source;

    assert_eq!(body, None);
    assert_eq!(source, None);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn passive_reply_needed_fallback_suppresses_old_plan_when_completion_has_no_body() {
    let dir = temp_test_dir("agenthub-fallback-old-plan-empty-complete");
    fs::create_dir_all(&dir).unwrap();
    let rollout = dir.join("rollout.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧 fallback 计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"继续"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-plan","last_agent_message":null}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    assert_eq!(
        agenthub_core::codex::rollout_completion_last_agent_message(&rollout, Some("turn-plan"),)
            .unwrap(),
        None
    );
    let thread = agenthub_core::codex::ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: "thread-fallback-empty-complete".to_string(),
        title: "fallback 空完成线程".to_string(),
        status: agenthub_core::codex::ThreadStatus::ReplyNeeded,
        updated_at: None,
        archived_at: None,
        message_count: 3,
        latest_message: None,
        cwd: None,
        workspace_root: None,
        model: None,
        rollout_path: Some(rollout),
        active_turn_id: Some("turn-plan".to_string()),
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: Some("task_complete".to_string()),
    };

    let plan = probe_service::probe_passive_thread_notification_plan(&thread, "reply-needed");
    let body = plan.body;
    let source = plan.body_source;

    assert_eq!(body, None);
    assert_eq!(source, None);
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn probe_thread_scan_does_not_resend_completed_old_plan_when_app_server_is_offline() {
    let dir = temp_test_dir("agenthub-thread-scan-old-plan-offline");
    let codex_home = dir.join(".codex");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    let rollout = codex_home.join("sessions").join("rollout-old-plan.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"response_item","turn_id":"turn-plan","item_id":"item-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 旧计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"user","content":[{"text":"同意，继续。"}]}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-work","last_agent_message":"已完成。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT,
            source TEXT,
            thread_source TEXT,
            updated_at INTEGER,
            archived_at INTEGER,
            rollout_path TEXT
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
        params![
            "thread-old-plan",
            "真实旧计划线程",
            jiff::Timestamp::now().as_millisecond(),
            rollout.to_string_lossy().as_ref()
        ],
    )
    .unwrap();
    fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
    let mut config = test_config(codex_home.clone());

    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let state = passive_test_state(config, db.clone(), &dir);

    let count = run_probe_thread_scan_if_due(state).await.unwrap();

    assert_eq!(count, 0);
    assert!(db.list_probe_events(10).unwrap().is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn passive_request_user_input_does_not_resend_after_ttl_or_revive_after_answer() {
    let dir = temp_test_dir("agenthub-request-user-input-no-revive");
    let codex_home = dir.join(".codex");
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    let rollout = codex_home.join("sessions").join("rollout-question.jsonl");
    fs::write(
        &rollout,
        json!({
            "type": "response_item",
            "turn_id": "turn-question",
            "payload": {
                "type": "function_call",
                "name": "request_user_input",
                "status": "pending",
                "call_id": "call-question",
                "questions": [{
                    "id": "choice",
                    "header": "Mode",
                    "question": "Continue?",
                    "options": [
                        {"label": "继续", "description": "按计划执行"},
                        {"label": "停止"}
                    ]
                }]
            }
        })
        .to_string(),
    )
    .unwrap();
    let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT,
            source TEXT,
            thread_source TEXT,
            updated_at INTEGER,
            archived_at INTEGER,
            rollout_path TEXT
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
        params![
            "thread-question-no-repeat",
            "待选择线程",
            jiff::Timestamp::now().as_millisecond(),
            rollout.to_string_lossy().as_ref()
        ],
    )
    .unwrap();
    fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
    let mut config = test_config(codex_home.clone());

    config.probe.notifications.enabled = true;
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"super-secret-device")
        .unwrap();

    let first_count =
        run_probe_thread_scan_if_due(passive_test_state(config.clone(), db.clone(), &dir))
            .await
            .unwrap();
    db.maintain_probe_events(1, 100).unwrap();
    let second_count =
        run_probe_thread_scan_if_due(passive_test_state(config.clone(), db.clone(), &dir))
            .await
            .unwrap();
    fs::write(
        &rollout,
        [
            json!({"type":"response_item","turn_id":"turn-question","payload":{"type":"function_call","name":"request_user_input","status":"pending","call_id":"call-question","questions":[{"id":"choice","header":"Mode","question":"Continue?","options":[{"label":"继续","description":"按计划执行"},{"label":"停止"}]}]}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-question","payload":{"type":"UserInputAnswer","call_id":"call-question","answers":{"choice":["继续"]}}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-question","last_agent_message":"已继续。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let answered_count = run_probe_thread_scan_if_due(passive_test_state(config, db.clone(), &dir))
        .await
        .unwrap();

    assert_eq!(first_count, 1);
    assert_eq!(second_count, 0);
    assert_eq!(answered_count, 0);
    assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn send_bark_success_reports_redacted_request_metadata() {
    let server = TestHttpServer::start_n(
        1,
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
    );
    let (mut config, sandbox_root) = crate::api::test_support::sandboxed_test_config();
    config.probe.notifications.enabled = true;
    config.probe.notifications.server_url = server.url();
    config.probe.notifications.group = "Probe Group".to_string();

    let result = bark::send_chunk(
        &config,
        b"device-key-secret",
        &ProbeBarkRequest {
            url: None,
            presentation: None,
            title: "AgentHub 推送测试".to_string(),
            body: "ok".to_string(),
            dedupe_key: "hook-stop:thread-a:turn-1".to_string(),
        },
        "ok",
        0,
        1,
        "fixture-request",
        1,
        std::time::Duration::from_secs(2),
    )
    .await;

    assert!(result.sent);
    assert_eq!(result.reason, None);
    assert_eq!(result.http_status, Some(200));
    assert_eq!(result.request_count, 1);
    assert_eq!(result.chunk_count, 1);
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("device-key-secret"));
    let _ = server.request();
    let _ = fs::remove_dir_all(sandbox_root);
}

#[tokio::test]
async fn bark_test_uses_agenthub_title_and_machine_body() {
    let server = TestHttpServer::start_n(
        1,
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
    );
    let dir = temp_test_dir("agenthub-bark-cli");
    fs::create_dir_all(&dir).unwrap();
    let mut config = test_config(dir.join(".codex"));
    config.probe.notifications.enabled = true;
    config.probe.notifications.server_url = server.url();
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    db.set_secret_setting_bytes("probe_bark_device_key", b"device-key")
        .unwrap();

    let path = dir.join("config.toml");
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    run_probe_command(ProbeCommand::BarkTest, &config, db, &path)
        .await
        .unwrap();
    fs::remove_dir_all(dir).unwrap();

    let raw = server.request();
    assert!(raw.starts_with("POST /push "));
    let body = raw.split("\r\n\r\n").nth(1).unwrap();
    let payload: Value = serde_json::from_str(body).unwrap();
    assert_eq!(payload["device_key"], "device-key");
    assert_eq!(payload["title"], "AgentHub 推送测试");
    assert_eq!(payload["body"], agenthub_core::probe::bark_test_body());
}

#[tokio::test]
async fn hook_stop_cli_payload_keeps_stdout_codex_only_and_stderr_diagnostics() {
    let (mut config, _identity) =
        notification_accuracy_tests::test_notification_config(&["thread-a"]);
    config.probe.notifications.enabled = false;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let result = handle_built_probe_event(
        &|| Ok(config.clone()),
        &db,
        build_probe_event(
            &config,
            ProbeEventInput::hook_stop_with_context(
                Some("thread-a"),
                Some("turn-a"),
                None,
                None,
                None,
                "hook-stop",
            ),
        ),
    )
    .await
    .unwrap();

    let (stdout, stderr) = hook_stop_cli_output(&result).unwrap();
    assert_eq!(stdout.trim(), r#"{"continue":true,"suppressOutput":false}"#);
    let diagnostics: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(diagnostics["probe_event"]["recorded"], true);
    assert_eq!(diagnostics["bark"]["skipped"], true);
    assert_eq!(diagnostics["bark"]["reason"], "notifications_disabled");
    assert!(!stdout.contains("bark"));
    assert!(!stdout.contains("probe_event"));
}

#[test]
fn antigravity_stop_allow_output_returns_decision_allow_json() {
    let out = antigravity_stop_allow_output();
    assert_eq!(out, json!({"decision": "allow"}));

    let printed = std::sync::atomic::AtomicBool::new(false);
    let mut buffer = Vec::new();
    assert!(print_antigravity_stop_allow_once(&printed, &mut buffer));
    assert!(!print_antigravity_stop_allow_once(&printed, &mut buffer));
    assert_eq!(
        String::from_utf8(buffer).unwrap(),
        "{\"decision\":\"allow\"}\n"
    );
}

pub(super) struct TestHttpServer {
    address: std::net::SocketAddr,
    request: std::sync::mpsc::Receiver<String>,
}

impl TestHttpServer {
    pub(super) fn start_n(expected_requests: usize, response: &'static str) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    request.push_str(&line);
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                let content_length = request
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                let mut body = vec![0_u8; content_length];
                reader.read_exact(&mut body).unwrap();
                request.push_str(&String::from_utf8_lossy(&body));
                tx.send(request).unwrap();
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        Self {
            address,
            request: rx,
        }
    }

    pub(super) fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(super) fn request(self) -> String {
        self.request
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
    }

    fn requests(self, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                self.request
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap()
            })
            .collect()
    }
}

#[tokio::test]
async fn probe_thread_scan_records_request_user_input_body_for_reply_needed() {
    let dir = temp_test_dir("agenthub-thread-scan-request-user-input");
    let codex_home = dir.join(".codex");
    fs::create_dir_all(&codex_home).unwrap();
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    fs::write(
        codex_home
            .join("sessions")
            .join("rollout-stale-reply.jsonl"),
        json!({
            "type": "response_item",
            "turn_id": "turn-stale",
            "payload": {
                "type": "function_call",
                "name": "request_user_input",
                "status": "pending",
                "call_id": "call-choice",
                "questions": [{
                    "id": "q1",
                    "header": "确认",
                    "question": "Continue?",
                    "options": [
                        {"label": "继续", "description": "按计划执行"},
                        {"label": "停止"}
                    ]
                }]
            }
        })
        .to_string(),
    )
    .unwrap();
    let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            title TEXT,
            source TEXT,
            thread_source TEXT,
            updated_at INTEGER,
            archived_at INTEGER,
            rollout_path TEXT
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, title, source, thread_source, updated_at, rollout_path) VALUES(?1, ?2, 'vscode', 'user', ?3, ?4)",
        params![
            "stale-reply",
            "stale reply",
            jiff::Timestamp::now().as_millisecond(),
            codex_home
                .join("sessions")
                .join("rollout-stale-reply.jsonl")
                .to_string_lossy()
                .as_ref()
        ],
    )
    .unwrap();
    fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
    let mut config = test_config(codex_home.clone());
    config.probe.notifications.enabled = true;
    config.probe.notifications.server_url = "http://127.0.0.1:9".to_string();
    config.probe.notifications.notify_reply_needed = true;
    config.probe.notifications.notify_recoverable = true;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let state = passive_test_state(config, db.clone(), &dir);

    let count = run_probe_thread_scan_if_due(state).await.unwrap();

    assert_eq!(count, 1);
    let events = db.list_probe_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "reply-needed");
    assert_eq!(events[0].payload["thread_title"], "stale reply");
    assert_eq!(events[0].payload["body_source"], "request_user_input");
    let body_summary = events[0].payload["body_summary"].as_str().unwrap();
    assert!(!body_summary.contains("等待用户选择"));
    assert!(!body_summary.contains("Turn ID："));
    assert!(!body_summary.contains("Call ID："));
    assert!(!body_summary.contains("状态说明："));
    assert!(body_summary.contains("问题 1：Continue?"));
    assert!(body_summary.contains("选项 1：继续"));
    assert!(body_summary.contains("说明：按计划执行"));
    assert!(body_summary.contains("选项 2：停止"));
    assert!(events[0].payload["bark"].get("body").is_none());
    assert!(events[0].payload["bark"]["title"]
        .as_str()
        .unwrap()
        .contains("等待回复"));
    fs::remove_dir_all(&dir).unwrap();
}

fn passive_test_state(config: Config, db: PanelDb, dir: &Path) -> AppState {
    let config_path = dir.join("passive-config.toml");
    fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
    AppState::new(config, db).with_config_path(config_path)
}

#[tokio::test]
async fn passive_scan_reloads_external_disable_and_reports_bad_config() {
    let dir = temp_test_dir("agenthub-passive-config-refresh");
    fs::create_dir_all(&dir).unwrap();
    let mut config = test_config(dir.join("unreadable-codex"));
    config.probe.enabled = true;
    config.probe.notifications.enabled = true;
    let db = PanelDb::open_with_secret_box(
        ":memory:",
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let state = passive_test_state(config.clone(), db, &dir);
    config.probe.enabled = false;
    agenthub_core::config::write_config_atomic(
        state.config_path(),
        &toml::to_string(&config).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run_probe_thread_scan_if_due(state.clone()).await.unwrap(),
        0
    );
    assert!(!state.config().probe.enabled);
    fs::write(state.config_path(), "[invalid").unwrap();
    assert!(run_probe_thread_scan_if_due(state).await.is_err());
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn question_monitors_sync_channels_from_fresh_config_without_wiping_newly_enabled_bark_queue()
{
    let dir = temp_test_dir("agenthub-fresh-bark-sync");
    fs::create_dir_all(&dir).unwrap();
    let mut stale_tick_cfg = test_config(dir.join(".codex"));
    stale_tick_cfg.probe.enabled = true;
    stale_tick_cfg.probe.error_monitor.enabled = true;
    stale_tick_cfg.probe.notifications.enabled = false;
    stale_tick_cfg.probe.notifications.notify_codex = true;
    stale_tick_cfg.probe.notifications.notify_reply_needed = true;

    let mut fresh_enabled_cfg = stale_tick_cfg.clone();
    fresh_enabled_cfg.probe.notifications.enabled = true;
    fresh_enabled_cfg.probe.notifications.server_url = "https://example.invalid/push".into();

    let db = PanelDb::open_with_secret_box(
        dir.join("panel.sqlite"),
        agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    let fresh_loader = || Ok(fresh_enabled_cfg.clone());
    let tick_loader = || Ok(stale_tick_cfg.clone());

    // Simulate user enabling Bark mid-tick and staging an encrypted pending notification.
    db.sync_notification_channels_from_config(&fresh_loader)
        .unwrap();
    let now_ms = jiff::Timestamp::now().as_millisecond();
    assert!(db
        .stage_notification_delivery(
            agenthub_core::db::NotificationChannel::Bark,
            "mid-tick:newly-queued",
            now_ms,
            &json!({"title": "New Alert", "body": "Must survive monitor sync"}),
        )
        .unwrap());

    crate::async_question_monitor::run_with_threads_and_fresh_loader(
        &tick_loader,
        &fresh_loader,
        &db,
        Some(&[]),
    )
    .await
    .unwrap();
    crate::question_monitor::run_with_threads_and_fresh_loader(
        &tick_loader,
        &fresh_loader,
        &db,
        Some(&[]),
    )
    .await
    .unwrap();

    let conn = Connection::open(db.path()).unwrap();
    let (status, has_cipher, has_nonce): (String, bool, bool) = conn
        .query_row(
            "SELECT status, ciphertext IS NOT NULL, nonce IS NOT NULL FROM probe_notification_deliveries WHERE event_key = 'mid-tick:newly-queued'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        status, "pending",
        "stale tick_config must never mark newly queued Bark delivery skipped"
    );
    assert!(
        has_cipher && has_nonce,
        "newly queued Bark delivery ciphertext and nonce must remain intact"
    );
    let bark_enabled: bool = conn
        .query_row(
            "SELECT enabled FROM probe_notification_channels WHERE channel = 'bark'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(bark_enabled);
    fs::remove_dir_all(dir).unwrap();
}

fn test_config(codex_home: PathBuf) -> Config {
    let root = codex_home.parent().unwrap_or(&codex_home).to_path_buf();
    let mut config = Config::current_defaults().unwrap();
    config.codex.home = codex_home;
    config.codex.workspace = root.join("workspace");
    config.paths.data_dir = root.join("data");
    config.paths.db_path = root.join("data/panel.sqlite");
    config.paths.log_dir = root.join("logs");

    config
}

fn temp_test_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{unique}"))
}
