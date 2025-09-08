// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import { Transaction, CostModel } from '@midnight-ntwrk/ledger';
import * as zkirV2 from '@midnight-ntwrk/zkir-v2';

async function get(url) {
  const resp = await fetch(url);
  const blob = await resp.blob();
  return new Uint8Array(await blob.arrayBuffer());
}

(async () => {
  const tx = Transaction.deserialize('signature', 'pre-proof', 'pre-binding', await get('/unproven.bin'));
  console.log(tx.toString());
  const kmProvider = {
    lookupKey: async (keyLocation) => {
      return {
        proverKey: await get(`/${keyLocation}.prover`),
        verifierKey: await get(`/${keyLocation}.verifier`),
        ir: await get(`/${keyLocation}.bzkir`),
      };
    },
    getParams: async (k) => {
      return get(`/bls_filecoin_2p${k}`);
    },
  };
  const provingProvider = zkirV2.provingProvider(kmProvider);
  postMessage("start");
  const provePromise = tx.prove(provingProvider, CostModel.dummyCostModel());
  const provenTx = await provePromise;
  console.log(provenTx.toString());
  postMessage("done");
})()
