# Multi-Architecture Proof Server Docker Container

This guide explains how to build and deploy a multi-architecture Docker container for the Midnight proof server that runs on:
- **macOS** (Apple Silicon ARM64)
- **Linux** (AMD64 and ARM64)
- **AWS Nitro Enclaves**

## Prerequisites

### For Building
- Docker Desktop 20.10+ with BuildKit enabled
- Docker Buildx plugin (included in Docker Desktop)

### For AWS Nitro
- EC2 instance with Nitro Enclave support (C5, M5, R5, C6i, M6i, R6i families)
- AWS CLI configured
- `nitro-cli` installed on the EC2 instance

## Quick Start

### 1. Build for Local Development

```bash
# Build for your local platform only (fastest)
./scripts/build-multiarch-proof-server.sh --load

# Run locally
docker run -p 6300:6300 midnight/proof-server:latest
```

### 2. Build Multi-Architecture Images

```bash
# Build for all platforms (dry-run, no push)
./scripts/build-multiarch-proof-server.sh

# Build and push to registry
IMAGE_NAME=myregistry/proof-server VERSION=v1.0.0 ./scripts/build-multiarch-proof-server.sh --push
```

### 3. Run with Docker Compose

```bash
docker-compose -f docker-compose.proof-server.yml up -d
```

## Architecture Support Matrix

| Platform | Architecture | Use Case | Status |
|----------|--------------|----------|--------|
| macOS (Intel) | linux/amd64 | Development | ✅ Supported |
| macOS (Apple Silicon) | linux/arm64 | Development | ✅ Supported |
| Linux (Intel/AMD) | linux/amd64 | Production | ✅ Supported |
| Linux (ARM) | linux/arm64 | Production | ✅ Supported |
| AWS Graviton | linux/arm64 | Production | ✅ Supported |
| AWS x86 Nitro | linux/amd64 | Production + Enclave | ✅ Supported |

## Detailed Build Instructions

### Step 1: Set Up Docker Buildx

Docker Buildx is required for multi-architecture builds:

```bash
# Create a new builder instance
docker buildx create --name multiarch-builder --driver docker-container --bootstrap --use

# Verify it's working
docker buildx inspect --bootstrap
```

### Step 2: Build Images

#### Local Platform Only (Development)
```bash
./scripts/build-multiarch-proof-server.sh --load
```

#### All Architectures (Production)
```bash
# Set your registry and version
export REGISTRY=ghcr.io/your-org  # or docker.io, ECR, etc.
export IMAGE_NAME=proof-server
export VERSION=v1.0.0

# Build and push
./scripts/build-multiarch-proof-server.sh --push
```

### Step 3: Verify Multi-Arch Manifest

```bash
docker buildx imagetools inspect myregistry/proof-server:v1.0.0
```

You should see output like:
```
Name:      myregistry/proof-server:v1.0.0
MediaType: application/vnd.docker.distribution.manifest.list.v2+json
Digest:    sha256:abc123...

Manifests:
  Name:      myregistry/proof-server:v1.0.0@sha256:def456...
  MediaType: application/vnd.docker.distribution.manifest.v2+json
  Platform:  linux/amd64

  Name:      myregistry/proof-server:v1.0.0@sha256:ghi789...
  MediaType: application/vnd.docker.distribution.manifest.v2+json
  Platform:  linux/arm64
```

## Running the Container

### Local Development (macOS/Linux)

```bash
# Simple run
docker run -p 6300:6300 midnight/proof-server:latest

# With environment variables
docker run -p 6300:6300 \
  -e MIDNIGHT_NETWORK=preview \
  -e RUST_LOG=debug \
  midnight/proof-server:latest

# With persistent data
docker run -p 6300:6300 \
  -v $(pwd)/proof-data:/app/data \
  midnight/proof-server:latest
```

### Using Docker Compose

```bash
# Start the service
docker-compose -f docker-compose.proof-server.yml up -d

# View logs
docker-compose -f docker-compose.proof-server.yml logs -f

# Stop the service
docker-compose -f docker-compose.proof-server.yml down
```

## AWS Deployment

### Standard EC2 Deployment

```bash
# Pull the image
docker pull myregistry/proof-server:latest

# Run as a service
docker run -d \
  --name proof-server \
  --restart unless-stopped \
  -p 6300:6300 \
  myregistry/proof-server:latest
```

### AWS Nitro Enclave Deployment

#### Prerequisites
1. Launch a Nitro-enabled EC2 instance (e.g., m5.xlarge, c5.2xlarge)
2. Install Nitro CLI:
```bash
sudo amazon-linux-extras install aws-nitro-enclaves-cli
sudo yum install aws-nitro-enclaves-cli-devel -y
```

3. Configure resources for enclaves:
```bash
# Allocate CPU and memory for enclaves
sudo sed -i 's/^cpu_count:.*$/cpu_count: 2/' /etc/nitro_enclaves/allocator.yaml
sudo sed -i 's/^memory_mib:.*$/memory_mib: 4096/' /etc/nitro_enclaves/allocator.yaml

# Restart the allocator service
sudo systemctl restart nitro-enclaves-allocator.service
```

