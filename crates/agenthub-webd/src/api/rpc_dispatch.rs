use super::ApiErrorKind;
use super::{
    antigravity_archive, antigravity_detail, antigravity_list, antigravity_rename,
    antigravity_restore, claude_delete_execute, claude_delete_preview, claude_detail, claude_list,
    claude_rename, dsh_archive, dsh_detail, dsh_list, dsh_rename, dsh_restore, ClaudeListQuery,
};
use super::{
    api_error, archive_delete_dry_run, archive_delete_execute, archive_thread,
    dsh_archive_delete_dry_run, dsh_archive_delete_execute, get_probe_events, get_probe_settings,
    get_probe_status, grok_delete_execute, grok_delete_preview, grok_detail, grok_list,
    grok_rename, hidden_threads_delete_dry_run, hidden_threads_delete_execute, job_detail,
    list_jobs, list_providers, patch_probe_settings, platform_overview, rename_thread,
    restore_thread, start_probe_action, start_update_action, system_capabilities,
    system_update_status, system_version, thread_blocks, thread_detail, unified_cleanup_dry_run,
    unified_cleanup_execute, ApiResponse, GrokListQuery, ProbeEventsQuery, ProbeStatusQuery,
};
use crate::{
    api::payload::{
        rpc_nested_payload_or_empty, rpc_payload, rpc_query_strings, rpc_required_string,
        rpc_wrapped_payload,
    },
    state::AppState,
};
use agenthub_core::services::{
    commands as rpc_commands,
    probe::{self as probe_service},
    updates::UpdateAction,
};
use serde_json::Value;

