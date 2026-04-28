# Mobile Proof Bench — Iteration 1 Design

**Date:** 2026-04-28
**Branch:** `mobile-bench/iteration-1` (off `ledger-8`)
**Author:** Yurii Shynbuiev with Claude

## Goal

Run a real Midnight zero-knowledge proof from a single Rust codebase on three
surfaces — a macOS desktop app, an Android emulator, and a Samsung S24 Ultra —
and capture proof latency on each. The app is a Dioxus UI; the prover is a new
thin library crate that calls the same proving functions used by
`midnight-proof-server`.

This is a benchmark spike, not a product. There is no wallet, no chain
interaction, no transaction submission. The only persisted output of a run is
JSON copy-pasted into `mobile-bench/RESULTS.md`.

## Scope

In scope for this iteration:

- Two proofs:
  1. **Dust spend proof** (k=13, same circuit the existing wasm-proving demo
     uses).
  2. **A `zkir` example proof** sourced from
     `proof-server/tests/fallible.compact`.
- Two execution paths, both surfaced in the UI:
  1. **Library path** (recommended default): direct Rust calls into
     `transient-crypto::proofs` / `ledger::prove` / `zkir`.
  2. **HTTP path** (feature-flagged): in-process spawn of
     `midnight-proof-server::server()` and a localhost HTTP request.
- Three runtime targets: macOS desktop, Android emulator (arm64), real S24
  Ultra (arm64).
- KZG public parameters downloaded on first run via the existing
  `MidnightDataProvider` flow; cached in a platform-appropriate directory.
- Latency measurement, verify-after-prove correctness check, JSON export of
  results.

Explicitly **out of scope** for this iteration:

- iOS, Windows, Linux desktop targets.
- Run history, charts, or any persisted UI state beyond the most recent run.
- A params-management UI (clear cache, choose mirror, etc.).
- WASM / browser execution. Native Rust everywhere.
- Anything Docker-related.
- New circuits authored from scratch.
- Multi-threaded HTTP path on Android (HTTP path is desktop-only this
  iteration; see Risks).

## High-level architecture

```
                 ┌──────────────────────────────────────┐    ┌────────────────────────────┐
                 │         dioxus-bench (UI app)        │    │      cargo test/bench       │
                 │   buttons → runner → ProverCore      │    │                             │
                 └──────────────┬───────────────────┬───┘    └──────────────┬──────────────┘
                                │                   │ feature                │
                                │                   │ proof-server-http      │
                                ▼                   ▼                        ▼
                       ┌────────────────────────────────────────────────────────────┐
                       │                    prover-core                              │
                       │   prove_dust_spend / prove_zkir_example / prove_via_http    │
                       │   ParamsCache (desktop XDG dir / Android getFilesDir)       │
                       └────────────────┬─────────────────────────┬──────────────────┘
                                        │ (always)                │ (feature-gated)
                                        ▼                         ▼
                       ┌────────────────────────────────┐  ┌─────────────────────────┐
                       │ transient-crypto, ledger,      │  │ midnight-proof-server   │
                       │ zkir, zswap, midnight-{proofs, │  │ ::server() spawned on   │
                       │ curves, circuits, zk-stdlib}   │  │ 127.0.0.1:<random>       │
                       └────────────────────────────────┘  └─────────────────────────┘
```

Single source of proving truth: only `prover-core` calls into the existing
crypto crates. The HTTP path's server uses the same crates internally — they
are not two implementations of proving, just two callers.

## Components

### `mobile-bench/prover-core`

A `lib`-only crate. Workspace member, but not in `default-members`.

Public API (intentionally small):

```rust
pub struct ProverCore { /* opaque */ }

pub struct ProofRun {
    pub proof_bytes: Vec<u8>,
    pub elapsed: Duration,
    pub verify_elapsed: Option<Duration>,
    pub verified: Option<bool>,
    pub k: u8,
    pub label: &'static str,
}

pub struct BenchOpts {
    pub verify_after: bool,
    pub seed: Option<u64>,
}

impl ProverCore {
    pub async fn new(cache_dir: PathBuf) -> Result<Self>;

    pub async fn prove_dust_spend(&self, opts: BenchOpts) -> Result<ProofRun>;
    pub async fn prove_zkir_example(&self, opts: BenchOpts) -> Result<ProofRun>;

    pub async fn prove_via_http(&self, label: &str, base_url: &str)
        -> Result<ProofRun>;
}
```

Internals:

- `prove_dust_spend` builds a Dust spend `ProofPreimage` from canned witness
  data and calls `ProofPreimage::prove` with `DustResolver` + `ZswapResolver`,
  mirroring the path `/prove-tx` follows in `proof-server::endpoints`.
- `prove_zkir_example` extracts a small IR + proving key from
  `proof-server/tests/fallible.compact` at build time (build script writes the
  decoded chunks to `OUT_DIR`) and calls `zkir::prove_unchecked`.
- `prove_via_http` posts a serialized payload to a base URL using `reqwest`
  and times the round-trip including serialization. Used to compare against
  the library path; only meaningful when paired with a running `proof-server`.

