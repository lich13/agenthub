use agenthub_core::config::Config;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static SANDBOX_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn write_grok_notification_fixture(
    root: &Path,
    kind: &str,
    body: &str,
    timestamp_ms: i64,
) -> agenthub_core::native_probe::NativeScan {
    use serde_json::json;
    let id = "00000000-0000-4000-8000-000000000003";
    let dir = root.join("sessions/workspace").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("summary.json"), json!({"info":{"id":id,"cwd":"/workspace/fixture"},"generated_title":"Fixture","num_messages":2}).to_string()).unwrap();
    let events = [
        json!({"type":"turn_started","session_id":id,"session_relationship":"primary","ts":timestamp_ms-500}),
        json!({"type":"turn_ended","outcome":if kind=="failure" {"error"} else {"completed"},"ts":timestamp_ms}),
    ];
    let updates = [
        json!({"timestamp":timestamp_ms-250,"params":{"sessionId":id,"_meta":{"promptId":"fixture-prompt"},"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":body}}}}),
        json!({"timestamp":timestamp_ms,"params":{"sessionId":id,"update":{"sessionUpdate":"turn_completed","prompt_id":"fixture-prompt","stop_reason":if kind=="failure" {"error"} else {"end_turn"}}}}),
    ];
    for (name, rows) in [("events.jsonl", events), ("updates.jsonl", updates)] {
        fs::write(
            dir.join(name),
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }
    let scan = agenthub_core::native_probe::scan(agenthub_core::native_probe::NativeProvider::Grok)
        .unwrap();
    assert_eq!(scan.errors, 0);
    assert_eq!(scan.streams.len(), 1);
    assert_eq!(scan.streams[0].events.len(), 1);
    assert_eq!(scan.streams[0].events[0].kind, kind);
    scan
}
pub(crate) static CONFIG_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) struct AsyncEnvGuard {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl AsyncEnvGuard {
    pub(crate) async fn set_many(vars: &[(&'static str, &Path)]) -> Self {
        let guard = CONFIG_ENV_LOCK.lock().await;
        let mut previous = Vec::with_capacity(vars.len());
        for &(key, value) in vars {
            previous.push((key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        Self {
            _guard: guard,
            previous,
        }
    }

    pub(crate) async fn set_config(path: &Path) -> Self {
        Self::set_many(&[("AGENTHUB_CONFIG", path)]).await
    }

    pub(crate) async fn lock_read_only() -> tokio::sync::MutexGuard<'static, ()> {
        CONFIG_ENV_LOCK.lock().await
    }
}

impl Drop for AsyncEnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn write_executable_fixture(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let rest = script
        .strip_prefix("#!/bin/sh\n")
        .expect("executable fixture script must start with #!/bin/sh");
    let body = format!(
        "#!/bin/sh\nif [ \"${{AGENTHUB_TEST_EXEC_PROBE:-}}\" = \"1\" ]; then exit 0; fi\n{rest}"
    );
    fs::write(path, body).expect("write executable fixture");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("set executable fixture permissions");
    for attempt in 0..10 {
        match std::process::Command::new(path)
            .env("AGENTHUB_TEST_EXEC_PROBE", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 9 => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(_) => break,
            Err(err) => panic!("probe executable fixture {}: {err}", path.display()),
        }
    }
}

pub(crate) fn sandboxed_test_config() -> (Config, PathBuf) {
    let idx = SANDBOX_COUNTER.fetch_add(1, Ordering::Relaxed);
    let sandbox_root = std::env::temp_dir().join(format!(
        "agenthub-test-cfg-sandbox-{}-{idx}",
        std::process::id()
    ));
    let codex_home = sandbox_root.join(".codex");
    let _ = fs::create_dir_all(&codex_home);
    let mut config = Config::default();
    config.codex.home = codex_home;
    config.codex.workspace = sandbox_root.join("workspace");
    config.paths.data_dir = sandbox_root.join("data");
    config.paths.db_path = sandbox_root.join("data/panel.sqlite");
    config.paths.log_dir = sandbox_root.join("logs");

    (config, sandbox_root)
}
