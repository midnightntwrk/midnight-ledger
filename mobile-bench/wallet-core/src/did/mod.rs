//! Midnight DID — Rust-native port.
//!
//! Mirrors `midnight-did-domain` (DID Core types) and a subset of
//! `midnight-did-api` (the wallet-facing operations) directly in
//! Rust, without going through the upstream TS / WebAssembly stack.
//!
//! Layout:
//! - [`id`] — `DidId` parser + bech32m codec (read-only smoke today).
//! - [`types`] — DID Core types: `DidDocument`, `VerificationMethod`,
//!   `Service`, etc.
//! - [`error`] — shared error enum.
//!
//! Phases (see `mobile-bench/DID_PLAN.md`):
//! 1. **types + codec** (this module) — pure-data, fully testable
//!    without network IO.
//! 2. **resolve** — query indexer, decode contract state, build a
//!    `DidDocument`.
//! 3. **create** — first write circuit, contract deploy.
//! 4. **all circuits** — addVerificationMethod / removeService / …

pub(crate) mod artifacts;
pub(crate) mod contract;
pub(crate) mod deploy;
mod error;
mod id;
mod types;

pub use error::DidError;
pub use id::{CONTRACT_ADDRESS_LEN, ContractAddressBytes, DidId, DidIdError};
pub use types::{
    CurveType, DidDocument, KeyType, PublicKeyJwk, Service, ServiceEndpoint,
    VerificationMethod, VerificationMethodRef, VerificationMethodRelation,
    VerificationMethodType,
};

/// A DID document plus the on-chain housekeeping that doesn't live
/// in DID Core. Returned by [`crate::Wallet::resolve_did_full`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDid {
    pub document: DidDocument,
    /// Counter the chain stamps on the contract's maintenance
    /// authority. The next `MaintenanceUpdate` for this contract
    /// must use exactly this value.
    pub maintenance_counter: u32,
    /// Block height of the last action (deploy / call / update)
    /// the indexer has seen for the DID.
    pub last_block_height: Option<i64>,
    /// `tx_hash` of that last action, hex-encoded.
    pub last_tx_hash: String,
    /// Hex-encoded `ContractState` bytes the indexer returned —
    /// raw on-chain state, surfaced to the UI's "Raw ledger
    /// state" tab for diagnostics.
    pub raw_state_hex: String,
    /// Wall-clock duration the resolve took (indexer round-trip
    /// + state decode). Surfaced as "Resolver latency" in the
    /// UI's Resolver tab.
    pub resolve_latency_ms: u64,
    /// Names of the verification-method `id`s in each relation
    /// set. The UI builds the relationship matrix from this.
    pub authentication_ids: Vec<String>,
    pub assertion_method_ids: Vec<String>,
    pub key_agreement_ids: Vec<String>,
    pub capability_invocation_ids: Vec<String>,
    pub capability_delegation_ids: Vec<String>,
}