`ParamsCache`:

- Wraps `MidnightDataProvider` configured `FetchMode::OnDemand` +
  `OutputMode::Log`.
- Cache directory passed in by the caller. Desktop callers use
  `dirs::data_dir().join("midnight-bench/params")`. Android callers use
  `Context.getFilesDir()/params/` resolved via JNI at app startup.
- Streams downloads to `<name>.partial`, atomically renames on success.
- All integrity checks delegate to `MidnightDataProvider`'s existing manifest
  logic (`ZSWAP_EXPECTED_FILES`, `DUST_EXPECTED_FILES`); no new hashing.

Crate features:

- default = `[]`
- `proof-server-http` — adds `midnight-proof-server` dependency and
  `pub fn spawn_local_server() -> (JoinHandle, String /* base_url */)`.
- `bench` — enables a `criterion` `[[bench]]` target.

Tests:

- `tests/library_path.rs` — `#[tokio::test]` runs both proofs end-to-end and
  asserts `verified == Some(true)`.
- `tests/http_path.rs` (gated on `proof-server-http`) — spawns the local
  server and runs both proofs through `prove_via_http`; asserts the proof
  bytes verify.
- `benches/proofs.rs` (gated on `bench`) — criterion benches for both proofs,
  library path only.

### `mobile-bench/dioxus-bench`

A `bin` crate using Dioxus 0.6. Workspace member, not in `default-members`.

Layout:

```
mobile-bench/dioxus-bench/
├── Cargo.toml
├── Dioxus.toml
├── src/
│   ├── main.rs
│   ├── app.rs
│   ├── runner.rs
│   └── platform/
│       ├── desktop.rs
│       └── android.rs
└── assets/styles.css
```

Single screen layout (only screen in this iteration):

- Radio: **Library** | **HTTP server** (HTTP disabled on Android — see
  Risks).
- Buttons: **Run Dust spend**, **Run zkir example**.
- Status line: idle / downloading-`<name>` / proving / verifying / done /
  error.
- Last-run panel: label, path, k, prove time, verify time, verified flag,
  proof size in bytes, platform/device, core count.
- **Copy result as JSON** button.

State (Dioxus signals):

- `path: Signal<ExecPath>` — Library | Http
- `status: Signal<RunStatus>` — Idle | Downloading{file,bytes,total} |
  Proving | Verifying | Done | Error(String)
- `last_run: Signal<Option<ProofRun>>`

Concurrency:

- Button press spawns an async task on the Dioxus runtime.
- The actual proving runs on `tokio::task::spawn_blocking` so the UI thread
  stays responsive.
- A `tokio::sync::watch` channel reports download / phase progress to the UI.

Platform glue:

- `platform/desktop.rs` resolves `cache_dir()` via the `dirs` crate.
- `platform/android.rs` resolves `cache_dir()` by calling
  `Context.getFilesDir()` through `jni` + `ndk-context` once at app startup.
- Bundle id: `io.iohk.midnight.bench`. App label: "Midnight Proof Bench".

Android manifest additions:

```xml
<uses-permission android:name="android.permission.INTERNET"/>
<application android:hardwareAccelerated="true"
             android:largeHeap="true">
```

## Public params: download and cache

Files needed before any proof runs:

- `bls_filecoin_2p13` — KZG params, k=13, ~256 MB. Downloaded on first run.
- `bls_midnight_2p14` — KZG verifier params, k=14. Already embedded in
  `transient-crypto` via `include_bytes!`; no download needed.
- Per-circuit prover/verifier keys: Dust `dust/<version>/spend.prover` and
  `spend.verifier`; zkir example keys derived from
  `proof-server/tests/fallible.compact`.

Download source: whichever URL `MidnightDataProvider` already uses (we do not
introduce new mirrors).

Cache locations:

| Platform     | Path                                           | Resolver |
|--------------|------------------------------------------------|----------|
| macOS        | `~/Library/Application Support/midnight-bench/params/` | `dirs::data_dir()` |
| Linux        | `$XDG_DATA_HOME/midnight-bench/params/`        | `dirs::data_dir()` |
| Android      | `<app private files dir>/params/`              | JNI to `Context.getFilesDir()` |

First-run UX: status line shows `Downloading <name> (12 / 256 MB)…`. Total
first-run download is dominated by `bls_filecoin_2p13`. Subsequent runs hit
the cache.

## Test plan

| Surface                       | Command                                                          | What it proves |
|-------------------------------|------------------------------------------------------------------|----------------|
| Rust integration test (host)  | `cargo test -p prover-core --test library_path`                  | Library API end-to-end on macOS arm64. CI-runnable. |
| Rust integration test (HTTP)  | `cargo test -p prover-core --features proof-server-http --test http_path` | HTTP path produces a verifying proof; equivalence with library. |
| Criterion bench (host)        | `cargo bench -p prover-core --features bench`                    | Stable per-platform baseline. |
| Dioxus desktop                | `dx serve --platform desktop` then click both buttons            | UI thread integration on macOS. |
| Android emulator              | `dx serve --platform android` (arm64 AVD running)                | Cross-compile + params download + proof completes on Android. |
| S24 Ultra                     | `dx bundle --platform android --release && adb install …`        | The latency number we care about. |

