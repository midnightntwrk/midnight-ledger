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
  LedgerParameters,
  LedgerState,
  shieldedToken,
  Transaction,
  WellFormedStrictness,
  ZswapChainState
} from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { type ShieldedTokenType, Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess, mapFindByKey } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - ProofErasedTransaction [@slow][@proving]', () => {
  /**
   * Test proof erasure from proven transactions.
   *
   * @given A proven transaction with guaranteed and fallible offers
   * @when Erasing proofs from the transaction
   * @then Should maintain structure while removing proofs and match unproven transaction string representation
   */
  test('should erase proofs correctly', async () => {
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput(1)
    );
    const transaction = await prove(unprovenTransaction);
    const proofErasedTransaction = transaction.eraseProofs();

    expect(proofErasedTransaction.toString()).toMatch(/StandardTransaction {.*/);
    expect(proofErasedTransaction.toString()).toEqual(unprovenTransaction.toString());
    expect(proofErasedTransaction.identifiers()).toHaveLength(2);
    expect(proofErasedTransaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(proofErasedTransaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(proofErasedTransaction.guaranteedOffer?.deltas.size).toEqual(1);
    expect(proofErasedTransaction.fallibleOffer?.get(1)!.inputs).toHaveLength(0);
    expect(proofErasedTransaction.fallibleOffer?.get(1)!.outputs).toHaveLength(1);
    expect(proofErasedTransaction.fallibleOffer?.get(1)!.deltas.size).toEqual(1);
    expect(proofErasedTransaction.intents).toBeUndefined();
    assertSerializationSuccess(
      proofErasedTransaction,
      SignatureMarker.signature,
      ProofMarker.noProof,
      BindingMarker.noBinding
    );
  });

  /**
   * Test symmetric property of transaction merging.
   *
   * @given Two distinct proof-erased transactions
   * @when Merging in both directions
   * @then Should produce identical results regardless of merge order
   */
  test('should have symmetric merge operation', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const unprovenTransaction2 = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput(1)
    );
    const transaction2 = await prove(unprovenTransaction2);
    const proofErasedTransaction = transaction.eraseProofs();
    const proofErasedTransaction2 = transaction2.eraseProofs();
    const mergedTx = proofErasedTransaction.merge(proofErasedTransaction2);
    const mergedTx2 = proofErasedTransaction2.merge(proofErasedTransaction);

    expect(mergedTx.guaranteedOffer?.inputs.length).toEqual(0);
    expect(mergedTx.guaranteedOffer?.outputs.length).toEqual(2);
    expect(mergedTx.guaranteedOffer?.transients.length).toEqual(0);
    expect(mergedTx.guaranteedOffer?.deltas.get((shieldedToken() as ShieldedTokenType).raw)).toEqual(-248n);
    expect(mergedTx.fallibleOffer?.get(1)!.inputs.length).toEqual(0);
    expect(mergedTx.fallibleOffer?.get(1)!.outputs.length).toEqual(2);
    expect(mergedTx.fallibleOffer?.get(1)!.transients.length).toEqual(0);
    expect(mergedTx.fallibleOffer?.get(1)!.deltas.get((shieldedToken() as ShieldedTokenType).raw)).toEqual(-248n);
    expect(mergedTx.intents?.size).toEqual(1);
    expect(mergedTx.intents?.get(1)?.actions.length).toEqual(2);
    expect(mergedTx.rewards).toBeUndefined();
    expect(mergedTx.toString()).toEqual(mergedTx2.toString());
    assertSerializationSuccess(
      proofErasedTransaction,
      SignatureMarker.signature,
      ProofMarker.noProof,
      BindingMarker.noBinding
    );
  });

  /**
   * Test serialization and deserialization of proof-erased transactions.
   *
   * @given A proof-erased transaction with contract calls
   * @when Serializing and deserializing the transaction
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize correctly', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const proofErasedTransaction = transaction.eraseProofs();

    const serialized = proofErasedTransaction.serialize();
    expect(Transaction.deserialize('signature', 'no-proof', 'no-binding', serialized).toString()).toEqual(
      transaction.toString()
    );
  });

  /**
   * Test well-formed validation without balance enforcement.
   *
   * @given A proof-erased transaction and ledger state
   * @when Validating with strictness settings (enforceBalancing = false)
   * @then Should pass validation within valid time window and verify all proofs
   */
  test('should validate well-formed without enforcing balancing', async () => {
    const date = new Date();
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const proofErasedTransaction = transaction.eraseProofs();

    const zSwapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zSwapChainState);
    const strictness = new WellFormedStrictness();
    strictness.verifyContractProofs = true;
    strictness.enforceBalancing = false;
    strictness.verifyNativeProofs = true;
    expect(() => proofErasedTransaction.wellFormed(ledgerState, strictness, Static.calcBlockTime(date, 70))).toThrow();
    expect(() =>
      proofErasedTransaction.wellFormed(ledgerState, strictness, Static.calcBlockTime(date, -15))
    ).not.toThrow();
    expect(transaction.identifiers().length).toEqual(3);
    expect(strictness.verifyContractProofs).toEqual(true);
    expect(strictness.enforceBalancing).toEqual(false);
    expect(strictness.verifyNativeProofs).toEqual(true);
    assertSerializationSuccess(
      proofErasedTransaction,
      SignatureMarker.signature,
      ProofMarker.noProof,
      BindingMarker.noBinding
    );
  });

  /**
   * Test well-formed validation with balance enforcement.
   *
   * @given A proof-erased transaction and ledger state
   * @when Validating with strictness settings (enforceBalancing = true)
   * @then Should throw balance validation error for negative token balances
   */
  test('should validate well-formed with enforcing balancing', async () => {
    const date = new Date();
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const proofErasedTransaction = transaction.eraseProofs();

    const zSwapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zSwapChainState);
    const strictness = new WellFormedStrictness();
    strictness.verifyContractProofs = true;
    strictness.enforceBalancing = true;
    strictness.verifyNativeProofs = true;

    expect(() => proofErasedTransaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).toThrow(
      /invalid balance -\d+ for token .* in segment \d+; balance must be positive/
    );
    assertSerializationSuccess(
      proofErasedTransaction,
      SignatureMarker.signature,
      ProofMarker.noProof,
      BindingMarker.noBinding
    );
  });

  /**
   * Test equivalence between proof-erased and unproven transaction construction.
   *
   * @given An unproven transaction that can be proven and then proof-erased
   * @when Comparing direct proof-erasure with prove-then-erase approach
   * @then Should produce transactions with identical structure and content
   */
  test('should produce same result via two construction methods', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const transaction = await prove(unprovenTransaction);
    const proofErasedTransaction2 = transaction.eraseProofs();

    expect(proofErasedTransaction.guaranteedOffer!.inputs.length).toEqual(
      proofErasedTransaction2.guaranteedOffer!.inputs.length
    );
    expect(proofErasedTransaction.guaranteedOffer!.outputs.length).toEqual(
      proofErasedTransaction2.guaranteedOffer!.outputs.length
    );
    expect(proofErasedTransaction.fallibleOffer!.get(1)!.inputs.length).toEqual(
      proofErasedTransaction2.fallibleOffer!.get(1)!.inputs.length
    );
    expect(proofErasedTransaction.fallibleOffer!.get(1)!.outputs.length).toEqual(
      proofErasedTransaction2.fallibleOffer!.get(1)!.outputs.length
    );
    expect(proofErasedTransaction.intents!.size).toEqual(proofErasedTransaction2.intents!.size);
    assertSerializationSuccess(
      proofErasedTransaction,
      SignatureMarker.signature,
      ProofMarker.noProof,
      BindingMarker.noBinding
    );
  });

  /**
   * Test imbalance calculation for different segments and fee scenarios.
   *
   * @given A proof-erased transaction with guaranteed and fallible offers
   * @when Calculating imbalances for different segments with and without fees
   * @then Should return correct negative imbalances and proper fee adjustments
   */
  test('should calculate imbalances correctly', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();

    const imbalances = proofErasedTransaction.imbalances(0, 10n);
    const imbalancesZeroFees = proofErasedTransaction.imbalances(0);
    const imbalancesFallible = proofErasedTransaction.imbalances(1, 10n);
    const imbalancesFallibleZeroFees = proofErasedTransaction.imbalances(1);

    expect(mapFindByKey(imbalances, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalancesZeroFees, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalancesFallible, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalancesFallibleZeroFees, shieldedToken())).toBeLessThan(0n);
    expect(mapFindByKey(imbalances, { tag: 'dust' })).toEqual(-10n);
    expect(mapFindByKey(imbalancesZeroFees, { tag: 'dust' })).toBeUndefined();
    assertSerializationSuccess(
      proofErasedTransaction,
      SignatureMarker.signature,
      ProofMarker.noProof,
      BindingMarker.noBinding
    );
  });

  /**
   * Test fee calculation for proof-erased transactions.
   *
   * @given A proof-erased transaction with contract calls
   * @when Calculating fees using initial ledger parameters
   * @then Should return positive fee amount
   */
  test('should calculate fees correctly', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();

    expect(proofErasedTransaction.fees(LedgerParameters.initialParameters())).toBeGreaterThan(0n);
  });
});
