//! Shared by the Linux service and the desktop's port-free monitor helper.
use super::{notification_delivery, ProbeBarkOutcome};
use agenthub_core::{
    config::Config,
    db::PanelDb,
    native_probe::{self, NativeProvider, NativeScan},
};
use anyhow::Result;
use std::time::Duration;

pub async fn run(config: &Config, db: &PanelDb) -> Result<()> {
    let idle_ttl = agenthub_core::resource_limits::DETAIL_CACHE_IDLE_TTL;
    agenthub_core::evict_idle_detail_caches(idle_ttl);
    super::antigravity::evict_idle_cache(idle_ttl).await;
    db.maintain_notification_history_if_due(config.probe.observability.event_retention_days)?;
    db.recover_interrupted_native_deliveries()?;

    let mut snapshots = Vec::new();
    for provider in [
        NativeProvider::Grok,
        NativeProvider::GrokBot,
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
            provider == NativeProvider::Claude
                && provider.enabled(config)
                && super::claude_hook::installed(),
        )?;
        snapshots.extend(scan.streams);
    }
    deliver_pending(config, db, &snapshots).await
}

pub async fn run_provider_hook(
    config_path: &std::path::Path,
    provider: NativeProvider,
) -> Result<()> {
    let config = Config::load_runtime(config_path)?;
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

pub async fn run_antigravity_hook(
    config_path: &std::path::Path,
    conversation_id: Option<&str>,
) -> Result<()> {
    let provider = NativeProvider::Antigravity;
    let config = Config::load_runtime(config_path)?;
    if !provider.enabled(&config) {
        return Ok(());
    }
    let db = super::open_panel_db(&config)?;
    db.recover_interrupted_native_deliveries()?;
    let scan = match conversation_id.filter(|id| !id.trim().is_empty()) {
        Some(id) => super::antigravity::scan_one(id).await,
        None => super::antigravity::scan().await,
    }
    .unwrap_or(NativeScan {
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
    delivery: &agenthub_core::db::NativeDelivery,
) -> Result<bool> {
    if let Some(stream) = scan.streams.iter().find(|s| {
        s.provider == delivery.provider
            && s.id == delivery.thread_id
            && s.session_key == delivery.session_key
    }) {
        return Ok(stream.event_key(&delivery.event) == delivery.event_key
            && stream.events.iter().any(|e| {
                if delivery.provider == NativeProvider::GrokBot {
                    e.turn_id == delivery.event.turn_id && e.kind == delivery.event.kind
                } else {
                    native_probe::same_event(e, &delivery.event)
                }
            }));
    }
    anyhow::ensure!(scan.errors == 0, "native notification source pending");
    Ok(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn grokbot_rechecks_request_identity_and_stops_after_resolution_or_account_switch() {
        let event = agenthub_core::native_probe::NativeTurnEvent {
            position: 1000,
            turn_id: "request".into(),
            kind: "authorization".into(),
            body: "fixture".into(),
            timestamp_ms: 1000,
            prompt_id: Some("request".into()),
        };
        let mut stream = agenthub_core::native_probe::NativeStreamSnapshot {
            provider: NativeProvider::GrokBot,
            session_key: "account:thread".into(),
            id: "thread".into(),
            title: "Fixture".into(),
            identity: "account:thread".into(),
            record_count: 1000,
            settled_count: 0,
            events: vec![event.clone()],
        };
        let delivery = agenthub_core::db::NativeDelivery {
            event_key: stream.event_key(&event),
            provider: NativeProvider::GrokBot,
            session_key: stream.session_key.clone(),
            thread_id: stream.id.clone(),
            title: stream.title.clone(),
            event,
        };
        stream.events[0].timestamp_ms = 2000;
        stream.events[0].body = "updated native prompt".into();
        let mut scan = NativeScan {
            streams: vec![stream],
            errors: 0,
        };
        assert!(local_event_current(&scan, &delivery).unwrap());
        scan.streams[0].events.clear();
        assert!(!local_event_current(&scan, &delivery).unwrap());
        scan.streams[0].events.push(delivery.event.clone());
        scan.streams[0].session_key = "another:thread".into();
        assert!(!local_event_current(&scan, &delivery).unwrap());
    }

    use super::*;
    use crate::api::test_support::sandboxed_test_config;
    use native_probe::{NativeStreamSnapshot, NativeTurnEvent};
    use serde_json::Value;

    #[tokio::test]
    async fn claude_candidate_without_a_readable_source_stays_in_the_bounded_queue() {
        let _env_guard = crate::api::test_support::AsyncEnvGuard::lock_read_only().await;
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

    #[tokio::test]
    async fn all_five_platforms_produce_probe_events_through_hook_or_poll_pipeline() {
        let server = crate::tests::TestHttpServer::start_n(
            5,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let (mut config, sandbox_root) = sandboxed_test_config();
        config.probe.enabled = true;
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = server.url();
        let n = &mut config.probe.notifications;
        n.notify_completion = true;
        n.notify_claude = true;
        n.notify_claude_completion = true;
        n.notify_grok = true;
        n.notify_grok_completion = true;
        n.notify_antigravity = true;
        n.notify_antigravity_completion = true;
        n.notify_dsh = true;
        n.notify_dsh_completion = true;

        let db = PanelDb::open(":memory:").unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();

        // 1. Codex via notify_completion_context + record_probe_event_with_bark
        let (codex_config, _codex_identity) =
            crate::notification_accuracy_tests::test_notification_config(&[
                "codex-self-test-thread",
            ]);
        let mut codex_config = codex_config;
        codex_config.probe = config.probe.clone();
        let codex_transcript = sandbox_root.join("codex-rollout.jsonl");
        std::fs::write(
            &codex_transcript,
            serde_json::json!({
                "type": "event_msg",
                "payload": {
                    "type": "task_complete",
                    "turn_id": "codex-self-test-turn",
                    "last_agent_message": "Codex self-test completion."
                }
            })
            .to_string(),
        )
        .unwrap();
        let codex_input = crate::notify_completion_context(
            &codex_config,
            Some(&serde_json::json!({
                "thread_id": "codex-self-test-thread",
                "turn_id": "codex-self-test-turn",
                "transcript_path": codex_transcript,
            })),
            None,
            None,
        )
        .unwrap();
        let codex_event = crate::probe_runtime(&codex_config).build_event(codex_input);
        let (codex_outcome, _) =
            crate::record_probe_event_with_bark(&codex_config, &db, codex_event)
                .await
                .unwrap();
        assert!(codex_outcome.recorded);

        // 2-5. Claude Code, Grok, Antigravity, and DSH via their real parsers and hook inputs
        let claude_config_dir = sandbox_root.join("claude");
        let grok_home_dir = sandbox_root.join("grok");
        let dsh_home_dir = sandbox_root.join("dsh");
        let _env_guard = crate::api::test_support::AsyncEnvGuard::set_many(&[
            ("CLAUDE_CONFIG_DIR", &claude_config_dir),
            ("GROK_HOME", &grok_home_dir),
            ("DSH_HOME", &dsh_home_dir),
        ])
        .await;

        let start_ms = chrono::Utc::now().timestamp_millis() + 1_000;
        let now_ms = start_ms + 1_000;
        let start_rfc3339 = chrono::DateTime::from_timestamp_millis(start_ms)
            .unwrap()
            .to_rfc3339();
        let now_rfc3339 = chrono::DateTime::from_timestamp_millis(now_ms)
            .unwrap()
            .to_rfc3339();

        // 2. Claude Code: real JSONL transcript + claude_hook::parse + notification_delivery::claude_hook
        let claude_paths = agenthub_core::claude::ClaudePaths::default_for_user();
        let claude_transcript = claude_paths.projects.join("fixture/session.jsonl");
        std::fs::create_dir_all(claude_transcript.parent().unwrap()).unwrap();
        let claude_user = serde_json::json!({
            "type": "user",
            "uuid": "msg-user-1",
            "parentUuid": null,
            "promptId": "prompt-self-test-1",
            "sessionId": "00000000-0000-4000-8000-000000000002",
            "version": "2.1.284",
            "cwd": "/fixture/project",
            "timestamp": start_rfc3339,
            "message": { "role": "user", "content": "Claude self-test prompt" }
        });
        let claude_assistant = serde_json::json!({
            "type": "assistant",
            "uuid": "msg-assistant-1",
            "parentUuid": "msg-user-1",
            "sessionId": "00000000-0000-4000-8000-000000000002",
            "timestamp": now_rfc3339,
            "message": {
                "role": "assistant",
                "content": "Claude self-test completed.",
                "stop_reason": "end_turn"
            }
        });
        std::fs::write(
            &claude_transcript,
            format!("{claude_user}\n{claude_assistant}\n"),
        )
        .unwrap();
        let claude_hook_event = crate::claude_hook::parse(&serde_json::json!({
            "session_id": "00000000-0000-4000-8000-000000000002",
            "prompt_id": "prompt-self-test-1",
            "transcript_path": claude_transcript,
            "hook_event_name": "Stop",
            "last_assistant_message": "Claude self-test completed."
        }))
        .unwrap()
        .expect("real claude hook parser must accept Stop payload");
        assert_eq!(claude_hook_event.kind, "completion");
        db.stage_native_notifications(NativeProvider::Claude, &NativeScan::default(), &config)
            .unwrap();
        crate::notification_delivery::claude_hook(
            &config,
            &db,
            claude_hook_event,
            Duration::from_secs(3),
        )
        .await
        .unwrap();

        // 3. Grok: real summary.json + events.jsonl + updates.jsonl + native_probe::scan(Grok)
        let grok_paths = agenthub_core::grok::GrokPaths::default_for_user();
        let grok_id = "00000000-0000-4000-8000-000000000003";
        let grok_session_dir = grok_paths.home.join("sessions/%2Ffixture").join(grok_id);
        std::fs::create_dir_all(&grok_session_dir).unwrap();
        std::fs::write(
            grok_session_dir.join("summary.json"),
            serde_json::to_vec(&serde_json::json!({
                "info": { "id": grok_id, "cwd": "/fixture" },
                "generated_title": "Grok self-test",
                "num_messages": 2
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            grok_session_dir.join("events.jsonl"),
            format!(
                "{{\"type\":\"turn_started\",\"session_id\":\"{grok_id}\",\"session_relationship\":\"primary\",\"ts\":\"{start_rfc3339}\"}}\n{{\"type\":\"turn_ended\",\"outcome\":\"completed\",\"ts\":\"{now_rfc3339}\"}}\n"
            ),
        )
        .unwrap();
        std::fs::write(
            grok_session_dir.join("updates.jsonl"),
            format!(
                "{{\"timestamp\":\"{start_rfc3339}\",\"params\":{{\"sessionId\":\"{grok_id}\",\"_meta\":{{\"promptId\":\"p-grok-1\"}},\"update\":{{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{{\"type\":\"text\",\"text\":\"Grok self-test completed.\"}}}}}}}}\n{{\"timestamp\":\"{now_rfc3339}\",\"params\":{{\"sessionId\":\"{grok_id}\",\"update\":{{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"p-grok-1\",\"stop_reason\":\"end_turn\"}}}}}}\n"
            ),
        )
        .unwrap();
        let grok_scan = agenthub_core::native_probe::scan(NativeProvider::Grok).unwrap();
        assert_eq!(grok_scan.errors, 0);
        assert_eq!(grok_scan.streams.len(), 1);
        assert_eq!(grok_scan.streams[0].events.len(), 1);

        // 4. Antigravity: real trajectory parser antigravity::snapshot
        let ag_id = "00000000-0000-4000-8000-000000000005";
        let ag_summary = serde_json::json!({
            "trajectoryId": "traj-self-test",
            "createdTime": start_rfc3339,
            "lastModifiedTime": now_rfc3339,
            "summary": "Antigravity self-test",
            "status": "CASCADE_RUN_STATUS_IDLE",
            "trajectoryMetadata": { "rootConversationId": ag_id }
        });
        let ag_response = serde_json::json!({
            "status": "CASCADE_RUN_STATUS_IDLE",
            "numTotalSteps": 2,
            "trajectory": {
                "cascadeId": ag_id,
                "trajectoryId": "traj-self-test",
                "steps": [
                    {
                        "type": "CORTEX_STEP_TYPE_USER_INPUT",
                        "status": "CORTEX_STEP_STATUS_DONE",
                        "metadata": { "executionId": "exec-ag-1", "createdAt": start_rfc3339 }
                    },
                    {
                        "type": "CORTEX_STEP_TYPE_PLANNER_RESPONSE",
                        "status": "CORTEX_STEP_STATUS_DONE",
                        "metadata": {
                            "executionId": "exec-ag-1",
                            "createdAt": start_rfc3339,
                            "completedAt": now_rfc3339
                        },
                        "plannerResponse": { "response": "Antigravity self-test completed." }
                    }
                ],
                "executorMetadatas": [
                    {
                        "executionId": "exec-ag-1",
                        "lastStepIdx": 1,
                        "terminationReason": "EXECUTOR_TERMINATION_REASON_NO_TOOL_CALL"
                    }
                ]
            }
        });
        let ag_stream =
            agenthub_core::antigravity::snapshot(ag_id, &ag_summary, &ag_response).unwrap();
        assert_eq!(ag_stream.events.len(), 1);
        let ag_scan = NativeScan {
            streams: vec![ag_stream],
            errors: 0,
        };

        // 5. DSH: real v4 session.v4.jsonl + native_probe::scan(Dsh)
        let dsh_paths = agenthub_core::dsh::DshPaths::default_for_user();
        let dsh_id = "00000000-0000-4000-8000-000000000004";
        let dsh_dir = dsh_paths.home.join("sessions/fixture").join(dsh_id);
        std::fs::create_dir_all(&dsh_dir).unwrap();
        let dsh_lines = [
            serde_json::json!({"type":"session","version":4,"id":dsh_id,"isSeeded":false}),
            serde_json::json!({"type":"turn/start","seq":0,"time":start_ms,"data":{"turn":1}}),
            serde_json::json!({"type":"assistant/message","seq":1,"time":start_ms,"data":{"turn":1,"message":{"content":[{"type":"text","text":"DSH self-test completed."}]}}}),
            serde_json::json!({"type":"turn/end","seq":2,"time":now_ms,"data":{"turn":1,"reason":{"kind":"completed"}}}),
        ];
        std::fs::write(
            dsh_dir.join("session.v4.jsonl"),
            dsh_lines
                .iter()
                .map(|line| format!("{line}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let dsh_scan = agenthub_core::native_probe::scan(NativeProvider::Dsh).unwrap();
        assert_eq!(dsh_scan.errors, 0);
        assert_eq!(dsh_scan.streams.len(), 1);
        assert_eq!(dsh_scan.streams[0].events.len(), 1);

        let native_scans = vec![
            (NativeProvider::Grok, grok_scan),
            (NativeProvider::Antigravity, ag_scan),
            (NativeProvider::Dsh, dsh_scan),
        ];

        for (provider, scan) in native_scans {
            db.stage_native_notifications(provider, &NativeScan::default(), &config)
                .unwrap();
            assert_eq!(
                db.stage_native_notifications(provider, &scan, &config)
                    .unwrap(),
                1,
                "provider {} must stage 1 completion notification",
                provider.as_str()
            );
            deliver_pending(&config, &db, &scan.streams).await.unwrap();
        }

        let events = db.list_probe_events(20).unwrap();
        assert_eq!(events.len(), 5, "all 5 platforms must record a probe_event");
        assert!(
            events
                .iter()
                .any(|e| e.thread_id.as_deref() == Some("codex-self-test-thread")),
            "missing probe_event for provider codex"
        );
        assert!(
            events.iter().any(|e| {
                e.source == "claude_hook"
                    && e.payload["provider"] == "claude_code"
                    && e.payload["bark_status"] == "sent"
            }),
            "Claude hook must enqueue, deliver, and record a sent claude_hook probe_event"
        );
        for expected_provider in ["claude_code", "grok", "antigravity", "dsh"] {
            assert!(
                events
                    .iter()
                    .any(|e| e.payload["provider"] == expected_provider),
                "missing probe_event for provider {expected_provider}"
            );
        }
        let _ = std::fs::remove_dir_all(sandbox_root);
    }
}
