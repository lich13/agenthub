use super::ApiErrorKind;
use super::{
    antigravity_archive, antigravity_detail, antigravity_list, antigravity_rename,
    antigravity_restore, claude_delete_execute, claude_delete_preview, claude_detail, claude_list,
    claude_rename, dsh_archive, dsh_detail, dsh_list, dsh_rename, dsh_restore, ClaudeListQuery,
};
use super::{
    api_error, archive_thread, get_probe_events, get_probe_settings, get_probe_status,
    grok_delete_execute, grok_delete_preview, grok_detail, grok_list, grok_rename,
    hidden_threads_delete_dry_run, job_detail, list_jobs, patch_probe_settings, rename_thread,
    restore_thread, start_probe_action, start_update_action, system_capabilities,
    system_update_status, thread_blocks, thread_detail, unified_cleanup_dry_run,
    unified_cleanup_execute, ApiResponse, GrokListQuery, ProbeEventsQuery,
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

mod conversations;
mod maintenance;
mod providers;

macro_rules! remote_handlers {
    ($( $id:path => $handler:path, )*) => {
        #[cfg(test)]
        const HANDLED_COMMANDS: &[&str] = &[$($id),*];
        pub(crate) async fn rpc_dispatch(state: AppState, command: String, args: Value) -> ApiResponse {
            match command.as_str() {
                $($id => $handler(state, args).await,)*
                _ => Err(api_error(ApiErrorKind::NotFound, "不支持的远程操作")),
            }
        }
    };
}

remote_handlers! {
    rpc_commands::GITHUB_CI_STATUS => maintenance::handle_github_ci_status,
    rpc_commands::THREADS_SUBAGENT_DETAIL => conversations::handle_threads_subagent_detail,
    rpc_commands::SESSIONS_ATTACHMENT_READ => conversations::handle_sessions_attachment_read,
    rpc_commands::SESSIONS_SEARCH => conversations::handle_sessions_search,
    rpc_commands::SESSIONS_BULK_PREVIEW => conversations::handle_sessions_bulk_preview,
    rpc_commands::SESSIONS_BULK_EXECUTE => conversations::handle_sessions_bulk_execute,
    rpc_commands::GROK_LIST => providers::handle_grok_list,
    rpc_commands::GROK_DETAIL => providers::handle_grok_detail,
    rpc_commands::GROK_RENAME => providers::handle_grok_rename,
    rpc_commands::GROK_DELETE_PREVIEW => providers::handle_grok_delete_preview,
    rpc_commands::GROK_DELETE_EXECUTE => providers::handle_grok_delete_execute,
    rpc_commands::CLAUDE_LIST => providers::handle_claude_list,
    rpc_commands::CLAUDE_DETAIL => providers::handle_claude_detail,
    rpc_commands::CLAUDE_RENAME => providers::handle_claude_rename,
    rpc_commands::CLAUDE_DELETE_PREVIEW => providers::handle_claude_delete_preview,
    rpc_commands::CLAUDE_DELETE_EXECUTE => providers::handle_claude_delete_execute,
    rpc_commands::ANTIGRAVITY_LIST => providers::handle_antigravity_list,
    rpc_commands::ANTIGRAVITY_DETAIL => providers::handle_antigravity_detail,
    rpc_commands::ANTIGRAVITY_RENAME => providers::handle_antigravity_rename,
    rpc_commands::ANTIGRAVITY_ARCHIVE => providers::handle_antigravity_archive,
    rpc_commands::ANTIGRAVITY_RESTORE => providers::handle_antigravity_restore,
    rpc_commands::DSH_LIST => providers::handle_dsh_list,
    rpc_commands::DSH_DETAIL => providers::handle_dsh_detail,
    rpc_commands::DSH_RENAME => providers::handle_dsh_rename,
    rpc_commands::DSH_ARCHIVE => providers::handle_dsh_archive,
    rpc_commands::DSH_RESTORE => providers::handle_dsh_restore,
    rpc_commands::PROBE_STATUS => maintenance::handle_probe_status,
    rpc_commands::PROBE_SETTINGS_GET => maintenance::handle_probe_settings_get,
    rpc_commands::PROBE_SETTINGS_SAVE => maintenance::handle_probe_settings_save,
    rpc_commands::PROBE_EVENTS => maintenance::handle_probe_events,
    rpc_commands::PROBE_BARK_TEST => maintenance::handle_probe_bark_test,
    rpc_commands::PROBE_INSTALL_HOOKS => maintenance::handle_probe_install_hooks,
    rpc_commands::PROBE_REMOVE_HOOKS => maintenance::handle_probe_remove_hooks,
    rpc_commands::CLEANUP_UNIFIED_DRY_RUN => maintenance::handle_cleanup_unified_dry_run,
    rpc_commands::CLEANUP_UNIFIED_EXECUTE => maintenance::handle_cleanup_unified_execute,
    rpc_commands::CLEANUP_HIDDEN_DRY_RUN => maintenance::handle_cleanup_hidden_dry_run,
    rpc_commands::UPDATES_STATUS => maintenance::handle_updates_status,
    rpc_commands::UPDATES_CHECK => maintenance::handle_updates_check,
    rpc_commands::UPDATES_INSTALL => maintenance::handle_updates_install,
    rpc_commands::UPDATES_PRUNE => maintenance::handle_updates_prune,
    rpc_commands::THREADS_LIST => conversations::handle_threads_list,
    rpc_commands::THREADS_DETAIL => conversations::handle_threads_detail,
    rpc_commands::THREADS_BLOCKS => conversations::handle_threads_blocks,
    rpc_commands::THREADS_ARCHIVE => conversations::handle_threads_archive,
    rpc_commands::THREADS_RESTORE => conversations::handle_threads_restore,
    rpc_commands::THREADS_RENAME => conversations::handle_threads_rename,
    rpc_commands::THREADS_ACCOUNT_SCOPE_STATUS => conversations::handle_threads_account_scope_status,
    rpc_commands::THREADS_ACCOUNT_SCOPE_CLEAR => conversations::handle_threads_account_scope_clear,
    rpc_commands::SYSTEM_CAPABILITIES => maintenance::handle_system_capabilities,
    rpc_commands::JOBS_LIST => maintenance::handle_jobs_list,
    rpc_commands::JOBS_DETAIL => maintenance::handle_jobs_detail,
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
            agenthub_core::db::PanelDb::open_with_secret_box(
                ":memory:",
                agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap(),
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
        let contract: serde_json::Value =
            serde_json::from_str(include_str!("../../../../contracts/agenthub-contract.json"))
                .expect("contract registry must be valid JSON");
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
        assert_eq!(
            contract_linux_rpc,
            HANDLED_COMMANDS.iter().copied().collect()
        );
    }
}
