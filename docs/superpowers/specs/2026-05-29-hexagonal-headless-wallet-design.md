# Hexagonal Headless Wallet — Architecture & Refactor Plan

**Date:** 2026-05-29
**Status:** Draft — design phase
**Scope:** `mobile-bench/wallet-core/` + `mobile-bench/dioxus-wallet/` + new
`mobile-bench/headless-wallet/` crate. The upstream Midnight crates
(`zswap`, `transient-crypto`, `ledger`, etc.) and the broader workspace
are out of scope; this design consumes them as fixed dependencies.

---

## Summary

The Midnight wallet's `wallet-core` crate is already partly hexagonal — nine
ports (`HttpClient`, `IndexerClient`, `NodeClient`, `Prover`, `SecretStorage`,
`VcStorage`, `Clock`, `Metrics`, `ResourceProbe`) with real + mock adapters,
and four use-case functions that consume them cleanly
(`bootstrap_did_with_keys`, `oid4vp_run_authentication`,
`oid4vci_run_issuance`, `self_verify_and_cache`). The pattern works.

The remaining business logic lives inside Dioxus `#[component]` functions —
each section in `identity_centre.rs` and each callback in `app.rs` carries
its own `spawn(async move { … })` body that *is* the use-case. The same
"guard + locator" prelude appears four times, and `BridgeState` plus a
family of process-wide statics carry the dependencies that should be
plumbed explicitly. None of these flows can be exercised without the UI.

This document plans the refactor that finishes the hexagon: extract every
use-case into a service that takes its ports as `Arc<dyn Trait>` fields;
build a headless wallet binary that drives every service over a
line-delimited JSON protocol; write integration tests that drive the
headless binary against a fresh standalone Midnight env, covering every
flow the UI exposes. The ultimate invariant: anything reachable from a
Dioxus button is also reachable from a `headless-wallet` verb. If it
isn't, the logic has leaked into the component and needs to be promoted
back into a service.

The plan has three sections:

- **§1 Current State** — port inventory, UI-coupled use-case sites,
  integration test coverage, twelve concrete coupling smells.
- **§2 Target Architecture** — domain core + value objects + ten use-case
  services + the full port catalog + the headless wallet binary + the
  UserInterface adapter pattern + the three-layer test strategy + the
  manual dependency injection container.
- **§3 Refactor Plan** — seven sequenced waves of work (A-G), each
  shippable, no big-bang rewrite, with effort estimates and three
  pre-known risks called out.

Estimated total effort: ~41-58 commits / 3-4 weeks single-developer.

---

## §1 Current State

This section inventories what exists today in the two crates in scope on
branch `dioxus-vc-demo`:

- `mobile-bench/wallet-core/` — a pure-Rust library that already exposes
  several "port" traits backed by both real-deps and in-memory/test
  adapters.
- `mobile-bench/dioxus-wallet/` — the Dioxus UI shell, where a god-object
  `BridgeState` plus a family of process-wide statics carry the
  dependencies that should be plumbed through use-cases.

It is descriptive: existing files, traits, adapters, and call sites — no
recommendations.

### 1.1 What's already in hexagonal shape

`wallet-core` carries seven ports with explicit `Send + Sync + 'static`
traits, each paired with a real-deps adapter and (almost universally) a
gated test adapter behind `#[cfg(any(test, feature = "test-support"))]`.
The `Wallet` aggregate root accepts these via fluent
`with_indexer / with_node / with_prover / with_metering /
with_proof_server_url / with_dust_syncer / with_js_bridge` setters
(`mobile-bench/wallet-core/src/wallet.rs:319-571`).

| Port (trait) | File | Methods | Real adapter | Test adapter | Used by |
|---|---|---|---|---|---|
| `HttpClient` | `http.rs:39-51` | `get(&str)`, `post_json(&str, &Value, Option<&str>)` | `ReqwestHttpClient` (`http.rs:55`) | `mock::MockHttpClient` (`http.rs:99-175`) — `push_response/push_json/push_status_body`, records all requests | `oid4vp_client`, `oid4vci_client`, `did_auth`, `vc_self_verify`, Identity Centre sections in dioxus-wallet |
| `IndexerClient` | `chain.rs:43-55` | `chain_tip() -> Option<ChainTipInfo>`, `contract_state(address_hex) -> Option<ContractStateInfo>` | `HttpIndexerClient` (`indexer.rs`) via `chain::default_indexer(network)` | `StubIndexerClient` in `test_support.rs:60-80`, shares `StubDidMap` with stub wallet | `Wallet` chain-ops; also constructed inline by `app.rs::connect()` for the probe step |
| `NodeClient` | `chain.rs:63-74` | `submit_deploy(bytes, &MidnightSigner) -> SubmitResult` | `SubxtNodeClient` (`node/client.rs`) | none yet — stub wallet routes around `submit_deploy` via the in-process bypass | `Wallet` deploy path |
| `Prover` | `chain.rs:82-87` | `prove(UnprovenTx) -> ProvenTx` | `LocalProver` (in-process zkir, `chain.rs:94-102`) or `HttpProver` (proof-server URL, `chain.rs:109-128`) — selected by `default_prover(proof_server_url)` | none in `test_support.rs` (stub bypass short-circuits earlier) | `Wallet` write circuits |
| `SecretStorage` | `secret_storage/types.rs:239-350` | `initialize / list_keys / generate_key / import_key / derive_key_from_seed / get_public_key / sign / verify / delete_key`, default `import_ed25519`, `import_jubjub`, `find_by_kid` | `FileSecretStore` (`file_secret_store.rs:109`), `RedbSecretStore` (`redb_secret_store.rs:37`) — UI uses redb | `InMemorySecretStore` (`in_memory.rs:38`) | Bootstrap, OID4VP/VCI auth, VC self-verify, Identity Centre sections |
| `VcStorage` | `vc_store/mod.rs:32-61` | `insert_vc / get_vc / insert_opening / get_opening / get_metadata / list_ordered / delete_vc / insert_vc_with_openings / update_metadata` (uses `&mut dyn FnMut` for object safety) | `RedbVcStore` (`vc_store/api.rs:33`) | `InMemoryVcStore` (`vc_store/in_memory.rs:12`) | OID4VCI issuance, `self_verify_and_cache`, `VcInventorySection` |
| `Clock` | `clock.rs:17-25` | `now_ms() -> u64` (single method by design) | `SystemClock` (`clock.rs:30`) | `FixedClock` with atomic `set`/`advance` (`clock.rs:46-69`) | `self_verify_and_cache`, OID4VP/VCI flows for timestamping |
| `Metrics` | `telemetry/mod.rs:94-102` | `record_http(&HttpRecord) / record_op(&OpRecord) / incr(&str, u64)` | `InMemoryMetrics` (locked counters + histograms), `TracingMetrics` (re-emits to `tracing::info!`), `CompositeMetrics` fan-out | `NoopMetrics` default in same file | All Identity Centre flows wrap calls in `time_op` against this port; `MeteredHttpClient` wraps `HttpClient` in this port |
| `ResourceProbe` | `telemetry/resource.rs:38-40` | `sample() -> Option<ResourceSample>` (rss_kb, cpu_us) | `RusageProbe` (POSIX `getrusage`, `resource.rs:55-98`) | `NoopResourceProbe` (`resource.rs:46-52`) | `time_op` brackets across `BootstrapSection`, `Oid4vpSection`, `Oid4vciSection`, `VcInventorySection` |
| `JsBridge` | `js_bridge.rs:57-93` | `request_json(&serde_json::Value) -> serde_json::Value`, plus `JsBridgeExt` for typed wrappers | `NodeChildBridge` (spawns Node harness, `js_bridge.rs:102-260`); `eval_bridge::DioxusEvalBridge` in `dioxus-wallet/src/eval_bridge.rs` | none — `js_bridge_smoke.rs` test exercises `NodeChildBridge` directly | `Wallet::call_did_circuit` / `prepareUnprovenCallTx` path |

A few details worth flagging:

- The ports use one shape consistently — a trait with `Send + Sync +
  'static`, an `async_trait` for async methods, the real adapter living
  next to the trait, and the test adapter feature-gated to keep it out
  of release builds. `HttpClient`, `Clock`, `SecretStorage`, and
  `VcStorage` all repeat the same pattern verbatim.
- The `Wallet` struct in `wallet.rs` is the only place ports are
  composed; `with_*` setters mutate the wallet by move, so call sites
  build a wallet then mutate it through a chain (`base.with_proof_server_url(url).with_dust_syncer(syncer).with_js_bridge(b)`).
- `test_support.rs` (`mobile-bench/wallet-core/src/test_support.rs:429
  lines`) provides a `stub_wallet` factory that wires `StubIndexerClient`
  + a shared `StubDidMap` + `InMemorySecretStore` so the four core DID
  write/read methods bypass the JS contract layer and operate on an
  `Arc<Mutex<HashMap<DidId, DidDocument>>>` directly. Path 2
  (wallet-level bypass) is in; Path 1 (full pipeline fidelity) is
  documented as deferred.
- `MeteredHttpClient` (`telemetry/http_metered.rs`) and
  `MeteredIndexerClient` / `MeteredNodeClient` / `MeteredProver`
  (`telemetry/chain_metered.rs`) are decorators that wrap any port impl
  and emit `HttpRecord` / `OpRecord` to the injected `Metrics` — non-
  invasive (no flow signatures change to opt in).

What's not yet a port:

- `tx::prove::prove` and `prove_via_http` are still free functions; the
  `Prover` trait wraps them but the underlying `reqwest::Client` build
  is per-call.
- Network connectivity (`probe::probe_connectivity`) is not behind a
  trait; it directly constructs `reqwest` + `subxt` clients.
- `Wallet::sync_unshielded` and `Wallet::sync_dust` are concrete methods
  on the aggregate; sync orchestration is inside the wallet, not behind
  a separate port. `DustSyncer` is exposed and injectable via
  `with_dust_syncer`, but is a concrete struct, not a trait.

### 1.2 What's still business logic mixed with UI

#### `dioxus-wallet/src/identity_centre.rs` (966 lines)

Four `#[component]` sections, each containing `spawn(async move { … })`
blocks that perform the use-case logic inline:

- **`BootstrapSection`** (lines 162-415). Mounts a "Bootstrap DID with VC
  keys" button; the `bootstrap` closure (lines 215-306) reads
  `bridge_state.store()`, `bridge_state.active_wallet_id()`,
  `bridge_state.metrics_dyn()`, `bridge_state.metrics()`,
  `bridge_state.resource_probe()`, builds a `RedbSecretStore`, a
  metered wallet via `metered_app_wallet_for(network, metrics, probe)`
  (lines 52-69), wraps the call in `time_op(…, "bootstrap_did",
  bootstrap_did_with_keys(&wallet, &mut secret_store, &DEMO_IC_SEED))`,
  then writes the per-DID controller secret back through
  `bridge_state.remember_controller_secret(network, did, sk)` and
  invokes `on_did_minted.call((did_str, network))` so the App can
  insert a Pending entry into the `did_inventory` signal. The use-case
  body — orchestration of bootstrap + persistence — lives in the
  component.
  - Sketch: `use_case::bootstrap_identity_centre_did(deps) ->
    BootstrapOutcome` where `deps` carries `wallet`, `secret_store`,
    `controller_secret_writer`, `inventory_writer`, `metrics`.
  - Lines 188-213 contain a `use_effect` that does its own probe of
    the secret store at mount time (`find_by_kid` lookalike via
    `list_keys(None)` then linear scan for `#key-auth`), again on the
    UI side.
  - Lines 318-345 contain another `use_effect` that polls
    `bridge_state.log_capture().snapshot()` every 500 ms to feed an
    "activity feed" Signal — UI-driven log scraping.

