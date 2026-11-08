use agenthub_core::codex::{MessageBlock, ThreadDetail, ThreadStatus, ThreadSummary};
use agenthub_core::{
    config::Config,
    platform::{PlatformKind, PlatformPaths},
    services::{
        commands,
        jobs::{
            archive_thread_response, plan_thread_archive_with_capability,
            plan_thread_rename_with_capability, plan_thread_restore_with_capability,
            rename_thread_response, thread_state_action_response, ThreadRenameRequest,
        },
        probe::{plan_probe_action, ProbeAction, ProbeUseCases},
        settings::{
            ProbeNotificationsSavePatch, ProbeSecretState, ProbeSettingsSavePatch,
            ProbeSettingsSaveRequest, SettingsUseCases,
        },
        threads::{
            normalize_thread_block_limit, normalize_thread_detail_block_limit,
            plan_thread_blocks_request, plan_thread_detail_request, plan_threads_list_request,
            thread_blocks_page_for_plan, ThreadDetailRequest, ThreadsQuery,
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
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);

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
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);

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
fn probe_action_capability_gate_rejects_unsupported_platforms() {
    let platform = PlatformPaths::for_kind(PlatformKind::Windows);

    let err = plan_probe_action(&platform, ProbeAction::InstallHooks, &platform.config_file)
        .expect_err("Windows must not expose Probe actions");

    assert!(err.to_string().contains("probe is unavailable on windows"));
}

#[test]
fn update_action_facade_plans_linux_jobs_and_macos_native_updates_but_not_prune() {
    let linux = PlatformPaths::for_kind(PlatformKind::Linux);
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
    let linux_config = Config::for_platform_kind(PlatformKind::Linux);
    let linux = PlatformPaths::for_kind(PlatformKind::Linux);

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
fn thread_detail_cleanup_facades_are_capability_gated() {
    let linux = PlatformPaths::for_kind(PlatformKind::Linux);
    let windows = PlatformPaths::for_kind(PlatformKind::Windows);

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

    assert!(plan_thread_detail_request(
        &windows,
        ThreadDetailRequest {
            id: "thread-a".into(),
            ..ThreadDetailRequest::default()
        }
    )
    .is_err());
}

#[test]
fn thread_list_and_blocks_facades_return_adapter_ready_core_plans() {
    let linux = PlatformPaths::for_kind(PlatformKind::Linux);
    let windows = PlatformPaths::for_kind(PlatformKind::Windows);

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

    let blocks = plan_thread_blocks_request(&linux, " thread-a ", Some(1), None).unwrap();
    let page = thread_blocks_page_for_plan(thread_detail_with_blocks("thread-a", 2), &blocks);
    assert_eq!(page.thread_id, "thread-a");
    assert_eq!(page.blocks.len(), 1);
    assert_eq!(page.total_blocks, 2);
    assert!(page.has_more_blocks);

    assert!(plan_threads_list_request(&windows, ThreadsQuery::default()).is_err());
}

#[test]
fn thread_limit_helpers_are_shared_core_contracts() {
    assert_eq!(normalize_thread_detail_block_limit(None, false), Some(120));
    assert_eq!(
        normalize_thread_detail_block_limit(Some(999), false),
        Some(500)
    );
    assert_eq!(normalize_thread_detail_block_limit(Some(0), false), Some(1));
    assert_eq!(normalize_thread_detail_block_limit(Some(25), true), None);

    assert_eq!(normalize_thread_block_limit(None), 120);
    assert_eq!(normalize_thread_block_limit(Some(0)), 1);
    assert_eq!(normalize_thread_block_limit(Some(999)), 500);
}

#[test]
fn cleanup_and_thread_action_response_commands_use_unified_dot_contracts() {
    let archived = archive_thread_response("thread-a".to_string(), true);
    assert_eq!(archived.command, commands::THREADS_ARCHIVE);
    let archive_plan = plan_thread_archive_with_capability(
        &PlatformPaths::for_kind(PlatformKind::Linux),
        "thread-a",
    )
    .unwrap();
    assert_eq!(
        thread_state_action_response(&archive_plan).unwrap().command,
        commands::THREADS_ARCHIVE
    );

    let restored = archive_thread_response("thread-a".to_string(), false);
    assert_eq!(restored.command, commands::THREADS_RESTORE);
    let restore_plan = plan_thread_restore_with_capability(
        &PlatformPaths::for_kind(PlatformKind::Linux),
        "thread-a",
    )
    .unwrap();
    assert_eq!(
        thread_state_action_response(&restore_plan).unwrap().command,
        commands::THREADS_RESTORE
    );

    let renamed = rename_thread_response("thread-a".to_string(), "new name").unwrap();
    assert_eq!(renamed.command, commands::THREADS_RENAME);
    let rename_plan = plan_thread_rename_with_capability(
        &PlatformPaths::for_kind(PlatformKind::Linux),
        ThreadRenameRequest {
            thread_id: "thread-a".to_string(),
            name: " new name ".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        thread_state_action_response(&rename_plan).unwrap().data,
        Some(serde_json::json!({"name": "new name"}))
    );
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

fn thread_detail_with_blocks(thread_id: &str, block_count: usize) -> ThreadDetail {
    let blocks = (0..block_count)
        .map(|idx| MessageBlock {
            subagent: None,
            user_message: None,
            id: format!("b:{idx}"),
            role: "assistant".to_string(),
            kind: "message".to_string(),
            display_kind: None,
            status: None,
            text: Some(format!("block {idx}")),
            summary: None,
            input: None,
            truncated: None,
            resolved: None,
            answers: Vec::new(),
            plan_status: None,
            group_id: None,
            tool_name: None,
            call_id: None,
            turn_id: None,
            item_id: None,
            created_at: None,
            questions: Vec::new(),
            payload: None,
        })
        .collect::<Vec<_>>();

    ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: thread_id.to_string(),
            title: format!("Thread {thread_id}"),
            status: ThreadStatus::Recent,
            updated_at: None,
            archived_at: None,
            message_count: 1,
            latest_message: None,
            cwd: None,
            model: None,
            rollout_path: None,
            active_turn_id: None,
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: None,
        },
        messages: Vec::new(),
        blocks,
        raw_event_count: block_count,
        total_blocks: block_count,
        has_more_blocks: false,
        before_cursor: None,
    }
}
