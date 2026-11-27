//! Shared by the Linux service and the desktop's port-free monitor helper.
use super::{
    notification_delivery::{self, ConfigLoader},
    ProbeBarkOutcome,
};
use agenthub_core::{
    db::PanelDb,
    native_probe::{self, NativeProvider, NativeScan},
};
use anyhow::Result;
use std::time::Duration;

pub(crate) async fn run(load_config: &ConfigLoader<'_>, db: &PanelDb) -> Result<()> {
    static LAST_IDLE_EVICT: std::sync::Mutex<Option<std::time::Instant>> =
        std::sync::Mutex::new(None);
    let config = &load_config()?;
    let should_evict = LAST_IDLE_EVICT
        .lock()
        .ok()
        .is_some_and(|mut guard| match *guard {
            Some(last) if last.elapsed() < Duration::from_secs(60) => false,
            _ => {
                *guard = Some(std::time::Instant::now());
                true
            }
        });
    if should_evict {
        let idle_ttl = agenthub_core::resource_limits::DETAIL_CACHE_IDLE_TTL;
        super::antigravity::evict_idle_cache(idle_ttl).await;
        let _ = agenthub_core::read_tasks::spawn_blocking(move || {
            agenthub_core::evict_idle_detail_caches(idle_ttl);
        })
        .await;
    }
    let retired = db.recover_interrupted_native_deliveries(None)?;
    if retired > 0 {
        tracing::warn!(retired, "retired unreadable interrupted native deliveries");
    }

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
        db.stage_native_notifications_with_precheck(
            provider,
            &scan,
            Some(config),
            load_config,
            provider == NativeProvider::Claude
                && provider.enabled(config)
                && super::claude_hook::installed(),
            provider.enabled(config),
        )?;
    }
    db.maintain_notification_history_if_due(config.probe.observability.event_retention_days)?;
    deliver_pending(load_config, db, None).await
}

pub(crate) async fn run_provider_hook(
    config_path: &std::path::Path,
    provider: NativeProvider,
) -> Result<()> {
    let cache = super::monitor_start::DaemonMonitorCache::default();
    let load_config = || cache.load_config(config_path);
    let config = load_config()?;
    if !provider.enabled(&config) {
        return Ok(());
    }
    let db = super::open_panel_db(&config)?;
    let retired = db.recover_interrupted_native_deliveries(Some(provider))?;
    if retired > 0 {
        tracing::warn!(retired, "retired unreadable interrupted native deliveries");
    }
    let scan = scan(provider).await.unwrap_or(NativeScan {
        streams: vec![],
        errors: 1,
    });
    db.stage_native_notifications_from_config(provider, &scan, &load_config, false, true)?;
    deliver_pending(&load_config, &db, Some(provider)).await
}

pub(crate) async fn run_antigravity_hook(
    config_path: &std::path::Path,
    conversation_id: Option<&str>,
) -> Result<()> {
    let provider = NativeProvider::Antigravity;
    let cache = super::monitor_start::DaemonMonitorCache::default();
    let load_config = || cache.load_config(config_path);
    let config = load_config()?;
    if !provider.enabled(&config) {
        return Ok(());
    }
    let db = super::open_panel_db(&config)?;
    let retired = db.recover_interrupted_native_deliveries(Some(provider))?;
    if retired > 0 {
        tracing::warn!(retired, "retired unreadable interrupted native deliveries");
    }
    let conversation_id = conversation_id.filter(|id| !id.trim().is_empty());
    let scan = match conversation_id {
        Some(id) => super::antigravity::scan_one(id).await,
        None => super::antigravity::scan().await,
    }
    .unwrap_or(NativeScan {
        streams: vec![],
        errors: 1,
    });
    db.stage_native_notifications_from_config(
        provider,
        &scan,
        &load_config,
        false,
        conversation_id.is_none(),
    )?;
    deliver_pending(&load_config, &db, Some(provider)).await
}

