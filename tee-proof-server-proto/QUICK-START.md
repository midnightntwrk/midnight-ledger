# Quick Start: Multi-Arch Proof Server

## TL;DR - Get Running in 5 Minutes

### Option 1: Local Development (Easiest)
```bash
# Build for your platform
make -f Makefile.proof-server build-local

# Run it
make -f Makefile.proof-server run

# Test it
curl http://localhost:6300/health
```

### Option 2: Docker Compose
```bash
docker-compose -f docker-compose.proof-server.yml up -d
```

### Option 3: Multi-Arch Production Build
```bash
# Build for all platforms and push to registry
export REGISTRY=ghcr.io/your-org
export IMAGE_NAME=proof-server
export VERSION=v1.0.0

./scripts/build-multiarch-proof-server.sh --push
```

## Platform-Specific Instructions

### macOS (Apple Silicon)
```bash
# Everything works out of the box
make -f Makefile.proof-server dev
```

### macOS (Intel)
```bash
# Same as Apple Silicon
make -f Makefile.proof-server dev
```

### Linux (AMD/Intel)
```bash
# Build and run
make -f Makefile.proof-server dev
```

### Linux (ARM - Raspberry Pi, etc.)
```bash
# Works the same way
make -f Makefile.proof-server dev
```

### AWS EC2 (Standard)
```bash
# Pull from your registry
docker pull myregistry/proof-server:latest

# Run
docker run -d -p 6300:6300 --restart unless-stopped myregistry/proof-server:latest
```

### AWS Nitro Enclave
```bash
# On a Nitro-enabled instance
./scripts/aws-nitro-deploy.sh
```

## Common Commands

```bash
# See all available commands
make -f Makefile.proof-server help

# Build for local use (fast)
make -f Makefile.proof-server build-local

# Build for all platforms (slower, no push)
make -f Makefile.proof-server build

# Build and push to registry
make -f Makefile.proof-server build-push

# Run the server
make -f Makefile.proof-server run

# View logs
make -f Makefile.proof-server logs

# Stop the server
make -f Makefile.proof-server stop

# Clean up
make -f Makefile.proof-server clean
```

## Update Lace to Use Your Local Proof Server

Once your proof server is running on `localhost:6300`, update your Lace extension storage:

1. Open Chrome DevTools → Application → Storage → Extension Storage
2. Find `redux:persist:midnightContext`
3. Update to:
```json
{
  "userNetworksConfigOverrides": "{\"preview\":{\"proofServerAddress\":\"http://localhost:6300\"}}"
}
```
4. Reload the extension

## Troubleshooting

**Problem: "docker buildx not found"**
```bash
# Update Docker Desktop to latest version, or:
docker buildx version
```

**Problem: Port 6300 already in use**
```bash
# Check what's using it
lsof -i :6300

# Use a different port
docker run -p 6301:6300 midnight/proof-server:latest
```

**Problem: Container exits immediately**
```bash
# Check logs
docker logs midnight-proof-server

# Run interactively for debugging
docker run -it --entrypoint /bin/bash midnight/proof-server:latest
```

## Next Steps

- Read the full [PROOF-SERVER-DOCKER.md](./PROOF-SERVER-DOCKER.md) for detailed instructions
- Check [.github/workflows/proof-server-multiarch.yml](./.github/workflows/proof-server-multiarch.yml) for CI/CD setup
- Review [Dockerfile.proof-server](./Dockerfile.proof-server) to customize for your needs

## Files Created

- `Dockerfile.proof-server` - Multi-arch Dockerfile
- `docker-compose.proof-server.yml` - Docker Compose configuration
- `scripts/build-multiarch-proof-server.sh` - Build script
- `scripts/aws-nitro-deploy.sh` - AWS Nitro deployment
- `Makefile.proof-server` - Convenient make targets
- `.github/workflows/proof-server-multiarch.yml` - CI/CD workflow
- `PROOF-SERVER-DOCKER.md` - Comprehensive documentation
- `QUICK-START-PROOF-SERVER.md` - This file
