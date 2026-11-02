use crate::platform::{PlatformKind, PlatformPaths};
use crate::security::redact_output;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const CODEX_TURN_LOG_TARGET: &str = "codex_core::session::turn";
pub const PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL: &str = "com.lich13.agenthub.probe-error-monitor";
const TERMINAL_ERROR_MARKER: &str = ":run_turn: Turn error:";
const MAX_ERROR_SUMMARY_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeErrorCursor {
    pub database_identity: String,
    pub ts: i64,
    pub ts_nanos: i64,
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeErrorScan {
    pub cursor: ProbeErrorCursor,
    pub baseline_only: bool,
    pub rows_seen: usize,
    pub incidents: Vec<CodexTurnErrorIncident>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeErrorMonitorRuntimeStatus {
    pub status: String,
    pub last_scan_at: i64,
    pub last_error: Option<String>,
    pub rows_seen: usize,
    pub new_incidents: usize,
    pub incident_count: u64,
    pub cursor: Option<ProbeErrorCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeErrorMonitorLaunchAgentStatus {
    pub supported: bool,
    pub enabled: bool,
    pub loaded: bool,
    pub changed: bool,
    pub label: String,
    pub plist_path: Option<PathBuf>,
    pub helper_path: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexTurnErrorClass {
    ServerOverloaded,
    RateLimited,
    ServerError,
    TransportError,
    AuthenticationError,
    PolicyError,
    InvalidRequest,
    Unknown,
}

impl CodexTurnErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServerOverloaded => "server_overloaded",
            Self::RateLimited => "rate_limited",
            Self::ServerError => "server_error",
            Self::TransportError => "transport_error",
            Self::AuthenticationError => "authentication_error",
            Self::PolicyError => "policy_error",
            Self::InvalidRequest => "invalid_request",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexTurnErrorIncident {
    pub source_ts: i64,
    pub source_ts_nanos: i64,
    pub source_row_id: i64,
    pub thread_id: String,
    pub turn_id: String,
    pub classification: CodexTurnErrorClass,
    pub summary: String,
    pub error_sha256: String,
    pub incident_key: String,
}

pub fn scan_codex_turn_errors(
    logs_db_path: &Path,
    previous: Option<&ProbeErrorCursor>,
    batch_limit: usize,
) -> Result<ProbeErrorScan> {
    let database_identity = logs_database_identity(logs_db_path)?;
    let conn = Connection::open_with_flags(
        logs_db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open Codex logs DB {}", logs_db_path.display()))?;
    conn.busy_timeout(std::time::Duration::from_millis(500))?;
    let latest = latest_cursor_tuple(&conn)?;
    let establish_baseline = match previous {
        None => true,
        Some(cursor) if cursor.database_identity != database_identity => true,
        Some(cursor) => !cursor_tuple_exists(&conn, cursor)?,
    };
    if establish_baseline {
        let (ts, ts_nanos, id) = latest.unwrap_or((0, 0, 0));
        return Ok(ProbeErrorScan {
            cursor: ProbeErrorCursor {
                database_identity,
                ts,
                ts_nanos,
                id,
            },
            baseline_only: true,
            rows_seen: 0,
            incidents: Vec::new(),
        });
    }

    let previous = previous.expect("checked above");
    let limit = batch_limit.clamp(1, 1_000) as i64;
    let mut statement = conn.prepare(
        r#"
        SELECT id, ts, ts_nanos, target, feedback_log_body, thread_id
        FROM logs
        WHERE ts > ?1
           OR (ts = ?1 AND ts_nanos > ?2)
           OR (ts = ?1 AND ts_nanos = ?2 AND id > ?3)
        ORDER BY ts ASC, ts_nanos ASC, id ASC
        LIMIT ?4
        "#,
    )?;
    let rows = statement.query_map(
        params![previous.ts, previous.ts_nanos, previous.id, limit],
        |row| {
            Ok(LogRow {
                id: row.get(0)?,
                ts: row.get(1)?,
                ts_nanos: row.get(2)?,
                target: row.get(3)?,
                body: row.get(4)?,
                thread_id: row.get(5)?,
            })
        },
    )?;
    let mut cursor = previous.clone();
    let mut rows_seen = 0usize;
    let mut incidents = Vec::new();
    for row in rows {
        let row = row?;
        rows_seen += 1;
        cursor.ts = row.ts;
        cursor.ts_nanos = row.ts_nanos;
        cursor.id = row.id;
        if let Some(incident) = parse_turn_error_row(row) {
            incidents.push(incident);
        }
    }
    Ok(ProbeErrorScan {
        cursor,
        baseline_only: false,
        rows_seen,
        incidents,
    })
}

#[derive(Debug)]
struct LogRow {
    id: i64,
    ts: i64,
    ts_nanos: i64,
    target: String,
    body: Option<String>,
    thread_id: Option<String>,
}

fn parse_turn_error_row(row: LogRow) -> Option<CodexTurnErrorIncident> {
    if row.target != CODEX_TURN_LOG_TARGET {
        return None;
    }
    let thread_id = required_identity(row.thread_id.as_deref())?;
    let body = row.body.as_deref()?;
    let (scope, error) = body.rsplit_once(TERMINAL_ERROR_MARKER)?;
    if !scope.contains(":turn{") || !scope.contains(":session_task.run") {
        return None;
    }
    let turn_id =
        extract_scope_value(scope, "turn.id=").and_then(|value| required_identity(Some(value)))?;
    let summary = bounded_redacted_summary(error)?;
    let classification = classify_codex_turn_error(&summary);
    let error_sha256 = sha256_hex(summary.as_bytes());
    let incident_key = sha256_hex(format!("{thread_id}\0{turn_id}\0{error_sha256}").as_bytes());
    Some(CodexTurnErrorIncident {
        source_ts: row.ts,
        source_ts_nanos: row.ts_nanos,
        source_row_id: row.id,
        thread_id,
        turn_id,
        classification,
        summary,
        error_sha256,
        incident_key,
    })
}

pub fn classify_codex_turn_error(summary: &str) -> CodexTurnErrorClass {
    let lowered = summary.to_ascii_lowercase();
    if lowered.contains("at capacity")
        || lowered.contains("server overloaded")
        || lowered.contains("server_overloaded")
        || lowered.contains("temporarily overloaded")
    {
        CodexTurnErrorClass::ServerOverloaded
    } else if lowered.contains("429")
        || lowered.contains("rate limit")
        || lowered.contains("too many requests")
    {
        CodexTurnErrorClass::RateLimited
    } else if ["500", "502", "503", "504", "internal server error"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        CodexTurnErrorClass::ServerError
    } else if [
        "connection",
        "stream disconnected",
        "stream ended",
        "network error",
        "timed out",
        "timeout",
        "unexpected eof",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        CodexTurnErrorClass::TransportError
    } else if ["authentication", "unauthorized", "401", "api key"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        CodexTurnErrorClass::AuthenticationError
    } else if ["policy", "safety", "content filter", "permission denied"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        CodexTurnErrorClass::PolicyError
    } else if [
        "invalid parameter",
        "invalid request",
        "unsupported parameter",
        "bad request",
        "400",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        CodexTurnErrorClass::InvalidRequest
    } else {
        CodexTurnErrorClass::Unknown
    }
}

fn bounded_redacted_summary(value: &str) -> Option<String> {
    let redacted = redact_output(value.trim());
    let redacted = redacted.trim();
    if redacted.is_empty() {
        return None;
    }
    if redacted.len() <= MAX_ERROR_SUMMARY_BYTES {
        return Some(redacted.to_string());
    }
    let mut end = MAX_ERROR_SUMMARY_BYTES;
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{} [truncated]", &redacted[..end]))
}

fn extract_scope_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let start = text.rfind(key)? + key.len();
    let rest = &text[start..];
    if let Some(rest) = rest.strip_prefix('"') {
        return rest.split('"').next();
    }
    let end = rest
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '}' | ':' | ','))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn required_identity(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn latest_cursor_tuple(conn: &Connection) -> Result<Option<(i64, i64, i64)>> {
    conn.query_row(
        "SELECT ts, ts_nanos, id FROM logs ORDER BY ts DESC, ts_nanos DESC, id DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(Into::into)
}

fn cursor_tuple_exists(conn: &Connection, cursor: &ProbeErrorCursor) -> Result<bool> {
    if (cursor.ts, cursor.ts_nanos, cursor.id) == (0, 0, 0) {
        return Ok(true);
    }
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM logs WHERE ts = ?1 AND ts_nanos = ?2 AND id = ?3
        )",
        params![cursor.ts, cursor.ts_nanos, cursor.id],
        |row| row.get::<_, bool>(0),
    )
    .map_err(Into::into)
}

fn sha256_hex(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn logs_database_identity(path: &Path) -> Result<String> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("resolve Codex logs DB {}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .with_context(|| format!("stat Codex logs DB {}", canonical.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!(
            "{}:{}:{}",
            canonical.display(),
            metadata.dev(),
            metadata.ino()
        ))
    }
    #[cfg(not(unix))]
    {
        let created = metadata
            .created()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        Ok(format!("{}:{created}", canonical.display()))
    }
}

pub fn probe_error_monitor_launch_agent_plist(
    helper: &Path,
    config: &Path,
    stdout_log: &Path,
    stderr_log: &Path,
) -> String {
    let xml = |value: &Path| xml_escape(&value.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>--config</string>
    <string>{}</string>
    <string>probe</string>
    <string>monitor-errors</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL,
        xml(helper),
        xml(config),
        xml(stdout_log),
        xml(stderr_log),
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn ensure_probe_error_monitor_launch_agent(
    platform: &PlatformPaths,
    enabled: bool,
) -> Result<ProbeErrorMonitorLaunchAgentStatus> {
    if platform.kind != PlatformKind::Macos {
        return Ok(ProbeErrorMonitorLaunchAgentStatus {
            supported: false,
            enabled,
            loaded: false,
            changed: false,
            label: PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL.to_string(),
            plist_path: None,
            helper_path: platform.daemon_binary(),
            config_path: platform.config_file.clone(),
        });
    }
    let home = dirs::home_dir().context("resolve home for Probe error monitor LaunchAgent")?;
    ensure_probe_error_monitor_launch_agent_at(
        platform,
        enabled,
        &home.join("Library/LaunchAgents"),
        Path::new("/bin/launchctl"),
    )
}

#[doc(hidden)]
pub fn ensure_probe_error_monitor_launch_agent_at(
    platform: &PlatformPaths,
    enabled: bool,
    launch_agents_dir: &Path,
    launchctl: &Path,
) -> Result<ProbeErrorMonitorLaunchAgentStatus> {
    let helper = platform.daemon_binary();
    let plist_path =
        launch_agents_dir.join(format!("{PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL}.plist"));
    let domain = launch_agent_domain()?;
    let service = format!("{domain}/{PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL}");
    let loaded_before = Command::new(launchctl)
        .arg("print")
        .arg(&service)
        .output()
        .is_ok_and(|output| output.status.success());

    if !enabled {
        if loaded_before {
            let status = Command::new(launchctl)
                .arg("bootout")
                .arg(&service)
                .status()
                .context("stop Probe error monitor LaunchAgent")?;
            anyhow::ensure!(
                status.success(),
                "launchctl bootout Probe error monitor failed"
            );
        }
        let changed = if plist_path.exists() {
            fs::remove_file(&plist_path)?;
            true
        } else {
            loaded_before
        };
        return Ok(ProbeErrorMonitorLaunchAgentStatus {
            supported: true,
            enabled: false,
            loaded: false,
            changed,
            label: PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL.to_string(),
            plist_path: Some(plist_path),
            helper_path: helper,
            config_path: platform.config_file.clone(),
        });
    }

    anyhow::ensure!(
        helper.is_file(),
        "Probe error monitor helper missing: {}",
        helper.display()
    );
    fs::create_dir_all(launch_agents_dir)?;
    fs::create_dir_all(&platform.log_dir)?;
    let desired = probe_error_monitor_launch_agent_plist(
        &helper,
        &platform.config_file,
        &platform.log_dir.join("probe-error-monitor.out.log"),
        &platform.log_dir.join("probe-error-monitor.err.log"),
    );
    let changed = fs::read_to_string(&plist_path).ok().as_deref() != Some(desired.as_str());
    if changed {
        fs::write(&plist_path, desired)?;
    }
    if changed && loaded_before {
        let status = Command::new(launchctl)
            .arg("bootout")
            .arg(&service)
            .status()
            .context("stop stale Probe error monitor LaunchAgent")?;
        anyhow::ensure!(
            status.success(),
            "launchctl bootout stale Probe error monitor failed"
        );
    }
    if changed || !loaded_before {
        let status = Command::new(launchctl)
            .arg("bootstrap")
            .arg(&domain)
            .arg(&plist_path)
            .status()
            .context("start Probe error monitor LaunchAgent")?;
        anyhow::ensure!(
            status.success(),
            "launchctl bootstrap Probe error monitor failed"
        );
    }
    let loaded = Command::new(launchctl)
        .arg("print")
        .arg(&service)
        .output()
        .is_ok_and(|output| output.status.success());
    anyhow::ensure!(loaded, "Probe error monitor LaunchAgent is not loaded");
    let status = Command::new(launchctl)
        .arg("kickstart")
        .arg("-k")
        .arg(&service)
        .status()
        .context("restart Probe error monitor LaunchAgent")?;
    anyhow::ensure!(
        status.success(),
        "launchctl kickstart Probe error monitor failed"
    );
    Ok(ProbeErrorMonitorLaunchAgentStatus {
        supported: true,
        enabled: true,
        loaded,
        changed,
        label: PROBE_ERROR_MONITOR_LAUNCH_AGENT_LABEL.to_string(),
        plist_path: Some(plist_path),
        helper_path: helper,
        config_path: platform.config_file.clone(),
    })
}

fn launch_agent_domain() -> Result<String> {
    let output = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .context("resolve uid for Probe error monitor LaunchAgent")?;
    anyhow::ensure!(output.status.success(), "id -u failed");
    let uid = String::from_utf8(output.stdout)?.trim().to_string();
    anyhow::ensure!(!uid.is_empty(), "id -u returned empty uid");
    Ok(format!("gui/{uid}"))
}
