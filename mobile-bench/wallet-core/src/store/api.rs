//! `WalletStorage` port — async trait wrapping the persistent
//! state services need, plus an in-memory test adapter.
//!
//! The existing `WalletStore` (`store::mod`) is a synchronous,
//! redb-backed concrete that today is consumed directly from
//! `Wallet` and from a handful of UI threads. The headless
//! refactor needs:
//!
//! 1. An **async** surface so services can run inside the
//!    standard `tokio` runtime without juggling
//!    `spawn_blocking` at every call site.
//! 2. An **in-memory** implementation so integration tests
//!    can drive flows without a tempdir + redb file.
//! 3. A trait the service layer can depend on without
//!    knowing which concrete is in play.
//!
//! Wave B2 (this commit): the trait + `InMemoryWalletStorage`
//! test adapter. Wave D wires the real `RedbWalletStorage`
//! adapter and migrates call sites — until then `WalletStore`
//! continues to be used directly by `Wallet`, the headless
//! binary, and the Dioxus app.
//!
//! See design doc §2.3 (`WalletStorage` port) and §3 wave B2.

#[cfg(any(test, feature = "test-support"))]
use std::collections::HashMap;
#[cfg(any(test, feature = "test-support"))]
use std::sync::Mutex;

use async_trait::async_trait;
use zeroize::Zeroizing;

use crate::Network;
use crate::store::{DidInventoryEntry, StoreError, WalletMeta};
use crate::store::schema::WalletId;

/// Object-safe async port. Methods mirror the subset of
/// [`crate::store::WalletStore`] services actually call —
/// wallet CRUD, controller-secret CRUD, DID-inventory CRUD.
///
/// Things deliberately NOT on this trait:
///
/// - keys/sessions/logs/dust_sync — those are touched only
///   by their own subsystems and don't need to be
///   port-mediated yet. We can promote them as services
///   demand abstraction.
/// - backup/restore — wave G concern; the redb concrete's
///   inherent methods stay reachable for now.
#[async_trait]
pub trait WalletStorage: Send + Sync + 'static {
    // ── Wallets ───────────────────────────────────────────────

    /// Mint a fresh wallet row + wrap the seed under the
    /// store passphrase.
    async fn create_wallet(
        &self,
        label: &str,
        network: Network,
        seed: &[u8; 32],
    ) -> Result<WalletId, StoreError>;

    /// Enumerate every wallet currently in the store.
    async fn list_wallet_ids(&self) -> Result<Vec<WalletId>, StoreError>;

    /// Read wallet metadata (label + timestamps) without
    /// unwrapping the seed.
    async fn wallet_meta(
        &self,
        id: WalletId,
    ) -> Result<Option<WalletMeta>, StoreError>;

    /// Unwrap the seed for a wallet. The `Zeroizing` wrapper
    /// scrubs the bytes on drop.
    async fn wallet_seed(
        &self,
        id: WalletId,
    ) -> Result<Option<Zeroizing<[u8; 32]>>, StoreError>;

    // ── Controller secrets ────────────────────────────────────

    async fn put_controller_secret(
        &self,
        network: Network,
        did: &str,
        sk: &[u8; 32],
    ) -> Result<(), StoreError>;

    async fn get_controller_secret(
        &self,
        network: Network,
        did: &str,
    ) -> Result<Option<Zeroizing<[u8; 32]>>, StoreError>;

    async fn list_controller_secrets(
        &self,
        network: Network,
    ) -> Result<Vec<(String, Zeroizing<[u8; 32]>)>, StoreError>;

    // ── DID inventory ─────────────────────────────────────────

    async fn put_did_inventory(
        &self,
        entry: DidInventoryEntry,
    ) -> Result<(), StoreError>;

    async fn list_did_inventory(
        &self,
        network: Network,
    ) -> Result<Vec<DidInventoryEntry>, StoreError>;
}

// ───────────────────────────────────────────────────────────────
// `InMemoryWalletStorage` — test adapter, gated to `test-support`.
// ───────────────────────────────────────────────────────────────

/// HashMap-backed `WalletStorage`. Behind `test-support` so
/// production binaries can't accidentally pick it up. The
/// headless binary's `--in-memory-store` flag opts in
/// explicitly (wave E).
///
/// Semantics match the redb backend where they overlap:
///
/// - `create_wallet` mints a fresh `WalletId` per call (no
///   collision detection — callers don't reuse seeds).
/// - `put_did_inventory` preserves `created_at` on update,
///   stamps `updated_at` to "now".
/// - `list_controller_secrets` returns entries in insertion
///   order (the redb backend orders by raw bytes; tests
///   that care about order should sort).
///
/// The store does **not** encrypt anything — there's no
/// passphrase to wrap under in a test fixture. Callers that
/// care about envelope wrap/unwrap behaviour should test
/// against the real backend.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct InMemoryWalletStorage {
    inner: Mutex<MemState>,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
