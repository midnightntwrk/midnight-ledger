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
  type AlignedValue,
  bigIntToValue,
  type Bindingish,
  ChargedState,
  ClaimRewardsTransaction,
  communicationCommitmentRandomness,
  type ContractAddress,
  ContractCall,
  ContractDeploy,
  ContractMaintenanceAuthority,
  ContractOperation,
  ContractState,
  createShieldedCoinInfo,
  encodeContractAddress,
  encodeShieldedCoinInfo,
  Intent,
  type IntentHash,
  LedgerState,
  PrePartitionContractCall,
  PreTranscript,
  type Proofish,
  QueryContext,
  runtimeCoinCommitment,
  sampleIntentHash,
  sampleSigningKey,
  sampleUserAddress,
  type SegmentSpecifier,
  type ShieldedCoinInfo,
  type SignatureEnabled,
  SignatureErased,
  signatureVerifyingKey,
  type Signaturish,
  signData,
  StateValue,
  Transaction,
  UnshieldedOffer,
  WellFormedStrictness,
  ZswapChainState,
  ZswapOffer,
  ZswapOutput
} from '@midnight-ntwrk/ledger';
import {
  INITIAL_NIGHT_AMOUNT,
  LOCAL_TEST_NETWORK_ID,
  Random,
  type ShieldedTokenType,
  Static,
  TestResource
} from '@/test-objects';
import { assertSerializationSuccess, mapFindByKey, plus1Hour, testIntents } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import { TestState } from '@/test/utils/TestState';
import { ATOM_BYTES_1, ATOM_BYTES_16, ATOM_BYTES_32, EMPTY_VALUE, ONE_VALUE } from '@/test/utils/value-alignment';
import {
  cellRead,
  cellWrite,
  getKey,
  kernelClaimZswapCoinReceive,
  kernelSelf,
  programWithResults
} from '@/test/utils/onchain-runtime-program-fragments';

