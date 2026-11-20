//! Durable channel claims. Payloads needed for bounded retry are encrypted at rest.
use super::PanelDb;
use crate::config::Config;
use anyhow::Result;
use jiff::Timestamp;
use rand::Rng;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    Bark,
}

impl NotificationChannel {
    pub const ALL: [Self; 1] = [Self::Bark];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bark => "bark",
        }
    }
    pub fn enabled(self, config: &Config) -> bool {
        match self {
            Self::Bark => config.probe.notifications.enabled,
        }
    }
}

pub struct NotificationDelivery {
    pub event_key: String,
    pub channel: NotificationChannel,
    pub payload: Value,
}

/// A lease fences acknowledgements from expired or competing senders.
#[derive(Debug, Clone)]
pub struct NotificationLease {
    pub event_key: String,
    pub channel: NotificationChannel,
    pub owner: String,
    pub chunk_index: usize,
    pub chunk_count: usize,
    pub chunk_attempts: usize,
    pub attempts: usize,
    pub deadline_ms: i64,
    pub uncertain: bool,
}

pub const NOTIFICATION_LIFETIME_MS: i64 = 600_000;
const LEASE_MS: i64 = 30_000;

pub fn notification_retry_delay_ms(attempt: usize) -> i64 {
    let base = if attempt <= 1 { 15_000 } else { 60_000 };
    rand::thread_rng().gen_range(base * 80 / 100..=base * 120 / 100)
}

fn publish_outcome(tx: &rusqlite::Transaction<'_>, key: &str, now: i64) -> Result<()> {
    let (status, outcome): (String, String) = tx.query_row(
        "SELECT status,outcome_json FROM probe_notification_deliveries WHERE event_key=?1 AND channel='bark'",
        [key], |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    tx.execute(
        "UPDATE probe_events SET payload_json=json_set(payload_json,
            '$.bark',json(?2),'$.bark_status',json_extract(?2,'$.status'))
         WHERE json_valid(payload_json) AND json_extract(payload_json,'$.notification_event_key')=?1",
        params![key, outcome],
    )?;
    // Initial native delivery owns creation of its visible event. Its completion reads this ledger.
    if let Some(native_key) = key.strip_prefix("native:") {
        tx.execute(
            "UPDATE native_probe_deliveries SET status=?2,updated_at=?3,ciphertext=NULL,nonce=NULL
             WHERE event_key=?1 AND status!='delivering'",
            params![
                native_key,
                if matches!(status.as_str(), "pending" | "delivering") {
                    "queued"
                } else {
                    &status
                },
                now / 1000
            ],
        )?;
    }
    tx.execute(
        "UPDATE probe_error_incidents SET bark_status=json_extract(?2,'$.status'),updated_at=?3
         WHERE event_id IN (SELECT id FROM probe_events
            WHERE json_valid(payload_json) AND json_extract(payload_json,'$.notification_event_key')=?1)",
        params![key, outcome, now / 1000],
    )?;
    Ok(())
}

pub(super) const SCHEMA: &str = "
CREATE TABLE probe_notification_channels (
 channel TEXT PRIMARY KEY, enabled INTEGER NOT NULL, activated_ms INTEGER NOT NULL
);
CREATE TABLE probe_notification_deliveries (
 event_key TEXT NOT NULL, channel TEXT NOT NULL,
 ciphertext BLOB, nonce BLOB, status TEXT NOT NULL, outcome_json TEXT,
 attempts INTEGER NOT NULL DEFAULT 0, created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL,
 next_ms INTEGER NOT NULL DEFAULT 0, queue_version INTEGER NOT NULL DEFAULT 0,
 chunk_index INTEGER NOT NULL DEFAULT 0, chunk_count INTEGER NOT NULL DEFAULT 1,
 chunk_attempts INTEGER NOT NULL DEFAULT 0, chunk_uncertain INTEGER NOT NULL DEFAULT 0,
 first_attempt_ms INTEGER, lease_owner TEXT, lease_until_ms INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(event_key, channel)
);
CREATE INDEX idx_probe_notification_pending ON probe_notification_deliveries(status,next_ms);
";

