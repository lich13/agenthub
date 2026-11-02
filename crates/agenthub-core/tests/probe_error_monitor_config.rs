use agenthub_core::{
    config::{patch_probe_config_toml, Config, ProbeConfigFilePatch},
    platform::PlatformKind,
    probe_error_monitor::{
        ensure_probe_error_monitor_launch_agent_at, probe_error_monitor_launch_agent_plist,
    },
};

#[test]
fn probe_error_monitor_config_defaults_enabled_and_patches_independent_switches() {
    let config = Config::for_platform_kind_with_home(PlatformKind::Macos, "/tmp/agenthub-home");
    assert!(config.probe.error_monitor.enabled);

    let patch: ProbeConfigFilePatch = serde_json::from_value(serde_json::json!({
        "probe": {
            "error_monitor": {
                "enabled": false,
            }
        }
    }))
    .unwrap();
    let updated = patch_probe_config_toml("[probe]\nenabled = true\n", &patch).unwrap();
    assert!(updated.contains("[probe.error_monitor]"));
    assert!(updated.contains("enabled = false"));
    assert!(!updated.contains("auto_resume_goals"));
}

#[test]
fn retired_goal_switch_is_removed_during_config_load() {
    let root = std::env::temp_dir().join(format!("agenthub-goal-config-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.toml");
    let text = toml::to_string_pretty(&Config::default()).unwrap().replace(
        "[probe.error_monitor]\n",
        "[probe.error_monitor]\nauto_resume_goals = true\n",
    );
    std::fs::write(&path, text).unwrap();
    let config = Config::load(&path).unwrap();
    assert!(config.probe.error_monitor.enabled);
    let rewritten = std::fs::read_to_string(&path).unwrap();
    assert!(!rewritten.contains("auto_resume_goals"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_error_monitor_launch_agent_repair_is_idempotent_and_unloads_when_disabled() {
    let root = std::env::temp_dir().join(format!(
        "agenthub-launch-agent-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut platform =
        agenthub_core::platform::PlatformPaths::for_kind_with_home(PlatformKind::Macos, &root);
    platform.data_dir = root.join("Application Support/AgentHub");
    platform.config_file = platform.data_dir.join("config.toml");
    platform.log_dir = root.join("Logs/AgentHub");
    let helper = platform.daemon_binary();
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, b"#!/bin/sh\nexit 0\n").unwrap();
    let launch_agents = root.join("LaunchAgents");
    let launchctl = root.join("launchctl");
    let state = root.join("launchctl-loaded");
    std::fs::write(
        &launchctl,
        format!(
            "#!/bin/sh\ncase \"$1\" in\n  print) test -f '{}' ;;\n  bootstrap) touch '{}' ;;\n  kickstart) exit 0 ;;\n  bootout) rm -f '{}' ;;\n  *) exit 2 ;;\nesac\n",
            state.display(),
            state.display(),
            state.display(),
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&helper, &launchctl] {
            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).unwrap();
        }
    }

    let first =
        ensure_probe_error_monitor_launch_agent_at(&platform, true, &launch_agents, &launchctl)
            .unwrap();
    assert!(first.changed);
    assert!(first.loaded);
    let second =
        ensure_probe_error_monitor_launch_agent_at(&platform, true, &launch_agents, &launchctl)
            .unwrap();
    assert!(!second.changed);
    assert!(second.loaded);

    let disabled =
        ensure_probe_error_monitor_launch_agent_at(&platform, false, &launch_agents, &launchctl)
            .unwrap();
    assert!(disabled.changed);
    assert!(!disabled.loaded);
    assert!(!disabled.plist_path.unwrap().exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_error_monitor_launch_agent_uses_fixed_helper_without_network_listener() {
    let plist = probe_error_monitor_launch_agent_plist(
        std::path::Path::new("/Users/test/Library/Application Support/AgentHub/bin/agenthub-webd"),
        std::path::Path::new("/Users/test/Library/Application Support/AgentHub/config.toml"),
        std::path::Path::new("/Users/test/Library/Logs/AgentHub/probe-error-monitor.out.log"),
        std::path::Path::new("/Users/test/Library/Logs/AgentHub/probe-error-monitor.err.log"),
    );
    assert!(plist.contains("com.lich13.agenthub.probe-error-monitor"));
    assert!(plist.contains("<string>probe</string>"));
    assert!(plist.contains("<string>monitor-errors</string>"));
    assert!(plist.contains("<string>--config</string>"));
    assert!(!plist.contains("serve"));
    assert!(!plist.contains("15742"));
    assert!(!plist.contains("Sockets"));
}

#[test]
fn probe_error_monitor_launch_agent_preserves_plist_when_unload_fails() {
    let root = std::env::temp_dir().join(format!(
        "agenthub-launch-agent-unload-failure-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let launch_agents = root.join("LaunchAgents");
    std::fs::create_dir_all(&launch_agents).unwrap();
    let platform =
        agenthub_core::platform::PlatformPaths::for_kind_with_home(PlatformKind::Macos, &root);
    let plist = launch_agents.join("com.lich13.agenthub.probe-error-monitor.plist");
    std::fs::write(&plist, "managed").unwrap();
    let launchctl = root.join("launchctl");
    std::fs::write(
        &launchctl,
        "#!/bin/sh\ncase \"$1\" in\n  print) exit 0 ;;\n  bootout) exit 9 ;;\n  *) exit 2 ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&launchctl).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&launchctl, permissions).unwrap();
    }

    let error =
        ensure_probe_error_monitor_launch_agent_at(&platform, false, &launch_agents, &launchctl)
            .unwrap_err();

    assert!(error.to_string().contains("bootout"));
    assert!(plist.is_file());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn probe_error_monitor_launch_agent_requires_loaded_state_after_bootstrap() {
    let root = std::env::temp_dir().join(format!(
        "agenthub-launch-agent-load-failure-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut platform =
        agenthub_core::platform::PlatformPaths::for_kind_with_home(PlatformKind::Macos, &root);
    platform.data_dir = root.join("Application Support/AgentHub");
    platform.config_file = platform.data_dir.join("config.toml");
    platform.log_dir = root.join("Logs/AgentHub");
    let helper = platform.daemon_binary();
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, b"#!/bin/sh\nexit 0\n").unwrap();
    let launch_agents = root.join("LaunchAgents");
    let launchctl = root.join("launchctl");
    std::fs::write(
        &launchctl,
        "#!/bin/sh\ncase \"$1\" in\n  print) exit 1 ;;\n  bootstrap) exit 0 ;;\n  *) exit 2 ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&helper, &launchctl] {
            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).unwrap();
        }
    }

    let error =
        ensure_probe_error_monitor_launch_agent_at(&platform, true, &launch_agents, &launchctl)
            .unwrap_err();

    assert!(error.to_string().contains("not loaded"));
    std::fs::remove_dir_all(root).unwrap();
}
