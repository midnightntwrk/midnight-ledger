//! In-memory [`SecretStorage`] backend for unit tests and the
//! `test-support` feature.
//!
//! HashMap-backed; no persistence; no encryption envelope; not
//! thread-safe across processes but `Send + Sync` within one (a
//! `std::sync::Mutex` guards the map).
//!
//! Used by:
//! - The Identity Centre bootstrap test fixture (Task 2) to spin up
//!   a wallet + secret store without touching disk.
//! - Future async smoke tests that want a curve-bearing store
//!   without paying the redb / file-store setup tax.
//!
//! Gated behind `#[cfg(any(test, feature = "test-support"))]` so the
//! type does not appear in release builds unless a downstream crate
//! opts in.

#![cfg(any(test, feature = "test-support"))]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use uuid::Uuid;

use crate::secret_storage::curve_support;
use crate::secret_storage::hd_derivation;
use crate::secret_storage::{
    AlgorithmTag, DeriveKeyFromSeedInput, GenerateKeyInput, ImportKeyInput, PublicJwk,
    SecretKeyRef, SecretStorage, SecretStoreError, SignOutput, StoredKeyMeta, VerifyInput,
    types::SignatureFormat,
};

/// HashMap-backed [`SecretStorage`]. Each entry holds the
/// caller-supplied metadata together with the curve-specific
/// scalar record produced by [`curve_support`].
#[derive(Debug, Default)]
pub struct InMemorySecretStore {
    inner: Mutex<HashMap<String, Entry>>,
}

#[derive(Debug, Clone)]
struct Entry {
    meta: StoredKeyMeta,
    record: curve_support::StoredPrivateRecord,
    public_jwk: PublicJwk,
}

impl InMemorySecretStore {
    /// Build an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Re-derivation retry budget. Mirrors the cap used by the file and
/// redb backends so curve rejection probabilities are uniform.
const MAX_DERIVE_CANDIDATES: u32 = 512;

#[async_trait]
impl SecretStorage for InMemorySecretStore {
    async fn initialize(
        &mut self,
        _location: &std::path::Path,
        _passphrase: Option<&str>,
    ) -> Result<(), SecretStoreError> {
        // Pure in-memory; nothing to open or migrate.
        Ok(())
    }

    async fn list_keys(
        &self,
        did_filter: Option<&str>,
    ) -> Result<Vec<StoredKeyMeta>, SecretStoreError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
        let mut out: Vec<StoredKeyMeta> = guard
            .values()
            .filter(|e| {
                did_filter
                    .map(|d| e.meta.did.as_deref() == Some(d))
                    .unwrap_or(true)
            })
            .map(|e| e.meta.clone())
            .collect();
        out.sort_by(|a, b| a.key_ref.uuid().cmp(b.key_ref.uuid()));
        Ok(out)
    }

    async fn generate_key(
        &mut self,
        params: GenerateKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        use rand::SeedableRng;
        let mut rng = rand_chacha::ChaCha20Rng::from_entropy();
        let (record, public_jwk) =
            curve_support::generate(params.kty, params.crv, &mut rng)?;
        let key_ref = self.insert_entry(
            &params.id,
            params.did,
            params.purpose,
            record,
            public_jwk.clone(),
        )?;
        Ok((key_ref, public_jwk))
    }

    async fn import_key(
        &mut self,
        params: ImportKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        let (record, public_jwk) = curve_support::from_private_bytes(
            params.kty,
            params.crv,
            &params.private_key,
        )?;
        let key_ref = self.insert_entry(
            &params.id,
            params.did,
            params.purpose,
            record,
            public_jwk.clone(),
        )?;
        Ok((key_ref, public_jwk))
    }

    async fn derive_key_from_seed(
        &mut self,
        params: DeriveKeyFromSeedInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        let mut last_err: Option<SecretStoreError> = None;
        for candidate in 0..MAX_DERIVE_CANDIDATES {
            let derived = hd_derivation::derive_curve_private_from_seed(&params, candidate)?;
            match curve_support::from_private_bytes(
                derived.kty,
                derived.crv,
                &derived.private_bytes,
            ) {
                Ok((record, public_jwk)) => {
                    let key_ref = self.insert_entry(
                        &params.id,
                        params.did.clone(),
                        params.purpose.clone(),
                        record,
                        public_jwk.clone(),
                    )?;
                    return Ok((key_ref, public_jwk));
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            SecretStoreError::Crypto(
                "deriveKeyFromSeed: exhausted retry candidates".into(),
            )
        }))
    }

    async fn get_public_key(&self, key_ref: &str) -> Result<PublicJwk, SecretStoreError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
        guard
            .get(key_ref)
            .map(|e| e.public_jwk.clone())
            .ok_or_else(|| SecretStoreError::NotFound(key_ref.to_string()))
    }

    async fn sign(
        &self,
        key_ref: &str,
        payload: &[u8],
    ) -> Result<SignOutput, SecretStoreError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
        let entry = guard
            .get(key_ref)
            .ok_or_else(|| SecretStoreError::NotFound(key_ref.to_string()))?;
        let signature = curve_support::sign(&entry.record, payload)?;
        Ok(SignOutput {
            signature,
            format: SignatureFormat::Raw,
        })
    }

    async fn verify(&self, input: VerifyInput) -> Result<bool, SecretStoreError> {
        let pk = if let Some(pk) = input.public_jwk {
            pk
        } else if let Some(kr) = input.key_ref {
            self.get_public_key(kr.uuid()).await?
        } else {
            return Err(SecretStoreError::InvalidInput(
                "verify: must supply either key_ref or public_jwk".into(),
            ));
        };
        curve_support::verify(&pk, &input.payload, &input.signature)
    }

    async fn delete_key(&mut self, key_ref: &str) -> Result<(), SecretStoreError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
        if guard.remove(key_ref).is_none() {
            return Err(SecretStoreError::NotFound(key_ref.to_string()));
        }
        Ok(())
    }
}

