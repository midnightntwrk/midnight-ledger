# Unshielded Sync (Subsystem A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Wallet::sync_unshielded() -> Result<UtxoSet, UnshieldedError>` — a snapshot-on-demand sync of the wallet's default unshielded address via a graphql-transport-ws subscription to the Midnight indexer — plus a Dioxus `BalancePanel`, a CLI example, and a live integration test.

**Architecture:** A new `wallet_core::unshielded` subdirectory module (`mod.rs` / `snapshot.rs` / `transport.rs`). Each call opens a fresh WebSocket, replays `UnshieldedTransaction` events into an in-memory `UtxoSet`, terminates on the first `UnshieldedTransactionsProgress` event, closes the WS. Stateless, no persistence, no streaming.

**Tech Stack:** Rust 2024 edition · `tokio` async runtime · `tokio-tungstenite` for WebSockets · `serde_json` for graphql-transport-ws framing · `futures` for `Stream` traits · `bech32` (already vendored) for address validation · Dioxus 0.6 for the UI panel.

**Spec:** `docs/superpowers/specs/2026-05-14-unshielded-sync-design.md`.

**Repository conventions to respect:**
- `mobile-bench/wallet-core/src/lib.rs` has `#![deny(warnings)]`. Every file must compile clean — no unused imports, no dead code.
- Re-exports go through `lib.rs` (see existing pattern for `pub use indexer::{…}`).
- `pub(crate)` for internal helpers; `pub` only at re-export boundaries.
- Tests live under `#[cfg(test)] mod tests { … }` at the bottom of each module (see `wallet-core/src/did/deploy.rs` for the pattern).
- All commits MUST use `git commit -S -s -m "…"` (GPG sign + DCO sign-off). After every commit, verify the signature with `git log --format="%h %G? %s" -1` — `G` = good, `B`/`N` = bad. Only on `B`/`N` re-sign with `git commit --amend --no-edit -S`. Never amend otherwise.
- Run `bash ~/iohk/git-iohk.sh` once at the start of the session to set per-repo `user.name`/`user.email`/`user.signingkey`.

---

## File Structure

| Path | Role | Status |
|---|---|---|
| `mobile-bench/wallet-core/src/unshielded/mod.rs` | Public types: `TokenType`, `UtxoId`, `UnshieldedUtxo`, `UtxoSet`, `UnshieldedError`; `UtxoSet` methods. | **Create (Task 1)** |
| `mobile-bench/wallet-core/src/unshielded/snapshot.rs` | `snapshot()` driver + internal `Event` enum + `decode_event` JSON parser. | **Create (Task 2 + 4)** |
| `mobile-bench/wallet-core/src/unshielded/transport.rs` | Minimal `graphql-transport-ws` client (`connection_init` / `subscribe` / `next` / `complete` framing). | **Create (Task 3)** |
| `mobile-bench/wallet-core/queries/midnight-indexer/unshielded_transactions.subscription.graphql` | Subscription document (string-only — not graphql_client codegen, since we drive the WS by hand). | **Create (Task 2)** |
| `mobile-bench/wallet-core/src/wallet.rs` | Add `Wallet::sync_unshielded()`. | **Modify (Task 5)** |
| `mobile-bench/wallet-core/src/lib.rs` | `mod unshielded;` + `pub use unshielded::{…}`. | **Modify (Task 5)** |
| `mobile-bench/wallet-core/examples/sync_unshielded.rs` | CLI: `cargo run --example sync_unshielded -- preprod`. | **Create (Task 6)** |
| `mobile-bench/dioxus-wallet/src/app.rs` | Add `BalancePanel` component, mount it next to `CreateDidPanel`. | **Modify (Task 7)** |
| `mobile-bench/wallet-core/tests/unshielded_live.rs` | Live integration test gated by `#[cfg(feature = "network-tests")]`. | **Create (Task 8)** |

---

### Task 1: Public types and `UtxoSet` operations

**Files:**
- Create: `mobile-bench/wallet-core/src/unshielded/mod.rs`
- Test: `mobile-bench/wallet-core/src/unshielded/mod.rs` (same file, `#[cfg(test)] mod tests`)

- [ ] **Step 1.1: Create the module file with type definitions**

Create `mobile-bench/wallet-core/src/unshielded/mod.rs`:

```rust
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
        candidates.sort_by(|a, b| b.value.cmp(&a.value));

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
```

- [ ] **Step 1.2: Add unit tests for `UtxoSet`**

