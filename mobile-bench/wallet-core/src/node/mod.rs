//! Node-side primitives.
//!
//! - [`client`] — JSON-RPC client for substrate methods
//!   (`system_health`, `chain_getFinalizedHead`, …).
//! - [`signer`] — ECDSA signer for the substrate tx envelope,
//!   reusing the wallet's BIP32-derived secp256k1 secret. See the
//!   `signer.rs` module-doc for the rationale.

mod client;
mod signer;

pub use client::{NodeClient, NodeError, NodeHealth, NodeStatus, SubmitResult};
pub use signer::{MidnightSigner, SignerError};
