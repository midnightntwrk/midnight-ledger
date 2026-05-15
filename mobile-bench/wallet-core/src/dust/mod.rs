//! DUST UTXO sync. Hydrates a `ledger::dust::DustLocalState` by
//! replaying the indexer's `dustLedgerEvents` stream into it via
//! `DustLocalState::replay_events`.
//!
//! Public entry point is `crate::Wallet::sync_dust()`. The
//! returned state is consumed by the fee balancer in
//! `crate::tx::balance`.

pub(crate) mod snapshot;

#[derive(Debug, thiserror::Error)]
pub enum DustError {
    #[error("ws connect failed: {0}")]
    WsConnect(String),
    #[error("graphql-transport-ws handshake failed: {0}")]
    WsHandshake(String),
    #[error("graphql error frame: {0}")]
    GqlError(String),
    #[error("unexpected ws frame: {0}")]
    UnexpectedFrame(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("stream closed before final progress event")]
    StreamClosedEarly,
    #[error("invalid dust public key: {0}")]
    InvalidPublicKey(String),
    #[error("replay events: {0}")]
    Replay(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies DustError carries enough context to render. The
    /// real DUST-state ops live on `ledger::dust::DustLocalState`
    /// (re-exported via lib.rs) — no point retesting them here.
    #[test]
    fn error_variants_format() {
        let e = DustError::WsConnect("boom".into());
        assert!(format!("{e}").contains("boom"));
    }
}
