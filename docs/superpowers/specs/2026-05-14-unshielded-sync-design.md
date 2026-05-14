# Unshielded sync — snapshot-on-demand

Subsystem A of the Midnight DID CRUD slice. Unblocks fee balancing
for the deploy-submit slice (subsystem B) by giving the wallet a
point-in-time view of its NIGHT/DUST UTXOs.

The whole DID-management feature decomposes into independent
sub-projects (A — unshielded sync; B — tx envelope + submit; D' —
publish/deploy as the first write; then proof-bearing circuits).
This spec covers **only A**. Subsequent sub-projects get their own
specs.

## Goal

Add `Wallet::sync_unshielded() -> Result<UtxoSet, UnshieldedError>`:
an async, on-demand snapshot of the wallet's default unshielded
address. Each call opens a fresh WebSocket to the indexer, replays
UTXO create/spend events into an in-memory `UtxoSet`, terminates as
soon as the indexer reports the address backlog is drained, closes
the WS, and returns the set. No persistence between calls.

The deliverable is the library API plus a Dioxus `BalancePanel`
that surfaces it, plus a CLI example
(`examples/sync_unshielded.rs`) for scripted use.

## Non-goals

- **No persistent streaming.** A long-lived subscription that keeps
  the wallet's view live in real-time is a follow-up. Snapshot is
  enough for fee-balancing in subsystem B.
- **No cross-call caching / cursor.** Stateless re-sync from
  `transactionId: 0` every call. Bandwidth cost is acceptable for a
  demo wallet; caching is a follow-up if real usage exposes a problem.
- **No multi-address sync.** Only the wallet's default unshielded
  address (`Wallet::unshielded_address()`). Multi-account / role
  support is future work.
- **No optimal coin selection.** `UtxoSet::pick_for_amount` is
  greedy (sort by value descending, take until covered). Subsystem
  B can swap in something smarter later.
- **No reorg handling.** We trust whatever the indexer reports as
  the current state. The indexer's own finality model is opaque to
  us.
- **No token-type parsing.** `TokenType` is opaque bytes for this
  slice; subsystem B identifies NIGHT (and DUST if needed) by
  matching the bytes against a known constant.

## Architecture

One async function, fresh WebSocket per call:

```
Wallet::sync_unshielded()
   │
   ▼
unshielded::snapshot(ws_url, address)
   │  • tokio_tungstenite::connect_async, Sec-WebSocket-Protocol: graphql-transport-ws
   │  • send {type: "connection_init"}, await {type: "connection_ack"}
   │  • send {type: "subscribe", id: "1",
   │           payload: {query: SUBSCRIBE_UNSHIELDED,
   │                    variables: {address, transactionId: 0}}}
   │
   ▼
loop over `next` frames
   │  UnshieldedTransaction → fold createdUtxos / spentUtxos into UtxoSet
   │  UnshieldedTransactionsProgress → terminate
   │
   ▼
drop stream → WS close frame → return UtxoSet
```

**File layout** (all under `mobile-bench/`):

| Path | Role |
|---|---|
| `wallet-core/src/unshielded/mod.rs` | Public types (`UnshieldedUtxo`, `UtxoSet`, `TokenType`, `UtxoId`, `UnshieldedError`). |
| `wallet-core/src/unshielded/snapshot.rs` | Subscription loop. |
| `wallet-core/src/unshielded/transport.rs` | Minimal `graphql-transport-ws` client. Generic — reusable by subsystem B and future subscriptions. |
| `wallet-core/queries/midnight-indexer/unshielded_transactions.subscription.graphql` | Subscription document. |
| `wallet-core/src/wallet.rs` | Adds `sync_unshielded()`. |
| `wallet-core/examples/sync_unshielded.rs` | CLI: prints `UtxoSet` for `Wallet::demo(<network>)`. |
| `dioxus-wallet/src/app.rs` | Adds `BalancePanel`. |
| `wallet-core/tests/unshielded_live.rs` | Live integration test (gated behind `--features network-tests`). |

## Types

```rust
/// Hex-encoded serialized token type from the indexer. Opaque for
/// this slice; subsystem B compares against a known NIGHT constant.
pub struct TokenType(pub Vec<u8>);

/// Intent-hash + output-index pair — the indexer's natural UTXO key.
pub struct UtxoId {
    pub intent_hash: [u8; 32],
    pub output_index: u32,
}

/// One live unshielded UTXO. Field shape mirrors the indexer's
/// `UnshieldedUtxo` graphql type 1:1, with strings parsed to native.
pub struct UnshieldedUtxo {
    pub owner: String,          // bech32m UnshieldedAddress
    pub token_type: TokenType,
    pub value: u128,             // parsed from indexer's String
    pub id: UtxoId,
    pub ctime: Option<u64>,
    pub initial_nonce: [u8; 32],
}

pub struct UtxoSet {
    utxos: HashMap<UtxoId, UnshieldedUtxo>,
}

impl UtxoSet {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn iter(&self) -> impl Iterator<Item = &UnshieldedUtxo>;
    pub fn get(&self, id: &UtxoId) -> Option<&UnshieldedUtxo>;
    pub fn balance_by_token(&self) -> HashMap<TokenType, u128>;
    pub fn total_for(&self, token: &TokenType) -> u128;

    /// Greedy selection: sort by value descending, take until the
    /// cumulative sum covers `amount`. Returns None if the total
    /// for `token` is insufficient. No change-minimisation, no
    /// fragmentation logic — explicit non-goal for this slice.
    pub fn pick_for_amount(
        &self,
        token: &TokenType,
        amount: u128,
    ) -> Option<Vec<&UnshieldedUtxo>>;
}

pub enum UnshieldedError {
    WsConnect(String),
    WsHandshake(String),         // connection_init / ack failure
    GqlError(String),            // server-emitted error frame
    UnexpectedFrame(String),     // frame we couldn't parse
    Decode(String),              // value / hex / u128 / bech32 parse
    StreamClosedEarly,           // server hung up before Progress
    InvalidAddress(String),      // unshielded_address() rejected
}
```

