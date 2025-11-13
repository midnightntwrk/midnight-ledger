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
  signatureVerifyingKey,
  Transaction,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { assertSerializationSuccess } from '@/test-utils';
import { INITIAL_NIGHT_AMOUNT, LOCAL_TEST_NETWORK_ID, Static } from '@/test-objects';
import { TestState } from '@/test/utils/TestState';

describe('Ledger API - ClaimRewardsTransaction', () => {
  /**
   * Test construction of ClaimRewardsTransaction.
   */
  test('should construct ClaimRewardsTransaction correctly', async () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const tx = new ClaimRewardsTransaction(signature.instance, LOCAL_TEST_NETWORK_ID, 100n, svk, nonce, signature);

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
      LOCAL_TEST_NETWORK_ID,
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
    const tx = ClaimRewardsTransaction.new(LOCAL_TEST_NETWORK_ID, 100n, svk, nonce, 'CardanoBridge');

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
    const tx = ClaimRewardsTransaction.new(LOCAL_TEST_NETWORK_ID, 100n, svk, Static.nonce(), 'CardanoBridge');
    expect(tx.signature.toString()).toEqual(new SignatureErased().toString());

    const signature = signData(signingKey, tx.dataToSign);
    const signedTx = tx.addSignature(signature);
    expect(signedTx.signature.toString()).toEqual(new SignatureEnabled(signature).toString());
    assertSerializationSuccess(signedTx, signedTx.signature.instance);
  });

  test('should apply the ClaimRewardsTransaction correctly', async () => {
    const state = TestState.new();
    state.distributeNight(state.initialNightAddress, INITIAL_NIGHT_AMOUNT, state.time);
    expect(state.ledger.utxo.utxos.values().next().value).toBeUndefined();

    const rewards = ClaimRewardsTransaction.new(
      LOCAL_TEST_NETWORK_ID,
      INITIAL_NIGHT_AMOUNT,
      state.nightKey.verifyingKey(),
      Static.nonce(),
      'Reward'
    );
    const signature = state.nightKey.signData(rewards.dataToSign);
    const signedRewards = rewards.addSignature(signature);

    const tx = Transaction.fromRewards(signedRewards);
    state.assertApply(tx, new WellFormedStrictness());
    expect(state.ledger.utxo.utxos.values().next().value?.value).toEqual(INITIAL_NIGHT_AMOUNT);
  });
});
