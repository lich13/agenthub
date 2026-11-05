use super::{
    load_probe_threads, probe_config_path,
    test_support::{
        sandboxed_test_config, source_line_count, write_executable_fixture, AsyncEnvGuard,
    },
    update_service, UpdateAction,
};
use super::{rpc_dispatch::rpc_dispatch, ApiErrorKind};
use agenthub_core::codex::{MessageBlock, ThreadDetail, ThreadStatus, ThreadSummary};
use agenthub_core::{
    config::Config,
    db::{JobRecord, PanelDb},
    jobs::ProcessCommand,
    platform::{PlatformKind, PlatformPaths},
    services::{
        app_server_threads::{
            app_server_detail_from_read, app_server_thread_summaries,
            apply_app_server_thread_detail, merge_thread_summaries, thread_title,
        },
        probe::{probe_process_command, ProbeAction, PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS},
        threads as thread_service,
    },
};
use axum::http::StatusCode;
use rusqlite::{params, Connection};
use serde_json::json;
use std::collections::HashSet;
use std::{
    env,
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn api_facade_stays_under_line_budget() {
    assert!(
        source_line_count("api.rs") < 260,
        "api.rs should stay a thin facade; move tests and domain logic into api/* modules"
    );
}

fn temp_test_dir(prefix: &str) -> PathBuf {
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    env::temp_dir().join(format!("{}-{}-{unique}", prefix, std::process::id()))
}

fn mark_codex_home(home: &std::path::Path) {
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::write(home.join("state_5.sqlite"), b"").unwrap();
    fs::write(home.join("session_index.jsonl"), b"").unwrap();
    fs::create_dir_all(home.join("app-server-control")).unwrap();
}

fn init_isolated_codex_home(codex_home: &std::path::Path) {
    fs::create_dir_all(codex_home.join("sessions")).unwrap();
    fs::create_dir_all(codex_home.join("app-server-control")).unwrap();
    fs::write(codex_home.join("session_index.jsonl"), b"").unwrap();
    let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            archived INTEGER NOT NULL DEFAULT 0,
            archived_at INTEGER,
            source TEXT NOT NULL DEFAULT 'cli',
            thread_source TEXT NOT NULL DEFAULT 'user',
            cwd TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            first_user_message TEXT NOT NULL DEFAULT '',
            preview TEXT NOT NULL DEFAULT '',
            has_user_event INTEGER NOT NULL DEFAULT 1
        );",
    )
    .unwrap();
}

fn rpc_test_state() -> crate::state::AppState {
    let mut config = Config::default();
    let sandbox_root = temp_test_dir("agenthub-test-sandbox");
    let codex_home = sandbox_root.join(".codex");
    init_isolated_codex_home(&codex_home);
    config.codex.home = codex_home;
    config.codex.workspace = sandbox_root.join("workspace");
    config.paths.data_dir = sandbox_root.join("data");
    config.paths.db_path = sandbox_root.join("data/panel.sqlite");
    config.paths.log_dir = sandbox_root.join("logs");
    config.codex.bridge_enabled = false;
    config.update.panel_precheck_command =
        "/usr/local/bin/agenthub-webd-update --precheck".to_string();
    config.update.panel_update_command =
        "/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest".to_string();

    let db = PanelDb::open(":memory:").unwrap();
    crate::state::AppState::new(config, db).with_test_sandbox(sandbox_root)
}

fn rpc_test_state_with_config_file() -> (crate::state::AppState, PathBuf, PathBuf) {
    let state = rpc_test_state();
    let dir = temp_test_dir("agenthub-config");
    fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    fs::write(
        &config_path,
        toml::to_string_pretty(&state.config()).unwrap(),
    )
    .unwrap();
    (state, dir, config_path)
}

fn fallback_summary(id: &str, title: &str) -> ThreadSummary {
    ThreadSummary {
        activity_updated_at: None,
        storage_size: None,
        id: id.to_string(),
        title: title.to_string(),
        status: ThreadStatus::Recent,
        updated_at: None,
        archived_at: None,
        message_count: 0,
        latest_message: None,
        cwd: None,
        model: None,
        rollout_path: None,
        active_turn_id: None,
        active_job_id: None,
        pending_elicitation: None,
        last_event_kind: None,
    }
}

