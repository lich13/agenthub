use crate::state::{AppState, ADMIN_ID};
use agenthub_core::services::jobs::thread_state_action_response as core_thread_state_action_response;
use agenthub_core::{
    codex,
    config::Config,
    services::{
        jobs::{self as job_service, ThreadStateAction},
        probe as probe_service, settings as settings_service, updates as update_service,
    },
};
use anyhow::{anyhow, Result};
use serde_json::json;
use std::path::Path;

pub(crate) fn apply_probe_settings_save_plan(
    state: &AppState,
    config_path: &Path,
    plan: settings_service::ProbeSettingsSavePlan,
) -> Result<Config> {
    let response_config =
        settings_service::apply_probe_settings_save_plan(config_path, &state.db, plan)?;
    state.replace_config(response_config.clone());
    state.notify_github_ci();
    state.db.record_audit(
        Some(ADMIN_ID),
        "probe_settings.updated",
        Some("probe"),
        Some("settings"),
        json!({"config_path": config_path}),
    )?;

    Ok(response_config)
}

pub(crate) fn start_probe_action_plan(
    state: &AppState,
    plan: probe_service::ProbeActionPlan,
    config_path: &Path,
) -> Result<String> {
    let action = plan.action;
    let spec = plan.job;

    state.db.record_audit(
        Some(ADMIN_ID),
        &format!("{}.started", spec.kind),
        Some("probe"),
        Some(&spec.title),
        json!({"args": spec.args, "action": action.as_rpc_action()}),
    )?;
    let process = probe_service::probe_process_command(state.platform(), config_path, action);
    let group = spec.exclusive_group.as_deref().unwrap_or(&spec.kind);
    state
        .jobs
        .start_exclusive_process_job(&spec.kind, &spec.title, process, group)
}

pub(crate) async fn apply_thread_state_action_plan(
    state: &AppState,
    plan: &job_service::ThreadStateActionPlan,
) -> Result<job_service::ActionResponse> {
    let paths = state.codex_paths();
    match &plan.action {
        ThreadStateAction::Archive | ThreadStateAction::Restore => {
            let archived = matches!(plan.action, ThreadStateAction::Archive);
            codex::set_thread_archived(&paths, &plan.thread_id, archived)?;
            state.db.record_audit(
                Some(ADMIN_ID),
                if archived {
                    "thread.archived"
                } else {
                    "thread.restored"
                },
                Some("thread"),
                Some(&plan.thread_id),
                json!({}),
            )?;
        }
        ThreadStateAction::Rename { name } => {
            state
                .app_server_client
                .rename_thread(&paths, &plan.thread_id, name)
                .await?;
            state.db.record_audit(
                Some(ADMIN_ID),
                "thread.renamed",
                Some("thread"),
                Some(&plan.thread_id),
                json!({"name": name}),
            )?;
        }
    }
    Ok(core_thread_state_action_response(plan))
}

pub(crate) fn start_update_action_plan(
    state: &AppState,
    plan: update_service::UpdateActionPlan,
    audit_action: Option<&str>,
) -> Result<String> {
    let action = plan.action;
    let spec = plan
        .linux_job
        .ok_or_else(|| anyhow!("Linux update job is unavailable"))?;
    let process = spec.process_command()?;
    // RPC tests must never start an installed updater or prune real backups.
    #[cfg(test)]
    let process = {
        let _ = process;
        agenthub_core::jobs::ProcessCommand::new("true", std::iter::empty::<&str>())
    };
    if let Some(audit_action) = audit_action {
        state.db.record_audit(
            Some(ADMIN_ID),
            audit_action,
            Some("system"),
            Some("updates"),
            json!({ "action": format!("{action:?}") }),
        )?;
    }
    if let Some(group) = spec.exclusive_group.as_deref() {
        state
            .jobs
            .start_exclusive_process_job(&spec.kind, &spec.title, process, group)
    } else {
        state
            .jobs
            .start_process_job(&spec.kind, &spec.title, process)
    }
}
