//! Background detection and final validation of native asynchronous questions.
use super::{build_probe_event, record_probe_event_with_bark};
use agenthub_core::{
    codex::{self, AsyncQuestionCall, CodexTaskIdentity, ThreadSummary},
    config::Config,
    db::PanelDb,
    probe::{ProbeBuiltEvent, ProbeEventInput},
    services::probe::format_probe_pending_elicitation,
};
use anyhow::Result;
use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) fn is_enabled(config: &Config) -> bool {
    enabled(config)
}

fn enabled(config: &Config) -> bool {
    config.probe.codex_questions_enabled()
}

fn event_for(
    config: &Config,
    thread: &ThreadSummary,
    call: &AsyncQuestionCall,
) -> Option<ProbeBuiltEvent> {
    let body = format_probe_pending_elicitation(&call.pending()?);
    let mut event = build_probe_event(
        config,
        ProbeEventInput::hook_stop_with_context(
            Some(&thread.id),
            Some(&call.turn_id),
            Some(&thread.id),
            thread.rollout_path.as_deref().and_then(|p| p.to_str()),
            Some(&body),
            "reply-needed",
        )
        .with_thread_title(Some(&thread.title))
        .with_body_source(Some("request_user_input"))
        .with_call_id(Some(&call.call_id))
        .with_passive_scan_source(),
    );
    event.payload["question_tool"] = json!("request_user_input_async");
    event.payload["question_content_hash"] = json!(call.content_hash);
    event.payload["question_created_at_ms"] = json!(call.created_at_ms);
    Some(event)
}

pub(crate) fn delivery_key(event: &ProbeBuiltEvent) -> Option<String> {
    if event.payload["question_tool"] != "request_user_input_async" {
        return None;
    }
    let identity = format!(
        "{}:{}:{}:{}",
        event.thread_id.as_deref()?,
        event.turn_id.as_deref()?,
        event.payload["call_id"].as_str()?,
        event.payload["question_content_hash"].as_str()?
    );
    Some(format!(
        "probe_async_question_delivery:{}",
        hex::encode(Sha256::digest(identity.as_bytes()))
    ))
}

pub(crate) fn prepare_event(
    config: &Config,
    db: &PanelDb,
    event: ProbeBuiltEvent,
) -> Result<ProbeBuiltEvent> {
    if !matches!(
        event.event_type.as_str(),
        "completion" | "hook_stop" | "reply_needed"
    ) {
        return Ok(event);
    }
    if event.payload["body_source"] == "proposed_plan" {
        return Ok(event);
    }
    let mut unconfirmed = event.clone();
    if event.payload["question_tool"]
        .as_str()
        .is_some_and(codex::is_async_question_tool)
    {
        unconfirmed.suppression_reason = Some("unconfirmed_question_transcript".into());
    }
    let Some(id) = event.thread_id.as_deref() else {
        return Ok(event);
    };
    let paths = codex::resolve_codex_paths(&config.codex.home).codex_paths();
    let Ok(Some(thread)) = codex::notification_thread_header(&paths, id) else {
        return Ok(unconfirmed);
    };
    let Some(path) = thread.rollout_path.as_deref() else {
        return Ok(unconfirmed);
    };
    let calls = match codex::rollout_async_questions(path) {
        Ok(calls) => calls,
        Err(_) => {
            let mut event = event;
            // A newly appended answer must never be ignored by a stale sender.
            if event.payload["body_source"] == "request_user_input" {
                event.suppression_reason = Some("unconfirmed_question_transcript".into());
            }
            return Ok(event);
        }
    };
    let call_id = event.payload["call_id"]
        .as_str()
        .or_else(|| event.payload["item_id"].as_str());
    let candidate = calls.iter().find(|c| {
        event
            .turn_id
            .as_deref()
            .is_none_or(|turn| turn == c.turn_id)
            && match call_id {
                Some(id) => id == c.call_id,
                None => c.pending().is_some(),
            }
    });
    let Some(call) = candidate else {
        return Ok(unconfirmed);
    };
    if call.session_id.as_deref() != Some(id) {
        let mut event = event;
        event.suppression_reason = Some("question_transcript_identity_mismatch".into());
        return Ok(event);
    }
    let since = db
        .current_notification_baseline("probe_async_question_baseline")?
        .filter(|_| enabled(config));
    let eligible = since
        .zip(call.created_at_ms)
        .is_some_and(|(since, created)| {
            created > since && created <= jiff::Timestamp::now().as_millisecond()
        });
    if !eligible || call.pending().is_none() {
        let mut event = event;
        event.suppression_reason = Some(
            if call.pending().is_none() {
                "question_already_resolved"
            } else {
                "question_before_baseline_or_disabled"
            }
            .into(),
        );
        return Ok(event);
    }
    let mut prepared = event_for(config, &thread, call).expect("pending question checked");
    if let Some(source) = event.payload.get("scan_source") {
        prepared.payload["scan_source"] = source.clone();
    }
    Ok(prepared)
}

