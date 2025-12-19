# Proof Server Debugging Guide

**Midnight Proof Server - Deep Dive Debugging**

## Document Control

| Version | Date       | Author               | Changes       |
| ------- | ---------- | -------------------- | ------------- |
| 1.0     | 2025-12-19 | Bob Blessing-Hartley | Initial draft |

---

## 

---

## Table of Contents

1. [Logging and Observability](#logging-and-observability)
2. [Request/Response Debugging](#requestresponse-debugging)
3. [Proof Generation Debugging](#proof-generation-debugging)
4. [Performance Profiling](#performance-profiling)
5. [Network Debugging](#network-debugging)
6. [Memory Analysis](#memory-analysis)
7. [Worker Pool Debugging](#worker-pool-debugging)
8. [Parameter Fetching Debug](#parameter-fetching-debug)
9. [TEE-Specific Debugging](#tee-specific-debugging)
10. [Production Debugging](#production-debugging)

---

## Logging and Observability

### Log Levels and Configuration

The proof server uses Rust's `tracing` framework with multiple log levels:

```bash
# Environment variable priority (highest to lowest):
# 1. RUST_LOG (fine-grained control)
# 2. MIDNIGHT_PROOF_SERVER_VERBOSE (simple DEBUG/INFO toggle)

# Example configurations:
export RUST_LOG=midnight_proof_server_axum=trace  # Most verbose
export RUST_LOG=midnight_proof_server_axum=debug  # Debug level
export RUST_LOG=midnight_proof_server_axum=info   # Production (default)
export RUST_LOG=midnight_proof_server_axum=warn   # Warnings only
export RUST_LOG=midnight_proof_server_axum=error  # Errors only
```

### Fine-Grained Logging

Enable debug logging for specific components:

```bash
# Debug proof server, info for everything else
export RUST_LOG="midnight_proof_server_axum=debug,info"

# Debug multiple components
export RUST_LOG="midnight_proof_server_axum=debug,\
ledger=debug,\
transient_crypto=info,\
zswap=debug"

# Trace specific functions (very verbose!)
export RUST_LOG="midnight_proof_server_axum::prove_handler=trace"

# Enable tower HTTP middleware debugging
export RUST_LOG="midnight_proof_server_axum=debug,\
tower_http=debug"
```

### Structured Logging Output

Logs are output in a semi-structured format. Parse them with standard tools:

```bash
# View logs in real-time with color highlighting
docker logs -f midnight-proof-server 2>&1 | \
  grep --color=always -E "ERROR|WARN|$"

# Extract all ERROR logs
docker logs midnight-proof-server 2>&1 | \
  grep "ERROR" > errors.log

# Count log levels
docker logs midnight-proof-server 2>&1 | \
  grep -oE "TRACE|DEBUG|INFO|WARN|ERROR" | \
  sort | uniq -c

# Timeline of specific events
docker logs midnight-proof-server 2>&1 | \
  grep "Prove request" | \
  awk '{print $1, $2, $(NF-2), $(NF-1), $NF}'
```

### JSON Structured Logging (Optional)

For better parsing in production, you can enable JSON logging:

```bash
# Add to Dockerfile or startup script
export RUST_LOG_FORMAT=json

# Then parse with jq
docker logs midnight-proof-server 2>&1 | \
  jq 'select(.level == "ERROR")'

# Find slow requests (if duration is logged)
docker logs midnight-proof-server 2>&1 | \
  jq 'select(.duration_ms > 5000)'
```

---

## Request/Response Debugging

### Enable Request Tracing

Enable verbose logging to see request details:

```bash
docker run -d \
  --name midnight-proof-server \
  -p 6300:6300 \
  -e MIDNIGHT_PROOF_SERVER_VERBOSE=true \
  -e RUST_LOG=debug \
  midnight-proof-server:latest
```

### Inspect Request Payloads

The `/k` endpoint logs hex dumps of inputs in DEBUG mode:

```bash
# Send request
curl -X POST http://localhost:6300/k \
  -H "X-API-Key: your-key" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @zkir.bin

# Check logs for hex dump
docker logs midnight-proof-server 2>&1 | grep "Received request:"
# [DEBUG] Received request: d9010383190258636f6e7374727563746f72...
```

### Capture Full Request/Response Cycle

Use `curl` verbose mode to see everything:

```bash
# Save request and response
curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: your-key" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @zkir.bin \
  -o proof.bin \
  -v \
  -w "\n\nTiming:\n\
  DNS Lookup: %{time_namelookup}s\n\
  TCP Connect: %{time_connect}s\n\
  TLS Handshake: %{time_appconnect}s\n\
  Server Processing: %{time_starttransfer}s\n\
  Total Time: %{time_total}s\n\
  HTTP Status: %{http_code}\n\
  Bytes Sent: %{size_upload}\n\
  Bytes Received: %{size_download}\n\
  Speed Download: %{speed_download} bytes/sec\n"
```

### Trace HTTP Requests Through the Stack

```bash
# Enable tower_http tracing
export RUST_LOG="midnight_proof_server_axum=debug,tower_http=trace"

# You'll see:
# [TRACE] tower_http::trace: started processing request
# [DEBUG] midnight_proof_server_axum: Prove request received
# [TRACE] tower_http::trace: finished processing request status=200
```

### Log API Key Validation (Without Exposing Keys)

Check if authentication is working:

```bash
# Good request (with valid API key)
curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: valid-key" \
  --data-binary @zkir.bin

# Check logs - should show request received
docker logs midnight-proof-server | grep "Prove request received"

# Bad request (no API key)
curl -X POST http://localhost:6300/prove \
  --data-binary @zkir.bin

# Should return 401 and log nothing (auth failed before handler)
```

---

## Proof Generation Debugging

### Enable Proof Generation Tracing

```bash
# Maximum verbosity for proof debugging
export RUST_LOG="midnight_proof_server_axum=debug,\
ledger=debug,\
transient_crypto=debug,\
zswap=trace,\
zkir=debug"

docker run -d \
  -e RUST_LOG="$RUST_LOG" \
  midnight-proof-server:latest
```

### Monitor Proof Generation Progress

The proof generation process has several phases:

```bash
# Watch logs in real-time
docker logs -f midnight-proof-server 2>&1 | grep -E "Prove|proof|ZKIR|circuit|constraint"

# Expected output:
# [INFO] Prove request received, payload size: 12345 bytes
# [DEBUG] Deserializing proof preimage...
# [DEBUG] Initializing resolver...
# [DEBUG] Setting up proving key...
# [DEBUG] Generating circuit...
# [DEBUG] Computing witness...
# [DEBUG] Generating proof...
# [DEBUG] Serializing proof...
# [INFO] Proof generation completed
```

### Identify Slow Proof Generation Steps

Add timing instrumentation:

```bash
# Create a wrapper script
cat > time-proof.sh << 'EOF'
#!/bin/bash
set -e

START=$(date +%s)
echo "Starting proof generation at $(date)"

curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: $API_KEY" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @$1 \
  -o proof.bin \
  -w "\nHTTP Status: %{http_code}\nTime: %{time_total}s\n"

END=$(date +%s)
DURATION=$((END - START))

echo "Proof generation took $DURATION seconds"

# Extract timing from logs
docker logs midnight-proof-server 2>&1 | tail -50 | grep -E "Prove|time|duration"
EOF

chmod +x time-proof.sh
./time-proof.sh zkir.bin
```

### Debug Proof Failures

Common failure points:

#### 1. Deserialization Failure

```bash
# Error: "Failed to deserialize ZKIR"
# Debug:
export RUST_LOG=debug
docker logs midnight-proof-server 2>&1 | grep "deserialize"

# Verify ZKIR format
file zkir.bin
hexdump -C zkir.bin | head -20

# Check if it's CBOR-encoded
cat zkir.bin | cbor-diag | head -50
```

#### 2. Resolver Failure

```bash
# Error: "Failed to initialize resolver"
# Debug: Check if ZSwap parameters are available
docker exec midnight-proof-server ls -lh /root/.cache/midnight/

# Check parameter fetch logs
docker logs midnight-proof-server 2>&1 | grep -i "fetch\|param"
```

#### 3. Proving Key Material Missing

```bash
# Error: "Missing proving key material"
# Debug: Check if key material was included in request
docker logs midnight-proof-server 2>&1 | grep -i "key"

# The request should include Option<ProvingKeyMaterial>
# If None, the proof may require client-provided keys
```

#### 4. Out of Memory During Proof

```bash
# Error: Container exits with code 137
# Debug: Check memory usage
docker stats midnight-proof-server --no-stream

# Check system OOM killer
dmesg | grep -i "out of memory"
dmesg | grep -i "midnight-proof-server"

# Solution: Increase memory
docker run -d --memory=64g midnight-proof-server:latest
```

### Test with Known-Good Inputs

Create test fixtures:

```bash
# Generate test ZKIR (requires midnight-ledger tools)
# This would come from your Midnight wallet/SDK

# Test with minimal proof (k=10)
curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: $API_KEY" \
  --data-binary @test-zkir-k10.bin \
  -o test-proof.bin

# Verify proof was generated
file test-proof.bin
ls -lh test-proof.bin

# Test deserialization (if you have tools)
# cat test-proof.bin | deserialize-proof
```

---

## Performance Profiling

### CPU Profiling

#### Using `perf` (Linux)

```bash
# SSH into VM
ssh into-vm

# Install perf
sudo apt-get install linux-tools-generic

# Find proof server process
PID=$(pgrep -f midnight-proof-server)

# Profile for 30 seconds
sudo perf record -p $PID -g -- sleep 30

# Generate report
sudo perf report --stdio > perf-report.txt

# Look for hot spots
sudo perf report --sort=dso,symbol | head -50
```

#### Using `cargo flamegraph` (Development)

```bash
# Install flamegraph
cargo install flamegraph

# Build with debug symbols
cd /Users/robertblessing-hartley/code/tee-prover-prototype/proof-server
cargo build --release

# Profile during proof generation
cargo flamegraph --bin midnight-proof-server-prototype

# Send test request while flamegraph is running
curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: test" \
  --data-binary @zkir.bin \
  -o proof.bin

# View flamegraph.svg in browser
```

### Memory Profiling

#### Using `valgrind` (Development)

```bash
# Install valgrind
sudo apt-get install valgrind

# Build with debug symbols
cargo build --release

# Profile memory usage
valgrind --tool=massif \
  --massif-out-file=massif.out \
  ./target/release/midnight-proof-server-prototype \
  --port 6300 \
  --disable-auth

# In another terminal, send requests
curl -X POST http://localhost:6300/prove \
  --data-binary @zkir.bin \
  -o proof.bin

# Stop server and analyze
ms_print massif.out > memory-report.txt
```

#### Using Docker Stats

```bash
# Monitor memory in real-time
watch -n 1 'docker stats midnight-proof-server --no-stream'

# Log memory usage over time
while true; do
  docker stats midnight-proof-server --no-stream | \
    awk '{print strftime("%Y-%m-%d %H:%M:%S"), $0}'
  sleep 60
done > memory-log.txt

# Plot memory usage
cat memory-log.txt | \
  awk '{print $1, $2, $7}' | \
  gnuplot -e "set terminal png; plot '-' using 3 with lines" > memory-graph.png
```

### Worker Pool Metrics

Monitor worker pool utilization:

```bash
# Query /ready endpoint repeatedly
while true; do
  READY=$(curl -s http://localhost:6300/ready)
  QUEUE=$(echo $READY | jq -r '.queue_size // "unknown"')
  WORKERS=$(echo $READY | jq -r '.active_workers // "unknown"')
  echo "[$(date +%H:%M:%S)] Queue: $QUEUE, Active Workers: $WORKERS"
  sleep 5
done
```

Create a monitoring dashboard:

```bash
#!/bin/bash
# File: monitor-dashboard.sh

while true; do
  clear
  echo "=== Midnight Proof Server Dashboard ==="
  echo "Time: $(date)"
  echo ""

  # Server status
  echo "--- Server Status ---"
  curl -s http://localhost:6300/health | jq '.'
  echo ""

  # Queue stats
  echo "--- Queue Stats ---"
  curl -s http://localhost:6300/ready | jq '.'
  echo ""

  # Docker stats
  echo "--- Resource Usage ---"
  docker stats midnight-proof-server --no-stream
  echo ""

  sleep 5
done
```

---

## Network Debugging

### TCP Connection Debugging

```bash
# Check if server is listening
netstat -tulpn | grep 6300
# Should show: tcp 0.0.0.0:6300 ... LISTEN

# Check established connections
netstat -an | grep 6300 | grep ESTABLISHED

# Monitor connections in real-time
watch -n 1 'netstat -an | grep 6300'
```

### Packet Capture

```bash
# Capture traffic on port 6300
sudo tcpdump -i any -w capture.pcap 'port 6300'

# In another terminal, send request
curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: your-key" \
  --data-binary @zkir.bin

# Stop tcpdump (Ctrl+C) and analyze
wireshark capture.pcap

# Or use tcpdump to analyze
tcpdump -r capture.pcap -A | less
```



❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

## DANGER ZONE: All of the below is experimental, not yet tested ##

❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌❌

### Load Balancer Debugging

#### AWS ALB

```bash
# Check target health
aws elbv2 describe-target-health \
  --target-group-arn arn:aws:elasticloadbalancing:... \
  --query 'TargetHealthDescriptions[*].[Target.Id,TargetHealth.State,TargetHealth.Reason]' \
  --output table

# View ALB logs (if enabled)
aws s3 cp s3://your-alb-logs-bucket/AWSLogs/... - | gunzip

# Check ALB metrics
aws cloudwatch get-metric-statistics \
  --namespace AWS/ApplicationELB \
  --metric-name TargetResponseTime \
  --dimensions Name=LoadBalancer,Value=app/midnight-proof-server/... \
  --start-time 2025-12-18T00:00:00Z \
  --end-time 2025-12-18T23:59:59Z \
  --period 300 \
  --statistics Average
```

#### GCP Load Balancer

```bash
# Check backend health
gcloud compute backend-services get-health midnight-proof-server-backend \
  --global

# View load balancer logs
gcloud logging read "resource.type=http_load_balancer" \
  --limit=100 \
  --format=json

# Check latency metrics
gcloud monitoring time-series list \
  --filter='metric.type="loadbalancing.googleapis.com/https/request_duration"'
```

#### Azure Application Gateway

```bash
# Check backend health
az network application-gateway show-backend-health \
  --resource-group midnight-proof-server-rg \
  --name midnight-appgw

# Query logs
az monitor log-analytics query \
  --workspace <workspace-id> \
  --analytics-query "AzureDiagnostics | where ResourceType == 'APPLICATIONGATEWAYS'" \
  --output table
```

### SSL/TLS Debugging

```bash
# Test SSL connection
openssl s_client -connect proof.midnight.network:443 -servername proof.midnight.network

# Check certificate chain
echo | openssl s_client -connect proof.midnight.network:443 -showcerts

# Verify certificate dates
echo | openssl s_client -connect proof.midnight.network:443 2>/dev/null | \
  openssl x509 -noout -dates

# Check cipher suites
nmap --script ssl-enum-ciphers -p 443 proof.midnight.network

# Test with specific TLS version
openssl s_client -connect proof.midnight.network:443 -tls1_2
openssl s_client -connect proof.midnight.network:443 -tls1_3
```

---

## Memory Analysis

### Detect Memory Leaks

```bash
# Monitor memory over time
cat > monitor-memory.sh << 'EOF'
#!/bin/bash
while true; do
  MEM=$(docker stats midnight-proof-server --no-stream --format "{{.MemUsage}}")
  echo "$(date +%s) $MEM" >> memory-usage.log
  sleep 60
done
EOF

chmod +x monitor-memory.sh
./monitor-memory.sh &

# After running for several hours, analyze
cat memory-usage.log | \
  awk '{print $1, $2}' | \
  gnuplot -e "set terminal png; plot '-' using 1:2 with lines" > memory-trend.png

# If memory constantly grows, you have a leak
```

### Heap Profiling (Rust-specific)

```bash
# Use jemalloc with profiling
# Add to Cargo.toml:
# [dependencies]
# jemallocator = "0.5"

# Rebuild with profiling
MALLOC_CONF=prof:true cargo build --release

# Run and generate heap profile
./target/release/midnight-proof-server-prototype &
# Send requests...
# Kill and analyze jeprof output
```

### Check for File Descriptor Leaks

```bash
# Check open file descriptors
lsof -p $(pgrep midnight-proof-server) | wc -l

# Monitor over time
watch -n 5 'lsof -p $(pgrep midnight-proof-server) | wc -l'

# If constantly growing, you have a file descriptor leak
```

---

## Worker Pool Debugging

### Monitor Worker Pool State

```bash
# Query /ready endpoint
curl -s http://localhost:6300/ready | jq

# Expected output:
# {
#   "status": "ready",
#   "queue_size": 5,
#   "active_workers": 12,
#   "total_workers": 16
# }
```

### Detect Worker Starvation

```bash
# If queue_size is high and active_workers < total_workers:
# Workers may be blocked or crashed

# Check logs for panics
docker logs midnight-proof-server 2>&1 | grep -i "panic\|thread.*panicked"

# Check if workers are stuck
docker exec midnight-proof-server ps aux

# Send SIGUSR1 to dump thread state (if implemented)
docker exec midnight-proof-server kill -USR1 1
docker logs midnight-proof-server | tail -50
```

### Load Testing Worker Pool

```bash
# Send concurrent requests
cat > load-test.sh << 'EOF'
#!/bin/bash
CONCURRENT=20
for i in $(seq 1 $CONCURRENT); do
  (
    echo "Request $i starting..."
    curl -X POST http://localhost:6300/prove \
      -H "X-API-Key: $API_KEY" \
      --data-binary @zkir.bin \
      -o proof-$i.bin \
      -w "Request $i: %{http_code} in %{time_total}s\n"
  ) &
done
wait
EOF

chmod +x load-test.sh
./load-test.sh

# Monitor queue during load test
watch -n 1 'curl -s http://localhost:6300/ready | jq'
```

---

## Parameter Fetching Debug

### Monitor Parameter Downloads

```bash
# Enable debug logging
export RUST_LOG=debug

# Watch parameter fetches
docker logs -f midnight-proof-server 2>&1 | grep -i "fetch\|param\|download"

# Check cache directory
docker exec midnight-proof-server du -sh /root/.cache/midnight/
docker exec midnight-proof-server ls -lh /root/.cache/midnight/
```

### Debug Slow Parameter Fetches

```bash
# Test network speed to parameter source
# Parameters are typically fetched from IPFS or HTTP

# Test with curl
time curl -o /tmp/test-params https://path-to-params/k10.params

# Monitor network usage during fetch
curl -X GET http://localhost:6300/fetch-params/24 &
docker stats midnight-proof-server

# Check if download is progressing
watch -n 5 'docker exec midnight-proof-server du -sh /root/.cache/midnight/'
```

### Clear Parameter Cache

```bash
# If parameters are corrupted
docker exec midnight-proof-server rm -rf /root/.cache/midnight/

# Or rebuild container
docker stop midnight-proof-server
docker rm midnight-proof-server
docker run -d midnight-proof-server:latest

# Rewarm cache
for k in 10 14 18; do
  curl http://localhost:6300/fetch-params/$k
done
```

---

## TEE-Specific Debugging

### AWS Nitro Enclaves

```bash
# Check enclave status
nitro-cli describe-enclaves

# Check enclave console output
nitro-cli console --enclave-id i-xxx-encYYY

# Check parent instance system log
sudo journalctl -u nitro-enclaves-allocator.service

# Check vsock proxy
sudo systemctl status vsock-proxy
sudo journalctl -u vsock-proxy

# Test vsock connection from parent
nc -v localhost 6300
```

### GCP Confidential VM

```bash
# Verify confidential computing is enabled
gcloud compute instances describe midnight-proof-server \
  --zone=us-central1-a \
  --format="get(confidentialInstanceConfig)"

# Check vTPM status
gcloud compute ssh midnight-proof-server --zone=us-central1-a \
  --command="sudo tpm2_pcrread"

# Check serial console for boot errors
gcloud compute instances get-serial-port-output midnight-proof-server \
  --zone=us-central1-a | tail -100
```

### Azure Confidential VM

```bash
# Check confidential VM status
az vm get-instance-view \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --query "instanceView.statuses"

# Check vTPM
az vm run-command invoke \
  --resource-group midnight-proof-server-rg \
  --name midnight-proof-server \
  --command-id RunShellScript \
  --scripts "sudo tpm2_pcrread"

# Check attestation service connectivity
az attestation show \
  --name midnight-attestation \
  --resource-group midnight-proof-server-rg
```

---

## Production Debugging

### Safe Production Debugging

**⚠️ NEVER enable debug logging in production without considering:**

1. **Performance impact** (~2-3% overhead)
2. **Disk space** (logs can grow quickly)
3. **Sensitive data** (debug logs may contain request details)

### Selective Debug Logging

Enable debug only for specific endpoints:

```bash
# Debug only /prove endpoint
export RUST_LOG="midnight_proof_server_axum::prove_handler=debug,\
midnight_proof_server_axum=info"

# Or only for errors
export RUST_LOG="midnight_proof_server_axum=error"
```

### Production Log Analysis

```bash
# Export logs from cloud logging
# AWS
aws logs create-export-task \
  --log-group-name /aws/ec2/midnight-proof-server \
  --from 1609459200000 \
  --to 1609545600000 \
  --destination s3://your-bucket/logs/

# GCP
gcloud logging read "resource.type=gce_instance" \
  --format=json \
  --limit=10000 > logs.json

# Azure
az monitor log-analytics query \
  --workspace <workspace-id> \
  --analytics-query "ContainerLog | where TimeGenerated > ago(1d)" \
  --output json > logs.json

# Analyze locally
cat logs.json | jq 'select(.level == "ERROR")' | less
```

### Distributed Tracing (Advanced)

For multi-instance deployments, use distributed tracing:

```bash
# Add OpenTelemetry to proof server (requires code changes)
# See https://docs.rs/tracing-opentelemetry/

# Export traces to Jaeger/Zipkin
export OTEL_EXPORTER_JAEGER_ENDPOINT=http://jaeger:14268/api/traces

# View traces in Jaeger UI
# http://jaeger-ui:16686
```

---

## Debugging Checklist

Before opening a support ticket, run through this checklist:

- [ ] Check server health: `curl http://localhost:6300/health`
- [ ] Check server logs: `docker logs midnight-proof-server | tail -100`
- [ ] Check resource usage: `docker stats midnight-proof-server`
- [ ] Check disk space: `df -h`
- [ ] Check network connectivity: `curl -v https://proof.midnight.network/health`
- [ ] Check SSL certificate: `openssl s_client -connect proof.midnight.network:443`
- [ ] Check queue status: `curl http://localhost:6300/ready`
- [ ] Check for recent errors: `docker logs midnight-proof-server 2>&1 | grep ERROR`
- [ ] Check TEE status (cloud-specific commands above)
- [ ] Test with known-good request
- [ ] Check firewall rules
- [ ] Check load balancer health
- [ ] Review recent configuration changes

---

## Advanced Debugging Scenarios

### Scenario 1: Intermittent Failures

```bash
# Log all requests with timestamps
docker logs -f midnight-proof-server 2>&1 | \
  while read line; do echo "[$(date +%s)] $line"; done > timestamped.log

# Correlate with system metrics
sar -u 1 > cpu.log &
sar -r 1 > memory.log &

# Find pattern in failures
grep ERROR timestamped.log | \
  awk '{print $1}' | \
  xargs -I {} date -d @{} "+%Y-%m-%d %H:%M:%S"
```

### Scenario 2: Proof Generation Hangs

```bash
# Attach debugger (development only!)
docker exec -it midnight-proof-server gdb -p $(pgrep midnight-proof-server)
# (gdb) bt  # backtrace
# (gdb) thread apply all bt  # all threads

# Or use strace
docker exec -it midnight-proof-server strace -p $(pgrep midnight-proof-server) -f

# Look for system calls that are blocking
```

### Scenario 3: Mysterious Crashes

```bash
# Check for core dumps
docker exec midnight-proof-server ls -lh /cores/

# Enable core dumps if not already
ulimit -c unlimited

# Analyze core dump with gdb
gdb target/release/midnight-proof-server-prototype /cores/core.12345
# (gdb) bt full
```

---

## Summary

### Quick Debug Commands

```bash
# Health check
curl http://localhost:6300/health

# Enable debug logging
docker run -d -e RUST_LOG=debug midnight-proof-server:latest

# View logs
docker logs -f midnight-proof-server

# Check resources
docker stats midnight-proof-server

# Test endpoint
curl -v http://localhost:6300/version

# Monitor queue
watch -n 1 'curl -s http://localhost:6300/ready | jq'
```

### Common Issues and Quick Fixes

| Issue | Quick Debug | Quick Fix |
|-------|-------------|-----------|
| Server not responding | `curl -v http://localhost:6300/health` | Restart container |
| High memory usage | `docker stats` | Increase memory limit |
| Proof fails | `docker logs` + grep ERROR | Check ZKIR format |
| Slow performance | `curl http://localhost:6300/ready` | Increase workers |
| Network timeout | `curl -v --max-time 600` | Increase timeout |

For additional help, see [troubleshooting.md](troubleshooting.md) 