Append to the same file (above `pub(crate) mod snapshot;` won't work; the tests go at the bottom of the file):

```rust
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
```

- [ ] **Step 1.3: Wire the module into the crate without breaking compile**

Append to `mobile-bench/wallet-core/src/lib.rs` in two places.

First, in the `mod` declarations block (currently `mod address; … mod wallet;`), add (alphabetic insertion right after `mod probe;`):

```rust
mod unshielded;
```

Then add a new `pub use` block at the end of the existing re-export section:

```rust
pub use unshielded::{
    TokenType, UnshieldedError, UnshieldedUtxo, UtxoId, UtxoSet,
};
```

The `pub(crate) mod snapshot;` and `pub(crate) mod transport;` lines inside `unshielded/mod.rs` will fail to resolve until those files exist — fix in Step 1.4.

- [ ] **Step 1.4: Create empty `snapshot.rs` and `transport.rs` so the module tree resolves**

Create `mobile-bench/wallet-core/src/unshielded/snapshot.rs`:

```rust
//! Snapshot driver — filled in by Task 4.
```

Create `mobile-bench/wallet-core/src/unshielded/transport.rs`:

```rust
//! graphql-transport-ws client — filled in by Task 3.
```

- [ ] **Step 1.5: Run unit tests + check warnings-clean compile**

```
cargo test -p wallet-core --lib unshielded::tests
cargo check -p wallet-core
```

Expected: 9 tests pass (`empty_set_is_empty`, `insert_and_remove`, `remove_missing_is_noop`, `balance_groups_by_token`, `total_for_unknown_token_is_zero`, `pick_for_zero_returns_empty`, `pick_for_insufficient_balance_returns_none`, `pick_picks_largest_first`, `pick_ignores_other_tokens`). `cargo check` clean — no warnings.

- [ ] **Step 1.6: Commit**

```bash
bash ~/iohk/git-iohk.sh
git add mobile-bench/wallet-core/src/unshielded/ mobile-bench/wallet-core/src/lib.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): unshielded sync — public types + UtxoSet ops

Subsystem A of the DID CRUD slice. Adds the wallet_core::unshielded
module with the snapshot output types (UnshieldedUtxo, UtxoSet,
UtxoId, TokenType) and the error enum. UtxoSet exposes insert/remove
(crate-internal, used by the snapshot driver) and the read-side
helpers balance_by_token / total_for / pick_for_amount that
subsystem B will consume.

pick_for_amount is intentionally greedy — sort by value descending,
take until covered — with no change-minimisation. Documented as a
non-goal in the design doc; subsystem B can swap in something
smarter when it composes a fee-balanced transaction.

Submodules `snapshot` and `transport` exist as empty stubs and
will be filled in by subsequent tasks.

Spec: docs/superpowers/specs/2026-05-14-unshielded-sync-design.md

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G` (good signature). On `B`/`N`, amend once with `git commit --amend --no-edit -S` and verify again.

---

### Task 2: Subscription document + `Event` decoder

**Files:**
- Create: `mobile-bench/wallet-core/queries/midnight-indexer/unshielded_transactions.subscription.graphql`
- Modify: `mobile-bench/wallet-core/src/unshielded/snapshot.rs` (replace stub)
- Test: same file

- [ ] **Step 2.1: Write the subscription document**

Create `mobile-bench/wallet-core/queries/midnight-indexer/unshielded_transactions.subscription.graphql`:

```graphql
subscription UnshieldedTransactions($address: UnshieldedAddress!, $transactionId: Int) {
  unshieldedTransactions(address: $address, transactionId: $transactionId) {
    __typename
    ... on UnshieldedTransaction {
      createdUtxos {
        owner
        tokenType
        value
        intentHash
        outputIndex
        ctime
        initialNonce
      }
      spentUtxos {
        intentHash
        outputIndex
      }
    }
    ... on UnshieldedTransactionsProgress {
      highestTransactionId
    }
  }
}
```

- [ ] **Step 2.2: Add `Event` enum + `decode_event` to `snapshot.rs`**

Replace `mobile-bench/wallet-core/src/unshielded/snapshot.rs` with:

```rust
//! Snapshot driver: open a graphql-transport-ws subscription,
//! replay create/spend events into a `UtxoSet`, terminate on the
//! first `UnshieldedTransactionsProgress` event.

use serde_json::Value;

use super::{TokenType, UnshieldedError, UnshieldedUtxo, UtxoId};

/// The subscription document, embedded at compile time. We don't
/// run graphql_client codegen for subscriptions — the WS protocol
/// is hand-rolled (`transport.rs`), and the response shape is
/// narrow enough to decode by walking serde_json::Value.
pub(super) const UNSHIELDED_TRANSACTIONS_QUERY: &str = include_str!(
    "../../queries/midnight-indexer/unshielded_transactions.subscription.graphql"
);

/// One decoded element of the subscription stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Event {
    Transaction {
        created: Vec<UnshieldedUtxo>,
        spent: Vec<UtxoId>,
    },
    /// The indexer's "you're caught up" signal. The carried
    /// `highest_transaction_id` is informational — we terminate on
    /// any Progress event regardless of value.
    Progress {
        highest_transaction_id: i64,
    },
}

/// Decode one `next.payload.data.unshieldedTransactions` JSON value.
pub(super) fn decode_event(data: &Value) -> Result<Event, UnshieldedError> {
    let obj = data
        .get("unshieldedTransactions")
        .ok_or_else(|| UnshieldedError::Decode("missing unshieldedTransactions".into()))?;
    let typename = obj
        .get("__typename")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("missing __typename".into()))?;
    match typename {
        "UnshieldedTransaction" => {
            let created_raw = obj
                .get("createdUtxos")
                .and_then(Value::as_array)
                .ok_or_else(|| UnshieldedError::Decode("missing createdUtxos".into()))?;
            let spent_raw = obj
                .get("spentUtxos")
                .and_then(Value::as_array)
                .ok_or_else(|| UnshieldedError::Decode("missing spentUtxos".into()))?;
            let created = created_raw
                .iter()
                .map(decode_utxo)
                .collect::<Result<Vec<_>, _>>()?;
            let spent = spent_raw
                .iter()
                .map(decode_utxo_id)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Event::Transaction { created, spent })
        }
        "UnshieldedTransactionsProgress" => {
            let high = obj
                .get("highestTransactionId")
                .and_then(Value::as_i64)
                .ok_or_else(|| UnshieldedError::Decode("missing highestTransactionId".into()))?;
            Ok(Event::Progress {
                highest_transaction_id: high,
            })
        }
        other => Err(UnshieldedError::UnexpectedFrame(format!(
            "__typename={other}"
        ))),
    }
}

fn decode_utxo(v: &Value) -> Result<UnshieldedUtxo, UnshieldedError> {
    let owner = v
        .get("owner")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("utxo.owner".into()))?
        .to_string();
    let token_hex = v
        .get("tokenType")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("utxo.tokenType".into()))?;
    let token_bytes = hex::decode(token_hex.trim_start_matches("0x"))
        .map_err(|e| UnshieldedError::Decode(format!("utxo.tokenType: {e}")))?;
    let value_str = v
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("utxo.value".into()))?;
    let value: u128 = value_str
        .parse()
        .map_err(|e| UnshieldedError::Decode(format!("utxo.value: {e}")))?;
    let intent_hash = decode_intent_hash(v.get("intentHash"))?;
    let output_index = v
        .get("outputIndex")
        .and_then(Value::as_i64)
        .ok_or_else(|| UnshieldedError::Decode("utxo.outputIndex".into()))?;
    if !(0..=u32::MAX as i64).contains(&output_index) {
        return Err(UnshieldedError::Decode(format!(
            "utxo.outputIndex out of u32 range: {output_index}"
        )));
    }
    let ctime = v.get("ctime").and_then(Value::as_i64).and_then(|s| {
        if s >= 0 { Some(s as u64) } else { None }
    });
    let nonce_hex = v
        .get("initialNonce")
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("utxo.initialNonce".into()))?;
    let nonce_bytes = hex::decode(nonce_hex.trim_start_matches("0x"))
        .map_err(|e| UnshieldedError::Decode(format!("utxo.initialNonce: {e}")))?;
    let mut initial_nonce = [0u8; 32];
    if nonce_bytes.len() != 32 {
        return Err(UnshieldedError::Decode(format!(
            "utxo.initialNonce: expected 32 bytes, got {}",
            nonce_bytes.len()
        )));
    }
    initial_nonce.copy_from_slice(&nonce_bytes);

    Ok(UnshieldedUtxo {
        owner,
        token_type: TokenType(token_bytes),
        value,
        id: UtxoId {
            intent_hash,
            output_index: output_index as u32,
        },
        ctime,
        initial_nonce,
    })
}

fn decode_utxo_id(v: &Value) -> Result<UtxoId, UnshieldedError> {
    let intent_hash = decode_intent_hash(v.get("intentHash"))?;
    let output_index = v
        .get("outputIndex")
        .and_then(Value::as_i64)
        .ok_or_else(|| UnshieldedError::Decode("spent.outputIndex".into()))?;
    if !(0..=u32::MAX as i64).contains(&output_index) {
        return Err(UnshieldedError::Decode(format!(
            "spent.outputIndex out of u32 range: {output_index}"
        )));
    }
    Ok(UtxoId {
        intent_hash,
        output_index: output_index as u32,
    })
}

fn decode_intent_hash(v: Option<&Value>) -> Result<[u8; 32], UnshieldedError> {
    let hex_str = v
        .and_then(Value::as_str)
        .ok_or_else(|| UnshieldedError::Decode("intentHash".into()))?;
    let bytes = hex::decode(hex_str.trim_start_matches("0x"))
        .map_err(|e| UnshieldedError::Decode(format!("intentHash: {e}")))?;
    if bytes.len() != 32 {
        return Err(UnshieldedError::Decode(format!(
            "intentHash: expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}
```

- [ ] **Step 2.3: Add unit tests for `decode_event` against canned JSON**

Append to `snapshot.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn intent_hex(b: u8) -> String {
        hex::encode([b; 32])
    }
    fn nonce_hex(b: u8) -> String {
        hex::encode([b; 32])
    }

    #[test]
    fn decode_progress_event() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransactionsProgress",
                "highestTransactionId": 42
            }
        });
        let ev = decode_event(&data).expect("decode");
        assert_eq!(
            ev,
            Event::Progress { highest_transaction_id: 42 }
        );
    }

    #[test]
    fn decode_transaction_with_created_and_spent() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "createdUtxos": [{
                    "owner": "mn_addr_test1abcd",
                    "tokenType": hex::encode([0xAB]),
                    "value": "1000000",
                    "intentHash": intent_hex(0x11),
                    "outputIndex": 0,
                    "ctime": 1_700_000_000,
                    "initialNonce": nonce_hex(0x22)
                }],
                "spentUtxos": [{
                    "intentHash": intent_hex(0x33),
                    "outputIndex": 1
                }]
            }
        });
        let ev = decode_event(&data).expect("decode");
        match ev {
            Event::Transaction { created, spent } => {
                assert_eq!(created.len(), 1);
                assert_eq!(created[0].value, 1_000_000);
                assert_eq!(created[0].id.output_index, 0);
                assert_eq!(created[0].id.intent_hash, [0x11; 32]);
                assert_eq!(created[0].initial_nonce, [0x22; 32]);
                assert_eq!(created[0].token_type.0, vec![0xAB]);
                assert_eq!(spent.len(), 1);
                assert_eq!(spent[0].intent_hash, [0x33; 32]);
                assert_eq!(spent[0].output_index, 1);
            }
            other => panic!("expected Transaction, got {other:?}"),
        }
    }

    #[test]
    fn decode_transaction_with_empty_arrays() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "createdUtxos": [],
                "spentUtxos": []
            }
        });
        let ev = decode_event(&data).expect("decode");
        match ev {
            Event::Transaction { created, spent } => {
                assert!(created.is_empty());
                assert!(spent.is_empty());
            }
            other => panic!("expected Transaction, got {other:?}"),
        }
    }

    #[test]
    fn decode_unknown_typename_errors() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "SomethingElse"
            }
        });
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, UnshieldedError::UnexpectedFrame(_)));
    }

    #[test]
    fn decode_missing_root_errors() {
        let data = json!({});
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, UnshieldedError::Decode(_)));
    }

    #[test]
    fn decode_bad_intent_hash_length() {
        let data = json!({
            "unshieldedTransactions": {
                "__typename": "UnshieldedTransaction",
                "createdUtxos": [],
                "spentUtxos": [{
                    "intentHash": hex::encode([0x44; 16]),
                    "outputIndex": 0
                }]
            }
        });
        let err = decode_event(&data).unwrap_err();
        assert!(matches!(err, UnshieldedError::Decode(_)));
    }
}
```

- [ ] **Step 2.4: Run unit tests + warnings-clean check**

```
cargo test -p wallet-core --lib unshielded::snapshot::tests
cargo check -p wallet-core
```

Expected: 6 tests pass. No warnings.

- [ ] **Step 2.5: Commit**

```bash
git add mobile-bench/wallet-core/queries/midnight-indexer/unshielded_transactions.subscription.graphql mobile-bench/wallet-core/src/unshielded/snapshot.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): unshielded sync — subscription doc + Event decoder

Adds the unshieldedTransactions subscription document and the
serde_json walker that turns a `next.payload.data` JSON value into
either an `Event::Transaction { created, spent }` or
`Event::Progress { highest_transaction_id }`. Strict on field
shapes: 32-byte intent hashes and initial nonces, u32-bounded
output indices, u128-parsed values.

We hand-roll the decode instead of using graphql_client's
subscription codegen because the WS protocol is also hand-rolled
(transport.rs lands next), and graphql_client doesn't drive
graphql-transport-ws natively.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 3: graphql-transport-ws client (`transport.rs`)

**Files:**
- Modify: `mobile-bench/wallet-core/src/unshielded/transport.rs` (replace stub)
- Test: same file

The graphql-transport-ws protocol (the modern one, as opposed to legacy `graphql-ws`):
1. WS upgrade with `Sec-WebSocket-Protocol: graphql-transport-ws`.
2. Client → `{"type": "connection_init", "payload": {}}`.
3. Server → `{"type": "connection_ack"}` (may also send `{"type": "ping"}` / `{"type": "pong"}` keep-alives that we must tolerate).
4. Client → `{"type": "subscribe", "id": "1", "payload": {"query": "…", "variables": {…}}}`.
5. Server → repeated `{"type": "next", "id": "1", "payload": {"data": {…}}}`.
6. Server → `{"type": "complete", "id": "1"}` (or error frame), at which point the caller's stream ends.

- [ ] **Step 3.1: Add the framing helpers + `subscribe()` driver**

Replace `mobile-bench/wallet-core/src/unshielded/transport.rs` with:

```rust
//! Minimal graphql-transport-ws client. One subscription per WS, no
//! multiplexing. Callers receive an async `Stream<Result<Value, _>>`
//! of `next.payload.data` JSON values; framing is hidden inside.

use std::time::Duration;

use futures::{Stream, StreamExt, SinkExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, protocol::CloseFrame, protocol::frame::coding::CloseCode},
};

