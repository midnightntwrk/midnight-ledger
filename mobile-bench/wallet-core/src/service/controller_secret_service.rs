//! `ControllerSecretService` — per-DID random controller secrets
//! (cache + CRUD against the `CONTROLLER_SECRETS` redb table).
//!
//! Today these three methods live on `BridgeState`
//! (`dioxus-wallet/src/bridge.rs::remember_controller_secret`,
//! `controller_secret_for_on`, `hydrate_controller_secrets`).
//! Wave C1 lifts the bodies here so:
//!
//! - the headless binary (wave E) can reuse the same logic
//!   without dragging in Dioxus, and
//! - use-case tests can assert cache + store semantics
//!   against an in-memory store fixture.
//!
//! The bridge methods will be collapsed into one-line
//! delegations in a follow-up commit once we're confident the
//! service surface matches; for now the service stands alone
//! and the bridge keeps its inline copy. Both share the same
//! `WalletStore`, so behaviour stays unchanged.
//!
//! ## Cache semantics
//!
//! - `remember` — write-through: insert into the in-memory
//!   HashMap *and* persist. Persistence errors are logged but
//!   not propagated; the cache write always succeeds. This
//!   matches the existing bridge behaviour — the wallet keeps
//!   running with an in-RAM secret if persistence fails (e.g.
//!   disk full), surfacing the issue via `tracing::warn!`.
//! - `lookup_cached` — cache-only, sync, hot path. Returns
//!   `None` on miss without touching the store.
//! - `lookup_on(network)` — tries cache first, then falls
//!   back to the store and warms the cache on success.
//! - `hydrate(network)` — bulk-load every secret on a network
//!   into the cache; called once at app startup.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::store::WalletStore;
use crate::Network;

pub struct ControllerSecretService {
    pub(crate) store: Arc<WalletStore>,
    cache: Mutex<HashMap<String, [u8; 32]>>,
}

impl ControllerSecretService {
    pub fn new(store: Arc<WalletStore>) -> Self {
        Self {
            store,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Persist + cache a new (or replacement) controller
    /// secret. Cache insert always runs; store failures are
    /// logged but not surfaced because the runtime cache is
    /// already correct.
    pub fn remember(&self, network: Network, did: &str, sk: &[u8; 32]) {
        if let Ok(mut g) = self.cache.lock() {
            g.insert(did.to_string(), *sk);
        }
        if let Err(e) = self.store.put_controller_secret(network, did, sk) {
            tracing::warn!(error = %e, did = %did, "persist controller secret failed");
        }
    }

    /// Cache-only lookup. The hot path for the bridge's RPC
    /// loop where we don't have the network in scope cheaply.
    pub fn lookup_cached(&self, did: &str) -> Option<[u8; 32]> {
        self.cache.lock().ok().and_then(|g| g.get(did).copied())
    }

    /// Network-aware lookup: cache first, store fallback.
    /// Repopulates the cache on a store hit so subsequent
    /// `lookup_cached` calls are warm.
    pub fn lookup_on(&self, network: Network, did: &str) -> Option<[u8; 32]> {
        if let Some(found) = self.lookup_cached(did) {
            return Some(found);
        }
        match self.store.get_controller_secret(network, did) {
            Ok(Some(sk)) => {
                let bytes: [u8; 32] = *sk;
                if let Ok(mut g) = self.cache.lock() {
                    g.insert(did.to_string(), bytes);
                }
                Some(bytes)
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error = %e, did = %did, "load controller secret failed");
                None
            }
        }
    }

    /// Bulk-load every controller secret on `network` into
    /// the cache. Returns the count loaded (0 if the store
    /// list call fails — error is logged).
    pub fn hydrate(&self, network: Network) -> usize {
        match self.store.list_controller_secrets(network) {
            Ok(rows) => {
                let n = rows.len();
                if let Ok(mut g) = self.cache.lock() {
                    for (did, sk) in rows {
                        let bytes: [u8; 32] = *sk;
                        g.insert(did, bytes);
                    }
                }
                n
            }
            Err(e) => {
                tracing::warn!(error = %e, "hydrate controller secrets failed");
                0
            }
        }
    }

    /// Test-only: drain the cache without touching the store.
    /// Lets a test assert that `lookup_on` actually hit the
    /// store rather than just reading the cache.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_cache(&self) {
        if let Ok(mut g) = self.cache.lock() {
            g.clear();
        }
    }

