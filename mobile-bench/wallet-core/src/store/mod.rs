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
use redb::{Database, ReadableTable, ReadableTableMetadata};
use uuid::Uuid;

use crate::Network;

pub use error::StoreError;
pub use schema::{NetworkTag, SCHEMA_VERSION, WalletId};

use codec::Bincoded;
pub use envelope::{SecretEnvelope, decrypt_secret, encrypt_secret};
pub(crate) use envelope::encrypt_secret as wrap_secret;
pub use schema::{InventoryStatus, KeyDerivation};
use schema::{
    CONTROLLER_SECRETS, DID_INVENTORY, DIDS_BY_NETWORK, DidInventoryRowV1, KEYS, KEYS_BY_WALLET,
    KeyRowV1, META, RESOLVED_CACHE, ResolvedCacheRowV1, SESSION_CURRENT_KEY, SESSIONS,
    SessionRowV1, WALLETS, WalletRowV1,
};

use crate::secret_storage::{
    AlgorithmTag, MidnightCurve, MidnightKeyType, PublicJwk,
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

    /// Enumerate every `WalletId` in the store. Order is the
    /// redb iteration order (raw-bytes ascending), good enough
    /// for "pick the only wallet" or "show a wallet list" UX.
    pub fn list_wallet_ids(&self) -> Result<Vec<WalletId>, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(WALLETS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        for entry in table
            .iter()
            .map_err(|e| StoreError::Backend(e.to_string()))?
        {
            let (k, _) = entry.map_err(|e| StoreError::Backend(e.to_string()))?;
            out.push(WalletId(k.value()));
        }
        Ok(out)
    }

    /// Read the wallet metadata (everything except the seed
    /// envelope) for callers that want to render labels +
    /// timestamps in a wallet picker without unwrapping the
    /// secret.
    pub fn wallet_meta(&self, id: WalletId) -> Result<Option<WalletMeta>, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(WALLETS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let Some(raw) = table
            .get(id.0)
            .map_err(|e| StoreError::Backend(e.to_string()))?
        else {
            return Ok(None);
        };
        let row: WalletRowV1 = Bincoded::decode(raw.value())?;
        Ok(Some(WalletMeta {
            id,
            label: row.label,
            network: row.network,
            address_bech32: row.address_bech32,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }))
    }

    // ── Keys ──────────────────────────────────────────────────────

    /// Persist a key row. Replaces any existing row at
    /// `(wallet_id, key_ref)` and inserts into the secondary
    /// `KEYS_BY_WALLET` index. Bumps `updated_at` to "now".
    pub fn put_key(
        &self,
        wallet_id: WalletId,
        key_ref: &str,
        mut row: KeyRow,
    ) -> Result<(), StoreError> {
        let now = unix_now_ms();
        if row.created_at == 0 {
            row.created_at = now;
        }
        row.updated_at = now;
        let bincoded = Bincoded::encode(&row.into_v1())?;
        let key = (wallet_id.0, key_ref);
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        {
            let mut table = txn
                .open_table(KEYS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            table
                .insert(key, bincoded.as_slice())
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            let mut idx = txn
                .open_multimap_table(KEYS_BY_WALLET)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            idx.insert(wallet_id.0, key_ref)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Load a key row, if present.
    pub fn get_key(
        &self,
        wallet_id: WalletId,
        key_ref: &str,
    ) -> Result<Option<KeyRow>, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(KEYS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let Some(g) = table
            .get((wallet_id.0, key_ref))
            .map_err(|e| StoreError::Backend(e.to_string()))?
        else {
            return Ok(None);
        };
        let v1: KeyRowV1 = Bincoded::decode(g.value())?;
        Ok(Some(KeyRow::from_v1(v1)))
    }

    /// List every key belonging to a wallet. Optionally
    /// filters by DID (matches `StoredKeyMeta.did`).
    pub fn list_keys(
        &self,
        wallet_id: WalletId,
        did_filter: Option<&str>,
    ) -> Result<Vec<(String, KeyRow)>, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let idx = txn
            .open_multimap_table(KEYS_BY_WALLET)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let mut refs: Vec<String> = Vec::new();
        let iter = idx
            .get(wallet_id.0)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        for entry in iter {
            let r = entry.map_err(|e| StoreError::Backend(e.to_string()))?;
            refs.push(r.value().to_string());
        }
        let table = txn
            .open_table(KEYS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(refs.len());
        for r in refs {
            let Some(g) = table
                .get((wallet_id.0, r.as_str()))
                .map_err(|e| StoreError::Backend(e.to_string()))?
            else {
                continue;
            };
            let v1: KeyRowV1 = Bincoded::decode(g.value())?;
            if let Some(want) = did_filter {
                if v1.did.as_deref() != Some(want) {
                    continue;
                }
            }
            out.push((r, KeyRow::from_v1(v1)));
        }
        // Sort by created_at then ref so listings are stable.
        out.sort_by(|a, b| a.1.created_at.cmp(&b.1.created_at).then(a.0.cmp(&b.0)));
        Ok(out)
    }

    /// Delete a key row + drop the index entry.
    pub fn delete_key(&self, wallet_id: WalletId, key_ref: &str) -> Result<(), StoreError> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        {
            let mut table = txn
                .open_table(KEYS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            table
                .remove((wallet_id.0, key_ref))
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            let mut idx = txn
                .open_multimap_table(KEYS_BY_WALLET)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            idx.remove(wallet_id.0, key_ref)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Recover the raw scalar for a key. Branches on the
    /// derivation variant: HKDF rows re-derive from the
    /// wallet seed (which we unwrap once); Direct rows unwrap
    /// the row's envelope.
    pub fn key_private_bytes(
        &self,
        wallet_id: WalletId,
        key_ref: &str,
    ) -> Result<zeroize::Zeroizing<Vec<u8>>, StoreError> {
        let Some(row) = self.get_key(wallet_id, key_ref)? else {
            return Err(StoreError::NotFound("key"));
        };
        match row.derivation {
            KeyDerivation::Hkdf {
                account,
                index,
                candidate,
            } => {
                let seed = self.wallet_seed(wallet_id)?;
                let seed_hex = hex::encode(&*seed);
                let params = crate::secret_storage::DeriveKeyFromSeedInput {
                    id: row.label,
                    seed_hex,
                    kty: row.kty,
                    crv: row.crv,
                    account: Some(account),
                    index: Some(index),
                    did: row.did,
                    purpose: row.purpose,
                };
                let derived = crate::secret_storage::hd_derivation::derive_curve_private_from_seed(
                    &params, candidate,
                )
                .map_err(StoreError::from)?;
                Ok(zeroize::Zeroizing::new(derived.private_bytes))
            }
            KeyDerivation::Direct { envelope } => {
                let bytes = decrypt_secret(self.passphrase(), &envelope)?;
                Ok(zeroize::Zeroizing::new(bytes))
            }
        }
    }

    // ── DID inventory ─────────────────────────────────────────────

    /// Upsert an inventory row for the given DID. Stamps
    /// `updated_at`; preserves `created_at` if a row already
    /// exists.
    pub fn put_did_inventory(
        &self,
        entry: DidInventoryEntry,
    ) -> Result<(), StoreError> {
        let net = NetworkTag::from(entry.network).0;
        let did = entry.did.as_str();
        let now = unix_now_ms();
        let mut row = DidInventoryRowV1 {
            network: NetworkTag::from(entry.network),
            status: entry.status,
            counter: entry.counter,
            vm_count: entry.vm_count,
            service_count: entry.service_count,
            last_block_height: entry.last_block_height,
            created_at: 0,
            updated_at: now,
        };
        // Preserve created_at if the row already exists.
        {
            let txn = self
                .db
                .begin_read()
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            let table = txn
                .open_table(DID_INVENTORY)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            if let Some(g) = table
                .get((net, did))
                .map_err(|e| StoreError::Backend(e.to_string()))?
            {
                let prior: DidInventoryRowV1 = Bincoded::decode(g.value())?;
                row.created_at = prior.created_at;
            }
        }
        if row.created_at == 0 {
            row.created_at = now;
        }
        let bincoded = Bincoded::encode(&row)?;
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        {
            let mut table = txn
                .open_table(DID_INVENTORY)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            table
                .insert((net, did), bincoded.as_slice())
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            let mut idx = txn
                .open_multimap_table(DIDS_BY_NETWORK)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            idx.insert(net, did)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Pull every inventory row for a network. Used to
    /// hydrate the App's `did_inventory` signal at startup.
    pub fn list_did_inventory(
        &self,
        network: Network,
    ) -> Result<Vec<DidInventoryEntry>, StoreError> {
        let net = NetworkTag::from(network).0;
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(DID_INVENTORY)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        for entry in table
            .range((net, "")..(net.saturating_add(1), ""))
            .map_err(|e| StoreError::Backend(e.to_string()))?
        {
            let (k, v) = entry.map_err(|e| StoreError::Backend(e.to_string()))?;
            let (_, did) = k.value();
            let row: DidInventoryRowV1 = Bincoded::decode(v.value())?;
            out.push(DidInventoryEntry {
                did: did.to_string(),
                network,
                status: row.status,
                counter: row.counter,
                vm_count: row.vm_count,
                service_count: row.service_count,
                last_block_height: row.last_block_height,
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.did.cmp(&b.did)));
        Ok(out)
    }

    // ── Resolved-DID cache ────────────────────────────────────────

    /// Persist the most-recent resolve for a DID. Caller
    /// passes the JSON-serialised `ResolvedDid` so this module
    /// doesn't need to depend on the DID layer.
    pub fn put_resolved_cache(
        &self,
        network: Network,
        did: &str,
        resolved_json: String,
    ) -> Result<(), StoreError> {
        let now = unix_now_ms();
        let row = ResolvedCacheRowV1 {
            resolved_json,
            cached_at: now,
        };
        let bincoded = Bincoded::encode(&row)?;
        let key = (NetworkTag::from(network).0, did);
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        {
            let mut table = txn
                .open_table(RESOLVED_CACHE)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            table
                .insert(key, bincoded.as_slice())
                .map_err(|e| StoreError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Read a cached resolve, if any. Returns the JSON string
    /// + the cached-at timestamp; the UI decodes via serde
    /// (a drift error is the caller's signal to drop the row
    /// and re-resolve).
    pub fn get_resolved_cache(
        &self,
        network: Network,
        did: &str,
    ) -> Result<Option<(String, i64)>, StoreError> {
        let key = (NetworkTag::from(network).0, did);
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(RESOLVED_CACHE)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let Some(g) = table
            .get(key)
            .map_err(|e| StoreError::Backend(e.to_string()))?
        else {
            return Ok(None);
        };
        let row: ResolvedCacheRowV1 = Bincoded::decode(g.value())?;
        Ok(Some((row.resolved_json, row.cached_at)))
    }

    /// Bulk load every cached resolve for a network. Powers
    /// the App-startup hydration of `resolved_cache` so the
    /// detail view's tabs still have content after a reload.
    pub fn list_resolved_cache(
        &self,
        network: Network,
    ) -> Result<Vec<(String, String, i64)>, StoreError> {
        let net = NetworkTag::from(network).0;
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(RESOLVED_CACHE)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        for entry in table
            .range((net, "")..(net.saturating_add(1), ""))
            .map_err(|e| StoreError::Backend(e.to_string()))?
        {
            let (k, v) = entry.map_err(|e| StoreError::Backend(e.to_string()))?;
            let (_, did) = k.value();
            let row: ResolvedCacheRowV1 = Bincoded::decode(v.value())?;
            out.push((did.to_string(), row.resolved_json, row.cached_at));
        }
        Ok(out)
    }

    // ── Session ───────────────────────────────────────────────────

    /// Persist the most recent session snapshot. Single-row
    /// table — overwrites the prior value. Cheap enough to
    /// call on every state change of interest (the UI debounces
    /// rapid mutations).
    pub fn put_session(&self, snapshot: SessionSnapshot) -> Result<(), StoreError> {
        let row = SessionRowV1 {
            network: NetworkTag::from(snapshot.network),
            active_tab: snapshot.active_tab,
            open_did: snapshot.open_did,
            last_did_id: snapshot.last_did_id,
            last_resolved: snapshot.last_resolved,
            updated_at: unix_now_ms(),
        };
        let bincoded = Bincoded::encode(&row)?;
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        {
            let mut table = txn
                .open_table(SESSIONS)
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            table
                .insert(SESSION_CURRENT_KEY, bincoded.as_slice())
                .map_err(|e| StoreError::Backend(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Restore the last persisted session snapshot, if any.
    /// `None` on a fresh wallet store.
    pub fn get_session(&self) -> Result<Option<SessionSnapshot>, StoreError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let table = txn
            .open_table(SESSIONS)
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let Some(g) = table
            .get(SESSION_CURRENT_KEY)
            .map_err(|e| StoreError::Backend(e.to_string()))?
        else {
            return Ok(None);
        };
        let row: SessionRowV1 = Bincoded::decode(g.value())?;
        // Reject corrupt network tags rather than silently
        // falling back to mainnet.
        let net = match row.network.0 {
            1 => Network::Mainnet,
            2 => Network::PreProd,
            3 => Network::Preview,
            4 => Network::QaNet,
            5 => Network::DevNet,
            6 => Network::Undeployed,
            other => {
                return Err(StoreError::Corruption(format!(
                    "unknown network tag {other} in session row",
                )));
            }
        };
        Ok(Some(SessionSnapshot {
            network: net,
            active_tab: row.active_tab,
            open_did: row.open_did,
            last_did_id: row.last_did_id,
            last_resolved: row.last_resolved,
            updated_at: row.updated_at,
        }))
    }

    /// One-shot stats snapshot — table row counts + schema
    /// version. Lets the UI render a Settings card without
    /// holding the file open.
    pub fn stats(&self) -> Result<StoreStats, StoreError> {
        let v = self.schema_version()?;
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        // Each table has its own concrete (K, V) so we can't
        // share one helper; the `len()` call lives on the
        // `ReadableTable` trait already imported at the top
        // of the file.
        let wallets = txn.open_table(WALLETS).ok().and_then(|t| t.len().ok()).unwrap_or(0);
        let keys = txn.open_table(KEYS).ok().and_then(|t| t.len().ok()).unwrap_or(0);
        let controller_secrets = txn
            .open_table(CONTROLLER_SECRETS)
            .ok()
            .and_then(|t| t.len().ok())
            .unwrap_or(0);
        let did_inventory = txn
            .open_table(DID_INVENTORY)
            .ok()
            .and_then(|t| t.len().ok())
            .unwrap_or(0);
        let resolved_cache = txn
            .open_table(RESOLVED_CACHE)
            .ok()
            .and_then(|t| t.len().ok())
            .unwrap_or(0);
        let sessions = txn.open_table(SESSIONS).ok().and_then(|t| t.len().ok()).unwrap_or(0);
        Ok(StoreStats {
            schema_version: v,
            wallets,
            keys,
            controller_secrets,
            did_inventory,
            resolved_cache,
            sessions,
        })
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

/// Counts + schema version snapshot — what
/// [`WalletStore::stats`] returns. Wired into the
/// dioxus-wallet Settings tab so the user can see how many
/// rows live in each table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreStats {
    pub schema_version: u32,
    pub wallets: u64,
    pub keys: u64,
    pub controller_secrets: u64,
    pub did_inventory: u64,
    pub resolved_cache: u64,
    pub sessions: u64,
}

/// Last-session snapshot — what the UI needs to restore its
/// per-session state after a reload. Persisted under a
/// single-row key (`"current"`); a fresh wallet store returns
/// `None` from `get_session`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub network: Network,
    /// Integer the dioxus-wallet `Tab` enum maps to. Kept as
    /// `u8` so a UI variant rename doesn't break decoding —
    /// the App's match-all-other-variants-to-default policy
    /// is in the wrapper code.
    pub active_tab: u8,
    pub open_did: Option<String>,
    pub last_did_id: Option<String>,
    pub last_resolved: Option<(String, u32)>,
    pub updated_at: i64,
}

/// One DID inventory row as the UI sees it. Mirrors the
/// `DidInventoryEntry` struct dioxus-wallet keeps in-memory
/// today — the App will replace that signal's persistence
/// with calls into the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DidInventoryEntry {
    /// `did:midnight:<network>:<address>` — primary key.
    pub did: String,
    pub network: Network,
    pub status: InventoryStatus,
    pub counter: Option<u32>,
    pub vm_count: Option<u32>,
    pub service_count: Option<u32>,
    pub last_block_height: Option<i64>,
    /// Unix-ms. Zero on a fresh row → `put_did_inventory`
    /// stamps it on first write.
    pub created_at: i64,
    pub updated_at: i64,
}

/// Wallet metadata — everything in the wallet row except the
/// encrypted seed. Returned by [`WalletStore::wallet_meta`]
/// for UI surfaces that want to render labels / timestamps
/// without unwrapping a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletMeta {
    pub id: WalletId,
    pub label: String,
    pub network: NetworkTag,
    pub address_bech32: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Public face of `KeyRowV1` — same fields, but exported so
/// callers (e.g. the upcoming `RedbSecretStore`) can build /
/// inspect rows without depending on the crate-private V1
/// alias. A version bump introduces `KeyRowV2` etc.; the
/// `from_vN` / `into_vN` shims live next to the table
/// accessors so the public API stays stable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRow {
    pub label: String,
    pub did: Option<String>,
    pub purpose: Option<String>,
    pub kty: MidnightKeyType,
    pub crv: MidnightCurve,
    pub public_jwk: PublicJwk,
    pub algorithm: AlgorithmTag,
    pub derivation: KeyDerivation,
    /// Unix-ms. Zero on a fresh row → `put_key` stamps it on
    /// first write.
    pub created_at: i64,
    pub updated_at: i64,
}

impl KeyRow {
    fn into_v1(self) -> KeyRowV1 {
        KeyRowV1 {
            label: self.label,
            did: self.did,
            purpose: self.purpose,
            kty: self.kty,
            crv: self.crv,
            public_jwk: self.public_jwk.into(),
            algorithm: self.algorithm,
            derivation: self.derivation,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    fn from_v1(v1: KeyRowV1) -> Self {
        Self {
            label: v1.label,
            did: v1.did,
            purpose: v1.purpose,
            kty: v1.kty,
            crv: v1.crv,
            public_jwk: v1.public_jwk.into(),
            algorithm: v1.algorithm,
            derivation: v1.derivation,
            created_at: v1.created_at,
            updated_at: v1.updated_at,
        }
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
    fn did_inventory_round_trip() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        let did = "did:midnight:undeployed:0000000000000000000000000000000000000000000000000000000000000abc";
        store
            .put_did_inventory(DidInventoryEntry {
                did: did.to_string(),
                network: Network::Undeployed,
                status: InventoryStatus::Pending,
                counter: None,
                vm_count: None,
                service_count: None,
                last_block_height: None,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();
        let all = store.list_did_inventory(Network::Undeployed).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].did, did);
        assert_eq!(all[0].status, InventoryStatus::Pending);
        assert!(all[0].created_at > 0);
    }

    #[test]
    fn did_inventory_update_preserves_created_at() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        let did = "did:midnight:undeployed:abc";
        store
            .put_did_inventory(DidInventoryEntry {
                did: did.to_string(),
                network: Network::Undeployed,
                status: InventoryStatus::Pending,
                counter: None,
                vm_count: None,
                service_count: None,
                last_block_height: None,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();
        let first = store.list_did_inventory(Network::Undeployed).unwrap()[0].clone();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store
            .put_did_inventory(DidInventoryEntry {
                did: did.to_string(),
                network: Network::Undeployed,
                status: InventoryStatus::Active,
                counter: Some(1),
                vm_count: Some(2),
                service_count: Some(1),
                last_block_height: Some(99),
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();
        let second = store.list_did_inventory(Network::Undeployed).unwrap()[0].clone();
        assert_eq!(first.created_at, second.created_at);
        assert!(second.updated_at >= first.updated_at);
        assert_eq!(second.status, InventoryStatus::Active);
    }

    #[test]
    fn session_round_trip() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        assert!(store.get_session().unwrap().is_none());
        store
            .put_session(SessionSnapshot {
                network: Network::Undeployed,
                active_tab: 2,
                open_did: Some("did:midnight:undeployed:x".into()),
                last_did_id: Some("did:midnight:undeployed:x".into()),
                last_resolved: Some(("did:midnight:undeployed:x".into(), 7)),
                updated_at: 0,
            })
            .unwrap();
        let snap = store.get_session().unwrap().unwrap();
        assert_eq!(snap.network, Network::Undeployed);
        assert_eq!(snap.active_tab, 2);
        assert_eq!(snap.last_resolved.as_ref().unwrap().1, 7);
        assert!(snap.updated_at > 0);
    }

    #[test]
    fn session_overwrites_in_place() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        store
            .put_session(SessionSnapshot {
                network: Network::Undeployed,
                active_tab: 0,
                open_did: None,
                last_did_id: None,
                last_resolved: None,
                updated_at: 0,
            })
            .unwrap();
        store
            .put_session(SessionSnapshot {
                network: Network::PreProd,
                active_tab: 5,
                open_did: Some("did:b".into()),
                last_did_id: Some("did:b".into()),
                last_resolved: None,
                updated_at: 0,
            })
            .unwrap();
        let snap = store.get_session().unwrap().unwrap();
        assert_eq!(snap.network, Network::PreProd);
        assert_eq!(snap.active_tab, 5);
    }

    #[test]
    fn resolved_cache_round_trip() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        let did = "did:midnight:undeployed:abc";
        store
            .put_resolved_cache(Network::Undeployed, did, r#"{"foo":"bar"}"#.to_string())
            .unwrap();
        let (json, cached_at) = store
            .get_resolved_cache(Network::Undeployed, did)
            .unwrap()
            .unwrap();
        assert_eq!(json, r#"{"foo":"bar"}"#);
        assert!(cached_at > 0);
    }

    #[test]
    fn list_resolved_cache_filters_by_network() {
        let store = WalletStore::open_in_memory("pw").unwrap();
        store
            .put_resolved_cache(Network::Undeployed, "did:undep:a", "{}".into())
            .unwrap();
        store
            .put_resolved_cache(Network::Undeployed, "did:undep:b", "{}".into())
            .unwrap();
        store
            .put_resolved_cache(Network::PreProd, "did:preprod:c", "{}".into())
            .unwrap();
        let undep = store.list_resolved_cache(Network::Undeployed).unwrap();
        assert_eq!(undep.len(), 2);
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
