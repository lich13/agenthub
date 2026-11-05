//! Durable Bark delivery claims and bounded retries; credentials stay outside the queue.
use super::{
    async_question_monitor, bark, question_monitor, task_notification_suppression_reason,
    ProbeBarkOutcome, ProbeBarkRequest,
};
use agenthub_core::{
    claude::ClaudeNotificationState,
    codex,
    config::Config,
    db::{NativeDelivery, NotificationChannel, PanelDb},
    native_probe,
    probe::ProbeBuiltEvent,
    services::{probe as probe_service, settings::PROBE_BARK_DEVICE_KEY_SETTING},
};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
enum Source {
    Test,
    Codex {
        event: Box<ProbeBuiltEvent>,
        fingerprint: Option<SourceProof>,
        #[serde(default)]
        original_body: Option<String>,
    },
    Native {
        delivery: NativeDelivery,
    },
    ClaudeHook {
        hook: super::claude_hook::HookEvent,
        proof: Option<SourceProof>,
        verified: bool,
    },
}

#[derive(Clone, Serialize, Deserialize)]
struct Candidate {
    source: Source,
    request: ProbeBarkRequest,
    event_ms: i64,
    chunks: Vec<String>,
    instance_id: String,
    target_fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct SourceProof {
    identity: String,
    length: u64,
    digest: String,
    #[serde(default)]
    record_count: Option<u64>,
}

fn codex_path(config: &Config, event: &ProbeBuiltEvent) -> Option<PathBuf> {
    let paths = codex::resolve_codex_paths(&config.codex.home).codex_paths();
    codex::notification_thread_headers(&paths)
        .ok()?
        .into_iter()
        .find(|t| Some(&t.id) == event.thread_id.as_ref())?
        .rollout_path
}

fn prefix_digest(path: &Path, length: u64) -> Option<String> {
    if length > 512 * 1024 * 1024 {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?.take(length);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 32768];
    let mut count = 0;
    loop {
        let size = file.read(&mut buffer).ok()?;
        if size == 0 {
            break;
        }
        count += size as u64;
        digest.update(&buffer[..size]);
    }
    (count == length).then(|| hex::encode(digest.finalize()))
}

fn codex_fingerprint(config: &Config, event: &ProbeBuiltEvent) -> Option<SourceProof> {
    let path = codex_path(config, event)?;
    let identity = native_probe::file_identity(&path).ok()?;
    let length = std::fs::metadata(&path).ok()?.len();
    // Preserve the existing Codex source-validation bound. Claude's hook
    // verifier shares the digest reader but has its own larger transcript limit.
    if length > 128 * 1024 * 1024 {
        return None;
    }
    let digest = prefix_digest(&path, length)?;
    Some(SourceProof {
        identity,
        length,
        digest,
        record_count: None,
    })
}

fn claude_proof(
    hook: &super::claude_hook::HookEvent,
    context: &agenthub_core::claude::ClaudeNotificationContext,
) -> Result<SourceProof> {
    let length = std::fs::metadata(&hook.path)?.len();
    let proof = SourceProof {
        identity: context.stream.identity.clone(),
        length,
        digest: prefix_digest(&hook.path, length)
            .ok_or_else(|| anyhow::anyhow!("source proof pending"))?,
        record_count: Some(context.stream.record_count),
    };
    anyhow::ensure!(same_source(&hook.path, &proof), "source changed");
    Ok(proof)
}

fn same_source(path: &Path, proof: &SourceProof) -> bool {
    native_probe::file_identity(path).ok().as_deref() == Some(&proof.identity)
        && prefix_digest(path, proof.length).as_deref() == Some(&proof.digest)
}

fn target_fingerprint(config: &Config, secret: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(config.probe.notifications.server_url.trim().as_bytes());
    hash.update([0]);
    hash.update(secret);
    hex::encode(hash.finalize())
}

fn queued_candidate(
    config: &Config,
    db: &PanelDb,
    source: Source,
    request: ProbeBarkRequest,
    event_ms: i64,
) -> Result<Candidate> {
    let secret = db
        .get_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING)?
        .unwrap_or_default();
    Ok(Candidate {
        source,
        chunks: vec![request.body.clone()],
        request,
        event_ms,
        instance_id: db.notification_instance_id()?,
        target_fingerprint: target_fingerprint(config, &secret),
    })
}