JSON shape recorded for each run (used for `RESULTS.md`):

```json
{
  "label": "dust-spend",
  "path": "library",
  "platform": "android-arm64",
  "device": "SM-S928 (S24 Ultra)",
  "rust_target": "aarch64-linux-android",
  "k": 13,
  "cores": 8,
  "elapsed_ms": 8421,
  "verify_ms": 18,
  "verified": true,
  "proof_bytes": 3412,
  "params_first_run": false,
  "build": "release",
  "git_sha": "…",
  "timestamp": "2026-04-28T…Z"
}
```

Latency methodology:

- Each button press runs the proof three times sequentially. Run 1 is
  warm-up (file reads, lazy init); reported separately. Reported latency =
  median of runs 2 and 3.
- HTTP path measurement includes serialization (that is the cost we want).
- All runs use the same seed across surfaces.
- Memory captured: `mem_peak_kb` from `/proc/self/status` on Android/Linux,
  `mach_task_basic_info` on macOS, plus `Debug.getNativeHeapAllocatedSize()`
  on Android via JNI.

## Acceptance criteria

The iteration is done when all of the following are true:

1. `cargo test -p prover-core` passes on macOS arm64 (both library and HTTP
   tests).
2. The Dioxus desktop app proves both circuits, shows correct verify, and
   "Copy result as JSON" produces valid JSON in the shape above.
3. The Android emulator (arm64-v8a, API 34) proves both circuits successfully
   from the same APK we'll install on the device.
4. The S24 Ultra proves both circuits successfully; the JSON results from at
   least one run on each circuit are pasted into
   `mobile-bench/RESULTS.md` together with the desktop and emulator numbers
   for comparison.

## Risks

| # | Risk                                                                                                  | Mitigation |
|---|-------------------------------------------------------------------------------------------------------|------------|
| 1 | A transitive dep of `transient-crypto` / `midnight-proofs` won't cross-compile to `aarch64-linux-android`. | Run `cargo ndk -t arm64-v8a build -p prover-core` on day one before any UI work. If it fails, identify the C/build-script offender and either flip a feature, vendor a pure-Rust replacement, or scope the broken proof out of Android for this iteration. |
| 2 | `midnight-proof-server` (actix-web + tokio multi-thread) won't link cleanly for Android.              | HTTP path is **desktop-only** this iteration: the `proof-server-http` feature is gated on `cfg(not(target_os = "android"))`. Android records library-only numbers. The desktop comparison still runs. |
| 3 | Android default heap kills the prover when KZG params load.                                           | `android:largeHeap="true"` already in manifest; if still OOM, we fall back to the embedded `2p14` verifier and skip the 256 MB params load (Dust spend then becomes unsupported on Android — accepted regression for this spike). |
| 4 | First-run download fails on cellular / metered network.                                               | Status panel surfaces the error; user re-taps. No auto-retry storms. We document "use Wi-Fi" in the readme. |
| 5 | `dx`'s Android pipeline is confused by our workspace layout.                                          | First fallback: add `[workspace.metadata.dioxus]` config. Second fallback: move `dioxus-bench` to its own `[workspace]` (pre-evaluated escape hatch). |
| 6 | S24 Ultra USB debugging not detected.                                                                 | Documented procedure in `mobile-bench/README.md`: enable Developer options → USB debugging → trust prompt → `adb devices`. |

## Sequencing

This is an outline; the detailed step-by-step plan is produced by the
`writing-plans` skill in the next phase.

1. Create branch `mobile-bench/iteration-1`. Add workspace skeleton
   (`mobile-bench/prover-core`, `mobile-bench/dioxus-bench`, `Cargo.toml`
   workspace member entries; **not** added to `default-members`).
2. Build `prover-core` library path. Get `cargo test -p prover-core --test
   library_path` green on macOS.
3. **Cross-compile gate.** Run `cargo ndk -t arm64-v8a build -p prover-core`.
   This is the highest-risk step; resolve before touching the UI.
4. Add `proof-server-http` feature and `tests/http_path.rs`. Get HTTP test
   green on macOS.
5. Scaffold Dioxus app, desktop target only. Wire buttons to library calls.
   Manual smoke test with `dx serve --platform desktop`.
6. Add Android target to the Dioxus app. Smoke test on arm64 emulator.
7. Side-load on S24 Ultra. Capture numbers. Fill in `mobile-bench/RESULTS.md`.

Each numbered step is a clean stopping point. If we get stuck at step 3, we
can still ship a Dioxus desktop app with the library/HTTP comparison and
report the Android cross-compile blocker.

## Conventions

- Workspace members added to `members`, kept out of `default-members`, so
  existing devs don't pay the Dioxus compile cost on a bare `cargo build`.
- All new crates use `edition = "2024"` to match the rest of the workspace.
- Branch: `mobile-bench/iteration-1`. PR target: `ledger-8`.
- Results captured to `mobile-bench/RESULTS.md` (manual paste from the app's
  Copy button). No automated history.
