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

import { LedgerState, ZswapChainState } from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - ZSwapChainState', () => {
  /**
   * Test string representation of ZswapChainState.
   *
   * @given A new ZswapChainState
   * @when Converting to string
   * @then Should return expected string format with initial values
   */
  test('should print out correct string representation', () => {
    const zswapChainState = new ZswapChainState();

    expect(zswapChainState.toString()).toEqual(
      'State {\n' +
        '    coin_coms: MerkleTree(root = Some(-)) {},\n' +
        '    coin_coms_set: {},\n' +
        '    first_free: 0,\n' +
        '    nullifiers: {},\n' +
        '    past_roots: {},\n' +
        '}'
    );
  });

  /**
   * Test deserialization from LedgerState.
   *
   * @given A ZswapChainState serialized through LedgerState
   * @when Deserializing back to ZswapChainState
   * @then Should maintain identical string representation
   */
  test('should deserialize from LedgerState correctly', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const serialized = ledgerState.serialize();
    const zswapChainStateDeserialized = ZswapChainState.deserializeFromLedgerState(serialized);

    expect(zswapChainStateDeserialized.toString()).toEqual(zswapChainState.toString());
  });

  /**
   * Test direct serialization and deserialization.
   *
   * @given A ZswapChainState
   * @when Serializing and then deserializing directly
   * @then Should maintain identical string representation
   */
  test('should serialize and deserialize correctly', () => {
    const zswapChainState = new ZswapChainState();
    const serialized = zswapChainState.serialize();
    const zswapChainStateDeserialized = ZswapChainState.deserialize(serialized);

    expect(zswapChainStateDeserialized.toString()).toEqual(zswapChainState.toString());
  });

  /**
   * Test applying proof-erased transaction without whitelist.
   *
   * @given A ZswapChainState and proof-erased transaction
   * @when Applying the transaction without whitelist
   * @then Should successfully update state and return results
   */
  test('should apply proof-erased transaction without whitelist', () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const state = new ZswapChainState();

    const [stateAfter, _] = state.tryApply(proofErasedTransaction.guaranteedOffer!);

    expect(stateAfter.toString()).not.toEqual(state.toString());
    assertSerializationSuccess(stateAfter);
  });

  /**
   * Test applying proof-erased transaction with whitelist.
   *
   * @given A ZswapChainState, proof-erased transaction, and contract whitelist
   * @when Applying the transaction with whitelist constraint
   * @then Should successfully update state and return mapping results
   */
  test('should apply proof-erased transaction with whitelist', () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const state = new ZswapChainState();
    const contractAddress = Static.contractAddress();
    const whitelist: Set<string> = new Set<string>([contractAddress]);

    const [stateAfter, mapResult] = state.tryApply(proofErasedTransaction.guaranteedOffer!, whitelist);

    expect(stateAfter.toString()).not.toEqual(state.toString());
    expect(stateAfter.firstFree).toEqual(1n);
    expect(mapResult.size).toEqual(1);
    const next = mapResult.keys().next();
    expect(next.done).toEqual(false);
    assertSerializationSuccess(stateAfter);
  });

  /**
   * Test applying offer without whitelist constraint.
   *
   * @given A ZswapChainState and proof-erased transaction
   * @when Applying offer without whitelist constraint
   * @then Should successfully update state and increment firstFree counter
   */
  test('should apply offer without whitelist constraint', () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const state = new ZswapChainState();

    const [stateAfter, mapResult] = state.tryApply(proofErasedTransaction.guaranteedOffer!);

    expect(stateAfter.toString()).not.toEqual(state.toString());
    expect(stateAfter.firstFree).toEqual(1n);
    expect(mapResult.size).toEqual(1);
    assertSerializationSuccess(stateAfter);
  });

  test('filter - should filter state for specific contract address', () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const state = new ZswapChainState();
    const contractAddress = Static.contractAddress();

    // Apply transaction to populate state with data
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const [stateAfter, _] = state.tryApply(proofErasedTransaction.guaranteedOffer!);

    // Filter the state for the specific contract address
    const filteredState = stateAfter.filter(contractAddress);

    // Verify the filtered state is different from the original
    expect(filteredState.toString()).not.toEqual(stateAfter.toString());

    // Verify filtered state is valid and serializable
    assertSerializationSuccess(filteredState);
  });
});
