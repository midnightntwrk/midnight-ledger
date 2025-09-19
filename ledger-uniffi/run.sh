#!/bin/bash

# =============================================================================
# React Native UniFFI Build Script
# =============================================================================
# 
# WARNING: This script MUST generate NEW native libraries with updated content
#          for the React Native app to show updated data from Rust!
#
# This script will:
# 1. Set up Android NDK for ARM64 builds (PROPERLY CONFIGURED)
# 2. Build Rust library for Android ARM64 (MUST succeed)
# 3. Set up iOS toolchain for iOS builds
# 4. Build Rust library for iOS (device and simulator)
# 5. Generate new UniFFI bindings from updated Rust code
# 6. Build React Native app with new bindings
# 7. Deploy updated app to show new Rust data
# =============================================================================

set -e  # Exit on any error

# Check platform argument
PLATFORM=${1:-"both"}

echo "🚀 Building for platform: $PLATFORM"
echo "   Options: android, ios, both (default: both)"
echo ""

# =============================================================================
# ANDROID BUILD
# =============================================================================
if [ "$PLATFORM" = "android" ] || [ "$PLATFORM" = "both" ]; then
    echo "🤖 Building for Android..."
    echo "🔧 Setting up Android NDK for ARM64 builds..."
    # Step 1: Find and set up Android NDK properly
    NDK_PATH=""

    # Check common NDK locations
    if [ -d "$HOME/Library/Android/sdk/ndk" ]; then
        # Find the latest NDK version
        LATEST_NDK=$(ls -1 "$HOME/Library/Android/sdk/ndk" | sort -V | tail -1)
        NDK_PATH="$HOME/Library/Android/sdk/ndk/$LATEST_NDK"
        echo "✅ Found Android NDK at: $NDK_PATH"
    elif [ -n "$ANDROID_NDK_HOME" ]; then
        NDK_PATH="$ANDROID_NDK_HOME"
        echo "✅ Using ANDROID_NDK_HOME: $NDK_PATH"
    else
        echo "❌ No Android NDK found!"
        echo "   Please install Android NDK via Android Studio or download manually"
        exit 1
    fi

    # Step 2: Set up environment variables
    echo "🔧 Setting up environment variables..."
    export ANDROID_NDK_HOME="$NDK_PATH"
    export PATH="$NDK_PATH/toolchains/llvm/prebuilt/darwin-x86_64/bin:$PATH"

    # Step 3: Verify tools are available
    echo "🔧 Verifying Android NDK tools..."
    if command -v aarch64-linux-android21-clang >/dev/null 2>&1; then
        echo "✅ Found aarch64-linux-android21-clang: $(which aarch64-linux-android21-clang)"
    elif command -v aarch64-linux-android-clang >/dev/null 2>&1; then
        echo "✅ Found aarch64-linux-android-clang: $(which aarch64-linux-android-clang)"
    else
        echo "❌ No Android NDK clang found in PATH"
        echo "   PATH: $PATH"
        exit 1
    fi

    if command -v llvm-ar >/dev/null 2>&1; then
        echo "✅ Found llvm-ar: $(which llvm-ar)"
    else
        echo "❌ No llvm-ar found in PATH"
        exit 1
    fi

    # Add Rust target for Android ARM64
    echo "🎯 Adding Rust Android ARM64 target..."
    rustup target add aarch64-linux-android

    echo "🔨 Building Rust library for Android ARM64..."

    # Set environment variables for the build
    echo "🔧 Setting build environment variables..."
    export CC_aarch64_linux_android="$NDK_PATH/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android21-clang"
    export AR_aarch64_linux_android="$NDK_PATH/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-ar"
    export CFLAGS_aarch64_linux_android="-fPIC"
    export CXXFLAGS_aarch64_linux_android="-fPIC"

    # Build for Android ARM64
    echo "🚀 Building with cargo..."
    if cargo build --target aarch64-linux-android --release; then
        echo "✅ Cargo build succeeded!"
        
        # Check if new .so was created
        NEW_SO="target/aarch64-linux-android/release/libledger_uniffi.so"
        if [ -f "$NEW_SO" ]; then
            echo "✅ New .so file created at: $NEW_SO"
            echo "   Timestamp: $(ls -la "$NEW_SO" | awk '{print $6, $7, $8}')"
            echo "   Size: $(ls -lh "$NEW_SO" | awk '{print $5}')"
            echo ""
            
            # Copy to React Native library
            echo "📦 Copying new .so to React Native library..."
            cp "$NEW_SO" react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/
            echo "✅ New native library copied to React Native library!"
            echo "📅 New library timestamp: $(ls -la react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/libledger_uniffi.so | awk '{print $6, $7, $8}')"
        else
            echo "❌ FAILURE: No new .so file created"
            echo "   Expected location: $NEW_SO"
            echo "   This indicates a build failure despite cargo reporting success."
            exit 1
        fi
    else
        echo "❌ FAILURE: Cargo build failed"
        echo "   This indicates a deeper issue with the Android NDK setup."
        echo "   Please check your Android NDK installation and try again."
        exit 1
    fi
    echo "✅ Android build complete!"
    echo ""
