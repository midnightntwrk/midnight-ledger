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

set -euo pipefail

# Append, never clobber: `.cargo/config.toml` is tracked and carries
# `[env] RUST_MIN_STACK`, without which ZK proving overflows the 2 MiB default
# thread stack and the micro-dao test aborts with SIGABRT. Deleting the file
# here silently removed that setting from every CI job.
mkdir -p ./.cargo
if ! grep -q '^git-fetch-with-cli' ./.cargo/config.toml 2>/dev/null; then
  {
    echo
    echo "[net]"
    echo "git-fetch-with-cli = true"
  } >> ./.cargo/config.toml
fi
cat ./.cargo/config.toml
