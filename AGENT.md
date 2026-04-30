# AGENT.md — context for Claude / agent sessions

Project-level notes carried across Claude sessions. Captures the *why*
behind in-progress branches, decisions taken, and pitfalls
discovered. Read this first; CLAUDE.md (if present) covers global
conventions, AGENT.md covers what's specific to this repo right now.

> **Maintenance rule.** Update this file at the end of every
> iteration — add a new section under *Iteration log* with date,
> what shipped, what moved, and any non-obvious knowledge. Don't
> summarise commits (`git log` is authoritative); capture only
> knowledge that is **not derivable from code or git history**.

## Repo layout (high level)

`midnight-ledger` is the core ledger crate workspace for Midnight.
The top-level Cargo workspace has ~25 crates: cryptography
(`base-crypto`, `transient-crypto`), ZK circuits (`zkir`, `zkir-v3`,
`zkir-precompiles`), storage (`storage`, `storage-core`), VM
(`onchain-vm`, `onchain-runtime`), ledger types (`ledger`,
`coin-structure`, `zswap`), wasm bindings (`*-wasm`), proof server
(`proof-server`).

**Self-dev-deps are intentional.** Many crates list themselves under
`[dev-dependencies]` with extra features
(proptest/test-utilities) so `cargo test -p X` automatically
activates the test harness. **Do not remove this pattern** — it's a
deliberate choice; removing it would force every contributor to
remember `--features` flags. (This is also what panics
`dioxus-cli`'s bundled `krates 0.17.5` — see the dx workaround
below.)

### `mobile-bench/` subtree

Everything under `mobile-bench/` is the agent-driven mobile + wallet
work. None of it is on `ledger-8`; live on
`mobile-bench/iteration-{1,2}` and successor branches.

| Path | Purpose |
|---|---|
| `mobile-bench/prover-core/` | Embeddable Rust prover wrapping Midnight primitives. Consumed by tests, criterion benches, the dioxus UI, and the headless `bench-runner`. HTTP path lives behind feature `proof-server-http`. |
| `mobile-bench/dioxus-bench/` | Dioxus 0.6 UI for **proof benchmarking**. Three buttons (`zkir-minimal-assert`, `zkir-hash-to-curve`, `zkir-ec-mul-add`). Cross-platform (`cdylib` on Android, `bin` on desktop). |
| `mobile-bench/wallet-core/` | Pure-Rust wallet primitives: `Network`, `Wallet`, BIP32 HD, bech32m address, indexer GraphQL client, node JSON-RPC client, connectivity probe. No UI. |
| `mobile-bench/dioxus-wallet/` | Dioxus 0.6 wallet UI. Phone-sized window (390 × 844) on desktop so we can iterate without an emulator. Loads a hardcoded demo seed on first paint. |
| `mobile-bench/fixtures/` | Vendored zkir test artefacts. Iter-2 still uses inline raw-IR strings; fixtures held for later. |
| `mobile-bench/scripts/setup-android-toolchain.sh` | One-shot macOS toolchain installer. |
| `mobile-bench/scripts/standalone.yml` + `standalone-up.sh` / `standalone-down.sh` | Local Midnight stack (node + indexer + proof-server) on fixed loopback ports for `Undeployed` testing. Pulls `midnightntwrk/midnight-node:0.22.3`, `indexer-standalone:4.0.0`, `proof-server:8.0.3` images. |
| `mobile-bench/RESULTS.md` | Captured proof-bench latency numbers (iter-1 + iter-2). |
| `mobile-bench/DEPLOY_TO_DEVICE.md` | APK / `bench-runner` deployment to a real phone. |
| `mobile-bench/WALLET_PLAN.md` | Full wallet implementation plan: dep strategy, iterations 1–4, open questions, reference index. |
| `mobile-bench/MOBILE_WALLET.md` | Mobile-focused use cases + dark theme spec + implementation phases. |
| `mobile-bench/DID_PLAN.md` | Rust-native Midnight DID API plan; replaces the abandoned in-WebView TS approach. |
| `mobile-bench/UX_DESIGN.md` | UX master: sitemap + nav + screen catalog + cross-cutting patterns + component library. **Update in the same commit as any user-visible change.** |
| `mobile-bench/WALLET_RESULTS.md` | Live wallet outcomes per step (preprod chain tip, node status, etc.). |

## Active work

