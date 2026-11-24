use agenthub_core::codex::CodexPaths;
use agenthub_core::{
    config::Config,
    platform::{PlatformKind, PlatformPaths},
    services::{
        commands,
        jobs::{
            plan_thread_archive_with_capability, plan_thread_rename_with_capability,
            plan_thread_restore_with_capability, thread_state_action_response, ThreadRenameRequest,
        },
        probe::{plan_probe_action, ProbeAction, ProbeUseCases},
        settings::{
            ProbeNotificationsSavePatch, ProbeSecretState, ProbeSettingsSavePatch,
            ProbeSettingsSaveRequest, SettingsUseCases,
        },
        threads::{
            plan_thread_blocks_request, plan_thread_detail_request, plan_threads_list_request,
            read_thread_blocks_page, ThreadDetailRequest, ThreadsQuery,
        },
        updates::{plan_update_action, UpdateAction},
    },
};

#[test]
fn probe_and_update_actions_expose_shared_rpc_and_desktop_command_names() {
    assert_eq!(
        ProbeAction::BarkTest.as_rpc_action(),
        commands::PROBE_BARK_TEST
    );
    assert_eq!(
        ProbeAction::BarkTest.as_desktop_command(),
        commands::PROBE_BARK_TEST
    );
    assert_eq!(
        ProbeAction::InstallHooks.as_rpc_action(),
        commands::PROBE_INSTALL_HOOKS
    );
    assert_eq!(
        ProbeAction::InstallHooks.as_desktop_command(),
        commands::PROBE_INSTALL_HOOKS
    );
    assert_eq!(
        ProbeAction::RemoveHooks.as_rpc_action(),
        commands::PROBE_REMOVE_HOOKS
    );
    assert_eq!(
        ProbeAction::RemoveHooks.as_desktop_command(),
        commands::PROBE_REMOVE_HOOKS
    );

    assert_eq!(UpdateAction::Check.as_rpc_action(), commands::UPDATES_CHECK);
    assert_eq!(
        UpdateAction::Install.as_rpc_action(),
        commands::UPDATES_INSTALL
    );
    assert_eq!(UpdateAction::Prune.as_rpc_action(), commands::UPDATES_PRUNE);
}

#[test]
fn probe_actions_parse_string_aliases_and_plan_fixed_jobs_in_core() {
    let platform = PlatformPaths::linux_server();

    assert_eq!(
        "barkTest".parse::<ProbeAction>().unwrap(),
        ProbeAction::BarkTest
    );
    assert_eq!(
        "installHooks".parse::<ProbeAction>().unwrap(),
        ProbeAction::InstallHooks
    );
    assert_eq!(
        "removeHooks".parse::<ProbeAction>().unwrap(),
        ProbeAction::RemoveHooks
    );
    assert_eq!(
        "hooks-remove".parse::<ProbeAction>().unwrap(),
        ProbeAction::RemoveHooks
    );
    assert!("unknown".parse::<ProbeAction>().is_err());

    let bark = plan_probe_action(&platform, ProbeAction::BarkTest, &platform.config_file).unwrap();
    assert_eq!(bark.action, ProbeAction::BarkTest);
    assert_eq!(bark.job.kind, "probe_bark_test");
    assert_eq!(
        bark.job.args,
        vec!["probe".to_string(), "bark-test".to_string()]
    );
    assert_eq!(bark.job.exclusive_group.as_deref(), Some("probe_bark"));

    assert!("logsDbDryRun".parse::<ProbeAction>().is_err());
    assert!("logs-db-execute".parse::<ProbeAction>().is_err());
}

