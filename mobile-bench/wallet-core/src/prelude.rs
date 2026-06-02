//! Common imports for downstream callers.
//!
//! Reaching into a half-dozen wallet-core modules to pull
//! `Network`, `DidId`, `Wallet`, the active `Clock`, `HttpClient`,
//! a couple of error types, etc. is the most-typed boilerplate in
//! every shell-level file. This module gathers the surface most
//! callers want into one glob-importable bundle, so the typical
//! pattern shrinks from:
//!
//! ```ignore
//! use wallet_core::{
//!     bootstrap_did_with_keys, BootstrapError, Clock, DidId,
//!     HttpClient, Network, SystemClock, Wallet,
//! };
//! use wallet_core::oid4vp_client::{
//!     DidAuthnDiscovery, DidSigner, IdTokenBuilder, LoginCoordinator,
//!     run_authentication,
//! };
//! use wallet_core::oid4vci_client::{
//!     run_issuance, CredentialCoordinator, IdTokenProofBuilder,
//! };
//! ```
//!
//! to:
//!
//! ```ignore
//! use wallet_core::prelude::*;
//! ```
//!
//! Curation rule: only items downstream **call sites** routinely
//! reach for. Internal implementation types, the full adapter
//! menagerie, and orchestration internals stay accessible via
//! the fully-qualified paths.
//!
//! The prelude has no implementation of its own — every item is
//! a re-export from somewhere already public.

// ── Domain ──────────────────────────────────────────────────────
pub use crate::network::{Network, NetworkConfig};
pub use crate::did::{
    DidDocument, DidId, ResolvedDid, VerificationMethod, VerificationMethodRef,
    VerificationMethodRelation,
};

// ── The wallet façade ───────────────────────────────────────────
pub use crate::wallet::{Wallet, WalletError};

// ── Port traits — the ones every caller wires up ────────────────
pub use crate::clock::Clock;
pub use crate::http::{HttpClient, HttpError};
pub use crate::oid4vp_client::{DidAuthnDiscovery, DidSigner};
pub use crate::vc_store::VcStorage;

// ── Default production adapters ────────────────────────────────
pub use crate::clock::SystemClock;
pub use crate::http::ReqwestHttpClient;

// ── Use-case orchestrators — what callers actually call ─────────
pub use crate::did::{bootstrap_did_with_keys, BootstrapError, BootstrappedDid};
pub use crate::oid4vci_client::{
    run_issuance, CredentialCoordinator, IdTokenProofBuilder, IssuanceFlowError,
    ProofBuilder,
};
pub use crate::oid4vp_client::{
    run_authentication, AuthFlowError, IdTokenBuilder, LoginCoordinator,
    LoginError,
};
pub use crate::vc_self_verify::{self_verify, self_verify_and_cache, SelfVerifyResult};