**Branch:** `mobile-bench/iteration-2`
**Worktree:** `.claude/worktrees/thirsty-lovelace-092f50/`
**Current focus:** wallet UI (mobile dark theme, full bech32m address
display, Connect → probe + chain_tip + system_health). Phase B —
real WS sync drivers — is the next step.

The iter-2 branch contains both the proof-bench iter-2 work
(hash-to-curve + ec proofs) and the wallet scaffolding through
Phase A. The wallet doesn't have its own branch yet.

## Decision log — upstream repos as deps, not reinvent

A 2026-04-29 survey of `midnightntwrk/{midnight-zk, midnight-node,
midnight-indexer, midnight-wallet}` confirmed:

- **midnight-zk** is pure circuits/proofs (`midnight-{curves,
  proofs, circuits, zk-stdlib}`). Already pulled transitively via
  `transient-crypto` / `zkir`. **No wallet-shaped helpers** (no
  BIP39, bech32, address encoders). Skip as a direct dep.
- **midnight-indexer** ships the canonical schema at
  `indexer-api/graphql/schema-v4.graphql`, a wallet-shaped query set
  at `indexer-tests/e2e.graphql`, and a reference
  `graphql-transport-ws` client at
  `indexer-tests/src/graphql_ws_client.rs`. **No published client
  crate** (workspace `publish = false`) but the schema is the
  redistributable artifact and the maintainers themselves use
  `graphql_client = 0.16` codegen — so do we.
- **midnight-node** is a Substrate/FRAME node. Workspace
  `publish = false`, but `midnight-node-metadata` purpose-builds for
  off-node consumers: bundles SCALE metadata blobs through
  `subxt::subxt!`. Custom RPC modules
  (`pallet-midnight-rpc`, `pallet-system-parameters-rpc`,
  `pallet-sidechain-rpc`, `pallet-session-validator-management-rpc`)
  define `jsonrpsee` server traits whose **request/response structs
  we can reuse client-side**.
- **midnight-wallet** is a Yarn 4 + Turbo monorepo with 15 packages.
  The wallet *facade* itself is RxJS/Effect glue we don't port. The
  load-bearing pieces we **do** port to Rust:
  - `packages/hd/src/HDWallet.ts` → `wallet_core::hd` (over `bip32`)
  - `packages/address-format/src/index.ts` → `wallet_core::address`
    (over `bech32` Bech32m variant)
  - `packages/abstractions/src/SyncProgress.ts` → Rust struct
  - `packages/{dust,unshielded,shielded}-wallet/src/v1/Sync.ts` →
    Rust sync drivers (Phase B onwards)
- **example-counter** `counter-cli/src/cli.ts` is the iter-1
  functional bar — every wallet operation that CLI does, our Rust
  wallet does too on the same endpoints with the same observable
  behavior.

## Key technical findings

These cost real time to discover. Each is a cross-session pitfall.

### dioxus-cli `dx` is unusable on this workspace

`dx serve` and `dx build` from dioxus-cli 0.6.3 *and* 0.7.6 panic on
this workspace's cargo metadata. Root cause: both ship `krates
0.17.5`, which panics on the self-path-deps pattern (10+ crates list
themselves as `dev-dependencies`). dioxus main is also still on
0.17.5, so a CLI upgrade alone won't fix it.

**Workaround.** Use plain `cargo` everywhere:
- Desktop UI: `cargo run -p dioxus-bench` / `cargo run -p dioxus-wallet`
- Android cross-compile: `cargo ndk -t arm64-v8a build --release -p dioxus-bench --lib`
- APK packaging: hand-rolled Gradle scaffold under
  `mobile-bench/dioxus-bench/android/` (copied from dioxus-cli
  0.6.3's bundled template, `.hbs` rendered manually).

### Dioxus feature gating must be **mutually exclusive** per target

Cargo unions features across `[dependencies]` and
`[target.'cfg(...)'.dependencies]`. If both `dioxus/desktop` and
`dioxus/mobile` are active on Android, `dioxus::launch` selects the
desktop launcher → blocks the JNI thread inside `Activity.onCreate`
→ WebView never attaches → splash screen forever
(`InputDispatcher: NO_INPUT_CHANNEL`).

**Correct pattern** (used in both `dioxus-bench` and `dioxus-wallet`
Cargo.toml):

```toml
[target.'cfg(target_os = "android")'.dependencies]
dioxus = { version = "0.6", features = ["mobile"] }

