//! [`DidSigner`] backed by `wallet_core::secret_storage::redb_secret_store::RedbSecretStore`.
//!
//! The wallet's secret-storage trait already exposes the
//! "find-by-kid → sign" pair; this adapter is a thin shim that
//! adapts the `SecretStoreError` family to the narrow
//! [`wallet_core::oid4vp_client::SignError`] surface the
//! OID4VP login pipeline expects.
//!
//! ## Why a dedicated trait
//!
//! `SecretStorage` is broad — it also covers `import_key`,
//! `delete_key`, `generate_key`, `list_keys`, etc. The OID4VP
//! login pipeline only needs the sign primitive. Narrowing the
//! port to [`DidSigner`] makes unit tests for the JWS builder
//! independent of the storage backend's mutator surface, and
//! lets a future HSM-backed signer drop in without implementing
//! the full `SecretStorage` trait.
//!
//! ## Lifecycle
//!
//! One `RedbDidSigner` per (network, wallet_id). The worker's
//! `UseCaseContext` (Task 8) builds a fresh signer alongside the
//! discovery + wallet when the active context is rebuilt
//! (network switch, wallet rotation). Cheap to construct — just
//! holds the `WalletStore` + `WalletId` already in
//! `BridgeState`.

use async_trait::async_trait;

use wallet_core::oid4vp_client::{DidSigner, SignError};
use wallet_core::secret_storage::redb_secret_store::RedbSecretStore;
use wallet_core::secret_storage::SecretStorage;

pub struct RedbDidSigner {
    store: RedbSecretStore,
}

impl RedbDidSigner {
    pub fn new(store: RedbSecretStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl DidSigner for RedbDidSigner {
    async fn sign(&self, kid: &str, payload: &[u8]) -> Result<Vec<u8>, SignError> {
        // `find_by_kid` returns the `SecretKeyRef`; its
        // `uuid()` is the opaque handle `sign` actually
        // expects. Two redb reads on the cold path (`list_keys`
        // walk + the `sign` lookup) — fine for the once-per-
        // login frequency the login pipeline drives.
        let key_ref = self
            .store
            .find_by_kid(kid)
            .await
            .ok_or_else(|| SignError::NoLocalSecret(kid.to_string()))?;
        let out = self
            .store
            .sign(key_ref.uuid(), payload)
            .await
            .map_err(|e| SignError::Sign(e.to_string()))?;
        Ok(out.signature)
    }
}