use super::UnshieldedError;

/// Subscribe over a fresh graphql-transport-ws WebSocket. Returns
/// an async stream of `next.payload.data` JSON values, ending when
/// the server sends `complete`/`error` or when the WS drops.
pub(super) async fn subscribe(
    ws_url: &str,
    query: &str,
    variables: Value,
) -> Result<impl Stream<Item = Result<Value, UnshieldedError>>, UnshieldedError> {
    let mut req = ws_url
        .into_client_request()
        .map_err(|e| UnshieldedError::WsConnect(e.to_string()))?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "graphql-transport-ws"
            .parse()
            .expect("static subprotocol header parses"),
    );

    let (mut ws, _resp) = tokio::time::timeout(Duration::from_secs(15), connect_async(req))
        .await
        .map_err(|_| UnshieldedError::WsConnect("connect timeout".into()))?
        .map_err(|e| UnshieldedError::WsConnect(e.to_string()))?;

    // Handshake: connection_init -> connection_ack
    let init = json!({ "type": "connection_init", "payload": {} }).to_string();
    ws.send(Message::Text(init))
        .await
        .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;

    loop {
        let frame = tokio::time::timeout(Duration::from_secs(15), ws.next())
            .await
            .map_err(|_| UnshieldedError::WsHandshake("ack timeout".into()))?
            .ok_or_else(|| UnshieldedError::WsHandshake("ws closed before ack".into()))?
            .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;
        match parse_text_frame(&frame)? {
            Some(v) => match frame_type(&v) {
                Some("connection_ack") => break,
                Some("ping") => {
                    let pong = json!({ "type": "pong" }).to_string();
                    ws.send(Message::Text(pong))
                        .await
                        .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;
                }
                Some(t) => {
                    return Err(UnshieldedError::WsHandshake(format!(
                        "unexpected pre-ack frame type {t}"
                    )));
                }
                None => {
                    return Err(UnshieldedError::WsHandshake("frame missing type".into()));
                }
            },
            None => continue,
        }
    }

    // subscribe frame
    let sub = json!({
        "type": "subscribe",
        "id": "1",
        "payload": { "query": query, "variables": variables },
    })
    .to_string();
    ws.send(Message::Text(sub))
        .await
        .map_err(|e| UnshieldedError::WsHandshake(e.to_string()))?;

    Ok(into_data_stream(ws))
}

