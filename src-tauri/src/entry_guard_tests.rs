#[cfg(test)]
mod tests {
    fn production_lib_source() -> &'static str {
        include_str!("lib.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("lib source must include production section")
    }

    fn registered_invoke_command_paths() -> Vec<String> {
        let production_source = production_lib_source();
        let marker = "tauri::generate_handler![";
        let start = production_source
            .find(marker)
            .expect("lib source must include tauri generate_handler")
            + marker.len();
        let body = production_source[start..]
            .split_once(']')
            .map(|(commands, _)| commands)
            .expect("generate_handler block must close");
        // Inspect every registered path, including commands outside the older
        // commands/remote modules, so new host-only modules cannot escape this guard.
        body.split(',')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn command_path(module: &str, name: &str) -> String {
        format!("commands::{module}::{name}")
    }

    fn contract_actions() -> Vec<serde_json::Value> {
        let raw = include_str!("../../contracts/agenthub-contract.json");
        let value: serde_json::Value =
            serde_json::from_str(raw).expect("contract registry must be valid JSON");
        value
            .get("actions")
            .and_then(serde_json::Value::as_array)
            .expect("contract actions must be an array")
            .clone()
    }

    fn retired_compat_path(module: &str, stem: &str) -> String {
        command_path(module, &format!("{stem}_{}", "command"))
    }

    fn concat_token(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn tauri_commands_stay_in_domain_modules() {
        let lib_source = include_str!("lib.rs");
        for domain in [
            "threads", "jobs", "settings", "system", "probe", "updates", "grok",
        ] {
            assert!(
                lib_source.contains(&format!("commands::{domain}::")),
                "Tauri invoke handler must register {domain} commands through commands/{domain}.rs"
            );
        }
        for forbidden in [
            "\nfn desktop_",
            "\nasync fn desktop_",
            "\npub fn desktop_",
            "\npub async fn desktop_",
        ] {
            assert!(
                !lib_source.contains(forbidden),
                "desktop command wrappers must live in src-tauri/src/commands/*, not lib.rs"
            );
        }
    }

    #[test]
    fn tauri_entry_delegates_resources_and_boot_to_modules() {
        let lib_source = production_lib_source();
        let resources_source = include_str!("resources.rs");
        let overview_source = include_str!("overview.rs");
        let boot_source = include_str!("desktop_boot.rs");

        for required in [
            "resources::sync_agenthub_webd_helper_from_resource(&resource_dir)",
            "DesktopState::current()",
            "desktop_boot::initialize_main_window(&window)",
        ] {
            assert!(
                lib_source.contains(required),
                "lib.rs must compose startup helpers through thin modules: {required}"
            );
        }
        let monitor_restart = overview_source
            .find("crate::resources::repair_probe_error_monitor_launch_agent(")
            .expect("desktop startup must restart the monitor through resources.rs");
        let database_open = overview_source
            .find("let db = open_panel_db(&config)?;")
            .expect("desktop startup must open the configured database");
        assert!(
            monitor_restart < database_open,
            "the upgraded monitor must replace the old writer before queue migration"
        );
        for forbidden in [
            "fn sync_agenthub_webd_helper_file",
            "fn sync_directory",
            "fn migrate_desktop_webui_dir_config",
            "fn reveal_main_window",
            "fn fit_main_window_to_work_area",
            "fn schedule_delayed_main_window_reveal",
            "fn schedule_desktop_boot_probe",
            "const DESKTOP_BOOT_PROBE_SCRIPT",
        ] {
            assert!(
                !lib_source.contains(forbidden),
                "lib.rs must not own resource or boot helper implementation: {forbidden}"
            );
        }
        assert!(
            resources_source.contains("fn sync_agenthub_webd_helper_file")
                && resources_source
                    .contains("pub(crate) fn sync_agenthub_webd_helper_from_resource"),
            "resources.rs must own helper resource sync implementation"
        );
        assert!(
            boot_source.contains("fn fit_main_window_to_work_area")
                && boot_source.contains("pub(crate) fn initialize_main_window")
                && boot_source.contains("pub(crate) fn reveal_main_window"),
            "desktop_boot.rs must own one-time window initialization and reveal implementation"
        );
    }

    #[test]
    fn tauri_invoke_handler_excludes_retired_desktop_command_compat_wrappers() {
        let commands = registered_invoke_command_paths();
        for command in &commands {
            let Some(name) = command.rsplit("::").next() else {
                continue;
            };
            assert!(
                !(name.starts_with("desktop_") && name.ends_with("_command")),
                "desktop_*_command compatibility command must not be registered: {command}"
            );
        }
        for retired in [
            command_path("settings", "startProbeJob"),
            command_path("updates", "runUpdateAction"),
            command_path("updates", "updatesPrune"),
            command_path("system", "getDesktopOverview"),
            command_path("system", "getDesktopHome"),
            command_path("system", "getDesktopPlatformStatus"),
            command_path("system", "getDesktopClaudeCodeOverview"),
        ] {
            assert!(
                !commands.contains(&retired),
                "retired or Linux-only update command must not be registered: {retired}"
            );
        }
        for (module, stem) in [
            ("threads", "desktop_threads"),
            ("threads", "desktop_thread_detail"),
            ("probe", "desktop_probe_status"),
            ("settings", "desktop_archive_plan"),
            ("settings", "desktop_hidden_plan"),
            ("settings", "desktop_open_config_dir"),
            ("settings", "desktop_open_log_dir"),
            ("settings", "desktop_save_goal"),
            ("settings", "desktop_clear_goal"),
            ("settings", "desktop_pause_goal"),
            ("settings", "desktop_resume_goal"),
            ("settings", "desktop_upload_files"),
        ] {
            let retired = retired_compat_path(module, stem);
            assert!(
                !commands.contains(&retired),
                "unused desktop compatibility command must not be registered: {retired}"
            );
        }
    }

    #[test]
    fn tauri_invoke_handler_matches_contract_registry() {
        let registered = registered_invoke_command_paths()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let expected = contract_actions()
            .into_iter()
            .filter_map(|action| {
                action
                    .get("tauriCommand")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            })
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            registered, expected,
            "Tauri invoke registration must be declared by contracts/agenthub-contract.json"
        );
    }

    #[test]
    fn tauri_invoke_handler_keeps_desktop_compat_out_of_frontend_workflows() {
        let commands = registered_invoke_command_paths();
        for typed in [
            command_path("system", "getSystemCapabilities"),
            command_path("system", "getSystemVersion"),
            command_path("system", "listProviders"),
            command_path("system", "getPlatformOverview"),
            command_path("threads", "listThreads"),
            command_path("threads", "getThread"),
            command_path("threads", "getThreadBlocks"),
            command_path("threads", "archiveThread"),
            command_path("threads", "restoreThread"),
            command_path("threads", "renameThread"),
            command_path("probe", "getProbeStatus"),
            command_path("updates", "getUpdateStatus"),
            command_path("updates", "updatesCheck"),
            command_path("updates", "updatesInstall"),
            command_path("settings", "getProbeSettings"),
            command_path("settings", "saveProbeSettings"),
            command_path("settings", "getProbeEvents"),
            command_path("settings", "probeBarkTest"),
            command_path("settings", "probeInstallHooks"),
            command_path("settings", "probeRemoveHooks"),
            command_path("settings", "dryRunArchiveDelete"),
            command_path("settings", "startArchiveDelete"),
            command_path("settings", "dryRunHiddenThreadDelete"),
            command_path("settings", "startHiddenThreadDelete"),
            command_path("jobs", "listJobs"),
            command_path("jobs", "getJob"),
            command_path("grok", "listGrokSessions"),
            command_path("grok", "getGrokSession"),
            command_path("grok", "renameGrokSession"),
            command_path("grok", "previewGrokSessionDelete"),
            command_path("grok", "deleteGrokSession"),
        ] {
            assert!(
                commands.contains(&typed),
                "typed desktop command must be registered: {typed}"
            );
        }

        for command in &commands {
            let Some(name) = command.rsplit("::").next() else {
                continue;
            };
            assert!(
                !name.starts_with("desktop_"),
                "frontend workflow must use typed command registration instead of desktop_* compat: {command}"
            );
        }
    }

    #[test]
    fn tauri_invoke_handler_registers_only_typed_probe_and_update_commands() {
        let commands = registered_invoke_command_paths();
        for legacy in [
            command_path("updates", "checkUpdate"),
            command_path("updates", "installUpdateAndRestart"),
            command_path("settings", "startProbeBarkTest"),
            command_path("settings", "startProbeHooksInstall"),
            command_path("settings", "startProbeLogsDbDryRun"),
            command_path("settings", "startProbeLogsDbExecute"),
            command_path("system", "getDesktopOverview"),
            command_path("system", "getDesktopHome"),
            command_path("system", "getDesktopPlatformStatus"),
            command_path("system", "getDesktopClaudeCodeOverview"),
        ] {
            assert!(
                !commands.contains(&legacy),
                "legacy WebUI compatibility command must not be registered in Tauri: {legacy}"
            );
        }
    }

    #[test]
    fn tauri_command_modules_do_not_define_legacy_probe_or_update_wrappers() {
        for (source, legacy) in [
            (
                include_str!("commands/updates.rs"),
                "pub async fn checkUpdate",
            ),
            (
                include_str!("commands/updates.rs"),
                "pub async fn installUpdateAndRestart",
            ),
            (
                include_str!("commands/settings.rs"),
                "pub fn startProbeBarkTest",
            ),
            (
                include_str!("commands/settings.rs"),
                "pub fn startProbeHooksInstall",
            ),
            (
                include_str!("commands/settings.rs"),
                "pub fn startProbeLogsDbDryRun",
            ),
            (
                include_str!("commands/settings.rs"),
                "pub fn startProbeLogsDbExecute",
            ),
            (
                include_str!("commands/system.rs"),
                "pub fn getDesktopOverview",
            ),
            (
                include_str!("commands/system.rs"),
                "pub async fn getDesktopHome",
            ),
            (
                include_str!("commands/system.rs"),
                "pub async fn getDesktopPlatformStatus",
            ),
            (
                include_str!("commands/system.rs"),
                "pub fn getDesktopClaudeCodeOverview",
            ),
        ] {
            assert!(
                !source.contains(legacy),
                "legacy Tauri command wrapper must not be defined: {legacy}"
            );
        }
    }

    #[test]
    fn tauri_update_commands_do_not_plan_linux_prune_actions() {
        let source = include_str!("commands/updates.rs");
        assert!(
            !source.contains("UpdateAction::Prune"),
            "macOS Tauri update commands must not expose Linux update prune"
        );
    }

    #[test]
    fn tauri_cleanup_execute_commands_require_confirmation_payload() {
        let source = include_str!("commands/settings.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("settings command source must include production section");

        for command in ["startArchiveDelete", "startHiddenThreadDelete"] {
            let start = source
                .find(&format!("pub fn {command}("))
                .or_else(|| source.find(&format!("pub async fn {command}(")))
                .unwrap_or_else(|| panic!("cleanup execute command must exist: {command}"));
            let body = &source[start..];
            let signature = body.split(") ->").next().unwrap_or_else(|| {
                panic!("cleanup execute command signature must close: {command}")
            });
            assert!(
                signature.contains("request:") || signature.contains("payload:"),
                "cleanup execute command must accept a confirmation payload: {command}"
            );
            assert!(
                signature.contains("DesktopCleanupExecuteRequest"),
                "cleanup execute command must use the typed cleanup confirmation payload: {command}"
            );
        }
    }

    #[test]
    fn tauri_cleanup_service_is_native_effect_executor_only() {
        let source = include_str!("services/settings.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("settings service source must include production section");

        assert!(
            source.contains(
                "type DesktopCleanupExecuteRequest = cleanup_service::CleanupExecuteRequest"
            ),
            "Tauri cleanup execute payload must reuse the shared core confirmation request"
        );
        assert!(
            source.contains("AgentHubUseCases::new(state.platform()).cleanup()")
                && source.contains(".execute_confirmed(")
                && source.contains(".dry_run_archived(")
                && source.contains(".execute_archived(")
                && source.contains(".dry_run_hidden(")
                && source.contains(".execute_hidden("),
            "Tauri cleanup service must consume the core cleanup use-case facade before native delete effects"
        );
        for forbidden in [
            "cleanup_service::plan_cleanup_execute_operation",
            "cleanup_service::dry_run_archived_with_capability(",
            "cleanup_service::execute_archived_with_capability(",
            "cleanup_service::dry_run_hidden_with_capability(",
            "cleanup_service::execute_hidden_with_capability(",
            "cleanup_service::validate_cleanup_expected_count(",
            "ARCHIVE_DELETE_CONFIRMATION_MESSAGE",
            "HIDDEN_DELETE_CONFIRMATION_MESSAGE",
            "CLEANUP_EXPECTED_COUNT_REQUIRED_MESSAGE",
            "archive deletion must be confirmed",
            "hidden thread deletion must be confirmed",
            "expectedCount mismatch",
            "fn ensure_cleanup_expected_count",
        ] {
            assert!(
                !source.contains(forbidden),
                "Tauri cleanup service must not define cleanup business semantic token: {forbidden}"
            );
        }
    }

    #[test]
    fn overview_only_keeps_desktop_state_home_and_startup_types() {
        let overview_source = include_str!("overview.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("overview source must include production section");

        for forbidden in [
            "pub struct DesktopActionResponse",
            "pub struct DesktopThreadBlockPage",
            "pub struct DesktopProbeSettings",
            "pub struct DesktopJobResponse",
            "pub struct DesktopProbeEventsResponse",
            "pub struct DesktopDeleteUploadResponse",
            "pub struct DesktopUploadFile",
            "pub struct ThreadListRequest",
            "pub struct ThreadDetailRequest",
            "pub struct ThreadBlocksRequest",
            "pub struct DesktopSendMessageRequest",
            "pub struct DesktopStopRequest",
            "pub struct DesktopThreadIdRequest",
            "pub struct DesktopRenameThreadRequest",
            "pub struct DesktopPlanAcceptRequest",
            "pub struct DesktopPlanReviseRequest",
            "pub struct DesktopElicitationAnswerRequest",
            "pub struct DesktopJobsRequest",
            "pub struct DesktopJobDetailRequest",
            "pub struct DesktopDeleteUploadRequest",
            "pub struct DesktopFollowupRequest",
            "pub struct DesktopCancelFollowupRequest",
            "DesktopGoal",
            "ProbeRuntime",
            "ProbeStatus",
            "SystemCapabilitiesResponse",
            "ArchiveDeletePlan",
            "HiddenThreadDeletePlan",
            "first_thread_goal",
            "ThreadSummary",
            "home_thread_summaries",
        ] {
            assert!(
                !overview_source.contains(forbidden),
                "overview.rs must not define command adapter DTO: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_thread_commands_use_core_thread_query_and_detail_plans() {
        let thread_commands_source = include_str!("commands/threads.rs")
            .split("\n#[cfg(test)]")
            .next()
            .unwrap_or(include_str!("commands/threads.rs"));
        let threads_source = include_str!("services/threads.rs")
            .split("\n#[cfg(test)]")
            .next()
            .unwrap_or(include_str!("services/threads.rs"));

        for required in [
            "thread_summaries_with_query(",
            ".list_read(query)?",
            ".detail_read(",
            ".blocks_read(",
            "thread_detail_read_model",
            "window_thread_detail_for_plan",
            "thread_blocks_page_for_plan",
            "AgentHubUseCases::new(state.platform()).threads()",
        ] {
            assert!(
                threads_source.contains(required),
                "Tauri thread adapter must consume shared core plan: {required}"
            );
        }

        for forbidden in [
            "fn thread_list_with_jobs(",
            "window_thread_detail(",
            "detail_block_limit(",
            "block_page_limit(",
            "thread_service::normalize_thread_detail_block_limit",
            "thread_service::normalize_thread_block_limit",
        ] {
            assert!(
                !threads_source.contains(forbidden),
                "Tauri thread adapter must not duplicate core thread paging logic: {forbidden}"
            );
        }

        assert!(
            thread_commands_source.contains("thread_service::threads_with_state")
                && !thread_commands_source.contains("thread_service::send_message_with_state")
                && !thread_commands_source.contains("state.db.")
                && !thread_commands_source.contains("state.jobs."),
            "Tauri thread commands must stay thin and delegate to services/threads.rs"
        );
        assert!(
            threads_source.contains(".list_read(query)?")
                && threads_source.contains("thread_service::thread_list_read_model")
                && threads_source.contains("thread_service::thread_detail_read_model")
                && !threads_source.contains("thread_service::build_threads_overview")
                && !threads_source.contains("thread_service::apply_running_job_to_summary"),
            "desktop thread service must consume shared core read-model plans"
        );
    }

    #[test]
    fn tauri_thread_job_submission_is_retired() {
        let source = [
            production_lib_source(),
            include_str!("commands/threads.rs"),
            include_str!("commands/settings.rs"),
            include_str!("services/threads.rs"),
            include_str!("services/settings.rs"),
        ]
        .join("\n");
        for forbidden in [
            "commands::threads::createThread",
            "commands::threads::sendMessage",
            "commands::threads::stopThread",
            "commands::threads::forkThread",
            "commands::settings::saveCodexGoal",
        ] {
            assert!(
                !source.contains(forbidden),
                "retired execution must not be reachable: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_settings_commands_use_core_settings_view_and_secret_write_plans() {
        let settings_commands_source = include_str!("commands/settings.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("settings command source must include production section");
        let settings_source = include_str!("services/settings.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("settings service source must include production section");

        for required in [
            "AgentHubUseCases::with_config",
            ".settings()?",
            ".probe_settings_view(",
            ".save_probe_settings(",
            "for secret_write in plan.secret_writes",
        ] {
            assert!(
                settings_source.contains(required),
                "Tauri settings adapter must consume shared core settings facade: {required}"
            );
        }

        assert!(
            settings_commands_source.contains("settings_service::probe_settings_with_state")
                && !settings_commands_source.contains("goal_service::save_goal_with_state")
                && !settings_commands_source.contains("state.db.")
                && !settings_commands_source.contains("plan_probe_settings_save"),
            "Tauri settings commands must stay thin and delegate to native services"
        );
        assert!(
            !settings_source.contains("if let Some(device_key) = plan.bark_device_key"),
            "Tauri settings adapter must not special-case Probe secret writes outside the core plan"
        );
    }

    #[test]
    fn tauri_settings_service_is_retired() {
        let source = [
            production_lib_source(),
            include_str!("commands/threads.rs"),
            include_str!("commands/settings.rs"),
            include_str!("services/threads.rs"),
            include_str!("services/settings.rs"),
        ]
        .join("\n");
        for forbidden in ["goal_service", "GoalRequest", "execute_goal_command"] {
            assert!(
                !source.contains(forbidden),
                "retired execution must not be reachable: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_probe_status_uses_core_probe_use_case_facade() {
        let probe_source = include_str!("services/probe.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or(include_str!("services/probe.rs"));

        assert!(
            probe_source.contains("require_capability(state.platform(), Capability::Probe)?")
                && probe_source.contains("ProbeRuntime::new(config, state.platform().clone())"),
            "Tauri probe status must derive read-model buckets through single-pass ProbeRuntime"
        );
        for forbidden in [
            "status.running_threads =",
            "status.reply_needed_threads =",
            "status.recoverable_threads =",
            "status.running_count =",
            "status.reply_needed_count =",
            "status.recoverable_count =",
        ] {
            assert!(
                !probe_source.contains(forbidden),
                "Tauri probe status must not assign read-model fields outside core helper: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_probe_actions_use_core_probe_use_case_facade() {
        let settings_source = include_str!("services/settings.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("settings service source must include production section");

        for required in [
            "AgentHubUseCases::with_config",
            ".probe()?",
            ".action_with_device_key(",
            "probe_fixed_shell_job_with_state(state, action, plan)",
        ] {
            assert!(
                settings_source.contains(required),
                "Tauri Probe actions must consume the shared Probe use-case facade: {required}"
            );
        }
        assert!(
            !settings_source.contains("probe_service::plan_probe_action_with_device_key("),
            "Tauri Probe action service must not bypass the shared Probe use-case facade"
        );
    }

    #[test]
    fn tauri_system_commands_delegate_to_native_service_layer() {
        let system_commands_source = include_str!("commands/system.rs")
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("system command source must include production section");

        assert!(
            system_commands_source.contains("services::system"),
            "commands/system.rs must delegate native work to services/system.rs"
        );
        for forbidden in [
            "agenthub_core::local::local_plugin_catalog",
            "agenthub_core::local::default_codex_models",
            "agenthub_core::local::default_permission_profiles",
            "agenthub_core::local::local_codex_config",
        ] {
            assert!(
                !system_commands_source.contains(forbidden),
                "commands/system.rs must stay thin and not execute native system logic: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_retains_cleanup_and_archive_without_goal_or_followup_execution() {
        let settings = include_str!("services/settings.rs");
        let threads = include_str!("services/threads.rs");
        for required in [
            ".dry_run_archived(",
            ".execute_archived(",
            ".dry_run_hidden(",
            ".execute_hidden(",
            ".execute_confirmed(",
        ] {
            assert!(
                settings.contains(required),
                "cleanup safety must remain: {required}"
            );
        }
        for required in [".archive(", ".restore(", ".rename("] {
            assert!(
                threads.contains(required),
                "native task management must remain: {required}"
            );
        }
        for forbidden in [
            ".uploads()",
            ".goals()",
            ".apply_enqueue_followup(",
            ".apply_cancel_followup(",
            ".resolve_stop(",
            "execute_goal_command",
        ] {
            assert!(
                !settings.contains(forbidden) && !threads.contains(forbidden),
                "retired task mutation: {forbidden}"
            );
        }
        assert!(!std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/services/goals.rs")
            .exists());
    }

    #[test]
    fn tauri_thread_approval_is_retired() {
        let source = [
            production_lib_source(),
            include_str!("commands/threads.rs"),
            include_str!("commands/settings.rs"),
            include_str!("services/threads.rs"),
            include_str!("services/settings.rs"),
        ]
        .join("\n");
        for forbidden in [
            "approve_plan",
            "answer_questions",
            "fork_thread",
            "stop_thread",
        ] {
            assert!(
                !source.contains(forbidden),
                "retired execution must not be reachable: {forbidden}"
            );
        }
    }

    #[test]
    fn overview_does_not_export_desktop_business_helper_functions() {
        let overview_source = include_str!("overview.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("overview source must include production section");

        for forbidden in [
            "desktop_threads",
            "desktop_thread_detail",
            "desktop_thread_blocks",
            "desktop_send_message",
            "desktop_continue_thread",
            "desktop_stop_thread",
            "desktop_plan_accept",
            "desktop_plan_revise",
            "desktop_answer_elicitation",
            "desktop_archive_thread",
            "desktop_restore_thread",
            "desktop_rename_thread",
            "desktop_fork_thread",
            "desktop_probe_status",
            "desktop_probe_settings",
            "desktop_probe_save_settings",
            "desktop_probe_bark_test",
            "desktop_probe_hooks_install",
            "desktop_probe_logs_db_maintain",
            "desktop_probe_events",
            "desktop_archive_plan",
            "desktop_hidden_plan",
            "desktop_archive_delete",
            "desktop_hidden_delete",
            "desktop_delete_upload",
            "desktop_store_uploads",
            "desktop_jobs",
            "desktop_job_detail",
            "desktop_list_followups",
            "desktop_enqueue_followup",
            "desktop_cancel_followup",
            "desktop_codex_job_spec",
        ] {
            assert!(
                !overview_source.contains(forbidden),
                "overview.rs must not retain desktop business helper: {forbidden}"
            );
        }
    }

    #[test]
    fn overview_does_not_depend_on_command_modules() {
        let overview_source = include_str!("overview.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("overview source must include production section");

        for forbidden in [
            "use crate::commands::",
            "crate::commands::",
            "commands::settings::DesktopGoal",
            "commands::threads::threads_for_home",
            "commands::settings::first_thread_goal",
        ] {
            assert!(
                !overview_source.contains(forbidden),
                "overview.rs must not depend on command adapters: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_command_modules_remain_thin_typed_adapters() {
        for (module, source) in [
            ("threads", include_str!("commands/threads.rs")),
            ("settings", include_str!("commands/settings.rs")),
            ("updates", include_str!("commands/updates.rs")),
            ("jobs", include_str!("commands/jobs.rs")),
            ("probe", include_str!("commands/probe.rs")),
        ] {
            for forbidden in [
                "state.db.",
                "state.jobs.",
                "set_thread_archived",
                "set_thread_title",
                "thread_detail(",
                "patch_probe_config_toml",
                "std::fs::write",
                "updater_builder",
                "create_job(",
                "append_job_output(",
                "finish_job(",
                "running_job_for_thread",
                "ok_or_else",
                "approval actions are unavailable",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "commands/{module}.rs must stay a thin typed adapter and not contain {forbidden}"
                );
            }
        }
    }

    #[test]
    fn tauri_goal_commands_is_retired() {
        let source = [
            production_lib_source(),
            include_str!("commands/threads.rs"),
            include_str!("commands/settings.rs"),
            include_str!("services/threads.rs"),
            include_str!("services/settings.rs"),
        ]
        .join("\n");
        for forbidden in [
            "goal_get",
            "goal_save",
            "goal_pause",
            "goal_resume",
            "goal_clear",
        ] {
            assert!(
                !source.contains(forbidden),
                "retired execution must not be reachable: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_command_guard_does_not_embed_retired_compat_tokens_in_tests() {
        let test_source = include_str!("lib.rs")
            .split("\n#[cfg(test)]")
            .nth(1)
            .expect("lib source must include test section");
        for retired in [
            command_path("settings", "startProbeJob"),
            command_path("updates", "runUpdateAction"),
            command_path("updates", "updatesPrune"),
            command_path("updates", "pruneBackups"),
        ] {
            assert!(
                !test_source.contains(&retired),
                "tests must not embed retired string action command token: {retired}"
            );
        }
        for (module, stem) in [
            ("threads", "desktop_threads"),
            ("threads", "desktop_thread_detail"),
            ("probe", "desktop_probe_status"),
            ("settings", "desktop_archive_plan"),
            ("settings", "desktop_hidden_plan"),
            ("settings", "desktop_open_config_dir"),
            ("settings", "desktop_open_log_dir"),
            ("settings", "desktop_save_goal"),
            ("settings", "desktop_clear_goal"),
            ("settings", "desktop_pause_goal"),
            ("settings", "desktop_resume_goal"),
            ("settings", "desktop_upload_files"),
        ] {
            let retired = retired_compat_path(module, stem);
            assert!(
                !test_source.contains(&retired),
                "tests must not embed retired compatibility command token: {retired}"
            );
        }
    }

    #[test]
    fn macos_tauri_sources_do_not_suggest_linux_host_repair_steps() {
        for (label, source) in [
            ("commands/updates.rs", include_str!("commands/updates.rs")),
            ("services/updates.rs", include_str!("services/updates.rs")),
            ("commands/settings.rs", include_str!("commands/settings.rs")),
            ("services/settings.rs", include_str!("services/settings.rs")),
        ] {
            for forbidden in [
                "systemctl",
                "systemd",
                "Nginx",
                "nginx",
                "sudo ",
                "/opt/agenthub",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "macOS Tauri source must not suggest Linux host repair step {forbidden} in {label}"
                );
            }
        }
    }

    #[test]
    fn tauri_invoke_handler_excludes_linux_web_host_command_surfaces() {
        let commands = registered_invoke_command_paths();
        for parts in [
            &["get", "Security"][..],
            &["save", "Security"][..],
            &["security", "Status"][..],
            &["change", "Password"][..],
            &["security", "_status"][..],
            &["auth", "Status"][..],
            &["log", "in"][..],
            &["log", "out"][..],
            &["cs", "rf"][..],
            &["turn", "stile"][..],
            &["admin", "_password"][..],
            &["system", "d"][..],
            &["System", "d"][..],
            &["ngi", "nx"][..],
            &["Nginx"][..],
            &["web", "Auth"][..],
            &["web", "auth"][..],
            &["system_update", "_prune"][..],
            &["desktop_update", "_prune"][..],
            &["prune", "_backups"][..],
            &["Probe", "Job"][..],
            &["run", "UpdateAction"][..],
        ] {
            let forbidden = concat_token(parts);
            assert!(
                commands.iter().all(|command| !command.contains(&forbidden)),
                "macOS desktop invoke handler must not register Linux Web host command surface: {forbidden}"
            );
        }
    }

    #[test]
    fn tauri_shell_injects_desktop_runtime_marker_before_webui_bootstrap() {
        let lib_source = production_lib_source();
        let boot_source = include_str!("desktop_boot.rs");

        assert!(
            boot_source.contains("__AGENTHUB_DESKTOP_RUNTIME__"),
            "Tauri must inject a desktop runtime marker before the WebUI bootstraps so macOS does not render the Web login gate"
        );
        assert!(
            lib_source.contains(".append_invoke_initialization_script(")
                && lib_source.contains("desktop_boot::DESKTOP_RUNTIME_MARKER_SCRIPT"),
            "Tauri must register the marker through an initialization script that runs before the bundled WebUI"
        );
    }

    #[test]
    fn macos_shell_creates_and_reveals_main_window_explicitly() {
        let lib_source = production_lib_source();
        let boot_source = include_str!("desktop_boot.rs");
        let config = include_str!("../tauri.conf.json");

        for required in [
            r#""width": 1280"#,
            r#""height": 820"#,
            r#""minWidth": 1000"#,
            r#""minHeight": 680"#,
            r#""maximized": false"#,
            r#""fullscreen": false"#,
            r#""visible": false"#,
        ] {
            assert!(
                config.contains(required),
                "main Tauri window config must start hidden and unmaximized before one-time geometry initialization: {required}"
            );
        }
        assert!(
            config.contains(r#""create": false"#),
            "Tauri must not rely on implicit tauri.conf window creation for the macOS shell"
        );
        assert!(
            lib_source.contains("WebviewWindowBuilder::from_config"),
            "Tauri must explicitly build the main WebView window after desktop resources are prepared"
        );
        assert!(
            lib_source.contains("RunEvent::Ready"),
            "Tauri must re-show and focus the main window once the event loop is ready"
        );
        let (setup_and_run_source, run_source) = lib_source
            .split_once(".run(|app, event|")
            .expect("Tauri run loop must own Ready and Reopen window handling");
        let setup_source = setup_and_run_source
            .split_once(".setup(|app| {")
            .map(|(_, source)| source)
            .expect("Tauri setup must build and initialize the main window");
        assert!(
            setup_source.contains("desktop_boot::initialize_main_window(&window)"),
            "setup must initialize the hidden window"
        );
        assert!(
            !setup_source.contains("desktop_boot::reveal_main_window(&window)"),
            "setup must not reveal the window before the Ready event"
        );
        assert!(
            run_source.contains("RunEvent::Ready") && run_source.contains("RunEvent::Reopen"),
            "Ready and Reopen must both use the main window lifecycle handler"
        );
        assert_eq!(
            run_source
                .matches("desktop_boot::reveal_main_window(&window)")
                .count(),
            2,
            "Ready and Reopen must each reveal the initialized window once"
        );

        let reveal_source = boot_source
            .split("pub(crate) fn reveal_main_window")
            .nth(1)
            .and_then(|source| source.split("pub(crate) fn initialize_main_window").next())
            .expect("desktop_boot.rs must define reveal_main_window before initialization");
        let show_index = reveal_source
            .find("window.show()")
            .expect("reveal_main_window must show the main window");
        let unminimize_index = reveal_source
            .find("window.unminimize()")
            .expect("reveal_main_window must unminimize the main window");
        let focus_index = reveal_source
            .find("window.set_focus()")
            .expect("reveal_main_window must focus the main window");
        assert!(
            show_index < unminimize_index && unminimize_index < focus_index,
            "reveal_main_window must preserve show -> unminimize -> set_focus order"
        );
        for forbidden in [
            "window.maximize()",
            "fit_main_window_to_work_area(",
            "window.current_monitor()",
        ] {
            assert!(
                !reveal_source.contains(forbidden),
                "reveal_main_window must not reapply startup geometry: {forbidden}"
            );
        }
        for required in [
            "fn fit_main_window_to_work_area",
            "window.current_monitor()",
            "window.primary_monitor()",
            "monitor.work_area()",
            "window.set_position(PhysicalPosition::new(",
            "window.set_size(Size::Physical(PhysicalSize::new(",
        ] {
            assert!(
                boot_source.contains(required),
                "explicit macOS window creation must fall back to the monitor work area when native maximize does not resize the window: {required}"
            );
        }
        let initialize_source = boot_source
            .split("pub(crate) fn initialize_main_window")
            .nth(1)
            .and_then(|source| source.split("\nfn fit_main_window_to_work_area").next())
            .expect("desktop_boot.rs must define initialization before its geometry fallback");
        for required in [
            "window.maximize()",
            "window.is_maximized().unwrap_or(false)",
            "fit_main_window_to_work_area(window)",
        ] {
            assert!(
                initialize_source.contains(required),
                "initialize_main_window must own the one-time geometry adjustment: {required}"
            );
        }
        assert_eq!(
            initialize_source.matches("window.maximize()").count(),
            1,
            "initialize_main_window must maximize only once"
        );
        assert_eq!(
            initialize_source
                .matches("fit_main_window_to_work_area(window)")
                .count(),
            1,
            "initialize_main_window must apply at most one work-area fallback"
        );
        for forbidden in [
            "window.show()",
            "window.unminimize()",
            "window.set_focus()",
            "reveal_main_window(window)",
        ] {
            assert!(
                !initialize_source.contains(forbidden),
                "initialize_main_window must leave reveal to Ready: {forbidden}"
            );
        }
        assert!(
            !boot_source.contains("schedule_delayed_main_window_reveal")
                && !lib_source.contains("schedule_delayed_main_window_reveal"),
            "startup must not schedule a delayed second reveal"
        );
    }

    #[test]
    fn tauri_goal_flat_invoke_abi_is_retired() {
        let source = [
            production_lib_source(),
            include_str!("commands/threads.rs"),
            include_str!("commands/settings.rs"),
            include_str!("services/threads.rs"),
            include_str!("services/settings.rs"),
        ]
        .join("\n");
        for forbidden in [
            "threads.goal.get",
            "threads.goal.save",
            "threads.goal.pause",
            "threads.goal.resume",
            "threads.goal.clear",
        ] {
            assert!(
                !source.contains(forbidden),
                "retired execution must not be reachable: {forbidden}"
            );
        }
    }
}
