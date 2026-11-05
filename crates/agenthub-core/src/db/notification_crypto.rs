//! Runtime-key upgrades touch only AgentHub notification secrets and payloads.
//! The desktop submission ledger has a separate Keychain key.
use super::{encrypted_setting_parts, PanelDb};
use crate::crypto::SecretBox;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

fn decrypt_legacy(ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
    for key in SecretBox::legacy_development_keys() {
        if let Ok(plaintext) = key.decrypt(ciphertext, nonce) {
            return Ok(plaintext);
        }
    }
    bail!("通知密钥缺失或不匹配，原数据已保留，请恢复原密钥")
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == column))
}

fn check_payloads(
    conn: &Connection,
    decrypt: impl Fn(&[u8], &[u8]) -> Result<Vec<u8>>,
) -> Result<()> {
    if has_column(conn, "settings", "value")? {
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key='probe_bark_device_key'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(parts) = value.as_deref().and_then(encrypted_setting_parts) {
            let (ciphertext, nonce) = parts?;
            decrypt(&ciphertext, &nonce)?;
        }
    }
    for table in ["probe_notification_deliveries", "native_probe_deliveries"] {
        if has_column(conn, table, "ciphertext")? {
            let mut stmt = conn.prepare(&format!(
                "SELECT ciphertext,nonce FROM {table} WHERE ciphertext IS NOT NULL LIMIT 1"
            ))?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                decrypt(&row.get::<_, Vec<u8>>(0)?, &row.get::<_, Vec<u8>>(1)?)?;
            }
        }
    }
    Ok(())
}

pub(super) fn validate_key(conn: &Connection, crypto: &SecretBox) -> Result<()> {
    check_payloads(conn, |ciphertext, nonce| crypto.decrypt(ciphertext, nonce))
        .context("通知密钥或加密数据异常，原数据已保留")
}

impl PanelDb {
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn validate_legacy_notification_key(path: &std::path::Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        check_payloads(&conn, decrypt_legacy)
    }

    pub(super) fn migrate_notification_key(&self, tx: &rusqlite::Transaction<'_>) -> Result<()> {
        let value: Option<String> = tx
            .query_row(
                "SELECT value FROM settings WHERE key='probe_bark_device_key'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(parts) = value.as_deref().and_then(encrypted_setting_parts) {
            let (ciphertext, nonce) = parts?;
            if self.crypto.decrypt(&ciphertext, &nonce).is_err() {
                let (ciphertext, nonce) =
                    self.crypto.encrypt(&decrypt_legacy(&ciphertext, &nonce)?)?;
                tx.execute("UPDATE settings SET value=?1 WHERE key='probe_bark_device_key'",
                    [json!({"ciphertext":STANDARD.encode(ciphertext),"nonce":STANDARD.encode(nonce)}).to_string()])?;
            }
        }
        // Native payloads are plaintext before schema 3. Retry chunks already use
        // encryption; rekey their bytes without changing claims or acknowledgements.
        let mut stmt = tx.prepare("SELECT event_key,channel,ciphertext,nonce FROM probe_notification_deliveries WHERE ciphertext IS NOT NULL")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let ciphertext: Vec<u8> = row.get(2)?;
            let nonce: Vec<u8> = row.get(3)?;
            if self.crypto.decrypt(&ciphertext, &nonce).is_err() {
                let (ciphertext, nonce) =
                    self.crypto.encrypt(&decrypt_legacy(&ciphertext, &nonce)?)?;
                tx.execute("UPDATE probe_notification_deliveries SET ciphertext=?3,nonce=?4 WHERE event_key=?1 AND channel=?2",
                    params![row.get::<_, String>(0)?,row.get::<_, String>(1)?,ciphertext,nonce])?;
            }
        }
        Ok(())
    }
}