fn into_data_stream(
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> impl Stream<Item = Result<Value, UnshieldedError>> {
    async_stream::stream! {
        let mut ws = ws;
        loop {
            let item = match ws.next().await {
                None => break,
                Some(Err(e)) => {
                    yield Err(UnshieldedError::WsConnect(e.to_string()));
                    break;
                }
                Some(Ok(m)) => m,
            };
            match parse_text_frame(&item) {
                Err(e) => {
                    yield Err(e);
                    break;
                }
                Ok(None) => continue,
                Ok(Some(v)) => match frame_type(&v) {
                    Some("next") => {
                        match v.get("payload").and_then(|p| p.get("data")) {
                            Some(data) => yield Ok(data.clone()),
                            None => {
                                yield Err(UnshieldedError::Decode(
                                    "next frame missing payload.data".into(),
                                ));
                                break;
                            }
                        }
                    }
                    Some("error") => {
                        let payload = v.get("payload").map(|p| p.to_string())
                            .unwrap_or_else(|| "<no payload>".into());
                        yield Err(UnshieldedError::GqlError(payload));
                        break;
                    }
                    Some("complete") => break,
                    Some("ping") => {
                        let pong = json!({ "type": "pong" }).to_string();
                        if let Err(e) = ws.send(Message::Text(pong)).await {
                            yield Err(UnshieldedError::WsConnect(e.to_string()));
                            break;
                        }
                    }
                    Some("pong") | Some("connection_ack") => continue,
                    other => {
                        let _ = ws.close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: "unexpected frame".into(),
                        })).await;
                        yield Err(UnshieldedError::UnexpectedFrame(
                            format!("type={other:?}"),
                        ));
                        break;
                    }
                },
            }
        }
        let _ = ws.close(None).await;
    }
}