fi

# =============================================================================
# iOS BUILD
# =============================================================================
if [ "$PLATFORM" = "ios" ] || [ "$PLATFORM" = "both" ]; then
    echo "🍎 Building for iOS..."
    echo "🔧 Setting up iOS toolchain..."
    
    # Check if Xcode is installed
    if ! command -v xcodebuild >/dev/null 2>&1; then
        echo "❌ Xcode not found!"
        echo "   Please install Xcode from the App Store"
        exit 1
    fi
    
    # Check if iOS targets are available
    echo "🎯 Adding Rust iOS targets..."
    rustup target add aarch64-apple-ios-sim
    
    echo "🔨 Building Rust library for iOS..."
    
    # Build for iOS simulator (ARM64 for Apple Silicon Macs)
    echo "🚀 Building for iOS simulator (aarch64-apple-ios-sim)..."
    if cargo build --target aarch64-apple-ios-sim --release; then
        echo "✅ iOS simulator ARM64 build succeeded!"
        
        # Check if new .a was created
        NEW_A_SIM_ARM64="target/aarch64-apple-ios-sim/release/libledger_uniffi.a"
        if [ -f "$NEW_A_SIM_ARM64" ]; then
            echo "✅ New .a file created at: $NEW_A_SIM_ARM64"
            echo "   Timestamp: $(ls -la "$NEW_A_SIM_ARM64" | awk '{print $6, $7, $8}')"
            echo "   Size: $(ls -lh "$NEW_A_SIM_ARM64" | awk '{print $5}')"
        else
            echo "❌ FAILURE: No new .a file created for iOS simulator ARM64"
            exit 1
        fi
    else
        echo "❌ FAILURE: iOS simulator ARM64 build failed"
        exit 1
    fi
    
    # Use the ARM64 library directly (no universal library needed)
    echo "🔧 Using ARM64 library for iOS Simulator..."
    UNIVERSAL_LIB="rn-demo-app/ios/build/ExpoLedgerModule/libledger_uniffi.a"
    mkdir -p "$(dirname "$UNIVERSAL_LIB")"
    
    # Copy the ARM64 library directly
    if cp "$NEW_A_SIM_ARM64" "$UNIVERSAL_LIB"; then
        echo "✅ ARM64 library copied to: $UNIVERSAL_LIB"
        echo "   Timestamp: $(ls -la "$UNIVERSAL_LIB" | awk '{print $6, $7, $8}')"
        echo "   Size: $(ls -lh "$UNIVERSAL_LIB" | awk '{print $5}')"
        echo "   Architecture: $(lipo -info "$UNIVERSAL_LIB")"
    else
        echo "❌ FAILURE: Failed to copy ARM64 library"
        exit 1
    fi
    
    # Copy library to React Native module
    echo "📦 Copying library to React Native module..."
    cp "$UNIVERSAL_LIB" "react-native-ledger-ffi/ios/"
    echo "✅ Library copied to react-native-ledger-ffi/ios/"
    
    # Copy library to Pods directory for immediate use
    echo "📦 Copying library to Pods directory..."
    mkdir -p "rn-demo-app/ios/Pods/react-native-ledger-ffi/ios"
    cp "$UNIVERSAL_LIB" "rn-demo-app/ios/Pods/react-native-ledger-ffi/ios/"
    echo "✅ Library copied to Pods directory"
    
    echo "✅ iOS build complete!"
    echo ""
fi

echo "📦 Generating new UniFFI bindings..."
# Generate fresh bindings from the current Rust code
# First, build release version for current platform to generate bindings
cargo build --release
echo "✅ Rust library built for bindings generation"

# Generate Kotlin bindings for Android
if [ "$PLATFORM" = "android" ] || [ "$PLATFORM" = "both" ]; then
    echo "🔧 Generating Kotlin bindings for Android..."
    # Clean up old bindings
    rm -rf react-native-ledger-ffi/android/src/main/kotlin/com/midnight/ledgerffi/uniffi
    # Generate Kotlin bindings using the release library
    cargo run --features=uniffi/cli --bin uniffi-bindgen generate \
        --library target/release/libledger_uniffi.dylib \
        --language kotlin \
        --out-dir react-native-ledger-ffi/android/src/main/kotlin/com/midnight/ledgerffi/uniffi
    echo "✅ Kotlin bindings generated successfully!"
fi

