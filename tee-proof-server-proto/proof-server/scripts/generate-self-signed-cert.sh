#!/bin/bash
# This file is part of midnight-ledger.
# Copyright (C) 2025 Midnight Foundation
# SPDX-License-Identifier: Apache-2.0

# Script to generate self-signed TLS certificates for Midnight Proof Server
# For testing and development purposes only!

set -e

# Default values
CERT_DIR="certs"
CERT_FILE="cert.pem"
KEY_FILE="key.pem"
DAYS=365
DOMAIN="localhost"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Parse command line arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --cert-dir)
      CERT_DIR="$2"
      shift 2
      ;;
    --domain)
      DOMAIN="$2"
      shift 2
      ;;
    --days)
      DAYS="$2"
      shift 2
      ;;
    --help)
      echo "Usage: $0 [OPTIONS]"
      echo ""
      echo "Generate self-signed TLS certificate for Midnight Proof Server"
      echo ""
      echo "Options:"
      echo "  --cert-dir DIR   Directory to store certificates (default: certs)"
      echo "  --domain DOMAIN  Domain name for certificate (default: localhost)"
      echo "  --days DAYS      Certificate validity in days (default: 365)"
      echo "  --help           Show this help message"
      echo ""
      echo "Examples:"
      echo "  $0"
      echo "  $0 --domain proof-server.example.com --days 730"
      echo "  $0 --cert-dir /etc/midnight/certs"
      exit 0
      ;;
    *)
      echo -e "${RED}Unknown option: $1${NC}"
      echo "Use --help for usage information"
      exit 1
      ;;
  esac
done

CERT_PATH="$CERT_DIR/$CERT_FILE"
KEY_PATH="$CERT_DIR/$KEY_FILE"

echo -e "${GREEN}Midnight Proof Server - Certificate Generator${NC}"
echo "=============================================="
echo ""
echo "Configuration:"
echo "  Domain:      $DOMAIN"
echo "  Validity:    $DAYS days"
echo "  Certificate: $CERT_PATH"
echo "  Private Key: $KEY_PATH"
echo ""

# Check if openssl is available
if ! command -v openssl &> /dev/null; then
  echo -e "${RED}Error: openssl command not found${NC}"
  echo ""
  echo "Please install OpenSSL:"
  echo "  Ubuntu/Debian: sudo apt-get install openssl"
  echo "  macOS:         brew install openssl"
  echo "  RHEL/CentOS:   sudo yum install openssl"
  exit 1
fi

# Create certificate directory if it doesn't exist
if [ ! -d "$CERT_DIR" ]; then
  echo "Creating certificate directory: $CERT_DIR"
  mkdir -p "$CERT_DIR"
fi

# Check if certificates already exist
if [ -f "$CERT_PATH" ] || [ -f "$KEY_PATH" ]; then
  echo -e "${YELLOW}Warning: Certificate files already exist!${NC}"
  echo "  $CERT_PATH"
  echo "  $KEY_PATH"
  echo ""
  read -p "Overwrite existing certificates? (y/N) " -n 1 -r
  echo
  if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Cancelled."
    exit 0
  fi
  echo "Removing existing certificates..."
  rm -f "$CERT_PATH" "$KEY_PATH"
fi

echo ""
echo "Generating self-signed certificate..."
echo ""

# Generate certificate
openssl req -x509 -newkey rsa:4096 -nodes \
  -keyout "$KEY_PATH" \
  -out "$CERT_PATH" \
  -days "$DAYS" \
  -subj "/CN=${DOMAIN}/O=Midnight Proof Server/C=US" \
  -addext "subjectAltName=DNS:${DOMAIN},DNS:*.${DOMAIN},DNS:localhost,IP:127.0.0.1,IP:0.0.0.0" \
  2>/dev/null

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Certificate generated successfully!${NC}"
  echo ""

  # Set appropriate permissions
  chmod 644 "$CERT_PATH"
  chmod 600 "$KEY_PATH"

  # Display certificate info
  echo "Certificate Information:"
  echo "------------------------"
  openssl x509 -in "$CERT_PATH" -noout -subject -issuer -dates
  echo ""
  echo "Subject Alternative Names:"
  openssl x509 -in "$CERT_PATH" -noout -ext subjectAltName
  echo ""

  # Display file paths and permissions
  echo "Files Created:"
  ls -lh "$CERT_PATH" "$KEY_PATH"
  echo ""

  # Display usage instructions
  echo -e "${GREEN}Next Steps:${NC}"
  echo "------------"
  echo ""
  echo "1. Start the proof server:"
  echo "   cargo run --release -- --tls-cert $CERT_PATH --tls-key $KEY_PATH"
  echo ""
  echo "2. Or use environment variables:"
  echo "   export MIDNIGHT_PROOF_SERVER_TLS_CERT=$CERT_PATH"
  echo "   export MIDNIGHT_PROOF_SERVER_TLS_KEY=$KEY_PATH"
  echo "   cargo run --release"
  echo ""
  echo "3. Test the HTTPS endpoint:"
  echo "   curl -k https://localhost:6300/health"
  echo ""
  echo -e "${YELLOW}⚠️  WARNING: Self-signed certificates should only be used for testing!${NC}"
  echo -e "${YELLOW}⚠️  For production, use certificates from a trusted CA (Let's Encrypt, etc.)${NC}"
  echo ""
  echo "See TLS-SETUP.md for production certificate setup instructions."

else
  echo -e "${RED}✗ Failed to generate certificate${NC}"
  exit 1
fi
