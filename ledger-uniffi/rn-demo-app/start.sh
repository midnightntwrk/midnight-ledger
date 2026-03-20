#!/usr/bin/env bash
set -euo pipefail

# RN Demo App bootstrapper
# - Clears common caches (Metro, Watchman tmp)
# - Regenerates native artifacts (iOS xcframework + Swift, Android JNI + Kotlin)
# - Installs deps, prebuilds native projects
# - Starts the app on the chosen platform (default: ios)
#
# Usage:
#   ./start.sh [ios|android]
#
# Notes:
# - Requires Rust toolchain for regeneration steps.
# - iOS requires CocoaPods; Android requires Android SDKs.

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
CRATE_DIR=$(cd "$SCRIPT_DIR/.." && pwd)                # ledger-uniffi
RN_PKG_DIR="$CRATE_DIR/react-native-ledger-ffi"
PLATFORM="${1:-ios}"   # default to iOS

banner() { echo; echo "==> $*"; }

clear_caches() {
  banner "Clearing Metro/Watchman caches"
  if command -v watchman >/dev/null 2>&1; then
    watchman watch-del-all || true
  fi
  # Metro / haste map caches (best-effort)
  rm -rf "${TMPDIR:-/tmp}"/metro-* || true
  rm -rf "${TMPDIR:-/tmp}"/haste-map-* || true
  rm -rf "$SCRIPT_DIR/.expo" || true
  
  # Clear iOS build artifacts
  banner "Clearing iOS build artifacts"
  rm -rf "$SCRIPT_DIR/ios/Pods" || true
  rm -rf "$SCRIPT_DIR/ios/build" || true
  rm -rf "$SCRIPT_DIR/ios/RNDemoApp.xcworkspace" || true
  
  # Clear Android build artifacts
  banner "Clearing Android build artifacts"
  rm -rf "$SCRIPT_DIR/android/.gradle" || true
  rm -rf "$SCRIPT_DIR/android/app/build" || true
  rm -rf "$SCRIPT_DIR/android/build" || true
}

regenerate_native() {
  banner "Regenerating native artifacts (iOS + Android)"
  (cd "$CRATE_DIR" && ./scripts/regenerate-all.sh)
  # Ensure RN package copies/updates any generated sources explicitly as well
  if [ -d "$RN_PKG_DIR" ]; then
    banner "Generating iOS native modules"
    (cd "$RN_PKG_DIR" && npm run gen:ios || true)
    banner "Generating Android native modules"
    (cd "$RN_PKG_DIR" && npm run gen:android || true)
    banner "Building React Native package"
    (cd "$RN_PKG_DIR" && npm run build || true)
  fi
}

prepare_app() {
  banner "Installing app dependencies"
  (cd "$SCRIPT_DIR" && npm install)
  
  # Clean install to ensure fresh dependencies
  banner "Cleaning node_modules and reinstalling"
  rm -rf "$SCRIPT_DIR/node_modules" || true
  rm -rf "$SCRIPT_DIR/package-lock.json" || true
  (cd "$SCRIPT_DIR" && npm install)
  
  banner "Prebuilding native projects (Expo)"
  (cd "$SCRIPT_DIR" && npm run prebuild)
}

run_ios() {
  banner "Running iOS app"
  (cd "$SCRIPT_DIR" && npm run ios)
}

run_android() {
  banner "Running Android app"
  (cd "$SCRIPT_DIR" && npm run android)
}

main() {
  case "$PLATFORM" in
    ios|android) ;;
    *) echo "Unknown platform: $PLATFORM (expected: ios|android)" >&2; exit 1 ;;
  esac

  clear_caches
  regenerate_native
  prepare_app

  # Verify native modules are properly linked
  banner "Verifying native module linking"
  if [ "$PLATFORM" = "ios" ]; then
    if [ -d "$SCRIPT_DIR/ios/Pods" ]; then
      echo "✅ iOS Pods directory found - native modules should be linked"
    else
      echo "❌ iOS Pods directory missing - linking may have failed"
    fi
    run_ios
  else
    if [ -d "$SCRIPT_DIR/android/app/src/main/java/com/midnight/ledgerffi" ]; then
      echo "✅ Android native modules found - linking should be successful"
    else
      echo "❌ Android native modules missing - linking may have failed"
    fi
    run_android
  fi
}

main "$@"
