//! Unshielded UTXO sync — snapshot-on-demand. See
//! `docs/superpowers/specs/2026-05-14-unshielded-sync-design.md`.
//!
//! Public entry point is `crate::Wallet::sync_unshielded()`. This
//! module exposes the result types; internals live in submodules
//! (`snapshot`, `transport`).

pub(crate) mod snapshot;
pub(crate) mod transport;

use std::collections::HashMap;

/// Hex-encoded serialized token type from the indexer. Opaque for
/// this slice — subsystem B matches against a known NIGHT constant
/// when it composes a fee-balanced transaction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenType(pub Vec<u8>);

/// `(intent_hash, output_index)` — the indexer's natural UTXO key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UtxoId {
    pub intent_hash: [u8; 32],
    pub output_index: u32,
}

/// One live unshielded UTXO. Field shape mirrors the indexer's
/// `UnshieldedUtxo` graphql type, with strings parsed to native.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnshieldedUtxo {
    /// Bech32m `mn_addr_*` (or equivalent HRP) address.
    pub owner: String,
    pub token_type: TokenType,
    /// Parsed from the indexer's `String` (u128-as-decimal).
    pub value: u128,
    pub id: UtxoId,
    /// Unix seconds at creation; `None` for genesis-ish UTXOs.
    pub ctime: Option<u64>,
    /// Used for DUST generation tracking. Opaque to this slice.
    pub initial_nonce: [u8; 32],
}

/// Live UTXO set produced by one snapshot call. Keyed by `UtxoId`
/// so the snapshot loop can apply `spentUtxos` events in O(1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UtxoSet {
    utxos: HashMap<UtxoId, UnshieldedUtxo>,
}

impl UtxoSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or overwrite) one UTXO.
    pub(crate) fn insert(&mut self, u: UnshieldedUtxo) {
        self.utxos.insert(u.id, u);
    }

    /// Remove a UTXO by id (no-op if absent — matches the
    /// indexer's at-least-once guarantee for spends).
    pub(crate) fn remove(&mut self, id: &UtxoId) {
        self.utxos.remove(id);
    }

    pub fn len(&self) -> usize {
        self.utxos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.utxos.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &UnshieldedUtxo> {
        self.utxos.values()
    }

    pub fn get(&self, id: &UtxoId) -> Option<&UnshieldedUtxo> {
        self.utxos.get(id)
    }

    /// Sum values per token type.
    pub fn balance_by_token(&self) -> HashMap<TokenType, u128> {
        let mut out: HashMap<TokenType, u128> = HashMap::new();
        for u in self.utxos.values() {
            *out.entry(u.token_type.clone()).or_default() += u.value;
        }
        out
    }

    /// Sum for one token. 0 if absent.
    pub fn total_for(&self, token: &TokenType) -> u128 {
        self.utxos
            .values()
            .filter(|u| &u.token_type == token)
            .map(|u| u.value)
            .sum()
    }

    /// Greedy selection: sort UTXOs of the given token by value
    /// descending, take until the cumulative sum covers `amount`.
    /// Returns `None` if the total for `token` is insufficient.
    /// **No** change-minimisation; **no** fragmentation logic —
    /// explicit non-goal for this slice.
    pub fn pick_for_amount(
        &self,
        token: &TokenType,
        amount: u128,
    ) -> Option<Vec<&UnshieldedUtxo>> {
        if amount == 0 {
            return Some(Vec::new());
        }
        let mut candidates: Vec<&UnshieldedUtxo> = self
            .utxos
            .values()
            .filter(|u| &u.token_type == token)
            .collect();
        candidates.sort_by_key(|u| std::cmp::Reverse(u.value));

        let mut picked = Vec::new();
        let mut sum: u128 = 0;
        for u in candidates {
            picked.push(u);
            sum = sum.saturating_add(u.value);
            if sum >= amount {
                return Some(picked);
            }
        }
        None
    }
}

/// All failure modes for `Wallet::sync_unshielded()`.
#[derive(Debug, thiserror::Error)]
pub enum UnshieldedError {
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
    #[error("stream closed before Progress event")]
    StreamClosedEarly,
    #[error("invalid unshielded address: {0}")]
    InvalidAddress(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utxo(intent: u8, idx: u32, token: u8, value: u128) -> UnshieldedUtxo {
        UnshieldedUtxo {
            owner: "mn_addr_test1abcd".to_string(),
            token_type: TokenType(vec![token]),
            value,
            id: UtxoId {
                intent_hash: [intent; 32],
                output_index: idx,
            },
            ctime: Some(1_700_000_000),
            initial_nonce: [0u8; 32],
        }
    }

    #[test]
    fn empty_set_is_empty() {
        let s = UtxoSet::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert!(s.balance_by_token().is_empty());
    }

    #[test]
    fn insert_and_remove() {
        let mut s = UtxoSet::new();
        let u = utxo(1, 0, 0xAB, 100);
        let id = u.id;
        s.insert(u);
        assert_eq!(s.len(), 1);
        s.remove(&id);
        assert!(s.is_empty());
    }

    #[test]
    fn remove_missing_is_noop() {
        let mut s = UtxoSet::new();
        s.remove(&UtxoId { intent_hash: [9; 32], output_index: 0 });
        assert!(s.is_empty());
    }

    #[test]
    fn balance_groups_by_token() {
        let mut s = UtxoSet::new();
        s.insert(utxo(1, 0, 0xAB, 100));
        s.insert(utxo(2, 0, 0xAB, 50));
        s.insert(utxo(3, 0, 0xCD, 200));
        let bals = s.balance_by_token();
        assert_eq!(bals.get(&TokenType(vec![0xAB])), Some(&150));
        assert_eq!(bals.get(&TokenType(vec![0xCD])), Some(&200));
    }

    #[test]
    fn total_for_unknown_token_is_zero() {
        let s = UtxoSet::new();
        assert_eq!(s.total_for(&TokenType(vec![0x77])), 0);
    }

    #[test]
    fn pick_for_zero_returns_empty() {
        let s = UtxoSet::new();
        let picked = s.pick_for_amount(&TokenType(vec![0xAB]), 0).unwrap();
        assert!(picked.is_empty());
    }

    #[test]
    fn pick_for_insufficient_balance_returns_none() {
        let mut s = UtxoSet::new();
        s.insert(utxo(1, 0, 0xAB, 100));
        assert!(s.pick_for_amount(&TokenType(vec![0xAB]), 1000).is_none());
    }

    #[test]
    fn pick_picks_largest_first() {
        let mut s = UtxoSet::new();
        s.insert(utxo(1, 0, 0xAB, 100));
        s.insert(utxo(2, 0, 0xAB, 50));
        s.insert(utxo(3, 0, 0xAB, 25));
        let picked = s.pick_for_amount(&TokenType(vec![0xAB]), 120).unwrap();
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].value, 100);
        assert_eq!(picked[1].value, 50);
    }

    #[test]
    fn pick_ignores_other_tokens() {
        let mut s = UtxoSet::new();
        s.insert(utxo(1, 0, 0xAB, 100));
        s.insert(utxo(2, 0, 0xCD, 1_000_000));
        assert!(s.pick_for_amount(&TokenType(vec![0xAB]), 1000).is_none());
    }
}