fn seed_local_codex_thread(home: &std::path::Path, thread_id: &str, title: &str) -> PathBuf {
    fs::create_dir_all(home).unwrap();
    mark_codex_home(home);
    let rollout = home.join(format!("{thread_id}.jsonl"));
    fs::write(
        &rollout,
        [
            json!({"session_meta":{"payload":{"id":thread_id}}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"local detail body"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            archived INTEGER NOT NULL DEFAULT 0,
            archived_at INTEGER,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO threads(id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, archived, archived_at, preview)
         VALUES(?1, ?2, 1, 2, 'codex', '', '/tmp', ?3, '', '', 0, NULL, 'preview should not replace title')",
        (thread_id, rollout.display().to_string(), title),
    )
    .unwrap();
    rollout
}

fn app_server_missing_socket_state() -> (crate::state::AppState, PathBuf) {
    let state = rpc_test_state();
    let home = temp_test_dir("agenthub-local-codex");
    let mut config = state.config();
    config.codex.home = home.clone();
    config.codex.workspace = home.clone();
    config.codex.bridge_enabled = true;
    config.codex.app_server_socket = Some(home.join("missing-app-server.sock"));
    state.replace_config(config);
    (state, home)
}

async fn request_rpc_json(
    state: crate::state::AppState,
    command: &str,
    body: &str,
) -> serde_json::Value {
    rpc_dispatch(state, command.into(), serde_json::from_str(body).unwrap())
        .await
        .unwrap()
}
async fn request_rpc_status(
    state: crate::state::AppState,
    command: &str,
    body: &str,
) -> Result<(), ApiErrorKind> {
    let args = serde_json::from_str(body).map_err(|_| ApiErrorKind::InvalidRequest)?;
    rpc_dispatch(state, command.into(), args)
        .await
        .map(|_| ())
        .map_err(|error| error.kind)
}

#[tokio::test]
async fn thread_routes_use_local_state_when_app_server_socket_is_missing() {
    let (mut state, home) = app_server_missing_socket_state();
    seed_local_codex_thread(&home, "thread-a", "local title");
    let executable = home.join("native-codex-fixture");
    write_executable_fixture(
        &executable,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.153.4"
  exit 0
fi
exec node -e '
const fs = require("node:fs");
const path = require("node:path");
const { DatabaseSync } = require("node:sqlite");
const db = new DatabaseSync(path.join(process.env.CODEX_HOME, "state_5.sqlite"));
require("node:readline").createInterface({ input: process.stdin }).on("line", line => {
  fs.appendFileSync(path.join(process.env.CODEX_HOME, "native-requests.jsonl"), line + "\n");
  const request = JSON.parse(line);
  if (request.id == null) return;
  let result = {};
  if (request.method === "thread/name/set") {
    db.prepare("UPDATE threads SET title=? WHERE id=?").run(request.params.name, request.params.threadId);
    fs.appendFileSync(path.join(process.env.CODEX_HOME, "session_index.jsonl"), JSON.stringify({ id:request.params.threadId, thread_name:request.params.name }) + "\n");
  } else if (request.method === "thread/read") {
    result = { thread: db.prepare("SELECT id,title AS name FROM threads WHERE id=?").get(request.params.threadId) };
  } else if (request.method !== "initialize") process.exit(2);
  console.log(JSON.stringify({ id: request.id, result }));
});
' "$@"
"#,
    );
    state.app_server_client = agenthub_core::codex::CodexAppServerClient::with_candidates(
        vec![executable],
        std::time::Duration::from_secs(5),
    );
    let app = state.clone();

    let list = request_rpc_json(app.clone(), "threads.list", r#"{"limit":10}"#).await;
    assert_eq!(list[0]["id"], "thread-a");
    assert_eq!(list[0]["title"], "local title");

    let detail = request_rpc_json(app.clone(), "threads.detail", r#"{"id":"thread-a"}"#).await;
    assert_eq!(detail["summary"]["title"], "local title");

    assert!(!home.join("native-requests.jsonl").exists());

    let _ = request_rpc_json(
        app,
        "threads.rename",
        r#"{"threadId":"thread-a","name":"local renamed"}"#,
    )
    .await;

    let rows = agenthub_core::codex::list_threads(
        &agenthub_core::codex::CodexPaths::new(&home),
        false,
        Some("thread-a"),
        10,
    )
    .unwrap();
    assert_eq!(rows[0].title, "local renamed");
    let requests: Vec<serde_json::Value> = fs::read_to_string(home.join("native-requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        requests
            .iter()
            .map(|request| request["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "initialize",
            "initialized",
            "thread/name/set",
            "thread/read"
        ]
    );
    state.app_server_client = agenthub_core::codex::CodexAppServerClient::with_candidates(
        vec![],
        std::time::Duration::from_secs(2),
    );
    assert_eq!(
        request_rpc_status(
            state,
            "threads.rename",
            r#"{"threadId":"thread-a","name":"must not use SQL"}"#
        )
        .await,
        Err(ApiErrorKind::OperationFailed)
    );
    let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("SELECT title FROM threads WHERE id='thread-a'", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "local renamed"
    );
    drop(conn);
    let _ = fs::remove_dir_all(home);
}

#[tokio::test]
async fn probe_threads_use_local_state_when_app_server_socket_is_missing() {
    let (state, home) = app_server_missing_socket_state();
    seed_local_codex_thread(&home, "thread-a", "local title");

    let rows = load_probe_threads(&state, "recent", 10).await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "thread-a");
    assert_eq!(rows[0].title, "local title");
    let _ = fs::remove_dir_all(home);
}

#[tokio::test]
async fn rpc_probe_typed_commands_start_matching_jobs() {
    let _env_guard = AsyncEnvGuard::lock_read_only().await;
    for (command, kind) in [
        ("probe.barkTest", "probe_bark_test"),
        ("probe.installHooks", "probe_hooks_install"),
        ("probe.removeHooks", "probe_hooks_remove"),
    ] {
        let state = rpc_test_state();
        let app = state.clone();
        let payload = request_rpc_json(app, command, "{}").await;
        let job_id = payload["job_id"].as_str().unwrap();
        let job = state.db.job(job_id).unwrap().unwrap();
        assert_eq!(job.kind, kind, "{command}");
    }
}

#[tokio::test]
async fn rpc_update_typed_commands_start_update_jobs() {
    for (command, kind) in [
        ("updates.check", "agenthub_update_check"),
        ("updates.install", "agenthub_update_install"),
        ("updates.prune", "agenthub_update_prune"),
    ] {
        let rpc_state = rpc_test_state();
        let rpc = request_rpc_json(rpc_state.clone(), command, "{}").await;
        let job_id = rpc["job_id"].as_str().unwrap();
        let job = rpc_state.db.job(job_id).unwrap().unwrap();
        assert_eq!(job.kind, kind, "{command}");
    }
}

#[tokio::test]
async fn rpc_cleanup_execute_requires_confirmed_targets() {
    let state = rpc_test_state();
    let app = state;

    for command in ["cleanup.archiveExecute", "cleanup.hiddenExecute"] {
        let status = request_rpc_status(app.clone(), command, r#"{"confirmed":true}"#).await;
        assert_eq!(status, Err(ApiErrorKind::InvalidRequest), "{command}");
    }
}

#[tokio::test]
async fn rpc_hidden_cleanup_execute_requires_preview_candidates() {
    let state = rpc_test_state();
    let app = state;

    let missing_candidates = request_rpc_status(
        app.clone(),
        "cleanup.hiddenExecute",
        r#"{"confirmed":true,"expectedCount":0}"#,
    )
    .await;
    assert_eq!(missing_candidates, Err(ApiErrorKind::InvalidRequest));

    let mismatched_count = request_rpc_status(
        app,
        "cleanup.hiddenExecute",
        r#"{"confirmed":true,"expectedCount":1,"candidates":[]}"#,
    )
    .await;
    assert_eq!(mismatched_count, Ok(()));
}

#[test]
fn linux_probe_actions_route_through_shared_core_adapter() {
    let api_source = include_str!("probe.rs")
        .split("\nmod tests {")
        .next()
        .expect("api/probe.rs source must include production section");
    let adapter_source = include_str!("../linux_adapter.rs");

    assert!(
        api_source.contains("linux_adapter::linux_probe_action_plan"),
        "Linux HTTP Probe handlers should request the shared core Probe plan through the adapter"
    );
    assert!(
        api_source.contains("linux_adapter::start_probe_action_plan"),
        "Linux HTTP Probe handlers should delegate Linux execution to the adapter"
    );
    assert!(
        !api_source.contains("patch_probe_config_toml"),
        "api.rs must not patch Probe config TOML directly"
    );
    assert!(
        !api_source.contains("start_exclusive_shell_job(&spec.kind"),
        "api.rs must not start Probe shell jobs directly"
    );
    for forbidden in [
        "state.jobs.start_codex_job(",
        "state.jobs.cancel_job(",
        "codex::set_thread_archived(",
        "codex::set_thread_title(",
        "start_shell_job(&spec.kind",
    ] {
        assert!(
            !api_source.contains(forbidden),
            "api.rs must execute host-specific thread/job plans through linux_adapter: {forbidden}"
        );
    }
    assert!(
        adapter_source.contains("patch_probe_config_toml"),
        "Linux adapter is the only agenthub-webd module that applies Probe settings TOML patches"
    );
    assert!(
        adapter_source.contains("ProbeUseCases::new"),
        "Linux adapter should request Probe action plans through ProbeUseCases"
    );
    assert!(
        adapter_source.contains("UpdateUseCases::new"),
        "Linux adapter should request update action plans through UpdateUseCases"
    );
}

#[test]
fn linux_probe_process_command_preserves_paths_as_literal_arguments() {
    let platform = PlatformPaths::for_kind_with_home(
        PlatformKind::Macos,
        std::path::Path::new("/synthetic/runtime security/home with spaces"),
    );
    let config_path =
        PathBuf::from("/synthetic/runtime security/config path with spaces; fixture.toml");
    let cases = [
        (ProbeAction::BarkTest, "bark-test"),
        (ProbeAction::InstallHooks, "hooks-install"),
        (ProbeAction::RemoveHooks, "hooks-remove"),
    ];

    for (action, subcommand) in cases {
        let process = probe_process_command(&platform, &config_path, action);
        assert_eq!(process.program, platform.daemon_binary());
        assert_eq!(
            process,
            ProcessCommand::new(
                platform.daemon_binary(),
                [
                    OsString::from("--config"),
                    config_path.as_os_str().to_owned(),
                    OsString::from("probe"),
                    OsString::from(subcommand),
                ],
            )
        );
    }
}

#[test]
fn linux_entry_delegates_to_current_core_plans() {
    let source = include_str!("../api.rs")
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .expect("api source must include production section");
    let cleanup_source = include_str!("cleanup.rs");
    let jobs_source = include_str!("jobs.rs");
    let threads_source = include_str!("threads.rs");
    let handler_source = format!("{source}\n{cleanup_source}\n{jobs_source}\n{threads_source}");
    let adapter_source = include_str!("../linux_adapter.rs");

    for required in [
        "AgentHubUseCases::new",
        "linux_adapter::apply_thread_state_action_plan",
        "linux_adapter::execute_cleanup_plan",
        "linux_adapter::list_jobs_plan",
        "linux_adapter::job_detail_plan",
    ] {
        assert!(
            handler_source.contains(required),
            "Linux RPC handlers must call the shared core facade/plan: {required}"
        );
    }
    for required_adapter_landing in [
        "AgentHubUseCases::new(&platform).cleanup()",
        ".dry_run_archived(",
        ".execute_archived(",
        ".dry_run_hidden(",
        ".execute_hidden(",
    ] {
        assert!(
            adapter_source.contains(required_adapter_landing),
            "Linux adapter must retain cleanup side-effect landing: {required_adapter_landing}"
        );
    }

    let rpc_dispatch_source = include_str!("rpc_dispatch.rs");
    assert!(
        !rpc_dispatch_source.contains("ArchiveExecuteRequest { confirmed: true }"),
        "Linux RPC cleanup execute must preserve confirmed/expectedCount from the request payload"
    );
}

#[tokio::test]
async fn rpc_runtime_capabilities_has_minimal_dto_shape() {
    let state = rpc_test_state();
    let app = state;

    let rpc = request_rpc_json(app, "system.capabilities", "{}").await;

    assert_eq!(rpc["api_version"], 2);
    assert_eq!(rpc["host_surface"], "linux_server_api");
    assert_eq!(rpc["capabilities"]["threads"], true);
    assert!(rpc["capabilities"].get("web_auth").is_none());
    assert!(rpc["capabilities"].get("turnstile").is_none());
    assert_eq!(rpc["capabilities"]["systemd"], true);
    assert!(rpc["capabilities"].get("nginx").is_none());
    assert_eq!(rpc["capabilities"]["linux_update_job"], true);
}

#[tokio::test]
async fn rpc_update_typed_actions_start_jobs() {
    for (command, kind) in [
        ("updates.check", "agenthub_update_check"),
        ("updates.install", "agenthub_update_install"),
        ("updates.prune", "agenthub_update_prune"),
    ] {
        let state = rpc_test_state();
        let rpc = request_rpc_json(state.clone(), command, "{}").await;
        let job_id = rpc["job_id"].as_str().unwrap();
        let job = state.db.job(job_id).unwrap().unwrap();
        assert_eq!(job.kind, kind, "{command}");
    }
}

#[tokio::test]
async fn rpc_update_status_uses_shared_update_status_shape() {
    let state = rpc_test_state();
    let app = state;

    let rpc = request_rpc_json(app, "updates.status", "{}").await;

    assert_eq!(rpc["method"], "linux_systemd_job");
    assert_eq!(rpc["state"], "idle");
}

#[tokio::test]
async fn panel_update_starts_fixed_panel_jobs() {
    for (command, kind, title) in [
        (
            "updates.check",
            "agenthub_update_check",
            "AgentHub update precheck",
        ),
        (
            "updates.install",
            "agenthub_update_install",
            "AgentHub update install",
        ),
        (
            "updates.prune",
            "agenthub_update_prune",
            "AgentHub update backup prune",
        ),
    ] {
        let state = rpc_test_state();
        let app = state.clone();

        let payload = request_rpc_json(app.clone(), command, "{}").await;
        let job_id = payload["job_id"].as_str().unwrap();
        let job = state.db.job(job_id).unwrap().unwrap();

        assert_eq!(job.kind, kind, "{command}");
        assert_eq!(job.title, title, "{command}");
    }
}

#[test]
fn linux_update_adapter_builds_process_job_specs_outside_core_service() {
    let mut config = Config::for_platform_kind(PlatformKind::Linux);
    config.update.panel_precheck_command =
        "/usr/local/bin/agenthub-webd-update --precheck".to_string();
    config.update.panel_update_command =
        "/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest".to_string();
    let platform = PlatformPaths::for_kind(PlatformKind::Linux);

    let precheck = update_service::linux_update_job_spec(
        &config,
        update_service::update_action_plan(&platform, UpdateAction::Check),
    )
    .unwrap();
    assert_eq!(precheck.kind, "agenthub_update_check");
    assert_eq!(
        precheck.command,
        "/usr/local/bin/agenthub-webd-update --precheck"
    );
    assert_eq!(
        precheck.process_command().unwrap(),
        ProcessCommand::new("/usr/local/bin/agenthub-webd-update", ["--precheck"])
    );

    let install = update_service::linux_update_job_spec(
        &config,
        update_service::update_action_plan(&platform, UpdateAction::Install),
    )
    .unwrap();
    assert_eq!(install.kind, "agenthub_update_install");
    assert_eq!(
        install.command,
        "/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest"
    );
    assert_eq!(
        install.process_command().unwrap(),
        ProcessCommand::new(
            "/usr/local/bin/agenthub-webd-update",
            ["--repo", "lich13/agenthub", "--version", "latest"]
        )
    );

    let prune = update_service::linux_update_job_spec(
        &config,
        update_service::update_action_plan(&platform, UpdateAction::Prune),
    )
    .unwrap();
    assert_eq!(prune.kind, "agenthub_update_prune");
    assert!(prune.command.contains("release update backups"));
}

#[tokio::test]
async fn unified_update_status_uses_shared_shape() {
    let state = rpc_test_state();
    let app = state.clone();

    let payload = request_rpc_json(app, "updates.status", "{}").await;
    assert_eq!(payload["method"], "linux_systemd_job");
    assert_eq!(payload["state"], "idle");
    assert_eq!(payload["channel"], "stable");
    assert!(payload["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "job_history"));
}

#[tokio::test]
async fn runtime_capabilities_preserve_linux_features() {
    let state = rpc_test_state();
    let app = state;

    let payload = request_rpc_json(app, "system.capabilities", "{}").await;
    assert_eq!(payload["host_surface"], "linux_server_api");
    assert_eq!(payload["api_version"], 2);
    assert_eq!(payload.as_object().unwrap().len(), 3);
    assert_eq!(payload["capabilities"]["threads"], true);
    assert_eq!(payload["capabilities"]["jobs"], true);
    assert_eq!(payload["capabilities"]["probe"], true);
    assert!(payload["capabilities"].get("status").is_none());
    assert_eq!(payload["capabilities"]["settings"], true);
    assert_eq!(payload["capabilities"]["job_history"], true);
    assert!(payload["capabilities"].get("web_auth").is_none());
    assert!(payload["capabilities"].get("security_settings").is_none());
    assert!(payload["capabilities"].get("turnstile").is_none());
    assert_eq!(payload["capabilities"]["systemd"], true);
    assert!(payload["capabilities"].get("nginx").is_none());
    assert!(payload["capabilities"].get("public_endpoint").is_none());
    assert!(payload["capabilities"].get("admin_password").is_none());
    assert_eq!(payload["capabilities"]["linux_update_job"], true);
    let text = serde_json::to_string(&payload).unwrap();
    for forbidden in [
        "LaunchAgent",
        "launchagent",
        "Cloudflare Tunnel",
        "cloudflare_tunnel",
        "macos_web_login",
        "browser_webui",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
}

#[tokio::test]
async fn probe_status_routes_use_canonical_probe_name_without_sentinel_alias() {
    let state = rpc_test_state();
    let dir = temp_test_dir("agenthub-probe-status-resolved");
    let codex_home = dir.join(".codex");
    mark_codex_home(&codex_home);
    let mut config = state.config();
    config.codex.home = codex_home.clone();
    state.replace_config(config);
    let app = state.clone();

    let status = request_rpc_json(app.clone(), "probe.status", "{}").await;
    assert_eq!(status["label"], "Probe");
    assert_ne!(status["label"], "Sentinel");
    assert!(status["flavor"].as_str().is_some());
    assert!(status["hook_status"].as_str().is_some());
    assert!(status.get("logs_db_status").is_none());
    assert_eq!(status["codex_home"], codex_home.to_string_lossy().as_ref());
    assert_eq!(
        status["configured_codex_home"],
        codex_home.to_string_lossy().as_ref()
    );
    assert_eq!(
        status["resolved_codex_home"],
        codex_home.to_string_lossy().as_ref()
    );
    assert_eq!(status["codex_home_source"], "configured");
    assert_eq!(status["logs_db_source"], "configured");
    assert!(status["discovery_warnings"].as_array().unwrap().is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn probe_status_route_caches_initial_snapshot_and_throttles_background_refresh() {
    let state = rpc_test_state();
    let dir = temp_test_dir("agenthub-probe-status-snapshot");
    let codex_home = dir.join(".codex");
    mark_codex_home(&codex_home);
    let mut config = state.config();
    config.codex.home = codex_home.clone();
    state.replace_config(config);
    let app = state.clone();

    let first_status = request_rpc_json(app.clone(), "probe.status", "{}").await;
    assert_eq!(first_status["label"], "Probe");
    assert_eq!(first_status["snapshot_status"], "initial");
    assert_eq!(first_status["is_refreshing"], false);
    assert_eq!(first_status["snapshot_age_seconds"], 0);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let second_status = request_rpc_json(app.clone(), "probe.status", "{}").await;
    assert_eq!(second_status["snapshot_status"], "cached");
    assert_eq!(second_status["is_refreshing"], false);
    assert!(second_status["snapshot_age_seconds"].as_i64().unwrap() < 2);
    assert!(second_status["running_threads"].as_array().is_some());
    assert!(second_status["reply_needed_threads"].as_array().is_some());
    assert!(second_status["recoverable_threads"].as_array().is_some());

    state
        .probe_status_cache
        .lock()
        .unwrap()
        .snapshot
        .as_mut()
        .unwrap()
        .refreshed_at_unix -= 3;
    let third_status = request_rpc_json(app, "probe.status", "{}").await;
    assert_eq!(third_status["snapshot_status"], "cached");
    assert_eq!(third_status["is_refreshing"], true);
    assert!(third_status["snapshot_age_seconds"].as_i64().unwrap() >= 2);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn probe_reply_needed_bucket_only_includes_fresh_pending_actions() {
    let state = rpc_test_state();
    let dir = temp_test_dir("agenthub-probe-reply-needed-fresh");
    let codex_home = dir.join(".codex");
    mark_codex_home(&codex_home);
    let now = chrono::Utc::now().timestamp();
    let old_updated = now - PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS - 60;
    let fresh_updated = now - 30;
    let old_rollout = codex_home.join("old-plan.jsonl");
    let fresh_rollout = codex_home.join("fresh-plan.jsonl");
    let plan_events = |thread_id: &str, turn_id: &str| {
        [
            json!({"session_meta":{"payload":{"id":thread_id}}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":thread_id,"turn_id":turn_id,"item":{"type":"Plan","id":format!("{turn_id}-plan"),"text":"# 计划\n- 等待确认"}}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 计划\n- 等待确认\n</proposed_plan>"}],"phase":"final_answer"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":turn_id,"last_agent_message":null}}).to_string(),
        ]
        .join("\n")
    };
    fs::write(&old_rollout, plan_events("old-thread", "turn-old")).unwrap();
    fs::write(&fresh_rollout, plan_events("fresh-thread", "turn-fresh")).unwrap();
    let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads(
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            preview TEXT NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES('old-thread', ?1, ?2, ?2, 'codex', '/tmp', '旧计划', '')",
        params![old_rollout.display().to_string(), old_updated],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
         VALUES('fresh-thread', ?1, ?2, ?2, 'codex', '/tmp', '新计划', '')",
        params![fresh_rollout.display().to_string(), fresh_updated],
    )
    .unwrap();
    let mut config = state.config();
    config.codex.home = codex_home;
    state.replace_config(config);

    let rows = load_probe_threads(&state, "reply-needed", 20)
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "fresh-thread");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn probe_events_route_lists_recent_events_with_auth_and_redacts_sensitive_payloads() {
    let state = rpc_test_state();
    state
        .db
        .record_probe_event(agenthub_core::db::NewProbeEvent {
            kind: "hook-stop",
            thread_id: Some("thread-a"),
            title: Some("Hook event"),
            message: Some("stop hook received"),
            dedupe_key: Some("hook-stop:thread-a"),
            source: "test",
            payload: json!({
                "session_id": "session-a",
                "transcript_path": "/tmp/turn.json",
                "last_assistant_message": {
                    "summary": "hello",
                    "sha256": "abc123"
                },
                "body_summary": "<proposed_plan>\n# Safe summary\n</proposed_plan>",
                "body_sha256": "abc123",
                "body_length": 9876,
                "body_source": "last_assistant_message",
                "body_truncated": true,
                "device_key": "secret-device",
                "bark": {
                    "title": "等待回复：Hook event",
                    "chunk_count": 3,
                    "request_count": 3,
                    "sent": true,
                    "http_status": 200,
                    "request_url": "https://api.day.app/secret-device/title",
                    "device_key_configured": true,
                    "dedupe_key": "hook-stop:thread-a",
                    "body": "完整 Bark 正文不应返回"
                },
                "nested": {
                    "token": "abc",
                    "ok": true,
                    "headers": [{ "Authorization": "Bearer abc" }]
                }
            }),
        })
        .unwrap();
    let app = state;

    let payload = request_rpc_json(app, "probe.events", r#"{"limit":1}"#).await;
    assert_eq!(payload["limit"], 1);
    assert_eq!(payload["events"].as_array().unwrap().len(), 1);
    let event = &payload["events"][0];
    assert_eq!(event["kind"], "hook-stop");
    assert_eq!(event["payload"]["device_key"], "[redacted]");
    assert_eq!(event["payload"]["bark"]["request_url"], "[redacted]");
    assert_eq!(event["payload"]["bark"]["device_key_configured"], true);
    assert_eq!(event["payload"]["bark"]["dedupe_key"], "hook-stop:thread-a");
    assert_eq!(event["payload"]["bark"]["title"], "等待回复：Hook event");
    assert_eq!(event["payload"]["bark"]["chunk_count"], 3);
    assert_eq!(event["payload"]["bark"]["request_count"], 3);
    assert!(event["payload"]["bark"].get("body").is_none());
    assert_eq!(event["payload"]["body_summary"], "# Safe summary");
    assert_eq!(event["payload"]["body_sha256"], "abc123");
    assert_eq!(event["payload"]["body_length"], 9876);
    assert_eq!(event["payload"]["body_source"], "last_assistant_message");
    assert_eq!(event["payload"]["body_truncated"], true);
    assert_eq!(event["payload"]["nested"]["token"], "[redacted]");
    assert_eq!(
        event["payload"]["nested"]["headers"][0]["Authorization"],
        "[redacted]"
    );
    assert_eq!(event["payload"]["session_id"], "session-a");
    assert_eq!(event["payload"]["transcript_path"], "/tmp/turn.json");
    assert_eq!(
        event["payload"]["last_assistant_message"]["summary"],
        "hello"
    );
    assert!(!serde_json::to_string(event)
        .unwrap()
        .contains("完整 Bark 正文不应返回"));
    assert!(!serde_json::to_string(event)
        .unwrap()
        .contains("<proposed_plan>"));
}

#[tokio::test]
async fn probe_settings_rejects_invalid_server_url_before_storing_device_key() {
    let state = rpc_test_state();
    let dir = temp_test_dir("agenthub-probe-settings-invalid-url");
    fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    let mut config = state.config();
    config.security.secret_key = state.config().security.secret_key.clone();
    config.paths.db_path = state.config().paths.db_path.clone();
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    let _config_env = AsyncEnvGuard::set_config(&config_path).await;
    let app = state.clone();

    let status = request_rpc_status(
        app,
        "probe.settings.save",
        r#"{"settings":{"notifications":{"server_url":"http://example.com","device_key":"secret-device"}}}"#)
    .await;

    assert_eq!(status, Err(ApiErrorKind::InvalidRequest));
    assert!(state
        .db
        .get_secret_setting_bytes("probe_bark_device_key")
        .unwrap()
        .is_none());
    assert!(!fs::read_to_string(probe_config_path())
        .unwrap()
        .contains("http://example.com"));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn probe_actions_expose_safe_probe_surfaces() {
    let _env_guard = AsyncEnvGuard::lock_read_only().await;
    for (command, kind, title, forbidden_arg) in [
        (
            "probe.installHooks",
            "probe_hooks_install",
            "探针 Hook 安装",
            "codex-sentinel",
        ),
        (
            "probe.removeHooks",
            "probe_hooks_remove",
            "探针 Hook 移除",
            "codex-sentinel",
        ),
        (
            "probe.barkTest",
            "probe_bark_test",
            "AgentHub 推送测试",
            "device_key",
        ),
    ] as [(&str, &str, &str, &str); 3]
    {
        let state = rpc_test_state();
        let app = state.clone();

        let payload = request_rpc_json(app.clone(), command, "{}").await;
        let job_id = payload["job_id"].as_str().unwrap();
        let job = state.db.job(job_id).unwrap().unwrap();
        assert_eq!(job.kind, kind);
        assert_eq!(job.title, title);
        assert!(!job.output.contains(forbidden_arg));
    }

    let state = rpc_test_state();
    let app = state;
    for command in [
        "probe.logsDb.status",
        "probe.logsDbDryRun",
        "probe.logsDbExecute",
    ] {
        let status = request_rpc_status(app.clone(), command, "{}").await;
        assert_eq!(status, Err(ApiErrorKind::NotFound));
    }
}

#[tokio::test]
async fn probe_settings_patch_rejects_missing_config_file() {
    let state = rpc_test_state();
    let dir = temp_test_dir("agenthub-missing-config");
    fs::create_dir_all(&dir).unwrap();
    let missing_path = dir.join("missing-config.toml");
    let _config_env = AsyncEnvGuard::set_config(&missing_path).await;
    let app = state;

    let status = request_rpc_status(
        app,
        "probe.settings.save",
        r#"{"probe":{"poll_seconds":25}}"#,
    )
    .await;

    assert_eq!(status, Err(ApiErrorKind::InvalidRequest));
}

#[tokio::test]
async fn probe_settings_patch_refreshes_runtime_config_snapshots() {
    let (state, _dir, config_path) = rpc_test_state_with_config_file();
    let _config_env = AsyncEnvGuard::set_config(&config_path).await;
    let app = state.clone();

    let _ = request_rpc_json(
        app.clone(),
        "probe.settings.save",
        r#"{"codex":{"host_label":"fresh-host"},"probe":{"poll_seconds":33,"observability":{"event_retention_days":2}}}"#)
    .await;

    let settings = request_rpc_json(app.clone(), "probe.settings.get", "{}").await;
    assert_eq!(settings["probe"]["poll_seconds"], 33);
    assert_eq!(settings["codex"]["host_label"], "fresh-host");

    let status = request_rpc_json(app.clone(), "probe.status", "{}").await;
    assert_eq!(status["poll_seconds"], 33);
    assert_eq!(status["host_label"], "fresh-host");

    assert_eq!(
        settings["probe"]["observability"]["event_retention_days"],
        2
    );

    let payload = request_rpc_json(app, "probe.barkTest", "{}").await;
    let job_id = payload["job_id"].as_str().unwrap();
    let job = state.db.job(job_id).unwrap().unwrap();
    assert_eq!(job.kind, "probe_bark_test");
    let config_text = fs::read_to_string(&config_path).unwrap();
    assert!(config_text.contains("fresh-host"));
}

#[test]
fn thread_block_page_returns_latest_window_without_detail_fields() {
    let detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback_summary("thread-a", "example-user"),
        messages: vec![],
        blocks: (0..6)
            .map(|index| MessageBlock {
                subagent: None,
                user_message: None,
                id: format!("block-{index}"),
                role: "assistant".to_string(),
                kind: "message".to_string(),
                display_kind: None,
                status: None,
                text: Some(format!("message-{index}")),
                summary: None,
                input: None,
                truncated: None,
                resolved: None,
                answers: Vec::new(),
                plan_status: None,
                group_id: None,
                tool_name: None,
                call_id: None,
                turn_id: None,
                item_id: None,
                created_at: None,
                questions: Vec::new(),
                payload: None,
            })
            .collect(),
        raw_event_count: 6,
        total_blocks: 6,
        has_more_blocks: false,
        before_cursor: None,
    };

    let plan = thread_service::plan_thread_blocks_request(
        &PlatformPaths::for_kind(PlatformKind::Linux),
        "thread-a",
        Some(2),
        None,
    )
    .unwrap();
    let page = thread_service::thread_blocks_page_for_plan(detail, &plan);
    let value = serde_json::to_value(&page).unwrap();

    assert_eq!(page.thread_id, "thread-a");
    assert_eq!(page.total_blocks, 6);
    assert!(page.has_more_blocks);
    assert_eq!(page.before_cursor.as_deref(), Some("b:4"));
    assert_eq!(page.blocks.len(), 2);
    assert_eq!(page.blocks[0].text.as_deref(), Some("message-4"));
    assert_eq!(page.blocks[1].text.as_deref(), Some("message-5"));
    assert!(value.get("summary").is_none());
    assert!(value.get("messages").is_none());
    assert!(value.get("raw_event_count").is_none());
}

#[test]
fn thread_block_page_uses_before_cursor() {
    let detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback_summary("thread-a", "example-user"),
        messages: vec![],
        blocks: (0..6)
            .map(|index| MessageBlock {
                subagent: None,
                user_message: None,
                id: format!("block-{index}"),
                role: "assistant".to_string(),
                kind: "message".to_string(),
                display_kind: None,
                status: None,
                text: Some(format!("message-{index}")),
                summary: None,
                input: None,
                truncated: None,
                resolved: None,
                answers: Vec::new(),
                plan_status: None,
                group_id: None,
                tool_name: None,
                call_id: None,
                turn_id: None,
                item_id: None,
                created_at: None,
                questions: Vec::new(),
                payload: None,
            })
            .collect(),
        raw_event_count: 6,
        total_blocks: 6,
        has_more_blocks: false,
        before_cursor: None,
    };

    let plan = thread_service::plan_thread_blocks_request(
        &PlatformPaths::for_kind(PlatformKind::Linux),
        "thread-a",
        Some(2),
        Some("b:4".to_string()),
    )
    .unwrap();
    let page = thread_service::thread_blocks_page_for_plan(detail, &plan);

    assert_eq!(page.total_blocks, 6);
    assert!(page.has_more_blocks);
    assert_eq!(page.before_cursor.as_deref(), Some("b:2"));
    assert_eq!(page.blocks.len(), 2);
    assert_eq!(page.blocks[0].text.as_deref(), Some("message-2"));
    assert_eq!(page.blocks[1].text.as_deref(), Some("message-3"));
}

#[test]
fn thread_title_reads_explicit_name_without_using_preview() {
    assert_eq!(
        thread_title(&json!({
            "name": "example-user",
            "preview": "接手这个线程的工作 019e86d2..."
        })),
        Some("example-user".to_string())
    );
    assert_eq!(
        thread_title(&json!({
            "preview": "接手这个线程的工作 019e86d2..."
        })),
        None
    );
}

#[test]
fn app_server_metadata_updates_preserve_conversation_time_and_probe_freshness() {
    let mut fallback = fallback_summary("thread-a", "Fixture");
    fallback.updated_at = Some("2026-01-01T00:00:00Z".into());
    let response = json!({"threads":[{"id":"thread-a","updatedAt":1780824385}]});
    let rows = app_server_thread_summaries(&response, &[fallback]);
    assert_eq!(rows[0].updated_at.as_deref(), Some("2026-01-01T00:00:00Z"));
    assert!(rows[0].activity_updated_at.is_some());
    assert!(serde_json::to_value(&rows[0])
        .unwrap()
        .get("activity_updated_at")
        .is_none());
}

#[test]
fn app_server_thread_summary_keeps_fallback_title_when_only_preview_exists() {
    let fallback = vec![fallback_summary("thread-a", "example-user")];
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "preview": "接手这个线程的工作 019e86d2...",
                "updatedAt": 1780824385
            }]
        }),
        &fallback,
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "example-user");
    assert_eq!(
        rows[0].latest_message.as_deref(),
        Some("接手这个线程的工作 019e86d2...")
    );
}

#[test]
fn app_server_thread_detail_does_not_overwrite_title_with_preview() {
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback_summary("thread-a", "example-user"),
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };

    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "preview": "接手这个线程的工作 019e86d2...",
                "updatedAt": 1780824385,
                "cwd": "/home/ubuntu/codex-workspace"
            }
        }),
    );

    assert_eq!(detail.summary.title, "example-user");
    assert_eq!(
        detail.summary.cwd.as_deref(),
        Some("/home/ubuntu/codex-workspace")
    );
    assert_eq!(detail.summary.last_event_kind, None);
}

#[test]
fn app_server_thread_detail_does_not_overwrite_title_with_placeholder() {
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback_summary("thread-a", "example-user"),
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };

    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "name": "未命名线程",
                "status": { "type": "active" }
            }
        }),
    );

    assert_eq!(detail.summary.title, "example-user");
    assert_eq!(detail.summary.status, ThreadStatus::Running);
}

#[test]
fn app_server_thread_detail_updates_title_with_real_name() {
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback_summary("thread-a", "example-user"),
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };

    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "name": "example-user renamed"
            }
        }),
    );

    assert_eq!(detail.summary.title, "example-user renamed");
}

