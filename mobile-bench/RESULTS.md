# mobile-bench iteration-1 results

Captured 2026-04-28 on Apple Silicon (M-series). Surfaces marked `…` are
pending — they require manual setup (running emulator, plugged-in S24 Ultra)
the autonomous run-through could not perform.

## Circuit

Iteration-1 exercises `zkir-minimal-assert` — a 1-input `assert(cond == 0)`
circuit at `k=4`, mirroring `zkir/tests/proofs.rs::test_minimal_proof`. The
fallible/count fixtures are vendored under `mobile-bench/fixtures/` for future
use; they require a real `communications_commitment` derived from contract
simulation and so are not exercised this iteration.

## Latency snapshot

| Surface                      | Path     | Prove time | Verify time | Proof bytes |
|------------------------------|----------|-----------:|------------:|------------:|
| macOS desktop (debug)        | library  | 33.5 ms    | 14.7 ms     | 2549 B      |
| macOS desktop (debug)        | http     | 24.2 ms    | n/a (server-side) | 2549 B |
| macOS desktop (release bench)| library  | 41.5 ms ¹  | (skipped)   | 2549 B      |
| Android emulator (arm64-v8a) | library  | …          | …           | …           |
| Samsung S24 Ultra            | library  | …          | …           | …           |

¹ Criterion mean across 100 samples (`cargo bench -p prover-core --features bench`).
The release-mode number is higher than the debug `prove_zkir_example` test
because each iteration includes keygen + prove (no setup phase).

## What's been validated

- ✅ Library path: `prove_zkir_example` end-to-end (load IR → keygen →
  `ProofPreimage::prove` → `VerifierKey::verify`) returns `verified=true` on
  desktop debug and release.
- ✅ HTTP path: in-process `midnight-proof-server` reachable at a random port,
  POST `/prove` round-trip with a `(ProofPreimageVersioned,
  Some(ProvingKeyMaterial), None)` payload, response deserializes as
  `ProofVersioned::V2(Proof)`, verifies locally with the same `VerifierKey`.
- ✅ Android cross-compile gate: full proving stack
  (`midnight-zk-stdlib`, `transient-crypto`, `ledger`, `zswap`, `zkir`)
  compiles cleanly to `aarch64-linux-android` with NDK r27 + `cargo-ndk`,
  ~1m 15s cold. No C/CMake patches required.
- ✅ Dioxus desktop scaffold (`mobile-bench/dioxus-bench`) builds clean with
  the default desktop feature set.

## What still needs a human

- ❌ Live UI smoke-test: `dx serve --platform desktop` requires
  dioxus-cli **0.6.x** on PATH — `cargo install dioxus-cli --version "^0.6"`.
  The 0.7.x CLI panics on this workspace via a `krates` cargo-metadata bug
  ("resolved a dependency for a dependency not specified by the crate").
  As a workaround you can run the desktop UI directly via
  `cargo run -p dioxus-bench` once dioxus-cli@0.6 is installed; the binary
  builds clean today.
- ❌ Dust-spend proving (`prove_dust_spend`): deferred. Building a valid
  `ProofPreimage` for a Dust spend requires reproducing the wallet
  state-machine (DustState, secret keys, UTXOs, kernel transcript). The
  plan as written contained `todo!()` placeholders here; until those are
  replaced with real wallet glue this circuit is not ready.
- ❌ Android emulator latency: needs `emulator -avd midnight_bench_arm64_api34 &`
  followed by `dx serve --platform android` (or `dx bundle --platform android`).
- ❌ Samsung S24 Ultra latency: needs the device plugged in over USB with
  Developer Options + USB debugging enabled, then `dx bundle --platform
  android --release` and `adb install -r …`.

## Reproducing the desktop numbers

```bash
# Library path
MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" \
  cargo test -p prover-core --test library_path -- --nocapture

# HTTP path
MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" \
  cargo test -p prover-core --features proof-server-http \
  --test http_path -- --nocapture

# Bench
MIDNIGHT_PP="$HOME/.cache/midnight/zk-params" \
  cargo bench -p prover-core --features bench --bench proofs -- \
  --warm-up-time 1 --measurement-time 5

# Android cross-compile gate (no emulator required)
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a build -p prover-core --release
```
