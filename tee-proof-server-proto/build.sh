#!/bin/bash
# Build script for Midnight Proof Server

set -e

echo "🔨 Building Midnight Proof Server..."
echo ""

cd "$(dirname "$0")/proof-server"

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Error: cargo is not installed"
    echo "   Install Rust from: https://rustup.rs/"
    exit 1
fi

# Build in release mode
echo "📦 Running: cargo build --release"
cargo build --release

echo ""
echo "✅ Build successful!"
echo ""
echo "Binary location:"
echo "  $(pwd)/target/release/midnight-proof-server-prototype"
echo ""
echo "Quick start:"
echo "  # Development mode (no auth)"
echo "  ./target/release/midnight-proof-server-prototype --disable-auth"
echo ""
echo "  # Production mode (with auth)"
echo "  export API_KEY=\"\$(openssl rand -hex 32)\""
echo "  ./target/release/midnight-proof-server-prototype --api-key \"\$API_KEY\""
echo ""
