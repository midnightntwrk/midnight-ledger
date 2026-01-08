#!/bin/bash
#
# PCR Measurement Publication Script
# Extracts PCR values from running enclave and creates a publication file
#

set -e

VERSION="${1:-v6.3.1}"
OUTPUT_FILE="${2:-pcr-measurements.json}"

# Colors
BLUE='\033[0;34m'
GREEN='\033[0;32m'
NC='\033[0m'

echo -e "${BLUE}Extracting PCR measurements from running enclave...${NC}"

# Get enclave info
ENCLAVE_INFO=$(nitro-cli describe-enclaves)

if ! echo "$ENCLAVE_INFO" | jq -e '.[] | .Measurements' > /dev/null 2>&1; then
    echo "Error: No running enclave found or cannot extract measurements"
    exit 1
fi

# Extract PCR values
PCR0=$(echo "$ENCLAVE_INFO" | jq -r '.[0].Measurements.PCR0')
PCR1=$(echo "$ENCLAVE_INFO" | jq -r '.[0].Measurements.PCR1')
PCR2=$(echo "$ENCLAVE_INFO" | jq -r '.[0].Measurements.PCR2')
ENCLAVE_NAME=$(echo "$ENCLAVE_INFO" | jq -r '.[0].EnclaveName')
CPU_COUNT=$(echo "$ENCLAVE_INFO" | jq -r '.[0].NumberOfCPUs')
MEMORY_MB=$(echo "$ENCLAVE_INFO" | jq -r '.[0].MemoryMiB')

echo "Enclave: $ENCLAVE_NAME"
echo "PCR0: $PCR0"
echo "PCR1: $PCR1"
echo "PCR2: $PCR2"
echo ""

# Create JSON file
cat > "$OUTPUT_FILE" <<EOF
{
  "version": "$VERSION",
  "environment": "devnet",
  "description": "Midnight Proof Server - AWS Nitro Enclave PCR Measurements",
  "buildDate": "$(date -u +%Y-%m-%d)",
  "measurements": {
    "hashAlgorithm": "SHA384",
    "pcr0": {
      "value": "$PCR0",
      "description": "Enclave image file - uniquely identifies the Docker image and application code"
    },
    "pcr1": {
      "value": "$PCR1",
      "description": "Kernel and boot ramfs - verifies the Linux kernel and initrd"
    },
    "pcr2": {
      "value": "$PCR2",
      "description": "Application vCPUs and memory - verifies CPU and memory configuration"
    }
  },
  "buildInfo": {
    "dockerImage": "midnight/proof-server:$VERSION",
    "eifFile": "$ENCLAVE_NAME.eif",
    "cpuCount": $CPU_COUNT,
    "memoryMB": $MEMORY_MB,
    "reproducible": true
  },
  "verificationInstructions": {
    "url": "https://docs.midnight.network/proof-server/attestation-verification",
    "notes": [
      "These PCR values are deterministic and reproducible from the source code",
      "To reproduce: Build the Docker image from the same commit and convert to EIF",
      "PCR0 changes with any code or dependency updates",
      "PCR1 is stable unless kernel/initrd changes",
      "PCR2 changes if CPU/memory configuration changes"
    ]
  },
  "publishedBy": "Midnight Foundation",
  "publishedAt": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

echo -e "${GREEN}✓ PCR measurements written to: $OUTPUT_FILE${NC}"
echo ""
echo "Next steps:"
echo "1. Review the file: cat $OUTPUT_FILE | jq ."
echo "2. Commit to git: git add $OUTPUT_FILE && git commit -m 'Publish PCR measurements for $VERSION'"
echo "3. Publish as GitHub release or make available via HTTPS"
echo "4. Configure clients to fetch from published location"
echo ""
echo "Example publication URLs:"
echo "  - https://proof-test.devnet.midnight.network/.well-known/pcr-measurements.json"
echo "  - https://github.com/midnight/midnight-ledger/releases/download/$VERSION/pcr-measurements.json"
