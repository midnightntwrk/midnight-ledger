# Midnight Proof Server

Zero-knowledge proof generation server for the Midnight blockchain with TEE attestation support.

## Overview

This is the core proof server implementation that generates zero-knowledge proofs for Midnight blockchain transactions. It's designed to run in Trusted Execution Environments (TEEs) with cryptographic attestation capabilities.

### Features

- **High-Performance Async I/O** - Built on Axum and Tokio
- **Worker Pool Architecture** - Configurable multi-threaded proof generation
- **TEE Attestation** - AWS Nitro, GCP Confidential, Azure Confidential
- **Security First** - API key auth, rate limiting, request validation
- **Production Ready** - Structured logging, health checks, graceful shutdown

## Building

### Prerequisites

This proof server is part of the midnight-ledger workspace and requires no additional dependencies beyond what's in the main repository.

```bash
# Rust toolchain (1.75+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone the midnight-ledger repository
git clone https://github.com/midnight/midnight-ledger.git
cd midnight-ledger
```

### Build Commands

```bash
# From the midnight-ledger root - build just the proof server
cargo build --release -p midnight-proof-server-prototype

# Or from the tee-proof-server-proto directory
cd tee-proof-server-proto
./build.sh

# Development build
cargo build -p midnight-proof-server-prototype

# Run tests
cargo test -p midnight-proof-server-prototype

# Check without building
cargo check -p midnight-proof-server-prototype
```

### Build Script

From the `tee-proof-server-proto` directory:

```bash
# Convenience build script
./build.sh
```

This creates: `./proof-server/target/release/midnight-proof-server-prototype`

## Running

### Basic Usage

```bash
# Development mode (no authentication)
./target/release/midnight-proof-server-prototype \
  --port 6300 \
  --disable-auth

# Production mode (with API key)
./target/release/midnight-proof-server-prototype \
  --port 6300 \
  --api-key "your-secure-api-key"

# View all options
./target/release/midnight-proof-server-prototype --help
```

### Configuration Options

| Option | Environment Variable | Default | Description |
|--------|---------------------|---------|-------------|
| `--port` | `MIDNIGHT_PROOF_SERVER_PORT` | `6300` | Server port |
| `--api-key` | `MIDNIGHT_PROOF_SERVER_API_KEY` | None | API key(s), comma-separated |
| `--disable-auth` | `MIDNIGHT_PROOF_SERVER_DISABLE_AUTH` | `false` | Disable auth (dev only) |
| `--rate-limit` | `MIDNIGHT_PROOF_SERVER_RATE_LIMIT` | `10` | Requests/sec per IP |
| `--max-payload-size` | `MIDNIGHT_PROOF_SERVER_MAX_PAYLOAD_SIZE` | `10485760` | Max request size (bytes) |
| `--num-workers` | `MIDNIGHT_PROOF_SERVER_NUM_WORKERS` | `16` | Worker threads |
| `--job-capacity` | `MIDNIGHT_PROOF_SERVER_JOB_CAPACITY` | `0` | Job queue size (0=unlimited) |

### Environment Variables

```bash
# Set via environment
export MIDNIGHT_PROOF_SERVER_PORT=6300
export MIDNIGHT_PROOF_SERVER_API_KEY="my-secret-key"
export RUST_LOG=info

./target/release/midnight-proof-server-prototype
```

### Multiple API Keys

```bash
# Comma-separated for multiple keys
./target/release/midnight-proof-server-prototype \
  --api-key "key1,key2,key3"
```

## API Reference

### Endpoints

#### Health Check

```bash
GET /health
```

**Response:**
```json
{
  "status": "ok",
  "timestamp": "2025-12-19T20:00:00Z"
}
```

#### Version Information

```bash
GET /version
```

**Response:**
```json
{
  "version": "6.2.0-alpha.1",
  "build_date": "2025-12-19",
  "git_commit": "abc123..."
}
```

#### Proof Generation

```bash
POST /prove
Content-Type: application/json
X-API-Key: your-api-key

{
  "transaction": "base64-encoded-transaction-data",
  "options": {
    "priority": "normal"
  }
}
```

**Response (Success - 200):**
```json
{
  "proof": "base64-encoded-proof-data",
  "job_id": "uuid-v4",
  "duration_ms": 1234
}
```

