#!/usr/bin/env bash

# This file is part of midnight-ledger.
# Copyright (C) 2025 Midnight Foundation
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

set -e
VERSION=$(cat static/version)
FILES_ZSWAP=$(echo "$MIDNIGHT_PP/zswap/$VERSION"/*.{prover,verifier,bzkir})
FILES_DUST=$(echo "$MIDNIGHT_PP/dust/$VERSION"/*.{prover,verifier,bzkir})
FILESTORE="s3://midnight-s3-fileshare-dev-eu-west-1"
for file in $FILES_ZSWAP; do
  NAME=$(basename "$file")
  echo ":: $file -> $FILESTORE/zswap/$VERSION/$NAME"
  aws s3 cp "$file" "$FILESTORE/zswap/$VERSION/$NAME"
done
for file in $FILES_DUST; do
  NAME=$(basename "$file")
  echo ":: $file -> $FILESTORE/dust/$VERSION/$NAME"
  aws s3 cp "$file" "$FILESTORE/dust/$VERSION/$NAME"
done