[target.'cfg(not(target_os = "android"))'.dependencies]
dioxus = { version = "0.6", features = ["desktop"] }
```

Never put a top-level `dioxus = { features = ["..."] }` line.

### Dioxus 0.6 desktop window sizing — for mobile iteration without an emulator

`dioxus-wallet` opens at **390 × 844** (iPhone 14 / Pixel 7a
envelope) so we can iterate on the mobile layout without an
emulator. Pattern:

```rust
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
let cfg = Config::new().with_window(
    WindowBuilder::new()
        .with_title("Midnight Wallet")
        .with_inner_size(LogicalSize::new(390.0, 844.0)),
);
dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(app::App);
```

The Android branch still uses the plain `dioxus::launch` shim — the
WindowBuilder API is desktop-only.

### Dioxus 0.6 manganis asset pipeline can drop CSS in release

`asset!("/assets/styles.css")` works in dev for `dioxus-bench` but
silently doesn't reach the WebView in release for `dioxus-wallet`.
**Fix**: inline the stylesheet via `include_str!`:

```rust
const STYLES: &str = include_str!("../assets/styles.css");
rsx! { style { "{STYLES}" } /* ... */ }
```

Compile-time inlined, no build-time bundler dance, works on every
target.

### `color-scheme: dark` is required for native dark form controls

Without `color-scheme: dark` on `:root`, the WebView renders native
scrollbars + `<select>` chevrons in light mode regardless of the
custom CSS. Set it on `:root` together with the `--bg` token in
`mobile-bench/dioxus-wallet/assets/styles.css`.

### rustls 0.23 multi-provider crash

`reqwest`, `tokio-tungstenite`, `jsonrpsee`, and `dioxus-desktop`
each pull rustls-with-different-providers transitively. rustls 0.23
**panics at TLS-handshake time** if multiple providers are linked
but none is marked default ("Could not automatically determine the
process-level CryptoProvider").

**Fix**: install `ring` once on first network use. See
`mobile-bench/wallet-core/src/crypto.rs::ensure_default_crypto_provider`,
called from `probe_connectivity`, `IndexerClient::new`, and
`NodeClient::connect`. Idempotent via `std::sync::Once`.

### Indexer WS requires the GraphQL subprotocol header

`wss://indexer.<env>.midnight.network/api/v4/graphql/ws` rejects
bare WebSocket upgrades with HTTP 400. The required header is:

```
Sec-WebSocket-Protocol: graphql-transport-ws
```

Set per-endpoint in
`mobile-bench/wallet-core/src/probe.rs::probe_ws` (subprotocol arg).
Plain WS endpoints like `wss://rpc.<env>.midnight.network` accept a
bare upgrade — set `subprotocol: None` for those.

### `bip32::Error` does not implement `std::error::Error`

`bip32 = "0.5"` is `no_std`-friendly so its `Error` enum doesn't
impl `std::error::Error`. `#[from]` in thiserror chokes. Wrap at the
boundary as a String:

```rust
#[derive(Debug, thiserror::Error)]
pub enum HdError {
    #[error("bip32: {0}")]
    Bip32(String),
}
impl From<bip32::Error> for HdError {
    fn from(e: bip32::Error) -> Self { HdError::Bip32(e.to_string()) }
}
```

### Unshielded address derivation pipeline

End-to-end: `seed (32 B) → BIP32 m/44'/2400'/0'/0/0 → secp256k1
secret → BIP340 schnorr verifying-key (32-byte x-only) → SHA-256(pk)
→ bech32m(HRP, 32-byte payload)`.

HRP per network — note the literal network string, **not**
`mn_addr_test...`:

| Network | HRP |
|---|---|
| Mainnet    | `mn_addr` |
| PreProd    | `mn_addr_preprod` |
| Preview    | `mn_addr_preview` |
| QANet      | `mn_addr_qanet` |
| DevNet     | `mn_addr_devnet` |
| Undeployed | `mn_addr_undeployed` |

The trailing `1` you see in `mn_addr_preprod1...` is the bech32
separator, not part of the HRP.

`base_crypto::signatures::SigningKey::from_bytes` +
`coin_structure::coin::UserAddress::from(VerifyingKey)` already
implement the schnorr-pubkey + SHA-256 pipeline — **don't reach for
k256 directly**, leverage the workspace.

### Android cross-compile of the proving stack

The full proving stack — `midnight-zk-stdlib`, `transient-crypto`,
`ledger`, `zswap`, `zkir`, `zkir-v3`, `prover-core` — cross-compiles
cleanly to `aarch64-linux-android` with **NDK r27**
(`27.0.12077973`) and `cargo-ndk`. No CMake patches, no C bindgen
tweaks. NDK r26 was not tested.