#[test]
fn app_server_active_status_overrides_stale_fallback_recent() {
    let fallback = vec![fallback_summary("thread-a", "example-user")];
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "active" }
            }]
        }),
        &fallback,
    );

    assert_eq!(rows[0].status, ThreadStatus::Running);
}

#[test]
fn app_server_recoverable_fallback_takes_priority_over_active_signal() {
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::Recoverable;
    fallback.rollout_path = Some(PathBuf::from("/tmp/rollout-thread-a.jsonl"));
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "active" },
                "activeTurnId": "turn-live"
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::Recoverable);
    assert_eq!(rows[0].active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn app_server_not_loaded_preserves_recoverable_fallback() {
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::Recoverable;
    fallback.rollout_path = Some(PathBuf::from("/tmp/rollout-thread-a.jsonl"));
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "notLoaded" },
                "path": "/tmp/rollout-thread-a.jsonl"
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::Recoverable);
}

#[test]
fn app_server_thread_summaries_filter_subagent_rows_without_main_fallback() {
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [
                {
                    "id": "main-thread",
                    "name": "主线程",
                    "sourceKind": "root"
                },
                {
                    "id": "child-thread",
                    "name": "worker",
                    "parentThreadId": "main-thread",
                    "sourceKind": "subAgentRun",
                    "agentNickname": "worker"
                },
                {
                    "id": "child-source",
                    "name": "explorer",
                    "source": { "kind": "subagent" },
                    "agentRole": "explorer"
                },
                {
                    "id": "child-nested-source",
                    "name": "nested",
                    "source": { "subagent": { "thread_spawn": { "parentThreadId": "main-thread" } } }
                }
            ]
        }),
        &[],
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "main-thread");
}

