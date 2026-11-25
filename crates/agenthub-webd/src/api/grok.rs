use super::{ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::{grok::GrokDeleteRequest, services::use_cases::AgentHubUseCases};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct GrokListQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct GrokDetailQuery {
    pub limit: Option<usize>,
    pub before: Option<String>,
}

pub(crate) async fn grok_list(state: AppState, query: GrokListQuery) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform)
            .grok()?
            .list(query.limit.unwrap_or(50), query.q.as_deref())
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn grok_detail(
    state: AppState,
    id: String,
    query: GrokDetailQuery,
) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform)
            .grok()?
            .page(&id, query.limit, query.before.as_deref())
    })
    .await
    .map_err(anyhow::Error::from)??)
}

#[derive(Debug, Deserialize)]
pub(crate) struct GrokRenameRequest {
    pub id: String,
    pub title: String,
}

pub(crate) async fn grok_rename(state: AppState, request: GrokRenameRequest) -> ApiResponse {
    ok(AgentHubUseCases::new(state.platform())
        .grok()?
        .rename(&request.id, &request.title)
        .await?)
}

pub(crate) async fn grok_delete_preview(state: AppState, id: String) -> ApiResponse {
    let platform = state.platform().clone();
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(&platform).grok()?.delete_preview(&id)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn grok_delete_execute(
    state: AppState,
    request: GrokDeleteRequest,
) -> ApiResponse {
    let platform = state.platform().clone();
    let db = state.db.clone();
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
            let result = AgentHubUseCases::new(&platform)
                .grok()?
                .delete_execute(request)?;
            if result.deleted && result.residuals.is_empty() {
                let _ = db.delete_native_probe_stream_cursors(
                    agenthub_core::native_probe::NativeProvider::Grok,
                    std::slice::from_ref(&result.id),
                );
            }
            Ok(result)
        })
        .await
        .map_err(anyhow::Error::from)??,
    )
}
