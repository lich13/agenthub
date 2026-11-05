use crate::{audit::AuditActor, state::AppState};
use agenthub_core::services::jobs::thread_state_action_response as core_thread_state_action_response;
use agenthub_core::{
    codex::{self, ThreadDetail, ThreadSummary},
    config::{patch_probe_config_toml, Config},
    db::JobRecord,
    platform::PlatformPaths,
    services::{
        cleanup as cleanup_service, jobs as job_service, probe as probe_service,
        settings as settings_service,
        threads::{self as thread_service, ThreadBlocksPage, ThreadsQuery},
        updates::{self as update_service, UpdateAction},
        use_cases::{AgentHubUseCases, JobDetailPlan, JobListPlan},
    },
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};

pub fn apply_probe_settings_save_plan(
    state: &AppState,
    auth: &AuditActor,
    config_path: &Path,
    plan: settings_service::ProbeSettingsSavePlan,
) -> Result<Config> {
    let text = fs::read_to_string(config_path)?;
    let updated = patch_probe_config_toml(&text, &plan.config_patch)?;
    fs::write(config_path, updated)?;
    let response_config = Config::load(config_path)?;

    for write in &plan.secret_writes {
        state
            .db
            .set_secret_setting_bytes(&write.setting_key, write.secret_value.as_bytes())?;
    }

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
    let spec = plan
        .job
        .ok_or_else(|| anyhow!("Probe job is unavailable"))?;

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
    let running_jobs = if plan.include_running_jobs {
        state.db.running_thread_jobs()?
    } else {
        Vec::new()
    };
    let inputs = thread_service::load_thread_read_model_inputs(&paths, &plan, running_jobs)?;
    let view = thread_service::thread_list_read_model(&platform, inputs, plan.list.query)?;
    Ok(view.threads)
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
        let view = thread_service::thread_detail_read_model(
            &PlatformPaths::for_kind(agenthub_core::platform::PlatformKind::Linux),
            detail,
            active_job,
        )?;
        let mut detail = view.detail;
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

pub fn list_jobs_plan(state: &AppState, plan: JobListPlan) -> Result<Vec<Value>> {
    state
        .db
        .list_jobs(plan.limit)?
        .into_iter()
        .map(job_response_value)
        .collect()
}

pub fn job_detail_plan(state: &AppState, plan: JobDetailPlan) -> Result<Option<Value>> {
    state
        .db
        .job(&plan.job_id)?
        .map(job_response_value)
        .transpose()
}

pub fn execute_cleanup_plan(
    state: &AppState,
    auth: &AuditActor,
    plan: cleanup_service::CleanupOperationPlan,
) -> Result<Value> {
    if plan.requires_confirmation && !plan.confirmation.confirmed {
        anyhow::bail!("cleanup execute must be confirmed");
    }
    let paths = state.codex_paths();
    let platform = PlatformPaths::for_kind(agenthub_core::platform::PlatformKind::Linux);
    let cleanup = AgentHubUseCases::new(&platform).cleanup();
    match plan.target {
        cleanup_service::CleanupTarget::Archived => {
            let result = if plan.execute {
                let candidates = cleanup_service::hidden_candidates(&plan)?;
                let result = cleanup.execute_archived(&paths, &candidates)?;
                state.db.record_audit(
                    Some(&auth.admin_id),
                    "archives.delete.execute",
                    Some("archives"),
                    Some("root-codex"),
                    None,
                    json!({"before_archived": result.before.archived_threads, "deleted_rollout_files": result.deleted_rollout_files}),
                )?;
                serde_json::to_value(result)?
            } else {
                serde_json::to_value(cleanup.dry_run_archived(&paths)?)?
            };
            Ok(result)
        }
        cleanup_service::CleanupTarget::Hidden => {
            let result = if plan.execute {
                let candidates = cleanup_service::hidden_candidates(&plan)?;
                let result = cleanup.execute_hidden(&paths, &candidates)?;
                state.db.record_audit(
                    Some(&auth.admin_id),
                    "hidden_threads.delete.execute",
                    Some("hidden_threads"),
                    Some("root-codex"),
                    None,
                    json!({
                        "before_hidden": result.before.hidden_threads,
                        "deleted_threads": result.deleted_threads,
                        "skipped_threads": result.skipped_threads,
                        "failed_threads": result.failed_threads,
                        "deleted_rollout_files": result.deleted_rollout_files,
                    }),
                )?;
                serde_json::to_value(result)?
            } else {
                serde_json::to_value(cleanup.dry_run_hidden(&paths)?)?
            };
            Ok(result)
        }
    }
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
    if let Some(archived) = plan.archived {
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

pub fn linux_probe_action_plan(
    state: &AppState,
    platform: &PlatformPaths,
    action: probe_service::ProbeAction,
    config_path: &Path,
) -> Result<probe_service::ProbeActionPlan> {
    let device_key_configured = state
        .db
        .get_secret_setting_bytes(settings_service::PROBE_BARK_DEVICE_KEY_SETTING)?
        .is_some_and(|value| !value.is_empty());
    let config = state.config();
    probe_service::ProbeUseCases::new(&config, platform).action_with_device_key_and_config_path(
        action,
        device_key_configured,
        config_path,
    )
}

pub fn linux_update_action_plan(
    state: &AppState,
    platform: &PlatformPaths,
    action: UpdateAction,
) -> Result<update_service::UpdateActionPlan> {
    let config = state.config();
    update_service::UpdateUseCases::new(&config, platform).action_plan(action)
}
