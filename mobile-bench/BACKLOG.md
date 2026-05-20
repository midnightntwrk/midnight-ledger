# Mobile-Bench Backlog

Deferred work items that have been scoped and decided-against-for-now,
but should be picked up later. Each entry should include enough context
that a future session can act on it without re-doing the investigation.

---

## Path B — Full wallet sync to redb

**Status:** Phase 1 LANDED in commit `5f7df14f` (schema v6 +
`DUST_SYNC` table + `WalletStore` get/put/clear methods).
Phase 2/3/4 still to do — sketched at the bottom of this section.

Background on Path A: per-call live fetch of `LedgerParameters` +
`ZswapChainState` from the indexer landed via commit `57068c39` +
follow-ups, plus the HTTP-proof-server route in `0875368c`. That
unblocked the PreProd write demo but each write still pays a full
DUST event replay (~534k events on PreProd → 30–50 min per click).
Path B persists that snapshot so subsequent launches resume from
`last_id + 1`.

**Why this is the natural next step:**
Upstream `midnight-did-manager-service` persists 5 JSON files in
`~/.midnight-did/profiles/<network>/<profile>/wallet-state/<seedHash6>/`:

| File | Size | Contains |
|---|---|---|
| `meta.json` | 175 B | `walletSchema`, `profile`, `updatedAt` |
| `shielded.json` | ~3.5 KB | tagged-serialized `zswap-local-state[v6]`, `offset` checkpoint, `coinPublicKey`, `encryptionPublicKey` |
| `unshielded.json` | ~700 B | available UTXOs, `appliedId` checkpoint |
| `unshielded-history.json` | ~1.7 KB | tx history |
| `dust.json` | **~4.2 MB** | tagged-serialized `dust-local-state[v1]`, `offset` checkpoint |

The pattern: each file stores `{ state: <bytes>, offset: <event_id>, ... }`.
On startup, deserialize → catch up from `offset+1` via indexer
subscription → re-serialize back. The 10-15 min sync is only on
first run; subsequent launches are fast.

**Why we'd want it (long term):**

- Tx history view, balance trends, "recent activity" tab — all need
  persisted state Path A doesn't have.
- Mobile cold-start UX: one slow first-run, fast thereafter, vs Path
  A's per-action indexer spinner.
- Indexer outage resilience: read cached state, retry write later.
- Adding new contracts beyond DID reuses the same sync infrastructure
  instead of N independent per-call queries.
- Schema migration story stays consolidated in redb (already at v5;
  v6 adds sync tables).
- Architectural alignment with upstream — when their state model
  evolves, ours evolves the same way.

**Why Path A is fine for now:**

- ~30 lines vs ~600 for B. Real maintenance delta.
- Always fresh — no drift window.
- The "fresh fetch right before submit" hook stays useful even
  after B lands (upstream's `publicDataProvider.queryZSwapAndContractState`
  is exactly this pattern at submit time).

---

### Phase 1 — Schema v6 snapshot tables (~150 LOC) — DONE `5f7df14f`

Mirror upstream's 5 JSON files 1-to-1 in redb. One row per "state kind"
keeps it small and all-or-nothing replaceable.

```rust
// mobile-bench/wallet-core/src/store/mod.rs (or wherever schema lives)
const WALLET_SYNC_STATE: TableDefinition<&str, &[u8]>;
// keys:
//   "zswap"             -> tagged-serialized ZswapLocalState bytes
//   "dust"              -> tagged-serialized DustLocalState bytes
//   "unshielded"        -> tagged-serialized UnshieldedState bytes
//   "unshielded_history" -> serialized history blob

const WALLET_SYNC_OFFSETS: TableDefinition<&str, u64>;
// keys: "zswap_offset", "dust_offset", "unshielded_applied_id"

const WALLET_SYNC_META: TableDefinition<&str, &str>;
// "wallet_schema" -> "ledger8"
// "network"       -> "preprod"
// "updated_at"    -> ISO-8601 timestamp
```

