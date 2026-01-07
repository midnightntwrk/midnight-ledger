#!/bin/bash
#
# Automated Nitro Enclave Deployment Script
# This script automates the build and deployment of the Midnight Proof Server to AWS Nitro Enclaves
#

set -e  # Exit on any error

# Configuration
VERSION="${VERSION:-v6.3.1}"
ENCLAVE_CID="${ENCLAVE_CID:-16}"
ENCLAVE_CPUS="${ENCLAVE_CPUS:-2}"
ENCLAVE_MEMORY="${ENCLAVE_MEMORY:-4096}"
DEBUG_MODE="${DEBUG_MODE:-false}"
WORKSPACE_ROOT="/home/ssm-user/midnight-ledger"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Print banner
echo "=================================="
echo "  Nitro Enclave Deployment Tool"
echo "  Version: $VERSION"
echo "=================================="
echo ""

# Step 1: Pull latest code
log_info "Step 1: Pulling latest code from Git..."
cd "$WORKSPACE_ROOT"

if [ -d ".git" ]; then
    git fetch origin
    git pull origin $(git branch --show-current)
    log_success "Code updated successfully"
else
    log_error "Not a git repository. Please clone the repository first."
    exit 1
fi

# Step 2: Build Docker image
log_info "Step 2: Building Docker image..."
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:$VERSION .
log_success "Docker image built: midnight/proof-server:$VERSION"

# Step 3: Build Nitro Enclave Image (EIF)
log_info "Step 3: Building Nitro Enclave Image (EIF)..."
EIF_FILE="proof-server-$VERSION.eif"

nitro-cli build-enclave \
  --docker-uri midnight/proof-server:$VERSION \
  --output-file "$EIF_FILE" | tee build-output.txt

# Extract and display PCR measurements
log_info "PCR Measurements (save these for attestation verification):"
echo "-----------------------------------------------------------"
grep -A 3 "Measurements:" build-output.txt || echo "Could not extract PCR measurements"
echo "-----------------------------------------------------------"

log_success "EIF created: $EIF_FILE"

# Step 4: Stop existing enclaves
log_info "Step 4: Stopping existing enclaves..."
if nitro-cli describe-enclaves | grep -q "EnclaveID"; then
    nitro-cli terminate-enclave --all
    log_success "Existing enclaves terminated"
    sleep 2
else
    log_info "No running enclaves found"
fi

# Step 5: Start new enclave
log_info "Step 5: Starting new enclave..."

if [ "$DEBUG_MODE" = "true" ]; then
    log_warning "Starting in DEBUG MODE (console logging enabled)"
    nitro-cli run-enclave \
      --eif-path "$EIF_FILE" \
      --cpu-count $ENCLAVE_CPUS \
      --memory $ENCLAVE_MEMORY \
      --enclave-cid $ENCLAVE_CID \
      --debug-mode > enclave-info.json
else
    log_info "Starting in PRODUCTION MODE (no console logging)"
    nitro-cli run-enclave \
      --eif-path "$EIF_FILE" \
      --cpu-count $ENCLAVE_CPUS \
      --memory $ENCLAVE_MEMORY \
      --enclave-cid $ENCLAVE_CID > enclave-info.json
fi

# Extract enclave ID
ENCLAVE_ID=$(jq -r '.EnclaveID' enclave-info.json)
log_success "Enclave started: $ENCLAVE_ID"

# Display enclave info
log_info "Enclave Configuration:"
cat enclave-info.json | jq '.'

# Step 6: Wait for enclave to boot
log_info "Step 6: Waiting for enclave to boot (10 seconds)..."
sleep 10

# Step 7: Set up vsock proxy
log_info "Step 7: Setting up vsock proxy..."

# Kill any existing socat processes
sudo pkill -f "socat.*6300" 2>/dev/null || true
sleep 1

