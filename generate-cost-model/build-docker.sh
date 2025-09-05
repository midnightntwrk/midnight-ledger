#!/bin/sh

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

mkdir -p .cargo
mv .cargo/config.toml .cargo/config.toml.bak || true
cargo vendor > .cargo/config.toml
cp -r $MIDNIGHT_PP midnight-pp
docker build -t ghcr.io/midnight-ntwrk/generate-ledger-cost-model:latest . -f generate-cost-model/Dockerfile
rm .cargo/config.toml
mv .cargo/config.toml.bak .cargo/config.toml || true