#[cfg(test)]
pub(crate) async fn run(config: &Config, db: &PanelDb) -> Result<()> {
    run_with_threads(&|| Ok(config.clone()), db, None).await
}

#[cfg(test)]
pub(crate) async fn run_with_threads(
    load_config: &super::notification_delivery::ConfigLoader<'_>,
    db: &PanelDb,
    preloaded_sorted: Option<&[(ThreadSummary, CodexTaskIdentity)]>,
) -> Result<()> {
    run_with_threads_and_fresh_loader(load_config, load_config, db, preloaded_sorted).await
}

pub(crate) async fn run_with_threads_and_fresh_loader(
    tick_config: &super::notification_delivery::ConfigLoader<'_>,
    fresh_config: &super::notification_delivery::ConfigLoader<'_>,
    db: &PanelDb,
    preloaded_sorted: Option<&[(ThreadSummary, CodexTaskIdentity)]>,
) -> Result<()> {
    let synced_config = db.sync_notification_channels_from_config(fresh_config)?;
    let config = &tick_config()?;
    let Some(since) = db
        .current_notification_baseline("probe_async_question_baseline")?
        .filter(|_| enabled(config) && enabled(&synced_config))
    else {
        return Ok(());
    };
    let owned_threads;
    let threads: &[(ThreadSummary, CodexTaskIdentity)] = match preloaded_sorted {
        Some(slice) => slice,
        None => {
            let paths = codex::resolve_codex_paths(&config.codex.home).codex_paths();
            owned_threads = agenthub_core::read_tasks::spawn_blocking(move || {
                let mut loaded = codex::notification_thread_headers_with_identities(&paths)?;
                loaded.sort_by(|a, b| b.0.updated_at.cmp(&a.0.updated_at));
                Ok::<_, anyhow::Error>(loaded)
            })
            .await
            .map_err(|err| anyhow::anyhow!("question header scan join failed: {err}"))??;
            &owned_threads
        }
    };
    let now_ms = jiff::Timestamp::now().as_millisecond();
    const PASSIVE_ASYNC_QUESTION_MAX_AGE_MS: i64 = 6 * 86_400_000;
    let candidates: Vec<(usize, std::path::PathBuf)> = threads
        .iter()
        .take(config.probe.recent_limit.clamp(1, 200))
        .enumerate()
        .filter(|(_, (_, identity))| identity.suppression_reason().is_none())
        .filter_map(|(idx, (thread, _))| {
            thread.rollout_path.as_ref().map(|path| (idx, path.clone()))
        })
        .collect();
    let (scanned_calls, had_transient_err) = if candidates.is_empty() {
        (Vec::new(), false)
    } else {
        agenthub_core::read_tasks::spawn_blocking(move || {
            let mut found = Vec::new();
            let mut transient_err = false;
            for (idx, path) in candidates {
                match codex::rollout_pending_async_questions_since(&path, since) {
                    Ok(pending) if !pending.is_empty() => found.push((idx, pending)),
                    Ok(_) => {}
                    Err(err) => {
                        let is_not_found = err
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|io_err| io_err.kind() == std::io::ErrorKind::NotFound)
                            || matches!(
                                std::fs::symlink_metadata(&path),
                                Err(meta_err) if meta_err.kind() == std::io::ErrorKind::NotFound
                            );
                        if !is_not_found {
                            transient_err = true;
                        }
                    }
                }
            }
            (found, transient_err)
        })
        .await
        .map_err(|err| anyhow::anyhow!("async question scan join failed: {err}"))?
    };
    let mut active_markers = Vec::new();
    for (idx, calls) in scanned_calls {
        let (thread, _) = &threads[idx];
        for call in &calls {
            let Some(event) = event_for(config, thread, call) else {
                continue;
            };
            if let Some(key) = delivery_key(&event) {
                active_markers.push(key.clone());
                if db.get_setting(&key)?.is_some() {
                    let _ = db.touch_setting_if_older_than(&key, 86_400);
                    continue;
                }
            }
            if call.created_at_ms.is_some_and(|created| {
                now_ms.saturating_sub(created) <= PASSIVE_ASYNC_QUESTION_MAX_AGE_MS
            }) {
                record_probe_event_with_bark(fresh_config, db, event).await?;
            }
        }
    }
    if had_transient_err {
        if let Ok(Some(prev)) = db.get_setting("probe_active_unresolved_markers:async_question") {
            if let Ok(prev_keys) = serde_json::from_str::<Vec<String>>(&prev) {
                active_markers.extend(prev_keys);
            }
        }
    }
    if let Err(err) = db.retain_active_unresolved_markers("async_question", &active_markers) {
        tracing::warn!(
            "failed to retain active async_question unresolved markers: {}",
            agenthub_core::security::redact_output(&err.to_string())
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
