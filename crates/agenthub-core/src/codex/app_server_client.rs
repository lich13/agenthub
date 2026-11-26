use super::{thread_rows::read_thread_rows_matching, CodexPaths, ThreadStatus};
use crate::system::ParsedVersion;
use anyhow::{anyhow, ensure, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::Mutex,
    task::JoinSet,
    time::{timeout, timeout_at, Instant},
};

const MIN_CODEX_APP_SERVER_VERSION: ParsedVersion = ParsedVersion::stable(0, 144, 2);
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROTOCOL_LINE_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct CodexAppServerClient {
    inner: Arc<CodexAppServerClientInner>,
}

struct CodexAppServerClientInner {
    candidates: Option<Vec<PathBuf>>,
    selected_executable: Mutex<Option<PathBuf>>,
}

impl Default for CodexAppServerClient {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexAppServerClient {
    pub fn new() -> Self {
        Self::with_optional_candidates(None)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn with_candidates(candidates: Vec<PathBuf>) -> Self {
        Self::with_optional_candidates(Some(candidates))
    }

    #[cfg(test)]
    pub(super) fn with_resolved_executable(executable: PathBuf) -> Self {
        Self {
            inner: Arc::new(CodexAppServerClientInner {
                candidates: Some(vec![executable.clone()]),
                selected_executable: Mutex::new(Some(executable)),
            }),
        }
    }

    fn with_optional_candidates(candidates: Option<Vec<PathBuf>>) -> Self {
        Self {
            inner: Arc::new(CodexAppServerClientInner {
                candidates,
                selected_executable: Mutex::new(None),
            }),
        }
    }

    pub async fn resolve_executable(&self) -> Result<PathBuf> {
        if let Some(path) = self.inner.selected_executable.lock().await.clone() {
            return Ok(path);
        }
        let candidates = self
            .inner
            .candidates
            .clone()
            .unwrap_or_else(discover_codex_candidates);
        let mut best: Option<(ParsedVersion, usize, PathBuf)> = None;
        let mut seen = HashSet::new();
        let mut probes = JoinSet::new();
        for (priority, candidate) in candidates.into_iter().enumerate() {
            let identity = fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
            if !seen.insert(identity) || !is_executable_file(&candidate) {
                continue;
            }
            probes.spawn(async move {
                let version = probe_codex_version(&candidate).await;
                (priority, candidate, version)
            });
        }
        while let Some(probe) = probes.join_next().await {
            let Ok((priority, candidate, Some(version))) = probe else {
                continue;
            };
            if version < MIN_CODEX_APP_SERVER_VERSION {
                continue;
            }
            let replace = best.as_ref().is_none_or(|(current, current_priority, _)| {
                version > *current || (version == *current && priority < *current_priority)
            });
            if replace {
                best = Some((version, priority, candidate));
            }
        }
        let path = best.map(|(_, _, path)| path).ok_or_else(|| {
            anyhow!("Codex CLI 0.144.2 or newer is required for native thread naming")
        })?;
        *self.inner.selected_executable.lock().await = Some(path.clone());
        Ok(path)
    }

    pub async fn rename_thread(&self, paths: &CodexPaths, id: &str, title: &str) -> Result<()> {
        rename_with_client(paths, id, title, self, Duration::from_secs(10)).await
    }
}

pub async fn set_thread_title(paths: &CodexPaths, id: &str, title: &str) -> Result<()> {
    static CLIENT: OnceLock<CodexAppServerClient> = OnceLock::new();
    CLIENT
        .get_or_init(CodexAppServerClient::new)
        .rename_thread(paths, id, title)
        .await
}

async fn rename_with_client(
    paths: &CodexPaths,
    id: &str,
    title: &str,
    client: &CodexAppServerClient,
    timeout: Duration,
) -> Result<()> {
    let name = title.trim();
    ensure!(!name.is_empty(), "thread title cannot be empty");
    ensure!(!id.trim().is_empty(), "thread_id is required");
    ensure!(
        !read_thread_rows_matching(paths, Some(id))?
            .iter()
            .any(|row| row.summary.status == ThreadStatus::Archived),
        "Archived tasks must be restored before renaming"
    );
    let deadline = Instant::now() + timeout;
    let executable = timeout_at(deadline, client.resolve_executable())
        .await
        .context("Codex CLI discovery timed out")?
        .context("Codex CLI is required to rename native tasks")?;
    let mut child = Command::new(executable)
        .args(["app-server", "--stdio"])
        .env("CODEX_HOME", &paths.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("start Codex app-server for task rename")?;
    let result = timeout_at(deadline, async {
        let mut stdin = child.stdin.take().context("open Codex app-server stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("open Codex app-server stdout")?;
        let mut stdout = BufReader::new(stdout.take(4 * 1024 * 1024));
        write_message(
            &mut stdin,
            &json!({"id":1,"method":"initialize","params":{
                "clientInfo":{"name":"agenthub","version":env!("CARGO_PKG_VERSION")}
            }}),
        )
        .await?;
        let _: Value = read_response(&mut stdout, 1).await?;
        write_message(&mut stdin, &json!({"method":"initialized"})).await?;
        write_message(
            &mut stdin,
            &json!({"id":2,"method":"thread/name/set","params":{"threadId":id,"name":name}}),
        )
        .await?;
        let _: Value = read_response(&mut stdout, 2).await?;
        // Native persistence chooses title/name by history format and updates the name index.
        write_message(
            &mut stdin,
            &json!({"id":3,"method":"thread/read","params":{"threadId":id,"includeTurns":false}}),
        )
        .await?;
        let value: Value = read_response(&mut stdout, 3).await?;
        ensure!(
            value.pointer("/thread/id").and_then(Value::as_str) == Some(id)
                && value.pointer("/thread/name").and_then(Value::as_str) == Some(name),
            "Codex native rename verification failed; resulting state is unknown"
        );
        Ok(())
    })
    .await
    .context("Codex native rename timed out; resulting state is unknown")
    .and_then(|result| result);
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

pub(crate) async fn write_message(stdin: &mut ChildStdin, value: &Value) -> Result<()> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    stdin.write_all(&encoded).await?;
    stdin.flush().await?;
    Ok(())
}

pub(crate) async fn read_response<T: DeserializeOwned>(
    stdout: &mut (impl tokio::io::AsyncBufRead + Unpin),
    expected_id: i64,
) -> Result<T> {
    loop {
        let mut line = Vec::new();
        let read = (&mut *stdout)
            .take(MAX_PROTOCOL_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        if read == 0 {
            return Err(anyhow!(
                "Codex app-server closed before response {expected_id}"
            ));
        }
        if line.len() > MAX_PROTOCOL_LINE_BYTES {
            return Err(anyhow!("Codex app-server response exceeded size limit"));
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            return Err(anyhow!("Codex app-server returned invalid JSON"));
        };
        if value.get("id").and_then(Value::as_i64) != Some(expected_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown Codex app-server error");
            return Err(anyhow!("Codex app-server request failed: {message}"));
        }
        let result = value
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("Codex app-server response did not include result"))?;
        return serde_json::from_value(result).context("decode Codex app-server response");
    }
}

async fn probe_codex_version(path: &Path) -> Option<ParsedVersion> {
    let mut command = Command::new(path);
    command.arg("--version").kill_on_drop(true);
    let output = timeout(VERSION_PROBE_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    ParsedVersion::extract_and_parse(&stdout)
}

fn discover_codex_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|entry| entry.join("codex")));
    }
    if let Some(home) = dirs::home_dir() {
        let nvm_root = home.join(".nvm/versions/node");
        if let Ok(entries) = fs::read_dir(nvm_root) {
            let mut paths = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path().join("bin/codex")))
                .collect::<Vec<_>>();
            paths.sort();
            candidates.extend(paths);
        }
        candidates.push(home.join(".volta/bin/codex"));
        candidates.push(home.join(".local/bin/codex"));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/usr/bin/codex"),
        // Newer desktop bundles keep the CLI in its own package. Retain the
        // previous layout for existing installations and probe both normally.
        PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex"),
        PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"),
    ]);
    candidates
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(test)]
#[path = "app_server_client_tests.rs"]
mod tests;
