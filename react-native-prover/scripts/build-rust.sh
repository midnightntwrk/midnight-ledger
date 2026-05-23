#!/usr/bin/env bash
# Build the Rust FFI crate for ALL supported target triples.
# Idempotent — calling this twice with no source changes is a no-op
# beyond the cargo fingerprint check.
#
# After this script: invoke build-ios.sh / build-android.sh to wrap
# the artefacts into the platform-native packaging.

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." &> /dev/null && pwd)
CRATE_DIR="$REPO_ROOT/crates/prover-ffi"

echo "[rust] building midnight-prover-ffi for all platforms"
echo "[rust] crate: $CRATE_DIR"

# Host first — fastest, doubles as a sanity check.
cargo build --release --manifest-path "$CRATE_DIR/Cargo.toml"
echo "[rust] host build OK"

# iOS device + simulator.
if rustup target list --installed | grep -q "aarch64-apple-ios"; then
  cargo build --release --target aarch64-apple-ios \
    --manifest-path "$CRATE_DIR/Cargo.toml"
  echo "[rust] aarch64-apple-ios OK"
else
  echo "[rust] WARN: aarch64-apple-ios target not installed (skipping)"
fi

if rustup target list --installed | grep -q "aarch64-apple-ios-sim"; then
  cargo build --release --target aarch64-apple-ios-sim \
    --manifest-path "$CRATE_DIR/Cargo.toml"
  echo "[rust] aarch64-apple-ios-sim OK"
else
  echo "[rust] WARN: aarch64-apple-ios-sim target not installed (skipping)"
fi

# Android arm64 (the only ABI we ship — emulator-only x86_64 is
# intentionally excluded per the architecture doc §13.3).
if command -v cargo-ndk &> /dev/null && rustup target list --installed | grep -q "aarch64-linux-android"; then
  : "${ANDROID_NDK_HOME:?need ANDROID_NDK_HOME set for the android build}"
  cargo ndk -t arm64-v8a build --release \
    --manifest-path "$CRATE_DIR/Cargo.toml"
  echo "[rust] aarch64-linux-android OK"
else
  echo "[rust] WARN: cargo-ndk or aarch64-linux-android target missing (skipping)"
fi

echo "[rust] all targets done"
