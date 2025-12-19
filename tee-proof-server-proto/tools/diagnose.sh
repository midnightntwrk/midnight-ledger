#!/bin/bash
# File: diagnose.sh
# Usage: ./diagnose.sh [SERVER_URL]
# Example: ./diagnose.sh https://proof.midnight.network
# Example: ./diagnose.sh http://localhost:6300

set -e

# Accept server URL as parameter, default to production server
SERVER_URL="${1:-https://proof.midnight.network}"

# Extract hostname and port for DNS/SSL checks
if [[ "$SERVER_URL" =~ ^https?://([^:/]+)(:([0-9]+))?$ ]]; then
    SERVER_HOST="${BASH_REMATCH[1]}"
    SERVER_PORT="${BASH_REMATCH[3]}"
    if [ -z "$SERVER_PORT" ]; then
        if [[ "$SERVER_URL" =~ ^https:// ]]; then
            SERVER_PORT="443"
        else
            SERVER_PORT="80"
        fi
    fi
else
    echo "Error: Invalid server URL format"
    echo "Usage: $0 [SERVER_URL]"
    echo "Example: $0 https://proof.midnight.network"
    exit 1
fi

echo "=== Midnight Proof Server Diagnostics ==="
echo "Server: $SERVER_URL"
echo "Timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo ""

# 1. Check if server is reachable
echo "1. Server Reachability:"
if curl -s -f -m 5 "$SERVER_URL/health" > /dev/null; then
    echo "   ✅ Server is reachable"
else
    echo "   ❌ Server is NOT reachable"
    echo "   Try: curl -v $SERVER_URL/health"
fi
echo ""

# 2. Check version
echo "2. Server Version:"
VERSION=$(curl -s "$SERVER_URL/version" 2>/dev/null || echo "ERROR")
if [ "$VERSION" != "ERROR" ]; then
    echo "   ✅ Version: $VERSION"
else
    echo "   ❌ Could not retrieve version"
fi
echo ""

# 3. Check readiness
echo "3. Server Readiness:"
READY=$(curl -s "$SERVER_URL/ready" 2>/dev/null || echo "ERROR")
if [ "$READY" != "ERROR" ]; then
    echo "   ✅ Ready: $READY"
    QUEUE_SIZE=$(echo "$READY" | jq -r '.queue_size // "unknown"')
    ACTIVE_WORKERS=$(echo "$READY" | jq -r '.active_workers // "unknown"')
    echo "   Queue size: $QUEUE_SIZE"
    echo "   Active workers: $ACTIVE_WORKERS"
else
    echo "   ❌ Server not ready"
fi
echo ""

# 4. Check SSL certificate (only for HTTPS)
if [[ "$SERVER_URL" =~ ^https:// ]]; then
    echo "4. SSL Certificate:"
    CERT_EXPIRY=$(echo | openssl s_client -servername "$SERVER_HOST" \
        -connect "$SERVER_HOST:$SERVER_PORT" 2>/dev/null | \
        openssl x509 -noout -dates 2>/dev/null || echo "ERROR")
    if [ "$CERT_EXPIRY" != "ERROR" ]; then
        echo "   ✅ Certificate valid"
        echo "   $CERT_EXPIRY"
    else
        echo "   ❌ Certificate error"
    fi
    echo ""
fi

# 5. Check DNS resolution
echo "5. DNS Resolution:"
DNS_IP=$(dig +short "$SERVER_HOST" @8.8.8.8 | head -1)
if [ -n "$DNS_IP" ]; then
    echo "   ✅ Resolves to: $DNS_IP"
else
    echo "   ⚠️  DNS resolution failed (might be localhost)"
fi
echo ""

# 6. Check response time
echo "6. Response Time:"
RESPONSE_TIME=$(curl -s -o /dev/null -w "%{time_total}" "$SERVER_URL/health" 2>/dev/null || echo "ERROR")
if [ "$RESPONSE_TIME" != "ERROR" ]; then
    echo "   ✅ Response time: ${RESPONSE_TIME}s"
    if (( $(echo "$RESPONSE_TIME > 2.0" | bc -l) )); then
        echo "   ⚠️  Slow response (>2s)"
    fi
else
    echo "   ❌ Could not measure response time"
fi
echo ""

# 7. Check proof-versions endpoint
echo "7. Supported Proof Versions:"
PROOF_VERSIONS=$(curl -s "$SERVER_URL/proof-versions" 2>/dev/null || echo "ERROR")
if [ "$PROOF_VERSIONS" != "ERROR" ]; then
    echo "   ✅ Supported: $PROOF_VERSIONS"
else
    echo "   ❌ Could not retrieve proof versions"
fi
echo ""

echo "=== Diagnostic Complete ==="
echo ""
echo "If any checks failed, see detailed troubleshooting below."