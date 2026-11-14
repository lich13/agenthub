use super::PanelDb;
use crate::{
    config::Config,
    native_probe::{NativeProvider, NativeScan, NativeTurnEvent},
    security::redact_output,
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeDelivery {
    pub event_key: String,
    pub provider: NativeProvider,
    pub session_key: String,
    pub thread_id: String,
    pub title: String,
    pub event: NativeTurnEvent,
}

fn identity_record(delivery: &NativeDelivery, identity: &str) -> Result<String> {
    let mut record = serde_json::to_value(delivery)?;
    record["title"] = json!("");
    record["event"]["body"] = json!("");
    record["stream_identity"] = json!(identity);
    Ok(record.to_string())
}

impl PanelDb {
    fn decode_native_payload(&self, ciphertext: Vec<u8>, nonce: Vec<u8>) -> Result<NativeDelivery> {
        Ok(serde_json::from_slice(
            &self.crypto.decrypt(&ciphertext, &nonce)?,
        )?)
    }

    /// Queue events and advance cursors together. First enable baselines every
    /// existing stream; new streams also ignore records older than activation.
    pub fn stage_native_notifications(
        &self,
        provider: NativeProvider,
        scan: &NativeScan,
        config: &Config,
    ) -> Result<usize> {
        self.stage_native_notifications_with_hooks(provider, scan, config, false)
    }

    pub fn stage_native_notifications_with_hooks(
        &self,
        provider: NativeProvider,
        scan: &NativeScan,
        config: &Config,
        local_claude_hooks: bool,
    ) -> Result<usize> {
        let instance = if local_claude_hooks {
            Some(self.notification_instance_id()?)
        } else {
            None
        };
        let enabled = provider.enabled(config);
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior = tx
            .query_row(
                "SELECT enabled,activated_ms,scanned_at FROM native_probe_providers WHERE provider=?1",
                [provider.as_str()],
                |r| {
                    Ok((
                        r.get::<_, bool>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2).unwrap_or(0),
                    ))
                },
            )
            .optional()?;
        let baseline = enabled && prior.is_none_or(|(was_enabled, _, _)| !was_enabled);
        let activated_ms = if baseline {
            chrono::Utc::now().timestamp_millis()
        } else {
            prior.map_or(0, |(_, ts, _)| ts)
        };
        let prior_scanned_at = if baseline {
            0
        } else {
            prior.map_or(0, |(_, _, scanned)| scanned)
        };
        tx.execute("INSERT INTO native_probe_providers(provider,enabled,activated_ms,scanned_at,stream_count,error_count) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(provider) DO UPDATE SET enabled=excluded.enabled,activated_ms=excluded.activated_ms,scanned_at=excluded.scanned_at,stream_count=excluded.stream_count,error_count=excluded.error_count WHERE enabled!=excluded.enabled OR activated_ms!=excluded.activated_ms OR stream_count!=excluded.stream_count OR error_count!=excluded.error_count OR (excluded.enabled AND scanned_at!=excluded.scanned_at)",
            params![provider.as_str(), enabled, activated_ms, Self::now(), scan.streams.len(), scan.errors])?;
        if !enabled {
            tx.execute("UPDATE native_probe_deliveries SET status='skipped',ciphertext=NULL,nonce=NULL,updated_at=?2 WHERE provider=?1 AND status='pending'", params![provider.as_str(),Self::now()])?;
            tx.commit()?;
            return Ok(0);
        }
        let mut queued = 0;
        for stream in &scan.streams {
            let old = tx.query_row("SELECT identity,cursor FROM native_probe_streams WHERE provider=?1 AND session_key=?2", params![provider.as_str(),stream.session_key], |r| Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?))).optional()?;
            let reset = baseline
                || old.as_ref().is_some_and(|(identity, cursor)| {
                    identity != &stream.identity
                        || (provider != NativeProvider::GrokBot && *cursor > stream.record_count)
                });
            // Antigravity mutates a waiting step in place. A new request on a
            // previously baselined step keeps that step's original timestamp.
            let known_mutable_source = provider == NativeProvider::Antigravity && old.is_some();
            let cursor = if reset {
                if provider.local_only() {
                    stream.settled_count.min(stream.record_count)
                } else {
                    stream.record_count
                }
            } else {
                old.as_ref().map_or(0, |(_, cursor)| *cursor)
            };
            if !reset || provider.local_only() {
                let now = Self::now();
                for event in stream.events.iter() {
                    let cursor_exempt =
                        provider == NativeProvider::GrokBot && event.kind != "completion";
                    let unrecorded_at_cursor = known_mutable_source
                        && event.position == cursor
                        && event.timestamp_ms >= activated_ms;
                    if !reset
                        && ((!cursor_exempt && !unrecorded_at_cursor && event.position <= cursor)
                            || (!known_mutable_source && event.timestamp_ms < activated_ms)
                            || (!provider.local_only()
                                && !provider.event_enabled(config, &event.kind)))
                    {
                        continue;
                    }
                    let upgrade_historical_grokbot_completion = provider == NativeProvider::GrokBot
                        && event.kind == "completion"
                        && old.as_ref().is_some_and(|(_, old_cursor)| *old_cursor == 0)
                        && prior_scanned_at > 0
                        && event.timestamp_ms < prior_scanned_at.saturating_mul(1000);
                    let delivery = NativeDelivery {
                        event_key: if let (NativeProvider::Claude, Some(instance), Some(prompt)) =
                            (provider, instance.as_deref(), event.prompt_id.as_deref())
                        {
                            stream.prompt_event_key(prompt, &event.kind, instance)
                        } else {
                            stream.event_key(event)
                        },
                        provider,
                        session_key: stream.session_key.clone(),
                        thread_id: stream.id.clone(),
                        title: redact_output(&stream.title),
                        event: event.clone(),
                    };
                    let status = if reset
                        || upgrade_historical_grokbot_completion
                        || !provider.event_enabled(config, &event.kind)
                    {
                        "skipped"
                    } else {
                        "pending"
                    };
                    let inserted = tx.execute("INSERT OR IGNORE INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)", params![delivery.event_key,provider.as_str(),identity_record(&delivery, &stream.identity)?,status,now])?;
                    if inserted > 0 && status == "pending" {
                        let (ciphertext, nonce) =
                            self.crypto.encrypt(&serde_json::to_vec(&delivery)?)?;
                        tx.execute("UPDATE native_probe_deliveries SET ciphertext=?2,nonce=?3 WHERE event_key=?1", params![delivery.event_key,ciphertext,nonce])?;
                    }
                    if inserted == 0 && event.position > stream.settled_count {
                        tx.execute(
                            "UPDATE native_probe_deliveries SET updated_at=?2 WHERE event_key=?1 AND status IN ('sent','failed','skipped') AND updated_at<?3",
                            params![delivery.event_key, now, now - 3600],
                        )?;
                    }
                    if status == "pending" {
                        queued += inserted;
                    }
                }
            }
            let cursor = cursor.max(stream.settled_count.min(stream.record_count));
            tx.execute("INSERT INTO native_probe_streams(provider,session_key,identity,cursor,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(provider,session_key) DO UPDATE SET identity=excluded.identity,cursor=excluded.cursor,updated_at=excluded.updated_at WHERE identity!=excluded.identity OR cursor!=excluded.cursor",
                params![provider.as_str(),stream.session_key,stream.identity,cursor,Self::now()])?;
        }
        tx.commit()?;
        Ok(queued)
    }

    pub fn native_probe_stream_cursor(
        &self,
        provider: NativeProvider,
        session_key: &str,
    ) -> Result<Option<u64>> {
        let conn = self.conn.lock().expect("db mutex");
        Ok(conn
            .query_row(
                "SELECT cursor FROM native_probe_streams WHERE provider=?1 AND session_key=?2",
                params![provider.as_str(), session_key],
                |r| r.get::<_, u64>(0),
            )
            .optional()?)
    }

    pub fn claude_legacy_delivery_keys(
        &self,
        session_key: &str,
        turn: &str,
        kind: &str,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut statement = conn.prepare("SELECT event_key,event_json FROM native_probe_deliveries WHERE provider='claude_code' AND json_valid(event_json) AND json_extract(event_json,'$.session_key')=?1 AND (json_extract(event_json,'$.event.turn_id')=?2 OR instr(json_extract(event_json,'$.event.turn_id'),?2 || ':')=1)")?;
        let mut keys = Vec::new();
        for row in statement.query_map(params![session_key, turn], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (key, value) = row?;
            let delivery: NativeDelivery = serde_json::from_str(&value)?;
            if crate::native_probe::event_category(&delivery.event) == kind {
                keys.push(format!("native:{key}"));
            }
        }
        Ok(keys)
    }

    /// Idle is only a fallback for a new terminal record, never a way to replay
    /// a stream that was baselined when notifications were enabled.
    pub fn claude_idle_after_baseline(
        &self,
        stream: &crate::native_probe::NativeStreamSnapshot,
        event: &NativeTurnEvent,
    ) -> Result<bool> {
        let conn = self.conn.lock().expect("db mutex");
        let active: bool = conn.query_row("SELECT enabled=1 AND activated_ms<=?1 FROM native_probe_providers WHERE provider='claude_code'", [event.timestamp_ms], |r| r.get(0)).optional()?.unwrap_or(false);
        if !active {
            return Ok(false);
        }
        let prior: Option<(String,u64)> = conn.query_row("SELECT identity,cursor FROM native_probe_streams WHERE provider='claude_code' AND session_key=?1", [&stream.session_key], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        Ok(prior.is_none_or(|(identity, cursor)| {
            identity == stream.identity && event.position > cursor
        }))
    }

    pub fn pending_native_deliveries(
        &self,
        limit: usize,
        provider: Option<NativeProvider>,
    ) -> Result<Vec<NativeDelivery>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare("SELECT ciphertext,nonce FROM native_probe_deliveries WHERE status='pending' AND (?2 IS NULL OR provider=?2) ORDER BY created_at,event_key LIMIT ?1")?;
        let rows = stmt
            .query_map(
                params![limit.min(100), provider.map(NativeProvider::as_str)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(ciphertext, nonce)| self.decode_native_payload(ciphertext, nonce))
            .collect()
    }
    pub fn claim_native_delivery(&self, key: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("db mutex");
        Ok(conn.execute("UPDATE native_probe_deliveries SET status='delivering',updated_at=?2 WHERE event_key=?1 AND status='pending'",params![key,Self::now()])?==1)
    }
    /// Delivery confirmation and the visible Probe event commit atomically.
    pub fn finish_native_delivery(&self, delivery: &NativeDelivery, bark: Value) -> Result<()> {
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved: Option<String> = tx.query_row(
            "SELECT outcome_json FROM probe_notification_deliveries WHERE event_key=?1 AND channel='bark'",
            [format!("native:{}", delivery.event_key)], |r| r.get(0)).optional()?.flatten();
        // Another sender may have advanced the durable queue while this caller was suspended.
        let bark = saved
            .map(|value| serde_json::from_str(&value))
            .transpose()?
            .unwrap_or(bark);
        let status = if matches!(bark["status"].as_str(), Some("waiting_retry" | "retrying")) {
            "queued"
        } else if bark["status"] == "unknown" {
            "unknown"
        } else if bark.get("sent").and_then(Value::as_bool) == Some(true) {
            "sent"
        } else if bark.get("skipped").and_then(Value::as_bool) == Some(true) {
            "skipped"
        } else {
            "failed"
        };
        let changed=tx.execute("UPDATE native_probe_deliveries SET status=?2,ciphertext=NULL,nonce=NULL,updated_at=?3 WHERE event_key=?1 AND status='delivering'",params![delivery.event_key,status,Self::now()])?;
        if changed > 0 {
            let summary = delivery.event.body.chars().take(240).collect::<String>();
            let payload = json!({"provider":delivery.provider,"session_key":delivery.session_key,"thread_id":delivery.thread_id,"turn_id":delivery.event.turn_id,"thread_title":delivery.title,"event_type":delivery.event.kind,"body_summary":summary,"body_source":"native_terminal_turn","bark":bark,"bark_status":status,"notification_event_key":format!("native:{}",delivery.event_key)});
            tx.execute("INSERT INTO probe_events(id,kind,thread_id,title,message,dedupe_key,source,payload_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,'native_provider_monitor',?7,?8)",params![uuid::Uuid::new_v4().to_string(),delivery.event.kind,delivery.thread_id,delivery.title,summary,delivery.event_key,payload.to_string(),Self::now()])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn recover_interrupted_native_deliveries(&self) -> Result<()> {
        let deliveries = {
            let conn = self.conn.lock().expect("db mutex");
            let mut stmt=conn.prepare("SELECT ciphertext,nonce FROM native_probe_deliveries WHERE status='delivering' AND updated_at < ?1")?;
            let rows = stmt
                .query_map([Self::now() - 120], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|(ciphertext, nonce)| self.decode_native_payload(ciphertext, nonce))
                .collect::<Result<Vec<_>>>()?
        };
        for delivery in deliveries {
            // The channel ledger owns recovery. Never create a second delivery here.
            let outcome=self.notification_delivery_outcome(super::NotificationChannel::Bark,&format!("native:{}",delivery.event_key))?
                .unwrap_or_else(||json!({"sent":false,"skipped":false,"status":"unknown","reason":"delivery_interrupted_outcome_unknown"}));
            self.finish_native_delivery(&delivery, outcome)?;
        }
        Ok(())
    }
    pub fn native_notification_status(&self) -> Result<Value> {
        let conn = self.conn.lock().expect("db mutex");
        let mut counts = conn.prepare("SELECT provider, SUM(status='failed'), SUM(status IN ('pending','delivering','queued')) FROM native_probe_deliveries WHERE status IN ('failed','pending','delivering','queued') GROUP BY provider")?;
        let counts = counts
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, u64>(1)?, row.get::<_, u64>(2)?),
                ))
            })?
            .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;
        let mut statuses = Vec::new();
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
            let row=conn.query_row("SELECT enabled,scanned_at,stream_count,error_count FROM native_probe_providers WHERE provider=?1",[provider.as_str()],|r|Ok((r.get::<_,bool>(0)?,r.get::<_,i64>(1)?,r.get::<_,u64>(2)?,r.get::<_,u64>(3)?))).optional()?;
            let (enabled, last_scan, streams, errors) = row.unwrap_or((false, 0, 0, 0));
            let (failed, pending) = counts.get(provider.as_str()).copied().unwrap_or_default();
            statuses.push(json!({"provider":provider,"enabled":enabled,"last_scan_at":last_scan,"streams":streams,"read_errors":errors,"failed_deliveries":failed,"pending_deliveries":pending,"failure_supported":true}));
        }
        Ok(Value::Array(statuses))
    }
    pub fn maintain_notification_history_if_due(&self, retention_days: u32) -> Result<()> {
        let now = Self::now();
        let last = self
            .get_setting_with_updated_at("probe_event_retention_last_run")?
            .map(|(_, at)| at)
            .unwrap_or(0);
        if now.saturating_sub(last) < 3600 {
            return Ok(());
        }
        self.maintain_probe_events(retention_days, 100_000)?;
        let marker_cutoff = now - i64::from(retention_days.clamp(7, 3650)) * 86_400;
        {
            let conn = self.conn.lock().expect("db mutex");
            conn.execute(
                "DELETE FROM native_probe_deliveries WHERE status IN ('sent','failed','skipped','unknown') AND updated_at < ?1
                 AND NOT EXISTS (SELECT 1 FROM native_probe_streams s
                    WHERE s.provider=native_probe_deliveries.provider
                    AND s.session_key=json_extract(event_json,'$.session_key')
                    AND s.identity=json_extract(event_json,'$.stream_identity')
                    AND (s.cursor<json_extract(event_json,'$.event.position')
                        OR (s.provider='antigravity' AND s.cursor=json_extract(event_json,'$.event.position'))))",
                [marker_cutoff],
            )?;
        }
        self.set_setting("probe_event_retention_last_run", "ok")?;
        let conn = self.conn.lock().expect("db mutex");
        let _ = conn.execute_batch("PRAGMA optimize;");
        let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_probe::{NativeStreamSnapshot, NativeTurnEvent};

    #[test]
    fn pending_provider_filter_precedes_limit_and_decryption() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let config = config();
        db.stage_native_notifications(NativeProvider::Grok, &NativeScan::default(), &config)
            .unwrap();
        let scan = scan("target", 1, chrono::Utc::now().timestamp_millis() + 1000);
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &scan, &config)
                .unwrap(),
            1
        );
        {
            let conn = db.conn.lock().unwrap();
            for i in 0..101 {
                // Unrelated payloads must never be loaded or decrypted by this query.
                conn.execute("INSERT INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at) VALUES(?1,'claude_code','{}','pending',0,0)", [format!("unrelated-{i}")]).unwrap();
            }
        }
        let pending = db
            .pending_native_deliveries(1, Some(NativeProvider::Grok))
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].session_key, "target");
        assert_eq!(pending[0].event.body, "test reply");
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT count(*) FROM native_probe_deliveries WHERE provider='claude_code' AND status='pending'", [], |r| r.get::<_, usize>(0)).unwrap(), 101);
    }

    #[test]
    fn notification_counts_preserve_states_provider_order_and_zero_values() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let statuses = [
            "failed",
            "pending",
            "delivering",
            "queued",
            "sent",
            "skipped",
            "unknown",
        ];
        {
            let conn = db.conn.lock().unwrap();
            for (i, status) in statuses.iter().enumerate() {
                conn.execute("INSERT INTO native_probe_deliveries(event_key,provider,event_json,status,created_at,updated_at) VALUES(?1,'grok','{}',?2,0,0)", params![format!("fixture-{i}"),status]).unwrap();
            }
        }
        let rows = db.native_notification_status().unwrap();
        let rows = rows.as_array().unwrap();
        let expected: Vec<_> = [
            NativeProvider::Grok,
            NativeProvider::GrokBot,
            NativeProvider::Claude,
            NativeProvider::Antigravity,
            NativeProvider::Dsh,
        ]
        .into_iter()
        .filter(|p| !p.local_only() || cfg!(target_os = "macos"))
        .map(NativeProvider::as_str)
        .collect();
        assert_eq!(
            rows.iter()
                .map(|r| r["provider"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(rows[0]["failed_deliveries"], 1);
        assert_eq!(rows[0]["pending_deliveries"], 3);
        for row in &rows[1..] {
            assert_eq!(row["failed_deliveries"], 0);
            assert_eq!(row["pending_deliveries"], 0);
        }
    }

    #[test]
    fn idle_scan_does_not_rewrite_streams_or_publish_history() {
        use rusqlite::hooks::{AuthAction, Authorization};
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let config = config();
        let scan = scan("fixture-idle", 1, chrono::Utc::now().timestamp_millis());
        db.stage_native_notifications(NativeProvider::Grok, &scan, &config)
            .unwrap();
        db.sync_notification_channels(&config).unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_idle_stream_update BEFORE UPDATE ON native_probe_streams
            WHEN OLD.identity=NEW.identity AND OLD.cursor=NEW.cursor
            BEGIN SELECT RAISE(ABORT,'idle stream update'); END;",
            )
            .unwrap();
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &scan, &config)
                .unwrap(),
            0
        );
        db.conn
            .lock()
            .unwrap()
            .authorizer(Some(
                |context: rusqlite::hooks::AuthContext<'_>| match context.action {
                    AuthAction::Read {
                        table_name: "probe_events" | "probe_error_incidents",
                        ..
                    } => Authorization::Deny,
                    _ => Authorization::Allow,
                },
            ))
            .unwrap();
        db.sync_notification_channels(&config).unwrap();
        assert!(db.due_notification_deliveries(10).unwrap().is_empty());
    }

    #[test]
    fn retention_keeps_mutable_cursor_identity_without_body_then_prunes_unknown_after_progress() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let mut config = config();
        config.probe.notifications.notify_antigravity = true;
        config.probe.notifications.notify_antigravity_completion = true;
        let old_ms = (PanelDb::now() - 8 * 86_400) * 1000;
        let mut scan = scan("fixture-ag", 5, old_ms);
        scan.streams[0].provider = NativeProvider::Antigravity;
        std::sync::Arc::make_mut(&mut scan.streams[0].events).clear();
        db.stage_native_notifications(NativeProvider::Antigravity, &scan, &config)
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE native_probe_providers SET activated_ms=1", [])
            .unwrap();
        std::sync::Arc::make_mut(&mut scan.streams[0].events).push(NativeTurnEvent {
            prompt_id: None,
            position: 5,
            turn_id: "finished".into(),
            kind: "completion".into(),
            body: "private fixture".repeat(80),
            timestamp_ms: old_ms,
        });
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Antigravity, &scan, &config)
                .unwrap(),
            1
        );
        let delivery = db.pending_native_deliveries(1, None).unwrap().remove(0);
        let raw: String = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT event_json FROM native_probe_deliveries",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!raw.contains("private fixture"));
        db.claim_native_delivery(&delivery.event_key).unwrap();
        db.finish_native_delivery(&delivery, json!({"status":"unknown","sent":false}))
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE native_probe_deliveries SET updated_at=?1",
                [old_ms / 1000],
            )
            .unwrap();
        db.maintain_notification_history_if_due(7).unwrap();
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Antigravity, &scan, &config)
                .unwrap(),
            0
        );
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT count(*) FROM native_probe_deliveries WHERE status='unknown' AND ciphertext IS NULL AND nonce IS NULL", [], |row| row.get::<_,usize>(0)).unwrap(), 1);
        scan.streams[0].record_count = 6;
        scan.streams[0].settled_count = 6;
        std::sync::Arc::make_mut(&mut scan.streams[0].events).clear();
        db.stage_native_notifications(NativeProvider::Antigravity, &scan, &config)
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM settings WHERE key='probe_event_retention_last_run'",
                [],
            )
            .unwrap();
        db.maintain_notification_history_if_due(7).unwrap();
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM native_probe_deliveries", [], |row| {
                    row.get::<_, usize>(0)
                })
                .unwrap(),
            0
        );
    }
    #[test]
    fn claude_enable_baselines_old_streams_and_keeps_channel_delivery_ownership() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let config = config();
        let now = chrono::Utc::now().timestamp_millis() + 1000;
        let mut old = scan("fixture", 1, now);
        old.streams[0].provider = NativeProvider::Claude;
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Claude, &old, &config)
                .unwrap(),
            0
        );
        old.streams[0].record_count = 2;
        old.streams[0].settled_count = 2;
        old.streams[0].events = vec![NativeTurnEvent {
            prompt_id: None,
            position: 2,
            turn_id: "new-turn".into(),
            kind: "completion".into(),
            body: "Fixture complete".into(),
            timestamp_ms: now,
        }]
        .into();
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Claude, &old, &config)
                .unwrap(),
            1
        );
        let delivery = db
            .pending_native_deliveries(10, None)
            .unwrap()
            .pop()
            .unwrap();
        assert!(db.claim_native_delivery(&delivery.event_key).unwrap());
        db.finish_native_delivery(
            &delivery,
            json!({"sent":false,"skipped":false,"http_status":503}),
        )
        .unwrap();
        assert!(!db.claim_native_delivery(&delivery.event_key).unwrap());
        assert!(db.pending_native_deliveries(10, None).unwrap().is_empty());
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT status FROM native_probe_deliveries", [], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "failed"
        );
    }

    fn config() -> Config {
        let mut c = Config::default();
        c.probe.notifications.enabled = true;
        c
    }
    fn scan(id: &str, count: u64, now: i64) -> NativeScan {
        NativeScan {
            errors: 0,
            streams: vec![NativeStreamSnapshot {
                provider: NativeProvider::Grok,
                session_key: id.into(),
                id: "duplicate-native-id".into(),
                title: "test".into(),
                identity: id.into(),
                record_count: count,
                settled_count: count,
                events: (1..=count)
                    .map(|n| NativeTurnEvent {
                        prompt_id: None,
                        position: n,
                        turn_id: format!("turn-{n}"),
                        kind: "completion".into(),
                        body: "test reply".into(),
                        timestamp_ms: now,
                    })
                    .collect::<Vec<_>>()
                    .into(),
            }],
        }
    }
    #[test]
    fn baseline_restart_partial_failure_and_duplicate_native_ids_use_durable_keys() {
        let p = std::env::temp_dir().join(format!("native-db-{}.sqlite", uuid::Uuid::new_v4()));
        let db = PanelDb::open_with_secret_box(
            &p,
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let c = config();
        let now = chrono::Utc::now().timestamp_millis() + 1000;
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &scan("a", 1, now), &c)
                .unwrap(),
            0
        );
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &scan("a", 2, now), &c)
                .unwrap(),
            1
        );
        let d = db
            .pending_native_deliveries(100, None)
            .unwrap()
            .pop()
            .unwrap();
        assert!(db.claim_native_delivery(&d.event_key).unwrap());
        assert!(!db.claim_native_delivery(&d.event_key).unwrap());
        db.finish_native_delivery(&d, json!({"sent":true})).unwrap();
        drop(db);
        let db = PanelDb::open_with_secret_box(
            &p,
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &scan("a", 2, now), &c)
                .unwrap(),
            0
        );
        assert!(db.pending_native_deliveries(100, None).unwrap().is_empty());
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &scan("b", 1, now), &c)
                .unwrap(),
            1
        );
        let other = db
            .pending_native_deliveries(100, None)
            .unwrap()
            .pop()
            .unwrap();
        assert_ne!(d.event_key, other.event_key);
        db.claim_native_delivery(&other.event_key).unwrap();
        db.finish_native_delivery(
            &other,
            json!({"sent":false,"skipped":false,"reason":"http_status"}),
        )
        .unwrap();
        assert_eq!(
            db.native_notification_status()
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["provider"] == "grok")
                .unwrap()["failed_deliveries"],
            1
        );
        assert_eq!(db.list_probe_events(10).unwrap().len(), 2);
        drop(db);
        std::fs::remove_file(p).unwrap();
    }
    #[test]
    fn disabled_poll_preserves_database_until_statistics_change() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let mut c = config();
        c.probe.notifications.notify_grok = false;
        let mut empty = NativeScan {
            streams: vec![],
            errors: 0,
        };
        db.stage_native_notifications(NativeProvider::Grok, &empty, &c)
            .unwrap();
        let changes = {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE native_probe_providers SET scanned_at=1", [])
                .unwrap();
            conn.query_row("SELECT total_changes()", [], |row| row.get::<_, u64>(0))
                .unwrap()
        };
        for _ in 0..3 {
            db.stage_native_notifications(NativeProvider::Grok, &empty, &c)
                .unwrap();
        }
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT total_changes()", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            changes
        );
        empty.errors = 1;
        db.stage_native_notifications(NativeProvider::Grok, &empty, &c)
            .unwrap();
        assert_eq!(
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT total_changes()", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            changes + 1
        );
        c.probe.notifications.notify_grok = true;
        db.stage_native_notifications(NativeProvider::Grok, &empty, &c)
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE native_probe_providers SET scanned_at=1", [])
            .unwrap();
        db.stage_native_notifications(NativeProvider::Grok, &empty, &c)
            .unwrap();
        let scanned: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT scanned_at FROM native_probe_providers", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(scanned > 1);
    }

    #[test]
    fn disabled_provider_and_reenabled_provider_never_replay_history() {
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let mut c = config();
        let now = chrono::Utc::now().timestamp_millis() + 1000;
        db.stage_native_notifications(NativeProvider::Grok, &scan("a", 1, now), &c)
            .unwrap();
        c.probe.notifications.notify_grok = false;
        db.stage_native_notifications(NativeProvider::Grok, &scan("a", 2, now), &c)
            .unwrap();
        c.probe.notifications.notify_grok = true;
        db.stage_native_notifications(NativeProvider::Grok, &scan("a", 3, now), &c)
            .unwrap();
        assert!(db.pending_native_deliveries(100, None).unwrap().is_empty());
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &scan("a", 4, now), &c)
                .unwrap(),
            1
        );
        let old = scan("imported", 20, now - 86_400_000);
        assert_eq!(
            db.stage_native_notifications(NativeProvider::Grok, &old, &c)
                .unwrap(),
            0
        );
    }
}

