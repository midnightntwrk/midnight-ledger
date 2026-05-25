# Midnight Mobile Wallet — Optimization phases

Every memory- and prove-time optimisation we landed during the
mobile-bench effort, in landing order, with the marginal delta it
contributed on top of all prior rows. Sized for the engineer who
needs to know *why* each patch exists and what it actually moved.

Companion docs:
- [`architecture.md`](./architecture.md) — design + integration reference.
- [`benchmark.md`](./benchmark.md) — raw sweep tables, the per-phase trace,
  and the k=10..20 before/after comparison (those numbers justify the
  deltas claimed here).
- [`react-native-adoption.md`](./react-native-adoption.md) — packaging
  options + wasm-target limitations.

**Cross-doc §-numbers:** the original single-file numbering is preserved in each split file. If you see a §-reference that isn't in the current doc, check `midnight-mobile-architecture.md` (the index) for the cross-doc map.

---

## §10. Outcomes — what actually shipped (2026-05-23)

The 2026-05-22 punch list above predicted that landing items 9 +
10 + 13 would save 500–1000 MB and that **k = 20 was the
"deepest fork patch" tier requiring 3–5 days**. The session that
followed produced a different shape: we did patch the fork (the
deeper levers turned out to be cheaper to implement than feared
*and* the only ones that actually mattered for k = 20), and the
"easy" levers (WebView suspend, PK cache cap) ended up being
non-factors because the real bottlenecks were elsewhere.

This section documents what landed, in landing order, with
measured numbers from a physical Samsung S24 Ultra.


### §10.2 Step-by-step changes, in landing order

Each row is a single commit (or tight cluster) with its measured
delta. The "delta" column is the marginal contribution **on top
of all prior rows in the table** — same caveat as benchmark
suites that include only the last patch's effect.