pub fn codex_key(event: &ProbeBuiltEvent) -> String {
    async_question_monitor::delivery_key(event)
        .or_else(|| question_monitor::delivery_key(event))
        .unwrap_or_else(|| format!("codex:{}:{}", event.dedupe_namespace, event.dedupe_key))
}

pub async fn codex(
    config: &Config,
    db: &PanelDb,
    event: &ProbeBuiltEvent,
    bark_allowed: bool,
    timeout: Duration,
) -> Result<ProbeBarkOutcome> {
    db.sync_notification_channels(config)?;
    let key = codex_key(event);
    let title = event
        .payload
        .get("notification_title")
        .unwrap_or(&event.payload["thread_title"])
        .as_str()
        .unwrap_or("");
    let project = if title.trim().is_empty() || matches!(title.trim(), "未命名会话" | "未命名线程")
    {
        let paths = codex::resolve_codex_paths(&config.codex.home).codex_paths();
        event
            .thread_id
            .as_deref()
            .and_then(|id| codex::local_thread_summary(&paths, id).ok().flatten())
            .and_then(|thread| thread.cwd)
            .and_then(|cwd| {
                std::path::Path::new(&cwd)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let origin_ms = event.payload["question_created_at_ms"]
        .as_i64()
        .or_else(|| event.payload["feedback_completed_at_ms"].as_i64())
        .or_else(|| event.payload["source_log_ts"].as_i64().map(|ts| ts * 1000))
        .or_else(|| event.payload["notification_source_ms"].as_i64())
        .or(db.notification_first_seen_ms(&event.dedupe_key)?)
        .unwrap_or_else(|| Utc::now().timestamp_millis());
    let candidate = queued_candidate(
        config,
        db,
        Source::Codex {
            event: Box::new(event.clone()),
            fingerprint: codex_fingerprint(config, event),
            original_body: Some(event.bark_body.clone()),
        },
        super::notification_format::request(
            "Codex",
            &event.event_type,
            title,
            &project,
            &event.bark_body,
            &event.dedupe_key,
        ),
        origin_ms,
    )?;
    let bark = deliver(
        config,
        db,
        &key,
        NotificationChannel::Bark,
        &candidate,
        bark_allowed,
        timeout,
    )
    .await?;
    Ok(bark)
}

pub async fn native(
    config: &Config,
    db: &PanelDb,
    delivery: &NativeDelivery,
    timeout: Duration,
) -> Result<ProbeBarkOutcome> {
    db.sync_notification_channels(config)?;
    let candidate = queued_candidate(
        config,
        db,
        Source::Native {
            delivery: delivery.clone(),
        },
        super::notification_format::request(
            match delivery.provider {
                native_probe::NativeProvider::Claude => "Claude Code",
                native_probe::NativeProvider::Grok => "Grok",
                native_probe::NativeProvider::GrokBot => "Grok Bot",
                native_probe::NativeProvider::Antigravity => "Antigravity",
                native_probe::NativeProvider::Dsh => "DSH",
            },
            &delivery.event.kind,
            &delivery.title,
            "",
            &delivery.event.body,
            &delivery.event_key,
        ),
        delivery.event.timestamp_ms,
    )?;
    let key = format!("native:{}", delivery.event_key);
    let bark = deliver(
        config,
        db,
        &key,
        NotificationChannel::Bark,
        &candidate,
        true,
        timeout,
    )
    .await?;
    Ok(bark)
}

fn event_enabled(config: &Config, candidate: &Candidate) -> bool {
    match &candidate.source {
        Source::Test => true,
        Source::ClaudeHook { hook, .. } => {
            native_probe::NativeProvider::Claude.enabled(config)
                && native_probe::NativeProvider::Claude.event_enabled(config, &hook.kind)
        }
        Source::Codex { event, .. } => {
            config.probe.enabled
                && probe_service::probe_event_bark_switch_enabled(config, &event.kind)
        }
        Source::Native { delivery } => {
            (!delivery.provider.local_only() || cfg!(target_os = "macos"))
                && delivery.provider.enabled(config)
                && delivery
                    .provider
                    .event_enabled(config, &delivery.event.kind)
        }
    }
}

fn claude_aliases(db: &PanelDb, candidate: &Candidate) -> Result<Vec<String>> {
    let (context, prompt, kind) = match &candidate.source {
        Source::ClaudeHook {
            hook,
            verified: true,
            ..
        } => (
            super::claude_hook::context(hook)?,
            hook.prompt_id.clone().unwrap_or_default(),
            hook.kind.clone(),
        ),
        Source::Native { delivery }
            if delivery.provider == native_probe::NativeProvider::Claude
                && super::claude_hook::installed() =>
        {
            let context = agenthub_core::claude::notification_context_for_key(
                &agenthub_core::claude::ClaudePaths::default_for_user(),
                &delivery.session_key,
            )?;
            let turn = delivery.event.turn_id.split(':').next().unwrap_or("");
            let Some((prompt, _)) = context
                .prompts
                .iter()
                .find(|(_, p)| !p.ambiguous && p.turn_id == turn)
            else {
                return Ok(vec![]);
            };
            let prompt = prompt.clone();
            (
                context,
                prompt,
                native_probe::event_category(&delivery.event).into(),
            )
        }
        _ => return Ok(vec![]),
    };
    let Some(turn) = context.prompts.get(&prompt) else {
        return Ok(vec![]);
    };
    let mut aliases =
        db.claude_legacy_delivery_keys(&context.stream.session_key, &turn.turn_id, &kind)?;
    aliases.push(format!(
        "native:{}",
        context
            .stream
            .prompt_event_key(&prompt, &kind, &db.notification_instance_id()?)
    ));
    for event in context.stream.events.iter().filter(|e| {
        e.prompt_id.as_deref() == Some(&prompt) && native_probe::event_category(e) == kind
    }) {
        aliases.push(format!("native:{}", context.stream.event_key(event)));
        if kind == "authorization" {
            let mut legacy = event.clone();
            legacy.kind = "reply_needed".into();
            aliases.push(format!("native:{}", context.stream.event_key(&legacy)));
        }
    }
    Ok(aliases)
}

pub(super) async fn claude_hook(
    config: &Config,
    db: &PanelDb,
    hook: super::claude_hook::HookEvent,
    timeout: Duration,
) -> Result<()> {
    db.sync_notification_channels(config)?;
    if !native_probe::NativeProvider::Claude.enabled(config)
        || !native_probe::NativeProvider::Claude.event_enabled(config, &hook.kind)
    {
        return Ok(());
    }
    let key = format!(
        "claude-hook:{}",
        hex::encode(Sha256::digest(format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            db.notification_instance_id()?,
            hook.path.display(),
            hook.identity.as_deref().unwrap_or(""),
            hook.session_id,
            hook.prompt_id.as_deref().unwrap_or(""),
            hook.kind
        )))
    );
    let request =
        super::notification_format::request("Claude Code", &hook.kind, "", "", &hook.body, &key);
    let event_ms = hook.received_ms;
    let proof_hook = hook.clone();
    let proof = tokio::task::spawn_blocking(move || {
        let context = super::claude_hook::context(&proof_hook).ok()?;
        claude_proof(&proof_hook, &context).ok()
    })
    .await
    .ok()
    .flatten();
    let candidate = queued_candidate(
        config,
        db,
        Source::ClaudeHook {
            hook,
            proof,
            verified: false,
        },
        request,
        event_ms,
    )?;
    // Even a transcript that has not caught up is stored encrypted before resolution.
    if db.stage_notification_delivery(
        NotificationChannel::Bark,
        &key,
        event_ms,
        &serde_json::to_value(&candidate)?,
    )? {
        process_claude_hook(config, db, &key, &candidate, timeout).await?;
    }
    Ok(())
}

async fn process_claude_hook(
    config: &Config,
    db: &PanelDb,
    key: &str,
    candidate: &Candidate,
    timeout: Duration,
) -> Result<()> {
    let Source::ClaudeHook { hook, proof, .. } = &candidate.source else {
        return Ok(());
    };
    let Some(lease) = db.claim_notification_delivery(NotificationChannel::Bark, key)? else {
        return Ok(());
    };
    let started = Instant::now();
    let hook = hook.clone();
    let original_proof = proof.clone();
    let resolved = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut hook = hook;
        let context = super::claude_hook::context(&hook)?;
        let state = context.hook_state(
            hook.prompt_id.as_deref().unwrap_or(""),
            &hook.kind,
            hook.received_ms,
            original_proof.as_ref().and_then(|p| p.record_count),
        );
        if hook.idle && state == ClaudeNotificationState::Current {
            hook.body = context
                .stream
                .events
                .iter()
                .find(|event| event.kind == "completion" && event.prompt_id == hook.prompt_id)
                .map(|e| e.body.clone())
                .ok_or_else(|| anyhow::anyhow!("terminal evidence pending"))?;
        }
        let proof = match original_proof {
            Some(proof) => proof,
            None => claude_proof(&hook, &context)?,
        };
        anyhow::ensure!(same_source(&hook.path, &proof), "source changed");
        Ok((hook, context, proof, state))
    })
    .await;
    let (hook, context, proof, state) = match resolved {
        Ok(Ok(value)) => value,
        _ => {
            db.finish_notification_delivery(&lease, &json!({"sent":false,"skipped":false,"retryable":true,"uncertain":false,"reason":"claude_identity_pending","error_category":"source_pending"}))?;
            return Ok(());
        }
    };
    if state != ClaudeNotificationState::Current {
        let outcome = claude_source_outcome(state, true, true);
        let mut value = serde_json::to_value(&outcome)?;
        value["retryable"] = json!(outcome.retryable);
        db.finish_notification_delivery(&lease, &value)?;
        return Ok(());
    }
    let prompt = hook.prompt_id.as_deref().unwrap_or("");
    let event_ms = if hook.idle {
        let terminal = context
            .stream
            .events
            .iter()
            .find(|e| e.kind == "completion" && e.prompt_id == hook.prompt_id)
            .expect("idle terminal was resolved");
        if !db.claude_idle_after_baseline(&context.stream, terminal)? {
            db.finish_notification_delivery(
                &lease,
                &json!({"sent":false,"skipped":true,"reason":"idle_baseline"}),
            )?;
            return Ok(());
        }
        terminal.timestamp_ms
    } else {
        candidate.event_ms
    };
    let canonical = format!(
        "native:{}",
        context
            .stream
            .prompt_event_key(prompt, &hook.kind, &db.notification_instance_id()?)
    );
    let title = if context.stream.title == context.stream.id {
        ""
    } else {
        &context.stream.title
    };
    let request = super::notification_format::request(
        "Claude Code",
        &hook.kind,
        title,
        &context.project,
        &hook.body,
        &canonical,
    );
    let kind = hook.kind.clone();
    let candidate = queued_candidate(
        config,
        db,
        Source::ClaudeHook {
            hook,
            proof: Some(proof),
            verified: true,
        },
        request,
        event_ms,
    )?;
    let aliases = claude_aliases(db, &candidate)?;
    let staged = if event_enabled(config, &candidate) {
        db.stage_notification_delivery_with_aliases(
            NotificationChannel::Bark,
            &canonical,
            candidate.event_ms,
            &serde_json::to_value(&candidate)?,
            &aliases,
        )?
    } else {
        false
    };
    db.finish_notification_delivery(
        &lease,
        &json!({"sent":false,"skipped":true,"reason":"claude_identity_resolved"}),
    )?;
    if staged {
        db.record_probe_event(agenthub_core::db::NewProbeEvent {
            kind: &kind,
            thread_id: Some(&context.stream.id),
            title: Some(&context.stream.title),
            message: Some(&candidate.request.body),
            dedupe_key: Some(&canonical),
            source: "claude_hook",
            payload: json!({"provider":"claude_code","notification_event_key":canonical,"body_source":"claude_hook","body_summary":candidate.request.body,"bark_status":"queued"}),
        })?;
        process(
            &|| Ok(config.clone()),
            db,
            &canonical,
            NotificationChannel::Bark,
            &candidate,
            timeout
                .saturating_sub(started.elapsed())
                .max(Duration::from_millis(1)),
            true,
        )
        .await?;
    }
    Ok(())
}

