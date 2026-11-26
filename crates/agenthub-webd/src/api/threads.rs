use super::{api_error, ok, ApiResponse};
use crate::{linux_adapter, state::AppState};
use agenthub_core::services::{
    jobs as job_service,
    threads::{self as thread_service, ThreadsQuery},
    use_cases::{AgentHubUseCases, PageWindowOptions},
};

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

pub(crate) async fn account_scope_status(state: AppState) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        AgentHubUseCases::new(state.platform())
            .threads()
            .account_scope_status(&state.codex_paths())
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn account_scope_clear(state: AppState) -> ApiResponse {
    let auth = crate::audit::actor();
    ok(
        agenthub_core::read_tasks::spawn_blocking(move || -> anyhow::Result<_> {
            let result = AgentHubUseCases::new(state.platform())
                .threads()
                .account_scope_clear(&state.codex_paths())?;
            state.db.record_audit(
                Some(&auth.admin_id),
                "codex_account_scope.clear",
                Some("codex_account_scope"),
                None,
                serde_json::json!({
                    "cleared_threads": result.cleared_threads,
                    "cleared_files": result.cleared_files,
                    "remaining_files": result.remaining_files,
                    "skipped_threads": result.items.len(),
                }),
            )?;
            Ok(result)
        })
        .await
        .map_err(anyhow::Error::from)??,
    )
}

pub(crate) async fn list_threads(state: AppState, query: ThreadsQuery) -> ApiResponse {
    ok(agenthub_core::read_tasks::spawn_blocking(move || {
        let plan = AgentHubUseCases::new(state.platform())
            .threads()
            .list_read(query)?;
        thread_service::load_thread_read_model_inputs(&state.codex_paths(), &plan)
    })
    .await
    .map_err(anyhow::Error::from)??)
}

pub(crate) async fn thread_detail(
    state: AppState,
    id: String,
    query: PageWindowOptions,
) -> ApiResponse {
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .detail_read(thread_service::ThreadDetailRequest {
            id,
            limit: query.limit,
            full: query.full,
            before: query.before,
        })?;
    match agenthub_core::read_tasks::spawn_blocking(move || {
        thread_service::read_window_thread_detail(&state.codex_paths(), &plan)
    })
    .await
    .map_err(anyhow::Error::from)??
    {
        Some(detail) => ok(detail),
        None => Err(api_error("thread not found")),
    }
}

pub(crate) async fn thread_blocks(
    state: AppState,
    id: String,
    query: PageWindowOptions,
) -> ApiResponse {
    let plan =
        AgentHubUseCases::new(state.platform())
            .threads()
            .blocks(&id, query.limit, query.before)?;
    match agenthub_core::read_tasks::spawn_blocking(move || {
        thread_service::read_thread_blocks_page(&state.codex_paths(), &plan)
    })
    .await
    .map_err(anyhow::Error::from)??
    {
        Some(page) => ok(page),
        None => Err(api_error("thread not found")),
    }
}

pub(crate) async fn archive_thread(state: AppState, id: String) -> ApiResponse {
    let auth = crate::audit::actor();
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .archive(&id)?;
    ok(linux_adapter::apply_thread_state_action_plan(&state, &auth, &plan).await?)
}

pub(crate) async fn restore_thread(state: AppState, id: String) -> ApiResponse {
    let auth = crate::audit::actor();
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .restore(&id)?;
    ok(linux_adapter::apply_thread_state_action_plan(&state, &auth, &plan).await?)
}

pub(crate) async fn rename_thread(
    state: AppState,
    request: job_service::ThreadRenameRequest,
) -> ApiResponse {
    let auth = crate::audit::actor();
    let plan = AgentHubUseCases::new(state.platform())
        .threads()
        .rename(request)?;
    ok(linux_adapter::apply_thread_state_action_plan(&state, &auth, &plan).await?)
}
