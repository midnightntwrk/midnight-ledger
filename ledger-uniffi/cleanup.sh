#!/bin/bash

# =============================================================================
# UniFFI Generated Files Cleanup Script
# =============================================================================
# 
# This script removes all generated UniFFI bindings and native libraries
# to ensure fresh generation without any caching issues.
#
# Usage: ./cleanup-generated.sh
# =============================================================================

set -e  # Exit on any error

echo "🧹 Cleaning up generated UniFFI files..."

# Clean up Android generated files
echo "🤖 Cleaning Android generated files..."
rm -rf react-native-ledger-ffi/android/src/main/kotlin/com/midnight/ledgerffi/uniffi/
rm -rf react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/libledger_uniffi.so
rm -rf react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/libjnidispatch.so
rm -rf react-native-ledger-ffi/android/build/
rm -rf react-native-ledger-ffi/android/.gradle/
rm -rf react-native-ledger-ffi/android/local.properties

# Clean up iOS generated files
echo "🍎 Cleaning iOS generated files..."
rm -rf react-native-ledger-ffi/ios/LedgerUniffi/
rm -rf react-native-ledger-ffi/ios/ledger_uniffi.swift
rm -rf react-native-ledger-ffi/ios/ledger_uniffiFFI.h
rm -rf react-native-ledger-ffi/ios/ledger_uniffiFFI.modulemap
rm -rf react-native-ledger-ffi/ios/libledger_uniffi.a
rm -rf rn-demo-app/ios/build/ExpoLedgerModule/libledger_uniffi.a
rm -rf rn-demo-app/ios/Pods/react-native-ledger-ffi/ios/libledger_uniffi.a

# Clean up Rust build artifacts
echo "🦀 Cleaning Rust build artifacts..."
rm -rf target/aarch64-linux-android/release/libledger_uniffi.so
rm -rf target/aarch64-apple-ios-sim/release/libledger_uniffi.a
rm -rf target/release/libledger_uniffi.dylib
rm -rf target/debug/libledger_uniffi.dylib
rm -rf target/debug/libledger_uniffi.a
rm -rf target/release/libledger_uniffi.a

# Clean up React Native build artifacts
echo "📱 Cleaning React Native build artifacts..."
rm -rf rn-demo-app/android/build/
rm -rf rn-demo-app/android/app/build/
rm -rf rn-demo-app/android/.gradle/
rm -rf rn-demo-app/android/local.properties
rm -rf rn-demo-app/ios/build/
rm -rf rn-demo-app/ios/Pods/
rm -rf rn-demo-app/ios/Podfile.lock
rm -rf rn-demo-app/react-native-ledger-ffi
rm -rf react-native-ledger-ffi/dist/
rm -rf react-native-ledger-ffi/*.tsbuildinfo
rm -rf react-native-ledger-ffi/android/build/
rm -rf react-native-ledger-ffi/android/.gradle/
rm -rf react-native-ledger-ffi/android/local.properties

echo "✅ Cleanup complete!"
echo ""
echo "💡 Next steps:"
echo "   1. Run ./run.sh to regenerate all bindings and libraries"
echo "   2. Or run ./run.sh android for Android only"
echo "   3. Or run ./run.sh ios for iOS only"
echo ""