#[test]
fn app_server_thread_summaries_keep_fallback_main_thread_even_when_app_row_looks_subagent() {
    let fallback = vec![fallback_summary("thread-a", "example-user")];
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "parentThreadId": "parent",
                "sourceKind": "subAgentRun",
                "agentNickname": "worker"
            }]
        }),
        &fallback,
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "thread-a");
    assert_eq!(rows[0].title, "example-user");
}

#[test]
fn app_server_threads_merge_preserves_local_only_fallback_rows() {
    let fallback = vec![
        fallback_summary("thread-a", "local fallback"),
        fallback_summary("thread-b", "shared fallback"),
    ];
    let app_threads = vec![fallback_summary("thread-b", "app-server")];

    let rows = merge_thread_summaries(fallback, app_threads);

    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.id == "thread-a"));
    assert_eq!(
        rows.iter()
            .find(|row| row.id == "thread-b")
            .map(|row| row.title.as_str()),
        Some("app-server")
    );
}

#[test]
fn app_server_threads_merge_keeps_fallback_title_when_app_row_has_placeholder_title() {
    let fallback = vec![fallback_summary("thread-a", "example-user")];
    let mut app_thread = fallback_summary("thread-a", "未命名线程");
    app_thread.status = ThreadStatus::Running;
    app_thread.latest_message = Some("正在执行".to_string());

    let rows = merge_thread_summaries(fallback, vec![app_thread]);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "example-user");
    assert_eq!(rows[0].status, ThreadStatus::Running);
    assert_eq!(rows[0].latest_message.as_deref(), Some("正在执行"));
}

