//! Error type surfaced by the store façade. Stays distinct from
//! `SecretStoreError` so callers can pattern-match on store-
//! specific failures (corrupt rows, missing tables, migration
//! mismatch) without conflating with curve-level crypto errors.

use std::fmt;

#[derive(Debug)]
pub enum StoreError {
    /// Underlying `redb` returned an error — schema mismatch,
    /// disk IO, transaction conflict, etc.
    Backend(String),
    /// A row's serialized bytes don't decode — schema version
    /// mismatch, corrupt file, or a deliberate downgrade
    /// attempt. The wallet should refuse to start in this
    /// case and surface the problem to the user.
    Corruption(String),
    /// Envelope wrap/unwrap failed. Most common cause: wrong
    /// passphrase. Distinguished from `Corruption` because the
    /// UI can prompt for a re-entry.
    Crypto(String),
    /// A typed lookup found no row at the requested key. The
    /// `&'static str` carries the table name for diagnostics
    /// ("wallet", "controller_secret", …) without leaking the
    /// key bytes.
    NotFound(&'static str),
    /// Schema migration could not run — usually because the
    /// on-disk version is higher than the binary's
    /// `SCHEMA_VERSION` (downgrade attempt).
    Migration(String),
    /// Bincode encode/decode failure. Almost always means a
    /// schema struct changed; see the table's row-type doc
    /// comment for the versioning rules.
    Codec(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Backend(m) => write!(f, "store backend: {m}"),
            StoreError::Corruption(m) => write!(f, "store corruption: {m}"),
            StoreError::Crypto(m) => write!(f, "store crypto: {m}"),
            StoreError::NotFound(t) => write!(f, "store not found: {t}"),
            StoreError::Migration(m) => write!(f, "store migration: {m}"),
            StoreError::Codec(m) => write!(f, "store codec: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<crate::secret_storage::SecretStoreError> for StoreError {
    fn from(e: crate::secret_storage::SecretStoreError) -> Self {
        use crate::secret_storage::SecretStoreError::*;
        match e {
            Crypto(m) | SigningNotSupported(m) | Init(m) | NotFound(m) => {
                StoreError::Crypto(m)
            }
            InvalidInput(m) | UnsupportedCurve(m) => StoreError::Corruption(m),
            Io(e) => StoreError::Backend(format!("io: {e}")),
            Json(e) => StoreError::Codec(format!("json: {e}")),
            Locked => StoreError::Crypto("store locked".into()),
            VerificationFailed => StoreError::Crypto("verification failed".into()),
        }
    }
}
