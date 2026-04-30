use crate::did::id::DidIdError;

/// Top-level DID error. Wraps `DidIdError` for parsing failures plus
/// the various IO failures we'll hit once the resolver lands.
#[derive(Debug, thiserror::Error)]
pub enum DidError {
    #[error("invalid DID: {0}")]
    InvalidId(#[from] DidIdError),
    #[error("indexer: {0}")]
    Indexer(String),
    #[error("contract state decode: {0}")]
    DecodeState(String),
    /// Phase 1 stub. Replaced once `Wallet::resolve_did` actually
    /// hits the indexer.
    #[error("resolver not yet implemented (Phase 2 of DID_PLAN)")]
    ResolverNotImplemented,
    /// Phase 1 stub. Replaced when `Wallet::create_did` lands.
    #[error("contract write not yet implemented (Phase 3 of DID_PLAN)")]
    WriteNotImplemented,
}