```
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a build --release ...
```

### Android entry point glue

`libdioxusmain.so` (or `libdioxuswalletmain.so`) exports both:
- `Java_dev_dioxus_main_WryActivity_*` JNI symbols (auto-generated
  by dioxus-mobile's macros — confirm with `nm -D ... | grep Java_`).
- A `pub extern "C" fn main` shim that dioxus-mobile's
  `JNI_OnLoad` looks up via `dlsym(RTLD_DEFAULT, "main")` to
  bootstrap the Tao event loop.

Both must be present. See
[mobile-bench/dioxus-bench/src/lib.rs](mobile-bench/dioxus-bench/src/lib.rs).

### Parameter cache on device

`prover-core` reads SRS params from `MIDNIGHT_PP`. On Android
without network, pre-push files to a world-readable location:

```bash
adb shell mkdir -p /data/local/tmp/midnight-pp
adb push ~/.cache/midnight/zk-params/bls_midnight_2pN /data/local/tmp/midnight-pp/
```

Files needed for iter-2 surfaces: `bls_midnight_2p4` (zkir minimal),
`bls_midnight_2p9` (htc), `bls_midnight_2p11` (ec).
[mobile-bench/dioxus-bench/src/platform/android.rs](mobile-bench/dioxus-bench/src/platform/android.rs)
defaults `MIDNIGHT_PP` to `/data/local/tmp/midnight-pp` if unset.

### Midnight node — extrinsic deploy path

Probed the running node via `subxt::OnlineClient` +
`midnight-node-metadata` (tag `node-0.22.3`,
`subxt = "0.44.0"`). Findings now cached in
`mobile-bench/wallet-core/examples/probe_metadata.rs`:

- 28 pallets exposed.
- **Contract deploy goes through `Midnight.send_mn_transaction`
  (1 SCALE-bytes field).** That's the single entry point for
  deploys / circuit calls / extrinsic-style updates — the bytes
  are a SCALE-encoded `LedgerTransaction` from the `ledger` crate.
  Other call on this pallet is `set_tx_size_weight` (admin).
- `spec_version = 22000` matches the `node-0.22.3` tag.

Next-session blocker: the substrate tx envelope expects
`MultiSignature` (sr25519/ecdsa/ed25519). Our wallet's keys are
BIP340 schnorr. midnight-did-api's `signTransactionIntents` does
a manual hop — verify what the metadata's `extrinsic.signature`
actually accepts before assuming sr25519, since the chain may
expose a custom `SignatureScheme` variant.

### Compact contract state encoding

Compact contracts (e.g. `did.compact`) emit on-chain state as a
nested `StateValue` tree, **not** a single SCALE blob. The
`midnight-indexer` returns the state hex via
`Query.contractAction(address).state`; we decode with
`tagged_deserialize::<onchain_state::ContractState<DefaultDB>>`,
then walk into `state.data.state` (a `StateValue`).

For DID specifically, root is a 2-element `StateValue::Array`:

- `root[0]` (constants): `[contractVersion, controllerPublicKey]`
- `root[1]` (mutable): 15 fields in declaration order — `id`,
  `alsoKnownAs` (Map), `version`, `created`, `updated`,
  `deactivated`, `active`, `operationCount`, `verificationMethods`
  (Map), 5 relation Maps, `services` (Map).

Field paths were extracted from
`midnight-did-contract/dist/managed/did/contract/index.js`'s ledger
accessors — every getter has a `path: [{ tag: 'value', value:
toValue(I) }, { tag: 'value', value: toValue(J) }]` pattern, and `(I,
J)` are exactly the field-tree indices. **Trust those over the
`.d.ts` field declaration order — they're the canonical layout.**

`AlignedValue` payload conventions:

- **`CompactTypeOpaqueString`** — single atom, raw UTF-8 (no length
  prefix; the atom boundary is the length).
- **`CompactTypeBoolean`** — single atom, 1 byte (0 / 1).
- **`CompactTypeEnum(max, bytes)`** — single atom, `bytes` long, big-
  endian tag. `did.compact`: `KeyType` (max=3, 1B), `CurveType` (max=2,
  1B), `VerificationMethodType` (max=1, 1B), `Relation` (max=5, 1B).
- **`CompactTypeBytes(N)`** — single atom, exactly N raw bytes.
- **`CompactTypeUnsignedInteger(MAX, bytes)`** — single atom, `bytes`
  long, big-endian, right-aligned in a fixed-width buffer.
- **Struct types** (e.g. `Service`, `VerificationMethod`,
  `PublicKeyJwk`) — concatenation of their fields' atoms in
  declaration order. e.g. `Service` AlignedValue = 3 atoms (id, typ,
  serviceEndpoint, all strings); `VerificationMethod` = 6 atoms
  (id-string, typ-1B-enum, kty-1B-enum, crv-1B-enum, x-32B-field,
  y-32B-field).
- **`Map<K, V>`** in a state slot — `StateValue::Map` whose keys are
  `AlignedValue` and values are `StateValue`. For sets the value is
  `StateValue::Null`. For struct values it's `StateValue::Cell(av)`
  carrying the struct's concatenated atoms.

The decoder lives in
[`mobile-bench/wallet-core/src/did/contract.rs`](mobile-bench/wallet-core/src/did/contract.rs).
It re-implements the `_descriptor_X.fromValue(value)` pattern by
reading atoms in order — no Compact runtime needed.

**To access `ChargedState.state`** you need
`onchain-state` with the `public-internal-structure` feature on
(the field is `pub(crate)` otherwise). Set in
`mobile-bench/wallet-core/Cargo.toml`.

### Pivot: Rust-native DID port instead of TS-in-WebView

We previously built a complete pipeline to load `midnight-did`'s TS
package inside the Dioxus WebView via a Wry custom protocol
(`mn-pkg://`), import maps, and a fetch-based wbindgen wrapper. It
worked end-to-end (proven: `await import("@midnight-ntwrk/midnight-
did-contract")` instantiated WebAssembly natively, dynamic imports
via the protocol succeeded). Decision (2026-04-30): **retire the
in-WebView TS approach**, port midnight-did to Rust directly. See
[`mobile-bench/DID_PLAN.md`](mobile-bench/DID_PLAN.md) for rationale.

The TS pipeline is **kept behind the `js-bridge` cargo feature**
(default off) for forward compatibility — flip on to load any
unported upstream TS package directly. All the JS pipeline machinery
(esbuild bundle, vendor.mjs, mn-pkg:// protocol, bridge.rs JSON-RPC,
head injection of import map + module bundle) compiles only when
the feature is on.

**Land-mines we hit on the TS side** (now mostly irrelevant unless
`--features js-bridge` is on):

- Native WebAssembly ESM Integration is **not enabled in WKWebView**
  even on macOS 14.4+. The default wbindgen entry
  `import * as wasm from "./xxx.wasm"` silently produces an
  uninstantiated module → `(void 0) is not a function` at first call.
  Fix in `web/vendor.mjs`: rewrite each `_wasm.js` entry at vendor
  time with a manual `fetch + WebAssembly.instantiateStreaming`
  wrapper that mirrors the upstream Node loader (`*_fs.js`).
- `<script type="module">` rendered into the DOM via Dioxus rsx
  doesn't execute (HTML spec: dynamically-inserted module scripts
  don't run). Inject the bundle via `Config::with_custom_head`
  instead.
- esbuild `loader: { ".wasm": "file" }` returns a URL string, not a
  module — does NOT trigger native WASM instantiation. Either the
  wbindgen rewrite above, or mark the package `external` and serve
  via `mn-pkg://`.
- Some upstream packages are CJS (e.g. `object-inspect`); browsers
  can't import CJS natively. `web/vendor.mjs` esbuild-converts those
  to ESM at vendor time.
- `web/build.mjs` needs `nodeModulesPolyfillPlugin` for `path` /
  `crypto` / `assert` / `util` / `events` / `stream` / `buffer`
  (and empty stubs for `fs` / `os`).

### Local Midnight stack (`Undeployed` network)

`mobile-bench/scripts/standalone-up.sh` brings up node + indexer +
proof-server on fixed loopback ports
(`127.0.0.1:9944` / `:8088` / `:6300`). Pinned images come from the
`midnight-did` standalone compose:

- `midnightntwrk/midnight-node:0.22.3` (`CFG_PRESET=dev`, dev preset
  has prefunded W0–W3 wallets we can faucet from once Phase 3 lands)
- `midnightntwrk/indexer-standalone:4.0.0` (env-file
  `mobile-bench/scripts/standalone.env.example` — dev-only secret +
  passwords)
- `midnightntwrk/proof-server:8.0.3`

Caveat: the proof-server image **does not ship `curl`**, so the
upstream healthcheck `curl -f http://localhost:6300/version`
permanently reports unhealthy. We override with a `/proc/net/tcp`
listening-port check that grep's for `:18A4 ` (= 0x18A4 = 6300 in
hex). Without that override `docker compose up --wait` hangs forever
on `--wait`.

