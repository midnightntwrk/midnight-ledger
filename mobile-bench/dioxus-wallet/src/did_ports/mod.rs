//! Adapters that implement the wallet-core OID4VP ports
//! ([`wallet_core::oid4vp_client::DidAuthnDiscovery`],
//! [`wallet_core::oid4vp_client::DidSigner`]) on top of the
//! dioxus-wallet runtime types (the chain-op-capable `Wallet`,
//! the persistent `RedbSecretStore`).
//!
//! Architecture: see
//! `docs/superpowers/specs/2026-06-02-login-with-did-architecture.md`.

// The adapters live behind `#[allow(unused_imports)]` /
// `#[allow(dead_code)]` until Task 8 wires them into the OID4VP
// click site. Compile-check + tests cover them in the meantime;
// the lib.rs-wide `#![deny(warnings)]` would otherwise reject a
// new module that has no callers.
#[allow(dead_code)]
mod cached_authn_discovery;
#[allow(dead_code)]
mod redb_signer;

#[allow(unused_imports)]
pub use cached_authn_discovery::CachedWalletAuthnDiscovery;
#[allow(unused_imports)]
pub use redb_signer::RedbDidSigner;