**Response (Error - 400/500):**
```json
{
  "error": "Error description",
  "code": "ERROR_CODE"
}
```

#### TEE Attestation

```bash
GET /attestation?nonce=hexadecimal-nonce
```

**Query Parameters:**
- `nonce` (optional): Hex-encoded nonce for freshness (32+ bytes recommended)

**Response (Development):**
```json
{
  "platform": "Development/Unknown",
  "format": "N/A",
  "nonce": "abc123...",
  "error": "Not running in a recognized TEE environment",
  "metadata": {
    "message": "Attestation is only available in production TEE deployments",
    "supported_platforms": ["AWS Nitro Enclaves", "GCP Confidential VM", "Azure Confidential VM"]
  }
}
```

**Response (AWS Nitro):**
```json
{
  "platform": "AWS Nitro Enclaves",
  "format": "CBOR",
  "nonce": "abc123...",
  "attestation": "base64-encoded-attestation-document",
  "metadata": {
    "pcr0": "sha384-hash...",
    "pcr1": "sha384-hash...",
    "pcr2": "sha384-hash..."
  }
}
```

### Authentication

All endpoints except `/health` and `/version` require authentication in production mode.

**Header:**
```
X-API-Key: your-api-key
```

**Example:**
```bash
curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: my-secret-key" \
  -H "Content-Type: application/json" \
  -d '{"transaction":"..."}'
```

### Rate Limiting

Per-IP rate limiting (default: 10 req/sec):

**Response (429):**
```json
{
  "error": "Rate limit exceeded",
  "retry_after": 60
}
```

### Error Codes

| Code | Status | Description |
|------|--------|-------------|
| `INVALID_REQUEST` | 400 | Malformed request |
| `UNAUTHORIZED` | 401 | Missing or invalid API key |
| `RATE_LIMIT_EXCEEDED` | 429 | Too many requests |
| `PROOF_GENERATION_FAILED` | 500 | Proof generation error |
| `INTERNAL_ERROR` | 500 | Server error |

## Architecture

### Components

```
┌─────────────────────────────────────────┐
│           HTTP Server (Axum)            │
│  - Request routing                      │
│  - Authentication middleware            │
│  - Rate limiting middleware             │
│  - CORS, tracing, timeouts              │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│          API Handlers (lib.rs)          │
│  - /health, /version, /prove            │
│  - /attestation                         │
│  - Request validation                   │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│      Worker Pool (worker_pool.rs)       │
│  - Job queue (async-channel)            │
│  - Worker threads (configurable)        │
│  - Proof generation tasks               │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│   Attestation Module (attestation.rs)   │
│  - Platform detection                   │
│  - AWS Nitro attestation                │
│  - GCP/Azure attestation                │
└─────────────────────────────────────────┘
```

### Worker Pool

The server uses a multi-threaded worker pool for proof generation:

- **Job Queue**: Async channel for incoming proof requests
- **Workers**: Dedicated threads running proof generation
- **Capacity**: Configurable queue depth (0 = unlimited)
- **Concurrency**: Workers process jobs in parallel

### State Management

```rust
pub struct AppState {
    pub security_config: Arc<SecurityConfig>,
    pub rate_limiter: Arc<RateLimiter>,
    pub worker_pool: Arc<Mutex<WorkerPool>>,
}
```

## Development

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_name

# With logging
RUST_LOG=debug cargo test

