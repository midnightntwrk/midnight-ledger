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

import { DUST_SK_CLEAR_MESSAGE } from '@/test-constants';
import { INITIAL_NIGHT_AMOUNT } from '@/test-objects';
import { generateSampleDust } from '@/test/utils/dust';

describe('Ledger API - DustSecretKey', () => {
  /**
   * Test clearing functionality.
   *
   * @given A dust secret key and a dust test state
   * @when Clearing the key
   * @then Should throw an error when trying to make a spend or access the public key
   */
  test('should be unusable after clear', () => {
    const testState = generateSampleDust(INITIAL_NIGHT_AMOUNT);
    const dustSecretKey = testState.dustKey.secretKey;
    const output = testState.dust.utxos[0];

    dustSecretKey.clear();

    expect(() => dustSecretKey.publicKey).toThrow(DUST_SK_CLEAR_MESSAGE);
    expect(() => testState.dust.spend(dustSecretKey, output, 0n, testState.time)).toThrow(DUST_SK_CLEAR_MESSAGE);
  });
});
