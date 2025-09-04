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

import { MerkleTreeCollapsedUpdate, ZswapChainState } from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';

describe('Ledger API - MerkleTreeCollapsedUpdate', () => {
  /**
   * Test validation when start index is greater than end index.
   *
   * @given A ZswapChainState with start index 2 and end index 1
   * @when Creating a MerkleTreeCollapsedUpdate
   * @then Should throw error about attempted update with end before start
   */
  test('should fail when start > end', () => {
    expect(() => new MerkleTreeCollapsedUpdate(new ZswapChainState(), 2n, 1n)).toThrow(
      'attempted update with end (1) after before (2)'
    );
  });

  /**
   * Test validation when updating on already updated sub-tree.
   *
   * @given A ZswapChainState with start index 1 and end index 1
   * @when Creating a MerkleTreeCollapsedUpdate
   * @then Should throw error about attempted update on updated sub-tree
   */
  test('should fail when update on updated sub-tree', () => {
    expect(() => new MerkleTreeCollapsedUpdate(new ZswapChainState(), 1n, 1n)).toThrow(
      'attempted update on updated sub-tree at 1/0'
    );
  });

  /**
   * Test serialization and deserialization of MerkleTreeCollapsedUpdate.
   *
   * @given A MerkleTreeCollapsedUpdate created from modified ZswapChainState
   * @when Serializing and then deserializing the update
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize correctly', () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const state = new ZswapChainState();
    const stateAfter = state.tryApply(proofErasedTransaction.guaranteedOffer!)[0].postBlockUpdate(new Date());
    const mt = new MerkleTreeCollapsedUpdate(stateAfter, 0n, 1n);
    const mt2 = MerkleTreeCollapsedUpdate.deserialize(mt.serialize());

    expect(mt.toString()).toEqual(mt2.toString());
  });
});
