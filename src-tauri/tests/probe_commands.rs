use agenthub_core::{
    config::Config,
    crypto::SecretBox,
    db::PanelDb,
    platform::{PlatformKind, PlatformPaths},
};
use agenthub_desktop_lib::{
    desktop_probe_status_with_state, desktop_update_status_with_state, DesktopState,
};
use serde_json::json;
use std::{path::Path, process::Command};

fn desktop_state(temp: &tempfile::TempDir) -> DesktopState {
    let mut config = Config::for_platform_kind_with_home(PlatformKind::Macos, temp.path());
    config.paths.data_dir = temp.path().join("data");
    config.paths.db_path = temp.path().join("panel.sqlite");
    config.paths.log_dir = temp.path().join("logs");
    config.codex.home = temp.path().join("codex-home");
    config.codex.workspace = temp.path().join("workspace");
    config.probe.recent_limit = 10;
    std::fs::create_dir_all(&config.paths.data_dir).unwrap();
    std::fs::create_dir_all(&config.paths.log_dir).unwrap();
    std::fs::create_dir_all(&config.codex.home).unwrap();
    std::fs::create_dir_all(config.codex.home.join("sessions")).unwrap();
    std::fs::create_dir_all(&config.codex.workspace).unwrap();
    let db = PanelDb::open_with_secret_box(
        &config.paths.db_path,
        SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
    )
    .unwrap();
    DesktopState::new(
        config,
        db,
        PlatformPaths::for_kind_with_home(PlatformKind::Macos, temp.path()),
    )
}

fn write_codex_thread(
    codex_home: &Path,
    id: &str,
    title: &str,
    rollout_events: &[serde_json::Value],
) {
    let now = chrono::Utc::now().timestamp();
    let rollout = codex_home
        .join("sessions")
        .join(format!("rollout-{id}.jsonl"));
    let text = rollout_events
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&rollout, text).unwrap();
    let db = codex_home.join("state_5.sqlite");
    let status = Command::new("sqlite3")
        .arg(&db)
        .arg(format!(
            "CREATE TABLE IF NOT EXISTS threads(
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL,
                preview TEXT NOT NULL DEFAULT ''
            );
            INSERT OR REPLACE INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
            VALUES('{id}', '{}', 1, {now}, 'codex', '/tmp', '{title}', '');",
            rollout.display()
        ))
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn typed_probe_status_includes_real_thread_buckets() {
    let temp = tempfile::tempdir().unwrap();
    let state = desktop_state(&temp);
    let codex_home = state.config().codex.home;
    write_codex_thread(
        &codex_home,
        "running-thread",
        "运行中的线程",
        &[json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-live"}})],
    );
    write_codex_thread(
        &codex_home,
        "reply-thread",
        "等待回复的线程",
        &[
            json!({"type":"response_item","turn_id":"turn-choice","payload":{"type":"function_call","name":"request_user_input","call_id":"choice-1","arguments":{"questions":[{"id":"choice","question":"选择方案","options":[{"label":"A"}]}]}}}),
        ],
    );

    let status = desktop_probe_status_with_state(&state).await.unwrap();

    assert_eq!(status.running_count, 1);
    assert_eq!(status.running_threads[0].id, "running-thread");
    assert_eq!(status.reply_needed_count, 1);
    assert_eq!(status.reply_needed_threads[0].id, "reply-thread");
}

#[tokio::test]
async fn desktop_update_status_uses_macos_tauri_updater_shape() {
    let temp = tempfile::tempdir().unwrap();
    let state = desktop_state(&temp);

    let status = desktop_update_status_with_state(&state, Some("v0.1.103"), None).unwrap();
    let serialized = serde_json::to_string(&status).unwrap();

    assert_eq!(
        status.method,
        agenthub_core::services::updates::UpdateExecutionMethod::MacosTauriUpdater
    );
    assert_eq!(status.current_version, env!("CARGO_PKG_VERSION"));
    assert!(!serialized.contains("systemctl"));
    assert!(!serialized.contains("nginx"));
    assert!(!serialized.contains("/opt/agenthub"));
}

#[tokio::test]
async fn desktop_update_status_remembers_recent_signed_check_result() {
    let temp = tempfile::tempdir().unwrap();
    let state = desktop_state(&temp);

    state
        .db
        .create_job(
            "desktop-update-check-test",
            "agenthub_update_check",
            "AgentHub app update check",
        )
        .unwrap();
    state
        .db
        .append_job_output(
            "desktop-update-check-test",
            "checking signed Tauri updater feed\nsigned app update available 999.0.0\n",
        )
        .unwrap();
    state
        .db
        .finish_job("desktop-update-check-test", "succeeded", Some(0), None)
        .unwrap();

    let status = desktop_update_status_with_state(&state, None, None).unwrap();

    assert_eq!(status.latest_version.as_deref(), Some("999.0.0"));
    assert_eq!(status.update_available, Some(true));
    assert_eq!(
        status.state,
        agenthub_core::services::updates::UpdateState::Ready
    );
    assert!(status.recommended_action.contains("Tauri updater"));
}

#[tokio::test]
async fn desktop_update_status_ignores_stale_older_signed_check_result() {
    let temp = tempfile::tempdir().unwrap();
    let state = desktop_state(&temp);

    state
        .db
        .create_job(
            "desktop-update-check-old",
            "agenthub_update_check",
            "AgentHub app update check",
        )
        .unwrap();
    state
        .db
        .append_job_output(
            "desktop-update-check-old",
            "checking signed Tauri updater feed\nsigned app update available 0.1.105\n",
        )
        .unwrap();
    state
        .db
        .finish_job("desktop-update-check-old", "succeeded", Some(0), None)
        .unwrap();

    let status = desktop_update_status_with_state(&state, None, None).unwrap();

    assert_eq!(status.latest_version.as_deref(), Some("0.1.105"));
    assert_eq!(status.update_available, Some(false));
    assert_eq!(
        status.state,
        agenthub_core::services::updates::UpdateState::Idle
    );
}

#[tokio::test]
async fn desktop_update_status_remembers_recent_signed_no_update_result() {
    let temp = tempfile::tempdir().unwrap();
    let state = desktop_state(&temp);

    state
        .db
        .create_job(
            "desktop-update-check-no-update",
            "agenthub_update_check",
            "AgentHub app update check",
        )
        .unwrap();
    state
        .db
        .append_job_output(
            "desktop-update-check-no-update",
            "checking signed Tauri updater feed\nno signed app update available\n",
        )
        .unwrap();
    state
        .db
        .finish_job("desktop-update-check-no-update", "succeeded", Some(0), None)
        .unwrap();

    let status = desktop_update_status_with_state(&state, None, None).unwrap();

    assert_eq!(
        status.latest_version.as_deref(),
        Some(status.current_version.as_str())
    );
    assert_eq!(status.update_available, Some(false));
    assert_eq!(
        status.state,
        agenthub_core::services::updates::UpdateState::Idle
    );
}
