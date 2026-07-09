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

# Draft CHANGELOG bullets from conventional-commit subjects since the last
# release tag. Output is a starting point for a human to edit, not final copy.
#
# Args (both optional; no args = top-level ledger changelog behavior):
#   $1  tag prefix   e.g. "onchain-runtime" -> last tag "onchain-runtime-<ver>".
#                    Defaults to "ledger". The glob is anchored on a digit
#                    (<prefix>-[0-9]*) so "storage" does not match "storage-core-*".
#   $2  path filter  e.g. "onchain-runtime" -> only commits touching that dir.
set -euo pipefail

prefix="${1:-ledger}"
path="${2:-}"

last_tag=$(git tag --list "${prefix}-[0-9]*" --sort=-v:refname | head -n1 || true)

# For a per-crate draft with no tag yet, leave a placeholder rather than dumping
# the whole history. Only applies when a prefix was passed explicitly; the
# no-arg (top-level) case keeps the original whole-history fallback.
if [ -z "$last_tag" ] && [ -n "${1:-}" ]; then
  echo "- No ${prefix} release tag found - fill in manually."
  exit 0
fi

log_args=(--no-merges --pretty=format:'%s')
[ -n "$last_tag" ] && log_args+=("${last_tag}..HEAD")
[ -n "$path" ] && log_args+=(-- "${path}/")

bullets=$(git log "${log_args[@]}" \
  | grep -E '^(feat|fix|breaking|perf)(\([^)]*\))?!?:' \
  | sed -E 's/^([a-z]+)(\([^)]*\))?!?:[[:space:]]*/- \1: /' \
  || true)

if [ -z "$bullets" ]; then
  echo "- No conventional-commit changes detected since ${last_tag:-repo start} - fill in manually."
else
  echo "$bullets"
fi
