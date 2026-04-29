# mobile-bench iteration-2 results

Captured 2026-04-28 on Apple Silicon (M2 Max). Android emulator + S24 Ultra
runs still pending — capture commands at the bottom of this section.

## Circuits added this iteration

Iteration-2 lifts two more `TestResolver`-based proofs from
`zkir/tests/proofs.rs` into `prover-core` so we can characterise hash and
curve workloads on mobile silicon (in addition to iter-1's bare assert):

| Surface              | Source test                              | Inputs | Workload                       |
|----------------------|------------------------------------------|-------:|--------------------------------|
| `zkir-hash-to-curve` | `test_htc_proof` (proofs.rs:162)         |      3 | `hash_to_curve`                |
| `zkir-ec-mul-add`    | `test_ec_proof` (proofs.rs:352)          |      4 | `ec_mul` + `ec_mul_generator` + `ec_add` |

Both are exposed as `ProverCore::prove_htc_example` /
`ProverCore::prove_ec_example`, share the same `ExampleResolver` plumbing
as iter-1's `prove_zkir_example`, and have buttons in the dioxus UI plus
a `bench-runner all` subcommand.

## Latency snapshot (macOS M2 Max, release)

`bench-runner all` — load IR + keygen + prove + verify, single-shot,
fresh cache.

| Surface              | k  | Prove (ms) | Verify (ms) | Proof bytes |
|----------------------|---:|-----------:|------------:|------------:|
| `zkir-minimal-assert`|  4 |         24 |          12 |        2549 |
| `zkir-hash-to-curve` |  9 |        107 |           2 |        3317 |
| `zkir-ec-mul-add`    | 11 |        317 |           2 |        3173 |

Observations:
- Hash-to-curve at k=9 is **~4.5× slower** than the bare `assert` — the
  expected cost of a single in-circuit hash + curve mapping.
- The ec circuit at k=11 is **~13× slower** than the bare assert — three
  curve ops bumped k by two more rows (16× more advice) plus the curve
  arithmetic itself.
- Verify time is dominated by the constant-cost halo2-kzg pairing, not
  circuit complexity, so it stays ≤ 12 ms across all three.
- Proof bytes grow modestly with k (2549 → 3173–3317 B).

## Android emulator (TBD)

Reuses `bench-runner` cross-compiled to `aarch64-linux-android` from
iter-1. From the workspace root:

```bash
# 1. Cross-compile (first time, ~2 min).
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a build -p prover-core --bin bench-runner --release

# 2. Push and run (params already on /data/local/tmp/midnight-pp from
#    iter-1; ec needs bls_midnight_2p11 which is already pushed). The
#    `all` subcommand captures one JSON line per surface.
ADB=~/Library/Android/sdk/platform-tools/adb
$ADB push target/aarch64-linux-android/release/bench-runner /data/local/tmp/
$ADB shell 'MIDNIGHT_PP=/data/local/tmp/midnight-pp \
  BENCH_CACHE_DIR=/data/local/tmp/bench-cache-iter2 \
  /data/local/tmp/bench-runner all'
```

Expected on Pixel_Fold_API_35 (arm64-v8a translated on Apple Silicon):
~3–4× the macOS-release numbers, matching iter-1's ratio. Confirm by
filling in:

| Surface              | k  | Prove (ms) | Verify (ms) |
|----------------------|---:|-----------:|------------:|
| `zkir-hash-to-curve` |  9 |        TBD |         TBD |
| `zkir-ec-mul-add`    | 11 |        TBD |         TBD |

> ⚠ **Do not screenshot the emulator** — the iteration-1 tooling captures
> latency entirely via `adb shell` stdout (one JSON line per run). No UI
> introspection is needed.

## What's been validated end-to-end (delta vs. iter-1)

- ✅ `prove_htc_example`: load IR (`hash_to_curve`, k=9) → keygen → prove
  → verify with binding input. `verified=true` on macOS release.
- ✅ `prove_ec_example`: load IR (`ec_mul` + `ec_mul_generator` + `ec_add`,
  k=11) → keygen → prove → verify. `verified=true` on macOS release.
- ✅ Shared `ExampleResolver` extracted from `zkir_example` so all three
  surfaces use one resolver shape — cuts ~30 lines of duplication.
- ✅ `bench-runner` accepts `zkir|htc|ec|all` arg; `all` prints one JSON
  line per surface for direct table ingestion.
- ✅ Dioxus desktop UI exposes the two new buttons; busy state shared
  across all three so a long ec run doesn't allow a concurrent click.

## What still needs a human (carryover from iter-1)

- ❌ Samsung S24 Ultra numbers (htc + ec) — same `bench-runner all` path.
- ❌ Dioxus APK on emulator (`dx` blocked by `krates 0.17.5`; cargo-apk
  path still WIP).
- ❌ Real-circuit workloads (Dust spend, zswap output) — deferred to a
  later iteration as previously discussed (#2 + #4 from the survey).

---

# mobile-bench iteration-1 results

Captured 2026-04-28 on Apple Silicon (M2 Max). Real-device numbers
(Samsung S24 Ultra) still pending.

## Circuit

Iteration-1 exercises `zkir-minimal-assert` — a 1-input `assert(cond == 0)`
circuit at `k=4`, mirroring `zkir/tests/proofs.rs::test_minimal_proof`. The
fallible/count fixtures are vendored under `mobile-bench/fixtures/` for future
use; they require a real `communications_commitment` derived from contract
simulation and so are not exercised this iteration.

## Latency snapshot

All runs include load-IR + keygen + prove. Verify uses `PARAMS_VERIFIER`.

| Surface                              | Path    | Prove (ms) | Verify (ms) | Proof bytes |
|--------------------------------------|---------|-----------:|------------:|------------:|
| macOS desktop M2 Max (debug)         | library | 33         | 15          | 2549        |
| macOS desktop M2 Max (debug)         | http    | 24         | server-side | 2549        |
| macOS desktop M2 Max (release)       | library | 24–27      | 9–18        | 2549        |
| macOS desktop M2 Max (release bench) | library | 41.5 ¹     | skipped     | 2549        |
| Android emulator arm64-v8a (release) | library | 82–106     | 38–58       | 2549        |
| Samsung S24 Ultra                    | library | …          | …           | …           |

¹ Criterion mean across 100 samples (`cargo bench -p prover-core --features bench`).
Higher than the per-call release runner because criterion measures
prepared-runtime overhead too.

The Android emulator runs **3.0–4.0× slower than desktop release** — within
expectation for a translated arm64 emulator on Apple Silicon, and
comparable to what we'd see on weaker mobile silicon.

## What's been validated end-to-end

- ✅ **Library path** (`prove_zkir_example`): load IR → keygen →
  `ProofPreimage::prove` → `VerifierKey::verify`. `verified=true` on macOS
  debug, macOS release, and Android emulator (arm64-v8a, Pixel Fold API 35).
- ✅ **HTTP path** (`prove_via_http`): in-process `midnight-proof-server`
  bound to a random port; POST `/prove` round-trip with
  `(ProofPreimageVersioned, Some(ProvingKeyMaterial), None)` payload;
  response deserializes as `ProofVersioned::V2(Proof)`; locally verifies
  with the same `VerifierKey`. Desktop only.
- ✅ **Android cross-compile gate**: full proving stack
  (`midnight-zk-stdlib`, `transient-crypto`, `ledger`, `zswap`, `zkir`,
  `prover-core`, the `bench-runner` binary) compiles cleanly to
  `aarch64-linux-android` with NDK r27 + `cargo-ndk`. No C/CMake patches.
- ✅ **Native Android run**: `bench-runner` binary pushed via `adb push`
  produces verifying proofs on the emulator with parameter cache primed
  via `adb push` of `bls_midnight_2p4..2p11`.
- ✅ **Dioxus desktop UI** (`cargo run -p dioxus-bench`): window opens, no
  startup panic.

## What still needs a human

- ❌ **`dx` is blocked**: dioxus-cli 0.6.3 and 0.7.6 both ship `krates 0.17.5`,
  which panics on this workspace's self-path-deps pattern (10 crates list
  themselves as dev-deps with non-default features for proptest/test
  unification — see e.g. [storage-core/Cargo.toml:71](../storage-core/Cargo.toml)).
  dioxus main is also still on `krates 0.17.5`, so a CLI upgrade alone
  doesn't fix it. Workarounds:
  - **Desktop**: `cargo run -p dioxus-bench` (validated; window opens).
  - **Android UI**: needs the cargo-apk path below (still WIP) or
    a fork of dioxus-cli with `krates ^0.21`.
- ❌ **Dust-spend proving** (`prove_dust_spend`): deferred. Building a valid
  `ProofPreimage` for a Dust spend requires reproducing the wallet
  state-machine (DustState, secret keys, UTXOs, kernel transcript).
- ❌ **Samsung S24 Ultra latency**: needs the device plugged in over USB
  with Developer Options + USB debugging enabled. Reuses the
  `bench-runner` binary — no UI required for the latency number.
- ❌ **Dioxus APK on emulator/device**: tracked as plan-B. `cargo-apk` is
  installed; the dioxus-bench manifest needs a `[package.metadata.android]`
  section and the lib needs to compile as `cdylib` for an Android Activity
  to load it. Not yet attempted.

## Reproducing the numbers

### Desktop (release, native runner)

```bash
cargo build -p prover-core --bin bench-runner --release
MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" ./target/release/bench-runner
```

### Android emulator

```bash
# 1. Cross-compile the runner.
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a build -p prover-core --bin bench-runner --release

# 2. Boot the (arm64-v8a) emulator.
~/Library/Android/sdk/emulator/emulator -avd Pixel_Fold_API_35 \
  -no-snapshot-load -no-audio -no-boot-anim &
~/Library/Android/sdk/platform-tools/adb wait-for-device
~/Library/Android/sdk/platform-tools/adb shell \
  'while [[ "$(getprop sys.boot_completed | tr -d \r)" != "1" ]]; do sleep 2; done'

# 3. Push runner + KZG params.
ADB=~/Library/Android/sdk/platform-tools/adb
$ADB push target/aarch64-linux-android/release/bench-runner /data/local/tmp/
$ADB shell mkdir -p /data/local/tmp/midnight-pp
for f in bls_midnight_2p4 bls_midnight_2p5 bls_midnight_2p7 bls_midnight_2p8 \
         bls_midnight_2p10 bls_midnight_2p11; do
  $ADB push "$HOME/.cache/midnight/zk-params/$f" /data/local/tmp/midnight-pp/
done

# 4. Run.
$ADB shell 'MIDNIGHT_PP=/data/local/tmp/midnight-pp \
  BENCH_CACHE_DIR=/data/local/tmp/bench-cache \
  /data/local/tmp/bench-runner'
```

### Real device (Samsung S24 Ultra)

Same as the emulator path — plug the phone in, enable Developer Options +
USB debugging, accept the trust prompt, then steps 3 and 4 with the
phone's serial passed via `-s`. The binary is portable arm64-v8a; no
re-build needed.

```bash
adb -s <phone-serial> push target/aarch64-linux-android/release/bench-runner /data/local/tmp/
# … (same params + run as above, with `adb -s <phone-serial> shell …`)
```

### Library + HTTP tests (desktop)

```bash
MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" \
  cargo test -p prover-core --test library_path -- --nocapture

MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" \
  cargo test -p prover-core --features proof-server-http \
  --test http_path -- --nocapture

MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" \
  cargo bench -p prover-core --features bench --bench proofs -- \
  --warm-up-time 1 --measurement-time 5
```

### Dioxus desktop UI

```bash
MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" cargo run -p dioxus-bench
```