The wallet's `Undeployed` config is already pointed at these URLs
(`wallet-core::network::Network::Undeployed`); switch the dropdown
to Undeployed in the UI and `Connect` works against the local stack.

### **Never screenshot mobile emulators**

Hard rule from a prior incident — also stored in
`~/.claude/projects/-Users-ysh-iohk-midnight-ledger/memory/`. Any
`adb exec-out screencap`, `xcrun simctl io ... screenshot`, or
MCP browser/preview screenshot tool pointed at an emulator hangs
Claude Code and prevents recovery. Capture mobile state via text
only (`adb logcat`, `bench-runner` stdout, criterion JSON, `adb
pull`). If the user needs a screenshot, ask them to capture it
themselves.

## How to resume / build / run

```bash
# Always start at the worktree root
cd .claude/worktrees/thirsty-lovelace-092f50

# ── Proof bench (mobile-bench/iteration-2 surfaces)
cargo run -p dioxus-bench --release      # 3-button UI
cargo run -p prover-core --bin bench-runner --release -- all
                                         # JSON line per surface (zkir|htc|ec|all)

# ── Wallet UI (Phase A — connect-but-no-sync)
cargo run -p dioxus-wallet --release     # opens at 390x844

# ── Wallet sanity tests
cargo test -p wallet-core --lib          # unit tests, no network
cargo test -p wallet-core --features network-tests --test preprod_probe \
  -- --nocapture                         # live preprod probe + chain tip + system_health

# ── Address derivation example
cargo run -p wallet-core --example show_addr
# Prints the demo wallet's bech32m address per network. The PreProd
# value is what to faucet to test the wallet against real funds:
#   mn_addr_preprod1ahhcw7swj7rnmcju6ldwgs0ghwxxwaakfz0sq7vdcmqj4827g68suryn3a

# ── Android emulator (proof bench)
~/Library/Android/sdk/platform-tools/adb push \
  target/aarch64-linux-android/release/bench-runner /data/local/tmp/
~/Library/Android/sdk/platform-tools/adb shell '
  MIDNIGHT_PP=/data/local/tmp/midnight-pp \
  BENCH_CACHE_DIR=/data/local/tmp/bench-cache \
  /data/local/tmp/bench-runner all'
```

