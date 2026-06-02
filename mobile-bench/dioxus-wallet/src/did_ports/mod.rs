//! Adapters that implement the wallet-core OID4VP ports
//! ([`wallet_core::oid4vp_client::DidAuthnDiscovery`],
//! [`wallet_core::oid4vp_client::DidSigner`]) on top of the
//! dioxus-wallet runtime types (the chain-op-capable `Wallet`,
//! the persistent `RedbSecretStore`).
//!
//! Architecture: see
//! `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.

// Both adapters are consumed by `identity_centre::run_oid4vp_authenticate`
// after Task 8 (commit ce97a5e0 + this commit). The `#[allow(dead_code)]`
// /` #[allow(unused_imports)]` shields the original Tasks 2-3 commit
// added are no longer needed — kept the file note for archaeology.
mod cached_authn_discovery;
mod redb_signer;

pub use cached_authn_discovery::CachedWalletAuthnDiscovery;
pub use redb_signer::RedbDidSigner;
