# dioxus-wallet — iteration log

Captured 2026-04-29 on Apple Silicon (M2 Max).

## Iteration 1, step 1 — wallet setup + connectivity probe

Smallest deliverable from `WALLET_PLAN.md` iter-1: stand up the
`wallet-core` and `dioxus-wallet` crates, derive keys from a 32-byte
seed, and confirm the indexer + node URLs for a chosen network are
reachable from this host.

### What landed

- **`mobile-bench/wallet-core/`** (new crate): `Network` enum (all 6
  envs, URLs copied verbatim from gsd-wallet's
  `src/shared/environments.ts`), `Wallet` (random / deterministic /
  hex / chacha-seed constructors over `zswap::keys::SecretKeys`),
  `probe_connectivity` (parallel HTTP `__typename` GraphQL probe + two
  WS upgrade probes with a 5 s budget each).
- **`mobile-bench/dioxus-wallet/`** (new crate): Dioxus 0.6 desktop
  app with a network dropdown, "Generate random wallet" button,
  "Connect" button. Renders the seed hex + coin/encryption public
  keys in a card, plus a per-endpoint probe row (✓/✗ + latency +
  failure detail). Same per-target dioxus split as
  `dioxus-bench` — `mobile` on Android, `desktop` elsewhere.
- **Tests**: 5 wallet-core unit tests (URL distinctness, gsd-wallet
  parity, deterministic key derivation, hex roundtrip, invalid-seed
  rejection). 1 live preprod probe gated behind
  `--features network-tests`.

### Live preprod probe (first run)

```
cargo test -p wallet-core --features network-tests --test preprod_probe -- --nocapture
```

| Endpoint                                                              | Status | Latency |
|-----------------------------------------------------------------------|--------|--------:|
| `https://indexer.preprod.midnight.network/api/v4/graphql`             | ✅ `__typename=Query` | 703 ms |
| `wss://indexer.preprod.midnight.network/api/v4/graphql/ws`            | ✅                   | 925 ms |
| `wss://rpc.preprod.midnight.network`                                  | ✅                   | 1186 ms |

> One catch worth recording: the indexer WS endpoint **rejects bare
> upgrades with HTTP 400**. We must send
> `Sec-WebSocket-Protocol: graphql-transport-ws` on the upgrade, which
> the probe now does for `indexer_ws_url` only. Plain WS endpoints
> (the node) take a bare upgrade.

### Reproducing

```bash
# Unit tests (no network)
cargo test -p wallet-core

# Live preprod probe
cargo test -p wallet-core --features network-tests --test preprod_probe -- --nocapture

# Desktop UI
cargo run -p dioxus-wallet --release
# Click "Generate random wallet" → seed hex + coin/enc PKs render
# Click "Connect" → 3 probe rows render with latency
```

## Iteration 1, step 2 — real indexer + node queries

Captured 2026-04-29 against live preprod.

After surveying `midnightntwrk/{midnight-zk, midnight-node, midnight-indexer}`,
we replaced the planned hand-rolled clients with the maintainers' own
toolchain.

### What landed

- **Vendored from midnight-indexer** (sha-pinned at the commit time of
  capture):
  - `wallet-core/queries/midnight-indexer/schema-v4.graphql` (1439 lines)
  - `wallet-core/queries/midnight-indexer/e2e.graphql` (851 lines —
    held for upcoming queries; not yet referenced)
- **`wallet_core::indexer::IndexerClient`** — `graphql_client = 0.16`
  codegen against the vendored schema (same toolchain `indexer-tests`
  uses upstream). First query: `ChainTip` → returns hash, height,
  protocol version, timestamp, optional author.
- **`wallet_core::node::NodeClient`** — `jsonrpsee = 0.24` ws client;
  exposes `system_health` and `chain_getFinalizedHead` (combined
  helper `status()`). No metadata dep yet — that lands in iter-1
  step-3 when we need typed extrinsics for sending.
- **Connect screen wiring** — after the connectivity probe goes
  green, the UI fires both queries in parallel and renders a Chain
  state card with indexer tip + node peers + finalized head.

### Live preprod results

```
cargo test -p wallet-core --features network-tests --test preprod_probe -- --nocapture
```

Indexer `ChainTip`:
| Field             | Value                                                                |
|-------------------|----------------------------------------------------------------------|
| height            | 557 902                                                              |
| hash              | `139d0c4eb96f7de8eb6ba55d0682f5b34bd4358f21a3dcd8616e2da89c0443e1`   |
| protocol version  | 22000                                                                |
| timestamp         | 1777442604000 (ms)                                                   |
| author            | `2eac7fc733dc77b682d679abba3a742e0309b42f05b84233dc2155444bcd2240`   |

Node `system_health` + `chain_getFinalizedHead`:
| Field              | Value                                                                |
|--------------------|----------------------------------------------------------------------|
| peers              | 8                                                                    |
| isSyncing          | false                                                                |
| shouldHavePeers    | true                                                                 |
| finalized head     | `0xe4092ff48e4f166fd773261294fc55e07f75f1b260de41789bb20c47f093fc3c` |

All three live tests green:
```
preprod_indexer_and_node_reachable … ok
preprod_chain_tip_query             … ok
preprod_node_status_query           … ok
```

### Decisions captured

- **graphql_client over cynic**: matches indexer-tests pin; less
  cognitive load when refreshing the SDL.
- **jsonrpsee over subxt for phase-1**: `system_health` /
  `chain_getFinalizedHead` are substrate-standard; we don't need typed
  storage or extrinsic encoding yet. `subxt + midnight-node-metadata`
  comes back in step-3 for typed `Author.submitExtrinsic` flow.
- **Probe stays**: the cheap reachability check runs *before* the
  GraphQL/RPC requests, so the UI can short-circuit on unreachable
  endpoints without firing failing queries.

### Notes for the next step (still inside iter-1)

- **Schema pin**: probe is GraphQL-spec generic (`__typename`). The
  next slice needs gsd-wallet's queries pinned in
  `wallet-core/queries/*.graphql` and a real client (cynic or
  graphql-client). Decision pending: vendor from gsd-wallet vs. pin
  to upstream `wallet-sdk-facade`.
- **Persistence**: `platform::data_dir()` is already wired but unused;
  iter-2 will land sled-backed wallet storage there.
- **Address format**: we display raw hex of the serialized public key.
  Midnight has a bech32 address format we should adopt before
  iter-1's end so the wallet matches what the explorer shows.
- **Node RPC**: the WS upgrade succeeds; substrate-style JSON-RPC
  framing (request `system_health`, etc.) is the next probe to add
  if we want a stronger reachability signal than "TLS handshake
  completed".
