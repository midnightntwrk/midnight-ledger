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

import { DustState } from '@midnight-ntwrk/ledger';
import { expect } from 'vitest';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - DustState', () => {
  /**
   * Test string representation of DustState.
   *
   * @given A new DustState instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const dustState = new DustState();

    const expected = `DustState {
    utxo: DustUtxoState {
        commitments: MerkleTree(root = Some(-)) {},
        commitments_first_free: 0,
        nullifiers: {},
        root_history: {},
    },
    generation: DustGenerationState {
        address_delegation: {},
        generating_tree: MerkleTree(root = Some(-)) {},
        generating_tree_first_free: 0,
        generating_set: {},
        night_indices: {},
        root_history: {},
    },
}`;

    expect(dustState.toString()).toEqual(expected);
  });

  /**
   * Test serialization and deserialization of DustState.
   *
   * @given A new DustState instance
   * @when Calling serialize method
   * @and Calling deserialize method
   * @then Should return formatted strings with the same values
   */
  test('should serialize and deserialize', () => {
    assertSerializationSuccess(new DustState());
  });

  /**
   * Test all getters of DustState.
   *
   * @given A new DustState instance
   * @when Checking all getters
   * @then Should return the same values as initially set
   */
  test('should have all getters valid', () => {
    const dustState = new DustState();

    expect(dustState.utxo).toBeDefined();
    expect(dustState.generation).toBeDefined();
  });
});
