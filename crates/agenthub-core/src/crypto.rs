use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct SecretBox {
    key: [u8; 32],
}

impl SecretBox {
    /// For native credential stores. Never persist this value beside ciphertext
    /// or expose it to a WebView, configuration DTO or diagnostic output.
    pub fn random_key_material() -> String {
        let mut key = [0_u8; 32];
        OsRng.fill_bytes(&mut key);
        general_purpose::STANDARD.encode(key)
    }

    pub fn from_key_material(value: &str) -> Result<Self> {
        let raw = parse_key_material(value)?;
        if raw.len() != 32 {
            return Err(anyhow!("secret key must decode to exactly 32 bytes"));
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&raw);
        Ok(Self { key })
    }

    pub fn deterministic_dev() -> Self {
        let digest = Sha256::digest(b"agenthub-development-key");
        let mut key = [0_u8; 32];
        key.copy_from_slice(&digest);
        Self { key }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let cipher = Aes256Gcm::new((&self.key).into());
        let mut nonce_bytes = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
            .map_err(|_| anyhow!("encrypt secret"))?;
        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        if nonce.len() != 12 {
            return Err(anyhow!("invalid secret nonce length"));
        }
        let cipher = Aes256Gcm::new((&self.key).into());
        cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| anyhow!("decrypt secret"))
    }
}

fn parse_key_material(value: &str) -> Result<Vec<u8>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("empty secret key"));
    }
    if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(hex::decode(trimmed)?);
    }
    for encoding in [
        general_purpose::STANDARD,
        general_purpose::STANDARD_NO_PAD,
        general_purpose::URL_SAFE,
        general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = encoding.decode(trimmed) {
            return Ok(decoded);
        }
    }
    if let Ok(decoded) = hex::decode(trimmed) {
        return Ok(decoded);
    }
    Err(anyhow!("secret key must be base64, base64url, or hex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_supported_encodings_preserve_the_same_key() {
        let bytes = [0xfb; 32];
        let expected = SecretBox::from_key_material(&hex::encode(bytes)).unwrap();
        let (ciphertext, nonce) = expected.encrypt(b"fixture-notification").unwrap();
        for encoded in [
            hex::encode_upper(bytes),
            general_purpose::STANDARD.encode(bytes),
            general_purpose::STANDARD_NO_PAD.encode(bytes),
            general_purpose::URL_SAFE.encode(bytes),
            general_purpose::URL_SAFE_NO_PAD.encode(bytes),
        ] {
            assert_eq!(
                SecretBox::from_key_material(&encoded)
                    .unwrap()
                    .decrypt(&ciphertext, &nonce)
                    .unwrap(),
                b"fixture-notification"
            );
        }
        assert!(SecretBox::from_key_material("fixture-invalid-key").is_err());
    }
}
