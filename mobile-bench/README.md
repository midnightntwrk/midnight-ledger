# mobile-bench

A workspace for measuring Midnight ZK proof latency on mobile (Android emulator
+ Samsung S24 Ultra) and desktop. See
`docs/superpowers/specs/2026-04-28-mobile-proof-bench-design.md` for the
design and `docs/superpowers/plans/2026-04-28-mobile-proof-bench.md` for the
iteration-1 implementation plan.

Layout:

- `prover-core/` — embeddable Rust crate that wraps Midnight's proving
  primitives. Used by both the dioxus-bench app and `cargo test` /
  `cargo bench`.
- `fixtures/` — vendored zkir test artifacts (kept for future use; the
  iteration-1 zkir example uses an inline raw-IR string instead, see
  `prover-core/src/zkir_example.rs`).
- `scripts/setup-android-toolchain.sh` — one-shot macOS toolchain installer
  for desktop + Android (emulator and device) builds.

## Cross-compile notes

The full proving stack (`midnight-zk-stdlib`, `transient-crypto`, `ledger`,
`zswap`, `zkir`, etc.) cross-compiles cleanly to `aarch64-linux-android`
with NDK r27 and `cargo-ndk`. Tested on macOS aarch64 against
`/Users/ysh/Library/Android/sdk/ndk/27.0.12077973`.

To repro:

```bash
ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/27.0.12077973 \
  cargo ndk -t arm64-v8a build -p prover-core --release
```

Output: `target/aarch64-linux-android/release/libprover_core.rlib`.

No C/CMake adapter flags or feature toggles were required at this stage.
Build time from cold: ~1m 15s on Apple Silicon. The `proof-server-http`
feature is desktop-only (cfg-gated) — leave it off for Android builds.
