//! Ports the OID4VP client consumes, split out of the old
//! `did_auth::sign_for_authentication` monolithic signature.
//!
//! Adapters live on the dioxus-wallet side (a Wallet-backed
//! resolver / discovery and a RedbSecretStore-backed signer);
//! wallet-core only defines the traits so unit tests can mock
//! each independently.
//!
//! Rationale and migration plan:
//! `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.
//!
//! Today's `did_auth::sign_for_authentication` does discovery
//! (resolve DID, pick authentication-relation VM, extract kid +
//! publicKeyJwk) AND signing in one call. `build_id_token` calls
//! it twice — once to discover the kid + jwk (signature
//! discarded), once to actually sign — costing two indexer
//! resolves and two signing operations per login. Splitting into
//! [`DidAuthnDiscovery`] and [`DidSigner`] lets the caller do
//! one of each, and lets discovery be cached at the adapter
//! layer (the dioxus-wallet `CachedWalletAuthnDiscovery` carries
//! a 30 s TTL by DID).

use async_trait::async_trait;

use crate::{DidId, PublicKeyJwk};

/// Result of resolving a DID + selecting an authentication-
/// relation verification method. Stable across the lifetime of
/// the returned value — callers are free to embed both fields
/// in a JWS / proof object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthnKey {
    /// Full DID URL form: `did:midnight:net:abc#key-auth`.
    pub kid: String,
    pub public_jwk: PublicKeyJwk,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    /// Failed to fetch / decode the DID document.
    #[error("resolve failed: {0}")]
    Resolve(String),
    /// The DID document has no entry in the `authentication`
    /// verification relation.
    #[error("no authentication-relation verification method on {0}")]
    NoAuthnKey(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    /// The local secret store has no key matching `kid`. Either
    /// the DID isn't this wallet's, or the wallet was opened
    /// against a different store than the one that holds the
    /// keys.
    #[error("no local secret for kid {0}")]
    NoLocalSecret(String),
    /// Sign primitive failed (key corrupted, hardware-wallet
    /// disconnect, etc.). Inner string is the platform message.
    #[error("sign failed: {0}")]
    Sign(String),
}

/// Discovery port. Resolves the DID, picks the verification
/// method authorized for the `authentication` relation, returns
/// its kid + public JWK. NO signing — this is purely a
/// look-up step.
///
/// Implementations are free to cache: the result is stable until
/// the underlying DID document is updated on chain
/// (`MaintenanceUpdate`). For the Phase-1 demo the wallet does
/// not rotate authentication VMs, so a 30 s TTL cache is the
/// right trade-off; longer caches may need explicit
/// invalidation when we ship rotation.
#[async_trait]
pub trait DidAuthnDiscovery: Send + Sync {
    async fn authn_key(&self, did: &DidId) -> Result<AuthnKey, DiscoverError>;
}

/// Signing port. Signs `payload` with the local secret bound to
/// `kid`. The `kid` is whatever [`DidAuthnDiscovery::authn_key`]
/// returned for the same DID.
///
/// The output is raw signature bytes; the JWS / detached-sig
/// framing is the caller's concern (`oid4vp_client::id_token`
/// handles it for SIOPv2).
#[async_trait]
pub trait DidSigner: Send + Sync {
    async fn sign(&self, kid: &str, payload: &[u8]) -> Result<Vec<u8>, SignError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke-check: trait objects compile + the error variants
    /// stringify the way the unified `LoginError::DiscoverFailed`
    /// / `LoginError::SignFailed` arms expect.
    #[test]
    fn errors_display_includes_payload() {
        let d = DiscoverError::NoAuthnKey("did:midnight:abc".into());
        assert!(format!("{d}").contains("did:midnight:abc"));
        let s = SignError::NoLocalSecret("did:midnight:abc#key-auth".into());
        assert!(format!("{s}").contains("#key-auth"));
    }
}
