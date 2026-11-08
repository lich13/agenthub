use crate::{audit::AuditActor, state::AppState};
use agenthub_core::services::jobs::thread_state_action_response as core_thread_state_action_response;
use agenthub_core::{
    codex::{self, ThreadDetail, ThreadSummary},
    config::Config,
    db::JobRecord,
    platform::PlatformPaths,
    services::{
        commands, jobs as job_service, probe as probe_service, settings as settings_service,
        threads::{self as thread_service, ThreadBlocksPage, ThreadsQuery},
        updates::{self as update_service, UpdateAction},
        use_cases::AgentHubUseCases,
    },
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;

pub fn apply_probe_settings_save_plan(
    state: &AppState,
    auth: &AuditActor,
    config_path: &Path,
    plan: settings_service::ProbeSettingsSavePlan,
) -> Result<Config> {
    let response_config = settings_service::apply_probe_settings_save_plan(
        config_path,
        &state.db,
        &state.config(),
        plan,
    )?;
    state.replace_config(response_config.clone());
    state.db.record_audit(
        Some(&auth.admin_id),
        "probe_settings.updated",
        Some("probe"),
        Some("settings"),
        None,
        json!({"config_path": config_path}),
    )?;

    Ok(response_config)
}

pub fn start_probe_action_plan(
    state: &AppState,
    auth: &AuditActor,
    plan: probe_service::ProbeActionPlan,
    config_path: &Path,
) -> Result<String> {
    let action = plan.action;
    let spec = plan.job;

    state.db.record_audit(
        Some(&auth.admin_id),
        &format!("{}.started", spec.kind),
        Some("probe"),
        Some(&spec.title),
        None,
        json!({"args": spec.args, "action": action.as_rpc_action()}),
    )?;
    let process = probe_service::probe_process_command(state.platform(), config_path, action);
    let group = spec.exclusive_group.as_deref().unwrap_or(&spec.kind);
    state
        .jobs
        .start_exclusive_process_job(&spec.kind, &spec.title, process, group)
}

pub fn list_threads_read_model(
    state: &AppState,
    query: ThreadsQuery,
) -> Result<Vec<ThreadSummary>> {
    let platform = PlatformPaths::for_kind(agenthub_core::platform::PlatformKind::Linux);
    let plan = AgentHubUseCases::new(&platform)
        .threads()
        .list_read(query)?;
    let paths = state.codex_paths();
    let running_jobs = state.db.running_thread_jobs()?;
    let inputs = thread_service::load_thread_read_model_inputs(&paths, &plan, running_jobs)?;
    let view = thread_service::thread_list_read_model(inputs, plan.query);
    Ok(view)
}

pub fn probe_threads_read_model(
    state: &AppState,
    status: &'static str,
    limit: usize,
) -> Result<Vec<ThreadSummary>> {
    let paths = state.codex_paths();
    probe_service::probe_threads_for_status_with_paths(&paths, state.db.path(), status, limit)
}

pub fn load_thread_detail_read_model(
    state: &AppState,
    plan: &thread_service::ThreadDetailPlan,
) -> Result<Option<ThreadDetail>> {
    let paths = state.codex_paths();
    let mut detail = codex::thread_detail_page(
        &paths,
        &plan.thread_id,
        plan.block_limit,
        plan.before.as_deref(),
        None,
    )?;
    if let Some(detail) = detail.take() {
        let detail_thread_id = detail.summary.id.clone();
        let active_job = active_job_for_thread(state, &detail_thread_id)?;
        let view = thread_service::thread_detail_read_model(detail, active_job);
        let mut detail = view;
        codex::subagents::enrich_subagent_blocks(&paths, &mut detail);
        return Ok(Some(detail));
    }
    Ok(None)
}

fn active_job_for_thread(state: &AppState, thread_id: &str) -> Result<Option<JobRecord>> {
    state.db.running_job_for_thread(thread_id)
}

pub fn window_thread_detail_read_model(
    state: &AppState,
    plan: &thread_service::ThreadDetailPlan,
) -> Result<Option<ThreadDetail>> {
    let Some(detail) = load_thread_detail_read_model(state, plan)? else {
        return Ok(None);
    };
    Ok(Some(thread_service::window_thread_detail_for_plan(
        detail, plan,
    )))
}

pub fn thread_blocks_read_model(
    state: &AppState,
    plan: &thread_service::ThreadDetailPlan,
) -> Result<Option<ThreadBlocksPage>> {
    let Some(detail) = load_thread_detail_read_model(state, plan)? else {
        return Ok(None);
    };
    Ok(Some(thread_service::thread_blocks_page_for_plan(
        detail, plan,
    )))
}

pub fn list_jobs_plan(state: &AppState, limit: u32) -> Result<Vec<Value>> {
    state
        .db
        .list_jobs(limit)?
        .into_iter()
        .map(job_response_value)
        .collect()
}

pub fn job_detail_plan(
    state: &AppState,
    job_id: String,
    include_output: bool,
) -> Result<Option<Value>> {
    state
        .db
        .job_with_output(&job_id, include_output)?
        .map(job_response_value)
        .transpose()
}

fn job_response_value(job: JobRecord) -> Result<Value> {
    let response = AgentHubUseCases::new(&PlatformPaths::for_kind(
        agenthub_core::platform::PlatformKind::Linux,
    ))
    .jobs()
    .response(job);
    Ok(serde_json::to_value(response)?)
}

pub async fn apply_thread_state_action_plan(
    state: &AppState,
    auth: &AuditActor,
    plan: &job_service::ThreadStateActionPlan,
) -> Result<job_service::ActionResponse> {
    let paths = state.codex_paths();
    if matches!(
        plan.command.as_str(),
        commands::THREADS_ARCHIVE | commands::THREADS_RESTORE
    ) {
        let archived = plan.command == commands::THREADS_ARCHIVE;
        codex::set_thread_archived(&paths, &plan.thread_id, archived)?;
        state.db.record_audit(
            Some(&auth.admin_id),
            if archived {
                "thread.archived"
            } else {
                "thread.restored"
            },
            Some("thread"),
            Some(&plan.thread_id),
            None,
            json!({}),
        )?;
    }
    if let Some(name) = plan.name.as_deref() {
        state
            .app_server_client
            .rename_thread(&paths, &plan.thread_id, name)
            .await?;
        state.db.record_audit(
            Some(&auth.admin_id),
            "thread.renamed",
            Some("thread"),
            Some(&plan.thread_id),
            None,
            json!({"name": name}),
        )?;
    }
    core_thread_state_action_response(plan)
}

pub fn start_update_action_plan(
    state: &AppState,
    auth: &AuditActor,
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
            Some(&auth.admin_id),
            audit_action,
            Some("system"),
            Some("updates"),
            None,
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

pub fn linux_update_action_plan(
    platform: &PlatformPaths,
    action: UpdateAction,
) -> Result<update_service::UpdateActionPlan> {
    update_service::plan_update_action(platform, action)
}