## Internals

### `transport::subscribe`

```rust
pub(super) async fn subscribe(
    ws_url: &str,
    query: &str,
    variables: serde_json::Value,
) -> Result<impl Stream<Item = Result<serde_json::Value, UnshieldedError>>, UnshieldedError>;
```

Hand-rolled minimal client over `tokio_tungstenite::connect_async`.
One subscription per WS — no multiplexing. Caller receives parsed
`next.payload.data` JSON values; framing/handshake is internal.
Closes on `complete`, server `error`, or caller dropping the stream.

### `snapshot::snapshot`

```rust
pub(super) async fn snapshot(
    ws_url: &str,
    address: &str,
) -> Result<UtxoSet, UnshieldedError> {
    let stream = transport::subscribe(
        ws_url,
        UNSHIELDED_TRANSACTIONS_QUERY,
        json!({ "address": address, "transactionId": 0 }),
    ).await?;

    let mut set = UtxoSet::new();
    pin_mut!(stream);
    while let Some(frame) = stream.next().await {
        match decode_event(&frame?)? {
            Event::Transaction { created, spent } => {
                for u in created { set.insert(u); }
                for id in spent { set.remove(&id); }
            }
            Event::Progress { .. } => return Ok(set),
        }
    }
    Err(UnshieldedError::StreamClosedEarly)
}
```

The first `Progress` event is our termination signal: it says "for
this address, everything up to transaction id N has been
delivered." Once we've consumed up to N, there's nothing left in
the backlog and we exit.

### `Wallet::sync_unshielded`

```rust
pub async fn sync_unshielded(&self) -> Result<UtxoSet, UnshieldedError> {
    let address = self.unshielded_address()
        .map_err(|e| UnshieldedError::InvalidAddress(e.to_string()))?;
    let cfg = self.network.config();
    crate::unshielded::snapshot::snapshot(cfg.indexer_ws_url, &address).await
}
```

### `BalancePanel`

Mirrors `CreateDidPanel`: a "Sync balance" button, spawn on click,
render `balance_by_token()` as `<hex-token>: <u128>` lines on
success, render the error string on failure. ~25 lines.

### CLI example

```rust
// examples/sync_unshielded.rs
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let network: Network = std::env::args().nth(1)
        .unwrap_or_else(|| "preprod".into())
        .parse()?;
    let w = wallet_core::Wallet::demo(network);
    let set = w.sync_unshielded().await?;
    println!("address: {}", w.unshielded_address()?);
    println!("utxos: {}", set.len());
    for (token, value) in set.balance_by_token() {
        println!("  {}: {}", hex::encode(&token.0), value);
    }
    Ok(())
}
```

## Error handling

Fail-fast, surface everything. Every variant carries a `String`
with enough context to diagnose from the UI without re-running. No
silent retries. Retry = user re-clicks; the slice is cheap enough
to redo.

| Failure mode | Variant |
|---|---|
| `wss://…` unreachable | `WsConnect(reason)` |
| Handshake error (no ack / wrong protocol) | `WsHandshake(reason)` |
| Server GQL error frame | `GqlError(message)` |
| Unrecognised frame | `UnexpectedFrame(snippet)` |
| Field-level parse failure | `Decode(field + reason)` |
| Server closes without Progress | `StreamClosedEarly` |
| `unshielded_address()` rejected | `InvalidAddress(reason)` |

## Testing

| Layer | Test | Lives in |
|---|---|---|
| Unit | `UtxoSet` insert / remove / balance / `pick_for_amount` (cover / overspend / mixed tokens / empty) | `unshielded/mod.rs::tests` |
| Unit | `snapshot` against a hand-built `Stream<Event>` — verifies create/spend folding in isolation, no WS | `unshielded/snapshot.rs::tests` |
| Unit | `transport` graphql-transport-ws framing — serialise `connection_init` / `subscribe`, parse canned `next` frame | `unshielded/transport.rs::tests` |
| Integration | Live snapshot against preprod, assertion only that the call returns `Ok` and the set's address matches | `tests/unshielded_live.rs`, gated by `--features network-tests` |

## Open questions to verify during implementation

These are read-the-ledger-code-and-write-a-probe questions, not
design questions:

1. Does `unshieldedTransactions(address, 0)` emit a `Progress` event
   when the address has zero history? An empty preprod wallet must
   terminate quickly; if the server stays silent waiting for an
   event, we need a fallback termination heuristic (e.g. an idle
   timeout, or "Progress before any tx ⇒ done").
2. Does the indexer accept the bech32m `mn_addr_*` HRP-encoded
   string in the `UnshieldedAddress` scalar (which is what
   `Wallet::unshielded_address()` returns)? Almost certainly yes,
   but worth a one-shot probe.
3. Subscription field ordering: confirm `createdUtxos` and
   `spentUtxos` arrive in the same frame as the parent `transaction`.
   Schema says yes; verifying against one live frame is the cheap
   confirmation.

These get resolved by the live integration test as a side effect.
If any of them surfaces a real problem, fix is local to
`snapshot.rs` and doesn't reshape the design.

## Out-of-scope follow-ups

- Persistent streaming subscription that keeps `UtxoSet` live.
- Cross-call caching of the transactionId cursor.
- Multi-address sync.
- Token-type parsing (`NIGHT` constant, custom token registry).
- Smarter coin-selection (knapsack, fragmentation-aware, etc.).
- DUST-specific accounting (mass, generation tracking, registrations).