Migration: follow the same pattern as v4→v5 (see existing migration
helpers). On open, if `META.get("wallet_schema") == None`, create
empty rows and write `{ "wallet_schema": "ledger8", "network": <net>,
"updated_at": now }`.

The 4.2 MB dust blob is fine: redb stores it as a single value,
atomic transaction, `Database::compact()` reclaims churn explicitly
when we ask.

---

### Phase 2 — `WalletSyncer` module (~200 LOC) — TODO

User wants:
- A **"Sync DUST" button** on the wallet page that triggers a full
  catch-up with a visible progress bar (initial cold sync UX).
- Subsequent CRUD operations do a **quick incremental resync** of
  the delta since the last persist — transparent, no spinner.

Sketch:

```rust
// mobile-bench/wallet-core/src/dust/syncer.rs (new)
pub struct DustSyncer {
    network: Network,
    store: Arc<WalletStore>,
    dust_key: Arc<DustSecretKey>,
}

pub struct SyncProgress {
    pub current_id: i64,
    pub max_id: i64,
}

impl DustSyncer {
    /// Load the persisted snapshot if any, subscribe from
    /// `last_id + 1`, fold events, persist on a debounce window.
    /// Returns a stream that emits progress updates so the UI
    /// can render a progress bar.
    pub fn sync(&self) -> impl Stream<Item = SyncProgress>;

    /// Read the current cached state without going to the network.
    /// Returns the most recent persisted snapshot. Pairs with
    /// `Wallet::call_did_circuit` which then needs only a tiny
    /// delta sync (current_id..tip).
    pub fn cached_state(&self) -> Option<DustLocalState<DefaultDB>>;
}
```

Reuse the `fold_events` pattern from
`mobile-bench/wallet-core/src/dust/snapshot.rs:64-112` — same
indexer subscription, just persists every N events / M seconds.

### Phase 3 — Wallet integration (~100 LOC) — TODO

Current `Wallet::sync_dust()` does a full replay every call. Make
it consult the cache first via the `DustSyncer`:

