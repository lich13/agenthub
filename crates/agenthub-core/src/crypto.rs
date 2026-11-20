use aes_gcm::{
    aead::{Aead, KeyInit, OsRng, Payload},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use rand::RngCore;
use zeroize::Zeroizing;

const BOUND_NONCE: u8 = 2;

#[derive(Clone)]
pub struct SecretBox {
    key: Zeroizing<[u8; 32]>,
}

impl SecretBox {
    /// For native credential stores. Never persist this value beside ciphertext
    /// or expose it to a WebView, configuration DTO or diagnostic output.
    pub fn random_key_material() -> String {
        let mut key = Zeroizing::new([0_u8; 32]);
        OsRng.fill_bytes(&mut key[..]);
        general_purpose::STANDARD.encode(&key[..])
    }

    pub fn from_key_material(value: &str) -> Result<Self> {
        let raw = Zeroizing::new(parse_key_material(value)?);
        if raw.len() != 32 {
            return Err(anyhow!("secret key must decode to exactly 32 bytes"));
        }
        let mut key = Zeroizing::new([0_u8; 32]);
        key.copy_from_slice(&raw);
        Ok(Self { key })
    }

    pub(crate) fn encrypt(
        &self,
        identity: &[&str],
        plaintext: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let cipher = Aes256Gcm::new((&*self.key).into());
        let mut nonce = [0; 12];
        OsRng.fill_bytes(&mut nonce);
        let aad = serde_json::to_vec(identity)?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("encrypt secret"))?;
        let mut encoded = Vec::with_capacity(13);
        encoded.push(BOUND_NONCE);
        encoded.extend_from_slice(&nonce);
        Ok((ciphertext, encoded))
    }

    pub(crate) fn decrypt(
        &self,
        identity: &[&str],
        ciphertext: &[u8],
        encoded: &[u8],
    ) -> Result<Vec<u8>> {
        let (nonce, aad) = match encoded {
            [BOUND_NONCE, rest @ ..] if rest.len() == 12 => (rest, serde_json::to_vec(identity)?),
            legacy if legacy.len() == 12 => (legacy, Vec::new()),
            _ => return Err(anyhow!("invalid encrypted record format")),
        };
        let cipher = Aes256Gcm::new((&*self.key).into());
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
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
    Err(anyhow!("secret key must be base64, base64url, or hex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_formats_open_and_bound_records_verify_their_owner() {
        let key = SecretBox::from_key_material(&"11".repeat(32)).unwrap();
        let owner = ["notifications", "one"];
        for bound in [false, true] {
            let (cipher, nonce) = if bound {
                key.encrypt(&owner, b"fixture").unwrap()
            } else {
                let nonce = [1; 12];
                let cipher = Aes256Gcm::new((&*key.key).into())
                    .encrypt(Nonce::from_slice(&nonce), b"fixture".as_slice())
                    .unwrap();
                (cipher, nonce.to_vec())
            };
            assert_eq!(key.decrypt(&owner, &cipher, &nonce).unwrap(), b"fixture");
            if bound {
                assert!(key
                    .decrypt(&["notifications", "two"], &cipher, &nonce)
                    .is_err());
                assert!(key.decrypt(&owner, &cipher, &nonce[1..]).is_err());
            }
        }
    }

    #[test]
    fn all_supported_encodings_preserve_the_same_key() {
        let bytes = [0xfb; 32];
        let expected = SecretBox::from_key_material(&hex::encode(bytes)).unwrap();
        let (ciphertext, nonce) = expected
            .encrypt(&["settings", "fixture"], b"fixture-notification")
            .unwrap();
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
                    .decrypt(&["settings", "fixture"], &ciphertext, &nonce)
                    .unwrap(),
                b"fixture-notification"
            );
        }
        assert!(SecretBox::from_key_material("fixture-invalid-key").is_err());
    }
}
