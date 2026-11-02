use agenthub_core::{
    config::{
        Config, ProbeNotificationsConfigPatch, ProbeObservabilityConfigPatch, ProbeSettingsPatch,
    },
    platform::{PlatformKind, PlatformPaths},
    services::{
        settings::{
            build_settings_view, merge_probe_notification_patch, normalize_bark_device_key,
            normalize_probe_settings_patch, plan_probe_settings_save,
            probe_settings_view_with_capability, ProbeNotificationsSavePatch, ProbeSecretState,
            ProbeSettingsSavePatch, ProbeSettingsSaveRequest, PROBE_BARK_DEVICE_KEY_SETTING,
        },
        system::{require_capability, system_capabilities, Capability},
    },
};

#[test]
fn linux_capabilities_expose_api_and_maintenance_features() {
    let config = Config::for_platform_kind(PlatformKind::Linux);
    let capabilities = system_capabilities(&config, &PlatformPaths::for_kind(PlatformKind::Linux));

    assert!(capabilities.threads);
    assert!(capabilities.jobs);
    assert!(capabilities.probe);
    assert!(capabilities.settings);
    assert!(capabilities.job_history);
    assert!(capabilities.app_updater);
    assert!(capabilities.thread_cleanup);
    assert!(capabilities.thread_archive_actions);
    assert!(capabilities.systemd);
    assert!(!capabilities.antigravity_notifications);
    assert!(!capabilities.dsh_notifications);
    assert!(capabilities.linux_update_job);
    assert!(capabilities.prune_backups);
}