describe('Ledger API - Transaction', () => {
  const STORE = 'store';
  const FIRST_SEGMENT_SPECIFIER: SegmentSpecifier = { tag: 'first' };
  const SPECIFIC_VALUE_SPECIFIER: SegmentSpecifier = { tag: 'specific', value: 2 };
  const GUARANTEED_ONLY_SPECIFIER: SegmentSpecifier = { tag: 'guaranteedOnly' };
  const TTL = new Date();
  /**
   * Test creating unproven transaction from guaranteed offer.
   *
   * @given A guaranteed unproven offer
   * @when Creating transaction from parts
   * @then Should create transaction with correct guaranteed offer properties
   */
  test('should create unproven transaction from guaranteed UnprovenOffer', () => {
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, Static.unprovenOfferFromOutput());

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
      LOCAL_TEST_NETWORK_ID,
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput()
    );

    expect(unprovenTransaction.fallibleOffer?.get(1)?.outputs).toHaveLength(1);
    expect(unprovenTransaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(unprovenTransaction.fallibleOffer?.get(1)?.inputs).toHaveLength(0);
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
      LOCAL_TEST_NETWORK_ID,
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
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, Intent.new(TTL));

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
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, undefined);

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
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, Static.unprovenOfferFromOutput());
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
    const unprovenTransaction = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      Static.unprovenOfferFromOutput(0, tokenType, 1n)
    );
    const unprovenTransaction2 = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
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
    const unprovenTransaction = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      Static.unprovenOfferFromOutput(0, tokenType, 1n)
    );
    const unprovenTransaction2 = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
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
    const unprovenTransaction = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      Static.unprovenOfferFromOutput(0, tokenType, 1n)
    );
    const unprovenTransaction2 = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
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
      LOCAL_TEST_NETWORK_ID,
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

    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

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
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined);

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
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, Intent.new(TTL));

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
    const unprovenTransaction = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      Intent.new(new Date())
    );
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
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, Intent.new(TTL));
    const wellFormedStrictness = new WellFormedStrictness();
    wellFormedStrictness.enforceBalancing = false;
    expect(() =>
      unprovenTransaction.wellFormed(
        new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState()),
        wellFormedStrictness,
        TTL
      )
    ).not.toThrow();
  });

  test('should not allow intent with bind called on it', () => {
    expect(() =>
      Transaction.fromParts(
        LOCAL_TEST_NETWORK_ID,
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
      LOCAL_TEST_NETWORK_ID,
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

    const transaction1 = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, guaranteedOffer, fallibleOffer, intent);
    const transaction2 = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, guaranteedOffer, fallibleOffer, intent);

    // The segment IDs should be randomized, allowing for merging
    expect(transaction1.toString()).not.toEqual(transaction2.toString());
    expect(transaction1.guaranteedOffer?.outputs).toHaveLength(1);
    expect(transaction1.fallibleOffer?.size).toEqual(1);
    expect(transaction1.intents?.size).toEqual(1);

    assertSerializationSuccess(transaction1, SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding);
    assertSerializationSuccess(transaction2, SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding);
  });

  test('fromRewards - should create rewarding transaction from ClaimRewardsTransaction', () => {
    const claimRewardsTransaction = new ClaimRewardsTransaction(
      SignatureMarker.signatureErased,
      LOCAL_TEST_NETWORK_ID,
      100n,
      signatureVerifyingKey(sampleSigningKey()),
      Random.nonce(),
      new SignatureErased()
    );
    const rewardsTransaction = Transaction.fromRewards(claimRewardsTransaction);

    expect(rewardsTransaction.rewards).toBeDefined();

    assertSerializationSuccess(
      rewardsTransaction,
      SignatureMarker.signatureErased,
      ProofMarker.preProof,
      BindingMarker.binding
    );
  });

  test('bind - should enforce binding for transaction', () => {
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, Static.unprovenOfferFromOutput());

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
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, Static.unprovenOfferFromOutput());

    expect(typeof unprovenTransaction.bindingRandomness).toBe('bigint');
    expect(unprovenTransaction.bindingRandomness).toBeGreaterThanOrEqual(0n);
  });

  test('guaranteedOffer - should provide access to guaranteed offer', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const transaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, guaranteedOffer);

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
    const transaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, guaranteedOffer);
    const boundTransaction = transaction.bind();

    expect(() => {
      boundTransaction.guaranteedOffer = Static.unprovenOfferFromOutput(0);
    }).toThrow('Transaction is already bound.');
  });

  test('fallibleOffer - should provide access to fallible offer', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const fallibleOffer = Static.unprovenOfferFromOutput(1);
    const transaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, guaranteedOffer, fallibleOffer);

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
    const transaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, guaranteedOffer, fallibleOffer);
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
    const transaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

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
    const transaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const boundTransaction = transaction.bind();

    expect(() => {
      const newIntent = Intent.new(TTL).addDeploy(contractDeploy);
      const newIntents = new Map();
      newIntents.set(1, newIntent);
      boundTransaction.intents = newIntents;
    }).toThrow('Transaction is already bound.');
  });

  test('addCalls - for first segment specifier', () => {
    testSpecificSegment(FIRST_SEGMENT_SPECIFIER, 1);
  });

  test('addCalls - for guaranteedOnly segment specifier', () => {
    testSpecificSegment(GUARANTEED_ONLY_SPECIFIER);
  });

  test('addCalls - for random segment specifier', () => {
    const segment: SegmentSpecifier = { tag: 'random' };
    testSpecificSegment(segment);
  });

  test('addCalls - for specific segment specifier with min value', () => {
    const minSpecificValue = 1;
    const segment: SegmentSpecifier = { tag: 'specific', value: minSpecificValue };
    testSpecificSegment(segment, minSpecificValue);
  });

  test('addCalls - for specific segment specifier with max value', () => {
    const maxSpecificValue = 65535;
    const segment: SegmentSpecifier = { tag: 'specific', value: maxSpecificValue };
    testSpecificSegment(segment, maxSpecificValue);
  });

  test('addCalls - throws for specific segment specifier with 0 value', () => {
    const invalidSpecificValue = 0;
    const segment: SegmentSpecifier = { tag: 'specific', value: invalidSpecificValue };
    expect(() => testSpecificSegment(segment)).toThrow('illegal manual specification of segment 0');
  });

  test('addCalls - throws for specific segment specifier when value is above u16', () => {
    const invalidSpecificValue = 65536;
    const segment: SegmentSpecifier = { tag: 'specific', value: invalidSpecificValue };
    expect(() => testSpecificSegment(segment)).toThrow(
      `Error: invalid value: integer \`${invalidSpecificValue}\`, expected u16`
    );
  });

  test('addCalls - works correctly on signature-erased transaction', () => {
    const state = TestState.new();
    const ttl = plus1Hour(state.time);

    let erased = Transaction.fromParts(LOCAL_TEST_NETWORK_ID).eraseSignatures();
    erased = erased.addCalls(FIRST_SEGMENT_SPECIFIER, [], state.ledger.parameters, ttl, [], [], []);
    expect(erased.intents!.size).toBe(1);
  });

  test('addCalls - works correctly on multiple addCalls', () => {
    const state = TestState.new();
    const ttl = plus1Hour(state.time);

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID)
      .addCalls(FIRST_SEGMENT_SPECIFIER, [], state.ledger.parameters, ttl, [], [], [])
      .addCalls(SPECIFIC_VALUE_SPECIFIER, [], state.ledger.parameters, ttl, [], [], []);

    expect(tx.intents!.size).toBe(2);
  });

  test('addCalls - throws on proof-erased transaction', () => {
    const state = TestState.new();
    const ttl = plus1Hour(state.time);

    const erased = Transaction.fromParts(LOCAL_TEST_NETWORK_ID).eraseProofs();

    expect(() => erased.addCalls(FIRST_SEGMENT_SPECIFIER, [], state.ledger.parameters, ttl, [], [], [])).toThrow(
      'Cannot add calls to proof-erased transaction.'
    );
  });

  test('addCalls - throws on mockProve() transaction', () => {
    const state = TestState.new();
    const ttl = plus1Hour(state.time);

    const proved = Transaction.fromParts(LOCAL_TEST_NETWORK_ID).mockProve();

    expect(() => proved.addCalls(FIRST_SEGMENT_SPECIFIER, [], state.ledger.parameters, ttl, [], [], [])).toThrow(
      'Cannot add calls to bound transaction.'
    );
  });

  test('addCalls - throws on bounded transaction', () => {
    const state = TestState.new();
    const ttl = plus1Hour(state.time);

    const bounded = Transaction.fromParts(LOCAL_TEST_NETWORK_ID).bind();

    expect(() => bounded.addCalls(FIRST_SEGMENT_SPECIFIER, [], state.ledger.parameters, ttl, [], [], [])).toThrow(
      'Cannot add calls to bound transaction.'
    );
  });

  test('addCalls - throws on wellFormed when contract-owned output exists but call does not claim receive', () => {
    const { state, addr, encodedAddr, op, token, unbalancedStrictness } = setup();

    const { preCall, zswapOutput } = buildCallAndOutput({
      state,
      addr,
      encodedAddr,
      op,
      token,
      baseKey: 0,
      includeClaimReceive: false
    });

    const ttl = plus1Hour(state.time);

    let tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID);
    tx = tx.addCalls(FIRST_SEGMENT_SPECIFIER, [preCall], state.ledger.parameters, ttl, [], [zswapOutput], []);

    expect(() => tx.wellFormed(state.ledger, unbalancedStrictness, state.time)).toThrow(
      'all contract-associated commitments must be claimed'
    );
  });

  test('addCalls - throws on wellFormed when call claims receive but no contract-owned output exists', () => {
    const { state, addr, encodedAddr, op, token, unbalancedStrictness } = setup();

    const { preCall } = buildCallAndOutput({
      state,
      addr,
      encodedAddr,
      op,
      token,
      baseKey: 0,
      includeClaimReceive: true
    });

    const ttl = plus1Hour(state.time);

    let tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID);
    tx = tx.addCalls(FIRST_SEGMENT_SPECIFIER, [preCall], state.ledger.parameters, ttl, [], [], []);

    expect(() => tx.wellFormed(state.ledger, unbalancedStrictness, state.time)).toThrow(
      'all contract-associated commitments must be claimed by exactly one instance of the same contract in the same segment'
    );
  });

  test('addCalls - two calls + two contract-owned outputs in one apply and land in the same segment', () => {
    const { state, addr, encodedAddr, op, token, unbalancedStrictness, balancedStrictness } = setup();

    const callA = buildCallAndOutput({
      state,
      addr,
      encodedAddr,
      op,
      token,
      baseKey: 0,
      includeClaimReceive: true
    });

    const callB = buildCallAndOutput({
      state,
      addr,
      encodedAddr,
      op,
      token,
      baseKey: 3,
      includeClaimReceive: true
    });

    const ttl = plus1Hour(state.time);

    let tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID);
    tx = tx.addCalls(
      FIRST_SEGMENT_SPECIFIER,
      [callA.preCall, callB.preCall],
      state.ledger.parameters,
      ttl,
      [],
      [callA.zswapOutput, callB.zswapOutput],
      []
    );

    const segA = findSegmentOfCall(tx, addr, STORE);
    expect(segA).toBe(1);

    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    const arr = state.ledger.index(addr)!.data.state.asArray()!;
    expect(arr[2].asCell().value[0]).toEqual(ONE_VALUE);
    expect(arr[5].asCell().value[0]).toEqual(ONE_VALUE);

    expect(arr[0].asCell().value[0]).toEqual(callA.coinCom.value[0]);
    expect(arr[3].asCell().value[0]).toEqual(callB.coinCom.value[0]);
  });

  test('addCalls - sets intent ttl to the provided ttl', () => {
    const { state, addr, encodedAddr, op, token, unbalancedStrictness, balancedStrictness } = setup();

    const { preCall, zswapOutput } = buildCallAndOutput({
      state,
      addr,
      encodedAddr,
      op,
      token,
      baseKey: 0,
      includeClaimReceive: true
    });

    const ttl = plus1Hour(state.time);

    let tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID);
    tx = tx.addCalls(FIRST_SEGMENT_SPECIFIER, [preCall], state.ledger.parameters, ttl, [], [zswapOutput], []);

    const seg = findSegmentOfCall(tx, addr, STORE);
    const intent = tx.intents!.get(seg)!;

    expect(intent.ttl.getTime()).toBe(ttl.getTime());

    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);
  });

  test('addCalls - empty calls with a contract-owned zswap output should fail wellFormed', () => {
    const state = TestState.new();
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, 5_000_000_000n);
    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;

    const op = new ContractOperation();
    op.verifierKey = TestResource.operationVerifierKey();
    const { addr } = deployContract({
      state,
      op,
      unbalancedStrictness,
      balancedStrictness: new WellFormedStrictness()
    });

    const coin: ShieldedCoinInfo = createShieldedCoinInfo(token.raw, 100_000n);
    const zswapOutput = ZswapOutput.newContractOwned(coin, undefined, addr);

    const ttl = plus1Hour(state.time);

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID).addCalls(
      FIRST_SEGMENT_SPECIFIER,
      [],
      state.ledger.parameters,
      ttl,
      [],
      [zswapOutput],
      []
    );

    expect(() => tx.wellFormed(state.ledger, unbalancedStrictness, state.time)).toThrow(
      'all contract-associated commitments must be claimed'
    );
  });

  test('addCalls - puts provided Zswap outputs and Zswap inputs into guaranteed section', () => {
    const state = TestState.new();
    const ttl = state.time;

    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);

    const op = new ContractOperation();
    op.verifierKey = TestResource.operationVerifierKey();

    const contract = new ContractState();
    contract.setOperation(STORE, op);
    contract.data = new ChargedState(StateValue.newArray());
    contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);

    const deploy = new ContractDeploy(contract);
    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([], [], [deploy], state.time)
    );

    const unbalanced = new WellFormedStrictness();
    unbalanced.enforceBalancing = false;

    tx.wellFormed(state.ledger, unbalanced, state.time);
    state.assertApply(state.balanceTx(tx.eraseProofs()), new WellFormedStrictness());

    const addr = tx.intents!.get(1)!.actions[0].address;

    const ctx = new QueryContext(new ChargedState(state.ledger.index(addr)!.data.state), addr);
    const preTranscript = new PreTranscript(ctx, []);

    const call = new PrePartitionContractCall(
      addr,
      STORE,
      op,
      preTranscript,
      [],
      { value: [], alignment: [] },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      STORE
    );

    const token: ShieldedTokenType = Static.defaultShieldedTokenType();
    state.rewardsShielded(token, 1_000_000n);

    const coinToSpend = Array.from(state.zswap.coins).find((c) => c.type === token.raw);
    expect(coinToSpend).toBeDefined();

    const [nextZswap, zswapInput] = state.zswap.spend(state.zswapKeys, coinToSpend!, 2);
    state.zswap = nextZswap;
    const outCoin = createShieldedCoinInfo(token.raw, 10_000n);
    const zswapOutput = ZswapOutput.new(outCoin, 0, state.zswapKeys.coinPublicKey, state.zswapKeys.encryptionPublicKey);

    const baseTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID);
    const txWithZswapOutput = baseTx.addCalls(
      GUARANTEED_ONLY_SPECIFIER,
      [call],
      state.ledger.parameters,
      ttl,
      [],
      [zswapOutput],
      []
    );

    const txWithZswapInput = baseTx.addCalls(
      SPECIFIC_VALUE_SPECIFIER,
      [call],
      state.ledger.parameters,
      ttl,
      [zswapInput],
      [],
      []
    );

    expect(txWithZswapOutput.guaranteedOffer, 'expected guaranteed offer to exist').toBeDefined();
    expect(txWithZswapOutput.fallibleOffer, 'expected fallible offer to be empty/undefined').toBeFalsy();

    expect(txWithZswapInput.guaranteedOffer, 'expected guaranteed offer to exist').toBeDefined();
    expect(txWithZswapInput.fallibleOffer, 'expected fallible offer to be empty/undefined').toBeFalsy();
  });

  test('addCalls - places Zswap outputs in guaranteed and inputs in fallible', () => {
    const state = TestState.new();

    const token: ShieldedTokenType = Static.defaultShieldedTokenType();
    state.rewardsShielded(token, 1_000_000n);
    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);

    const op = new ContractOperation();
    op.verifierKey = TestResource.operationVerifierKey();

    const contract = new ContractState();
    contract.setOperation(STORE, op);
    contract.data = new ChargedState(StateValue.newArray());
    contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);

    const deploy = new ContractDeploy(contract);
    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([], [], [deploy], state.time)
    );

    const unbalanced = new WellFormedStrictness();
    unbalanced.enforceBalancing = false;

    tx.wellFormed(state.ledger, unbalanced, state.time);
    state.assertApply(state.balanceTx(tx.eraseProofs()), new WellFormedStrictness());

    const addr = tx.intents!.get(1)!.actions[0].address;

    const ctx = new QueryContext(new ChargedState(state.ledger.index(addr)!.data.state), addr);
    const preTranscript = new PreTranscript(ctx, []);
    const call = new PrePartitionContractCall(
      addr,
      STORE,
      op,
      preTranscript,
      [],
      { value: [], alignment: [] },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      STORE
    );

    const coinToSpend = Array.from(state.zswap.coins).find((c) => c.type === token.raw);
    expect(coinToSpend).toBeTruthy();

    const [nextZswap, zswapInput] = state.zswap.spend(state.zswapKeys, coinToSpend!, 1);
    state.zswap = nextZswap;

    const outCoin = createShieldedCoinInfo(token.raw, 10_000n);
    const zswapOutput = ZswapOutput.new(outCoin, 0, state.zswapKeys.coinPublicKey, state.zswapKeys.encryptionPublicKey);

    const guaranteedOffer = ZswapOffer.fromOutput(zswapOutput, token.raw, 0n);
    const fallibleOffer = ZswapOffer.fromInput(zswapInput, token.raw, 0n);

    const txWithGuaranteed = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, guaranteedOffer);
    const txWithFallible = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, fallibleOffer);

    const baseTx = txWithGuaranteed.merge(txWithFallible);

    const tx2 = baseTx
      .addCalls(GUARANTEED_ONLY_SPECIFIER, [call], state.ledger.parameters, state.time)
      .addCalls(SPECIFIC_VALUE_SPECIFIER, [call], state.ledger.parameters, state.time);

    expect(tx2.guaranteedOffer).toBeDefined();
    expect(tx2.fallibleOffer).toBeDefined();

    expect(tx2.guaranteedOffer!.outputs?.length).toBe(1);
    expect(tx2.fallibleOffer!.size).toBe(1);
  });

  function testSpecificSegment(segment: SegmentSpecifier, expectedSegment?: number) {
    const { state, addr, encodedAddr, op, token, unbalancedStrictness, balancedStrictness } = setup();

    const { preCall, zswapOutput, coinCom, coinPayload } = buildCallAndOutput({
      state,
      addr,
      encodedAddr,
      op,
      token,
      baseKey: 0,
      includeClaimReceive: true
    });

    const ttl = plus1Hour(state.time);

    let tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID);
    tx = tx.addCalls(segment, [preCall], state.ledger.parameters, ttl, [], [zswapOutput], []);

    const segmentFound = findSegmentOfCall(tx, addr, STORE);
    if (expectedSegment !== undefined) {
      expect(segmentFound).toBe(expectedSegment);
    } else {
      expect(segmentFound).toBeGreaterThan(0);
      expect(segmentFound).toBeLessThanOrEqual(65535);
    }

    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    const contract = state.ledger.index(addr)!;
    const arr = contract.data.state.asArray()!;

    expect(arr[0].asCell().value[0]).toEqual(coinCom.value[0]);

    const storedPayload = arr[1].asCell().value;
    expect(storedPayload[0]).toEqual(coinPayload.value[0]);
    expect(storedPayload[1]).toEqual(coinPayload.value[1]);
    expect(storedPayload[2]).toEqual(coinPayload.value[2]);

    expect(arr[2].asCell().value[0]).toEqual(ONE_VALUE);

    const recomputed = runtimeCoinCommitment(
      {
        value: [storedPayload[0], storedPayload[1], storedPayload[2]],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, Static.trimTrailingZeros(encodedAddr)],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );
    expect(recomputed).toEqual(coinCom);
  }

  function setup() {
    const state = TestState.new();
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, 5_000_000_000n);
    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;
    const balancedStrictness = new WellFormedStrictness();

    const op = new ContractOperation();
    op.verifierKey = TestResource.operationVerifierKey();

    const { addr, encodedAddr } = deployContract({
      state,
      op,
      unbalancedStrictness,
      balancedStrictness
    });

    return { state, token, op, addr, encodedAddr, unbalancedStrictness, balancedStrictness };
  }

  function buildCallAndOutput(opts: {
    state: TestState;
    addr: ContractAddress;
    encodedAddr: Uint8Array;
    op: ContractOperation;
    token: ShieldedTokenType;
    baseKey: number;
    includeClaimReceive: boolean;
  }) {
    const { state, addr, encodedAddr, op, token, baseKey, includeClaimReceive } = opts;

    const coin: ShieldedCoinInfo = createShieldedCoinInfo(token.raw, 100_000n);
    const encodedCoin = encodeShieldedCoinInfo(coin);
    const value16 = bigIntToValue(encodedCoin.value)[0];

    const coinPayload: AlignedValue = {
      value: [Static.trimTrailingZeros(encodedCoin.nonce), Static.trimTrailingZeros(encodedCoin.color), value16],
      alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
    };

    const coinCom: AlignedValue = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(coinPayload.value[0]),
          Static.trimTrailingZeros(coinPayload.value[1]),
          Static.trimTrailingZeros(coinPayload.value[2])
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, Static.trimTrailingZeros(encodedAddr)],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    const transcriptOps = [
      ...kernelSelf(),
      ...(includeClaimReceive ? kernelClaimZswapCoinReceive(coinCom) : []),
      ...cellWrite(getKey(baseKey), true, coinCom),
      ...cellWrite(getKey(baseKey + 1), true, coinPayload),
      ...cellWrite(getKey(baseKey + 2), true, { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }),
      ...cellRead(getKey(baseKey + 2), false)
    ];

    const program = programWithResults(transcriptOps, [
      { value: [Static.trimTrailingZeros(encodedAddr)], alignment: [ATOM_BYTES_32] },
      { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }
    ]);

    const context = new QueryContext(new ChargedState(state.ledger.index(addr)!.data.state), addr);
    const preTranscript = new PreTranscript(context, program);

    const privateTranscriptOutputs: AlignedValue[] = [
      { value: [Static.trimTrailingZeros(Random.generate32Bytes())], alignment: [ATOM_BYTES_32] }
    ];

    const preCall = new PrePartitionContractCall(
      addr,
      STORE,
      op,
      preTranscript,
      privateTranscriptOutputs,
      coinPayload,
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      STORE
    );

    const zswapOutput = ZswapOutput.newContractOwned(coin, undefined, addr);

    return { preCall, zswapOutput, coinCom, coinPayload };
  }

  function deployContract({
    state,
    op,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    op: ContractOperation;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }): { addr: ContractAddress; encodedAddr: Uint8Array } {
    const contract = new ContractState();
    contract.setOperation(STORE, op);

    contract.data = new ChargedState(
      StateValue.newArray()
        .arrayPush(StateValue.newCell({ value: [EMPTY_VALUE], alignment: [ATOM_BYTES_32] }))
        .arrayPush(
          StateValue.newCell({
            value: [EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
          })
        )
        .arrayPush(StateValue.newCell({ value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }))
        .arrayPush(StateValue.newCell({ value: [EMPTY_VALUE], alignment: [ATOM_BYTES_32] }))
        .arrayPush(
          StateValue.newCell({
            value: [EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
          })
        )
        .arrayPush(StateValue.newCell({ value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }))
    );

    contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);

    const deploy = new ContractDeploy(contract);
    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([], [], [deploy], state.time)
    );

    const addr: ContractAddress = tx.intents!.get(1)!.actions[0].address;
    const encodedAddr = encodeContractAddress(addr);

    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    return { addr, encodedAddr };
  }

  function findSegmentOfCall(
    tx: Transaction<Signaturish, Proofish, Bindingish>,
    addr: ContractAddress,
    entryPoint: string
  ): number {
    const found = Array.from(tx.intents!.entries()).find(([, intent]) =>
      intent.actions.some((ca) => ca instanceof ContractCall && ca.address === addr && ca.entryPoint === entryPoint)
    );

    if (!found) throw new Error(`Call not found: ${entryPoint}`);
    return found[0];
  }
});
