use crate::shell::{shell_quote, QuoteStyle};
use crate::{
    codex::{extract_proposed_plan_text, resolve_codex_paths, ResolvedCodexPaths, ThreadSummary},
    config::Config,
    db::ProbeEvent,
    platform::{PlatformKind, PlatformPaths},
    security::{is_sensitive_output_line, redact_output},
    services::probe::aggregate_probe_status,
};
use anyhow::Result;
use chrono::Utc;
#[cfg(test)]
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const PROBE_EVENT_DEDUPE_NAMESPACE: &str = "probe_event";
pub const PROBE_QUESTION_HOOK_MATCHER: &str = r"^(functions\.)?request_user_input(_async)?$";
pub const PROBE_EVENT_TTL_SECONDS: i64 = 300;
pub const PROBE_PASSIVE_SCAN_EVENT_TTL_SECONDS: i64 = 6 * 60 * 60;
pub const INTERNAL_MEMORY_CONSOLIDATION_SUPPRESSION_REASON: &str = "internal_memory_consolidation";
const PROBE_EVENT_ASSISTANT_MESSAGE_MAX_BYTES: usize = 4096;
const PROBE_BARK_BODY_CHUNK_BYTES: usize = 2_400;

pub fn redact_probe_event_for_output(mut event: ProbeEvent) -> ProbeEvent {
    event.title = event.title.map(|value| redact_output(&value));
    event.message = event.message.map(|value| redact_output(&value));
    event.dedupe_key = event
        .dedupe_key
        .map(|value| redact_probe_event_string(&value));
    event.source = redact_output(&event.source);
    redact_probe_event_json(&mut event.payload);
    event
}

fn redact_probe_event_string(value: &str) -> String {
    let lowered = value.to_ascii_lowercase();
    if lowered.contains("device_key")
        || lowered.contains("secret")
        || lowered.contains("token")
        || lowered.contains("password")
        || lowered.contains("authorization")
        || lowered.contains("cookie")
    {
        "[redacted]".to_string()
    } else {
        redact_output(value)
    }
}

fn redact_probe_event_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let remove_bark_body = map.get("body").is_some()
                && map.contains_key("device_key_configured")
                && (map.contains_key("sent")
                    || map.contains_key("skipped")
                    || map.contains_key("chunk_count")
                    || map.contains_key("request_count"));
            if remove_bark_body {
                map.remove("body");
            }
            for (key, value) in map.iter_mut() {
                let lower = key.to_ascii_lowercase();
                if lower == "device_key_configured" {
                    continue;
                } else if lower.contains("device_key")
                    || lower.contains("secret")
                    || lower.contains("token")
                    || lower.contains("password")
                    || lower.contains("authorization")
                    || lower.contains("cookie")
                    || lower == "request_url"
                {
                    *value = Value::String("[redacted]".to_string());
                } else {
                    redact_probe_event_json(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_probe_event_json(item);
            }
        }
        Value::String(text) => {
            *text = redact_probe_event_text(text);
        }
        _ => {}
    }
}

fn redact_probe_event_text(text: &str) -> String {
    let redacted = redact_output(text);
    extract_proposed_plan_text(&redacted).unwrap_or(redacted)
}

/// The native sidecar executes on the source machine, independently of UI selection.
pub fn bark_test_body() -> &'static str {
    if cfg!(target_os = "macos") {
        "来自本机的测试通知。"
    } else {
        "来自远程机器的测试通知。"
    }
}

#[derive(Debug, Clone)]
pub struct ProbeRuntime {
    config: Config,
    paths: PlatformPaths,
}

impl ProbeRuntime {
    pub fn new(config: Config, paths: PlatformPaths) -> Self {
        Self { config, paths }
    }

    pub async fn status(&self) -> Result<ProbeStatus> {
        let runtime = Self::new(self.config.clone(), self.paths.clone());
        crate::read_tasks::spawn_blocking(move || runtime.status_blocking())
            .await
            .map_err(Into::into)
    }

    fn status_blocking(&self) -> ProbeStatus {
        let resolved = self.resolved_codex_paths();
        let aggregation = aggregate_probe_status(&self.config);
        let hook_status = self.hook_status();
        ProbeStatus {
            provider_notifications: json!([]),
            label: "Probe".to_string(),
            enabled: self.config.probe.enabled,
            available: true,
            flavor: "builtin".to_string(),
            platform: self.paths.kind,
            service_kind: self.paths.service_kind.clone(),
            service_name: self.paths.service_name.clone(),
            binary_path: None,
            hook_status: hook_status.hook_status,
            hook_command: hook_status.hook_command,
            actual_commands: hook_status.actual_commands,
            stale_command_count: hook_status.stale_command_count,
            empty_group_count: hook_status.empty_group_count,
            bark_status: if self.config.probe.notifications.enabled {
                "configured"
            } else {
                "not_configured"
            }
            .to_string(),
            hooks_enabled: self.config.probe.hooks.manage_stop_hook,
            hook_stop_enabled: self.config.probe.hooks.manage_stop_hook,
            bark_enabled: self.config.probe.notifications.enabled,
            bark_server_url: self.config.probe.notifications.server_url.clone(),
            bark_notify_completion: self.config.probe.notifications.notify_completion,
            bark_notify_reply_needed: self.config.probe.notifications.notify_reply_needed,
            bark_notify_recoverable: self.config.probe.notifications.notify_recoverable,
            error_monitor_enabled: self.config.probe.error_monitor.enabled,
            error_monitor_status: if self.config.probe.error_monitor.enabled {
                "enabled"
            } else {
                "disabled"
            }
            .to_string(),
            error_monitor_last_scan_at: None,
            error_monitor_last_error: None,
            error_monitor_incident_count: 0,
            lifecycle_status: self.lifecycle_status_text(),
            doctor_status: self.doctor_status_text(),
            runtime_version: env!("CARGO_PKG_VERSION").to_string(),
            poll_seconds: self.config.probe.poll_seconds,
            recent_event_count: aggregation.recent_event_count,
            running_count: aggregation.running_threads.len(),
            reply_needed_count: aggregation.reply_needed_threads.len(),
            recoverable_count: aggregation.recoverable_threads.len(),
            running_threads: aggregation.running_threads,
            reply_needed_threads: aggregation.reply_needed_threads,
            recoverable_threads: aggregation.recoverable_threads,
            active_sessions: aggregation.active_sessions,
            config_path: self.paths.config_file.clone(),
            codex_hooks_path: Some(resolved.home.join("hooks.json")),
            dsh_sessions_path: Some(
                crate::dsh::DshPaths::default_for_user()
                    .home
                    .join("sessions"),
            ),
            codex_home: resolved.home.clone(),
            configured_codex_home: resolved.configured_codex_home.clone(),
            resolved_codex_home: resolved.home.clone(),
            codex_home_source: resolved.codex_home_source.clone(),
            logs_db_source: resolved.logs_db_source.clone(),
            discovery_warnings: resolved.discovery_warnings.clone(),
            host_label: self.config.codex.host_label.clone(),
        }
    }

    pub fn hook_status(&self) -> ProbeHookStatus {
        let hook_command = self.hook_command();
        let request_user_input_hook_command = self.request_user_input_hook_command();
        let audit = audit_probe_hooks(
            &self.resolved_codex_paths().home.join("hooks.json"),
            &[
                ExpectedProbeHook {
                    event: "Stop",
                    matcher: "*",
                    command: &hook_command,
                    timeout_seconds: None,
                },
                ExpectedProbeHook {
                    event: "PreToolUse",
                    matcher: PROBE_QUESTION_HOOK_MATCHER,
                    command: &request_user_input_hook_command,
                    timeout_seconds: Some(5),
                },
            ],
        );
        let status = if !self.config.probe.hooks.manage_stop_hook {
            "disabled"
        } else if audit.stale_command_count > 0 || audit.empty_group_count > 0 {
            "stale"
        } else if audit.expected_installed {
            "managed"
        } else {
            "missing"
        }
        .to_string();
        ProbeHookStatus {
            hook_status: status,
            hook_command,
            actual_commands: audit.actual_commands,
            stale_command_count: audit.stale_command_count,
            empty_group_count: audit.empty_group_count,
        }
    }

