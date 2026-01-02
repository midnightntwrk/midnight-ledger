# Build Fixes Applied

## Issues Fixed

### 1. Rust Version Incompatibility
**Problem:** The Dockerfile used Rust 1.75, but the `zswap` crate requires Rust edition 2024, which needs Rust 1.85+ or later.

**Error:**
```
feature `edition2024` is required
The package requires the Cargo feature called `edition2024`, but that feature is
not stabilized in this version of Cargo (1.75.0)
```

**Solution:** Updated Dockerfile to use `rust:latest` which includes support for edition 2024.

### 2. Missing Workspace Dependencies
**Problem:** The Dockerfile didn't copy all required workspace crates. The workspace Cargo.toml references many members that weren't being copied.

**Error:**
```
failed to load manifest for workspace member `/workspace/ledger-wasm`
No such file or directory (os error 2)
```

**Solution:** Added ALL workspace dependencies from Cargo.toml:
- `ledger-wasm/`
- `onchain-runtime-wasm/`
- `zkir-wasm/`
- `proof-server/` (at workspace root)
- `generate-cost-model/`
- `wasm-proving-demos/`
- Plus all previously listed dependencies

### 3. Incorrect Build Context Paths
**Problem:** Build scripts and configurations had incorrect relative paths for navigating to workspace root.

**Solution:** Fixed paths in:
- `Makefile` - Changed `cd ../..` to `cd ..` (from `tee-proof-server-proto/` to `midnight-ledger/`)
- `docker-compose.yml` - Changed `context: ../..` to `context: ..`
- `scripts/build-multiarch-proof-server.sh` - Correctly uses `../..` (from `scripts/` to `midnight-ledger/`)

## Changes Made

### Dockerfile (`tee-proof-server-proto/Dockerfile`)
```dockerfile
# Changed FROM
FROM rust:1.75-slim AS builder
# To
FROM rust:latest AS builder

# Added ALL workspace members (complete list):
COPY base-crypto/ ./base-crypto/
COPY base-crypto-derive/ ./base-crypto-derive/
COPY transient-crypto/ ./transient-crypto/
COPY ledger/ ./ledger/
COPY ledger-wasm/ ./ledger-wasm/
COPY zswap/ ./zswap/
COPY storage/ ./storage/
COPY storage-macros/ ./storage-macros/
COPY serialize/ ./serialize/
COPY serialize-macros/ ./serialize-macros/
COPY zkir/ ./zkir/
COPY zkir-wasm/ ./zkir-wasm/
COPY coin-structure/ ./coin-structure/
COPY onchain-state/ ./onchain-state/
COPY onchain-vm/ ./onchain-vm/
COPY onchain-runtime/ ./onchain-runtime/
COPY onchain-runtime-wasm/ ./onchain-runtime-wasm/
COPY generate-cost-model/ ./generate-cost-model/
COPY proof-server/ ./proof-server/
COPY wasm-proving-demos/ ./wasm-proving-demos/
COPY static/ ./static/
COPY tee-proof-server-proto/ ./tee-proof-server-proto/
```

### .dockerignore (`midnight-ledger/.dockerignore`)
Created to optimize build and exclude unnecessary files:
```
**/target/
**/.git/
**/docs/
**/*.md
# ... and more
```

### Makefile (`tee-proof-server-proto/Makefile`)
```makefile
# All build targets now use:
@cd .. && docker buildx build \
    --file tee-proof-server-proto/Dockerfile \
    ...
```

### docker-compose.yml
```yaml
build:
  context: ..  # Build from midnight-ledger workspace root
  dockerfile: tee-proof-server-proto/Dockerfile
```

### build-multiarch-proof-server.sh
```bash
# Correctly navigates from scripts/ to midnight-ledger/
WORKSPACE_ROOT="$( cd "${SCRIPT_DIR}/../.." && pwd )"
```

## Verified Build Commands

All these commands now work correctly:

### Using Make
```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
make build-local    # Fast local build
make run            # Run the server
```

### Using Docker Compose
```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
docker-compose up -d
```

### Using Build Script
```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
./scripts/build-multiarch-proof-server.sh --load
```

### Manual Build
```bash
cd ~/code/midnight-code/midnight-ledger
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:latest .
```

## Directory Structure

```
midnight-ledger/                    # Workspace root
├── Cargo.toml                      # Workspace manifest
├── Cargo.lock
├── base-crypto/
├── base-crypto-derive/
├── ledger/
├── zswap/                          # Uses edition = "2024"
├── storage/
├── ...
└── tee-proof-server-proto/         # Proof server directory
    ├── Dockerfile                  # Must build from workspace root
    ├── docker-compose.yml
    ├── Makefile
    ├── scripts/
    │   ├── build-multiarch-proof-server.sh
    │   └── aws-nitro-deploy.sh
    └── proof-server/
        ├── Cargo.toml
        └── src/
```

## Build Context Explanation

The Dockerfile **must** be built from the `midnight-ledger` workspace root because:

1. The proof server depends on other workspace crates (`ledger`, `zswap`, etc.)
2. Cargo needs access to the workspace `Cargo.toml` to resolve dependencies
3. All workspace members must be present during the build

### Path Navigation Reference

From different locations:

| Current Directory | To Workspace Root | Command |
|-------------------|-------------------|---------|
| `tee-proof-server-proto/` | One level up | `cd ..` |
| `tee-proof-server-proto/scripts/` | Two levels up | `cd ../..` |
| `tee-proof-server-proto/proof-server/` | Two levels up | `cd ../..` |

## Testing the Build

```bash
# Navigate to the proof server directory
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto

# Test quick build
make build-local

# Verify image was created
docker images | grep midnight/proof-server

# Run the server
docker run -p 6300:6300 midnight/proof-server:latest

# Test in another terminal
curl http://localhost:6300/health
```

## Troubleshooting

### "feature `edition2024` is required"
- **Cause:** Using old Rust version
- **Solution:** Ensure Dockerfile uses `rust:latest` or `rust:1.85+`

### "no such file or directory: tee-proof-server-proto"
- **Cause:** Building from wrong directory
- **Solution:** Ensure build context is workspace root (`midnight-ledger/`)

### "failed to load manifest for workspace member"
- **Cause:** Missing workspace dependencies in Dockerfile
- **Solution:** Ensure all dependencies listed in workspace `Cargo.toml` are copied

### Build is slow or uses too much disk space
- **Solution:** Use build cache mounts (already configured in Dockerfile):
```dockerfile
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/target \
    cargo build --release ...
```

## Performance Notes

- **First build**: ~10-20 minutes (compiles all dependencies)
- **Subsequent builds**: ~2-5 minutes (uses cache)
- **Local platform only** (`--load`): Faster than multi-arch
- **Multi-arch build**: Takes longer due to cross-compilation

## Next Steps

1. ✅ Build works locally
2. Test the running container
3. Push to a registry if needed
4. Set up CI/CD (GitHub Actions workflow included)
5. Deploy to production (AWS Nitro, GCP, Azure)

---

**Fixed:** 2025-12-22
**Rust Version:** latest (1.85+ for edition 2024)
**Binary Name:** midnight-proof-server-prototype
