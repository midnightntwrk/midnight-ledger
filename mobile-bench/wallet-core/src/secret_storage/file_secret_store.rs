//! Encrypted-JSON-file backed [`SecretStorage`] implementation.
//! Mirrors `secret-storage/src/file-secret-store.ts`.
//!
//! On-disk layout:
//! ```json
//! {
//!   "version": 1,
//!   "encrypted": { "salt": "…", "iv": "…", "tag": "…", "ciphertext": "…" }
//! }
//! ```
//! The encrypted blob decodes to:
//! ```json
//! {
//!   "version": 1,
//!   "keys": { "<uuid>": { "meta": {...}, "privateRecord": {...}, "publicJwk": {...} } }
//! }
//! ```
//!
//! Not wire-compatible with the JS file format (different
//! `privateRecord` encoding — we store raw curve scalars, the
//! upstream stores PKCS8-DER for Ed25519/P-256 and raw32 for
//! Jubjub). The pieces necessary for cryptographic compatibility
//! at the *key* level (AES-256-GCM + scrypt + curve scalars) are
//! all standards-defined; only the on-disk envelope differs.

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

use crate::secret_storage::crypto::{EncryptedPayload, decrypt_json, encrypt_json};
use crate::secret_storage::curve_support;
use crate::secret_storage::hd_derivation;
use crate::secret_storage::{
    AlgorithmTag, DeriveKeyFromSeedInput, GenerateKeyInput, ImportKeyInput, MidnightCurve,
    MidnightKeyType, PublicJwk, SecretKeyRef, SecretStorage, SecretStoreError, SignOutput,
    StoredKeyMeta, VerifyInput, types::SignatureFormat,
};

/// Internal on-disk record for one key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEntry {
    meta: StoredKeyMeta,
    private_record: StoredPrivateRecordOnDisk,
    public_jwk: PublicJwk,
}

/// `curve_support::StoredPrivateRecord` carried across the wire
/// with bytes base64-encoded so the encrypted blob is pure JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPrivateRecordOnDisk {
    kty: MidnightKeyType,
    crv: MidnightCurve,
    /// base64(raw scalar bytes). Curve-specific length:
    /// Ed25519 / P-256 / Jubjub all use 32 bytes.
    private_key: String,
}

impl StoredPrivateRecordOnDisk {
    fn from_record(rec: &curve_support::StoredPrivateRecord) -> Self {
        Self {
            kty: rec.kty,
            crv: rec.crv,
            private_key: B64.encode(&rec.private_bytes),
        }
    }

    fn to_record(&self) -> Result<curve_support::StoredPrivateRecord, SecretStoreError> {
        let bytes = B64
            .decode(self.private_key.as_bytes())
            .map_err(|e| SecretStoreError::InvalidInput(format!("private_key b64: {e}")))?;
        Ok(curve_support::StoredPrivateRecord {
            kty: self.kty,
            crv: self.crv,
            private_bytes: bytes,
        })
    }
}

/// Decrypted store contents — `{ version, keys }`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    keys: HashMap<String, StoredEntry>,
}

/// On-disk envelope — `{ version, encrypted }`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct FileEnvelope {
    version: u32,
    encrypted: EncryptedPayload,
}

/// File-backed secret store. Single-process; uses an in-memory
/// `Mutex` to serialise reads + writes against the loaded
/// `StoreFile`. Maximum number of HKDF retries when a derived
/// scalar fails curve-specific validity (negligible probability;
/// matches the upstream's 512-attempt cap).
const MAX_DERIVE_CANDIDATES: u32 = 512;

#[derive(Debug)]
pub struct FileSecretStore {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    location: PathBuf,
    passphrase: String,
    store: StoreFile,
}

