//! AgentHub-owned outgoing messages only. This never reads or writes Codex's
//! native queue/database. The adapter supplies a dedicated Keychain-backed key;
//! it must not reuse PanelDb's development fallback encryption key.
use super::PanelDb;
use crate::crypto::SecretBox;
use anyhow::{anyhow, ensure, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS codex_desktop_submissions (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN
        ('queued','held','dispatching','accepted','outcome_unknown','cancelled','failed','dismissed')),
    ciphertext BLOB,
    nonce BLOB,
    turn_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_codex_desktop_submissions_pending
    ON codex_desktop_submissions(source_id,thread_id,status,created_at);
"#;

const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_RETAINED_BYTES: i64 = 32 * 1024 * 1024;
const MAX_PENDING: i64 = 1000;
const MAX_THREAD_PENDING: i64 = 50;
const MAX_TERMINAL: i64 = 1000;

#[derive(Clone)]
pub struct DesktopLedger {
    db: PanelDb,
    crypto: SecretBox,
}

/// One queue worker per AgentHub database, including across App processes.
/// Kept alive by the controller; it is not a lease that expires mid-dispatch.
#[cfg(unix)]
pub struct DesktopWorkerLease(std::fs::File);

#[cfg(unix)]
impl Drop for DesktopWorkerLease {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the file remains owned by this guard until after unlock.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

// These internal records intentionally have no Debug/Serialize implementation.
// Projection into public DTOs must not accidentally expose plaintext or keys.
pub struct Submission {
    pub id: String,
    pub source_id: String,
    pub thread_id: String,
    pub status: String,
    pub turn_id: Option<String>,
    pub created_at: i64,
    pub body: Option<Value>,
}

impl DesktopLedger {
    pub fn new(db: PanelDb, crypto: SecretBox) -> Self {
        Self { db, crypto }
    }

    pub fn has_encrypted_messages(db: &PanelDb) -> Result<bool> {
        db.conn.lock().map_err(|_| safe_error())?.query_row(
            "SELECT EXISTS(SELECT 1 FROM codex_desktop_submissions WHERE ciphertext IS NOT NULL)",[],|row|row.get(0),
        ).map_err(|_| safe_error())
    }

    #[cfg(unix)]
    pub fn lock_database(db: &PanelDb) -> Result<DesktopWorkerLease> {
        use std::os::{
            fd::AsRawFd,
            unix::fs::{MetadataExt, OpenOptionsExt},
        };
        let mut path = db.path.as_os_str().to_owned();
        path.push(".desktop.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| safe_error())?;
        let metadata = file.metadata().map_err(|_| safe_error())?;
        // SAFETY: geteuid takes no arguments and does not mutate process state.
        ensure!(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o077 == 0,
            "desktop queue ownership could not be verified"
        );
        // SAFETY: this descriptor is valid and retained for the entire worker.
        ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "desktop queue is owned by another AgentHub process"
        );
        Ok(DesktopWorkerLease(file))
    }

    pub fn enqueue(&self, source: &str, thread: &str, id: &str, body: &Value) -> Result<()> {
        validate_identity(source, thread, id)?;
        let plaintext = serde_json::to_vec(&json!({
            "source":source,"thread":thread,"id":id,"body":body
        }))
        .map_err(|_| safe_error())?;
        ensure!(
            plaintext.len() <= MAX_BODY_BYTES,
            "desktop submission is too large"
        );
        let (ciphertext, nonce) = self.crypto.encrypt(&plaintext).map_err(|_| safe_error())?;
        let mut conn = self.db.conn.lock().map_err(|_| safe_error())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| safe_error())?;
        // Idempotent UI retry must use exactly the same identity and plaintext.
        // It never resubmits a dispatching or unknown record to the native App.
        if let Some((existing_source, existing_thread, existing_cipher, existing_nonce)) = tx.query_row(
            "SELECT source_id,thread_id,ciphertext,nonce FROM codex_desktop_submissions WHERE id=?1",
            [id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<Vec<u8>>>(2)?,row.get::<_,Option<Vec<u8>>>(3)?)),
        ).optional().map_err(|_| safe_error())? {
            ensure!(existing_source == source && existing_thread == thread, "desktop submission identity changed");
            if let (Some(cipher), Some(nonce)) = (existing_cipher, existing_nonce) {
                ensure!(self.crypto.decrypt(&cipher, &nonce).map_err(|_| safe_error())? == plaintext,
                    "desktop submission identity changed");
            }
            return Ok(());
        }
        let (pending, bytes, thread_pending): (i64,i64,i64) = tx.query_row(
            "SELECT count(*),coalesce(sum(length(ciphertext)),0),coalesce(sum(source_id=?1 AND thread_id=?2),0)
             FROM codex_desktop_submissions WHERE ciphertext IS NOT NULL",
            params![source,thread], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).map_err(|_| safe_error())?;
        ensure!(
            pending < MAX_PENDING
                && thread_pending < MAX_THREAD_PENDING
                && bytes.saturating_add(ciphertext.len() as i64) <= MAX_RETAINED_BYTES,
            "desktop submission queue is full"
        );
        tx.execute(
            "INSERT INTO codex_desktop_submissions(id,source_id,thread_id,status,ciphertext,nonce,created_at,updated_at)
             VALUES(?1,?2,?3,'queued',?4,?5,?6,?6)",
            params![id,source,thread,ciphertext,nonce,chrono::Utc::now().timestamp_millis()],
        ).map_err(|_| safe_error())?;
        tx.execute(
            "DELETE FROM codex_desktop_submissions WHERE id IN (
                SELECT id FROM codex_desktop_submissions WHERE ciphertext IS NULL
                ORDER BY rowid DESC LIMIT -1 OFFSET ?1)",
            [MAX_TERMINAL],
        )
        .map_err(|_| safe_error())?;
        tx.commit().map_err(|_| safe_error())
    }

    pub fn list(&self, source: &str, thread: &str) -> Result<Vec<Submission>> {
        validate_source_thread(source, thread)?;
        let conn = self.db.conn.lock().map_err(|_| safe_error())?;
        let mut query = conn.prepare(
            "SELECT id,source_id,thread_id,status,turn_id,created_at,ciphertext,nonce
             FROM codex_desktop_submissions WHERE source_id=?1 AND thread_id=?2 AND ciphertext IS NOT NULL
             ORDER BY rowid LIMIT 100",
        ).map_err(|_| safe_error())?;
        let rows = query
            .query_map(params![source, thread], |row| {
                Ok((
                    Submission {
                        id: row.get(0)?,
                        source_id: row.get(1)?,
                        thread_id: row.get(2)?,
                        status: row.get(3)?,
                        turn_id: row.get(4)?,
                        created_at: row.get(5)?,
                        body: None,
                    },
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                ))
            })
            .map_err(|_| safe_error())?;
        let encrypted: Vec<_> = rows
            .collect::<rusqlite::Result<_>>()
            .map_err(|_| safe_error())?;
        drop(query);
        drop(conn);
        encrypted
            .into_iter()
            .map(|(mut record, cipher, nonce)| {
                let plaintext = self
                    .crypto
                    .decrypt(&cipher, &nonce)
                    .map_err(|_| safe_error())?;
                let mut payload: Value =
                    serde_json::from_slice(&plaintext).map_err(|_| safe_error())?;
                ensure!(
                    payload["source"] == record.source_id
                        && payload["thread"] == record.thread_id
                        && payload["id"] == record.id,
                    "desktop submission identity changed"
                );
                record.body = Some(payload["body"].take());
                Ok(record)
            })
            .collect()
    }

    /// Persist BEFORE attempting native IPC. A crash or ambiguous reply can
    /// therefore never cause automatic replay after reconnect/restart.
    pub fn claim(&self, source: &str, thread: &str, id: &str) -> Result<bool> {
        validate_identity(source, thread, id)?;
        let conn = self.db.conn.lock().map_err(|_| safe_error())?;
        let changed = conn.execute(
            "UPDATE codex_desktop_submissions SET status='dispatching',updated_at=?4
             WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status='queued'
             AND NOT EXISTS(SELECT 1 FROM codex_desktop_submissions WHERE source_id=?2 AND thread_id=?3
                AND status IN ('dispatching','outcome_unknown'))
             AND id=(SELECT id FROM codex_desktop_submissions WHERE source_id=?2 AND thread_id=?3
                AND status IN ('queued','held') ORDER BY rowid LIMIT 1)",
            params![id,source,thread,chrono::Utc::now().timestamp_millis()],
        ).map_err(|_| safe_error())?;
        Ok(changed == 1)
    }

    /// Explicit steer is a separate user action, not an automatic queue drain.
    /// It may pass queued items but never an in-flight/uncertain submission.
    pub fn claim_immediate(&self, source: &str, thread: &str, id: &str) -> Result<bool> {
        validate_identity(source, thread, id)?;
        let changed = self.db.conn.lock().map_err(|_| safe_error())?.execute(
            "UPDATE codex_desktop_submissions SET status='dispatching',updated_at=?4
             WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status='queued'
             AND NOT EXISTS(SELECT 1 FROM codex_desktop_submissions WHERE source_id=?2 AND thread_id=?3
                AND status IN ('dispatching','outcome_unknown'))",
            params![id,source,thread,chrono::Utc::now().timestamp_millis()],
        ).map_err(|_| safe_error())?;
        Ok(changed == 1)
    }

    pub fn accepted(&self, source: &str, thread: &str, id: &str, turn_id: &str) -> Result<bool> {
        validate_identity(source, thread, id)?;
        ensure!(
            !turn_id.is_empty() && turn_id.len() <= 128 && !turn_id.chars().any(char::is_control),
            "desktop turn identity is invalid"
        );
        let conn = self.db.conn.lock().map_err(|_| safe_error())?;
        let changed = conn.execute(
            "UPDATE codex_desktop_submissions SET status='accepted',turn_id=?4,ciphertext=NULL,nonce=NULL,updated_at=?5
             WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status IN ('dispatching','outcome_unknown')",
            params![id,source,thread,turn_id,chrono::Utc::now().timestamp_millis()],
        ).map_err(|_| safe_error())?;
        Ok(changed == 1)
    }

    pub fn outcome_unknown(&self, source: &str, thread: &str, id: &str) -> Result<()> {
        validate_identity(source, thread, id)?;
        self.db
            .conn
            .lock()
            .map_err(|_| safe_error())?
            .execute(
                "UPDATE codex_desktop_submissions SET status='outcome_unknown',updated_at=?4
             WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status='dispatching'",
                params![id, source, thread, chrono::Utc::now().timestamp_millis()],
            )
            .map_err(|_| safe_error())?;
        Ok(())
    }

    /// Only a definite pre-send rejection may put the message back on hold.
    pub fn not_sent(&self, source: &str, thread: &str, id: &str) -> Result<()> {
        validate_identity(source, thread, id)?;
        self.db
            .conn
            .lock()
            .map_err(|_| safe_error())?
            .execute(
                "UPDATE codex_desktop_submissions SET status='held',updated_at=?4
             WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status='dispatching'",
                params![id, source, thread, chrono::Utc::now().timestamp_millis()],
            )
            .map_err(|_| safe_error())?;
        Ok(())
    }

    pub fn cancel(&self, source: &str, thread: &str, id: &str) -> Result<bool> {
        validate_identity(source, thread, id)?;
        let changed = self.db.conn.lock().map_err(|_| safe_error())?.execute(
            "UPDATE codex_desktop_submissions SET status='cancelled',ciphertext=NULL,nonce=NULL,updated_at=?4
             WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status IN ('queued','held')",
            params![id,source,thread,chrono::Utc::now().timestamp_millis()],
        ).map_err(|_| safe_error())?;
        Ok(changed == 1)
    }

    pub fn hold(&self, source: &str, thread: &str) -> Result<()> {
        self.set_hold(source, thread, true)
    }

    /// An immediate steer that could not claim the native lane is retained for
    /// explicit removal, never converted into a delayed steer or a new turn.
    pub fn hold_queued(&self, source: &str, thread: &str, id: &str) -> Result<()> {
        validate_identity(source, thread, id)?;
        self.db.conn.lock().map_err(|_|safe_error())?.execute(
            "UPDATE codex_desktop_submissions SET status='held',updated_at=?4 WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status='queued'",
            params![id,source,thread,chrono::Utc::now().timestamp_millis()],
        ).map_err(|_|safe_error())?;
        Ok(())
    }

    pub fn resume(&self, source: &str, thread: &str) -> Result<()> {
        self.set_hold(source, thread, false)
    }

    /// Resume only the queue entries the worker has decoded and classified as
    /// follow-ups. A failed steer must never turn into a fresh turn on resume.
    pub fn resume_selected(&self, source: &str, thread: &str, ids: &[String]) -> Result<()> {
        validate_source_thread(source, thread)?;
        ensure!(
            ids.len() <= MAX_THREAD_PENDING as usize,
            "desktop queue selection is too large"
        );
        let mut conn = self.db.conn.lock().map_err(|_| safe_error())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| safe_error())?;
        for id in ids {
            validate_identity(source, thread, id)?;
            tx.execute("UPDATE codex_desktop_submissions SET status='queued',updated_at=?4 WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status='held'",
                params![id,source,thread,chrono::Utc::now().timestamp_millis()]).map_err(|_|safe_error())?;
        }
        tx.commit().map_err(|_| safe_error())
    }

    /// Explicit user acknowledgement after checking the native desktop. This
    /// removes only our uncertain record; it does not retry or interrupt Codex.
    pub fn dismiss_unknown(&self, source: &str, thread: &str, id: &str) -> Result<bool> {
        validate_identity(source, thread, id)?;
        self.db.conn.lock().map_err(|_|safe_error())?.execute(
            "UPDATE codex_desktop_submissions SET status='dismissed',ciphertext=NULL,nonce=NULL,updated_at=?4 WHERE id=?1 AND source_id=?2 AND thread_id=?3 AND status='outcome_unknown'",
            params![id,source,thread,chrono::Utc::now().timestamp_millis()],
        ).map(|changed|changed==1).map_err(|_|safe_error())
    }

    fn set_hold(&self, source: &str, thread: &str, hold: bool) -> Result<()> {
        validate_source_thread(source, thread)?;
        self.db
            .conn
            .lock()
            .map_err(|_| safe_error())?
            .execute(
                "UPDATE codex_desktop_submissions SET status=?3,updated_at=?5
             WHERE source_id=?1 AND thread_id=?2 AND status=?4",
                params![
                    source,
                    thread,
                    if hold { "held" } else { "queued" },
                    if hold { "queued" } else { "held" },
                    chrono::Utc::now().timestamp_millis()
                ],
            )
            .map_err(|_| safe_error())?;
        Ok(())
    }

    /// Called once when the owning desktop worker starts. Previously queued
    /// work stays visible but held; uncertain writes require reconciliation.
    pub fn recover(&self, source: &str) -> Result<()> {
        validate_source(source)?;
        self.db.conn.lock().map_err(|_| safe_error())?.execute(
            "UPDATE codex_desktop_submissions SET status=CASE status WHEN 'dispatching' THEN 'outcome_unknown' ELSE 'held' END,
                updated_at=?2 WHERE source_id=?1 AND status IN ('dispatching','queued')",
            params![source,chrono::Utc::now().timestamp_millis()],
        ).map_err(|_| safe_error())?;
        Ok(())
    }
}

fn validate_identity(source: &str, thread: &str, id: &str) -> Result<()> {
    validate_source_thread(source, thread)?;
    uuid::Uuid::parse_str(id).map_err(|_| anyhow!("desktop submission identity is invalid"))?;
    Ok(())
}
fn validate_source_thread(source: &str, thread: &str) -> Result<()> {
    validate_source(source)?;
    uuid::Uuid::parse_str(thread).map_err(|_| anyhow!("desktop thread identity is invalid"))?;
    Ok(())
}
fn validate_source(source: &str) -> Result<()> {
    ensure!(
        source.len() == 64 && source.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "desktop source identity is invalid"
    );
    Ok(())
}
fn safe_error() -> anyhow::Error {
    anyhow!("desktop submission storage is unavailable")
}

#[cfg(test)]
#[path = "desktop_submissions_tests.rs"]
mod tests;
