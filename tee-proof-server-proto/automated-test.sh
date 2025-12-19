#!/bin/bash
# Automated Testing Script for Midnight Proof Server
# Collects metrics: timing, throughput, memory usage, CPU usage

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
SERVER_PORT=${SERVER_PORT:-6300}
SERVER_BIN="./target/release/midnight-proof-server-prototype"
LOG_DIR="./test-results"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
RESULTS_FILE="$LOG_DIR/test-results-$TIMESTAMP.json"
SERVER_LOG="$LOG_DIR/server-$TIMESTAMP.log"

# Create results directory
mkdir -p "$LOG_DIR"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}Midnight Proof Server - Automated Tests${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo "Timestamp: $TIMESTAMP"
echo "Results will be saved to: $RESULTS_FILE"
echo ""

# Check if binary exists
if [ ! -f "$SERVER_BIN" ]; then
    echo -e "${RED}❌ Binary not found at $SERVER_BIN${NC}"
    echo "Run 'cargo build --release' first"
    exit 1
fi

# Function to start server
start_server() {
    echo -e "${YELLOW}Starting server...${NC}"
    $SERVER_BIN \
        --port "$SERVER_PORT" \
        --disable-auth \
        --verbose \
        > "$SERVER_LOG" 2>&1 &
    SERVER_PID=$!
    echo "Server PID: $SERVER_PID"

    # Wait for server to be ready
    for i in {1..30}; do
        if curl -s "http://localhost:$SERVER_PORT/health" > /dev/null 2>&1; then
            echo -e "${GREEN}✅ Server is ready${NC}"
            return 0
        fi
        sleep 0.5
    done

    echo -e "${RED}❌ Server failed to start${NC}"
    cat "$SERVER_LOG"
    exit 1
}

# Function to stop server
stop_server() {
    if [ -n "$SERVER_PID" ]; then
        echo -e "${YELLOW}Stopping server (PID: $SERVER_PID)...${NC}"
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
        echo -e "${GREEN}✅ Server stopped${NC}"
    fi
}

# Function to get system stats
get_system_stats() {
    local pid=$1

    if [ "$(uname)" == "Darwin" ]; then
        # macOS
        local cpu=$(ps -p $pid -o %cpu | tail -1 | tr -d ' ')
        local mem=$(ps -p $pid -o rss | tail -1 | tr -d ' ')
        local mem_mb=$((mem / 1024))
    else
        # Linux
        local cpu=$(ps -p $pid -o %cpu --no-headers | tr -d ' ')
        local mem=$(ps -p $pid -o rss --no-headers | tr -d ' ')
        local mem_mb=$((mem / 1024))
    fi

    echo "$cpu,$mem_mb"
}

# Function to run a test
run_test() {
    local test_name=$1
    local endpoint=$2
    local payload=$3
    local iterations=${4:-1}

    echo ""
    echo -e "${BLUE}Test: $test_name${NC}"
    echo "  Endpoint: $endpoint"
    echo "  Iterations: $iterations"

    local total_time=0
    local success_count=0
    local fail_count=0
    local min_time=999999
    local max_time=0
    local response_sizes=()

    for i in $(seq 1 $iterations); do
        # Get system stats before request
        local stats_before=$(get_system_stats $SERVER_PID)

        # Make request and measure time
        local start_time=$(date +%s%N)
        local response=$(echo -n "$payload" | curl -s -w "\n%{http_code}\n%{time_total}" \
            -X POST "http://localhost:$SERVER_PORT$endpoint" \
            --data-binary @- 2>/dev/null)
        local end_time=$(date +%s%N)

        # Parse response
        local http_code=$(echo "$response" | tail -2 | head -1)
        local curl_time=$(echo "$response" | tail -1)
        local response_body=$(echo "$response" | head -n -2)
        local response_size=${#response_body}

        # Calculate elapsed time in milliseconds
        local elapsed_ms=$(echo "scale=2; $curl_time * 1000" | bc)

        # Get system stats after request
        local stats_after=$(get_system_stats $SERVER_PID)

        # Track statistics
        if [ "$http_code" == "200" ]; then
            success_count=$((success_count + 1))
            response_sizes+=($response_size)

            # Update min/max/total
            if (( $(echo "$elapsed_ms < $min_time" | bc -l) )); then
                min_time=$elapsed_ms
            fi
            if (( $(echo "$elapsed_ms > $max_time" | bc -l) )); then
                max_time=$elapsed_ms
            fi
            total_time=$(echo "$total_time + $elapsed_ms" | bc)

            echo "  [$i/$iterations] ✅ ${elapsed_ms}ms, response: ${response_size} bytes, HTTP $http_code"
        else
            fail_count=$((fail_count + 1))
            echo "  [$i/$iterations] ❌ HTTP $http_code"
        fi

        # Small delay between requests
        sleep 0.1
    done

    # Calculate statistics
    local avg_time=0
    if [ $success_count -gt 0 ]; then
        avg_time=$(echo "scale=2; $total_time / $success_count" | bc)
    fi

    # Calculate average response size
    local avg_response_size=0
    if [ ${#response_sizes[@]} -gt 0 ]; then
        local sum=0
        for size in "${response_sizes[@]}"; do
            sum=$((sum + size))
        done
        avg_response_size=$((sum / ${#response_sizes[@]}))
    fi

    # Print summary
    echo ""
    echo "  Results:"
    echo "    Success: $success_count/$iterations"
    echo "    Failed:  $fail_count/$iterations"
    if [ $success_count -gt 0 ]; then
        echo "    Min time:  ${min_time}ms"
        echo "    Max time:  ${max_time}ms"
        echo "    Avg time:  ${avg_time}ms"
        echo "    Avg response size: ${avg_response_size} bytes"
    fi

    # Return results as JSON (basic format)
    cat >> "$RESULTS_FILE" <<EOF
{
  "test_name": "$test_name",
  "endpoint": "$endpoint",
  "iterations": $iterations,
  "success_count": $success_count,
  "fail_count": $fail_count,
  "min_time_ms": $min_time,
  "max_time_ms": $max_time,
  "avg_time_ms": $avg_time,
  "avg_response_size_bytes": $avg_response_size
},
EOF
}

# Function to test concurrent requests
test_concurrent() {
    local test_name=$1
    local endpoint=$2
    local payload=$3
    local concurrent=$4

    echo ""
    echo -e "${BLUE}Concurrent Test: $test_name${NC}"
    echo "  Endpoint: $endpoint"
    echo "  Concurrent requests: $concurrent"

    local start_time=$(date +%s%N)

    # Launch concurrent requests
    local pids=()
    for i in $(seq 1 $concurrent); do
        (echo -n "$payload" | curl -s -X POST "http://localhost:$SERVER_PORT$endpoint" \
            --data-binary @- > /dev/null 2>&1) &
        pids+=($!)
    done

    # Wait for all to complete
    for pid in "${pids[@]}"; do
        wait $pid
    done

    local end_time=$(date +%s%N)
    local total_ms=$(( (end_time - start_time) / 1000000 ))

    echo "  Total time: ${total_ms}ms for $concurrent concurrent requests"
    echo "  Throughput: $(echo "scale=2; $concurrent / ($total_ms / 1000)" | bc) req/s"
}

# Trap to ensure server is stopped on exit
trap stop_server EXIT

# Initialize results file
echo "[" > "$RESULTS_FILE"

# Start server
start_server

echo ""
echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}Running Tests${NC}"
echo -e "${BLUE}========================================${NC}"

# Test 1: Health check
echo ""
echo -e "${BLUE}Test 1: Health & Readiness Endpoints${NC}"
curl -s "http://localhost:$SERVER_PORT/health" | jq . || echo "Health check failed"
echo ""
curl -s "http://localhost:$SERVER_PORT/ready" | jq . || echo "Ready check failed"
echo ""
curl -s "http://localhost:$SERVER_PORT/version" || echo "Version check failed"

# Test 2: Small payload to /check endpoint
run_test "Check Endpoint - Small Payload" "/check" "test-data-123" 5

# Test 3: Medium payload to /check endpoint
run_test "Check Endpoint - Medium Payload" "/check" "$(head -c 1024 /dev/urandom | base64)" 3

# Test 4: Concurrent requests
test_concurrent "Check Endpoint - Concurrent" "/check" "test-concurrent" 10

# Test 5: Load test - sustained requests
echo ""
echo -e "${BLUE}Load Test: Sustained requests (30 seconds)${NC}"
echo "Sending requests continuously for 30 seconds..."
local load_start=$(date +%s)
local load_count=0
local load_success=0
while [ $(($(date +%s) - load_start)) -lt 30 ]; do
    if echo "load-test" | curl -s -X POST "http://localhost:$SERVER_PORT/check" \
        --data-binary @- > /dev/null 2>&1; then
        load_success=$((load_success + 1))
    fi
    load_count=$((load_count + 1))
    sleep 0.1
done
echo "  Completed: $load_count requests ($load_success successful)"
echo "  Throughput: $(echo "scale=2; $load_success / 30" | bc) req/s"

# Finalize results file
sed -i.bak '$ s/,$//' "$RESULTS_FILE" && rm "$RESULTS_FILE.bak"
echo "]" >> "$RESULTS_FILE"

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Tests Complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Results saved to:"
echo "  - Metrics: $RESULTS_FILE"
echo "  - Server logs: $SERVER_LOG"
echo ""

# Show summary from server logs
echo "Server log summary (last 50 lines):"
tail -50 "$SERVER_LOG"