| #   | Patch                                                                                                                              | Commit / PR                                                       | Layer            | Marginal delta                                                                       | Notes                                                                                                                                                                                                                                                                                       |
|----:|------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------|------------------|--------------------------------------------------------------------------------------|----|
| 1   | `cost-model OOM skip` — `ir.model()` HashMap walks every cell at k = 19+ regardless of proving                                     | `a474dd7a` (ledger)                                               | bench-side       | unblocks k = 19 even being attempted                                                 | Not a proving optimisation; an instrumentation bug that masqueraded as a proving OOM. Documented because the original "k = 19 always OOMs" finding included this red herring. |
| 2   | `MIDNIGHT_LAZY_PARAMS` + peak RSS measurement                                                                                       | `2b8a9b0c` (ledger)                                               | bench-side       | measurement only                                                                     | Enabled the per-phase profiling that made every subsequent decision data-driven. |
| 3   | `[patch.crates-io] midnight-proofs = { path = ../midnight-zk/proofs }`                                                              | `21a93efb` (ledger)                                               | workspace        | enables fork patches                                                                 | Lets the workspace consume the local midnight-zk fork. |
| 4   | `IR + ProverKey caches + headless bench_cli`                                                                                        | `e022f2d5` (ledger)                                               | bench-side       | enables instrumented iteration                                                       | The `bench_cli` binary became the measurement workhorse for everything that follows. |
| 5   | `lazy g_lagrange via OnceLock + drop_lazy_bases`                                                                                    | `45ea9f7` (midnight-zk)                                           | proofs (fork)    | enables next; ~100 MiB steady-state floor                                            | The Lagrange basis is rebuilt only when first used (zswap proves, etc.); off the prover hot path. |
| 6   | `bump to 0.7.99 + drop path-dep on midnight-curves`                                                                                 | `30688ef` (midnight-zk)                                           | proofs (fork)    | enables `[patch.crates-io]` resolution                                               | Plumbing. |
| 7   | `ParamsKZG::read_custom_lazy<R: Read + Seek>`                                                                                       | `a5f9cda` (midnight-zk)                                           | proofs (fork)    | enables #8                                                                           | API surface for streaming + mmap'd SRS reads. |
| 8   | `mmap-backed ParamsKZG via BasesStorage`                                                                                            | `946b1f0` (midnight-zk)                                           | proofs (fork)    | ~300 MiB at k = 20 steady-state (SRS pages now file-backed, kernel-evictable)        | The SRS is the largest single allocation in `ParamsKZG`; this moves it out of heap into the page cache. **Peak-during-MSM unchanged** (SRS pages stay hot during the prover walk), the win is between proofs and during keygen. |
| 9   | `per-phase memory instrumentation` (proofs side)                                                                                    | `3f0abef` (midnight-zk)                                           | proofs (fork)    | measurement only                                                                     | `tracing` markers at every keygen + prove phase, target `midnight_bench`. The trace fragments shown in this section are direct grep output from this layer. |
| 10  | `sample_rss_hwm_kb` for macOS / iOS via `getrusage`                                                                                 | `49f9134` (midnight-zk)                                           | proofs (fork)    | measurement only                                                                     | Linux/Android already had `/proc/self/status`; this closed the gap on Apple platforms so the same tracing layer works there. (macOS's `ru_maxrss` is a lifetime high-water mark, not current — flagged in commit notes; iOS `/proc` is unavailable so this is the best we can do.) |
| 11  | `warm_pk_cache` (ChainResolver pre-warms `PK_CACHE`)                                                                                | `4ecfcfe4` (ledger)                                               | workspace        | **k = 18 peak −1 300 MiB (3.9 GiB → 2.6 GiB on S24)** ; **k = 19 unlocked** (~7 + GiB → 5.3 GiB) | The single biggest user-visible win. Eliminates the 1.4 GiB "deserialise + rebuild" copy that lived between `ChainResolver::resolve_key` and `ProofPreimage::prove`. See [[Optimizations/PK_CACHE warm]] in the Obsidian vault for the full deep-dive. |
| 12  | `wire mmap-backed SRS into ParamsProver`                                                                                            | `fb4e3208` (ledger)                                               | workspace        | enables wallet to honour `MIDNIGHT_MMAP_BUILD=1`                                     | The wallet writes a `.mmap` companion alongside each `bls_midnight_2pK` SRS file on first run; subsequent runs mmap directly. |
| 13  | `mimalloc as global allocator + MIMALLOC_PURGE_DELAY=0` opt-in                                                                      | `afec4db7` + `31de1f78` (ledger)                                  | bench-side       | k = 18 peak −60 MiB (emulator); rough wash on desktop; **wins where it matters**     | Trades a small constant CPU overhead for aggressive page return to the OS. Bionic's malloc and macOS libmalloc both pool too greedily for the GiB-scale-and-drop pattern of `compute_h_poly`. |
| 14  | `defer fixed_cosets construction to per-prove`                                                                                      | `d54c690` (midnight-zk)                                           | proofs (fork)    | **k = 18 keygen-end HWM −608 MiB** (1 502 → 894 emulator)                            | Stops paying the keygen-side extended-domain expansion until the one place that actually consumes it (`evaluate_h`). |
| 15  | `defer permutation.cosets the same way as fixed_cosets`                                                                             | `6cec3e7` (midnight-zk)                                           | proofs (fork)    | **k = 18 keygen-end HWM −293 MiB** (894 → 601 emulator); total −60 % keygen vs base  | Same architectural pattern, applied to the permutation argument. |
| 16  | **`disk-spill cosets path for k ≥ 20 unlock`** (the architectural unlock)                                                           | `66b43d1` (midnight-zk) + `6b70d7fe` (ledger; Android auto-enable) | proofs (fork) + wallet | **k = 20 unlocked** (~6.8 GiB OOM → 4 393 MiB peak); k = 18 peak unchanged | `compute_h_poly` writes each extended-domain coset to a tempfile one at a time, drops the in-memory poly, then mmaps the file for `evaluate_h`. The wallet auto-points `MIDNIGHT_SPILL_DIR` at `/data/data/<APP_ID>/cache/midnight-cosets` (the 93 GiB `/data` partition on S24). See [[Optimizations/Disk-spill cosets — the k=20 unlock]] for the deep-dive. |


### §10.4 What the punch list got wrong, and why

Cross-referencing §9's punch list with what landed:

| Punch-list item                                            | Predicted impact            | What actually happened                                                                                                                                                                  |
|------------------------------------------------------------|-----------------------------|------|
| #5 Memoise `ParamsProver` per-k (web)                     | 5–10 % at k ≥ 15           | not done; web target deprioritised                                                                                                                                                       |
| #7 `PK_CACHE_SIZE` 5 → 1                                  | 1.2 GB *after* cache saturation | **not done — wrong fix**. The waste was the prover-side **rebuild** of an already-cached PK, not the cache itself. `warm_pk_cache` (item #11 in §10.2) sidestepped the whole problem by pre-warming the cache that the prover boundary's `tagged_deserialize` consults. The PK cache cap is still 5 (untouched) and still healthy. |
| #9 mmap-backed SRS via `memmap2`                          | 200–400 MB at k = 20       | **landed** as item #8 in §10.2. Saved ~300 MiB steady-state. **Peak-during-MSM unchanged** (the original prediction implicitly assumed mmap pages could be evicted during MSM — they can't, because the prover walks every element). The win is between proofs / during keygen, not during `compute_h_poly`. |
| #10 Reuse FFT scratch buffers                             | 100–200 MB                  | not pursued. The lazy-cosets + disk-spill stack made the cosets themselves disappear from heap, which dominated the FFT scratch question. |
| #11 Witness column streaming                              | 300–500 MB at k = 20        | **superseded by disk-spill**. The architectural pattern (build → commit → drop) is the same idea applied to witness instead of cosets; cosets were ~2× the size and unlocked k = 20 alone. Witness streaming is the path to **k = 21**, not k = 20 — see [[Open questions/H polynomial streaming]]. |
| #13 Suspend / destroy WebView during sweeps               | 200–400 MB                  | **not done — turned out unnecessary**. The `bench_cli` measurements (no WebView) showed the OOM was in the proving heap itself, not WebView competition. Once `warm_pk_cache` + lazy cosets + disk-spill landed, the wallet (with WebView resident) also passes k = 20 — confirming the WebView was a noise factor, not the limit. |
| #14 Allocator swap (jemalloc/mimalloc)                    | 5–20 MB                     | **landed** as item #13 in §10.2. Real impact: ~60 MiB at k = 18 (more than predicted) — the prediction was for "small object overhead", but the actual win is **page return cadence**, which matters for the GiB-scale temporary allocations in `compute_h_poly`. |

**The big lesson:** every punch-list item that targeted "trim
N MiB off the heap floor" was a 10–50× under-estimate of what
*defer-then-disk-back* could do for the same effort. The
keygen-end heap floor went from ~5 GiB (predicted "lower
ceiling") to ~600 MiB — order-of-magnitude, not percent.

### §10.5 Opt-in env vars (production wallet surface)

The mobile build sets these automatically at process start
(`mobile-bench/dioxus-wallet/src/lib.rs::main` on Android,
`::start_app` on iOS). They are exposed as env vars rather than
config flags so the headless `bench_cli` and the wallet share
exactly the same code path.

| Env var                            | Default in wallet              | Effect                                                                                              |
|------------------------------------|--------------------------------|-----------------------------------------------------------------------------------------------------|
| `MIDNIGHT_PP`                      | `/data/data/<APP_ID>/cache/midnight-pp` (Android), `Library/Caches/midnight-pp` (iOS) | SRS cache root                                                                                      |
| `MIDNIGHT_MMAP_BUILD=1`            | set on first wallet run        | Writes `.mmap` companion alongside each SRS file the first time it loads; subsequent runs mmap     |
| `MIDNIGHT_SPILL_COSETS=1`          | **set on Android** (Linux/iOS off by default) | Enables disk-spill in `compute_h_poly`                                                              |
| `MIDNIGHT_SPILL_DIR=<path>`        | `/data/data/<APP_ID>/cache/midnight-cosets` (Android) | Where `compute_h_poly` writes coset tempfiles. Override if your `TMPDIR` partition is too small.   |
| `MIMALLOC_PURGE_DELAY=0`           | not set by default             | mimalloc tuning; aggressively return freed pages to the OS. Set this via `adb shell` for the headless `bench_cli` when chasing the last 5 % of peak RSS. |
| `BENCH_LOG=midnight_bench=info`    | not set                        | Emits the per-phase RSS / HWM tracing lines used to build the table in §10.3. Set for `bench_cli`; the wallet has its own log tab that consumes these without the env var. |

### §10.6 What we deliberately did **not** do

Listed so future readers don't re-investigate paths that were
considered and explicitly skipped.

| Lever                                                  | Why skipped                                                                                                                                                                                                                                                                                                                                                       |
|--------------------------------------------------------|------|
| **k = 21** (~50 k constraints)                         | k = 20 already overshoots the largest real-world workload by ~2×. The next limit (`advice_cosets`, ~7.6 GiB at k = 21) would require the same disk-spill treatment applied to a different (and bigger) collection — ~300 LOC of focused work, but no business reason on the current roadmap. |
| **Row-streaming `evaluate_h`**                         | Cleanest long-term answer (constant memory regardless of k) but ~500 LOC of constraint-evaluator surgery, with hard-to-test invariants. Deferred. |
| **`tikv-jemallocator`** (in favour of mimalloc)        | Both deliver similar page-return characteristics; mimalloc was already present in the wallet's transitive dep tree. No need to evaluate two. |
| **WebView suspend during sweeps**                      | The proving heap was the bottleneck, not WebView competition. Verified via `bench_cli` (no WebView) hitting the same OOM at the same k. |
| **`simd128` on web/wasm**                              | Blocked on upstream: `transient-crypto`/`midnight-proofs` have no `#[cfg(target_feature = "simd128")]` paths. Re-evaluate once that ships upstream. |
| **Chunked Pippenger MSM**                              | Estimated 100–150 MiB savings, but ~3 days of work and would land *during* the prove peak which disk-spill has already brought well below the ceiling. Not worth it at k = 20. |

### §10.7 Disk usage (the cost we pay for the RAM win)

Disk-spill is the only optimisation that buys RAM with disk.
The numbers, on a real S24 Ultra during k = 20:

| Spill file                        | Size at k = 20 | Lifetime                          |
|-----------------------------------|---------------:|-----------------------------------|
| `midnight-cosets-XXXXXX` (fixed)  | ~3.2 GiB       | one `compute_h_poly` call         |
| `midnight-cosets-YYYYYY` (perm)   | ~1.5 GiB       | one `compute_h_poly` call         |
| `.mmap` companions in `MIDNIGHT_PP` | ~360 MiB at k = 20 (vs 240 MiB legacy format) | persistent (re-read each prove) |

Total transient disk per k = 20 prove: ~4.7 GiB. Held only for
the duration of `compute_h_poly` (~30 s of the 285 s prove
wall); deleted automatically when `SpilledCosets` drops. The
`/data` partition on the S24 has ~93 GiB free, so even worst-
case concurrent runs are fine.

If a downstream device has <10 GiB free `/data`, set
`MIDNIGHT_SPILL_DIR` to a larger filesystem; if no filesystem
has enough room, unset `MIDNIGHT_SPILL_COSETS=1` (the wallet
will fall back to the in-heap path and OOM-or-not behaviour
will match the pre-spill table in §9).


### §10.9 Prebuilt PK on disk — the cold-launch unlock (proposed)

Status: design proposal; not yet implemented. Sketched here
because it is the natural extrapolation of the §10 stack and
the next unlock the data points at.

#### The problem

After all §10 patches, **keygen at k = 20 still costs ~1 m 16 s
on every wallet cold-launch.** The `PK_CACHE` warm-cache lives
in process memory, so once the user closes the app and
relaunches, the cache is empty and the first prove pays the
full keygen cost again.

The 1 m 16 s breaks down (from the §10.3 trace):

| Sub-phase                          | k = 20 wall | What it does                              |
|------------------------------------|------------:|-------------------------------------------|
| `assembly_built`                   | 0.5 s       | parse circuit IR → cell assignments       |
| `synthesise.end`                   | 0.5 s       | populate fixed-column cells               |
| `batch_invert_rational.end`        | 0.9 s       | invert rational denominators              |
| `selectors_to_fixed.end`           | 0.1 s       | merge selector columns into fixed         |
| `fixed_polys.end`                  | 1.9 s       | inverse-FFT each fixed column to coeff form |
| `permutation_pk.end`               | 1.3 s       | compute σ-permutation polynomials         |
| `lagrange_polys.end`               | 3.1 s       | compute `l0`, `l_last`, `l_active_row`    |
| `evaluator.end`                    | 0.0 s       | metadata only                             |
| total                              | ~8 s        | (the rest is dominated by tagged_serialise + MidnightPK::read, which we already short-circuit via warm_pk_cache) |

The actual heavy keygen work is **~8 s of CPU**; the
remaining ~68 s is the bytes-API round-trip inside
`ChainResolver::resolve_key` (gzip-compress the PK to ~270 MB
bytes, then `PK_CACHE` lookup populates the deserialised side
without re-running keygen).

#### The proposal

**Persist the keygen output to a per-circuit, per-`k`,
per-halo2-version file on disk in mmap-friendly layout.** On
subsequent wallet launches, mmap the file, verify SHA, skip
keygen entirely.

Layout sketch:

```
$MIDNIGHT_PK_CACHE_DIR/
└── pk-<circuit_hash>-<k>-<midnight_proofs_version>/
    ├── manifest.json         # metadata: SHA256 of each blob,
    │                         #           halo2 version, IR hash
    ├── fixed_values.bin      # raw [F; n*ncols] — mmap-as-slice
    ├── fixed_polys.bin       # raw [F; n*ncols] — mmap-as-slice
    ├── permutation_polys.bin # raw [F; n*nperm] — mmap-as-slice
    ├── l0.bin / l_last.bin / l_active_row.bin
    ├── vk.bin                # the small verifying key (gzipped is fine)
    └── ev.json               # Evaluator structural metadata
```

The same `BasesStorage<F>` pattern that already powers the
mmap'd SRS (§10.2 item #8) extends to these vectors:
`Polynomial<F>.values` becomes either `Owned(Vec<F>)` (the
keygen output path) or `Mapped { mmap, ptr, len }` (the
mmap-from-disk path). The prover never knows which it has —
both expose `&[F]` via deref.

#### Sized projection

For each circuit, persistent disk footprint:

| k    | fixed_values + fixed_polys | perm.polys | l-polys | total per circuit |
|-----:|---------------------------:|-----------:|--------:|------------------:|
| 12   | 25 MiB                     | 11 MiB     | 1.5 MiB | ~38 MiB           |
| 14   | 100 MiB                    | 44 MiB     | 6 MiB   | ~150 MiB          |
| 17   | 800 MiB                    | 352 MiB    | 50 MiB  | ~1.2 GiB          |
| 18   | 1.6 GiB                    | 704 MiB    | 100 MiB | ~2.4 GiB          |
| 20   | 6.4 GiB                    | 2.8 GiB    | 400 MiB | ~9.6 GiB          |

The extended-NTT cosets are **intentionally not persisted** —
they remain lazy/disk-spilled per the §10 stack. They are
short-lived per-prove transient anyway; persisting them
would inflate the artifact 4× without functional benefit.

#### Performance projection

| Stage                                          | Current (post-§10) | With prebuilt PK |
|------------------------------------------------|-------------------:|-----------------:|
| First-prove keygen at k = 20 (cold launch)     | 76 s               | ~3–5 s (mmap setup + SHA verify) |
| Subsequent proves same session (warm PK_CACHE) | ~0                 | ~0 (in-mem cache still wins)     |
| Across wallet restart                          | full 76 s again    | ~5 s             |
| First-prove keygen at k = 12 (cold launch)     | ~0.4 s             | ~0.05 s (already fast)           |
| First-prove keygen at k = 17 (cold launch)     | ~10.7 s            | ~1.5 s            |

The wallet's actual workload (DID circuits at k ≈ 12) gets a
modest win (~350 ms saved per cold launch). The k = 17–20
power-user path gets a huge win (76 s → 5 s, a **15× speedup
on cold-launch first-prove**).

#### Tradeoffs

| Pro                                                              | Con                                                                  |
|------------------------------------------------------------------|----------------------------------------------------------------------|
| First-prove cold-launch latency drops 15× at high k              | Per-circuit artifact size: up to ~9.6 GiB at k = 20                  |
| Same architecture pattern as the mmap'd SRS — proven shape       | Wallet has to manage a cache directory + eviction policy             |
| Artifacts can be CDN-distributed (downloaded on first run)       | One blob per circuit × per k × per halo2-version → versioning matrix |
| OS evicts cold artifact pages under memory pressure              | Cache invalidation: any halo2 fork bump invalidates every PK file    |
| Works the same on Android, iOS, desktop — no platform-specific   | SHA verification is non-negligible (~3 GiB hash at k = 20 = ~1.5 s)  |

#### Where the artifacts come from

Two production options:

1. **Pre-built artifacts shipped via CDN.** Upstream Midnight
   provides per-circuit-per-k artifacts at e.g.
   `pk.midnight.network/<circuit_hash>-<k>-<halo2_ver>.tar.zst`.
   The wallet downloads on first need; same as SRS today.
   **UX:** ~10 GiB cumulative for typical DID + zswap circuit
   set at production-sized k, downloaded on demand. Probably
   2-3 GiB after the wallet curates "which circuits do my
   contracts use" — comparable to large iOS games.

2. **Locally generated artifacts cached after first compute.**
   First prove for a new circuit/k pair runs full keygen,
   then writes the result to disk; subsequent proves mmap.
   **UX:** no upfront download, but the first-ever prove
   pays the full keygen cost. Best UX trade-off for the
   wallet's actual workload (DID circuits computed once,
   reused thousands of times).

The pragmatic answer is **option 2** — the wallet generates
its own cache the first time each circuit is touched, then
mmaps thereafter. Option 1 becomes interesting if/when the
wallet ships with a fixed set of circuit IDs that everyone
will need (e.g. the canonical zswap shielded-transfer circuit).

#### Estimated effort

- ~3–5 days for the disk-cached-PK layer (analogous to the
  existing `BasesStorage<F>` for the SRS — same pattern, more
  fields).
- ~1 day for the wallet-side cache directory management
  (eviction policy, total-size cap, write-on-first-compute
  hook).
- ~1 day for CDN-distribution shape (if we pursue option 1).
- Total: ~5–7 working days for option 2 alone; ~7–10 if we
  also ship the CDN path.

#### Why not yet

This sits outside the §10 PR chain because:

1. **It is not a memory-peak unlock** — `PK_CACHE` already
   handles the in-session case. This optimisation is purely
   a *wall-time* win on cold launch.
2. **The architectural pattern is identical to mmap'd SRS**,
   so the design risk is minimal — but the implementation
   surface (`BasesStorage<F>` applied to every `Polynomial`
   field in `ProvingKey`) is bigger than any single §10
   patch.
3. **It's a wallet-side concern more than a prover-fork
   concern.** The midnight-zk fork needs the `BasesStorage<F>`
   plumbing extended, but the cache directory + lifecycle
   logic lives in the wallet (or in `transient-crypto`'s
   PK_CACHE neighbour).

Tracked as the next major optimisation; will land in a
follow-up PR pair after §10 is reviewed.

### §10.10 Loose-end fixes (post-§10.1–§10.9 follow-ups)

After the §10 stack landed, four loose ends were identified
and addressed before the next development phase:

1. **`SPILL_FLOOR_K = 18` gating in `compute_h_poly`** —
   midnight-zk `cf60e3c`. The wallet sets `MIDNIGHT_SPILL_COSETS=1`
   unconditionally on Android, but the disk round-trip is pure
   overhead at small k (+46–47 % prove time at k=16/17 vs the
   2026-05-21 baseline). The patch adds
   `pk.vk.domain.k() >= SPILL_FLOOR_K` to the spill decision
   in `compute_h_poly`, with a `MIDNIGHT_SPILL_FLOOR_K` override
   env var. Default 18; the k=20 unlock is unaffected.

2. **iOS `start_app()` disk-spill env-var setup** —
   midnight-ledger `4c6e912f`. Mirrors the Android-side wiring
   from `6b70d7fe`. Verified end-to-end on iPhone 17 Pro arm64
   simulator: `cargo build --target aarch64-apple-ios-sim
   --release` clean, xcodebuild succeeded, simctl boot/install/
   launch worked, both `Library/Caches/midnight-pp/` and
   `Library/Caches/midnight-cosets/` directories were created
   inside the app sandbox on first launch.

3. **Wallet UI "0 ms" keygen display bug** —
   midnight-ledger `4c6e912f` (same commit as #2). The bench
   tab rendered `format_ms(0) == "0 ms"` when the key cache hit
   on second-run-same-k, misleading users who expected the
   76 s keygen number from the first run. Fixed: when
   `keygen_ms == 0`, the cell now reads `"cached"`. The
   underlying `RunStats.keygen == Duration::ZERO` semantic is
   preserved.

4. **`.gitignore` for xcodebuild artefacts** —
   midnight-ledger `4c6e912f` (same commit). `ios/build/`
   (xcodebuild's `-derivedDataPath` output) and
   `ios/App/libdioxuswalletmain.dylib` (regenerated by every
   `cargo build --target aarch64-apple-ios-sim`) are now
   excluded.

### §10.11 PR map

Both PRs are inside the personal workspace `yshyn-iohk/*`; no
upstream `midnightntwrk/*` repos were touched.

| Repo            | PR                                                              | Branch                            | Base       | Latest commit                                                            |
|-----------------|-----------------------------------------------------------------|-----------------------------------|------------|--------------------------------------------------------------------------|
| midnight-zk     | https://github.com/yshyn-iohk/midnight-zk/pull/1                | `feat/v0.7-h-poly-streaming`      | `main`     | `cf60e3c perf(proofs): SPILL_FLOOR_K=18 — skip disk-spill at small k`     |
| midnight-ledger | https://github.com/yshyn-iohk/midnight-ledger/pull/1            | `mobile-prototype`                | `ledger-8` | `4c6e912f feat(wallet): iOS disk-spill + "cached" keygen UI + .gitignore` |

All commits in both PRs are GPG-signed (key `38080D6E`,
`yurii.shynbuiev@iohk.io`) and DCO-signed.

---