impl FileSecretStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                location: PathBuf::new(),
                passphrase: String::new(),
                store: StoreFile { version: 1, keys: HashMap::new() },
            }),
        }
    }

    /// Helper: encrypt + persist the in-memory store to disk.
    fn persist(inner: &mut Inner) -> Result<(), SecretStoreError> {
        let raw = serde_json::to_vec(&inner.store)?;
        let mut rng = rand_chacha::ChaCha20Rng::from_entropy();
        let env = encrypt_json(&inner.passphrase, &raw, &mut rng)?;
        let outer = FileEnvelope {
            version: 1,
            encrypted: env,
        };
        let bytes = serde_json::to_vec_pretty(&outer)?;
        if let Some(parent) = inner.location.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&inner.location, bytes)?;
        Ok(())
    }
}

impl Default for FileSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

fn now_iso() -> String {
    // RFC 3339 with seconds precision — no chrono dep needed; pull
    // the integer ms since epoch and format manually. Good enough
    // for the metadata timestamp which is informational only.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_s = now.as_secs() as i64;
    let days = total_s.div_euclid(86_400);
    let rem = total_s.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let min = ((rem % 3600) / 60) as u32;
    let sec = (rem % 60) as u32;
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Days since 1970-01-01 → (year, month, day). Civil-from-days
/// (Hinnant). Inlined to avoid a chrono dependency for one
/// timestamp.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d)
}

#[async_trait]
impl SecretStorage for FileSecretStore {
    async fn initialize(
        &mut self,
        location: &Path,
        passphrase: Option<&str>,
    ) -> Result<(), SecretStoreError> {
        let pass = passphrase.ok_or(SecretStoreError::Locked)?;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
        guard.location = location.to_path_buf();
        guard.passphrase = pass.to_string();
        match std::fs::read(location) {
            Ok(bytes) => {
                let env: FileEnvelope = serde_json::from_slice(&bytes)?;
                let decrypted = decrypt_json(&guard.passphrase, &env.encrypted)?;
                guard.store = serde_json::from_slice(&decrypted)?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                guard.store = StoreFile {
                    version: 1,
                    keys: HashMap::new(),
                };
                Self::persist(&mut guard)?;
                Ok(())
            }
            Err(e) => Err(SecretStoreError::Init(format!("read store: {e}"))),
        }
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
            .store
            .keys
            .values()
            .filter(|e| {
                did_filter
                    .map(|d| e.meta.did.as_deref() == Some(d))
                    .unwrap_or(true)
            })
            .map(|e| e.meta.clone())
            .collect();
        out.sort_by(|a, b| a.key_ref.cmp(&b.key_ref));
        Ok(out)
    }

    async fn generate_key(
        &mut self,
        params: GenerateKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        let mut rng = rand_chacha::ChaCha20Rng::from_entropy();
        let (record, public_jwk) = curve_support::generate(params.kty, params.crv, &mut rng)?;
        insert_entry(self, &params.id, params.did, params.purpose, record, public_jwk).await
    }

    async fn import_key(
        &mut self,
        params: ImportKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        let (record, public_jwk) =
            curve_support::from_private_bytes(params.kty, params.crv, &params.private_key)?;
        insert_entry(self, &params.id, params.did, params.purpose, record, public_jwk).await
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
                    return insert_entry(
                        self,
                        &params.id,
                        params.did.clone(),
                        params.purpose.clone(),
                        record,
                        public_jwk,
                    )
                    .await;
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
            .store
            .keys
            .get(key_ref)
            .map(|e| e.public_jwk.clone())
            .ok_or_else(|| SecretStoreError::NotFound(key_ref.to_string()))
    }

    async fn sign(&self, key_ref: &str, payload: &[u8]) -> Result<SignOutput, SecretStoreError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
        let entry = guard
            .store
            .keys
            .get(key_ref)
            .ok_or_else(|| SecretStoreError::NotFound(key_ref.to_string()))?;
        let record = entry.private_record.to_record()?;
        let sig = curve_support::sign(&record, payload)?;
        Ok(SignOutput {
            signature: sig,
            format: SignatureFormat::Raw,
        })
    }

    async fn verify(&self, input: VerifyInput) -> Result<bool, SecretStoreError> {
        let pk = if let Some(pk) = input.public_jwk {
            pk
        } else if let Some(key_ref) = input.key_ref {
            self.get_public_key(&key_ref).await?
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
        if guard.store.keys.remove(key_ref).is_none() {
            return Err(SecretStoreError::NotFound(key_ref.to_string()));
        }
        Self::persist(&mut guard)?;
        Ok(())
    }
}

