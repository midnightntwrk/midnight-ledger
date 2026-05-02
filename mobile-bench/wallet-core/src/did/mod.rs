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
