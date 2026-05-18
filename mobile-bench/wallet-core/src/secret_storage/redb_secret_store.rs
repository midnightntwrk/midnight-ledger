//! [`SecretStorage`] backend that lives in the unified
//! [`crate::store::WalletStore`].
//!
//! Differences from [`FileSecretStore`]:
//! - **No raw scalars on disk for HD-derived keys.** Rows
//!   carry `(account, index, candidate)`; the secret is re-
//!   derived from the parent wallet's seed at sign time. A
//!   leaked DB file without the seed is opaque
//!   (`KeyDerivation::Hkdf`) — the upstream "one seed unlocks
//!   everything" invariant the user asked for.
//! - **Imported keys live as `KeyDerivation::Direct`** with the
//!   raw 32-byte scalar wrapped under the store passphrase.
//!   These are the only rows in the keys table that hold a
//!   secret directly.
//! - **Shared envelope** — every wrapped value reuses
//!   `secret_storage::crypto::{encrypt_json, decrypt_json}`,
//!   the same primitives the FileSecretStore writes, so the
//!   wire-level cryptography is unchanged.
//!
//! Construction: `RedbSecretStore::new(store, wallet_id)` ties
//! the secret store to a specific wallet row in the unified
//! file. Multiple `RedbSecretStore` instances against the same
//! file (one per wallet) are fine — redb is `Send + Sync` and
//! the row keys are partitioned by `wallet_id`.

use async_trait::async_trait;
use uuid::Uuid;

use crate::secret_storage::{
    AlgorithmTag, DeriveKeyFromSeedInput, GenerateKeyInput, ImportKeyInput, PublicJwk,
    SecretKeyRef, SecretStorage, SecretStoreError, SignOutput, StoredKeyMeta, VerifyInput,
    curve_support, hd_derivation, types::SignatureFormat,
};
use crate::store::{KeyDerivation, KeyRow, StoreError, WalletId, WalletStore};

/// `SecretStorage` impl backed by the unified `WalletStore`.
pub struct RedbSecretStore {
    store: WalletStore,
    wallet_id: WalletId,
}

impl RedbSecretStore {
    /// Bind a secret store to one wallet in the unified file.
    /// Cheap clone of the `WalletStore` arc; safe to construct
    /// per-flow if you have the wallet id in scope.
    pub fn new(store: WalletStore, wallet_id: WalletId) -> Self {
        Self { store, wallet_id }
    }

    /// Borrow the underlying store handle. Lets the surrounding
    /// app reach the controller-secrets / DID-inventory tables
    /// without juggling two handles.
    pub fn store(&self) -> &WalletStore {
        &self.store
    }

    /// Wallet id the secrets store is bound to.
    pub fn wallet_id(&self) -> WalletId {
        self.wallet_id
    }

    /// Convenience used by the upcoming UI: build a fresh
    /// `RedbSecretStore` from an existing store + the
    /// least-recently-created wallet row in it. Returns
    /// `None` if no wallets exist yet — caller would normally
    /// create one before binding the secret store.
    pub fn for_default_wallet(_store: &WalletStore) -> Option<WalletId> {
        // Reserved for a follow-up that adds a `wallets_by_network`
        // multimap; today the dioxus-wallet App only ever owns
        // one wallet so we hold off on the index until slice 3.
        None
    }
}