struct MemState {
    /// Counter for synthetic `WalletId`s. We hand out
    /// `[counter, 0, 0, ..., 0]` rather than `Uuid`s so
    /// tests can assert on stable values across runs.
    next_wallet_seq: u8,
    wallets: HashMap<WalletId, WalletRowMem>,
    controller_secrets: HashMap<(Network, String), [u8; 32]>,
    did_inventory: HashMap<(Network, String), DidInventoryEntry>,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone)]
struct WalletRowMem {
    label: String,
    network: Network,
    seed: [u8; 32],
    created_at: i64,
    updated_at: i64,
    address_bech32: String,
}

#[cfg(any(test, feature = "test-support"))]
impl InMemoryWalletStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(any(test, feature = "test-support"))]
fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait]
impl WalletStorage for InMemoryWalletStorage {
    async fn create_wallet(
        &self,
        label: &str,
        network: Network,
        seed: &[u8; 32],
    ) -> Result<WalletId, StoreError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        let mut raw = [0u8; 16];
        // First byte is the sequence so tests get stable
        // values; later bytes stay zero. Wraps at 256 — fine
        // for tests, would not be fine in production (which
        // is why this adapter is `test-support` only).
        raw[0] = g.next_wallet_seq;
        g.next_wallet_seq = g.next_wallet_seq.wrapping_add(1);
        let id = WalletId(raw);
        let now = unix_now_ms();
        g.wallets.insert(
            id,
            WalletRowMem {
                label: label.to_string(),
                network,
                seed: *seed,
                created_at: now,
                updated_at: now,
                address_bech32: String::new(),
            },
        );
        Ok(id)
    }

    async fn list_wallet_ids(&self) -> Result<Vec<WalletId>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        let mut out: Vec<WalletId> = g.wallets.keys().copied().collect();
        out.sort_by_key(|w| w.0);
        Ok(out)
    }

    async fn wallet_meta(
        &self,
        id: WalletId,
    ) -> Result<Option<WalletMeta>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        Ok(g.wallets.get(&id).map(|row| WalletMeta {
            id,
            label: row.label.clone(),
            network: row.network.into(),
            address_bech32: row.address_bech32.clone(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }))
    }

    async fn wallet_seed(
        &self,
        id: WalletId,
    ) -> Result<Option<Zeroizing<[u8; 32]>>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        Ok(g.wallets.get(&id).map(|row| {
            let mut out = Zeroizing::new([0u8; 32]);
            out.copy_from_slice(&row.seed);
            out
        }))
    }

    async fn put_controller_secret(
        &self,
        network: Network,
        did: &str,
        sk: &[u8; 32],
    ) -> Result<(), StoreError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        g.controller_secrets.insert((network, did.to_string()), *sk);
        Ok(())
    }

    async fn get_controller_secret(
        &self,
        network: Network,
        did: &str,
    ) -> Result<Option<Zeroizing<[u8; 32]>>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        Ok(g.controller_secrets
            .get(&(network, did.to_string()))
            .map(|sk| {
                let mut out = Zeroizing::new([0u8; 32]);
                out.copy_from_slice(sk);
                out
            }))
    }

    async fn list_controller_secrets(
        &self,
        network: Network,
    ) -> Result<Vec<(String, Zeroizing<[u8; 32]>)>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        let mut out: Vec<_> = g
            .controller_secrets
            .iter()
            .filter(|((net, _), _)| *net == network)
            .map(|((_, did), sk)| {
                let mut s = Zeroizing::new([0u8; 32]);
                s.copy_from_slice(sk);
                (did.clone(), s)
            })
            .collect();
        out.sort_by(|(a, _), (b, _)| a.cmp(b));
        Ok(out)
    }

    async fn put_did_inventory(
        &self,
        mut entry: DidInventoryEntry,
    ) -> Result<(), StoreError> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        let key = (entry.network, entry.did.clone());
        let now = unix_now_ms();
        if let Some(prior) = g.did_inventory.get(&key) {
            entry.created_at = prior.created_at;
        } else if entry.created_at == 0 {
            entry.created_at = now;
        }
        entry.updated_at = now;
        g.did_inventory.insert(key, entry);
        Ok(())
    }

    async fn list_did_inventory(
        &self,
        network: Network,
    ) -> Result<Vec<DidInventoryEntry>, StoreError> {
        let g = self
            .inner
            .lock()
            .map_err(|_| StoreError::Backend("mem lock poisoned".into()))?;
        let mut out: Vec<_> = g
            .did_inventory
            .iter()
            .filter(|((net, _), _)| *net == network)
            .map(|(_, v)| v.clone())
            .collect();
        out.sort_by(|a, b| a.did.cmp(&b.did));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::schema::InventoryStatus;

    fn sample_entry(did: &str, network: Network) -> DidInventoryEntry {
        DidInventoryEntry {
            did: did.to_string(),
            network,
            status: InventoryStatus::Active,
            counter: Some(1),
            vm_count: Some(0),
            service_count: Some(0),
            last_block_height: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn create_wallet_then_read_back() {
        let s = InMemoryWalletStorage::new();
        let id = s
            .create_wallet("alice", Network::Undeployed, &[7u8; 32])
            .await
            .unwrap();
        let meta = s.wallet_meta(id).await.unwrap().expect("meta");
        assert_eq!(meta.label, "alice");
        let seed = s.wallet_seed(id).await.unwrap().expect("seed");
        assert_eq!(&seed[..], &[7u8; 32]);
    }

    #[tokio::test]
    async fn list_wallets_returns_inserted_ids_sorted() {
        let s = InMemoryWalletStorage::new();
        let a = s.create_wallet("a", Network::Undeployed, &[0u8; 32]).await.unwrap();
        let b = s.create_wallet("b", Network::Undeployed, &[0u8; 32]).await.unwrap();
        let ids = s.list_wallet_ids().await.unwrap();
        assert_eq!(ids, vec![a, b]);
    }

    #[tokio::test]
    async fn missing_wallet_meta_is_none() {
        let s = InMemoryWalletStorage::new();
        let id = WalletId([0xff; 16]);
        assert!(s.wallet_meta(id).await.unwrap().is_none());
        assert!(s.wallet_seed(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn controller_secrets_round_trip_and_isolate_by_network() {
        let s = InMemoryWalletStorage::new();
        s.put_controller_secret(Network::Undeployed, "did:midnight:undeployed:aa", &[1u8; 32])
            .await
            .unwrap();
        s.put_controller_secret(Network::PreProd, "did:midnight:preprod:bb", &[2u8; 32])
            .await
            .unwrap();
        // get returns the right one for the right network
        let undeployed = s
            .get_controller_secret(Network::Undeployed, "did:midnight:undeployed:aa")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&undeployed[..], &[1u8; 32]);
        // cross-network lookup misses
        assert!(s
            .get_controller_secret(Network::PreProd, "did:midnight:undeployed:aa")
            .await
            .unwrap()
            .is_none());
        // list only the requested network
        let pre = s.list_controller_secrets(Network::PreProd).await.unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].0, "did:midnight:preprod:bb");
    }

    #[tokio::test]
    async fn missing_controller_secret_is_none() {
        let s = InMemoryWalletStorage::new();
        assert!(s
            .get_controller_secret(Network::Undeployed, "did:midnight:undeployed:zz")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn put_inventory_stamps_timestamps_and_preserves_created() {
        let s = InMemoryWalletStorage::new();
        let did = "did:midnight:undeployed:00";
        s.put_did_inventory(sample_entry(did, Network::Undeployed))
            .await
            .unwrap();
        let after_insert = s
            .list_did_inventory(Network::Undeployed)
            .await
            .unwrap();
        assert_eq!(after_insert.len(), 1);
        let created = after_insert[0].created_at;
        assert!(created > 0);

        // Update the same DID — `created_at` must survive.
        let mut updated = sample_entry(did, Network::Undeployed);
        updated.vm_count = Some(5);
        s.put_did_inventory(updated).await.unwrap();
        let after_update = s
            .list_did_inventory(Network::Undeployed)
            .await
            .unwrap();
        assert_eq!(after_update[0].vm_count, Some(5));
        assert_eq!(after_update[0].created_at, created);
        assert!(after_update[0].updated_at >= created);
    }

    #[tokio::test]
    async fn list_inventory_filters_by_network_and_sorts_by_did() {
        let s = InMemoryWalletStorage::new();
        s.put_did_inventory(sample_entry("did:midnight:undeployed:bb", Network::Undeployed))
            .await
            .unwrap();
        s.put_did_inventory(sample_entry("did:midnight:undeployed:aa", Network::Undeployed))
            .await
            .unwrap();
        s.put_did_inventory(sample_entry("did:midnight:preprod:cc", Network::PreProd))
            .await
            .unwrap();
        let undeployed = s.list_did_inventory(Network::Undeployed).await.unwrap();
        assert_eq!(
            undeployed.iter().map(|e| e.did.as_str()).collect::<Vec<_>>(),
            vec!["did:midnight:undeployed:aa", "did:midnight:undeployed:bb"]
        );
        let preprod = s.list_did_inventory(Network::PreProd).await.unwrap();
        assert_eq!(preprod.len(), 1);
    }
}