async fn deliver_pending(
    load_config: &ConfigLoader<'_>,
    db: &PanelDb,
    provider: Option<NativeProvider>,
) -> Result<()> {
    let (deliveries, retired) = db.pending_native_deliveries_with_retired(100, provider)?;
    if retired > 0 {
        tracing::warn!(retired, "retired unreadable pending native deliveries");
    }
    for delivery in deliveries {
        let config = &load_config()?;
        let enabled = delivery.provider.enabled(config)
            && delivery
                .provider
                .event_enabled(config, &delivery.event.kind);
        if enabled
            && delivery.provider == NativeProvider::Claude
            && delivery.event.kind == "completion"
        {
            use agenthub_core::claude::ClaudeNotificationState;
            match notification_delivery::claude_native_state(db, &delivery).await {
                Ok(ClaudeNotificationState::Pending) => continue,
                Ok(ClaudeNotificationState::Obsolete)
                    if db.skip_unclaimed_native_delivery(&delivery.event_key)? =>
                {
                    continue
                }
                // Read errors still enter the existing bounded delivery retry path.
                _ => {}
            }
        }
        // The delivery queue rechecks each source immediately before sending;
        // temporary read errors retain the existing bounded retries.
        if !db.claim_native_delivery(&delivery.event_key)? {
            continue;
        }
        let outcome = if !enabled {
            ProbeBarkOutcome::skipped(
                "native_identity_or_branch_changed",
                config.probe.notifications.enabled,
                enabled,
                false,
            )
        } else {
            notification_delivery::native(load_config, db, &delivery, Duration::from_secs(8))
                .await?
        };
        db.finish_native_delivery(&delivery, serde_json::to_value(outcome)?)?;
    }
    Ok(())
}

pub(super) async fn scan(provider: NativeProvider) -> Result<NativeScan> {
    if provider == NativeProvider::Antigravity {
        super::antigravity::scan().await
    } else {
        agenthub_core::read_tasks::spawn_blocking(move || native_probe::scan(provider)).await?
    }
}