For full repro of bench numbers see
[mobile-bench/RESULTS.md](mobile-bench/RESULTS.md). For wallet
outcomes per step see
[mobile-bench/WALLET_RESULTS.md](mobile-bench/WALLET_RESULTS.md).
For physical-phone deployment see
[mobile-bench/DEPLOY_TO_DEVICE.md](mobile-bench/DEPLOY_TO_DEVICE.md).

## Iteration log

### Iteration 1 — proof bench desktop + emulator E2E (2026-04-28)

Branch: `mobile-bench/iteration-1`. Shipped:

- `prover-core` library + HTTP path + `bench-runner` binary (signed
  commits).
- Cross-compile recipe + emulator latency numbers in
  [mobile-bench/RESULTS.md](mobile-bench/RESULTS.md): macOS M2 Max
  release prove ≈ 25 ms / verify ≈ 9 ms; Pixel Fold API 35
  emulator (arm64-v8a translated on M2 Max) prove 82–106 ms / verify
  38–58 ms.
- Dioxus desktop window. Dioxus Android APK working — Gradle
  scaffold under `mobile-bench/dioxus-bench/android/`,
  desktop-vs-mobile feature unification fixed, APK launches the
  WebView.
- [mobile-bench/DEPLOY_TO_DEVICE.md](mobile-bench/DEPLOY_TO_DEVICE.md)
  for Samsung S24 Ultra deployment.

### Iteration 2 — hash-to-curve + ec proofs (2026-04-28)

Branch: `mobile-bench/iteration-2` (off iter-1). Shipped:

- New proof surfaces: `zkir-hash-to-curve` (k=9) and
  `zkir-ec-mul-add` (k=11), lifted from
  [zkir/tests/proofs.rs](zkir/tests/proofs.rs) (`test_htc_proof`,
  `test_ec_proof`).
