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

import { ZswapSecretKeys } from '@midnight-ntwrk/ledger';
import { HEX_64_REGEX } from '@/test-objects';

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
});
