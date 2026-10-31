//! Shared by the Linux service and the desktop's port-free monitor helper.
use super::{notification_delivery, ProbeBarkOutcome};
use anyhow::Result;
use nexushub_core::{
    config::Config,
    db::PanelDb,
    native_probe::{self, NativeProvider, NativeScan},
};
use std::time::Duration;

pub async fn run(config: &Config, db: &PanelDb) -> Result<()> {
    let idle_ttl = nexushub_core::resource_limits::DETAIL_CACHE_IDLE_TTL;
    nexushub_core::evict_idle_detail_caches(idle_ttl);
    super::antigravity::evict_idle_cache(idle_ttl).await;
    db.maintain_notification_history_if_due(config.probe.observability.event_retention_days)?;
    db.recover_interrupted_native_deliveries()?;

    let mut snapshots = Vec::new();
    for provider in [
        NativeProvider::Grok,
        NativeProvider::Claude,
        NativeProvider::Antigravity,
        NativeProvider::Dsh,
    ] {
        if provider.local_only() && !cfg!(target_os = "macos") {
            continue;
        }
        let scan = if provider.enabled(config) {
            match scan(provider).await {
                Ok(scan) => scan,
                Err(_) => NativeScan {
                    streams: vec![],
                    errors: 1,
                },
            }
        } else {
            NativeScan::default()
        };
        db.stage_native_notifications_with_hooks(
            provider,
            &scan,
            config,
            super::claude_hook::installed(),
        )?;
        snapshots.extend(scan.streams);
    }
    deliver_pending(config, db, &snapshots).await
}

pub async fn run_provider_hook(
    config_path: &std::path::Path,
    provider: NativeProvider,
) -> Result<()> {
    let config = Config::load(config_path)?;
    if !provider.enabled(&config) {
        return Ok(());
    }
    let db = super::open_panel_db(&config)?;
    db.recover_interrupted_native_deliveries()?;
    let scan = scan(provider).await.unwrap_or(NativeScan {
        streams: vec![],
        errors: 1,
    });
    db.stage_native_notifications_with_hooks(provider, &scan, &config, false)?;
    deliver_pending(&config, &db, &scan.streams).await
}

async fn deliver_pending(
    config: &Config,
    db: &PanelDb,
    snapshots: &[native_probe::NativeStreamSnapshot],
) -> Result<()> {
    let deliveries = db.pending_native_deliveries(100)?;
    let instance = db.notification_instance_id()?;
    for delivery in deliveries {
        // Identity and branch are resolved by each native parser. Codex's
        // main-task identity filter must never be applied to these providers.
        // Claude terminal events are re-read by the delivery queue immediately
        // before sending. Missing identity/terminal data must retain bounded retries.
        let still_current = snapshots.iter().any(|stream| {
            stream.provider == delivery.provider
                && stream.session_key == delivery.session_key
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
        });
        if !db.claim_native_delivery(&delivery.event_key)? {
            continue;
        }
        let enabled = delivery.provider.enabled(config)
            && delivery
                .provider
                .event_enabled(config, &delivery.event.kind);
        let outcome = if (!still_current
            && delivery.provider != NativeProvider::Claude
            && !delivery.provider.local_only())
            || !enabled
        {
            ProbeBarkOutcome::skipped(
                "native_identity_or_branch_changed",
                config.probe.notifications.enabled,
                enabled,
                false,
            )
        } else {
            notification_delivery::native(config, db, &delivery, Duration::from_secs(8)).await?
        };
        db.finish_native_delivery(&delivery, serde_json::to_value(outcome)?)?;
    }
    Ok(())
}

pub(super) async fn scan(provider: NativeProvider) -> Result<NativeScan> {
    if provider == NativeProvider::Antigravity {
        super::antigravity::scan().await
    } else {
        tokio::task::spawn_blocking(move || native_probe::scan(provider)).await?
    }
}