async fn deliver(
    config: &Config,
    db: &PanelDb,
    key: &str,
    channel: NotificationChannel,
    candidate: &Candidate,
    legacy_allowed: bool,
    timeout: Duration,
) -> Result<ProbeBarkOutcome> {
    let enabled = channel.enabled(config);
    if !enabled || !event_enabled(config, candidate) {
        return Ok(ProbeBarkOutcome::skipped(
            if enabled {
                "event_switch_disabled"
            } else {
                "notifications_disabled"
            },
            enabled,
            event_enabled(config, candidate),
            false,
        ));
    }
    if !legacy_allowed {
        return Ok(ProbeBarkOutcome::skipped("dedupe", enabled, true, true));
    }
    let aliases = claude_aliases(db, candidate)?;
    let staged = db.stage_notification_delivery_with_aliases(
        channel,
        key,
        candidate.event_ms,
        &serde_json::to_value(candidate)?,
        &aliases,
    )?;
    if !staged {
        return Ok(db
            .notification_delivery_outcome(channel, key)?
            .and_then(|v| serde_json::from_value(v).ok())
            .filter(|v: &ProbeBarkOutcome| !v.sent && !v.skipped)
            .unwrap_or_else(|| ProbeBarkOutcome::skipped("dedupe", enabled, true, true)));
    }
    process(
        &|| Ok(config.clone()),
        db,
        key,
        channel,
        candidate,
        timeout,
        false,
    )
    .await
}