#[test]
fn macos_capabilities_keep_shared_core_but_disable_linux_maintenance_features() {
    let home = temp_dir("agenthub-capabilities-macos");
    std::fs::create_dir_all(&home).unwrap();
    let config = Config::for_platform_kind_with_home(PlatformKind::Macos, &home);
    let capabilities = system_capabilities(
        &config,
        &PlatformPaths::for_kind_with_home(PlatformKind::Macos, &home),
    );

    assert!(capabilities.threads);
    assert!(capabilities.jobs);
    assert!(capabilities.probe);
    assert!(capabilities.settings);
    assert!(capabilities.job_history);
    assert!(capabilities.app_updater);
    assert!(capabilities.thread_cleanup);
    assert!(capabilities.thread_archive_actions);
    assert!(!capabilities.systemd);
    assert!(capabilities.antigravity_notifications);
    assert!(capabilities.dsh_notifications);
    assert!(!capabilities.linux_update_job);
    assert!(!capabilities.prune_backups);

    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn macos_linux_maintenance_capabilities_are_rejected_by_gate_and_matrix() {
    let config = Config::for_platform_kind(PlatformKind::Macos);
    let platform = PlatformPaths::for_kind(PlatformKind::Macos);
    let capabilities = system_capabilities(&config, &platform);

    for capability in [
        Capability::Systemd,
        Capability::LinuxUpdateJob,
        Capability::PruneBackups,
    ] {
        assert!(
            require_capability(&platform, capability).is_err(),
            "{capability:?} must stay unavailable on macOS"
        );
    }
    assert!(!capabilities.systemd);
    assert!(!capabilities.linux_update_job);
    assert!(!capabilities.prune_backups);
}

#[test]
fn bark_device_key_is_trimmed_and_empty_values_are_ignored() {
    assert_eq!(
        normalize_bark_device_key(Some("  bark-key-123  ".to_string())),
        Some("bark-key-123".to_string())
    );
    assert_eq!(normalize_bark_device_key(Some(" \n\t ".to_string())), None);
    assert_eq!(normalize_bark_device_key(None), None);
}

#[test]
fn settings_view_reports_secret_state_without_returning_secret() {
    let mut config = Config::for_platform_kind(PlatformKind::Linux);
    config.probe.notifications.enabled = true;
    config.probe.notifications.server_url = "https://bark.example.com".to_string();

    let view = build_settings_view(&config, ProbeSecretState::Configured);
    let serialized = serde_json::to_string(&view).unwrap();

    assert_eq!(
        view.probe.notifications.server_url,
        "https://bark.example.com"
    );
    assert!(view.notifications.device_key_configured);
    assert!(view.notifications.device_key.is_none());
    assert!(!serialized.contains("bark-key-123"));
    assert!(!serialized.contains("device_key\":\""));
}

#[test]
fn probe_settings_get_facade_is_shared_and_redacts_bark_secret() {
    let mut config = Config::for_platform_kind(PlatformKind::Linux);
    config.probe.notifications.enabled = true;
    config.probe.notifications.server_url = "https://bark.example.com".to_string();

    let linux = PlatformPaths::for_kind(PlatformKind::Linux);
    let plan = probe_settings_view_with_capability(&config, &linux, ProbeSecretState::Configured)
        .expect("Linux should expose shared Probe settings");
    let serialized = serde_json::to_string(&plan).unwrap();

    assert_eq!(plan.required_capability, Capability::Settings);
    assert!(plan.settings.notifications.device_key_configured);
    assert!(plan.settings.notifications.device_key.is_none());
    assert!(!serialized.contains("secret-bark-key"));
    assert!(!serialized.contains("device_key\":\""));

    let macos = PlatformPaths::for_kind(PlatformKind::Macos);
    assert!(
        probe_settings_view_with_capability(&config, &macos, ProbeSecretState::Missing).is_ok()
    );

    let windows = PlatformPaths::for_kind(PlatformKind::Windows);
    let err = probe_settings_view_with_capability(&config, &windows, ProbeSecretState::Missing)
        .expect_err("Windows should not expose shared settings");
    assert!(err
        .to_string()
        .contains("settings is unavailable on windows"));
}

#[test]
fn probe_settings_patch_validation_rejects_bad_url_and_clamps_numeric_ranges() {
    let invalid_url = ProbeSettingsPatch {
        notifications: Some(ProbeNotificationsConfigPatch {
            server_url: Some("http://example.com".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(normalize_probe_settings_patch(invalid_url)
        .unwrap_err()
        .to_string()
        .contains("server_url"));

    let patch = ProbeSettingsPatch {
        poll_seconds: Some(1),
        recent_limit: Some(5_000),
        notifications: Some(ProbeNotificationsConfigPatch {
            server_url: Some("  http://127.0.0.1:8080  ".to_string()),
            group: Some("   ".to_string()),
            ..Default::default()
        }),
        observability: Some(ProbeObservabilityConfigPatch {
            event_retention_days: Some(4),
            hook_event_max_lines: Some(1),
            hook_cooldown_max_lines: Some(50_000),
            log_max_bytes: Some(1),
        }),
        ..Default::default()
    };

    let normalized = normalize_probe_settings_patch(patch).unwrap();
    assert_eq!(normalized.poll_seconds, Some(5));
    assert_eq!(normalized.recent_limit, Some(500));
    let notifications = normalized.notifications.unwrap();
    assert_eq!(
        notifications.server_url.as_deref(),
        Some("http://127.0.0.1:8080")
    );
    assert_eq!(notifications.group.as_deref(), Some("AgentHub"));
    let observability = normalized.observability.unwrap();
    assert_eq!(observability.hook_event_max_lines, Some(10));
    assert_eq!(observability.hook_cooldown_max_lines, Some(10_000));
    assert_eq!(observability.log_max_bytes, Some(4_096));
}

#[test]
fn probe_settings_patch_clamps_log_max_bytes_at_shared_frontend_boundary() {
    let over_max = normalize_probe_settings_patch(ProbeSettingsPatch {
        poll_seconds: Some(3_601),
        recent_limit: Some(0),
        observability: Some(ProbeObservabilityConfigPatch {
            event_retention_days: Some(4),
            log_max_bytes: Some(8_388_609),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();

    assert_eq!(over_max.poll_seconds, Some(3_600));
    assert_eq!(over_max.recent_limit, Some(1));
    assert_eq!(
        over_max.observability.unwrap().log_max_bytes,
        Some(8_388_608)
    );

    let at_max = normalize_probe_settings_patch(ProbeSettingsPatch {
        observability: Some(ProbeObservabilityConfigPatch {
            event_retention_days: Some(4),
            log_max_bytes: Some(8_388_608),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();

    assert_eq!(at_max.observability.unwrap().log_max_bytes, Some(8_388_608));
}

#[test]
fn notification_patch_merge_preserves_existing_fields_when_source_omits_them() {
    let mut target = ProbeNotificationsConfigPatch {
        enabled: Some(true),
        server_url: Some("https://api.day.app".to_string()),
        group: Some("Ops".to_string()),
        ..Default::default()
    };

    merge_probe_notification_patch(
        &mut target,
        ProbeNotificationsConfigPatch {
            sound: Some(Some("alarm".to_string())),
            group: Some("  ".to_string()),
            ..Default::default()
        },
    );
    let normalized = normalize_probe_settings_patch(ProbeSettingsPatch {
        notifications: Some(target),
        ..Default::default()
    })
    .unwrap()
    .notifications
    .unwrap();

    assert_eq!(normalized.enabled, Some(true));
    assert_eq!(
        normalized.server_url.as_deref(),
        Some("https://api.day.app")
    );
    assert_eq!(normalized.sound, Some(Some("alarm".to_string())));
    assert_eq!(normalized.group.as_deref(), Some("AgentHub"));
}

#[test]
fn probe_settings_save_request_normalizes_bark_key_and_merges_notification_patches() {
    let request: ProbeSettingsSaveRequest = serde_json::from_value(serde_json::json!({
        "probe": {
            "notifications": {
                "enabled": true,
                "server_url": "  https://bark.example.com  ",
                "device_key": " nested-key ",
                "notify_completion": true
            }
        },
        "notifications": {
            "group": " Ops ",
            "device_key": " top-key "
        }
    }))
    .unwrap();

    let normalized = request.normalize().unwrap();

    assert_eq!(normalized.bark_device_key.as_deref(), Some("top-key"));
    let notifications = normalized
        .config_patch
        .probe
        .unwrap()
        .notifications
        .unwrap();
    assert_eq!(notifications.enabled, Some(true));
    assert_eq!(
        notifications.server_url.as_deref(),
        Some("https://bark.example.com")
    );
    assert_eq!(notifications.group.as_deref(), Some("Ops"));
    assert_eq!(notifications.notify_completion, Some(true));
}

#[test]
fn probe_settings_save_request_keeps_nested_bark_key_when_top_level_is_blank() {
    let request: ProbeSettingsSaveRequest = serde_json::from_value(serde_json::json!({
        "probe": {
            "notifications": {
                "device_key": " nested-key "
            }
        },
        "notifications": {
            "device_key": "  "
        }
    }))
    .unwrap();

    let normalized = request.normalize().unwrap();

    assert_eq!(normalized.bark_device_key.as_deref(), Some("nested-key"));
    assert!(normalized.config_patch.probe.is_none());
}

#[test]
fn probe_settings_save_plan_exposes_redacted_bark_secret_write_contract() {
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);
    let plan = plan_probe_settings_save(
        &platform,
        ProbeSettingsSaveRequest {
            probe: Some(ProbeSettingsSavePatch {
                notifications: Some(ProbeNotificationsSavePatch {
                    device_key: Some("  secret-bark-key  ".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .expect("Linux should allow shared Probe settings save");

    assert_eq!(plan.required_capability, Capability::Settings);
    assert_eq!(plan.bark_device_key.as_deref(), Some("secret-bark-key"));
    assert_eq!(plan.secret_writes.len(), 1);
    let write = &plan.secret_writes[0];
    assert_eq!(write.setting_key, PROBE_BARK_DEVICE_KEY_SETTING);
    assert_eq!(write.secret_value, "secret-bark-key");
    assert_eq!(write.audit_value, "[configured]");

    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(serialized.contains(PROBE_BARK_DEVICE_KEY_SETTING));
    assert!(serialized.contains("[configured]"));
    assert!(!serialized.contains("secret-bark-key"));
}

#[test]
fn core_services_do_not_import_host_frameworks_or_process_runners() {
    fn contains_command_constructor(source: &str) -> bool {
        let is_word = |character: char| character.is_alphanumeric() || character == '_';

        source.match_indices("Command").any(|(start, _)| {
            if source[..start].chars().next_back().is_some_and(is_word) {
                return false;
            }

            let after_command =
                source[start + "Command".len()..].trim_start_matches(char::is_whitespace);
            let Some(after_colons) = after_command.strip_prefix("::") else {
                return false;
            };
            let after_namespace = after_colons.trim_start_matches(char::is_whitespace);
            let Some(after_new) = after_namespace.strip_prefix("new") else {
                return false;
            };

            after_new
                .chars()
                .next()
                .is_none_or(|character| !is_word(character))
        })
    }

    assert!(!contains_command_constructor("ProcessCommand::new"));
    assert!(contains_command_constructor("Command::new"));
    assert!(contains_command_constructor("std::process::Command::new"));

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).unwrap();
    for forbidden_dependency in ["axum", "tauri", "agenthub-webd", "src-tauri"] {
        assert!(
            !manifest.contains(forbidden_dependency),
            "agenthub-core must not declare adapter/runtime dependency {forbidden_dependency}"
        );
    }

    for relative in [
        "src/services/probe.rs",
        "src/services/settings.rs",
        "src/services/system.rs",
        "src/services/updates.rs",
        "src/services/commands.rs",
        "src/services/mod.rs",
    ] {
        let path = manifest_dir.join(relative);
        let source = std::fs::read_to_string(&path).unwrap();
        for forbidden in [
            "use axum",
            "axum::",
            "use tauri",
            "tauri::",
            "HeaderMap",
            "HeaderValue",
            "std::process::Command",
            "tokio::process::Command",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} must stay adapter-neutral and not contain {forbidden}"
            );
        }
        assert!(
            !contains_command_constructor(&source),
            "{relative} must stay adapter-neutral and not contain a Command::new process constructor"
        );
    }
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(format!("{label}-{unique}"))
}

#[test]
fn local_notification_switches_default_off_and_round_trip_through_settings_and_toml() {
    use agenthub_core::config::{patch_probe_config_toml, ProbeConfigFilePatch};
    let config = Config::default();
    let defaults = serde_json::to_value(&config.probe.notifications).unwrap();
    let fields = [
        "notify_antigravity",
        "notify_antigravity_completion",
        "notify_antigravity_failure",
        "notify_antigravity_reply_needed",
        "notify_antigravity_authorization",
        "notify_dsh",
        "notify_dsh_completion",
        "notify_dsh_failure",
        "notify_dsh_reply_needed",
        "notify_dsh_authorization",
    ];
    let mut values = serde_json::Map::new();
    for field in fields {
        assert_eq!(defaults[field], false);
        values.insert(field.into(), serde_json::Value::Bool(true));
    }
    let patch: ProbeNotificationsConfigPatch = serde_json::from_value(values.into()).unwrap();
    let mut merged = ProbeNotificationsConfigPatch::default();
    merge_probe_notification_patch(&mut merged, patch);
    let file_patch = ProbeConfigFilePatch {
        probe: Some(ProbeSettingsPatch {
            notifications: Some(merged),
            ..Default::default()
        }),
        ..Default::default()
    };
    let updated = patch_probe_config_toml(&toml::to_string(&config).unwrap(), &file_patch).unwrap();
    let reloaded: Config = toml::from_str(&updated).unwrap();
    let view = build_settings_view(&reloaded, ProbeSecretState::Configured);
    let value = serde_json::to_value(view.notifications).unwrap();
    for field in fields {
        assert_eq!(value[field], true);
    }
}
