//! Row-level AES-256-GCM envelope for secret-bearing store
//! values. Reuses `secret_storage::crypto`'s scrypt + AES-GCM
//! pipeline so the on-disk wire format (modulo the redb framing)
//! matches what `FileSecretStore` writes.
//!
//! Each call to [`encrypt_secret`] generates a fresh scrypt
//! salt + 12-byte IV; rotating the passphrase requires
//! re-encrypting every wrapped row (a future
//! `WalletStore::rotate_passphrase()` will iterate them).

use serde::{Deserialize, Serialize};

use crate::secret_storage::SecretStoreError;
use crate::store::error::StoreError;

/// Wire form. Field names are intentionally short — bincode
/// already encodes them positionally, but if we ever swap the
/// codec to anything human-readable we don't want the table
/// growing by 4× for the tags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretEnvelope {
    pub salt: Vec<u8>,
    pub iv: Vec<u8>,
    pub tag: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Wrap a secret under the wallet store passphrase. Surfaces
/// the underlying crypto failure (very rare — scrypt + AES-GCM
/// are infallible in practice) as `StoreError::Crypto`.
pub fn encrypt_secret(passphrase: &str, plaintext: &[u8]) -> Result<SecretEnvelope, StoreError> {
    let mut rng = rand::thread_rng();
    let env = crate::secret_storage::crypto::encrypt_json(passphrase, plaintext, &mut rng)
        .map_err(StoreError::from)?;
    Ok(SecretEnvelope {
        salt: decode_b64(&env.salt)?,
        iv: decode_b64(&env.iv)?,
        tag: decode_b64(&env.tag)?,
        ciphertext: decode_b64(&env.ciphertext)?,
    })
}

/// Unwrap a stored envelope back to plaintext. Wrong passphrase
/// → `StoreError::Crypto` (the GCM auth tag mismatch surfaces
/// at the underlying crypto layer).
pub fn decrypt_secret(passphrase: &str, env: &SecretEnvelope) -> Result<Vec<u8>, StoreError> {
    let upstream = crate::secret_storage::crypto::EncryptedPayload {
        salt: encode_b64(&env.salt),
        iv: encode_b64(&env.iv),
        tag: encode_b64(&env.tag),
        ciphertext: encode_b64(&env.ciphertext),
    };
    crate::secret_storage::crypto::decrypt_json(passphrase, &upstream).map_err(StoreError::from)
}

// ── helpers ────────────────────────────────────────────────────

/// The upstream envelope is base64-coded for JSON; here we want
/// raw bytes in the bincoded row. Re-encode in both directions
/// to avoid double-base64 on the wire.
fn decode_b64(s: &str) -> Result<Vec<u8>, StoreError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .map_err(|e| StoreError::from(SecretStoreError::InvalidInput(format!("b64 decode: {e}"))))
}

fn encode_b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_under_same_passphrase() {
        let env = encrypt_secret("pw", b"the quick brown fox").unwrap();
        let back = decrypt_secret("pw", &env).unwrap();
        assert_eq!(back, b"the quick brown fox");
    }

    #[test]
    fn wrong_passphrase_fails() {
        let env = encrypt_secret("correct", b"secret").unwrap();
        let err = decrypt_secret("wrong", &env).unwrap_err();
        assert!(matches!(err, StoreError::Crypto(_)));
    }

    #[test]
    fn freshly_generated_salts_differ() {
        let a = encrypt_secret("pw", b"plain").unwrap();
        let b = encrypt_secret("pw", b"plain").unwrap();
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.iv, b.iv);
        assert_ne!(a.ciphertext, b.ciphertext);
    }
}
