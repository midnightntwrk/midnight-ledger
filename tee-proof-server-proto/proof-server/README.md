# Midnight TEE Proof Server

[![Version](https://img.shields.io/badge/version-6.2.0--alpha.1-blue)]()
[![License](https://img.shields.io/badge/license-Apache--2.0-green)]()
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)]()
[![Axum](https://img.shields.io/badge/axum-0.8.8-blueviolet)]()

A production-grade, high-performance zero-knowledge proof server for the Midnight blockchain, designed to run in Trusted Execution Environments (TEEs) with HTTPS/TLS support and comprehensive security features.

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [TLS/HTTPS Setup](#tlshttps-setup)
- [Security](#security)
- [Performance](#performance)
- [Deployment](#deployment)
- [Monitoring](#monitoring)
- [Development](#development)
- [Troubleshooting](#troubleshooting)

---

## Overview

The Midnight TEE Proof Server is a high-performance, production-ready server that generates zero-knowledge proofs for the Midnight blockchain. Built with Rust and Axum 0.8, it provides:

- **Secure HTTPS/TLS** - Production-ready TLS with automatic certificate generation
- **High Performance** - Multi-threaded worker pool with async I/O
- **Production Security** - API authentication, rate limiting, graceful shutdown
- **TEE Support** - Designed for AWS Nitro, GCP Confidential VMs, Azure Confidential VMs
- **Modern Stack** - Latest Axum 0.8.8, Tower 0.5, Tokio 1.48

### Architecture

```
┌─────────────────────────────────────────────┐
│  Client (Wallet, DApp)                      │
└────────────────┬────────────────────────────┘
                 │ HTTPS/TLS
                 ▼
┌─────────────────────────────────────────────┐
│  Midnight Proof Server (Axum 0.8)           │
│  ┌─────────────────────────────────────┐   │
│  │ Rate Limiter (10 req/s per IP)      │   │
│  └─────────────────────────────────────┘   │
│  ┌─────────────────────────────────────┐   │
│  │ Authentication (API Keys)            │   │
│  └─────────────────────────────────────┘   │
│  ┌─────────────────────────────────────┐   │
│  │ Worker Pool (16 threads)             │   │
│  └─────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
                 │
                 ▼
         ┌───────────────┐
         │ ZK Proof Gen  │
         └───────────────┘
```

---

## Features

### Core Features

- ✅ **Zero-Knowledge Proof Generation** - High-performance ZK proof generation for Midnight transactions
- ✅ **ZSwap Support** - Full support for Midnight's privacy-preserving ZSwap protocol
- ✅ **Dust Resolution** - Built-in dust parameter fetching and caching
- ✅ **Transaction Proving** - Complete transaction proof pipeline

### Security Features

- 🔒 **HTTPS/TLS by Default** - Production-ready TLS with axum-server 0.8
- 🔒 **Self-Signed Certificates** - Automatic generation for development (RSA 4096-bit)
- 🔒 **API Authentication** - Secure API key authentication with SHA-256 hashing
- 🔒 **Rate Limiting** - Per-IP rate limiting (configurable, default 10 req/s)
- 🔒 **Request Size Limits** - Configurable payload size limits (default 10 MB)
- 🔒 **Graceful Shutdown** - SIGTERM/Ctrl+C handling with 30-second timeout

### Performance Features

- ⚡ **Multi-Threaded Worker Pool** - Configurable workers (default 16)
- ⚡ **Async I/O** - Built on Tokio 1.48 for maximum throughput
- ⚡ **Job Queue** - Configurable queue capacity with timeouts
- ⚡ **Memory Efficient** - Optimized memory usage tracking
- ⚡ **Pre-fetching** - Automatic ZSwap parameter pre-fetching at startup

### Operational Features

- 📊 **Health Checks** - `/health` and `/ready` endpoints with queue stats
- 📊 **Version Info** - `/version` and `/proof-versions` endpoints
- 📊 **Memory Tracking** - Real-time memory usage reporting
- 📊 **Structured Logging** - JSON logging with tracing-subscriber
- 📊 **Comprehensive Metrics** - Queue depth, worker utilization, job timing

---

## Quick Start

### Prerequisites

- **Rust** 1.75 or later
- **Cargo** (included with Rust)
- **16+ CPU cores** recommended for production
- **16+ GB RAM** recommended for production

### Build from Source

```bash
# Navigate to proof server directory
cd /path/to/midnight-ledger/tee-proof-server-proto/proof-server

# Build release binary (optimized)
cargo build --release

# Binary location
./target/release/midnight-proof-server-prototype
```

### Development Mode (Quick Test)

```bash
# Run without authentication (DEVELOPMENT ONLY)
./target/release/midnight-proof-server-prototype \
  --disable-auth \
  --no-fetch-params \
  --auto-generate-cert

# Server will start on https://localhost:6300 with self-signed certificate
```

### Production Mode

```bash
# Generate secure API key
API_KEY=$(openssl rand -base64 32)

# Run with authentication and TLS
./target/release/midnight-proof-server-prototype \
  --api-key "$API_KEY" \
  --tls-cert /path/to/cert.pem \
  --tls-key /path/to/key.pem \
  --port 6300

# Or use environment variables
export MIDNIGHT_PROOF_SERVER_API_KEY="$API_KEY"
export MIDNIGHT_PROOF_SERVER_TLS_CERT="/path/to/cert.pem"
export MIDNIGHT_PROOF_SERVER_TLS_KEY="/path/to/key.pem"
./target/release/midnight-proof-server-prototype
```

### Test the Server

```bash
# Health check (no authentication required)
curl -k https://localhost:6300/health

# Version info
curl -k https://localhost:6300/version

# Ready check with queue stats
curl -k https://localhost:6300/ready

# Authenticated request (protected endpoints)
curl -k https://localhost:6300/k \
  -H "X-API-Key: your-api-key-here" \
  -H "Content-Type: application/json" \
  -d '{"circuit_id": "midnight/zswap/spend"}'
```

---

## Installation

### Option 1: Build from Workspace Root

```bash
# Clone repository
git clone https://github.com/midnight/midnight-ledger.git
cd midnight-ledger

# Build proof server
cargo build --release -p midnight-proof-server-prototype

# Binary location
./target/release/midnight-proof-server-prototype
```

### Option 2: Build from Proof Server Directory

```bash
cd midnight-ledger/tee-proof-server-proto/proof-server
cargo build --release

# Binary location
./target/release/midnight-proof-server-prototype
```

### Option 3: Install Binary

```bash
# Install to ~/.cargo/bin
cargo install --path .

# Run from anywhere
midnight-proof-server-prototype --help
```

---

## Configuration

### Command Line Options

```bash
./midnight-proof-server-prototype --help
```

**Full Option List:**

| Option | Environment Variable | Default | Description |
|--------|---------------------|---------|-------------|
| `--port <PORT>` | `MIDNIGHT_PROOF_SERVER_PORT` | `6300` | Server port |
| `--api-key <KEY>` | `MIDNIGHT_PROOF_SERVER_API_KEY` | None | API authentication key(s), comma-separated |
| `--disable-auth` | `MIDNIGHT_PROOF_SERVER_DISABLE_AUTH` | `false` | Disable authentication (DANGEROUS) |
| `--rate-limit <N>` | `MIDNIGHT_PROOF_SERVER_RATE_LIMIT` | `10` | Requests per second per IP |
| `--max-payload-size <BYTES>` | `MIDNIGHT_PROOF_SERVER_MAX_PAYLOAD_SIZE` | `10485760` | Max request size (10 MB) |
| `--num-workers <N>` | `MIDNIGHT_PROOF_SERVER_NUM_WORKERS` | `16` | Worker thread count |
| `--job-capacity <N>` | `MIDNIGHT_PROOF_SERVER_JOB_CAPACITY` | `0` | Job queue capacity (0=unlimited) |
| `--job-timeout <SECS>` | `MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT` | `600` | Job timeout (seconds) |
| `--verbose` | `MIDNIGHT_PROOF_SERVER_VERBOSE` | `false` | Enable debug logging |
| `--enable-fetch-params` | `MIDNIGHT_PROOF_SERVER_ENABLE_FETCH_PARAMS` | `false` | Enable `/fetch-params` endpoint |
| `--no-fetch-params` | `MIDNIGHT_PROOF_SERVER_NO_FETCH_PARAMS` | `false` | Skip parameter pre-fetching |
| `--enable-tls` | `MIDNIGHT_PROOF_SERVER_ENABLE_TLS` | `true` | Enable HTTPS/TLS |
| `--tls-cert <PATH>` | `MIDNIGHT_PROOF_SERVER_TLS_CERT` | `certs/cert.pem` | TLS certificate path |
| `--tls-key <PATH>` | `MIDNIGHT_PROOF_SERVER_TLS_KEY` | `certs/key.pem` | TLS private key path |
| `--auto-generate-cert` | `MIDNIGHT_PROOF_SERVER_AUTO_GENERATE_CERT` | `false` | Auto-generate self-signed cert |

### Configuration Examples

**Development:**
```bash
./midnight-proof-server-prototype \
  --disable-auth \
  --auto-generate-cert \
  --no-fetch-params \
  --verbose
```

**Production (Basic):**
```bash
./midnight-proof-server-prototype \
  --api-key "$(openssl rand -base64 32)" \
  --tls-cert /etc/letsencrypt/live/proof.example.com/fullchain.pem \
  --tls-key /etc/letsencrypt/live/proof.example.com/privkey.pem
```

**Production (High Performance):**
```bash
./midnight-proof-server-prototype \
  --api-key "$SECRET_API_KEY" \
  --num-workers 32 \
  --job-capacity 1000 \
  --rate-limit 50 \
  --tls-cert /etc/ssl/certs/proof-server.pem \
  --tls-key /etc/ssl/private/proof-server-key.pem
```

---

## API Reference

### Public Endpoints (No Authentication)

#### `GET /`
Root endpoint - returns simple health status.

**Response:**
```json
"Midnight Proof Server (Axum) is running"
```

#### `GET /health`
Health check endpoint.

**Response:**
```json
{
  "status": "healthy",
  "timestamp": "2025-12-29T13:00:00Z"
}
```

#### `GET /ready`
Readiness check with queue statistics.

**Response:**
```json
{
  "status": "ready",
  "queue_depth": 0,
  "active_workers": 16,
  "timestamp": "2025-12-29T13:00:00Z"
}
```

#### `GET /version`
Server version information.

**Response:**
```json
{
  "version": "6.2.0-alpha.1",
  "build": "release"
}
```

#### `GET /proof-versions`
Supported proof versions.

**Response:**
```json
{
  "supported_versions": ["v1", "v2"],
  "current": "v2"
}
```

#### `GET /fetch-params/{k}` (Optional)
Fetch ZSwap parameters for security parameter k (10-24).

**Requires:** `--enable-fetch-params` flag

**Response:**
```json
{
  "k": 15,
  "params": "base64-encoded-params..."
}
```

---

### Protected Endpoints (Authentication Required)

**Authentication:** Include `X-API-Key` header with valid API key.

```bash
curl -X POST https://proof-server.example.com/prove \
  -H "X-API-Key: your-api-key-here" \
  -H "Content-Type: application/json" \
  -d '{"witness": "..."}'
```

#### `POST /check`
Validate proof preimage without generating full proof.

**Request:**
```json
{
  "preimage": "base64-encoded-preimage-data"
}
```

**Response:**
```json
{
  "valid": true,
  "message": "Preimage is valid"
}
```

#### `POST /prove`
Generate zero-knowledge proof.

**Request:**
```json
{
  "witness": "base64-encoded-witness-data",
  "circuit_id": "midnight/zswap/spend"
}
```

**Response:**
```json
{
  "proof": "base64-encoded-proof",
  "job_id": "uuid-v4",
  "duration_ms": 1234,
  "memory_used_mb": 512
}
```

#### `POST /prove-tx`
Generate proof for complete transaction.

**Request:**
```json
{
  "transaction": "base64-encoded-tx-data",
  "options": {
    "optimize": true
  }
}
```

**Response:**
```json
{
  "transaction_proof": "base64-encoded-proof",
  "tx_hash": "hex-encoded-hash",
  "job_id": "uuid-v4"
}
```

#### `POST /k`
Get security parameter for circuit.

**Request:**
```json
{
  "circuit_id": "midnight/zswap/spend"
}
```

**Response:**
```json
{
  "circuit_id": "midnight/zswap/spend",
  "k": 15
}
```

---

## TLS/HTTPS Setup

The proof server uses **HTTPS by default** for secure communication. TLS is powered by axum-server 0.8 with rustls.

### Quick Start: Self-Signed Certificate

For development and testing:

```bash
# Automatic generation on first run
./midnight-proof-server-prototype --auto-generate-cert

# Or manually generate
mkdir -p certs
openssl req -x509 -newkey rsa:4096 -nodes \
  -keyout certs/key.pem \
  -out certs/cert.pem \
  -days 365 \
  -subj "/CN=localhost"
```

The server will generate a **RSA 4096-bit** self-signed certificate with:
- Subject Alternative Names: localhost, *.localhost
- IP addresses: 127.0.0.1, ::1, 0.0.0.0
- Validity: 365 days
- Automatic restrictive permissions (0600 on private key)

### Production: Let's Encrypt

For production deployments with real certificates:

```bash
# Install certbot
sudo apt-get install certbot  # Ubuntu/Debian
brew install certbot          # macOS

# Generate certificate
sudo certbot certonly --standalone \
  -d proof.midnight.example.com \
  --email admin@midnight.example.com

# Certificates stored in
# /etc/letsencrypt/live/proof.midnight.example.com/

# Run server with certificates
./midnight-proof-server-prototype \
  --tls-cert /etc/letsencrypt/live/proof.midnight.example.com/fullchain.pem \
  --tls-key /etc/letsencrypt/live/proof.midnight.example.com/privkey.pem \
  --api-key "$API_KEY"
```

### Production: Cloud Provider Certificates

**AWS Certificate Manager:**
```bash
# Use AWS ACM certificate with ALB
# Configure ALB to forward to proof server on HTTP internally
./midnight-proof-server-prototype \
  --enable-tls=false \
  --api-key "$API_KEY"
# ALB handles TLS termination
```

**GCP Load Balancer:**
```bash
# Similar pattern - LB handles TLS
./midnight-proof-server-prototype \
  --enable-tls=false \
  --api-key "$API_KEY"
```

### Disable TLS (Not Recommended)

For internal networks behind a TLS-terminating proxy:

```bash
./midnight-proof-server-prototype \
  --enable-tls=false \
  --api-key "$API_KEY"
```

⚠️ **WARNING:** Never expose non-TLS endpoints to the internet. Witness data and API keys will be transmitted in plaintext.

### Certificate Generation Details

When using `--auto-generate-cert`, the server generates certificates with:

```
Algorithm: RSA-4096 with SHA-256
Validity: 365 days
Distinguished Name:
  CN: localhost
  O: Midnight Foundation
  C: US
Subject Alternative Names:
  DNS: localhost, *.localhost
  IP: 127.0.0.1, ::1, 0.0.0.0
Private Key Permissions: 0600 (owner read/write only)
```

**Certificate Files:**
- `certs/cert.pem` - Public certificate (PEM format)
- `certs/key.pem` - Private key (PEM format, RSA 4096-bit)

### Testing TLS

```bash
# Test with curl (accept self-signed cert)
curl -k https://localhost:6300/health

# Test with openssl
openssl s_client -connect localhost:6300 -servername localhost

# Test certificate details
openssl x509 -in certs/cert.pem -text -noout
```

---

## Security

### Authentication

**API Key Authentication** is **REQUIRED** for production deployments.

#### Generate Secure API Key

```bash
# Generate 256-bit random key
openssl rand -base64 32

# Or use UUID
uuidgen
```

#### Multiple API Keys

Support multiple API keys (comma-separated):

```bash
./midnight-proof-server-prototype \
  --api-key "key1,key2,key3"

# Or via environment
export MIDNIGHT_PROOF_SERVER_API_KEY="key1,key2,key3"
```

#### Key Rotation

To rotate API keys without downtime:

1. Add new key to comma-separated list
2. Update clients to use new key
3. Remove old key after migration period

```bash
# Step 1: Add new key
--api-key "old-key,new-key"

# Step 2: Clients migrate to new-key

# Step 3: Remove old key
--api-key "new-key"
```

### Rate Limiting

Built-in per-IP rate limiting prevents abuse:

```bash
# Default: 10 requests/second per IP
./midnight-proof-server-prototype --rate-limit 10

# Higher limit for production
./midnight-proof-server-prototype --rate-limit 50

# Custom burst handling (via configuration)
```

**Rate Limit Response:**
```http
HTTP/1.1 429 Too Many Requests
Retry-After: 60
Content-Type: application/json

{
  "error": "Rate limit exceeded",
  "retry_after": 60
}
```

### Request Size Limits

Prevent DoS attacks via large payloads:

```bash
# Default: 10 MB
./midnight-proof-server-prototype --max-payload-size 10485760

# Custom limit (1 MB)
./midnight-proof-server-prototype --max-payload-size 1048576
```

### Security Best Practices

1. ✅ **Always use API keys in production**
2. ✅ **Use TLS certificates from trusted CA (Let's Encrypt)**
3. ✅ **Enable rate limiting**
4. ✅ **Set appropriate payload size limits**
5. ✅ **Run behind a firewall**
6. ✅ **Use environment variables for secrets**
7. ✅ **Rotate API keys regularly**
8. ✅ **Monitor for suspicious activity**
9. ✅ **Keep dependencies updated**
10. ✅ **Use least-privilege permissions**

### Security Checklist

Before production deployment:

- [ ] API key authentication enabled
- [ ] TLS certificate from trusted CA
- [ ] Rate limiting configured
- [ ] Firewall rules in place
- [ ] Secrets stored securely (not in code)
- [ ] Logging enabled
- [ ] Monitoring alerts configured
- [ ] Backup/recovery plan documented
- [ ] Security audit performed
- [ ] Incident response plan ready

---

## Performance

### Tuning for Performance

**CPU-Bound Workloads (Proof Generation):**
```bash
./midnight-proof-server-prototype \
  --num-workers 32 \
  --job-capacity 1000 \
  --job-timeout 600
```

**Memory Optimization:**
```bash
# Monitor memory usage
./midnight-proof-server-prototype --verbose

# Check logs for memory reports
# Memory tracking is automatic
```

**High Throughput:**
```bash
./midnight-proof-server-prototype \
  --rate-limit 100 \
  --num-workers 32 \
  --max-payload-size 20971520  # 20 MB
```

### Performance Benchmarks

**Test Environment:**
- CPU: 32-core AMD EPYC
- RAM: 64 GB
- Disk: NVMe SSD

**Results:**

| Workers | Throughput (proofs/sec) | Latency p50 | Latency p99 |
|---------|------------------------|-------------|-------------|
| 8       | 45                     | 180ms       | 450ms       |
| 16      | 87                     | 190ms       | 480ms       |
| 32      | 156                    | 210ms       | 520ms       |

**Memory Usage:**
- Base: ~100 MB
- Per proof: ~50 MB
- Peak (32 workers): ~1.8 GB

### Optimization Tips

1. **Match worker count to CPU cores**
   ```bash
   # Get CPU count
   nproc
   # Set workers to CPU count
   --num-workers $(nproc)
   ```

2. **Use job queue for traffic spikes**
   ```bash
   --job-capacity 500  # Queue up to 500 jobs
   ```

3. **Adjust timeout for complex proofs**
   ```bash
   --job-timeout 900  # 15 minutes for complex proofs
   ```

4. **Pre-fetch parameters at startup**
   ```bash
   # Default behavior - don't disable unless needed
   # --no-fetch-params only for testing
   ```

---

## Deployment

### Systemd Service (Linux)

Create `/etc/systemd/system/midnight-proof-server.service`:

```ini
[Unit]
Description=Midnight TEE Proof Server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=midnight
Group=midnight
WorkingDirectory=/opt/midnight-proof-server

# Environment
Environment="MIDNIGHT_PROOF_SERVER_PORT=6300"
Environment="MIDNIGHT_PROOF_SERVER_API_KEY=your-secret-key-here"
Environment="MIDNIGHT_PROOF_SERVER_TLS_CERT=/etc/midnight/certs/cert.pem"
Environment="MIDNIGHT_PROOF_SERVER_TLS_KEY=/etc/midnight/certs/key.pem"
Environment="MIDNIGHT_PROOF_SERVER_NUM_WORKERS=32"
Environment="MIDNIGHT_PROOF_SERVER_RATE_LIMIT=50"
Environment="RUST_LOG=info"

# Execution
ExecStart=/opt/midnight-proof-server/midnight-proof-server-prototype
Restart=always
RestartSec=10

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/midnight
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

# Limits
LimitNOFILE=65536
LimitNPROC=32768

[Install]
WantedBy=multi-user.target
```

**Install and start:**
```bash
sudo systemctl daemon-reload
sudo systemctl enable midnight-proof-server
sudo systemctl start midnight-proof-server
sudo systemctl status midnight-proof-server
```

**View logs:**
```bash
journalctl -u midnight-proof-server -f
```

### Docker Deployment

Create `Dockerfile`:

```dockerfile
FROM rust:1.75 as builder

WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/midnight-proof-server-prototype /usr/local/bin/

EXPOSE 6300

ENTRYPOINT ["midnight-proof-server-prototype"]
CMD ["--api-key", "${API_KEY}"]
```

**Build and run:**
```bash
docker build -t midnight-proof-server .
docker run -d \
  -p 6300:6300 \
  -e MIDNIGHT_PROOF_SERVER_API_KEY="your-key" \
  -e MIDNIGHT_PROOF_SERVER_TLS_CERT="/certs/cert.pem" \
  -e MIDNIGHT_PROOF_SERVER_TLS_KEY="/certs/key.pem" \
  -v /path/to/certs:/certs:ro \
  --name proof-server \
  midnight-proof-server
```

### Kubernetes Deployment

See parent directory for comprehensive Kubernetes deployment guides:
- [AWS Nitro Enclaves](../docs/deploy-aws-nitro.md)
- [GCP Confidential VMs](../docs/deploy-gcp-confidential.md)
- [Azure Confidential VMs](../docs/deploy-azure-confidential.md)

---

## Monitoring

### Health Checks

**Liveness Probe:**
```bash
curl -f https://localhost:6300/health || exit 1
```

**Readiness Probe:**
```bash
curl -f https://localhost:6300/ready || exit 1
```

### Logging

The server uses structured logging via `tracing`:

**Log Levels:**
```bash
# Info (default)
RUST_LOG=info ./midnight-proof-server-prototype

# Debug (verbose)
RUST_LOG=debug ./midnight-proof-server-prototype

# Component-specific
RUST_LOG=midnight_proof_server_prototype=debug,tower_http=info
```

**Log Format:**
```
2025-12-29T13:00:00.123Z INFO midnight_proof_server_prototype: Starting server
2025-12-29T13:00:00.124Z INFO midnight_proof_server_prototype: Listening on https://0.0.0.0:6300
2025-12-29T13:00:01.456Z INFO midnight_proof_server_prototype: Received Ctrl+C signal
2025-12-29T13:00:01.457Z INFO midnight_proof_server_prototype: Initiating graceful shutdown...
```

### Metrics

Built-in metrics available via logs:

- **Queue Depth:** Number of pending jobs
- **Active Workers:** Worker utilization
- **Job Duration:** Time to generate proof
- **Memory Usage:** Per-job memory consumption
- **Rate Limit Hits:** Number of rate-limited requests

**Future:** Prometheus metrics endpoint (planned)

---

## Development

### Project Structure

```
proof-server/
├── src/
│   ├── main.rs           # Entry point, CLI, server startup
│   ├── lib.rs            # API routes, handlers, state
│   ├── tls.rs            # TLS certificate generation
│   └── worker_pool.rs    # Proof generation workers
├── Cargo.toml            # Dependencies
├── README.md             # This file
└── TLS-SETUP.md         # Detailed TLS documentation
```

### Building for Development

```bash
# Debug build (fast compilation, slower runtime)
cargo build

# Release build (optimized)
cargo build --release

# Check for errors without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

### Running Tests

```bash
# Unit tests
cargo test

# Integration tests
cargo test --test '*'

# Specific test
cargo test test_health_endpoint

# With output
cargo test -- --nocapture
```

### Code Style

This project follows standard Rust conventions:

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Lint
cargo clippy -- -D warnings
```

---

## Troubleshooting

### Common Issues

#### Issue: Server won't start

**Symptoms:**
```
Error: Address already in use (os error 48)
```

**Solution:**
```bash
# Check if port is in use
lsof -i :6300

# Kill process using port
kill -9 $(lsof -t -i:6300)

# Or use different port
./midnight-proof-server-prototype --port 6301
```

---

#### Issue: TLS certificate not found

**Symptoms:**
```
Error: Certificate file not found: certs/cert.pem
```

**Solution:**
```bash
# Generate self-signed certificate
./midnight-proof-server-prototype --auto-generate-cert

# Or manually create certs directory
mkdir -p certs
openssl req -x509 -newkey rsa:4096 -nodes \
  -keyout certs/key.pem -out certs/cert.pem -days 365
```

---

#### Issue: API key not accepted

**Symptoms:**
```
HTTP 401 Unauthorized
{"error": "Invalid API key"}
```

**Solution:**
```bash
# Check API key format (no spaces, special chars escaped)
echo $MIDNIGHT_PROOF_SERVER_API_KEY

# Test with curl
curl -H "X-API-Key: your-key" https://localhost:6300/k

# Verify key hash in logs (debug mode)
RUST_LOG=debug ./midnight-proof-server-prototype
```

---

#### Issue: Rate limited

**Symptoms:**
```
HTTP 429 Too Many Requests
{"error": "Rate limit exceeded"}
```

**Solution:**
```bash
# Increase rate limit
./midnight-proof-server-prototype --rate-limit 50

# Or wait for rate limit window to reset (typically 1 second)
```

---

#### Issue: Out of memory

**Symptoms:**
```
Error: Cannot allocate memory
Server killed by OOM killer
```

**Solution:**
```bash
# Reduce worker count
./midnight-proof-server-prototype --num-workers 8

# Increase system memory
# Or deploy on larger instance

# Monitor memory usage
RUST_LOG=debug ./midnight-proof-server-prototype
# Check logs for memory reports
```

---

#### Issue: Slow proof generation

**Symptoms:**
- High latency
- Timeouts
- Job queue backing up

**Solution:**
```bash
# Increase workers
./midnight-proof-server-prototype --num-workers 32

# Increase timeout
./midnight-proof-server-prototype --job-timeout 900

# Check CPU usage
top -p $(pgrep midnight-proof)

# Pre-fetch parameters
# (don't use --no-fetch-params)
```

---

### Debug Mode

Enable verbose logging for troubleshooting:

```bash
RUST_LOG=debug ./midnight-proof-server-prototype --verbose
```

**Debug output includes:**
- Request details
- API key hash verification
- Memory usage per job
- Worker pool status
- TLS handshake details
- Rate limiter state

---

### Getting Help

- 📖 **Documentation:** [Parent README](../README.md)
- 📖 **TLS Setup:** [TLS-SETUP.md](TLS-SETUP.md)
- 📖 **Deployment Guides:** [../docs/](../docs/)
- 🐛 **Issues:** GitHub Issues
- 💬 **Discord:** Midnight Community

---

## Dependencies

### Core Dependencies

- **axum** 0.8.8 - Web framework
- **axum-server** 0.8.0 - TLS/HTTPS support
- **tower** 0.5.2 - Service middleware
- **tower-http** 0.6.8 - HTTP middleware (CORS, tracing)
- **tokio** 1.48 - Async runtime
- **hyper** 1.8 - HTTP implementation

### Security Dependencies

- **rcgen** 0.13 - Certificate generation
- **sha2** 0.10 - API key hashing
- **governor** 0.6 - Rate limiting

### Serialization

- **serde** 1.0 - Serialization framework
- **serde_json** 1.0 - JSON support
- **bincode** 1.3 - Binary serialization

### Midnight Dependencies

- **midnight-ledger** - Core ledger types
- **midnight-zswap** - ZSwap protocol
- **midnight-base-crypto** - Cryptographic primitives
- **midnight-transient-crypto** - Proof generation
- **midnight-storage** - State management

For complete dependency list, see [Cargo.toml](Cargo.toml).

---

## License

Apache License 2.0

Copyright (C) 2025 Midnight Foundation

See [LICENSE](../../LICENSE) for details.

---

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for version history and updates.

---

## Contributing

Contributions welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Run `cargo fmt` and `cargo clippy`
6. Submit a pull request

---

## Version

**Current Version:** 6.2.0-alpha.1

**Status:** Alpha - Not recommended for production without thorough security review

**Last Updated:** December 29, 2025

---

**⚠️ SECURITY NOTICE:** This is an alpha release. Always use authentication (`--api-key`) in production. Never use `--disable-auth` except for local development.
