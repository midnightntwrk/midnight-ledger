//! Persistent wallet store, backed by a single `redb` file.
//!
//! Unified storage for everything the wallet needs to survive a
//! reload:
//!
//! - **Wallets** — labelled, network-scoped, with a wrapped
//!   32-byte seed (the root of trust). The seed is encrypted at
//!   row level using the same scrypt + AES-256-GCM envelope the
//!   `FileSecretStore` uses, so a leaked DB file without the
//!   passphrase is just opaque bytes.
//! - **Controller secrets** — per-DID 32-byte randoms minted at
//!   `Wallet::create_did`. Today these live in
//!   `BridgeState.controller_secrets` (in-memory `HashMap`) so
//!   any DID you created is lost on reload — the persistent
//!   table fixes that. Also envelope-wrapped.
//!
//! Future slices add `keys`, `did_inventory`, `resolved_cache`,
//! and `sessions` tables — see `mobile-bench/STORE_PLAN.md`
//! for the full schema.
//!
//! ## Threading model
//! `WalletStore` is `Send + Sync` — `redb::Database` is. Cheap
//! to clone the Arc inside; spawn it into Dioxus task closures
//! freely.
//!
//! ## Migration
//! `Meta::schema_version` records the on-disk schema. `open()`
//! reads it and dispatches into `migrate::run()` if behind the
//! current `SCHEMA_VERSION`. v0 → v1 mints the per-file app
//! salt and writes the version row; no data shape is changed
//! because v0 means "empty file".

mod codec;
mod envelope;
mod error;
mod migrate;
mod schema;

use std::path::Path;
use std::sync::Arc;

use rand::RngCore;
use redb::Database;
use uuid::Uuid;

use crate::Network;

pub use error::StoreError;
pub use schema::{NetworkTag, SCHEMA_VERSION, WalletId};

use codec::Bincoded;
use envelope::{decrypt_secret, encrypt_secret};
use schema::{
    CONTROLLER_SECRETS, META, WALLETS, WalletRowV1,
};

/// Façade over the on-disk redb file. Holds the unlocked
/// passphrase in memory for the lifetime of the store —
/// callers should drop it as soon as they're done.
#[derive(Clone)]
pub struct WalletStore {
    db: Arc<Database>,
    /// Wrap-key passphrase. Kept private and dropped via the
    /// `Zeroizing` wrapper on shutdown. Cheap copy: passphrases
    /// are user-typed, so they fit in a few dozen bytes.
    passphrase: zeroize::Zeroizing<String>,
}

impl std::fmt::Debug for WalletStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalletStore")
            .field("db", &"<redb::Database>")
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

impl WalletStore {
    /// Open (or create) a wallet store at `path`. Runs any
    /// pending migrations under the supplied passphrase. The
    /// passphrase is kept in memory for as long as the
    /// `WalletStore` lives; rotating it requires a future
    /// `rotate_passphrase()` call (not yet implemented).
    pub fn open<P: AsRef<Path>>(path: P, passphrase: &str) -> Result<Self, StoreError> {
        let db = Database::create(path).map_err(|e| StoreError::Backend(e.to_string()))?;
        let store = Self {
            db: Arc::new(db),
            passphrase: zeroize::Zeroizing::new(passphrase.to_string()),
        };
        migrate::run(&store)?;
        Ok(store)
    }

