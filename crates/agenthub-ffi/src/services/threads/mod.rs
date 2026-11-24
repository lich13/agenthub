use agenthub_core::{
    codex::{set_thread_archived, set_thread_title, ThreadDetail, ThreadSummary},
    services::{
        jobs::{self as job_service, ActionResponse},
        threads::{self as thread_service, ThreadBlocksPage, ThreadsQuery},
        use_cases::AgentHubUseCases,
    },
};
use anyhow::Result;

use crate::overview::DesktopState;

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
    thread_service::load_thread_read_model_inputs(&paths, &plan)
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
    thread_service::read_window_thread_detail(&state.codex_paths(), &plan)
}

pub(crate) fn thread_blocks_with_state(
    state: &DesktopState,
    request: ThreadBlocksRequest,
) -> Result<Option<ThreadBlocksPage>> {
    let plan = AgentHubUseCases::new(state.platform()).threads().blocks(
        &request.id,
        request.limit,
        request.before,
    )?;
    thread_service::read_thread_blocks_page(&state.codex_paths(), &plan)
}

pub(crate) fn archive_thread_with_state(
    state: &DesktopState,
    request: DesktopThreadIdRequest,
) -> Result<ActionResponse> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .archive(&request.thread_id)?;
    set_thread_archived(&state.codex_paths(), &plan.thread_id, true)?;
    job_service::thread_state_action_response(&plan)
}

pub(crate) fn restore_thread_with_state(
    state: &DesktopState,
    request: DesktopThreadIdRequest,
) -> Result<ActionResponse> {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .restore(&request.thread_id)?;
    set_thread_archived(&state.codex_paths(), &plan.thread_id, false)?;
    job_service::thread_state_action_response(&plan)
}

pub(crate) async fn rename_thread_with_state(
    state: &DesktopState,
    request: DesktopRenameThreadRequest,
) -> Result<ActionResponse> {
    let plan = AgentHubUseCases::new(state.platform()).threads().rename(
        job_service::ThreadRenameRequest {
            thread_id: request.thread_id,
            name: request.name,
        },
    )?;
    let name = plan.name.as_deref().unwrap_or_default();
    set_thread_title(&state.codex_paths(), &plan.thread_id, name).await?;
    job_service::thread_state_action_response(&plan)
}
