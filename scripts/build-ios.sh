#!/bin/bash
# This file is part of midnight-ledger.
# Copyright (C) 2025 Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
#
# Build script for iOS targets
# Compiles the ledger-ios crate for all iOS architectures and creates an XCFramework

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
IOS_OUT="$PROJECT_ROOT/ledger-ios/out"
EXPO_MODULE="$PROJECT_ROOT/expo-midnight-ledger"
PACKAGE_NAME="midnight-ledger-ios"
LIB_NAME="libmidnight_ledger_ios"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

echo_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

echo_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check for required tools
check_requirements() {
    echo_info "Checking requirements..."

    if ! command -v rustup &> /dev/null; then
        echo_error "rustup is not installed. Please install Rust first."
        exit 1
    fi

    if ! command -v cargo &> /dev/null; then
        echo_error "cargo is not installed. Please install Rust first."
        exit 1
    fi

    if ! command -v xcrun &> /dev/null; then
        echo_error "xcrun is not installed. Please install Xcode Command Line Tools."
        exit 1
    fi

    if ! command -v lipo &> /dev/null; then
        echo_error "lipo is not installed. Please install Xcode Command Line Tools."
        exit 1
    fi

    # uniffi-bindgen is built from the ledger-ios crate (see generate_bindings)
    echo_info "All requirements satisfied."
}

# Install Rust iOS targets
install_targets() {
    echo_info "Installing Rust iOS targets..."

    rustup target add aarch64-apple-ios || true
    rustup target add aarch64-apple-ios-sim || true
    rustup target add x86_64-apple-ios || true

    echo_info "Rust iOS targets installed."
}

# Build for a specific target
build_target() {
    local TARGET=$1
    local SDK=$2
    local ARCH=$3

    echo_info "Building for $TARGET ($SDK)..."

    export SDKROOT=$(xcrun --sdk $SDK --show-sdk-path)
    export CC=$(xcrun --sdk $SDK --find clang)
    export AR=$(xcrun --sdk $SDK --find ar)

    # Set CFLAGS for the blst crate (BLS12-381 crypto)
    export CFLAGS="-isysroot $SDKROOT"

    # For simulator targets, we need to specify the target triple
    if [[ "$SDK" == "iphonesimulator" ]]; then
        export CFLAGS="$CFLAGS -target $ARCH-apple-ios-simulator"
    else
        export CFLAGS="$CFLAGS -target $ARCH-apple-ios"
    fi

    cd "$PROJECT_ROOT"
    cargo build \
        --package "$PACKAGE_NAME" \
        --target "$TARGET" \
        --profile ios \
        --locked

    echo_info "Build complete for $TARGET"
}

# Generate Swift bindings
generate_bindings() {
    echo_info "Generating Swift bindings..."

    mkdir -p "$IOS_OUT/swift"
    mkdir -p "$IOS_OUT/headers"

    cd "$PROJECT_ROOT"

    # Generate Swift bindings using uniffi-bindgen from the package
    cargo run -p "$PACKAGE_NAME" --bin uniffi-bindgen generate \
        "ledger-ios/src/ledger_ios.udl" \
        --language swift \
        --out-dir "$IOS_OUT/swift"

    # Copy the generated header to headers directory
    if [ -f "$IOS_OUT/swift/ledger_iosFFI.h" ]; then
        cp "$IOS_OUT/swift/ledger_iosFFI.h" "$IOS_OUT/headers/"
    fi

    # Create a module map for the framework
    cat > "$IOS_OUT/headers/module.modulemap" << 'EOF'
framework module MidnightLedger {
    umbrella header "ledger_iosFFI.h"
    export *
    module * { export * }
}
EOF

    echo_info "Swift bindings generated."
}

