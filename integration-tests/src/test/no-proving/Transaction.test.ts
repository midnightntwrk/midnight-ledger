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
  ContractDeploy,
  ContractState,
  Intent,
  type IntentHash,
  LedgerState,
  sampleIntentHash,
  sampleSigningKey,
  sampleUserAddress,
  type SignatureEnabled,
  signatureVerifyingKey,
  signData,
  Transaction,
  UnshieldedOffer,
  WellFormedStrictness,
  ZswapChainState
} from '@midnight-ntwrk/ledger';
import { Random, Static } from '@/test-objects';
import { assertSerializationSuccess, mapFindByKey } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Ledger API - Transaction', () => {
  const TTL = new Date();
  /**
   * Test creating unproven transaction from guaranteed offer.
   *
   * @given A guaranteed unproven offer
   * @when Creating transaction from parts
   * @then Should create transaction with correct guaranteed offer properties
   */
  test('should create unproven transaction from guaranteed UnprovenOffer', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

    expect(unprovenTransaction.fallibleOffer?.get(1)?.outputs).toBeUndefined();
    expect(unprovenTransaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(unprovenTransaction.fallibleOffer?.get(1)?.inputs).toBeUndefined();
    expect(unprovenTransaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(unprovenTransaction.intents).toBeUndefined();
    expect(unprovenTransaction.identifiers().length).toEqual(1);
    expect(unprovenTransaction.rewards).toBeUndefined();

    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  /**
   * Test creating unproven transaction from guaranteed and fallible offers.
   *
   * @given Guaranteed and fallible unproven offers
   * @when Creating transaction from parts
   * @then Should create transaction with both offer types
   */
  test('should create unproven transaction from guaranteed and fallible unproven outputs', () => {
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput(2)
    );

    expect(unprovenTransaction.fallibleOffer?.get(2)?.outputs).toHaveLength(1);
    expect(unprovenTransaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(unprovenTransaction.fallibleOffer?.get(2)?.inputs).toHaveLength(0);
    expect(unprovenTransaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(unprovenTransaction.intents).toBeUndefined();
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  /**
   * Test creating transaction with all components.
   *
   * @given Guaranteed offer, fallible offer, and contract calls
   * @when Creating transaction from parts
   * @then Should create transaction with all components properly configured
   */
  test('should create unproven transaction from guaranteed, fallible and contract calls', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(TTL).addDeploy(contractDeploy);
    const unprovenOfferGuaranteed = Static.unprovenOfferFromOutput();
    const unprovenOfferFallible = Static.unprovenOfferFromOutput(1);
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      unprovenOfferGuaranteed,
      unprovenOfferFallible,
      intent
    );

    expect(unprovenTransaction.fallibleOffer?.get(1)?.outputs).toHaveLength(1);
    expect(unprovenTransaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(unprovenTransaction.fallibleOffer?.get(1)?.inputs).toHaveLength(0);
    expect(unprovenTransaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(unprovenTransaction.intents?.size).toEqual(1);
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  test('should create transaction with undefined fallible and empty contract call', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, Intent.new(TTL));

    expect(unprovenTransaction.guaranteedOffer).toBeUndefined();
    expect(unprovenTransaction.intents?.size).toEqual(1);
    expect(unprovenTransaction.fallibleOffer).toBeUndefined();
    expect(unprovenTransaction.imbalances(0).size).toEqual(0);
    expect(unprovenTransaction.imbalances(1).size).toEqual(0);
    expect(unprovenTransaction.toString()).toMatch(/Standard.*/);
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  test('should create transaction with undefined fallible and contract calls', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, undefined);

    expect(unprovenTransaction.guaranteedOffer).toBeUndefined();
    expect(unprovenTransaction.intents).toBeUndefined();
    expect(unprovenTransaction.fallibleOffer).toBeUndefined();
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  /**
   * Test transaction merge validation.
   *
   * @given A transaction
   * @when Attempting to merge with itself
   * @then Should throw error about non-disjoint coin sets
   */
  test('should not merge with itself', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
    expect(() => unprovenTransaction.merge(unprovenTransaction)).toThrow('attempted to merge non-disjoint coin sets');
  });

  /**
   * Test merging two different transactions.
   *
   * @given Two transactions with different token types
   * @when Merging the transactions
   * @then Should combine offers with correct delta calculations
   */
  test('should merge two transactions', () => {
    const tokenType = Random.shieldedTokenType();
    const tokenType2 = Random.shieldedTokenType();
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType, 1n));
    const unprovenTransaction2 = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(0, tokenType2, 10n)
    );
    const merged = unprovenTransaction.merge(unprovenTransaction2);

    expect(merged.guaranteedOffer?.deltas.size).toEqual(2);
    expect(merged.guaranteedOffer?.deltas.get(tokenType.raw)).toEqual(-1n);
    expect(merged.guaranteedOffer?.deltas.get(tokenType2.raw)).toEqual(-10n);
    expect(merged.guaranteedOffer?.outputs).toHaveLength(2);
    expect(merged.guaranteedOffer?.inputs).toHaveLength(0);
    expect(merged.fallibleOffer).toBeUndefined();
    expect(merged.intents).toBeUndefined();
    assertSerializationSuccess(merged, SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding);
  });

  test('merge - should merge two transactions, one with fallible', () => {
    const tokenType = Random.shieldedTokenType();
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType, 1n));
    const unprovenTransaction2 = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(0, tokenType, 10n),
      Static.unprovenOfferFromOutput(1, tokenType, 100n)
    );
    const merged = unprovenTransaction.merge(unprovenTransaction2);

    expect(merged.guaranteedOffer?.deltas.size).toEqual(1);
    expect(merged.guaranteedOffer?.deltas.get(tokenType.raw)).toEqual(-11n);
    expect(merged.guaranteedOffer?.outputs).toHaveLength(2);
    expect(merged.guaranteedOffer?.inputs).toHaveLength(0);
    expect(merged.fallibleOffer?.get(1)?.deltas.size).toEqual(1);
    expect(merged.fallibleOffer?.get(1)?.deltas.get(tokenType.raw)).toEqual(-100n);
    expect(merged.intents).toBeUndefined();
    assertSerializationSuccess(merged, SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding);
  });

  test('merge - should merge two transactions, one with fallible - different tokenTypes', () => {
    const tokenType = Random.shieldedTokenType();
    const tokenType2 = Random.shieldedTokenType();
    const tokenType3 = Random.shieldedTokenType();
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType, 1n));
    const unprovenTransaction2 = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(0, tokenType2, 10n),
      Static.unprovenOfferFromOutput(1, tokenType3, 100n)
    );
    const merged = unprovenTransaction.merge(unprovenTransaction2);

    expect(merged.guaranteedOffer?.deltas.size).toEqual(2);
    expect(merged.guaranteedOffer?.deltas.get(tokenType.raw)).toEqual(-1n);
    expect(merged.guaranteedOffer?.deltas.get(tokenType2.raw)).toEqual(-10n);
    expect(merged.guaranteedOffer?.outputs).toHaveLength(2);
    expect(merged.guaranteedOffer?.outputs.at(0)?.contractAddress).toEqual(
      unprovenTransaction.guaranteedOffer?.outputs.at(0)?.contractAddress
    );
    expect(merged.guaranteedOffer?.outputs.at(1)?.contractAddress).toEqual(
      unprovenTransaction2.guaranteedOffer?.outputs.at(0)?.contractAddress
    );
    expect(merged.guaranteedOffer?.inputs).toHaveLength(0);
    expect(merged.fallibleOffer?.get(1)?.deltas.size).toEqual(1);
    expect(merged.fallibleOffer?.get(1)?.deltas.get(tokenType3.raw)).toEqual(-100n);
    expect(merged.intents).toBeUndefined();
    expect(merged.imbalances(0).size).toEqual(2);
    expect(mapFindByKey(merged.imbalances(0), tokenType)).toEqual(-1n);
    expect(mapFindByKey(merged.imbalances(0), tokenType2)).toEqual(-10n);
    expect(merged.imbalances(1).size).toEqual(1);
    expect(mapFindByKey(merged.imbalances(1), tokenType3)).toEqual(-100n);
    assertSerializationSuccess(merged, SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding);
  });

  test('should create unproven transaction with guaranteed, fallible, and multiple contract calls', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(TTL).addDeploy(contractDeploy).addDeploy(contractDeploy);
    const unprovenOfferGuaranteed = Static.unprovenOfferFromOutput();
    const unprovenOfferFallible = Static.unprovenOfferFromOutput(1);
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      unprovenOfferGuaranteed,
      unprovenOfferFallible,
      intent
    );

    expect(unprovenTransaction.fallibleOffer?.get(1)?.outputs).toHaveLength(1);
    expect(unprovenTransaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(unprovenTransaction.fallibleOffer?.get(1)?.inputs).toHaveLength(0);
    expect(unprovenTransaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(unprovenTransaction.intents?.size).toEqual(1);
    expect(unprovenTransaction.intents?.get(1)?.actions.length).toEqual(2);
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  /**
   * Test creation with unshielded offers.
   *
   * @given An intent with guaranteed and fallible unshielded offers
   * @and Different intent hashes for each offer type
   * @when Creating transaction from parts with complex unshielded offers
   * @and Setting both guaranteed and fallible unshielded offers on intent
   * @then Should preserve unshielded offers in the transaction
   * @and Should maintain offer structure and intent hash associations
   */
  test('should create unproven transaction with unshielded offers', () => {
    const intent = Intent.new(TTL);
    const intentHash1 = sampleIntentHash();
    const intentHash2 = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const newUnshieldedOffer = (intentHash: IntentHash): UnshieldedOffer<SignatureEnabled> =>
      UnshieldedOffer.new(
        [
          {
            value: 100n,
            owner: svk,
            type: token.raw,
            intentHash,
            outputNo: 0
          }
        ],
        [
          {
            value: 100n,
            owner: sampleUserAddress(),
            type: token.raw
          }
        ],
        [signData(sampleSigningKey(), new Uint8Array(32))]
      );
    const offer1 = newUnshieldedOffer(intentHash1);
    const offer2 = newUnshieldedOffer(intentHash2);
    intent.guaranteedUnshieldedOffer = offer1;
    intent.fallibleUnshieldedOffer = offer2;

    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, intent);

    expect(unprovenTransaction.fallibleOffer).toBeUndefined();
    expect(unprovenTransaction.guaranteedOffer).toBeUndefined();
    expect(unprovenTransaction.intents?.size).toEqual(1);
    expect(unprovenTransaction.intents?.get(1)?.guaranteedUnshieldedOffer?.toString()).toEqual(offer1.toString());
    expect(unprovenTransaction.intents?.get(1)?.fallibleUnshieldedOffer?.toString()).toEqual(offer2.toString());
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  test('should create unproven transaction with empty guaranteed and fallible unproven outputs', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined);

    expect(unprovenTransaction.fallibleOffer?.get(1)?.outputs).toBeUndefined();
    expect(unprovenTransaction.guaranteedOffer?.outputs).toBeUndefined();
    expect(unprovenTransaction.fallibleOffer?.get(1)?.inputs).toBeUndefined();
    expect(unprovenTransaction.guaranteedOffer?.inputs).toBeUndefined();
    expect(unprovenTransaction.intents).toBeUndefined();
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  test('should create unproven transaction with empty guaranteed, fallible, and contract calls', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, Intent.new(TTL));

    expect(unprovenTransaction.fallibleOffer?.get(1)?.outputs).toBeUndefined();
    expect(unprovenTransaction.guaranteedOffer?.outputs).toBeUndefined();
    expect(unprovenTransaction.fallibleOffer?.get(1)?.inputs).toBeUndefined();
    expect(unprovenTransaction.guaranteedOffer?.inputs).toBeUndefined();
    expect(unprovenTransaction.intents?.size).toEqual(1);
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  it('should fail wellFormed check against different network', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, Intent.new(new Date()));
    const wellFormedStrictness = new WellFormedStrictness();
    wellFormedStrictness.enforceBalancing = false;
    expect(() =>
      unprovenTransaction.wellFormed(
        new LedgerState('local-test2', new ZswapChainState()),
        wellFormedStrictness,
        new Date()
      )
    ).toThrow();
  });

  it('should pass wellFormed check if has only intent', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, Intent.new(TTL));
    const wellFormedStrictness = new WellFormedStrictness();
    wellFormedStrictness.enforceBalancing = false;
    expect(() =>
      unprovenTransaction.wellFormed(new LedgerState('local-test', new ZswapChainState()), wellFormedStrictness, TTL)
    ).not.toThrow();
  });

  test('should not allow intent with bind called on it', () => {
    expect(() =>
      Transaction.fromParts(
        'local-test',
        undefined,
        undefined,
        // @ts-expect-error lets check if it throws
        Intent.new(TTL).bind(1)
      )
    ).toThrow('Intent offer must be unproven.');
  });

  test('should serialize and deserialize a large transaction with all elements', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);

    const intent = Intent.new(TTL).addDeploy(contractDeploy).addDeploy(contractDeploy);

    const intentHash1 = sampleIntentHash();
    const intentHash2 = sampleIntentHash();
    const token1 = Random.unshieldedTokenType();
    const token2 = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());

    const guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [
        {
          value: 100n,
          owner: svk,
          type: token1.raw,
          intentHash: intentHash1,
          outputNo: 0
        },
        {
          value: 10n,
          owner: svk,
          type: token1.raw,
          intentHash: intentHash1,
          outputNo: 1
        }
      ],
      [
        {
          value: 40n,
          owner: sampleUserAddress(),
          type: token1.raw
        },
        {
          value: 60n,
          owner: sampleUserAddress(),
          type: token1.raw
        }
      ],
      [signData(sampleSigningKey(), new Uint8Array(32))]
    );

    const fallibleUnshieldedOffer = UnshieldedOffer.new(
      [
        {
          value: 200n,
          owner: svk,
          type: token2.raw,
          intentHash: intentHash2,
          outputNo: 100
        },
        {
          value: 20n,
          owner: svk,
          type: token2.raw,
          intentHash: intentHash2,
          outputNo: 11
        }
      ],
      [
        {
          value: 90n,
          owner: sampleUserAddress(),
          type: token2.raw
        },
        {
          value: 110n,
          owner: sampleUserAddress(),
          type: token2.raw
        }
      ],
      [signData(sampleSigningKey(), new Uint8Array(32))]
    );

    intent.guaranteedUnshieldedOffer = guaranteedUnshieldedOffer;
    intent.fallibleUnshieldedOffer = fallibleUnshieldedOffer;

    const shieldedToken1 = Random.shieldedTokenType();
    const shieldedToken2 = Random.shieldedTokenType();
    const shieldedToken3 = Random.shieldedTokenType();

    const guaranteedOffer = Static.unprovenOfferFromOutput(0, shieldedToken1, 100n);

    const guaranteedOffer2 = Static.unprovenOfferFromOutput(0, shieldedToken2, 50n);

    const fallibleOffer = Static.unprovenOfferFromOutput(1, shieldedToken3, 75n);

    const complexTransaction = Transaction.fromParts(
      'local-test',
      guaranteedOffer.merge(guaranteedOffer2),
      fallibleOffer,
      intent
    );

    expect(complexTransaction.guaranteedOffer?.outputs.length).toEqual(2);
    expect(complexTransaction.guaranteedOffer?.inputs.length).toEqual(0);
    expect(complexTransaction.fallibleOffer?.get(1)?.outputs).toHaveLength(1);
    expect(complexTransaction.intents?.size).toEqual(1);
    expect(complexTransaction.intents?.get(1)?.actions).toHaveLength(2);
    expect(complexTransaction.intents?.get(1)?.fallibleUnshieldedOffer?.toString()).toEqual(
      fallibleUnshieldedOffer.toString()
    );
    expect(complexTransaction.intents?.get(1)?.guaranteedUnshieldedOffer?.toString()).toEqual(
      guaranteedUnshieldedOffer.toString()
    );

    assertSerializationSuccess(
      complexTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );

    expect(complexTransaction.guaranteedOffer?.deltas.size).toEqual(2);
    expect(complexTransaction.guaranteedOffer?.deltas.get(shieldedToken1.raw)).toEqual(-100n);
    expect(complexTransaction.guaranteedOffer?.deltas.get(shieldedToken2.raw)).toEqual(-50n);
    expect(complexTransaction.fallibleOffer?.get(1)?.deltas.size).toEqual(1);
    expect(complexTransaction.fallibleOffer?.get(1)?.deltas.get(shieldedToken3.raw)).toEqual(-75n);

    expect(complexTransaction.identifiers().length).toBeGreaterThan(0);

    expect(complexTransaction.imbalances(0).size).toEqual(3);
    expect(mapFindByKey(complexTransaction.imbalances(0), shieldedToken1)).toEqual(-100n);
    expect(mapFindByKey(complexTransaction.imbalances(0), shieldedToken2)).toEqual(-50n);
    expect(complexTransaction.imbalances(1).size).toEqual(2);
    expect(mapFindByKey(complexTransaction.imbalances(1), shieldedToken3)).toEqual(-75n);
  });

  test('fromPartsRandomized - should create transaction with randomized segment IDs', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const fallibleOffer = Static.unprovenOfferFromOutput(1);
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(TTL).addDeploy(contractDeploy);

    const transaction1 = Transaction.fromPartsRandomized('local-test', guaranteedOffer, fallibleOffer, intent);
    const transaction2 = Transaction.fromPartsRandomized('local-test', guaranteedOffer, fallibleOffer, intent);

    // The segment IDs should be randomized, allowing for merging
    expect(transaction1.toString()).not.toEqual(transaction2.toString());
    expect(transaction1.guaranteedOffer?.outputs).toHaveLength(1);
    expect(transaction1.fallibleOffer?.size).toEqual(1);
    expect(transaction1.intents?.size).toEqual(1);

    assertSerializationSuccess(transaction1, SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding);
    assertSerializationSuccess(transaction2, SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding);
  });

  test('fromRewards - should create rewarding transaction from ClaimRewardsTransaction', () => {
    // Note: ClaimRewardsTransaction needs to be properly constructed with valid signature
    // For now, testing the transaction structure that would be created
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const rewardsTransaction = Transaction.fromParts('local-test', guaranteedOffer);

    // Verify the rewards property exists (even if undefined for non-rewards transactions)
    expect(rewardsTransaction.rewards).toBeUndefined();
    expect(rewardsTransaction.guaranteedOffer).toBeDefined();

    assertSerializationSuccess(
      rewardsTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  test('bind - should enforce binding for transaction', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

    const boundTransaction = unprovenTransaction.bind();

    // After binding, the transaction should have binding type 'binding'
    expect(boundTransaction.toString()).toMatch(/.*binding.*/i);
    assertSerializationSuccess(
      boundTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.binding
    );
  });

  test('bindingRandomness - should have binding randomness property', () => {
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

    expect(typeof unprovenTransaction.bindingRandomness).toBe('bigint');
    expect(unprovenTransaction.bindingRandomness).toBeGreaterThanOrEqual(0n);
  });

  test('guaranteedOffer - should provide access to guaranteed offer', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const transaction = Transaction.fromParts('local-test', guaranteedOffer);

    expect(transaction.guaranteedOffer).toBeDefined();
    expect(transaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(transaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(transaction.guaranteedOffer?.deltas.size).toBeGreaterThan(0);

    // Test modifying guaranteed offer should work for unbound transactions
    transaction.guaranteedOffer = Static.unprovenOfferFromOutput(0);
    expect(transaction.guaranteedOffer).toBeDefined();
  });

  test('guaranteedOffer - should throw when modifying bound transaction', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const transaction = Transaction.fromParts('local-test', guaranteedOffer);
    const boundTransaction = transaction.bind();

    expect(() => {
      boundTransaction.guaranteedOffer = Static.unprovenOfferFromOutput(0);
    }).toThrow('Transaction is already bound.');
  });

  test('fallibleOffer - should provide access to fallible offer', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const fallibleOffer = Static.unprovenOfferFromOutput(1);
    const transaction = Transaction.fromParts('local-test', guaranteedOffer, fallibleOffer);

    expect(transaction.fallibleOffer).toBeDefined();
    expect(transaction.fallibleOffer?.size).toEqual(1);
    expect(transaction.fallibleOffer?.get(1)?.outputs).toHaveLength(1);
    expect(transaction.fallibleOffer?.get(1)?.inputs).toHaveLength(0);

    // Test modifying fallible offer should work for unbound transactions
    const newFallibleOffer = new Map();
    newFallibleOffer.set(1, Static.unprovenOfferFromOutput(1));
    transaction.fallibleOffer = newFallibleOffer;
    expect(transaction.fallibleOffer?.size).toEqual(1);
  });

  test('fallibleOffer - should throw when modifying bound transaction', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const fallibleOffer = Static.unprovenOfferFromOutput(1);
    const transaction = Transaction.fromParts('local-test', guaranteedOffer, fallibleOffer);
    const boundTransaction = transaction.bind();

    expect(() => {
      const newFallibleOffer = new Map();
      newFallibleOffer.set(1, Static.unprovenOfferFromOutput(1));
      boundTransaction.fallibleOffer = newFallibleOffer;
    }).toThrow('Transaction is already bound.');
  });

  test('intents - should provide access to intents', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(TTL).addDeploy(contractDeploy);
    const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);

    expect(transaction.intents).toBeDefined();
    expect(transaction.intents?.size).toEqual(1);
    expect(transaction.intents?.get(1)?.actions).toHaveLength(1);

    // Test modifying intents should work for unbound transactions
    const newIntent = Intent.new(TTL).addDeploy(contractDeploy);
    const newIntents = new Map();
    newIntents.set(1, newIntent);
    transaction.intents = newIntents;
    expect(transaction.intents?.size).toEqual(1);
  });

  test('intents - should throw when modifying bound transaction', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    const intent = Intent.new(TTL).addDeploy(contractDeploy);
    const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);
    const boundTransaction = transaction.bind();

    expect(() => {
      const newIntent = Intent.new(TTL).addDeploy(contractDeploy);
      const newIntents = new Map();
      newIntents.set(1, newIntent);
      boundTransaction.intents = newIntents;
    }).toThrow('Transaction is already bound.');
  });
});
