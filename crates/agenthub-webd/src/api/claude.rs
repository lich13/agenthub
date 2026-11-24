use super::{ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::{claude::ClaudeDeleteRequest, services::use_cases::AgentHubUseCases};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ClaudeListQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

pub(crate) async fn claude_list(state: AppState, query: ClaudeListQuery) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform)
            .claude()?
            .list(query.limit.unwrap_or(100), query.q.as_deref())
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn claude_detail(
    state: AppState,
    request: agenthub_core::claude::ClaudeDetailRequest,
) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform).claude()?.detail(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeRenameRequest {
    pub session_key: String,
    pub title: String,
}

pub(crate) async fn claude_rename(state: AppState, request: ClaudeRenameRequest) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform)
            .claude()?
            .rename(&request.session_key, &request.title)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn claude_delete_preview(state: AppState, session_key: String) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform)
            .claude()?
            .delete_preview(&session_key)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn claude_delete_execute(
    state: AppState,
    request: ClaudeDeleteRequest,
) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform)
            .claude()?
            .delete_execute(request)
    })
    .await
    .map_err(anyhow::Error::from)??)
}
