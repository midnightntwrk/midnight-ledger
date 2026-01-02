#!/bin/bash
set -e

# AWS Nitro Enclave Deployment Script for Proof Server
# This script helps deploy the proof server on AWS EC2 with Nitro Enclave support
#
# IMPORTANT: This script has two modes:
#   1. BUILD MODE: Build the Docker image locally, then deploy to Nitro
#   2. PULL MODE: Pull pre-built image from registry, then deploy to Nitro
#
# Usage:
#   BUILD MODE:  ./aws-nitro-deploy.sh --build
#   PULL MODE:   ./aws-nitro-deploy.sh --pull
#   DEFAULT:     ./aws-nitro-deploy.sh (assumes image already exists locally)

# Configuration
IMAGE_NAME="${IMAGE_NAME:-midnight/proof-server:latest}"
ENCLAVE_CPU_COUNT="${ENCLAVE_CPU_COUNT:-2}"
ENCLAVE_MEMORY_MB="${ENCLAVE_MEMORY_MB:-4096}"
WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

# Parse arguments
BUILD_IMAGE=false
PULL_IMAGE=false

for arg in "$@"; do
    case $arg in
        --build)
            BUILD_IMAGE=true
            shift
            ;;
        --pull)
            PULL_IMAGE=true
            shift
            ;;
    esac
done

echo -e "${GREEN}=== AWS Nitro Enclave Deployment ===${NC}"
echo ""

# Step 0: Build or Pull the Docker image (if requested)
if [ "$BUILD_IMAGE" = true ]; then
    echo -e "${BLUE}=== BUILDING DOCKER IMAGE ===${NC}"
    echo -e "${YELLOW}Building proof server Docker image...${NC}"
    echo ""
    echo "Build location: ${WORKSPACE_ROOT}"
    echo "Dockerfile: ${WORKSPACE_ROOT}/tee-proof-server-proto/Dockerfile"
    echo ""

    # Verify we're in the right place
    if [ ! -f "${WORKSPACE_ROOT}/tee-proof-server-proto/Dockerfile" ]; then
        echo -e "${RED}Error: Dockerfile not found at expected location${NC}"
        echo "Expected: ${WORKSPACE_ROOT}/tee-proof-server-proto/Dockerfile"
        echo ""
        echo "Make sure you're running this script from the midnight-ledger workspace"
        exit 1
    fi

    # Build using the EXACT steps that work
    echo -e "${YELLOW}Step 1: Setting up Docker Buildx...${NC}"
    docker buildx create --name multiarch-builder --driver docker-container --bootstrap --use 2>/dev/null || \
        docker buildx use multiarch-builder

    echo -e "${YELLOW}Step 2: Building Docker image from workspace root...${NC}"
    echo "Command: cd ${WORKSPACE_ROOT} && docker buildx build \\"
    echo "  --platform linux/amd64 \\"
    echo "  --file tee-proof-server-proto/Dockerfile \\"
    echo "  --tag ${IMAGE_NAME} \\"
    echo "  --load \\"
    echo "  ."
    echo ""

    cd "${WORKSPACE_ROOT}"
    docker buildx build \
        --platform linux/amd64 \
        --file tee-proof-server-proto/Dockerfile \
        --tag ${IMAGE_NAME} \
        --load \
        .

    BUILD_EXIT_CODE=$?
    if [ $BUILD_EXIT_CODE -ne 0 ]; then
        echo -e "${RED}Error: Docker build failed with exit code ${BUILD_EXIT_CODE}${NC}"
        exit 1
    fi

    echo -e "${GREEN}✓ Docker image built successfully: ${IMAGE_NAME}${NC}"
    echo ""

elif [ "$PULL_IMAGE" = true ]; then
    echo -e "${BLUE}=== PULLING DOCKER IMAGE ===${NC}"
    echo -e "${YELLOW}Pulling Docker image from registry...${NC}"
    docker pull ${IMAGE_NAME}

    if [ $? -ne 0 ]; then
        echo -e "${RED}Error: Failed to pull Docker image${NC}"
        exit 1
    fi

    echo -e "${GREEN}✓ Docker image pulled successfully${NC}"
    echo ""
fi

# Verify image exists
echo -e "${YELLOW}Verifying Docker image exists...${NC}"
if ! docker image inspect ${IMAGE_NAME} >/dev/null 2>&1; then
    echo -e "${RED}Error: Docker image '${IMAGE_NAME}' not found${NC}"
    echo ""
    echo "You have three options:"
    echo "  1. Build locally:  $0 --build"
    echo "  2. Pull from registry:  $0 --pull"
    echo "  3. Use the Makefile: cd tee-proof-server-proto && make build-local"
    echo ""
    exit 1
fi
echo -e "${GREEN}✓ Image found: ${IMAGE_NAME}${NC}"
echo ""

# Step 1: Check if running on Nitro-enabled instance
echo -e "${BLUE}=== CHECKING NITRO ENCLAVE SUPPORT ===${NC}"
echo -e "${YELLOW}Verifying Nitro Enclave CLI...${NC}"
if ! command -v nitro-cli &> /dev/null; then
    echo -e "${RED}Error: nitro-cli not found.${NC}"
    echo ""
    echo "This script must be run on a Nitro-enabled EC2 instance with Nitro CLI installed."
    echo ""
    echo "Supported instance types:"
    echo "  - C5, C5a, C5n, C6i, C6a, C6in, C7i"
    echo "  - M5, M5a, M5n, M6i, M6a, M6in, M7i"
    echo "  - R5, R5a, R5n, R6i, R6a, R6in, R7i"
    echo "  - And others (see: https://aws.amazon.com/ec2/nitro/nitro-enclaves/)"
    echo ""
    echo "To install Nitro CLI:"
    echo "  sudo amazon-linux-extras install aws-nitro-enclaves-cli"
    echo "  sudo yum install aws-nitro-enclaves-cli aws-nitro-enclaves-cli-devel"
    echo ""
    exit 1
