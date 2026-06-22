//! Shielded (zswap) note sync. Hydrates a `zswap::local::State` by
//! replaying the indexer's `zswapLedgerEvents` stream and decrypting
//! the wallet's own coins, so the deposit balancer
//! ([`crate::tx::balance`]) has spendable `QualifiedShieldedCoinInfo`s
//! plus a live commitment Merkle tree to build spend proofs from.
//!
//! Mirrors the DUST sync ([`crate::dust`]): the same
//! `graphql-transport-ws` transport + an incremental, redb-persisted
//! checkpoint (`last_id`). Unlike DUST there is no viewing-key/session
//! handshake — we replay the chain-wide `zswapLedgerEvents` and filter
//! locally with the wallet's `zswap::keys::SecretKeys`, so every coin
//! commitment lands in the tree in index order (no gaps) and owned
//! coins are kept while others are collapsed.

pub(crate) mod snapshot;
pub(crate) mod syncer;

pub use syncer::{ShieldedSyncer, ShieldedSyncProgress, SpendableCoin, spendable_coins};

#[derive(Debug, thiserror::Error)]
pub enum ShieldedError {
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
    #[error("replay events: {0}")]
    Replay(String),
    #[error("store: {0}")]
    Store(String),
}
