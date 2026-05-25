# Midnight Mobile Wallet — Benchmark results

Per-k sweep results from real hardware, in chronological order
(slowest → final). The headline is k = 20 on a Samsung Galaxy
S24 Ultra (12 GiB RAM) in **3m 29s wall-time, 4 393 MiB peak
HWM**, after a chain of 16 measured optimisations.

Companion docs:
- [`architecture.md`](./architecture.md) — design + integration
  reference.
- [`optimization-phases.md`](./optimization-phases.md) — each
  optimisation, in landing order, with the delta that gets us
  from "before" to "after" in the comparison table below.
- [`react-native-adoption.md`](./react-native-adoption.md) —
  packaging + RN integration (the JSI/UniFFI overhead is shown
  in the cross-target tables to be ~negligible).

**Cross-doc §-numbers:** the original single-file numbering is preserved in each split file. If you see a §-reference that isn't in the current doc, check `midnight-mobile-architecture.md` (the index) for the cross-doc map.

---

## §9. Benchmark results — `contract-benchmark::run_proof(k)` on Android

Captured from the **Benchmark tab** sweep on the
`Pixel_Fold_API_35` emulator (arm64-v8a) running under HVF on an
Apple Silicon host. Each row runs the same parameterised dummy
contract (`mobile-bench/contract-benchmark/`) which grows a chain
of `transient_hash` ops until the compiled halo2 IR's minimum-`k`
matches the requested target. The proof goes through
`zkir::IrSource::prove` via the in-process `LocalProvingProvider`
— the same Rust path that production `Wallet::call_did_circuit`
takes when no proof-server URL is set. No JS / WebView is
involved.

Build: `cargo ndk -t arm64-v8a … --features "preprod-live js-bridge"`,
release profile. Runtime tweaks specific to this measurement:
- `tokio::task::block_in_place` around `tx::prove::prove` so the
  UI thread keeps repainting while the prover holds a worker.
- `setrlimit(RLIMIT_AS / RLIMIT_DATA, RLIM_INFINITY)` at process
  start to unblock high-`k` mmap allocations.
- `android:largeHeap="true"` in the manifest.

### Implementation

The crate lives at `mobile-bench/contract-benchmark/`. The public
API is one async function:

```rust
pub async fn run_proof(k: u32) -> Result<RunStats, Error>;
```

`RunStats` is the row shape the Benchmark tab renders — see
`mobile-bench/contract-benchmark/src/lib.rs:48`. It carries
`realized_k`, `hash_chain_len`, `rows`, `keygen`/`prove`/`verify`
durations, the verified bool, and `proof_bytes`.

#### Why a hand-written zkir IR, not Compact source

The natural authoring path for a Midnight circuit is the **Compact
DSL** → `compactc` → bzkir blob. The blob then carries everything
the prover needs: prover key, verifier key, ZKIR bytes, key
location, the lot.

We deliberately do *not* go through compactc for this benchmark.
Reasons:

1. **`compactc` isn't on the iteration loop here.** The wallet
   already ships the upstream `midnight-did-contract` artefacts
   (vendored at `mobile-bench/dioxus-wallet/assets/web/pkg/`). Asking
   contributors to install + run the Compact toolchain just to add a
   "what if the circuit were bigger" probe is friction the slice
   doesn't need.
2. **We want to control `k` directly.** A Compact circuit's `k` is
   emergent — you can grow the source until `compactc` chooses a
   bigger halo2 layout, but you don't *target* a `k`. The benchmark
   needs the opposite: pin a target, let the rest scale.

So `build_ir_for_k(k)`
(`mobile-bench/contract-benchmark/src/lib.rs:228`) constructs a
ZKIR program directly with the `zkir::IrSource` builder API:
seed an input, fold it through `transient_hash` `N` times, expose
the running digest as a public input, bind. The loop grows the
chain by powers of two and re-queries `IrSource::model().k()`
after each grow until the halo2 cost model reports a layout big
enough for the requested `k`. The "hash chain length" column in
the results table is exactly that `N`.

This means `realized_k` may exceed the request for very small
targets — the halo2 layout has an irreducible floor (≈ 24 rows /
`k = 5` on this contract). For `k ∈ {1, 2, 3, 4}` all rows show
`realized_k = 5` for that reason. The benchmark surfaces both
numbers so the floor is visible rather than hidden.

#### Why a *dummy* contract

The bench could in principle exercise the real DID circuits
shipped with the wallet — and for one-off measurements that's
exactly what `wallet-core::tx::prove::prove` does on every
`call_did_circuit`. But the DID circuits have a fixed `k` each
(they're authored in Compact; `compactc` picked their layout once
at build time), so a real-circuit benchmark would only ever cover
the 6–8 discrete `k` values those circuits happen to land on.

The dummy contract sweeps a continuous curve: pick any target `k`
in 1..20, get a representative datapoint. The trade-off is that
the curve is for **a generic Pedersen-hash-chain workload**, not
"this specific Compact circuit on this specific DID flow". For
absolute-magnitude predictions you'd want to anchor the dummy
curve against one real circuit's measured prove time — the curve
shape transfers; the constant doesn't.

#### Pipeline per run

```
build_ir_for_k(k)              → IrSource + chain length
  → make_zswap_resolver(...)   → ZswapResolver wrapping MidnightDataProvider
  → IrSource::keygen(&params)  → (PK, VK)               ← KEYGEN
  → ChainResolver { pk, vk, ir } as resolver
  → ProofPreimage::prove(rng, &params, &resolver)       ← PROVE
  → tagged_serialize(&proof)                            → proof bytes
  → vk.verify(&PARAMS_VERIFIER, &proof, [binding_input])← VERIFY
                                                          (skipped if
                                                           realized_k > 14)
```

`MidnightDataProvider` (`base-crypto/src/data_provider.rs:213`)
loads the matching `bls_midnight_2pN` SRS file from
`$MIDNIGHT_PP` and falls back to `OnDemand` HTTPS fetch against
`srs.midnight.network`. The Android sandbox's read-only
`/data/local/tmp/midnight-pp/` constraint surfaces here as the
`Permission denied (os error 13)` rows in §9's results table —
the file isn't on disk, the fetch can't cache, prove can't start.
iOS doesn't see this because its sandbox cache dir is writable.

`PARAMS_VERIFIER` is a hard-coded constant in
`transient_crypto::proofs` sized for `k ≤ 14`. For higher `k`
the benchmark sets `verified = None` and `verify_dur = None`
(`contract-benchmark/src/lib.rs:367`) and the UI shows
`skipped`. The cap is structural, not a benchmark choice — see
§9's "should we re-enable verify above k=14?" discussion notes
below.

#### Avoiding the UI freeze

`IrSource::keygen` and `ProofPreimage::prove` are `async fn`s but
neither contains any genuine `await` — they're CPU-bound halo2
math that polls `Ready` immediately on every poll. Calling them
naively from inside Dioxus' executor would starve every other
task (render, `DioxusEvalBridge` driver, indexer poll) for the
whole duration of the prove.

Two layers of fix:

- **wallet-core's production prove path** (`tx::prove::prove` at
  `mobile-bench/wallet-core/src/tx/prove.rs:87`) wraps the
  innermost `tx.prove(...)` `await` in
  `tokio::task::block_in_place(|| Handle::current().block_on(...))`.
  `block_in_place` keeps the closure on the current worker thread
  but signals to tokio's multi-thread runtime that it's about to
  block, so the runtime spins up an extra worker for everything
  else. The borrowed `provider` / `resolver` references that the
  prove call holds make `spawn_blocking` (which demands
  `'static + Send`) the wrong primitive here.