/// Returns `Ok(Some(v))` for parsed text frames, `Ok(None)` for
/// non-text we ignore (binary, pong, close), or `Err` on bad JSON.
pub(super) fn parse_text_frame(m: &Message) -> Result<Option<Value>, UnshieldedError> {
    match m {
        Message::Text(s) => {
            let v: Value = serde_json::from_str(s)
                .map_err(|e| UnshieldedError::UnexpectedFrame(format!("bad json: {e}")))?;
            Ok(Some(v))
        }
        Message::Binary(_) | Message::Pong(_) | Message::Ping(_) | Message::Frame(_) => Ok(None),
        Message::Close(_) => Ok(None),
    }
}

pub(super) fn frame_type(v: &Value) -> Option<&str> {
    v.get("type").and_then(Value::as_str)
}

/// Render the `connection_init` payload as wire text. Pulled out
/// for unit tests.
#[cfg(test)]
pub(super) fn connection_init_frame() -> String {
    json!({ "type": "connection_init", "payload": {} }).to_string()
}

/// Render the `subscribe` payload as wire text. Pulled out
/// for unit tests.
#[cfg(test)]
pub(super) fn subscribe_frame(query: &str, variables: Value) -> String {
    json!({
        "type": "subscribe",
        "id": "1",
        "payload": { "query": query, "variables": variables },
    })
    .to_string()
}
```

`async_stream` isn't currently in `Cargo.toml` — add it next.

- [ ] **Step 3.2: Add `async-stream` to `wallet-core/Cargo.toml`**

In `mobile-bench/wallet-core/Cargo.toml`, in the `[dependencies]` section, add (alphabetical near other async crates):

```toml
async-stream = "0.3"
```

- [ ] **Step 3.3: Add unit tests for framing**

Append to `transport.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn connection_init_frame_shape() {
        let s = connection_init_frame();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("type").and_then(Value::as_str), Some("connection_init"));
        assert!(v.get("payload").is_some());
    }

    #[test]
    fn subscribe_frame_shape() {
        let s = subscribe_frame("subscription X { x }", json!({"a": 1}));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("type").and_then(Value::as_str), Some("subscribe"));
        assert_eq!(v.get("id").and_then(Value::as_str), Some("1"));
        let payload = v.get("payload").unwrap();
        assert_eq!(
            payload.get("query").and_then(Value::as_str),
            Some("subscription X { x }")
        );
        assert_eq!(payload.get("variables").unwrap(), &json!({"a": 1}));
    }

    #[test]
    fn parse_next_frame() {
        let raw = json!({
            "type": "next",
            "id": "1",
            "payload": { "data": { "foo": 42 } }
        })
        .to_string();
        let m = Message::Text(raw);
        let v = parse_text_frame(&m).unwrap().unwrap();
        assert_eq!(frame_type(&v), Some("next"));
    }

    #[test]
    fn parse_complete_frame() {
        let raw = json!({"type": "complete", "id": "1"}).to_string();
        let v = parse_text_frame(&Message::Text(raw)).unwrap().unwrap();
        assert_eq!(frame_type(&v), Some("complete"));
    }

    #[test]
    fn parse_binary_returns_none() {
        let m = Message::Binary(vec![1, 2, 3]);
        assert!(parse_text_frame(&m).unwrap().is_none());
    }

    #[test]
    fn parse_bad_json_errors() {
        let m = Message::Text("not json".into());
        let err = parse_text_frame(&m).unwrap_err();
        assert!(matches!(err, UnshieldedError::UnexpectedFrame(_)));
    }
}
```

- [ ] **Step 3.4: Run unit tests + warnings-clean check**

```
cargo test -p wallet-core --lib unshielded::transport::tests
cargo check -p wallet-core
```

Expected: 6 tests pass. No warnings.

- [ ] **Step 3.5: Commit**

```bash
git add mobile-bench/wallet-core/Cargo.toml mobile-bench/wallet-core/src/unshielded/transport.rs Cargo.lock
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): unshielded sync — graphql-transport-ws client

Minimal hand-rolled client for the modern graphql-transport-ws
protocol. One subscription per WebSocket, no multiplexing. Drives
the connection_init / connection_ack handshake, sends the subscribe
frame, and surfaces `next.payload.data` JSON values as an async
`Stream<Result<Value, UnshieldedError>>`.

Tolerates server ping frames (sends pong) and ignores binary /
pong / close frames at the parse layer. Closes the WS cleanly when
the caller drops the stream or the server sends `complete`.

Adds `async-stream` for the `Stream` adapter.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 4: `snapshot()` driver

**Files:**
- Modify: `mobile-bench/wallet-core/src/unshielded/snapshot.rs` (append `snapshot()`)
- Test: same file

- [ ] **Step 4.1: Add `snapshot()` plus a stream-shaped internal helper that's testable without WS**

At the top of `mobile-bench/wallet-core/src/unshielded/snapshot.rs` (right after the `use` block and the existing `UNSHIELDED_TRANSACTIONS_QUERY` const), add the new imports and types. Replace the entire file structure so we have these blocks in order: imports → const → Event enum → fold_events helper → snapshot driver → decoders → tests.

Add these new imports at the top:

```rust
use futures::{Stream, StreamExt};
use serde_json::json;

use super::{UtxoSet, transport};
```

Add this helper function right after the `decode_event` function (above `fn decode_utxo`):

