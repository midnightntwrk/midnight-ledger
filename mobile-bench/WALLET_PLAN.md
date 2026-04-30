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
+---+-----+--+--+---+        +-------------------------+
    |     |  |  |
    v     v  |  +-------------------+ git deps
 +--------+--+--+                   |
 | midnight-    |    +--------------v---------------+
 | ledger       |    | midnight-indexer schema      |
 | (this repo): |    |   indexer-api/graphql/       |
 |  - ledger    |    |     schema-v4.graphql        |
 |  - zswap     |    |   indexer-tests/e2e.graphql  |
 |  - coin-     |    |   indexer-tests/             |
 |    structure |    |     graphql_ws_client.rs     |
 |  - transient-|    +-----+-------------------------+
 |    crypto    |          |  vendored
 |  - storage   |          v
 +--------------+    +------------------+
                     | graphql_client   |  (codegen)
                     | + reqwest        |
                     | + tokio-         |
                     |   tungstenite    |
                     +------------------+
                                  |
                     +------------v-----------------+
                     | midnight-node-metadata       | git dep
                     |   (subxt::subxt!)            |
                     | + subxt RpcClient            |
                     | + pallet-midnight-rpc types  | git dep
                     +------------------------------+
                                  |
                                  v
                     +------------------------------+
                     | proof-server HTTP            |
                     |   (already wired via         |
                     |    prover-core::http)        |
                     +------------------------------+
```

### Decision — use upstream Midnight repos as deps; don't reinvent

A 2026-04-29 survey of `midnightntwrk/{midnight-zk, midnight-node,
midnight-indexer}` confirmed:

- **midnight-zk** is pure circuits/proofs (`midnight-{curves,proofs,circuits,zk-stdlib}`).
  Already pulled transitively via `transient-crypto` / `zkir`. **No
  wallet-shaped helpers** (no BIP39, bech32, address encoders). Skip as a
  direct dep.
- **midnight-indexer** ships the canonical schema at
  `indexer-api/graphql/schema-v4.graphql`, a wallet-shaped query set at
  `indexer-tests/e2e.graphql`, and a reference `graphql-transport-ws`
  client at `indexer-tests/src/graphql_ws_client.rs`. **No published
  client crate** (workspace `publish = false`) but the schema is the
  redistributable artifact and the maintainers themselves use
  `graphql_client = 0.16` codegen — so do we.
- **midnight-node** is a Substrate/FRAME node. Workspace `publish = false`,
  but `midnight-node-metadata` purpose-builds for off-node consumers:
  bundles SCALE metadata blobs (`midnight_metadata_0.21.0`, `0.22.0`,
  `1.0.0`, `_latest`) wired through `subxt::subxt!`. Custom RPC modules
  (`pallet-midnight-rpc`, `pallet-system-parameters-rpc`,
  `pallet-sidechain-rpc`, `pallet-session-validator-management-rpc`)
  define `jsonrpsee` server traits whose **request/response structs we
  can reuse client-side** for SCALE decoding via
  `subxt::backend::rpc::RpcClient`.

This shrinks the surface we have to invent.

### Crates we own (revised)

1. **`wallet-core/`** (new, exists) — pure-Rust wallet business logic.
   Owns:
   - `Network` enum + URLs (landed iter-1 step-1)
   - `Wallet` struct (seed, derived keys, sync state, indexer/node
     handles)
   - **`wallet_core::hd`** (port of
     `midnight-wallet/packages/hd`) over the `bip32` + `bip39`
     crates: BIP32 derivation along path
     `m/44'/2400'/<account>'/<role>/<index>` with role enum
     `NightExternal=0 | NightInternal=1 | Dust=2 | Zswap=3 |
     Metadata=4`.
   - **`wallet_core::address`** (port of
     `midnight-wallet/packages/address-format`) over `bech32`:
     bech32m HRP `mn_<type>[_<network>]`. iter-1 needs
     `addr` (unshielded) only; shielded + dust codecs land later.
   - **`wallet_core::sync::{progress, unshielded, dust, shielded}`**
     (port of `midnight-wallet/packages/{abstractions,
     unshielded-wallet, dust-wallet, shielded-wallet}/src/v1/Sync.ts`):
     `SyncProgress` struct + per-asset `applyUpdate` folds.
   - **Vendors `schema-v4.graphql` + `e2e.graphql`** under
     `wallet-core/queries/midnight-indexer/`, runs `graphql_client`
     codegen at build time. **Lifts `graphql_ws_client.rs`** from
     midnight-indexer (vendored, with attribution) for
     `graphql-transport-ws` subscriptions.
   - `wallet_core::indexer::IndexerClient` (landed: `chain_tip`).
   - `wallet_core::node::NodeClient` over `jsonrpsee` (landed:
     `system_health`, `chain_getFinalizedHead`); upgrades to
     `subxt` + `midnight-node-metadata` (git dep) for typed
     extrinsic submission when iter-1 send lands.
   - `WalletStore` (sled-backed persistence — iter-2).
   - Tx builders + local/remote prover dispatch — iter-1 send
     onwards.