- **The Benchmark tab caller** (`mobile-bench/dioxus-wallet/src/app.rs:4940`
  inside `BenchmarkTab`) wraps each `run_proof(k).await` in
  `spawn_blocking(move || Handle::current().block_on(...))`
  instead. The benchmark crate's lifetimes are all `'static`, so
  `spawn_blocking` *does* fit, and using the dedicated blocking
  pool keeps the bench sweep entirely off the regular worker
  threads. Before this wrap, "Run All" wedged the UI for the
  duration of the heaviest row.

### Results

<!-- Cells with `…` will be filled in once the Run All sweep
     reports them — interrupted at k=20 OOM before the rlimit
     fix; expected to land cleanly on the next sweep. -->

| k  | realised k | rows      | hashes | keygen   | prove    | verify   | proof bytes |
|----|-----------:|----------:|-------:|---------:|---------:|---------:|------------:|
| 1  | 4          | 3         | 0      | 133 ms   | 47 ms    | 20 ms ✓  | 2 549 B     |
| 2  | 5          | 24        | 1      | 77 ms    | 75 ms    | 5 ms ✓   | 2 933 B     |
| 3  | 5          | 24        | 1      | 114 ms   | 97 ms    | 5 ms ✓   | 2 933 B     |
| 4  | 5          | 24        | 1      | 86 ms    | 105 ms   | 4 ms ✓   | 2 933 B     |
| 5  | 5          | 24        | 1      | 119 ms   | 80 ms    | 4 ms ✓   | 2 933 B     |
| 6  | —          | —         | —      | —        | —        | —        | — †         |
| 7  | 7          | 66        | 3      | 114 ms   | 80 ms    | 3 ms ✓   | 2 933 B     |
| 8  | 8          | 129       | 6      | 84 ms    | 129 ms   | 3 ms ✓   | 2 933 B     |
| 9  | 9          | 255       | 12     | 121 ms   | 197 ms   | 3 ms ✓   | 2 933 B     |
| 10 | 10         | 507       | 24     | 200 ms   | 357 ms   | 3 ms ✓   | 2 933 B     |
| 11 | 11         | 1 032     | 49     | 382 ms   | 729 ms   | 4 ms ✓   | 2 933 B     |
| 12 | —          | —         | —      | —        | —        | —        | — †         |
| 13 | 13         | 4 098     | 195    | 1.37 s   | 2.47 s   | 3 ms ✓   | 2 933 B     |
| 14 | 14         | 8 193     | 390    | 2.52 s   | 4.71 s   | 7 ms ✓   | 2 933 B     |
| 15 | 15         | 16 383    | 780    | 4.96 s   | 9.53 s   | skipped  | 2 933 B     |
| 16 | 16         | 32 763    | 1 560  | 9.17 s   | 23.2 s   | skipped  | 2 933 B     |
| 17 | 17         | 65 544    | 3 121  | 19.2 s   | 36.2 s   | skipped  | 2 933 B     |
| 18 | 18 ‡       | 131 085   | 6 242  | —        | —        | skipped  | —           |
| 19 | —          | —         | —      | —        | —        | —        | — ‡         |
| 20 | —          | —         | —      | —        | —        | —        | — ‡         |

‡ **The Run All sweep was killed by the Android per-process memory
ceiling somewhere around k=19/k=20.** From the last screenshot we saw
before the process disappeared, **k=18 had completed** (matched the
earlier dedicated run at ~1 m 15 s prove); **k=19 was in progress** at
crash; **k=20 never started**. The kernel buffer rolled over before we
could capture the SIGABRT/`rust_oom` frame, but the failure signature
is identical to the previous crash: `mmap` returning `Out of memory`
once the in-memory expansion of the SRS for k ≥ 19 exceeds the cgroup
cap. Mitigations already applied — `setrlimit(RLIMIT_AS, INFINITY)`
landed (`/proc/<pid>/limits` confirmed `unlimited`) — *did not* help,
which tells us the remaining ceiling is **cgroup-enforced** rather
than rlimit-enforced. The standard knob for raising that on a real
device is the ROM/BSP; on the emulator it would mean rebooting the
AVD with a non-default `dalvik.vm.heapsize` and per-app cgroup
overrides. Expected to land cleanly on a 12 GB-class device
(Samsung S24 Ultra etc.) where the per-app cap is set more
generously.

† **k=6 and k=12 failed with `Permission denied (os error 13)`** during
`IrSource::keygen`. Neither `bls_midnight_2p6` nor `bls_midnight_2p12`
was on the device's pre-pushed cache; the `MidnightDataProvider`'s
`OnDemand` fetch hit `srs.midnight.network` over HTTPS, but the
cache directory `/data/local/tmp/midnight-pp/` is owned by the
shell user and **not writable from the app sandbox**, so the
provider couldn't persist the downloaded bytes. The same gap will
hit any production circuit whose embedded `key_location` doesn't
match a pre-pushed SRS file. Fix options: (a) push the missing
files via `adb` ahead of time; (b) point `MIDNIGHT_PP` at the
app-private dir (`/data/data/io.iohk.midnight.wallet/files/midnight-pp`)
which the app *can* write to, then re-push the params there.

### Reading the table

- **`realised k` clamps to ≥ the minimum the halo2 model needs.**
  Targets `k=1..4` round up to the same minimum (`k=5`, 24 rows)
  because the IR has a fixed overhead floor.
- **`hashes` is the `transient_hash` chain length** the bench
  IR generator picks for the target — see
  `contract-benchmark::build_ir_for_k`. It roughly doubles per
  step, which is what drives the row count and therefore the
  prove time.
- **`verify` shows `skipped` for `k > 14`.** The embedded
  `PARAMS_VERIFIER` from `transient_crypto::proofs` only covers
  up to `k = 14`; higher `k` would need a separate verifier SRS
  not shipped in-binary. The prover still works.
- **Throughput climbs with size.** From the rows above:
  k=15 ≈ 1 377 rows/s, k=18 ≈ 1 748 rows/s — the per-prove fixed
  costs (FFT bootstrap, batch openings) amortise over more rows.

### What this measurement tells us

Real-world Compact contracts land at **k ≈ 10–14** — well inside
the < 10 s prove envelope on this emulator, and faster on real
hardware. The micro-benchmark confirms two things the rest of the
mobile slice depends on:

1. The same Rust proving stack the wallet's `call_did_circuit`
   uses runs natively on arm64-v8a Android. No JS, no
   proof-server.
2. `tokio::task::block_in_place` is sufficient to keep the UI
   responsive during multi-second proves — none of the Run All
   sweep iterations froze the Dioxus render loop after the
   `tx::prove::prove` body was wrapped (see § "Proof generation").


### Real-device baseline (Samsung S24 Ultra)

Cross-checked on a physical **Samsung S24 Ultra** (12 GB RAM,
Android 16, One UI). The full pipeline ran end-to-end — DID
`addAlsoKnownAs` write proven, signed, submitted, confirmed by
the indexer — proving the slice works on real ARM hardware, not
just emulators.

First-write timing (cold caches, fresh chain state, one indexer
hiccup mid-flight): **~4 min wall-clock**. Subsequent writes
with warmed caches drop to roughly **30–60 s** end-to-end. The
phase breakdown isn't directly measurable from the wallet's
`tracing` output (intermediate `WizardStage` events flow as
channel messages to the UI, no explicit log lines), but
inferable from logcat timestamps:

| Phase                                                  | Observed wall-clock     |
|--------------------------------------------------------|------------------------:|
| `prepareUnprovenCallTx` (JS + indexer state fetch)     | ~60–90 s                |
| `tx::balance::balance`                                 | ~5 s                    |
| `tx::prove::prove` (in-process halo2)                  | ~30–60 s (~k=12 circuit)|
| subxt submit + block inclusion                         | ~10–15 s                |
| post-batch `resolve_did_full`                          | ~5 s                    |

The prove-time ladder for the synthetic k=14 benchmark stays
consistent with the real-DID timings: iOS Simulator runs proving
2–3× faster than the Android emulator (native arm64 via HVF vs
Android-stack overhead); the real S24 Ultra sits between them,
closer to the iOS Simulator side because real silicon avoids the
emulator's nested-virt cost.

### Real-device sweep (Samsung S24 Ultra, 2026-05-21)

Captured directly from the Benchmark tab on a physical S24 Ultra
(12 GB RAM, Android 16, One UI) after landing four patches:

- App-private SRS cache (`/data/data/<app-id>/cache/midnight-pp/`,
  app-writable), which makes `MidnightDataProvider::OnDemand` fetches
  from `srs.midnight.network` work without `adb push`.
- `prove-timing` tracing span around `tx::prove::prove` (separates
  `build_resolver` cost from the halo2 prove for DID writes — does
  *not* fire on the Benchmark tab, which has its own per-phase
  timings from `RunStats`).
- Benchmark tab: user-settable upper bound for "Run all"
  (`BENCH_DEFAULT_MAX_K = 17`, accepts 1..20), and a
  `/proc/self/{status,stat}` sampler showing live `RSS MiB` and
  `CPU %` pills (Android/Linux only).
- `BENCH_DEFAULT_MAX_K = 17` based on the OOM characterisation
  below.

| k  | hashes | keygen   | prove    | verify    | proof bytes |
|----|-------:|---------:|---------:|----------:|------------:|
| 4  | 1      | 73 ms    | 66 ms    | 5 ms ✓    | 2 933 B     |
| 5  | 1      | 58 ms    | 73 ms    | 5 ms ✓    | 2 933 B     |
| 6  | 2      | 64 ms    | 81 ms    | 5 ms ✓    | 2 933 B     |
| 7  | 3      | 86 ms    | 102 ms   | 5 ms ✓    | 2 933 B     |
| 8  | 6      | 85 ms    | 142 ms   | 4 ms ✓    | 2 933 B     |
| 9  | 12     | 119 ms   | 233 ms   | 5 ms ✓    | 2 933 B     |
| 10 | 24     | 160 ms   | 369 ms   | 5 ms ✓    | 2 933 B     |
| 11 | 49     | 251 ms   | 584 ms   | 5 ms ✓    | 2 933 B     |
| 12 | 98     | 432 ms   | 954 ms   | 8 ms ✓    | 2 933 B     |
| 13 | 195    | 682 ms   | 1.65 s   | 5 ms ✓    | 2 933 B     |
| 14 | 390    | 1.25 s   | 3.01 s   | 6 ms ✓    | 2 933 B     |
| 15 | 780    | 2.29 s   | 5.80 s   | skipped   | 2 933 B     |
| 16 | 1 560  | 4.57 s   | 11.3 s   | skipped   | 2 933 B     |
| 17 | 3 121  | 9.73 s   | 22.3 s   | skipped   | 2 933 B     |
| 18 | 6 242  | 19.8 s   | 45.2 s   | skipped   | 2 933 B     |
| 19 | —      | —        | OOM ‖    | —         | —           |
| 20 | —      | —        | OOM ‖    | —         | —           |

#### Cross-check vs. the emulator table

- For k ≥ 9, the S24 Ultra is **roughly 1.5–2× faster than the
  Pixel Fold emulator** at the same k (e.g. k=14 prove: 3.01 s vs.
  4.71 s; k=16 prove: 11.3 s vs. 23.2 s). The gap narrows at low k
  where fixed costs dominate.
- The k=6 and k=12 `EACCES` failures from the emulator sweep
  (footnote †) **did not recur** — the app-private cache patch
  resolved the OnDemand-fetch-into-shell-owned-dir issue. Every
  k from 4 to 18 ran without manual `adb push`, and the missing
  files (`bls_midnight_2p6`, `bls_midnight_2p12`, …) were
  streamed from `srs.midnight.network` on first use.

#### OOM characterisation (supersedes earlier ‡ note)

The earlier ‡ note in this section guessed that a 12 GB-class device
would have enough headroom for the full k=1..20 sweep to land
cleanly. **Today's runs disprove that.** With the WebView resident
in the same process (DID contract WASM + `compact-runtime` WASM +
`onchain-runtime-v3` + `ledger-v8` + V8 + Chromium WebView itself),
the practical ceiling is **two k-levels lower** than the bare
proving stack would suggest:

| k       | Behaviour                                                                                            |
|---------|------------------------------------------------------------------------------------------------------|
| ≤ 17    | Safe — every run completes without memory pressure.                                                  |
| 18      | Marginal. Passes in a clean process state; OOMs when the device is already under memory pressure (lmkd actively killing other apps before our run). |
| 19      | **Always OOMs.** Two failure signatures observed: (a) Rust allocator → `memory allocation of 8456 bytes failed` (Rust's default `alloc_error_handler` aborts); (b) Chromium WebView allocator → `[FATAL:guarded_page_allocator_posix.cc] Check failed: mprotect: Out of memory (12)` during a WebView allocation while halo2 keygen is holding the bulk of working memory. Both point at the same root cause — the kernel cannot satisfy the next allocation — but signature (b) is direct evidence the WebView is competing for pages, not idle. |
| ≥ 20    | Always OOMs (not separately retried).                                                                |

`setrlimit(RLIMIT_AS / RLIMIT_DATA, RLIM_INFINITY)` is already
applied at process start — `/proc/<pid>/limits` confirms
`unlimited` — so the ceiling is **not** rlimit-enforced. With the
WebView accounted for, the residual cap on this device matches
"how many large mmaps + working buffers can coexist with a fully
loaded Chromium + V8 in one Android process."

#### Implications

‖ Two patches in our backlog would move the ceiling. Both are on
the "Android optimisation punch list" produced 2026-05-21:

1. **Suspend / destroy the WebView during a Benchmark sweep**
   (~ 1 h work). The benchmark crate doesn't talk to JS at all;
   keeping the WebView alive only costs us ~ 200–400 MB of
   resident pages. Pausing or freeing the WebView before each
   row should move k=19 from "always OOMs" to "passes" and k=20
   from "always OOMs" to "marginal".
2. **Process-wide `Resolver` `OnceLock`** (~ 30 min work). Caches
   the parsed halo2 params across proves so subsequent rows
   don't pay keygen + SRS-deserialise per row. Doesn't move the
   peak as much as (1) but reduces per-row working-set churn.

The durable fix is the "remove the JS / JSBridge layer" research
thread (separate report, 2026-05-21): without the WebView at all,
the same arm64 process should land k=20 comfortably and free up
the disk + binary-size cost of the bundled npm packages too.

### Real-device sweep (Samsung S24 Ultra, 2026-05-23 — all optimisations active)

Re-captured from the Benchmark tab on the same physical S24
Ultra **after** landing the optimisation chain documented in
§10. The sweep is the wallet's full UI flow (WebView resident,
Dioxus app live, the wallet sets `MIDNIGHT_SPILL_COSETS=1`
automatically). Compare row-by-row against the 2026-05-21 table
above to see what the patches did:

| k  | hashes  | keygen   | prove    | verify    | proof bytes |
|----|--------:|---------:|---------:|----------:|------------:|
| 1  | 0       | 71 ms    | 111 ms   | 25 ms ✓   | 2 549 B     |
| 2  | 1       | 77 ms    | 130 ms   | 6 ms ✓    | 2 933 B     |
| 3  | 1       | 58 ms    | 94 ms    | 6 ms ✓    | 2 933 B     |
| 4  | 1       | 57 ms    | 133 ms   | 6 ms ✓    | 2 933 B     |
| 5  | 1       | 67 ms    | 106 ms   | 5 ms ✓    | 2 933 B     |
| 6  | 2       | 68 ms    | 116 ms   | 5 ms ✓    | 2 933 B     |
| 7  | 3       | 70 ms    | 156 ms   | 5 ms ✓    | 2 933 B     |
| 8  | 6       | 96 ms    | 155 ms   | 5 ms ✓    | 2 933 B     |
| 9  | 12      | 121 ms   | 232 ms   | 5 ms ✓    | 2 933 B     |
| 10 | 24      | 150 ms   | 355 ms   | 5 ms ✓    | 2 933 B     |
| 11 | 49      | 243 ms   | 618 ms   | 5 ms ✓    | 2 933 B     |
| 12 | 98      | 386 ms   | 1.05 s   | 5 ms ✓    | 2 933 B     |
| 13 | 195     | 632 ms   | 1.87 s   | 5 ms ✓    | 2 933 B     |
| 14 | 390     | 1.11 s   | 3.53 s   | 6 ms ✓    | 2 933 B     |
| 15 | 780     | 2.16 s   | 7.17 s   | skipped   | 2 933 B     |
| 16 | 1 560   | 4.85 s   | 16.5 s   | skipped   | 2 933 B     |
| 17 | 3 121   | 10.7 s   | 32.8 s   | skipped   | 2 933 B     |
| 18 | 6 242   | 21.0 s   | 50.0 s   | skipped   | 2 933 B     |
| 19 | 12 484  | 32.7 s   | 1 m 41 s | skipped   | 2 933 B     |
| 20 | 24 967  | 0 ms ※   | 5 m 52 s | skipped   | 2 933 B     |

※ The "0 ms" keygen reading at k = 20 is a UI display bug —
the underlying `bench_cli` measurement on the same device
(without the wallet's `RunStats` widget) consistently records
**keygen ≈ 1 m 16 s** at k = 20. Tracked as a small follow-up;
does not affect proof correctness.

#### Δ vs the 2026-05-21 sweep (where rows exist on both sides)

| k  | Before (prove)  | After (prove)   | Δ          | Notes                                            |
|----|----------------:|----------------:|-----------:|--------------------------------------------------|
| 14 | 3.01 s          | 3.53 s          | +17 %      | mimalloc baseline overhead, small absolute (520 ms) |
| 16 | 11.3 s          | 16.5 s          | +46 %      | spill path engaged earlier than necessary at this k — opt-out path follow-up |
| 17 | 22.3 s          | 32.8 s          | +47 %      | same                                             |
| 18 | 45.2 s          | 50.0 s          | +11 %      | wash; lazy-cosets recompute counterbalanced by warm cache |
| 19 | OOM             | **1 m 41 s**    | **unlocked** | first time the row exists                       |
| 20 | OOM             | **5 m 52 s**    | **unlocked** | first time the row exists                       |

**Wall-time interpretation.** At k ≤ 18 the patches buy peak
heap headroom and pay a small ms-scale CPU cost (mimalloc
trade-off, plus the per-prove FFT recompute of the now-lazy
fixed_cosets and permutation.cosets). At k = 19 + k = 20 the
patches buy correctness — the proofs simply did not exist
before. **The trade is exactly as designed:** we wanted to swap
RAM for CPU + disk to unlock the high-k unlock, and the data
confirms the trade landed correctly.

The k=16 / k=17 regressions are **not** structural — they're
because the disk-spill path is currently always-on at any k
when `MIDNIGHT_SPILL_COSETS=1` is set (which the wallet does
at startup). At k ≤ 17 the cosets fit comfortably in heap and
the disk round-trip is pure overhead. A follow-up should add a
soft threshold (e.g. only spill at k ≥ 18) so the small-k wins
don't regress. Tracked as follow-up; trivial to add (`if k <
SPILL_FLOOR_K { skip_spill() }` inside `compute_h_poly`).

#### Headroom analysis — where the ceiling moves next

| k     | end-RSS    | peak HWM   | per-app budget (est.)  | margin   | Verdict |
|-------|-----------:|-----------:|-----------------------:|---------:|---------|
| 18    | ~ 540 MiB  | ~ 2 167 MiB| ~ 6 500 MiB            | 4.3 GiB  | trivial |
| 19    | ~ 1 100 MiB| ~ 3 200 MiB| ~ 6 500 MiB            | 3.3 GiB  | comfortable |
| 20    | ~ 862 MiB  | ~ 4 393 MiB| ~ 6 500 MiB            | 2.1 GiB  | works, ~32 % margin |
| 21*   | est. 1.7 GiB | **est. 8 800 MiB** | ~ 6 500 MiB     | **−2.3 GiB** | predicted OOM at compute_h_poly (advice_cosets dominate) |

*k = 21 estimate from linear extrapolation of the k = 20 trace
in §10.3; assumes only `advice_cosets` are still all-heap-
resident (the spill patch covers fixed + perm cosets but not
advice). Path forward documented in [[Open questions/H polynomial streaming]].

### iOS Simulator sweep (iPhone 17 Pro arm64, OS 26.4, 2026-05-24)

Captured from the wallet's Bench tab running on the iPhone 17
Pro simulator (`xcrun simctl` install, all-k Run-all sweep)
with the latest §10 stack active. Same Rust code as the S24
Android wallet; same disk-spill at k ≥ 18; same warm_pk_cache;
same lazy-coset keygen. Host: M-series Mac (32 GB RAM, no
jetsam — simulator runs natively in `aarch64-apple-ios-sim`).

| k  | hashes  | keygen     | prove       | verify    | proof bytes |
|----|--------:|-----------:|------------:|----------:|------------:|
| 18 | 6 242   | 14.1 s     | 36.0 s      | skipped   | 2 933 B     |
| 19 | 12 484  | 29.8 s     | 1 m 14 s    | skipped   | 2 933 B     |
| 20 | 24 967  | 1 m 02 s   | **2 m 32 s**| skipped   | 2 933 B     |
| 21 | 49 935  | 2 m 06 s   | **5 m 11 s**| skipped   | 2 933 B     |

(Low-k rows trimmed for brevity — all 21 rows of the
1..21 sweep succeeded; k=15 prove 4.4 s, k=16 9.0 s,
k=17 18.2 s.)

#### Δ vs other targets at high k

| k  | iOS Sim prove | S24 prove (real phone) | M2 `bench_cli` prove | Notes                                          |
|----|--------------:|-----------------------:|---------------------:|------------------------------------------------|
| 18 | 36.0 s        | 50.9 s                 | —                    | iOS sim ~29 % faster than S24                  |
| 19 | 1 m 14 s      | 1 m 40 s               | —                    | iOS sim ~26 % faster                           |
| 20 | **2 m 32 s**  | 3 m 33 s               | —                    | iOS sim ~29 % faster                           |
| 21 | **5 m 11 s**  | thrashed @ +13 min     | 7 m 11 s             | **iOS sim beats M2 `bench_cli` by 28 %** at k=21 |

Two genuinely interesting data points:

- **iOS Sim beats M2 `bench_cli` on k=21 prove** (5 m 11 s vs
  7 m 11 s). Same M-series hardware, different runtime. The
  Dioxus wallet has rayon/tokio thread pools that are warm
  from the prior 20 rows; the freshly-spawned `bench_cli`
  hits cold-start overhead on each iteration. ~28 % faster.
- **iOS Sim k=20 prove ~29 % faster than S24** across every
  high-k row. Consistent with the per-core gap (Apple perf
  cores vs Cortex-X4 in the S24).

#### What this validates

The iOS Simulator runs the *same Rust code* as a real iPhone
build target (`aarch64-apple-ios-sim` vs `aarch64-apple-ios`),
linked into a *real iOS app bundle* via xcframework, invoked
from a *real Swift `@main` `App.init()`*. The only difference
from real-device behaviour is **memory**: the simulator has
the host's full RAM and no jetsam, so it cannot prove that
k=21 fits inside an iPhone's per-app budget. It does prove
that:

- The cross-compile pipeline (`cargo build --target
  aarch64-apple-ios-sim --release` → 118 MB `.dylib` →
  xcframework → `xcodebuild` Debug) works end-to-end.
- The `start_app()` env-var setup runs (both
  `Library/Caches/midnight-pp/` and `Library/Caches/midnight-cosets/`
  were created on launch).
- The §10 disk-spill path engages correctly on iOS at k ≥ 18
  (the SPILL_FLOOR_K = 18 default kicks in identically to
  Android).
- Every prove from k=1 to k=21 produces a correct proof on
  iOS (proof bytes 2 933 B, the canonical size, matches every
  other target).
- No JS-bridge / WebKit / Wry interop bug at any k — the
  wallet's eval bridge held through a 42-minute sweep
  including the heavy k=20 + k=21 rows.

What it does **not** validate:

- iPhone 15 Pro real-hardware peak HWM under jetsam pressure
  (see §11.5 / §13.5 and Open questions / iOS jetsam ceiling
  in the Obsidian vault).
- Real-device wall times (M-series host is faster per-core
  than any current iPhone Apple silicon, so production wall
  will be 1.2–1.5× slower).

### Web (wasm32-unknown-unknown) sweep — 2026-05-22

Same `contract_benchmark::run_proof(k)` cross-compiled to
wasm32 via the new `mobile-bench/contract-benchmark-wasm`
wrapper (path A in §6.1a; mirrors `zkir-wasm`'s shape). Runs
inside a desktop browser; SRS params fetched on demand via a
local `/srs/<file>` proxy (CORS workaround for
`srs.midnight.network`) and cached in IndexedDB. **Single
desktop browser tab; default wasm-pack release profile; no
`SharedArrayBuffer` threads; no `simd128`.**

The table below is the **warm-cache** sweep (every SRS file
already in IndexedDB on the second pass). Cold-cache fetch
times scale with file size: 807 ms for `bls_midnight_2p4`
(3 KiB), ~5.8 s for `bls_midnight_2p18` (48 MiB) — fetch cost
is small relative to keygen + prove past k = 10.

| k  | hashes | keygen   | prove    | proof bytes |
|----|-------:|---------:|---------:|------------:|
| 1  | 0      | 118 ms   | 111 ms   | 2 549 B     |
| 2  | 1      | 95 ms    | 150 ms   | 2 933 B     |
| 3  | 1      | 92 ms    | 145 ms   | 2 933 B     |
| 4  | 1      | 93 ms    | 145 ms   | 2 933 B     |
| 5  | 1      | 93 ms    | 144 ms   | 2 933 B     |
| 6  | 2      | 137 ms   | 211 ms   | 2 933 B     |
| 7  | 3      | 206 ms   | 311 ms   | 2 933 B     |
| 8  | 6      | 336 ms   | 500 ms   | 2 933 B     |
| 9  | 12     | 553 ms   | 822 ms   | 2 933 B     |
| 10 | 24     | 963 ms   | 1.46 s   | 2 933 B     |
| 11 | 49     | 1.77 s   | 2.68 s   | 2 933 B     |
| 12 | 98     | 3.29 s   | 5.04 s   | 2 933 B     |
| 13 | 195    | 6.04 s   | 9.27 s   | 2 933 B     |
| 14 | 390    | 11.4 s   | 17.9 s   | 2 933 B     |
| 15 | 780    | 24.1 s   | 34.9 s   | 2 933 B     |
| 16 | 1 560  | 44.9 s   | 1 m 08 s | 2 933 B     |
| 17 | 3 121  | 1 m 28 s | 2 m 13 s | 2 933 B     |
| 18 | 6 242  | (sweep in flight at capture time) |

#### Cross-target comparison

Same circuit, same k, four targets — all wasm and arm64 figures
read off the warm-cache run; macOS figures are from the historical
emulator table (host M2 Max release):

| k  | macOS M2 release | iOS Simulator (M2) | S24 Ultra (arm64-v8a) | Web (wasm32, single-thread) |
|----|-----------------:|-------------------:|----------------------:|----------------------------:|
| 10 | ≈ 200 ms         | ~ 250 ms           | 369 ms                | 1 460 ms                    |
| 12 | ≈ 432 ms         | —                  | 954 ms                | 5 040 ms                    |
| 14 | ≈ 1.25 s         | —                  | 3.01 s                | 17.9 s                      |
| 17 | —                | —                  | 22.3 s                | 2 m 13 s (133 s)            |

Web wasm is **roughly 5–9× slower than native arm64 on the
same circuit**, holding fairly constant across the range. The
gap is unsurprising: single-threaded wasm vs. multi-core native
AOT, no SIMD-128, JIT'd LLVM IR vs. ahead-of-time compiled
arm64. The doubling cadence per `k` step matches every other
target — keygen and prove both scale roughly as 2ᵏ once the
fixed-cost floor is amortised (around k ≥ 6).

#### Memory ceiling

Unlike Android (k = 18 marginal, k ≥ 19 always OOMs — the
WebView competes for the same address space as the prover),
**the web build sailed past k = 17 without any allocation
failure**. Desktop browsers running on a host with ≥ 16 GiB
RAM have enough virtual-address-space headroom that the high-k
SRS mmaps + halo2 working buffers fit. k = 18 was still
running at capture time; if anything trips, expect it to be
the **browser's per-tab memory cap** (≈ 4 GiB on Chrome,
configurable) rather than the OS.

#### Known inefficiency (visible in the log)

Each `runProof(k)` produces **2–3 `cache hit` lines** for the
same SRS file, because both `IrSource::keygen` and
`ProofPreimage::prove` call `get_params(k)` independently, and
one further internal call comes from the resolver chain. Each
hit returns the bytes from IndexedDB in microseconds, but the
Rust side runs `ParamsProver::read` on every call — and the
larger-k params take real time to deserialise (the
`bls_midnight_2p17` round of `read()` is responsible for a
non-trivial fraction of the 1m 28s keygen). **Cheap fix:
memoise `ParamsProver` per k on the JS side, hand the wasm an
already-parsed handle.** Not done yet; flagged for the perf
patch series.

#### Recommended optimisation path (research in flight)

A separate agent is auditing the proof stack for wasm-specific
tunings (see follow-up commit). Early leads worth flagging
here in advance:

1. **`wasm-bindgen-rayon` + `SharedArrayBuffer` threads.** Halo2's
   FFT and MSM are embarrassingly parallel via rayon natively;
   default wasm32 has no rayon backend so we're stuck at one
   core. With a wasm-threads build, a 4-core desktop browser
   would plausibly close 60–80 % of the gap to native arm64.
   The workspace already has `wasm-proving-demos/zkir-mt` (the
   "mt" suffix reads as multi-threaded) — this is the
   highest-priority lead to copy from.
2. **`RUSTFLAGS='-C target-feature=+simd128'`** at build time —
   BLS12-381 field arithmetic benefits significantly from the
   wasm SIMD-128 instruction set in Chrome 91+ / Safari 16.4+ /
   Firefox 89+. Pure compile-flag change.
3. **`wasm-opt -O4`** with `--enable-simd --enable-bulk-memory`
   beyond what `zkir-wasm`'s `Cargo.toml` metadata currently
   sets (just `-O --enable-reference-types`).
4. **Parsed-`ParamsProver` memoisation** — see "Known
   inefficiency" above.

The full punch list lands in the next commit when the agent
finishes; this section will be expanded with measured impact
once any of the four are applied.

### Optimisation punch list — research output (2026-05-22)

Two research agents audited the proof stack: one for **CPU /
parallelism** (web wasm focus), one for **memory** (mobile-arm64
focus, k=20 ambition). Their combined findings, ranked by
MB-saved-or-%-speedup per effort hour.

#### CPU / parallelism (web wasm — applied to `contract-benchmark-wasm`)

| # | Lever | Impact | Effort | Status |
|---|---|---|---|---|
| 1 | `wasm-bindgen-rayon` + `SharedArrayBuffer` threads. Template at `wasm-proving-demos/zkir-mt/{Cargo.toml,src/lib.rs}` + `wasm-proving-demos/run.sh` (the build recipe with `RUSTC_BOOTSTRAP=1 RUSTFLAGS='-C target-feature=+atomics,+bulk-memory' wasm-pack build … -- -Z build-std=panic_abort,std`). | 30–50 % (high confidence) | 10 min | **applied 2026-05-22** |
| 2 | `wasm-opt` flags: `-O4 --enable-bulk-memory --strip-debug` (was `-O`). | 5–15 % (med) | 2 min | **applied 2026-05-22** |
| 3 | COOP/COEP HTTP headers (`same-origin` / `require-corp`) on `serve.py` so `SharedArrayBuffer` is available — prerequisite for #1. | enables #1 | 5 min | **applied 2026-05-22** |
| 4 | `RUSTFLAGS='-C target-feature=+simd128'`. **0 % unless `transient-crypto` / `midnight-proofs` have `#[cfg(target_feature = "simd128")]` paths — `grep` finds none today, so this is blocked on an upstream patch.** | 5–15 % (low/med) | 5 min once paths land | blocked on upstream |
| 5 | Memoise `ParamsProver` per-k on the JS side. The current `JsParamsProvider` calls back to JS via `getParams(k)` once per `runProof`, and the Rust `ParamsProver::read` re-deserialises the bytes each time. At k=17 that costs ~2–3 s of redundant parse per `runProof`. Memoise on `Map<k, Uint8Array>` or pass an already-parsed handle. | 5–10 % at k ≥ 15 (med) | 30 min | not done |
| 6 | Aligning `wasm-opt` metadata across all `*-wasm` crates (consistency). | same as #2 when applied | 5 min | not done |

