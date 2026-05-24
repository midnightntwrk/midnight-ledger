# `@midnight-ntwrk/react-native-prover`

React Native bindings for the Midnight ZK prover. Wraps the same
Rust core that powers the Dioxus mobile wallet (`mobile-bench/
contract-benchmark`) via UniFFI Turbo Modules, so an RN app can
generate halo2-KZG proofs entirely on-device.

## Status (2026-05-24 evening)

**Alpha — Rust side fully working, RN host integration gated
on ubrn CLI/npm version alignment.**

What's verified end-to-end on the workspace side:

- ✅ `cargo check -p midnight-prover-ffi` clean
- ✅ `cargo test -p midnight-prover-ffi --lib` passes
- ✅ `uniffi-bindgen-react-native build ios --sim-only`
  produces `ios/MidnightProver.xcframework` (70 MB)
- ✅ `uniffi-bindgen-react-native build android` produces
  `.so` for all 4 ABIs (12–17 MB each)
- ✅ `uniffi-bindgen-react-native generate jsi bindings`
  emits TS + C++ JSI module
- ✅ TypeScript public surface (`src/index.ts`) type-checks
- ✅ Generated package installable via `file:` in an RN host

What's gated on environment alignment:

- ⚠️ Building the demo in a real RN host app currently
  requires the **ubrn CLI binary and the
  `uniffi-bindgen-react-native` npm runtime to be at the
  same git commit**. The CLI installed from
  `cargo install --git https://github.com/jhugman/uniffi-bindgen-react-native uniffi-bindgen-react-native`
  pulls latest main (currently `44a1862`), which emits C++
  code referencing newer symbols (`arraybufferToUint8Array`,
  `kUbrnRustCapacity`, `string_to_buffer`) than the latest
  npm-published runtime (`0.31.0-2`) exposes. Build fails
  with `error: no member named ...` in the generated
  `cpp/generated/midnight_prover.cpp`.

  **Fix path**: either pin the CLI to a commit matching
  the npm version (`cargo install --git ... --rev <sha>`),
  or wait for a fresh ubrn release that aligns the two.

- ⚠️ The ubrn-generated `ios/ReactNativeProver.mm` is
  **new-arch only** (`#ifdef RCT_NEW_ARCH_ENABLED`). Old-
  architecture RN hosts will get
  `TurboModuleRegistry.getEnforcing(...): 'ReactNativeProver'
  could not be found`. Enable new arch in the host app via
  `RCT_NEW_ARCH_ENABLED=1` in `ios/.xcode.env.local` and
  `newArchEnabled=true` in `android/gradle.properties`.

- ⚠️ `react-native-screens` < 3.35 has C++ ABI compile
  errors with RN 0.74 new arch. The demo dropped the
  navigation deps entirely to avoid this.

What works today:

- ✅ `cargo test -p midnight-prover-ffi` — unit tests pass
- ✅ `cargo build --release -p midnight-prover-ffi` — clean
  build on host (and on any installed Rust target)
- ✅ TypeScript public surface (`src/index.ts`) type-checks
- ✅ The thin Native bridge falls back to a clear "native
  module not loaded" error when no JSI binding is registered,
  so Jest tooling doesn't crash on import

What's gated on `ubrn` (`cargo install uniffi-bindgen-react-native`):

- ⏳ Auto-generated Swift bindings (`ios/Sources/`)
- ⏳ Auto-generated Kotlin bindings (`android/src/main/java/`)
- ⏳ Auto-generated TS binding shims (`src/native/`)
- ⏳ Actually calling `prove()` from a real RN app

## Architecture

