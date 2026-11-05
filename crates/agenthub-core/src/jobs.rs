use crate::{db::PanelDb, security::redact_output};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::OsString,
    future::Future,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::broadcast,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobEvent {
    pub job_id: String,
    pub status: String,
    pub chunk: Option<String>,
}

#[derive(Clone)]
pub struct JobRunner {
    db: PanelDb,
    tx: broadcast::Sender<JobEvent>,
    running: Arc<Mutex<HashMap<String, u32>>>,
}

impl JobRunner {
    pub fn new(db: PanelDb) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            db,
            tx,
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.tx.subscribe()
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
        let exclusive_group_lock_key = exclusive_group.map(exclusive_group_key);
        if let Some(group) = exclusive_group {
            let key = exclusive_group_lock_key
                .as_deref()
                .expect("exclusive group key");
            let mut running = self.running.lock().expect("running jobs");
            if running.contains_key(key) {
                anyhow::bail!("codex update job already running: {group}");
            }
            running.insert(key.to_string(), 0);
        }
        let id = Uuid::new_v4().to_string();
        if let Err(err) = self.db.create_job(&id, kind, title) {
            if let Some(key) = exclusive_group_lock_key.as_deref() {
                self.running.lock().expect("running jobs").remove(key);
            }
            return Err(err);
        }
        let db = self.db.clone();
        let tx = self.tx.clone();
        let running = self.running.clone();
        let id_for_task = id.clone();
        let exclusive_group = exclusive_group.map(str::to_string);
        spawn_job_task(async move {
            let result = run_process_command(
                &db,
                &tx,
                &running,
                &id_for_task,
                &command,
                exclusive_group.clone(),
            )
            .await;
            if let Err(err) = result {
                if let Some(group) = exclusive_group.as_deref() {
                    running
                        .lock()
                        .expect("running jobs")
                        .remove(&exclusive_group_key(group));
                }
                let _ = db.append_job_output(&id_for_task, &format!("\nerror: {err}\n"));
                let _ = db.finish_job(&id_for_task, "failed", None, Some(&err.to_string()));
                let _ = tx.send(JobEvent {
                    job_id: id_for_task,
                    status: "failed".to_string(),
                    chunk: Some(err.to_string()),
                });
            }
        });
        Ok(id)
    }

    pub fn exclusive_group_job(&self, group: &str) -> Option<String> {
        let key = exclusive_group_key(group);
        let running = self.running.lock().expect("running jobs");
        running.contains_key(&key).then_some(group.to_string())
    }
}

async fn run_process_command(
    db: &PanelDb,
    tx: &broadcast::Sender<JobEvent>,
    running: &Arc<Mutex<HashMap<String, u32>>>,
    job_id: &str,
    command: &ProcessCommand,
    exclusive_group: Option<String>,
) -> Result<()> {
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("unable to start managed job")?;
    if let Some(pid) = child.id() {
        running
            .lock()
            .expect("running jobs")
            .insert(job_id.to_string(), pid);
    }

    let mut handles = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        handles.push(stream_reader(
            db.clone(),
            tx.clone(),
            job_id.to_string(),
            stdout,
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        handles.push(stream_reader(
            db.clone(),
            tx.clone(),
            job_id.to_string(),
            stderr,
        ));
    }
    let status = child.wait().await?;
    {
        let mut running = running.lock().expect("running jobs");
        running.remove(job_id);
        if let Some(group) = exclusive_group.as_deref() {
            running.remove(&exclusive_group_key(group));
        }
    }
    for handle in handles {
        let _ = handle.await;
    }
    let code = status.code();
    let status_text = if status.success() {
        "succeeded"
    } else {
        "failed"
    };
    db.finish_job(job_id, status_text, code, None)?;
    let _ = tx.send(JobEvent {
        job_id: job_id.to_string(),
        status: status_text.to_string(),
        chunk: None,
    });
    Ok(())
}

fn exclusive_group_key(group: &str) -> String {
    format!("exclusive:{group}")
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
            match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(future),
                Err(err) => eprintln!("failed to start AgentHub job runtime: {err}"),
            }
        })
        .expect("spawn AgentHub job runtime thread");
}

fn stream_reader<R>(
    db: PanelDb,
    tx: broadcast::Sender<JobEvent>,
    job_id: String,
    reader: R,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let mut chunk = redact_output(&String::from_utf8_lossy(&buf[..n]));
                    if buf.ends_with(b"\n") {
                        chunk.push('\n');
                    }
                    buf.clear();
                    let _ = db.append_job_output(&job_id, &chunk);
                    let _ = tx.send(JobEvent {
                        job_id: job_id.clone(),
                        status: "running".to_string(),
                        chunk: Some(chunk),
                    });
                }
                Err(_) => break,
            }
        }
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
        let db = PanelDb::open(":memory:").unwrap();
        db.create_job("fixture", "fixture", "fixture").unwrap();
        let (mut writer, reader) = tokio::io::duplex(3);
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let reader = super::stream_reader(db.clone(), tx, "fixture".into(), reader);
        for byte in "中文完成\n\nOPENAI_API_KEY=fixture-secret\n尾行".as_bytes() {
            writer.write_all(&[*byte]).await.unwrap();
        }
        drop(writer);
        reader.await.unwrap();
        let output = db.job("fixture").unwrap().unwrap().output;
        assert_eq!(output, "中文完成\n\n[redacted sensitive line]\n尾行");
        assert!(db.list_jobs(10).unwrap()[0].output.is_empty());
    }

    #[tokio::test]
    async fn exclusive_shell_jobs_reject_same_group_until_finished() {
        let db = PanelDb::open(":memory:").unwrap();
        let runner = JobRunner::new(db);
        let mut events = runner.subscribe();

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

        wait_for_job_status(&mut events, &first, "succeeded").await;
        assert!(runner.exclusive_group_job("panel_update").is_none());

        let third = runner.start_exclusive_process_job(
            "panel_update_prune",
            "Panel prune",
            super::ProcessCommand::new("true", std::iter::empty::<&str>()),
            "panel_update",
        );
        assert!(third.is_ok());
    }

    async fn wait_for_job_status(
        events: &mut tokio::sync::broadcast::Receiver<super::JobEvent>,
        job_id: &str,
        status: &str,
    ) {
        let timeout = tokio::time::sleep(Duration::from_secs(3));
        tokio::pin!(timeout);
        loop {
            tokio::select! {
                _ = &mut timeout => panic!("timed out waiting for job {job_id} to reach {status}"),
                event = events.recv() => {
                    let event = event.expect("job event");
                    if event.job_id == job_id && event.status == status {
                        return;
                    }
                }
            }
        }
    }

    #[test]
    fn shell_jobs_start_without_current_tokio_runtime() {
        let db_path =
            std::env::temp_dir().join(format!("agenthub-job-runner-{}.sqlite", Uuid::new_v4()));
        let db = PanelDb::open(&db_path).unwrap();
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
