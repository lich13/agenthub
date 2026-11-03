//! Short-lived native helper for catalogs, empty-thread creation and deletion.
//! It has no resume/turn/queue execution entrypoint. All conversation execution
//! is routed to the already running desktop owner through the IPC session.
use super::{DesktopError, Result};
use crate::resource_limits::{
    CODEX_DESKTOP_APPS_WAIT as APPS_WAIT, CODEX_DESKTOP_FRAME_BYTES,
    CODEX_DESKTOP_HELPER_PERMIT_WAIT as PERMIT_WAIT, CODEX_DESKTOP_HELPER_WAIT as HELPER_WAIT,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{timeout, timeout_at, Instant},
};

static HELPER_GATE: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub enum CatalogQuery {
    Models { cursor: Option<String> },
    Modes,
    Skills { cwd: PathBuf },
    Plugins { cwd: PathBuf },
    Apps { cursor: Option<String> },
}

pub struct DesktopHelper {
    executable: PathBuf,
    codex_home: PathBuf,
}

impl DesktopHelper {
    pub async fn management(codex_home: PathBuf) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            Self::installed(codex_home).await
        }
        #[cfg(not(target_os = "macos"))]
        {
            let executable = crate::codex::CodexAppServerClient::new()
                .resolve_executable()
                .await
                .map_err(|_| DesktopError::Unavailable)?;
            Ok(Self {
                executable,
                codex_home,
            })
        }
    }

    /// No resume, turn or arbitrary method. Once submitted, the caller must
    /// inspect storage even on failure and must never fall back or retry.
    pub async fn installed(codex_home: PathBuf) -> Result<Self> {
        let executable = crate::read_tasks::spawn_blocking(|| {
            let bundle = Path::new("/Applications/ChatGPT.app/Contents/Resources");
            for relative in ["codex-cli/bin/codex", "codex"] {
                let candidate = bundle.join(relative);
                if let Ok(metadata) = candidate.metadata() {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.is_file()
                        && metadata.permissions().mode() & 0o111 != 0
                        && metadata.permissions().mode() & 0o022 == 0
                    {
                        let canonical = candidate
                            .canonicalize()
                            .map_err(|_| DesktopError::Unavailable)?;
                        if canonical.starts_with(bundle) {
                            return Ok(canonical);
                        }
                    }
                }
            }
            Err(DesktopError::Unavailable)
        })
        .await
        .map_err(|_| DesktopError::Unavailable)??;
        Ok(Self {
            executable,
            codex_home,
        })
    }

    pub async fn catalog(&self, query: CatalogQuery) -> Result<Value> {
        let mut reader = self
            .catalog_reader(matches!(query, CatalogQuery::Apps { .. }))
            .await?;
        let result = reader.catalog(query, false).await;
        reader.finish().await;
        result
    }

    /// One permit and one native process for a complete logical catalog read.
    /// The deadline includes initialization and all pages, not queue waiting.
    pub async fn catalog_reader(&self, apps: bool) -> Result<CatalogReader> {
        let permit = helper_permit().await?;
        let mut process = HelperProcess::start(&self.executable, &self.codex_home)?;
        let deadline = Instant::now() + if apps { APPS_WAIT } else { HELPER_WAIT };
        timeout_at(deadline, process.initialize())
            .await
            .map_err(|_| DesktopError::Timeout)??;
        Ok(CatalogReader {
            _permit: permit,
            process,
            deadline,
            next_id: 2,
        })
    }

    /// Native thread/start creates an empty persistent thread; it never starts a
    /// turn. Loading it into a desktop window remains a separate explicit action.
    pub async fn create_empty(&self, cwd: &Path) -> Result<String> {
        if !cwd.is_absolute() {
            return Err(DesktopError::InvalidInput);
        }
        let result = self
            .with_process(
                "thread/start",
                json!({
                    "cwd":cwd,"ephemeral":false,"historyMode":"paginated"
                }),
            )
            .await?;
        let id = result["thread"]["id"]
            .as_str()
            .ok_or(DesktopError::OutcomeUnknown)?;
        uuid::Uuid::parse_str(id).map_err(|_| DesktopError::OutcomeUnknown)?;
        Ok(id.into())
    }

    async fn with_process(&self, method: &'static str, params: Value) -> Result<Value> {
        let _permit = helper_permit().await?;
        let mut process = HelperProcess::start(&self.executable, &self.codex_home)?;
        let mut submitted = false;
        let operation = async {
            process.initialize().await?;
            submitted = true;
            process.request(2, method, params).await
        };
        let result = timeout(HELPER_WAIT, operation)
            .await
            .map_err(|_| DesktopError::Timeout)
            .and_then(|result| result);
        process.finish().await;
        // No verified helper error proves absence of a persistent thread.
        // Once submitted, never make an uncertain creation look retryable.
        if method == "thread/start" && submitted && result.is_err() {
            return Err(DesktopError::OutcomeUnknown);
        }
        result
    }
}

