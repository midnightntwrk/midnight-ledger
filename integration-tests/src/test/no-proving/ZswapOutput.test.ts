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

import { createShieldedCoinInfo, ZswapOutput } from '@midnight-ntwrk/ledger';
import { HEX_64_REGEX, Random } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe('Ledger API -  ZswapOutput', () => {
  /**
   * Test creation of ZswapOutput.
   *
   * @given A shielded coin info with random token type and value 10,000
   * @when Creating a new ZswapOutput with coin and encryption public keys
   * @then Should have valid commitment, undefined contract address, and pass serialization check
   */
  test('should create output', () => {
    const unprovenOutput = ZswapOutput.new(
      createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
      0,
      Random.coinPublicKey(),
      Random.encryptionPublicKey()
    );

    expect(unprovenOutput.commitment).toMatch(HEX_64_REGEX);
    expect(unprovenOutput.contractAddress).toBeUndefined();
    assertSerializationSuccess(unprovenOutput, undefined, ProofMarker.preProof);
  });

  /**
   * Test serialization and deserialization of ZswapOutput.
   *
   * @given A ZswapOutput created with random parameters
   * @when Serializing and then deserializing the output
   * @then Should maintain identical string representation
   */
  test('should serialize and deserialize correctly', () => {
    const unprovenOutput = ZswapOutput.new(
      createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
      0,
      Random.coinPublicKey(),
      Random.encryptionPublicKey()
    );

    expect(ZswapOutput.deserialize('pre-proof', unprovenOutput.serialize()).toString()).toEqual(
      unprovenOutput.toString()
    );
  });

  /**
   * Test proof property of unproven output.
   *
   * @given A newly created ZswapOutput
   * @when Checking the proof instance
   * @then Should be of pre-proof type for unproven output
   */
  test('should have a proof instance of pre-proof type', () => {
    const unprovenOutput = ZswapOutput.new(
      createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
      0,
      Random.coinPublicKey(),
      Random.encryptionPublicKey()
    );

    expect(unprovenOutput.proof.instance).toEqual('pre-proof');
  });
});