Per the [architecture doc §13](https://github.com/yshyn-iohk/midnight-ledger/blob/mobile-prototype/mobile-bench/midnight-mobile-architecture.md#13-react-native-packaging--feasibility--concrete-proposal),
this is **Option A — UniFFI same-process on BOTH platforms.**

| | Android | iOS |
|---|---|---|
| Process model | In-process (no `:proverProcess` Service) | In-process |
| Bindings | UniFFI → Kotlin | UniFFI → Swift |
| Memory model | Shared with host RN app | Shared with host RN app |
| Prover OOM | **Crashes the host RN app** | **Crashes the host RN app** |

**Trade-off acknowledged.** Option A is the simpler shape and
the user explicitly opted for it (vs the hybrid Option B with
Android process isolation). At k = 20 the prover's peak heap
is ~4.4 GiB; on Android phones with < 6 GiB RAM, OOM-killing
the host app is possible. For production, evaluate whether to
upgrade to Option B (separate-process isolation on Android)
based on the target device floor.

## Repo layout

```
react-native-prover/
├── crates/
│   └── prover-ffi/            # The UniFFI Rust crate
│       ├── Cargo.toml
│       ├── build.rs           # runs uniffi::generate_scaffolding
│       └── src/
│           ├── lib.rs         # public Rust API
│           └── midnight_prover.udl   # UniFFI interface definition
├── ios/                       # Podspec + (generated) Swift bindings
│   └── MidnightProver.xcframework  ← built by scripts/build-ios.sh
├── android/                   # Gradle module + (generated) Kotlin bindings
│   ├── build.gradle
│   └── src/main/jniLibs/arm64-v8a/libmidnight_prover_ffi.so
│                              ← built by scripts/build-android.sh
├── src/
│   ├── index.ts               # public TypeScript surface
│   └── NativeMidnightProver.ts# Turbo Module shim
├── scripts/
│   ├── build-rust.sh          # cross-build the .a / .so / .dylib
│   ├── build-ios.sh           # xcframework + Swift bindings via ubrn
│   └── build-android.sh       # .so + Kotlin bindings via ubrn
├── example/                   # see ../react-native-demo for a real RN app
├── package.json
├── tsconfig.json
├── MidnightProver.podspec     # consumed by host RN app's `pod install`
└── README.md (this file)
```

## Public API

```ts
import { prove, libraryVersion, type ProveOptions, type ProveResult } from "@midnight-ntwrk/react-native-prover";

console.log(libraryVersion());
// → "midnight-prover-ffi 0.1.0"

const result: ProveResult = await prove(14, {
  seed: 42n,
  verifyAfter: true,
  cacheKeys: true,
  // cacheDir omitted → uses platform default
});

console.log(`prove ${result.proveMs}ms, verify ${result.verifyMs}ms, size ${result.proofBytes}B`);
// → "prove 3530ms, verify 6ms, size 2933B"  (k=14 on iOS Sim, see Benchmarks)
```

The TS `prove()` returns a `Promise<ProveResult>`. Under the
hood it calls a synchronous Rust function; the platform layer
(Swift / Kotlin) wraps the call in a `Promise` / `Coroutine`
so the JS thread doesn't block during the multi-minute high-k
proves.

### Options

| Field | Type | Default | Meaning |
|---|---|---|---|
| `seed` | `number \| bigint` | `0` → use library default (0x42) | RNG seed for proving. Constant for reproducible runs. |
| `verifyAfter` | `boolean` | `true` | Skip the verifier check at the end of prove. The embedded verifier params only cover k ≤ 14; verify is auto-skipped at higher k regardless. |
| `cacheDir` | `string` | `""` → platform default | Where to read/write the BLS SRS files. Empty = use `$MIDNIGHT_PP` / `$XDG_CACHE_HOME` resolution. |
| `cacheKeys` | `boolean` | `true` | Re-use `(ProverKey, VerifierKey)` pairs across prove calls. Set `false` to time cold keygen explicitly. |

### Result

| Field | Type | Meaning |
|---|---|---|
| `k` | `number` | The requested `k`. |
| `realizedK` | `number` | The actual minimum-`k` halo2 reported (== `k` on success). |
| `hashChainLen` | `number` | Number of `transient_hash` ops in the generated chain. |
| `rows` | `bigint` | halo2-cost-model row count. |
| `keygenMs` | `bigint` | Keygen wall time in ms. |
| `proveMs` | `bigint` | Prove wall time in ms. |
| `verifyMs` | `bigint \| null` | Verify wall time. `null` when k > 14 or `verifyAfter=false`. |
| `verified` | `boolean \| null` | Whether the verify call succeeded. `null` if not attempted. |
| `proofBytes` | `bigint` | Serialised proof size in bytes (typically 2933). |

## Build

### One-time setup

```bash
# Rust targets for the platforms you want to ship
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
rustup target add aarch64-linux-android

# cargo-ndk for the Android cross-compile
cargo install cargo-ndk

# uniffi-bindgen-react-native for the JSI binding generation
cargo install uniffi-bindgen-react-native

# Set ANDROID_NDK_HOME (replace with your NDK install path)
export ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973
```

### Per-build

```bash
# All platforms in one go
yarn build:all

# Or individually:
yarn build:rust       # produce the .a / .so / .dylib slices
yarn build:ios        # wrap into MidnightProver.xcframework + Swift bindings
yarn build:android    # stage .so + generate Kotlin bindings
```

### Running tests

```bash
yarn test          # cargo test on the FFI crate
yarn typecheck     # tsc on the TS surface
```

## Roadmap

1. **Wire `ubrn` into CI** so the iOS / Android binding
   generation runs automatically.
2. **Validate `prove()` from a real RN host** — see
   `../react-native-demo/` for the test app.
3. **Add `cancel()` API** — currently the prove call blocks
   until done; a cooperative-cancel via `Arc<AtomicBool>` at
   each phase boundary is the standard pattern, ~50 LOC in
   the FFI crate + a `signal: AbortSignal` parameter in the
   TS API.
4. **Add progress callback** — the proofs crate already emits
   `tracing` events at every phase; thread a callback through
   the FFI and surface them as `(phase, etaSeconds)` updates
   to JS.
5. **Consider upgrading to Option B (hybrid)** if real-device
   testing shows the OOM-kill rate on Android < 6 GiB devices
   is unacceptable. See architecture doc §13.5 for the
   `:proverProcess` Service design.

## License

Apache-2.0 — matches the upstream `midnight-ledger` workspace.
