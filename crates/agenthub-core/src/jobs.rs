use crate::{db::PanelDb, security::redact_output};
use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    ffi::OsString,
    future::Future,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use uuid::Uuid;

/// Internal execution data, deliberately separate from serialized display commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl ProcessCommand {
    pub fn new(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone)]
pub struct JobRunner {
    db: PanelDb,
    running: Arc<Mutex<HashSet<String>>>,
}

impl JobRunner {
    pub fn new(db: PanelDb) -> Self {
        Self {
            db,
            running: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn start_process_job(
        &self,
        kind: &str,
        title: &str,
        command: ProcessCommand,
    ) -> Result<String> {
        self.start_process_job_inner(kind, title, command, None)
    }

    pub fn start_exclusive_process_job(
        &self,
        kind: &str,
        title: &str,
        command: ProcessCommand,
        group: &str,
    ) -> Result<String> {
        self.start_process_job_inner(kind, title, command, Some(group))
    }

    fn start_process_job_inner(
        &self,
        kind: &str,
        title: &str,
        command: ProcessCommand,
        exclusive_group: Option<&str>,
    ) -> Result<String> {
        if let Some(group) = exclusive_group {
            let mut running = self.running.lock().expect("running jobs");
            if running.contains(group) {
                anyhow::bail!("exclusive job already running: {group}");
            }
            running.insert(group.to_string());
        }
        let id = Uuid::new_v4().to_string();
        if let Err(err) = self.db.create_job(&id, kind, title) {
            if let Some(key) = exclusive_group {
                self.running.lock().expect("running jobs").remove(key);
            }
            return Err(err);
        }
        let db = self.db.clone();
        let running = self.running.clone();
        let id_for_task = id.clone();
        let exclusive_group = exclusive_group.map(str::to_string);
        spawn_job_task(async move {
            let result = run_process_command(&db, &id_for_task, &command).await;
            if let Err(err) = result {
                let message = redact_output(&err.to_string());
                let _ = db.append_job_output(&id_for_task, &format!("\nerror: {message}\n"));
                let _ = db.finish_job(&id_for_task, "failed", None, Some(&message));
            }
            if let Some(group) = exclusive_group.as_deref() {
                running.lock().expect("running jobs").remove(group);
            }
        });
        Ok(id)
    }
}

async fn run_process_command(
    db: &PanelDb,
    job_id: &str,
    command: &ProcessCommand,
) -> Result<&'static str> {
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("unable to start managed job")?;

    let mut handles = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        handles.push(stream_reader(db.clone(), job_id.to_string(), stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        handles.push(stream_reader(db.clone(), job_id.to_string(), stderr));
    }
    let status = child.wait().await;
    let mut drain_result = Ok(());
    for handle in handles {
        if let Err(error) = handle.await.map_err(anyhow::Error::from).and_then(|r| r) {
            drain_result = Err(error);
        }
    }
    let status = status?;
    drain_result?;
    let code = status.code();
    let status_text = if status.success() {
        "succeeded"
    } else {
        "failed"
    };
    db.finish_job(job_id, status_text, code, None)?;
    Ok(status_text)
}

fn spawn_job_task<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
        return;
    }

    std::thread::Builder::new()
        .name("agenthub-job-runtime".to_string())
        .spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(future),
                Err(err) => eprintln!("failed to start AgentHub job runtime: {err}"),
            }
        })
        .expect("spawn AgentHub job runtime thread");
}

fn flush_job_output(db: &PanelDb, job_id: &str, output: &mut String) -> Result<()> {
    if output.is_empty() {
        return Ok(());
    }
    db.append_job_output(job_id, output)?;
    output.clear();
    Ok(())
}

fn stream_reader<R>(db: PanelDb, job_id: String, reader: R) -> tokio::task::JoinHandle<Result<()>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        use tokio::time::{sleep_until, Duration, Instant};
        let mut reader = BufReader::new(reader);
        let mut record = Vec::new();
        let mut output = String::new();
        let mut after_cr = false;
        let mut deadline = Instant::now() + Duration::from_millis(200);
        loop {
            let bytes = tokio::select! {
                result = reader.fill_buf() => result?,
                _ = sleep_until(deadline), if !output.is_empty() => {
                    flush_job_output(&db, &job_id, &mut output)?;
                    continue;
                }
            };
            if bytes.is_empty() {
                break;
            }
            for &byte in bytes {
                if byte == b'\n' && after_cr {
                    after_cr = false;
                    continue;
                }
                after_cr = byte == b'\r';
                if byte == b'\r' || byte == b'\n' {
                    if output.is_empty() {
                        deadline = Instant::now() + Duration::from_millis(200);
                    }
                    output.push_str(&redact_output(&String::from_utf8_lossy(&record)));
                    output.push('\n');
                    record.clear();
                    if output.len() >= 16 * 1024 {
                        flush_job_output(&db, &job_id, &mut output)?;
                    }
                } else {
                    record.push(byte);
                }
            }
            let consumed = bytes.len();
            reader.consume(consumed);
        }
        if !record.is_empty() {
            output.push_str(&redact_output(&String::from_utf8_lossy(&record)));
        }
        flush_job_output(&db, &job_id, &mut output)
    })
}