    /// Open an in-memory store — for unit tests and ephemeral
    /// session scratch. The database vanishes when the
    /// `WalletStore` drops. Public so integration tests can
    /// build a scratch store; not exposed via a feature flag
    /// because the cost of always-compiling-in is one struct
    /// alias.
    pub fn open_in_memory(passphrase: &str) -> Result<Self, StoreError> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let store = Self {
            db: Arc::new(db),
            passphrase: zeroize::Zeroizing::new(passphrase.to_string()),
        };
        migrate::run(&store)?;
        Ok(store)
    }

    /// Borrow the underlying database for migrations. Crate-
    /// private — external callers go through typed accessors.
    pub(crate) fn db(&self) -> &Database {
        &self.db
    }

    /// Borrow the passphrase for envelope encryption. Crate-
    /// private; the bytes never leave the module.
    pub(crate) fn passphrase(&self) -> &str {
        &self.passphrase
    }

    // ── Wallets ───────────────────────────────────────────────────

    /// Mint a fresh wallet row. The seed is wrapped under the
    /// store passphrase before it touches disk. `network` is the
    /// natural sharding axis — same seed can produce one wallet
    /// per network if a future workflow wants that.
    pub fn create_wallet(
        &self,
        label: &str,
        network: Network,
        seed: &[u8; 32],
    ) -> Result<WalletId, StoreError> {
        let id = WalletId(*Uuid::new_v4().as_bytes());
        let now = unix_now_ms();
        let seed_envelope =
            encrypt_secret(self.passphrase(), seed)?;
        let row = WalletRowV1 {
            label: label.to_string(),
            network: NetworkTag::from(network),
            address_bech32: String::new(),
            created_at: now,
            updated_at: now,
            seed_envelope,
        };
        let bincoded = Bincoded::encode(&row)?;
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        {
            let mut table = txn
                .open_table(WALLETS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            table
                .insert(id.0, bincoded.as_slice())
                .map_err(|e| StoreError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(id)
    }

    /// Decode a wallet row + decrypt its seed. The returned
    /// seed is wrapped in `Zeroizing` so it scrubs itself when
    /// the caller drops it.
    pub fn wallet_seed(&self, id: WalletId) -> Result<zeroize::Zeroizing<[u8; 32]>, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(WALLETS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let raw = table
            .get(id.0)
            .map_err(|e| StoreError::Backend(e.to_string()))?
            .ok_or(StoreError::NotFound("wallet"))?;
        let row: WalletRowV1 = Bincoded::decode(raw.value())?;
        let bytes = decrypt_secret(self.passphrase(), &row.seed_envelope)?;
        if bytes.len() != 32 {
            return Err(StoreError::Corruption(format!(
                "wallet seed not 32 bytes (got {})",
                bytes.len()
            )));
        }
        let mut out = zeroize::Zeroizing::new([0u8; 32]);
        out.copy_from_slice(&bytes);
        Ok(out)
    }

    // ── Controller secrets ────────────────────────────────────────

    /// Persist a per-DID controller secret. The DID encodes its
    /// network in its bech32 string, so we key on
    /// `(NetworkTag, did)` — same DID on different networks is
    /// a distinct row. Wrapped under the store passphrase.
    pub fn put_controller_secret(
        &self,
        network: Network,
        did: &str,
        secret: &[u8; 32],
    ) -> Result<(), StoreError> {
        let env = encrypt_secret(self.passphrase(), secret)?;
        let bincoded = Bincoded::encode(&env)?;
        let key = (NetworkTag::from(network).0, did);
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        {
            let mut table = txn
                .open_table(CONTROLLER_SECRETS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            table
                .insert(key, bincoded.as_slice())
                .map_err(|e| StoreError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Fetch a single controller secret. Returns `None` if the
    /// DID hasn't been created by this wallet — distinct from
    /// the error case (wrong passphrase, corrupt envelope).
    pub fn get_controller_secret(
        &self,
        network: Network,
        did: &str,
    ) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, StoreError> {
        let key = (NetworkTag::from(network).0, did);
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(CONTROLLER_SECRETS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let Some(row) = table
            .get(key)
            .map_err(|e| StoreError::Backend(e.to_string()))?
        else {
            return Ok(None);
        };
        let env: envelope::SecretEnvelope = Bincoded::decode(row.value())?;
        let bytes = decrypt_secret(self.passphrase(), &env)?;
        if bytes.len() != 32 {
            return Err(StoreError::Corruption(format!(
                "controller secret not 32 bytes (got {})",
                bytes.len()
            )));
        }
        let mut out = zeroize::Zeroizing::new([0u8; 32]);
        out.copy_from_slice(&bytes);
        Ok(Some(out))
    }

    /// Bulk hydrate every controller secret on a given network.
    /// Returns `Vec<(did, sk)>` — convenient shape for seeding
    /// `BridgeState.controller_secrets` at app startup.
    pub fn list_controller_secrets(
        &self,
        network: Network,
    ) -> Result<Vec<(String, zeroize::Zeroizing<[u8; 32]>)>, StoreError> {
        let tag = NetworkTag::from(network).0;
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(CONTROLLER_SECRETS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        let iter = table
            .range((tag, "")..(tag.saturating_add(1), ""))
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        for entry in iter {
            let (k, v) = entry.map_err(|e| StoreError::Backend(e.to_string()))?;
            let (_, did) = k.value();
            let env: envelope::SecretEnvelope = Bincoded::decode(v.value())?;
            let bytes = decrypt_secret(self.passphrase(), &env)?;
            if bytes.len() == 32 {
                let mut sk = zeroize::Zeroizing::new([0u8; 32]);
                sk.copy_from_slice(&bytes);
                out.push((did.to_string(), sk));
            }
        }
        Ok(out)
    }

    /// Inspect the on-disk schema version. Public so callers
    /// can surface "store is up to date" diagnostics without
    /// touching internals.
    pub fn schema_version(&self) -> Result<u32, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(META)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let v = table
            .get(schema::META_SCHEMA_VERSION_KEY)
            .map_err(|e| StoreError::Backend(e.to_string()))?
            .map(|g| {
                let bytes = g.value();
                if bytes.len() == 4 {
                    u32::from_le_bytes(bytes.try_into().unwrap())
                } else {
                    0
                }
            })
            .unwrap_or(0);
        Ok(v)
    }
}

/// Helper around `std::time::SystemTime` — milliseconds since
/// the Unix epoch. Used for `created_at` / `updated_at` on
/// every row so the UI can show "first seen" timestamps and
/// the migration code has a stable "now" reference.
fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Cryptographically-random `[u8; 32]`. Used by callers that
/// mint controller secrets but want to fold the persistence
/// step into the store API. Re-exported here so callers don't
/// need to import `rand` themselves.
pub fn random_secret() -> [u8; 32] {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixed_seed() -> [u8; 32] {
        let mut s = [0u8; 32];
        for (i, b) in s.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(3);
        }
        s
    }

    #[test]
    fn open_in_memory_runs_migration_to_v1() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn create_wallet_then_read_seed_back() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        let seed = fixed_seed();
        let id = store
            .create_wallet("demo", Network::Undeployed, &seed)
            .unwrap();
        let back = store.wallet_seed(id).unwrap();
        assert_eq!(&*back, &seed);
    }

    #[test]
    fn wrong_passphrase_cannot_read_seed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wallet.redb");
        let id = {
            let store = WalletStore::open(&path, "right").unwrap();
            store
                .create_wallet("demo", Network::Undeployed, &fixed_seed())
                .unwrap()
        };
        // Reopen with the wrong passphrase.
        let bad = WalletStore::open(&path, "wrong").unwrap();
        let err = bad.wallet_seed(id).unwrap_err();
        match err {
            StoreError::Crypto(_) | StoreError::Corruption(_) => {}
            other => panic!("expected crypto/corruption, got {other:?}"),
        }
    }

    #[test]
    fn controller_secret_round_trips() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        let sk = fixed_seed();
        let did = "did:midnight:undeployed:0000000000000000000000000000000000000000000000000000000000000001";
        store
            .put_controller_secret(Network::Undeployed, did, &sk)
            .unwrap();
        let back = store
            .get_controller_secret(Network::Undeployed, did)
            .unwrap()
            .expect("controller_secret present");
        assert_eq!(&*back, &sk);
    }

    #[test]
    fn controller_secret_returns_none_for_unknown_did() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        let out = store
            .get_controller_secret(
                Network::Undeployed,
                "did:midnight:undeployed:does-not-exist",
            )
            .unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn list_controller_secrets_filters_by_network() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        let did_a = "did:midnight:undeployed:a";
        let did_b = "did:midnight:undeployed:b";
        let did_p = "did:midnight:preprod:p";
        store
            .put_controller_secret(Network::Undeployed, did_a, &[1u8; 32])
            .unwrap();
        store
            .put_controller_secret(Network::Undeployed, did_b, &[2u8; 32])
            .unwrap();
        store
            .put_controller_secret(Network::PreProd, did_p, &[3u8; 32])
            .unwrap();
        let undep = store.list_controller_secrets(Network::Undeployed).unwrap();
        assert_eq!(undep.len(), 2);
        let dids: Vec<&str> = undep.iter().map(|(d, _)| d.as_str()).collect();
        assert!(dids.contains(&did_a));
        assert!(dids.contains(&did_b));
        let pre = store.list_controller_secrets(Network::PreProd).unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].0, did_p);
    }

    #[test]
    fn seed_survives_close_and_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wallet.redb");
        let seed = fixed_seed();
        let id = {
            let store = WalletStore::open(&path, "pw").unwrap();
            store
                .create_wallet("demo", Network::Undeployed, &seed)
                .unwrap()
        };
        // Drop store, reopen.
        let store = WalletStore::open(&path, "pw").unwrap();
        let back = store.wallet_seed(id).unwrap();
        assert_eq!(&*back, &seed);
    }
}
