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
  signData,
  SignatureEnabled,
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

  test('should construct ClaimRewardsTransaction with a different claim kind', async () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const tx = new ClaimRewardsTransaction(
      signature.instance,
      'local-test',
      100n,
      svk,
      nonce,
      signature,
      'CardanoBridge'
    );

    expect(tx.value).toEqual(value);
    expect(tx.signature.toString()).toEqual(signature.toString());
    expect(tx.nonce).toEqual(nonce);
    expect(tx.owner).toEqual(svk);
    expect(tx.kind).toEqual('CardanoBridge');
    assertSerializationSuccess(tx, signature.instance);
  });

  test('new - should construct a signature-erased ClaimRewardsTransaction', async () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const tx = ClaimRewardsTransaction.new('local-test', 100n, svk, nonce, 'CardanoBridge');

    expect(tx.value).toEqual(value);
    expect(tx.signature.toString()).toEqual(signature.toString());
    expect(tx.nonce).toEqual(nonce);
    expect(tx.owner).toEqual(svk);
    expect(tx.kind).toEqual('CardanoBridge');
    assertSerializationSuccess(tx, signature.instance);
  });

  test('addSignature - should sign and insert a signature', async () => {
    const signingKey = sampleSigningKey();
    const svk = signatureVerifyingKey(signingKey);
    const nonce = Static.nonce();
    const tx = ClaimRewardsTransaction.new('local-test', 100n, svk, nonce, 'CardanoBridge');
    expect(tx.signature.toString()).toEqual(new SignatureErased().toString());

    const signature = signData(signingKey, tx.dataToSign);
    const signedTx = tx.addSignature(signature);
    expect(signedTx.signature.toString()).toEqual(new SignatureEnabled(signature).toString());
    assertSerializationSuccess(signedTx, signedTx.signature.instance);
  });
});
