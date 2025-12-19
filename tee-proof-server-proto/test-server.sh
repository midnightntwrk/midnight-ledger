#!/bin/bash
# Test script for Midnight Proof Server ()

echo "======================================"
echo "Testing Midnight Proof Server (Prototype)"
echo "======================================"
echo ""

cd "$(dirname "$0")/proof-server" || exit 1

# Check if binary exists
if [ ! -f "./target/release/midnight-proof-server-prototype" ]; then
    echo "❌ Binary not found. Run ./build.sh first."
    exit 1
fi

echo "✅ Binary found"
echo ""

# Test 1: Run without arguments (should show error)
echo "Test 1: Running without arguments (should require API key)..."
./target/release/midnight-proof-server-prototype 2>&1 | head -5
echo ""

# Test 2: Show help
echo "Test 2: Showing help..."
./target/release/midnight-proof-server-prototype --help | head -15
echo ""

# Test 3: Start server in background
echo "Test 3: Starting server with --disable-auth..."
./target/release/midnight-proof-server-prototype --disable-auth --port 6301 > /tmp/prototype-server.log 2>&1 &
SERVER_PID=$!
echo "Server started with PID: $SERVER_PID"
sleep 2

# Test 4: Health check
echo ""
echo "Test 4: Testing /health endpoint..."
curl -s http://localhost:6301/health | jq
echo ""

# Test 5: Ready check
echo "Test 5: Testing /ready endpoint..."
curl -s http://localhost:6301/ready | jq
echo ""

# Test 6: Version
echo "Test 6: Testing /version endpoint..."
curl -s http://localhost:6301/version
echo ""
echo ""

# Test 7: Protected endpoint without auth (should fail)
echo "Test 7: Testing /check without API key (should fail)..."
curl -s -w "HTTP Status: %{http_code}\n" -X POST http://localhost:6301/check --data "test" 2>&1 | tail -1
echo ""

# Show server logs
echo "Server logs:"
echo "----------------------------------------"
head -20 /tmp/axum-server.log
echo "----------------------------------------"
echo ""

# Cleanup
echo "Stopping server (PID: $SERVER_PID)..."
kill $SERVER_PID 2>/dev/null
sleep 1

echo ""
echo "✅ All tests completed!"
echo ""
echo "To run the server normally:"
echo "  cd proof-server"
echo "  ./target/release/midnight-proof-server-prototype --disable-auth"
echo ""
