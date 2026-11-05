// In-process harness for the existing business RPC regression fixtures; no network listener.
use super::rpc_dispatch::rpc_dispatch;
use crate::{rpc_surface::RPC_COMMAND_ROUTE, state::AppState};
use axum::{routing::post, Router};
pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route(RPC_COMMAND_ROUTE, post(rpc_dispatch))
        .with_state(state)
}
