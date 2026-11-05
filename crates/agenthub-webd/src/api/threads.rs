use super::ApiErrorKind;
use super::{api_error, ok, system::linux_update_platform, ApiError, ApiResponse};
use crate::{linux_adapter, state::AppState};
use agenthub_core::services::{
    jobs as job_service,
    threads::{self as thread_service, ThreadsQuery},
    use_cases::AgentHubUseCases,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadDetailQuery {
    limit: Option<usize>,
    before: Option<String>,
    full: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThreadBlocksQuery {
    limit: Option<usize>,
    before: Option<String>,
}

pub(crate) async fn subagent_detail(
    state: AppState,
    request: agenthub_core::codex::subagents::SubagentDetailRequest,
) -> ApiResponse {
    let result = agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .threads()
            .subagent_detail(&state.codex_paths(), &request)
    })
    .await
    .map_err(anyhow::Error::from)??;
    ok(result)
}

pub(crate) async fn list_threads(state: AppState, query: ThreadsQuery) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        linux_adapter::list_threads_read_model(&state, query)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn thread_detail(
    state: AppState,
    id: String,
    query: ThreadDetailQuery,
) -> ApiResponse {
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .threads()
        .detail_read(thread_service::ThreadDetailRequest {
            id: id.clone(),
            limit: query.limit,
            full: query.full,
            before: query.before.clone(),
        })
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    match agenthub_core::read_tasks::spawn_blocking(move || {
        linux_adapter::window_thread_detail_read_model(&state, &plan.detail)
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(api_error_for_thread_detail_load)?
    {
        Some(detail) => ok(detail),
        None => Err(api_error(ApiErrorKind::NotFound, "thread not found")),
    }
}

fn api_error_for_thread_detail_load(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    api_error(ApiErrorKind::OperationFailed, &message)
}

pub(crate) async fn thread_blocks(
    state: AppState,
    id: String,
    query: ThreadBlocksQuery,
) -> ApiResponse {
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .threads()
        .blocks(&id, query.limit, query.before.clone())
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    match agenthub_core::read_tasks::spawn_blocking(move || {
        linux_adapter::thread_blocks_read_model(&state, &plan)
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(api_error_for_thread_detail_load)?
    {
        Some(page) => ok(page),
        None => Err(api_error(ApiErrorKind::NotFound, "thread not found")),
    }
}

pub(crate) async fn archive_thread(state: AppState, id: String) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .threads()
        .archive(&id)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    ok(linux_adapter::apply_thread_state_action_plan(&state, &auth, &plan).await?)
}

pub(crate) async fn restore_thread(state: AppState, id: String) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .threads()
        .restore(&id)
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    ok(linux_adapter::apply_thread_state_action_plan(&state, &auth, &plan).await?)
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenameThreadRequest {
    name: String,
}

pub(crate) async fn rename_thread(
    state: AppState,
    id: String,
    payload: RenameThreadRequest,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let platform = linux_update_platform();
    let plan = AgentHubUseCases::new(&platform)
        .threads()
        .rename(job_service::ThreadRenameRequest {
            thread_id: id,
            name: payload.name,
        })
        .map_err(|err| api_error(ApiErrorKind::InvalidRequest, &err.to_string()))?;
    ok(linux_adapter::apply_thread_state_action_plan(&state, &auth, &plan).await?)
}
