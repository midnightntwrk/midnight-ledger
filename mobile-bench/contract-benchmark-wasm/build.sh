#!/usr/bin/env bash
# Build the contract-benchmark-wasm crate for `wasm32-unknown-unknown`
# with `wasm-bindgen-rayon` threading enabled.
#
# Requirements:
# - `wasm-pack` (`brew install wasm-pack` or
#   `cargo install wasm-pack`).
# - Homebrew LLVM (default clang on macOS lacks the wasm32
#   target). `brew install llvm` if missing.
# - `wasm32-unknown-unknown` target installed
#   (`rustup target add wasm32-unknown-unknown`).
#
# Outputs `web/pkg/` (gitignored). Pair with `web/serve.py` to
# run the bench in a browser.
#
# Threading mechanics:
# - `RUSTC_BOOTSTRAP=1` opens nightly cargo flags on stable.
# - `-C target-feature=+atomics,+bulk-memory` enables wasm
#   threads + bulk-memory ops.
# - `-Z build-std=panic_abort,std` recompiles std with the
#   above target feature set.
# - The matching runtime requirement — `SharedArrayBuffer` —
#   needs the page served with COOP/COEP headers (handled by
#   `web/serve.py`).

set -euo pipefail

LLVM_PREFIX="${LLVM_PREFIX:-$(brew --prefix llvm)}"

cd "$(dirname "$0")"

CC="$LLVM_PREFIX/bin/clang" \
AR="$LLVM_PREFIX/bin/llvm-ar" \
RUSTC_BOOTSTRAP=1 \
RUSTFLAGS='-C target-feature=+atomics,+bulk-memory' \
  wasm-pack build --release --target web --out-dir web/pkg . \
    -- -Z build-std=panic_abort,std

echo
echo "wasm pkg written to web/pkg/"
echo "now: cd web && python3 serve.py 8080"