impl InMemorySecretStore {
    /// Stash a record + meta under a fresh UUID. The kid on the
    /// returned `SecretKeyRef` is the caller-supplied `id`,
    /// matching the convention used by the file and redb backends.
    fn insert_entry(
        &self,
        id: &str,
        did: Option<String>,
        purpose: Option<String>,
        record: curve_support::StoredPrivateRecord,
        public_jwk: PublicJwk,
    ) -> Result<SecretKeyRef, SecretStoreError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
        let uuid = Uuid::new_v4().to_string();
        let key_ref = SecretKeyRef::new(&uuid, id);
        let ts = String::new();
        let meta = StoredKeyMeta {
            id: id.to_string(),
            key_ref: key_ref.clone(),
            did,
            purpose,
            created_at: ts.clone(),
            updated_at: ts,
            algorithm: AlgorithmTag {
                kty: record.kty,
                crv: record.crv,
            },
        };
        let entry = Entry {
            meta,
            record,
            public_jwk,
        };
        guard.insert(uuid, entry);
        Ok(key_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_storage::{MidnightCurve, MidnightKeyType};

    #[tokio::test]
    async fn import_ed25519_then_lookup_by_kid() {
        let mut s = InMemorySecretStore::default();
        let secret = [1u8; 32];
        let key_ref = s
            .import_ed25519(&secret, "ed25519/authentication")
            .await
            .unwrap();
        assert_eq!(key_ref.id(), "ed25519/authentication");
        assert!(!key_ref.uuid().is_empty());
    }

    #[tokio::test]
    async fn import_jubjub_uses_different_curve_tag() {
        let mut s = InMemorySecretStore::default();
        let key_ref = s
            .import_jubjub(&[2u8; 32], "jubjub/assertionMethod")
            .await
            .unwrap();
        assert_eq!(key_ref.id(), "jubjub/assertionMethod");
    }

    #[tokio::test]
    async fn ed25519_and_jubjub_dont_collide() {
        let mut s = InMemorySecretStore::default();
        let ed = s.import_ed25519(&[3u8; 32], "k1").await.unwrap();
        let jb = s.import_jubjub(&[3u8; 32], "k1").await.unwrap();
        // Same kid, different curves → different uuids.
        assert_ne!(ed.uuid(), jb.uuid());
        // But both `.id()` report the same kid — that's by design.
        assert_eq!(ed.id(), jb.id());
        assert_eq!(ed.id(), "k1");
    }

    #[tokio::test]
    async fn import_then_sign_then_verify() {
        let mut s = InMemorySecretStore::default();
        // Use a real Ed25519 32-byte seed.
        let secret = [4u8; 32];
        let key_ref = s.import_ed25519(&secret, "ed25519/test").await.unwrap();
        let payload = b"hello, in-memory";
        let sig = s.sign(key_ref.uuid(), payload).await.unwrap();
        let ok = s
            .verify(VerifyInput {
                key_ref: Some(key_ref.clone()),
                public_jwk: None,
                payload: payload.to_vec(),
                signature: sig.signature,
            })
            .await
            .unwrap();
        assert!(ok);
    }

    #[tokio::test]
    async fn list_keys_returns_imported_entries() {
        let mut s = InMemorySecretStore::default();
        let _ed = s.import_ed25519(&[5u8; 32], "kid-a").await.unwrap();
        let _jb = s.import_jubjub(&[6u8; 32], "kid-b").await.unwrap();
        let listed = s.list_keys(None).await.unwrap();
        assert_eq!(listed.len(), 2);
        let kids: std::collections::HashSet<_> =
            listed.iter().map(|m| m.id.clone()).collect();
        assert!(kids.contains("kid-a"));
        assert!(kids.contains("kid-b"));
        // Algorithm tags reflect the curve used at import time.
        let by_id: std::collections::HashMap<_, _> =
            listed.iter().map(|m| (m.id.clone(), m.algorithm)).collect();
        assert_eq!(by_id["kid-a"].crv, MidnightCurve::Ed25519);
        assert_eq!(by_id["kid-a"].kty, MidnightKeyType::OKP);
        assert_eq!(by_id["kid-b"].crv, MidnightCurve::Jubjub);
        assert_eq!(by_id["kid-b"].kty, MidnightKeyType::EC);
    }

    #[tokio::test]
    async fn delete_key_then_lookup_fails() {
        let mut s = InMemorySecretStore::default();
        let kref = s.import_ed25519(&[7u8; 32], "doomed").await.unwrap();
        s.delete_key(kref.uuid()).await.unwrap();
        let err = s.get_public_key(kref.uuid()).await.unwrap_err();
        assert!(matches!(err, SecretStoreError::NotFound(_)));
    }
}
