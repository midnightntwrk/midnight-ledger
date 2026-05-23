#!/usr/bin/env bash
# Wrap the Rust FFI .a / .dylib artefacts into a MidnightProver
# .xcframework that iOS hosts (CocoaPods, SwiftPM, Xcode) can
# consume directly. Idempotent.
#
# Requires:
#   - `xcodebuild` (Xcode 15+)
#   - `cargo` + `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`
#   - `ubrn` (uniffi-bindgen-react-native CLI) installed
#
# Output:
#   ios/MidnightProver.xcframework  — the binary slice
#   src/native/midnight_prover.ts   — the generated TS bindings (via ubrn)

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." &> /dev/null && pwd)
CRATE_DIR="$REPO_ROOT/crates/prover-ffi"
TARGET_DIR="$REPO_ROOT/../target"  # workspace target dir
XCF_OUT="$REPO_ROOT/ios/MidnightProver.xcframework"

echo "[ios] building xcframework"

# 1. Make sure the Rust artefacts exist.
bash "$SCRIPT_DIR/build-rust.sh"

# 2. Wipe the old xcframework.
rm -rf "$XCF_OUT"

# 3. Assemble the new one. `xcodebuild -create-xcframework` needs
#    one `-library` per slice; we ship arm64-device + arm64-sim.
DEVICE_LIB="$TARGET_DIR/aarch64-apple-ios/release/libmidnight_prover_ffi.a"
SIM_LIB="$TARGET_DIR/aarch64-apple-ios-sim/release/libmidnight_prover_ffi.a"

[ -f "$DEVICE_LIB" ] || { echo "missing $DEVICE_LIB"; exit 1; }
[ -f "$SIM_LIB" ]    || { echo "missing $SIM_LIB"; exit 1; }

xcodebuild -create-xcframework \
  -library "$DEVICE_LIB" \
  -library "$SIM_LIB" \
  -output "$XCF_OUT"

echo "[ios] xcframework written to $XCF_OUT"

# 4. Generate the Swift + TS bindings from the UDL.
if command -v ubrn &> /dev/null; then
  echo "[ios] generating Swift/TS bindings via ubrn"
  (cd "$REPO_ROOT" && ubrn build ios --release --and-generate)
else
  echo "[ios] WARN: 'ubrn' not on PATH — skipping binding generation."
  echo "[ios]       install with: cargo install uniffi-bindgen-react-native"
  echo "[ios]       then re-run this script."
fi

echo "[ios] done"
