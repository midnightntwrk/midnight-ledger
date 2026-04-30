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

# Uploads SRS parameters to the new S3 bucket (srs.midnight.network).
# Intended to be run from GitHub Actions with OIDC credentials.
# See: .github/workflows/upload-srs.yml

set -e

DRY_RUN=false
if [ "$1" = "--dry-run" ]; then
  DRY_RUN=true
  echo "=== DRY RUN - no files will be uploaded ==="
fi

VERSION=$(cat static/version)
MIDNIGHT_PP=$(nix build .#local-params --print-out-paths)
FILES_ZSWAP=$(echo "$MIDNIGHT_PP/zswap/$VERSION"/*.{prover,verifier,bzkir})
FILES_DUST=$(echo "$MIDNIGHT_PP/dust/$VERSION"/*.{prover,verifier,bzkir})
FILESTORE="s3://stl-euw1-mainnet-srs-download"

for file in $FILES_ZSWAP; do
  NAME=$(basename "$file")
  echo ":: $file -> $FILESTORE/zswap/$VERSION/$NAME"
  if [ "$DRY_RUN" = false ]; then
    aws s3 cp "$file" "$FILESTORE/zswap/$VERSION/$NAME"
  fi
done
for file in $FILES_DUST; do
  NAME=$(basename "$file")
  echo ":: $file -> $FILESTORE/dust/$VERSION/$NAME"
  if [ "$DRY_RUN" = false ]; then
    aws s3 cp "$file" "$FILESTORE/dust/$VERSION/$NAME"
  fi
done