# Start vsock proxy
log_info "Starting vsock proxy: localhost:6300 → enclave CID $ENCLAVE_CID:6300"
sudo socat TCP-LISTEN:6300,reuseaddr,fork VSOCK-CONNECT:$ENCLAVE_CID:6300 &
SOCAT_PID=$!

# Wait a moment for socat to start
sleep 2

# Verify socat is running
if ps -p $SOCAT_PID > /dev/null; then
    log_success "Vsock proxy started (PID: $SOCAT_PID)"
else
    log_error "Failed to start vsock proxy"
    exit 1
fi

# Step 8: Health check
log_info "Step 8: Running health checks..."

# Function to test endpoint with retries
test_endpoint() {
    local endpoint=$1
    local max_attempts=5
    local attempt=1

    while [ $attempt -le $max_attempts ]; do
        log_info "Testing $endpoint (attempt $attempt/$max_attempts)..."

        if curl -sf -m 5 "http://localhost:6300$endpoint" > /dev/null 2>&1; then
            return 0
        fi

        attempt=$((attempt + 1))
        sleep 2
    done

    return 1
}

# Test health endpoint
if test_endpoint "/health"; then
    log_success "✓ Health endpoint responding"
else
    log_error "✗ Health endpoint not responding"
    log_warning "Check enclave console with: nitro-cli console --enclave-id $ENCLAVE_ID"
    exit 1
fi

# Test attestation endpoint
log_info "Testing attestation endpoint..."
ATTESTATION_RESPONSE=$(curl -s "http://localhost:6300/attestation?nonce=deployment-test-$(date +%s)")

if echo "$ATTESTATION_RESPONSE" | jq -e '.platform' > /dev/null 2>&1; then
    PLATFORM=$(echo "$ATTESTATION_RESPONSE" | jq -r '.platform')
    log_success "✓ Attestation endpoint responding"
    log_info "  Platform detected: $PLATFORM"

    # Check if attestation document was generated
    if echo "$ATTESTATION_RESPONSE" | jq -e '.attestation' > /dev/null 2>&1; then
        ATTESTATION_SIZE=$(echo "$ATTESTATION_RESPONSE" | jq -r '.attestation' | wc -c)
        log_success "  ✓ NSM attestation document generated ($ATTESTATION_SIZE bytes)"
    else
        log_warning "  ⚠ No attestation document (NSM may not be available)"
    fi
else
    log_error "✗ Attestation endpoint not responding correctly"
fi

# Step 9: Display summary
echo ""
echo "=================================="
echo "  Deployment Summary"
echo "=================================="
echo "Version:        $VERSION"
echo "Enclave ID:     $ENCLAVE_ID"
echo "Enclave CID:    $ENCLAVE_CID"
echo "CPUs:           $ENCLAVE_CPUS"
echo "Memory:         ${ENCLAVE_MEMORY}MB"
echo "Debug Mode:     $DEBUG_MODE"
echo "Vsock Proxy:    localhost:6300 → CID $ENCLAVE_CID:6300"
echo ""
log_success "Deployment completed successfully!"
echo ""

# Step 10: Useful commands
echo "Useful Commands:"
echo "  Check status:     nitro-cli describe-enclaves"
echo "  View console:     nitro-cli console --enclave-id $ENCLAVE_ID  (debug mode only)"
echo "  View logs:        sudo tail -f /var/log/nitro_enclaves/nitro_enclaves.log"
echo "  Test health:      curl http://localhost:6300/health"
echo "  Test attestation: curl 'http://localhost:6300/attestation?nonce=test123'"
echo "  Stop enclave:     nitro-cli terminate-enclave --enclave-id $ENCLAVE_ID"
echo ""

# Optional: Show console output if in debug mode
if [ "$DEBUG_MODE" = "true" ]; then
    log_info "Debug mode enabled. You can view console with:"
    echo "  nitro-cli console --enclave-id $ENCLAVE_ID"
    echo ""
    read -p "Would you like to view the console now? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        nitro-cli console --enclave-id "$ENCLAVE_ID"
    fi
fi