```rust
/// Fold an event stream into a `UtxoSet`, stopping on the first
/// `Progress` event. Returns `StreamClosedEarly` if the stream
/// ends before a `Progress` event arrives.
///
/// Pulled out so the folding/termination logic can be unit-tested
/// against a hand-built `Stream<Event>` without a live WS.
pub(super) async fn fold_events<S>(stream: S) -> Result<UtxoSet, UnshieldedError>
where
    S: Stream<Item = Result<Event, UnshieldedError>>,
{
    let mut set = UtxoSet::new();
    futures::pin_mut!(stream);
    while let Some(item) = stream.next().await {
        match item? {
            Event::Transaction { created, spent } => {
                for u in created {
                    set.insert(u);
                }
                for id in spent {
                    set.remove(&id);
                }
            }
            Event::Progress { .. } => return Ok(set),
        }
    }
    Err(UnshieldedError::StreamClosedEarly)
}

/// Open a fresh graphql-transport-ws subscription against the
/// indexer, replay UTXO events into a `UtxoSet`, terminate on
/// the first `Progress` event. Closes the WS on return.
pub(super) async fn snapshot(
    ws_url: &str,
    address: &str,
) -> Result<UtxoSet, UnshieldedError> {
    let stream = transport::subscribe(
        ws_url,
        UNSHIELDED_TRANSACTIONS_QUERY,
        json!({ "address": address, "transactionId": 0 }),
    )
    .await?;

    // Adapt the raw JSON stream into an `Event` stream so we can
    // reuse `fold_events`.
    let events = stream.map(|item| item.and_then(|v| decode_event(&v)));
    fold_events(events).await
}
```

- [ ] **Step 4.2: Add unit tests for `fold_events`**

Append to the existing `mod tests` block in `snapshot.rs` (below the existing tests):

```rust
    use crate::unshielded::{TokenType, UnshieldedUtxo, UtxoId};
    use futures::stream;

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

    #[tokio::test]
    async fn fold_terminates_on_first_progress() {
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Transaction {
                created: vec![utxo(1, 0, 0xAB, 100)],
                spent: vec![],
            }),
            Ok(Event::Progress { highest_transaction_id: 10 }),
            // Below should never be reached:
            Ok(Event::Transaction {
                created: vec![utxo(2, 0, 0xAB, 999)],
                spent: vec![],
            }),
        ];
        let set = fold_events(stream::iter(events)).await.expect("ok");
        assert_eq!(set.len(), 1);
        assert_eq!(set.total_for(&TokenType(vec![0xAB])), 100);
    }

    #[tokio::test]
    async fn fold_applies_create_then_spend() {
        let id_a = UtxoId {
            intent_hash: [0x11; 32],
            output_index: 0,
        };
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Transaction {
                created: vec![utxo(0x11, 0, 0xAB, 100)],
                spent: vec![],
            }),
            Ok(Event::Transaction {
                created: vec![],
                spent: vec![id_a],
            }),
            Ok(Event::Progress { highest_transaction_id: 5 }),
        ];
        let set = fold_events(stream::iter(events)).await.expect("ok");
        assert!(set.is_empty());
    }

    #[tokio::test]
    async fn fold_returns_stream_closed_early_without_progress() {
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Transaction {
                created: vec![utxo(1, 0, 0xAB, 100)],
                spent: vec![],
            }),
        ];
        let err = fold_events(stream::iter(events)).await.unwrap_err();
        assert!(matches!(err, UnshieldedError::StreamClosedEarly));
    }

    #[tokio::test]
    async fn fold_propagates_first_error() {
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Transaction {
                created: vec![utxo(1, 0, 0xAB, 100)],
                spent: vec![],
            }),
            Err(UnshieldedError::Decode("boom".into())),
            Ok(Event::Progress { highest_transaction_id: 1 }),
        ];
        let err = fold_events(stream::iter(events)).await.unwrap_err();
        assert!(matches!(err, UnshieldedError::Decode(_)));
    }

    #[tokio::test]
    async fn fold_handles_empty_transaction_events() {
        let events: Vec<Result<Event, UnshieldedError>> = vec![
            Ok(Event::Transaction { created: vec![], spent: vec![] }),
            Ok(Event::Progress { highest_transaction_id: 0 }),
        ];
        let set = fold_events(stream::iter(events)).await.expect("ok");
        assert!(set.is_empty());
    }
```

- [ ] **Step 4.3: Run unit tests + warnings-clean check**

```
cargo test -p wallet-core --lib unshielded::snapshot
cargo check -p wallet-core
```

Expected: 11 tests pass (6 decoders from Task 2 + 5 fold_events tests). No warnings.

- [ ] **Step 4.4: Commit**

```bash
git add mobile-bench/wallet-core/src/unshielded/snapshot.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): unshielded sync — snapshot driver

Adds the `snapshot()` entry point that wires the
graphql-transport-ws transport from transport.rs into the Event
decoder from this module, folding create/spend events into a
UtxoSet and terminating on the first Progress event.

Folding logic is extracted into `fold_events`, which takes any
`Stream<Item = Result<Event, _>>` so we can unit-test
termination, error propagation, and create-then-spend ordering
against a hand-built `stream::iter`, with no live WebSocket.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 5: `Wallet::sync_unshielded()` entry point

**Files:**
- Modify: `mobile-bench/wallet-core/src/wallet.rs`
- Modify: `mobile-bench/wallet-core/src/lib.rs` (re-exports — already covered in Task 1, double-check)

- [ ] **Step 5.1: Add the method to `Wallet`**

In `mobile-bench/wallet-core/src/wallet.rs`, append inside the existing `impl Wallet { … }` block (right after the existing `unshielded_address` method, around line ~135 in the current file):

```rust
    /// Snapshot the unshielded UTXO set for this wallet's default
    /// address. See `docs/superpowers/specs/2026-05-14-unshielded-sync-design.md`.
    /// Opens a fresh `graphql-transport-ws` WebSocket to the
    /// indexer, replays UTXO create/spend events, terminates on
    /// the first `Progress` event, and returns the live set.
    pub async fn sync_unshielded(&self) -> Result<crate::UtxoSet, crate::UnshieldedError> {
        let address = self
            .unshielded_address()
            .map_err(|e| crate::UnshieldedError::InvalidAddress(e.to_string()))?;
        let cfg = self.network.config();
        crate::unshielded::snapshot::snapshot(cfg.indexer_ws_url, &address).await
    }