- **`Oid4vpSection`** (lines 419-573). The `authenticate` closure (lines
  430-508) collects URL + DID + secret store + wallet ID guards, builds
  `MeteredHttpClient::new(ReqwestHttpClient, metrics)`, wraps
  `oid4vp_run_authentication(&*http, &clock, &url, &wallet,
  &secret_store, &did)` in `time_op`, and writes the outcome to two UI
  signals (`ok_msg` / `err_msg`) plus bumps two counter metrics
  (`oid4vp.ok` / `oid4vp.failed`).
  - Sketch: `use_case::oid4vp_authenticate(deps, url, ic_did) ->
    AuthOutcome`.
  - A second closure `scan` (lines 510-534) drives the
    `eval_bridge::global_bridge()` QR scanner — pure UI input plumbing.

- **`Oid4vciSection`** (lines 577-746). The `request_vc` closure
  (lines 588-684) repeats almost the same prelude (store guard, wallet
  ID guard, metrics, metered http, system clock), opens a `RedbVcStore`
  at `vc_store_path()`, and runs
  `time_op("issuance", oid4vci_run_issuance(&*http, &clock, &url,
  &wallet, &secret_store, &did, &vc_store))`. The VC URI is then
  surfaced via `ok_msg`.
  - Sketch: `use_case::oid4vci_request_credential(deps, url, ic_did) ->
    Result<VcUri>`.

- **`VcInventorySection`** + `render_vc_row` (lines 789-966). The
  `vcs` resource calls `RedbVcStore::open(vc_store_path()).list_ordered()`
  inside `use_resource`. Each row's `verify` closure (lines 852-941)
  again grabs store + wallet ID, opens the same VC store again, runs
  `time_op_simple("self_verify", self_verify_and_cache(&vc, &wallet,
  &secret_store, &vc_store, &clock))`, updates a `badges` HashMap, and
  bumps `verifies.valid / .invalid / .error` counters.
  - Sketch: `use_case::self_verify_vc(deps, vc_uri) ->
    SelfVerifyOutcome`; `use_case::list_vcs(deps) -> Vec<StoredVc>`.

Each of these four sections re-implements the same guard prelude:

```rust
let Some(store) = bridge_state.store().cloned() else { err_msg.set(…); return; };
let Some(wallet_id) = bridge_state.active_wallet_id() else { err_msg.set(…); return; };
let metrics = bridge_state.metrics_dyn();
let in_mem_metrics = bridge_state.metrics();
let probe = bridge_state.resource_probe();
let wallet = metered_app_wallet_for(network, metrics.clone(), probe.clone());
let secret_store = RedbSecretStore::new(store, wallet_id);
```

That prelude is identical at four call sites and is effectively the
service-locator pattern.

#### `dioxus-wallet/src/app.rs` (8861 lines)

Several closures embedded in components drive substantial flows:

- **`connect()` closure** (lines 1507-1584). Probes connectivity via
  `probe_connectivity(net)`, then joins two futures inline —
  `HttpIndexerClient::new(net).chain_tip()` and
  `SubxtNodeClient::connect(net).await.status()` — to populate a
  `ChainSnapshot` signal. Bypasses the wallet entirely, talking to the
  real-deps adapters directly. On success, bumps `sync_trigger` so the
  `WalletSyncPane` cascades the NIGHT + DUST sync.
  - Sketch: `use_case::probe_and_snapshot_chain(deps, network) ->
    ChainSnapshot`.

- **Unlock + store-open flow** (lines 1196-1429, embedded in App's
  `on_unlock`). Opens `wallet_core::store::WalletStore::open(&path,
  &passphrase)`, calls `state.set_store(store.clone())`, spawns the
  log persistence drainer, derives a `DustSyncer` from a temporary
  `app_wallet_for(net)`, registers it with
  `set_dust_syncer_for(net, syncer)`, runs `seed_preprod_live_state`
  + `seed_preprod_live_keys` under the `preprod-live` feature,
  hydrates controller secrets via `state.hydrate_controller_secrets(net)`,
  loads persisted inventory + resolved cache, restores last-session
  state, then for every hydrated DID spawns a per-DID auto-resolve
  task (lines 1356-1428) that updates both the in-memory signal and
  the redb table.
  - Sketch: `use_case::unlock_wallet(deps, passphrase) -> UnlockOutcome`
    + `use_case::refresh_inventory(deps, network) -> Vec<InventoryEntry>`.

- **Create-DID wizard's `on_done`** (lines 1902-2068, inside the Dids
  tab `match` arm). On a `DeployOutcome`: persist `controller_sk` via
  `BridgeState::remember_controller_secret`, build a `Pending`
  `DidInventoryEntry`, mutate `did_inventory` signal, call
  `persist_inventory_entry(&bridge, net, &entry)`, log a `Deploy`
  event into `session_log`, and spawn a 10-attempt × 3-sec back-off
  retry loop calling `app_wallet_for(net).resolve_did_full(&did)` to
  flip Pending → Active and overwrite the entry. The retry loop is
  inlined inside the EventHandler closure.
  - Sketch: `use_case::deploy_did(deps, params) -> DeployOutcome` +
    `use_case::wait_for_resolution(deps, did, attempts) -> ResolvedDid`.

- **Dids tab `on_resolved` / `on_seen`** (lines 2065-2090,
  3074-3140). The `DidInventoryPanel` emits `on_resolved((did,
  counter))` and `on_seen(DidInventoryEntry)`; the parent's handler
  reaches into `did_inventory.read().clone()`, inserts/updates, sets
  back, then calls `persist_inventory_entry`. The "Resolve" button
  inside `DidInventoryPanel` itself (lines 3107-3140) spawns
  `app_wallet_for(net).resolve_did_full(&did)`, constructs an entry,
  and fires the callbacks — orchestrator logic in the component.
  - Sketch: `use_case::resolve_did(deps, did) -> InventoryEntry`.

- **Deactivate flow** (`DidDetailView::deactivate`, lines 6686-6761).
  Builds an `app_wallet_for(network)`, parses the DID, takes a
  `balance_snapshot` for cost accounting, opens a `call_did_circuit`
  stream with `"deactivate"`, loops over `WizardStage` updates while
  pushing each into a `deactivating: Signal<Vec<WizardStage>>`, fires
  `on_deactivated` + `on_timing` + `on_cost` callbacks at terminal
  stages, and computes a final `CostRun` from the before/after
  balance.
  - Sketch: `use_case::deactivate_did(deps, did, controller_sk) ->
    Stream<DeactivateStage>`.

- **Operation Builder submit flow** (`DidOperationBuilder`'s submit
  spawn, lines 3727-4050+). For each row in the queue: if the verifier
  key isn't loaded, spawn `wallet.load_did_circuit(did, circuit,
  counter)` and wait for its terminal stage; sleep 30 s for indexer
  catch-up; then spawn `wallet.call_did_circuit(did, circuit,
  args_json, sk)` and stream stages; update row status; fire
  `on_event` / `on_resolved` / `on_cost`. Counter cursor + loaded-set
  state lives in the closure's local variables; row state lives in a
  `Signal<Vec<(DidOperation, QueueStatus)>>`.
  - Sketch: `use_case::run_op_batch(deps, did, ops) ->
    Stream<BatchStageUpdate>`.

#### `dioxus-wallet/src/bridge.rs` (`BridgeState`, lines 50-285)

`BridgeState` is the de facto dependency container. Composition:

- `proof_server_url: Arc<OnceCell<String>>` — set once at App boot
  when the embedded proof-server spawns; read by `app_wallet_for` via
  the parallel `app::PROOF_SERVER_URL` static.
- `controller_secrets: Arc<Mutex<HashMap<String, [u8; 32]>>>` —
  in-memory hot cache of per-DID 32-byte controller secrets.
- `store: Arc<OnceCell<WalletStore>>` — persistent backing.
- `active_wallet_id: Arc<Mutex<Option<WalletId>>>` — pinned for
  the network the UI is bound to.
- `log_capture: Arc<OnceCell<LogCapture>>` — process-wide tracing ring
  buffer.
- `metrics: Arc<InMemoryMetrics>` — telemetry aggregator.
- `resource_probe: Arc<RusageProbe>` — RSS/CPU sampler.

The struct derives custom `PartialEq` via `Arc::ptr_eq` of every field
because Dioxus' `#[component]` macro requires props to be `Eq`. That
constraint forces `BridgeState` to be cloned through the prop tree as a
single bundle.

`BridgeState`'s methods conflate three layers:

1. Persistence: `remember_controller_secret`,
   `hydrate_controller_secrets`, `controller_secret_for_on` (writes
   through to `WalletStore::put_controller_secret /
   list_controller_secrets`).
2. In-memory cache: `controller_secret_for` (legacy network-less
   accessor for the bridge RPC hot path).
3. Telemetry/log handles: `metrics`, `metrics_dyn` (composes
   `InMemoryMetrics` + `TracingMetrics`), `resource_probe`,
   `log_capture`.

It also owns the wallet-identity binding (`set_active_wallet_id`,
`active_wallet_id`) and the proof-server URL slot.

Every UI section dereferences this struct to assemble its dependency
graph: `bridge_state.store().cloned() / .active_wallet_id() /
.metrics_dyn() / .metrics() / .resource_probe() /
.remember_controller_secret(...) / .controller_secret_for_on(...)`.

### 1.3 What the integration tests already cover

`mobile-bench/wallet-core/tests/` carries 22 test files. They fall into
four bands:

