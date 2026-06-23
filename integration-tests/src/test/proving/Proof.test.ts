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

import { Transaction, ZswapOffer, Proof } from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import '@/setup-proving';
import { LOCAL_TEST_NETWORK_ID, Static } from '@/test-objects';
import { createValidZSwapInput } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe('Ledger API - Proof [@slow][@proving]', () => {
  /**
   * Test the Proof marker carried by a proven ZswapInput.
   *
   * @given A proven transaction containing a spend
   * @when Reading the proven input's Proof and serializing it
   * @then It should report its instance, render a string, and round-trip through deserialize
   */
  test('serializes, round-trips, and reports its instance', async () => {
    const tokenType = 'a'.repeat(64);
    const { zswapInput } = createValidZSwapInput(100n, tokenType);
    const inputOffer = ZswapOffer.fromInput(zswapInput, tokenType, 100n);
    const outputOffer = Static.unprovenOfferFromOutput(0, { tag: 'shielded', raw: tokenType }, 100n);

    const txProven = await prove(Transaction.fromParts(LOCAL_TEST_NETWORK_ID, inputOffer.merge(outputOffer)));
    const { proof } = txProven.guaranteedOffer!.inputs![0];

    expect(proof.instance).toEqual(ProofMarker.proof);
    expect(proof.toString().length).toBeGreaterThan(0);

    const serialized = proof.serialize();
    expect(Proof.deserialize(serialized).serialize()).toEqual(serialized);
  });
});
