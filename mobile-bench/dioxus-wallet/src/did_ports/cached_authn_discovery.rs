//! [`DidAuthnDiscovery`] backed by `wallet_core::Wallet`
//! resolution + a 30 s TTL cache keyed on the DID string.
//!
//! ## Why cache
//!
//! Every OID4VP / OID4VCI login resolves the holder DID once
//! to discover `(kid, public_jwk)`. The old `did_auth` path
//! resolved twice per login (probe + real sign); the new ports
//! drop that to once. A short-lived per-DID cache cuts indexer
//! roundtrips further for rapid-fire flows ("scan QR → present
//! VC → scan another QR" within seconds).
//!
//! ## TTL trade-off
//!
//! 30 s strikes a balance for the Phase-1 demo:
//!
//! - Long enough to absorb back-to-back logins without
//!   re-hitting the indexer.
//! - Short enough that a `MaintenanceUpdate` adding a new
//!   authentication VM is reflected on the next login.
//!
//! When the wallet ships VM rotation as a user-driven feature
//! (Phase 2 spec — not landed yet), this cache will need an
//! explicit invalidation hook. Tracked in
//! `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`
//! §"Risks".
//!
//! ## Thread safety
//!
//! `Mutex<HashMap<…>>` is `Send + Sync`; entries are `AuthnKey`,
//! which is `Clone + Send`. The cache itself is held inside the
//! adapter and never crosses the worker boundary by reference —
//! the worker holds an `Arc<dyn DidAuthnDiscovery>` and clones
//! the trait object once at session start.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use wallet_core::oid4vp_client::{AuthnKey, DidAuthnDiscovery, DiscoverError};
use wallet_core::{DidId, VerificationMethodRef, Wallet};

/// 30 seconds — see file-level "TTL trade-off" note.
const CACHE_TTL: Duration = Duration::from_secs(30);

struct Entry {
    key: AuthnKey,
    inserted: Instant,
}

/// `DidAuthnDiscovery` adapter that wraps the chain-op-capable
/// `Wallet` and caches the picked authentication key per DID.
pub struct CachedWalletAuthnDiscovery {
    wallet: Wallet,
    cache: Mutex<HashMap<String, Entry>>,
}

impl CachedWalletAuthnDiscovery {
    pub fn new(wallet: Wallet) -> Self {
        Self {
            wallet,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Drop the cache. Wallet-state changes (network switch, new
    /// wallet_id) should call this so the next discovery hits a
    /// fresh document. The worker calls it when rebuilding its
    /// `UseCaseContext` after a network switch (Task 8+).
    #[allow(dead_code)] // Wired into the worker context in Task 8.
    pub fn clear_cache(&self) {
        self.cache.lock().expect("poisoned").clear();
    }
}

#[async_trait]
impl DidAuthnDiscovery for CachedWalletAuthnDiscovery {
    async fn authn_key(&self, did: &DidId) -> Result<AuthnKey, DiscoverError> {
        let did_str = did.to_did_string();

        // Cache hit (still fresh) — short-circuit. Borrow the
        // mutex only as long as needed; never hold across .await.
        if let Some(hit) = self.cache.lock().expect("poisoned").get(&did_str) {
            if hit.inserted.elapsed() < CACHE_TTL {
                return Ok(hit.key.clone());
            }
        }

        // Cache miss / stale — resolve.
        let doc = self
            .wallet
            .resolve_did(&did_str)
            .await
            .map_err(|e| DiscoverError::Resolve(e.to_string()))?;

        // First entry in the `authentication` relation. Both the
        // `Id(String)` (kid pointer) and `Inline(VerificationMethod)`
        // shapes appear in real documents; coerce to one
        // `(kid, jwk)` pair.
        let (kid, public_jwk) = match doc
            .authentication
            .first()
            .ok_or_else(|| DiscoverError::NoAuthnKey(did_str.clone()))?
        {
            VerificationMethodRef::Inline(vm) => {
                (vm.id.clone(), vm.public_key_jwk.clone())
            }
            VerificationMethodRef::Id(id) => {
                let vm = doc
                    .verification_method
                    .iter()
                    .find(|v| v.id == *id)
                    .ok_or_else(|| {
                        DiscoverError::Resolve(format!(
                            "authentication kid {id} not present in \
                             verificationMethod[]"
                        ))
                    })?;
                (vm.id.clone(), vm.public_key_jwk.clone())
            }
        };

        let key = AuthnKey { kid, public_jwk };
        self.cache.lock().expect("poisoned").insert(
            did_str,
            Entry {
                key: key.clone(),
                inserted: Instant::now(),
            },
        );
        Ok(key)
    }
}
