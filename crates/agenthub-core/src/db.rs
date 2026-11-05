pub(crate) mod migration_lock;
#[cfg(test)]
mod migration_tests;
use crate::crypto::SecretBox;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

pub mod desktop_submissions;
mod native_probe;
mod notification_channels;
pub use native_probe::NativeDelivery;
pub use notification_channels::{
    notification_retry_delay_ms, NotificationChannel, NotificationDelivery, NotificationLease,
    NOTIFICATION_LIFETIME_MS,
};

#[derive(Clone)]
pub struct PanelDb {
    path: PathBuf,
    conn: Arc<Mutex<Connection>>,
    crypto: SecretBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationRetentionCounts {
    pub event_count: usize,
    pub dedupe_count: usize,
    pub pending_event_count: usize,
    pub pending_dedupe_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadFollowUp {
    pub id: String,
    pub thread_id: String,
    pub status: String,
    pub message: String,
    pub options_json: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub submitted_at: Option<i64>,
    pub cancelled_at: Option<i64>,
    pub result_json: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeEvent {
    pub id: String,
    pub kind: String,
    pub thread_id: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
    pub dedupe_key: Option<String>,
    pub source: String,
    pub payload: Value,
    pub created_at: i64,
    pub handled_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewProbeEvent<'a> {
    pub kind: &'a str,
    pub thread_id: Option<&'a str>,
    pub title: Option<&'a str>,
    pub message: Option<&'a str>,
    pub dedupe_key: Option<&'a str>,
    pub source: &'a str,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeErrorIncident {
    pub incident_key: String,
    pub source_ts: i64,
    pub source_ts_nanos: i64,
    pub source_row_id: i64,
    pub thread_id: String,
    pub turn_id: String,
    pub classification: String,
    pub error_sha256: String,
    pub error_summary: String,
    pub event_id: Option<String>,
    pub bark_status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewProbeErrorIncident {
    pub incident_key: String,
    pub source_ts: i64,
    pub source_ts_nanos: i64,
    pub source_row_id: i64,
    pub thread_id: String,
    pub turn_id: String,
    pub classification: String,
    pub error_sha256: String,
    pub error_summary: String,
}

impl PanelDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_secret_box(path, SecretBox::deterministic_dev())
    }

    pub fn open_with_secret_box(path: impl AsRef<Path>, crypto: SecretBox) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create data dir {}", parent.display()))?;
        }
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        let db = Self {
            path,
            conn: Arc::new(Mutex::new(conn)),
            crypto,
        };
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn now() -> i64 {
        Utc::now().timestamp()
    }

    fn migrate(&self) -> Result<()> {
        const SCHEMA_VERSION: i64 = 3;
        {
            let conn = self.conn.lock().expect("db mutex");
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
            let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if version >= SCHEMA_VERSION {
                return Ok(());
            }
        }
        let _migration = migration_lock::MigrationLock::acquire(&self.path)?;
        let mut conn = self.conn.lock().expect("db mutex");
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version >= SCHEMA_VERSION {
            return Ok(());
        }
        let populated: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')", [], |row| row.get(0))?;
        anyhow::ensure!(
            !populated,
            "旧数据库请先使用 v2.0.10 完成升级；原数据已保留"
        );
        let tx = conn.transaction()?;
        tx.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;

            CREATE TABLE IF NOT EXISTS settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL,
              updated_at INTEGER NOT NULL
            );


            CREATE TABLE IF NOT EXISTS audit_log (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              admin_id TEXT,
              action TEXT NOT NULL,
              target_type TEXT,
              target_id TEXT,
              ip TEXT,
              detail_json TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS jobs (
              id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              status TEXT NOT NULL,
              title TEXT NOT NULL,
              thread_id TEXT,
              turn_id TEXT,
              started_at INTEGER NOT NULL,
              finished_at INTEGER,
              exit_code INTEGER,
              output TEXT NOT NULL DEFAULT '',
              error TEXT
            );


            CREATE TABLE IF NOT EXISTS thread_followups (
              id TEXT PRIMARY KEY,
              thread_id TEXT NOT NULL,
              status TEXT NOT NULL,
              message TEXT NOT NULL,
              options_json TEXT NOT NULL,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              submitted_at INTEGER,
              cancelled_at INTEGER,
              result_json TEXT,
              error TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_thread_followups_thread_status_created
              ON thread_followups(thread_id, status, created_at);

            CREATE TABLE IF NOT EXISTS probe_events (
              id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              thread_id TEXT,
              title TEXT,
              message TEXT,
              dedupe_key TEXT,
              source TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at INTEGER NOT NULL,
              handled_at INTEGER
            );

            CREATE INDEX IF NOT EXISTS idx_probe_events_created_at
              ON probe_events(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_probe_events_thread_created_at
              ON probe_events(thread_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS probe_dedupe (
              namespace TEXT NOT NULL,
              dedupe_key TEXT NOT NULL,
              expires_at INTEGER NOT NULL,
              created_at INTEGER NOT NULL,
              PRIMARY KEY(namespace, dedupe_key)
            );

            CREATE TABLE IF NOT EXISTS probe_error_incidents (
              incident_key TEXT PRIMARY KEY,
              source_ts INTEGER NOT NULL,
              source_ts_nanos INTEGER NOT NULL,
              source_row_id INTEGER NOT NULL,
              thread_id TEXT NOT NULL,
              turn_id TEXT NOT NULL,
              classification TEXT NOT NULL,
              error_sha256 TEXT NOT NULL,
              error_summary TEXT NOT NULL,
              event_id TEXT,
              bark_status TEXT NOT NULL DEFAULT 'pending',
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_probe_error_incidents_source
              ON probe_error_incidents(source_ts, source_ts_nanos, source_row_id);

            CREATE TABLE IF NOT EXISTS native_probe_providers (
              provider TEXT PRIMARY KEY,
              enabled INTEGER NOT NULL,
              activated_ms INTEGER NOT NULL,
              scanned_at INTEGER NOT NULL,
              stream_count INTEGER NOT NULL,
              error_count INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS native_probe_streams (
              provider TEXT NOT NULL,
              session_key TEXT NOT NULL,
              identity TEXT NOT NULL,
              cursor INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              PRIMARY KEY(provider, session_key)
            );
            CREATE TABLE IF NOT EXISTS native_probe_deliveries (
              event_key TEXT PRIMARY KEY,
              provider TEXT NOT NULL,
              event_json TEXT NOT NULL,
              ciphertext BLOB,
              nonce BLOB,
              status TEXT NOT NULL,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL
            );

            "#,
        )?;
        tx.execute_batch(desktop_submissions::SCHEMA)?;
        tx.execute_batch(notification_channels::SCHEMA)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_setting_with_updated_at(&self, key: &str) -> Result<Option<(String, i64)>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            "SELECT value, updated_at FROM settings WHERE key=?1",
            params![key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            INSERT INTO settings(key, value, updated_at) VALUES(?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at
            "#,
            params![key, value, Self::now()],
        )?;
        Ok(())
    }

    /// Atomically establish a first-enable baseline shared by hooks and the monitor.
    pub fn feedback_notification_baseline(&self, enabled: bool) -> Result<Option<i64>> {
        self.notification_baseline("probe_feedback_baseline", enabled)
    }

    pub fn notification_baseline(&self, key: &str, enabled: bool) -> Result<Option<i64>> {
        let mut conn = self.conn.lock().expect("db mutex");
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let previous: Option<String> = tx
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        let previous =
            previous.and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok());
        if previous.as_ref().and_then(|v| v["enabled"].as_bool()) == Some(enabled) {
            if let Some(since) = previous.as_ref().and_then(|v| v["since_ms"].as_i64()) {
                return Ok(enabled.then_some(since));
            }
        }
        let since = if previous.as_ref().and_then(|v| v["enabled"].as_bool()) == Some(true) {
            previous
                .as_ref()
                .and_then(|v| v["since_ms"].as_i64())
                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
        } else {
            chrono::Utc::now().timestamp_millis()
        };
        tx.execute("INSERT INTO settings(key,value,updated_at) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
            rusqlite::params![key, serde_json::json!({"enabled":enabled,"since_ms":since}).to_string(), Self::now()])?;
        tx.commit()?;
        Ok(enabled.then_some(since))
    }

    pub fn claim_feedback_delivery(&self, key: &str) -> Result<bool> {
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO settings(key,value,updated_at) VALUES(?1,'delivering',?2)",
            rusqlite::params![key, now],
        )? == 1;
        if !inserted {
            let _ = conn.execute(
                "UPDATE settings SET updated_at = ?2 WHERE key = ?1 AND updated_at <= ?3",
                rusqlite::params![key, now, now - 86_400],
            );
        }
        Ok(inserted)
    }

    pub fn touch_setting_if_older_than(&self, key: &str, min_age_seconds: i64) -> Result<bool> {
        let now = Self::now();
        let threshold = now - min_age_seconds.max(1);
        let conn = self.conn.lock().expect("db mutex");
        Ok(conn.execute(
            "UPDATE settings SET updated_at = ?2 WHERE key = ?1 AND updated_at <= ?3",
            rusqlite::params![key, now, threshold],
        )? > 0)
    }

    pub fn retain_active_unresolved_markers(
        &self,
        namespace: &str,
        marker_keys: &[String],
    ) -> Result<()> {
        let mut sorted: Vec<&str> = marker_keys
            .iter()
            .map(String::as_str)
            .filter(|k| {
                k.starts_with("probe_feedback_delivery:")
                    || k.starts_with("probe_passive_sent_marker:")
                    || k.starts_with("probe_async_question_delivery:")
            })
            .collect();
        sorted.sort_unstable();
        sorted.dedup();
        let setting_key = format!("probe_active_unresolved_markers:{namespace}");
        let encoded = serde_json::to_string(&sorted)?;
        let now = Self::now();
        if let Some((prev_val, prev_at)) = self.get_setting_with_updated_at(&setting_key)? {
            if prev_val == encoded && now.saturating_sub(prev_at) < 86_400 {
                return Ok(());
            }
        }
        self.set_setting(&setting_key, &encoded)
    }

    pub fn set_secret_setting_bytes(&self, key: &str, plaintext: &[u8]) -> Result<()> {
        if plaintext.is_empty() {
            return Ok(());
        }
        let (ciphertext, nonce) = self.crypto.encrypt(plaintext)?;
        let value = serde_json::json!({
            "ciphertext": general_purpose::STANDARD.encode(ciphertext),
            "nonce": general_purpose::STANDARD.encode(nonce)
        })
        .to_string();
        self.set_setting(key, &value)
    }

    pub fn remove_secret_setting(&self, key: &str) -> Result<()> {
        self.conn
            .lock()
            .expect("db mutex")
            .execute("DELETE FROM settings WHERE key=?1", [key])?;
        Ok(())
    }

    pub fn get_secret_setting_bytes(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let Some(value) = self.get_setting(key)? else {
            return Ok(None);
        };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        match encrypted_setting_parts(trimmed) {
            Some(Ok((ciphertext, nonce))) => self.crypto.decrypt(&ciphertext, &nonce).map(Some),
            Some(Err(err)) => Err(err),
            None => Ok(Some(trimmed.as_bytes().to_vec())),
        }
    }

    pub fn record_audit(
        &self,
        admin_id: Option<&str>,
        action: &str,
        target_type: Option<&str>,
        target_id: Option<&str>,
        ip: Option<&str>,
        detail: Value,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            INSERT INTO audit_log(admin_id, action, target_type, target_id, ip, detail_json, created_at)
            VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                admin_id,
                action,
                target_type,
                target_id,
                ip,
                detail.to_string(),
                Self::now()
            ],
        )?;
        Ok(())
    }

    pub fn create_job(&self, id: &str, kind: &str, title: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "INSERT INTO jobs(id, kind, status, title, started_at) VALUES(?1, ?2, 'running', ?3, ?4)",
            params![id, kind, title, Self::now()],
        )?;
        Ok(())
    }

    pub fn link_job_thread(
        &self,
        id: &str,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "UPDATE jobs SET thread_id=?2, turn_id=?3 WHERE id=?1",
            params![id, thread_id, turn_id],
        )?;
        Ok(())
    }

