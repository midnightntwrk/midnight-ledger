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

# Checks that the pre-built artifacts committed under `*/static` still
# correspond to the circuits in `zkir-precompiles`.
#
# `zkir-precompiles` is the *input* to the `local-params` Nix derivation, which
# runs `zkir compile-many` to produce the proving keys, verifying keys and
# binary IR. A subset of that output is committed back into the repo, because
# the crates embed it at compile time with `include_bytes!`:
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

cd "$(dirname "$0")/.."

if command -v sha256sum >/dev/null 2>&1; then
  hash_of() { sha256sum "$1" | cut -d' ' -f1; }
else
  hash_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

# Maps a directory of precompiled circuits to the directory holding the
# committed build output for it.
PAIRS=(
  "zkir-precompiles/zswap:zswap/static"
  "zkir-precompiles/dust:ledger/static/dust"
)

failed=0

fail() {
  echo "error: $*" >&2
  failed=1
}

# 1. Every circuit in zkir-precompiles has a byte-identical copy in static/,
#    and vice versa. This is what pins the committed keys to a known circuit.
for pair in "${PAIRS[@]}"; do
  src="${pair%%:*}"
  dst="${pair##*:}"

  for circuit in "$src"/*.zkir; do
    name="$(basename "$circuit")"
    if [ ! -f "$dst/$name" ]; then
      fail "$circuit has no committed copy at $dst/$name" \
        "-- rebuild 'nix build .#local-params' and commit its output"
    elif ! cmp -s "$circuit" "$dst/$name"; then
      fail "$dst/$name differs from $circuit" \
        "-- the committed keys in $dst were built from a different circuit;" \
        "rebuild 'nix build .#local-params' and commit its output"
    fi
  done

  for copy in "$dst"/*.zkir; do
    name="$(basename "$copy")"
    if [ ! -f "$src/$name" ]; then
      fail "$copy has no source circuit at $src/$name -- stale leftover?"
    fi
  done
done

# 2. Each committed .sha256 sidecar matches the file it describes. The sidecars
#    are embedded verbatim as the expected hashes in ZSWAP_EXPECTED_FILES and
#    DUST_EXPECTED_FILES, so a sidecar that disagrees with its own artifact is a
#    partial regeneration that will break key fetching at runtime.
#    Prover keys are not committed (too large), so their sidecars are skipped.
for pair in "${PAIRS[@]}"; do
  dst="${pair##*:}"

  for sidecar in "$dst"/*.sha256; do
    artifact="${sidecar%.sha256}"
    [ -f "$artifact" ] || continue

    expected="$(cut -d' ' -f1 <"$sidecar")"
    actual="$(hash_of "$artifact")"
    if [ "$expected" != "$actual" ]; then
      fail "$sidecar records $expected but $artifact hashes to $actual" \
        "-- rebuild 'nix build .#local-params' and commit its output"
    fi
  done
done

if [ "$failed" -ne 0 ]; then
  exit 1
fi

echo "zkir-precompiles and committed static artifacts are in sync"
