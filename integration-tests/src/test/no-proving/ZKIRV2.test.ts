// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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

import { Zkir } from '@midnight-ntwrk/zkir-v2';
import { describe, it, expect } from 'vitest';
import { keyMaterialProvider } from '../../test-objects';

const keys = [
  { keyLocation: 'midnight/zswap/spend', k: 15 },
  { keyLocation: 'midnight/zswap/output', k: 14 },
  { keyLocation: 'midnight/zswap/sign', k: 13 },
  { keyLocation: 'midnight/dust/spend', k: 13 }
];

describe('ZKIRV2', () => {
  it.concurrent.each(keys)('reports the correct k value for $keyLocation', async ({ keyLocation, k }) => {
    const rawKeyMaterial = await keyMaterialProvider.lookupKey(keyLocation);

    const zkir = Zkir.deserialize(rawKeyMaterial!.ir);

    expect(zkir.getK()).toBe(k);
  });

  it.concurrent.each(keys)('can serialize and deserialize a Zkir of $keyLocation', async ({ keyLocation }) => {
    const jsonZkir = await keyMaterialProvider.lookupJsonIr(keyLocation);
    const serializedFromJson = Zkir.fromJson(jsonZkir!).serialize();

    const serializedLoaded = (await keyMaterialProvider.lookupKey(keyLocation))!.ir;

    expect(Buffer.from(serializedFromJson)).toEqual(serializedLoaded);
  });
});
