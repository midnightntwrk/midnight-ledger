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
  shieldedToken,
  ZswapOffer,
  type SignatureEnabled,
  type NoProof,
  type Transaction,
  type NoBinding
} from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { type ShieldedTokenType, Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - ProofErasedOffer [@slow][@proving]', () => {
  let proofErasedTransaction: Transaction<SignatureEnabled, NoProof, NoBinding>;

  beforeAll(async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const transaction = await prove(unprovenTransaction);
    proofErasedTransaction = transaction.eraseProofs();
  });

  /**
   * Test merging proof-erased offers with correct delta calculation.
   *
   * @given Proof-erased transaction with guaranteed and fallible offers
   * @when Merging guaranteed offer with fallible offer
   * @then Should produce merged offer with combined outputs and summed deltas
   */
  test('should merge proof-erased offers correctly', async () => {
    const proofErasedOfferGuaranteed = proofErasedTransaction.guaranteedOffer;
    const proofErasedOfferFallible = proofErasedTransaction.fallibleOffer!.get(1);
    const merged = proofErasedOfferGuaranteed?.merge(proofErasedOfferFallible!);
    const deltaGuaranteed = proofErasedOfferGuaranteed?.deltas.get((shieldedToken() as ShieldedTokenType).raw);
    const deltaFallible = proofErasedOfferFallible?.deltas.get((shieldedToken() as ShieldedTokenType).raw);

    expect(proofErasedOfferGuaranteed).toBeDefined();
    expect(proofErasedOfferFallible).toBeDefined();
    expect(merged?.outputs.length).toEqual(2);
    expect(merged?.deltas.get((shieldedToken() as ShieldedTokenType).raw)).toEqual(deltaGuaranteed! + deltaFallible!);
    assertSerializationSuccess(merged!, undefined, ProofMarker.noProof);
  });

  /**
   * Test symmetric property of offer merging.
   *
   * @given Two distinct proof-erased offers
   * @when Merging in both directions (A.merge(B) and B.merge(A))
   * @then Should produce identical results regardless of merge order
   */
  test('should have symmetric merge operation', async () => {
    const proofErasedOfferGuaranteed = proofErasedTransaction.guaranteedOffer;
    const proofErasedOfferFallible = proofErasedTransaction.fallibleOffer!.get(1);
    const merged = proofErasedOfferGuaranteed?.merge(proofErasedOfferFallible!);
    const merged2 = proofErasedOfferFallible?.merge(proofErasedOfferGuaranteed!);

    expect(proofErasedOfferGuaranteed).toBeDefined();
    expect(proofErasedOfferFallible).toBeDefined();
    expect(merged?.toString()).toEqual(merged2?.toString());
    assertSerializationSuccess(merged2!, undefined, ProofMarker.noProof);
  });

  /**
   * Test error handling for self-merge attempts.
   *
   * @given A proof-erased offer
   * @when Attempting to merge offer with itself
   * @then Should throw error about non-disjoint coin sets
   */
  test('should not allow merging offer with itself', async () => {
    const proofErasedOfferGuaranteed = proofErasedTransaction.guaranteedOffer;
    expect(() => proofErasedOfferGuaranteed?.merge(proofErasedOfferGuaranteed!)).toThrow(
      'attempted to merge non-disjoint coin sets'
    );
  });

  /**
   * Test serialization and deserialization of proof-erased offers.
   *
   * @given A proof-erased guaranteed offer
   * @when Serializing and deserializing the offer
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize proof-erased offers correctly', async () => {
    const proofErasedOfferGuaranteed = proofErasedTransaction.guaranteedOffer;

    expect(proofErasedOfferGuaranteed).toBeDefined();
    expect(ZswapOffer.deserialize('no-proof', proofErasedOfferGuaranteed!.serialize()).toString()).toEqual(
      proofErasedOfferGuaranteed?.toString()
    );
  });
});
