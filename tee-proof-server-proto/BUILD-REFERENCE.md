# Build Reference - Exact Working Steps

This document contains the **exact commands that work** for building the Midnight proof server Docker image.

## Quick Reference

### Using Makefile (Recommended) ✅

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto

# Build for local platform only (fastest)
make build-local

# Run the server
make run

# Check status
docker ps | grep midnight-proof-server

# View logs
make logs

# Stop
make stop
```

### Manual Build from Workspace Root ✅

```bash
# IMPORTANT: Build must be run from workspace root, NOT from tee-proof-server-proto/

cd ~/code/midnight-code/midnight-ledger  # ← Workspace root

# Set up buildx (one-time)
docker buildx create --name multiarch-builder --driver docker-container --bootstrap --use || \
    docker buildx use multiarch-builder

# Build for local platform
docker buildx build \
    --platform $(docker version --format '{{.Server.Os}}/{{.Server.Arch}}') \
    --file tee-proof-server-proto/Dockerfile \
    --tag midnight/proof-server:latest \
    --load \
    .

# Verify image was created
docker images | grep midnight/proof-server
```

### Why Build from Workspace Root?

The Dockerfile **requires access to all workspace members** because the proof server depends on:

```
midnight-ledger/              ← Build from HERE
├── Cargo.toml               ← Workspace manifest
├── Cargo.lock
├── base-crypto/             ← Required dependency
├── ledger/                  ← Required dependency
├── zswap/                   ← Required dependency
├── storage/                 ← Required dependency
└── tee-proof-server-proto/  ← Your starting directory
    ├── Dockerfile           ← References all workspace members
    └── proof-server/        ← The actual server code
```

## Directory Structure

```bash
# Where you typically are:
~/code/midnight-code/midnight-ledger/tee-proof-server-proto/
    ├── Dockerfile          ← The Dockerfile
    ├── Makefile            ← Use this!
    └── scripts/
        └── aws-nitro-deploy.sh

# Where Docker build context must be:
~/code/midnight-code/midnight-ledger/  ← One level UP
    ├── Cargo.toml         ← Workspace root
    └── tee-proof-server-proto/
```

## Common Mistakes ❌

### ❌ Wrong: Building from tee-proof-server-proto/

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
docker build -f Dockerfile -t midnight/proof-server:latest .
# ERROR: Can't find workspace members (ledger/, zswap/, etc.)
```

### ❌ Wrong: Using non-existent Dockerfile

```bash
docker build -f Dockerfile.enclave -t midnight/proof-server:latest .
# ERROR: Dockerfile.enclave doesn't exist
```

### ❌ Wrong: Using plain docker build

```bash
docker build -f Dockerfile -t midnight/proof-server:latest .
# ERROR: 'docker buildx build' requires 1 argument
```

## Correct Approaches ✅

### ✅ Option 1: Use Makefile (Easiest)

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
make build-local
```

The Makefile automatically:
- Changes to workspace root (`cd ..`)
- Sets up buildx
- Uses correct paths
- Handles platform detection

### ✅ Option 2: Manual from Workspace Root

```bash
cd ~/code/midnight-code/midnight-ledger  # ← One directory UP
docker buildx build \
    --platform linux/amd64 \
    --file tee-proof-server-proto/Dockerfile \
    --tag midnight/proof-server:latest \
    --load \
    .
```

### ✅ Option 3: Use AWS Nitro Deployment Script

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
./scripts/aws-nitro-deploy.sh --build
```

This script:
- Automatically finds workspace root
- Verifies Dockerfile exists
- Runs the correct build command
- Optionally deploys to AWS Nitro Enclave

## Build Times

| Build Type | Time | Cache Status |
|------------|------|--------------|
| First build | ~60s | None |
| Cached build | ~30s | Full (using --mount=type=cache) |
| Clean build | ~60s | Registry cache only |

## Build Command Breakdown

```bash
docker buildx build \
    --platform linux/amd64 \           # Target platform (or linux/arm64)
    --file tee-proof-server-proto/Dockerfile \  # Path from workspace root
    --tag midnight/proof-server:latest \        # Image name and tag
    --load \                           # Load into local Docker
    .                                  # Build context = workspace root
```

### Key Parameters

- `--platform`: Build for specific architecture
  - `linux/amd64` - Intel/AMD (x86_64)
  - `linux/arm64` - ARM64 (Apple Silicon, AWS Graviton)
  - Use `$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')` for auto-detection

- `--file`: Path to Dockerfile **relative to build context**
  - Build context is `.` (workspace root)
  - So path is `tee-proof-server-proto/Dockerfile`

- `--load`: Import built image into local Docker
  - Without this, image stays in buildx cache
  - Required to run with `docker run`

- `.`: Build context directory
  - Must be workspace root
  - Docker can access all files under this directory

## Verification

After building, verify:

```bash
# 1. Check image exists
docker images | grep midnight/proof-server
# Expected: midnight/proof-server:latest   ~134MB

# 2. Inspect image
docker image inspect midnight/proof-server:latest | jq '.[0].Config.Labels'

# 3. Test run
docker run --rm midnight/proof-server:latest --help

# 4. Full test
docker run -d -p 6300:6300 --name test-proof midnight/proof-server:latest
sleep 3
curl http://localhost:6300/health
curl http://localhost:6300/version
docker stop test-proof && docker rm test-proof
```

## Multi-Architecture Builds

Build for both AMD64 and ARM64:

```bash
cd ~/code/midnight-code/midnight-ledger

docker buildx build \
    --platform linux/amd64,linux/arm64 \
    --file tee-proof-server-proto/Dockerfile \
    --tag midnight/proof-server:latest \
    --push \                          # Push to registry (can't use --load with multi-arch)
    .
```

**Note:** Multi-arch builds can't use `--load` - they must `--push` to a registry.

## AWS Nitro Enclave Deployment

### Local Build + Deploy

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto

# Build and deploy in one command
./scripts/aws-nitro-deploy.sh --build
```

### Use Pre-built Image

```bash
# If image already exists locally
./scripts/aws-nitro-deploy.sh

# Or pull from registry and deploy
./scripts/aws-nitro-deploy.sh --pull
```

## Troubleshooting

### "No such file or directory: tee-proof-server-proto"

**Cause:** Building from wrong directory

**Fix:** Build from workspace root
```bash
cd ~/code/midnight-code/midnight-ledger  # ← Go UP one level
docker buildx build -f tee-proof-server-proto/Dockerfile ...
```

### "failed to load manifest for workspace member"

**Cause:** Missing workspace dependencies in Dockerfile

**Fix:** Ensure all workspace members are copied (already fixed in current Dockerfile)

### "docker buildx build requires 1 argument"

**Cause:** Using `docker build` instead of `docker buildx build`, or missing build context

**Fix:** Always include `.` at the end:
```bash
docker buildx build [OPTIONS] .  # ← Don't forget the dot!
```

### "GLIBC version not found"

**Cause:** Builder and runtime use different glibc versions

**Fix:** Already fixed - both use `bookworm` (glibc 2.36)

## Summary

**TL;DR - Just Use Make:**

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
make build-local && make run
```

**For AWS Nitro:**

```bash
cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto
./scripts/aws-nitro-deploy.sh --build
```

---

**Last Updated:** 2025-12-22
**Verified Working:** ✅ Yes
**Build Time:** ~30s (with cache)
**Image Size:** ~134MB
