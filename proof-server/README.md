# Midnight Proof Server

PLONK proof generation server with GPU acceleration support.

## Server Management

The `manage-proof-server.sh` script controls the proof server lifecycle:

```sh
# Start the server (runs in background)
MIDNIGHT_DEVICE=auto ./manage-proof-server.sh start

# Check if server is running
./manage-proof-server.sh status

# View server logs (follow mode)
./manage-proof-server.sh logs

# Stop the server
./manage-proof-server.sh stop

# Restart the server
./manage-proof-server.sh restart
```

The script manages a background process that listens on port 6300 (configurable via `MIDNIGHT_PROOF_SERVER_PORT`).

## Device Modes

Configure proof generation backend with the `MIDNIGHT_DEVICE` environment variable:

- `cpu` - BLST CPU backend for all circuits
- `gpu` - ICICLE CUDA backend for all circuits (requires CUDA-capable GPU)
- `auto` - Hybrid mode that selects backend per circuit based on K value

### Auto Mode (Recommended)

Auto mode dynamically selects the optimal backend:
- Uses GPU for K≥14 circuits where parallelization provides significant speedup
- Uses BLST CPU for K<14 circuits to avoid GPU memory transfer overhead
- Lazily initializes ICICLE backend only when needed (singleton pattern)
- Configure threshold with `MIDNIGHT_GPU_MIN_K` (default: 14)

Example with custom threshold:
```sh
MIDNIGHT_DEVICE=auto MIDNIGHT_GPU_MIN_K=15 ./manage-proof-server.sh start
```

## Debugging and Tracing

### Enable Detailed Logging

Set `RUST_LOG` to see proof generation steps:

```sh
# Info level (recommended)
RUST_LOG=info MIDNIGHT_DEVICE=auto ./manage-proof-server.sh start

# Debug level (verbose, shows circuit details)
RUST_LOG=debug MIDNIGHT_DEVICE=auto ./manage-proof-server.sh start

# Trace level (very verbose, shows all internal operations)
RUST_LOG=trace MIDNIGHT_DEVICE=auto ./manage-proof-server.sh start
```

### Follow Proof Generation Steps

View logs in real-time to track each proof generation phase:

```sh
# In one terminal, start server with debug logging
RUST_LOG=debug MIDNIGHT_DEVICE=auto ./manage-proof-server.sh start

# In another terminal, follow logs
./manage-proof-server.sh logs

# Send a proof request (from examples or tests)
MIDNIGHT_LEDGER_TEST_STATIC_DIR=$PWD/zkir-precompiles \
  cargo run --release -p midnight-proof-server --features gpu --example send_zswap_proof
```

The logs will show:
- Circuit loading and verification
- Backend selection (CPU vs GPU)
- Witness generation
- Proof computation phases
- Response serialization

## Key Generation

```sh
# Build zkir tool
cargo build --release -p zkir --features binary

# Generate keys for a contract
cd zkir-precompiles/<contract-name>
mkdir -p zkir keys
cp *.bzkir zkir/
cd ../..
./target/release/zkir compile-many zkir-precompiles/<contract-name> zkir-precompiles/<contract-name>/keys
```

## Running Tests

```sh
# Run micro-dao test with GPU auto mode
MIDNIGHT_DEVICE=auto \
MIDNIGHT_GPU_MIN_K=14 \
MIDNIGHT_LEDGER_TEST_STATIC_DIR=$PWD/zkir-precompiles \
cargo test --release -p midnight-ledger --features "proving test-utilities gpu" micro_dao
```

## Examples

### Send Single Proof Request

Sends a zswap transaction (K=14) to the running server:

```sh
MIDNIGHT_LEDGER_TEST_STATIC_DIR=$PWD/zkir-precompiles \
  cargo run --release -p midnight-proof-server --features gpu --example send_zswap_proof
```

### Benchmark Proof Generation

Sends multiple proof requests and measures throughput:

```sh
MIDNIGHT_LEDGER_TEST_STATIC_DIR=$PWD/zkir-precompiles \
  cargo run --release -p midnight-proof-server --features gpu --example benchmark_proof_server
```

See individual example files in `examples/` for detailed prerequisites and usage instructions.
