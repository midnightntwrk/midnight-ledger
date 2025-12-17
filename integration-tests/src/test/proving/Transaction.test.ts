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

import { LedgerParameters, shieldedToken, Transaction } from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { type ShieldedTokenType, Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess, mapFindByKey } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - Transaction [@slow][@proving]', () => {
  /**
   * Test creating transaction with empty guaranteed and fallible unproven outputs.
   *
   * @given Empty guaranteed and fallible transaction parts
   * @when Creating and proving transaction
   * @then Should create valid transaction with undefined inputs, valid hash, positive fees, and no identifiers
   */
  test('should create transaction with empty guaranteed and fallible unproven outputs', async () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined);
    const transaction = await prove(unprovenTransaction);

    expect(transaction.guaranteedOffer?.inputs).toBeUndefined();
    expect(transaction.fees(LedgerParameters.initialParameters())).toBeGreaterThan(0n);
    expect(transaction.identifiers().length).toEqual(0);
    expect(transaction.rewards).toBeUndefined();
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  /**
   * Test creating transaction without fallible unproven outputs.
   *
   * @given Transaction with only guaranteed offer from output
   * @when Creating and proving transaction
   * @then Should have 0 inputs, 1 output, and 1 delta in guaranteed offer
   */
  test('should create transaction without fallible unproven outputs', async () => {
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
    const transaction = await prove(unprovenTransaction);

    expect(transaction.guaranteedOffer?.inputs?.length).toEqual(0);
    expect(transaction.guaranteedOffer?.outputs?.length).toEqual(1);
    expect(transaction.guaranteedOffer?.deltas.size).toEqual(1);
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  /**
   * Test proof erasure functionality.
   *
   * @given Transaction with guaranteed and fallible offers
   * @when Erasing proofs from proven transaction
   * @then Should match StandardTransaction pattern and equal original unproven transaction
   */
  test('should erase proofs correctly', async () => {
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput(1)
    );
    const transaction = await prove(unprovenTransaction);
    const proofErasedTransaction = transaction.eraseProofs();

    expect(proofErasedTransaction.toString()).toMatch(/StandardTransaction .*/);
    expect(proofErasedTransaction.toString()).toEqual(unprovenTransaction.toString());
    expect(proofErasedTransaction.identifiers()).toHaveLength(2);
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  /**
   * Test creating transaction with both guaranteed and fallible unproven outputs.
   *
   * @given Transaction with both guaranteed and fallible offers
   * @when Creating and proving transaction
   * @then Should have 2 identifiers and successful serialization
   */
  test('should create transaction with guaranteed and fallible unproven outputs', async () => {
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput(1)
    );
    const transaction = await prove(unprovenTransaction);

    expect(transaction.identifiers()).toHaveLength(2);
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  /**
   * Test creating unproven transaction with empty guaranteed, fallible, and contract calls.
   *
   * @given Transaction with all undefined parts
   * @when Creating and proving transaction
   * @then Should have all offers and intents undefined
   */
  test('should create unproven transaction with empty guaranteed, fallible, and contract calls', async () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, undefined);
    const transaction = await prove(unprovenTransaction);

    expect(transaction.fallibleOffer?.get(1)?.outputs).toBeUndefined();
    expect(transaction.fallibleOffer?.get(1)?.inputs).toBeUndefined();
    expect(transaction.guaranteedOffer?.outputs).toBeUndefined();
    expect(transaction.guaranteedOffer?.inputs).toBeUndefined();
    expect(transaction.intents).toBeUndefined();
    expect(transaction.rewards).toBeUndefined();
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  /**
   * Test creating unproven transaction with non-empty guaranteed, fallible, and contract calls.
   *
   * @given Transaction with guaranteed, fallible, and contract call parts
   * @when Creating and proving transaction
   * @then Should have correct outputs, inputs, intents size, and actions count
   */
  test('should create unproven transaction with non-empty guaranteed, fallible, and contract calls', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());

    expect(transaction.fallibleOffer?.get(1)?.outputs).toHaveLength(1);
    expect(transaction.fallibleOffer?.get(1)?.inputs).toHaveLength(0);
    expect(transaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(transaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(transaction.intents?.size).toEqual(1);
    expect(transaction.intents?.get(1)?.actions.length).toEqual(2);
    expect(transaction.rewards).toBeUndefined();
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  /**
   * Test transaction merging functionality.
   *
   * @given Two proven transactions with offers and intents
   * @when Merging the transactions
   * @then Should combine offers correctly with expected counts and negative deltas
   */
  test('should merge transactions correctly', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const unprovenTransaction2 = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput(1)
    );
    const transaction2 = await prove(unprovenTransaction2);
    const mergedTx = transaction.merge(transaction2);

    expect(mergedTx.guaranteedOffer?.inputs.length).toEqual(0);
    expect(mergedTx.guaranteedOffer?.outputs.length).toEqual(2);
    expect(mergedTx.guaranteedOffer?.transients.length).toEqual(0);
    expect(mergedTx.guaranteedOffer?.deltas.get((shieldedToken() as ShieldedTokenType).raw)).toEqual(-248n);
    expect(mergedTx.fallibleOffer?.get(1)?.inputs.length).toEqual(0);
    expect(mergedTx.fallibleOffer?.get(1)?.outputs.length).toEqual(2);
    expect(mergedTx.fallibleOffer?.get(1)?.transients.length).toEqual(0);
    expect(mergedTx.fallibleOffer?.get(1)?.deltas.get((shieldedToken() as ShieldedTokenType).raw)).toEqual(-248n);
    expect(mergedTx.intents?.size).toEqual(1);
    expect(mergedTx.intents?.get(1)?.actions.length).toEqual(2);
    expect(mergedTx.rewards).toBeUndefined();
    assertSerializationSuccess(mergedTx, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  /**
   * Test transaction merge collision prevention.
   *
   * @given A proven transaction
   * @when Attempting to merge transaction with itself
   * @then Should throw error about segment ID collision
   */
  test('should prevent merging transaction to itself', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    expect(() => transaction.merge(transaction)).toThrow('key (segment_id) collision during intents merge: 1');
  });

  /**
   * Test transaction serialization and deserialization.
   *
   * @given A proven transaction with guaranteed, fallible, and contract calls
   * @when Serializing and then deserializing the transaction
   * @then Should maintain identical string representation
   */
  test('should serialize and deserialize correctly', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());

    const serialized = transaction.serialize();
    expect(Transaction.deserialize('signature', 'proof', 'pre-binding', serialized).toString()).toEqual(
      transaction.toString()
    );
  });

  /**
   * Test transaction imbalances calculation.
   *
   * @given A proven transaction with guaranteed and fallible offers
   * @when Calculating imbalances with different fees and modes
   * @then Should return consistent negative values and proper fee adjustments
   */
  test('should calculate imbalances correctly', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const imbalances = transaction.imbalances(0, 10n);
    const imbalancesZeroFees = transaction.imbalances(1);
    const imbalancesFallible = transaction.imbalances(1, 10n);
    const imbalancesFallibleZeroFees = transaction.imbalances(1);

    expect(mapFindByKey(imbalances, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalancesZeroFees, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalancesFallible, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalancesFallibleZeroFees, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalances, { tag: 'dust' })).toEqual(-10n);
    expect(mapFindByKey(imbalancesZeroFees, { tag: 'dust' })).toBeUndefined();
  });

  /**
   * Test proving transaction from unproven input.
   *
   * @given Two different unproven transactions
   * @when Proving the second transaction
   * @then Should return proven transaction with same guaranteed offer length and no fallible offer or intents
   * @note BUG: PM-13900
   */
  test('should return proven transaction from unproven input', async () => {
    const unproven = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const unproven2 = Static.unprovenTransactionGuaranteed();
    const transaction = await prove(unproven2);

    expect(transaction.guaranteedOffer?.outputs.length).toEqual(unproven.guaranteedOffer?.outputs.length);
    expect(transaction.fallibleOffer).toBeUndefined();
    expect(transaction.intents).toBeUndefined();
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });
});
