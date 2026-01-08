#!/bin/bash
#
# Publish PCR Measurements to GitHub Release
#
# This script helps automate PCR publication to GitHub releases
#

set -e

VERSION="${1}"
PCR_FILE="${2:-pcr-measurements-$VERSION.json}"

if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version> [pcr-file]"
    echo "Example: $0 v6.3.1"
    exit 1
fi

if [ ! -f "$PCR_FILE" ]; then
    echo "Error: PCR file not found: $PCR_FILE"
    exit 1
fi

echo "================================================"
echo "  PCR Publication to GitHub Release"
echo "================================================"
echo ""
echo "Version:  $VERSION"
echo "PCR File: $PCR_FILE"
echo ""

# Validate JSON
if ! jq . "$PCR_FILE" > /dev/null 2>&1; then
    echo "Error: Invalid JSON in $PCR_FILE"
    exit 1
fi

echo "✓ PCR file is valid JSON"
echo ""

# Display PCR values
echo "PCR Values:"
jq -r '.measurements | "  PCR0: \(.pcr0.value)\n  PCR1: \(.pcr1.value)\n  PCR2: \(.pcr2.value)"' "$PCR_FILE"
echo ""

# Check if gh CLI is installed
if ! command -v gh &> /dev/null; then
    echo "Error: GitHub CLI (gh) is not installed"
    echo "Install: https://cli.github.com/"
    echo ""
    echo "Alternative: Manually create release and upload $PCR_FILE"
    exit 1
fi

# Check if authenticated
if ! gh auth status &> /dev/null; then
    echo "Error: Not authenticated with GitHub"
    echo "Run: gh auth login"
    exit 1
fi

echo "Creating GitHub release..."
echo ""

# Create release (or use existing)
if gh release view "$VERSION" &> /dev/null; then
    echo "Release $VERSION already exists"
    read -p "Upload PCR file to existing release? (y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 0
    fi
else
    echo "Creating new release $VERSION..."
    gh release create "$VERSION" \
        --title "Proof Server $VERSION" \
        --notes "## Midnight Proof Server $VERSION

### PCR Measurements

This release includes PCR (Platform Configuration Register) measurements for attestation verification.

\`\`\`json
$(jq . "$PCR_FILE")
\`\`\`

### Verification

Clients should fetch PCR measurements from:
\`\`\`
https://github.com/midnight/midnight-ledger/releases/download/$VERSION/pcr-measurements.json
\`\`\`

### Reproducible Build

To reproduce these PCR values:
\`\`\`bash
git checkout $VERSION
docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:$VERSION .
nitro-cli build-enclave --docker-uri midnight/proof-server:$VERSION --output-file proof-server.eif
\`\`\`

The PCR values should match exactly." \
        --draft
fi

# Upload PCR file
echo "Uploading $PCR_FILE to release..."
gh release upload "$VERSION" "$PCR_FILE" --clobber

echo ""
echo "✓ PCR file uploaded successfully!"
echo ""
echo "Release URL:"
gh release view "$VERSION" --json url -q .url
echo ""
echo "PCR Download URL:"
echo "https://github.com/midnight/midnight-ledger/releases/download/$VERSION/$(basename $PCR_FILE)"
echo ""
echo "Next steps:"
echo "  1. Review the release draft"
echo "  2. Publish the release: gh release edit $VERSION --draft=false"
echo "  3. Configure Lace to fetch PCRs from the download URL"
echo ""
