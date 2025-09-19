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
  coinNullifier,
  createShieldedCoinInfo,
  sampleCoinPublicKey,
  sampleEncryptionPublicKey,
  ZswapLocalState,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys
} from '@midnight-ntwrk/ledger';
import { ESK_CLEAR_MESSAGE, CSK_CLEAR_MESSAGE, ZSWAP_SK_CLEAR_MESSAGE } from '@/test-constants';
import { HEX_64_REGEX, Random } from '@/test-objects';

describe('Ledger API - ZswapSecretKeys', () => {
  /**
   * Test error handling for incorrect seed length.
   *
   * @given Seeds with incorrect lengths (31 and 33 bytes)
   * @when Creating ZswapSecretKeys from seed
   * @then Should throw 'Expected 32-byte seed' error
   */
  test('should fail on wrong length seed', () => {
    expect(() => ZswapSecretKeys.fromSeed(new Uint8Array(31))).toThrow('Expected 32-byte seed');
    expect(() => ZswapSecretKeys.fromSeed(new Uint8Array(33))).toThrow('Expected 32-byte seed');
  });

  /**
   * Test creation from seed.
   *
   * @given A 32-byte seed array filled with value 1
   * @when Creating ZswapSecretKeys from seed
   * @then Should generate valid public keys matching expected patterns
   */
  test('should create from seed', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    expect(secretKeys.coinPublicKey).toMatch(HEX_64_REGEX);
    expect(secretKeys.encryptionPublicKey).toMatch(/^[a-fA-F0-9]{64}$/);
  });

  /**
   * Test creation from seed using RNG.
   *
   * @given A 32-byte seed array filled with value 1
   * @when Creating ZswapSecretKeys from seed using RNG
   * @then Should generate valid public keys matching expected patterns
   */
  test('should create from seed using RNG', () => {
    const secretKeys = ZswapSecretKeys.fromSeedRng(new Uint8Array(32).fill(1));
    expect(secretKeys.coinPublicKey).toMatch(HEX_64_REGEX);
    expect(secretKeys.encryptionPublicKey).toMatch(/^[a-fA-F0-9]{64}$/);
  });

  /**
   * Test that secret keys differ from public keys.
   *
   * @given ZswapSecretKeys created from seed RNG
   * @when Comparing secret keys to their corresponding public keys
   * @then Secret keys should not equal their public counterparts
   */
  test('should have secret keys different from public keys', () => {
    const secretKeys = ZswapSecretKeys.fromSeedRng(new Uint8Array(32).fill(1));
    expect(secretKeys.encryptionSecretKey).not.toEqual(secretKeys.encryptionPublicKey);
    expect(secretKeys.coinSecretKey).not.toEqual(secretKeys.coinPublicKey);
  });

  /**
   * Test that secret keys are unusable after clear
   *
   * @given ZswapSecretKeys, an offer and a ZswapLocalState with the offer applied
   * @when Clearing the secret keys
   * @then Should throw an error on attempt to use the keys:
   * - accessing any of the keys
   * - trying to make a spend
   * - trying to apply an offer
   */
  test('should be unusable after clear', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
        0,
        secretKeys.coinPublicKey,
        secretKeys.encryptionPublicKey
      ),
      Random.shieldedTokenType().raw,
      10_000n
    );
    const zswapLocalState = new ZswapLocalState().apply(secretKeys, unprovenOffer);
    const firstCoin = [...zswapLocalState.coins.values()][0];

    secretKeys.clear();

    expect(() => secretKeys.coinPublicKey).toThrow(ZSWAP_SK_CLEAR_MESSAGE);
    expect(() => secretKeys.encryptionPublicKey).toThrow(ZSWAP_SK_CLEAR_MESSAGE);
    expect(() => secretKeys.coinSecretKey).toThrow(ZSWAP_SK_CLEAR_MESSAGE);
    expect(() => secretKeys.encryptionSecretKey).toThrow(ZSWAP_SK_CLEAR_MESSAGE);
    expect(() => zswapLocalState.apply(secretKeys, unprovenOffer)).toThrow(ZSWAP_SK_CLEAR_MESSAGE);
    expect(() => zswapLocalState.spend(secretKeys, firstCoin, 0)).toThrow(ZSWAP_SK_CLEAR_MESSAGE);
  });

  /**
   * Test that coin and encryption secret keys do not outlive the wrapper
   * it is to ensure there is no possibility of dangling references being usable after the wrapper is cleared
   * 
   * @given ZswapSecretKeys and its component secret keys, a zswap offfer, and a coin info
   * @when Clearing the secret keys
   * @then Should throw an error on attempt to use the component keys: serialization, testing an offer and nullifier computation
   
   */
  test('component secret keys should not outlive the wrapper', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const { coinSecretKey, encryptionSecretKey } = secretKeys;

    const coinInfo = createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, sampleCoinPublicKey(), sampleEncryptionPublicKey()),
      Random.shieldedTokenType().raw,
      10_000n
    );

    secretKeys.clear();

    expect(() => coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).toThrow(CSK_CLEAR_MESSAGE);
    expect(() => encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).toThrow(ESK_CLEAR_MESSAGE);
    expect(() => encryptionSecretKey.test(unprovenOffer)).toThrow(ESK_CLEAR_MESSAGE);
    expect(() => coinNullifier(coinInfo, coinSecretKey)).toThrow(CSK_CLEAR_MESSAGE);
  });
});
