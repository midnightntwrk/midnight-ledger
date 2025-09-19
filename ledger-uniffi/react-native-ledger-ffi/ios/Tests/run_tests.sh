#!/bin/bash

# Swift Test Runner for react-native-ledger-ffi iOS Module
# This script runs the Swift tests for the hello function

echo "🧪 Running Swift tests for react-native-ledger-ffi iOS module..."

# Set up the test environment
export SWIFT_MODULE_PATH="$(pwd)/.."
export LIBRARY_PATH="$(pwd)/../libledger_uniffi.a"

# Check if we're in the right directory
if [ ! -f "../libledger_uniffi.a" ]; then
    echo "❌ Error: libledger_uniffi.a not found in parent directory"
    echo "   Please run the build script first: ../../run.sh ios"
    exit 1
fi

# Check if UniFFI bindings exist
if [ ! -f "../ledger_uniffi.swift" ]; then
    echo "❌ Error: UniFFI Swift bindings not found"
    echo "   Please run the build script first: ../../run.sh ios"
    exit 1
fi

echo "✅ Test environment ready"
echo "📁 Library: $(ls -lh ../libledger_uniffi.a | awk '{print $5}')"
echo "📁 Bindings: $(ls -lh ../ledger_uniffi.swift | awk '{print $5}')"

# For now, we'll just validate the test files exist
echo ""
echo "📋 Test files found:"
ls -la *.swift

echo ""
echo "✅ Test files are ready"
echo ""
echo "To run the tests in Xcode:"
echo "1. Open the project in Xcode"
echo "2. Create a new iOS Unit Test target"
echo "3. Add the test files to the target"
echo "4. Link against libledger_uniffi.a"
echo "5. Set up the module map for UniFFI bindings"
echo "6. Run the tests (Cmd+U)"
echo ""
echo "Or run manually with:"
echo "swift test --package-path ."
