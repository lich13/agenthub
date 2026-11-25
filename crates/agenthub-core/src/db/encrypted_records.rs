use super::{encrypted_setting_parts, NativeDelivery, PanelDb};
use crate::crypto::SecretBox;
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Transaction};
use zeroize::Zeroizing;

impl PanelDb {
    /// Run when the App or server starts. Short-lived provider hooks do not migrate unrelated queues.
    pub fn bind_legacy_panel_records(&self) -> Result<()> {
        self.with_transaction(|tx| {
            let mut settings = tx.prepare(
                "SELECT key, value FROM settings WHERE CASE WHEN json_valid(value) THEN
                     json_type(value, '$.ciphertext') = 'text' AND length(json_extract(value, '$.nonce')) = 16
                 END",
            )?;
            // The old 12-byte nonce encodes to 16 Base64 characters. Preserve the setting timestamp.
            let legacy = settings
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (key, encoded) in legacy {
                let (cipher, nonce) = encrypted_setting_parts(&encoded)
                    .context("invalid encrypted setting")??;
                ensure!(nonce.len() == 12, "invalid legacy setting nonce");
                let plaintext = Zeroizing::new(
                    self.crypto.decrypt(&["setting", &key], &cipher, &nonce)?
                );
                tx.execute(
                    "UPDATE settings SET value=?2 WHERE key=?1",
                    params![key, self.encrypt_setting(&key, &plaintext)?],
                )?;
            }
            Queue::Native.bind(tx, &self.crypto)?;
            Queue::Channel.bind(tx, &self.crypto)
        })
    }
}

pub(super) enum Queue {
    Native,
    Channel,
}

impl Queue {
    pub(super) fn bind(&self, tx: &Transaction<'_>, crypto: &SecretBox) -> Result<()> {
        // All SQL identifiers come from the schema. Keep at most one decrypted body in memory.
        let (table, purpose, columns) = match self {
            Self::Native => (
                "native_probe_deliveries",
                "native_notification",
                "event_key",
            ),
            Self::Channel => (
                "probe_notification_deliveries",
                "notification",
                "channel,event_key",
            ),
        };
        let ids = tx
            .prepare(&format!(
                "SELECT rowid FROM {table} WHERE ciphertext IS NOT NULL AND length(nonce)=12"
            ))?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if ids.is_empty() {
            return Ok(());
        }
        let mut read = tx.prepare(&format!(
            "SELECT ciphertext,nonce,{columns} FROM {table} WHERE rowid=?1"
        ))?;
        let mut write = tx.prepare(&format!(
            "UPDATE {table} SET ciphertext=?2,nonce=?3 WHERE rowid=?1"
        ))?;
        for id in ids {
            let mut rows = read.query([id])?;
            let row = rows.next()?.context("encrypted record disappeared")?;
            let mut identity = vec![purpose];
            for column in 2..row.as_ref().column_count() {
                identity.push(row.get_ref(column)?.as_str()?);
            }
            let plaintext = Zeroizing::new(crypto.decrypt(
                &identity,
                row.get_ref(0)?.as_blob()?,
                row.get_ref(1)?.as_blob()?,
            )?);
            match self {
                Self::Native => {
                    let delivery: NativeDelivery = serde_json::from_slice(&plaintext)?;
                    ensure!(
                        delivery.event_key == identity[1],
                        "native notification identity changed"
                    );
                }
                Self::Channel => {}
            }
            let (cipher, nonce) = crypto.encrypt(&identity, &plaintext)?;
            write.execute(params![id, cipher, nonce])?;
        }
        Ok(())
    }
}
