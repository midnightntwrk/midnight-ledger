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

# Repack an npm tarball under a different scope (@old/name -> @new/name).
# Only package.json's `name` changes; everything else is repacked as-is.
# Prints the path of the rescoped tarball on stdout.
#
# Args:
#   $1  tgz path (may live in the read-only nix store)
#   $2  target scope, e.g. @midnightntwrk
#   $3  writable output directory
set -euo pipefail

TGZ="$1"
NEW_SCOPE="$2"
OUT_DIR="$3"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

tar -xzf "$TGZ" -C "$WORK"
NAME=$(jq -r '.name' "$WORK/package/package.json")
BASE="${NAME##*/}"
jq --arg name "${NEW_SCOPE}/${BASE}" '.name = $name' "$WORK/package/package.json" > "$WORK/package.json.new"
# mv (not cp) so a read-only file extracted from the nix store is replaced.
mv "$WORK/package.json.new" "$WORK/package/package.json"

OUT="${OUT_DIR}/${NEW_SCOPE#@}-${BASE}-rescoped.tgz"
# COPYFILE_DISABLE keeps BSD tar (macOS dev machines) from adding AppleDouble
# metadata entries; GNU tar (CI) ignores it.
COPYFILE_DISABLE=1 tar -czf "$OUT" -C "$WORK" package
echo "$OUT"
