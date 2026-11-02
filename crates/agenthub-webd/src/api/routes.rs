use super::{api_error, rpc_dispatch::rpc_dispatch, ApiResponse};
use crate::{
    rpc_surface::{LEGACY_API_FALLBACK_ROUTE, RPC_COMMAND_ROUTE},
    state::AppState,
};
use axum::{
    http::StatusCode,
    routing::{any, get, post},
    Router,
};
use serde_json::json;
use tower_http::compression::{
    predicate::{DefaultPredicate, Predicate, SizeAbove},
    CompressionLayer,
};

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route(RPC_COMMAND_ROUTE, post(rpc_dispatch))
        .route(LEGACY_API_FALLBACK_ROUTE, any(api_not_found))
        .layer(
            CompressionLayer::new()
                .gzip(true)
                .compress_when(DefaultPredicate::new().and(SizeAbove::new(1024))),
        )
        .with_state(state)
}

async fn healthz() -> ApiResponse {
    super::ok(json!({"ok": true}))
}

async fn api_not_found() -> ApiResponse {
    Err(api_error(StatusCode::NOT_FOUND, "not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenthub_core::db::{NewProbeEvent, PanelDb};
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn router_gzip_compresses_large_json_responses_and_skips_tiny_payloads() {
        let db = PanelDb::open(":memory:").unwrap();
        let api_key = db.rotate_admin_api_key().unwrap();
        for idx in 0..8 {
            let body = "compressible payload segment ".repeat(12);
            let dedupe = format!("gzip-test-{idx}");
            db.record_probe_event(NewProbeEvent {
                kind: "completion",
                thread_id: Some("thread-gzip"),
                title: Some("Large event payload"),
                message: Some(&body),
                dedupe_key: Some(&dedupe),
                source: "test",
                payload: serde_json::json!({"index": idx}),
            })
            .unwrap();
        }
        let (config, sandbox_root) = crate::api::test_support::sandboxed_test_config();
        let app = router(AppState::new(config, db).with_test_sandbox(sandbox_root));

        let small = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(small.status(), StatusCode::OK);
        assert!(small.headers().get(header::CONTENT_ENCODING).is_none());

        let large = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc/probe.events")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-api-key", api_key)
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(large.status(), StatusCode::OK);
        assert_eq!(
            large
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("gzip")
        );
        let compressed = to_bytes(large.into_body(), 1024 * 1024).await.unwrap();
        assert!(compressed.starts_with(&[0x1f, 0x8b]));
    }
}