| File | Exercises | Adapters | Gating |
|---|---|---|---|
| `js-harness/` (dir) | Node harness — runs the Compact runtime + DID contract layer for the Rust-side JS bridge. Not a Rust test; it's the Node child invoked by `NodeChildBridge` | `NodeChildBridge` real adapter | Build-time; needs `npm install` once |
| `js_bridge_smoke.rs` | Round-trip ping + RPC over `NodeChildBridge`; verifies the JSON-RPC transport without involving Compact runtime, contracts, or chain | Real `NodeChildBridge` | `cargo test`, no feature gate |
| `js_inspect_circuits.rs` | Offline coverage for every DID write circuit via the Node harness; for each, build state → run circuit in JS → deserialise `ProofPreimage` on Rust → assert structural invariants | Real `NodeChildBridge`, no chain | `cargo test`, no feature gate |
| `js_inspect_deactivate.rs` | Same as above, but focused on the `deactivate` circuit | Real `NodeChildBridge`, no chain | `cargo test`, no feature gate |
| `js_prepare_call_tx.rs` | Asserts the harness's `prepareUnprovenCallTx` produces a SCALE-serialised `UnprovenTransaction` for a DID circuit call | Real `NodeChildBridge`, no chain | `cargo test`, no feature gate |
| `jubjub_schnorr_interop.rs` | Cross-impl interop — upstream JS reference via JSON-RPC harness vs `secret_storage::jubjub_schnorr` Rust impl on the same inputs | Real `NodeChildBridge` + Rust crypto | `cargo test`, no feature gate |
| `did_bootstrap_standalone.rs` | `bootstrap_did_with_keys` end-to-end against the standalone docker-compose Midnight stack. **Currently breaks** at the Compact-JS contract layer (the Node-child bridge can't carry that state). Both tests `#[ignore]`'d | Real `HttpIndexerClient`, `SubxtNodeClient`, `HttpProver`, `NodeChildBridge` | `#[ignore]`; needs `STANDALONE_RUN=1` + docker stack |
| `deploy_undeployed_live.rs` | `Wallet::create_did()` end-to-end against the local standalone stack | All real, `Network::Undeployed` | `#![cfg(feature = "network-tests")]` + docker stack |
| `load_circuit_undeployed_live.rs` | `Wallet::load_did_circuit` (MaintenanceUpdate to register a circuit's verifier key on a freshly-deployed DID) | All real | `network-tests` feature + docker |
| `call_circuit_undeployed_live.rs` | Full create → load → call → resolve loop driving the deactivate circuit via the JS path | All real + JS bridge | `network-tests` feature + docker |
| `batch_autoload_undeployed_live.rs` | Operation Builder's auto-load path — verifier key not on chain, builder prepends a `load_did_circuit`, then the original call | All real | `network-tests` feature + docker |
| `batch_circuits_undeployed_live.rs` | Operation Builder's batched submission — multiple write circuits in sequence | All real | `network-tests` feature + docker |
| `preprod_probe.rs` | Reachability check against the live PreProd indexer + node | Real `HttpIndexerClient` / `SubxtNodeClient` | `network-tests` feature; live PreProd |
| `preprod_probe_indexer_state.rs` | Confirms the extended `contract_state.graphql` returns `zswap_state_hex` + `ledger_parameters_hex` fields on PreProd | Real indexer | `network-tests` feature; live PreProd |
| `preprod_smoke_live.rs` | Read-only inventory resolve + (gated) MaintenanceUpdate writes against PreProd | All real | `network-tests` + `#[ignore]` on the write tests |
| `preprod_maintenance_authority.rs` | Probe + gated write path for the DID maintenance authority on PreProd | All real | `network-tests` feature; live PreProd |
| `preprod_vk_diff.rs` | Tagged-serialisation parity check: on-chain VK bytes vs bundled `.verifier` files | Real indexer | `network-tests`; live PreProd |
| `preprod_fetch_tx_bytes.rs` | Pulls SCALE-encoded inner-Midnight bytes from a known PreProd `send_mn_transaction` extrinsic | Real subxt | `network-tests`; live PreProd |
| `preprod_decode_diff.rs` | Diffs `/tmp/our-tx.hex` against `/tmp/upstream-tx.hex` | None | `network-tests`; offline diff |
| `annotate_preprod_keys.rs` | Resolves each PreProd-default DID, walks VMs, prints `key_id → did` mapping | Real indexer | `network-tests`; live PreProd |
| `balance_diagnostic.rs` | Always-passes diagnostic: prints NIGHT + DUST balance for the Undeployed demo wallet on standalone | All real | `network-tests` + docker |
| `import_manager_keys.rs` | One-shot importer for keys exported from the manager profile's `manager-secrets.json` into the wallet's secret store | Real `RedbSecretStore` | `network-tests`; offline |
| `unshielded_live.rs` | UTXO snapshot + sync against PreProd | Real | `network-tests`; live PreProd |

Summary:

- `cargo test --lib` (no features): the JS-bridge tests + interop test
  run (they need only `npm install` in `tests/js-harness/`). Nothing
  here exercises the wallet's port traits via the
  `test_support::stub_wallet` factory — `test_support.rs` is consumed
  only by inline `#[cfg(test)]` modules inside the wallet-core crate
  itself, not by the integration-test files.
- `cargo test -p wallet-core --features network-tests`: pulls in the
  `preprod_*` + `*_undeployed_live` + `unshielded_live` set, requires
  either docker-compose standalone or live PreProd reachability.
- `#[ignore]` is used in only two places — `did_bootstrap_standalone.rs`
  (both tests) and the two write tests in `preprod_smoke_live.rs`.
  Everywhere else the `cfg(feature = "network-tests")` gate is the
  off-switch.
- No integration test today drives a wallet built from
  `Wallet::with_deps(indexer, node, prover)` against a fully stubbed
  port set. The stub factory exists; nothing consumes it from
  `tests/`.

### 1.4 Coupling smells worth flagging

1. **`BridgeState` is a god object that's also props-shaped.**
   `dioxus-wallet/src/bridge.rs:50-85` declares one struct that bundles
   persistence (`store`), in-memory cache (`controller_secrets`),
   UI-binding state (`active_wallet_id`), telemetry (`metrics`,
   `resource_probe`), and log sink (`log_capture`). Because Dioxus
   `#[component]` requires `Eq` props, the struct also carries a
   custom `Arc::ptr_eq`-based `PartialEq` implementation (lines 87-98)
   — the bundle has to be cloned through the prop tree atomically, so
   any future per-use-case dependency surface has to either subset it
   or wrap it.

2. **`app_wallet_for(net)` is implicit-state wallet construction
   reaching into four process-wide statics.** `app.rs:721-796` reads
   `PROOF_SERVER_URL: OnceLock<String>` (line 853), the `DUST_SYNCERS`
   `OnceLock<Mutex<HashMap<…>>>` (lines 801-817) via
   `dust_syncer_for(net)`, and
   `crate::eval_bridge::global_bridge()` (line 782). Every
   `app_wallet_for` call rebuilds a fresh `Wallet` value and re-applies
   the four optional layers. The function is the wallet-construction
   choke point, and every flow's wallet is implicitly stateful.

3. **The "guard + locator" prelude is duplicated four times in
   identity_centre.rs.** `identity_centre.rs:228-256` (Bootstrap),
   `456-477` (Oid4vp), `611-625` (Oid4vci), `860-889` (VcInventory)
   each pull `bridge_state.store().cloned()`,
   `bridge_state.active_wallet_id()`, `bridge_state.metrics_dyn()`,
   `bridge_state.metrics()`, `bridge_state.resource_probe()`, build a
   `metered_app_wallet_for(network, metrics, probe)`, build a
   `RedbSecretStore::new(store, wallet_id)`, then proceed. No shared
   "use-case dependencies" type.

4. **`bootstrap_did_with_keys` returns a value the use-case doesn't own
   the persistence boundary of.** The function in `wallet-core` returns
   a `BootstrappedDid` with `controller_sk` + `did`; the UI is then
   responsible for `bridge_state.remember_controller_secret(network,
   did, sk)` and for calling `on_did_minted` to notify the App's
   `did_inventory` signal + redb (`identity_centre.rs:275-291`,
   `app.rs:1912-1937`). The same DID is persisted via two different
   channels (controller secrets table + DID inventory table) at two
   different sites, and the wallet-core flow is unaware of either.

5. **Wallet ID + active-network state lives in `BridgeState`, but
   network selection lives in App-level `Signal<Network>`.** The
   App's `network` signal (`app.rs:1734` swap path) drives
   `app_wallet_for(n)` and `bridge_state.set_active_wallet_id(...)`,
   but `BridgeState::active_wallet_id` is *also* mutated independently
   from elsewhere. The "current wallet" is the join of (a) the
   App-level network signal, (b) `BridgeState::active_wallet_id`,
   (c) the `app_wallet_for` reading process-wide statics, and (d) the
   per-DID controller-secrets map. None of these is the single source
   of truth.

6. **`spawn(async move { … })` blocks own the use-case bodies.**
   Every flow in `identity_centre.rs` and every callback in `app.rs`'s
   tab handlers (Dids, Wallet, Identity, Diagnostics) wraps its
   work in `spawn(async move { … })` plus a chain of UI-signal
   writes. There's no "Outcome" type; success/failure is encoded
   as side effects on multiple signals (`busy`, `err_msg`, `ok_msg`,
   `activity`, plus parent callbacks). Re-running the same flow
   without the UI is not possible today.

7. **`use_effect` blocks do orchestration too.** `BootstrapSection`'s
   first `use_effect` (lines 186-213) does a blocking
   `futures::executor::block_on(s.list_keys(None))` to infer the IC DID
   from a `#key-auth` kid — that's a discovery use-case running on
   the UI thread on mount. Its second `use_effect` (lines 318-345)
   polls `log_capture().snapshot()` for an activity feed. Both of
   these would have to be untangled before any of the bootstrap
   logic can run headlessly.

8. **The Operation Builder's submit loop carries
   `counter_cursor` + `loaded_set` state in closure locals.**
   `app.rs:3724-…`'s `spawn(async move { … })` body owns the
   monotonic counter cursor across `load_did_circuit` writes and
   the set of already-loaded circuits for the run. Both are
   recoverable from on-chain state, but in the current flow that
   state lives only inside the spawn — a re-render or cancellation
   loses it.

9. **Persistence helpers are free functions split between bridge +
   app.** `BridgeState::remember_controller_secret` writes through
   to `WalletStore`, but `persist_inventory_entry` (app.rs) and
   `persist_resolved_cache` (app.rs) are top-level fns the UI calls
   directly — they reach into the bridge to pull the store, then
   write a different table. The "persist a DID I learned about"
   path is split across `BridgeState` (for controller secret),
   `persist_inventory_entry` (for inventory row), and
   `persist_resolved_cache` (for the resolved document). Three
   sites, no single use-case.

10. **`eval_bridge::global_bridge()` is a process-wide
    `OnceCell<Arc<dyn JsBridge>>` that flows in implicitly.**
    `app_wallet_for` calls it without a parameter to decide whether
    to attach a `DioxusEvalBridge` or fall back to spawning a
    `NodeChildBridge`. Tests and headless callers have no way to
    inject a stub bridge without going through `eval_bridge::install_global`
    or through `Wallet::with_js_bridge` explicitly, and the App's
    construction path uses the former.

11. **Wallet-construction surface drift.** `Wallet` exposes
    `with_indexer / with_node / with_prover / with_metering /
    with_proof_server_url / with_dust_syncer / with_js_bridge /
    with_stub_did_state`, plus the convenience constructors
    `demo / from_seed / new_random / from_seed_hex / with_deps`.
    The dioxus-wallet currently uses `Wallet::demo` /
    `Wallet::from_seed` + the four `with_*` layered statics; tests
    use `Wallet::with_deps`; `stub_wallet` uses
    `with_stub_did_state`. Three call patterns produce three
    different "configured wallet" shapes.

12. **No port boundary for chain probing.**
    `probe::probe_connectivity` (used by `app.rs:1517`) is a free
    function that builds its own reqwest + subxt clients; the
    `connect` closure also constructs `HttpIndexerClient::new(net)`
    and `SubxtNodeClient::connect(net)` directly to populate the
    chain snapshot. Both bypass the wallet's
    `IndexerClient` / `NodeClient` ports.

---

## §2 Target Architecture

This section describes where we want the Midnight wallet to land after the
hexagonal refactor finishes. It is not a clean rewrite. The pieces already in
place in `wallet-core` — `HttpClient`, `IndexerClient` / `NodeClient` /
`Prover`, `SecretStorage`, `VcStorage`, `Clock`, `Metrics`, `ResourceProbe`,
and the clean use-case functions in `did::bootstrap`, `oid4vp_client`,
`oid4vci_client`, `vc_self_verify` — are the model to extend. The destination
is "every flow looks like those", not "every flow gets rewritten".

### 2.1 Architectural goals

1. **Headless parity.** A `headless-wallet` binary drives every flow the
   Dioxus app drives. If a behaviour exists in the UI but not in the CLI, that
   is a bug — the logic has leaked out of the service layer.
2. **Pure domain core.** `wallet-core` types and use-case services depend on
   zero concrete I/O, storage, time, or randomness. Every external interaction
   goes through a port trait.
3. **At least two adapters per port.** A real-deps adapter (reqwest, subxt,
   redb, OsRng, SystemClock, …) and a test adapter (mock, in-memory, fixed,
   deterministic). Where it earns its keep, a third in-process adapter (e.g.
   `InMemoryVcStore` for headless dev runs without a redb file).
4. **Adapter-independent testability.** Each adapter is independently testable
   against its trait contract; the use-case services are tested against the
   mock adapters; the headless binary is the integration driver.
5. **UI as adapter.** Dioxus is one possible adapter of a `UserInterface`
   port. A CLI is another. A test harness collecting events into a `Vec` is a
   third. The use-cases do not know which one they are talking to.
6. **`Arc<dyn Trait>` everywhere.** No generic gymnastics, no GAT acrobatics.
   Object-safe traits, `Arc<dyn Trait>` fields on services, `async_trait` for
   async methods. Matches what `Wallet::with_deps` and the rest of the crate
   already do.
7. **Plain tokio.** No actor framework, no `actix`, no message-passing
   runtime. `async fn` + `tokio::spawn` + channels where genuinely needed.

### 2.2 Domain core

The core is `wallet-core/src/` minus the adapters. Three layers inside it:
value objects, entities, and use-case services.

#### Value objects and entities

Already in the crate; the refactor consolidates them and removes stringly-typed
leakage at the service boundary.

```rust
// Identity & key material
pub struct WalletSeed([u8; 32]);              // root of trust
pub struct WalletIdentity { id: WalletId, label: String, network: Network }
pub struct DidId(String);                     // already exists
pub struct DidDocument { /* resolver output */ }
pub struct VerificationMethod { id: String, typ: VmType, jwk: PublicJwk }
pub struct VerifiableCredential { uri: String, body: Vec<u8>, /* … */ }
pub struct VcOpening { vc_uri: String, claim_path: String, blob: Vec<u8> }
pub struct ControllerSecret([u8; 32]);        // per-DID, per-network
pub struct SchnorrJubjubKey { /* assertion-method key */ }
pub struct Ed25519Key { /* authentication key */ }

// Bootstrapped Identity-Centre DID — the unit the IC card displays.
pub struct IdentityCentreDid {
    pub did: DidId,
    pub ed25519_ref: SecretKeyRef,
    pub jubjub_ref: SecretKeyRef,
    pub controller_sk: ControllerSecret,
    pub created_ms: u64,
}
```

Two rules for value objects:

- **No `String` at the public service boundary** for things that have a type.
  A method that takes a DID takes `&DidId`, not `&str`. Conversion happens at
  the adapter edges (CLI parses a string into `DidId`; the Dioxus UI does the
  same; the use-case never sees the unparsed form).
- **No `serde_json::Value` at the public service boundary** either. JSON
  shapes are an HTTP-adapter concern; use-cases return typed Rust.

#### Use-case services

Each service is a struct holding `Arc<dyn Port>` fields. Construction is
explicit. Methods are `async`. Errors are typed per service.

```rust
pub struct WalletService {
    storage:   Arc<dyn WalletStorage>,
    secrets:   Arc<dyn SecretStorage>,
    indexer:   Arc<dyn IndexerClient>,
    node:      Arc<dyn NodeClient>,
    prover:    Arc<dyn Prover>,
    clock:     Arc<dyn Clock>,
    rng:       Arc<dyn Randomness>,
    metrics:   Arc<dyn Metrics>,
    ui:        Arc<dyn UserInterface>,
    unlock:    Arc<dyn UnlockGate>,
}

impl WalletService {
    pub async fn unlock(&self, label: &str, passphrase: &str)
        -> Result<WalletIdentity, WalletError>;
    pub async fn lock(&self) -> Result<(), WalletError>;
    pub async fn snapshot_utxo(&self, id: &WalletId)
        -> Result<UtxoSet, WalletError>;
    pub async fn balance(&self, id: &WalletId)
        -> Result<BalanceSnapshot, WalletError>;
    pub fn networks(&self) -> Vec<Network>;
}

pub struct DustSyncService { /* indexer, prover, storage, clock, ui, metrics */ }
impl DustSyncService {
    pub async fn sync(&self, id: &WalletId) -> Result<DustReport, DustError>;
    pub async fn cached_balance(&self, id: &WalletId)
        -> Result<DustBalance, DustError>;
}

pub struct DidService { /* wallet, indexer, node, prover, storage, ui */ }
impl DidService {
    pub async fn create_did(&self, id: &WalletId)
        -> Result<(DidId, ControllerSecret), DidError>;
    pub async fn deactivate_did(&self, id: &WalletId, did: &DidId)
        -> Result<(), DidError>;
    pub async fn resolve_did(&self, did: &DidId)
        -> Result<DidDocument, DidError>;
    pub async fn list_dids(&self, id: &WalletId)
        -> Result<Vec<DidId>, DidError>;
    pub async fn update_did(&self, did: &DidId, op: DidUpdateOp)
        -> Result<(), DidError>;
}

pub struct IdentityCentreService { /* did, secrets, storage, ui */ }
impl IdentityCentreService {
    pub async fn bootstrap(&self, id: &WalletId, seed: &WalletSeed)
        -> Result<IdentityCentreDid, BootstrapError>;
    pub async fn get_or_create(&self, id: &WalletId, seed: &WalletSeed)
        -> Result<IdentityCentreDid, BootstrapError>;
}

pub struct Oid4vpService { /* http, clock, wallet, secrets, ui */ }
impl Oid4vpService {
    pub async fn run_authentication(&self, qr_url: &str, did: &DidId)
        -> Result<AuthSessionResult, Oid4vpError>;
}

pub struct Oid4vciService { /* http, clock, wallet, secrets, vc_store, ui */ }
impl Oid4vciService {
    pub async fn run_issuance(&self, offer_url: &str, holder: &DidId)
        -> Result<VcUri, Oid4vciError>;
}

pub struct VcVerifyService { /* wallet, secrets, vc_store, clock, ui */ }
impl VcVerifyService {
    pub async fn self_verify(&self, vc_uri: &VcUri)
        -> Result<SelfVerifyResult, VerifyError>;
    pub async fn list(&self) -> Result<Vec<VcSummary>, VerifyError>;
    pub async fn mark_revoked(&self, vc_uri: &VcUri) -> Result<(), VerifyError>;
}

pub struct BackupService { /* storage, secrets, vc_store, ui */ }
impl BackupService {
    pub async fn export(&self, passphrase: &str) -> Result<BackupBlob, BackupError>;
    pub async fn import(&self, blob: BackupBlob, passphrase: &str)
        -> Result<RestoreReport, BackupError>;
}

pub struct ControllerSecretService { /* storage */ }
impl ControllerSecretService {
    pub async fn store(&self, net: Network, did: &DidId, sk: ControllerSecret)
        -> Result<(), StoreError>;
    pub async fn fetch(&self, net: Network, did: &DidId)
        -> Result<Option<ControllerSecret>, StoreError>;
    pub async fn list(&self, net: Network)
        -> Result<Vec<(DidId, ControllerSecret)>, StoreError>;
}

pub struct TelemetryService { /* metrics, probe */ }
impl TelemetryService {
    pub fn snapshot(&self) -> MetricsSnapshot;
    pub fn reset(&self);
}
```

Three things to note:

- Services are **stateless** beyond their port references. State lives inside
  the ports (the redb store, the in-memory metrics aggregator, the secret
  store). A service can be cloned via `Arc` and used from any task.
- Services compose. `IdentityCentreService::bootstrap` calls `DidService`
  internally rather than reaching into `Wallet::create_did_awaitable` itself.
  This is the only way the headless binary stays in sync — every verb is a
  service method.
- The big remaining wart is **`Wallet`** itself. It is still the carrier of
  the chain-op + DID-call methods and that does not change in this refactor.
  The plan is: services *use* `Wallet` via a `ChainPublisher` port (see §2.3)
  rather than holding the concrete type, so the integration tests can swap in
  a stub. Pulling `Wallet`'s remaining methods into smaller services is a
  follow-up.

### 2.3 Port catalog

Existing ports, the new ones the refactor introduces, and the adapters each
needs. Every trait is `Send + Sync + 'static`, dyn-compatible, with
`async_trait` where async is required.

#### `HttpClient` (existing, `src/http.rs`)

GET / POST-JSON port for OID4VP / OID4VCI / arbitrary external endpoints.

- Real: `ReqwestHttpClient`.
- Mock: `MockHttpClient` — scripted responses + recorded requests, behind
  `#[cfg(feature = "test-support")]`.

#### `IndexerClient` (existing, `src/chain.rs`)

`chain_tip`, `contract_state` from the GraphQL indexer.

```rust
#[async_trait]
pub trait IndexerClient: Send + Sync + 'static {
    async fn chain_tip(&self) -> Result<Option<ChainTipInfo>, IndexerError>;
    async fn contract_state(&self, address_hex: &str)
        -> Result<Option<ContractStateInfo>, IndexerError>;
}
```

- Real: `HttpIndexerClient`.
- Mock: `StubIndexer` — scripted contract-state responses, behind
  `test-support`.
- In-process: a `RecordedIndexer` that replays a JSON capture file from a
  previous live run — useful for fast offline reruns of an integration suite.

#### `NodeClient` (existing)

`submit_deploy(bytes, signer) -> SubmitResult`.

- Real: `SubxtNodeClient`.
- Mock: `StubNode` — returns a deterministic `SubmitResult`, records inputs.

#### `Prover` (existing)

`prove(UnprovenTx) -> ProvenTx`.

- Real: `LocalProver`, `HttpProver` (per `chain.rs`).
- Mock: `StubProver` — returns a hand-rolled `ProvenTx` with a pinned proof
  payload; used by the standalone-only integration tests that do not exercise
  the prover surface itself.

#### `SecretStorage` (existing, `src/secret_storage/`)

Multi-curve secret storage. The trait already covers generate/import/derive,
sign, verify, list, delete (`SecretStorage` in `secret_storage/types.rs`).

- Real: `FileSecretStore`, `RedbSecretStore`.
- Mock: `InMemorySecretStore`, behind `test-support`.

#### `VcStorage` (existing, `src/vc_store/`)

Per-holder VC persistence (`insert_vc`, `get_vc`, `insert_opening`, …,
`update_metadata`).

- Real: `RedbVcStore`.
- Mock: `InMemoryVcStore`, behind `test-support`.

#### `Clock` (existing)

`now_ms() -> u64`.

- Real: `SystemClock`.
- Test: `FixedClock` — pinned, with `set` and `advance`.

#### `Metrics` (existing)

`record_http`, `record_op`, `incr`, plus the in-memory snapshot side.

- Real: `InMemoryMetrics`, `TracingMetrics`, `CompositeMetrics`.
- Null-object: `NoopMetrics`.

#### `ResourceProbe` (existing)

RSS / CPU sampling.

- Real: `RusageProbe`.
- Null: `NoopResourceProbe`.

#### `WalletStorage` (new)

Wraps the `redb`-backed `wallet.redb` file. The redb-specific code currently
lives in `wallet-core/src/store/` and is called directly from a handful of
places; the refactor pulls the call shape behind a trait so an in-memory
backend can drive the headless tests without disk I/O.

```rust
#[async_trait]
pub trait WalletStorage: Send + Sync + 'static {
    async fn list_wallets(&self) -> Result<Vec<WalletIdentity>, StoreError>;
    async fn put_wallet(&self, w: &WalletRow) -> Result<(), StoreError>;
    async fn get_wallet(&self, id: &WalletId)
        -> Result<Option<WalletRow>, StoreError>;
    async fn put_controller_secret(
        &self, net: Network, did: &DidId, sk: &ControllerSecret,
    ) -> Result<(), StoreError>;
    async fn get_controller_secret(
        &self, net: Network, did: &DidId,
    ) -> Result<Option<ControllerSecret>, StoreError>;
    async fn list_controller_secrets(&self, net: Network)
        -> Result<Vec<(DidId, ControllerSecret)>, StoreError>;
    async fn put_did_inventory_entry(&self, e: &DidInventoryRow)
        -> Result<(), StoreError>;
    async fn list_did_inventory(&self, net: Network)
        -> Result<Vec<DidInventoryRow>, StoreError>;
    // backup/restore
    async fn export_all(&self) -> Result<StoreSnapshot, StoreError>;
    async fn import_all(&self, snap: StoreSnapshot) -> Result<(), StoreError>;
}
```

- Real: `RedbWalletStorage` — the existing `wallet-core/src/store/` code,
  reshaped to implement the trait.
- In-process: `InMemoryWalletStorage` — `HashMap`-backed, behind
  `test-support`. The default for the headless binary's `--in-memory` mode.
- Test mock: same `InMemoryWalletStorage` — there is no separate "scripted
  response" mock here because the trait is a key/value store, not a
  call-and-response.

#### `UnlockGate` (new)

Encapsulates passphrase-based unlock policy: how many attempts, what backoff,
what shape of error gets surfaced. Today, the unlock check is inline in the
UI; the refactor moves it behind a port so the headless binary uses the same
policy with a passphrase passed via env or stdin.

```rust
pub trait UnlockGate: Send + Sync + 'static {
    fn verify(&self, passphrase: &str, wrapped_seed: &[u8])
        -> Result<WalletSeed, UnlockError>;
    fn record_attempt(&self, ok: bool);
    fn is_locked_out(&self) -> bool;
}
```

- Real: `ScryptUnlockGate` — uses the same scrypt+AES envelope `FileSecretStore`
  uses today.
- Test: `AlwaysOkUnlockGate` and `NeverOkUnlockGate` for fast unit coverage of
  failure paths.

#### `ChainPublisher` (new)

The chain-write part of `Wallet` (deploy a contract, call a circuit, submit a
maintenance update) pulled out as a port so services do not hold a concrete
`Wallet` value. The trait is intentionally narrow — only the verbs the
services actually call.

```rust
#[async_trait]
pub trait ChainPublisher: Send + Sync + 'static {
    async fn create_did_with_controller(&self)
        -> Result<(DidId, ControllerSecret), ChainError>;
    async fn deactivate_did(&self, did: &DidId, sk: &ControllerSecret)
        -> Result<(), ChainError>;
    async fn call_did_circuit(
        &self, did: &DidId, circuit: &str, arg: serde_json::Value,
        sk: &ControllerSecret,
    ) -> Result<CallReceipt, ChainError>;
    async fn load_did_circuit(
        &self, did: &DidId, circuit: &str, counter: u64,
    ) -> Result<(), ChainError>;
}
```

- Real: `WalletChainPublisher` — wraps the existing `Wallet` methods.
- Stub: `StubChainPublisher` — deterministic answers, behind `test-support`.

The `serde_json::Value` argument is a deliberate, narrow leak — the underlying
Compact-codegen circuit signatures are typed maps and the wire format is JSON.
A future pass can introduce typed argument enums per circuit, but that is not
the hex refactor's job.

#### `UserInterface` / `UiPort` (new) — see §2.5

#### `Randomness` (new)

Wraps `OsRng`.

```rust
pub trait Randomness: Send + Sync + 'static {
    fn fill_bytes(&self, buf: &mut [u8]);
    fn next_u64(&self) -> u64;
}
```

- Real: `OsRandomness`.
- Test: `DeterministicRng` — ChaCha-seeded, returns reproducible bytes.

The existing `Wallet::from_chacha_seed` already supports deterministic mode at
the wallet level; this port pushes the same idea into every service that
needs random material (controller secrets, OID4VCI nonces, oid4vp `nonce`
echoing where applicable).

#### `Notifications` (new)

Push channel for transient banners ("DID created", "VC issued",
"Verification failed"). Distinct from `UserInterface` because it is fire-and-
forget — no prompt, no input. The same use-cases that prompt via `UiPort`
also notify via `Notifications`.

```rust
pub trait Notifications: Send + Sync + 'static {
    fn notify(&self, level: NotifyLevel, msg: &str);
}
```

- Real (Dioxus): `DioxusNotifier` — pushes a `Signal<VecDeque<Toast>>`.
- Real (CLI): `StderrNotifier` — prints `[INFO] msg` to stderr.
- Test: `CollectingNotifier` — collects into a `Vec<NotifyRecord>` for asserts.

### 2.4 The headless wallet binary

`mobile-bench/headless-wallet/` is a new crate that exposes every service from
§2.2 over a line-based JSON protocol. It is the integration-test driver,
the operations-debugging tool, and the proof that the hex pieces are honest.

#### Configuration

```
cargo run -p headless-wallet -- \
    --network standalone \
    --store-path ~/.midnight/headless.redb \
    --passphrase-env HEADLESS_PASSPHRASE \
    --proof-server http://127.0.0.1:57610 \
    --indexer http://127.0.0.1:8088/api/v1/graphql \
    --node ws://127.0.0.1:9944
```

Flags:

- `--network {standalone|preprod|mainnet}`
- `--store-path PATH` or `--in-memory-store` (mutually exclusive)
- `--passphrase-env VAR`, `--passphrase-stdin`, or `--passphrase VALUE`
- `--proof-server URL` (omit → use `LocalProver`)
- `--indexer URL`, `--node WS` (defaults per network)
- `--mock-http` (replaces `HttpClient` with a `MockHttpClient` driven from
  stdin commands — used by the oid4vp / oid4vci integration tests)
- `--mock-chain` (replaces indexer/node/prover with stubs — for unit-style
  end-to-end runs that exercise the service-composition path without a live
  chain)
- `--metrics-out PATH` (dumps `MetricsSnapshot` JSON on exit)

#### Protocol: line-delimited JSON

Every line on stdin is one command; every line on stdout is one response or
event. JSON-RPC was considered and rejected: the framing overhead (id, method,
params nesting) buys nothing here, and the integration test driver wants to
stream events back without correlating ids. Plain line-delimited JSON gives
both — request and response carry a `verb`, and progress events carry a `kind`.

Outbound lines fall in three buckets:

- `{ "type": "result", "verb": "...", "ok": true, "data": { ... } }`
- `{ "type": "error",  "verb": "...", "code": "...", "message": "..." }`
- `{ "type": "event",  "verb": "...", "stage": "...", "data": { ... } }`

Stages emitted by long-running flows (`oid4vp`, `oid4vci`, `bootstrap_did`,
`sync_dust`) give the test driver a stable sequence to assert on without
parsing logs.

#### Verbs

One verb per service method, named to mirror the Rust method.

| Verb                       | Service                                       |
|----------------------------|-----------------------------------------------|
| `unlock`                   | `WalletService::unlock`                       |
| `lock`                     | `WalletService::lock`                         |
| `list-networks`            | `WalletService::networks`                     |
| `balance`                  | `WalletService::balance`                      |
| `sync-night`               | `WalletService::snapshot_utxo`                |
| `sync-dust`                | `DustSyncService::sync`                       |
| `dust-balance`             | `DustSyncService::cached_balance`             |
| `create-did`               | `DidService::create_did`                      |
| `deactivate-did`           | `DidService::deactivate_did`                  |
| `resolve-did`              | `DidService::resolve_did`                     |
| `list-dids`                | `DidService::list_dids`                       |
| `update-did`               | `DidService::update_did`                      |
| `bootstrap-did`            | `IdentityCentreService::bootstrap`            |
| `identity-centre`          | `IdentityCentreService::get_or_create`        |
| `oid4vp-authenticate`      | `Oid4vpService::run_authentication`           |
| `oid4vci-issue`            | `Oid4vciService::run_issuance`                |
| `verify-vc`                | `VcVerifyService::self_verify`                |
| `list-vcs`                 | `VcVerifyService::list`                       |
| `export-wallet`            | `BackupService::export`                       |
| `import-wallet`            | `BackupService::import`                       |
| `metrics`                  | `TelemetryService::snapshot`                  |

Exit codes:

- `0` — clean shutdown after stdin EOF.
- `2` — config / CLI error.
- `3` — unlock failure exceeding the gate's lockout policy.
- `4` — fatal storage or chain error during startup.

Per-verb errors travel inline as `{ "type": "error", ... }` and the process
keeps running; the test driver can recover or escalate. Only configuration
and unrecoverable startup conditions exit non-zero.

#### Sample session

```
$ cargo run -p headless-wallet -- \
    --network standalone --in-memory-store \
    --passphrase test --proof-server http://127.0.0.1:57610

> {"verb":"unlock","args":{"label":"alice","passphrase":"test"}}
< {"type":"result","verb":"unlock","ok":true,
   "data":{"wallet_id":"w_4f3a","network":"standalone","address":"mn_shield…"}}

> {"verb":"sync-night","args":{"wallet_id":"w_4f3a"}}
< {"type":"event","verb":"sync-night","stage":"indexer.chain_tip","data":{"height":4211}}
< {"type":"event","verb":"sync-night","stage":"utxo.scan","data":{"scanned":12}}
< {"type":"result","verb":"sync-night","ok":true,
   "data":{"utxos":3,"balance_night":"1000000000"}}

> {"verb":"bootstrap-did","args":{"wallet_id":"w_4f3a"}}
< {"type":"event","verb":"bootstrap-did","stage":"create_did","data":{}}
< {"type":"event","verb":"bootstrap-did","stage":"create_did.done",
   "data":{"did":"did:midnight:standalone:abc…"}}
< {"type":"event","verb":"bootstrap-did","stage":"indexer.settle"}
< {"type":"event","verb":"bootstrap-did","stage":"vk.load","data":{"circuit":"setVerificationMethod"}}
< {"type":"event","verb":"bootstrap-did","stage":"vk.load","data":{"circuit":"setSchnorrJubjubVerificationMethod"}}
< {"type":"event","verb":"bootstrap-did","stage":"vm.attach","data":{"kind":"ed25519/authentication"}}
< {"type":"event","verb":"bootstrap-did","stage":"vm.attach","data":{"kind":"jubjub/assertionMethod"}}
< {"type":"result","verb":"bootstrap-did","ok":true,
   "data":{"did":"did:midnight:standalone:abc…","ed25519_kid":"…#key-auth",
           "jubjub_kid":"…#key-assert"}}

> {"verb":"oid4vp-authenticate","args":{
     "wallet_id":"w_4f3a",
     "qr_url":"openid4vp://?request_uri=http://issuer.local/request/x",
     "did":"did:midnight:standalone:abc…"}}
< {"type":"event","verb":"oid4vp-authenticate","stage":"qr.parse"}
< {"type":"event","verb":"oid4vp-authenticate","stage":"request.fetch"}
< {"type":"event","verb":"oid4vp-authenticate","stage":"id_token.sign"}
< {"type":"event","verb":"oid4vp-authenticate","stage":"response.post"}
< {"type":"result","verb":"oid4vp-authenticate","ok":true,
   "data":{"session_id":"S-42","status":"authenticated"}}

> {"verb":"oid4vci-issue","args":{
     "wallet_id":"w_4f3a",
     "offer_url":"openid-credential-offer://?credential_offer_uri=…",
     "did":"did:midnight:standalone:abc…"}}
< {"type":"event","verb":"oid4vci-issue","stage":"offer.parse"}
< {"type":"event","verb":"oid4vci-issue","stage":"token.request"}
< {"type":"event","verb":"oid4vci-issue","stage":"credential.request"}
< {"type":"event","verb":"oid4vci-issue","stage":"vc.store"}
< {"type":"result","verb":"oid4vci-issue","ok":true,
   "data":{"vc_uri":"urn:uuid:abc-123"}}

> {"verb":"verify-vc","args":{"vc_uri":"urn:uuid:abc-123"}}
< {"type":"result","verb":"verify-vc","ok":true,
   "data":{"outcome":"Valid","vm_id":"did:midnight:…#key-assert",
           "last_verified_ms":1735000000000}}

> {"verb":"metrics"}
< {"type":"result","verb":"metrics","ok":true,
   "data":{"http":{"calls":11,"p50_ms":18,"p95_ms":210}, …}}
```

### 2.5 The UI port

The `UserInterface` trait is what use-cases call when they need to communicate
with whoever is driving them: status updates, user prompts, error display.
The trait is async because prompts may block on user action.

```rust
#[async_trait]
pub trait UserInterface: Send + Sync + 'static {
    /// Emit a stage update with arbitrary structured context. Used
    /// by long-running flows to give the driver something to assert
    /// on (or display).
    fn report_stage(&self, verb: &str, stage: &str, data: serde_json::Value);

    /// Surface a non-fatal user-visible event — analogous to a
    /// toast. `Notifications` covers fire-and-forget; this is for
    /// events the use-case wants to know the driver saw.
    fn report_outcome(&self, verb: &str, outcome: UiOutcome);

    /// Prompt for free-form text (paste a QR URL, paste an offer
    /// URL, etc). In headless mode this is satisfied by an
    /// existing CLI arg; in the UI it pops a modal; in tests the
    /// scripted answer is dequeued.
    async fn prompt_text(&self, prompt: &str) -> Result<String, UiError>;

    /// Prompt for a passphrase. Same shape; passphrase-typed for
    /// clarity (and so adapters can mask the input).
    async fn prompt_passphrase(&self, prompt: &str) -> Result<String, UiError>;

    /// Yes/no confirm.
    async fn confirm(&self, prompt: &str) -> Result<bool, UiError>;
}

#[derive(Debug, Clone)]
pub enum UiOutcome { Ok(String), Warn(String), Err(String) }

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("user cancelled")]
    Cancelled,
    #[error("input closed")]
    Closed,
}
```

Three adapters:

- **`DioxusUiAdapter`** — owns the relevant `Signal<…>` handles. `report_stage`
  pushes onto a per-verb progress signal; `prompt_text` mounts a modal and
  awaits an `oneshot::Receiver<String>` fulfilled by the modal's submit
  button; `confirm` is the same with a `bool` payload. The Dioxus components
  themselves never look at the service layer — they observe the signals the
  adapter populates and emit events back through it.

- **`CliUiAdapter`** — `report_stage` prints
  `{"type":"event","verb":…,"stage":…,"data":…}` to stdout (this is the headless
  binary's wire format). `prompt_text` either reads from stdin (if
  `--interactive`) or returns a pre-supplied CLI arg via a lookup the headless
  binary sets up. Same for `prompt_passphrase` (`--passphrase` flag or stdin).

- **`TestUiAdapter`** — collects every `report_stage` / `report_outcome` into
  a `Vec<UiEvent>` the test asserts on. Prompts dequeue from a pre-loaded
  `VecDeque<String>`; an empty queue is an error. Built-in helpers:
  `expect_stage("bootstrap-did", "create_did.done")`, `prompts_drained()`,
  `events()`.

The use-case never branches on adapter type. Services accept
`Arc<dyn UserInterface>`; the constructor wires the right one.

### 2.6 Test strategy

Three layers, with clear responsibility separation. Adapter tests pin the
contract; use-case tests prove the composition; integration tests prove the
end-to-end against the live (or mocked-live) world.

#### Layer 1 — Adapter tests

Each adapter, real and mock, gets its own test file under the adapter's
module. Goals:

- **Real adapters** — smoke-tested against their backing system. `ReqwestHttpClient`
  hits a tiny `httpmock` in-process server. `RedbWalletStorage` hits a tempdir
  redb. `HttpIndexerClient` and `SubxtNodeClient` are exercised only behind
  `#[cfg(feature = "standalone-tests")]` against a running docker stack.
- **Mock adapters** — the contract is exercised transitively by every
  use-case test (they all depend on the mocks). A small set of standalone
  unit tests pins surprising behaviour: e.g. `MockHttpClient` returns the
  scripted responses in FIFO order; `InMemoryVcStore::list_ordered` honours
  insertion order; `FixedClock::advance` is monotonic under concurrent
  reads.

File layout:

```
wallet-core/src/
  http.rs                # contains mod tests for ReqwestHttpClient + MockHttpClient
  chain.rs               # mod tests for LocalProver, HttpProver, stubs
  secret_storage/
    file_secret_store.rs # mod tests for the file backend
    in_memory.rs         # mod tests for the in-memory backend
  vc_store/
    api.rs               # mod tests for RedbVcStore
    in_memory.rs         # mod tests for InMemoryVcStore
  clock.rs               # mod tests for SystemClock + FixedClock (already present)
  store/
    api.rs               # mod tests for RedbWalletStorage
  …
```

Adapter tests run on `cargo test`. The `standalone-tests` and `preprod-tests`
features gate the ones that need a real chain.

#### Layer 2 — Use-case tests

Every public method on every service gets at least one test:

- One happy path through the mocks.
- One failure path per service-defined error variant where it can be exercised
  from input shape alone.
- One test for any non-trivial sequencing assertion the service is responsible
  for (e.g. `IdentityCentreService::bootstrap` calls `create_did` before
  `set_verification_method`).

These tests construct the service with mock adapters by hand. They are pure
unit tests — no tokio runtime tricks beyond `#[tokio::test]`, no network, no
disk, no time, no entropy from the host. Each test should be under one second
on a warm cache.

File layout:

```
wallet-core/tests/usecase/
  wallet_service.rs
  dust_sync_service.rs
  did_service.rs
  identity_centre_service.rs
  oid4vp_service.rs
  oid4vci_service.rs
  vc_verify_service.rs
  backup_service.rs
  controller_secret_service.rs
```

Existing tests inside `oid4vp_client/mod.rs::flow_tests` and
`oid4vci_client/mod.rs::flow_tests` are the template — same MockHttpClient +
FixedClock + InMemoryVcStore + stub-wallet pattern, just hoisted up to the
service struct rather than the bare free function. They move from inline
`#[cfg(test)] mod` into the `tests/usecase/` tree as the service wrappers
land.

#### Layer 3 — Integration tests

Drive the headless binary against a live standalone (or mocked-live)
environment. Each test is a black-box driver: it spawns
`headless-wallet`, pipes JSON commands, asserts on the JSON responses and
event stream.

File layout:

```
mobile-bench/headless-wallet/tests/
  unlock_and_sync_night.rs
  unlock_and_sync_dust.rs
  did_create_and_resolve.rs
  did_bootstrap_inventory.rs
  did_update_via_op_builder.rs
  did_deactivate.rs
  oid4vp_against_mock_issuer.rs
  oid4vci_against_mock_issuer.rs
  vc_self_verify_live_and_cached.rs
  backup_restore_roundtrip.rs
```

Behind `#[cfg(feature = "standalone-tests")]`. They are slow (most need a
warmed-up indexer, a node, and a proof server) so `cargo test` does not run
them by default; CI runs them on a dedicated job with the docker stack
pre-launched.

#### Must-have flows for completeness

The hex refactor is considered done when these integration tests pass against
a fresh standalone docker env:

1. **Unlock + sync NIGHT + sync DUST** — fresh standalone, default config.
2. **`create_did` → `resolve_did`** — round-trip, controller secret persisted.
3. **`bootstrap_did_with_keys`** — both VMs attached, inventory entry written,
   controller secret round-trippable via `ControllerSecretService`.
4. **`oid4vp_run_authentication`** — against the issuer-mock harness.
5. **`oid4vci_run_issuance`** — VC + opening land in `VcStorage`.
6. **`self_verify_and_cache`** — both the live (first call) and cached
   (subsequent) paths, with `last_verified_ms` written.
7. **Backup + restore round-trip** — export, wipe, import, all wallets +
   controller secrets + VCs survive.
8. **`deactivate_did`** — DID is unresolvable after; on-chain state matches.
9. **`update_did`** — via the Operation Builder verbs (`setSchnorrJubjubVerificationMethod`,
   etc), proves the redesigned DID circuit set works end-to-end.

### 2.7 Dependency injection

Three options considered.

- **Manual builder** — `WalletServices::new(http, store, clock, …).build()`
  returns a struct with one `Arc<dyn Service>` per service. Verbose but
  fully explicit; every wire crosses through one file. Hard-coded; no runtime
  swapping; trivial to read.

- **External crate (`shaku`, `dependency-injection`)** — reduces the wiring
  boilerplate but adds a runtime dep, a new mental model, and macro-laden
  error messages. The codebase has no existing DI framework and the trait
  shapes are stable; the saving is small.

- **In-house typed `Container`** — a `HashMap<TypeId, Arc<dyn Any + Send + Sync>>`
  with typed `get<T>()` and `register<T>()` methods. Hides the wire graph,
  pushes the construction errors to runtime, and ends up not much shorter than
  the manual builder once you write the registration code.

**Recommendation: the manual builder.**

```rust
pub struct WalletServices {
    pub wallet:    Arc<WalletService>,
    pub dust:      Arc<DustSyncService>,
    pub did:       Arc<DidService>,
    pub ic:        Arc<IdentityCentreService>,
    pub oid4vp:    Arc<Oid4vpService>,
    pub oid4vci:   Arc<Oid4vciService>,
    pub verify:    Arc<VcVerifyService>,
    pub backup:    Arc<BackupService>,
    pub secrets:   Arc<ControllerSecretService>,
    pub telemetry: Arc<TelemetryService>,
}

pub struct WalletServicesBuilder { /* all the Arc<dyn Port> fields */ }

impl WalletServicesBuilder {
    pub fn new() -> Self;
    pub fn with_http(self, h: Arc<dyn HttpClient>) -> Self;
    pub fn with_indexer(self, i: Arc<dyn IndexerClient>) -> Self;
    // … one per port …
    pub fn build(self) -> Result<WalletServices, BuildError>;
}
```

Defense: the wire graph is small (~15 ports, ~10 services). It is read often
when onboarding and changed rarely. The builder gives you compile-time errors
on missing fields once `build` checks for `None`s, no runtime reflection, no
extra dep, and the same source file is the single place to look when "where
does the headless binary get its `HttpClient` from?" comes up. The headless
binary's `main.rs` calls `WalletServicesBuilder::new().with_http(...).…build()`;
the Dioxus app's startup does the same with different adapter choices. Two
call sites, one builder.

### 2.8 Where the Dioxus app fits

After the refactor, `mobile-bench/dioxus-wallet/` shrinks to four concerns:

1. **Process startup** — `main.rs` constructs the adapters (the Dioxus-aware
   ones for `UserInterface` and `Notifications`, the platform-specific ones
   for the secret store on iOS / Android, the redb-backed wallet store on
   desktop), invokes `WalletServicesBuilder`, hands the resulting
   `Arc<WalletServices>` into the Dioxus app via `use_context_provider`.

2. **Component tree** — every `#[component]` accepts an injected service from
   context, not a `BridgeState`. A button calls
   `did.create_did(wallet_id).await`; a screen renders a `Signal<DidDocument>`
   that the component populated by awaiting `did.resolve_did(...)`. No
   component imports anything from `wallet-core` outside the service traits
   and the value-object types.

3. **`DioxusUiAdapter`** — the `UserInterface` impl that bridges the trait
   methods to Dioxus's reactive primitives: a `Signal<ProgressMap>` per verb
   for `report_stage`, a modal-mounting helper for `prompt_text`, etc. This
   is the only piece of the dioxus crate that knows about both Dioxus and the
   `UserInterface` port.

4. **Wry custom protocol** — the existing per-platform Wry setup for
   `midnight://` URIs etc. Unchanged by the refactor; lives in
   `platform/` as today.

What goes away from `BridgeState`:

- The `controller_secrets: HashMap` — now in `WalletStorage` / served by
  `ControllerSecretService`.
- The `WalletStore` handle — owned by `WalletService` directly.
- The metrics / probe handles — exposed via `TelemetryService`.
- The log capture handle — stays in `BridgeState` (logging is a cross-cutting
  concern the services do not own).
- The active-wallet-id and proof-server URL — move to a small
  `AppContext` struct that holds UI-only session state, or directly into the
  `WalletService`'s active-id slot.

The size target: today's `BridgeState` is the de-facto god object; after the
refactor it is empty or close to it, and the file `bridge.rs` either deletes
or becomes the home of the `DioxusUiAdapter`.

**The invariant:** anything you can do in the Dioxus UI you can do from
`headless-wallet`. If a feature is reachable from a button but not from a
verb, it is a layering bug — the logic lives in a component instead of a
service. The cure is to promote the logic into a service and have the
component call the service. If we hold that line, the integration test suite
in §2.6 is by construction a faithful regression test for the UI; the only
extra coverage the UI needs is rendering / interaction tests, which are out
of scope here.

---

## §3 Refactor Plan

This section translates the gap between §1 (what exists) and §2 (where we
want to land) into a sequenced refactor. Each wave is shippable: existing UI
keeps working, lib tests stay green, and the integration test coverage grows
monotonically. No big-bang rewrite, no broken-middle period.

The plan has seven waves. The first three are scaffolding; waves 4-6 do the
actual logic migration; wave 7 is cleanup. Estimated 30-45 commits total
depending on how aggressively we split each wave.

### Wave A — Foundations (scaffolding only)

**Goal:** establish the new module + crate boundaries without moving any
business logic yet.

| Step | Files touched | Verification |
|---|---|---|
| A1. Add `wallet-core/src/service/` module skeleton — one file per service from §2.2 (`wallet_service.rs`, `did_service.rs`, `identity_centre_service.rs`, `oid4vp_service.rs`, `oid4vci_service.rs`, `vc_verify_service.rs`, `backup_service.rs`, `controller_secret_service.rs`, `dust_sync_service.rs`, `telemetry_service.rs`). Each is a `pub struct` with `Arc<dyn Port>` fields + a constructor only. No methods yet. | `wallet-core/src/service/` (10 new files), `wallet-core/src/lib.rs` (re-exports) | `cargo build --features test-support`; `cargo test --lib` |
| A2. Add `wallet-core/src/service/mod.rs` with `WalletServices` struct + `WalletServicesBuilder`. Builder fluently accepts every existing port (`with_http`, `with_indexer`, `with_node`, `with_prover`, `with_clock`, `with_metrics`, `with_resource_probe`, `with_secret_storage`, `with_vc_storage`). Build returns `Result<WalletServices, BuildError>` with `BuildError::MissingPort("<name>")`. Builder consumes every port without yet using any. | `wallet-core/src/service/mod.rs` (new); `lib.rs` re-export | Round-trip test: builder with all mocks → service struct with all 10 services non-`Arc::ptr_eq` to None. |
| A3. New crate: `mobile-bench/headless-wallet/`. `Cargo.toml` carries `wallet-core` + `tokio` + `serde_json` + `clap`. `src/main.rs` is a stub that parses CLI flags + prints the parsed config to stderr + exits 0. Nothing functional, just the binary surface. | `mobile-bench/headless-wallet/Cargo.toml`, `src/main.rs`, workspace `Cargo.toml` (add member) | `cargo run -p headless-wallet -- --help`; smoke-test the flag parsing. |

**Why first:** these are pure additions. Existing code untouched. Each step
compiles + tests pass. The skeleton commits establish the module landmarks
that subsequent waves slot logic into; reviewers can read §2's catalog
against the actual file tree from the first PR onward.

### Wave B — Introduce missing ports

**Goal:** every port catalogued in §2.3 has a trait + a real adapter + a
mock adapter. No service code yet uses them; they're just available.

| Step | Files touched | Verification |
|---|---|---|
| B1. `Randomness` port — trait in `src/randomness.rs`; `OsRandomness` real adapter; `DeterministicRng` test adapter (ChaCha-seeded). Re-export from `lib.rs`. | `src/randomness.rs`, `lib.rs` | Adapter unit tests (`fill_bytes` length, determinism with same seed) |
| B2. `WalletStorage` port — extract the existing `wallet-core/src/store/` API into a trait. `RedbWalletStorage` is the existing impl renamed; `InMemoryWalletStorage` is a new `HashMap`-backed test adapter (`#[cfg(any(test, feature = "test-support"))]`). Existing `WalletStore` struct stays as the redb concrete; new trait wraps it for callers that want abstraction. Call sites are NOT migrated yet — that's wave D. | `src/store/{mod,api,memory}.rs` | Existing 253 lib tests still pass; new round-trip test against the in-memory backend |
| B3. `UnlockGate` port — `ScryptUnlockGate` real adapter wraps the existing scrypt+AES envelope; `AlwaysOk`/`NeverOk` test adapters. Today the unlock logic is inline in `app.rs::unlock_view`; we extract the policy, not the UI. | `src/unlock.rs` | Round-trip: same passphrase → identical seed; wrong passphrase → `UnlockError::BadPassphrase`. Lockout policy after N attempts. |
| B4. `ChainPublisher` port — trait carries `create_did_with_controller`, `deactivate_did`, `call_did_circuit`, `load_did_circuit`. `WalletChainPublisher` real adapter wraps `Wallet`; `StubChainPublisher` test adapter is the existing `test_support::stub_wallet` logic re-shaped into a standalone adapter. | `src/chain_publisher.rs`; touches `test_support.rs` | Existing `test_support` tests pass through the new adapter shape. New use-case-level test (in wave D) consumes `StubChainPublisher`. |
| B5. `Notifications` port — `StderrNotifier` for headless, `CollectingNotifier` for tests. (Dioxus adapter lands in wave E.) | `src/notifications.rs` | Smoke test: emit 3 notifies into `CollectingNotifier`, snapshot has 3 records. |
| B6. `UserInterface` port — trait per §2.5. `CliUiAdapter` for the headless binary (writes JSON events to stdout, reads stdin prompts). `TestUiAdapter` collects everything into `Vec<UiEvent>` with scripted prompt answers. (Dioxus adapter lands in wave E.) | `src/ui_port.rs`; touches `headless-wallet/src/cli_ui.rs` | `TestUiAdapter` driver test: report 5 stages, prompt twice, assert collected events match. |

**Total: 6 ports, ~6-8 commits.** Each port follows the same shape, so the
template is identical across the wave: trait → real adapter → mock/test
adapter → adapter unit tests → re-export.

After wave B, the port catalog in §2.3 is complete. The codebase has all
the abstractions it needs; no existing flow has moved through them yet.

### Wave C — Extract use-case bodies into services

**Goal:** every flow from §1.2 lives in a service method. The UI components
become thin wrappers that call the service and observe results — they no
longer own the `spawn(async move { … })` body.

Tackle each service one at a time. Each step has the same shape: read the
existing UI-embedded body, lift it into the service method, write
use-case tests with mocks, update the UI to call the service.

| Step | Service | UI sites collapsed | Use-case tests |
|---|---|---|---|
| C1. `ControllerSecretService` — simplest, no flows | `bridge.rs:remember_controller_secret` / `controller_secret_for_on` / `hydrate_controller_secrets` | 3 methods → CRUD on the secret table |
| C2. `TelemetryService` — second-simplest | `BridgeState::metrics`, `metrics_dyn`, `resource_probe`, `metrics_snapshot` | snapshot, reset, snapshot-after-reset |
| C3. `WalletService` — unlock + balance + UTXO snapshot | `app.rs` unlock view + `app.rs::connect()` lines 1507-1584 + Wallet-tab balance card | unlock happy path, unlock bad-passphrase, snapshot empty-utxo, snapshot N-utxo |
| C4. `DustSyncService` — sync DUST events | `WalletSyncPane`'s effect — currently builds `dust_syncer_for(net)`, drives it, updates signals | sync empty stream, sync N events, resume from checkpoint |
| C5. `DidService` — create / resolve / list / deactivate / update | `CreateDidWizard::on_done` + `DidInventoryPanel::on_resolved/on_seen` + `DidDetailView::deactivate` + `DidOperationBuilder` submit loop | create happy, create + immediate resolve, create + retry-on-indexer-lag, resolve unknown DID, deactivate, batch update |
| C6. `IdentityCentreService` — bootstrap + get-or-create | `BootstrapSection`'s spawn + `BootstrapSection`'s mount effect (the `find_by_kid` probe) | bootstrap fresh, bootstrap idempotent (re-call returns same), get-or-create finds existing |
| C7. `Oid4vpService` — paste URL → authenticate | `Oid4vpSection`'s `authenticate` closure | happy round-trip (mock issuer), 401 from issuer, malformed QR URL |
| C8. `Oid4vciService` — paste offer → issue VC | `Oid4vciSection`'s `request_vc` closure | happy round-trip, /token 4xx, /credential 5xx, vc_store full |
| C9. `VcVerifyService` — verify + list + revoke | `VcInventorySection`'s `verify` per-row closure + `list_ordered` use_resource | self_verify Valid, self_verify Invalid (bad signature), list ordering, revoke |
| C10. `BackupService` — already a service via R10's `store::backup`; just wrap it | `WalletBackupCard` in Settings | export + re-import round-trip; import-wrong-format; import-mid-version |

For each step:

1. **Lift the body.** Copy the `spawn(async move { … })` block contents out of the UI component into the service method. The closures of `BridgeState::…().cloned()` become `&self.field` references.
2. **Type the boundary.** Replace `&str` for DIDs with `&DidId`, `serde_json::Value` for circuit args with typed `DidUpdateOp` enums where the surface is small enough, error tuples with typed `thiserror` enums.
3. **Write use-case tests.** Spin up the service with mock adapters. Cover happy + failure paths per the table above. These tests are the contract — they outlast any individual rewrite of the service body.
4. **Migrate the UI call site.** The component now calls `self.services.did.create_did(&wallet_id).await` instead of running the body itself. UI signals (busy / err_msg / ok_msg) get set from the service's `Result` rather than driven by the body. The four "guard + locator" preludes in `identity_centre.rs:228-256 / 456-477 / 611-625 / 860-889` all collapse to one line apiece: a service call.

By the end of wave C, `BridgeState` is no longer the dependency container —
the services are. `app.rs::app_wallet_for()` shrinks dramatically because
the Wallet is constructed once at startup and held by the services, not
rebuilt per click.

**Critical sequencing constraint:** C5 (`DidService`) is the biggest. The
Create-DID wizard's on_done callback (`app.rs:1902-2068`) is ~160 lines of
business logic. Plan two PRs for C5: one for `create_did` + `resolve_did`
+ retry loop, one for `deactivate_did` + `update_did` + the Operation
Builder submit pipeline.

### Wave D — Replace `BridgeState` god-object with service injection

**Goal:** Dioxus components stop reaching for `BridgeState` and instead
pull services from a Dioxus `use_context`. `BridgeState` shrinks to UI-only
session state.

| Step | Files touched | Verification |
|---|---|---|
| D1. App startup constructs `WalletServices` once via `WalletServicesBuilder` (in `app.rs::run()` or `lib.rs::run()`). Result wrapped in `Arc<WalletServices>`, pushed into Dioxus context via `use_context_provider`. | `lib.rs`, `app.rs::run()` | App still launches; nothing wired through the context yet. |
| D2. Each service used by a `#[component]` gets pulled via `use_context::<Arc<WalletServices>>().wallet`. Migrate one component at a time: WalletSyncPane, BalancesCard, BootstrapSection, Oid4vpSection, Oid4vciSection, VcInventorySection, CreateDidWizard, DidInventoryPanel, DidDetailView, DidOperationBuilder, ControllerSecretCard, WalletBackupCard. | per-component edits in `dioxus-wallet/src/{app.rs,identity_centre.rs}` | UI still works; each migrated component has zero `BridgeState::…` calls. |
| D3. Remove `BridgeState` fields whose only remaining consumer was the now-migrated components: `controller_secrets`, `store`, `metrics`, `metrics_dyn`, `resource_probe`, `active_wallet_id`, `proof_server_url`. What remains is the `LogCapture` handle (logging is cross-cutting, lives outside the service tree) and any genuinely UI-only state (open-dialog flags etc). | `bridge.rs` | UI still works; `BridgeState` is ≤30 lines, single-responsibility (log routing + UI session). |
| D4. Delete `app_wallet_for(network)`. The four statics it read (`PROOF_SERVER_URL`, `DUST_SYNCERS`, `eval_bridge::global_bridge()`, `preprod-live` seed) are consumed by the `WalletServicesBuilder` at startup. The function call sites are all gone (replaced in C/D by service calls). | `app.rs` | UI still works; no more implicit-state wallet construction. |
| D5. Migrate the inline-bypass site at `app.rs:1530-1545` — `connect()` currently constructs `HttpIndexerClient` + `SubxtNodeClient` directly. After this step, the `connect()` flow calls `services.wallet.snapshot_chain(net)` which routes through the same `IndexerClient` / `NodeClient` ports the rest of the code uses. | `app.rs` | Connect button still does what it did; metrics now include the chain-probe ops. |

**Net effect of wave D:** `BridgeState` is gone-but-not-quite (it survives
as the `LogCapture` carrier + the UI-session-state slot). Every component
has one and only one source of dependencies (the service context). The
`app_wallet_for` static-soup is replaced by services that were constructed
once at startup with explicit adapter choices.

### Wave E — Headless binary

**Goal:** `cargo run -p headless-wallet` drives every flow from §2.4. This
is the moment we can write integration tests that don't need a simulator.

| Step | Files touched | Verification |
|---|---|---|
| E1. CLI argument parsing + service construction. `main.rs` reads `--network`, `--store-path`, `--passphrase-env/--passphrase-stdin/--passphrase`, `--proof-server`, `--indexer`, `--node`, the mock toggles, `--metrics-out`. Builds the dependency graph: same `WalletServicesBuilder` the dioxus side uses, just with `CliUiAdapter` for `UserInterface` and `StderrNotifier` for `Notifications`. | `headless-wallet/src/main.rs`, `headless-wallet/src/config.rs` | `cargo run -p headless-wallet -- --help` shows all flags; `--network standalone --in-memory-store` boots without errors. |
| E2. Line-delimited JSON protocol dispatcher. One `match` arm per verb from §2.4's table. Each verb deserialises `args` to a per-verb input struct, calls the service method, serialises the result to the response shape. Event-emitting verbs (`bootstrap-did`, `oid4vp-authenticate`, `oid4vci-issue`, `sync-dust`) collect stages from `UserInterface::report_stage` into the stdout stream as they fire. | `headless-wallet/src/dispatch.rs`, per-verb input/output structs | Drive a verb manually via `echo '{"verb":"unlock",...}' | headless-wallet`; check output JSON shape. |
| E3. `CliUiAdapter` implementation — `report_stage` prints `{"type":"event","verb":...,"stage":...,"data":...}`; `report_outcome` is folded into the `result`/`error` response (not its own line); `prompt_text` / `prompt_passphrase` read from stdin if `--interactive`, else from the per-verb `args` map. | `headless-wallet/src/cli_ui.rs` | Walk through a `--interactive` `oid4vp-authenticate` session against the issuer mock; QR URL prompt works. |
| E4. `--mock-http` and `--mock-chain` modes for offline integration tests. `--mock-http` registers a `MockHttpClient` that the test driver pre-loads via a sidecar command (`{"verb":"http-mock-push", ...}`). `--mock-chain` swaps the three chain ports for stubs from `test_support`. | `headless-wallet/src/mock_modes.rs` | Mock-mode session: load a 200-response mock, run `oid4vp-authenticate`, assert the response was consumed. |

**Sanity check at end of wave E:** the sample session in §2.4 actually
runs against a live standalone. Type each verb manually; observe the
event stream; confirm the resulting state matches (run `list-dids`,
`list-vcs`, `metrics`).

### Wave F — Integration tests

**Goal:** the 9 "must-have flows" from §2.6 are integration tests that
run against either a fresh standalone or via the `--mock-chain` mode.

| Step | Test file | Drives | Backing env |
|---|---|---|---|
| F1. `unlock_and_sync_night.rs` | `unlock` + `sync-night` | standalone |
| F2. `unlock_and_sync_dust.rs` | `unlock` + `sync-dust` (empty + N-event scenarios) | standalone |
| F3. `did_create_and_resolve.rs` | `create-did` + `resolve-did` + `list-dids` (asserts on inventory) | standalone |
| F4. `did_bootstrap_inventory.rs` | `bootstrap-did` end-to-end + inventory entry exists with correct VM count + controller secret round-trippable | standalone |
| F5. `oid4vp_against_mock_issuer.rs` | `bootstrap-did` + `oid4vp-authenticate` against a `httpmock`-driven mock issuer | standalone for the DID; `httpmock` for the issuer |
| F6. `oid4vci_against_mock_issuer.rs` | `bootstrap-did` + `oid4vci-issue` against mock issuer; assert VC + opening landed | standalone + `httpmock` |
| F7. `vc_self_verify_live_and_cached.rs` | issuance + `verify-vc` (twice: live first call, cached second call) + assert `last_verified_ms` written | standalone + mock issuer |
| F8. `backup_restore_roundtrip.rs` | mint a bunch of state → `export-wallet` → wipe → `import-wallet` → re-list → state matches | standalone + tempdir |
| F9. `did_deactivate.rs` + `did_update_via_op_builder.rs` | `deactivate-did` and `update-did` for each verb in the new circuit set (setVerificationMethod, setSchnorrJubjub, setRelation, removeService, etc.) | standalone |

Each test is structured the same way:

```rust
#[tokio::test]
#[cfg_attr(not(feature = "standalone-tests"), ignore)]
async fn did_bootstrap_inventory() {
    let env = TestEnv::up().await;          // brings up the docker stack if not running
    let mut wallet = HeadlessDriver::spawn(&env).await;

    wallet.unlock("alice", "test").await?;
    let did = wallet.bootstrap_did().await?;

    let list = wallet.list_dids().await?;
    assert!(list.iter().any(|d| d.did == did));

    let cs = wallet.controller_secret(&did).await?;
    assert!(cs.is_some());

    let resolved = wallet.resolve_did(&did).await?;
    assert_eq!(resolved.verification_method.len(), 2);

    env.drop_or_keep_dirty();                // env-aware cleanup
}
```

`HeadlessDriver` is a small test helper that spawns `headless-wallet` as a
child process, pipes stdin/stdout, exposes typed Rust methods that produce
+ parse the JSON. Lives in `headless-wallet/tests/common/`.

`TestEnv::up()` reuses the docker stack if up + healthy; otherwise spins
it up. Lives in `mobile-bench/tests/common/`. Both are reused across all
F-wave tests.

Gating: `#[cfg(feature = "standalone-tests")]` so `cargo test` does not
inadvertently spin up a docker stack. CI runs them in a dedicated job
with the stack pre-warmed.

### Wave G — Cleanup

**Goal:** delete the now-dead code paths, consolidate the Wallet builder,
fix the asymmetric cfg gates surfaced in §1.

| Step | Files touched |
|---|---|
| G1. Delete `BridgeState`'s remaining shadow fields (anything not exclusively `LogCapture` or UI-session state). |
| G2. Consolidate `Wallet`'s 12 `with_*` setters. After waves B-D, the production wallet is constructed once at startup with an explicit set of dependencies. The fluent setters can collapse into one constructor that takes the full record. The pre-existing `with_deps(seed, network, indexer, node, prover)` is the model; extend it to cover the now-canonical port set. |
| G3. Delete `test_support.rs::stub_wallet` if `StubChainPublisher` from B4 fully replaces it. (It probably does, but the existing internal callers — `#[cfg(test)] mod` blocks inside wallet-core — may still want it for legacy fixture reasons. Decide per-call-site.) |
| G4. Fix the `proof-server-http` cfg asymmetry — setter is gated, getter is not. After wave D the proof-server URL is a constructor arg, not a static, so the asymmetry just dissolves. |
| G5. Document the public surface. `wallet-core/README.md` gets a "Hexagonal architecture" section that links to this design doc and lists every service + port + adapter, mapped to file paths. |
| G6. Migrate the four orphan UI flows we noticed in §1 that did not get a service wrapper: the QR scanner (`scan` closure in `Oid4vpSection`), the activity-feed log-polling (the `use_effect` in `BootstrapSection`), the unlock-screen passphrase form (already covered by `WalletService::unlock` + UI port). |

After wave G the architecture matches §2 verbatim. The dioxus crate is
thin; the wallet-core crate is the home of every service + port; the
headless-wallet crate exists as the verb gateway and the integration test
driver.

### Sequencing rationale

The waves are ordered for two reasons.

**Risk minimisation.** Waves A-B are pure additions: nothing breaks
because nothing existing changes. Wave C is the largest behaviour-changing
wave but it migrates one flow at a time — each commit is shippable and
the UI keeps working. Wave D is mostly mechanical (replacing
`bridge_state.…` with `services.…`) and benefits from C's already-extracted
services. Wave E is again additive (a new binary). Wave F is test-only.
Wave G is deletion-only.

**Earliest possible integration tests.** The user's stated goal is "all
use cases covered by integration tests" without running the simulator.
Wave E (headless binary) is the earliest point we can start writing those
tests — F1-F3 can start mid-wave-E as proof that the binary actually
works. The simulator is only needed thereafter for genuine UI
regression. Most logic-bug regressions get caught at the headless layer.

### Notable risks

Three things are likely to bite during the refactor:

1. **`Wallet::call_did_circuit` requires the JS bridge.** The
   `did_bootstrap_standalone.rs` test pair is currently `#[ignore]`'d
   because `NodeChildBridge` can't carry the Compact runtime state for a
   live deploy + circuit call — only the `DioxusEvalBridge` (in-WebView)
   path works. The integration tests in wave F depend on driving
   `bootstrap_did` from a non-UI binary. Two paths to resolve:
   - **Path 1 (the right one long-term):** finish the
     `test_support::stub_wallet` path so `ChainPublisher`'s stub adapter
     fully bypasses the JS-bridge route for tests. This is what
     `test_support.rs` was set up for; it just needs to be wired into the
     `StubChainPublisher` in B4.
   - **Path 2 (interim):** stand up a Node-based bridge sidecar process
     the `headless-wallet` binary spawns and talks to over JSON-RPC,
     using the existing `tests/js-harness/`. Slower but exercises the
     real Compact runtime in tests.

   Wave F's F4 (`did_bootstrap_inventory.rs`) will need one of these to
   work. Path 1 is the recommended sequence; Path 2 is the escape hatch.

2. **`serde_json::Value` leakage at the `ChainPublisher::call_did_circuit`
   trait.** The §2 design accepts this as a known wart — typed circuit
   arguments require typed Rust bindings from the Compact compiler which
   is still in flux upstream. The integration tests cope by carrying the
   JSON shape verbatim from the production code. When the Compact
   toolchain stabilises around typed Rust output, the trait can be
   redesigned to use those types and the JSON serialisation pushed down
   to the real adapter — no service-level changes needed.

3. **PreProd operator-deployed DIDs cannot be controlled.** §1's
   coupling-smell #4 highlights that `bootstrap_did_with_keys` carries
   the controller_sk in its `BootstrappedDid` return value, but PreProd
   DIDs deployed by the manager-service have non-HD-derived random
   controller keys we don't possess. The hex refactor surfaces this
   cleanly (errors arrive from `ChainPublisher`, not from tangled call
   stacks) but does not solve it. Update / Deactivate verbs against
   operator-deployed PreProd DIDs will fail at the chain layer.
   Integration tests F-wave should distinguish "DIDs we created" from
   "DIDs we observed"; only the former should expect Update / Deactivate
   to work.

### Effort estimate

| Wave | Commits | Time |
|---|---|---|
| A — Foundations | 3-4 | 1 day |
| B — Missing ports | 6-8 | 3-4 days |
| C — Extract services | 10-15 | 1-2 weeks |
| D — Replace BridgeState | 5-7 | 2-3 days |
| E — Headless binary | 4-6 | 3-4 days |
| F — Integration tests | 9-12 | 1 week |
| G — Cleanup | 4-6 | 1-2 days |
| **Total** | **41-58 commits** | **~3-4 weeks** |

Single developer + reviewer. Parallelisable across two developers if wave
C is split by service.