fi
echo -e "${GREEN}✓ Nitro CLI found${NC}"
echo ""

# Step 2: Build Nitro Enclave Image File (EIF)
echo -e "${BLUE}=== BUILDING NITRO ENCLAVE IMAGE ===${NC}"
echo -e "${YELLOW}Converting Docker image to Enclave Image File (EIF)...${NC}"
echo ""
echo "Image: ${IMAGE_NAME}"
echo "Output: proof-server.eif"
echo ""

nitro-cli build-enclave \
    --docker-uri ${IMAGE_NAME} \
    --output-file proof-server.eif

if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Failed to build Enclave Image File${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Enclave Image File built successfully${NC}"
echo ""

# Get EIF measurements for attestation
echo -e "${YELLOW}EIF Measurements (for attestation verification):${NC}"
nitro-cli describe-eif --eif-path proof-server.eif | jq -r '.Measurements'
echo ""

# Step 3: Run the enclave
echo -e "${BLUE}=== STARTING NITRO ENCLAVE ===${NC}"
echo -e "${YELLOW}Launching enclave with configuration:${NC}"
echo "  CPUs: ${ENCLAVE_CPU_COUNT}"
echo "  Memory: ${ENCLAVE_MEMORY_MB} MB"
echo "  Debug mode: enabled"
echo ""

ENCLAVE_OUTPUT=$(nitro-cli run-enclave \
    --eif-path proof-server.eif \
    --cpu-count ${ENCLAVE_CPU_COUNT} \
    --memory ${ENCLAVE_MEMORY_MB} \
    --debug-mode)

if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Failed to start enclave${NC}"
    echo "Output: ${ENCLAVE_OUTPUT}"
    exit 1
fi

ENCLAVE_ID=$(echo "${ENCLAVE_OUTPUT}" | jq -r '.EnclaveID')
echo -e "${GREEN}✓ Enclave started successfully${NC}"
echo -e "${GREEN}Enclave ID: ${ENCLAVE_ID}${NC}"
echo ""

# Wait for enclave to initialize
echo -e "${YELLOW}Waiting for enclave to initialize...${NC}"
sleep 5

# Check enclave status
echo -e "${YELLOW}Enclave status:${NC}"
nitro-cli describe-enclaves | jq '.'
echo ""

# Step 4: Set up vsock proxy for communication
echo -e "${BLUE}=== SETTING UP COMMUNICATION ===${NC}"
echo -e "${YELLOW}Configuring vsock proxy for enclave communication...${NC}"
echo ""

# Note: vsock-proxy setup depends on your specific networking requirements
# This is a basic example - adjust for your use case

echo "Creating vsock proxy configuration..."
cat > /tmp/vsock-proxy.sh << 'EOF'
#!/bin/bash
# Proxy connections from host to enclave via vsock
# CID 16 is typically the enclave CID
# Port 6300 is the proof server port

# Forward host port 6300 to enclave port 6300
vsock-proxy 6300 vsock://16:6300 &

echo "vsock proxy started"
echo "Host port 6300 -> Enclave CID 16 port 6300"
EOF

chmod +x /tmp/vsock-proxy.sh

# Note: You may need to start the proxy manually or as a systemd service
echo -e "${YELLOW}To start the vsock proxy:${NC}"
echo "  /tmp/vsock-proxy.sh"
echo ""

# Step 5: Verification and Management
echo -e "${BLUE}=== DEPLOYMENT COMPLETE ===${NC}"
echo ""
echo -e "${GREEN}✓ Proof server deployed in Nitro Enclave${NC}"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "ENCLAVE INFORMATION"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Enclave ID:       ${ENCLAVE_ID}"
echo "Image:            ${IMAGE_NAME}"
echo "EIF File:         proof-server.eif"
echo "CPUs:             ${ENCLAVE_CPU_COUNT}"
echo "Memory:           ${ENCLAVE_MEMORY_MB} MB"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo -e "${YELLOW}MANAGEMENT COMMANDS:${NC}"
echo ""
echo "View enclave console output:"
echo "  nitro-cli console --enclave-id ${ENCLAVE_ID}"
echo ""
echo "Describe running enclaves:"
echo "  nitro-cli describe-enclaves"
echo ""
echo "Terminate this enclave:"
echo "  nitro-cli terminate-enclave --enclave-id ${ENCLAVE_ID}"
echo ""
echo "Describe EIF (for attestation):"
echo "  nitro-cli describe-eif --eif-path proof-server.eif"
echo ""

echo -e "${YELLOW}ATTESTATION:${NC}"
echo ""
echo "To get attestation document from the enclave:"
echo "  curl http://localhost:6300/attestation"
echo ""
echo "The enclave will provide cryptographic proof of:"
echo "  - PCR0: Enclave Image File hash"
echo "  - PCR1: Linux kernel and bootstrap"
echo "  - PCR2: Application (proof server)"
echo ""

echo -e "${YELLOW}NEXT STEPS:${NC}"
echo ""
echo "1. Start the vsock proxy (if needed):"
echo "   /tmp/vsock-proxy.sh"
echo ""
echo "2. Test the proof server:"
echo "   curl http://localhost:6300/health"
echo "   curl http://localhost:6300/version"
echo ""
echo "3. Monitor enclave logs:"
echo "   nitro-cli console --enclave-id ${ENCLAVE_ID}"
echo ""
echo "4. Configure your application to use the proof server:"
echo "   Proof Server Address: http://localhost:6300"
echo ""

echo -e "${GREEN}Deployment completed successfully!${NC}"
