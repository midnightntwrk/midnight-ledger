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
RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--import-memory -C link-arg=--max-memory=4294967296 -C link-arg=--initial-memory=8388608 -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base' \
  wasm-pack build --release --target web --out-dir web/pkg . \
    -- -Z build-std=panic_abort,std

# Post-build patch: wasm-bindgen-rayon's `workerHelpers.js` does
# `await import('../../..')` which assumes a bundler will
# resolve that to `pkg/<crate>.js` via `package.json#main`. With
# plain `--target web` (no bundler) the browser fetches the
# directory itself and the server returns HTML, breaking the
# module load. Rewrite the import to the explicit filename.
HELPER=$(find web/pkg/snippets -name 'workerHelpers.js' -print -quit)
if [[ -n "$HELPER" ]]; then
  sed -i.bak "s|import('../../..')|import('../../../contract_benchmark_wasm.js')|" "$HELPER"
  rm -f "$HELPER.bak"
  echo "patched $HELPER to import the explicit JS path"
fi

echo
echo "wasm pkg written to web/pkg/"
echo "now: cd web && python3 serve.py 8080"