type ConfigLoader<'a> = dyn Fn() -> Result<Config> + Sync + 'a;

fn latest_config(load_config: &ConfigLoader<'_>) -> Result<Config> {
    // Do not expose configuration paths or values in monitor/job errors.
    load_config().map_err(|_| anyhow::anyhow!("notification configuration unavailable"))
}

async fn process(
    load_config: &ConfigLoader<'_>,
    db: &PanelDb,
    key: &str,
    channel: NotificationChannel,
    candidate: &Candidate,
    timeout: Duration,
    revalidate: bool,
) -> Result<ProbeBarkOutcome> {
    let started = Instant::now();
    let mut latest = None;
    loop {
        let checked_config = latest_config(load_config)?;
        db.sync_notification_channels(&checked_config)?;
        let Some(lease) = db.claim_notification_delivery(channel, key)? else {
            return Ok(db
                .notification_delivery_outcome(channel, key)?
                .and_then(|v| serde_json::from_value(v).ok())
                .or(latest)
                .unwrap_or_else(|| {
                    ProbeBarkOutcome::skipped(
                        "dedupe",
                        channel.enabled(&checked_config),
                        true,
                        true,
                    )
                }));
        };
        let claude_state = claude_source_state(db, candidate).await;
        let local_state = local_native_source_state(candidate).await;
        let valid_source = match claude_state {
            Some(state) => state != ClaudeNotificationState::Obsolete,
            None if local_state.is_some() => !matches!(local_state, Some(Ok(false))),
            None => {
                !(revalidate || lease.chunk_attempts > 1)
                    || current(&checked_config, db, candidate).await
            }
        };
        // Source checks can yield while a scan runs. Refresh again before any HTTP
        // request, including another segment of the same queued notification.
        let config = latest_config(load_config)?;
        db.sync_notification_channels(&config)?;
        let valid_source = valid_source
            && (!matches!(&candidate.source, Source::Codex { .. })
                || config.codex.home == checked_config.codex.home);
        let config = &config;
        let secret = db
            .get_secret_setting_bytes(PROBE_BARK_DEVICE_KEY_SETTING)?
            .unwrap_or_default();
        let valid_target = candidate.target_fingerprint == target_fingerprint(config, &secret);
        let outcome = if !channel.enabled(config)
            || !event_enabled(config, candidate)
            || !valid_target
            || !valid_source
        {
            ProbeBarkOutcome::skipped(
                "source_changed_or_disabled",
                channel.enabled(config),
                false,
                !secret.is_empty(),
            )
        } else if secret.is_empty() {
            ProbeBarkOutcome::skipped("device_key_missing", channel.enabled(config), true, false)
        } else if claude_state == Some(ClaudeNotificationState::Pending)
            || matches!(local_state, Some(Err(_)))
        {
            let mut outcome = claude_source_outcome(ClaudeNotificationState::Pending, true, true);
            if local_state.is_some() {
                outcome.reason = Some("native_source_pending".into());
            }
            outcome
        } else if candidate.request.presentation.is_some()
            && super::claude_hook::installed()
            && matches!(&candidate.source, Source::Native { delivery } if delivery.provider == native_probe::NativeProvider::Claude && delivery.event.prompt_id.is_none())
        {
            // Missing native promptId is not a license to use message uuid as
            // hook identity. Keep it in the existing bounded retry queue.
            let mut pending =
                ProbeBarkOutcome::skipped("claude_identity_pending", true, true, true);
            pending.skipped = false;
            pending.retryable = true;
            pending.error_category = Some("source_pending".into());
            pending
        } else if let Some(chunk) = candidate.chunks.get(lease.chunk_index) {
            let Some(remaining_ms) = db.notification_delivery_lease_remaining_ms(&lease)? else {
                return Ok(db
                    .notification_delivery_outcome(channel, key)?
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_else(|| {
                        ProbeBarkOutcome::skipped(
                            "delivery_lease_expired",
                            channel.enabled(config),
                            true,
                            true,
                        )
                    }));
            };
            let id = hex::encode(Sha256::digest(format!(
                "{}\0{}\0{}",
                candidate.instance_id, key, lease.chunk_index
            )));
            bark::send_chunk(
                config,
                &secret,
                &candidate.request,
                chunk,
                lease.chunk_index,
                candidate.chunks.len(),
                &id,
                lease.chunk_attempts,
                timeout
                    .saturating_sub(started.elapsed())
                    .max(Duration::from_millis(1))
                    .min(Duration::from_millis(remaining_ms as u64)),
            )
            .await
        } else {
            ProbeBarkOutcome::skipped(
                "invalid_retry_context",
                channel.enabled(config),
                false,
                true,
            )
        };
        let chunk_accepted = outcome.sent;
        let mut confirmation = serde_json::to_value(&outcome)?;
        confirmation["retryable"] = json!(outcome.retryable);
        confirmation["retry_after_ms"] = json!(outcome.retry_after_ms);
        confirmation["uncertain"] = json!(outcome.uncertain);
        let Some(value) = db.finish_notification_delivery(&lease, &confirmation)? else {
            return Ok(db
                .notification_delivery_outcome(channel, key)?
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_else(|| {
                    ProbeBarkOutcome::skipped(
                        "delivery_lease_expired",
                        channel.enabled(config),
                        true,
                        true,
                    )
                }));
        };
        let result: ProbeBarkOutcome = serde_json::from_value(value)?;
        if result.status.as_deref() != Some("waiting_retry")
            || !chunk_accepted
            || started.elapsed() >= timeout
        {
            return Ok(result);
        }
        latest = Some(result);
    }
}