#[test]
fn app_server_threads_merge_keeps_archived_fallback_out_of_default_list() {
    let mut archived = fallback_summary("thread-a", "archived");
    archived.status = ThreadStatus::Archived;
    archived.archived_at = Some("2026-06-16T00:00:00Z".to_string());
    let mut app_thread = fallback_summary("thread-a", "app-server");
    app_thread.status = ThreadStatus::Running;

    let rows = merge_thread_summaries(vec![archived], vec![app_thread]);
    let default_rows = thread_service::filter_thread_summaries(rows, false, None, 10);

    assert!(default_rows.is_empty());
}

#[test]
fn hidden_state_db_thread_ids_prune_app_server_rows_after_merge() {
    let fallback = vec![fallback_summary("main-thread", "example-user")];
    let app_threads = vec![
        fallback_summary("main-thread", "app main"),
        fallback_summary("child-thread", "subagent child"),
    ];
    let rows = merge_thread_summaries(fallback, app_threads);
    let hidden = ["child-thread".to_string()].into_iter().collect();

    let rows = thread_service::prune_hidden_thread_summaries(rows, &hidden);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "main-thread");
}

#[test]
fn filter_thread_summaries_separates_active_and_archived_lists() {
    let recent = fallback_summary("recent-thread", "recent");
    let mut archived = fallback_summary("archived-thread", "archived");
    archived.status = ThreadStatus::Archived;

    let active_rows = thread_service::filter_thread_summaries(
        vec![recent.clone(), archived.clone()],
        false,
        None,
        10,
    );
    let archived_rows =
        thread_service::filter_thread_summaries(vec![recent, archived], true, None, 10);

    assert_eq!(active_rows.len(), 1);
    assert_eq!(active_rows[0].id, "recent-thread");
    assert_eq!(archived_rows.len(), 1);
    assert_eq!(archived_rows[0].id, "archived-thread");
}

#[test]
fn thread_list_fetch_limit_clamps_requested_page_size() {
    assert_eq!(thread_service::thread_list_fetch_limit(Some(120)), 120);
    assert_eq!(thread_service::thread_list_fetch_limit(None), 80);
}

#[test]
fn running_job_marks_summary_running_with_active_job_id() {
    let mut summary = fallback_summary("thread-a", "example-user");
    let job = JobRecord {
        id: "job-live".to_string(),
        kind: "codex_chat".to_string(),
        status: "running".to_string(),
        title: "Codex resume thread".to_string(),
        thread_id: Some("thread-a".to_string()),
        turn_id: None,
        started_at: 1,
        finished_at: None,
        exit_code: None,
        output: String::new(),
        error: None,
    };

    thread_service::apply_running_job_to_summary(&mut summary, &job);

    assert_eq!(summary.status, ThreadStatus::Running);
    assert_eq!(summary.active_job_id.as_deref(), Some("job-live"));
    assert_eq!(summary.last_event_kind, None);
}

#[test]
fn running_job_does_not_inject_archived_thread_missing_from_limited_local_list() {
    let mut archived = fallback_summary("thread-archived", "old");
    archived.status = ThreadStatus::Archived;
    archived.archived_at = Some("2026-06-16T00:00:00Z".to_string());
    let job = JobRecord {
        id: "job-live".to_string(),
        kind: "codex_chat".to_string(),
        status: "running".to_string(),
        title: "Codex resume archived".to_string(),
        thread_id: Some("thread-archived".to_string()),
        turn_id: Some("turn-live".to_string()),
        started_at: 1,
        finished_at: None,
        exit_code: None,
        output: String::new(),
        error: None,
    };
    let mut rows = vec![fallback_summary("thread-visible", "visible")];

    let archived_ids = HashSet::from([archived.id.clone()]);
    thread_service::merge_running_jobs(&mut rows, &[job], &archived_ids);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "thread-visible");
}

#[test]
fn app_server_idle_status_clears_stale_running_without_pending_signal() {
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::Running;
    fallback.active_turn_id = Some("turn-old".to_string());
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "idle" }
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::Recent);
    assert_eq!(rows[0].active_turn_id, None);
}

#[test]
fn app_server_idle_status_preserves_reply_needed_with_current_pending_signal() {
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::ReplyNeeded;
    fallback.active_turn_id = Some("turn-choice".to_string());
    fallback.pending_elicitation = Some(agenthub_core::codex::PendingElicitation {
        turn_id: Some("turn-choice".to_string()),
        item_id: Some("item-choice".to_string()),
        questions: Vec::new(),
    });
    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "idle" }
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::ReplyNeeded);
}

#[test]
fn running_signal_takes_priority_over_stale_pending_signal_for_not_loaded() {
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::ReplyNeeded;
    fallback.pending_elicitation = Some(agenthub_core::codex::PendingElicitation {
        turn_id: Some("turn-old".to_string()),
        item_id: Some("item-old".to_string()),
        questions: Vec::new(),
    });

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "notLoaded" },
                "turns": [{
                    "id": "turn-live",
                    "items": [{ "status": { "type": "running" } }]
                }]
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::Running);
    assert_eq!(rows[0].active_turn_id.as_deref(), Some("turn-live"));
    assert!(rows[0].pending_elicitation.is_none());
}