```

- [ ] **Step 5.2: Verify lib.rs re-exports already cover `UnshieldedError` / `UtxoSet`**

`mobile-bench/wallet-core/src/lib.rs` should already have (from Task 1):

```rust
pub use unshielded::{
    TokenType, UnshieldedError, UnshieldedUtxo, UtxoId, UtxoSet,
};
```

If it doesn't, add it now. Otherwise no-op.

- [ ] **Step 5.3: Compile-only test**

```
cargo check -p wallet-core
cargo check -p dioxus-wallet
```

Expected: both clean, no warnings.

- [ ] **Step 5.4: Commit**

```bash
git add mobile-bench/wallet-core/src/wallet.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): expose Wallet::sync_unshielded()

Wires the unshielded::snapshot module into the public Wallet API.
Reads self.unshielded_address() + the network's indexer_ws_url
from NetworkConfig, delegates to the snapshot driver, surfaces
InvalidAddress as a typed error variant.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 6: CLI example

**Files:**
- Create: `mobile-bench/wallet-core/examples/sync_unshielded.rs`

- [ ] **Step 6.1: Add the example**

Create `mobile-bench/wallet-core/examples/sync_unshielded.rs`:

```rust
//! Scripted unshielded-sync probe.
//!
//! Usage:
//!     cargo run -p wallet-core --example sync_unshielded -- preprod
//!
//! Prints the address, UTXO count, and per-token balance for the
//! demo wallet on the requested network. Used to bench/iterate on
//! the snapshot path without dragging the Dioxus app along.

use wallet_core::{Network, Wallet};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();

    let arg = std::env::args().nth(1).unwrap_or_else(|| "preprod".into());
    let network = parse_network(&arg)?;
    let w = Wallet::demo(network);
    let address = w.unshielded_address()?;
    println!("network: {:?}", network);
    println!("address: {address}");

    let started = std::time::Instant::now();
    let set = w.sync_unshielded().await?;
    let elapsed = started.elapsed();

    println!("sync ms: {}", elapsed.as_millis());
    println!("utxos:   {}", set.len());
    for (token, value) in set.balance_by_token() {
        println!("  {}: {}", hex::encode(&token.0), value);
    }
    Ok(())
}

fn parse_network(s: &str) -> anyhow::Result<Network> {
    let lower = s.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "mainnet" => Network::Mainnet,
        "preprod" => Network::PreProd,
        "preview" => Network::Preview,
        "qanet" => Network::QaNet,
        "devnet" => Network::DevNet,
        "undeployed" | "local" => Network::Undeployed,
        other => return Err(anyhow::anyhow!("unknown network: {other}")),
    })
}
```

- [ ] **Step 6.2: Verify it builds**

```
cargo build -p wallet-core --example sync_unshielded
```

Expected: clean build. Do NOT run it (live network test is in Task 8).

- [ ] **Step 6.3: Commit**

```bash
git add mobile-bench/wallet-core/examples/sync_unshielded.rs
git commit -S -s -m "$(cat <<'EOF'
feat(wallet-core): examples/sync_unshielded — CLI probe

Scripted target for the unshielded sync path. Builds clean; run
manually with `cargo run --example sync_unshielded -- preprod`.
Doubles as a manual integration check and a clean target for
benching sync latency without the Dioxus app.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 7: `BalancePanel` in dioxus-wallet

**Files:**
- Modify: `mobile-bench/dioxus-wallet/src/app.rs`

- [ ] **Step 7.1: Add the `BalancePanel` component**

In `mobile-bench/dioxus-wallet/src/app.rs`, find the `CreateDidPanel` definition (`fn CreateDidPanel(network: Network) -> Element` around line ~291). Append a new component definition immediately after `CreateDidPanel`'s closing brace:

```rust
#[component]
fn BalancePanel(network: Network) -> Element {
    let mut result = use_signal::<Option<Result<String, String>>>(|| None);
    let mut pending = use_signal(|| false);

    let sync = move |_| {
        if *pending.read() {
            return;
        }
        pending.set(true);
        result.set(None);
        spawn(async move {
            let w = Wallet::demo(network);
            let r = match w.sync_unshielded().await {
                Ok(set) => {
                    let mut lines = Vec::new();
                    lines.push(format!("utxos: {}", set.len()));
                    for (token, value) in set.balance_by_token() {
                        lines.push(format!("  {}: {}", hex::encode(&token.0), value));
                    }
                    Ok(lines.join("\n"))
                }
                Err(e) => Err(e.to_string()),
            };
            result.set(Some(r));
            pending.set(false);
        });
    };

    rsx! {
        div { class: "row", "Balance" }
        div { class: "row",
            button {
                disabled: *pending.read(),
                onclick: sync,
                {if *pending.read() { "Syncing…" } else { "Sync balance" }}
            }
        }
        if let Some(res) = result.read().as_ref() {
            match res {
                Ok(text) => rsx! { div { class: "seed-blob", "{text}" } },
                Err(e) => rsx! { div { class: "seed-blob", style: "color: var(--error);", "{e}" } },
            }
        }
    }
}
```

- [ ] **Step 7.2: Mount the panel in the wallet view**

Find the `rsx! { … }` block that contains `CreateDidPanel { network: *network.read() }` (around line ~284). Add `BalancePanel` right before it:

```rust
                BalancePanel { network: *network.read() }
                ResolveDidPanel { network: *network.read() }
                CreateDidPanel { network: *network.read() }
