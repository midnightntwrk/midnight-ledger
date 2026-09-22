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

# Checks that the ledger stays buildable, with its default (verification-only)
# features, for a target that has `std` but no operating system. This is what a
# Substrate runtime embedding the ledger needs (wasm32 today, PolkaVM later).
#
# Two checks:
#
#  1. The normal-dependency graph of `midnight-ledger-v9` for
#     `wasm32-unknown-unknown` must not contain any crate from the deny list
#     below: HTTP/TLS stacks (C/asm), database engines, async runtimes,
#     terminal I/O, or `getrandom` newer than 0.2 (0.3+ needs a backend selected
#     by the final binary, via a feature or `--cfg getrandom_backend=...`, and
#     only ever appeared here through file-writing helpers).
#  2. `cargo build --target wasm32-unknown-unknown -p midnight-ledger-v9` must
#     succeed.
#
# Crates that are still expected in the graph because they come from
# dependencies outside this repository (`midnight-proofs`, `midnight-curves`,
# `midnight-zk-stdlib`) are on the *warn* list: they are reported but do not
# fail the check. See docs/vm-targets.md for the status of each.
#
# Usage: scripts/check-vm-target.sh [--no-build]
#
# Requires the `wasm32-unknown-unknown` target:
#     rustup target add wasm32-unknown-unknown

set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")/.."

TARGET=wasm32-unknown-unknown
PACKAGE=midnight-ledger-v9

# Crate names that must never appear in a verification-only build.
DENY=(
  reqwest hyper hyper-util hyper-rustls rustls rustls-webpki webpki-roots
  aws-lc-sys aws-lc-rs ring openssl-sys native-tls
  rusqlite libsqlite3-sys r2d2 r2d2_sqlite parity-db
  tokio tokio-util mio
  tracing-subscriber indicatif atomic-write-file sysinfo
  os_pipe pprof criterion
)
# Crates we know are still pulled in by dependencies outside this repository.
WARN=(
  rayon goldenfile similar-asserts console tempfile
)

echo "==> Resolving normal dependencies of ${PACKAGE} for ${TARGET}"
graph=$(cargo tree -p "$PACKAGE" -e normal --target "$TARGET" --prefix none --locked 2>/dev/null \
  | awk '{print $1" "$2}' | sort -u)

status=0

for crate in "${DENY[@]}"; do
  if hit=$(grep -E "^${crate} v" <<<"$graph"); then
    echo "::error::forbidden crate in verification build: ${hit}"
    cargo tree -p "$PACKAGE" -e normal --target "$TARGET" --locked -i "$crate" --depth 4 2>/dev/null | head -20 || true
    status=1
  fi
done

# getrandom: only the 0.2 line is acceptable (see docs/vm-targets.md for the
# custom-backend recipe); 0.3+ requires the final binary to select a backend
# (`wasm_js` feature on wasm32, `--cfg getrandom_backend=...` elsewhere), which
# a library cannot do for its consumers.
if hit=$(grep -E '^getrandom v(0\.[3-9]|[1-9])' <<<"$graph"); then
  echo "::error::getrandom newer than 0.2 in verification build: ${hit}"
  cargo tree -p "$PACKAGE" -e normal --target "$TARGET" --locked -i getrandom@0.3 --depth 6 2>/dev/null | head -20 || true
  status=1
fi

for crate in "${WARN[@]}"; do
  if hit=$(grep -E "^${crate} v" <<<"$graph"); then
    echo "::warning::known upstream leftover in verification build: ${hit}"
  fi
done

echo "==> C / assembly crates in the graph (build-script edges included):"
cargo tree -p "$PACKAGE" -e normal,build --target "$TARGET" --prefix none --locked 2>/dev/null \
  | awk '{print $1" "$2}' | sort -u | grep -E '^(blst|cc|cmake|secp256k1-sys|zstd-sys|libsqlite3-sys|ring|aws-lc-sys) ' \
  | sed 's/^/    /' || true

if [ "$status" -ne 0 ]; then
  echo "==> dependency check FAILED"
  exit "$status"
fi
echo "==> dependency check passed"

if [ "${1:-}" = "--no-build" ]; then
  exit 0
fi

echo "==> cargo build --target ${TARGET} -p ${PACKAGE} --locked"
cargo build --target "$TARGET" -p "$PACKAGE" --locked
echo "==> ${PACKAGE} builds for ${TARGET} with default features"
