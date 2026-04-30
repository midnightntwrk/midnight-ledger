#!/usr/bin/env bash
# Tear down the local Midnight stack started by ./standalone-up.sh.
# `--volumes` so chain state doesn't accumulate across runs.

set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

docker compose -f standalone.yml down --volumes