2. **`dioxus-wallet/`** (new, exists) — the app crate. Mirrors the
   layout of `mobile-bench/dioxus-bench`. Mobile dark-theme spec
   in [`MOBILE_WALLET.md`](MOBILE_WALLET.md).

**Crates we are not introducing** (vs. the original plan):
- ~~`indexer-client/`~~ — folded into `wallet-core` per the indexer's
  own toolchain (graphql_client codegen against vendored schema).
- ~~`node-rpc-client/`~~ — folded into `wallet-core` via `subxt` +
  `midnight-node-metadata` git dep.

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

Step-1 already shipped (wallet-core skeleton, Network enum, Wallet
keys, connectivity probe). Remaining steps within iter-1:

- [x] **Skeleton**: `wallet-core/` + `dioxus-wallet/` crates exist.
- [x] **Network config**: `Network` enum, all 6 envs URLs verbatim
      from gsd-wallet's `environments.ts`.
- [x] **Seed → keys**: `Wallet::from_seed()` over
      `zswap::keys::SecretKeys`.
- [x] **Connectivity probe**: `probe_connectivity()` confirms
      indexer HTTP / WS (with `graphql-transport-ws` subprotocol) /
      node WS reachability. Live preprod = green.
- [ ] **Indexer queries (real)**: vendor
      `indexer-api/graphql/schema-v4.graphql` + relevant queries from
      `indexer-tests/e2e.graphql` into `wallet-core/queries/`; add
      `graphql_client` codegen build wiring; expose `chain_tip()` /
      `block_at(height)` / `unshielded_transactions(address)` and a
      `subscribe_shielded_events()` stream over `graphql-transport-ws`.
- [ ] **Node RPC** (phased):
   - **Phase 1 (this step)**: `jsonrpsee::ws_client` raw RPC —
     `system_health()`, `chain_get_finalized_head()`. No metadata
     dependency. Substrate-node standard methods work the same on
     all 6 envs. ~6 transitive deps.
   - **Phase 2 (iter-1 send)**: `subxt::backend::rpc::RpcClient` +
     `midnight-node-metadata` (git dep) for typed extrinsic
     submission and `pallet-midnight-rpc` SCALE-decoded responses.
- [ ] **Shielded send**: `Wallet::send_shielded(to, amount, token)`
      builds `ZSwap` offer + `StandardTransaction` via
      `ledger::construct::StandardTransaction::from_intents()`, hands
      `ProofPreimage` to `prover-core::ProverCore`, posts proven tx
      via `subxt::tx::sign_and_submit_then_watch`.
- [ ] **Persistence**: settings TOML in `data_dir/` (single hardcoded
      wallet for iter-1; sled lands in iter-2).
