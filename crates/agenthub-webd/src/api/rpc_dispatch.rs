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
    rpc_surface::{is_business_rpc_command, is_retired_rpc_command, is_transport_rpc_command},
    state::AppState,
};
use agenthub_core::services::{
    commands as rpc_commands,
    probe::{self as probe_service},
    updates::UpdateAction,
};
use axum::{
    extract::connect_info::ConnectInfo,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;
use std::net::SocketAddr;

pub(crate) async fn rpc_dispatch(
    State(state): State<AppState>,
    Path(command): Path<String>,
    _connect: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(args): Json<Value>,
) -> ApiResponse {
    if is_transport_rpc_command(&command) {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            &format!("transport endpoint is not a business rpc command: {command}"),
        ));
    }
    if is_retired_rpc_command(&command) {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            &format!("unavailable: retired rpc command: {command}"),
        ));
    }
    if !is_business_rpc_command(&command) {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            &format!("unknown rpc command: {command}"),
        ));
    }

    match command.as_str() {
        rpc_commands::THREADS_SUBAGENT_DETAIL => {
            super::threads::subagent_detail(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::SESSIONS_ATTACHMENT_READ => {
            super::sessions::attachment_read(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::SESSIONS_SEARCH => {
            super::sessions::search(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::SESSIONS_BULK_PREVIEW => {
            super::sessions::bulk_preview(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::SESSIONS_BULK_EXECUTE => {
            super::sessions::bulk_execute(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::GROK_LIST => {
            grok_list(
                State(state),
                headers,
                axum::extract::Query(GrokListQuery {
                    q: args
                        .get("q")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                }),
            )
            .await
        }
        rpc_commands::GROK_DETAIL => {
            let id = rpc_required_string(&args, "id")?;
            grok_detail(
                State(state),
                headers,
                axum::extract::Path(id),
                axum::extract::Query(super::grok::GrokDetailQuery {
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize),
                    before: args
                        .get("before")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                }),
            )
            .await
        }
        rpc_commands::GROK_RENAME => {
            grok_rename(State(state), headers, Json(rpc_payload(&args)?)).await
        }
        rpc_commands::GROK_DELETE_PREVIEW => {
            grok_delete_preview(
                State(state),
                headers,
                Path(rpc_required_string(&args, "id")?),
            )
            .await
        }
        rpc_commands::GROK_DELETE_EXECUTE => {
            grok_delete_execute(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::CLAUDE_LIST => {
            claude_list(
                State(state),
                headers,
                axum::extract::Query(ClaudeListQuery {
                    q: args
                        .get("q")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|v| v as usize),
                }),
            )
            .await
        }
        rpc_commands::CLAUDE_DETAIL => {
            claude_detail(State(state), headers, Json(rpc_payload(&args)?)).await
        }
        rpc_commands::CLAUDE_RENAME => {
            claude_rename(State(state), headers, Json(rpc_payload(&args)?)).await
        }
        rpc_commands::CLAUDE_DELETE_PREVIEW => {
            let session_key = rpc_required_string(&args, "sessionKey")?;
            claude_delete_preview(State(state), headers, axum::extract::Path(session_key)).await
        }
        rpc_commands::CLAUDE_DELETE_EXECUTE => {
            claude_delete_execute(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::ANTIGRAVITY_LIST => {
            let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
                Default::default()
            } else {
                rpc_wrapped_payload(&args, &["request"])?
            };
            antigravity_list(State(state), headers, Json(request)).await
        }
        rpc_commands::ANTIGRAVITY_DETAIL => {
            antigravity_detail(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::ANTIGRAVITY_RENAME => {
            antigravity_rename(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::ANTIGRAVITY_ARCHIVE => {
            antigravity_archive(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::ANTIGRAVITY_RESTORE => {
            antigravity_restore(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::DSH_LIST => {
            let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
                Default::default()
            } else {
                rpc_wrapped_payload(&args, &["request"])?
            };
            dsh_list(State(state), headers, Json(request)).await
        }
        rpc_commands::DSH_DETAIL => {
            dsh_detail(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::DSH_RENAME => {
            dsh_rename(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::DSH_ARCHIVE => {
            dsh_archive(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::DSH_RESTORE => {
            dsh_restore(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::SYSTEM_PROVIDERS => list_providers(State(state), headers).await,
        rpc_commands::SYSTEM_PLATFORM => platform_overview(State(state), headers).await,
        rpc_commands::PROBE_STATUS => {
            get_probe_status(
                State(state),
                Query(ProbeStatusQuery {
                    refresh: args.get("refresh").and_then(Value::as_bool),
                }),
                headers,
            )
            .await
        }
        rpc_commands::PROBE_SETTINGS_GET => get_probe_settings(State(state), headers).await,
        rpc_commands::PROBE_SETTINGS_SAVE => {
            patch_probe_settings(
                State(state),
                headers,
                Json(rpc_wrapped_payload(
                    &args,
                    &["settings", "payload", "request"],
                )?),
            )
            .await
        }
        rpc_commands::PROBE_EVENTS => {
            get_probe_events(
                State(state),
                Query(ProbeEventsQuery {
                    limit: args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|value| value as u32),
                }),
                headers,
            )
            .await
        }
        rpc_commands::PROBE_BARK_TEST => {
            start_probe_action(state, headers, probe_service::ProbeAction::BarkTest).await
        }
        rpc_commands::PROBE_INSTALL_HOOKS => {
            start_probe_action(state, headers, probe_service::ProbeAction::InstallHooks).await
        }
        rpc_commands::PROBE_REMOVE_HOOKS => {
            start_probe_action(state, headers, probe_service::ProbeAction::RemoveHooks).await
        }
        rpc_commands::CLEANUP_DSH_ARCHIVE_DRY_RUN => {
            dsh_archive_delete_dry_run(State(state), headers).await
        }
        rpc_commands::CLEANUP_DSH_ARCHIVE_EXECUTE => {
            dsh_archive_delete_execute(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::CLEANUP_UNIFIED_DRY_RUN => {
            let request = if args.is_null() || args.as_object().is_some_and(|m| m.is_empty()) {
                Default::default()
            } else {
                rpc_wrapped_payload(&args, &["request"])?
            };
            unified_cleanup_dry_run(State(state), headers, Json(request)).await
        }
        rpc_commands::CLEANUP_UNIFIED_EXECUTE => {
            unified_cleanup_execute(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::CLEANUP_ARCHIVE_DRY_RUN => {
            archive_delete_dry_run(State(state), headers).await
        }
        rpc_commands::CLEANUP_ARCHIVE_EXECUTE => {
            archive_delete_execute(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["payload", "request"])?),
            )
            .await
        }
        rpc_commands::CLEANUP_HIDDEN_DRY_RUN => {
            hidden_threads_delete_dry_run(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["request"])?),
            )
            .await
        }
        rpc_commands::CLEANUP_HIDDEN_EXECUTE => {
            hidden_threads_delete_execute(
                State(state),
                headers,
                Json(rpc_wrapped_payload(&args, &["payload", "request"])?),
            )
            .await
        }
        rpc_commands::UPDATES_STATUS => system_update_status(State(state), headers).await,
        rpc_commands::UPDATES_CHECK => {
            start_update_action(state, headers, UpdateAction::Check, None).await
        }
        rpc_commands::UPDATES_INSTALL => {
            start_update_action(
                state,
                headers,
                UpdateAction::Install,
                Some("agenthub.update.install_started"),
            )
            .await
        }
        rpc_commands::UPDATES_PRUNE => {
            start_update_action(
                state,
                headers,
                UpdateAction::Prune,
                Some("agenthub.update.prune_started"),
            )
            .await
        }
        rpc_commands::THREADS_LIST => {
            super::list_threads(State(state), headers, Query(rpc_payload(&args)?)).await
        }
        rpc_commands::THREADS_DETAIL => {
            thread_detail(
                State(state),
                headers,
                Path(rpc_required_string(&args, "id")?),
                Query(rpc_nested_payload_or_empty(&args, "options")?),
            )
            .await
        }
        rpc_commands::THREADS_BLOCKS => {
            thread_blocks(
                State(state),
                headers,
                Path(rpc_required_string(&args, "id")?),
                Query(rpc_nested_payload_or_empty(&args, "options")?),
            )
            .await
        }
        rpc_commands::THREADS_ARCHIVE => {
            archive_thread(
                State(state),
                headers,
                Path(rpc_required_string(&args, "threadId")?),
            )
            .await
        }
        rpc_commands::THREADS_RESTORE => {
            restore_thread(
                State(state),
                headers,
                Path(rpc_required_string(&args, "threadId")?),
            )
            .await
        }
        rpc_commands::THREADS_RENAME => {
            rename_thread(
                State(state),
                headers,
                Path(rpc_required_string(&args, "threadId")?),
                Json(rpc_payload(&args)?),
            )
            .await
        }
        rpc_commands::SYSTEM_CAPABILITIES => system_capabilities(State(state), headers).await,
        rpc_commands::SYSTEM_VERSION => system_version(State(state), headers).await,
        rpc_commands::JOBS_LIST => {
            list_jobs(
                State(state),
                headers,
                Query(rpc_query_strings(&args, &["limit"])),
            )
            .await
        }
        rpc_commands::JOBS_DETAIL => {
            job_detail(
                State(state),
                headers,
                Path(rpc_required_string(&args, "id")?),
            )
            .await
        }
        _ => Err(api_error(
            StatusCode::NOT_FOUND,
            &format!("rpc command allowlist drifted without handler mapping: {command}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::{api::routes::router, state::AppState};
    use agenthub_core::{db::PanelDb, services::commands as rpc_commands};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use std::collections::BTreeSet;
    use tower::ServiceExt;

    fn authenticated_test_state() -> (AppState, String) {
        let (config, sandbox_root) = crate::api::test_support::sandboxed_test_config();
        let db = PanelDb::open(":memory:").unwrap();
        let api_key = String::new();
        (
            AppState::new(config, db).with_test_sandbox(sandbox_root),
            api_key,
        )
    }

    async fn request_rpc_status(
        app: axum::Router,
        command: &str,
        body: &str,
        api_key: Option<&str>,
    ) -> StatusCode {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/api/rpc/{command}"))
            .header("content-type", "application/json");
        if let Some(api_key) = api_key {
            builder = builder.header("x-api-key", api_key);
        }
        app.oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn attachments_reject_client_paths() {
        let (state, token) = authenticated_test_state();
        let body = r#"{"request":{"provider":"codex","sessionKey":"example","messageId":"m","attachmentId":"a","path":"/etc/passwd"}}"#;
        let app = super::super::router(state);
        assert_eq!(
            request_rpc_status(app, "sessions.attachmentRead", body, Some(&token)).await,
            StatusCode::BAD_REQUEST
        );
    }

    async fn assert_unconfirmed_desktop_delete_request(
        command: &str,
        request: serde_json::Value,
        expected_error: &str,
    ) {
        let (state, api_key) = authenticated_test_state();
        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/rpc/{command}"))
                    .header("content-type", "application/json")
                    .header("x-api-key", api_key)
                    .body(Body::from(
                        serde_json::json!({"request": request}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], expected_error);
    }

    #[tokio::test]
    async fn grok_desktop_delete_request_wrapper_reaches_confirmation_guard() {
        assert_unconfirmed_desktop_delete_request(
            "grok.deleteExecute",
            serde_json::json!({
                "id": "fixture-grok-session",
                "confirmed": false,
                "fingerprint": "fixture-fingerprint"
            }),
            "Grok deletion requires confirmation",
        )
        .await;
    }

    #[tokio::test]
    async fn retired_rpc_commands_are_rejected_even_when_authenticated() {
        for command in agenthub_core::services::commands::RETIRED_COMMANDS {
            let (state, api_key) = authenticated_test_state();
            let response = router(state)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/rpc/{command}"))
                        .header("content-type", "application/json")
                        .header("x-api-key", api_key)
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{command}");
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                body,
                serde_json::json!({"error": format!("unavailable: retired rpc command: {command}")})
            );
        }
    }

    #[test]
    fn rpc_dispatch_hard_deletes_string_action_compat_commands() {
        let source = include_str!("rpc_dispatch.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("rpc dispatcher source must include production section");
        for typed in [
            "rpc_commands::PROBE_BARK_TEST",
            "rpc_commands::PROBE_INSTALL_HOOKS",
            "rpc_commands::PROBE_REMOVE_HOOKS",
            "rpc_commands::UPDATES_CHECK",
            "rpc_commands::UPDATES_INSTALL",
            "rpc_commands::UPDATES_PRUNE",
        ] {
            assert!(
                source.contains(typed),
                "RPC dispatcher must expose typed command {typed}"
            );
        }
        for compat in [
            "\"startProbeJob\"",
            "\"runUpdateAction\"",
            "\"startProbeBarkTest\"",
            "\"startProbeHooksInstall\"",
            "\"checkUpdate\"",
            "\"installUpdateAndRestart\"",
            "\"stopThread\"",
            "\"archiveThread\"",
            "\"restoreThread\"",
            "\"renameThread\"",
            "\"listFollowUps\"",
            "\"enqueueFollowUp\"",
            "\"cancelFollowUp\"",
            "\"listJobs\"",
            "\"getJob\"",
            "\"deleteArchivedThreadsDryRun\"",
            "\"deleteArchivedThreadsExecute\"",
            "\"deleteHiddenThreadsDryRun\"",
            "\"deleteHiddenThreadsExecute\"",
        ] {
            assert!(
                !source.contains(compat),
                "{compat} must not remain in the production RPC dispatcher"
            );
        }
        assert!(
            source.contains("is_retired_rpc_command"),
            "RPC dispatcher should consult retired command guard rails"
        );
        assert!(
            source.contains("is_business_rpc_command"),
            "RPC dispatcher should consult shared business RPC allowlist"
        );
        assert!(
            !source.contains("/api/rpc/uploadFiles"),
            "RPC dispatcher should keep upload transport out of business command dispatch"
        );
        assert!(
            !source.contains("/api/rpc/threadEvents/:id"),
            "RPC dispatcher should keep thread event transport out of business command dispatch"
        );
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