#[cfg(test)]
mod tests {
    use super::JobRunner;
    use crate::db::PanelDb;
    use std::{fs, thread, time::Duration};
    use uuid::Uuid;

    #[tokio::test]
    async fn split_utf8_sensitive_lines_and_unterminated_tail_are_preserved() {
        use tokio::io::AsyncWriteExt;
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.create_job("fixture", "fixture", "fixture").unwrap();
        let (mut writer, reader) = tokio::io::duplex(3);
        let reader = super::stream_reader(db.clone(), "fixture".into(), reader);
        for byte in "中文完成\n\nOPENAI_API_KEY=fixture-secret\n尾行".as_bytes() {
            writer.write_all(&[*byte]).await.unwrap();
        }
        drop(writer);
        reader.await.unwrap().unwrap();
        let output = db.job("fixture").unwrap().unwrap().output;
        assert_eq!(output, "中文完成\n\n[redacted sensitive line]\n尾行");
        assert!(db.list_jobs(10).unwrap()[0].output.is_empty());
    }

    #[tokio::test]
    async fn progress_records_flush_during_running_jobs_and_redact_before_storage() {
        use tokio::io::AsyncWriteExt;
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.create_job("fixture", "fixture", "fixture").unwrap();
        let (mut writer, reader) = tokio::io::duplex(1024);
        let reader = super::stream_reader(db.clone(), "fixture".into(), reader);
        writer
            .write_all("下载 10%\r下载 20%\r\nOPENAI_API_KEY=fixture-secret\r".as_bytes())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !db.job("fixture").unwrap().unwrap().output.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            db.job("fixture").unwrap().unwrap().output,
            "下载 10%\n下载 20%\n[redacted sensitive line]\n"
        );
        assert!(db
            .job_with_output("fixture", false)
            .unwrap()
            .unwrap()
            .output
            .is_empty());
        writer.write_all("完成".as_bytes()).await.unwrap();
        drop(writer);
        reader.await.unwrap().unwrap();
        assert!(db.job("fixture").unwrap().unwrap().output.ends_with("完成"));
    }

    #[tokio::test]
    async fn terminal_event_releases_group_after_all_output_is_drained() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let runner = JobRunner::new(db.clone());
        let id = runner
            .start_exclusive_process_job(
                "fixture",
                "Fixture",
                super::ProcessCommand::new(
                    "/bin/sh",
                    ["-c", "(sleep 0.1; printf tail)& printf start"],
                ),
                "drain",
            )
            .unwrap();
        wait_for_job_status(&runner, &id, "succeeded").await;
        assert_eq!(db.job(&id).unwrap().unwrap().output, "starttail");
        assert!(runner.running.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn exclusive_shell_jobs_reject_same_group_until_finished() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let runner = JobRunner::new(db.clone());

        let first = runner
            .start_exclusive_process_job(
                "panel_update_start",
                "Panel update",
                super::ProcessCommand::new("sleep", ["0.2"]),
                "panel_update",
            )
            .unwrap();
        assert!(!first.is_empty());

        let second = runner.start_exclusive_process_job(
            "panel_update_prune",
            "Panel prune",
            super::ProcessCommand::new("true", std::iter::empty::<&str>()),
            "panel_update",
        );
        assert!(second.is_err());

        wait_for_job_status(&runner, &first, "succeeded").await;
        assert!(runner.running.lock().unwrap().is_empty());

        let third = runner.start_exclusive_process_job(
            "panel_update_prune",
            "Panel prune",
            super::ProcessCommand::new("true", std::iter::empty::<&str>()),
            "panel_update",
        );
        assert!(third.is_ok());
    }

    async fn wait_for_job_status(runner: &JobRunner, job_id: &str, status: &str) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if runner
                    .db
                    .job(job_id)
                    .unwrap()
                    .is_some_and(|j| j.status == status)
                    && runner.running.lock().unwrap().is_empty()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("job completes and releases its group");
    }

    #[test]
    fn shell_jobs_start_without_current_tokio_runtime() {
        let db_path =
            std::env::temp_dir().join(format!("agenthub-job-runner-{}.sqlite", Uuid::new_v4()));
        let db = PanelDb::open_with_secret_box(
            &db_path,
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let runner = JobRunner::new(db.clone());

        let job_id = runner
            .start_exclusive_process_job(
                "probe_bark_test",
                "Probe Bark test",
                super::ProcessCommand::new("true", std::iter::empty::<&str>()),
                "probe:bark-test",
            )
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let job = db.job(&job_id).unwrap().expect("job exists");
            if job.status == "succeeded" {
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "timed out waiting for job {job_id}, last status {}",
                    job.status
                );
            }
            thread::sleep(Duration::from_millis(50));
        }

        drop(runner);
        drop(db);
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(db_path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(db_path.with_extension("sqlite-shm"));
    }
}
