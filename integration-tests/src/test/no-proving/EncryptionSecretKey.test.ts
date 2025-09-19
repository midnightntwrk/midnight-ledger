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

import {
  EncryptionSecretKey,
  ZswapSecretKeys,
  ZswapOffer,
  ZswapOutput,
  createShieldedCoinInfo,
  sampleCoinPublicKey,
  sampleEncryptionPublicKey
} from '@midnight-ntwrk/ledger';
import { ESK_CLEAR_MESSAGE } from '@/test-constants';
import { Random } from '@/test-objects';

describe('Ledger API - EncryptionSecretKey', () => {
  /**
   * Test serialization and deserialization.
   *
   * @given An encryption secret key from seed
   * @when Serializing
   * @then Should have successful deserialization
   */
  test('should serialize and deserialize correctly', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const { encryptionSecretKey } = secretKeys;
    const serialized = encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize();

    expect(() => EncryptionSecretKey.deserialize(serialized)).not.toThrow();
  });

  /**
   * Test offer testing functionality.
   *
   * @given An encryption secret key and a ZswapOffer
   * @when Testing the offer against the secret key
   * @then Should return boolean result (typically false for random keys/offer)
   */
  test('should return boolean when testing offer', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const { encryptionSecretKey } = secretKeys;
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
        0,
        sampleCoinPublicKey(),
        sampleEncryptionPublicKey()
      ),
      Random.shieldedTokenType().raw,
      10_000n
    );

    const result = encryptionSecretKey.test(unprovenOffer);

    expect(typeof result).toBe('boolean');
    expect(result).toBe(false);
  });

  /**
   * Test clearing functionality.
   *
   * @given An encryption secret key and a ZswapOffer
   * @when Clearing the key
   * @then Should throw an error on testing the offer or trying to serialize the key
   */
  test('should be unusable after clear', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const { encryptionSecretKey } = secretKeys;
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
        0,
        sampleCoinPublicKey(),
        sampleEncryptionPublicKey()
      ),
      Random.shieldedTokenType().raw,
      10_000n
    );

    encryptionSecretKey.clear();

    expect(() => encryptionSecretKey.test(unprovenOffer)).toThrow(ESK_CLEAR_MESSAGE);
    expect(() => encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).toThrow(ESK_CLEAR_MESSAGE);
  });
});