fn now_rfc3339() -> String {
    // Match the upstream's `new Date().toISOString()` format,
    // accurate to milliseconds. Avoid pulling in chrono — the
    // upstream parser tolerates trailing-Z UTC.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let ms = now.subsec_millis();
    let (year, month, day, hour, minute, second) = epoch_to_civil(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z")
}

/// Howard Hinnant's days-from-civil algorithm, inverted. Avoids
/// a chrono dependency for the one place we render timestamps.
fn epoch_to_civil(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400) as u32;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + (era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    (year, m, d, hour, minute, second)
}

fn meta_from_row(key_ref: &SecretKeyRef, row: &KeyRow) -> StoredKeyMeta {
    StoredKeyMeta {
        id: row.label.clone(),
        key_ref: key_ref.clone(),
        did: row.did.clone(),
        purpose: row.purpose.clone(),
        created_at: now_rfc3339_from_ms(row.created_at),
        updated_at: now_rfc3339_from_ms(row.updated_at),
        algorithm: row.algorithm,
    }
}

fn now_rfc3339_from_ms(ms: i64) -> String {
    if ms == 0 {
        return now_rfc3339();
    }
    let secs = ms.div_euclid(1000);
    let sub_ms = ms.rem_euclid(1000) as u32;
    let (year, month, day, hour, minute, second) = epoch_to_civil(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{sub_ms:03}Z")
}

#[async_trait]
impl SecretStorage for RedbSecretStore {
    async fn initialize(
        &mut self,
        _location: &std::path::Path,
        _passphrase: Option<&str>,
    ) -> Result<(), SecretStoreError> {
        // No-op for the redb backend — the underlying
        // `WalletStore` was already opened (and possibly
        // migrated) when the caller built it. Kept on the
        // trait for parity with the file backend; FileSecretStore
        // uses it to lazily build the encrypted-JSON file.
        Ok(())
    }

    async fn list_keys(
        &self,
        did_filter: Option<&str>,
    ) -> Result<Vec<StoredKeyMeta>, SecretStoreError> {
        let rows = self
            .store
            .list_keys(self.wallet_id, did_filter)
            .map_err(map_store_err)?;
        Ok(rows
            .into_iter()
            .map(|(k, row)| meta_from_row(&k, &row))
            .collect())
    }

    async fn generate_key(
        &mut self,
        params: GenerateKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        // Pick the next free HKDF index for this wallet by
        // walking the wallet's existing keys. Linear scan is
        // fine at prototype key counts (< ~100); a future
        // slice can add a `next_key_index` meta row to make
        // this O(1).
        let next_index = self.next_hkdf_index().map_err(map_store_err)?;
        let seed = self.store.wallet_seed(self.wallet_id).map_err(map_store_err)?;
        let seed_hex = hex::encode(&*seed);
        // Re-use the upstream derive path so a key generated
        // via `RedbSecretStore` is bit-for-bit identical to
        // one the JS lib would produce for the same seed +
        // index.
        let derive_params = DeriveKeyFromSeedInput {
            id: params.id.clone(),
            seed_hex,
            kty: params.kty,
            crv: params.crv,
            account: Some(0),
            index: Some(next_index),
            did: params.did.clone(),
            purpose: params.purpose.clone(),
        };
        let (private_bytes, candidate) = derive_with_retry(&derive_params)?;
        let (_record, public_jwk) = curve_support::from_private_bytes(
            params.kty,
            params.crv,
            &private_bytes,
        )?;
        let key_ref = Uuid::new_v4().to_string();
        let row = KeyRow {
            label: params.id,
            did: params.did,
            purpose: params.purpose,
            kty: params.kty,
            crv: params.crv,
            public_jwk: public_jwk.clone(),
            algorithm: AlgorithmTag {
                kty: params.kty,
                crv: params.crv,
            },
            derivation: KeyDerivation::Hkdf {
                account: 0,
                index: next_index,
                candidate,
            },
            created_at: 0,
            updated_at: 0,
        };
        self.store
            .put_key(self.wallet_id, &key_ref, row)
            .map_err(map_store_err)?;
        Ok((key_ref, public_jwk))
    }

    async fn import_key(
        &mut self,
        params: ImportKeyInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        // Imported keys have no HD path → store as Direct.
        // The wrapping uses the store's passphrase; the
        // private bytes never touch the file unencrypted.
        let (record, public_jwk) = curve_support::from_private_bytes(
            params.kty,
            params.crv,
            &params.private_key,
        )?;
        let envelope = crate::store::wrap_secret(
            self.store.passphrase(),
            &record.private_bytes,
        )
        .map_err(map_store_err)?;
        let key_ref = Uuid::new_v4().to_string();
        let row = KeyRow {
            label: params.id,
            did: params.did,
            purpose: params.purpose,
            kty: params.kty,
            crv: params.crv,
            public_jwk: public_jwk.clone(),
            algorithm: AlgorithmTag {
                kty: params.kty,
                crv: params.crv,
            },
            derivation: KeyDerivation::Direct { envelope },
            created_at: 0,
            updated_at: 0,
        };
        self.store
            .put_key(self.wallet_id, &key_ref, row)
            .map_err(map_store_err)?;
        Ok((key_ref, public_jwk))
    }

    async fn derive_key_from_seed(
        &mut self,
        params: DeriveKeyFromSeedInput,
    ) -> Result<(SecretKeyRef, PublicJwk), SecretStoreError> {
        let account = params.account.unwrap_or(0);
        let index = params.index.unwrap_or(0);
        let (private_bytes, candidate) = derive_with_retry(&params)?;
        let (_record, public_jwk) = curve_support::from_private_bytes(
            params.kty,
            params.crv,
            &private_bytes,
        )?;
        let key_ref = Uuid::new_v4().to_string();
        // If the seed-hex matches our wallet's seed, store as
        // Hkdf (re-derivable). Otherwise the caller is
        // deriving from an external seed and we have to wrap
        // the result.
        let derivation = if seed_matches_wallet(&self.store, self.wallet_id, &params.seed_hex)
            .map_err(map_store_err)?
        {
            KeyDerivation::Hkdf {
                account,
                index,
                candidate,
            }
        } else {
            let env = crate::store::wrap_secret(
                self.store.passphrase(),
                &private_bytes,
            )
            .map_err(map_store_err)?;
            KeyDerivation::Direct { envelope: env }
        };
        let row = KeyRow {
            label: params.id,
            did: params.did,
            purpose: params.purpose,
            kty: params.kty,
            crv: params.crv,
            public_jwk: public_jwk.clone(),
            algorithm: AlgorithmTag {
                kty: params.kty,
                crv: params.crv,
            },
            derivation,
            created_at: 0,
            updated_at: 0,
        };
        self.store
            .put_key(self.wallet_id, &key_ref, row)
            .map_err(map_store_err)?;
        Ok((key_ref, public_jwk))
    }

    async fn get_public_key(&self, key_ref: &str) -> Result<PublicJwk, SecretStoreError> {
        let row = self
            .store
            .get_key(self.wallet_id, key_ref)
            .map_err(map_store_err)?
            .ok_or_else(|| SecretStoreError::NotFound(key_ref.to_string()))?;
        Ok(row.public_jwk)
    }

    async fn sign(&self, key_ref: &str, payload: &[u8]) -> Result<SignOutput, SecretStoreError> {
        let row = self
            .store
            .get_key(self.wallet_id, key_ref)
            .map_err(map_store_err)?
            .ok_or_else(|| SecretStoreError::NotFound(key_ref.to_string()))?;
        let secret = self
            .store
            .key_private_bytes(self.wallet_id, key_ref)
            .map_err(map_store_err)?;
        let record = curve_support::StoredPrivateRecord {
            kty: row.kty,
            crv: row.crv,
            private_bytes: secret.to_vec(),
        };
        let signature = curve_support::sign(&record, payload)?;
        Ok(SignOutput {
            signature,
            format: SignatureFormat::Raw,
        })
    }

    async fn verify(&self, input: VerifyInput) -> Result<bool, SecretStoreError> {
        let pk = if let Some(ref pk) = input.public_jwk {
            pk.clone()
        } else if let Some(ref kr) = input.key_ref {
            self.get_public_key(kr).await?
        } else {
            return Err(SecretStoreError::InvalidInput(
                "verify needs either key_ref or public_jwk".into(),
            ));
        };
        curve_support::verify(&pk, &input.payload, &input.signature)
    }

    async fn delete_key(&mut self, key_ref: &str) -> Result<(), SecretStoreError> {
        self.store
            .delete_key(self.wallet_id, key_ref)
            .map_err(map_store_err)
    }
}

impl RedbSecretStore {
    /// Walk every key row for the wallet and return the
    /// smallest unused HKDF index (account 0). Linear scan,
    /// fine at prototype scale.
    fn next_hkdf_index(&self) -> Result<u32, StoreError> {
        let rows = self.store.list_keys(self.wallet_id, None)?;
        let mut used: Vec<u32> = rows
            .iter()
            .filter_map(|(_, row)| match row.derivation {
                KeyDerivation::Hkdf {
                    account: 0, index, ..
                } => Some(index),
                _ => None,
            })
            .collect();
        used.sort_unstable();
        used.dedup();
        // Smallest non-negative integer not in `used`.
        for (i, slot) in used.iter().enumerate() {
            if *slot != i as u32 {
                return Ok(i as u32);
            }
        }
        Ok(used.len() as u32)
    }
}

/// Re-derive with the upstream's retry loop. If a curve
/// rejects the derived scalar (e.g. P-256 zero scalar — vanishingly
/// improbable but standards-required), bump `candidate` and
/// re-derive. Returns `(private_bytes, candidate_used)`.
fn derive_with_retry(
    params: &DeriveKeyFromSeedInput,
) -> Result<(Vec<u8>, u32), SecretStoreError> {
    const MAX_CANDIDATES: u32 = 512;
    for candidate in 0..MAX_CANDIDATES {
        let derived = hd_derivation::derive_curve_private_from_seed(params, candidate)?;
        // Re-attempt the curve normalisation. A future failure
        // mode is "curve_support rejects the scalar" — we
        // catch via the same `from_private_bytes` it uses.
        if curve_support::from_private_bytes(params.kty, params.crv, &derived.private_bytes)
            .is_ok()
        {
            return Ok((derived.private_bytes, candidate));
        }
    }
    Err(SecretStoreError::Crypto(format!(
        "exhausted {MAX_CANDIDATES} HKDF candidates for {:?}/{:?}",
        params.kty, params.crv,
    )))
}

/// Compare a caller-supplied seed hex to the wallet's stored
/// seed. Equal means we can store as `Hkdf` (re-derive on
/// every sign); unequal means the secret has to be wrapped.
fn seed_matches_wallet(
    store: &WalletStore,
    wallet_id: WalletId,
    other_hex: &str,
) -> Result<bool, StoreError> {
    let seed = store.wallet_seed(wallet_id)?;
    let my_hex = hex::encode(&*seed);
    let theirs = other_hex.trim();
    Ok(my_hex.eq_ignore_ascii_case(theirs))
}

fn map_store_err(e: StoreError) -> SecretStoreError {
    match e {
        StoreError::Crypto(m) => SecretStoreError::Crypto(m),
        StoreError::Corruption(m) | StoreError::Codec(m) => SecretStoreError::Crypto(m),
        StoreError::Backend(m) | StoreError::Migration(m) => SecretStoreError::Crypto(m),
        StoreError::NotFound(t) => SecretStoreError::NotFound(t.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Network;
    use crate::secret_storage::{MidnightCurve, MidnightKeyType};

    fn fresh_store_with_wallet() -> (WalletStore, WalletId) {
        let store = WalletStore::open_in_memory("pw").expect("open in-memory store");
        let mut seed = [0u8; 32];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = i as u8;
        }
        let id = store
            .create_wallet("demo", Network::Undeployed, &seed)
            .expect("create wallet");
        (store, id)
    }

    #[tokio::test]
    async fn generate_ed25519_round_trip() {
        let (store, id) = fresh_store_with_wallet();
        let mut s = RedbSecretStore::new(store, id);
        let (kref, pk) = s
            .generate_key(GenerateKeyInput {
                id: "k1".into(),
                kty: MidnightKeyType::OKP,
                crv: MidnightCurve::Ed25519,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        assert_eq!(pk.kty, MidnightKeyType::OKP);
        assert_eq!(pk.crv, MidnightCurve::Ed25519);
        let sig = s.sign(&kref, b"hello").await.unwrap();
        let ok = s
            .verify(VerifyInput {
                key_ref: Some(kref.clone()),
                public_jwk: None,
                payload: b"hello".to_vec(),
                signature: sig.signature.clone(),
            })
            .await
            .unwrap();
        assert!(ok);
    }

    #[tokio::test]
    async fn generate_p256_round_trip() {
        let (store, id) = fresh_store_with_wallet();
        let mut s = RedbSecretStore::new(store, id);
        let (kref, _) = s
            .generate_key(GenerateKeyInput {
                id: "k1".into(),
                kty: MidnightKeyType::EC,
                crv: MidnightCurve::P256,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        let sig = s.sign(&kref, b"hello").await.unwrap();
        assert!(s
            .verify(VerifyInput {
                key_ref: Some(kref),
                public_jwk: None,
                payload: b"hello".to_vec(),
                signature: sig.signature,
            })
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn generate_jubjub_round_trip() {
        let (store, id) = fresh_store_with_wallet();
        let mut s = RedbSecretStore::new(store, id);
        let (kref, _) = s
            .generate_key(GenerateKeyInput {
                id: "k1".into(),
                kty: MidnightKeyType::EC,
                crv: MidnightCurve::Jubjub,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        let sig = s.sign(&kref, b"hello").await.unwrap();
        assert!(s
            .verify(VerifyInput {
                key_ref: Some(kref),
                public_jwk: None,
                payload: b"hello".to_vec(),
                signature: sig.signature,
            })
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn generated_keys_use_distinct_indices() {
        let (store, id) = fresh_store_with_wallet();
        let mut s = RedbSecretStore::new(store, id);
        let (a, _) = s
            .generate_key(GenerateKeyInput {
                id: "a".into(),
                kty: MidnightKeyType::OKP,
                crv: MidnightCurve::Ed25519,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        let (b, _) = s
            .generate_key(GenerateKeyInput {
                id: "b".into(),
                kty: MidnightKeyType::OKP,
                crv: MidnightCurve::Ed25519,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        let row_a = s.store.get_key(s.wallet_id, &a).unwrap().unwrap();
        let row_b = s.store.get_key(s.wallet_id, &b).unwrap().unwrap();
        match (row_a.derivation, row_b.derivation) {
            (
                KeyDerivation::Hkdf { index: ai, .. },
                KeyDerivation::Hkdf { index: bi, .. },
            ) => {
                assert_ne!(ai, bi, "indices must differ");
            }
            _ => panic!("expected both rows to be Hkdf"),
        }
    }

    #[tokio::test]
    async fn import_then_sign_then_delete() {
        let (store, id) = fresh_store_with_wallet();
        let mut s = RedbSecretStore::new(store, id);
        // Ed25519 32-byte raw seed.
        let raw = [42u8; 32];
        let (kref, _) = s
            .import_key(ImportKeyInput {
                id: "imp".into(),
                private_key: raw.to_vec(),
                kty: MidnightKeyType::OKP,
                crv: MidnightCurve::Ed25519,
                did: None,
                purpose: None,
            })
            .await
            .unwrap();
        // Sign with the imported key.
        let sig = s.sign(&kref, b"msg").await.unwrap();
        assert!(s
            .verify(VerifyInput {
                key_ref: Some(kref.clone()),
                public_jwk: None,
                payload: b"msg".to_vec(),
                signature: sig.signature,
            })
            .await
            .unwrap());
        // Imported key MUST land as Direct (no HD path).
        let row = s.store.get_key(s.wallet_id, &kref).unwrap().unwrap();
        assert!(matches!(row.derivation, KeyDerivation::Direct { .. }));
        // Delete.
        s.delete_key(&kref).await.unwrap();
        assert!(s.store.get_key(s.wallet_id, &kref).unwrap().is_none());
    }

    #[tokio::test]
    async fn list_keys_filters_by_did() {
        let (store, id) = fresh_store_with_wallet();
        let mut s = RedbSecretStore::new(store, id);
        s.generate_key(GenerateKeyInput {
            id: "no-did".into(),
            kty: MidnightKeyType::OKP,
            crv: MidnightCurve::Ed25519,
            did: None,
            purpose: None,
        })
        .await
        .unwrap();
        s.generate_key(GenerateKeyInput {
            id: "did-A".into(),
            kty: MidnightKeyType::OKP,
            crv: MidnightCurve::Ed25519,
            did: Some("did:midnight:undeployed:aaa".into()),
            purpose: None,
        })
        .await
        .unwrap();
        s.generate_key(GenerateKeyInput {
            id: "did-B".into(),
            kty: MidnightKeyType::OKP,
            crv: MidnightCurve::Ed25519,
            did: Some("did:midnight:undeployed:bbb".into()),
            purpose: None,
        })
        .await
        .unwrap();
        let all = s.list_keys(None).await.unwrap();
        assert_eq!(all.len(), 3);
        let only_a = s
            .list_keys(Some("did:midnight:undeployed:aaa"))
            .await
            .unwrap();
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].id, "did-A");
    }

    #[tokio::test]
    async fn hkdf_keys_re_derive_after_reopen() {
        // Sign with a key, drop the store, reopen, sign the
        // same payload → identical signature for Ed25519
        // (deterministic) and a valid signature for the
        // others. Demonstrates the "no scalar persisted"
        // invariant works end-to-end.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wallet.redb");
        let mut seed = [0u8; 32];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = i as u8;
        }
        let (kref, sig_a) = {
            let store = WalletStore::open(&path, "pw").unwrap();
            let id = store
                .create_wallet("demo", Network::Undeployed, &seed)
                .unwrap();
            let mut s = RedbSecretStore::new(store, id);
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
            let sig = s.sign(&kref, b"determinism").await.unwrap().signature;
            (kref, sig)
        };
        // Reopen.
        let store = WalletStore::open(&path, "pw").unwrap();
        // Look up the wallet by listing wallets.
        // Today the App keeps the WalletId, so this lookup is
        // ad-hoc — list the wallet rows; for the test pull
        // the first one.
        let wallets = list_wallets(&store);
        let id = wallets[0];
        let s = RedbSecretStore::new(store, id);
        let sig_b = s.sign(&kref, b"determinism").await.unwrap().signature;
        assert_eq!(sig_a, sig_b, "Ed25519 determinism: same key, same payload → same sig");
    }

    /// Test helper — public via `WalletStore::list_wallet_ids`
    /// in production code; here we just call through.
    fn list_wallets(store: &WalletStore) -> Vec<WalletId> {
        store.list_wallet_ids().expect("list_wallet_ids")
    }
}