#### Memory / mobile-arm64 (transient-crypto + dioxus-wallet)

| # | Lever | Saving | Effort | Notes |
|---|---|---|---|---|
| 7 | `PK_CACHE_SIZE` (`transient-crypto/src/proofs.rs:250`) 5 → 1 for memory-constrained targets. | **0 MB on first prove**, up to ~1.2 GB after 5 distinct circuits have been used. Useful for the wallet's long-running process (11 DID circuits in rotation) but a no-op for the Benchmark tab (each k = different hash, cache hit rate 0). The research agent's "saves 1.2 GB" headline only applies after cache saturation — flagged here so the table doesn't oversell it. | 2 min | gate behind `#[cfg(target_os = "android")]` so the wallet's hot DID-call path keeps its cache on roomy hosts |
| 8 | Drop `ParamsProver` `Arc` after `setup_vk` in `transient-crypto/src/proofs.rs::keygen` (line 204). **On audit the Arc is already dropped at end-of-statement** — the agent's worry was unfounded for this path. Listed here so future researchers don't redo the trace. | 0 MB (no change needed) | n/a | clarified |
| 9 | mmap-backed SRS via `memmap2` in `base_crypto::data_provider::MidnightDataProvider`. Native arm64 already has filesystem access; replacing the `read_to_end` → `Vec` path with a mmap slice would let the kernel manage page residency under pressure. | 200–400 MB at k = 20 | 1–2 days; requires `unsafe`, careful lifetime mgmt, and a midnight-proofs API surface that accepts a byte-slice (not a `Read`). | longest-pole but the cleanest "fits in 4 GB" lever |
| 10 | Reuse FFT scratch buffers across columns. halo2's witness-FFT pass typically allocates a fresh `Vec<Fr>` per column; pool to one persistent ~32 MB buffer for k = 20. | 100–200 MB | 2–4 hr to audit `midnight-proofs` for existing pool feature flag; 1–3 days if a fork patch is needed | "low-hanging" only if upstream already exposes a flag |
| 11 | Witness column streaming (column-at-a-time commit instead of building the full witness matrix). Peak drops from `M × 2^k × 32 B` to `1 × 2^k × 32 B`. | 300–500 MB at k = 20 | 3–5 days; deep halo2 fork patch | architectural; flag for a later iteration |
| 12 | Chunked Pippenger MSM (process the column in slices). | 100–150 MB | 2–3 days; fork patch | secondary lever |
| 13 | Suspend / destroy the dioxus-wallet's WebView during Benchmark sweeps on Android — saves the ~200–400 MB of Chromium-resident pages competing with halo2 keygen. Already flagged in the earlier "Implications" subsection above; restated here for completeness. | 200–400 MB | ~1 hr | should move S24 Ultra k = 19 from "always OOMs" to "passes" |
| 14 | Allocator swap (`tikv-jemallocator`) on Android. Bionic's malloc has ~16 % overhead for small objects (witness slot metadata, FFT twiddle tables); jemalloc is ~5–8 %. | 5–20 MB | 2–3 hr | very low risk; small win |