    pub fn build_event(&self, mut input: ProbeEventInput) -> ProbeBuiltEvent {
        let suppression_reason = input.suppression_reason.clone().or_else(|| {
            matches!(input.kind, ProbeEventInputKind::HookStop)
                .then(|| input.last_assistant_message.as_deref())
                .flatten()
                .filter(|message| is_probe_machine_control_payload(message))
                .map(|_| "internal_control_payload".to_string())
        });
        if suppression_reason.is_some() {
            input.transcript_path = None;
            input.last_assistant_message = None;
            input.body_source = None;
            input.body_selection_diagnostics.clear();
        }
        let event_type = probe_event_type(&input);
        let event_kind = probe_event_kind(&input, &event_type);
        let event_thread_id = input.thread_id.clone().or_else(|| input.session_id.clone());
        let thread_component = event_thread_id.as_deref().unwrap_or("unknown-thread");
        let turn_component = input.turn_id.as_deref().unwrap_or("unknown-turn");
        let notify_completion = matches!(input.kind, ProbeEventInputKind::NotifyCompletion);
        let raw_body = input
            .last_assistant_message
            .as_deref()
            .unwrap_or_else(|| default_probe_event_body(&event_type));
        let (default_title, message) = probe_event_text(&event_type);
        let title = input
            .thread_title
            .as_deref()
            .unwrap_or(default_title.as_str())
            .to_string();
        let mut body_source = input
            .body_source
            .as_deref()
            .unwrap_or(if input.last_assistant_message.is_some() {
                "last_assistant_message"
            } else {
                "default"
            })
            .to_string();
        if event_type == "reply_needed" && extract_proposed_plan_text(raw_body).is_some() {
            body_source = "proposed_plan".to_string();
        }
        let redacted_body = redact_output(raw_body);
        let bark_body = probe_event_bark_body(&event_type, &body_source, raw_body, &redacted_body);
        let last_assistant_message = input
            .last_assistant_message
            .as_deref()
            .map(|value| summarize_probe_event_assistant_message(value, &event_type, &body_source));
        let body_summary = last_assistant_message
            .as_ref()
            .and_then(|value| value.get("summary"))
            .and_then(Value::as_str)
            .unwrap_or(&message)
            .to_string();
        let body_length = last_assistant_message
            .as_ref()
            .and_then(|value| value.get("original_length"))
            .and_then(Value::as_u64)
            .unwrap_or(message.len() as u64);
        let body_sha256 = last_assistant_message
            .as_ref()
            .and_then(|value| value.get("sha256"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| hex::encode(Sha256::digest(message.as_bytes())));
        let body_truncated = body_summary.contains("[truncated]");
        let body_hash_component = body_sha256.get(..16).unwrap_or(body_sha256.as_str());
        let dedupe_body_component = probe_event_dedupe_body_component(
            &event_type,
            &body_source,
            &redacted_body,
            body_hash_component,
        );
        let ttl_seconds = input.ttl_seconds.unwrap_or(PROBE_EVENT_TTL_SECONDS);
        let scan_source = input
            .scan_source
            .as_deref()
            .unwrap_or(if notify_completion {
                "notify-completion"
            } else {
                "hook-stop"
            })
            .to_string();
        let dedupe_key = format!(
            "{event_kind}:{}:{}:{}:{}",
            dedupe_component(thread_component),
            dedupe_component(turn_component),
            dedupe_component(&event_type),
            dedupe_component(&dedupe_body_component)
        );
        let beijing_time = Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).expect("valid offset"))
            .format("%Y-%m-%d %H:%M:%S 北京时间")
            .to_string();
        let bark_message = probe_event_bark_message(&event_type, &body_source, &message);
        let bark = probe_event_bark_text(
            &event_type,
            input.thread_title.as_deref(),
            &bark_message,
            Some(&bark_body),
        );
        let bark_chunk_count =
            probe_bark_body_chunks(&bark.body, PROBE_BARK_BODY_CHUNK_BYTES).len();
        let source = input.source_override.clone().unwrap_or_else(|| {
            if notify_completion {
                "agenthub-webd probe notify-completion".to_string()
            } else {
                "agenthub-webd probe hook-stop".to_string()
            }
        });
        let mut payload = json!({
            "raw_kind": input.event_kind(),
            "turn_id": input.turn_id.clone(),
            "call_id": input.call_id.clone(),
            "session_id": input.session_id.clone(),
            "transcript_path": input.transcript_path.clone(),
            "last_assistant_message": last_assistant_message.clone(),
            "thread_title": title.clone(),
            "notification_title": input.thread_title.clone(),
            "thread_id": event_thread_id.clone(),
            "body_summary": body_summary.clone(),
            "body_sha256": body_sha256.clone(),
            "body_length": body_length,
            "body_source": body_source.clone(),
            "body_truncated": body_truncated,
            "dedupe_ttl_seconds": ttl_seconds,
            "scan_source": scan_source.clone(),
            "beijing_time": beijing_time.clone(),
            "reason_label": title.clone(),
            "source": source.clone(),
            "host_label": self.config.codex.host_label.clone(),
            "platform": self.paths.kind,
            "notify_completion": notify_completion,
            "auto_reply": false,
            "hidden_desktop_control": false,
            "event_type": event_type.clone(),
            "bark": {
                "title": bark.title.clone(),
                "body_sha256": body_sha256.clone(),
                "body_length": body_length,
                "body_source": body_source.clone(),
                "body_truncated": body_truncated,
                "dedupe_ttl_seconds": ttl_seconds,
                "scan_source": scan_source.clone(),
                "chunk_count": bark_chunk_count,
                "request_count": 0,
                "status": "pending"
            }
        });
        if let Some(map) = payload.as_object_mut() {
            for (key, value) in &input.body_selection_diagnostics {
                map.insert(key.clone(), value.clone());
            }
            if let Some(bark) = map.get_mut("bark").and_then(Value::as_object_mut) {
                for (key, value) in &input.body_selection_diagnostics {
                    bark.insert(key.clone(), value.clone());
                }
            }
        }
        ProbeBuiltEvent {
            kind: event_kind,
            event_type: event_type.clone(),
            thread_id: event_thread_id,
            turn_id: input.turn_id.clone(),
            title,
            message,
            bark_title: bark.title.clone(),
            bark_body: bark.body.clone(),
            dedupe_namespace: PROBE_EVENT_DEDUPE_NAMESPACE.to_string(),
            dedupe_key,
            ttl_seconds,
            source,
            payload,
            suppression_reason,
        }
    }

    pub fn hook_context_suppression_reason(
        &self,
        cwd: Option<&str>,
        transcript_path: Option<&str>,
    ) -> Option<&'static str> {
        if transcript_path.is_some_and(|path| !path.trim().is_empty()) {
            return None;
        }
        let cwd = cwd.map(str::trim).filter(|value| !value.is_empty())?;
        let cwd = Path::new(cwd);
        if !cwd.is_absolute() {
            return None;
        }
        let memory_root = self.resolved_codex_paths().home.join("memories");
        let canonical_cwd = fs::canonicalize(cwd).ok()?;
        let canonical_memory_root = fs::canonicalize(memory_root).ok()?;
        (canonical_cwd == canonical_memory_root)
            .then_some(INTERNAL_MEMORY_CONSOLIDATION_SUPPRESSION_REASON)
    }

    fn hook_command(&self) -> String {
        format!(
            "{} --config {} probe hook-stop",
            shell_quote(
                &self.paths.daemon_binary().display().to_string(),
                QuoteStyle::Minimal
            ),
            shell_quote(
                &self.paths.config_file.display().to_string(),
                QuoteStyle::Minimal
            )
        )
    }

    fn request_user_input_hook_command(&self) -> String {
        format!(
            "{} --config {} probe hook-request-user-input",
            shell_quote(
                &self.paths.daemon_binary().display().to_string(),
                QuoteStyle::Minimal
            ),
            shell_quote(
                &self.paths.config_file.display().to_string(),
                QuoteStyle::Minimal
            )
        )
    }

    fn resolved_codex_paths(&self) -> ResolvedCodexPaths {
        resolve_codex_paths(&self.config.codex.home)
    }

    fn doctor_status_text(&self) -> String {
        if self.config.probe.enabled {
            "ready"
        } else {
            "disabled"
        }
        .to_string()
    }

    fn lifecycle_status_text(&self) -> String {
        if self.config.probe.enabled {
            "managed"
        } else {
            "disabled"
        }
        .to_string()
    }
}

#[derive(Debug, Clone, Default)]
struct ProbeHookAudit {
    expected_installed: bool,
    actual_commands: Vec<String>,
    stale_command_count: usize,
    empty_group_count: usize,
}

struct ExpectedProbeHook<'a> {
    event: &'a str,
    matcher: &'a str,
    command: &'a str,
    timeout_seconds: Option<u64>,
}

