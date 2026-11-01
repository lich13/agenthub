use nexushub_core::{
    config::Config,
    crypto::SecretBox,
    db::PanelDb,
    platform::{PlatformKind, PlatformPaths},
};
use nexushub_desktop_lib::{
    desktop_probe_status_with_state, desktop_update_status_with_state, DesktopState,
};
use serde_json::json;
use std::{path::Path, process::Command};

fn registered_invoke_command_paths() -> Vec<String> {
    let lib_source = include_str!("../src/lib.rs");
    let production_source = lib_source
        .split("\n#[cfg(test)]")
        .next()
        .expect("lib source must include production section");
    let marker = "tauri::generate_handler![";
    let start = production_source
        .find(marker)
        .expect("lib source must include tauri generate_handler")
        + marker.len();
    let (body, _) = production_source[start..]
        .split_once(']')
        .expect("generate_handler block must close");
    body.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("commands::"))
        .map(|line| line.trim_end_matches(',').to_string())
        .collect()
}

fn command_path(module: &str, name: &str) -> String {
    format!("commands::{module}::{name}")
}

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
    let db = PanelDb::open_with_secret_box(&config.paths.db_path, SecretBox::deterministic_dev())
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

#[test]
fn tauri_does_not_register_retired_string_action_or_macos_prune_handlers() {
    let commands = registered_invoke_command_paths();

    for retired in [
        command_path("settings", "startProbeJob"),
        command_path("updates", "runUpdateAction"),
        command_path("updates", "updatesPrune"),
        command_path("updates", "backupPrune"),
    ] {
        assert!(
            !commands.contains(&retired),
            "Tauri must not register retired string action or compat handler: {retired}"
        );
    }

    for typed in [
        command_path("updates", "updatesCheck"),
        command_path("updates", "updatesInstall"),
    ] {
        assert!(
            commands.contains(&typed),
            "Tauri must keep typed non-prune update handler: {typed}"
        );
    }

    assert!(
        !commands.iter().any(|command| command.contains("Prune")),
        "macOS Tauri invoke handler must not register update prune handlers"
    );
}

#[test]
fn tauri_update_sources_never_plan_update_prune() {
    for (name, source) in [
        (
            "commands/updates.rs",
            include_str!("../src/commands/updates.rs"),
        ),
        (
            "services/updates.rs",
            include_str!("../src/services/updates.rs"),
        ),
    ] {
        assert!(
            !source.contains("UpdateAction::Prune"),
            "{name} must not plan Linux update prune from macOS Tauri"
        );
        assert!(
            !source.contains("updatesPrune") && !source.contains("runUpdateAction"),
            "{name} must not define retired update action wrappers"
        );
    }
}

#[test]
fn tauri_cleanup_execute_services_are_native_effect_executors() {
    let settings_source = include_str!("../src/services/settings.rs")
        .split("\n#[cfg(test)]\nmod tests")
        .next()
        .expect("settings service source must include production section");

    for executor in [
        "archive_delete_execute_with_state",
        "hidden_delete_execute_with_state",
        ".cleanup()",
        ".execute_confirmed(",
        ".dry_run_archived(",
        ".execute_archived(",
        ".dry_run_hidden(",
        ".execute_hidden(",
    ] {
        assert!(
            settings_source.contains(executor),
            "settings service must keep only the cleanup executor landing point: {executor}"
        );
    }

    for forbidden in [
        "cleanup_service::plan_cleanup_execute_operation",
        "cleanup_service::validate_cleanup_expected_count",
        "cleanup_service::execute_archived_with_capability",
        "cleanup_service::execute_hidden_with_capability",
        "ARCHIVE_DELETE_CONFIRMATION_MESSAGE",
        "HIDDEN_DELETE_CONFIRMATION_MESSAGE",
        "CLEANUP_EXPECTED_COUNT_REQUIRED_MESSAGE",
        "archive deletion must be confirmed",
        "hidden thread deletion must be confirmed",
        "expectedCount mismatch",
        "fn ensure_cleanup_expected_count",
    ] {
        assert!(
            !settings_source.contains(forbidden),
            "settings service must not embed cleanup business semantic token: {forbidden}"
        );
    }
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
        nexushub_core::services::updates::UpdateExecutionMethod::MacosTauriUpdater
    );
    assert_eq!(status.current_version, env!("CARGO_PKG_VERSION"));
    assert!(status
        .capabilities
        .iter()
        .any(|capability| capability == "signature_verification"));
    assert!(!serialized.contains("systemctl"));
    assert!(!serialized.contains("nginx"));
    assert!(!serialized.contains("/opt/nexushub"));
}

#[tokio::test]
async fn desktop_update_status_remembers_recent_signed_check_result() {
    let temp = tempfile::tempdir().unwrap();
    let state = desktop_state(&temp);

    state
        .db
        .create_job(
            "desktop-update-check-test",
            "nexushub_update_check",
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
        nexushub_core::services::updates::UpdateState::Ready
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
            "nexushub_update_check",
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
        nexushub_core::services::updates::UpdateState::Idle
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
            "nexushub_update_check",
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
        nexushub_core::services::updates::UpdateState::Idle
    );
}
