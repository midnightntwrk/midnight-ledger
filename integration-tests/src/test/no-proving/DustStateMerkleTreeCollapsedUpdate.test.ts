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

import { DustStateMerkleTreeCollapsedUpdate } from '@midnight-ntwrk/ledger';
import { INITIAL_NIGHT_AMOUNT } from '@/test-objects';
import { generateSampleDust } from '@/test/utils/dust';

describe('Ledger API - DustStateMerkleTreeCollapsedUpdate', () => {
  /**
   * Test building collapsed updates from the generation and commitment trees.
   *
   * @given An on-chain dust state populated by a sample registration
   * @when Building collapsed updates via the generation and commitment factories
   * @then The updates should be serializable, round-trip through deserialize, and render a string
   */
  test('builds serializable, round-tripping updates from the generation and commitment trees', () => {
    const { ledger } = generateSampleDust(INITIAL_NIGHT_AMOUNT);

    const generationUpdate = DustStateMerkleTreeCollapsedUpdate.newFromGenerationTree(ledger.dust.generation, 0n, 0n);
    const generationSerialized = generationUpdate.serialize();

    expect(generationSerialized.length).toBeGreaterThan(0);
    expect(generationUpdate.toString().length).toBeGreaterThan(0);
    expect(DustStateMerkleTreeCollapsedUpdate.deserialize(generationSerialized).serialize()).toEqual(
      generationSerialized
    );

    const commitmentUpdate = DustStateMerkleTreeCollapsedUpdate.newFromCommitmentTree(ledger.dust.utxo, 0n, 0n);
    expect(commitmentUpdate.serialize().length).toBeGreaterThan(0);
  });
});
