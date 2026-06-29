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
# ledger tag. Output is a starting point for a human to edit, not final copy.
set -euo pipefail

last_tag=$(git tag --list 'ledger-*' --sort=-v:refname | head -n1 || true)
range=""
[ -n "$last_tag" ] && range="${last_tag}..HEAD"

bullets=$(git log --no-merges --pretty=format:'%s' $range \
  | grep -E '^(feat|fix|breaking|perf)(\([^)]*\))?!?:' \
  | sed -E 's/^([a-z]+)(\([^)]*\))?!?:[[:space:]]*/- \1: /' \
  || true)

if [ -z "$bullets" ]; then
  echo "- No conventional-commit changes detected since ${last_tag:-repo start} - fill in manually."
else
  echo "$bullets"
fi