#[cfg(test)]
mod local_provider_tests {
    use super::*;
    use crate::native_probe::NativeStreamSnapshot;
    fn config() -> Config {
        let mut c = Config::default();
        c.probe.enabled = true;
        c.probe.notifications.enabled = true;
        let n = &mut c.probe.notifications;
        n.notify_antigravity = true;
        n.notify_antigravity_completion = true;
        n.notify_antigravity_failure = true;
        n.notify_antigravity_reply_needed = true;
        n.notify_antigravity_authorization = true;
        n.notify_dsh = true;
        n.notify_dsh_completion = true;
        n.notify_dsh_failure = true;
        n.notify_dsh_reply_needed = true;
        n.notify_dsh_authorization = true;
        c
    }
    fn scan(provider: NativeProvider, kind: &str, request: &str) -> NativeScan {
        NativeScan {
            errors: 0,
            streams: vec![NativeStreamSnapshot {
                provider,
                session_key: "fixture".into(),
                id: "fixture".into(),
                title: "Fixture".into(),
                identity: "source".into(),
                record_count: 2,
                settled_count: 0,
                events: vec![NativeTurnEvent {
                    prompt_id: None,
                    position: 2,
                    turn_id: request.into(),
                    kind: kind.into(),
                    body: "Fixture question".into(),
                    timestamp_ms: chrono::Utc::now().timestamp_millis() + 1000,
                }]
                .into(),
            }],
        }
    }
    #[test]
    fn pending_baseline_restart_and_reenable_preserve_dedupe_without_losing_same_step_completion() {
        for provider in [NativeProvider::Antigravity, NativeProvider::Dsh] {
            let mut c = config();
            let db = PanelDb::open_with_secret_box(
                ":memory:",
                crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap();
            let old = scan(provider, "reply_needed", "execution-1");
            assert_eq!(
                db.stage_native_notifications(provider, &old, &c).unwrap(),
                0
            );
            assert_eq!(
                db.stage_native_notifications(provider, &old, &c).unwrap(),
                0
            );
            let completed = scan(provider, "completion", "execution-1");
            assert_eq!(
                db.stage_native_notifications(provider, &completed, &c)
                    .unwrap(),
                1
            );
            assert_eq!(
                db.stage_native_notifications(provider, &completed, &c)
                    .unwrap(),
                0
            );
            c.probe.notifications.enabled = false;
            db.stage_native_notifications(provider, &NativeScan::default(), &c)
                .unwrap();
            c.probe.notifications.enabled = true;
            let new = scan(provider, "authorization", "request-2");
            assert_eq!(
                db.stage_native_notifications(provider, &new, &c).unwrap(),
                0
            );
            assert_eq!(
                db.stage_native_notifications(provider, &new, &c).unwrap(),
                0
            );
            let later = scan(provider, "authorization", "request-3");
            assert_eq!(
                db.stage_native_notifications(provider, &later, &c).unwrap(),
                1
            );
        }
    }
    #[test]
    fn disabled_categories_do_not_replay_existing_waits_when_enabled() {
        for provider in [NativeProvider::Antigravity, NativeProvider::Dsh] {
            let mut c = config();
            let db = PanelDb::open_with_secret_box(
                ":memory:",
                crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            )
            .unwrap();
            db.stage_native_notifications(provider, &NativeScan::default(), &c)
                .unwrap();
            c.probe.notifications.notify_antigravity_reply_needed = false;
            c.probe.notifications.notify_dsh_reply_needed = false;
            let waiting = scan(provider, "reply_needed", "request");
            assert_eq!(
                db.stage_native_notifications(provider, &waiting, &c)
                    .unwrap(),
                0
            );
            c.probe.notifications.notify_antigravity_reply_needed = true;
            c.probe.notifications.notify_dsh_reply_needed = true;
            assert_eq!(
                db.stage_native_notifications(provider, &waiting, &c)
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn new_interaction_on_a_baselined_step_keeps_its_original_creation_time() {
        let provider = NativeProvider::Antigravity;
        let c = config();
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();
        let mut waiting = scan(provider, "reply_needed", "old-request");
        std::sync::Arc::make_mut(&mut waiting.streams[0].events)[0].timestamp_ms = 1;
        assert_eq!(
            db.stage_native_notifications(provider, &waiting, &c)
                .unwrap(),
            0
        );
        assert_eq!(
            db.stage_native_notifications(provider, &waiting, &c)
                .unwrap(),
            0
        );
        std::sync::Arc::make_mut(&mut waiting.streams[0].events)[0].turn_id = "new-request".into();
        assert_eq!(
            db.stage_native_notifications(provider, &waiting, &c)
                .unwrap(),
            1
        );
        assert_eq!(
            db.stage_native_notifications(provider, &waiting, &c)
                .unwrap(),
            0
        );
    }

    #[test]
    fn unrecorded_event_at_cursor_queues_once_when_post_activation_and_skips_pre_activation_or_before_cursor(
    ) {
        let provider = NativeProvider::Antigravity;
        let c = config();
        let db = PanelDb::open_with_secret_box(
            ":memory:",
            crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
        )
        .unwrap();

        // Baseline stream at record_count = 5, settled_count = 5 -> cursor = 5
        let baseline_scan = NativeScan {
            errors: 0,
            streams: vec![NativeStreamSnapshot {
                provider,
                session_key: "cursor-boundary".into(),
                id: "cursor-boundary".into(),
                title: "Cursor boundary".into(),
                identity: "source".into(),
                record_count: 5,
                settled_count: 5,
                events: vec![].into(),
            }],
        };
        assert_eq!(
            db.stage_native_notifications(provider, &baseline_scan, &c)
                .unwrap(),
            0
        );
        assert_eq!(
            db.native_probe_stream_cursor(provider, "cursor-boundary")
                .unwrap(),
            Some(5)
        );

        let post_activation_ms = chrono::Utc::now().timestamp_millis() + 5_000;

        // 1. Pre-activation event at cursor (position == 5, timestamp_ms < activated_ms) -> skipped (0)
        let mut at_cursor = baseline_scan.clone();
        at_cursor.streams[0].events = vec![NativeTurnEvent {
            prompt_id: None,
            position: 5,
            turn_id: "exec-pre-activation".into(),
            kind: "completion".into(),
            body: "Old completion at cursor".into(),
            timestamp_ms: 1,
        }]
        .into();
        assert_eq!(
            db.stage_native_notifications(provider, &at_cursor, &c)
                .unwrap(),
            0
        );

        // 2. Post-activation event strictly before cursor (position == 4 < 5) -> skipped (0)
        std::sync::Arc::make_mut(&mut at_cursor.streams[0].events)[0].position = 4;
        std::sync::Arc::make_mut(&mut at_cursor.streams[0].events)[0].turn_id =
            "exec-before-cursor".into();
        std::sync::Arc::make_mut(&mut at_cursor.streams[0].events)[0].timestamp_ms =
            post_activation_ms;
        assert_eq!(
            db.stage_native_notifications(provider, &at_cursor, &c)
                .unwrap(),
            0
        );

        // 3. Post-activation event exactly at cursor (position == 5, timestamp_ms >= activated_ms)
        // queues 1 on first scan and 0 on repeat scan.
        std::sync::Arc::make_mut(&mut at_cursor.streams[0].events)[0].position = 5;
        std::sync::Arc::make_mut(&mut at_cursor.streams[0].events)[0].turn_id =
            "exec-at-cursor".into();
        std::sync::Arc::make_mut(&mut at_cursor.streams[0].events)[0].timestamp_ms =
            post_activation_ms;
        assert_eq!(
            db.stage_native_notifications(provider, &at_cursor, &c)
                .unwrap(),
            1
        );
        assert_eq!(
            db.stage_native_notifications(provider, &at_cursor, &c)
                .unwrap(),
            0
        );
    }
}
