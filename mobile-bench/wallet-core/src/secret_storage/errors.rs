//! Error taxonomy — Rust counterpart of
//! `secret-storage/src/errors.ts`. One enum with thiserror-derived
//! `Display` so callers can match per-variant without re-parsing
//! message strings.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretStoreError {
    /// Initialization failure: corrupt file, wrong passphrase,
    /// unsupported version, etc.
    #[error("init: {0}")]
    Init(String),

    /// Operation that needs an unlocked store ran while the store
    /// is still locked / no passphrase supplied for an encrypted
    /// file.
    #[error("store requires a passphrase")]
    Locked,

    /// `keyRef` doesn't resolve to any stored key.
    #[error("secret not found: {0}")]
    NotFound(String),

    /// Curve / kty combination outside the protocol's allowed set.
    #[error("unsupported curve: {0}")]
    UnsupportedCurve(String),

    /// Backend cannot sign for the given curve. Veramo-style
    /// adapters may hit this for Jubjub.
    #[error("signing not supported for curve {0}")]
    SigningNotSupported(String),

    /// Detached `verify()` returned false.
    #[error("signature verification failed")]
    VerificationFailed,

    /// Caller supplied invalid input (bad hex, wrong byte length,
    /// malformed JWK coordinate, etc.).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Underlying crypto primitive returned an error (key
    /// generation rejected, scrypt failure, AES-GCM tag mismatch).
    #[error("crypto: {0}")]
    Crypto(String),

    /// File I/O error reading or writing the store on disk.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// JSON encode/decode of the at-rest store format failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
