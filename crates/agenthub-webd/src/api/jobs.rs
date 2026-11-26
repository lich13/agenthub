use super::{api_error, ok, ApiResponse};
use crate::state::AppState;
use agenthub_core::services::use_cases::{AgentHubUseCases, JobDetailRequest, LimitU32Request};

pub(crate) async fn list_jobs(state: AppState, query: LimitU32Request) -> ApiResponse {
    ok(AgentHubUseCases::new(state.platform())
        .jobs()
        .list_from_db(&state.db, query.limit)?)
}

pub(crate) async fn job_detail(state: AppState, request: JobDetailRequest) -> ApiResponse {
    match AgentHubUseCases::new(state.platform())
        .jobs()
        .detail_from_db(&state.db, &request.id, request.include_output)?
    {
        Some(job) => ok(job),
        None => Err(api_error("job not found")),
    }
}
