#!/bin/bash
set -e

# Multi-Architecture Docker Build Script for Midnight Proof Server
# This script builds and pushes Docker images for multiple architectures:
# - linux/amd64 (Intel/AMD - Linux, AWS x86 Nitro)
# - linux/arm64 (Apple Silicon macOS, AWS Graviton/Nitro)

# Configuration
IMAGE_NAME="${IMAGE_NAME:-midnight/proof-server}"
VERSION="${VERSION:-latest}"
REGISTRY="${REGISTRY:-docker.io}" # Change to your registry (e.g., ghcr.io, ECR)

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== Multi-Architecture Proof Server Build ===${NC}"
echo "Image: ${IMAGE_NAME}:${VERSION}"
echo "Registry: ${REGISTRY}"
echo ""

# Step 1: Ensure Docker Buildx is set up
echo -e "${YELLOW}Step 1: Setting up Docker Buildx...${NC}"
if ! docker buildx inspect multiarch-builder > /dev/null 2>&1; then
    echo "Creating multiarch-builder..."
    docker buildx create --name multiarch-builder --driver docker-container --bootstrap --use
else
    echo "Using existing multiarch-builder..."
    docker buildx use multiarch-builder
fi

# Inspect the builder
docker buildx inspect --bootstrap

# Step 2: Build for multiple architectures
echo -e "${YELLOW}Step 2: Building multi-architecture images...${NC}"
echo "Platforms: linux/amd64, linux/arm64"
echo ""
echo "Building from midnight-ledger workspace root..."

# Navigate to workspace root (two levels up from scripts/)
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "${SCRIPT_DIR}/../.." && pwd )"
cd "${WORKSPACE_ROOT}" || exit 1

echo "Workspace: ${WORKSPACE_ROOT}"
echo ""

# Build and push (or load for local testing)
if [ "$1" = "--push" ]; then
    echo -e "${GREEN}Building and pushing to registry...${NC}"
    docker buildx build \
        --platform linux/amd64,linux/arm64 \
        --file tee-proof-server-proto/Dockerfile \
        --tag ${REGISTRY}/${IMAGE_NAME}:${VERSION} \
        --tag ${REGISTRY}/${IMAGE_NAME}:latest \
        --push \
        .

    echo -e "${GREEN}✓ Successfully built and pushed multi-arch image!${NC}"
    echo ""
    echo "To pull and run:"
    echo "  docker pull ${REGISTRY}/${IMAGE_NAME}:${VERSION}"
    echo "  docker run -p 6300:6300 ${REGISTRY}/${IMAGE_NAME}:${VERSION}"

elif [ "$1" = "--load" ]; then
    echo -e "${GREEN}Building for local platform only...${NC}"
    docker buildx build \
        --platform $(docker version --format '{{.Server.Os}}/{{.Server.Arch}}') \
        --file tee-proof-server-proto/Dockerfile \
        --tag ${IMAGE_NAME}:${VERSION} \
        --tag ${IMAGE_NAME}:latest \
        --load \
        .

    echo -e "${GREEN}✓ Successfully built and loaded image!${NC}"
    echo ""
    echo "To run locally:"
    echo "  docker run -p 6300:6300 ${IMAGE_NAME}:${VERSION}"

else
    echo -e "${YELLOW}Building without pushing (dry-run)...${NC}"
    docker buildx build \
        --platform linux/amd64,linux/arm64 \
        --file tee-proof-server-proto/Dockerfile \
        --tag ${REGISTRY}/${IMAGE_NAME}:${VERSION} \
        --tag ${REGISTRY}/${IMAGE_NAME}:latest \
        .

    echo -e "${GREEN}✓ Build successful!${NC}"
    echo ""
    echo "To push to registry, run:"
    echo "  $0 --push"
    echo ""
    echo "To build and load for local testing:"
    echo "  $0 --load"
fi

# Step 3: Verify images (if pushed)
if [ "$1" = "--push" ]; then
    echo -e "${YELLOW}Step 3: Verifying multi-arch manifest...${NC}"
    docker buildx imagetools inspect ${REGISTRY}/${IMAGE_NAME}:${VERSION}
fi

echo -e "${GREEN}=== Build Complete ===${NC}"
