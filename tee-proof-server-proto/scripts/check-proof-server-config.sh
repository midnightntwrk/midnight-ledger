#!/bin/bash
# Check proof server configuration and compatibility
# Usage: ./check-proof-server-config.sh

set -e

echo "🔍 Midnight Proof Server Configuration Check"
echo "=============================================="
echo ""

# Check local server
echo "📍 Checking LOCAL proof server..."
if docker ps | grep -q midnight-proof-server; then
    echo "   ✅ Local server is RUNNING"
    LOCAL_VERSION=$(curl -s http://localhost:6300/version 2>/dev/null || echo "ERROR")
    if [ "$LOCAL_VERSION" != "ERROR" ]; then
        echo "   📦 Local version: $LOCAL_VERSION"
    else
        echo "   ❌ Local server not responding"
        LOCAL_VERSION="NOT_RESPONDING"
    fi
else
    echo "   ⚠️  Local server is NOT running"
    LOCAL_VERSION="NOT_RUNNING"
fi
echo ""

# Check remote server
echo "🌐 Checking REMOTE proof server (preview network)..."
REMOTE_VERSION=$(curl -s https://lace-proof-pub.preview.midnight.network/version 2>/dev/null || echo "ERROR")
if [ "$REMOTE_VERSION" != "ERROR" ]; then
    echo "   ✅ Remote server is REACHABLE"
    echo "   📦 Remote version: $REMOTE_VERSION"
else
    echo "   ❌ Remote server not reachable"
    REMOTE_VERSION="NOT_REACHABLE"
fi
echo ""

# Compare versions
echo "🔄 Version Compatibility Check..."
if [ "$LOCAL_VERSION" != "NOT_RUNNING" ] && [ "$LOCAL_VERSION" != "NOT_RESPONDING" ] && [ "$REMOTE_VERSION" != "NOT_REACHABLE" ]; then
    if [ "$LOCAL_VERSION" = "$REMOTE_VERSION" ]; then
        echo "   ✅ COMPATIBLE: Local ($LOCAL_VERSION) matches network ($REMOTE_VERSION)"
        echo "   💡 You can safely use your local proof server"
    else
        echo "   ❌ INCOMPATIBLE: Local ($LOCAL_VERSION) ≠ Network ($REMOTE_VERSION)"
        echo "   ⚠️  Transactions using local server will FAIL"
        echo "   💡 Use remote server OR rebuild local to match network version"
    fi
else
    echo "   ⚠️  Cannot compare - insufficient data"
fi
echo ""

# Check for domain separator commits
echo "📝 Checking for breaking changes..."
cd "$(dirname "$0")/../.." || exit 1

if git log --oneline --all | grep -q "domain.*separator\|PM-20172"; then
    DOMAIN_COMMIT=$(git log --oneline --all | grep -i "domain.*separator\|PM-20172" | head -1 || echo "")
    if [ -n "$DOMAIN_COMMIT" ]; then
        echo "   ℹ️  Found domain separator commit: $DOMAIN_COMMIT"
        echo "   📅 This is a BREAKING CHANGE for proof compatibility"
    fi
fi
echo ""

# Provide recommendations
echo "💡 RECOMMENDATIONS"
echo "=================="

if [ "$LOCAL_VERSION" = "NOT_RUNNING" ]; then
    echo "✅ Local server not running - Lace will use REMOTE server (good!)"
    echo ""
    echo "Next steps:"
    echo "  1. Lace should work fine with remote server"
    echo "  2. No configuration changes needed"

elif [ "$LOCAL_VERSION" != "$REMOTE_VERSION" ] && [ "$LOCAL_VERSION" != "NOT_RESPONDING" ]; then
    echo "⚠️  VERSION MISMATCH DETECTED"
    echo ""
    echo "Option 1: Stop local server (RECOMMENDED)"
    echo "  docker stop midnight-proof-server"
    echo "  → Lace will automatically use remote server"
    echo ""
    echo "Option 2: Downgrade local server to match network"
    echo "  git checkout v${REMOTE_VERSION} 2>/dev/null || echo 'Tag not found'"
    echo "  cd tee-proof-server-proto"
    echo "  make build-local && make run"
    echo ""
    echo "Option 3: Wait for network upgrade"
    echo "  watch -n 300 'curl -s https://lace-proof-pub.preview.midnight.network/version'"
    echo "  → When network shows ${LOCAL_VERSION}, you can use local server"

elif [ "$LOCAL_VERSION" = "$REMOTE_VERSION" ]; then
    echo "✅ VERSIONS MATCH - Local server is compatible!"
    echo ""
    echo "You can safely use either:"
    echo "  • Local server: http://localhost:6300"
    echo "  • Remote server: https://lace-proof-pub.preview.midnight.network"
    echo ""
    echo "To use local server in Lace:"
    echo "  1. DevTools → Application → Storage → Extension Storage"
    echo "  2. Find: redux:persist:midnightContext"
    echo "  3. Set: {\"userNetworksConfigOverrides\": \"{\\\"preview\\\":{\\\"proofServerAddress\\\":\\\"http://localhost:6300\\\"}}\"}"
fi

echo ""
echo "📚 Documentation"
echo "================"
echo "  • VERSION-MISMATCH.md - Details about version compatibility"
echo "  • DEBUG-INTERMITTENT.md - Debug intermittent transaction failures"
echo "  • SUCCESS.md - Complete setup guide"
echo ""

# Health check summary
echo "📊 Health Check Summary"
echo "======================"
printf "Local Server:  "
if [ "$LOCAL_VERSION" != "NOT_RUNNING" ] && [ "$LOCAL_VERSION" != "NOT_RESPONDING" ]; then
    echo "✅ Running ($LOCAL_VERSION)"
elif [ "$LOCAL_VERSION" = "NOT_RESPONDING" ]; then
    echo "❌ Not responding"
else
    echo "⚠️  Not running"
fi

printf "Remote Server: "
if [ "$REMOTE_VERSION" != "NOT_REACHABLE" ]; then
    echo "✅ Reachable ($REMOTE_VERSION)"
else
    echo "❌ Not reachable"
fi

printf "Compatibility: "
if [ "$LOCAL_VERSION" = "$REMOTE_VERSION" ]; then
    echo "✅ Compatible"
elif [ "$LOCAL_VERSION" = "NOT_RUNNING" ]; then
    echo "✅ N/A (using remote)"
else
    echo "❌ Incompatible"
fi

echo ""
echo "✅ Check complete!"
