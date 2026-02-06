#!/bin/bash

# Download JNA native library for Android aarch64
JNA_VERSION="5.14.0"
ARCH="aarch64"
OUTPUT_DIR="android/app/src/main/jniLibs/aarch64-v8a"

echo "Downloading JNA native library for Android..."

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Download the JNA native library
curl -L "https://repo1.maven.org/maven2/net/java/dev/jna/jna/$JNA_VERSION/jna-$JNA_VERSION.jar" -o jna.jar

# Extract the native library
unzip -j jna.jar "com/sun/jna/android-$ARCH/libjnidispatch.so" -d "$OUTPUT_DIR"

# Clean up
rm jna.jar

echo "JNA native library downloaded to $OUTPUT_DIR"
ls -la "$OUTPUT_DIR"