    pub fn append_job_output(&self, id: &str, chunk: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "UPDATE jobs SET output = output || ?2 WHERE id=?1",
            params![id, chunk],
        )?;
        Ok(())
    }

    pub fn finish_job(
        &self,
        id: &str,
        status: &str,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "UPDATE jobs SET status=?2, finished_at=?3, exit_code=?4, error=?5 WHERE id=?1",
            params![id, status, Self::now(), exit_code, error],
        )?;
        Ok(())
    }

    pub fn list_jobs(&self, limit: u32) -> Result<Vec<JobRecord>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            "SELECT id, kind, status, title, thread_id, turn_id, started_at, finished_at, exit_code, '' AS output, error FROM jobs ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], job_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn running_thread_jobs(&self) -> Result<Vec<JobRecord>> {
        let conn = self.conn.lock().expect("db mutex");
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
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn running_job_for_thread(&self, thread_id: &str) -> Result<Option<JobRecord>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            r#"
            SELECT id, kind, status, title, thread_id, turn_id, started_at, finished_at, exit_code,
                   output, error
            FROM jobs
            WHERE thread_id=?1 AND status='running'
            ORDER BY started_at DESC
            LIMIT 1
            "#,
            params![thread_id],
            job_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn job(&self, id: &str) -> Result<Option<JobRecord>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            "SELECT id, kind, status, title, thread_id, turn_id, started_at, finished_at, exit_code, output, error FROM jobs WHERE id=?1",
            params![id],
            job_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn enqueue_followup(
        &self,
        thread_id: &str,
        message: &str,
        options: Value,
    ) -> Result<ThreadFollowUp> {
        let id = Uuid::new_v4().to_string();
        let now = Self::now();
        let options_json = options.to_string();
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            INSERT INTO thread_followups(id, thread_id, status, message, options_json, created_at, updated_at)
            VALUES(?1, ?2, 'pending', ?3, ?4, ?5, ?5)
            "#,
            params![id, thread_id, message, options_json, now],
        )?;
        Ok(ThreadFollowUp {
            id,
            thread_id: thread_id.to_string(),
            status: "pending".to_string(),
            message: message.to_string(),
            options_json,
            created_at: now,
            updated_at: now,
            submitted_at: None,
            cancelled_at: None,
            result_json: None,
            error: None,
        })
    }

    pub fn list_followups(&self, thread_id: &str, limit: u32) -> Result<Vec<ThreadFollowUp>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, thread_id, status, message, options_json, created_at, updated_at,
                   submitted_at, cancelled_at, result_json, error
            FROM thread_followups
            WHERE thread_id=?1
            ORDER BY created_at DESC, rowid DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![thread_id, limit.max(1)], followup_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn claim_next_pending_followup(&self, thread_id: &str) -> Result<Option<ThreadFollowUp>> {
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        let followup = conn
            .query_row(
                r#"
                SELECT id, thread_id, status, message, options_json, created_at, updated_at,
                       submitted_at, cancelled_at, result_json, error
                FROM thread_followups
                WHERE thread_id=?1 AND status='pending'
                ORDER BY created_at ASC, rowid ASC
                LIMIT 1
                "#,
                params![thread_id],
                followup_from_row,
            )
            .optional()?;
        let Some(followup) = followup else {
            return Ok(None);
        };
        let changed = conn.execute(
            "UPDATE thread_followups SET status='submitting', updated_at=?2 WHERE id=?1 AND status='pending'",
            params![followup.id, now],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        Ok(Some(ThreadFollowUp {
            status: "submitting".to_string(),
            updated_at: now,
            ..followup
        }))
    }

    pub fn active_followup_upload_ids(&self) -> Result<HashSet<String>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            r#"
            SELECT options_json
            FROM thread_followups
            WHERE status IN ('pending', 'submitting')
            "#,
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = HashSet::new();
        for row in rows {
            let options_json = row?;
            let Ok(options) = serde_json::from_str::<Value>(&options_json) else {
                continue;
            };
            if let Some(attachments) = options.get("attachments").and_then(Value::as_array) {
                ids.extend(
                    attachments
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|id| Uuid::parse_str(id).is_ok())
                        .map(str::to_string),
                );
            }
        }
        Ok(ids)
    }

    pub fn cancel_followup(&self, thread_id: &str, id: &str) -> Result<bool> {
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        let changed = conn.execute(
            r#"
            UPDATE thread_followups
            SET status='cancelled', cancelled_at=?3, updated_at=?3
            WHERE thread_id=?1 AND id=?2 AND status='pending'
            "#,
            params![thread_id, id, now],
        )?;
        Ok(changed > 0)
    }

    pub fn mark_followup_submitted(&self, id: &str, result: Value) -> Result<()> {
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            UPDATE thread_followups
            SET status='submitted', submitted_at=?2, updated_at=?2, result_json=?3, error=NULL
            WHERE id=?1
            "#,
            params![id, now, result.to_string()],
        )?;
        Ok(())
    }

    pub fn mark_followup_error(&self, id: &str, error: &str) -> Result<()> {
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            UPDATE thread_followups
            SET status='error', updated_at=?2, error=?3
            WHERE id=?1
            "#,
            params![id, now, error],
        )?;
        Ok(())
    }

    pub fn record_probe_event(&self, event: NewProbeEvent<'_>) -> Result<ProbeEvent> {
        let id = Uuid::new_v4().to_string();
        let now = Self::now();
        let payload_json = event.payload.to_string();
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            INSERT INTO probe_events(
              id, kind, thread_id, title, message, dedupe_key, source, payload_json, created_at
            )
            VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                id,
                event.kind,
                event.thread_id,
                event.title,
                event.message,
                event.dedupe_key,
                event.source,
                payload_json,
                now,
            ],
        )?;
        Ok(ProbeEvent {
            id,
            kind: event.kind.to_string(),
            thread_id: event.thread_id.map(str::to_string),
            title: event.title.map(str::to_string),
            message: event.message.map(str::to_string),
            dedupe_key: event.dedupe_key.map(str::to_string),
            source: event.source.to_string(),
            payload: event.payload,
            created_at: now,
            handled_at: None,
        })
    }

    pub fn list_probe_events(&self, limit: u32) -> Result<Vec<ProbeEvent>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut stmt = conn.prepare(
            r#"
            SELECT id, kind, thread_id, title, message, dedupe_key, source, payload_json, created_at, handled_at
            FROM probe_events
            ORDER BY created_at DESC, rowid DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit.clamp(1, 500)], probe_event_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn mark_probe_event_handled(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().expect("db mutex");
        let changed = conn.execute(
            "UPDATE probe_events SET handled_at=?2 WHERE id=?1 AND handled_at IS NULL",
            params![id, Self::now()],
        )?;
        Ok(changed > 0)
    }

    pub fn claim_probe_dedupe(
        &self,
        namespace: &str,
        dedupe_key: &str,
        ttl_seconds: i64,
    ) -> Result<bool> {
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            "DELETE FROM probe_dedupe WHERE expires_at <= ?1",
            params![now],
        )?;
        let changed = conn.execute(
            r#"
            INSERT OR IGNORE INTO probe_dedupe(namespace, dedupe_key, expires_at, created_at)
            VALUES(?1, ?2, ?3, ?4)
            "#,
            params![namespace, dedupe_key, now + ttl_seconds.max(1), now],
        )?;
        Ok(changed > 0)
    }

    pub fn claim_probe_error_incident(&self, incident: &NewProbeErrorIncident) -> Result<bool> {
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        Ok(conn.execute(
            r#"
            INSERT OR IGNORE INTO probe_error_incidents(
              incident_key, source_ts, source_ts_nanos, source_row_id,
              thread_id, turn_id, classification, error_sha256, error_summary,
              bark_status, created_at, updated_at
            ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10, ?10)
            "#,
            params![
                incident.incident_key,
                incident.source_ts,
                incident.source_ts_nanos,
                incident.source_row_id,
                incident.thread_id,
                incident.turn_id,
                incident.classification,
                incident.error_sha256,
                incident.error_summary,
                now,
            ],
        )? == 1)
    }

    pub fn probe_error_incident_count(&self) -> Result<u64> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row("SELECT COUNT(*) FROM probe_error_incidents", [], |row| {
            row.get(0)
        })
        .map_err(Into::into)
    }

    pub fn list_pending_probe_error_deliveries(
        &self,
        limit: u32,
    ) -> Result<Vec<ProbeErrorIncident>> {
        let conn = self.conn.lock().expect("db mutex");
        let mut statement = conn.prepare(
            r#"
            SELECT incident_key, source_ts, source_ts_nanos, source_row_id,
                   thread_id, turn_id, classification, error_sha256, error_summary,
                   event_id, bark_status, created_at, updated_at
            FROM probe_error_incidents
            WHERE bark_status='pending'
            ORDER BY created_at ASC, source_ts ASC, source_ts_nanos ASC, source_row_id ASC
            LIMIT ?1
            "#,
        )?;
        let rows =
            statement.query_map(params![limit.clamp(1, 100)], probe_error_incident_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_probe_error_incident(
        &self,
        incident_key: &str,
    ) -> Result<Option<ProbeErrorIncident>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            r#"
            SELECT incident_key, source_ts, source_ts_nanos, source_row_id,
                   thread_id, turn_id, classification, error_sha256, error_summary,
                   event_id, bark_status, created_at, updated_at
            FROM probe_error_incidents WHERE incident_key=?1
            "#,
            params![incident_key],
            probe_error_incident_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn update_probe_error_incident_delivery(
        &self,
        incident_key: &str,
        event_id: Option<&str>,
        bark_status: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex");
        conn.execute(
            r#"
            UPDATE probe_error_incidents
            SET event_id=COALESCE(?2, event_id),
                bark_status=COALESCE((SELECT json_extract(payload_json,'$.bark.status')
                    FROM probe_events WHERE id=COALESCE(?2,probe_error_incidents.event_id)),?3),
                updated_at=?4
            WHERE incident_key=?1
            "#,
            params![incident_key, event_id, bark_status, Self::now()],
        )?;
        Ok(())
    }

    pub fn probe_event_id_by_dedupe_key(&self, dedupe_key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("db mutex");
        conn.query_row(
            "SELECT id FROM probe_events WHERE dedupe_key=?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
            params![dedupe_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn notification_retention_counts(
        &self,
        retention_days: u32,
    ) -> Result<NotificationRetentionCounts> {
        let cutoff = Self::now() - i64::from(retention_days.max(1)) * 86_400;
        let now = Self::now();
        let conn = self.conn.lock().expect("db mutex");
        let event_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM probe_events", [], |row| row.get(0))?;
        let dedupe_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM probe_dedupe", [], |row| row.get(0))?;
        let pending_event_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM probe_events WHERE created_at < ?1",
            params![cutoff],
            |row| row.get(0),
        )?;
        let pending_dedupe_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM probe_dedupe WHERE expires_at <= ?1",
            params![now],
            |row| row.get(0),
        )?;
        Ok(NotificationRetentionCounts {
            event_count: event_count as usize,
            dedupe_count: dedupe_count as usize,
            pending_event_count: pending_event_count as usize,
            pending_dedupe_count: pending_dedupe_count as usize,
        })
    }

    pub fn maintain_probe_events(
        &self,
        retention_days: u32,
        max_delete_rows: u32,
        dry_run: bool,
    ) -> Result<(usize, usize)> {
        let cutoff = Self::now() - i64::from(retention_days.max(1)) * 86_400;
        let limit = i64::from(max_delete_rows.max(1));
        let conn = self.conn.lock().expect("db mutex");
        let event_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT rowid FROM probe_events WHERE created_at < ?1 ORDER BY created_at ASC LIMIT ?2
            )",
            params![cutoff, limit],
            |row| row.get(0),
        )?;
        let dedupe_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT rowid FROM probe_dedupe WHERE expires_at <= ?1 ORDER BY expires_at ASC LIMIT ?2
            )",
            params![Self::now(), limit],
            |row| row.get(0),
        )?;
        if dry_run {
            return Ok((event_count as usize, dedupe_count as usize));
        }
        let events_deleted = conn.execute(
            "DELETE FROM probe_events WHERE rowid IN (
                SELECT rowid FROM probe_events WHERE created_at < ?1 ORDER BY created_at ASC LIMIT ?2
            )",
            params![cutoff, limit],
        )?;
        let dedupe_deleted = conn.execute(
            "DELETE FROM probe_dedupe WHERE rowid IN (
                SELECT rowid FROM probe_dedupe WHERE expires_at <= ?1 ORDER BY expires_at ASC LIMIT ?2
            )",
            params![Self::now(), limit],
        )?;
        let marker_cutoff = Self::now() - i64::from(retention_days.clamp(7, 3650)) * 86_400;
        let mut protected_markers = std::collections::HashSet::new();
        {
            let mut stmt = conn.prepare(
                "SELECT value FROM settings WHERE key LIKE 'probe_active_unresolved_markers:%' AND updated_at >= ?1",
            )?;
            let rows = stmt.query_map([marker_cutoff], |row| row.get::<_, String>(0))?;
            for value in rows.flatten() {
                if let Ok(keys) = serde_json::from_str::<Vec<String>>(&value) {
                    protected_markers.extend(keys);
                }
            }
        }
        let candidate_markers: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT key FROM settings
                 WHERE (
                     key LIKE 'probe_feedback_delivery:%'
                     OR key LIKE 'probe_passive_sent_marker:%'
                     OR key LIKE 'probe_async_question_delivery:%'
                 ) AND updated_at < ?1
                 ORDER BY updated_at ASC
                 LIMIT ?2",
            )?;
            let rows =
                stmt.query_map(params![marker_cutoff, limit], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for key in candidate_markers {
            if !protected_markers.contains(&key) {
                conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            }
        }
        conn.execute(
            "DELETE FROM probe_notification_deliveries WHERE rowid IN (
                SELECT rowid FROM probe_notification_deliveries
                WHERE status IN ('sent', 'failed', 'skipped', 'legacy', 'unknown')
                  AND updated_ms < ?1
                ORDER BY updated_ms ASC
                LIMIT ?2
            )",
            params![marker_cutoff * 1_000, limit],
        )?;
        conn.execute(
            "DELETE FROM probe_error_incidents WHERE rowid IN (
                SELECT rowid FROM probe_error_incidents
                WHERE bark_status != 'pending' AND updated_at < ?1
                ORDER BY updated_at ASC
                LIMIT ?2
            )",
            params![marker_cutoff, limit],
        )?;
        Ok((events_deleted, dedupe_deleted))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub title: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub output: String,
    pub error: Option<String>,
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

