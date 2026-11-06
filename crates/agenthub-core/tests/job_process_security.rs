use agenthub_core::{
    db::{JobRecord, PanelDb},
    jobs::{JobRunner, ProcessCommand},
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct TempFixture {
    root: PathBuf,
}

impl TempFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-job-process-security-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("create isolated process fixture");
        Self { root }
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

async fn wait_for_status(db: &PanelDb, job_id: &str, expected: &str) -> JobRecord {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let job = db
            .job(job_id)
            .expect("read process job")
            .expect("job exists");
        if job.status == expected {
            return job;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for process job to reach {expected}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
fn write_executable_fixture(path: &Path, script: &str) {
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
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(_) => break,
            Err(err) => panic!("probe executable fixture {}: {err}", path.display()),
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn process_job_preserves_program_path_and_literal_argument_boundaries() {
    let fixture = TempFixture::new();
    let program = fixture.root.join("argument recorder with spaces");
    let marker = fixture.root.join("shell-injection-marker");
    write_executable_fixture(&program, "#!/bin/sh\nprintf '<%s>\\n' \"$@\"\n");

    let config_path = Path::new("/synthetic/runtime security/config file.toml");
    let hostile_argument = format!("; touch {}; #", marker.display());
    let process = ProcessCommand::new(
        program.clone(),
        [
            "--config",
            config_path.to_str().unwrap(),
            "probe",
            "bark-test",
            hostile_argument.as_str(),
        ],
    );
    let db = PanelDb::open(":memory:").expect("open in-memory job database");
    let runner = JobRunner::new(db.clone());

    let job_id = runner
        .start_process_job("probe_bark_test", "Fixture Probe process", process)
        .expect("start fixture process");
    let job = wait_for_status(&db, &job_id, "succeeded").await;

    assert_eq!(job.exit_code, Some(0));
    let expected_output = format!(
        "<--config>\n<{}>\n<probe>\n<bark-test>\n<{}>\n",
        config_path.display(),
        hostile_argument
    );
    assert_eq!(job.output, expected_output);
    assert!(
        !marker.exists(),
        "shell syntax in an argument must not execute"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn failed_process_start_releases_its_exclusive_group() {
    let fixture = TempFixture::new();
    let db = PanelDb::open(":memory:").expect("open in-memory job database");
    let runner = JobRunner::new(db.clone());
    let group = "runtime-security-fixture";
    let missing_program = fixture.root.join("missing executable");

    let failed_id = runner
        .start_exclusive_process_job(
            "fixture_missing_process",
            "Fixture missing process",
            ProcessCommand::new(missing_program, std::iter::empty::<&str>()),
            group,
        )
        .expect("record the attempted process job");
    let failed = wait_for_status(&db, &failed_id, "failed").await;

    assert_eq!(failed.status, "failed");

    let retry_id = runner
        .start_exclusive_process_job(
            "fixture_process_retry",
            "Fixture process retry",
            ProcessCommand::new("/usr/bin/true", std::iter::empty::<&str>()),
            group,
        )
        .expect("exclusive group should be available after spawn failure");
    let retried = wait_for_status(&db, &retry_id, "succeeded").await;

    assert_eq!(retried.exit_code, Some(0));
}