1. `Wallet` grows an `Option<Arc<DustSyncer>>` field (clean: trait
   abstraction so wallet-core doesn't bind to the store concretely).
2. `with_dust_syncer(syncer)` builder, called by the App at wallet
   construction.
3. `sync_dust()` checks the syncer's cached state; if missing,
   falls back to today's full-replay path.
4. Internal `Wallet::from_seed(seed_bytes, network)` calls inside
   `call_did_circuit` / `create_did` / `load_did_circuit` thread
   the syncer Arc the same way they currently propagate
   `proof_server_url`.

### Phase 4 — UI sync button + progress (~100 LOC) — TODO

- New row on the wallet page: "DUST sync: last synced 2026-05-20 16:30, last_id=534,302"
- "Sync now" button → kicks `DustSyncer::sync()`, renders progress
  bar driven by the `SyncProgress` stream.
- Progress format: `Indexing… 234,567 / 534,302 (43%)`.
- Auto-trigger an incremental sync on every CRUD submit (small,
  fast).

### Original Phase 2 — Sync loop per state kind (~250 LOC, deferred)

Reuse the exact `fold_events` pattern from
`mobile-bench/wallet-core/src/dust/snapshot.rs:64-112`. That code
already handles the indexer's "you're caught up" marker (`max_id`)
and the idle-timeout backstop.

```rust
// mobile-bench/wallet-core/src/wallet_sync/mod.rs (new module)
pub struct WalletSyncer { store: Arc<WalletStore>, network: Network, ... }

impl WalletSyncer {
    pub async fn cold_start(&mut self) -> Result<(), SyncError> {
        // 1. Load offsets from redb
        let offsets = self.store.load_offsets()?;
        // 2. Hydrate state from redb
        let mut zswap = self.store.load_zswap_state()?.unwrap_or_default();
        let mut dust  = self.store.load_dust_state()?.unwrap_or_default();
        let mut unsh  = self.store.load_unshielded_state()?.unwrap_or_default();
        // 3. Catch up from offset+1
        self.catchup_zswap(&mut zswap, offsets.zswap_offset).await?;
        self.catchup_dust(&mut dust, offsets.dust_offset).await?;
        self.catchup_unshielded(&mut unsh, offsets.unshielded_applied_id).await?;
        // 4. Persist atomically
        self.store.persist_all(&zswap, &dust, &unsh, &offsets)?;
        Ok(())
    }
}
```

Subscriptions (one per kind):
- `zswapLedgerEvents` — chain-wide Zswap event stream (use indexer
  WebSocket subscription, same `transport::subscribe` we use for
  dust)
- `dustLedgerEvents` — already wired in `dust/snapshot.rs`
- `unshieldedTransactions` — wallet-specific stream, takes the
  wallet's address as filter

Fold semantics:
- zswap: `ZswapLocalState::apply_event(event)`
- dust: `DustLocalState::apply_event(event)` (we already do this in
  fold_events; just persist instead of replay-from-zero)
- unshielded: maintain `availableUtxos` and `pendingUtxos` sets

Persist on every N events or every M seconds (whichever first), in
one redb transaction so all kinds stay consistent.

---

### Phase 3 — Read-through cache + fresh-fetch hooks (~100 LOC)

**Reads** (resolve, balance display, history): serve from redb
cache. Always fast.

**Writes** (`call_did_circuit`, `create_did`, `load_did_circuit`):
do a fresh fetch of `(ContractState, ZswapChainState, LedgerParameters)`
right before composing the unproven tx. This is what upstream's
`publicDataProvider.queryZSwapAndContractState` does at submit time
and is the pattern Path A already implements. Keep it.

The split:
- `WalletSyncer::get_state_for_read() -> CachedState` — fast, may be slightly stale
- `WalletSyncer::get_state_for_submit() -> FreshState` — slow, indexer round-trip, guaranteed current

---

### Acceptance criteria

- [ ] Schema v6 migration round-trips cleanly on a v5 store
- [ ] Cold start hydrates `(zswap, dust, unshielded)` from redb in <100ms
- [ ] First-run catch-up against PreProd completes (expect ~10-15 min
      based on upstream manager benchmarks)
- [ ] Subsequent launches resume from `offset` and catch up the delta
      in <30s on a current chain
- [ ] `preprod_add_also_known_as` still passes after Phase 3 — fresh
      fetch at submit time preserves Path A's correctness
- [ ] redb file size after a full preprod sync is bounded (verify with
      a real run; expect ~5 MB based on upstream's JSON sizes)
- [ ] `Database::compact()` reclaims at least 30% of churn after a
      thousand updates

### Test plan

1. Unit: round-trip each state kind through redb (encode → store →
   load → decode → assert equal)
2. Integration: replay a recorded stream of indexer events and assert
   the final state matches a known checkpoint
3. Live (`network-tests` feature): cold-start against PreProd, verify
   resume on second launch processes only the delta
4. Regression: re-run the byte-diff test in `preprod_decode_diff` and
   confirm the partition stays in `guaranteed_transcript`

### Open questions for Phase 1 kickoff

- Subscribe per-kind in parallel (3 concurrent streams) or sequential?
  Upstream's manager runs them concurrently — start there.
- Where does `WalletSyncer` live in the binary's lifecycle? Probably
  the same place that opens the redb store today. Needs care around
  shutdown to avoid torn writes (redb handles atomicity per-txn but
  we should drain in-flight events before close).
- Does Compaction belong in a background task on app idle? Or
  user-triggered? Decision: user-triggered via a Diagnostics-tab
  button (matches the user's "we have control over it" framing).

---

## Other items

(none yet — append below as they come up)
