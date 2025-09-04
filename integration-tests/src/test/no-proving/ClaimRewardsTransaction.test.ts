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
  ClaimRewardsTransaction,
  sampleSigningKey,
  SignatureErased,
  signatureVerifyingKey
} from '@midnight-ntwrk/ledger';
import { assertSerializationSuccess } from '@/test-utils';
import { Static } from '@/test-objects';

describe('Ledger API - ClaimRewardsTransaction', () => {
  /**
   * Test construction of ClaimRewardsTransaction.
   */
  test('should construct ClaimRewardsTransaction correctly', async () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const tx = new ClaimRewardsTransaction(signature.instance, 'local-test', 100n, svk, nonce, signature);

    expect(tx.value).toEqual(value);
    expect(tx.signature.toString()).toEqual(signature.toString());
    expect(tx.nonce).toEqual(nonce);
    expect(tx.owner).toEqual(svk);
    expect(tx.kind).toEqual('Reward');
    assertSerializationSuccess(tx, signature.instance);
  });
});