pub(crate) async fn rpc_dispatch(state: AppState, command: String, args: Value) -> ApiResponse {
    if !rpc_commands::is_allowed_rpc_command(&command) {
        return Err(api_error(ApiErrorKind::NotFound, "不支持的远程操作"));
    }
    match command.as_str() {
        rpc_commands::THREADS_SUBAGENT_DETAIL => {
            super::threads::subagent_detail(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::SESSIONS_ATTACHMENT_READ => {
            super::sessions::attachment_read(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::SESSIONS_SEARCH => {
            super::sessions::search(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::SESSIONS_BULK_PREVIEW => {
            super::sessions::bulk_preview(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::SESSIONS_BULK_EXECUTE => {
            super::sessions::bulk_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::GROK_LIST => {
            grok_list(
                state,
                GrokListQuery {
                    q: args
                        .get("q")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                },
            )
            .await
        }
        rpc_commands::GROK_DETAIL => {
            let id = rpc_required_string(&args, "id")?;
            grok_detail(
                state,
                id,
                super::grok::GrokDetailQuery {
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize),
                    before: args
                        .get("before")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
            )
            .await
        }
        rpc_commands::GROK_RENAME => grok_rename(state, rpc_payload(&args)?).await,
        rpc_commands::GROK_DELETE_PREVIEW => {
            grok_delete_preview(state, rpc_required_string(&args, "id")?).await
        }
        rpc_commands::GROK_DELETE_EXECUTE => {
            grok_delete_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::CLAUDE_LIST => {
            claude_list(
                state,
                ClaudeListQuery {
                    q: args
                        .get("q")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                },
            )
            .await
        }
        rpc_commands::CLAUDE_DETAIL => claude_detail(state, rpc_payload(&args)?).await,
        rpc_commands::CLAUDE_RENAME => claude_rename(state, rpc_payload(&args)?).await,
        rpc_commands::CLAUDE_DELETE_PREVIEW => {
            let session_key = rpc_required_string(&args, "sessionKey")?;
            claude_delete_preview(state, session_key).await
        }
        rpc_commands::CLAUDE_DELETE_EXECUTE => {
            claude_delete_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::ANTIGRAVITY_LIST => {
            let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
                Default::default()
            } else {
                rpc_wrapped_payload(&args, &["request"])?
            };
            antigravity_list(state, request).await
        }
        rpc_commands::ANTIGRAVITY_DETAIL => {
            antigravity_detail(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::ANTIGRAVITY_RENAME => {
            antigravity_rename(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::ANTIGRAVITY_ARCHIVE => {
            antigravity_archive(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::ANTIGRAVITY_RESTORE => {
            antigravity_restore(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::DSH_LIST => {
            let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
                Default::default()
            } else {
                rpc_wrapped_payload(&args, &["request"])?
            };
            dsh_list(state, request).await
        }
        rpc_commands::DSH_DETAIL => {
            dsh_detail(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::DSH_RENAME => {
            dsh_rename(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::DSH_ARCHIVE => {
            dsh_archive(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::DSH_RESTORE => {
            dsh_restore(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::SYSTEM_PROVIDERS => list_providers(state).await,
        rpc_commands::SYSTEM_PLATFORM => platform_overview(state).await,
        rpc_commands::PROBE_STATUS => {
            get_probe_status(
                state,
                ProbeStatusQuery {
                    refresh: args.get("refresh").and_then(Value::as_bool),
                },
            )
            .await
        }
        rpc_commands::PROBE_SETTINGS_GET => get_probe_settings(state).await,
        rpc_commands::PROBE_SETTINGS_SAVE => {
            patch_probe_settings(
                state,
                rpc_wrapped_payload(&args, &["settings", "payload", "request"])?,
            )
            .await
        }
        rpc_commands::PROBE_EVENTS => {
            get_probe_events(
                state,
                ProbeEventsQuery {
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|value| value as u32),
                },
            )
            .await
        }
        rpc_commands::PROBE_BARK_TEST => {
            start_probe_action(state, probe_service::ProbeAction::BarkTest).await
        }
        rpc_commands::PROBE_INSTALL_HOOKS => {
            start_probe_action(state, probe_service::ProbeAction::InstallHooks).await
        }
        rpc_commands::PROBE_REMOVE_HOOKS => {
            start_probe_action(state, probe_service::ProbeAction::RemoveHooks).await
        }
        rpc_commands::CLEANUP_DSH_ARCHIVE_DRY_RUN => dsh_archive_delete_dry_run(state).await,
        rpc_commands::CLEANUP_DSH_ARCHIVE_EXECUTE => {
            dsh_archive_delete_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::CLEANUP_UNIFIED_DRY_RUN => {
            let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
                Default::default()
            } else {
                rpc_wrapped_payload(&args, &["request"])?
            };
            unified_cleanup_dry_run(state, request).await
        }
        rpc_commands::CLEANUP_UNIFIED_EXECUTE => {
            unified_cleanup_execute(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::CLEANUP_ARCHIVE_DRY_RUN => archive_delete_dry_run(state).await,
        rpc_commands::CLEANUP_ARCHIVE_EXECUTE => {
            archive_delete_execute(state, rpc_wrapped_payload(&args, &["payload", "request"])?)
                .await
        }
        rpc_commands::CLEANUP_HIDDEN_DRY_RUN => {
            hidden_threads_delete_dry_run(state, rpc_wrapped_payload(&args, &["request"])?).await
        }
        rpc_commands::CLEANUP_HIDDEN_EXECUTE => {
            hidden_threads_delete_execute(
                state,
                rpc_wrapped_payload(&args, &["payload", "request"])?,
            )
            .await
        }
        rpc_commands::UPDATES_STATUS => system_update_status(state).await,
        rpc_commands::UPDATES_CHECK => start_update_action(state, UpdateAction::Check, None).await,
        rpc_commands::UPDATES_INSTALL => {
            start_update_action(
                state,
                UpdateAction::Install,
                Some("agenthub.update.install_started"),
            )
            .await
        }
        rpc_commands::UPDATES_PRUNE => {
            start_update_action(
                state,
                UpdateAction::Prune,
                Some("agenthub.update.prune_started"),
            )
            .await
        }
        rpc_commands::THREADS_LIST => super::list_threads(state, rpc_payload(&args)?).await,
        rpc_commands::THREADS_DETAIL => {
            thread_detail(
                state,
                rpc_required_string(&args, "id")?,
                rpc_nested_payload_or_empty(&args, "options")?,
            )
            .await
        }
        rpc_commands::THREADS_BLOCKS => {
            thread_blocks(
                state,
                rpc_required_string(&args, "id")?,
                rpc_nested_payload_or_empty(&args, "options")?,
            )
            .await
        }
        rpc_commands::THREADS_ARCHIVE => {
            archive_thread(state, rpc_required_string(&args, "threadId")?).await
        }
        rpc_commands::THREADS_RESTORE => {
            restore_thread(state, rpc_required_string(&args, "threadId")?).await
        }
        rpc_commands::THREADS_RENAME => {
            rename_thread(
                state,
                rpc_required_string(&args, "threadId")?,
                rpc_payload(&args)?,
            )
            .await
        }
        rpc_commands::SYSTEM_CAPABILITIES => system_capabilities(state).await,
        rpc_commands::SYSTEM_VERSION => system_version(state).await,
        rpc_commands::JOBS_LIST => list_jobs(state, rpc_query_strings(&args, &["limit"])).await,
        rpc_commands::JOBS_DETAIL => {
            job_detail(
                state,
                rpc_required_string(&args, "id")?,
                args.get("includeOutput")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            )
            .await
        }
        _ => Err(api_error(
            ApiErrorKind::NotFound,
            &format!("rpc command allowlist drifted without handler mapping: {command}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;
    fn state() -> AppState {
        let (config, root) = super::super::test_support::sandboxed_test_config();
        AppState::new(
            config,
            agenthub_core::db::PanelDb::open(":memory:").unwrap(),
        )
        .with_test_sandbox(root)
    }
    #[tokio::test]
    async fn attachments_reject_client_paths() {
        let error = rpc_dispatch(state(), "sessions.attachmentRead".into(), json!({"request":{"provider":"codex","sessionKey":"example","messageId":"m","attachmentId":"a","path":"/etc/passwd"}})).await.unwrap_err();
        assert_eq!(error.kind, ApiErrorKind::InvalidRequest);
    }
    #[tokio::test]
    async fn unconfirmed_delete_and_unknown_commands_are_rejected() {
        let error = rpc_dispatch(
            state(),
            "grok.deleteExecute".into(),
            json!({"request":{"id":"fixture-grok-session","confirmed":false}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.message, "Grok deletion requires confirmation");
        assert_eq!(
            rpc_dispatch(state(), "unknown.command".into(), json!({}))
                .await
                .unwrap_err()
                .kind,
            ApiErrorKind::NotFound
        );
    }
    #[tokio::test]
    async fn job_detail_defaults_to_full_output_and_supports_summary_reads() {
        let state = state();
        state
            .db
            .create_job("fixture", "fixture", "Fixture job")
            .unwrap();
        state
            .db
            .append_job_output("fixture", "fixture output\n")
            .unwrap();
        for (args, expected) in [
            (json!({"id":"fixture"}), "fixture output\n"),
            (
                json!({"id":"fixture","includeOutput":true}),
                "fixture output\n",
            ),
            (json!({"id":"fixture","includeOutput":false}), ""),
        ] {
            let job = rpc_dispatch(state.clone(), "jobs.detail".into(), args)
                .await
                .unwrap();
            assert_eq!(job["output"], expected);
            assert_eq!(job["id"], "fixture");
        }
    }
    #[test]
    fn rpc_dispatch_match_arms_cover_contract_linux_rpc_actions() {
        let source = include_str!("rpc_dispatch.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("rpc dispatcher source must include production section");
        let contract: serde_json::Value =
            serde_json::from_str(include_str!("../../../../contracts/agenthub-contract.json"))
                .expect("contract registry must be valid JSON");
        let command_source = include_str!("../../../agenthub-core/src/services/commands.rs");
        let contract_linux_rpc = contract
            .get("actions")
            .and_then(serde_json::Value::as_array)
            .expect("contract actions must be an array")
            .iter()
            .filter_map(|action| action.get("linuxRpc").and_then(serde_json::Value::as_str))
            .collect::<BTreeSet<_>>();
        let allowed_rpc = rpc_commands::ALLOWED_RPC_COMMANDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();

        assert_eq!(
            contract_linux_rpc, allowed_rpc,
            "contract registry Linux RPC names must match the core allowlist"
        );
        for command in rpc_commands::ALLOWED_RPC_COMMANDS {
            let const_name = command_source
                .lines()
                .filter_map(|line| line.strip_prefix("pub const "))
                .find_map(|line| {
                    let (name, rest) = line.split_once(": &str = ")?;
                    let value = rest.trim().trim_end_matches(';').trim_matches('"');
                    (value == *command).then_some(name)
                })
                .unwrap_or_else(|| panic!("core command constant missing for {command}"));
            let match_arm = format!("rpc_commands::{const_name} =>");
            assert!(
                source.contains(&match_arm),
                "RPC dispatcher must have a match arm for contract Linux RPC {command}: {match_arm}"
            );
        }
    }
}
