# Midnight Proof Server

Zero-knowledge proof generation server with GPU acceleration support via ICICLE v4.0.0.

## Quick Start

```bash
# 1. Install ICICLE v4.0.0 CUDA backend
wget https://github.com/ingonyama-zk/icicle/releases/download/v4.0.0/icicle_4_0_0-ubuntu22-cuda122.tar.gz
sudo tar -xzf icicle_4_0_0-ubuntu22-cuda122.tar.gz -C /opt

# 2. Build with GPU support
cd proof-server
FEATURES="gpu" ./manage-proof-server.sh build

# 3. Start server
./manage-proof-server.sh start

# 4. Verify GPU is working
./manage-proof-server.sh check-gpu
```

## Prerequisites

### Required
- Rust 1.83.0+
- NVIDIA GPU with CUDA support (for GPU acceleration)
- NVIDIA drivers 525.60.13+
- CUDA toolkit 12.2+

### Recommended
- Ubuntu 22.04 LTS
- 8GB+ GPU VRAM for K≥16 circuits
- 16GB+ system RAM

## Installation

### 1. NVIDIA Drivers & CUDA

```bash
# Check existing installation
nvidia-smi
nvcc --version

# Install if needed
sudo apt update
sudo apt install nvidia-driver-565 nvidia-cuda-toolkit
sudo reboot
```

### 2. ICICLE Backend

ICICLE consists of two parts:
- **Frontend**: Rust bindings (MIT license) - built automatically by cargo
- **Backend**: Pre-built CUDA kernels (special license) - must be downloaded

```bash
# Download pre-built backend for Ubuntu 22.04 + CUDA 12.2+
wget https://github.com/ingonyama-zk/icicle/releases/download/v4.0.0/icicle_4_0_0-ubuntu22-cuda122.tar.gz

# Extract to /opt
sudo tar -xzf icicle_4_0_0-ubuntu22-cuda122.tar.gz -C /opt

# Verify installation
ls -lh /opt/icicle/lib/backend/bls12_381/cuda/
# Should show:
# libicicle_bls12_381_cuda.so (66MB)
# libingo_bls12_381_field_cuda.so (54MB)
```

### 3. Build Proof Server

```bash
# With GPU support (recommended)
FEATURES="gpu" ./manage-proof-server.sh build

# CPU-only (no GPU)
./manage-proof-server.sh build
```

## Server Management

### Basic Commands

```bash
# Start server
./manage-proof-server.sh start

# Stop server
./manage-proof-server.sh stop

# Restart server
./manage-proof-server.sh restart

# Check status
./manage-proof-server.sh status

# View logs
./manage-proof-server.sh logs

# Verify GPU support
./manage-proof-server.sh check-gpu
```

### Configuration

Set environment variables or create `proof-server.conf`:

```bash
# Server settings
PORT=6300
VERBOSE=false
NUM_WORKERS=2
JOB_CAPACITY=0
JOB_TIMEOUT=600.0

# Build settings
FEATURES="gpu"
CARGO_PROFILE=release
```

### Advanced Usage

```bash
# Custom port
./manage-proof-server.sh start --port 8080

# More workers
./manage-proof-server.sh start --workers 4

# Verbose logging
./manage-proof-server.sh start --verbose

# Monitor with auto-restart
./manage-proof-server.sh monitor

# Show metrics
./manage-proof-server.sh metrics

# Test API endpoints
./manage-proof-server.sh test-api
```

## API Endpoints

### Health & Status

```bash
# Health check
curl http://localhost:6300/health

# Readiness check
curl http://localhost:6300/ready

# Version info
curl http://localhost:6300/version

# Supported proof versions
curl http://localhost:6300/proof-versions
```

### Proof Generation

```bash
# Generate proof
curl -X POST http://localhost:6300/prove \
  -H "Content-Type: application/json" \
  -d @proof_request.json
```

## GPU Acceleration

### Performance

Benchmarked on NVIDIA GeForce RTX 5060 (8GB VRAM):

- GPU threshold: K≥14 (16,384 constraints)
- Estimated speedup: 2x for large circuits
- VRAM growth: ~1.17x per K level

### Verification

```bash
# Check GPU availability
./manage-proof-server.sh check-gpu

# Run GPU benchmark (CPU mode)
cargo test -p midnight-proof-server --release --test gpu_proof_benchmark -- --nocapture

# Run GPU benchmark (with GPU)
cargo test -p midnight-proof-server --release --features gpu --test gpu_proof_benchmark -- --nocapture
```

**Note on SRS Files:** The benchmark tests use Filecoin trusted setup files for faster and more reliable proof generation. If these files are not found in `../midnight-zk/circuits/examples/assets/`, the tests will automatically fall back to `unsafe_setup()` which generates parameters on-the-fly. This fallback works fine for testing but is slower and should not be used in production.

To use Filecoin SRS files:
```bash
# Download Filecoin trusted setup (optional, improves test performance)
# Place files in: midnight-zk/circuits/examples/assets/
# Files: bls_filecoin_2p10, bls_filecoin_2p12, bls_filecoin_2p14, etc.
```

Expected output:
```
✓ GPU Backend: ENABLED (ICICLE CUDA)
✓ NVIDIA GPU detected: NVIDIA GeForce RTX 5060
✓ Binary linked with ICICLE GPU libraries
```

### Backend not found

**Check installation:**
```bash
ls /opt/icicle/lib/backend/bls12_381/cuda/

# Should show .so files
# If missing, reinstall ICICLE backend
```

### Binary not linked with ICICLE

