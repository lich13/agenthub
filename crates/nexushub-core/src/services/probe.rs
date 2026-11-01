use crate::shell::{shell_quote, QuoteStyle};
use crate::{
    codex::{self, CodexPaths, ThreadStatus, ThreadSummary},
    config::Config,
    db::JobRecord,
    platform::PlatformPaths,
    probe as probe_core,
    services::commands,
    services::system::{require_capability, Capability},
    services::threads::{self, ThreadListRuntimeState},
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, str::FromStr};

pub const PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS: i64 = 10 * 60;

#[derive(Debug, Clone, Default)]
pub struct ProbeStatusAggregation {
    pub recent_event_count: usize,
    pub running_threads: Vec<ThreadSummary>,
    pub reply_needed_threads: Vec<ThreadSummary>,
    pub recoverable_threads: Vec<ThreadSummary>,
    pub active_sessions: Vec<probe_core::ProbeLiveSession>,
}

#[derive(Debug, Clone)]
pub struct ProbeStatusFacadePlan {
    pub required_capability: Capability,
    pub status: ProbeStatusAggregation,
}

#[derive(Debug, Clone, Copy)]
pub struct ProbeUseCases<'a> {
    config: &'a Config,
    platform: &'a PlatformPaths,
}

impl<'a> ProbeUseCases<'a> {
    pub fn new(config: &'a Config, platform: &'a PlatformPaths) -> Self {
        Self { config, platform }
    }

    pub fn claude_hooks(
        self,
        request: crate::claude::hooks::ClaudeHooksRequest,
    ) -> Result<crate::claude::hooks::ClaudeHooksStatus> {
        self.provider_hooks(crate::claude::hooks::HookProvider::Claude, request)
    }

    pub fn provider_hooks(
        self,
        provider: crate::claude::hooks::HookProvider,
        request: crate::claude::hooks::ClaudeHooksRequest,
    ) -> Result<crate::claude::hooks::ClaudeHooksStatus> {
        anyhow::ensure!(
            self.platform.kind == crate::platform::PlatformKind::Macos,
            "{} hooks 仅支持本机 macOS 设置",
            provider.label()
        );
        crate::claude::hooks::manage_provider(
            provider,
            &self.platform.daemon_binary(),
            &self.platform.config_file,
            request,
        )
    }

    pub fn status(self) -> Result<ProbeStatusFacadePlan> {
        probe_status_with_capability(self.config, self.platform)
    }

    pub fn action(self, action: ProbeAction) -> Result<ProbeActionPlan> {
        plan_probe_action(self.config, self.platform, action)
    }

    pub fn action_with_device_key(
        self,
        action: ProbeAction,
        device_key_configured: bool,
    ) -> Result<ProbeActionPlan> {
        plan_probe_action_with_device_key(self.config, self.platform, action, device_key_configured)
    }

