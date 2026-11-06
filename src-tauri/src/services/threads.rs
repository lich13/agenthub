use agenthub_core::{
    codex::{
        set_thread_archived, set_thread_title, thread_detail_page, ThreadDetail, ThreadSummary,
    },
    db::JobRecord,
    services::{
        jobs as job_service,
        threads::{self as thread_service, ThreadBlocksPage, ThreadsQuery},
        use_cases::AgentHubUseCases,
    },
};
use anyhow::Result;

use crate::{overview::DesktopState, services::actions::DesktopActionResponse};

mod types;

pub(crate) use types::{
    DesktopRenameThreadRequest, DesktopThreadIdRequest, ThreadBlocksRequest, ThreadDetailRequest,
    ThreadListRequest,
};

pub(crate) fn thread_summaries_with_query(
    state: &DesktopState,
    query: ThreadsQuery,
) -> Result<Vec<ThreadSummary>> {
    let paths = state.codex_paths();
    let use_cases = AgentHubUseCases::new(state.platform()).threads();
    let plan = use_cases.list_read(query)?;
    let running_jobs = state.db.running_thread_jobs()?;
    let inputs = thread_service::load_thread_read_model_inputs(&paths, &plan, running_jobs)?;
    let view = thread_service::thread_list_read_model(state.platform(), inputs, plan.list.query)?;
    Ok(view.threads)
}

pub(crate) fn threads_with_state(
    state: &DesktopState,
    request: ThreadListRequest,
) -> Result<Vec<ThreadSummary>> {
    thread_summaries_with_query(
        state,
        ThreadsQuery {
            archived: request.archived,
            q: request.query,
            limit: request.limit,
        },
    )
}

pub(crate) fn thread_detail_with_state(
    state: &DesktopState,
    request: ThreadDetailRequest,
) -> Result<Option<ThreadDetail>> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .detail_read(thread_service::ThreadDetailRequest {
            id: request.id,
            limit: request.limit,
            full: request.full,
            before: request.before,
        })?;
    let Some(detail) = load_thread_detail_read_model(state, &plan.detail)? else {
        return Ok(None);
    };
    Ok(Some(thread_service::window_thread_detail_for_plan(
        detail,
        &plan.detail,
    )))
}

pub(crate) fn thread_blocks_with_state(
    state: &DesktopState,
    request: ThreadBlocksRequest,
) -> Result<Option<ThreadBlocksPage>> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .blocks_read(&request.id, request.limit, request.before)?;
    let Some(detail) = load_thread_detail_read_model(state, &plan.detail)? else {
        return Ok(None);
    };
    Ok(Some(thread_service::thread_blocks_page_for_plan(
        detail,
        &plan.detail,
    )))
}

pub(crate) fn archive_thread_with_state(
    state: &DesktopState,
    request: DesktopThreadIdRequest,
) -> Result<DesktopActionResponse> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .archive(&request.thread_id)?;
    set_thread_archived(&state.codex_paths(), &plan.thread_id, true)?;
    Ok(job_service::thread_state_action_response(&plan)?.into())
}

pub(crate) fn restore_thread_with_state(
    state: &DesktopState,
    request: DesktopThreadIdRequest,
) -> Result<DesktopActionResponse> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .restore(&request.thread_id)?;
    set_thread_archived(&state.codex_paths(), &plan.thread_id, false)?;
    Ok(job_service::thread_state_action_response(&plan)?.into())
}

pub(crate) async fn rename_thread_with_state(
    state: &DesktopState,
    request: DesktopRenameThreadRequest,
) -> Result<DesktopActionResponse> {
    let plan = AgentHubUseCases::new(state.platform()).threads().rename(
        job_service::ThreadRenameRequest {
            thread_id: request.thread_id,
            name: request.name,
        },
    )?;
    let name = plan.name.as_deref().unwrap_or_default();
    set_thread_title(&state.codex_paths(), &plan.thread_id, name).await?;
    job_service::thread_state_action_response(&plan).map(Into::into)
}

fn load_thread_detail_read_model(
    state: &DesktopState,
    plan: &thread_service::ThreadDetailPlan,
) -> Result<Option<ThreadDetail>> {
    let paths = state.codex_paths();
    let Some(detail) = thread_detail_page(
        &paths,
        &plan.thread_id,
        plan.block_limit,
        plan.before.as_deref(),
        None,
    )?
    else {
        return Ok(None);
    };
    let active_job = active_job_for_thread(state, &detail.summary.id)?;
    let view = thread_service::thread_detail_read_model(state.platform(), detail, active_job)?;
    let mut detail = view.detail;
    agenthub_core::codex::subagents::enrich_subagent_blocks(&paths, &mut detail);
    Ok(Some(detail))
}

fn active_job_for_thread(state: &DesktopState, thread_id: &str) -> Result<Option<JobRecord>> {
    state.db.running_job_for_thread(thread_id)
}