# Generate Swift bindings for iOS
if [ "$PLATFORM" = "ios" ] || [ "$PLATFORM" = "both" ]; then
    echo "🔧 Generating Swift bindings for iOS..."
    # Clean up old bindings
    rm -rf react-native-ledger-ffi/ios/LedgerUniffi
    rm -rf react-native-ledger-ffi/ios/ledger_uniffi.swift
    rm -rf react-native-ledger-ffi/ios/ledger_uniffiFFI.h
    rm -rf react-native-ledger-ffi/ios/ledger_uniffiFFI.modulemap
    # Generate Swift bindings using the release library
    cargo run --features=uniffi/cli --bin uniffi-bindgen generate \
        --library target/release/libledger_uniffi.dylib \
        --language swift \
        --out-dir react-native-ledger-ffi/ios
    echo "✅ Swift bindings generated successfully!"
fi

echo "📱 Checking native libraries..."

    # Check Android libraries
    if [ "$PLATFORM" = "android" ] || [ "$PLATFORM" = "both" ]; then
        echo "🤖 Checking Android libraries..."
        # Ensure the native library directory exists
        mkdir -p react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a

        # Check if the native libraries are present
        if [ -f react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/libledger_uniffi.so ]; then
            echo "✅ Found libledger_uniffi.so in React Native library"
            echo "📅 Library timestamp: $(ls -la react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/libledger_uniffi.so | awk '{print $6, $7, $8}')"
        else
            echo "❌ Error: libledger_uniffi.so not found in React Native library!"
            echo "   This should not happen if the build succeeded."
            exit 1
        fi

        if [ -f react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/libjnidispatch.so ]; then
            echo "✅ Found libjnidispatch.so in React Native library"
        else
            echo "⚠️  Warning: libjnidispatch.so not found in React Native library."
            echo "   Please download it from:"
            echo "   https://github.com/java-native-access/jna/tree/5.5.0/lib/native"
            echo "   Extract android-aarch64.jar and place libjnidispatch.so in:"
            echo "   react-native-ledger-ffi/android/src/main/jniLibs/arm64-v8a/"
        fi
    fi

# Check iOS libraries
if [ "$PLATFORM" = "ios" ] || [ "$PLATFORM" = "both" ]; then
    echo "🍎 Checking iOS libraries..."
    # Check if the universal library is present
    if [ -f rn-demo-app/ios/build/ExpoLedgerModule/libledger_uniffi.a ]; then
        echo "✅ Found libledger_uniffi.a (universal)"
        echo "📅 Library timestamp: $(ls -la rn-demo-app/ios/build/ExpoLedgerModule/libledger_uniffi.a | awk '{print $6, $7, $8}')"
    else
        echo "❌ Error: libledger_uniffi.a not found!"
        echo "   This should not happen if the build succeeded."
        exit 1
    fi
fi

echo "🚀 Building React Native demo app..."

# Build React Native library first
echo "📦 Building React Native library..."
cd react-native-ledger-ffi
npm run build
echo "🔨 Building React Native library Android module..."
cd android
./gradlew assembleRelease
cd ../..

# Install dependencies in demo app
echo "📦 Installing demo app dependencies..."
cd rn-demo-app
npm install
cd ..

# Build Android app
if [ "$PLATFORM" = "android" ] || [ "$PLATFORM" = "both" ]; then
    echo "🤖 Building Android app..."
    cd rn-demo-app/android
    echo "🧹 Cleaning build cache..."
    ./gradlew clean
    echo "🔨 Building with fresh cache..."
    ./gradlew assembleDebug
    cd ../..
    echo "✅ Android build complete!"
fi

# Build iOS app
if [ "$PLATFORM" = "ios" ] || [ "$PLATFORM" = "both" ]; then
    echo "🍎 Building iOS app..."
    cd rn-demo-app/ios
    # Always reinstall pods to ensure fresh state
    echo "📦 Installing CocoaPods dependencies..."
    pod install
    cd ..
    # Build the iOS app (use specific simulator destination)
    echo "🔨 Building iOS app with specific simulator..."
    npx react-native run-ios --simulator="iPhone 16" --no-packager
    cd ..
    echo "✅ iOS build complete!"
fi

echo ""
echo "✅ Build complete! To run the demo app:"
echo "   cd rn-demo-app"
if [ "$PLATFORM" = "android" ] || [ "$PLATFORM" = "both" ]; then
    echo "   npx react-native run-android"
fi
if [ "$PLATFORM" = "ios" ] || [ "$PLATFORM" = "both" ]; then
    echo "   npx react-native run-ios"
fi
echo ""
echo "🎯 The app will now show updated data from Rust functions!"
echo ""
echo "💡 Usage:"
echo "   ./run.sh          # Build both Android and iOS (default)"
echo "   ./run.sh android  # Build only Android"
echo "   ./run.sh ios      # Build only iOS"
echo "   ./run.sh both     # Build both platforms (explicit)"