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

import { provingProvider, type ProvingKeyMaterial } from '@midnight-ntwrk/zkir-v2';
import { readFile } from 'fs/promises';
import { parentPort, workerData } from 'worker_threads';
import path from 'path';

let __filename: string;

const keyMaterialProvider = {
  lookupKey: async (keyLocation: string): Promise<ProvingKeyMaterial | undefined> => {
    // Ideally get this from /static/version, but I'm not sure if this gets run
    // against a consistent dir.
    const staticVersionFile = path.resolve(path.dirname(__filename), '../../static/version');
    const ver = await readFile(staticVersionFile, 'utf-8');
    const pth = {
      'midnight/zswap/spend': `zswap/${ver}/spend`,
      'midnight/zswap/output': `zswap/${ver}/output`,
      'midnight/zswap/sign': `zswap/${ver}/sign`
    }[keyLocation];
    if (pth === undefined) {
      return undefined;
    }
    const pk = readFile(`${process.env.MIDNIGHT_PP}/${pth}.prover`);
    const vk = readFile(`${process.env.MIDNIGHT_PP}/${pth}.verifier`);
    const ir = readFile(`${process.env.MIDNIGHT_PP}/${pth}.bzkir`);
    return {
      proverKey: await pk,
      verifierKey: await vk,
      ir: await ir
    };
  },
  getParams: async (k: number): Promise<Uint8Array> => {
    return readFile(`${process.env.MIDNIGHT_PP}/bls_filecoin_2p${k}`);
  }
};
const wasmProver = provingProvider(keyMaterialProvider);

const [op, fname, args]: ['check' | 'prove', string, any[]] = workerData;
__filename = fname;
// we handle polymorphic data here
// @ts-nocheck
if (op === 'check') {
  const [a, b] = args;
  const result = await wasmProver.check(a, b);
  parentPort!.postMessage(result);
} else {
  const [a, b, c] = args;
  const result = await wasmProver.prove(a, b, c);
  parentPort!.postMessage(result);
}
