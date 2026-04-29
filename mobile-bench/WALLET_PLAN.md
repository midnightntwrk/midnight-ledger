# Dioxus Midnight Wallet — implementation plan

Drafted 2026-04-29. Status: **plan**, not yet started.

A native Rust + Dioxus wallet for the Midnight blockchain — desktop-first,
Android/iOS later — modeled after the [GSD Wallet](https://github.com/adamreynolds-io/gsd-wallet)
Chrome extension but with **zero JS/WASM bridge**. Where gsd-wallet calls
into `@midnight-ntwrk/wallet-sdk-facade` and `@midnight-ntwrk/ledger-v8`
WASM, we link the source Rust crates from this same workspace directly.
Where it relies on `chrome.storage.local` / IndexedDB, we use sled or
sqlite via rusqlite.

## Target audience (matches gsd-wallet)

dApp developers, QA engineers, and SDK integrators who need a wallet to
test contract deployments, token transfers, and DApp connector flows
across **Undeployed (local), DevNet, QANet, Preview, PreProd, Mainnet**.
**Not a production wallet** — seeds stored in plaintext under
`~/.local/share/midnight-dx-wallet/`. Same disclaimer as gsd-wallet.

## Why Dioxus + native Rust (vs. browser extension)

- **One codebase, four targets**: macOS / Linux / Windows desktop + Android
  + iOS, share 95%+ of the code. Same Dioxus app shell as `mobile-bench/dioxus-bench`.
- **No WASM perf cliff**: shielded proving runs in native arm64-v8a — iter-2
  numbers show k=11 ec at 317 ms desktop / projected ~1 s on mobile, vs.
  worker-WASM in gsd-wallet that gives several × overhead and triggers
  Chrome unresponsiveness dialogs without the offscreen-document hack.
- **No service-worker/offscreen plumbing**: gsd-wallet's `Page → CS → SW →
  Offscreen → Worker` 5-hop architecture exists *only* to escape Chrome's
  responsiveness checks. A native app spawns a tokio runtime and is done.
- **Already using these crates**: `mobile-bench/prover-core` consumes
  `ledger`, `zswap`, `transient-crypto`, `zkir` directly. The wallet
  consumes the same set + the missing client crates we'll add.

## What gsd-wallet has that we'll match (eventually)

| Feature | gsd-wallet | Rust port — building block | Iter |
|---|---|---|---|
| Multi-environment switcher | TS env enum | New `wallet-core::Network` enum | 1 |
| Wallet menu (create/switch/delete/copy-seed) | React Popup | Dioxus side panel | 1 |
| Genesis wallets W0–W3 (localnet) | Hardcoded seeds | Same hardcoded seeds | 1 |
| Multi-wallet | Per-wallet IndexedDB key | Per-wallet sled tree | 2 |
| Shielded transfers | wallet-sdk-facade | `wallet-core` over `zswap`+`ledger` | 2 |
| Unshielded transfers | wallet-sdk-facade | `wallet-core` over `ledger` | 2 |
| NIGHT + custom tokens | `coin_structure::TokenType` (TS) | `midnight-coin-structure::TokenType` (Rust) | 2 |
| Dust register/deregister | wallet-sdk-facade | `wallet-core` over `ledger::dust` | 3 |
| DApp connector (`window.midnight`) | content script + injected script | **out of scope** for native app — see §"Non-goals" | — |
| Custom endpoints (Node/Indexer/Prover URLs) | Per-env settings | Same settings, persisted to TOML | 1 |
| Sync diagnostics (level/category filters, NDJSON export) | 2000-event ring buffer | `tracing` → in-memory ring buffer + UI panel | 3 |
| Bundled mainnet snapshot | 88k events in `dist/` | Same `.bin`/NDJSON snapshot bundled at build | 4 |
| Built-in explorer (v4 indexer) | React tab | Dioxus tab over the same indexer client | 4 |
| Failed-tx download | JSON dump | Same | 3 |

## Architecture

```
+-------------------------------------------------+
| dioxus-wallet  (UI: routes, components, signals)|
+----------+--------------------------------+-----+
           | uses                           | uses
           v                                v
+-------------------+        +-------------------------+
| wallet-core (new) |--------|  prover-core (existing) |
|   sync, txs, keys |        |   ProverCore + bench    |
+---+-----+----+----+        +-------------------------+
    |     |    |
    |     |    +--------------+  
    v     v                   v
+--------+--------+   +-------+--------+   +--------------+
| midnight-ledger |   |  midnight-zswap|   | indexer-client (new) |
| transient-crypto|   |   coin-structure|  | node-rpc-client (new)|
|     storage     |   |    onchain-vm   |  |proof-server-client(*)|
+-----------------+   +-----------------+  +----------------------+
```

(*) `proof-server-client` lives behind a feature flag in `prover-core`
already (`mobile-bench/prover-core/src/http.rs`); we'll lift its callable
surface up into `wallet-core` so the wallet can choose local vs. remote
proving without duplicating the HTTP plumbing.

### New crates introduced

1. **`wallet-core/`** — pure-Rust wallet business logic (no UI). Owns:
   - `Network` enum (Undeployed/DevNet/QANet/Preview/PreProd/Mainnet)
   - `Wallet` struct (seed, derived keys, current state, sync handles)
   - `WalletStore` (sled-backed persistence: wallets, checkpoints, event
     cache, settings)
   - Sync drivers per subsystem (Shielded/Unshielded/Dust), each a
     tokio task with progress channels
   - Tx builders for shielded/unshielded/dust register
   - Local + remote prover dispatch
2. **`indexer-client/`** — async GraphQL/WebSocket client for Midnight's
   v4 indexer. **Schema + endpoint URLs need to be sourced from
   gsd-wallet's TS code.** Will use `cynic` or `graphql-client` for codegen,
   `tokio-tungstenite` for the WS subscription path.
3. **`node-rpc-client/`** — JSON-RPC to substrate-style node endpoints
   for chain-tip / block lookups gsd-wallet does via `@polkadot/api`.
4. **`dioxus-wallet/`** — the app crate. Mirrors the layout of
   `mobile-bench/dioxus-bench`: `src/app.rs`, `src/runner.rs`,
   `src/platform/{desktop,android,ios}.rs`, `android/` Gradle scaffold.

### What we **don't** add

- Mnemonic/BIP39: gsd-wallet stores raw seed bytes; we do the same to keep
  parity. Adding `bip39` later is a one-crate addition.
- DApp connector (`window.midnight`): see *Non-goals*.
- React/JS in any form.

## Iterations

Each iteration ends with a runnable demo + an updated `WALLET_RESULTS.md`
in the same vein as `RESULTS.md`. The bar is ~1 week per iteration with one
engineer; trim/move scope freely.

### Iteration 1 — "single shielded transfer on Undeployed"

Smallest end-to-end slice. Hardcode one localnet endpoint, one wallet (W0
seed), one shielded transfer.

- [ ] **Skeleton**: create `wallet-core/`, `dioxus-wallet/` crates;
      desktop window opens with a single-page UI: balance + recipient +
      amount + Send button; status pane below.
- [ ] **Network config**: `Network` enum, hardcode Undeployed
      `node_url`/`indexer_url`/`prover_url` from gsd-wallet's TS const
      file. Add `wallet-core/src/network.rs`.
- [ ] **Seed → keys**: wrap `zswap::keys::SecretKeys::from_rng_seed()`
      in `wallet-core::Wallet::from_seed()`. Reuse W0 seed from gsd-wallet.
- [ ] **Indexer client (read-only)**: GraphQL query for shielded events
      since block N. Fetch + decrypt with `SecretKeys::try_decrypt()` to
      compute balance. **Schema-pinning**: copy gsd-wallet's `.graphql`
      files into `indexer-client/schema/`.
- [ ] **Shielded send**: `wallet-core::Wallet::send_shielded(to, amount,
      token)` builds `ZSwap` offer + `StandardTransaction` via
      `ledger::construct::StandardTransaction::from_intents()`, hands
      `ProofPreimage` to `prover-core::ProverCore`, posts proven tx to
      node RPC.
- [ ] **Persistence**: settings TOML (single hardcoded wallet for iter-1,
      no encrypted store). `wallet-core/src/store.rs` stub for iter-2.
- [ ] **End-to-end test**: integration test that boots a fake indexer,
      sends a transfer, asserts the proof verifies, and the tx hex is
      well-formed. No node required for the test.

**Deliverable**: `cargo run -p dioxus-wallet` opens a window, types a
recipient + amount, clicks Send, sees "tx submitted; <hash>" within 5 s
(no real network). Documented in `WALLET_RESULTS.md`.

### Iteration 2 — "multi-wallet + multi-env + unshielded"

- [ ] Wallet menu UI: create / switch / delete / copy-seed. List wallets
      grouped by network.
- [ ] sled-backed `WalletStore`: per-wallet tree with checkpoint blob,
      event cache, key material.
- [ ] All 6 networks selectable; per-env endpoint overrides persisted.
- [ ] Genesis wallets W0–W3 surfaced as a "Quick start" panel on
      Undeployed only.
- [ ] Unshielded transfers via `ledger::structure::UnshieldedOffer`.
- [ ] NIGHT + custom token transfers (`coin_structure::TokenType`).
- [ ] Per-wallet sync handle: tokio task pulls indexer events
      incrementally, updates balance signal, persists checkpoint on
      successful epoch.

### Iteration 3 — "dust + diagnostics + failed-tx export"

- [ ] Dust register/deregister flows over `ledger::dust`.
- [ ] Per-subsystem sync progress (Shielded/Unshielded/Dust) in the
      header — phase + percent + ETA per gsd-wallet's pattern.
- [ ] Stall detection (≥ 30 s no event progress → "Stalled" indicator).
- [ ] Diagnostics panel: 2000-event ring buffer fed by a `tracing`
      Layer; level/category filters; NDJSON download.
- [ ] Failed-tx export: serialize tx hex + ledger params + error chain
      + workspace versions to a JSON file.

### Iteration 4 — "explorer + bundled snapshot + Android packaging"

- [ ] Explorer tab: indexer queries for tx/block/contract details.
- [ ] Bundled mainnet snapshot: extract a sled-format snapshot from a
      live mainnet sync, include as `assets/mainnet-snapshot.bin`,
      restore-on-first-launch path in `WalletStore`.
- [ ] Android APK packaging following the `mobile-bench/dioxus-bench`
      Gradle scaffold pattern.
- [ ] iOS investigation (no commitment yet — Dioxus 0.6 has rough iOS
      support; spike a one-screen build to gauge effort).

## Non-goals

- **DApp connector / `window.midnight`**: native apps don't host web
  pages, so this has no analogue. If we want the wallet to *be* connectable
  from a Chrome dApp, that requires a localhost WebSocket + browser-side
  helper extension — explicitly deferred.
- **Production crypto**: same disclaimer as gsd-wallet — seeds in plaintext.
  Encryption-at-rest is a distinct iteration we'll plan separately if and
  when the wallet leaves "developer/QA tool" scope.
- **Account abstraction / smart-wallet flows**: out of scope.

## Open questions / blockers

1. **Indexer GraphQL schema** lives in TS. Need to extract `.graphql`
   files from gsd-wallet (or upstream `@midnight-ntwrk/wallet-sdk-facade`)
   and pin a version. Decision before iter-1 starts: pin a v4 schema
   commit and copy in, or vendor the upstream `.graphql`?
2. **Node RPC**: gsd-wallet uses `@polkadot/api`. Rust equivalent
   (`subxt`?) needs sizing — full subxt may be overkill for the wallet's
   needs. Evaluate during iter-1.
3. **Dioxus 0.6 vs. 0.7**: iter-1 of `mobile-bench/dioxus-bench` is on
   0.6 because of the dx-vs-krates panic on this workspace
   (RESULTS.md:57). The wallet faces the same constraint. **Recommendation**:
   start on 0.6 to match, plan a 0.7 migration for iter-3 once the krates
   issue is upstream-fixed.
4. **Storage choice — sled vs. rusqlite**: sled is pure Rust, mobile-friendly,
   embedded, and matches gsd-wallet's per-wallet-namespace IndexedDB
   pattern. rusqlite gives proper SQL queries for the explorer/event
   panels but pulls in a C dep that complicates Android cross-compile.
   **Recommendation**: sled for iter-1/2; reassess if explorer needs
   indexed range queries.
5. **Concurrency model**: gsd-wallet runs the SDK in a Web Worker. We get
   tokio for free — but we still need to decide if proving runs on the
   tokio thread-pool (current `prover-core` design) or on a dedicated
   blocking pool. Iter-1 chooses tokio thread-pool; revisit if UI jank
   appears.

## Success criteria

- **Iter-1 ✅** when a developer can `cargo run -p dioxus-wallet`, send a
  shielded transfer to W1 from W0 against a local node, and see the
  receiver's balance update.
- **Iter-2 ✅** when the same flow works across all 6 networks, with
  multiple wallets per env, persisted across restarts.
- **Iter-3 ✅** when a failed tx produces a downloadable JSON debug bundle
  comparable to gsd-wallet's.
- **Iter-4 ✅** when a fresh install on a new Mac syncs mainnet to current
  tip in < 5 minutes (matching gsd-wallet's bundled-snapshot baseline).

## Reference index

Workspace entry points pulled from a survey on 2026-04-29; full map in
the kickoff thread:

- Ledger state: [ledger/src/structure.rs](../ledger/src/structure.rs) (`LedgerState<D>`)
- Tx construction: [ledger/src/construct.rs](../ledger/src/construct.rs) (`StandardTransaction::from_intents`)
- Seed → keys: [zswap/src/keys.rs](../zswap/src/keys.rs) (`Seed`, `SecretKeys::from_rng_seed`)
- Dust keys + spend: [ledger/src/dust.rs](../ledger/src/dust.rs)
- Storage backend: [storage-core/src/backend.rs](../storage-core/src/backend.rs) (`StorageBackend`)
- Local prover: [mobile-bench/prover-core/src/lib.rs](prover-core/src/lib.rs) (`ProverCore`)
- Proof-server HTTP wiring (reference): [mobile-bench/prover-core/src/http.rs](prover-core/src/http.rs)
- WASM JS-bridge surface (reference for what the TS facade exposes):
  `ledger-wasm/src/{tx.rs, unshielded.rs, zswap_wasm.rs}`
- Reference E2E: [ledger/tests/intent.rs](../ledger/tests/intent.rs)
- gsd-wallet upstream: <https://github.com/adamreynolds-io/gsd-wallet>
