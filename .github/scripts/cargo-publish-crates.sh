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

# Publish publishable workspace crates to crates.io, in dependency order.
#
# Final releases only ($1 with a pre-release suffix is refused; the caller also
# gates the job out). cargo-workspaces handles the dependency ordering, the
# index-propagation wait between crates, and skipping crates already published
# or marked `publish = false` - so re-runs after a partial failure are safe.
# crates.io is immutable, so there is no "force": a bad version needs a bump.
#
# Args: $1  display version, e.g. 8.2.0 (final) or 8.2.0-rc.1 (pre-release).
# Env:  CARGO_REGISTRY_TOKEN  crates.io token (real run); DRY_RUN  "true" to rehearse.
set -euo pipefail

VERSION="${1:?usage: cargo-publish-crates.sh <version>}"

if [[ "$VERSION" == *-* ]]; then
  echo "::error::$VERSION is a pre-release - crates.io publishing is skipped for pre-releases (see RELEASING.md)."
  exit 1
fi

# Pin the CLI surface; cargo install drops the binary in $CARGO_HOME/bin.
cargo install cargo-workspaces --version '^0.3' --locked
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

# --from-git: publish versions as committed (tags already pushed by the build job).
# --no-verify: the test jobs already built this commit; skip the redundant verify.
# --allow-dirty: the release run may have committed regenerated docs.
args=(--from-git --no-verify --allow-dirty --yes)

if [ "${DRY_RUN:-false}" = "true" ]; then
  echo "[dry-run] cargo workspaces publish ${args[*]} --dry-run"
  cargo workspaces publish "${args[@]}" --dry-run
  exit 0
fi

if [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
  echo "::error::CARGO_REGISTRY_TOKEN is not set - cannot authenticate to crates.io"
  exit 1
fi

# Auth comes from CARGO_REGISTRY_TOKEN in the environment (cargo reads it
# natively); keep it off argv so it can't leak via process listings or traces.
cargo workspaces publish "${args[@]}"
