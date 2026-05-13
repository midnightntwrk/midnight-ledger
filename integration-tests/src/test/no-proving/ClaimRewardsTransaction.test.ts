// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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
  addressFromKey,
  ClaimRewardsTransaction,
  sampleSigningKey,
  SignatureEnabled,
  SignatureErased,
  signatureVerifyingKey,
  signData,
  Transaction,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { assertSerializationSuccess } from '@/test-utils';
import { INITIAL_NIGHT_AMOUNT, LOCAL_TEST_NETWORK_ID, Static } from '@/test-objects';
import { TestState } from '@/test/utils/TestState';
import { SignatureKindMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Ledger API - ClaimRewardsTransaction', () => {
  /**
   * Test construction of ClaimRewardsTransaction.
   */
  test('should construct ClaimRewardsTransaction correctly', () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const tx = new ClaimRewardsTransaction(signature.instance, LOCAL_TEST_NETWORK_ID, value, svk, nonce, signature);

    expect(tx.value).toEqual(value);
    expect(tx.signature.toString()).toEqual(signature.toString());
    expect(tx.nonce).toEqual(nonce);
    expect(tx.owner).toEqual(svk);
    expect(tx.kind).toEqual('Reward');
    assertSerializationSuccess(tx, signature.instance);
  });

  test('should construct ClaimRewardsTransaction with the CardanoBridge claim kind', () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const kind = 'CardanoBridge';
    const tx = new ClaimRewardsTransaction(
      signature.instance,
      LOCAL_TEST_NETWORK_ID,
      value,
      svk,
      nonce,
      signature,
      kind
    );

    expect(tx.value).toEqual(value);
    expect(tx.signature.toString()).toEqual(signature.toString());
    expect(tx.nonce).toEqual(nonce);
    expect(tx.owner).toEqual(svk);
    expect(tx.kind).toEqual(kind);
    assertSerializationSuccess(tx, signature.instance);
  });

  test('should construct ClaimRewardsTransaction with the Reward claim kind', () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const kind = 'Reward';
    const tx = new ClaimRewardsTransaction(
      signature.instance,
      LOCAL_TEST_NETWORK_ID,
      value,
      svk,
      nonce,
      signature,
      kind
    );

    expect(tx.value).toEqual(value);
    expect(tx.signature.toString()).toEqual(signature.toString());
    expect(tx.nonce).toEqual(nonce);
    expect(tx.owner).toEqual(svk);
    expect(tx.kind).toEqual(kind);
    assertSerializationSuccess(tx, signature.instance);
  });

  test('new - should construct a signature-erased ClaimRewardsTransaction', () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = new SignatureErased();
    const value = 100n;
    const nonce = Static.nonce();
    const tx = ClaimRewardsTransaction.new(LOCAL_TEST_NETWORK_ID, value, svk, nonce, 'CardanoBridge');

    expect(tx.value).toEqual(value);
    expect(tx.signature.toString()).toEqual(signature.toString());
    expect(tx.nonce).toEqual(nonce);
    expect(tx.owner).toEqual(svk);
    expect(tx.kind).toEqual('CardanoBridge');
    assertSerializationSuccess(tx, signature.instance);
  });

  test('new - should construct a signature-enabled ClaimRewardsTransaction', () => {
    const signingKey = sampleSigningKey();
    const svk = signatureVerifyingKey(signingKey);
    const value = 100n;
    const nonce = Static.nonce();
    const kind = 'CardanoBridge';

    // create a signature-erased tx
    const tx = ClaimRewardsTransaction.new(LOCAL_TEST_NETWORK_ID, value, svk, nonce, kind);

    // sign and add a signature
    const signature = signData(signingKey, tx.dataToSign);
    const signedTx = tx.addSignature(signature);

    // create a signature-enabled tx from the signed tx
    const signatureEnabledTx = new ClaimRewardsTransaction(
      signedTx.signature.instance,
      LOCAL_TEST_NETWORK_ID,
      value,
      svk,
      nonce,
      signedTx.signature,
      kind
    );

    // validate they are equal
    expect(signatureEnabledTx.toString(true)).toEqual(signedTx.toString(true));
    assertSerializationSuccess(signatureEnabledTx, signatureEnabledTx.signature.instance);
  });

  test('addSignature - should sign and insert a signature', () => {
    const signingKey = sampleSigningKey();
    const svk = signatureVerifyingKey(signingKey);
    const tx = ClaimRewardsTransaction.new(LOCAL_TEST_NETWORK_ID, 100n, svk, Static.nonce(), 'CardanoBridge');
    expect(tx.signature.toString()).toEqual(new SignatureErased().toString());

    const signature = signData(signingKey, tx.dataToSign);
    const signedTx = tx.addSignature(signature);
    expect(signedTx.signature.toString()).toEqual(new SignatureEnabled(signature).toString());
    assertSerializationSuccess(signedTx, signedTx.signature.instance);
  });

  test('should apply the ClaimRewardsTransaction correctly', () => {
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

  describe('ECDSA signature kind', () => {
    /**
     * Test that the owner getter round-trips an ECDSA verifying key.
     *
     * @given A ClaimRewardsTransaction constructed with an ECDSA owner
     * @when Reading back via the owner getter
     * @then The returned verifying key must equal the input and carry the
     *   'ecdsa' tag
     */
    test('owner getter round-trips an ECDSA verifying key through the constructor', () => {
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const tx = new ClaimRewardsTransaction(
        SignatureMarker.signatureErased,
        LOCAL_TEST_NETWORK_ID,
        100n,
        ecdsaVk,
        Static.nonce(),
        new SignatureErased(),
        'Reward'
      );

      expect(tx.owner).toEqual(ecdsaVk);
      expect(tx.owner.tag).toEqual(SignatureKindMarker.ecdsa);
    });

    /**
     * Test that addSignature accepts an ECDSA signature.
     *
     * @given A ClaimRewardsTransaction with an ECDSA owner, and an ECDSA
     *   signature over its dataToSign
     * @when Calling addSignature with the ECDSA signature
     * @then The resulting tx should report the ECDSA signature via its
     *   getter, with the owner tag still 'ecdsa'
     */
    test('addSignature attaches an ECDSA signature with matching tag', () => {
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const tx = ClaimRewardsTransaction.new(LOCAL_TEST_NETWORK_ID, 100n, ecdsaVk, Static.nonce(), 'Reward');

      const sig = signData(ecdsaSk, tx.dataToSign);
      const signed = tx.addSignature(sig);

      expect(signed.signature.toString()).toEqual(new SignatureEnabled(sig).toString());
      expect(signed.owner.tag).toEqual(SignatureKindMarker.ecdsa);
    });

    /**
     * Test v2 wire-format round-trip for an ECDSA-signed claim.
     *
     * @given A signed ClaimRewardsTransaction with an ECDSA owner
     * @when Serialising and deserialising the transaction
     * @then The deserialised value must match the original via toString
     */
    test('serialization v2 round-trip stable for an ECDSA-owned signed reward', () => {
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const tx = ClaimRewardsTransaction.new(LOCAL_TEST_NETWORK_ID, 100n, ecdsaVk, Static.nonce(), 'CardanoBridge');
      const sig = signData(ecdsaSk, tx.dataToSign);
      const signed = tx.addSignature(sig);

      assertSerializationSuccess(signed, signed.signature.instance);
    });

    /**
     * End-to-end happy path: ECDSA-keyed account claims Night rewards.
     *
     * @given A ledger with NIGHT distributed to an address derived from an
     *   ECDSA verifying key, and a ClaimRewardsTransaction signed by the
     *   matching ECDSA secret key (verifySignatures strictness enabled)
     * @when Applying the transaction
     * @then The ledger must succeed and mint a UTXO of the expected value
     *   owned by the ECDSA-derived address
     */
    test('apply an ECDSA-signed reward claim', () => {
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const ecdsaAddress = addressFromKey(ecdsaVk);

      const state = TestState.new();
      state.distributeNight(ecdsaAddress, INITIAL_NIGHT_AMOUNT, state.time);

      const rewards = ClaimRewardsTransaction.new(
        LOCAL_TEST_NETWORK_ID,
        INITIAL_NIGHT_AMOUNT,
        ecdsaVk,
        Static.nonce(),
        'Reward'
      );
      const sig = signData(ecdsaSk, rewards.dataToSign);
      const signed = rewards.addSignature(sig);

      const tx = Transaction.fromRewards(signed);
      const strictness = new WellFormedStrictness();
      strictness.verifySignatures = true;
      state.assertApply(tx, strictness);

      const utxo = Array.from(state.ledger.utxo.utxos).find((u) => u.owner === ecdsaAddress);

      expect(utxo?.value).toEqual(INITIAL_NIGHT_AMOUNT);
    });

    /**
     * Adversarial path: a ClaimRewardsTransaction with an ECDSA-tagged
     * owner is signed by a Schnorr key (wrong algorithm). The ledger's
     * signature verifier must reject.
     *
     * @given A ClaimRewardsTransaction with an ECDSA owner, signed by a
     *   schnorr signing key (verifySignatures strictness enabled)
     * @when Calling wellFormed
     * @then It should throw 'signature verification failed for supplied intent'
     */
    test('rejects ECDSA-owned reward signed with a Schnorr key', () => {
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const ecdsaAddress = addressFromKey(ecdsaVk);

      const state = TestState.new();
      state.distributeNight(ecdsaAddress, INITIAL_NIGHT_AMOUNT, state.time);

      const rewards = ClaimRewardsTransaction.new(
        LOCAL_TEST_NETWORK_ID,
        INITIAL_NIGHT_AMOUNT,
        ecdsaVk,
        Static.nonce(),
        'Reward'
      );

      const schnorrSk = sampleSigningKey(SignatureKindMarker.schnorr);
      const wrongSig = signData(schnorrSk, rewards.dataToSign);
      const signed = rewards.addSignature(wrongSig);

      const tx = Transaction.fromRewards(signed);
      const strictness = new WellFormedStrictness();
      strictness.verifySignatures = true;

      expect(() => tx.wellFormed(state.ledger, strictness, state.time)).toThrow(
        'signature verification failed for supplied intent'
      );
    });
  });
});