# Integration tests only
cargo test --test '*'
```

### Code Structure

```
src/
├── main.rs           # Entry point, CLI parsing, server startup
├── lib.rs            # API routes, handlers, middleware
├── attestation.rs    # TEE attestation logic
└── worker_pool.rs    # Proof generation worker pool
```

### Adding New Endpoints

1. Define handler in `lib.rs`:
```rust
async fn my_endpoint(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Response>, StatusCode> {
    // Handler logic
}
```

2. Add route in `create_app()`:
```rust
.route("/my-endpoint", get(my_endpoint))
```

3. Add authentication middleware if needed

### Logging

The server uses structured logging via `tracing`:

```rust
use tracing::{info, warn, error, debug};

info!("Server started on port {}", port);
warn!("Running with authentication disabled");
error!("Failed to generate proof: {}", err);
debug!("Request details: {:?}", request);
```

Set log level:
```bash
export RUST_LOG=debug  # or info, warn, error
```

### Performance Tuning

**Worker Count:**
- Default: 16 workers
- Recommended: 1-2x CPU cores
- Adjust based on proof complexity

**Queue Capacity:**
- Default: 0 (unlimited)
- Set limit to prevent memory exhaustion
- Monitor queue depth

**Rate Limiting:**
- Default: 10 req/sec per IP
- Adjust based on capacity
- Consider per-key limits

## Testing

### Manual Testing

```bash
# From repository root
../test-server.sh        # Full test suite
../test-prove-endpoint.sh  # Proof endpoint only
```

### Example Requests

```bash
# Health check
curl http://localhost:6300/health

# Version
curl http://localhost:6300/version

# Attestation
curl "http://localhost:6300/attestation?nonce=$(openssl rand -hex 32)"

# Proof (requires API key in production)
curl -X POST http://localhost:6300/prove \
  -H "X-API-Key: test-key" \
  -H "Content-Type: application/json" \
  -d '{
    "transaction": "base64-encoded-data"
  }'
```

## Deployment

### Docker

Create `Dockerfile`:

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/midnight-proof-server-prototype /usr/local/bin/
EXPOSE 6300
CMD ["midnight-proof-server-prototype", "--port", "6300"]
```

Build and run:
```bash
docker build -t midnight-proof-server .
docker run -p 6300:6300 \
  -e MIDNIGHT_PROOF_SERVER_API_KEY="your-key" \
  midnight-proof-server
```

### TEE Deployment

See platform-specific guides:
- [AWS Nitro Enclaves](../docs/deploy-aws-nitro.md)
- [GCP Confidential VMs](../docs/deploy-gcp-confidential.md)
- [Azure Confidential VMs](../docs/deploy-azure-confidential.md)

## Security

### Production Checklist

- [ ] API key authentication enabled (`--api-key`)
- [ ] `--disable-auth` flag NOT used
- [ ] Rate limiting configured appropriately
- [ ] Running in TEE environment
- [ ] Debug mode disabled
- [ ] TLS termination (via nginx/ALB)
- [ ] Monitoring and alerting configured
- [ ] Regular security updates

### Security Best Practices

1. **API Keys**: Use cryptographically random keys (32+ bytes)
   ```bash
   openssl rand -base64 32
   ```

2. **TLS**: Always use HTTPS in production (terminate at proxy/ALB)

3. **Rate Limiting**: Adjust based on capacity and abuse patterns

4. **Monitoring**: Watch for failed auth attempts, rate limit hits

5. **Updates**: Regularly update dependencies
   ```bash
   cargo update
   cargo audit
   ```

## Troubleshooting

### Build Issues

**Missing dependencies:**
```bash
# Check Midnight ledger paths
ls ../../tee-prover/midnight-ledger/

# Verify Rust version
rustc --version  # Should be 1.75+
```

**Link errors:**
```bash
# Clean and rebuild
cargo clean
cargo build --release
```

### Runtime Issues

**Server won't start:**
```bash
# Check port availability
lsof -i :6300

# Increase log level
RUST_LOG=debug ./target/release/midnight-proof-server-prototype --disable-auth
```

**Memory issues:**
```bash
# Reduce workers
--num-workers 8

# Limit queue
--job-capacity 100
```

**Performance issues:**
```bash
# Increase workers
--num-workers 32

# Check system resources
top
htop
```

## Dependencies

Key dependencies (see `Cargo.toml` for complete list):

- **axum** - Web framework
- **tokio** - Async runtime
- **tower** / **tower-http** - Middleware
- **serde** / **serde_json** - Serialization
- **tracing** - Structured logging
- **governor** - Rate limiting
- **clap** - CLI parsing

Midnight ledger dependencies:
- **midnight-ledger** - Core ledger functionality
- **midnight-zswap** - Zero-knowledge swap
- **midnight-base-crypto** - Cryptographic primitives
- **midnight-transient-crypto** - Transient keys
- **midnight-storage** - Storage abstraction
- **zkir** - Zero-knowledge IR

## License

Apache License 2.0

Copyright (C) 2025 Midnight Foundation