fn followup_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadFollowUp> {
    Ok(ThreadFollowUp {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        status: row.get(2)?,
        message: row.get(3)?,
        options_json: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        submitted_at: row.get(7)?,
        cancelled_at: row.get(8)?,
        result_json: row.get(9)?,
        error: row.get(10)?,
    })
}

fn probe_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProbeEvent> {
    let payload_json: String = row.get(7)?;
    let payload = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
    Ok(ProbeEvent {
        id: row.get(0)?,
        kind: row.get(1)?,
        thread_id: row.get(2)?,
        title: row.get(3)?,
        message: row.get(4)?,
        dedupe_key: row.get(5)?,
        source: row.get(6)?,
        payload,
        created_at: row.get(8)?,
        handled_at: row.get(9)?,
    })
}

fn probe_error_incident_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProbeErrorIncident> {
    Ok(ProbeErrorIncident {
        incident_key: row.get(0)?,
        source_ts: row.get(1)?,
        source_ts_nanos: row.get(2)?,
        source_row_id: row.get(3)?,
        thread_id: row.get(4)?,
        turn_id: row.get(5)?,
        classification: row.get(6)?,
        error_sha256: row.get(7)?,
        error_summary: row.get(8)?,
        event_id: row.get(9)?,
        bark_status: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn encrypted_setting_parts(value: &str) -> Option<Result<(Vec<u8>, Vec<u8>)>> {
    let Ok(json) = serde_json::from_str::<Value>(value) else {
        return None;
    };
    let Value::Object(map) = json else {
        return None;
    };
    if !(map.contains_key("ciphertext") || map.contains_key("nonce")) {
        return None;
    }
    let result = (|| {
        let ciphertext = map
            .get("ciphertext")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("invalid encrypted setting ciphertext"))?;
        let nonce = map
            .get("nonce")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("invalid encrypted setting nonce"))?;
        let ciphertext = general_purpose::STANDARD
            .decode(ciphertext)
            .context("invalid encrypted setting ciphertext")?;
        let nonce = general_purpose::STANDARD
            .decode(nonce)
            .context("invalid encrypted setting nonce")?;
        Ok((ciphertext, nonce))
    })();
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::{NewProbeEvent, PanelDb};
    use serde_json::json;
    use std::fs;

    #[test]
    fn notification_baseline_only_writes_on_activation_changes() {
        let db = PanelDb::open(":memory:").unwrap();
        db.set_setting("fixture", r#"{"enabled":true,"since_ms":1}"#)
            .unwrap();
        let changes = || {
            db.conn
                .lock()
                .unwrap()
                .query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let before = changes();
        assert_eq!(db.notification_baseline("fixture", true).unwrap(), Some(1));
        assert_eq!(changes(), before);
        assert_eq!(db.notification_baseline("fixture", false).unwrap(), None);
        let disabled = changes();
        assert_eq!(db.notification_baseline("fixture", false).unwrap(), None);
        assert_eq!(changes(), disabled);
        let restarted = db.notification_baseline("fixture", true).unwrap().unwrap();
        assert!(restarted > 1);
        let enabled = changes();
        assert_eq!(
            db.notification_baseline("fixture", true).unwrap(),
            Some(restarted)
        );
        assert_eq!(changes(), enabled);
    }

    #[test]
    fn followup_queue_persists_claims_cancels_and_marks_results() {
        let db = PanelDb::open(":memory:").unwrap();
        let first = db
            .enqueue_followup("thread-a", "first follow-up", json!({"model":"gpt-5.5"}))
            .unwrap();
        let second = db
            .enqueue_followup(
                "thread-a",
                "second follow-up",
                json!({"reasoning_effort":"xhigh"}),
            )
            .unwrap();

        let listed = db.list_followups("thread-a", 10).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, second.id);
        assert_eq!(listed[1].id, first.id);

        let claimed = db.claim_next_pending_followup("thread-a").unwrap().unwrap();
        assert_eq!(claimed.id, first.id);
        assert_eq!(claimed.status, "submitting");
        assert!(!db.cancel_followup("thread-a", &first.id).unwrap());
        assert!(db.cancel_followup("thread-a", &second.id).unwrap());

        db.mark_followup_submitted(&first.id, json!({"turn_id":"turn-next"}))
            .unwrap();
        let listed = db.list_followups("thread-a", 10).unwrap();
        let submitted = listed.iter().find(|item| item.id == first.id).unwrap();
        let cancelled = listed.iter().find(|item| item.id == second.id).unwrap();
        assert_eq!(submitted.status, "submitted");
        assert_eq!(
            submitted.result_json.as_deref(),
            Some("{\"turn_id\":\"turn-next\"}")
        );
        assert_eq!(cancelled.status, "cancelled");
    }

    #[test]
    fn active_followup_upload_ids_only_include_pending_or_submitting_items() {
        let db = PanelDb::open(":memory:").unwrap();
        let pending = db
            .enqueue_followup(
                "thread-a",
                "pending",
                json!({"attachments":["00000000-0000-0000-0000-000000000001", "not-a-uuid"]}),
            )
            .unwrap();
        let submitting = db
            .enqueue_followup(
                "thread-a",
                "submitting",
                json!({"attachments":["00000000-0000-0000-0000-000000000002"]}),
            )
            .unwrap();
        let submitted = db
            .enqueue_followup(
                "thread-a",
                "submitted",
                json!({"attachments":["00000000-0000-0000-0000-000000000003"]}),
            )
            .unwrap();

        let claimed = db.claim_next_pending_followup("thread-a").unwrap().unwrap();
        assert_eq!(claimed.id, pending.id);
        db.mark_followup_submitted(&submitted.id, json!({"ok":true}))
            .unwrap();

        let ids = db.active_followup_upload_ids().unwrap();

        assert!(ids.contains("00000000-0000-0000-0000-000000000001"));
        assert!(ids.contains("00000000-0000-0000-0000-000000000002"));
        assert!(!ids.contains("00000000-0000-0000-0000-000000000003"));
        assert!(!ids.contains("not-a-uuid"));
        assert_eq!(submitting.status, "pending");
    }

    #[test]
    fn followup_error_records_message_and_is_not_claimed_again() {
        let db = PanelDb::open(":memory:").unwrap();
        let followup = db
            .enqueue_followup("thread-a", "retry later", json!({}))
            .unwrap();
        let claimed = db.claim_next_pending_followup("thread-a").unwrap().unwrap();
        db.mark_followup_error(&claimed.id, "local state unavailable")
            .unwrap();

        assert!(db
            .claim_next_pending_followup("thread-a")
            .unwrap()
            .is_none());
        let listed = db.list_followups("thread-a", 10).unwrap();
        let errored = listed.iter().find(|item| item.id == followup.id).unwrap();
        assert_eq!(errored.status, "error");
        assert_eq!(errored.error.as_deref(), Some("local state unavailable"));
    }

    #[test]
    fn probe_events_and_dedupe_persist_recent_runtime_state() {
        let db = PanelDb::open(":memory:").unwrap();

        let first_claim = db
            .claim_probe_dedupe("reply_needed", "thread-a:turn-1", 60)
            .unwrap();
        let duplicate_claim = db
            .claim_probe_dedupe("reply_needed", "thread-a:turn-1", 60)
            .unwrap();
        assert!(first_claim);
        assert!(!duplicate_claim);

        let event = db
            .record_probe_event(super::NewProbeEvent {
                kind: "reply-needed",
                thread_id: Some("thread-a"),
                title: Some("等待确认"),
                message: Some("等待用户选择"),
                dedupe_key: Some("thread-a:turn-1"),
                source: "hook-stop",
                payload: json!({"turn_id":"turn-1"}),
            })
            .unwrap();

        let events = db.list_probe_events(10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event.id);
        assert_eq!(events[0].kind, "reply-needed");
        assert_eq!(events[0].thread_id.as_deref(), Some("thread-a"));
        assert_eq!(events[0].payload["turn_id"], "turn-1");
        assert!(events[0].handled_at.is_none());

        assert!(db.mark_probe_event_handled(&event.id).unwrap());
        let handled = db.list_probe_events(10).unwrap();
        assert!(handled[0].handled_at.is_some());
    }

    #[test]
    fn maintain_probe_events_deletes_old_events_and_expired_dedupe_with_limit() {
        let db = PanelDb::open(":memory:").unwrap();
        let now = PanelDb::now();
        {
            let conn = db.conn.lock().expect("db mutex");
            for index in 0..3 {
                conn.execute(
                    r#"
                    INSERT INTO probe_events(
                      id, kind, thread_id, title, message, dedupe_key, source, payload_json, created_at
                    )
                    VALUES(?1, 'hook-stop', 'thread-a', 'old', 'old', ?1, 'test', '{}', ?2)
                    "#,
                    rusqlite::params![format!("old-event-{index}"), now - 172_900],
                )
                .unwrap();
                conn.execute(
                    r#"
                    INSERT INTO probe_dedupe(namespace, dedupe_key, expires_at, created_at)
                    VALUES('probe_event', ?1, ?2, ?3)
                    "#,
                    rusqlite::params![format!("old-dedupe-{index}"), now - 1, now - 172_900],
                )
                .unwrap();
            }
            conn.execute(
                r#"
                INSERT INTO probe_events(
                  id, kind, thread_id, title, message, dedupe_key, source, payload_json, created_at
                )
                VALUES('fresh-event', 'hook-stop', 'thread-a', 'fresh', 'fresh', 'fresh-event', 'test', '{}', ?1)
                "#,
                rusqlite::params![now],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO probe_dedupe(namespace, dedupe_key, expires_at, created_at)
                VALUES('probe_event', 'fresh-dedupe', ?1, ?2)
                "#,
                rusqlite::params![now + 300, now],
            )
            .unwrap();
        }

        let dry_run = db.maintain_probe_events(1, 2, true).unwrap();
        assert_eq!(dry_run, (2, 2));

        let deleted = db.maintain_probe_events(1, 2, false).unwrap();
        assert_eq!(deleted, (2, 2));

        let counts = db.notification_retention_counts(1).unwrap();
        assert_eq!(counts.event_count, 2);
        assert_eq!(counts.dedupe_count, 2);
        assert_eq!(counts.pending_event_count, 1);
        assert_eq!(counts.pending_dedupe_count, 1);

        let deleted = db.maintain_probe_events(1, 10, false).unwrap();
        assert_eq!(deleted, (1, 1));
        let counts = db.notification_retention_counts(1).unwrap();
        assert_eq!(counts.event_count, 1);
        assert_eq!(counts.dedupe_count, 1);
        assert_eq!(counts.pending_event_count, 0);
        assert_eq!(counts.pending_dedupe_count, 0);
    }

    #[test]
    fn maintain_probe_events_prunes_expired_markers_deliveries_and_incidents_without_touching_pending_or_settings(
    ) {
        let db = PanelDb::open(":memory:").unwrap();
        let now = PanelDb::now();
        let old_secs = now - 10 * 86_400;
        let fresh_secs = now - 2 * 86_400;
        {
            let conn = db.conn.lock().expect("db mutex");
            for (k, ts) in [
                ("probe_passive_sent_marker:old", old_secs),
                ("probe_passive_sent_marker:fresh", fresh_secs),
                ("probe_async_question_delivery:old", old_secs),
                ("probe_async_question_delivery:fresh", fresh_secs),
                ("probe_feedback_delivery:old", old_secs),
                ("admin_api_key_sha256", old_secs),
            ] {
                conn.execute(
                    "INSERT OR REPLACE INTO settings(key, value, updated_at) VALUES(?1, '1', ?2)",
                    rusqlite::params![k, ts],
                )
                .unwrap();
            }
            for (ek, status, ms) in [
                ("term-old", "sent", old_secs * 1_000),
                ("term-fresh", "sent", fresh_secs * 1_000),
                ("pending-old", "pending", old_secs * 1_000),
            ] {
                conn.execute(
                    "INSERT INTO probe_notification_deliveries(event_key, channel, status, created_ms, updated_ms)
                     VALUES(?1, 'bark', ?2, ?3, ?3)",
                    rusqlite::params![ek, status, ms],
                )
                .unwrap();
            }
            for (ik, row_id, status, ts) in [
                ("inc-old-sent", 1_i64, "sent", old_secs),
                ("inc-fresh-sent", 2_i64, "sent", fresh_secs),
                ("inc-old-pending", 3_i64, "pending", old_secs),
            ] {
                conn.execute(
                    "INSERT INTO probe_error_incidents(
                        incident_key, source_ts, source_ts_nanos, source_row_id,
                        thread_id, turn_id, classification, error_sha256, error_summary,
                        bark_status, created_at, updated_at
                    ) VALUES(?1, 1, 0, ?2, 't1', 'u1', 'network', 'sha', 'err', ?3, ?4, ?4)",
                    rusqlite::params![ik, row_id, status, ts],
                )
                .unwrap();
            }
        }

        db.maintain_notification_history_if_due(7).unwrap();

        let conn = db.conn.lock().expect("db mutex");
        let has_setting = |k: &str| -> bool {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM settings WHERE key=?1)",
                [k],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(!has_setting("probe_passive_sent_marker:old"));
        assert!(has_setting("probe_passive_sent_marker:fresh"));
        assert!(!has_setting("probe_async_question_delivery:old"));
        assert!(has_setting("probe_async_question_delivery:fresh"));
        assert!(!has_setting("probe_feedback_delivery:old"));
        assert!(has_setting("admin_api_key_sha256"));

        let delivery_keys: Vec<String> = conn
            .prepare("SELECT event_key FROM probe_notification_deliveries ORDER BY event_key")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(delivery_keys, vec!["pending-old", "term-fresh"]);

        let incident_keys: Vec<String> = conn
            .prepare("SELECT incident_key FROM probe_error_incidents ORDER BY incident_key")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(incident_keys, vec!["inc-fresh-sent", "inc-old-pending"]);
    }

    #[test]
    fn active_unresolved_markers_survive_seven_day_prune_and_touch_throttles_daily() {
        let db = PanelDb::open(":memory:").unwrap();
        let now = PanelDb::now();
        let ten_days_ago = now - 10 * 86_400;
        let two_hours_ago = now - 2 * 3_600;
        {
            let conn = db.conn.lock().expect("db mutex");
            for (k, ts) in [
                ("probe_async_question_delivery:active-top-n", ten_days_ago),
                ("probe_passive_sent_marker:active-top-n", ten_days_ago),
                ("probe_feedback_delivery:inactive-old", ten_days_ago),
                ("probe_feedback_delivery:recently-touched", two_hours_ago),
            ] {
                conn.execute(
                    "INSERT INTO settings(key, value, updated_at) VALUES(?1, '1', ?2)",
                    rusqlite::params![k, ts],
                )
                .unwrap();
            }
        }
        // Within 24h window: touch must NOT write.
        assert!(!db
            .touch_setting_if_older_than("probe_feedback_delivery:recently-touched", 86_400)
            .unwrap());
        // Even before touch, active unresolved marker registry protects >7d top-N markers from pruning.
        db.retain_active_unresolved_markers(
            "test",
            &[
                "probe_async_question_delivery:active-top-n".to_string(),
                "probe_passive_sent_marker:active-top-n".to_string(),
            ],
        )
        .unwrap();
        db.maintain_probe_events(7, 100, false).unwrap();
        assert!(db
            .get_setting("probe_async_question_delivery:active-top-n")
            .unwrap()
            .is_some());
        assert!(db
            .get_setting("probe_passive_sent_marker:active-top-n")
            .unwrap()
            .is_some());
        assert!(db
            .get_setting("probe_feedback_delivery:inactive-old")
            .unwrap()
            .is_none());

        // Older than 24h: touch refreshes updated_at once, and subsequent touch in same day is a no-op.
        assert!(db
            .touch_setting_if_older_than("probe_async_question_delivery:active-top-n", 86_400)
            .unwrap());
        assert!(!db
            .touch_setting_if_older_than("probe_async_question_delivery:active-top-n", 86_400)
            .unwrap());
    }

    #[test]
    fn maintain_notification_history_truncates_wal_file_on_disk() {
        let root =
            std::env::temp_dir().join(format!("agenthub-wal-trunc-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db_path = root.join("panel.sqlite");
        let wal_path = root.join("panel.sqlite-wal");
        let db = PanelDb::open(&db_path).unwrap();
        let expired = PanelDb::now() - 10 * 86_400;
        let big_payload = serde_json::json!({"blob": "x".repeat(64 * 1024)});
        for i in 0..8 {
            let ev = db
                .record_probe_event(NewProbeEvent {
                    kind: "completion",
                    thread_id: Some("t-wal"),
                    title: Some("wal"),
                    message: Some("msg"),
                    dedupe_key: None,
                    source: "test",
                    payload: big_payload.clone(),
                })
                .unwrap();
            db.conn
                .lock()
                .expect("db mutex")
                .execute(
                    "UPDATE probe_events SET created_at = ?2 WHERE id = ?1",
                    rusqlite::params![ev.id, expired + i],
                )
                .unwrap();
        }
        let wal_before = fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert!(wal_before > 0, "expected non-empty WAL before checkpoint");
        db.maintain_notification_history_if_due(7).unwrap();
        let wal_after = fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        assert_eq!(
            wal_after, 0,
            "expected TRUNCATE checkpoint to shrink WAL to 0B"
        );
        drop(db);
        let _ = fs::remove_dir_all(&root);
    }
}