async fn helper_permit() -> Result<OwnedSemaphorePermit> {
    timeout(
        PERMIT_WAIT,
        HELPER_GATE
            .get_or_init(|| Arc::new(Semaphore::new(1)))
            .clone()
            .acquire_owned(),
    )
    .await
    .map_err(|_| DesktopError::Busy)?
    .map_err(|_| DesktopError::Unavailable)
}

pub struct CatalogReader {
    _permit: OwnedSemaphorePermit,
    process: HelperProcess,
    deadline: Instant,
    next_id: u64,
}

impl CatalogReader {
    pub async fn catalog(&mut self, query: CatalogQuery, force_refetch: bool) -> Result<Value> {
        let (method, params) = catalog_request(query, force_refetch)?;
        let id = self.next_id;
        self.next_id += 1;
        let result = timeout_at(self.deadline, self.process.request(id, method, params))
            .await
            .map_err(|_| DesktopError::Timeout)
            .and_then(|result| result);
        if let Err(error) = result {
            super::diagnostics::record(
                super::diagnostics::Source::Helper,
                method,
                None,
                None,
                None,
                error,
            );
        }
        result
    }
    pub async fn finish(self) {
        self.process.finish().await;
    }
}

fn catalog_request(query: CatalogQuery, force_refetch: bool) -> Result<(&'static str, Value)> {
    Ok(match query {
        CatalogQuery::Models { cursor } => (
            "model/list",
            json!({"includeHidden":false,"limit":100,"cursor":cursor}),
        ),
        CatalogQuery::Modes => ("collaborationMode/list", json!({})),
        CatalogQuery::Skills { cwd } => (
            "skills/list",
            json!({"cwds":[cwd],"forceReload":force_refetch}),
        ),
        CatalogQuery::Plugins { cwd } => (
            "plugin/list",
            json!({"cwds":[cwd],"forceRefetch":force_refetch}),
        ),
        CatalogQuery::Apps { cursor } => {
            if cursor.as_ref().is_some_and(|cursor| cursor.len() > 4096) {
                return Err(DesktopError::InvalidInput);
            }
            // A transient helper does not own or resume the desktop thread.
            (
                "app/list",
                json!({"threadId":null,"cursor":cursor,"limit":100,"forceRefetch":force_refetch}),
            )
        }
    })
}

struct HelperProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl HelperProcess {
    async fn initialize(&mut self) -> Result<()> {
        self.request(1, "initialize", json!({"clientInfo":{"name":"agenthub","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
        self.write(&json!({"method":"initialized"})).await
    }

    async fn finish(mut self) {
        self.stdin.take();
        if timeout(Duration::from_secs(1), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }

    fn start(executable: &Path, codex_home: &Path) -> Result<Self> {
        let mut child = crate::jobs::spawn_retrying_etxtbsy(
            Command::new(executable)
                .args(["app-server", "--listen", "stdio://"])
                .env("CODEX_HOME", codex_home)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true),
        )
        .map_err(|_| DesktopError::Unavailable)?;
        let stdin = child.stdin.take().ok_or(DesktopError::Unavailable)?;
        let stdout = child.stdout.take().ok_or(DesktopError::Unavailable)?;
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        })
    }

    async fn write(&mut self, message: &Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(message).map_err(|_| DesktopError::InvalidInput)?;
        if bytes.len() > CODEX_DESKTOP_FRAME_BYTES {
            return Err(DesktopError::FrameTooLarge);
        }
        bytes.push(b'\n');
        let stdin = self.stdin.as_mut().ok_or(DesktopError::Disconnected)?;
        stdin
            .write_all(&bytes)
            .await
            .map_err(|_| DesktopError::Disconnected)?;
        stdin.flush().await.map_err(|_| DesktopError::Disconnected)
    }

    async fn request(&mut self, id: u64, method: &'static str, params: Value) -> Result<Value> {
        self.write(&json!({"id":id,"method":method,"params":params}))
            .await?;
        loop {
            let mut value = read_bounded_line(&mut self.stdout).await?;
            if value["id"].as_u64() != Some(id) {
                continue;
            }
            if value.get("error").is_some() {
                super::diagnostics::record(
                    super::diagnostics::Source::Helper,
                    method,
                    None,
                    value["error"]["code"].as_i64(),
                    value["error"]["message"].as_str(),
                    DesktopError::Rejected,
                );
                return Err(DesktopError::Rejected);
            }
            return value
                .as_object_mut()
                .and_then(|value| value.remove("result"))
                .ok_or(DesktopError::InvalidFrame);
        }
    }
}

async fn read_bounded_line(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Value> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|_| DesktopError::Disconnected)?;
        if available.is_empty() {
            return Err(DesktopError::Disconnected);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(available.len(), |index| index + 1);
        if bytes.len().saturating_add(length) > CODEX_DESKTOP_FRAME_BYTES {
            return Err(DesktopError::FrameTooLarge);
        }
        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);
        if newline.is_some() {
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| DesktopError::InvalidFrame)?;
            if !value.is_object() {
                return Err(DesktopError::InvalidFrame);
            }
            return Ok(value);
        }
    }
}

#[cfg(test)]
#[path = "helper_tests.rs"]
mod tests;