#### Deploy to Enclave
```bash
# Run the deployment script
./scripts/aws-nitro-deploy.sh

# Monitor the enclave
nitro-cli describe-enclaves

# View enclave console output
nitro-cli console --enclave-id <ENCLAVE_ID>
```

## Registry Options

### Docker Hub
```bash
export REGISTRY=docker.io
export IMAGE_NAME=yourusername/proof-server
docker login
./scripts/build-multiarch-proof-server.sh --push
```

### GitHub Container Registry
```bash
export REGISTRY=ghcr.io
export IMAGE_NAME=your-org/proof-server
echo $GITHUB_TOKEN | docker login ghcr.io -u USERNAME --password-stdin
./scripts/build-multiarch-proof-server.sh --push
```

### AWS ECR
```bash
# Create repository
aws ecr create-repository --repository-name proof-server

# Login to ECR
aws ecr get-login-password --region us-east-1 | \
  docker login --username AWS --password-stdin 123456789.dkr.ecr.us-east-1.amazonaws.com

# Build and push
export REGISTRY=123456789.dkr.ecr.us-east-1.amazonaws.com
export IMAGE_NAME=proof-server
./scripts/build-multiarch-proof-server.sh --push
```

## Customization

### Modify the Dockerfile

The `Dockerfile.proof-server` uses a two-stage build:
1. **Builder stage**: Compiles the Rust binary for the target architecture
2. **Runtime stage**: Creates a minimal runtime image

Edit these sections based on your proof server's requirements:
- Dependencies in the builder stage
- Runtime dependencies in the final stage
- Environment variables
- Entry point and command arguments

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Logging level (debug, info, warn, error) |
| `PROOF_SERVER_PORT` | `6300` | Port the server listens on |
| `PROOF_SERVER_HOST` | `0.0.0.0` | Host address to bind to |
| `MIDNIGHT_NETWORK` | - | Network to connect to (preview, preprod, mainnet) |

## Troubleshooting

### Build Issues

**Error: "docker buildx not found"**
```bash
# Update Docker Desktop to latest version
# Or install buildx plugin manually
docker buildx version
```

**Error: "multiple platforms feature is currently not supported"**
```bash
# Ensure you're using docker-container driver
docker buildx create --name multiarch-builder --driver docker-container --use
```

### Runtime Issues

**Container exits immediately**
```bash
# Check logs
docker logs <container-id>

# Run interactively for debugging
docker run -it --entrypoint /bin/bash midnight/proof-server:latest
```

**Connection refused on port 6300**
```bash
# Verify container is running
docker ps

# Check if port is exposed
docker port <container-id>

# Test from inside container
docker exec <container-id> curl localhost:6300/health
```

### AWS Nitro Issues

**Error: "Enclave process failed"**
- Check CPU/memory allocation in `/etc/nitro_enclaves/allocator.yaml`
- Verify instance type supports Nitro Enclaves
- Check enclave logs: `nitro-cli console --enclave-id <ID>`

**Error: "Insufficient resources"**
- Increase allocated resources in allocator config
- Restart allocator service: `sudo systemctl restart nitro-enclaves-allocator.service`

## Performance Optimization

### Build Cache
```bash
# Use BuildKit cache mounts for faster builds
export DOCKER_BUILDKIT=1
```

### Layer Caching
The Dockerfile is optimized to cache dependencies separately from source code. When you modify your proof server code, only the final build stage runs.

### Resource Limits
Adjust CPU and memory limits in `docker-compose.proof-server.yml` based on your workload:
```yaml
deploy:
  resources:
    limits:
      cpus: '4'
      memory: 8G
```

## Security Considerations

1. **Non-root user**: The container runs as user `proofserver` (UID 1000)
2. **Minimal base image**: Uses Debian slim for smaller attack surface
3. **No shell access**: Uses `ENTRYPOINT` to prevent shell access
4. **Read-only root filesystem**: Consider adding `--read-only` flag
5. **Nitro Enclaves**: Provides cryptographic attestation and isolation

## CI/CD Integration

### GitHub Actions Example
```yaml
name: Build Multi-Arch Proof Server

on:
  push:
    branches: [main]
    tags: ['v*']

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Set up QEMU
        uses: docker/setup-qemu-action@v2

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v2

      - name: Login to Registry
        uses: docker/login-action@v2
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Build and Push
        run: |
          export REGISTRY=ghcr.io/${{ github.repository_owner }}
          export VERSION=${{ github.ref_name }}
          ./scripts/build-multiarch-proof-server.sh --push
```

## Support

For issues or questions:
- Check the [troubleshooting section](#troubleshooting)
- Review Docker logs: `docker logs <container-id>`
- Check Nitro console: `nitro-cli console --enclave-id <id>`

## License

[Your License Here]
