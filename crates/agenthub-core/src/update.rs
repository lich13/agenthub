use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateVersionInfo {
    pub panel: PanelVersionInfo,
    pub codex: CodexVersionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelVersionInfo {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexVersionInfo {
    pub user: Option<String>,
    pub root: Option<String>,
    pub raw: Option<String>,
    pub latest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobFailureCategory {
    ReleaseMissing,
    DownloadSha256Mismatch,
    SystemdFailure,
    NginxFailure,
    PermissionDeniedSudo,
    CodexAuthFailure,
    SqliteIntegrityFailure,
    ReadOnlyFileSystem,
    NetworkTlsEof,
    CodexLocalStateUnavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobFailureAnalysis {
    pub category: JobFailureCategory,
    pub explanation: String,
    pub suggestions: Vec<String>,
}

pub fn panel_update_command(configured: &str) -> String {
    if configured.trim().is_empty() {
        default_panel_update_command()
    } else {
        configured.to_string()
    }
}

pub fn default_panel_update_command() -> String {
    PANEL_UPDATE_DEFAULT.to_string()
}

pub const PANEL_PRUNE_SCRIPT: &str = r#"from pathlib import Path
import shutil

root = Path("/var/lib/agenthub-webd/backups/release-updates")
if not root.exists():
    print("no release update backups")
    raise SystemExit(0)

backups = sorted(path for path in root.iterdir() if path.is_dir())
remove = backups[:-3]
for path in remove:
    shutil.rmtree(path)
    print(f"removed {path}")

print(f"kept {len(backups) - len(remove)} release update backups")"#;

pub fn panel_prune_command() -> String {
    format!("python3 - <<'PY'\n{PANEL_PRUNE_SCRIPT}\nPY")
}

pub const PANEL_UPDATE_DEFAULT: &str =
    "/usr/local/bin/agenthub-webd-update --repo lich13/agenthub --version latest";
pub const PANEL_PRECHECK_DEFAULT: &str = "/usr/local/bin/agenthub-webd-update --precheck";

pub fn is_legacy_panel_update(value: &str) -> bool {
    matches!(value,
        "/usr/local/bin/codex-cloud-panel-update --repo lich13/codex-cloud-panel --version latest" |
        "/usr/local/bin/codex-cloud-panel-update --repo lich13/agenthub --version latest" |
        "/usr/local/bin/agenthub-webd-update --repo lich13/codex-cloud-panel --version latest")
}

pub fn is_legacy_panel_precheck(value: &str) -> bool {
    matches!(value,
        "test -x /usr/local/bin/agenthub-webd-update && systemctl is-active agenthub-webd && curl -fsS http://127.0.0.1:15742/healthz" |
        "test -x /usr/local/bin/codex-cloud-panel-update && systemctl is-active codex-cloud-panel && curl -fsS http://127.0.0.1:15732/healthz" |
        "/usr/local/bin/codex-cloud-panel-update --precheck" |
        "test -x /usr/local/bin/agenthub-update && systemctl is-active agenthub && curl -fsS http://127.0.0.1:15732/healthz")
}

/// Exact compatibility entries only. Never tokenize or execute user-supplied shell text.
pub fn panel_process(command: &str, precheck: bool) -> anyhow::Result<crate::jobs::ProcessCommand> {
    let valid = if precheck {
        command == PANEL_PRECHECK_DEFAULT || is_legacy_panel_precheck(command)
    } else {
        command.is_empty() || command == PANEL_UPDATE_DEFAULT || is_legacy_panel_update(command)
    };
    anyhow::ensure!(valid, "unsupported managed update command");
    Ok(crate::jobs::ProcessCommand::new(
        "/usr/local/bin/agenthub-webd-update",
        if precheck {
            vec!["--precheck"]
        } else {
            vec!["--repo", "lich13/agenthub", "--version", "latest"]
        },
    ))
}

pub fn analyze_job_failure(
    kind: &str,
    output: &str,
    error: Option<&str>,
    exit_code: Option<i32>,
) -> Option<JobFailureAnalysis> {
    if matches!(exit_code, Some(0)) && error.unwrap_or_default().trim().is_empty() {
        return None;
    }
    if exit_code.is_none() && error.unwrap_or_default().trim().is_empty() {
        return None;
    }

    let combined = format!("{kind}\n{output}\n{}", error.unwrap_or_default());
    let text = combined.to_ascii_lowercase();
    let category = if contains_any(
        &text,
        &[
            "release not found",
            "releases/tags",
            "404 not found",
            "not found for repo",
            "no release found",
        ],
    ) {
        JobFailureCategory::ReleaseMissing
    } else if contains_any(
        &text,
        &[
            "sha256",
            "checksum mismatch",
            "shasum",
            "digest mismatch",
            "hash mismatch",
        ],
    ) {
        JobFailureCategory::DownloadSha256Mismatch
    } else if is_read_only_file_system(&text) {
        JobFailureCategory::ReadOnlyFileSystem
    } else if contains_any(
        &text,
        &[
            "sudo: a password is required",
            "sudo: a terminal is required",
            "permission denied",
            "operation not permitted",
            "not in the sudoers",
        ],
    ) {
        JobFailureCategory::PermissionDeniedSudo
    } else if contains_any(
        &text,
        &[
            "systemctl",
            "systemd",
            "failed to start",
            "unit ",
            "journalctl",
        ],
    ) {
        JobFailureCategory::SystemdFailure
    } else if contains_any(
        &text,
        &[
            "nginx",
            "nginx -t",
            "proxy_pass",
            "emerg",
            "conflicting server name",
        ],
    ) {
        JobFailureCategory::NginxFailure
    } else if contains_any(
        &text,
        &[
            "codex login",
            "not authenticated",
            "authentication failed",
            "invalid api key",
            "401 unauthorized",
        ],
    ) {
        JobFailureCategory::CodexAuthFailure
    } else if contains_any(
        &text,
        &[
            "integrity_check",
            "database disk image is malformed",
            "sqlite corruption",
            "sqlite error",
            "pragma integrity_check",
        ],
    ) {
        JobFailureCategory::SqliteIntegrityFailure
    } else if contains_any(
        &text,
        &[
            "tls",
            "ssl",
            "connection reset",
            "connection refused",
            "connection timed out",
            "network timeout",
            "network is unreachable",
            "unexpected eof",
            "early eof",
            "curl:",
            "reqwest",
        ],
    ) {
        JobFailureCategory::NetworkTlsEof
    } else if contains_any(
        &text,
        &[
            "app-server unavailable",
            "app server unavailable",
            "app-server-control.sock",
            "bridge unavailable",
            "websocket",
        ],
    ) {
        JobFailureCategory::CodexLocalStateUnavailable
    } else {
        JobFailureCategory::Unknown
    };

    Some(analysis_for(category))
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| text.contains(pattern))
}

fn is_read_only_file_system(text: &str) -> bool {
    contains_any(
        text,
        &[
            "read-only file system",
            "read only file system",
            "os error 30",
            "erofs",
            "install.lock",
        ],
    ) || (text.contains(".codex")
        && contains_any(
            text,
            &[
                "not writable",
                "cannot write",
                "can't write",
                "read-only",
                "read only",
                "readonly",
                "不可写",
            ],
        ))
}

fn analysis_for(category: JobFailureCategory) -> JobFailureAnalysis {
    let (explanation, suggestions) = match category {
        JobFailureCategory::ReleaseMissing => (
            "The requested GitHub release could not be found for the configured repository or version.",
            vec![
                "Check that the repo and version/tag are correct.",
                "Retry with --version latest if a pinned release was removed or never published.",
            ],
        ),
        JobFailureCategory::DownloadSha256Mismatch => (
            "The downloaded archive did not match the expected SHA-256 digest.",
            vec![
                "Delete the partial download and rerun the update.",
                "Verify the release asset and .sha256 file were published from the same build.",
            ],
        ),
        JobFailureCategory::SystemdFailure => (
            "A systemd service command failed during the update.",
            vec![
                "Run systemctl status for the affected service.",
                "Check journalctl logs before retrying the update.",
            ],
        ),
        JobFailureCategory::NginxFailure => (
            "Nginx validation or reload failed.",
            vec![
                "Run nginx -t to identify the invalid config line.",
                "Check the panel nginx location and upstream service before reloading.",
            ],
        ),
        JobFailureCategory::PermissionDeniedSudo => (
            "The update command could not run with the required permissions or passwordless sudo.",
            vec![
                "Confirm the panel daemon user has passwordless sudo for the fixed update wrapper.",
                "Check file ownership and execute permissions on the update script.",
            ],
        ),
        JobFailureCategory::CodexAuthFailure => (
            "Codex CLI authentication failed for the user used by the update or precheck.",
            vec![
                "Run the Codex auth check as the same user shown in the job output.",
                "Refresh Codex credentials before retrying.",
            ],
        ),
        JobFailureCategory::SqliteIntegrityFailure => (
            "A SQLite integrity check failed or the database could not be read safely.",
            vec![
                "Stop services that write to the database before investigating.",
                "Restore from a known-good backup if integrity_check reports corruption.",
            ],
        ),
        JobFailureCategory::ReadOnlyFileSystem => (
            "The update tried to write to a read-only filesystem, commonly the Codex home or install lock path.",
            vec![
                "Run the configured update wrapper from a separate root systemd unit.",
                "Check that /root/.codex and the Codex admin paths are writable from the transient unit.",
            ],
        ),
        JobFailureCategory::NetworkTlsEof => (
            "A network, TLS, or EOF error interrupted a download or remote API request.",
            vec![
                "Retry after checking DNS and outbound HTTPS connectivity.",
                "If the failure repeats, inspect proxy/firewall settings and GitHub reachability.",
            ],
        ),
        JobFailureCategory::CodexLocalStateUnavailable => (
            "Codex local state or the controlled Codex job path was unavailable.",
            vec![
                "Check the resolved Codex home and required state files.",
                "Review the fixed Codex job output before retrying.",
            ],
        ),
        JobFailureCategory::Unknown => (
            "The job failed, but the output did not match a known failure pattern.",
            vec![
                "Review the full job output and exit code.",
                "Rerun the job after preserving the current logs for comparison.",
            ],
        ),
    };

    JobFailureAnalysis {
        category,
        explanation: explanation.to_string(),
        suggestions: suggestions.into_iter().map(str::to_string).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::panel_prune_command;

    #[test]
    fn panel_prune_command_keeps_recent_release_update_backups() {
        let command = panel_prune_command();

        assert!(command.contains("/var/lib/agenthub-webd/backups/release-updates"));
        assert!(command.contains("backups[:-3]"));
        assert!(command.contains("shutil.rmtree"));
    }
}
