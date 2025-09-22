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

import { ZswapSecretKeys, createShieldedCoinInfo, coinNullifier } from '@midnight-ntwrk/ledger';
import { CSK_CLEAR_MESSAGE } from '@/test-constants';
import { Random } from '@/test-objects';

describe('Ledger API - CoinSecretKey', () => {
  /**
   * Test clearing functionality.
   *
   * @given An encryption secret key and a ZswapOffer
   * @when Clearing the key
   * @then Should throw an error on key serialization as well as computing nullifier
   */
  test('should be unusable after clear', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const { coinSecretKey } = secretKeys;

    const coinInfo = createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n);

    coinSecretKey.clear();

    expect(() => coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).toThrow(CSK_CLEAR_MESSAGE);
    expect(() => coinNullifier(coinInfo, coinSecretKey)).toThrow(CSK_CLEAR_MESSAGE);
  });
});