/// Compose a `StoredKeyMeta`, drop the entry into the store map,
/// persist, and return `(key_ref, public_jwk)`. Common tail of
/// `generate_key` / `import_key` / `derive_key_from_seed`.
async fn insert_entry(
    store: &FileSecretStore,
    id: &str,
    did: Option<String>,
    purpose: Option<String>,
    record: curve_support::StoredPrivateRecord,
    public_jwk: PublicJwk,
) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
    let mut guard = store
        .inner
        .lock()
        .map_err(|_| SecretStoreError::Init("mutex poisoned".into()))?;
    let key_ref = Uuid::new_v4().to_string();
    let ts = now_iso();
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
    let entry = StoredEntry {
        meta,
        private_record: StoredPrivateRecordOnDisk::from_record(&record),
        public_jwk: public_jwk.clone(),
    };
    guard.store.keys.insert(key_ref.clone(), entry);
    FileSecretStore::persist(&mut guard)?;
    Ok((key_ref, public_jwk))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_storage::{MidnightCurve, MidnightKeyType};

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let uuid = Uuid::new_v4();
        p.push(format!("wallet-core-test-{uuid}-{name}.json"));
        p
    }

    async fn fresh(passphrase: &str) -> (FileSecretStore, PathBuf) {
        let p = tmp_path("store");
        let mut s = FileSecretStore::new();
        s.initialize(&p, Some(passphrase)).await.unwrap();
        (s, p)
    }

    #[tokio::test]
    async fn initialize_creates_file_on_first_use() {
        let (_s, path) = fresh("pw").await;
        assert!(path.exists(), "store file should exist after initialize");
    }

    #[tokio::test]
    async fn locked_when_no_passphrase() {
        let p = tmp_path("locked");
        let mut s = FileSecretStore::new();
        let err = s.initialize(&p, None).await.unwrap_err();
        assert!(matches!(err, SecretStoreError::Locked));
    }

    #[tokio::test]
    async fn generate_and_list_round_trip() {
        let (mut s, _p) = fresh("pw").await;
        let params = GenerateKeyInput {
            id: "key-0".into(),
            kty: MidnightKeyType::OKP,
            crv: MidnightCurve::Ed25519,
            did: Some("did:midnight:undeployed:abc".into()),
            purpose: Some("authentication".into()),
        };
        let (key_ref, pk) = s.generate_key(params).await.unwrap();
        assert!(!key_ref.is_empty());
        assert_eq!(pk.kty, MidnightKeyType::OKP);

        let listed = s.list_keys(None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key_ref, key_ref);
        assert_eq!(listed[0].id, "key-0");
        assert_eq!(
            listed[0].did.as_deref(),
            Some("did:midnight:undeployed:abc")
        );
    }

    #[tokio::test]
    async fn sign_verify_round_trip() {
        let (mut s, _p) = fresh("pw").await;
        let (key_ref, _pk) = s
            .generate_key(GenerateKeyInput {
                id: "k".into(),
                kty: MidnightKeyType::EC,
                crv: MidnightCurve::P256,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        let payload = b"hello, store";
        let out = s.sign(&key_ref, payload).await.unwrap();
        assert_eq!(out.signature.len(), 64);
        let ok = s
            .verify(VerifyInput {
                key_ref: Some(key_ref),
                public_jwk: None,
                payload: payload.to_vec(),
                signature: out.signature,
            })
            .await
            .unwrap();
        assert!(ok);
    }

    #[tokio::test]
    async fn reopen_with_same_passphrase_reads_existing_keys() {
        let p = tmp_path("reopen");
        // First session: generate two keys.
        let key_refs: Vec<String> = {
            let mut s = FileSecretStore::new();
            s.initialize(&p, Some("pw")).await.unwrap();
            let mut refs = Vec::new();
            for id in ["a", "b"] {
                let (kref, _) = s
                    .generate_key(GenerateKeyInput {
                        id: id.into(),
                        kty: MidnightKeyType::OKP,
                        crv: MidnightCurve::Ed25519,
                        did: None,
                        purpose: None,
                    })
                    .await
                    .unwrap();
                refs.push(kref);
            }
            refs
        };
        // Second session: open the same file, list keys.
        let mut s2 = FileSecretStore::new();
        s2.initialize(&p, Some("pw")).await.unwrap();
        let listed = s2.list_keys(None).await.unwrap();
        assert_eq!(listed.len(), 2);
        let listed_refs: std::collections::HashSet<_> =
            listed.iter().map(|m| m.key_ref.clone()).collect();
        for r in &key_refs {
            assert!(listed_refs.contains(r), "missing key {r}");
        }
    }

    #[tokio::test]
    async fn reopen_with_wrong_passphrase_fails() {
        let p = tmp_path("wrongpw");
        {
            let mut s = FileSecretStore::new();
            s.initialize(&p, Some("right")).await.unwrap();
            s.generate_key(GenerateKeyInput {
                id: "k".into(),
                kty: MidnightKeyType::OKP,
                crv: MidnightCurve::Ed25519,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        }
        let mut s2 = FileSecretStore::new();
        let err = s2.initialize(&p, Some("wrong")).await.unwrap_err();
        // Wrong key → AES tag mismatch → bubbles up as Crypto from
        // `decrypt_json` (which is wrapped here in no specific layer
        // since we propagate directly).
        assert!(matches!(err, SecretStoreError::Crypto(_)));
    }

    #[tokio::test]
    async fn derive_key_from_seed_is_deterministic() {
        let (mut s, _p) = fresh("pw").await;
        let params = DeriveKeyFromSeedInput {
            id: "derived".into(),
            seed_hex: "00".repeat(32),
            kty: MidnightKeyType::OKP,
            crv: MidnightCurve::Ed25519,
            account: Some(0),
            index: Some(0),
            did: None,
            purpose: None,
        };
        let (kref_a, pk_a) = s.derive_key_from_seed(params.clone()).await.unwrap();
        // Same seed → same public key, but a new key_ref (UUID).
        let (kref_b, pk_b) = s.derive_key_from_seed(params).await.unwrap();
        assert_eq!(pk_a, pk_b, "deterministic public key from same seed");
        assert_ne!(kref_a, kref_b, "fresh UUID per insertion");
    }

    #[tokio::test]
    async fn delete_key_removes_it() {
        let (mut s, _p) = fresh("pw").await;
        let (kref, _) = s
            .generate_key(GenerateKeyInput {
                id: "k".into(),
                kty: MidnightKeyType::OKP,
                crv: MidnightCurve::Ed25519,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        s.delete_key(&kref).await.unwrap();
        let err = s.get_public_key(&kref).await.unwrap_err();
        assert!(matches!(err, SecretStoreError::NotFound(_)));
        // Deleting the same key again fails.
        let err2 = s.delete_key(&kref).await.unwrap_err();
        assert!(matches!(err2, SecretStoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_keys_did_filter() {
        let (mut s, _p) = fresh("pw").await;
        let mk = |id: &str, did: Option<&str>| GenerateKeyInput {
            id: id.into(),
            kty: MidnightKeyType::OKP,
            crv: MidnightCurve::Ed25519,
            did: did.map(String::from),
            purpose: None,
        };
        s.generate_key(mk("a", Some("did:a"))).await.unwrap();
        s.generate_key(mk("b", Some("did:b"))).await.unwrap();
        s.generate_key(mk("c", None)).await.unwrap();
        let filtered = s.list_keys(Some("did:a")).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "a");
        let all = s.list_keys(None).await.unwrap();
        assert_eq!(all.len(), 3);
    }
}
