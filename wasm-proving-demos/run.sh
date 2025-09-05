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

#!/usr/bin/env bash
STATIC_VERSION=$(cat ../static/version)
pushd zkir-mt
  RUSTC_BOOTSTRAP=1 RUSTFLAGS='-C target-feature=+atomics,+bulk-memory' wasm-pack build --target web . -- -Z build-std=panic_abort,std
popd
pushd ..
  nix build .#wasm-dust
popd
pushd webpage/public
  ln -fs $MIDNIGHT_PP/bls_filecoin_2p13 .
popd
pushd webpage/public/midnight/dust
  ln -fs $MIDNIGHT_PP/dust/$STATIC_VERSION/spend.prover .
popd
pushd webpage
  yarn
  yarn serve
popd
