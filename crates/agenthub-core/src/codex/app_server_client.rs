use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    process::{ChildStdin, Command},
    sync::Mutex,
    task::JoinSet,
    time::timeout,
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

    #[doc(hidden)]
    pub fn with_candidates(candidates: Vec<PathBuf>, _request_timeout: Duration) -> Self {
        Self::with_optional_candidates(Some(candidates))
    }

    #[cfg(test)]
    pub(super) fn with_resolved_executable(
        executable: PathBuf,
        _request_timeout: Duration,
    ) -> Self {
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
        let mut line = String::new();
        let read = stdout.read_line(&mut line).await?;
        if read == 0 {
            return Err(anyhow!(
                "Codex app-server closed before response {expected_id}"
            ));
        }
        if line.len() > MAX_PROTOCOL_LINE_BYTES {
            return Err(anyhow!("Codex app-server response exceeded size limit"));
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
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
    parse_codex_version(&stdout)
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ParsedVersion {
    major: u64,
    minor: u64,
    patch: u64,
    stable: bool,
    suffix: String,
}

impl ParsedVersion {
    const fn stable(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            stable: true,
            suffix: String::new(),
        }
    }
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.major,
            self.minor,
            self.patch,
            self.stable,
            self.suffix.as_str(),
        )
            .cmp(&(
                other.major,
                other.minor,
                other.patch,
                other.stable,
                other.suffix.as_str(),
            ))
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_codex_version(value: &str) -> Option<ParsedVersion> {
    value.split_whitespace().find_map(|part| {
        let part = part.trim_start_matches('v');
        let (core, suffix) = part.split_once('-').unwrap_or((part, ""));
        let mut pieces = core.split('.');
        let major = pieces.next()?.parse().ok()?;
        let minor = pieces.next()?.parse().ok()?;
        let patch = pieces.next()?.parse().ok()?;
        if pieces.next().is_some() {
            return None;
        }
        Some(ParsedVersion {
            major,
            minor,
            patch,
            stable: suffix.is_empty(),
            suffix: suffix.to_string(),
        })
    })
}

#[cfg(test)]
#[path = "app_server_client_tests.rs"]
mod tests;