#[test]
fn app_server_not_loaded_uses_rollout_running_signal() {
    let root = unique_temp_dir("paneld-notloaded-running");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"turn_started","turn_id":"turn-active"}).to_string(),
            json!({"type":"response_item","turn_id":"turn-active","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":{"cmd":"sleep 10"}}}).to_string(),
        ].join("\n"),
    )
    .unwrap();

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "name": "example-user",
                "status": { "type": "notLoaded" },
                "path": rollout.display().to_string()
            }]
        }),
        &[],
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, ThreadStatus::Running);
    assert_eq!(rows[0].active_turn_id.as_deref(), Some("turn-active"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_not_loaded_uses_event_msg_task_started_rollout_signal() {
    let root = unique_temp_dir("paneld-notloaded-event-task-running");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_started"}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"continue"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "name": "example-user",
                "status": { "type": "notLoaded" },
                "path": rollout.display().to_string()
            }]
        }),
        &[],
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, ThreadStatus::Running);
    assert!(rows[0].active_turn_id.is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_not_loaded_preserves_rollout_running_after_prior_task_complete() {
    let root = unique_temp_dir("paneld-notloaded-prior-complete");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-old"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-old","last_agent_message":"done"}}).to_string(),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"still working"}]}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "name": "example_task",
                "status": { "type": "notLoaded" },
                "path": rollout.display().to_string()
            }]
        }),
        &[],
    );

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, ThreadStatus::Running);
    assert_eq!(rows[0].active_turn_id.as_deref(), Some("turn-live"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_not_loaded_completed_rollout_stays_recent() {
    let root = unique_temp_dir("paneld-notloaded-completed");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"turn_started","turn_id":"turn-active"}).to_string(),
            json!({"type":"turn_completed","turn_id":"turn-active"}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "notLoaded" },
                "path": rollout.display().to_string()
            }]
        }),
        &[],
    );

    assert_eq!(rows[0].status, ThreadStatus::Recent);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_not_loaded_completed_rollout_clears_stale_fallback_running_when_path_is_current() {
    let root = unique_temp_dir("paneld-notloaded-stale-running");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"turn_started","turn_id":"turn-old"}).to_string(),
            json!({"type":"turn_completed","turn_id":"turn-old"}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::Running;
    fallback.active_turn_id = Some("turn-old".to_string());
    fallback.rollout_path = Some(rollout.clone());
    fallback.last_event_kind = Some("task_started".to_string());

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "notLoaded" },
                "path": rollout.display().to_string()
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::Recent);
    assert_eq!(rows[0].active_turn_id, None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_not_loaded_ld_style_completed_rollout_clears_stale_running() {
    let root = unique_temp_dir("paneld-notloaded-ld-completed");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"event_msg","payload":{"type":"task_started"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":null}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-stale"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-latest"}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-latest","last_agent_message":"done"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let mut fallback = fallback_summary("thread-a", "LD");
    fallback.status = ThreadStatus::Running;
    fallback.active_turn_id = Some("turn-stale".to_string());
    fallback.rollout_path = Some(rollout.clone());
    fallback.last_event_kind = Some("task_started".to_string());

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "name": "LD",
                "status": { "type": "notLoaded" },
                "path": rollout.display().to_string()
            }]
        }),
        &[fallback.clone()],
    );

    assert_eq!(rows[0].status, ThreadStatus::Recent);
    assert_eq!(rows[0].active_turn_id, None);

    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback,
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };
    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "name": "LD",
                "status": { "type": "notLoaded" },
                "path": rollout.display().to_string()
            }
        }),
    );

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert_eq!(detail.summary.active_turn_id, None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_active_does_not_override_completed_rollout_wait_agent() {
    let root = unique_temp_dir("paneld-active-completed-wait-agent");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-main"}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-main","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-main","last_agent_message":"主线程完成。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.rollout_path = Some(rollout.clone());

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "name": "example-user",
                "status": { "type": "active" },
                "activeTurnId": "turn-main",
                "path": rollout.display().to_string()
            }]
        }),
        &[fallback.clone()],
    );
    let active = thread_service::filter_thread_summaries(rows.clone(), false, None, 50);

    assert_eq!(rows[0].status, ThreadStatus::Recent);
    assert_eq!(rows[0].active_turn_id, None);
    assert_eq!(active.len(), 1);

    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback,
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };
    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "name": "example-user",
                "status": { "type": "active" },
                "activeTurnId": "turn-main",
                "path": rollout.display().to_string()
            }
        }),
    );

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert_eq!(detail.summary.active_turn_id, None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_not_loaded_running_item_does_not_override_completed_rollout_wait_agent() {
    let root = unique_temp_dir("paneld-notloaded-completed-wait-agent");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-main"}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-main","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-main","last_agent_message":"主线程完成。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.rollout_path = Some(rollout.clone());

    let app_thread = json!({
        "id": "thread-a",
        "name": "example-user",
        "status": { "type": "notLoaded" },
        "path": rollout.display().to_string(),
        "turns": [{
            "id": "turn-main",
            "items": [{ "type": "function_call", "name": "wait_agent", "status": { "type": "running" } }]
        }]
    });
    let rows = app_server_thread_summaries(
        &json!({ "threads": [app_thread.clone()] }),
        &[fallback.clone()],
    );
    let active = thread_service::filter_thread_summaries(rows.clone(), false, None, 50);

    assert_eq!(rows[0].status, ThreadStatus::Recent);
    assert_eq!(rows[0].active_turn_id, None);
    assert_eq!(active.len(), 1);

    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback,
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };
    apply_app_server_thread_detail(&mut detail, &json!({ "thread": app_thread }));

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert_eq!(detail.summary.active_turn_id, None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_newer_active_turn_overrides_completed_rollout_wait_agent() {
    let root = unique_temp_dir("paneld-newer-active-after-completed-wait-agent");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        [
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-main"}}).to_string(),
            json!({"type":"response_item","turn_id":"turn-main","payload":{"type":"function_call","name":"wait_agent","call_id":"wait-agent-1","arguments":{"targets":["agent-1"]}}}).to_string(),
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-main","last_agent_message":"主线程完成。"}}).to_string(),
        ]
        .join("\n"),
    )
    .unwrap();
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.rollout_path = Some(rollout.clone());

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "name": "example-user",
                "status": { "type": "active" },
                "activeTurnId": "turn-new",
                "path": rollout.display().to_string()
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::Running);
    assert_eq!(rows[0].active_turn_id.as_deref(), Some("turn-new"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_not_loaded_without_rollout_path_clears_stale_local_active_turn() {
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::Running;
    fallback.active_turn_id = Some("turn-live".to_string());
    fallback.last_event_kind = Some("task_started".to_string());

    let rows = app_server_thread_summaries(
        &json!({
            "threads": [{
                "id": "thread-a",
                "status": { "type": "notLoaded" }
            }]
        }),
        &[fallback],
    );

    assert_eq!(rows[0].status, ThreadStatus::Recent);
    assert_eq!(rows[0].active_turn_id, None);
    assert_eq!(rows[0].last_event_kind.as_deref(), Some("task_started"));
}

#[test]
fn app_server_thread_detail_not_loaded_clears_stale_local_active_turn() {
    let mut summary = fallback_summary("thread-a", "example-user");
    summary.status = ThreadStatus::Running;
    summary.active_turn_id = Some("turn-live".to_string());
    summary.last_event_kind = Some("task_started".to_string());
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary,
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };

    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "status": { "type": "notLoaded" }
            }
        }),
    );

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert_eq!(detail.summary.active_turn_id, None);
    assert_eq!(
        detail.summary.last_event_kind.as_deref(),
        Some("task_started")
    );
}

#[test]
fn app_server_thread_read_turns_provide_running_status_and_active_turn() {
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback_summary("thread-a", "example-user"),
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };

    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "status": { "type": "notLoaded" },
                "turns": [{
                    "id": "turn-live",
                    "items": [{ "status": { "type": "running" } }]
                }]
            }
        }),
    );

    assert_eq!(detail.summary.status, ThreadStatus::Running);
    assert_eq!(detail.summary.active_turn_id.as_deref(), Some("turn-live"));
}

