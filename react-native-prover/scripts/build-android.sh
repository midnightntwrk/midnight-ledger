#!/usr/bin/env bash
# Build the Android side: cross-compile the Rust FFI for
# aarch64-linux-android, drop the .so into the Gradle module's
# jniLibs, and (if available) generate the Kotlin bindings.
#
# Requires:
#   - cargo-ndk + ANDROID_NDK_HOME exported
#   - `rustup target add aarch64-linux-android`
#   - `ubrn` (optional, for Kotlin/TS binding generation)
#
# Output:
#   android/src/main/jniLibs/arm64-v8a/libmidnight_prover_ffi.so
#   android/src/main/java/com/midnight/prover/  — generated Kotlin (via ubrn)

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." &> /dev/null && pwd)
CRATE_DIR="$REPO_ROOT/crates/prover-ffi"
TARGET_DIR="$REPO_ROOT/../target"
JNI_DIR="$REPO_ROOT/android/src/main/jniLibs/arm64-v8a"

: "${ANDROID_NDK_HOME:?need ANDROID_NDK_HOME set, e.g. ~/Library/Android/sdk/ndk/27.0.12077973}"

echo "[android] building libmidnight_prover_ffi.so"
echo "[android] NDK: $ANDROID_NDK_HOME"

# 1. Cross-build.
cargo ndk -t arm64-v8a build --release \
  --manifest-path "$CRATE_DIR/Cargo.toml"

# 2. Stage into jniLibs (overwrite any stale binary).
mkdir -p "$JNI_DIR"
SO_SRC="$TARGET_DIR/aarch64-linux-android/release/libmidnight_prover_ffi.so"
[ -f "$SO_SRC" ] || { echo "missing $SO_SRC"; exit 1; }
cp "$SO_SRC" "$JNI_DIR/"
echo "[android] staged $JNI_DIR/libmidnight_prover_ffi.so"

# 3. Generate Kotlin + TS bindings.
if command -v ubrn &> /dev/null; then
  echo "[android] generating Kotlin/TS bindings via ubrn"
  (cd "$REPO_ROOT" && ubrn build android --release --and-generate)
else
  echo "[android] WARN: 'ubrn' not on PATH — skipping binding generation."
  echo "[android]       install with: cargo install uniffi-bindgen-react-native"
  echo "[android]       then re-run this script."
fi

echo "[android] done"