    /// Test-only: snapshot cache size.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cache_size(&self) -> usize {
        self.cache.lock().map(|g| g.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> ControllerSecretService {
        let store = Arc::new(WalletStore::open_in_memory("pw").unwrap());
        ControllerSecretService::new(store)
    }

    #[test]
    fn remember_then_cached_lookup() {
        let s = svc();
        s.remember(Network::Undeployed, "did:midnight:undeployed:aa", &[7u8; 32]);
        assert_eq!(
            s.lookup_cached("did:midnight:undeployed:aa"),
            Some([7u8; 32])
        );
    }

    #[test]
    fn missing_did_is_none() {
        let s = svc();
        assert_eq!(s.lookup_cached("did:midnight:undeployed:none"), None);
        assert_eq!(
            s.lookup_on(Network::Undeployed, "did:midnight:undeployed:none"),
            None
        );
    }

    #[test]
    fn lookup_on_falls_back_to_store_and_warms_cache() {
        let s = svc();
        s.remember(Network::Undeployed, "did:midnight:undeployed:bb", &[3u8; 32]);
        // Drop the cache; lookup_on must hit the store and
        // repopulate.
        s.clear_cache();
        assert_eq!(s.cache_size(), 0);
        assert_eq!(
            s.lookup_on(Network::Undeployed, "did:midnight:undeployed:bb"),
            Some([3u8; 32])
        );
        // Cache is now warm — a follow-up cached lookup hits.
        assert_eq!(s.cache_size(), 1);
        assert_eq!(
            s.lookup_cached("did:midnight:undeployed:bb"),
            Some([3u8; 32])
        );
    }

    #[test]
    fn lookup_on_isolates_by_network() {
        let s = svc();
        s.remember(Network::PreProd, "did:midnight:preprod:cc", &[9u8; 32]);
        s.clear_cache();
        // Wrong network -> miss
        assert_eq!(
            s.lookup_on(Network::Undeployed, "did:midnight:preprod:cc"),
            None
        );
        // Right network -> hit
        assert_eq!(
            s.lookup_on(Network::PreProd, "did:midnight:preprod:cc"),
            Some([9u8; 32])
        );
    }

    #[test]
    fn hydrate_bulk_loads_a_networks_secrets() {
        let s = svc();
        s.remember(Network::Undeployed, "did:midnight:undeployed:1", &[1u8; 32]);
        s.remember(Network::Undeployed, "did:midnight:undeployed:2", &[2u8; 32]);
        s.remember(Network::PreProd, "did:midnight:preprod:3", &[3u8; 32]);
        s.clear_cache();
        let n = s.hydrate(Network::Undeployed);
        assert_eq!(n, 2);
        assert_eq!(s.cache_size(), 2);
        // PreProd secret was NOT pulled in.
        assert_eq!(s.lookup_cached("did:midnight:preprod:3"), None);
        // Both Undeployed secrets are now in the cache.
        assert_eq!(
            s.lookup_cached("did:midnight:undeployed:1"),
            Some([1u8; 32])
        );
        assert_eq!(
            s.lookup_cached("did:midnight:undeployed:2"),
            Some([2u8; 32])
        );
    }

    #[test]
    fn remember_overwrites_prior_secret() {
        let s = svc();
        s.remember(Network::Undeployed, "did:midnight:undeployed:k", &[0u8; 32]);
        s.remember(Network::Undeployed, "did:midnight:undeployed:k", &[1u8; 32]);
        assert_eq!(
            s.lookup_cached("did:midnight:undeployed:k"),
            Some([1u8; 32])
        );
        // Storage also reflects the overwrite.
        s.clear_cache();
        assert_eq!(
            s.lookup_on(Network::Undeployed, "did:midnight:undeployed:k"),
            Some([1u8; 32])
        );
    }
}
