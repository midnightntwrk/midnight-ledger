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

import { ZswapInput, ZswapChainState } from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, Random, Static } from '@/test-objects';
import { createValidZSwapInput } from '@/test-utils';

describe('Ledger API - ZSwapInput', () => {
  /**
   * Test error handling for non-existent merkle tree index.
   *
   * @given A qualified shielded coin info and an invalid merkle tree index (0)
   * @when Creating a contract-owned ZswapInput
   * @then Should throw an error about invalid index into sparse merkle tree
   */
  test('should throw error on non existent merkle tree index', () => {
    expect(() =>
      ZswapInput.newContractOwned(
        getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(10_000n)),
        0,
        Random.contractAddress(),
        new ZswapChainState()
      )
    ).toThrow('invalid index into sparse merkle tree: 0 -- write creating spend proof');
  });

  /**
   * Test error handling for negative value in shielded coin.
   *
   * @given A qualified shielded coin info with negative value
   * @when Creating a contract-owned ZswapInput
   * @then Should throw an error about u128 bounds
   */
  test('should throw error on negative value', () => {
    expect(() =>
      ZswapInput.newContractOwned(
        getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(-10_000n)),
        0,
        Random.contractAddress(),
        new ZswapChainState()
      )
    ).toThrow("Error: Couldn't deserialize u128 from a BigInt outside u128::MIN..u128::MAX bounds");
  });

  test('should not leak private data in toString()', () => {
    const validZswapInput = createValidZSwapInput(100n);

    const { nullifier } = validZswapInput.zswapInput;

    expect(validZswapInput.zswapInput.toString()).toEqual(`<shielded input Nullifier(${nullifier})>`);
  });

  test('should serialize and deserialize', () => {
    const validZswapInput = createValidZSwapInput(100n);

    const serialized = validZswapInput.zswapInput.serialize();
    const deserialized = ZswapInput.deserialize('pre-proof', serialized);
    expect(deserialized.toString()).toEqual(validZswapInput.zswapInput.toString());
  });
});
