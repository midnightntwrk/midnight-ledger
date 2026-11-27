#!/usr/bin/env bash

# This file is part of midnight-ledger.
# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# You may not use this file except in compliance with the License.
# You may obtain a copy of the License at
# http://www.apache.org/licenses/LICENSE-2.0
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Checks that the pre-built artifacts committed under `*/static` match the
# output of the `local-params` Nix derivation, whose path is given as the only
# argument:
#
#   ./scripts/check-static-artifacts-sync.sh "$(nix build .#local-params --no-link --print-out-paths)"
#
# `local-params` compiles `zswap/zswap.compact` and `ledger/dust.compact` with
# `compactc`, then produces the proving keys, verifying keys and binary IR with
# `zkir compile-many`. A subset of that output is committed back into the repo,
# because the crates embed it at compile time with `include_bytes!`:
#
#   *.verifier  -> zswap/src/verify.rs, ledger/src/dust.rs
#   *.sha256    -> ZSWAP_EXPECTED_FILES / DUST_EXPECTED_FILES integrity manifest
#   *.bzkir     -> the same manifest, plus zswap's test_pi_lengths
#
# Cargo never regenerates any of that, so editing a circuit without rebuilding
# `local-params` and committing the result leaves the crates embedding a stale
# verifying key. That builds and tests green locally, and only fails later when
# the data provider rejects the freshly uploaded S3 artifacts as hash
# mismatches. This script catches it at PR time instead.

set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <local-params output path>" >&2
  exit 2
fi
params="$1"

cd "$(dirname "$0")/.."

version="$(cat static/version)"

# Maps a directory of built circuits to the directory holding the committed
# copy of it.
PAIRS=(
  "$params/zswap/$version:zswap/static"
  "$params/dust/$version:ledger/static/dust"
)

failed=0

fail() {
  echo "error: $*" >&2
  failed=1
}

for pair in "${PAIRS[@]}"; do
  built="${pair%%:*}"
  committed="${pair##*:}"

  # Every committed file is byte-identical to its build output. Prover keys
  # are not committed (too large), so only their sidecars are compared.
  for copy in "$committed"/*; do
    name="$(basename "$copy")"
    if [ ! -f "$built/$name" ]; then
      fail "$copy is not built by local-params -- stale leftover?"
    elif ! cmp -s "$copy" "$built/$name"; then
      fail "$copy differs from the local-params build" \
        "-- rebuild 'nix build .#local-params' and commit its output"
    fi
  done

  # Every circuit built has a committed copy.
  for circuit in "$built"/*.zkir; do
    name="$(basename "$circuit")"
    if [ ! -f "$committed/$name" ]; then
      fail "$circuit has no committed copy at $committed/$name" \
        "-- rebuild 'nix build .#local-params' and commit its output"
    fi
  done
done

if [ "$failed" -ne 0 ]; then
  exit 1
fi

echo "committed static artifacts match the local-params build"
