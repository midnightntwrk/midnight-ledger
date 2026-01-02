#!/bin/bash
# Test script for startup validation

BIN="/Users/robertblessing-hartley/code/midnight-code/midnight-ledger/target/release/midnight-proof-server-prototype"

echo "=== Test 1: WITH validation (default) ==="
echo "Starting server..."
$BIN --disable-auth --verbose > /tmp/test-with-validation.log 2>&1 &
PID=$!
sleep 5
kill $PID 2>/dev/null
echo "Output:"
head -30 /tmp/test-with-validation.log
echo ""

echo "=== Test 2: WITHOUT validation (--no-fetch-params) ==="
echo "Starting server..."
$BIN --disable-auth --verbose --no-fetch-params > /tmp/test-no-validation.log 2>&1 &
PID=$!
sleep 2
kill $PID 2>/dev/null
echo "Output:"
head -30 /tmp/test-no-validation.log
echo ""

echo "=== Summary ==="
echo "With validation:"
grep -E "Ensuring zswap|Skipping|Missing zero-knowledge|✓" /tmp/test-with-validation.log | head -5
echo ""
echo "Without validation:"
grep -E "Ensuring zswap|Skipping|Missing zero-knowledge|✓" /tmp/test-no-validation.log | head -5