#[test]
fn app_server_detail_from_read_builds_detail_from_rollout_path_without_fallback_db_row() {
    let root = unique_temp_dir("paneld-detail-read");
    fs::create_dir_all(&root).unwrap();
    let rollout = root.join("rollout-thread-a.jsonl");
    fs::write(
        &rollout,
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"hello from rollout"}]}}).to_string(),
    )
    .unwrap();

    let detail = app_server_detail_from_read(&json!({
        "thread": {
            "id": "thread-a",
            "name": "example-user",
            "status": { "type": "notLoaded" },
            "path": rollout.display().to_string()
        }
    }))
    .unwrap();

    assert_eq!(detail.summary.id, "thread-a");
    assert_eq!(detail.summary.title, "example-user");
    assert_eq!(detail.blocks.len(), 1);
    assert_eq!(detail.blocks[0].text.as_deref(), Some("hello from rollout"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_server_detail_from_read_builds_blocks_from_turn_items_without_rollout_path() {
    let detail = app_server_detail_from_read(&json!({
        "thread": {
            "id": "thread-a",
            "name": "example-user",
            "status": { "type": "notLoaded" },
            "turns": [{
                "id": "turn-plan",
                "items": [
                    {
                        "type": "userMessage",
                        "id": "user-1",
                        "content": [{ "type": "input_text", "text": "帮我修复" }]
                    },
                    {
                        "type": "userMessage",
                        "id": "subagent-context",
                        "content": [{ "type": "input_text", "text": "<subagent_notification>{\"agent_path\":\"/tmp/child\"}</subagent_notification>" }]
                    },
                    {
                        "type": "agentMessage",
                        "id": "agent-1",
                        "text": "我先检查。"
                    },
                    {
                        "type": "plan",
                        "id": "plan-1",
                        "text": "<proposed_plan>\n# Summary\n- Fix it\n</proposed_plan>"
                    },
                    {
                        "type": "requestUserInput",
                        "id": "choice-1",
                        "turnId": "turn-plan",
                        "questions": [{
                            "id": "q1",
                            "header": "选择",
                            "question": "选择方案",
                            "options": [{ "label": "A", "description": "执行 A" }]
                        }]
                    }
                ]
            }]
        }
    }))
    .unwrap();

    assert_eq!(detail.summary.status, ThreadStatus::ReplyNeeded);
    assert_eq!(detail.summary.active_turn_id.as_deref(), Some("turn-plan"));
    assert_eq!(detail.blocks.len(), 5);
    assert_eq!(detail.blocks[0].id, "user-1");
    assert_eq!(detail.blocks[0].role, "user");
    assert_eq!(detail.blocks[0].text.as_deref(), Some("帮我修复"));
    assert_eq!(detail.blocks[1].id, "subagent-context");
    assert_eq!(detail.blocks[1].role, "user");
    let context_message = detail.blocks[1].user_message.as_ref().unwrap();
    assert_eq!(context_message.text, "");
    assert_eq!(context_message.context_blocks.len(), 1);
    assert_eq!(
        context_message.context_blocks[0].kind,
        agenthub_core::user_message::UserContextKind::Runtime
    );
    assert_eq!(
        context_message.context_blocks[0].text,
        "<subagent_notification>{\"agent_path\":\"/tmp/child\"}</subagent_notification>"
    );
    let timeline = detail.timeline.as_ref().unwrap();
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].id, "user-1");
    assert_eq!(detail.blocks[2].role, "assistant");
    assert_eq!(detail.blocks[2].text.as_deref(), Some("我先检查。"));
    assert_eq!(detail.blocks[3].kind, "plan");
    assert_eq!(detail.blocks[3].item_id.as_deref(), Some("plan-1"));
    assert_eq!(detail.blocks[4].kind, "request_user_input");
    assert_eq!(detail.blocks[4].call_id.as_deref(), Some("choice-1"));
    assert_eq!(detail.blocks[4].questions[0].question, "选择方案");
    assert_eq!(detail.total_blocks, detail.blocks.len());
}

#[test]
fn app_server_thread_detail_clears_stale_reply_needed_when_idle() {
    let mut summary = fallback_summary("thread-a", "example-user");
    summary.status = ThreadStatus::ReplyNeeded;
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary,
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };

    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "status": { "type": "idle" }
            }
        }),
    );

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
}

#[test]
fn app_server_thread_detail_ignores_historical_pending_blocks_without_active_turn() {
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback_summary("thread-a", "example-user"),
        messages: Vec::new(),
        blocks: vec![
            MessageBlock {
                subagent: None,
                user_message: None,
                id: "choice-old".to_string(),
                role: "assistant".to_string(),
                kind: "request_user_input".to_string(),
                display_kind: Some("question".to_string()),
                status: Some("pending".to_string()),
                text: None,
                summary: None,
                input: None,
                truncated: None,
                resolved: Some(false),
                answers: Vec::new(),
                plan_status: None,
                group_id: Some("call-old".to_string()),
                tool_name: None,
                call_id: Some("call-old".to_string()),
                turn_id: Some("turn-old".to_string()),
                item_id: Some("item-old".to_string()),
                created_at: None,
                questions: vec![agenthub_core::codex::UserInputQuestion {
                    id: "q1".to_string(),
                    header: None,
                    question: "旧选择".to_string(),
                    options: Vec::new(),
                }],
                payload: None,
            },
            MessageBlock {
                subagent: None,
                user_message: None,
                id: "assistant-later".to_string(),
                role: "assistant".to_string(),
                kind: "message".to_string(),
                display_kind: None,
                status: None,
                text: Some("已经继续执行。".to_string()),
                summary: None,
                input: None,
                truncated: Some(false),
                resolved: None,
                answers: Vec::new(),
                plan_status: None,
                group_id: Some("turn-new".to_string()),
                tool_name: None,
                call_id: None,
                turn_id: Some("turn-new".to_string()),
                item_id: None,
                created_at: None,
                questions: Vec::new(),
                payload: None,
            },
        ],
        raw_event_count: 2,
        total_blocks: 2,
        has_more_blocks: false,
        before_cursor: None,
    };

    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "status": { "type": "notLoaded" }
            }
        }),
    );

    assert_eq!(detail.summary.status, ThreadStatus::Recent);
    assert!(detail.summary.pending_elicitation.is_none());
    assert!(detail.summary.active_turn_id.is_none());
}

#[test]
fn app_server_status_derivation_is_shared_for_list_detail_and_probe_buckets() {
    let mut fallback = fallback_summary("thread-a", "example-user");
    fallback.status = ThreadStatus::ReplyNeeded;
    fallback.active_turn_id = Some("turn-choice".to_string());
    fallback.pending_elicitation = Some(agenthub_core::codex::PendingElicitation {
        turn_id: Some("turn-choice".to_string()),
        item_id: Some("item-choice".to_string()),
        questions: Vec::new(),
    });
    let app_value = json!({
        "threads": [{
            "id": "thread-a",
            "name": "example-user",
            "status": { "type": "idle" }
        }]
    });
    let rows = app_server_thread_summaries(&app_value, &[fallback.clone()]);
    let mut detail = ThreadDetail {
        timeline: None,
        after_cursor: None,
        history_subagent_blocks: Vec::new(),
        history: None,
        subagents: None,
        subagent_updates: Default::default(),
        summary: fallback,
        messages: Vec::new(),
        blocks: Vec::new(),
        raw_event_count: 0,
        total_blocks: 0,
        has_more_blocks: false,
        before_cursor: None,
    };
    apply_app_server_thread_detail(
        &mut detail,
        &json!({
            "thread": {
                "id": "thread-a",
                "name": "example-user",
                "status": { "type": "idle" }
            }
        }),
    );
    let active = thread_service::filter_thread_summaries(rows.clone(), false, None, 50);

    assert_eq!(rows[0].status, ThreadStatus::ReplyNeeded);
    assert_eq!(detail.summary.status, ThreadStatus::ReplyNeeded);
    assert_eq!(active.len(), 1);
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "agenthub-{label}-{}-{}-{}",
        std::process::id(),
        counter,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}

#[tokio::test]
async fn batch_rpc_rejects_paths_or_wildcards() {
    let state = rpc_test_state();
    let app = state;

    for payload in [
        r#"{"request":{"provider":"codex","operation":"delete","sessionKeys":[],"path":"/tmp"}}"#,
        r#"{"request":{"provider":"codex","operation":"delete","sessionKeys":["fixture"],"all":true}}"#,
    ] {
        assert_eq!(
            request_rpc_status(app.clone(), "sessions.bulkPreview", payload).await,
            Err(ApiErrorKind::InvalidRequest)
        );
    }
}

#[tokio::test]
async fn claude_rpc_identity_paging_rename_and_delete_are_scoped() {
    let root = temp_test_dir("agenthub-claude-rpc");
    let _env = AsyncEnvGuard::set_many(&[("CLAUDE_CONFIG_DIR", &root)]).await;
    let state = rpc_test_state();
    let app = state;
    assert_eq!(
        request_rpc_json(app.clone(), "claude.list", "{}").await,
        json!([])
    );
    let file = root.join("projects/example/native.jsonl");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let user = json!({"type":"user","uuid":"u1","sessionId":"claude-rpc-id","cwd":root.join("workspace"),"version":"2.1.284","timestamp":"2026-09-29T00:00:00Z","message":{"content":"Claude RPC fixture"}});
    let assistant = json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"claude-rpc-id","timestamp":"2026-09-29T00:00:01Z","message":{"content":[{"type":"text","text":"Done"}],"stop_reason":"end_turn"}});
    fs::write(&file, format!("{user}\n{assistant}\n")).unwrap();
    let rows = request_rpc_json(app.clone(), "claude.list", "{}").await;
    let key = rows[0]["sessionKey"].as_str().unwrap();
    let detail = request_rpc_json(
        app.clone(),
        "claude.detail",
        &json!({"sessionKey":key,"limit":1}).to_string(),
    )
    .await;
    assert_eq!(detail["events"].as_array().unwrap().len(), 1);
    assert_eq!(detail["hasMore"], true);
    let renamed = request_rpc_json(
        app.clone(),
        "claude.rename",
        &json!({"sessionKey":key,"title":"Renamed RPC fixture"}).to_string(),
    )
    .await;
    assert_eq!(renamed["title"], "Renamed RPC fixture");
    assert_eq!(
        request_rpc_status(
            app.clone(),
            "claude.detail",
            r#"{"sessionKey":"/etc/passwd"}"#
        )
        .await,
        Err(ApiErrorKind::OperationFailed)
    );
    assert_eq!(request_rpc_status(app.clone(),"sessions.attachmentRead",&json!({"provider":"claude_code","sessionKey":key,"messageId":"u1","attachmentId":"../../private"}).to_string()).await,Err(ApiErrorKind::OperationFailed));
    let preview = request_rpc_json(
        app.clone(),
        "claude.deletePreview",
        &json!({"sessionKey":key}).to_string(),
    )
    .await;
    let deleted = request_rpc_json(app,"claude.deleteExecute",&json!({"request":{"sessionKey":key,"confirmed":true,"fingerprint":preview["fingerprint"]}}).to_string()).await;
    assert_eq!(deleted["deleted"], true);
    assert!(!file.exists());
    fs::remove_dir_all(root).unwrap();
}