pub(super) fn native_event_current(
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
            events: vec![event.clone()].into(),
        };
        let delivery = agenthub_core::db::NativeDelivery {
            event_key: stream.event_key(&event),
            provider: NativeProvider::GrokBot,
            session_key: stream.session_key.clone(),
            thread_id: stream.id.clone(),
            title: stream.title.clone(),
            event,
        };
        std::sync::Arc::make_mut(&mut stream.events)[0].timestamp_ms = 2000;
        std::sync::Arc::make_mut(&mut stream.events)[0].body = "updated native prompt".into();
        let mut scan = NativeScan {
            streams: vec![stream],
            errors: 0,
        };
        assert!(native_event_current(&scan, &delivery).unwrap());
        let mut old_completion = delivery.clone();
        old_completion.event.kind = "completion".into();
        old_completion.event.turn_id = "old-message-id".into();
        old_completion.event_key = scan.streams[0].event_key(&old_completion.event);
        assert!(!native_event_current(&scan, &old_completion).unwrap());
        std::sync::Arc::make_mut(&mut scan.streams[0].events).clear();
        assert!(!native_event_current(&scan, &delivery).unwrap());
        std::sync::Arc::make_mut(&mut scan.streams[0].events).push(delivery.event.clone());
        scan.streams[0].session_key = "another:thread".into();
        assert!(!native_event_current(&scan, &delivery).unwrap());
    }

    use super::*;
    use crate::api::test_support::sandboxed_test_config;
    use native_probe::{NativeStreamSnapshot, NativeTurnEvent};

    #[tokio::test]
    async fn grok_read_failure_stays_queued_and_recovers_without_a_second_native_claim() {
        let server = crate::tests::TestHttpServer::start_n(1, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}");
        let (mut config, root) = sandboxed_test_config();
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = server.url();
        let grok_root = root.join("grok");
        let _env =
            crate::api::test_support::AsyncEnvGuard::set_many(&[("GROK_HOME", &grok_root)]).await;
        let db = PanelDb::open_with_secret_box(
            root.join("pending.sqlite"),
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        db.stage_native_notifications_from_config(
            NativeProvider::Grok,
            &NativeScan::default(),
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();
        let scan = crate::api::test_support::write_grok_notification_fixture(
            &grok_root,
            "completion",
            "Recovered fixture",
            jiff::Timestamp::now().as_millisecond() + 1000,
        );
        db.stage_native_notifications_from_config(
            NativeProvider::Grok,
            &scan,
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();
        let path = grok_root
            .join("sessions/workspace")
            .join(&scan.streams[0].id)
            .join("events.jsonl");
        let saved = path.with_extension("saved");
        std::fs::rename(&path, &saved).unwrap();
        std::fs::create_dir(&path).unwrap();
        deliver_pending(&|| Ok(config.clone()), &db, None)
            .await
            .unwrap();
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events[0].payload["bark"]["reason"], "native_source_pending");
        assert_eq!(events[0].payload["bark_status"], "queued");
        assert!(db.pending_native_deliveries(100, None).unwrap().is_empty());
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(&saved, &path).unwrap();
        rusqlite::Connection::open(db.path())
            .unwrap()
            .execute("UPDATE probe_notification_deliveries SET next_ms=0", [])
            .unwrap();
        notification_delivery::retry_pending_with_config(&db, &|| Ok(config.clone()))
            .await
            .unwrap();
        assert!(server.request().contains("Recovered fixture"));
        assert_eq!(
            db.list_probe_events(10).unwrap()[0].payload["bark_status"],
            "sent"
        );
        assert_eq!(
            db.stage_native_notifications_from_config(
                NativeProvider::Grok,
                &scan,
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap(),
            0
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    use serde_json::Value;

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn busy_claude_waits_without_claiming_bark_and_ready_poll_or_hook_sends_once() {
        use serde_json::json;
        use std::{fs, process::Command};
        struct Owner(std::process::Child);
        impl Drop for Owner {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        for (hook_first, revoked) in [(false, false), (true, false), (false, true), (true, true)] {
            let server = crate::tests::TestHttpServer::start_n(1, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}");
            let (mut config, root) = sandboxed_test_config();
            config.probe.notifications.enabled = true;
            config.probe.notifications.server_url = server.url();
            let claude_root = root.join("claude");
            let _env = crate::api::test_support::AsyncEnvGuard::set_many(&[(
                "CLAUDE_CONFIG_DIR",
                &claude_root,
            )])
            .await;
            let paths = agenthub_core::claude::ClaudePaths::default_for_user().unwrap();
            fs::create_dir_all(paths.projects.join("fixture")).unwrap();
            fs::create_dir_all(paths.config.join("sessions")).unwrap();
            let cwd = root.canonicalize().unwrap();
            let executable = cwd.join("claude-owner/claude");
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::copy("/bin/sleep", &executable).unwrap();
            // macOS kills a relocated copy of a platform binary on launch; an ad-hoc
            // signature turns the copy into an ordinary executable.
            #[cfg(target_os = "macos")]
            assert!(Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&executable)
                .output()
                .unwrap()
                .status
                .success());
            let owner = Owner(
                Command::new(&executable)
                    .arg("60")
                    .current_dir(&cwd)
                    .spawn()
                    .unwrap(),
            );
            let output = Command::new("/bin/ps")
                .args(["-p", &owner.0.id().to_string(), "-o", "lstart="])
                .env("LC_ALL", "C")
                .env("TZ", "UTC")
                .output()
                .unwrap();
            assert!(output.status.success());
            let proc_start = String::from_utf8(output.stdout).unwrap().trim().to_owned();
            let domain = if cfg!(target_os = "macos") {
                "darwin".to_owned()
            } else {
                format!(
                    "linux:{}:{}",
                    fs::read_to_string("/etc/machine-id").unwrap().trim(),
                    fs::read_link("/proc/self/ns/pid").unwrap().display()
                )
            };
            let registration = paths.config.join("sessions/fixture.json");
            let mut row = json!({"pid":owner.0.id(),"sessionId":"fixture","cwd":cwd,"procStart":proc_start,"startedAt":jiff::Timestamp::now().as_millisecond(),"pidDomain":domain,"status":"busy"});
            fs::write(&registration, row.to_string()).unwrap();
            let db = PanelDb::open_with_secret_box(
                root.join("pending.sqlite"),
                agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap();
            db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
                .unwrap();
            db.stage_native_notifications_from_config(
                NativeProvider::Claude,
                &NativeScan::default(),
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap();
            let timestamp =
                jiff::Timestamp::from_millisecond(jiff::Timestamp::now().as_millisecond() + 1000)
                    .unwrap()
                    .to_string();
            let transcript = paths.projects.join("fixture/fixture.jsonl");
            let rows = [
                json!({"type":"user","uuid":"user","promptId":"prompt-fixture","sessionId":"fixture","cwd":cwd,"version":"2.1.322","timestamp":timestamp,"message":{"role":"user","content":"Fixture request"}}),
                json!({"type":"assistant","uuid":"answer","parentUuid":"user","sessionId":"fixture","cwd":cwd,"version":"2.1.322","timestamp":timestamp,"message":{"role":"assistant","content":"Fixture ready","stop_reason":"end_turn"}}),
            ];
            fs::write(
                &transcript,
                rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
            )
            .unwrap();
            let scan = native_probe::scan(NativeProvider::Claude).unwrap();
            assert_eq!(
                db.stage_native_notifications_from_config(
                    NativeProvider::Claude,
                    &scan,
                    &|| Ok(config.clone()),
                    true,
                    true,
                )
                .unwrap(),
                1
            );
            for _ in 0..4 {
                deliver_pending(&|| Ok(config.clone()), &db, None)
                    .await
                    .unwrap();
                assert_eq!(db.pending_native_deliveries(100, None).unwrap().len(), 1);
                assert!(db.list_probe_events(10).unwrap().is_empty());
                assert_eq!(
                    rusqlite::Connection::open(db.path())
                        .unwrap()
                        .query_row(
                            "SELECT COUNT(*) FROM probe_notification_deliveries",
                            [],
                            |r| r.get::<_, u64>(0)
                        )
                        .unwrap(),
                    0
                );
            }
            let prompt_id = if revoked {
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&transcript)
                    .unwrap();
                writeln!(file,"{}",json!({"type":"system","subtype":"turn_duration","sessionId":"fixture","timestamp":timestamp,"pendingBackgroundAgentCount":1})).unwrap();
                drop(file);
                agenthub_core::evict_idle_detail_caches(Duration::ZERO);
                deliver_pending(&|| Ok(config.clone()), &db, None)
                    .await
                    .unwrap();
                assert!(db.pending_native_deliveries(100, None).unwrap().is_empty());
                assert!(db.list_probe_events(10).unwrap().is_empty());
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&transcript)
                    .unwrap();
                writeln!(file,"{}",json!({"type":"user","uuid":"wake","parentUuid":"answer","promptId":"prompt-wake","origin":{"kind":"task-notification"},"sessionId":"fixture","cwd":cwd,"version":"2.1.322","timestamp":timestamp,"message":{"role":"user","content":"Background finished"}})).unwrap();
                writeln!(file,"{}",json!({"type":"assistant","uuid":"final","parentUuid":"wake","sessionId":"fixture","cwd":cwd,"version":"2.1.322","timestamp":timestamp,"message":{"role":"assistant","content":"Fixture ready","stop_reason":"end_turn"}})).unwrap();
                drop(file);
                agenthub_core::evict_idle_detail_caches(Duration::ZERO);
                let final_scan = native_probe::scan(NativeProvider::Claude).unwrap();
                assert_eq!(
                    db.stage_native_notifications_from_config(
                        NativeProvider::Claude,
                        &final_scan,
                        &|| Ok(config.clone()),
                        true,
                        true,
                    )
                    .unwrap(),
                    1
                );
                "prompt-wake"
            } else {
                "prompt-fixture"
            };
            if hook_first {
                let hook = crate::claude_hook::parse(&json!({"session_id":"fixture","prompt_id":prompt_id,"transcript_path":transcript,"hook_event_name":"Stop","last_assistant_message":"Fixture ready","background_tasks":[]})).unwrap().unwrap();
                notification_delivery::claude_hook(
                    &|| Ok(config.clone()),
                    &db,
                    hook,
                    Duration::from_secs(3),
                )
                .await
                .unwrap();
            }
            row["status"] = "idle".into();
            fs::write(&registration, row.to_string()).unwrap();
            agenthub_core::evict_idle_detail_caches(Duration::ZERO);
            deliver_pending(&|| Ok(config.clone()), &db, None)
                .await
                .unwrap();
            deliver_pending(&|| Ok(config.clone()), &db, None)
                .await
                .unwrap();
            assert!(server.request().contains("Fixture ready"));
            let requests: u64 = rusqlite::Connection::open(db.path()).unwrap().query_row("SELECT SUM(attempts) FROM probe_notification_deliveries WHERE event_key LIKE 'native:%'", [], |r| r.get(0)).unwrap();
            assert_eq!(requests, 1);
            assert_eq!(db.list_probe_events(10).unwrap().len(), 1);
            assert!(db.pending_native_deliveries(100, None).unwrap().is_empty());
            drop(owner);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn provider_hook_leaves_other_pending_notifications_and_bodies_intact() {
        let (mut config, sandbox_root) = sandboxed_test_config();
        config.probe.notifications.enabled = true;
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.stage_native_notifications_from_config(
            NativeProvider::Grok,
            &NativeScan::default(),
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();
        let scan = NativeScan {
            errors: 0,
            streams: vec![NativeStreamSnapshot {
                provider: NativeProvider::Grok,
                session_key: "fixture-grok".into(),
                id: "fixture-grok".into(),
                title: "Fixture".into(),
                identity: "fixture-file".into(),
                record_count: 1,
                settled_count: 1,
                events: vec![NativeTurnEvent {
                    prompt_id: None,
                    position: 1,
                    turn_id: "fixture-turn".into(),
                    kind: "completion".into(),
                    body: "Unrelated completion stays pending".into(),
                    timestamp_ms: jiff::Timestamp::now().as_millisecond() + 1000,
                }]
                .into(),
            }],
        };
        assert_eq!(
            db.stage_native_notifications_from_config(
                NativeProvider::Grok,
                &scan,
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap(),
            1
        );
        let before =
            serde_json::to_value(db.pending_native_deliveries(100, None).unwrap()).unwrap();
        deliver_pending(
            &|| Ok(config.clone()),
            &db,
            Some(NativeProvider::Antigravity),
        )
        .await
        .unwrap();
        let after = serde_json::to_value(db.pending_native_deliveries(100, None).unwrap()).unwrap();
        assert_eq!(before, after);
        assert!(db.list_probe_events(10).unwrap().is_empty());
        std::fs::remove_dir_all(sandbox_root).unwrap();
    }

    #[tokio::test]
    async fn claude_candidate_without_a_readable_source_stays_in_the_bounded_queue() {
        let _env_guard = crate::api::test_support::AsyncEnvGuard::lock_read_only().await;
        let (mut config, sandbox_root) = sandboxed_test_config();
        config.probe.notifications.enabled = true;
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        db.stage_native_notifications_from_config(
            NativeProvider::Claude,
            &NativeScan::default(),
            &|| Ok(config.clone()),
            false,
            true,
        )
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
                timestamp_ms: jiff::Timestamp::now().as_millisecond() + 1000,
            }]
            .into(),
        };
        let scan = NativeScan {
            streams: vec![stream],
            errors: 0,
        };
        assert_eq!(
            db.stage_native_notifications_from_config(
                NativeProvider::Claude,
                &scan,
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.stage_native_notifications_from_config(
                NativeProvider::Claude,
                &scan,
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap(),
            0
        );
        deliver_pending(&|| Ok(config.clone()), &db, None)
            .await
            .unwrap();
        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["bark_status"], "queued");
        assert_eq!(
            events[0].payload["bark"]["reason"],
            "claude_terminal_pending"
        );
        db.stage_native_notifications_from_config(
            NativeProvider::Claude,
            &scan,
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();
        assert!(db.pending_native_deliveries(100, None).unwrap().is_empty());
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
            let grok_root = sandbox_root.join("grok");
            let _env =
                crate::api::test_support::AsyncEnvGuard::set_many(&[("GROK_HOME", &grok_root)])
                    .await;
            let db = PanelDb::open_with_secret_box(
                ":memory:",
                agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap();
            db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
                .unwrap();
            db.stage_native_notifications_from_config(
                provider,
                &NativeScan::default(),
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap();
            let stream = crate::api::test_support::write_grok_notification_fixture(
                &grok_root,
                kind,
                "Final fixture answer",
                jiff::Timestamp::now().as_millisecond() + 1000,
            )
            .streams
            .remove(0);
            db.stage_native_notifications_from_config(
                provider,
                &NativeScan {
                    streams: vec![stream.clone()],
                    errors: 0,
                },
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap();
            deliver_pending(&|| Ok(config.clone()), &db, None)
                .await
                .unwrap();
            let request = server.request();
            let capture: Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert!(capture["body"]
                .as_str()
                .unwrap()
                .contains(&stream.events[0].body));
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
            assert_eq!(events[0].payload["session_key"], stream.session_key);
            assert_eq!(events[0].payload["bark_status"], "sent");
            db.stage_native_notifications_from_config(
                provider,
                &NativeScan {
                    streams: vec![stream.clone()],
                    errors: 0,
                },
                &|| Ok(config.clone()),
                false,
                true,
            )
            .unwrap();
            deliver_pending(&|| Ok(config.clone()), &db, None)
                .await
                .unwrap();
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
        let grok_root = sandbox_root.join("grok");
        let _env =
            crate::api::test_support::AsyncEnvGuard::set_many(&[("GROK_HOME", &grok_root)]).await;
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        db.stage_native_notifications_from_config(
            NativeProvider::Grok,
            &NativeScan::default(),
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();
        let mut stream = crate::api::test_support::write_grok_notification_fixture(
            &grok_root,
            "completion",
            "Fixture reply",
            jiff::Timestamp::now().as_millisecond() + 1000,
        )
        .streams
        .remove(0);
        db.stage_native_notifications_from_config(
            NativeProvider::Grok,
            &NativeScan {
                streams: vec![stream.clone()],
                errors: 0,
            },
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();
        deliver_pending(&|| Ok(config.clone()), &db, None)
            .await
            .unwrap();
        assert!(server.request().contains("POST"));
        assert_eq!(
            db.list_probe_events(10).unwrap()[0].payload["bark_status"],
            "queued"
        );
        std::fs::write(
            grok_root
                .join("sessions/workspace")
                .join(&stream.id)
                .join("updates.jsonl"),
            "",
        )
        .unwrap();
        stream.record_count = 3;
        stream.settled_count = 3;
        std::sync::Arc::make_mut(&mut stream.events)[0].position = 3;
        std::sync::Arc::make_mut(&mut stream.events)[0].turn_id = "u:b".into();
        db.stage_native_notifications_from_config(
            NativeProvider::Grok,
            &NativeScan {
                streams: vec![stream],
                errors: 0,
            },
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();
        deliver_pending(&|| Ok(config.clone()), &db, None)
            .await
            .unwrap();
        let events = db.list_probe_events(10).unwrap();
        assert!(events
            .iter()
            .any(|event| event.payload["bark"]["reason"] == "source_changed_or_disabled"));
        let _ = std::fs::remove_dir_all(sandbox_root);
    }

    #[tokio::test]
    async fn claude_wake_turns_and_question_prompts_keep_one_claim_across_hook_and_polling() {
        let server = crate::tests::TestHttpServer::start_n(
            2,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"code\":200}",
        );
        let (mut config, sandbox_root) = sandboxed_test_config();
        config.probe.enabled = true;
        config.probe.notifications.enabled = true;
        config.probe.notifications.server_url = server.url();
        let n = &mut config.probe.notifications;
        n.notify_claude = true;
        n.notify_claude_completion = true;
        n.notify_claude_reply_needed = true;
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();
        let claude_config_dir = sandbox_root.join("claude");
        let _env_guard = crate::api::test_support::AsyncEnvGuard::set_many(&[(
            "CLAUDE_CONFIG_DIR",
            &claude_config_dir,
        )])
        .await;
        db.sync_notification_channels_from_config(&|| Ok(config.clone()))
            .unwrap();
        db.stage_native_notifications_from_config(
            NativeProvider::Claude,
            &NativeScan::default(),
            &|| Ok(config.clone()),
            false,
            true,
        )
        .unwrap();

        let projects = agenthub_core::claude::ClaudePaths::default_for_user()
            .unwrap()
            .projects;
        std::fs::create_dir_all(projects.join("fixture")).unwrap();
        let at = |offset: i64| {
            jiff::Timestamp::from_millisecond(
                jiff::Timestamp::now().as_millisecond() + 1_000 + offset,
            )
            .unwrap()
            .to_string()
        };
        let row = |session: &str,
                   id: &str,
                   parent: Option<&str>,
                   kind: &str,
                   content: Value,
                   stop: &str,
                   offset: i64| {
            serde_json::json!({"type":kind,"uuid":id,"parentUuid":parent,"sessionId":session,"version":"2.1.322","cwd":"/fixture/project","timestamp":at(offset),"message":{"role":kind,"content":content,"stop_reason":stop}})
        };

        // The main turn ends while a delegated agent runs; its wake-up later
        // finishes the thread under the wake-up's own prompt identity.
        let wake_session = "00000000-0000-4000-8000-000000000011";
        let wake_path = projects.join("fixture/wake.jsonl");
        let mut request = row(
            wake_session,
            "request",
            None,
            "user",
            "Delegate fixture".into(),
            "",
            0,
        );
        request["promptId"] = "prompt-human".into();
        request["origin"] = serde_json::json!({"kind":"human"});
        let mut wake = row(
            wake_session,
            "wake",
            Some("launched"),
            "user",
            "<task-notification><status>completed</status></task-notification>".into(),
            "",
            3,
        );
        wake["promptId"] = "prompt-wake".into();
        wake["origin"] = serde_json::json!({"kind":"task-notification"});
        let rows = [
            request,
            row(
                wake_session,
                "launched",
                Some("request"),
                "assistant",
                "Agents launched".into(),
                "end_turn",
                1,
            ),
            serde_json::json!({"type":"system","subtype":"turn_duration","sessionId":wake_session,"durationMs":1,"pendingBackgroundAgentCount":1,"timestamp":at(2)}),
            wake,
            row(
                wake_session,
                "report",
                Some("wake"),
                "assistant",
                "Final fixture report".into(),
                "end_turn",
                4,
            ),
            serde_json::json!({"type":"system","subtype":"turn_duration","sessionId":wake_session,"durationMs":1,"timestamp":at(5)}),
        ];
        std::fs::write(
            &wake_path,
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let stop = |prompt: &str, tasks: Value| {
            crate::claude_hook::parse(&serde_json::json!({
                "session_id": wake_session, "prompt_id": prompt, "transcript_path": wake_path,
                "hook_event_name": "Stop", "last_assistant_message": "Final fixture report",
                "background_tasks": tasks
            }))
            .unwrap()
        };
        assert!(stop(
            "prompt-human",
            serde_json::json!([{"id":"a1","type":"subagent","status":"running","description":"Fixture agent"}])
        )
        .is_none());
        let load = || Ok(config.clone());
        for prompt in ["prompt-human", "prompt-wake"] {
            crate::notification_delivery::claude_hook(
                &load,
                &db,
                stop(prompt, serde_json::json!([])).unwrap(),
                Duration::from_secs(3),
            )
            .await
            .unwrap();
        }
        let events = db.list_probe_events(20).unwrap();
        assert_eq!(events.len(), 1, "the paused main turn must not notify");
        assert_eq!(events[0].kind, "completion");
        assert!(events[0]
            .message
            .as_deref()
            .unwrap()
            .contains("Final fixture report"));

        // An interactive question arrives as a permission prompt and is
        // claimed once, as the question itself.
        let ask_session = "00000000-0000-4000-8000-000000000012";
        let ask_path = projects.join("fixture/ask.jsonl");
        let mut prompt = row(
            ask_session,
            "request",
            None,
            "user",
            "Ask fixture".into(),
            "",
            0,
        );
        prompt["promptId"] = "prompt-ask".into();
        let ask = row(
            ask_session,
            "ask",
            Some("request"),
            "assistant",
            serde_json::json!([{"type":"tool_use","id":"ask-call","name":"AskUserQuestion","input":{"questions":[{"question":"Which fixture option?","options":[{"label":"One"},{"label":"Two"}]}]}}]),
            "tool_use",
            1,
        );
        std::fs::write(&ask_path, format!("{prompt}\n{ask}\n")).unwrap();
        let hook = crate::claude_hook::parse(&serde_json::json!({
            "session_id": ask_session, "prompt_id": "prompt-ask", "transcript_path": ask_path,
            "hook_event_name": "Notification", "notification_type": "permission_prompt",
            "message": "Permission fixture notice"
        }))
        .unwrap()
        .unwrap();
        assert_eq!(hook.kind, "authorization");
        crate::notification_delivery::claude_hook(&load, &db, hook, Duration::from_secs(3))
            .await
            .unwrap();
        let events = db.list_probe_events(20).unwrap();
        assert_eq!(events.len(), 2);
        let question = events.iter().find(|e| e.kind == "reply_needed").unwrap();
        assert!(question
            .message
            .as_deref()
            .unwrap()
            .contains("Which fixture option?"));
        assert!(events.iter().all(|e| e.kind != "authorization"));

        // Polling finds the same turn and question; both claims already exist,
        // so nothing is sent again (the Bark fixture accepts two requests only).
        let scan = scan(NativeProvider::Claude).await.unwrap();
        assert_eq!(scan.errors, 0);
        db.stage_native_notifications_from_config(NativeProvider::Claude, &scan, &load, true, true)
            .unwrap();
        deliver_pending(&load, &db, None).await.unwrap();
        let events = db.list_probe_events(20).unwrap();
        assert!(events.iter().all(|e| e.kind != "authorization"));
        assert!(
            events.iter().all(|e| e.payload["bark_status"] == "sent"),
            "a second claim would reach the closed Bark fixture and fail"
        );
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

        let db = PanelDb::open_with_secret_box(
            ":memory:",
            agenthub_core::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        db.set_secret_setting_bytes("probe_bark_device_key", b"fixture-device")
            .unwrap();

        // Saving activation precedes every first hook/poll; no baseline scan has run.
        let config_path = sandbox_root.join("config.toml");
        let mut before = config.clone();
        before.probe.notifications.enabled = false;
        before.security.secret_key = "11".repeat(32);
        std::fs::write(&config_path, toml::to_string(&before).unwrap()).unwrap();
        config = agenthub_core::services::settings::apply_probe_settings_save_plan(
            &config_path,
            &db,
            agenthub_core::services::settings::ProbeSettingsSavePlan {
                config_patch: serde_json::from_value(
                    serde_json::json!({"probe":{"notifications":{"enabled":true}}}),
                )
                .unwrap(),
                secret_write: None,
            },
        )
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
        let codex_event = crate::build_probe_event(&codex_config, codex_input);
        let (codex_outcome, _) =
            crate::record_probe_event_with_bark(&|| Ok(codex_config.clone()), &db, codex_event)
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

        let start_ms = jiff::Timestamp::now().as_millisecond() + 1_000;
        let now_ms = start_ms + 1_000;
        let start_rfc3339 = jiff::Timestamp::from_millisecond(start_ms)
            .unwrap()
            .to_string();
        let now_rfc3339 = jiff::Timestamp::from_millisecond(now_ms)
            .unwrap()
            .to_string();

        // 2. Claude Code: real JSONL transcript + claude_hook::parse + notification_delivery::claude_hook
        let claude_paths = agenthub_core::claude::ClaudePaths::default_for_user().unwrap();
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
            "hook_event_name": "Notification",
            "notification_type": "idle_prompt",
            "message": "Claude is waiting"
        }))
        .unwrap()
        .expect("real claude hook parser must accept idle_prompt payload");
        assert_eq!(claude_hook_event.kind, "completion");
        crate::notification_delivery::claude_hook(
            &|| Ok(config.clone()),
            &db,
            claude_hook_event,
            Duration::from_secs(3),
        )
        .await
        .unwrap();

        // 3. Grok: real summary.json + events.jsonl + updates.jsonl + native_probe::scan(Grok)
        let grok_paths = agenthub_core::grok::GrokPaths::default_for_user().unwrap();
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
        let dsh_paths = agenthub_core::dsh::DshPaths::default_for_user().unwrap();
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
            assert_eq!(
                db.stage_native_notifications_from_config(
                    provider,
                    &scan,
                    &|| Ok(config.clone()),
                    false,
                    provider != NativeProvider::Antigravity
                )
                .unwrap(),
                1,
                "provider {} must stage 1 completion notification",
                provider.as_str()
            );
            deliver_pending(&|| Ok(config.clone()), &db, None)
                .await
                .unwrap();
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