pub(super) fn local_event_current(
    scan: &NativeScan,
    delivery: &nexushub_core::db::NativeDelivery,
) -> Result<bool> {
    if let Some(stream) = scan.streams.iter().find(|s| {
        s.provider == delivery.provider
            && s.id == delivery.thread_id
            && s.session_key == delivery.session_key
    }) {
        return Ok(stream.event_key(&delivery.event) == delivery.event_key
            && stream
                .events
                .iter()
                .any(|e| native_probe::same_event(e, &delivery.event)));
    }
    anyhow::ensure!(scan.errors == 0, "native notification source pending");
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::sandboxed_test_config;
    use native_probe::{NativeStreamSnapshot, NativeTurnEvent};
    use serde_json::Value;

    #[tokio::test]
    async fn claude_candidate_without_a_readable_source_stays_in_the_bounded_queue() {
        let (mut config, sandbox_root) = sandboxed_test_config();
        config.probe.notifications.enabled = true;
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        db.stage_native_notifications(NativeProvider::Claude, &NativeScan::default(), &config)
            .unwrap();
        let stream = NativeStreamSnapshot {
            provider: NativeProvider::Claude,
            session_key: "fixture-session".into(),
            id: "fixture-native".into(),
            title: "Fixture completion".into(),
            identity: "fixture-file".into(),
            record_count: 2,
            settled_count: 2,
            events: vec![NativeTurnEvent {
                prompt_id: None,
                position: 2,
                turn_id: "fixture-turn".into(),
                kind: "completion".into(),
                body: "Fixture completed".into(),
                timestamp_ms: chrono::Utc::now().timestamp_millis() + 1000,
            }],
        };
        let scan = NativeScan {
            streams: vec![stream],
            errors: 0,
        };
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Claude, &scan, &config)
                .unwrap(),
            1
        );
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Claude, &scan, &config)
                .unwrap(),
            0
        );
        deliver_pending(&config, &db, &[]).await.unwrap();
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["bark_status"], "queued");
        assert_eq!(
            events[0].payload["bark"]["reason"],
            "claude_terminal_pending"
        );
        db.stage_native_notifications(NativeProvider::Claude, &scan, &config)
            .unwrap();
        assert!(db.pending_native_deliveries(100).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(sandbox_root);
    }

    #[tokio::test]
    async fn native_deliveries_use_provider_identity_and_do_not_replay_after_send() {
        for (provider, kind) in [
            (NativeProvider::Grok, "completion"),
            (NativeProvider::Grok, "failure"),
            (NativeProvider::Grok, "completion"),
        ] {
            let server = crate::tests::TestHttpServer::start_n(1, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}");
            let (mut config, sandbox_root) = sandboxed_test_config();
            config.probe.notifications.enabled = true;
            config.probe.notifications.server_url = server.url();
            let db = PanelDb::open(":memory:").unwrap();
            db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
                .unwrap();
            db.stage_native_notifications(provider, &NativeScan::default(), &config)
                .unwrap();
            let stream = NativeStreamSnapshot {
                provider,
                session_key: "opaque-native-key".into(),
                id: "custom-native-id".into(),
                title: "Isolated fixture".into(),
                identity: "fixture-file-identity".into(),
                record_count: 2,
                settled_count: 2,
                events: vec![NativeTurnEvent {
                    prompt_id: None,
                    position: 2,
                    turn_id: "native-turn".into(),
                    kind: kind.into(),
                    body: "Final fixture answer".into(),
                    timestamp_ms: chrono::Utc::now().timestamp_millis() + 1000,
                }],
            };
            db.stage_native_notifications(
                provider,
                &NativeScan {
                    streams: vec![stream.clone()],
                    errors: 0,
                },
                &config,
            )
            .unwrap();
            deliver_pending(&config, &db, std::slice::from_ref(&stream))
                .await
                .unwrap();
            let request = server.request();
            let capture: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert!(capture["body"]
                .as_str()
                .unwrap()
                .contains("Final fixture answer"));
            assert!(capture["title"].as_str().unwrap().contains(
                if provider == NativeProvider::Claude {
                    "Claude Code"
                } else {
                    "Grok"
                }
            ));
            let events = db.list_probe_events(10).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].payload["provider"], provider.as_str());
            assert_eq!(events[0].payload["session_key"], "opaque-native-key");
            assert_eq!(events[0].payload["bark_status"], "sent");
            db.stage_native_notifications(
                provider,
                &NativeScan {
                    streams: vec![stream.clone()],
                    errors: 0,
                },
                &config,
            )
            .unwrap();
            deliver_pending(&config, &db, &[stream]).await.unwrap();
            assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
            let _ = std::fs::remove_dir_all(sandbox_root);
        }
    }

    #[tokio::test]
    async fn failed_transport_is_visible_and_changed_branch_is_not_delivered() {
        let server = crate::tests::TestHttpServer::start_n(
            1,
            "HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
        let (mut config, sandbox_root) = sandboxed_test_config();
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = server.url();
        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        db.stage_native_notifications(NativeProvider::Grok, &NativeScan::default(), &config)
            .unwrap();
        let mut stream = NativeStreamSnapshot {
            provider: NativeProvider::Grok,
            session_key: "p/a.jsonl".into(),
            id: "custom".into(),
            title: "Fixture".into(),
            identity: "file".into(),
            record_count: 2,
            settled_count: 2,
            events: vec![NativeTurnEvent {
                prompt_id: None,
                position: 2,
                turn_id: "u:a".into(),
                kind: "completion".into(),
                body: "Fixture reply".into(),
                timestamp_ms: chrono::Utc::now().timestamp_millis() + 1000,
            }],
        };
        db.stage_native_notifications(
            NativeProvider::Grok,
            &NativeScan {
                streams: vec![stream.clone()],
                errors: 0,
            },
            &config,
        )
        .unwrap();
        deliver_pending(&config, &db, std::slice::from_ref(&stream))
            .await
            .unwrap();
        assert!(server.request().contains("POST"));
        assert_eq!(
            db.list_probe_events(10).unwrap()[0].payload["bark_status"],
            "queued"
        );
        stream.record_count = 3;
        stream.settled_count = 3;
        stream.events[0].position = 3;
        stream.events[0].turn_id = "u:b".into();
        db.stage_native_notifications(
            NativeProvider::Grok,
            &NativeScan {
                streams: vec![stream],
                errors: 0,
            },
            &config,
        )
        .unwrap();
        deliver_pending(&config, &db, &[]).await.unwrap();
        let events = db.list_probe_events(10).unwrap();
        assert!(events
            .iter()
            .any(|event| event.payload["bark"]["reason"] == "native_identity_or_branch_changed"));
        let _ = std::fs::remove_dir_all(sandbox_root);
    }
}