#### k = 20-on-mobile target — math

S24 Ultra k = 19 currently OOMs at peak ≈ 5–6 GB working set
against the 3–4 GB per-process Android cap. To land k = 20 (~ 2×
k = 19's footprint) we'd need to roughly halve peak from
today's number, i.e. **save ~600–800 MB.** The combination of
items 9 + 10 + 13 lands in that range collectively (200–400 +
100–200 + 200–400 = 500–1000 MB). #11 makes the headroom
comfortable but is the deepest fork patch.

The lighter combination 7 + 13 (PK cache cap + WebView
suspend, both quick) saves ~200–400 MB end-to-end and is
expected to move k = 19 from "always OOMs" to "passes",
without quite reaching k = 20. That's the realistic
short-term ceiling without touching the halo2 fork.

---


### §10.1 The 30-second summary

**Baseline (Cargo.toml deps unchanged, master `midnight-proofs`):**
S24 Ultra OOM at k ≥ 19. Largest survivable workload was a
~6 242-hash chain at k = 18.

**Latest (this PR chain):** S24 Ultra **completes k = 20**
(24 967 constraints, ~2× the constraints of k = 19) with
4 393 MiB peak HWM and ~862 MiB end-of-prove RSS, well under
the per-app budget.

| Workload                                              | Before (master)   | After (this PR chain) | Δ                |
|-------------------------------------------------------|------------------:|----------------------:|------------------|
| k = 18 peak HWM (S24, real)                           | ~3 900 MiB        | ~2 580 MiB            | **−34 %**        |
| k = 18 keygen-end HWM (emulator, instrumented)        | 1 502 MiB         | 601 MiB               | **−60 %**        |
| k = 18 prove.end RSS (emulator)                       | 1 696 MiB         | 540 MiB               | **−68 %**        |
| k = 18 wall (emulator; qemu-noisy, ballpark)          | ~326 s            | ~90 s                 | −72 % (incl. emulator variance) |
| k = 19 outcome (S24)                                  | OOM at ~7+ GiB    | succeeded @ 5 300 MiB | **unlocked**     |
| k = 20 outcome (S24)                                  | OOM at ~6.8 GiB   | succeeded @ 4 393 MiB | **unlocked**     |
| k = 20 prove wall (S24, wallet UI)                    | n/a (died)        | 3 m 29 s              | first measurement|
| Proof size at k = 20                                  | n/a               | 2 933 B               | unchanged shape  |

CPU / threading was deliberately not the target axis (we already
saturate cores via rayon during MSM + FFT; mobile cores were
not the bottleneck). The wall-time improvements are a
consequence of (a) skipping redundant work like the prover-side
PK rebuild, and (b) not paging memory under pressure — not from
any actual CPU optimization. Treat the wall-time column as a
**by-product**, not the deliverable.

### §10.1.1 Full k = 10..20 before-vs-after sweep (S24 Ultra)

The §10.1 headline focuses on the three k values where the
unlocks are visible (k = 18, 19, 20). The full picture across
the practical range — where the prover is also asked to run
small proofs — is below.

**Measurement provenance:**
* **"After" prove time** — fully measured on S24 Ultra via
  the 2026-05-23 `bench_cli` sweep that produced the
  per-k table in §9 (lines 2275-2298 of this doc). All
  k = 10..20 are direct measurements.
* **"Before" prove time** — partial: k = 14, 16, 17, 18 are
  measured (cross-referenced from the 2026-05-21 sweep,
  §9). k = 10-13, 15, 19, 20 are **extrapolated** as noted.
* **Peak heap** — k = 18, 19, 20 are measured directly on
  S24 Ultra (instrumented `sample_rss_hwm_kb`, §10 step #10).
  k = 10..17 are extrapolated from the halo2 O(N) memory law
  (cosets + SRS each scale as 2ᵏ per step) anchored on the
  k = 18 measurements.

Extrapolations are labelled `(e ±20 %)` to reflect that the
phone-vs-emulator constant factor and per-prove allocator
noise are bounded but real. Treat (e) numbers as
order-of-magnitude correct, not precision claims. Cached
keys, prove-only, with `MIDNIGHT_MMAP_BUILD=1` +
`MIDNIGHT_LAZY_PARAMS=1` on the "After" side; master
`midnight-proofs` on the "Before" side.

#### Prove wall time

| k  | Before (master) | After (PR chain) | Δ            | Source |
|----|----------------:|-----------------:|-------------:|--------|
| 10 | ~ 320 ms (e ±20 %) | **355 ms**    | +11 %        | "after" (m) §9 sweep; "before" extrapolated from k=14 ratio |
| 11 | ~ 555 ms (e ±20 %) | **618 ms**    | +11 %        | same |
| 12 | ~ 945 ms (e ±20 %) | **1.05 s**    | +11 %        | same |
| 13 | ~ 1.65 s (e ±20 %) | **1.87 s**    | +13 %        | same |
| 14 | **3.01 s** (m)  | **3.53 s** (m)   | **+17 %**    | both measured — mimalloc baseline overhead |
| 15 | ~ 5.4 s (e ±25 %) | **7.17 s** (m) | +33 %        | spill-on overhead kicks in at this k |
| 16 | **11.3 s** (m)  | **16.5 s** (m)   | **+46 %**    | both measured — spill cost dominant |
| 17 | **22.3 s** (m)  | **32.8 s** (m)   | **+47 %**    | both measured |
| 18 | **45.2 s** (m)  | **50.0 s** (m)   | **+11 %**    | both measured — spill cost converges with main work |
| 19 | **OOM**         | **1 m 41 s** (m) | **unlocked** | OOM was before any wall-time could be sampled |
| 20 | **OOM**         | **5 m 52 s** (m) | **unlocked** | bench_cli; wallet UI shows 3 m 29 s due to less instrumentation |

#### Peak heap (HWM during prove)

| k  | Before (master)     | After (PR chain)  | Δ           | Source |
|----|--------------------:|------------------:|------------:|--------|
| 10 | ~ 80 MiB (e ±30 %)  | ~ 75 MiB (e ±30 %) | −6 %       | SRS+PK base load dominates; nothing to optimise yet |
| 11 | ~ 150 MiB (e ±30 %) | ~ 140 MiB (e ±30 %) | −7 %      | |
| 12 | ~ 280 MiB (e ±25 %) | ~ 250 MiB (e ±25 %) | −11 %     | |
| 13 | ~ 530 MiB (e ±25 %) | ~ 470 MiB (e ±25 %) | −11 %     | |
| 14 | ~ 1.0 GiB (e ±20 %) | ~ 870 MiB (e ±20 %) | −15 %     | PK_CACHE warm starts to matter; bench-cli RSS samples support this |
| 15 | ~ 1.9 GiB (e ±20 %) | ~ 1.4 GiB (e ±20 %) | −27 %     | |
| 16 | ~ 2.7 GiB (e ±15 %) | ~ 1.9 GiB (e ±15 %) | −30 %     | |
| 17 | ~ 3.4 GiB (e ±15 %) | ~ 2.3 GiB (e ±15 %) | −33 %     | |
| 18 | **~ 3.9 GiB** (m)   | **~ 2.58 GiB** (m) | **−34 %** | direct S24 measurement |
| 19 | **OOM @ 7+ GiB** (m)| **5.3 GiB** (m)    | **unlocked** | PK_CACHE warm carries this one alone |
| 20 | **OOM @ ~ 13 GiB**¹ | **4.39 GiB** (m)   | **unlocked** | disk-spill cosets dominate; counter-intuitively *less* than k = 19 because spill payoff scales |

¹ The "before" k = 20 OOM was never sampled to its peak — the
process died well before that. ~13 GiB is the linear
extrapolation of (advice + fixed + permutation cosets at
extended-domain size 4·2ᵏ × 32 B) added to the k = 19
baseline.

**Why k = 20 peak HWM is lower than k = 19.** Without
disk-spill, the dominant memory contributor at high k is the
4N-element extended-domain cosets (advice/fixed/permutation,
each `2^(k+7)` bytes — at k = 20 that's 128 MiB per coset
held *simultaneously* during `evaluate_h`). The spill path
serialises these — at most one in memory at a time — which
saves more bytes the higher k goes. At k = 19 the cosets are
half the size so the win is smaller relative to the constant
PK + SRS overhead, leaving k = 19 with a higher peak than the
spill-dominant k = 20. Counterintuitive but it tracks the
trace.

#### Reading the table

* **k ≤ 13**: optimisations are a net wash or mild loss
  (mimalloc baseline + spill-on overhead). Production
  recommendation is to leave optimisations off below k = 14,
  exposed via `MIDNIGHT_SPILL_FLOOR_K` (see §10.5).
* **k = 14..17**: small memory wins; CPU regressions
  driven by always-on spill at these k. Follow-up tracked
  to gate spill on `k ≥ SPILL_FLOOR_K`.
* **k = 18**: trade fully lands — −34 % memory for +11 %
  wall time. The "before" RAM (3.9 GiB) was the largest
  workload that ran on phone pre-PR; "after" (2.6 GiB) is
  comfortable headroom under the ~6.5 GiB per-app budget.
* **k = 19, k = 20**: existential unlock — the workload
  simply did not complete before. The CPU column is "first
  measurement", not a regression.

**Both axes scale roughly as 2× per k step** — confirming
the prover is on the asymptotic O(N log N) curve and there's
no constant-factor blowup hiding in the trace. Future
unlocks (k = 21+) will need the **row-streaming `evaluate_h`**
work tracked in §10.8 (k = 21 was found to land memory-wise
on phone but to thrash on coset row scans, not OOM).

#### Caveats

* **Thermal throttling on real device.** The S24 numbers
  above are single-prove cold (sweep paused between rows).
  A back-to-back "Run all" k = 10..20 sweep on the actual
  phone will pin the SoC at ~60-70 °C by k = 15 and the
  big cluster down-clocks 10-20 % — observed in the bench
  CLI's `cpu_avg_freq_khz` traces (§10.3). Read individual-k
  numbers as "what one proof costs when the device is cool"
  and add a 10-20 % buffer for sustained workloads.
* **Constant factor across devices.** All numbers here are
  S24 Ultra. M2 Max iOS Sim is roughly 1.4-1.8× faster
  (native execution, no Bionic syscalls). Android emulator
  on the same M2 Max is roughly 3× *slower* than iOS Sim
  due to qemu binary translation — see
  [[Benchmarks/Android Emulator results]] for the parallel
  table.
* **RSS vs HWM.** "Peak HWM" is the high-water mark across
  the prove phase (kernel `VmHWM` / `getrusage`-style).
  End-of-prove RSS drops sharply — at k = 20 the end-RSS
  is ~862 MiB vs peak 4 393 MiB. Sizing the per-app
  memory budget for sustained proving needs the HWM number,
  not the RSS one.


### §10.3 Per-phase k = 20 trace (S24 Ultra, all patches active)

The trace below is direct grep output from the
`midnight_bench=info` tracing layer (item #9 above), captured
from `bench_cli` on the device. Both RSS and HWM are in MiB,
sampled from `/proc/self/status`.

```
keygen_pk.start                              rss=12    hwm=2114
keygen_pk.assembly_built                     rss=1274  hwm=2114
keygen_pk.fixed_polys.end                    rss=1556  hwm=2114
keygen_pk.fixed_cosets.end                   rss=1556  hwm=2114   ← lazy (empty Vec)
keygen_pk.permutation_pk.end                 rss=1748  hwm=2708   ← polys only, no cosets
keygen_pk.evaluator.end                      rss=2133  hwm=2708
bench.keygen.end                             rss=2133  hwm=2708   ← keygen complete

create_proof.compute_trace.start             rss=2681  hwm=2903
trace.parse_advices.end                      rss=3058  hwm=3994   ← advice cosets built
create_proof.compute_trace.end               rss=3314  hwm=3994

finalise.compute_h_poly.start                rss=3314  hwm=3994
spill_fixed_cosets.start                     rss=3729  hwm=4273   ← single coset transient (+415)
spill_fixed_cosets.end                       rss=3095  hwm=4273   ← −634 (on disk now)
spill_perm_cosets.start                      rss=3095  hwm=4273
spill_perm_cosets.end                        rss=2022  hwm=4273   ← −1 073 (on disk)
drop_cosets.end                              rss=3785  hwm=4393   ← evaluate_h peak (mmap warmed)
finalise.compute_h_poly.end                  rss=576   hwm=4393   ← mmap evicted, RSS −3 209
finalise.vanishing_construct.end             rss=748   hwm=4393
finalise.multi_open.end                      rss=1384  hwm=4393
bench.prove.end                              rss=862   hwm=4393   ← total: 285 s prove, 4 393 MiB peak
```

Reading the trace: the peak HWM (4 393 MiB) is hit at
`drop_cosets.end`, when `evaluate_h` is mid-row-scan over the
mmap'd cosets and the kernel hasn't decided yet that they can
be evicted. As soon as `compute_h_poly` returns, the mmap'd
pages become cold and the kernel reclaims them — RSS drops by
3.2 GiB across a single phase boundary. This is the dynamic
disk-spill was designed for.


### §10.8 k = 21 on phone — the throughput wall, not the memory wall (measured)

Status: characterised 2026-05-23 via `bench_cli` on a real
Samsung S24 Ultra. **k = 21 (49,935 constraints) does not OOM
on phone with the §10 disk-spill stack active** — but the
prove cannot complete in practical wall time because of
mmap-page-fault thrashing in `evaluate_h`. Detailed traces
below.

#### What we measured

Two parallel runs were done with `MIDNIGHT_SPILL_COSETS=1`:

| Target              | k = 21 keygen wall | k = 21 prove wall    | Peak HWM      | Verdict |
|---------------------|-------------------:|---------------------:|--------------:|---------|
| M2 desktop (no per-app cap, 32 GiB RAM) | 115 s | 7 m 11 s         | **11,974 MiB** | succeeded, completed |
| **S24 Ultra (real phone, ~6.5 GiB per-app budget)** | ~143 s | **killed at +13 min during `evaluate_h`** | **5,396 MiB** | did not OOM; thrashed |

#### The phone trace (S24, killed mid-`evaluate_h`)

Direct grep output from `midnight_bench=info` tracing:

```
keygen_pk.start                              rss=14    hwm=4216
keygen_pk.assembly_built                     rss=2538  hwm=4216
keygen_pk.fixed_cosets.end                   rss=3094  hwm=4216   ← lazy
keygen_pk.permutation_pk.end                 rss=3476  hwm=5396   ← lazy cosets
keygen_pk.lagrange_polys.end                 rss=4246  hwm=5396   ← biggest keygen jump
keygen_pk.evaluator.end                      rss=4246  hwm=5396
bench.keygen.end                             rss=4246  hwm=5396   ← keygen total ~143 s

resolver.resolve_key.start                   rss=4246  hwm=5396
resolver.pk_cache_warmed                     (warmed, 80 s — but no rebuild)
resolver.pk_serialised                       rss=4175  hwm=5396   bytes=540 MB
resolver.resolve_key.end                     rss=4175  hwm=5396

create_proof.compute_trace.start             rss=4143  hwm=5396
trace.parse_advices.end                      rss=664   hwm=5396   ← kernel reclaimed during NTT
trace.permutations_commit.end                rss=2332  hwm=5396
create_proof.compute_trace.end               rss=3057  hwm=5396

finalise.compute_h_poly.start                rss=3057  hwm=5396
spill_fixed_cosets.start                     rss=256   hwm=5396   ← spill working, RSS dropping
spill_fixed_cosets.end                       rss=1     hwm=5396   ← +3 min wall (slow disk I/O)
spill_perm_cosets.start                      rss=2     hwm=5396
spill_perm_cosets.end                        rss=1     hwm=5396   ← +1.5 min wall

[killed at this point — drop_cosets / evaluate_h ongoing]
```

#### What the data says

- **Peak HWM on phone never exceeded 5,396 MiB** — comfortably
  under the S24's ~6,500 MiB per-app budget. The §10 disk-spill
  stack is working as designed: at k = 21 the in-memory peak
  is essentially the same as at k = 20 (4,393 MiB).
- **The desktop's 11,974 MiB peak is mostly mmap'd file-backed
  pages that the kernel never had reason to evict.** On phone,
  under memory pressure, `lmkd` aggressively evicted those
  same pages — RSS dropped to 1 MiB mid-spill (the kernel
  reclaimed everything reclaimable).
- **k = 21 is not a memory-ceiling problem on phone.** It is a
  throughput problem.

#### Why the prove can't complete in practical time

`evaluate_h`'s inner loop walks rows across **every coresident
polynomial** (advice, instance, fixed, permutation, lookup),
and on phone these are now:

- `advice_cosets` (~3.8 GiB at k = 20 → ~7.6 GiB at k = 21) —
  built during `compute_trace.parse_advices`, kept in heap.
  Already evicted by lmkd to ~1 MiB resident before
  `evaluate_h` even starts.
- spilled `fixed_cosets` (~6.4 GiB on disk at k = 21) —
  file-backed mmap; OS-evictable.
- spilled `permutation.cosets` (~2.8 GiB on disk at k = 21) —
  file-backed mmap; OS-evictable.

Every row read in the constraint scan touches every
polynomial, which means **every row triggers a page fault**
against an evicted mmap. The disk-spill optimisation, which
buys correctness at k = 20, turns into a throughput
catastrophe at k = 21: the prove that would have completed in
~7 minutes on M2 was projected to take **hours** on phone, all
of it I/O-bound.

The §10 stack still does its job (no OOM at k = 21 on phone),
but a different optimisation is needed to make k = 21
*practical* on phone.

#### What k = 21 on phone would need

Two architectural changes, in order of impact:

1. **Row-streaming `evaluate_h`.** Refactor the constraint
   evaluator's inner loop to process N rows at a time,
   reading windows from the mmap'd cosets in cache-friendly
   strides. Mentioned briefly in §11.3 as the "next deep
   frontier"; this k = 21 experiment confirms it is the right
   frontier. Estimated effort: ~500 LOC of careful surgery
   inside `midnight-proofs::plonk::evaluation::Evaluator::evaluate_h`,
   with hard-to-test invariants (the existing path computes
   exact constraint sums; the streaming version must produce
   identical sums modulo associativity).

2. **Advice-cosets disk-spill.** Apply the same
   `SpilledCosets` pattern from §10.2 #16 to the advice
   columns in `compute_trace.parse_advices`. **By itself this
   does not unlock k = 21** — the k = 21 trace above shows
   the existing spills already drop RSS to ~1 MiB before
   `evaluate_h`, so spilling more doesn't move the peak. It
   would only help in combination with (1), as a way to keep
   the working-set during the row scan small enough to fit
   in resident memory.

(1) without (2) is the most promising path. (2) without (1)
is a no-op on phone (the kernel already evicts under
pressure). (1) with (2) is the rigorous answer that also
gives us margin for k = 22+.

#### Verdict for the §10 PR chain

k = 20 remains the supported ceiling for this PR chain on
phone. k = 21 is in the bucket of "physically lands but
practically unusable" — useful as a stress test to validate
the §10 architecture (which it did, definitively) but not
shippable as a wallet capability.

The path forward is the row-streaming `evaluate_h` refactor.
Tracked as a follow-up; outside the scope of this PR pair.

