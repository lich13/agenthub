use super::{router, test_support::sandboxed_test_config};
use agenthub_core::db::PanelDb;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

fn authenticated_test_state() -> (crate::state::AppState, String) {
    let (config, sandbox_root) = sandboxed_test_config();
    let db = PanelDb::open(":memory:").unwrap();
    let api_key = db.rotate_admin_api_key().unwrap();
    (
        crate::state::AppState::new(config, db).with_test_sandbox(sandbox_root),
        api_key,
    )
}

async fn request_path_status(
    app: axum::Router,
    method: &str,
    uri: &str,
    api_key: Option<&str>,
) -> StatusCode {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(api_key) = api_key {
        builder = builder.header("x-api-key", api_key);
    }
    app.oneshot(builder.body(Body::from("{}")).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn legacy_rest_routes_all_return_404() {
    let (state, api_key) = authenticated_test_state();
    let app = router(state);

    for (method, uri) in [
        ("GET", "/"),
        ("GET", "/index.html"),
        ("GET", "/assets/index.js"),
        ("GET", "/login"),
        ("GET", "/agenthub/"),
        ("GET", "/agenthub/login"),
        ("GET", "/agenthub/assets/index.js"),
        ("POST", "/api/rpc/auth.login"),
        ("POST", "/api/rpc/auth.logout"),
        ("POST", "/api/rpc/auth.me"),
        ("POST", "/api/rpc/auth.publicSettings"),
        ("POST", "/api/rpc/security.get"),
        ("POST", "/api/rpc/security.save"),
        ("POST", "/api/rpc/security.changePassword"),
        ("POST", "/api/rpc/threadEvents"),
        ("GET", "/api/rpc/threadEvents/thread-a"),
        ("GET", "/api/threads"),
        ("GET", "/api/threads/thread-a"),
        ("POST", "/api/threads/thread-a/messages"),
        ("POST", "/api/threads/thread-a/stop"),
        ("POST", "/api/threads/thread-a/archive"),
        ("POST", "/api/threads/thread-a/restore"),
        ("PATCH", "/api/threads/thread-a"),
        ("GET", "/api/threads/thread-a/followups"),
        ("POST", "/api/threads/thread-a/followups"),
        ("POST", "/api/threads/thread-a/followups/followup-a/cancel"),
        ("GET", "/api/probe/status"),
        ("GET", "/agenthub/api/probe/status"),
        ("POST", "/api/system/update/precheck"),
        ("POST", "/api/system/update/install"),
        ("POST", "/api/system/update/prune"),
        ("POST", "/api/system/panel/update/precheck"),
        ("POST", "/api/system/panel/update/start"),
        ("POST", "/api/system/panel/update/prune"),
        ("POST", "/api/system/codex/update/precheck"),
        ("POST", "/api/system/codex/update/start"),
        ("POST", "/api/system/codex/update/prune"),
        ("POST", "/api/system/update/start"),
        ("GET", "/api/system/status"),
        ("GET", "/api/jobs"),
        ("GET", "/api/uploads"),
        ("GET", "/api/security"),
        ("POST", "/api/auth/login"),
        ("GET", "/api/probe/diagnostics"),
        ("GET", "/agenthub/api/probe/diagnostics"),
        ("GET", "/api/probe/running"),
        ("GET", "/api/probe/reply-needed"),
        ("GET", "/api/probe/recoverable"),
        ("POST", "/api/probe/logs-db/plan"),
        ("POST", "/api/probe/logs-db/execute"),
        ("POST", "/api/probe/legacy-cleanup/dry-run"),
        ("POST", "/api/probe/legacy-cleanup/execute"),
        ("GET", "/api/probe/dashboard"),
        ("GET", "/agenthub/api/probe/dashboard"),
        ("GET", "/api/probe/thread-probe/thread-a"),
        ("POST", "/api/probe/lifecycle/repair"),
        ("POST", "/api/probe/service/restart"),
        ("POST", "/api/probe/legacy/import"),
        ("GET", "/api/sentinel/status"),
        ("GET", "/api/sentinel/dashboard"),
        ("GET", "/api/sentinel/running"),
        ("GET", "/api/sentinel/reply-needed"),
        ("GET", "/api/sentinel/recoverable"),
        ("GET", "/api/sentinel/thread-probe/thread-a"),
        ("GET", "/api/sentinel/hook-status"),
        ("GET", "/api/sentinel/logs-db/status"),
        ("POST", "/api/providers/claude-code/jobs/version-check"),
        ("POST", "/api/providers/claude-code/jobs/update/precheck"),
        ("POST", "/api/providers/claude-code/jobs/update/start"),
        ("POST", "/api/providers/claude-code/jobs/smoke"),
        ("POST", "/api/providers/claude-code/jobs/cache-status"),
        ("GET", "/api/jobs/job-a"),
        ("GET", "/api/cleanup/archive/dry-run"),
        ("POST", "/api/cleanup/archive/execute"),
        ("GET", "/api/cleanup/hidden/dry-run"),
        ("POST", "/api/cleanup/hidden/execute"),
        ("GET", "/api/no-such-route"),
    ] {
        let status = request_path_status(app.clone(), method, uri, Some(&api_key)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }
}

#[tokio::test]
async fn only_current_rpc_commands_and_health_are_available() {
    let (state, api_key) = authenticated_test_state();
    let app = router(state);

    let health = request_path_status(app.clone(), "GET", "/healthz", None).await;
    assert_eq!(health, StatusCode::OK);

    let upload =
        request_path_status(app.clone(), "POST", "/api/rpc/uploadFiles", Some(&api_key)).await;
    assert_eq!(upload, StatusCode::NOT_FOUND);

    let probe =
        request_path_status(app.clone(), "POST", "/api/rpc/probe.status", Some(&api_key)).await;
    assert_ne!(
        probe,
        StatusCode::NOT_FOUND,
        "/api/rpc/probe.status is the canonical Probe API route"
    );

    let event_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/rpc/threadEvents/thread-a")
                .header("x-api-key", &api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(event_response.status(), StatusCode::NOT_FOUND);

    let unknown = request_path_status(app, "GET", "/api/not-rpc", Some(&api_key)).await;
    assert_eq!(unknown, StatusCode::NOT_FOUND);
}
