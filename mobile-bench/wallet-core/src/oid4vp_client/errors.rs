//! Unified Phase-1 OID4VP / SIOPv2 error taxonomy on the wallet
//! side.
//!
//! Mirrors the normative guide §"Error handling" — same code
//! names, same semantic categories. The issuer-mock (TS) ships
//! the matching string codes; tests on either side can assert
//! against a single source of truth.
//!
//! ## Scope
//!
//! Only the **wallet-side** error variants live here. RP-side
//! verifier errors (`invalid_state`, `invalid_signature`,
//! `did_resolution_failed`, etc.) surface as HTTP 4xx/401
//! response bodies; the wallet's reaction is "show the message,
//! let the user retry", so no Rust enum captures them.

use thiserror::Error;

/// Errors a wallet-side OID4VP login can surface to the UI. The
/// `#[error]` payload is what the user sees — keep it short,
/// human-readable, and free of stack-trace noise.
#[derive(Debug, Error)]
pub enum LoginError {
    /// Wallet handed a malformed request — bad scheme, missing
    /// fields, unsupported mode. The inner message names the
    /// specific failure.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// `DidAuthnDiscovery::authn_key` returned an error.
    /// Inner message echoes the underlying `DiscoverError` —
    /// typically "no authentication-relation verification method"
    /// or an indexer-reachability failure.
    #[error("discover failed: {0}")]
    DiscoverFailed(String),

    /// `DidSigner::sign` returned an error. Either the local
    /// secret for the discovered kid is missing (wallet opened
    /// against a different store than the one holding the keys)
    /// or the sign primitive itself failed.
    #[error("sign failed: {0}")]
    SignFailed(String),

    /// JSON encoding of the JWS header / payload failed. Should
    /// be unreachable for well-formed inputs — surface defensively
    /// so a malformed `PublicKeyJwk` doesn't crash the flow.
    #[error("internal: {0}")]
    Internal(String),
}