#[test]
fn probe_fixed_job_command_is_core_generated_and_can_use_config_path_override() {
    let platform = PlatformPaths::linux_server();

    let default_plan =
        plan_probe_action(&platform, ProbeAction::BarkTest, &platform.config_file).unwrap();
    let default_command = &default_plan.job.command;
    assert_eq!(
        default_command,
        "/usr/local/bin/agenthub-webd --config /etc/agenthub-webd/config.toml probe bark-test"
    );

    let custom_config = std::path::Path::new("/tmp/agenthub custom/config.toml");
    let override_plan = plan_probe_action(&platform, ProbeAction::BarkTest, custom_config).unwrap();
    let override_job = override_plan.job;

    assert_eq!(
        override_job.command,
        "/usr/local/bin/agenthub-webd --config '/tmp/agenthub custom/config.toml' probe bark-test"
    );
    assert_eq!(
        override_job.args,
        vec!["probe".to_string(), "bark-test".to_string()]
    );
    assert!(!override_job
        .command
        .contains("/etc/agenthub-webd/config.toml"));
}

#[test]
fn update_action_facade_plans_linux_jobs_and_macos_native_updates_but_not_prune() {
    let linux = PlatformPaths::linux_server();
    let check = plan_update_action(&linux, UpdateAction::Check).unwrap();
    assert_eq!(
        check.linux_job.as_ref().unwrap().kind,
        "agenthub_update_check"
    );
    assert!(check.macos_job.is_none());

    let mac_home = temp_dir("agenthub-update-action-macos");
    std::fs::create_dir_all(&mac_home).unwrap();
    let mac = PlatformPaths::for_kind_with_home(PlatformKind::Macos, &mac_home);

    let mac_check = plan_update_action(&mac, UpdateAction::Check).unwrap();
    assert!(mac_check.linux_job.is_none());
    assert_eq!(
        mac_check.macos_job.as_ref().unwrap().kind,
        "agenthub_update_check"
    );

    let mac_install = plan_update_action(&mac, UpdateAction::Install).unwrap();
    assert_eq!(
        mac_install.macos_job.as_ref().unwrap().kind,
        "agenthub_update_install"
    );

    let err = plan_update_action(&mac, UpdateAction::Prune)
        .expect_err("backup prune is a Linux-only update action");
    assert!(err
        .to_string()
        .contains("prune_backups is unavailable on macos"));

    std::fs::remove_dir_all(mac_home).unwrap();
}