**Fix:** Rebuild with GPU features
```bash
cargo clean
FEATURES="gpu" ./manage-proof-server.sh build
```

### Server won't start

**Check logs:**
```bash
tail -f /tmp/midnight-proof-server.log
```

**Common issues:**
- Port already in use: Change port with `--port`
- Missing binary: Run `./manage-proof-server.sh build`
- Permission errors: Check file permissions

## ICICLE Architecture

### Two-Part System

1. **Frontend (Rust Wrappers)**
   - MIT licensed, open source
   - Built automatically by cargo
   - Located in `target/release/deps/icicle/lib/`

2. **Backend (CUDA Kernels)**
   - Special license, closed source
   - Pre-built binaries from GitHub releases
   - Located in `/opt/icicle/lib/backend/`

### Library Loading

The frontend dynamically loads the backend at runtime. The `manage-proof-server.sh` script automatically sets `LD_LIBRARY_PATH` to:

1. Cargo-built frontend: `target/release/deps/icicle/lib`
2. System backend: `/opt/icicle/lib/backend`
3. CUDA libraries: `/usr/local/cuda/lib64`

### Platform Support

Available ICICLE v4.0.0 releases:
- `icicle_4_0_0-ubuntu22-cuda122.tar.gz` - Ubuntu 22.04 + CUDA 12.2+
- `icicle_4_0_0-ubuntu20-cuda122.tar.gz` - Ubuntu 20.04 + CUDA 12.2+
- `icicle_4_0_0-ubuntu22.tar.gz` - Ubuntu 22.04 CPU-only
- `icicle_4_0_0-ubuntu20.tar.gz` - Ubuntu 20.04 CPU-only

## Production Deployment

### Systemd Service

```bash
# Copy service file
sudo cp midnight-proof-server.service /etc/systemd/system/

# Edit paths in service file
sudo nano /etc/systemd/system/midnight-proof-server.service

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable midnight-proof-server
sudo systemctl start midnight-proof-server

# Check status
sudo systemctl status midnight-proof-server
```

### Security Hardening

**Run as dedicated user:**
```bash
sudo useradd -r -s /bin/false midnight-proof
sudo chown -R midnight-proof:midnight-proof /path/to/proof-server
```

**Update service file:**
```ini
[Service]
User=midnight-proof
Group=midnight-proof
```

**Firewall:**
```bash
# Allow only from trusted IPs
sudo ufw allow from 10.0.0.0/8 to any port 6300
```

### Monitoring

**Health checks:**
```bash
# Add to cron or monitoring system
*/5 * * * * curl -f http://localhost:6300/health || systemctl restart midnight-proof-server
```

**Metrics:**
```bash
# CPU/Memory usage
./manage-proof-server.sh metrics

# GPU usage
nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv
```

**Logs:**
```bash
# Systemd logs
journalctl -u midnight-proof-server -f

# Application logs
tail -f /tmp/midnight-proof-server.log
```

## Performance Tuning

### Worker Configuration

```bash
# Adjust based on CPU cores and workload
NUM_WORKERS=4 ./manage-proof-server.sh start
```

### Job Queue

```bash
# Limit concurrent jobs
JOB_CAPACITY=10 ./manage-proof-server.sh start

# Adjust timeout for long proofs
JOB_TIMEOUT=1200.0 ./manage-proof-server.sh start
```

### GPU Optimization

**For multiple GPUs:**
```bash
# Set specific GPU
CUDA_VISIBLE_DEVICES=0 ./manage-proof-server.sh start
```

**For shared GPU:**
```bash
# Limit GPU memory
CUDA_DEVICE_MAX_CONNECTIONS=1 ./manage-proof-server.sh start
```

## Build Features

Available cargo features:

- `gpu` - Enable GPU acceleration (ICICLE backend)
- `trace-msm` - Enable MSM operation tracing
- `trace-fft` - Enable FFT operation tracing
- `trace-phases` - Enable proof phase tracing
- `trace-kzg` - Enable KZG commitment tracing
- `trace-all` - Enable all tracing features

**Example:**
```bash
FEATURES="gpu,trace-all" ./manage-proof-server.sh build
```

## Licensing

- **Proof server code**: MIT
- **ICICLE frontend**: MIT (Rust bindings)
- **ICICLE backend**: Special license (CUDA kernels)
  - Review: https://github.com/ingonyama-zk/icicle/blob/main/LICENSE
  - Not covered by MIT license
  - Review terms before production use

## Dependencies

- **midnight-zk v5.0.1**: GitHub fork at https://github.com/riusricardo/midnight-zk
- **ICICLE v4.0.0**: GPU acceleration library
- **Actix-web 4.11.0**: Web framework
- **CUDA 12.2+**: NVIDIA GPU support

## Development

### Run tests

```bash
# Unit tests
cargo test --package midnight-proof-server

# GPU benchmark tests
cargo test --release --package midnight-proof-server --features gpu --test gpu_proof_benchmark -- --nocapture

# Integration tests
./manage-proof-server.sh test-api
```

### Debug mode

```bash
# Build debug version
CARGO_PROFILE=debug ./manage-proof-server.sh build

# Start with verbose logging
./manage-proof-server.sh start --verbose
```

### Clean build

```bash
# Clean and rebuild
cargo clean
FEATURES="gpu" ./manage-proof-server.sh build
```

## References

- ICICLE: https://github.com/ingonyama-zk/icicle
- Midnight ZK Fork: https://github.com/riusricardo/midnight-zk
- CUDA Toolkit: https://developer.nvidia.com/cuda-downloads