/// Explicit user tests get a fresh identity and use the same durable queue.
pub async fn test(config_path: &Path, db: &PanelDb) -> Result<ProbeBarkOutcome> {
    let config = Config::load_runtime(config_path)?;
    db.sync_notification_channels(&config)?;
    let key = format!("test:{}", uuid::Uuid::new_v4());
    let candidate = queued_candidate(
        &config,
        db,
        Source::Test,
        ProbeBarkRequest {
            presentation: None,
            title: "AgentHub 推送测试".into(),
            body: agenthub_core::probe::bark_test_body().into(),
            dedupe_key: key.clone(),
        },
        Utc::now().timestamp_millis(),
    )?;
    let mut outcome = deliver(
        &config,
        db,
        &key,
        NotificationChannel::Bark,
        &candidate,
        true,
        Duration::from_secs(8),
    )
    .await?;
    let mut shown_attempt = 0;
    while matches!(
        outcome.status.as_deref(),
        Some("waiting_retry" | "retrying")
    ) {
        if shown_attempt != outcome.attempts {
            shown_attempt = outcome.attempts;
            println!("等待重试，已尝试 {} 次。", shown_attempt);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        retry_pending_with_config(db, &|| Config::load_runtime(config_path)).await?;
        if let Some(value) = db.notification_delivery_outcome(NotificationChannel::Bark, &key)? {
            outcome = serde_json::from_value(value)?;
        }
    }
    Ok(outcome)
}

#[cfg(test)]
#[path = "notification_delivery_tests.rs"]
mod tests;

fn claude_source_outcome(
    state: ClaudeNotificationState,
    enabled: bool,
    configured: bool,
) -> ProbeBarkOutcome {
    let mut outcome = ProbeBarkOutcome::skipped(
        if state == ClaudeNotificationState::Pending {
            "claude_terminal_pending"
        } else {
            "claude_event_obsolete"
        },
        enabled,
        false,
        configured,
    );
    if state == ClaudeNotificationState::Pending {
        outcome.skipped = false;
        outcome.retryable = true;
        outcome.error_category = Some("source_pending".into());
    }
    outcome
}

async fn claude_source_state(
    db: &PanelDb,
    candidate: &Candidate,
) -> Option<ClaudeNotificationState> {
    use ClaudeNotificationState::*;
    match &candidate.source {
        Source::ClaudeHook {
            hook,
            proof,
            verified,
        } => {
            let (hook, proof, verified) = (hook.clone(), proof.clone(), *verified);
            Some(
                tokio::task::spawn_blocking(move || {
                    if !verified || !proof.as_ref().is_some_and(|p| same_source(&hook.path, p)) {
                        return Obsolete;
                    }
                    let Ok(context) = super::claude_hook::context(&hook) else {
                        return Pending;
                    };
                    context.hook_state(
                        hook.prompt_id.as_deref().unwrap_or(""),
                        &hook.kind,
                        hook.received_ms,
                        proof.as_ref().and_then(|p| p.record_count),
                    )
                })
                .await
                .unwrap_or(Pending),
            )
        }
        Source::Native { delivery }
            if delivery.provider == native_probe::NativeProvider::Claude =>
        {
            let delivery = delivery.clone();
            let instance = db.notification_instance_id().unwrap_or_default();
            Some(
                tokio::task::spawn_blocking(move || {
                    let Ok(context) = agenthub_core::claude::notification_context_for_key(
                        &agenthub_core::claude::ClaudePaths::default_for_user(),
                        &delivery.session_key,
                    ) else {
                        return Pending;
                    };
                    let stream = &context.stream;
                    if stream.id != delivery.thread_id
                        || stream.session_key != delivery.session_key
                        || (delivery.event.kind == "failure"
                            && context.current_turn.as_ref() != Some(&delivery.event.turn_id))
                    {
                        return Obsolete;
                    }
                    if stream.events.iter().any(|event| {
                        native_probe::same_event(event, &delivery.event)
                            && (stream.event_key(&delivery.event) == delivery.event_key
                                || event.prompt_id.as_deref().is_some_and(|p| {
                                    stream.prompt_event_key(
                                        p,
                                        native_probe::event_category(event),
                                        &instance,
                                    ) == delivery.event_key
                                }))
                    }) {
                        Current
                    } else {
                        Obsolete
                    }
                })
                .await
                .unwrap_or(Pending),
            )
        }
        _ => None,
    }
}

async fn local_native_source_state(candidate: &Candidate) -> Option<Result<bool>> {
    let Source::Native { delivery } = &candidate.source else {
        return None;
    };
    if !delivery.provider.local_only() {
        return None;
    }
    if !cfg!(target_os = "macos") {
        return Some(Ok(false));
    }
    Some(
        if delivery.provider == native_probe::NativeProvider::Antigravity {
            super::antigravity::source_current(delivery).await
        } else {
            super::provider_monitor::scan(delivery.provider)
                .await
                .and_then(|scan| super::provider_monitor::local_event_current(&scan, delivery))
        },
    )
}

async fn current(config: &Config, db: &PanelDb, candidate: &Candidate) -> bool {
    if let Some(state) = local_native_source_state(candidate).await {
        return state.unwrap_or(false);
    }
    if let Some(state) = claude_source_state(db, candidate).await {
        return state == ClaudeNotificationState::Current;
    }
    match &candidate.source {
        Source::Test => true,
        Source::Native { delivery } => {
            let provider = delivery.provider;
            let instance = db.notification_instance_id().unwrap_or_default();
            tokio::task::spawn_blocking(move || native_probe::scan(provider))
                .await
                .ok()
                .and_then(Result::ok)
                .is_some_and(|scan| {
                    scan.streams.iter().any(|stream| {
                        native_probe::retry_event_is_current(
                            provider,
                            &delivery.thread_id,
                            &delivery.event,
                        ) && stream.session_key == delivery.session_key
                            && stream.id == delivery.thread_id
                            && stream.events.iter().any(|event| {
                                (stream.event_key(&delivery.event) == delivery.event_key
                                    || event.prompt_id.as_deref().is_some_and(|p| {
                                        stream.prompt_event_key(
                                            p,
                                            native_probe::event_category(event),
                                            &instance,
                                        ) == delivery.event_key
                                    }))
                                    && native_probe::same_event(event, &delivery.event)
                            })
                    })
                })
        }
        Source::ClaudeHook { .. } => unreachable!("Claude source checked above"),
        Source::Codex {
            event,
            fingerprint,
            original_body,
        } => {
            if task_notification_suppression_reason(config, event.thread_id.as_deref()).is_some() {
                return false;
            }
            let (Some(proof), Some(path)) = (fingerprint, codex_path(config, event)) else {
                return false;
            };
            if !same_source(&path, proof) {
                return false;
            }
            let mut event = (**event).clone();
            event.bark_body = original_body
                .clone()
                .unwrap_or_else(|| candidate.request.body.clone());
            if event.payload["question_tool"] == "request_user_input_async"
                || event.payload["body_source"] == "assistant_question"
            {
                async_question_monitor::prepare_event(config, db, event)
                    .and_then(|e| question_monitor::prepare_event(config, db, e))
                    .is_ok_and(|e| {
                        e.suppression_reason.is_none() && event_enabled(config, candidate)
                    })
            } else {
                if event.payload["body_source"] == "request_user_input"
                    || event.payload["body_source"] == "proposed_plan"
                {
                    let Some(path) = codex_path(config, &event) else {
                        return false;
                    };
                    let Ok(Some(selection)) =
                        codex::rollout_hook_stop_message_selection(&path, None)
                    else {
                        return false;
                    };
                    if selection.selected_turn_id != event.turn_id
                        || !matches!(
                            selection.source.as_str(),
                            "request_user_input" | "proposed_plan"
                        )
                    {
                        return false;
                    }
                }
                let (Some(proof), Some(path)) = (fingerprint, codex_path(config, &event)) else {
                    return false;
                };
                if !same_source(&path, proof) {
                    return false;
                }
                // Only append-only, unrelated records may leave an older notification current.
                let Ok(mut file) = std::fs::File::open(&path) else {
                    return false;
                };
                if file.seek(SeekFrom::Start(proof.length)).is_err() {
                    return false;
                }
                let mut tail = String::new();
                if file
                    .take(2 * 1024 * 1024 + 1)
                    .read_to_string(&mut tail)
                    .is_err()
                    || tail.len() > 2 * 1024 * 1024
                    || (!tail.is_empty() && !tail.ends_with('\n'))
                {
                    return false;
                }
                tail.lines()
                    .filter(|line| !line.trim().is_empty())
                    .all(|line| {
                        let Ok(row) = serde_json::from_str::<Value>(line) else {
                            return false;
                        };
                        let payload = row.get("payload").unwrap_or(&row);
                        let kind = payload["type"]
                            .as_str()
                            .or(row["type"].as_str())
                            .unwrap_or("");
                        let outer = row["type"].as_str().unwrap_or("");
                        let turn = payload["turn_id"].as_str().or(payload["turnId"].as_str());
                        !matches!(
                            kind,
                            "task_started"
                                | "turn_started"
                                | "turn/started"
                                | "turn_aborted"
                                | "turn_cancelled"
                                | "user_message"
                                | "function_call"
                                | "custom_tool_call"
                        ) && payload["role"] != "user"
                            && !(outer == "turn_context"
                                && turn.is_some_and(|id| Some(id) != event.turn_id.as_deref()))
                    })
            }
        }
    }
}

pub async fn retry_pending_with_config(db: &PanelDb, load_config: &ConfigLoader<'_>) -> Result<()> {
    db.sync_notification_channels(&latest_config(load_config)?)?;
    for job in db.due_notification_deliveries(10)? {
        let candidate = match serde_json::from_value::<Candidate>(job.payload) {
            Ok(candidate) => candidate,
            Err(_) => {
                if let Some(lease) = db.claim_notification_delivery(job.channel, &job.event_key)? {
                    db.finish_notification_delivery(
                        &lease,
                        &json!({"sent":false,"skipped":true,"reason":"invalid_retry_context"}),
                    )?;
                }
                continue;
            }
        };
        if matches!(
            &candidate.source,
            Source::ClaudeHook {
                verified: false,
                ..
            }
        ) {
            process_claude_hook(
                &latest_config(load_config)?,
                db,
                &job.event_key,
                &candidate,
                Duration::from_secs(5),
            )
            .await?;
            continue;
        }
        process(
            load_config,
            db,
            &job.event_key,
            job.channel,
            &candidate,
            Duration::from_secs(8),
            true,
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
async fn retry_pending(config: &Config, db: &PanelDb) -> Result<()> {
    retry_pending_with_config(db, &|| Ok(config.clone())).await
}