impl PanelDb {
    pub fn sync_notification_channels(&self, config: &Config) -> Result<()> {
        let now = Timestamp::now().as_millisecond();
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.sync_notification_channels_in_transaction(&tx, config, now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn sync_notification_channels_from_config(
        &self,
        load_config: &(dyn Fn() -> Result<Config> + Sync),
    ) -> Result<Config> {
        self.with_transaction(|tx| {
            let config = load_config()?;
            self.sync_notification_channels_in_transaction(
                tx,
                &config,
                Timestamp::now().as_millisecond(),
            )?;
            Self::sync_question_baselines(tx, &config, Timestamp::now().as_millisecond())?;
            Ok(config)
        })
    }

    pub(crate) fn sync_notification_channels_in_transaction(
        &self,
        tx: &rusqlite::Transaction<'_>,
        config: &Config,
        now: i64,
    ) -> Result<()> {
        for channel in NotificationChannel::ALL {
            let enabled = channel.enabled(config);
            let prior = tx
                .query_row(
                    "SELECT enabled,activated_ms FROM probe_notification_channels WHERE channel=?1",
                    [channel.as_str()],
                    |r| Ok((r.get::<_, bool>(0)?, r.get::<_, i64>(1)?)),
                )
                .optional()?;
            let activated = match prior {
                Some((false, _)) if enabled => now,
                Some((_, activated)) => activated,
                None => 0, // Preserve the existing Bark baseline.
            };
            tx.execute("INSERT INTO probe_notification_channels(channel,enabled,activated_ms) VALUES(?1,?2,?3) ON CONFLICT(channel) DO UPDATE SET enabled=excluded.enabled,activated_ms=excluded.activated_ms WHERE enabled!=excluded.enabled OR activated_ms!=excluded.activated_ms", params![channel.as_str(),enabled,activated])?;
            if !enabled || !config.probe.enabled || !config.probe.error_monitor.enabled {
                let keys = tx.prepare("UPDATE probe_notification_deliveries SET status='skipped',ciphertext=NULL,nonce=NULL,lease_owner=NULL,lease_until_ms=0,updated_ms=?2,
                    outcome_json=json_set(coalesce(outcome_json,'{}'),'$.sent',json('false'),'$.skipped',json('true'),'$.status','skipped','$.reason','notifications_disabled','$.next_retry_ms',NULL)
                    WHERE channel=?1 AND status IN ('pending','delivering') AND (?3=0 OR (?4=0 AND event_key NOT LIKE 'test:%') OR (?5=0 AND EXISTS(SELECT 1 FROM probe_events AS event JOIN probe_error_incidents AS incident ON incident.event_id=event.id WHERE json_valid(event.payload_json) AND json_extract(event.payload_json,'$.notification_event_key')=probe_notification_deliveries.event_key))) RETURNING event_key")?
                    .query_map(params![channel.as_str(),now,enabled,config.probe.enabled,config.probe.error_monitor.enabled], |row| row.get::<_,String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for key in keys {
                    publish_outcome(tx, &key, now)?;
                }
            }
        }
        Ok(())
    }

    pub fn stage_notification_delivery(
        &self,
        channel: NotificationChannel,
        key: &str,
        event_ms: i64,
        payload: &Value,
    ) -> Result<bool> {
        self.stage_notification_delivery_with_aliases(channel, key, event_ms, payload, &[])
    }

    pub fn stage_notification_delivery_with_aliases(
        &self,
        channel: NotificationChannel,
        key: &str,
        event_ms: i64,
        payload: &Value,
        aliases: &[String],
    ) -> Result<bool> {
        let (ciphertext, nonce) = self.crypto.encrypt(
            &["notification", channel.as_str(), key],
            &serde_json::to_vec(payload)?,
        )?;
        let now = Timestamp::now().as_millisecond();
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for alias in aliases.iter().filter(|alias| alias.as_str() != key) {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM probe_notification_deliveries WHERE channel=?1 AND event_key=?2)", params![channel.as_str(),alias], |r| r.get(0))?;
            if exists {
                return Ok(false);
            }
            if let Some(native) = alias.strip_prefix("native:") {
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM native_probe_deliveries WHERE event_key=?1)",
                    [native],
                    |r| r.get(0),
                )?;
                if exists {
                    return Ok(false);
                }
            }
        }
        let eligible = tx.query_row("SELECT enabled=1 AND ?2>=activated_ms FROM probe_notification_channels WHERE channel=?1", params![channel.as_str(),event_ms], |r|r.get::<_,bool>(0)).optional()?.unwrap_or(false);
        if !eligible {
            return Ok(false);
        }
        let chunks = payload["chunks"].as_array().map_or(1, Vec::len).max(1);
        let staged = tx.execute("INSERT OR IGNORE INTO probe_notification_deliveries(event_key,channel,ciphertext,nonce,status,created_ms,updated_ms,queue_version,chunk_count) VALUES(?1,?2,?3,?4,'pending',?5,?5,1,?6)", params![key,channel.as_str(),ciphertext,nonce,now,chunks])? > 0;
        tx.commit()?;
        Ok(staged)
    }

    pub fn notification_instance_id(&self) -> Result<String> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute("INSERT OR IGNORE INTO settings(key,value,updated_at) VALUES('notification_instance_id',?1,unixepoch())", [Uuid::new_v4().to_string()])?;
        Ok(conn.query_row(
            "SELECT value FROM settings WHERE key='notification_instance_id'",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn notification_delivery_outcome(
        &self,
        channel: NotificationChannel,
        key: &str,
    ) -> Result<Option<Value>> {
        let value: Option<String> = self.conn.lock().expect("db mutex").query_row(
            "SELECT outcome_json FROM probe_notification_deliveries WHERE event_key=?1 AND channel=?2",
            params![key, channel.as_str()], |r| r.get(0)).optional()?.flatten();
        value
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }

    pub fn claim_notification_delivery(
        &self,
        channel: NotificationChannel,
        key: &str,
    ) -> Result<Option<NotificationLease>> {
        let now = Timestamp::now().as_millisecond();
        let owner = Uuid::new_v4().to_string();
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute("UPDATE probe_notification_deliveries SET status='delivering',attempts=attempts+1,chunk_attempts=chunk_attempts+1,
            first_attempt_ms=coalesce(first_attempt_ms,?3),lease_owner=?4,lease_until_ms=?3+?5,updated_ms=?3
            WHERE event_key=?1 AND channel=?2 AND status='pending' AND queue_version=1 AND next_ms<=?3
            AND chunk_attempts<3 AND ciphertext IS NOT NULL AND (first_attempt_ms IS NULL OR first_attempt_ms+?6>?3)",
            params![key,channel.as_str(),now,owner,LEASE_MS,NOTIFICATION_LIFETIME_MS])?;
        if changed == 0 {
            return Ok(None);
        }
        let lease = tx.query_row("SELECT chunk_index,chunk_count,chunk_attempts,attempts,first_attempt_ms,chunk_uncertain FROM probe_notification_deliveries WHERE event_key=?1 AND channel=?2",
            params![key,channel.as_str()], |r| Ok(NotificationLease {event_key:key.into(),channel,owner,chunk_index:r.get(0)?,chunk_count:r.get(1)?,chunk_attempts:r.get(2)?,attempts:r.get(3)?,deadline_ms:r.get::<_,i64>(4)?+NOTIFICATION_LIFETIME_MS,uncertain:r.get(5)?}))?;
        tx.execute("UPDATE probe_notification_deliveries SET outcome_json=json_set(coalesce(outcome_json,'{}'),
            '$.sent',json('false'),'$.skipped',json('false'),'$.status','retrying',
            '$.attempts',chunk_attempts,'$.request_count',attempts,'$.chunk_count',chunk_count,
            '$.confirmed_chunks',chunk_index,'$.next_retry_ms',NULL)
            WHERE event_key=?1 AND channel=?2",params![key,channel.as_str()])?;
        publish_outcome(&tx, key, now)?;
        tx.commit()?;
        Ok(Some(lease))
    }

    /// Recheck ownership after source validation and bound the request by both deadlines.
    pub fn notification_delivery_lease_remaining_ms(
        &self,
        lease: &NotificationLease,
    ) -> Result<Option<i64>> {
        let now = Timestamp::now().as_millisecond();
        let conn = self.conn.lock().expect("db mutex");
        Ok(conn.query_row(
            "SELECT min(lease_until_ms,first_attempt_ms+?5)-?4 FROM probe_notification_deliveries
             WHERE event_key=?1 AND channel=?2 AND status='delivering' AND lease_owner=?3
             AND lease_until_ms>?4 AND first_attempt_ms+?5>?4 AND ciphertext IS NOT NULL
             AND EXISTS(SELECT 1 FROM probe_notification_channels WHERE channel=?2 AND enabled=1)",
            params![lease.event_key,lease.channel.as_str(),lease.owner,now,NOTIFICATION_LIFETIME_MS],
            |r| r.get(0),
        ).optional()?)
    }

    /// Acknowledges exactly one chunk, only while this sender still owns the lease.
    pub fn finish_notification_delivery(
        &self,
        lease: &NotificationLease,
        outcome: &Value,
    ) -> Result<Option<Value>> {
        let now = Timestamp::now().as_millisecond();
        let sent = outcome["sent"].as_bool() == Some(true);
        let skipped = outcome["skipped"].as_bool() == Some(true);
        let confirmed = lease.chunk_index + usize::from(sent);
        let uncertain = !sent && (lease.uncertain || outcome["uncertain"].as_bool() == Some(true));
        let delay = outcome["retry_after_ms"]
            .as_i64()
            .unwrap_or(0)
            .max(notification_retry_delay_ms(lease.chunk_attempts));
        let retry = !sent
            && !skipped
            && outcome["retryable"].as_bool() == Some(true)
            && lease.chunk_attempts < 3
            && now.saturating_add(delay) < lease.deadline_ms;
        let more = sent && confirmed < lease.chunk_count && now < lease.deadline_ms;
        let status = if sent && confirmed == lease.chunk_count {
            "sent"
        } else if skipped {
            "skipped"
        } else if retry || more {
            "pending"
        } else if uncertain {
            "unknown"
        } else {
            "failed"
        };
        let next = if retry {
            now.saturating_add(delay)
        } else {
            now
        };
        let mut value = outcome.clone();
        value["status"] = json!(if status == "pending" {
            "waiting_retry"
        } else {
            status
        });
        value["sent"] = json!(status == "sent");
        if sent && status == "failed" {
            value["reason"] = json!("retry_deadline_exceeded");
        }
        value["attempts"] = json!(lease.chunk_attempts);
        value["request_count"] = json!(lease.attempts);
        value["chunk_count"] = json!(lease.chunk_count);
        value["confirmed_chunks"] = json!(confirmed);
        value["next_retry_ms"] = if status == "pending" {
            json!(next)
        } else {
            Value::Null
        };
        if let Some(obj) = value.as_object_mut() {
            obj.remove("retryable");
            obj.remove("retry_after_ms");
            obj.remove("uncertain");
        }
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed=tx.execute("UPDATE probe_notification_deliveries SET status=?4,outcome_json=?5,updated_ms=?6,next_ms=?7,chunk_index=?8,
            chunk_attempts=CASE WHEN ?9 THEN 0 ELSE chunk_attempts END,chunk_uncertain=?10,lease_owner=NULL,lease_until_ms=0,
            ciphertext=CASE WHEN ?4='pending' THEN ciphertext ELSE NULL END,nonce=CASE WHEN ?4='pending' THEN nonce ELSE NULL END
            WHERE event_key=?1 AND channel=?2 AND status='delivering' AND lease_owner=?3 AND lease_until_ms>?6",
            params![lease.event_key,lease.channel.as_str(),lease.owner,status,value.to_string(),now,next,confirmed,sent,uncertain])?;
        if changed == 0 {
            return Ok(None);
        }
        publish_outcome(&tx, &lease.event_key, now)?;
        tx.commit()?;
        Ok(Some(value))
    }

    pub fn due_notification_deliveries(&self, limit: usize) -> Result<Vec<NotificationDelivery>> {
        let now = Timestamp::now().as_millisecond();
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed_keys = Vec::new();
        // An expired attempt may already have reached Bark. Recover only its unconfirmed chunk.
        let interrupted: Vec<(String, String, usize, i64)> = {
            let mut stmt=tx.prepare("SELECT event_key,channel,chunk_attempts,first_attempt_ms FROM probe_notification_deliveries
                WHERE queue_version=1 AND status='delivering' AND lease_until_ms<=?1")?;
            let rows = stmt
                .query_map([now], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        for (key, channel, attempt, first) in interrupted {
            let next = now + notification_retry_delay_ms(attempt);
            let retry = attempt < 3 && next < first + NOTIFICATION_LIFETIME_MS;
            tx.execute("UPDATE probe_notification_deliveries SET status=?3,lease_owner=NULL,lease_until_ms=0,next_ms=?4,chunk_uncertain=1,
                outcome_json=json_set(?5,'$.request_count',attempts,'$.chunk_count',chunk_count,'$.confirmed_chunks',chunk_index),
                updated_ms=?7,ciphertext=CASE WHEN ?6 THEN ciphertext ELSE NULL END,nonce=CASE WHEN ?6 THEN nonce ELSE NULL END
                WHERE event_key=?1 AND channel=?2",params![key,channel,if retry {"pending"} else {"unknown"},next,
                json!({"sent":false,"skipped":false,"status":if retry {"waiting_retry"}else{"unknown"},"reason":"delivery_interrupted_outcome_unknown","error_category":"network","attempts":attempt,"next_retry_ms":if retry {Some(next)}else{None}}).to_string(),retry,now])?;
            changed_keys.push(key);
        }
        changed_keys.extend(tx.prepare("UPDATE probe_notification_deliveries SET status=CASE WHEN chunk_uncertain=1 THEN 'unknown' ELSE 'failed' END,ciphertext=NULL,nonce=NULL,lease_owner=NULL,lease_until_ms=0,updated_ms=?1,
            outcome_json=json_set(coalesce(outcome_json,'{}'),'$.sent',json('false'),'$.status',CASE WHEN chunk_uncertain=1 THEN 'unknown' ELSE 'failed' END,'$.reason','retry_deadline_exceeded','$.next_retry_ms',NULL)
            WHERE queue_version=1 AND status='pending' AND first_attempt_ms+?2<=?1 RETURNING event_key")?
            .query_map(params![now,NOTIFICATION_LIFETIME_MS], |row| row.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?);
        for key in changed_keys {
            publish_outcome(&tx, &key, now)?;
        }
        let rows = {
            let mut stmt=tx.prepare("SELECT event_key,ciphertext,nonce FROM probe_notification_deliveries WHERE channel='bark' AND queue_version=1 AND status='pending'
                AND next_ms<=?1 AND chunk_attempts<3 AND ciphertext IS NOT NULL ORDER BY created_ms,event_key LIMIT ?2")?;
            let rows = stmt
                .query_map(params![now, limit.min(10)], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        tx.commit()?;
        rows.into_iter()
            .map(|(event_key, ciphertext, nonce)| {
                let plaintext = self.crypto.decrypt(
                    &[
                        "notification",
                        NotificationChannel::Bark.as_str(),
                        &event_key,
                    ],
                    &ciphertext,
                    &nonce,
                )?;
                Ok(NotificationDelivery {
                    channel: NotificationChannel::Bark,
                    payload: serde_json::from_slice(&plaintext)?,
                    event_key,
                })
            })
            .collect()
    }

    pub fn notification_first_seen_ms(&self, dedupe_key: &str) -> Result<Option<i64>> {
        self.conn
            .lock()
            .expect("db mutex")
            .query_row(
                "SELECT min(created_at)*1000 FROM probe_events WHERE dedupe_key=?1",
                [dedupe_key],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }
}