fn audit_probe_hooks(path: &Path, expected_hooks: &[ExpectedProbeHook<'_>]) -> ProbeHookAudit {
    let Ok(text) = fs::read_to_string(path) else {
        return ProbeHookAudit::default();
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return ProbeHookAudit::default();
    };
    let mut audit = ProbeHookAudit::default();
    let mut all_expected_installed = true;
    for expected in expected_hooks {
        let groups = root
            .get("hooks")
            .and_then(|hooks| hooks.get(expected.event))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut valid_count = 0usize;
        for group in groups {
            let hooks = group
                .get("hooks")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if hooks.is_empty() {
                audit.empty_group_count += 1;
            }
            for hook in hooks {
                let Some(command) = hook.get("command").and_then(Value::as_str) else {
                    continue;
                };
                audit.actual_commands.push(command.to_string());
                if command == expected.command {
                    if probe_hook_matches_expected(group, hook, expected) {
                        valid_count += 1;
                    } else {
                        audit.stale_command_count += 1;
                    }
                } else if is_agenthub_managed_hook_command(command) {
                    audit.stale_command_count += 1;
                }
            }
        }
        if valid_count != 1 {
            all_expected_installed = false;
            audit.stale_command_count += valid_count.saturating_sub(1);
        }
    }
    audit.expected_installed = all_expected_installed;
    audit
}

fn probe_hook_matches_expected(
    group: &Value,
    hook: &Value,
    expected: &ExpectedProbeHook<'_>,
) -> bool {
    group.get("matcher").and_then(Value::as_str) == Some(expected.matcher)
        && hook.get("type").and_then(Value::as_str) == Some("command")
        && match expected.timeout_seconds {
            Some(timeout) => hook.get("timeout").and_then(Value::as_u64) == Some(timeout),
            None => hook.get("timeout").is_none(),
        }
        && hook.get("async").is_none()
}

pub fn is_agenthub_managed_hook_command(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    (lowered.contains("probe hook-stop") || lowered.contains("probe hook-request-user-input"))
        && (lowered.contains("agenthubd")
            || lowered.contains("agenthub-webd")
            || lowered.contains("/opt/agenthub")
            || lowered.contains("application support/agenthub")
            || lowered.contains("codex-sentinel"))
}

pub fn remove_agenthub_managed_hooks(root: &mut Value) -> bool {
    let Some(hooks_object) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    let mut empty_events = Vec::new();
    for (event, event_hooks) in hooks_object.iter_mut() {
        let Some(groups) = event_hooks.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            let Some(items) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            let before = items.len();
            items.retain(|item| {
                item.get("command")
                    .and_then(Value::as_str)
                    .is_none_or(|command| !is_agenthub_managed_hook_command(command))
            });
            if items.len() != before {
                changed = true;
            }
        }
        let before_groups = groups.len();
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        });
        if groups.len() != before_groups {
            changed = true;
        }
        if groups.is_empty() {
            empty_events.push(event.clone());
        }
    }
    for event in empty_events {
        hooks_object.remove(&event);
        changed = true;
    }
    changed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeLiveSession {
    pub provider: String,
    pub id: String,
    pub session_key: String,
    pub title: String,
    pub bucket: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeStatus {
    #[serde(default)]
    pub provider_notifications: Value,
    pub label: String,
    pub enabled: bool,
    pub available: bool,
    pub flavor: String,
    pub platform: PlatformKind,
    pub service_kind: String,
    pub service_name: String,
    pub binary_path: Option<PathBuf>,
    pub hook_status: String,
    pub hook_command: String,
    pub actual_commands: Vec<String>,
    pub stale_command_count: usize,
    pub empty_group_count: usize,
    pub bark_status: String,
    pub hooks_enabled: bool,
    pub hook_stop_enabled: bool,
    pub bark_enabled: bool,
    pub bark_server_url: String,
    pub bark_notify_completion: bool,
    pub bark_notify_reply_needed: bool,
    pub bark_notify_recoverable: bool,
    pub error_monitor_enabled: bool,
    pub error_monitor_status: String,
    pub error_monitor_last_scan_at: Option<i64>,
    pub error_monitor_last_error: Option<String>,
    pub error_monitor_incident_count: u64,
    pub lifecycle_status: String,
    pub doctor_status: String,
    pub runtime_version: String,
    pub poll_seconds: u64,
    pub recent_event_count: usize,
    pub running_count: usize,
    pub reply_needed_count: usize,
    pub recoverable_count: usize,
    pub running_threads: Vec<ThreadSummary>,
    pub reply_needed_threads: Vec<ThreadSummary>,
    pub recoverable_threads: Vec<ThreadSummary>,
    #[serde(default)]
    pub active_sessions: Vec<ProbeLiveSession>,
    pub config_path: PathBuf,
    #[serde(default)]
    pub codex_hooks_path: Option<PathBuf>,
    #[serde(default)]
    pub dsh_sessions_path: Option<PathBuf>,
    pub codex_home: PathBuf,
    pub configured_codex_home: Option<String>,
    pub resolved_codex_home: PathBuf,
    pub codex_home_source: String,
    pub logs_db_source: String,
    pub discovery_warnings: Vec<String>,
    pub host_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeHookStatus {
    pub hook_status: String,
    pub hook_command: String,
    pub actual_commands: Vec<String>,
    pub stale_command_count: usize,
    pub empty_group_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeEventInput {
    kind: ProbeEventInputKind,
    thread_id: Option<String>,
    turn_id: Option<String>,
    session_id: Option<String>,
    transcript_path: Option<String>,
    thread_title: Option<String>,
    last_assistant_message: Option<String>,
    body_source: Option<String>,
    body_selection_diagnostics: BTreeMap<String, Value>,
    scan_source: Option<String>,
    ttl_seconds: Option<i64>,
    source_override: Option<String>,
    call_id: Option<String>,
    hook_kind: String,
    #[serde(default)]
    suppression_reason: Option<String>,
}

impl ProbeEventInput {
    pub fn hook_stop(thread_id: Option<&str>, turn_id: Option<&str>, kind: &str) -> Self {
        Self::hook_stop_with_context(thread_id, turn_id, None, None, None, kind)
    }

    pub fn hook_stop_with_context(
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        session_id: Option<&str>,
        transcript_path: Option<&str>,
        last_assistant_message: Option<&str>,
        kind: &str,
    ) -> Self {
        Self {
            kind: ProbeEventInputKind::HookStop,
            thread_id: thread_id.map(ToString::to_string),
            turn_id: turn_id.map(ToString::to_string),
            session_id: session_id.map(ToString::to_string),
            transcript_path: transcript_path.map(ToString::to_string),
            thread_title: None,
            last_assistant_message: last_assistant_message.map(ToString::to_string),
            body_source: last_assistant_message.map(|_| "last_assistant_message".to_string()),
            body_selection_diagnostics: BTreeMap::new(),
            scan_source: None,
            ttl_seconds: None,
            source_override: None,
            call_id: None,
            hook_kind: if kind.trim().is_empty() {
                "hook-stop".to_string()
            } else {
                kind.trim().to_string()
            },
            suppression_reason: None,
        }
    }

    pub fn notify_completion(thread_id: Option<&str>, turn_id: Option<&str>) -> Self {
        Self::notify_completion_with_context(thread_id, turn_id, None, None, None, None)
    }

    pub fn notify_completion_with_context(
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        session_id: Option<&str>,
        transcript_path: Option<&str>,
        last_assistant_message: Option<&str>,
        body_source: Option<&str>,
    ) -> Self {
        Self {
            kind: ProbeEventInputKind::NotifyCompletion,
            thread_id: thread_id.map(ToString::to_string),
            turn_id: turn_id.map(ToString::to_string),
            session_id: session_id.map(ToString::to_string),
            transcript_path: transcript_path.map(ToString::to_string),
            thread_title: None,
            last_assistant_message: last_assistant_message.map(ToString::to_string),
            body_source: body_source
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .or_else(|| last_assistant_message.map(|_| "last_assistant_message".to_string())),
            body_selection_diagnostics: BTreeMap::new(),
            scan_source: None,
            ttl_seconds: None,
            source_override: None,
            call_id: None,
            hook_kind: "completion".to_string(),
            suppression_reason: None,
        }
    }

    pub fn with_thread_title(mut self, thread_title: Option<&str>) -> Self {
        self.thread_title = thread_title
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        self
    }

    pub fn with_body_source(mut self, body_source: Option<&str>) -> Self {
        self.body_source = body_source
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        self
    }

    pub fn with_body_selection_diagnostics(mut self, diagnostics: BTreeMap<String, Value>) -> Self {
        self.body_selection_diagnostics = diagnostics;
        self
    }

    pub fn with_passive_scan_source(mut self) -> Self {
        self.scan_source = Some("passive-scan".to_string());
        self.ttl_seconds = Some(PROBE_PASSIVE_SCAN_EVENT_TTL_SECONDS);
        self.source_override = Some("agenthub-webd probe passive-scan".to_string());
        self
    }

    pub fn with_pre_tool_use_scan_source(mut self) -> Self {
        self.scan_source = Some("pre-tool-use-hook".to_string());
        self.source_override = Some("agenthub-webd probe hook-request-user-input".to_string());
        self
    }

    pub fn with_error_monitor_source(mut self) -> Self {
        self.scan_source = Some("logs_2-turn-error".to_string());
        self.source_override = Some("agenthub-webd probe monitor-errors".to_string());
        self
    }

    pub fn with_call_id(mut self, call_id: Option<&str>) -> Self {
        self.call_id = call_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        self
    }

    pub fn with_suppression_reason(mut self, reason: Option<&str>) -> Self {
        self.suppression_reason = reason
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        self
    }

    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn transcript_path(&self) -> Option<&str> {
        self.transcript_path.as_deref()
    }

    pub fn last_assistant_message(&self) -> Option<&str> {
        self.last_assistant_message.as_deref()
    }

    pub fn body_source(&self) -> Option<&str> {
        self.body_source.as_deref()
    }

    pub fn thread_title(&self) -> Option<&str> {
        self.thread_title.as_deref()
    }

    fn event_kind(&self) -> &str {
        match self.kind {
            ProbeEventInputKind::HookStop => self.hook_kind.as_str(),
            ProbeEventInputKind::NotifyCompletion => "completion",
        }
    }
}

fn hook_stop_event_type(input: &ProbeEventInput, raw_kind: &str) -> String {
    let normalized_kind = raw_kind.trim().to_ascii_lowercase();
    if matches!(normalized_kind.as_str(), "completion" | "notify-completion") {
        return "completion".to_string();
    }
    if matches!(normalized_kind.as_str(), "turn-error" | "turn_error") {
        return "recoverable".to_string();
    }
    if matches!(normalized_kind.as_str(), "reply-needed" | "reply_needed") {
        return "reply_needed".to_string();
    }
    if normalized_kind == "recoverable" {
        return "recoverable".to_string();
    }
    if input.body_source.as_deref() == Some("proposed_plan") {
        return "reply_needed".to_string();
    }
    if let Some(message) = input.last_assistant_message.as_deref() {
        if let Some((_, _, event_type)) = classify_hook_stop_event(message) {
            return event_type.to_string();
        }
    }
    "hook_stop".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ProbeEventInputKind {
    HookStop,
    NotifyCompletion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeBuiltEvent {
    pub kind: String,
    pub event_type: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub title: String,
    pub message: String,
    pub bark_title: String,
    #[serde(skip)]
    pub bark_body: String,
    pub dedupe_namespace: String,
    pub dedupe_key: String,
    pub ttl_seconds: i64,
    pub source: String,
    pub payload: Value,
    #[serde(skip)]
    pub suppression_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeEventOutcome {
    pub ok: bool,
    pub recorded: bool,
    pub duplicate: bool,
    pub kind: String,
    pub dedupe_namespace: String,
    pub dedupe_key: String,
    pub thread_id: Option<String>,
}

impl ProbeEventOutcome {
    pub fn from_claim(event: &ProbeBuiltEvent, recorded: bool) -> Self {
        Self {
            ok: true,
            recorded,
            duplicate: !recorded,
            kind: event.kind.clone(),
            dedupe_namespace: event.dedupe_namespace.clone(),
            dedupe_key: event.dedupe_key.clone(),
            thread_id: event.thread_id.clone(),
        }
    }
}

fn probe_event_type(input: &ProbeEventInput) -> String {
    match input.kind {
        ProbeEventInputKind::NotifyCompletion => "completion".to_string(),
        ProbeEventInputKind::HookStop => hook_stop_event_type(input, input.hook_kind.as_str()),
    }
}

fn probe_event_kind(input: &ProbeEventInput, event_type: &str) -> String {
    match input.kind {
        ProbeEventInputKind::NotifyCompletion => "completion".to_string(),
        ProbeEventInputKind::HookStop => match event_type {
            "completion" => "completion".to_string(),
            "reply_needed" => "reply-needed".to_string(),
            "recoverable" => "recoverable".to_string(),
            _ => input.event_kind().to_string(),
        },
    }
}

fn classify_hook_stop_event(body: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let trimmed = body.trim();
    let normalized = trimmed.to_ascii_lowercase();
    if normalized.is_empty()
        || is_probe_stop_hook_system_message(trimmed)
        || is_probe_machine_control_payload(trimmed)
    {
        return None;
    }
    if contains_probe_proposed_plan(trimmed)
        || normalized.contains("等待用户回复")
        || normalized.contains("等待用户选择")
        || normalized.contains("request_user_input")
    {
        return Some(("reply_needed", "需要回复", "reply_needed"));
    }
    if is_probe_terminal_stop_error(trimmed) {
        return Some(("recoverable", "线程异常停止", "recoverable"));
    }
    Some(("completion", "任务完成", "completion"))
}

fn contains_probe_proposed_plan(text: &str) -> bool {
    text.contains("<proposed_plan>") && text.contains("</proposed_plan>")
}

fn is_probe_terminal_stop_error(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    lower.starts_with("turn error:")
        || lower.starts_with("selected model is at capacity")
        || lower.starts_with("stream disconnected")
        || lower.starts_with("response_stream_disconnected")
        || lower.starts_with("error decoding response body")
        || lower.starts_with("exceeded retry limit")
        || lower.starts_with("429 too many requests")
        || lower.starts_with("this content was flagged for possible cybersecurity risk")
        || lower.starts_with("codex supervisor reported terminal thread status")
        || lower.starts_with("codex supervisor reported terminal turn status")
        || lower.starts_with("codex supervisor reported interrupted turn status")
        || lower.starts_with("background turn supervisor failed")
}

fn is_probe_stop_hook_system_message(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("Codex Sentinel Lite 检测到")
        || trimmed.starts_with("AgentHub Probe 检测到")
        || trimmed.starts_with("AgentHub Probe 检测到")
}

pub fn is_probe_machine_control_payload(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return false;
    }

    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };

    object
        .keys()
        .all(|key| matches!(key.as_str(), "suggestions" | "exclude"))
        && object
            .keys()
            .any(|key| matches!(key.as_str(), "suggestions" | "exclude"))
}

fn probe_event_text(event_type: &str) -> (String, String) {
    if event_type == "completion" {
        return ("任务完成".to_string(), "Codex 任务已正常完成。".to_string());
    }
    match event_type {
        "reply_needed" => (
            "需要回复".to_string(),
            "Codex 正在等待用户回复或选择后继续。".to_string(),
        ),
        "recoverable" => (
            "线程异常停止".to_string(),
            "Codex 任务可恢复，需要查看并手动处理。".to_string(),
        ),
        _ => (
            "Codex Stop Hook".to_string(),
            "Stop Hook 事件已记录。".to_string(),
        ),
    }
}

fn default_probe_event_body(event_type: &str) -> &'static str {
    match event_type {
        "completion" => "Codex 任务已正常完成。",
        "reply_needed" => "Codex 正在等待用户回复或选择后继续。",
        "recoverable" => "Codex 任务可恢复，需要查看并手动处理。",
        _ => "Stop Hook 事件已记录。",
    }
}

fn probe_event_bark_text(
    event_type: &str,
    title: Option<&str>,
    message: &str,
    detail: Option<&str>,
) -> ProbeBarkText {
    let title = fallback_bark_title(title);
    let body = detail.unwrap_or(message).trim().to_string();
    match event_type {
        "completion" => ProbeBarkText {
            title: format!("线程正常完成：{}", title),
            body,
        },
        "reply_needed" => ProbeBarkText {
            title: format!("等待回复：{}", title),
            body,
        },
        "recoverable" => ProbeBarkText {
            title: format!("线程异常停止：{}", title),
            body,
        },
        _ => ProbeBarkText {
            title: format!("探针事件：{}", title),
            body,
        },
    }
}

fn probe_event_bark_message(event_type: &str, body_source: &str, fallback: &str) -> String {
    let _ = (event_type, body_source);
    fallback.to_string()
}

fn fallback_bark_title(title: Option<&str>) -> &str {
    title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未命名线程")
}

fn probe_event_bark_body(
    event_type: &str,
    body_source: &str,
    raw_body: &str,
    redacted_body: &str,
) -> String {
    let body = match event_type {
        "reply_needed" if body_source == "proposed_plan" => extract_probe_plan_body(redacted_body)
            .or_else(|| extract_probe_plan_body(raw_body))
            .unwrap_or_else(|| redacted_body.to_string()),
        "reply_needed" if body_source == "request_user_input" => {
            extract_request_user_input_body(redacted_body)
                .unwrap_or_else(|| redacted_body.to_string())
        }
        "completion" => extract_completion_final_reply_body(redacted_body)
            .unwrap_or_else(|| redacted_body.to_string()),
        _ => redacted_body.to_string(),
    };
    sanitize_probe_bark_body(&body)
}

fn extract_probe_plan_body(body: &str) -> Option<String> {
    extract_proposed_plan_text(body).or_else(|| {
        body.split_once("Plan 摘要:")
            .map(|(_, plan)| plan.trim().to_string())
            .filter(|plan| !plan.is_empty())
    })
}

fn extract_request_user_input_body(body: &str) -> Option<String> {
    let mut kept = Vec::new();
    let mut in_questions = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("问题 ")
            || trimmed.starts_with("选项 ")
            || trimmed.starts_with("标题：")
            || trimmed.starts_with("标题:")
            || trimmed.starts_with("说明：")
            || trimmed.starts_with("说明:")
        {
            in_questions = true;
            kept.push(trimmed.to_string());
        } else if in_questions && trimmed.is_empty() {
            kept.push(String::new());
        }
    }
    let text = kept.join("\n").trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn extract_completion_final_reply_body(body: &str) -> Option<String> {
    for marker in ["最后反馈：", "最后反馈:", "最终反馈：", "最终反馈:"] {
        if let Some((_, reply)) = body.split_once(marker) {
            let cleaned = sanitize_probe_bark_body(reply);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn sanitize_probe_bark_body(body: &str) -> String {
    let cleaned = strip_memory_citation_blocks(body)
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return Some(String::new());
            }
            let lower = trimmed.to_ascii_lowercase();
            if trimmed == "[redacted sensitive line]"
                || is_sensitive_output_line(trimmed)
                || lower.starts_with("thread_id:")
                || lower.starts_with("turn_id:")
                || lower.starts_with("time:")
                || lower.starts_with("url:")
                || trimmed.starts_with("线程 ID：")
                || trimmed.starts_with("线程ID：")
                || trimmed.starts_with("Turn ID：")
                || trimmed.starts_with("Call ID：")
                || trimmed.starts_with("状态说明:")
                || trimmed.starts_with("状态说明：")
                || trimmed.starts_with("待回复内容:")
                || trimmed.starts_with("待回复内容：")
                || trimmed.starts_with("待选择内容:")
                || trimmed.starts_with("待选择内容：")
                || trimmed.starts_with("等待用户回复")
                || trimmed.starts_with("等待用户选择")
                || trimmed.starts_with("最后反馈:")
                || trimmed.starts_with("最后反馈：")
                || trimmed.contains("<proposed_plan>")
                || trimmed.contains("</proposed_plan>")
            {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut normalized = cleaned;
    while normalized.contains("\n\n\n") {
        normalized = normalized.replace("\n\n\n", "\n\n");
    }
    normalized.trim().to_string()
}

fn strip_memory_citation_blocks(body: &str) -> String {
    let mut inside = false;
    let mut fenced = false;
    let mut kept = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
        }
        if !fenced && trimmed.starts_with("<oai-mem-citation>") {
            inside = true;
        }
        if !inside {
            kept.push(line);
        }
        if inside && !fenced && trimmed.ends_with("</oai-mem-citation>") {
            inside = false;
        }
    }
    kept.join("\n")
}

fn probe_event_dedupe_body_component(
    event_type: &str,
    body_source: &str,
    body: &str,
    fallback_hash_component: &str,
) -> String {
    let stable_hash_component = if event_type == "reply_needed" && body_source == "proposed_plan" {
        stable_proposed_plan_dedupe_hash(body).unwrap_or(fallback_hash_component.to_string())
    } else {
        fallback_hash_component.to_string()
    };
    if event_type == "reply_needed" {
        if let Some(turn_id) = prefixed_body_value(body, "turn_id:") {
            return format!("turn:{turn_id}:body:{stable_hash_component}");
        }
        if let Some(call_id) = prefixed_body_value(body, "call_id:") {
            return format!("call:{call_id}:body:{stable_hash_component}");
        }
        if let Some(call_id) = prefixed_body_value(body, "Call ID：") {
            return format!("call:{call_id}:body:{stable_hash_component}");
        }
        if let Some(turn_id) = prefixed_body_value(body, "Turn ID：") {
            return format!("turn:{turn_id}:body:{stable_hash_component}");
        }
    }
    stable_hash_component
}

fn stable_proposed_plan_dedupe_hash(body: &str) -> Option<String> {
    let plan = extract_probe_plan_body(body)?;
    let digest = hex::encode(Sha256::digest(plan.trim().as_bytes()));
    Some(digest.get(..16).unwrap_or(digest.as_str()).to_string())
}

fn prefixed_body_value(body: &str, prefix: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.trim().strip_prefix(prefix).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn summarize_probe_event_assistant_message(
    value: &str,
    classification: &str,
    body_source: &str,
) -> Value {
    let redacted = redact_output(value);
    let summary_source = match classification {
        "reply_needed" if body_source == "proposed_plan" => extract_probe_plan_body(&redacted)
            .map(|plan| sanitize_probe_bark_body(&plan))
            .unwrap_or_else(|| sanitize_probe_bark_body(&redacted)),
        "reply_needed" if body_source == "request_user_input" => {
            extract_request_user_input_body(&redacted)
                .map(|input| sanitize_probe_bark_body(&input))
                .unwrap_or_else(|| sanitize_probe_bark_body(&redacted))
        }
        "completion" => extract_completion_final_reply_body(&redacted)
            .unwrap_or_else(|| sanitize_probe_bark_body(&redacted)),
        _ => sanitize_probe_bark_body(&redacted),
    };
    let summary =
        truncate_utf8_with_marker(&summary_source, PROBE_EVENT_ASSISTANT_MESSAGE_MAX_BYTES);
    json!({
        "summary": summary,
        "original_length": value.len(),
        "redacted_length": summary_source.len(),
        "sha256": hex::encode(Sha256::digest(value.as_bytes())),
        "classification": classification,
    })
}

#[derive(Debug)]
struct ProbeBarkText {
    title: String,
    body: String,
}

fn truncate_utf8_with_marker(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let marker = "\n[truncated]";
    let limit = max_bytes.saturating_sub(marker.len());
    let mut end = limit.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = value[..end].to_string();
    truncated.push_str(marker);
    truncated
}

fn utf8_chunks(value: &str, max_bytes: usize) -> Vec<String> {
    if value.is_empty() || max_bytes == 0 {
        return vec![String::new()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < value.len() {
        let mut end = (start + max_bytes).min(value.len());
        while end > start && !value.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = value[start..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| start + offset)
                .unwrap_or(value.len());
        }
        chunks.push(value[start..end].to_string());
        start = end;
    }
    chunks
}

fn probe_bark_body_chunks(value: &str, max_bytes: usize) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || max_bytes == 0 {
        return vec![String::new()];
    }
    let raw_chunks = utf8_chunks(trimmed, max_bytes);
    let chunk_count = raw_chunks.len();
    if chunk_count <= 1 {
        return raw_chunks;
    }
    raw_chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| format!("第 {}/{} 段\n\n{chunk}", index + 1, chunk_count))
        .collect()
}

fn dedupe_component(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.is_empty() {
        "unknown".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn probe_hook_context_suppresses_exact_canonical_memory_roots() {
        for layout in ["Users/example/.codex", "root/.codex", "custom/CODEX_HOME"] {
            let root = unique_temp_dir("agenthub-probe-memory-hook-context");
            let codex_home = root.join(layout);
            let memory_root = codex_home.join("memories");
            let normalized_memory_root = memory_root.join("nested").join("..");
            fs::create_dir_all(codex_home.join("sessions")).unwrap();
            fs::create_dir_all(memory_root.join("nested")).unwrap();
            let mut config = Config::default();
            config.codex.home = codex_home;
            let runtime = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux));

            assert_eq!(
                runtime.hook_context_suppression_reason(normalized_memory_root.to_str(), None),
                Some(INTERNAL_MEMORY_CONSOLIDATION_SUPPRESSION_REASON)
            );
            assert_eq!(
                runtime.hook_context_suppression_reason(memory_root.to_str(), Some("   ")),
                Some(INTERNAL_MEMORY_CONSOLIDATION_SUPPRESSION_REASON)
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn probe_hook_context_identity_is_exact_and_fails_open() {
        let root = unique_temp_dir("agenthub-probe-memory-hook-counterexamples");
        let codex_home = root.join(".codex");
        let memory_root = codex_home.join("memories");
        let memory_child = memory_root.join("child");
        let workspace = root.join("workspace");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::create_dir_all(&memory_child).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        let mut config = Config::default();
        config.codex.home = codex_home;
        let runtime = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux));

        for (cwd, transcript_path) in [
            (None, None),
            (Some(""), None),
            (Some("memories"), None),
            (workspace.to_str(), None),
            (memory_child.to_str(), None),
            (root.join("missing").to_str(), None),
            (memory_root.to_str(), Some("/tmp/rollout.jsonl")),
        ] {
            assert_eq!(
                runtime.hook_context_suppression_reason(cwd, transcript_path),
                None,
                "cwd={cwd:?} transcript_path={transcript_path:?}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn probe_memory_suppression_discards_body_even_if_a_caller_supplies_one() {
        let config = Config::default();
        let runtime = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux));
        let memory_body = "MEMORY_BODY_MUST_NOT_REACH_SUMMARY_HASH_OR_BARK";

        let event = runtime.build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-memory"),
                Some("turn-memory"),
                Some("thread-memory"),
                None,
                Some(memory_body),
                "hook-stop",
            )
            .with_suppression_reason(Some(INTERNAL_MEMORY_CONSOLIDATION_SUPPRESSION_REASON)),
        );

        assert_eq!(
            event.suppression_reason.as_deref(),
            Some(INTERNAL_MEMORY_CONSOLIDATION_SUPPRESSION_REASON)
        );
        assert!(event.payload["last_assistant_message"].is_null());
        assert!(!event.payload.to_string().contains(memory_body));
        assert!(!event.bark_body.contains(memory_body));
    }

    #[test]
    fn hook_status_reports_stale_when_hooks_json_contains_old_agenthub_command() {
        let root = unique_temp_dir("agenthub-probe-hook-status-stale");
        let codex_home = root.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        fs::write(
            codex_home.join("hooks.json"),
            json!({
                "hooks": {
                    "Stop": [
                        {"hooks": []},
                        {
                            "matcher": "*",
                            "hooks": [{
                                "type": "command",
                                "command": "/opt/agenthub/bin/agenthubd --config /opt/agenthub/config.toml probe hook-stop"
                            }]
                        }
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        let mut config = Config::default();
        config.codex.home = codex_home;
        config.probe.hooks.manage_stop_hook = true;

        let status =
            ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux)).hook_status();

        assert_eq!(status.hook_status, "stale");
        assert_eq!(status.stale_command_count, 1);
        assert_eq!(status.empty_group_count, 1);
        assert!(status
            .actual_commands
            .iter()
            .any(|command| command.contains("agenthubd")));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn status_includes_stop_hook_audit_fields_for_ui() {
        let root = unique_temp_dir("agenthub-probe-status-hook-audit");
        let codex_home = root.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        fs::write(
            codex_home.join("hooks.json"),
            json!({
                "hooks": {
                    "Stop": [
                        {"hooks": []},
                        {
                            "matcher": "*",
                            "hooks": [{
                                "type": "command",
                                "command": "/opt/agenthub/bin/agenthubd --config /opt/agenthub/config.toml probe hook-stop"
                            }]
                        }
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        let mut config = Config::default();
        config.codex.home = codex_home;
        config.probe.hooks.manage_stop_hook = true;

        let status = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux))
            .status()
            .await
            .unwrap();

        assert_eq!(status.hook_status, "stale");
        assert_eq!(status.stale_command_count, 1);
        assert_eq!(status.empty_group_count, 1);
        assert_eq!(status.actual_commands.len(), 1);
        assert!(status.hook_command.contains("agenthub-webd"));
        assert!(status.actual_commands[0].contains("agenthubd"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn probe_hook_status_requires_stop_and_request_user_input_hooks() {
        let root = unique_temp_dir("agenthub-probe-hook-status-pair");
        let codex_home = root.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        let mut config = Config::default();
        config.codex.home = codex_home.clone();
        config.probe.hooks.manage_stop_hook = true;
        let runtime = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux));
        let stop_command = runtime.hook_status().hook_command;
        let request_command =
            stop_command.replace("probe hook-stop", "probe hook-request-user-input");

        fs::write(
            codex_home.join("hooks.json"),
            json!({
                "hooks": {
                    "Stop": [{
                        "matcher": "*",
                        "hooks": [{"type": "command", "command": stop_command}]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(runtime.hook_status().hook_status, "missing");

        fs::write(
            codex_home.join("hooks.json"),
            json!({
                "hooks": {
                    "Stop": [{
                        "matcher": "*",
                        "hooks": [{"type": "command", "command": stop_command}]
                    }],
                    "PreToolUse": [{
                        "matcher": PROBE_QUESTION_HOOK_MATCHER,
                        "hooks": [{"type": "command", "command": request_command, "timeout": 5}]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(runtime.hook_status().hook_status, "managed");

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn status_aggregates_running_threads_from_codex_home() {
        let root = unique_temp_dir("agenthub-probe-status-running");
        let codex_home = root.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        let rollout = codex_home.join("running-rollout.jsonl");
        fs::write(
            &rollout,
            [
                json!({"type":"turn_started","turn_id":"turn-live"}).to_string(),
                json!({"type":"response_item","turn_id":"turn-live","payload":{"type":"message","role":"assistant","content":[{"text":"working"}]}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads(
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL,
                preview TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
             VALUES('running-thread', ?1, 1, 1, 'codex', '/tmp', '运行线程', '')",
            [rollout.display().to_string()],
        )
        .unwrap();
        drop(conn);
        let mut config = Config::default();
        config.codex.home = codex_home;
        config.probe.recent_limit = 20;
        let status = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux))
            .status()
            .await
            .unwrap();

        assert_eq!(status.running_count, 1);
        assert_eq!(status.running_threads.len(), 1);
        assert_eq!(status.running_threads[0].id, "running-thread");
        assert_eq!(
            status.running_threads[0].active_turn_id.as_deref(),
            Some("turn-live")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn status_keeps_jobs_separate_from_native_threads_and_counts_events() {
        let root = unique_temp_dir("agenthub-probe-status-panel");
        let codex_home = root.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        let panel_db_path = root.join("agenthub.sqlite");
        let db = crate::db::PanelDb::open_with_secret_box(
            &panel_db_path,
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.record_probe_event(crate::db::NewProbeEvent {
            kind: "hook-stop",
            thread_id: Some("job-thread"),
            title: Some("Probe event"),
            message: Some("event recorded"),
            dedupe_key: Some("hook-stop:job-thread"),
            source: "test",
            payload: json!({"event_type":"reply_needed"}),
        })
        .unwrap();
        db.create_job("job-live", "codex.run", "继续运行").unwrap();
        drop(db);
        let mut config = Config::default();
        config.codex.home = codex_home;
        config.paths.db_path = panel_db_path;
        config.probe.recent_limit = 20;
        let status = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux))
            .status()
            .await
            .unwrap();

        assert_eq!(status.recent_event_count, 1);
        assert_eq!(status.running_count, 0);
        assert!(status.running_threads.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn status_aggregates_reply_needed_and_recoverable_threads() {
        let root = unique_temp_dir("agenthub-probe-status-buckets");
        let codex_home = root.join(".codex");
        fs::create_dir_all(&codex_home).unwrap();
        let reply_rollout = codex_home.join("reply-rollout.jsonl");
        fs::write(
            &reply_rollout,
            [
                json!({"type":"item_completed","turn_id":"turn-plan","item":{"id":"plan-item","type":"Plan"}}).to_string(),
                json!({"type":"response_item","turn_id":"turn-plan","payload":{"type":"message","role":"assistant","content":[{"text":"<proposed_plan>\n# 计划\n- 等待确认\n</proposed_plan>"}]}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let recoverable_rollout = codex_home.join("recoverable-rollout.jsonl");
        fs::write(
            &recoverable_rollout,
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-crash","status":"interrupted","last_agent_message":null}}).to_string(),
        )
        .unwrap();
        let conn = Connection::open(codex_home.join("state_5.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads(
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                source TEXT NOT NULL,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL,
                preview TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
             VALUES('reply-thread', ?1, ?2, ?2, 'codex', '/tmp', '待确认', '')",
            params![reply_rollout.display().to_string(), now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads(id, rollout_path, created_at, updated_at, source, cwd, title, preview)
             VALUES('recoverable-thread', ?1, ?2, ?2, 'codex', '/tmp', '可恢复', '')",
            params![recoverable_rollout.display().to_string(), now - 30],
        )
        .unwrap();
        drop(conn);
        let mut config = Config::default();
        config.codex.home = codex_home;
        config.probe.recent_limit = 20;
        let status = ProbeRuntime::new(config, PlatformPaths::for_kind(PlatformKind::Linux))
            .status()
            .await
            .unwrap();

        assert_eq!(status.reply_needed_count, 1);
        assert_eq!(status.reply_needed_threads.len(), 1);
        assert_eq!(status.reply_needed_threads[0].id, "reply-thread");
        assert_eq!(status.recoverable_count, 1);
        assert_eq!(status.recoverable_threads.len(), 1);
        assert_eq!(status.recoverable_threads[0].id, "recoverable-thread");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn build_event_redacts_and_truncates_last_assistant_message_and_names_status_events() {
        let mut config = Config::default();
        config.codex.host_label = "probe-test".to_string();
        let runtime = ProbeRuntime::new(config, PlatformPaths::current());
        let message = format!(
            "first line\nTOKEN=secret-token\n{}UNIQUE_FULL_BARK_TAIL",
            "完成".repeat(3000)
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-a"),
            Some("turn-a"),
            Some("session-a"),
            Some("/tmp/transcript.jsonl"),
            Some(&message),
            "reply-needed",
        ));

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.event_type, "reply_needed");
        assert_eq!(event.title, "需要回复");
        assert_eq!(event.message, "Codex 正在等待用户回复或选择后继续。");
        assert_eq!(event.bark_title, "等待回复：未命名线程");
        assert!(!event.bark_body.contains("线程 ID："));
        assert!(!event.bark_body.contains("thread_id:"));
        assert!(!event.bark_body.contains("thread-a"));
        assert!(!event.bark_body.contains("状态说明："));
        assert!(!event.bark_body.contains("待回复内容："));
        assert!(!event.bark_body.contains("[redacted sensitive line]"));
        assert!(event.bark_body.contains("完成完成完成"));
        assert!(event.bark_body.contains("UNIQUE_FULL_BARK_TAIL"));
        assert!(!event.bark_body.contains("[truncated]"));
        assert!(event.payload["event_type"].as_str().unwrap() == "reply_needed");
        assert!(event.payload["bark"]["title"]
            .as_str()
            .unwrap()
            .contains("等待回复"));
        assert!(event.payload["bark"].get("body").is_none());
        assert!(event.payload["bark_body"].is_null());
        assert_eq!(
            event.payload["bark"]["body_sha256"],
            event.payload["body_sha256"]
        );
        assert_eq!(
            event.payload["bark"]["body_length"],
            event.payload["body_length"]
        );
        assert_eq!(
            event.payload["bark"]["body_source"],
            "last_assistant_message"
        );
        assert!(event.payload["bark"].get("device_key").is_none());
        assert!(event.payload["bark"].get("sound").is_none());
        assert!(event.payload["bark"].get("group").is_none());
        assert!(event.payload["bark"].get("url").is_none());
        assert_eq!(event.payload["thread_title"], "需要回复");
        assert_eq!(event.payload["thread_id"], "thread-a");
        assert_eq!(event.payload["turn_id"], "turn-a");
        assert_eq!(event.payload["reason_label"], "需要回复");
        assert_eq!(
            event.payload["body_summary"].as_str().unwrap(),
            event.payload["last_assistant_message"]["summary"]
                .as_str()
                .unwrap()
        );
        assert_eq!(event.payload["body_length"], message.len() as u64);
        assert_eq!(
            event.payload["body_sha256"],
            event.payload["last_assistant_message"]["sha256"]
        );
        assert!(event.payload["beijing_time"]
            .as_str()
            .unwrap()
            .ends_with("北京时间"));
        let stored = &event.payload["last_assistant_message"];
        assert!(!stored["summary"]
            .as_str()
            .unwrap()
            .contains("[redacted sensitive line]"));
        assert_eq!(stored["classification"], "reply_needed");
        assert_eq!(
            stored["original_length"].as_u64().unwrap(),
            message.len() as u64
        );
        assert!(
            stored["redacted_length"].as_u64().unwrap()
                >= stored["summary"].as_str().unwrap().len() as u64
        );
        assert!(stored["sha256"].as_str().unwrap().len() >= 64);
        assert!(!stored.to_string().contains("secret-token"));
        assert!(stored["summary"].as_str().unwrap().contains("[truncated]"));
        assert!(
            stored["summary"].as_str().unwrap().len() <= PROBE_EVENT_ASSISTANT_MESSAGE_MAX_BYTES
        );
        let serialized_event = serde_json::to_string(&event).unwrap();
        assert!(!serialized_event.contains("bark_body"));
        assert!(!serialized_event.contains("UNIQUE_FULL_BARK_TAIL"));

        let recoverable = runtime.build_event(ProbeEventInput::hook_stop(
            Some("thread-a"),
            Some("turn-b"),
            "recoverable",
        ));
        assert_eq!(recoverable.event_type, "recoverable");
        assert_eq!(recoverable.title, "线程异常停止");
        assert_eq!(
            recoverable.message,
            "Codex 任务可恢复，需要查看并手动处理。"
        );
        assert_eq!(recoverable.bark_title, "线程异常停止：未命名线程");
        assert_eq!(
            recoverable.bark_body,
            "Codex 任务可恢复，需要查看并手动处理。"
        );
        assert!(!recoverable.bark_body.contains("thread-a"));
        assert!(!recoverable.bark_body.contains("原因："));
        assert!(!recoverable.bark_body.contains("线程标题："));
    }

    #[test]
    fn build_event_formats_completion_bark_message_from_structural_event() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let completion = runtime.build_event(ProbeEventInput::notify_completion(
            Some("thread-a"),
            Some("turn-a"),
        ));

        assert_eq!(completion.kind, "completion");
        assert_eq!(completion.event_type, "completion");
        assert_eq!(completion.message, "Codex 任务已正常完成。");
        assert_eq!(completion.bark_title, "线程正常完成：未命名线程");
        assert_eq!(completion.bark_body, "Codex 任务已正常完成。");
        assert!(!completion.bark_body.contains("thread-a"));
        assert_eq!(completion.payload["event_type"], "completion");
        assert_eq!(completion.payload["bark"]["title"], completion.bark_title);
        assert!(completion.payload["bark"].get("body").is_none());
        assert!(completion.payload["bark"].get("device_key").is_none());
    }

    #[test]
    fn build_event_bark_text_matches_codex_sentinel_lite_formats() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let completion = runtime.build_event(
            ProbeEventInput::notify_completion_with_context(
                Some("thread-complete"),
                Some("turn-complete"),
                Some("thread-complete"),
                None,
                Some("最终反馈。"),
                Some("task_complete.last_agent_message"),
            )
            .with_thread_title(Some("完成线程")),
        );
        assert_eq!(completion.bark_title, "线程正常完成：完成线程");
        assert_eq!(completion.bark_body, "最终反馈。");

        let reply = runtime.build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-reply"),
                Some("turn-reply"),
                Some("thread-reply"),
                None,
                Some("<proposed_plan>\n# 修复计划\n- 等待确认\n</proposed_plan>"),
                "reply-needed",
            )
            .with_thread_title(Some("等待确认")),
        );
        assert_eq!(reply.bark_title, "等待回复：等待确认");
        assert_eq!(reply.bark_body, "# 修复计划\n- 等待确认");
        assert!(!reply.bark_body.contains("thread-reply"));
        assert!(!reply.bark_body.contains("<proposed_plan>"));
        assert!(!reply.bark_body.contains("</proposed_plan>"));

        let recoverable = runtime.build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-error"),
                Some("turn-error"),
                Some("thread-error"),
                None,
                Some("stream disconnected before completion"),
                "recoverable",
            )
            .with_thread_title(Some("异常线程")),
        );
        assert_eq!(recoverable.bark_title, "线程异常停止：异常线程");
        assert_eq!(
            recoverable.bark_body,
            "stream disconnected before completion"
        );
    }

    #[test]
    fn event_dedupe_key_changes_when_body_hash_changes() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let first = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-dedupe"),
            Some("turn-dedupe"),
            Some("thread-dedupe"),
            None,
            Some("第一版完成反馈"),
            "hook-stop",
        ));
        let second = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-dedupe"),
            Some("turn-dedupe"),
            Some("thread-dedupe"),
            None,
            Some("第二版完成反馈"),
            "hook-stop",
        ));
        let duplicate = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-dedupe"),
            Some("turn-dedupe"),
            Some("thread-dedupe"),
            None,
            Some("第一版完成反馈"),
            "hook-stop",
        ));

        assert_ne!(first.payload["body_sha256"], second.payload["body_sha256"]);
        assert_ne!(first.dedupe_key, second.dedupe_key);
        assert_eq!(first.dedupe_key, duplicate.dedupe_key);
        assert!(first.dedupe_key.contains(
            first.payload["body_sha256"]
                .as_str()
                .unwrap()
                .get(..16)
                .unwrap()
        ));
    }

    #[test]
    fn hook_stop_with_final_feedback_is_classified_as_completion() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-real"),
            Some("turn-real"),
            Some("thread-real"),
            Some("/tmp/rollout-real.jsonl"),
            Some("ok"),
            "hook-stop",
        ));

        assert_eq!(event.kind, "completion");
        assert_eq!(event.event_type, "completion");
        assert_eq!(event.title, "任务完成");
        assert_eq!(event.payload["event_type"], "completion");
        assert_eq!(
            event.payload["last_assistant_message"]["classification"],
            "completion"
        );
        assert_eq!(event.payload["body_summary"], "ok");
        assert_eq!(event.bark_title, "线程正常完成：未命名线程");
        assert!(!event.bark_body.contains("最后反馈："));
        assert_eq!(event.bark_body, "ok");
        assert!(
            event.dedupe_key.starts_with("completion:"),
            "dedupe key must use the classified event kind"
        );
    }

    #[test]
    fn hook_stop_with_plan_feedback_is_classified_as_reply_needed() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-plan"),
            Some("turn-plan"),
            Some("thread-plan"),
            Some("/tmp/rollout-plan.jsonl"),
            Some("<proposed_plan>\n# 修复计划\n- 等待确认\n</proposed_plan>"),
            "hook-stop",
        ));

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.event_type, "reply_needed");
        assert_eq!(
            event.payload["last_assistant_message"]["classification"],
            "reply_needed"
        );
        assert_eq!(event.bark_body, "# 修复计划\n- 等待确认");
        assert!(!event.bark_body.contains("thread-plan"));
    }

    #[test]
    fn reply_needed_plan_bark_body_uses_extracted_plan_without_xml_tags() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-plan"),
            Some("turn-plan"),
            Some("thread-plan"),
            Some("/tmp/rollout-plan.jsonl"),
            Some("<proposed_plan>\n# 修复计划\n- 等待确认\n</proposed_plan>"),
            "hook-stop",
        ));

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.bark_body, "# 修复计划\n- 等待确认");
        assert!(!event.bark_body.contains("thread-plan"));
        assert!(!event.bark_body.contains("状态说明："));
        assert!(!event.bark_body.contains("待回复内容："));
        assert!(!event.bark_body.contains("<proposed_plan>"));
        assert!(!event.bark_body.contains("</proposed_plan>"));
        assert_eq!(event.payload["bark"]["body_source"], "proposed_plan");
        assert!(event.payload["body_summary"]
            .as_str()
            .unwrap()
            .contains("# 修复计划\n- 等待确认"));
        assert!(!event.payload["body_summary"]
            .as_str()
            .unwrap()
            .contains("<proposed_plan>"));
        assert!(!event.payload["last_assistant_message"]["summary"]
            .as_str()
            .unwrap()
            .contains("<proposed_plan>"));
    }

    #[test]
    fn reply_needed_plan_event_keeps_bark_body_safe_and_persists_only_body_metadata() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );
        let body = "\
线程 ID：thread-secret\n\
状态说明：等待用户确认。\n\
<proposed_plan>\n\
# 安全计划\n\
- 检查状态\n\
- 不展示 https://example.com/path?token=secret-token\n\
Authorization: Bearer secret-token\n\
</proposed_plan>";

        let event = runtime.build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-secret"),
                Some("turn-secret"),
                Some("session-secret"),
                None,
                Some(body),
                "reply-needed",
            )
            .with_body_source(Some("proposed_plan")),
        );

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.bark_body, "# 安全计划\n- 检查状态");
        assert_probe_bark_body_is_safe(&event.bark_body);
        assert!(event.payload["bark"].get("body").is_none());
        assert!(event.payload["bark_body"].is_null());
        assert!(event.payload["bark"]["body_summary"].is_null());
        assert!(event.payload["bark"]["body_sha256"].is_string());
        assert!(event.payload["bark"]["body_length"].is_u64());
        assert!(event.payload["bark"]["chunk_count"].is_u64());
        assert!(event.payload["bark"]["request_count"].is_u64());

        let stored = serde_json::to_string(&event.payload).unwrap();
        assert!(!stored.contains("https://example.com"));
        assert!(!stored.contains("secret-token"));
        assert!(!stored.contains("Authorization"));
        assert!(!stored.contains("<proposed_plan>"));
        assert!(!stored.contains("</proposed_plan>"));
    }

    #[test]
    fn bark_body_and_summary_redact_common_api_key_lines() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );
        let body = "\
最终反馈：\n\
部署完成。\n\
OPENAI_API_KEY=sk-secret-value\n\
PRIVATE_KEY=-----BEGIN PRIVATE KEY-----\n\
Authorization: Bearer secret-token\n\
末尾可见。";

        let event = runtime.build_event(ProbeEventInput::notify_completion_with_context(
            Some("thread-secret"),
            Some("turn-secret"),
            None,
            None,
            Some(body),
            Some("task_complete.last_agent_message"),
        ));

        assert_eq!(event.bark_body, "部署完成。\n末尾可见。");
        assert_eq!(event.payload["body_summary"], "部署完成。\n末尾可见。");
        let stored = serde_json::to_string(&event.payload).unwrap();
        assert!(!stored.contains("sk-secret-value"));
        assert!(!stored.contains("PRIVATE_KEY"));
        assert!(!stored.contains("Bearer"));
    }

    #[test]
    fn long_bark_chunks_reassemble_to_original_body_after_prefix_removal() {
        let original = format!(
            "{}中间🙂{}尾段",
            "计划正文".repeat(280),
            "emoji🙂".repeat(220)
        );
        let chunks = probe_bark_body_chunks(&original, 2400);

        assert!(chunks.len() > 1);
        let mut reassembled = String::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let prefix = format!("第 {}/{} 段\n\n", index + 1, chunks.len());
            assert!(chunk.starts_with(&prefix));
            let body_part = chunk.strip_prefix(&prefix).unwrap();
            assert!(body_part.len() <= 2400);
            assert!(original.is_char_boundary(
                original
                    .find(body_part)
                    .expect("chunk should start at utf8 boundary")
            ));
            reassembled.push_str(body_part);
        }
        assert_eq!(reassembled, original);
    }

    #[test]
    fn request_user_input_bark_body_keeps_only_question_options_and_safe_text() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );
        let body = "\
等待用户选择：需要用户确认。\n\
\n\
thread_id: thread-question\n\
线程 ID：thread-question\n\
Turn ID：turn-question\n\
Call ID：call-question\n\
状态说明: 这是待回复状态。\n\
待选择内容：\n\
问题 1：怎么继续？\n\
选项 1：直接执行\n\
说明：按当前计划继续\n\
选项 2：先调整\n\
URL: https://example.com/secret-token\n\
Authorization: Bearer secret-token";

        let event = runtime.build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-question"),
                Some("turn-question"),
                Some("thread-question"),
                None,
                Some(body),
                "reply-needed",
            )
            .with_body_source(Some("request_user_input")),
        );

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(
            event.bark_body,
            "问题 1：怎么继续？\n选项 1：直接执行\n说明：按当前计划继续\n选项 2：先调整"
        );
        assert_eq!(event.payload["body_summary"], event.bark_body);
        assert_probe_bark_body_is_safe(&event.bark_body);
    }

    #[test]
    fn completion_bark_body_keeps_only_final_reply_text_and_safe_metadata() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );
        let body = "\
线程 ID：thread-complete\n\
thread_id: thread-complete\n\
状态说明: 线程已完成。\n\
最后反馈：\n\
最终回复第一行\n\
已完成用户要求。\n\
URL: https://example.com/path\n\
Authorization: Bearer secret-token";

        let event = runtime.build_event(
            ProbeEventInput::notify_completion_with_context(
                Some("thread-complete"),
                Some("turn-complete"),
                Some("thread-complete"),
                None,
                Some(body),
                Some("task_complete.last_agent_message"),
            )
            .with_thread_title(Some("完成线程")),
        );

        assert_eq!(event.kind, "completion");
        assert_eq!(event.bark_title, "线程正常完成：完成线程");
        assert_eq!(event.bark_body, "最终回复第一行\n已完成用户要求。");
        assert_eq!(event.payload["body_summary"], event.bark_body);
        assert_probe_bark_body_is_safe(&event.bark_body);
    }

    #[test]
    fn bark_body_drops_memory_citation_metadata_but_keeps_literal_code() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );
        let body = "完成。\n\n<oai-mem-citation>\n<rollout_ids>\nprivate-id\n</rollout_ids>\n</oai-mem-citation>\n\n```xml\n<oai-mem-citation>literal</oai-mem-citation>\n```";
        let event = runtime.build_event(ProbeEventInput::notify_completion_with_context(
            Some("thread-memory-citation"),
            Some("turn-memory-citation"),
            None,
            None,
            Some(body),
            Some("task_complete.last_agent_message"),
        ));
        assert_eq!(
            event.bark_body,
            "完成。\n\n```xml\n<oai-mem-citation>literal</oai-mem-citation>\n```"
        );
        assert!(!event.bark_body.contains("private-id"));
    }

    #[test]
    fn passive_old_plan_payload_keeps_only_safe_body_metadata() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );
        let old_plan = "\
等待用户回复：Plan 已完成，正在等待确认后继续。\n\
\n\
thread_id: thread-old-plan\n\
状态说明: old passive scan should not persist this full body.\n\
Plan 摘要:\n\
# 旧计划\n\
- 不应回填\n\
Authorization: Bearer secret-token";

        let event = runtime.build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-old-plan"),
                Some("turn-old-plan"),
                Some("thread-old-plan"),
                None,
                Some(old_plan),
                "reply-needed",
            )
            .with_body_source(Some("proposed_plan"))
            .with_passive_scan_source(),
        );

        assert_eq!(event.kind, "reply-needed");
        assert_eq!(event.payload["scan_source"], "passive-scan");
        assert_eq!(event.payload["body_summary"], "# 旧计划\n- 不应回填");
        assert_eq!(
            event.payload["body_length"].as_u64().unwrap(),
            old_plan.len() as u64
        );
        assert!(event.payload["body_sha256"].as_str().unwrap().len() >= 64);
        assert!(event.payload["bark"].get("body").is_none());
        assert!(event.payload["bark_body"].is_null());
        assert_eq!(event.payload["bark"]["chunk_count"], 1);
        assert_eq!(event.payload["bark"]["request_count"], 0);
        let stored = serde_json::to_string(&event.payload).unwrap();
        assert!(!stored.contains("Authorization"));
        assert!(!stored.contains("secret-token"));
        assert!(!stored.contains("thread_id:"));
        assert!(!stored.contains("状态说明"));
        assert!(!stored.contains("等待用户回复"));
    }

    #[test]
    fn passive_scan_status_events_use_six_hour_ttl_and_scan_metadata() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let passive = runtime.build_event(
            ProbeEventInput::hook_stop_with_context(
                Some("thread-passive"),
                Some("turn-passive"),
                Some("thread-passive"),
                None,
                Some("等待用户选择"),
                "reply-needed",
            )
            .with_passive_scan_source(),
        );

        assert_eq!(passive.kind, "reply-needed");
        assert_eq!(passive.ttl_seconds, 6 * 60 * 60);
        assert_eq!(passive.source, "agenthub-webd probe passive-scan");
        assert_eq!(passive.payload["scan_source"], "passive-scan");
        assert_eq!(passive.payload["dedupe_ttl_seconds"], 6 * 60 * 60);

        let hook = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-hook"),
            Some("turn-hook"),
            Some("thread-hook"),
            None,
            Some("等待用户选择"),
            "reply-needed",
        ));

        assert_eq!(hook.ttl_seconds, PROBE_EVENT_TTL_SECONDS);
        assert_eq!(hook.payload["dedupe_ttl_seconds"], PROBE_EVENT_TTL_SECONDS);
        assert_eq!(hook.payload["scan_source"], "hook-stop");
    }

    #[test]
    fn passive_scan_plan_dedupe_ignores_generated_time_but_tracks_plan_text() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );
        let body_at_t1 = "等待用户回复：Plan 已完成，正在等待确认后继续。\n\nthread_id: thread-plan\ntime: ignored\nturn_id: turn-plan\n时间: 2026-06-16 21:03:40 北京时间\n状态说明: 这一轮已经写出 Plan 并等待用户确认，所以这是待回复，不是异常停止或普通完成。\n\nPlan 摘要:\n# 修复计划\n- 等待确认";
        let body_at_t2 = body_at_t1.replace("21:03:40", "21:05:42");
        let changed_plan = body_at_t1.replace("- 等待确认", "- 等待确认\n- 增加验证");
        let build = |body: &str| {
            runtime.build_event(
                ProbeEventInput::hook_stop_with_context(
                    Some("thread-plan"),
                    Some("turn-plan"),
                    Some("thread-plan"),
                    None,
                    Some(body),
                    "reply-needed",
                )
                .with_body_source(Some("proposed_plan"))
                .with_passive_scan_source(),
            )
        };

        let first = build(body_at_t1);
        let same_plan_later = build(&body_at_t2);
        let changed = build(&changed_plan);

        assert_ne!(
            first.payload["body_sha256"],
            same_plan_later.payload["body_sha256"]
        );
        assert_eq!(first.dedupe_key, same_plan_later.dedupe_key);
        assert_ne!(first.dedupe_key, changed.dedupe_key);
    }

    #[test]
    fn hook_stop_with_terminal_error_is_classified_as_recoverable() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-error"),
            Some("turn-error"),
            Some("thread-error"),
            None,
            Some("stream disconnected before completion: Transport error"),
            "hook-stop",
        ));

        assert_eq!(event.kind, "recoverable");
        assert_eq!(event.event_type, "recoverable");
        assert_eq!(
            event.payload["last_assistant_message"]["classification"],
            "recoverable"
        );
        assert_eq!(
            event.bark_body,
            "stream disconnected before completion: Transport error"
        );
        assert!(!event.bark_body.contains("最后异常信息"));
    }

    #[test]
    fn hook_stop_with_normal_final_feedback_is_classified_as_completion() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-normal"),
            Some("turn-normal"),
            Some("thread-normal"),
            None,
            Some("一切已经处理完毕。"),
            "hook-stop",
        ));

        assert_eq!(event.kind, "completion");
        assert_eq!(event.event_type, "completion");
        assert_eq!(event.payload["raw_kind"], "hook-stop");
        assert!(!event.bark_body.contains("最后反馈："));
        assert!(event.bark_body.contains("一切已经处理完毕"));
    }

    #[test]
    fn hook_stop_with_product_named_final_feedback_is_classified_as_completion() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-product-named"),
            Some("turn-product-named"),
            Some("thread-product-named"),
            None,
            Some("AgentHub Probe v0.1.68 smoke completion finished normally."),
            "hook-stop",
        ));

        assert_eq!(event.kind, "completion");
        assert_eq!(event.event_type, "completion");
        assert_eq!(
            event.payload["last_assistant_message"]["classification"],
            "completion"
        );
    }

    #[test]
    fn hook_stop_ignores_machine_control_payloads() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-control"),
            Some("turn-control"),
            Some("thread-control"),
            None,
            Some(r#"{"suggestions":[{"title":"x","prompt":"y"}]}"#),
            "hook-stop",
        ));

        assert_eq!(event.kind, "hook-stop");
        assert_eq!(event.event_type, "hook_stop");
    }

    #[test]
    fn hook_stop_does_not_treat_sentinel_system_message_as_completion_or_abnormal() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-system"),
            Some("turn-system"),
            Some("thread-system"),
            None,
            Some("Codex Sentinel Lite 检测到「Rate limited」，只记录并推送异常信息，不自动恢复。"),
            "hook-stop",
        ));

        assert_eq!(event.kind, "hook-stop");
        assert_eq!(event.event_type, "hook_stop");
    }

    #[test]
    fn hook_stop_does_not_treat_probe_system_message_as_completion_or_abnormal() {
        let runtime = ProbeRuntime::new(
            Config::default(),
            PlatformPaths::for_kind(PlatformKind::Linux),
        );

        let event = runtime.build_event(ProbeEventInput::hook_stop_with_context(
            Some("thread-probe-system"),
            Some("turn-probe-system"),
            Some("thread-probe-system"),
            None,
            Some("AgentHub Probe 检测到「Rate limited」，只记录并推送异常信息，不自动恢复。"),
            "hook-stop",
        ));

        assert_eq!(event.kind, "hook-stop");
        assert_eq!(event.event_type, "hook_stop");
    }

    fn assert_probe_bark_body_is_safe(body: &str) {
        for forbidden in [
            "thread_id:",
            "thread-question",
            "thread-complete",
            "线程 ID：",
            "状态说明",
            "<proposed_plan>",
            "</proposed_plan>",
            "https://",
            "secret-token",
            "Authorization",
            "API_KEY",
            "PRIVATE_KEY",
            "Bearer",
            "sk-",
            "[redacted sensitive line]",
        ] {
            assert!(
                !body.contains(forbidden),
                "Bark body should not contain {forbidden:?}: {body}"
            );
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }

    #[test]
    fn remove_agenthub_managed_hooks_preserves_third_party_hooks_and_prunes_empty_groups() {
        let mut root = json!({
            "hooks": {
                "Stop": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "/opt/agenthub/bin/agenthub-webd --config /opt/agenthub/config.toml probe hook-stop"
                            },
                            {
                                "type": "command",
                                "command": "/usr/local/bin/custom-audit-hook --event stop"
                            }
                        ]
                    },
                    {
                        "matcher": "*",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "'/Library/Application Support/AgentHub/agenthub-webd' probe hook-stop"
                            }
                        ]
                    }
                ],
                "PreToolUse": [
                    {
                        "matcher": "^(functions\\.)?request_user_input(_async)?$",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "/opt/agenthub/bin/agenthub-webd probe hook-request-user-input",
                                "timeout": 5
                            }
                        ]
                    }
                ]
            }
        });

        assert!(remove_agenthub_managed_hooks(&mut root));
        let stop_groups = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop_groups.len(), 1);
        let stop_hooks = stop_groups[0]["hooks"].as_array().unwrap();
        assert_eq!(stop_hooks.len(), 1);
        assert_eq!(
            stop_hooks[0]["command"],
            "/usr/local/bin/custom-audit-hook --event stop"
        );
        assert!(root["hooks"].get("PreToolUse").is_none());
        assert!(!remove_agenthub_managed_hooks(&mut root));
    }
}
