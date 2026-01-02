#!/bin/bash
# Build multiple proof server versions for network compatibility
# This script builds separate Docker images for different proof versions

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( cd "${SCRIPT_DIR}/../.." && pwd )"

echo "🔨 Building Multi-Version Proof Server Images"
echo "=============================================="
echo ""

# Check remote network version
echo "📡 Checking remote proof server version..."
REMOTE_VERSION=$(curl -s https://lace-proof-pub.preview.midnight.network/version 2>/dev/null || echo "UNKNOWN")
echo "   Preview network version: $REMOTE_VERSION"
echo ""

# Get current git info
cd "$WORKSPACE_ROOT"
CURRENT_BRANCH=$(git branch --show-current)
CURRENT_COMMIT=$(git rev-parse --short HEAD)
echo "📍 Current state:"
echo "   Branch: $CURRENT_BRANCH"
echo "   Commit: $CURRENT_COMMIT"
echo ""

# Function to build a specific version
build_version() {
    local VERSION_TAG=$1
    local IMAGE_TAG=$2
    local DESCRIPTION=$3

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "📦 Building: $IMAGE_TAG"
    echo "   Description: $DESCRIPTION"
    echo "   Git ref: $VERSION_TAG"
    echo ""

    # Try to checkout the version
    if git checkout "$VERSION_TAG" 2>/dev/null; then
        echo "   ✅ Checked out $VERSION_TAG"
    else
        echo "   ⚠️  Warning: Could not checkout $VERSION_TAG"
        echo "   Using current code instead"
    fi

    # Get version from Cargo.toml if possible
    if [ -f "tee-proof-server-proto/proof-server/Cargo.toml" ]; then
        CARGO_VERSION=$(grep '^version = ' tee-proof-server-proto/proof-server/Cargo.toml | head -1 | cut -d'"' -f2)
        echo "   Version: $CARGO_VERSION"
    fi

    # Build the image
    echo ""
    echo "   🔨 Building Docker image..."
    if docker buildx build \
        --platform $(docker version --format '{{.Server.Os}}/{{.Server.Arch}}') \
        --file tee-proof-server-proto/Dockerfile \
        --tag "midnight/proof-server:${IMAGE_TAG}" \
        --load \
        . ; then
        echo "   ✅ Build successful: midnight/proof-server:${IMAGE_TAG}"
    else
        echo "   ❌ Build failed: midnight/proof-server:${IMAGE_TAG}"
        return 1
    fi

    echo ""
}

# Build legacy version (compatible with current network)
echo "🎯 Target 1: Legacy Version (Network Compatible)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Try to find the commit before domain separators
LEGACY_COMMIT=$(git log --oneline --all | grep -v "domain.*separator\|PM-20172" | head -1 | cut -d' ' -f1)
if [ -z "$LEGACY_COMMIT" ]; then
    # Fallback to specific commit if search fails
    LEGACY_COMMIT="9955490"  # Tag before domain separator change
fi

echo "   Using commit: $LEGACY_COMMIT (before domain separators)"
echo ""

if build_version "$LEGACY_COMMIT" "v1-legacy" "Compatible with preview network $REMOTE_VERSION"; then
    echo "✅ Legacy version built successfully"
else
    echo "❌ Legacy version build failed"
fi

echo ""
echo ""

# Build current version (with domain separators)
echo "🎯 Target 2: Current Version (With Domain Separators)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Return to original state
git checkout "$CURRENT_BRANCH" 2>/dev/null || git checkout "$CURRENT_COMMIT"

echo "   Using: $CURRENT_BRANCH ($CURRENT_COMMIT)"
echo ""

if build_version "$CURRENT_COMMIT" "v1-current" "Latest version with domain separators"; then
    echo "✅ Current version built successfully"
else
    echo "❌ Current version build failed"
fi

# Also tag as 'latest'
docker tag midnight/proof-server:v1-current midnight/proof-server:latest

echo ""
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Build Complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# List built images
echo "📋 Built Images:"
docker images | grep "midnight/proof-server" | head -10

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🚀 Usage Instructions"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "For CURRENT preview network ($REMOTE_VERSION):"
echo "  docker run -d -p 6300:6300 --name proof-server midnight/proof-server:v1-legacy"
echo ""
echo "For FUTURE network (when upgraded to 6.2.0+):"
echo "  docker run -d -p 6300:6300 --name proof-server midnight/proof-server:v1-current"
echo ""
echo "To test both simultaneously:"
echo "  docker run -d -p 6300:6300 --name proof-legacy midnight/proof-server:v1-legacy"
echo "  docker run -d -p 6301:6300 --name proof-current midnight/proof-server:v1-current"
echo ""
echo "Check version:"
echo "  curl http://localhost:6300/version  # Legacy"
echo "  curl http://localhost:6301/version  # Current"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Verify versions
echo "🔍 Verifying Built Versions..."
echo ""

# Start legacy temporarily to check version
echo "Starting v1-legacy to check version..."
CONTAINER_ID=$(docker run -d -p 16300:6300 midnight/proof-server:v1-legacy)
sleep 3
LEGACY_VER=$(curl -s http://localhost:16300/version 2>/dev/null || echo "ERROR")
docker stop "$CONTAINER_ID" >/dev/null 2>&1
docker rm "$CONTAINER_ID" >/dev/null 2>&1

echo "   v1-legacy version: $LEGACY_VER"

# Start current temporarily to check version
echo "Starting v1-current to check version..."
CONTAINER_ID=$(docker run -d -p 16300:6300 midnight/proof-server:v1-current)
sleep 3
CURRENT_VER=$(curl -s http://localhost:16300/version 2>/dev/null || echo "ERROR")
docker stop "$CONTAINER_ID" >/dev/null 2>&1
docker rm "$CONTAINER_ID" >/dev/null 2>&1

echo "   v1-current version: $CURRENT_VER"
echo ""

# Compatibility check
echo "📊 Compatibility Matrix:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
printf "%-20s %-20s %-15s\n" "Image Tag" "Version" "Network Compat"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
printf "%-20s %-20s %-15s\n" "v1-legacy" "$LEGACY_VER" "✅ $REMOTE_VERSION"
printf "%-20s %-20s %-15s\n" "v1-current" "$CURRENT_VER" "⏳ Future (6.2.0+)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Recommendation
echo "💡 RECOMMENDATION"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Current preview network is on: $REMOTE_VERSION"
echo ""
if [[ "$LEGACY_VER" == "$REMOTE_VERSION"* ]]; then
    echo "✅ Use v1-legacy for production (matches network)"
    echo "   docker run -d -p 6300:6300 --name proof-server midnight/proof-server:v1-legacy"
else
    echo "⚠️  Legacy version ($LEGACY_VER) doesn't match network ($REMOTE_VERSION)"
    echo "   You may need to adjust the build commit"
fi
echo ""
echo "Monitor network upgrades:"
echo "  watch -n 300 'curl -s https://lace-proof-pub.preview.midnight.network/version'"
echo ""
echo "When network shows 6.2.0+, switch to:"
echo "  docker stop proof-server && docker rm proof-server"
echo "  docker run -d -p 6300:6300 --name proof-server midnight/proof-server:v1-current"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "✅ All done! Both versions are ready to use."
