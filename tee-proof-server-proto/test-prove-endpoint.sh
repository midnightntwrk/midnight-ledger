#!/bin/bash
# Test the /prove endpoint with various inputs

cd "$(dirname "$0")/proof-server" || exit 1

echo "Starting server..."
./target/release/midnight-proof-server-prototype --disable-auth --port 6300 > /tmp/axum-server.log 2>&1 &
SERVER_PID=$!
echo "Server PID: $SERVER_PID"
sleep 2

echo ""
echo "=== Testing /prove endpoint with various inputs ==="
echo ""

echo "Test 1: Simple text"
echo "hello world" | curl -s -X POST http://localhost:6300/prove --data-binary @-
echo ""
echo ""

echo "Test 2: JSON data"
echo '{"circuit":"test","inputs":[1,2,3]}' | curl -s -X POST http://localhost:6300/prove -H "Content-Type: application/json" --data-binary @-
echo ""
echo ""

echo "Test 3: Binary data (100 bytes)"
dd if=/dev/urandom bs=100 count=1 2>/dev/null | curl -s -X POST http://localhost:6300/prove --data-binary @-
echo ""
echo ""

echo "Test 4: Larger payload (1KB)"
dd if=/dev/urandom bs=1024 count=1 2>/dev/null | curl -s -X POST http://localhost:6300/prove --data-binary @- | wc -c | xargs echo "Response size (bytes):"
echo ""

echo "Test 5: Check timing (should take ~1 second)"
time (echo "timing-test" | curl -s -X POST http://localhost:6300/prove --data-binary @- > /dev/null)
echo ""

echo "Test 6: Concurrent requests (5 at once)"
for i in 1 2 3 4 5; do
  (echo "request-$i" | curl -s -X POST http://localhost:6300/prove --data-binary @- && echo " [Request $i done]") &
done
wait
echo ""

echo "Test 7: Check queue status after load"
curl -s http://localhost:6300/ready | jq
echo ""

echo "=== Cleanup ==="
kill $SERVER_PID 2>/dev/null
echo "Server stopped"
echo ""
echo "✅ All tests complete!"
