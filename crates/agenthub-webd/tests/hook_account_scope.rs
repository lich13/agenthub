use agenthub_core::{config::Config, platform::PlatformKind};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

const THREAD: &str = "11111111-1111-4111-8111-111111111111";
const CHILD: &str = "22222222-2222-4222-8222-222222222222";
const OTHER: &str = "33333333-3333-4333-8333-333333333333";
const MARKER: &str = r#""account_scoped":true,"#;

#[test]
fn stop_hook_clears_the_finished_thread_tree_only_when_switched_on() {
    let root = std::env::temp_dir().join(format!(
        "agenthub-hook-account-scope-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = Config::for_platform_kind_with_home(PlatformKind::Macos, "/fixture/home");
    config.codex.home = root.join(".codex");
    config.paths.data_dir = root.join("data");
    config.paths.db_path = root.join("data/agenthub.sqlite");
    config.paths.log_dir = root.join("logs");
    config.security.secret_key = "7q9DCmCPyxnTrH3FhrV1sUJol1yqPgscQsBnR-mXA2E".to_string();
    config.probe.notifications.enabled = false;
    let config_path = root.join("config.toml");
    let sessions = config.codex.home.join("sessions/2026/01/01");
    fs::create_dir_all(&sessions).unwrap();
    let rollouts = [THREAD, CHILD, OTHER].map(|id| {
        let path = sessions.join(format!("rollout-2026-01-01T00-00-00-{id}.jsonl"));
        fs::write(&path, rollout(id)).unwrap();
        path
    });
    let conn = Connection::open(config.codex.home.join("state_5.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, first_user_message TEXT, source TEXT, thread_source TEXT, updated_at INTEGER, rollout_path TEXT);
         CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT);",
    )
    .unwrap();
    for (id, path) in [THREAD, CHILD, OTHER].iter().zip(&rollouts) {
        conn.execute(
            "INSERT INTO threads VALUES (?1, 'fixture', 'fixture', 'cli', 'user', 0, ?2)",
            params![id, path.to_str().unwrap()],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO thread_spawn_edges VALUES (?1, ?2)",
        params![THREAD, CHILD],
    )
    .unwrap();
    drop(conn);
    let payload = json!({
        "hook_event_name": "Stop",
        "session_id": THREAD,
        "turn_id": "turn-fixture",
        "transcript_path": rollouts[0],
    });

    write_config(&config_path, &config);
    assert_continue(&run_hook(&config_path, &payload));
    for (id, path) in [THREAD, CHILD, OTHER].iter().zip(&rollouts) {
        assert_eq!(fs::read_to_string(path).unwrap(), rollout(id), "{id}");
    }

    config.probe.hooks.clear_account_scope = true;
    write_config(&config_path, &config);
    assert_continue(&run_hook(&config_path, &payload));
    for (id, path) in [THREAD, CHILD].iter().zip(&rollouts) {
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            rollout(id).replace(MARKER, &" ".repeat(MARKER.len())),
            "{id}"
        );
    }
    assert_eq!(fs::read_to_string(&rollouts[2]).unwrap(), rollout(OTHER));
    fs::remove_dir_all(root).unwrap();
}

fn rollout(thread: &str) -> String {
    format!(
        "{{\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{{MARKER}\"id\":\"{thread}\",\"cwd\":\"/fixture\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"fixture\"}}}}\n"
    )
}

fn assert_continue(output: &Output) {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"continue": true, "suppressOutput": false})
    );
}

fn write_config(path: &Path, config: &Config) {
    fs::write(path, toml::to_string_pretty(config).unwrap()).unwrap();
}

fn run_hook(config_path: &Path, payload: &Value) -> Output {
    let mut child = Command::new(cargo_webd_binary())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "probe",
            "hook-stop",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn cargo_webd_binary() -> PathBuf {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_agenthub-webd"))
        .canonicalize()
        .expect("Cargo binary path must resolve to an existing build artifact");
    let test_binary = std::env::current_exe().unwrap().canonicalize().unwrap();
    // The hook must run this build's CLI, never an installed service binary.
    assert!(
        binary
            .parent()
            .is_some_and(|profile| test_binary.starts_with(profile)),
        "Cargo binary must belong to the same build directory and profile as this integration test"
    );
    binary
}