# Create XCFramework
create_xcframework() {
    echo_info "Creating XCFramework..."

    local DEVICE_LIB="$PROJECT_ROOT/target/aarch64-apple-ios/ios/${LIB_NAME}.a"
    local SIM_ARM_LIB="$PROJECT_ROOT/target/aarch64-apple-ios-sim/ios/${LIB_NAME}.a"
    local SIM_X86_LIB="$PROJECT_ROOT/target/x86_64-apple-ios/ios/${LIB_NAME}.a"
    local SIM_FAT_LIB="$IOS_OUT/${LIB_NAME}.a"

    # Check if all libraries exist
    if [ ! -f "$DEVICE_LIB" ]; then
        echo_error "Device library not found: $DEVICE_LIB"
        exit 1
    fi

    if [ ! -f "$SIM_ARM_LIB" ]; then
        echo_error "Simulator ARM library not found: $SIM_ARM_LIB"
        exit 1
    fi

    if [ ! -f "$SIM_X86_LIB" ]; then
        echo_error "Simulator x86_64 library not found: $SIM_X86_LIB"
        exit 1
    fi

    # Create fat library for simulators (arm64 + x86_64)
    echo_info "Creating fat library for simulators..."
    lipo -create \
        "$SIM_ARM_LIB" \
        "$SIM_X86_LIB" \
        -output "$SIM_FAT_LIB"

    # Remove existing XCFramework if it exists
    rm -rf "$IOS_OUT/MidnightLedger.xcframework"

    # Create XCFramework
    echo_info "Creating XCFramework..."
    xcodebuild -create-xcframework \
        -library "$DEVICE_LIB" \
        -headers "$IOS_OUT/headers" \
        -library "$SIM_FAT_LIB" \
        -headers "$IOS_OUT/headers" \
        -output "$IOS_OUT/MidnightLedger.xcframework"

    echo_info "XCFramework created at $IOS_OUT/MidnightLedger.xcframework"
}

# Copy artifacts to expo module
copy_to_expo() {
    echo_info "Copying artifacts to expo-midnight-ledger..."

    # Check if expo module exists
    if [ ! -d "$EXPO_MODULE" ]; then
        echo_warn "Expo module not found at $EXPO_MODULE, skipping copy."
        return
    fi

    # Create Frameworks directory if it doesn't exist
    mkdir -p "$EXPO_MODULE/ios/Frameworks"

    # Remove old XCFramework and copy new one
    rm -rf "$EXPO_MODULE/ios/Frameworks/MidnightLedger.xcframework"
    cp -r "$IOS_OUT/MidnightLedger.xcframework" "$EXPO_MODULE/ios/Frameworks/"
    echo_info "  - Copied XCFramework"

    # Copy Swift bindings
    if [ -f "$IOS_OUT/swift/ledger_ios.swift" ]; then
        cp "$IOS_OUT/swift/ledger_ios.swift" "$EXPO_MODULE/ios/"
        echo_info "  - Copied ledger_ios.swift"
    fi

    # Copy header file (needed for Pods symlink)
    if [ -f "$IOS_OUT/swift/ledger_iosFFI.h" ]; then
        cp "$IOS_OUT/swift/ledger_iosFFI.h" "$EXPO_MODULE/ios/"
        echo_info "  - Copied ledger_iosFFI.h"
    fi

    # Copy modulemap if needed
    if [ -f "$IOS_OUT/swift/ledger_iosFFI.modulemap" ]; then
        cp "$IOS_OUT/swift/ledger_iosFFI.modulemap" "$EXPO_MODULE/ios/"
        echo_info "  - Copied ledger_iosFFI.modulemap"
    fi

    echo_info "Artifacts copied to expo module."
}

# Clean build artifacts
clean() {
    echo_info "Cleaning build artifacts..."
    rm -rf "$IOS_OUT"
    echo_info "Clean complete."
}

# Main build process
main() {
    local COMMAND=${1:-build}

    case $COMMAND in
        clean)
            clean
            ;;
        build)
            check_requirements
            install_targets

            mkdir -p "$IOS_OUT"

            # Build for all targets
            build_target "aarch64-apple-ios" "iphoneos" "arm64"
            build_target "aarch64-apple-ios-sim" "iphonesimulator" "arm64"
            build_target "x86_64-apple-ios" "iphonesimulator" "x86_64"

            # Generate bindings
            generate_bindings

            # Create XCFramework
            create_xcframework

            # Copy to expo module
            copy_to_expo

            echo_info "Build complete!"
            echo_info "Output:"
            echo_info "  - XCFramework: $IOS_OUT/MidnightLedger.xcframework"
            echo_info "  - Swift bindings: $IOS_OUT/swift/"
            echo_info "  - Expo module: $EXPO_MODULE/ios/"
            ;;
        bindings)
            generate_bindings
            copy_to_expo
            ;;
        copy)
            copy_to_expo
            ;;
        *)
            echo "Usage: $0 [build|clean|bindings|copy]"
            echo ""
            echo "Commands:"
            echo "  build    - Build the library for all iOS targets (default)"
            echo "  clean    - Clean build artifacts"
            echo "  bindings - Generate Swift bindings only and copy to expo module"
            echo "  copy     - Copy existing artifacts to expo module"
            exit 1
            ;;
    esac
}

main "$@"