- [ ] **End-to-end test**: integration test against
      [`midnight-indexer-standalone`](https://github.com/midnightntwrk/midnight-indexer)
      brought up via docker (or test harness mock when CI cannot run
      docker). Assert the proven tx round-trips through indexer
      events.

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

1. ~~**Indexer GraphQL schema** lives in TS.~~ **Resolved**: vendor
   `indexer-api/graphql/schema-v4.graphql` + `indexer-tests/e2e.graphql`
   from `midnightntwrk/midnight-indexer` at a pinned `release/4.2.x`
   tag and run `graphql_client = 0.16` codegen — same toolchain the
   indexer maintainers use.
2. ~~**Node RPC**: gsd-wallet uses `@polkadot/api`.~~ **Resolved**:
   depend on `subxt` + `midnight-node-metadata` (git dep, pinned tag);
   reuse `pallet-midnight-rpc`'s `*RpcResponse` structs (also git dep)
   for SCALE-decoding the custom `midnight_*` / `systemParameters_*`
   methods via `subxt::backend::rpc::RpcClient`.
3. **Dioxus 0.6 vs. 0.7**: iter-1 of `mobile-bench/dioxus-bench` is on
   0.6 because of the dx-vs-krates panic on this workspace
   (RESULTS.md:57). The wallet faces the same constraint.
   **Recommendation**: start on 0.6 to match, plan a 0.7 migration for
   iter-3 once the krates issue is upstream-fixed.
4. **Storage choice — sled vs. rusqlite**: sled is pure Rust,
   mobile-friendly, embedded, and matches gsd-wallet's
   per-wallet-namespace IndexedDB pattern. rusqlite gives proper SQL
   queries for the explorer/event panels but pulls in a C dep that
   complicates Android cross-compile. **Recommendation**: sled for
   iter-1/2; reassess if explorer needs indexed range queries.
5. **Concurrency model**: gsd-wallet runs the SDK in a Web Worker. We
   get tokio for free — but we still need to decide if proving runs on
   the tokio thread-pool (current `prover-core` design) or on a
   dedicated blocking pool. Iter-1 chooses tokio thread-pool; revisit
   if UI jank appears.
6. **`midnight-node-metadata` version pinning**: bundles
   `midnig​ht_metadata_{0.21.0, 0.22.0, 1.0.0, _latest}`. Default to
   `_latest` for preprod/mainnet and let the wallet downgrade per
   chain runtime version on connect; track the `pallet-midnight-rpc`
   git rev separately so SCALE response types stay aligned.
7. **Indexer wallet session lifecycle**: the v4 schema models a
   wallet as `Mutation.connect(viewingKey) → sessionId` then a per-
   subscription session. Need to decide reconnect semantics on iter-2
   sled-restore — call `connect` lazily on first balance read, keep
   `sessionId` in memory only.

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

Workspace entry points pulled from surveys on 2026-04-29; full maps
in the kickoff threads.

**This workspace (midnight-ledger):**
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

**Companion plans:**
- [`MOBILE_WALLET.md`](MOBILE_WALLET.md) — mobile UI / dark theme spec
- [`DID_PLAN.md`](DID_PLAN.md) — Midnight DID Rust-native port
  (decision 2026-04-30: drop in-WebView TS approach; build native
  Rust API matching midnight-did-domain + midnight-did-api)

**External Midnight repos (git deps / vendored sources):**
- **midnight-wallet** (TS port source for HD, address-format, sync):
  <https://github.com/midnightntwrk/midnight-wallet>
  - `packages/hd/src/HDWallet.ts` — BIP32 path
    `m/44'/2400'/a'/r/i`, roles
    `NightExternal=0 | NightInternal=1 | Dust=2 | Zswap=3 |
    Metadata=4`
  - `packages/hd/src/MnemonicUtils.ts` — BIP39 (24-word default)
  - `packages/address-format/src/index.ts` — bech32m, HRP
    `mn_<type>[_<network>]`, codecs for `addr` /
    `mn_shield-addr` / `mn_dust-…` / `mn_shield-{cpk,epk,esk}`
  - `packages/abstractions/src/SyncProgress.ts` — sync state shape
  - `packages/{dust,unshielded,shielded}-wallet/src/v1/Sync.ts` —
    per-asset apply-update folds
  - `packages/indexer-client/src/graphql/subscriptions/{Dust,
    Unshielded,Shielded}.ts` — operation strings + response shapes
- **midnight-indexer** schema + reference WS client:
  <https://github.com/midnightntwrk/midnight-indexer>
  - `indexer-api/graphql/schema-v4.graphql` (vendored)
  - `indexer-tests/e2e.graphql` (vendored, used in iter-1 step-3+)
  - `indexer-tests/src/graphql_ws_client.rs` (lift in Phase B)
- **midnight-node** metadata crate (subxt entry point):
  <https://github.com/midnightntwrk/midnight-node/tree/main/metadata>
- midnight-node custom RPC traits + response structs:
  `pallet-midnight-rpc`, `pallet-system-parameters-rpc`,
  `pallet-sidechain-rpc`, `pallet-session-validator-management-rpc`
- midnight-zk (transitively via this workspace, no direct dep):
  <https://github.com/midnightntwrk/midnight-zk>
- **example-counter** (iter-1 functional bar — TS reference flow):
  <https://github.com/midnightntwrk/example-counter/blob/main/counter-cli/src/cli.ts>
- gsd-wallet (TS reference): <https://github.com/adamreynolds-io/gsd-wallet>
- 1am.xyz (mobile UI/UX inspiration): <https://1am.xyz/>
- gsd-wallet upstream: <https://github.com/adamreynolds-io/gsd-wallet>