    pub fn action_with_device_key_and_config_path(
        self,
        action: ProbeAction,
        device_key_configured: bool,
        config_path: impl AsRef<Path>,
    ) -> Result<ProbeActionPlan> {
        plan_probe_action_with_device_key_and_config_path(
            self.config,
            self.platform,
            action,
            device_key_configured,
            config_path,
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProbeAction {
    #[serde(rename = "barkTest", alias = "bark-test")]
    BarkTest,
    #[serde(rename = "installHooks", alias = "hooks-install")]
    InstallHooks,
    #[serde(rename = "removeHooks", alias = "hooks-remove")]
    RemoveHooks,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeExecutionKind {
    FixedShellJob,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeFixedJobSpec {
    pub kind: String,
    pub title: String,
    pub args: Vec<String>,
    pub command: String,
    pub exclusive_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeBarkDeliveryDecision {
    pub notifications_enabled: bool,
    pub relevant_switch_enabled: bool,
    pub device_key_configured: bool,
    pub should_send: bool,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeSnapshotStatus {
    Fresh,
    Cached,
    Initial,
}

impl ProbeSnapshotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Cached => "cached",
            Self::Initial => "initial",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeThreadNotificationPlan {
    pub body: Option<String>,
    pub body_source: Option<String>,
    pub reason_label: Option<String>,
    pub fresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeEventRecordPlan {
    pub event: probe_core::ProbeBuiltEvent,
    pub passive_marker_key: Option<String>,
    pub duplicate_outcome: probe_core::ProbeEventOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeEventRecordWritePlan {
    pub outcome: probe_core::ProbeEventOutcome,
    pub record: Option<ProbeEventRecordWrite>,
    pub passive_marker: Option<ProbePassiveMarkerWrite>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeEventRecordWrite {
    pub kind: String,
    pub thread_id: Option<String>,
    pub title: String,
    pub message: String,
    pub dedupe_key: String,
    pub source: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbePassiveMarkerWrite {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeActionPlan {
    pub required_capability: Capability,
    pub action: ProbeAction,
    pub execution: ProbeExecutionKind,
    pub job: Option<ProbeFixedJobSpec>,
    pub diagnostic_plan: Option<probe_core::ProbeActionPlan>,
}

pub fn probe_status_with_capability(
    config: &Config,
    platform: &PlatformPaths,
) -> Result<ProbeStatusFacadePlan> {
    require_capability(platform, Capability::Probe)?;
    Ok(ProbeStatusFacadePlan {
        required_capability: Capability::Probe,
        status: aggregate_probe_status(config),
    })
}

pub fn probe_status_snapshot_view(
    mut value: Value,
    snapshot_age_seconds: i64,
    is_refreshing: bool,
    snapshot_status: ProbeSnapshotStatus,
) -> Value {
    if let Value::Object(ref mut object) = value {
        object.insert(
            "snapshot_age_seconds".to_string(),
            json!(snapshot_age_seconds),
        );
        object.insert("is_refreshing".to_string(), json!(is_refreshing));
        object.insert(
            "snapshot_status".to_string(),
            json!(snapshot_status.as_str()),
        );
    }
    value
}

pub fn probe_status_with_runtime_read_model(
    mut status: probe_core::ProbeStatus,
    read_model: ProbeStatusAggregation,
    recent_event_count_override: Option<usize>,
) -> probe_core::ProbeStatus {
    status.recent_event_count =
        recent_event_count_override.unwrap_or(read_model.recent_event_count);
    status.running_count = read_model.running_threads.len();
    status.reply_needed_count = read_model.reply_needed_threads.len();
    status.recoverable_count = read_model.recoverable_threads.len();
    status.running_threads = read_model.running_threads;
    status.reply_needed_threads = read_model.reply_needed_threads;
    status.recoverable_threads = read_model.recoverable_threads;
    status.active_sessions = read_model.active_sessions;
    status
}

pub fn plan_probe_action(
    config: &Config,
    platform: &PlatformPaths,
    action: ProbeAction,
) -> Result<ProbeActionPlan> {
    plan_probe_action_with_device_key(config, platform, action, false)
}

pub fn plan_probe_action_with_config_path(
    config: &Config,
    platform: &PlatformPaths,
    action: ProbeAction,
    config_path: impl AsRef<Path>,
) -> Result<ProbeActionPlan> {
    plan_probe_action_with_device_key_and_config_path(config, platform, action, false, config_path)
}

pub fn plan_probe_action_with_device_key(
    config: &Config,
    platform: &PlatformPaths,
    action: ProbeAction,
    device_key_configured: bool,
) -> Result<ProbeActionPlan> {
    plan_probe_action_with_device_key_and_config_path(
        config,
        platform,
        action,
        device_key_configured,
        &platform.config_file,
    )
}

pub fn plan_probe_action_with_device_key_and_config_path(
    config: &Config,
    platform: &PlatformPaths,
    action: ProbeAction,
    device_key_configured: bool,
    config_path: impl AsRef<Path>,
) -> Result<ProbeActionPlan> {
    let required_capability = probe_action_capability(action);
    require_capability(platform, required_capability)?;
    let config_path = config_path.as_ref();
    let runtime = probe_core::ProbeRuntime::new(config.clone(), platform.clone());
    let diagnostic_plan = match action {
        ProbeAction::BarkTest => Some(runtime.bark_test_plan(device_key_configured)),
        ProbeAction::InstallHooks => {
            Some(runtime.plan_action(probe_core::ProbeActionPlanKind::InstallHooks)?)
        }
        ProbeAction::RemoveHooks => {
            Some(runtime.plan_action(probe_core::ProbeActionPlanKind::RemoveHooks)?)
        }
    };
    let job = Some(probe_fixed_job_spec(platform, action, config_path)?);
    Ok(ProbeActionPlan {
        required_capability,
        action,
        execution: ProbeExecutionKind::FixedShellJob,
        job,
        diagnostic_plan,
    })
}

fn resolve_provider_sandbox_root(config: &Config) -> Option<std::path::PathBuf> {
    let raw = config.codex.home.as_os_str().to_string_lossy();
    if raw.trim().is_empty() || raw == "auto" {
        return None;
    }
    let default_codex = dirs::home_dir().map(|home| home.join(".codex"));
    if default_codex.as_deref() == Some(config.codex.home.as_path()) {
        return None;
    }
    if config.codex.home.file_name().and_then(|s| s.to_str()) == Some(".codex") {
        config.codex.home.parent().map(Path::to_path_buf)
    } else {
        Some(config.codex.home.clone())
    }
}

fn native_stream_live_bucket(
    stream: &crate::native_probe::NativeStreamSnapshot,
    now_ms: i64,
) -> Option<&'static str> {
    if stream.record_count != stream.settled_count {
        return None;
    }
    let last = stream.events.last()?;
    let fresh_ms = PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS.saturating_mul(1000);
    if now_ms.saturating_sub(last.timestamp_ms) > fresh_ms {
        return None;
    }
    match last.kind.as_str() {
        "reply_needed" | "authorization" => Some("reply_needed"),
        "failure" => Some("recoverable"),
        _ => None,
    }
}

fn is_updated_within_fresh_window(updated_at: &Option<String>, now_ms: i64) -> bool {
    let Some(ts) = updated_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis())
    else {
        return true;
    };
    let fresh_ms = PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS.saturating_mul(1000) + 60_000;
    now_ms.saturating_sub(ts) <= fresh_ms
}

type LiveProviderSessionsCacheKey = (Option<std::path::PathBuf>, std::path::PathBuf, usize);
type LiveProviderSessionsCacheEntry = (
    LiveProviderSessionsCacheKey,
    Vec<probe_core::ProbeLiveSession>,
    std::time::Instant,
);

static LIVE_PROVIDER_SESSIONS_CACHE: std::sync::Mutex<Option<LiveProviderSessionsCacheEntry>> =
    std::sync::Mutex::new(None);

pub fn invalidate_live_provider_sessions_cache() {
    if let Ok(mut guard) = LIVE_PROVIDER_SESSIONS_CACHE.lock() {
        *guard = None;
    }
}

pub(crate) fn evict_idle_live_provider_sessions_cache(max_idle: std::time::Duration) {
    if let Ok(mut guard) = LIVE_PROVIDER_SESSIONS_CACHE.lock() {
        if guard
            .as_ref()
            .is_some_and(|(_, _, ts)| ts.elapsed() >= max_idle)
        {
            *guard = None;
        }
    }
}

fn live_session(
    provider: &str,
    id: String,
    session_key: String,
    title: String,
    bucket: &str,
    updated_at: Option<String>,
    cwd: Option<String>,
) -> probe_core::ProbeLiveSession {
    probe_core::ProbeLiveSession {
        provider: provider.to_string(),
        title: if title.trim().is_empty() {
            id.clone()
        } else {
            title
        },
        id,
        session_key,
        bucket: bucket.to_string(),
        updated_at,
        cwd: cwd.filter(|c| !c.is_empty()),
    }
}

fn stream_live_buckets(
    streams: &[crate::native_probe::NativeStreamSnapshot],
    now_ms: i64,
) -> std::collections::HashMap<String, &'static str> {
    streams
        .iter()
        .filter_map(|s| native_stream_live_bucket(s, now_ms).map(|b| (s.id.clone(), b)))
        .collect()
}

fn aggregate_active_sessions(
    config: &Config,
    limit: usize,
    running_threads: &[ThreadSummary],
    reply_needed_threads: &[ThreadSummary],
    recoverable_threads: &[ThreadSummary],
) -> Vec<probe_core::ProbeLiveSession> {
    let mut sessions = Vec::new();
    let per_bucket_limit = limit.clamp(1, 50);
    for (bucket, threads) in [
        ("running", running_threads),
        ("reply_needed", reply_needed_threads),
        ("recoverable", recoverable_threads),
    ] {
        for thread in threads.iter().take(per_bucket_limit) {
            sessions.push(live_session(
                "codex",
                thread.id.clone(),
                thread.id.clone(),
                thread.title.clone(),
                bucket,
                thread.updated_at.clone(),
                thread.cwd.clone(),
            ));
        }
    }

    let sandbox_root = resolve_provider_sandbox_root(config);
    let cache_key = (
        sandbox_root.clone(),
        config.codex.home.clone(),
        per_bucket_limit,
    );
    if let Ok(guard) = LIVE_PROVIDER_SESSIONS_CACHE.lock() {
        if let Some((ref cached_key, ref cached_items, ts)) = *guard {
            if *cached_key == cache_key && ts.elapsed() < std::time::Duration::from_secs(3) {
                sessions.extend(cached_items.iter().cloned());
                return sessions;
            }
        }
    }

    let codex_len = sessions.len();
    let now_ms = Utc::now().timestamp_millis();

    let claude_paths = match &sandbox_root {
        Some(root) => crate::claude::ClaudePaths {
            projects: root.join(".claude/projects"),
            config: root.join(".claude"),
        },
        None => crate::claude::ClaudePaths::default_for_user(),
    };
    if claude_paths.available() {
        if let Ok(items) =
            crate::claude::list_claude_sessions(&claude_paths, per_bucket_limit, None)
        {
            for item in items {
                let bucket = if item.status == "running" {
                    Some("running")
                } else if item.read_error.is_some() {
                    Some("recoverable")
                } else if is_updated_within_fresh_window(&item.updated_at, now_ms) {
                    crate::claude::probe_snapshot_for_path(&claude_paths, &item.path)
                        .ok()
                        .and_then(|s| native_stream_live_bucket(&s, now_ms))
                } else {
                    None
                };
                if let Some(bucket) = bucket {
                    sessions.push(live_session(
                        "claude_code",
                        item.id,
                        item.session_key,
                        item.title,
                        bucket,
                        item.updated_at,
                        Some(item.cwd),
                    ));
                }
            }
        }
    }

    let grok_paths = match &sandbox_root {
        Some(root) => crate::grok::GrokPaths {
            home: root.join(".grok"),
        },
        None => crate::grok::GrokPaths::default_for_user(),
    };
    if grok_paths.sessions().is_dir() {
        if let Ok(items) = crate::grok::list_grok_sessions(&grok_paths, per_bucket_limit, None) {
            let stream_buckets = if items.iter().any(|item| {
                item.status != "running" && is_updated_within_fresh_window(&item.updated_at, now_ms)
            }) {
                crate::grok::notification_snapshots(&grok_paths)
                    .map(|scan| stream_live_buckets(&scan.streams, now_ms))
                    .unwrap_or_default()
            } else {
                std::collections::HashMap::new()
            };
            for item in items {
                let bucket = if item.status == "running" {
                    Some("running")
                } else {
                    stream_buckets.get(&item.id).copied()
                };
                if let Some(bucket) = bucket {
                    sessions.push(live_session(
                        "grok",
                        item.id.clone(),
                        item.id,
                        item.title,
                        bucket,
                        item.updated_at,
                        Some(item.cwd),
                    ));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let list_req = crate::provider_sessions::ProviderListRequest {
            archived: Some(false),
            q: None,
            limit: Some(per_bucket_limit),
        };
        let ag_paths = match &sandbox_root {
            Some(root) => crate::antigravity::sessions::AntigravityPaths::offline(
                root.join(".gemini/antigravity"),
            ),
            None => crate::antigravity::sessions::AntigravityPaths::default_for_user(),
        };
        if ag_paths.available() {
            if let Ok(items) = crate::antigravity::sessions::list_sessions(&ag_paths, &list_req) {
                for item in items {
                    let bucket = match item.status.as_str() {
                        "running" => Some("running"),
                        "waiting" | "reply_needed" => Some("reply_needed"),
                        "failed" => Some("recoverable"),
                        _ if item.read_error.is_some() => Some("recoverable"),
                        _ => None,
                    };
                    if let Some(bucket) = bucket {
                        sessions.push(live_session(
                            "antigravity",
                            item.id,
                            item.session_key,
                            item.title,
                            bucket,
                            item.updated_at,
                            Some(item.cwd),
                        ));
                    }
                }
            }
        }

        let dsh_paths = match &sandbox_root {
            Some(root) => crate::dsh::DshPaths {
                home: root.join(".dsh"),
            },
            None => crate::dsh::DshPaths::default_for_user(),
        };
        if dsh_paths.home.join("sessions").is_dir() {
            if let Ok(items) = crate::dsh::sessions::list_sessions(&dsh_paths, &list_req) {
                let stream_buckets = if items.iter().any(|item| {
                    item.status != "running"
                        && item.read_error.is_none()
                        && is_updated_within_fresh_window(&item.updated_at, now_ms)
                }) {
                    crate::dsh::notifications::scan(&dsh_paths)
                        .map(|scan| stream_live_buckets(&scan.streams, now_ms))
                        .unwrap_or_default()
                } else {
                    std::collections::HashMap::new()
                };
                for item in items {
                    let bucket = if item.status == "running" {
                        Some("running")
                    } else if let Some(&b) = stream_buckets.get(&item.id) {
                        Some(b)
                    } else if item.read_error.is_some() {
                        Some("recoverable")
                    } else {
                        None
                    };
                    if let Some(bucket) = bucket {
                        sessions.push(live_session(
                            "dsh",
                            item.id,
                            item.session_key,
                            item.title,
                            bucket,
                            item.updated_at,
                            Some(item.cwd),
                        ));
                    }
                }
            }
        }
    }

    if let Ok(mut guard) = LIVE_PROVIDER_SESSIONS_CACHE.lock() {
        *guard = Some((
            cache_key,
            sessions[codex_len..].to_vec(),
            std::time::Instant::now(),
        ));
    }

    sessions
}

fn probe_codex_buckets(
    config: &Config,
    limit: usize,
) -> (Vec<ThreadSummary>, Vec<ThreadSummary>, Vec<ThreadSummary>) {
    let limit = limit.clamp(1, 200);
    let resolved = codex::resolve_codex_paths(&config.codex.home);
    let paths = resolved.codex_paths();
    let Ok(raw_threads) = codex::list_threads(&paths, false, None, usize::MAX) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let hidden_thread_ids = codex::hidden_thread_ids(&paths).unwrap_or_default();
    let archived_thread_ids = codex::archived_thread_ids(&paths).unwrap_or_default();
    let running_jobs = running_thread_jobs(&config.paths.db_path).unwrap_or_default();
    let mut threads = threads::apply_thread_list_runtime_state(
        raw_threads,
        ThreadListRuntimeState {
            running_jobs: &running_jobs,
            hidden_thread_ids: &hidden_thread_ids,
            archived_thread_ids: &archived_thread_ids,
        },
    );
    ThreadSummary::sort_by_message_time(&mut threads);
    let mut running = Vec::new();
    let mut reply_needed = Vec::new();
    let mut recoverable = Vec::new();
    for row in threads {
        match row.status {
            ThreadStatus::Running if running.len() < limit => running.push(row),
            ThreadStatus::ReplyNeeded
                if reply_needed.len() < limit && probe_reply_needed_thread_is_fresh(&row) =>
            {
                reply_needed.push(row);
            }
            ThreadStatus::Recoverable if recoverable.len() < limit => recoverable.push(row),
            _ => {}
        }
    }
    (running, reply_needed, recoverable)
}

pub fn aggregate_probe_status(config: &Config) -> ProbeStatusAggregation {
    let limit = config.probe.recent_limit.clamp(1, 200);
    let (running_threads, reply_needed_threads, recoverable_threads) =
        probe_codex_buckets(config, limit);
    let active_sessions = aggregate_active_sessions(
        config,
        limit,
        &running_threads,
        &reply_needed_threads,
        &recoverable_threads,
    );
    ProbeStatusAggregation {
        recent_event_count: recent_probe_event_count(&config.paths.db_path, limit as u32)
            .unwrap_or(0),
        running_threads,
        reply_needed_threads,
        recoverable_threads,
        active_sessions,
    }
}

pub fn probe_threads_for_status(
    config: &Config,
    status: &str,
    limit: usize,
) -> Result<Vec<ThreadSummary>> {
    let resolved = codex::resolve_codex_paths(&config.codex.home);
    probe_threads_for_status_with_paths(
        &resolved.codex_paths(),
        &config.paths.db_path,
        status,
        limit,
    )
}

pub fn probe_threads_for_status_with_paths(
    paths: &CodexPaths,
    panel_db_path: &Path,
    status: &str,
    limit: usize,
) -> Result<Vec<ThreadSummary>> {
    let limit = limit.clamp(1, 200);
    let hidden_thread_ids = codex::hidden_thread_ids(paths).unwrap_or_default();
    let archived_thread_ids = codex::archived_thread_ids(paths).unwrap_or_default();
    let threads = codex::list_threads(paths, false, None, usize::MAX)?;
    let running_jobs = running_thread_jobs(panel_db_path).unwrap_or_default();
    let mut threads = threads::apply_thread_list_runtime_state(
        threads,
        ThreadListRuntimeState {
            running_jobs: &running_jobs,
            hidden_thread_ids: &hidden_thread_ids,
            archived_thread_ids: &archived_thread_ids,
        },
    );
    if status == "reply-needed" {
        threads.retain(probe_reply_needed_thread_is_fresh);
    }
    threads.retain(|row| {
        matches!(
            (status, &row.status),
            ("running", ThreadStatus::Running)
                | ("reply-needed", ThreadStatus::ReplyNeeded)
                | ("recoverable", ThreadStatus::Recoverable)
                | ("recent", ThreadStatus::Recent)
        )
    });
    ThreadSummary::sort_by_message_time(&mut threads);
    threads.truncate(limit);
    Ok(threads)
}

pub fn probe_passive_thread_notification_plan(
    thread: &ThreadSummary,
    status: &str,
) -> ProbeThreadNotificationPlan {
    let (body, body_source) = probe_thread_notification_body(thread, status);
    let fresh = probe_thread_passive_bark_fresh(thread, status, body_source.as_deref());
    ProbeThreadNotificationPlan {
        body,
        body_source,
        reason_label: probe_passive_reason_label(status).map(str::to_string),
        fresh,
    }
}

fn probe_passive_reason_label(status: &str) -> Option<&'static str> {
    match status {
        "reply-needed" => Some("等待用户确认"),
        "recoverable" => Some("异常/可恢复"),
        _ => None,
    }
}

pub fn probe_event_bark_switch_enabled(config: &Config, kind: &str) -> bool {
    if !config.probe.notifications.notify_codex {
        return false;
    }
    match kind {
        "completion" => config.probe.notifications.notify_completion,
        "reply-needed" => config.probe.notifications.notify_reply_needed,
        "recoverable" => config.probe.notifications.notify_recoverable,
        _ => config.probe.notifications.notify_completion,
    }
}

pub fn probe_bark_delivery_decision(
    config: &Config,
    kind: &str,
    device_key_configured: bool,
    dedupe_claimed: bool,
) -> ProbeBarkDeliveryDecision {
    let relevant_switch_enabled = probe_event_bark_switch_enabled(config, kind);
    let notifications_enabled = config.probe.notifications.enabled;
    let skip_reason = if !notifications_enabled {
        Some("notifications_disabled")
    } else if !relevant_switch_enabled {
        Some("event_switch_disabled")
    } else if !device_key_configured {
        Some("device_key_missing")
    } else if !dedupe_claimed {
        Some("dedupe")
    } else {
        None
    };
    ProbeBarkDeliveryDecision {
        notifications_enabled,
        relevant_switch_enabled,
        device_key_configured,
        should_send: skip_reason.is_none(),
        skip_reason: skip_reason.map(str::to_string),
    }
}

pub fn probe_bark_status_label(sent: bool, skipped: bool, reason: Option<&str>) -> &'static str {
    if sent {
        "sent"
    } else if skipped && reason == Some("dedupe") {
        "dedupe_hit"
    } else if skipped {
        "skipped"
    } else {
        "failed"
    }
}

pub fn probe_event_record_plan(mut event: probe_core::ProbeBuiltEvent) -> ProbeEventRecordPlan {
    normalize_probe_event_dedupe_key(&mut event);
    let passive_marker_key = probe_passive_unresolved_action_marker_key(&event);
    let mut duplicate_outcome = probe_core::ProbeEventOutcome::from_claim(&event, false);
    duplicate_outcome.recorded = false;
    duplicate_outcome.duplicate = true;
    ProbeEventRecordPlan {
        event,
        passive_marker_key,
        duplicate_outcome,
    }
}

pub fn probe_event_record_write_plan(
    event: &probe_core::ProbeBuiltEvent,
    claimed: bool,
    bark: &impl Serialize,
    bark_status: &str,
) -> Result<ProbeEventRecordWritePlan> {
    let mut outcome = probe_core::ProbeEventOutcome::from_claim(event, claimed);
    if !claimed {
        outcome.recorded = false;
        outcome.duplicate = true;
        return Ok(ProbeEventRecordWritePlan {
            outcome,
            record: None,
            passive_marker: None,
        });
    }

    let mut payload = event.payload.clone();
    merge_probe_bark_payload(&mut payload, bark)?;
    payload["dedupe"] = json!({
        "namespace": &event.dedupe_namespace,
        "key": &event.dedupe_key,
        "claimed": claimed,
        "duplicate": !claimed,
        "status": "claimed",
    });
    payload["bark_status"] = json!(bark_status);
    payload["dedupe_status"] = json!("claimed");

    let passive_marker =
        probe_passive_unresolved_action_marker_key(event).map(|key| ProbePassiveMarkerWrite {
            key,
            value: json!({
                "dedupe_key": event.dedupe_key,
                "thread_id": event.thread_id,
                "turn_id": event.turn_id,
                "body_source": event.payload.get("body_source").and_then(Value::as_str),
                "body_sha256": event.payload.get("body_sha256").and_then(Value::as_str),
            }),
        });

    Ok(ProbeEventRecordWritePlan {
        outcome,
        record: Some(ProbeEventRecordWrite {
            kind: event.kind.clone(),
            thread_id: event.thread_id.clone(),
            title: event.title.clone(),
            message: event.message.clone(),
            dedupe_key: event.dedupe_key.clone(),
            source: event.source.clone(),
            payload,
        }),
        passive_marker,
    })
}

pub fn normalize_probe_event_dedupe_key(event: &mut probe_core::ProbeBuiltEvent) {
    if event.kind != "reply-needed" {
        return;
    }
    match event.payload.get("body_source").and_then(Value::as_str) {
        Some("proposed_plan") => normalize_proposed_plan_dedupe_key(event),
        Some("request_user_input") => normalize_request_user_input_dedupe_key(event),
        _ => {}
    }
}

fn event_action_keys(
    event: &probe_core::ProbeBuiltEvent,
    is_plan: bool,
    prefer_item_id: bool,
) -> (String, String, String, String) {
    let thread_id = event
        .thread_id
        .as_deref()
        .or_else(|| event.payload.get("thread_id").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    let turn_id = event
        .turn_id
        .as_deref()
        .or_else(|| event.payload.get("turn_id").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    let (primary, secondary) = if prefer_item_id {
        ("item_id", "call_id")
    } else {
        ("call_id", "item_id")
    };
    let action_id = event
        .payload
        .get(primary)
        .and_then(Value::as_str)
        .or_else(|| event.payload.get(secondary).and_then(Value::as_str))
        .unwrap_or(&turn_id)
        .to_string();
    let content_key = if is_plan {
        proposed_plan_hash_from_text(&event.bark_body)
    } else {
        event.payload["question_content_hash"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| request_user_input_hash_from_text(&event.bark_body))
    }
    .or_else(|| {
        event
            .payload
            .get("body_sha256")
            .and_then(Value::as_str)
            .and_then(|value| value.get(..16))
            .map(str::to_owned)
    })
    .unwrap_or_else(|| "unknown".to_string());
    (thread_id, turn_id, action_id, content_key)
}

pub fn probe_passive_unresolved_action_marker_key(
    event: &probe_core::ProbeBuiltEvent,
) -> Option<String> {
    if event.kind != "reply-needed" {
        return None;
    }
    if !matches!(
        event.payload.get("scan_source").and_then(Value::as_str),
        Some("passive-scan" | "pre-tool-use-hook")
    ) {
        return None;
    }
    let body_source = event.payload.get("body_source").and_then(Value::as_str)?;
    if !matches!(body_source, "proposed_plan" | "request_user_input") {
        return None;
    }
    let (thread_id, turn_id, action_id, content_key) =
        event_action_keys(event, body_source == "proposed_plan", true);
    Some(format!(
        "probe_passive_sent_marker:{}:{}:{}:{}:{}:{}",
        dedupe_component(body_source),
        dedupe_component(&thread_id),
        dedupe_component(&turn_id),
        dedupe_component(&action_id),
        dedupe_component(&content_key),
        dedupe_component(&event.kind),
    ))
}

impl ProbeAction {
    pub fn as_rpc_action(self) -> &'static str {
        match self {
            Self::BarkTest => commands::PROBE_BARK_TEST,
            Self::InstallHooks => commands::PROBE_INSTALL_HOOKS,
            Self::RemoveHooks => commands::PROBE_REMOVE_HOOKS,
        }
    }

    pub fn as_desktop_command(self) -> &'static str {
        self.as_rpc_action()
    }
}

impl FromStr for ProbeAction {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim() {
            "barkTest" | "bark-test" => Ok(Self::BarkTest),
            "installHooks" | "hooks-install" => Ok(Self::InstallHooks),
            "removeHooks" | "hooks-remove" => Ok(Self::RemoveHooks),
            action => Err(anyhow!("unknown probe action: {action}")),
        }
    }
}

fn probe_action_capability(action: ProbeAction) -> Capability {
    match action {
        ProbeAction::BarkTest | ProbeAction::InstallHooks | ProbeAction::RemoveHooks => {
            Capability::Probe
        }
    }
}

fn merge_probe_bark_payload(payload: &mut Value, bark: &impl Serialize) -> Result<()> {
    let outcome = serde_json::to_value(bark)?;
    if let Some(existing) = payload.get_mut("bark").and_then(Value::as_object_mut) {
        if let Some(outcome_object) = outcome.as_object() {
            for (key, value) in outcome_object {
                existing.insert(key.clone(), value.clone());
            }
        }
    } else {
        payload["bark"] = outcome;
    }
    Ok(())
}

fn probe_thread_notification_body(
    thread: &ThreadSummary,
    status: &str,
) -> (Option<String>, Option<String>) {
    if status == "reply-needed" {
        if let Some(elicitation) = &thread.pending_elicitation {
            if !thread_rollout_still_request_user_input_needed(thread) {
                return (None, None);
            }
            return (
                Some(format_probe_pending_elicitation(elicitation)),
                Some("request_user_input".to_string()),
            );
        }
        if let Some(message) = thread
            .latest_message
            .as_deref()
            .filter(|value| value.contains("<proposed_plan>") && value.contains("</proposed_plan>"))
        {
            if !thread_rollout_still_reply_needed(thread) {
                return (None, None);
            }
            return (
                Some(format_proposed_plan_reply_needed(message)),
                Some("proposed_plan".to_string()),
            );
        }
        if let Some(path) = thread.rollout_path.as_deref() {
            if let Ok(Some(message)) =
                codex::rollout_completion_last_agent_message(path, thread.active_turn_id.as_deref())
            {
                if message.contains("<proposed_plan>") && message.contains("</proposed_plan>") {
                    if !thread_rollout_still_reply_needed(thread) {
                        return (None, None);
                    }
                    return (
                        Some(format_proposed_plan_reply_needed(&message)),
                        Some("proposed_plan".to_string()),
                    );
                }
            }
        }
    }
    if status == "recoverable" {
        if let Some(message) = thread.latest_message.as_deref() {
            return (
                Some(message.to_string()),
                Some("latest_exception".to_string()),
            );
        }
    }
    (
        thread.latest_message.clone(),
        thread
            .latest_message
            .as_ref()
            .map(|_| "latest_message".to_string()),
    )
}

fn probe_thread_passive_bark_fresh(
    thread: &ThreadSummary,
    status: &str,
    body_source: Option<&str>,
) -> bool {
    if status != "reply-needed" {
        return true;
    }
    if !thread_updated_within(thread, PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS) {
        return false;
    }
    match body_source {
        Some("request_user_input") => thread_rollout_still_request_user_input_needed(thread),
        Some("proposed_plan") => thread_rollout_still_reply_needed(thread),
        Some(_) | None => false,
    }
}

fn thread_rollout_still_reply_needed(thread: &ThreadSummary) -> bool {
    if thread.rollout_path.is_none() {
        return true;
    }
    let mut refreshed = thread.clone();
    if codex::enrich_thread_from_rollout(&mut refreshed).is_err() {
        return true;
    }
    let reply_needed = matches!(refreshed.status, ThreadStatus::ReplyNeeded);
    let has_plan_message = refreshed.latest_message.as_deref().is_some_and(|value| {
        value.contains("<proposed_plan>") && value.contains("</proposed_plan>")
    });
    let same_turn = refreshed
        .active_turn_id
        .as_deref()
        .is_some_and(|turn_id| Some(turn_id) == thread.active_turn_id.as_deref())
        || (thread.active_turn_id.is_none() && refreshed.active_turn_id.is_none());
    reply_needed && has_plan_message && same_turn
}

fn thread_rollout_still_request_user_input_needed(thread: &ThreadSummary) -> bool {
    if thread.rollout_path.is_none() {
        return thread.pending_elicitation.is_some();
    }
    let mut refreshed = thread.clone();
    if codex::enrich_thread_from_rollout(&mut refreshed).is_err() {
        return false;
    }
    let reply_needed = matches!(refreshed.status, ThreadStatus::ReplyNeeded);
    let has_pending_elicitation = refreshed.pending_elicitation.is_some();
    let same_turn = refreshed
        .active_turn_id
        .as_deref()
        .is_some_and(|turn_id| Some(turn_id) == thread.active_turn_id.as_deref())
        || (thread.active_turn_id.is_none() && refreshed.active_turn_id.is_none());
    reply_needed && has_pending_elicitation && same_turn
}

pub fn format_probe_pending_elicitation(elicitation: &codex::PendingElicitation) -> String {
    let mut lines = Vec::new();
    for (index, question) in elicitation.questions.iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        let number = index + 1;
        lines.push(format!("问题 {number}：{}", question.question.trim()));
        if let Some(header) = question
            .header
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("标题：{header}"));
        }
        for (option_index, option) in question.options.iter().enumerate() {
            let marker = option_index + 1;
            lines.push(format!("选项 {marker}：{}", option.label.trim()));
            if let Some(description) = option
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                lines.push(format!("说明：{description}"));
            }
        }
    }
    if lines.is_empty() {
        "Codex 请求用户输入。".to_string()
    } else {
        lines.join("\n")
    }
}

fn format_proposed_plan_reply_needed(raw: &str) -> String {
    let plan_text =
        codex::extract_proposed_plan_text(raw).unwrap_or_else(|| raw.trim().to_string());
    plan_text.trim().to_string()
}

fn normalize_proposed_plan_dedupe_key(event: &mut probe_core::ProbeBuiltEvent) {
    let (thread_id, turn_id, item_or_call_id, plan_hash) = event_action_keys(event, true, true);
    event.dedupe_key = format!(
        "{}:{}:{}:{}:{}",
        dedupe_component(&event.kind),
        dedupe_component(&thread_id),
        dedupe_component(&turn_id),
        dedupe_component(&item_or_call_id),
        dedupe_component(&format!("plan_hash:{plan_hash}")),
    );
    event.payload["dedupe_plan_hash"] = json!(plan_hash);
    event.payload["dedupe_item_or_call_id"] = json!(item_or_call_id);
}

fn normalize_request_user_input_dedupe_key(event: &mut probe_core::ProbeBuiltEvent) {
    let (thread_id, turn_id, call_id, input_hash) = event_action_keys(event, false, false);
    event.dedupe_key = format!(
        "{}:{}:{}:{}:{}",
        dedupe_component(&event.kind),
        dedupe_component(&thread_id),
        dedupe_component(&turn_id),
        dedupe_component(&call_id),
        dedupe_component(&format!("input_hash:{input_hash}")),
    );
    event.payload["dedupe_input_hash"] = json!(input_hash);
    event.payload["dedupe_item_or_call_id"] = json!(call_id);
}

fn proposed_plan_hash_from_text(value: &str) -> Option<String> {
    let plan = codex::extract_proposed_plan_text(value)
        .or_else(|| {
            value
                .split_once("Plan 摘要:")
                .map(|(_, plan)| plan.trim().to_string())
                .filter(|plan| !plan.is_empty())
        })
        .or_else(|| {
            value
                .split_once("待回复内容：")
                .map(|(_, plan)| plan.trim().to_string())
                .filter(|plan| !plan.is_empty())
        })?;
    Some(stable_hash64_hex(plan.trim()))
}

fn request_user_input_hash_from_text(value: &str) -> Option<String> {
    let body = value
        .split_once("待回复内容：")
        .map(|(_, body)| body)
        .unwrap_or(value);
    let normalized = body
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.starts_with("时间：")
                && !line.starts_with("时间:")
                && !line.starts_with("事件时间：")
                && !line.starts_with("事件时间:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = normalized.trim();
    (!normalized.is_empty()).then(|| stable_hash64_hex(normalized))
}

fn stable_hash64_hex(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
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

fn probe_fixed_job_spec(
    platform: &PlatformPaths,
    action: ProbeAction,
    config_path: &Path,
) -> Result<ProbeFixedJobSpec> {
    let (kind, title, args, exclusive_group) = match action {
        ProbeAction::BarkTest => (
            "probe_bark_test",
            "AgentHub 推送测试",
            vec!["probe", "bark-test"],
            "probe_bark",
        ),
        ProbeAction::InstallHooks => (
            "probe_hooks_install",
            "探针 Hook 安装",
            vec!["probe", "hooks-install"],
            "probe_hooks",
        ),
        ProbeAction::RemoveHooks => (
            "probe_hooks_remove",
            "探针 Hook 移除",
            vec!["probe", "hooks-remove"],
            "probe_hooks",
        ),
    };
    let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
    Ok(ProbeFixedJobSpec {
        kind: kind.to_string(),
        title: title.to_string(),
        command: fixed_probe_shell_command(platform, config_path, &args),
        args,
        exclusive_group: Some(exclusive_group.to_string()),
    })
}

pub fn probe_process_command(
    platform: &PlatformPaths,
    config_path: &Path,
    action: ProbeAction,
) -> crate::jobs::ProcessCommand {
    let subcommand = match action {
        ProbeAction::BarkTest => "bark-test",
        ProbeAction::InstallHooks => "hooks-install",
        ProbeAction::RemoveHooks => "hooks-remove",
    };
    crate::jobs::ProcessCommand::new(
        platform.daemon_binary(),
        [
            std::ffi::OsString::from("--config"),
            config_path.as_os_str().to_owned(),
            "probe".into(),
            subcommand.into(),
        ],
    )
}

fn fixed_probe_shell_command(
    platform: &PlatformPaths,
    config_path: &Path,
    args: &[String],
) -> String {
    let mut parts = vec![
        platform.daemon_binary().display().to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
    ];
    parts.extend(args.iter().cloned());
    parts
        .iter()
        .map(|part| shell_quote(part, QuoteStyle::MinimalWithEquals))
        .collect::<Vec<_>>()
        .join(" ")
}

fn recent_probe_event_count(path: &Path, limit: u32) -> rusqlite::Result<usize> {
    let conn = open_readonly_panel_db(path)?;
    if !table_exists(&conn, "probe_events")? {
        return Ok(0);
    }
    conn.query_row(
        "SELECT COUNT(*) FROM (
            SELECT 1 FROM probe_events ORDER BY created_at DESC, rowid DESC LIMIT ?1
        )",
        params![limit.clamp(1, 500)],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count.max(0) as usize)
}

fn running_thread_jobs(path: &Path) -> rusqlite::Result<Vec<JobRecord>> {
    let conn = open_readonly_panel_db(path)?;
    if !table_exists(&conn, "jobs")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        r#"
        SELECT id, kind, status, title, thread_id, turn_id, started_at, finished_at, exit_code,
               substr(output, 1, 24000), error
        FROM jobs
        WHERE status='running' AND thread_id IS NOT NULL
        ORDER BY started_at DESC
        "#,
    )?;
    let rows = stmt.query_map([], job_from_row)?;
    rows.collect()
}

fn open_readonly_panel_db(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn table_exists(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        params![name],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
}

fn probe_reply_needed_thread_is_fresh(thread: &ThreadSummary) -> bool {
    if !matches!(thread.status, ThreadStatus::ReplyNeeded) {
        return false;
    }
    if !thread_updated_within(thread, PROBE_REPLY_NEEDED_FRESH_WINDOW_SECONDS) {
        return false;
    }
    thread.pending_elicitation.is_some()
        || thread.latest_message.as_deref().is_some_and(|value| {
            value.contains("<proposed_plan>")
                || value.contains("</proposed_plan>")
                || !value.trim().is_empty()
        })
}

fn thread_updated_within(thread: &ThreadSummary, max_age_seconds: i64) -> bool {
    let Some(updated_at) = thread
        .activity_updated_at
        .as_deref()
        .or(thread.updated_at.as_deref())
    else {
        return false;
    };
    let Ok(updated_at) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return false;
    };
    let age_seconds = Utc::now()
        .signed_duration_since(updated_at.with_timezone(&Utc))
        .num_seconds();
    (0..=max_age_seconds).contains(&age_seconds)
}

fn job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRecord> {
    Ok(JobRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        status: row.get(2)?,
        title: row.get(3)?,
        thread_id: row.get(4)?,
        turn_id: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        exit_code: row.get(8)?,
        output: row.get(9)?,
        error: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        plan_probe_action, probe_bark_delivery_decision, probe_bark_status_label,
        probe_passive_thread_notification_plan, probe_status_with_capability, ProbeAction,
        ProbeExecutionKind,
    };
    use crate::{
        codex::{ThreadStatus, ThreadSummary},
        config::Config,
        platform::{PlatformKind, PlatformPaths},
        services::system::Capability,
    };

    #[test]
    fn probe_status_facade_requires_probe_capability() {
        let config = Config::for_platform_kind(PlatformKind::Linux);
        let platform = PlatformPaths::for_kind(PlatformKind::Linux);

        let status = probe_status_with_capability(&config, &platform)
            .expect("Linux should allow probe facade");

        assert_eq!(status.required_capability, Capability::Probe);
    }

    #[test]
    fn probe_status_facade_rejects_unsupported_platform() {
        let config = Config::for_platform_kind(PlatformKind::Windows);
        let platform = PlatformPaths::for_kind(PlatformKind::Windows);

        let err = probe_status_with_capability(&config, &platform)
            .expect_err("Windows should not expose probe facade");

        assert!(err.to_string().contains("probe is unavailable on windows"));
    }

    #[test]
    fn probe_action_plans_are_platform_scoped_and_include_execution_contracts() {
        let linux_config = Config::for_platform_kind(PlatformKind::Linux);
        let linux = PlatformPaths::for_kind(PlatformKind::Linux);
        let bark = plan_probe_action(&linux_config, &linux, ProbeAction::BarkTest)
            .expect("Linux should allow fixed Probe jobs");

        assert_eq!(bark.required_capability, Capability::Probe);
        assert_eq!(bark.execution, ProbeExecutionKind::FixedShellJob);
        assert_eq!(bark.job.as_ref().unwrap().kind, "probe_bark_test");

        let windows_config = Config::for_platform_kind(PlatformKind::Windows);
        let windows = PlatformPaths::for_kind(PlatformKind::Windows);
        let bark_err = plan_probe_action(&windows_config, &windows, ProbeAction::BarkTest)
            .expect_err("Windows should not allow Probe actions");
        assert!(bark_err
            .to_string()
            .contains("probe is unavailable on windows"));
    }

    #[test]
    fn bark_delivery_and_status_labels_are_core_defined() {
        let mut config = Config::for_platform_kind(PlatformKind::Linux);
        config.probe.notifications.enabled = true;
        config.probe.notifications.notify_reply_needed = false;

        let disabled = probe_bark_delivery_decision(&config, "reply-needed", true, true);
        assert!(!disabled.should_send);
        assert_eq!(
            disabled.skip_reason.as_deref(),
            Some("event_switch_disabled")
        );
        assert_eq!(
            probe_bark_status_label(false, true, Some("dedupe")),
            "dedupe_hit"
        );
        assert_eq!(
            probe_bark_status_label(false, true, Some("sent_marker")),
            "skipped"
        );
        assert_eq!(
            probe_bark_status_label(false, false, Some("http_status")),
            "failed"
        );

        config.probe.notifications.notify_reply_needed = true;
        assert!(probe_bark_delivery_decision(&config, "reply-needed", true, true).should_send);
        assert_eq!(
            probe_bark_delivery_decision(&config, "reply-needed", false, true)
                .skip_reason
                .as_deref(),
            Some("device_key_missing")
        );
    }

    #[test]
    fn passive_notification_plans_are_core_defined() {
        let thread = ThreadSummary {
            activity_updated_at: None,
            storage_size: None,
            id: "thread-a".to_string(),
            title: "Thread A".to_string(),
            status: ThreadStatus::Recoverable,
            updated_at: None,
            archived_at: None,
            message_count: 1,
            latest_message: Some("failed".to_string()),
            cwd: None,
            model: None,
            rollout_path: None,
            active_turn_id: None,
            active_job_id: None,
            pending_elicitation: None,
            last_event_kind: None,
        };
        let plan = probe_passive_thread_notification_plan(&thread, "recoverable");
        assert_eq!(plan.reason_label.as_deref(), Some("异常/可恢复"));
    }

    #[test]
    fn aggregate_probe_status_collects_multi_provider_active_sessions_and_remove_hooks_plan() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nexushub-probe-agg-{unique}"));
        let codex_home = root.join(".codex");
        let grok_session_id = "11111111-2222-4333-8444-555555555555";
        let grok_dir = root
            .join(".grok/sessions/workspace-a")
            .join(grok_session_id);
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::create_dir_all(&grok_dir).unwrap();
        let now_rfc3339 = chrono::Utc::now().to_rfc3339();
        let now_ms = chrono::Utc::now().timestamp_millis();
        std::fs::write(
            grok_dir.join("summary.json"),
            serde_json::json!({
                "info": {
                    "id": grok_session_id,
                    "title": "Grok Question Session",
                    "cwd": "/workspace/demo",
                    "updatedAt": now_rfc3339
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            grok_dir.join("events.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::json!({
                    "type": "turn_started",
                    "ts": now_ms - 500,
                    "session_id": grok_session_id,
                    "session_relationship": "primary",
                    "turn_number": 1
                }),
                serde_json::json!({
                    "type": "turn_ended",
                    "ts": now_ms,
                    "outcome": "completed"
                })
            ),
        )
        .unwrap();
        std::fs::write(
            grok_dir.join("updates.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::json!({
                    "timestamp": now_ms - 200,
                    "params": {
                        "sessionId": grok_session_id,
                        "_meta": { "promptId": "turn-1", "chunkId": "chunk-1" },
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": "请选择需要部署的目标环境：测试环境还是预发环境？" }
                        }
                    }
                }),
                serde_json::json!({
                    "timestamp": now_ms,
                    "params": {
                        "sessionId": grok_session_id,
                        "update": {
                            "sessionUpdate": "turn_completed",
                            "prompt_id": "turn-1",
                            "stop_reason": "end_turn"
                        }
                    }
                })
            ),
        )
        .unwrap();

        let mut config = Config::default();
        config.codex.home = codex_home;
        config.paths.db_path = root.join("panel.sqlite");
        let agg = super::aggregate_probe_status(&config);
        assert!(
            agg.active_sessions.iter().any(|s| s.provider == "grok"
                && s.id == grok_session_id
                && s.bucket == "reply_needed"),
            "expected grok reply_needed in active_sessions, got {:?}",
            agg.active_sessions
        );

        let platform =
            PlatformPaths::for_kind_with_home(crate::platform::PlatformKind::Linux, &root);
        let remove_plan = plan_probe_action(&config, &platform, ProbeAction::RemoveHooks).unwrap();
        assert_eq!(remove_plan.action, ProbeAction::RemoveHooks);
        assert_eq!(
            remove_plan.job.as_ref().map(|j| j.kind.as_str()),
            Some("probe_hooks_remove")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn live_provider_sessions_cache_is_keyed_by_sandbox_root_and_codex_home_and_evicts_on_idle() {
        super::evict_idle_live_provider_sessions_cache(std::time::Duration::ZERO);
        let root_a = std::env::temp_dir().join(format!(
            "nexushub-probe-cache-a-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let root_b = std::env::temp_dir().join(format!(
            "nexushub-probe-cache-b-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let now_rfc3339 = chrono::Utc::now().to_rfc3339();
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Seed Grok reply_needed in root_a only
        let grok_id_a = "aaaaaaaa-2222-4333-8444-555555555555";
        let grok_dir_a = root_a.join(".grok/sessions/workspace-a").join(grok_id_a);
        std::fs::create_dir_all(&grok_dir_a).unwrap();
        std::fs::write(
            grok_dir_a.join("summary.json"),
            serde_json::json!({
                "info": {
                    "id": grok_id_a,
                    "title": "Grok A",
                    "cwd": "/workspace/a",
                    "updatedAt": now_rfc3339
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            grok_dir_a.join("events.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::json!({
                    "type": "turn_started",
                    "ts": now_ms - 500,
                    "session_id": grok_id_a,
                    "session_relationship": "primary",
                    "turn_number": 1
                }),
                serde_json::json!({
                    "type": "turn_ended",
                    "ts": now_ms,
                    "outcome": "completed"
                })
            ),
        )
        .unwrap();
        std::fs::write(
            grok_dir_a.join("updates.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::json!({
                    "timestamp": now_ms - 200,
                    "params": {
                        "sessionId": grok_id_a,
                        "_meta": { "promptId": "turn-1", "chunkId": "chunk-1" },
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": "请选择需要部署的目标环境：测试环境还是预发环境？" }
                        }
                    }
                }),
                serde_json::json!({
                    "timestamp": now_ms,
                    "params": {
                        "sessionId": grok_id_a,
                        "update": {
                            "sessionUpdate": "turn_completed",
                            "prompt_id": "turn-1",
                            "stop_reason": "end_turn"
                        }
                    }
                })
            ),
        )
        .unwrap();
        std::fs::create_dir_all(root_b.join(".codex")).unwrap();

        let mut cfg_a = Config::default();
        cfg_a.codex.home = root_a.join(".codex");
        cfg_a.paths.db_path = root_a.join("panel.sqlite");

        let mut cfg_b = Config::default();
        cfg_b.codex.home = root_b.join(".codex");
        cfg_b.paths.db_path = root_b.join("panel.sqlite");

        let sessions_a = super::aggregate_active_sessions(&cfg_a, 20, &[], &[], &[]);
        assert!(sessions_a.iter().any(|s| s.id == grok_id_a));

        // Querying with cfg_b must NOT hit cfg_a's cached sessions
        let sessions_b = super::aggregate_active_sessions(&cfg_b, 20, &[], &[], &[]);
        assert!(!sessions_b.iter().any(|s| s.id == grok_id_a));

        crate::evict_idle_detail_caches(std::time::Duration::ZERO);
        assert!(super::LIVE_PROVIDER_SESSIONS_CACHE
            .lock()
            .unwrap()
            .is_none());

        let _ = std::fs::remove_dir_all(&root_a);
        let _ = std::fs::remove_dir_all(&root_b);
    }
}