#[test]
fn probe_settings_update_use_cases_group_adapter_ready_plans_in_core() {
    let linux_config = Config::linux_defaults();
    let linux = PlatformPaths::linux_server();

    let probe = ProbeUseCases::new(&linux);
    let bark = probe.action(ProbeAction::BarkTest).unwrap();
    assert_eq!(bark.job.kind, "probe_bark_test");

    let settings = SettingsUseCases::new(&linux_config, &linux);
    let view = settings
        .probe_settings_view(ProbeSecretState::Configured)
        .unwrap();
    assert!(view.notifications.device_key_configured);

    let save = settings
        .save_probe_settings(ProbeSettingsSaveRequest {
            probe: Some(ProbeSettingsSavePatch {
                notifications: Some(ProbeNotificationsSavePatch {
                    device_key: Some(" use-case-bark-key ".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        Some(save.secret_writes[0].secret_value.as_str()),
        Some("use-case-bark-key")
    );
    let serialized = serde_json::to_string(&save).unwrap();
    assert!(!serialized.contains("use-case-bark-key"));
}

#[test]
fn thread_detail_facades_normalize_inputs() {
    let linux = PlatformPaths::linux_server();

    let detail = plan_thread_detail_request(
        &linux,
        ThreadDetailRequest {
            id: " thread-a ".to_string(),
            limit: Some(999),
            full: Some(false),
            before: Some("b:120".to_string()),
        },
    )
    .unwrap();
    assert_eq!(detail.thread_id, "thread-a");
    assert_eq!(detail.block_limit, Some(500));
    assert_eq!(detail.before.as_deref(), Some("b:120"));

    let full = plan_thread_detail_request(
        &linux,
        ThreadDetailRequest {
            id: "thread-a".to_string(),
            limit: Some(1),
            full: Some(true),
            before: None,
        },
    )
    .unwrap();
    assert_eq!(full.block_limit, None);

    let blocks = plan_thread_blocks_request(&linux, "thread-a", Some(0), None).unwrap();
    assert_eq!(blocks.block_limit, Some(1));
}

#[test]
fn thread_list_and_blocks_facades_return_adapter_ready_core_plans() {
    let linux = PlatformPaths::linux_server();

    let list = plan_threads_list_request(
        &linux,
        ThreadsQuery {
            archived: Some(false),
            q: Some("  work  ".to_string()),
            limit: Some(25),
        },
    )
    .unwrap();
    assert_eq!(list.fetch_limit, 25);
    assert_eq!(list.query.q.as_deref(), Some("work"));

    let home = temp_dir("agenthub-facade-blocks");
    std::fs::create_dir_all(&home).unwrap();
    let rollout = home.join("thread-a.jsonl");
    std::fs::write(
        &rollout,
        concat!(
            "{\"session_meta\":{\"payload\":{\"id\":\"thread-a\"}}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"text\":\"block 0\"}]}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"text\":\"block 1\"}]}}\n"
        ),
    )
    .unwrap();
    let conn = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, preview)
         VALUES('thread-a', ?1, 1, 2, 'codex', '', '/tmp', 'Thread thread-a', '', '', '')",
        [rollout.display().to_string()],
    )
    .unwrap();

    let blocks = plan_thread_blocks_request(&linux, " thread-a ", Some(1), None).unwrap();
    let page = read_thread_blocks_page(&CodexPaths::new(&home), &blocks)
        .unwrap()
        .unwrap();
    assert_eq!(page.thread_id, "thread-a");
    assert_eq!(page.blocks.len(), 1);
    assert_eq!(page.total_blocks, 2);
    assert!(page.has_more_blocks);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn thread_limit_helpers_are_shared_core_contracts() {
    let linux = PlatformPaths::linux_server();
    let plan = |limit, full| {
        plan_thread_detail_request(
            &linux,
            ThreadDetailRequest {
                id: "thread-a".to_string(),
                limit,
                full: Some(full),
                before: None,
            },
        )
        .unwrap()
        .block_limit
    };
    assert_eq!(plan(None, false), Some(120));
    assert_eq!(plan(Some(999), false), Some(500));
    assert_eq!(plan(Some(0), false), Some(1));
    assert_eq!(plan(Some(25), true), None);

    assert_eq!(
        plan_thread_blocks_request(&linux, "thread-a", None, None)
            .unwrap()
            .block_limit,
        Some(120)
    );
    assert_eq!(
        plan_thread_blocks_request(&linux, "thread-a", Some(0), None)
            .unwrap()
            .block_limit,
        Some(1)
    );
    assert_eq!(
        plan_thread_blocks_request(&linux, "thread-a", Some(999), None)
            .unwrap()
            .block_limit,
        Some(500)
    );
}

#[test]
fn cleanup_and_thread_action_response_commands_use_unified_dot_contracts() {
    let archive_plan = plan_thread_archive_with_capability("thread-a").unwrap();
    assert_eq!(
        thread_state_action_response(&archive_plan).unwrap().command,
        commands::THREADS_ARCHIVE
    );

    let restore_plan = plan_thread_restore_with_capability("thread-a").unwrap();
    assert_eq!(
        thread_state_action_response(&restore_plan).unwrap().command,
        commands::THREADS_RESTORE
    );

    let rename_plan = plan_thread_rename_with_capability(ThreadRenameRequest {
        thread_id: "thread-a".to_string(),
        name: " new name ".to_string(),
    })
    .unwrap();
    let renamed = thread_state_action_response(&rename_plan).unwrap();
    assert_eq!(renamed.command, commands::THREADS_RENAME);
    assert_eq!(renamed.data, Some(serde_json::json!({"name": "new name"})));
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
