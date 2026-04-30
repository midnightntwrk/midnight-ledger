#!/usr/bin/env bash
# Bring up the local Midnight stack on fixed loopback ports for the
# wallet's `Undeployed` network. See ./standalone.yml for the full
# topology.

set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "Starting Midnight standalone stack…"
docker compose -f standalone.yml up -d --wait

echo
echo "Stack is up. Endpoints:"
echo "  indexer http   http://127.0.0.1:8088/api/v4/graphql"
echo "  indexer ws     ws://127.0.0.1:8088/api/v4/graphql/ws"
echo "  node  ws       ws://127.0.0.1:9944"
echo "  proof server   http://127.0.0.1:6300"
echo
echo "Run the desktop wallet on Undeployed:"
echo "  cargo run -p dioxus-wallet --release"
echo "  → switch the Network dropdown to 'Undeployed'"
echo
echo "Tear down:  ./standalone-down.sh"