- Shared `ExampleResolver` extracted from `zkir_example`.
- `bench-runner` accepts `zkir|htc|ec|all`.
- Dioxus desktop UI exposes three buttons sharing a busy-state
  guard.
- macOS M2 Max release numbers: zkir k=4 24 ms / htc k=9 107 ms /
  ec k=11 317 ms (all verify=true).
- Library tests in
  [mobile-bench/prover-core/tests/library_path.rs](mobile-bench/prover-core/tests/library_path.rs)
  exercise all three surfaces.
- Android emulator + S24 Ultra capture deferred but documented
  (text-only via `adb shell`).

### Wallet iter-1 step-1 — wallet-core + dioxus-wallet skeleton (2026-04-28)

Same branch (`mobile-bench/iteration-2`). Shipped:

- `mobile-bench/wallet-core/` crate: `Network` enum (all 6 envs
  URLs verbatim from gsd-wallet's `environments.ts`), `Wallet`
  (random / deterministic / from-hex / chacha-seed / **demo from
  hardcoded `DEMO_SEED_HEX`**), `probe_connectivity` (parallel HTTP
  `__typename` GraphQL probe + WS upgrade probes with 5 s budget).
- `mobile-bench/dioxus-wallet/` crate: Dioxus 0.6 UI with a
  network dropdown, "Generate random wallet" / "Reload demo
  wallet", "Connect" CTA, probe results card.
- 5 `wallet-core` unit tests + 1 live preprod probe (gated on
  `--features network-tests`).

### Wallet iter-1 step-2 — real indexer + node queries (2026-04-29)

Same branch. Shipped:

- Vendored `schema-v4.graphql` + `e2e.graphql` from
  midnightntwrk/midnight-indexer (sha-pinned at the commit time of
  capture).
- `wallet_core::indexer::IndexerClient` with `chain_tip` query via
  `graphql_client = 0.16` codegen (matches indexer-tests' own
  toolchain).
- `wallet_core::node::NodeClient` over `jsonrpsee = 0.24` raw
  JSON-RPC: `system_health`, `chain_getFinalizedHead`. **Phase-1**
  of node RPC; switches to `subxt` + `midnight-node-metadata` git
  dep when typed extrinsics land.
- Connect screen wires probe → chain_tip + node status in parallel,
  renders a Chain state card with indexer tip + node peers +
  finalized head.
- Live preprod numbers: indexer tip ≈ 557 902, node finalized head
  `0xe4092ff…`, 8 peers.
- Discovered the **graphql-transport-ws subprotocol** requirement
  (above) and the **rustls multi-provider** crash (above).

### Wallet iter-1 step-3 — Phase A mobile UI + dark theme (2026-04-29)

Same branch. Shipped:

- Surveyed `midnightntwrk/{midnight-zk, midnight-node,
  midnight-indexer, midnight-wallet}` + `example-counter` +
  `1am.xyz` to pin the dep strategy. Resolved the address-format
  open question (bech32m + per-network HRP) and the sync-algorithm
  open question (ports of midnight-wallet's per-asset Sync.ts).
- Updated [mobile-bench/WALLET_PLAN.md](mobile-bench/WALLET_PLAN.md)
  + new [mobile-bench/MOBILE_WALLET.md](mobile-bench/MOBILE_WALLET.md)
  capturing the four use cases, dark theme spec, and four
  implementation phases.
- `wallet_core::hd` over `bip32 = "0.5"` — BIP32 derivation along
  `m/44'/2400'/<account>'/<role>/<index>` with role enum
  `NightExternal=0 / NightInternal=1 / Dust=2 / Zswap=3 /
  Metadata=4`.
- `wallet_core::address::unshielded_bech32m` over `bech32 = "0.11"`
  — bech32m HRP per network. Reuses
  `base_crypto::signatures::{SigningKey, VerifyingKey}` +
  `coin_structure::coin::UserAddress::from(VerifyingKey)` (in-
  workspace types do the schnorr → SHA-256 work already).
- `Wallet::unshielded_address(network)` — what the user pastes
  into a faucet to top the wallet up.
- Mobile dark-theme UI: phone-sized desktop window (390 × 844),
  dark palette refined against 1am.xyz (near-black `#0a0b0d`,
  5-step surface scale, deep blue-violet `#7c8cff` accent), full
  bech32m address visible (no truncation; user requested), `arboard`
  clipboard wired for desktop, status sub-line as
  dot+UPPERCASE label, primary CTA with cubic-bezier press-bounce.
- 14 `wallet-core` unit tests pass; all 3 live preprod tests
  (probe / chain tip / node status) green.

Open / deferred:

- **Phase B — sync drivers**: lift
  `indexer-tests/src/graphql_ws_client.rs` from
  midnight-indexer; port `unshielded` + `dust` Sync.ts folds;
  `Wallet::start_sync(network) -> SyncHandle` over `tokio::spawn` +
  `watch` channels.
- **Phase C — UC-4 balances**: NIGHT + Dust formatters (stars →
  decimal NIGHT; specks → decimal DUST), DUST regen progress bar,
  Advanced disclosure with seed / shielded address / session id.
- **Phase D — Android polish**: Gradle scaffold for
  `dioxus-wallet`, Android `ClipboardManager` JNI hop, safe-area
  insets verification.
- **Address derivation parity check**: confirm
  `mn_addr_preprod1ahhcw7swj7rnmcju6ldwgs0ghwxxwaakfz0sq7vdcmqj4827g68suryn3a`
  is recognised by an actual midnight-wallet TS client (e.g.
  paste into gsd-wallet's send form) before we trust user funds
  faucet'd against it.
- **Real-device latency** (S24 Ultra) for proof bench iter-1/iter-2
  surfaces — hardware needed.
- **Subxt + midnight-node-metadata** for typed extrinsic
  submission — required for iter-2 send.

### DID iter — types, codec, full resolver, standalone stack (2026-04-30)

Same branch (`mobile-bench/iteration-2`). Shipped:

- `wallet_core::did` module with full DID Core type set
  (`DidDocument`, `VerificationMethod`, `Service`, `PublicKeyJwk`,
  `KeyType`, `CurveType`); `DidId` parser/codec for
  `did:midnight:<network>:<64-hex>` with `testnet → preprod`
  alias; serde end-to-end. 13 unit tests on the codec path.
- `Wallet::resolve_did(did_str)` chain: parse → indexer
  `contractAction(address)` GraphQL query → tagged_deserialize
  `ContractState` → walk the `StateValue` tree → port of
  `midnight-did-domain/LedgerToDomain` → fully populated
  `DidDocument`. Both Phase 2b (scalar fields) and Phase 2c (Map
  walks for VMs / services / 5 relations / alsoKnownAs) landed.
- Field paths (constants[2] + mutable[15]) were extracted from
  the upstream `index.js` accessors — see "Compact contract state
  encoding" above for the canonical layout reference.
- `js-bridge` cargo feature gates the WebView TS pipeline (default
  off after the architectural pivot — see "Pivot: Rust-native DID
  port" above).
- Dark-theme UI gained a `ResolveDidPanel` in the Advanced
  disclosure (input + Resolve button + JSON or error result).
- Comprehensive UX master at
  [`mobile-bench/UX_DESIGN.md`](mobile-bench/UX_DESIGN.md):
  sitemap (Wallet · Identity · Activity · Settings tabs), 13
  screens with layout sketches, cross-cutting interaction patterns
  (hold-to-confirm, status pill, action sheets, empty/loading/error
  recipes), component library extending MOBILE_WALLET.md.
  **Update this in the same commit as any user-visible change.**
- DID plan at [`mobile-bench/DID_PLAN.md`](mobile-bench/DID_PLAN.md)
  (4 phases: types/codec → resolve → create → all circuits) — Phase
  1 + 2 done.
- Local Midnight stack via
  [`mobile-bench/scripts/standalone-up.sh`](mobile-bench/scripts/standalone-up.sh)
  for Undeployed-network testing. Verified end-to-end: probe + chain
  tip + node status all green.
- 29 `wallet-core` unit tests pass.

Open / deferred:

- **Phase 3 — `Wallet::create_did` write path**: vendor contract
  artifacts (PKM + IR + Compact source) into
  `mobile-bench/wallet-core/contracts/midnight-did/`; derive
  controller signing key from BIP32 NightExternal role; build
  initial state input; prove via `prover-core::ProverCore`; submit
  via `subxt + midnight-node-metadata` (still need to add as git
  dep). **Gated on the wallet's unshielded sync** for fee
  balancing — option B (assume single available DUST UTXO and
  surface a clear error otherwise) is the agreed path so Phase 3
  can make visible progress before Phase B sync lands.
- **Phase 4 — remaining DID circuits** (addVerificationMethod,
  removeVerificationMethod, services, also-known-as, deactivate,
  relations) — mechanical once Phase 3's deploy flow works; each
  is the same input-encoding + prove + submit recipe.
