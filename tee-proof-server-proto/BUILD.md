# Build Instructions - Midnight TEE Proof Server

Complete guide to building the Midnight TEE Proof Server from the midnight-ledger repository.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Quick Build](#quick-build)
3. [Detailed Build Process](#detailed-build-process)
4. [Workspace Integration](#workspace-integration)
5. [Troubleshooting](#troubleshooting)
6. [Advanced Build Options](#advanced-build-options)

---

## Prerequisites

### System Requirements

- **Operating System**: Linux, macOS, or Windows (with WSL2)
- **CPU**: x86_64 architecture (ARM64 supported with Rosetta on macOS)
- **RAM**: 8GB minimum, 16GB+ recommended for faster builds
- **Disk Space**: ~10GB for full build (including dependencies and target artifacts)

### Required Software

#### 1. Rust Toolchain

```bash
# Install Rust (version 1.75 or later required)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version  # Should be 1.75.0 or later
cargo --version
```

#### 2. Git

```bash
# macOS (via Homebrew)
brew install git

# Ubuntu/Debian
sudo apt-get install git

# Verify installation
git --version
```

#### 3. Build Tools

**macOS:**
```bash
xcode-select --install
```

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install build-essential pkg-config libssl-dev
```

**Fedora/RHEL:**
```bash
sudo dnf install gcc gcc-c++ make pkgconfig openssl-devel
```

---

## Quick Build

### Step 1: Clone the Repository

```bash
# Clone the midnight-ledger repository
git clone https://github.com/midnight/midnight-ledger.git
cd midnight-ledger
```

⚠️ **IMPORTANT**: The TEE Proof Server **must** be built from within the midnight-ledger repository. It depends on workspace crates that are part of the main repository.

### Step 2: Build

**Option A: Use the build script (recommended)**

```bash
cd tee-proof-server-proto
./build.sh
```

**Option B: Use cargo directly**

```bash
# From the midnight-ledger root
cargo build --release -p midnight-proof-server-prototype
```

### Step 3: Verify Build

```bash
# Check binary exists
ls -lh tee-proof-server-proto/proof-server/target/release/midnight-proof-server-prototype

# Test binary
cd tee-proof-server-proto
./test-server.sh
```

---

## Detailed Build Process

### Understanding the Project Structure

```
midnight-ledger/                    # Main repository root
├── Cargo.toml                      # Workspace configuration
├── ledger/                         # Core ledger implementation
├── zswap/                          # Zero-knowledge swap
├── base-crypto/                    # Cryptographic primitives
├── transient-crypto/               # Transient key crypto
├── storage/                        # Storage abstraction
├── serialize/                      # Serialization utilities
├── zkir/                           # Zero-knowledge IR
└── tee-proof-server-proto/        # TEE Proof Server
    ├── build.sh                    # Build script
    ├── proof-server/               # Server implementation
    │   ├── Cargo.toml             # Dependencies
    │   └── src/
    │       ├── main.rs            # Entry point
    │       ├── lib.rs             # API implementation
    │       ├── attestation.rs     # TEE attestation
    │       └── worker_pool.rs     # Proof generation
    └── docs/                       # Documentation
```

### Build Dependencies

The proof server depends on these workspace crates (automatically resolved):

| Crate | Purpose |
|-------|---------|
| `midnight-ledger` | Core ledger functionality and proving |
| `midnight-zswap` | Zero-knowledge swap operations |
| `midnight-base-crypto` | Cryptographic primitives |
| `midnight-transient-crypto` | Transient key management |
| `midnight-storage` | Storage abstraction layer |
| `midnight-serialize` | Serialization utilities |
| `zkir` | Zero-knowledge intermediate representation |

All dependencies are resolved via relative paths in `Cargo.toml`:

```toml
[dependencies]
ledger = { path = "../../ledger", package = "midnight-ledger", ... }
zswap = { path = "../../zswap", package = "midnight-zswap" }
# ... etc
```

### Build Process Steps

#### 1. Workspace Resolution

Cargo detects the workspace root (`midnight-ledger/Cargo.toml`) and includes:

```toml
[workspace]
members = [
    "ledger",
    "zswap",
    "tee-proof-server-proto/proof-server",
    # ... other members
]
```

#### 2. Dependency Resolution

Cargo resolves all dependencies, including:
- External crates from crates.io
- Workspace member crates (ledger, zswap, etc.)
- Git dependencies (midnight-zk circuits)

#### 3. Compilation

```
1. Build workspace dependencies (ledger, zswap, etc.)
   → Compiles in dependency order
   → Shared compilation cache across workspace

2. Build proof-server crate
   → Links against workspace dependencies
   → Applies release optimizations

3. Generate binary
   → Output: target/release/midnight-proof-server-prototype
```

#### 4. Optimization

Release builds apply aggressive optimizations:

```toml
[profile.release]
opt-level = 3          # Maximum optimization
lto = true             # Link-time optimization
codegen-units = 1      # Single codegen unit for better optimization
strip = true           # Strip debug symbols
```

Typical build times:
- **Clean build**: 10-20 minutes (depending on CPU)
- **Incremental build**: 1-3 minutes
- **No changes**: 5-10 seconds

---

## Workspace Integration

### How the Workspace Works

The midnight-ledger repository uses Cargo workspaces to manage multiple related crates. The TEE Proof Server is integrated as a workspace member.

#### Benefits

1. **Shared Dependencies**: All workspace members use the same dependency versions
2. **Unified Build**: `cargo build` compiles all workspace members together
3. **Shared Target Directory**: Compilation artifacts are shared, reducing disk usage
4. **Cross-Crate Optimizations**: LTO can optimize across workspace boundaries

#### Building Specific Crates

```bash
# Build only the proof server
cargo build --release -p midnight-proof-server-prototype

# Build with dependencies
cargo build --release -p midnight-proof-server-prototype --all-features

# Build entire workspace
cargo build --release

# Build specific workspace member
cargo build --release -p midnight-ledger
```

#### Testing

```bash
# Test only proof server
cargo test -p midnight-proof-server-prototype

# Test entire workspace
cargo test --workspace

# Test with specific features
cargo test -p midnight-proof-server-prototype --features proving
```

---

## Troubleshooting

### Common Build Issues

#### Issue 1: Cargo Not Found

**Symptom:**
```
bash: cargo: command not found
```

**Solution:**
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Reload shell
source $HOME/.cargo/env

# Verify
cargo --version
```

#### Issue 2: Wrong Rust Version

**Symptom:**
```
error: package requires rustc 1.75.0 or newer
```

**Solution:**
```bash
# Update Rust
rustup update stable

# Set as default
rustup default stable

# Verify
rustc --version
```

#### Issue 3: Linker Errors (OpenSSL)

**Symptom:**
```
error: linking with `cc` failed
  = note: ld: library not found for -lssl
```

**Solution:**

**macOS:**
```bash
brew install openssl@3
export OPENSSL_DIR=$(brew --prefix openssl@3)
```

**Ubuntu/Debian:**
```bash
sudo apt-get install libssl-dev pkg-config
```

#### Issue 4: Out of Memory

**Symptom:**
```
error: could not compile `midnight-proof-server-prototype`
signal: 9, SIGKILL: kill
```

**Solution:**
```bash
# Reduce parallel jobs
export CARGO_BUILD_JOBS=2
cargo build --release

# Or limit memory per job
ulimit -v 4000000  # Limit to 4GB per process
```

#### Issue 5: Workspace Not Found

**Symptom:**
```
error: current package believes it's in a workspace when it's not
```

**Solution:**

Ensure you're building from within the midnight-ledger repository:

```bash
# Check you're in the right place
pwd  # Should end in /midnight-ledger

# Verify workspace configuration
cat Cargo.toml | grep workspace

# Rebuild
cargo clean
cargo build --release -p midnight-proof-server-prototype
```

#### Issue 6: Dependency Version Conflicts

**Symptom:**
```
error: failed to select a version for the requirement
```

**Solution:**
```bash
# Update Cargo.lock
cargo update

# Clean and rebuild
cargo clean
cargo build --release
```

#### Issue 7: Slow Compilation

**Symptoms:**
- Build takes > 30 minutes
- High CPU/memory usage

**Solutions:**

1. **Enable incremental compilation (dev builds):**
```bash
export CARGO_INCREMENTAL=1
cargo build
```

2. **Use faster linker (mold on Linux):**
```bash
# Install mold
sudo apt install mold  # Ubuntu/Debian

# Use it
RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo build --release
```

3. **Use faster linker (lld):**
```bash
# Install LLVM/lld
rustup component add llvm-tools-preview

# Use it
RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build --release
```

4. **Reduce optimization for faster dev builds:**
```toml
# Add to .cargo/config.toml
[profile.dev]
opt-level = 1  # Basic optimization
```

---

## Advanced Build Options

### Cross-Compilation

Building for different platforms:

```bash
# Add target
rustup target add x86_64-unknown-linux-musl

# Build for target
cargo build --release --target x86_64-unknown-linux-musl
```

### Custom Build Profiles

Create custom profiles in workspace `Cargo.toml`:

```toml
[profile.production]
inherits = "release"
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

Build with custom profile:
```bash
cargo build --profile production
```

### Build with Features

```bash
# Build with specific features
cargo build --release --features "attestation,monitoring"

# Build with all features
cargo build --release --all-features

# Build without default features
cargo build --release --no-default-features
```

### Environment Variables

```bash
# Increase verbosity
CARGO_LOG=cargo::core::compiler=trace cargo build --release

# Set rust flags
RUSTFLAGS="-C target-cpu=native" cargo build --release

# Use custom linker
RUSTFLAGS="-C linker=clang" cargo build --release

# Enable debug info in release
RUSTFLAGS="-C debuginfo=2" cargo build --release
```

### Caching for CI/CD

```bash
# Install cargo-cache
cargo install cargo-cache

# Cache dependencies
export CARGO_HOME=$HOME/.cargo
export CARGO_TARGET_DIR=./target

# Build
cargo build --release

# Clean old artifacts (save space)
cargo cache --autoclean
```

### Docker Build

```dockerfile
FROM rust:1.75 as builder

WORKDIR /workspace
COPY . .

# Build workspace
RUN cargo build --release -p midnight-proof-server-prototype

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder \
    /workspace/tee-proof-server-proto/proof-server/target/release/midnight-proof-server-prototype \
    /usr/local/bin/

EXPOSE 6300
ENTRYPOINT ["midnight-proof-server-prototype"]
CMD ["--disable-auth"]
```

Build:
```bash
docker build -t midnight-proof-server:latest .
```

---

## Verification

### Verify Build Success

```bash
# Check binary exists
test -f ./tee-proof-server-proto/proof-server/target/release/midnight-proof-server-prototype && echo "✅ Build successful"

# Check binary size (should be ~50-100MB)
ls -lh ./tee-proof-server-proto/proof-server/target/release/midnight-proof-server-prototype

# Check binary is executable
./tee-proof-server-proto/proof-server/target/release/midnight-proof-server-prototype --help

# Run integration tests
cd tee-proof-server-proto
./test-server.sh
```

### Build Metrics

Track build performance:

```bash
# Time the build
time cargo build --release -p midnight-proof-server-prototype

# Build with timing info
cargo build --release -p midnight-proof-server-prototype --timings

# Open timing report
open target/cargo-timings/cargo-timing.html
```

---

## Next Steps

After successful build:

1. **Test the Server**: Run `./test-server.sh` to verify functionality
2. **Configure**: Set up API keys and configuration options
3. **Deploy**: Follow deployment guides in `docs/`
4. **Monitor**: Set up monitoring and logging

## Additional Resources

- [Main README](README.md) - Project overview
- [Proof Server README](proof-server/README.md) - API documentation
- [Deployment Guides](docs/) - Cloud deployment instructions
- [Troubleshooting Guide](docs/troubleshooting.md) - Runtime issues

---

**Version**: 6.2.0-alpha.1
**Last Updated**: 2025-12-19
**License**: Apache-2.0