const SUBAGENT_RPC_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN1cAAAAASUVORK5CYII=";

struct SubagentRpcFixture {
    home: PathBuf,
    state: crate::state::AppState,
}

impl SubagentRpcFixture {
    fn new() -> Self {
        let home = temp_test_dir("agenthub-subagent-rpc");
        fs::create_dir(&home).unwrap();
        let home = fs::canonicalize(home).unwrap();
        mark_codex_home(&home);
        let conn = Connection::open(home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                thread_source TEXT NOT NULL,
                parent_thread_id TEXT,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0,
                has_user_event INTEGER NOT NULL DEFAULT 1
            );",
        )
        .unwrap();
        for (id, parent) in [
            ("rpc-root", None),
            ("rpc-child", Some("rpc-root")),
            ("rpc-other-root", None),
        ] {
            let source = parent.map_or_else(
                || json!("cli"),
                |parent| json!({"subagent":{"thread_spawn":{"parent_thread_id":parent}}}),
            );
            let rollout = home.join("sessions").join(format!("{id}.jsonl"));
            let events = [
                json!({"type":"session_meta","payload":{
                    "id":id,"source":source,"cwd":"/workspace/example"
                }}),
                json!({"type":"response_item","payload":{
                    "id":format!("{id}-image"),"type":"message","role":"user","content":[
                        {"type":"input_text","text":"Inspect the example image."},
                        {"type":"input_image","image_url":format!("data:image/png;base64,{SUBAGENT_RPC_PNG}")}
                    ]
                }}),
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"fixture-turn"}}),
            ];
            let mut text = events
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            text.push('\n');
            fs::write(&rollout, text).unwrap();
            conn.execute(
                "INSERT INTO threads (
                    id, rollout_path, created_at, updated_at, source, thread_source,
                    parent_thread_id, cwd, title
                ) VALUES (?1, ?2, 1, 2, ?3, ?4, ?5, '/workspace/example', 'RPC fixture')",
                params![
                    id,
                    rollout.to_str().unwrap(),
                    source.to_string(),
                    if parent.is_some() { "subagent" } else { "user" },
                    parent,
                ],
            )
            .unwrap();
        }
        let state = rpc_test_state();
        let mut config = state.config();
        config.codex.home = home.clone();
        config.codex.workspace = home.clone();
        state.replace_config(config);
        Self { home, state }
    }

    async fn attachment_request(&self, app: crate::state::AppState) -> serde_json::Value {
        let detail = request_rpc_json(
            app,
            "threads.subagentDetail",
            r#"{"rootThreadId":"rpc-root","agentId":"rpc-child"}"#,
        )
        .await;
        assert_eq!(detail["rootThreadId"], "rpc-root");
        assert_eq!(detail["parentThreadId"], "rpc-root");
        assert_eq!(detail["detail"]["summary"]["id"], "rpc-child");
        assert!(!detail.to_string().contains(SUBAGENT_RPC_PNG));
        let message = detail["detail"]["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|block| block.get("user_message"))
            .find(|message| {
                message["attachments"]
                    .as_array()
                    .is_some_and(|attachments| !attachments.is_empty())
            })
            .unwrap();
        json!({
            "rootThreadId":"rpc-root",
            "provider":"codex",
            "sessionKey":"rpc-child",
            "messageId":message["id"],
            "attachmentId":message["attachments"][0]["id"]
        })
    }
}

impl Drop for SubagentRpcFixture {
    fn drop(&mut self) {
        // This unique directory contains only this test's native fixtures.
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[tokio::test]
async fn subagent_detail_and_attachment_rpc_validate_native_reads() {
    let fixture = SubagentRpcFixture::new();
    let app = fixture.state.clone();
    let attachment = fixture.attachment_request(app.clone()).await;
    let detail = json!({"rootThreadId":"rpc-root","agentId":"rpc-child"});

    for (command, payload) in [
        ("threads.subagentDetail", detail),
        ("sessions.attachmentRead", attachment.clone()),
    ] {
        let payload = json!({"request":payload}).to_string();
        let response = request_rpc_json(app.clone(), command, &payload).await;
        if command == "sessions.attachmentRead" {
            assert_eq!(response["mimeType"], "image/png");
            assert_eq!(response["base64"], SUBAGENT_RPC_PNG);
        } else {
            assert_eq!(response["detail"]["summary"]["id"], "rpc-child");
        }
    }
    let response = request_rpc_json(app, "sessions.attachmentRead", &attachment.to_string()).await;
    assert_eq!(response["base64"], SUBAGENT_RPC_PNG);
}

#[tokio::test]
async fn subagent_rpc_rejects_client_paths_in_otherwise_valid_requests() {
    let fixture = SubagentRpcFixture::new();
    let app = fixture.state.clone();
    let attachment = fixture.attachment_request(app.clone()).await;
    let response = request_rpc_json(
        app.clone(),
        "sessions.attachmentRead",
        &attachment.to_string(),
    )
    .await;
    assert_eq!(response["base64"], SUBAGENT_RPC_PNG);
    let native_path = fixture.home.join("sessions/rpc-child.jsonl");

    for (command, payload) in [
        (
            "threads.subagentDetail",
            json!({"rootThreadId":"rpc-root","agentId":"rpc-child"}),
        ),
        ("sessions.attachmentRead", attachment.clone()),
    ] {
        for field in ["path", "filePath", "rolloutPath", "cwd", "url"] {
            let mut invalid = payload.clone();
            invalid[field] = json!(native_path);
            for wrapped in [false, true] {
                let body = if wrapped {
                    json!({"request":invalid})
                } else {
                    invalid.clone()
                };
                assert_eq!(
                    request_rpc_status(app.clone(), command, &body.to_string(),).await,
                    Err(ApiErrorKind::InvalidRequest),
                    "{command} must reject caller field {field}"
                );
            }
        }
    }

    for field in ["rootThreadId", "agentId"] {
        let mut invalid = json!({"rootThreadId":"rpc-root","agentId":"rpc-child"});
        invalid[field] = json!(native_path);
        assert_eq!(
            request_rpc_status(app.clone(), "threads.subagentDetail", &invalid.to_string(),).await,
            Err(ApiErrorKind::OperationFailed)
        );
    }
    for field in ["rootThreadId", "sessionKey", "messageId", "attachmentId"] {
        let mut invalid = attachment.clone();
        invalid[field] = json!(native_path);
        assert_eq!(
            request_rpc_status(app.clone(), "sessions.attachmentRead", &invalid.to_string(),).await,
            Err(ApiErrorKind::OperationFailed)
        );
    }
    let mut wrong_root = attachment;
    wrong_root["rootThreadId"] = json!("rpc-other-root");
    assert_eq!(
        request_rpc_status(app, "sessions.attachmentRead", &wrong_root.to_string(),).await,
        Err(ApiErrorKind::OperationFailed)
    );
}

mod maintenance_security {
    use super::*;
    use std::{io::Write, sync::Arc, time::Duration};
    use tracing::instrument::WithSubscriber;

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

    impl Write for CapturedLogs {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl CapturedLogs {
        fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync {
            let logs = self.clone();
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .without_time()
                .with_writer(move || logs.clone())
                .finish()
        }

        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    struct BarkEchoServer {
        base: String,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for BarkEchoServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn bark_echo_server(status: u16, code: i64, prefix: &'static str) -> BarkEchoServer {
        let app = axum::Router::new().route(
            "/push",
            axum::routing::post(
                move |axum::Json(request): axum::Json<serde_json::Value>| async move {
                    let echoed = request["device_key"]
                        .as_str()
                        .expect("fixture request omitted its device key");
                    (
                        StatusCode::from_u16(status).unwrap(),
                        axum::Json(json!({"code":code,"message":format!("{prefix} {echoed}")})),
                    )
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        BarkEchoServer { base, task }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bark_error_responses_keep_echoed_device_keys_out_of_outcomes_and_logs() {
        const DEVICE_KEY: &str = "fixture-bark-credential-value";
        const TITLE: &str = "fixture private title";
        const BODY: &str = "fixture private notification body";
        for (status, code, prefix, reason) in [
            (400, 400, "fixture echo", "http_status"),
            (503, 503, "fixture echo", "http_status"),
            (200, 400, "fixture echo", "bark_response_code"),
            (200, 400, "BadDeviceToken", "apns_permanent_rejection"),
        ] {
            let server = bark_echo_server(status, code, prefix).await;
            let (mut config, _sandbox_root) = super::sandboxed_test_config();
            config.probe.notifications.enabled = true;
            config.probe.notifications.server_url = server.base.clone();
            let request = crate::ProbeBarkRequest {
                presentation: None,
                title: TITLE.into(),
                body: BODY.into(),
                dedupe_key: "fixture-maintenance-notification".into(),
            };
            let logs = CapturedLogs::default();
            let outcome = crate::send_bark_notification(
                &config,
                DEVICE_KEY.as_bytes(),
                &request,
                Duration::from_secs(2),
            )
            .with_subscriber(logs.subscriber())
            .await
            .unwrap();
            assert!(!outcome.sent && !outcome.skipped);
            assert_eq!(outcome.http_status, Some(status));
            assert_eq!(outcome.reason.as_deref(), Some(reason));
            assert_eq!(outcome.request_count, 1);
            let serialized = serde_json::to_string(&outcome).unwrap();
            let debug = format!("{outcome:?}");
            let log_text = logs.text();
            assert!(log_text.contains("Bark delivery attempt"));
            for secret in [DEVICE_KEY, TITLE, BODY] {
                assert!(
                    !serialized.contains(secret),
                    "Bark outcome exposed request content"
                );
                assert!(
                    !debug.contains(secret),
                    "Bark debug output exposed request content"
                );
                assert!(
                    !log_text.contains(secret),
                    "Bark logs exposed request content"
                );
            }
        }
    }
}