```

(Adjust ordering — `BalancePanel` first because seeing your balance is the natural pre-action.)

- [ ] **Step 7.3: Verify the UI compiles**

```
cargo check -p dioxus-wallet
```

Expected: clean.

- [ ] **Step 7.4: Commit**

```bash
git add mobile-bench/dioxus-wallet/src/app.rs
git commit -S -s -m "$(cat <<'EOF'
feat(dioxus-wallet): BalancePanel — surface sync_unshielded() in UI

Adds a "Sync balance" button next to the existing CreateDidPanel
and ResolveDidPanel. On click, runs Wallet::sync_unshielded()
against the selected network and renders `utxos: N` plus per-token
balances. Errors surface inline with the same red-on-error style
the other panels use.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

### Task 8: Live integration test (gated by `network-tests` feature)

**Files:**
- Create: `mobile-bench/wallet-core/tests/unshielded_live.rs`

- [ ] **Step 8.1: Add the test**

Create `mobile-bench/wallet-core/tests/unshielded_live.rs`:

```rust
//! Live integration tests for unshielded sync. Gated behind the
//! `network-tests` feature so CI/offline runs don't depend on
//! preprod reachability.
//!
//! Run with:
//!     cargo test -p wallet-core --features network-tests --test unshielded_live -- --nocapture

#![cfg(feature = "network-tests")]

use wallet_core::{Network, Wallet};

#[tokio::test]
async fn snapshot_preprod_demo_wallet() {
    // Initialize crypto provider once — required by the rustls
    // stack the indexer WS sits behind.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let w = Wallet::demo(Network::PreProd);
    let address = w
        .unshielded_address()
        .expect("demo wallet has an unshielded address");
    println!("snapshot for: {address}");

    let started = std::time::Instant::now();
    let set = w
        .sync_unshielded()
        .await
        .expect("snapshot returns Ok against live preprod");
    let elapsed = started.elapsed();

    println!("sync took {} ms", elapsed.as_millis());
    println!("utxos: {}", set.len());

    // Assertion is shape-only — the demo wallet's address may be
    // empty on preprod. What we care about is that the call
    // terminated cleanly (not that it found UTXOs).
    for u in set.iter() {
        assert_eq!(u.owner, address, "every utxo's owner matches our address");
    }
}
```

- [ ] **Step 8.2: Manual sanity run (do NOT run in CI)**

```
cargo test -p wallet-core --features network-tests --test unshielded_live -- --nocapture
```

Expected:
- Test prints `snapshot for: mn_addr_…`, `sync took N ms`, `utxos: K`.
- Returns `Ok` within ~30s. (If it hangs, suspect open question #1 in the spec — the indexer may not emit a Progress event for an empty address. Resolution: in `snapshot.rs`, wrap the `fold_events` call in a `tokio::time::timeout` and surface a new `UnshieldedError::Timeout(Duration)` variant, then update the BalancePanel + CLI example error rendering.)
- If `Decode` errors fire: capture the failing JSON in a comment, update the decoder in `snapshot.rs`, and re-run.
- If `InvalidAddress` fires: verify `Wallet::unshielded_address()`'s bech32m output is what the indexer expects. May need an `addr_test1…` vs `mn_addr_test1…` HRP adjustment in `address.rs`.

Any fixes needed go into a follow-up commit (or amend Task 8 if the test never landed green).

- [ ] **Step 8.3: Commit**

```bash
git add mobile-bench/wallet-core/tests/unshielded_live.rs
git commit -S -s -m "$(cat <<'EOF'
test(wallet-core): live preprod snapshot integration test

Gated behind --features network-tests so CI and offline runs stay
fast. Confirms sync_unshielded() against live preprod returns Ok,
prints sync latency and UTXO count, and asserts every returned
UTXO's owner matches the wallet's own unshielded address.

Implementation-time resolution for the three open questions in the
spec (empty-address Progress emission, address-format
compatibility, frame ordering) happens here as a side effect — any
failures surface as concrete `Decode`/`StreamClosedEarly`/
`InvalidAddress` errors that point at the line to fix.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
git log --format="%h %G? %s" -1
```

Expected: `G`.

---

## Self-review notes (for the executing engineer)

After all 8 tasks complete, run a final full check:

```
cargo test -p wallet-core --lib
cargo check -p wallet-core
cargo check -p dioxus-wallet
cargo build -p wallet-core --example sync_unshielded
# optional, network-gated:
cargo test -p wallet-core --features network-tests --test unshielded_live -- --nocapture
git log --format="%h %G? %s" -10   # confirm 8 G-signed commits since the design doc
```

Total expected `cargo test -p wallet-core --lib` count: original 41 (from prior commits) + 9 (UtxoSet) + 6 (decode_event) + 6 (transport framing) + 5 (fold_events) = **67 tests**. All should pass.

---

## Spec coverage check

| Spec section | Task(s) | Notes |
|---|---|---|
| Goal: `Wallet::sync_unshielded()` | Task 5 | wires through to snapshot |
| `UtxoSet` + helpers | Task 1 | greedy `pick_for_amount` confirmed greedy-only |
| `UnshieldedUtxo` / `UtxoId` / `TokenType` | Task 1 | shapes match spec exactly |
| Snapshot semantics (open → replay → Progress → close) | Tasks 3 + 4 | transport opens/closes; snapshot terminates on first Progress |
| `graphql-transport-ws` framing | Task 3 | init/ack/subscribe/next/complete + ping/pong tolerated |
| `BalancePanel` Dioxus surface | Task 7 | mirrors CreateDidPanel pattern |
| `examples/sync_unshielded.rs` CLI | Task 6 | prints latency + per-token balance |
| `tests/unshielded_live.rs` integration test | Task 8 | gated behind `network-tests` |
| Open questions resolved during impl | Task 8 step 8.2 | concrete error-driven path |
| Non-goals (no streaming, no cursor, no multi-address, no smart picker, no token parsing, no reorgs) | n/a | all non-goals explicitly preserved by design choices in tasks above |
