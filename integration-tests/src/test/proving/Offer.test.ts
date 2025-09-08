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

import { shieldedToken, ZswapOffer, Transaction } from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { type ShieldedTokenType, Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - OfferX [@slow][@proving]', () => {
  /**
   * Test merging offers from proven transactions.
   *
   * @given Two offers with shielded tokens (100 and 200)
   * @when Merging guaranteed offer with fallible offer
   * @then Should create merged offer with correct inputs, outputs and deltas
   */
  test('should merge offers correctly', async () => {
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(0, shieldedToken() as ShieldedTokenType, 100n),
      Static.unprovenOfferFromOutput(1, shieldedToken() as ShieldedTokenType, 200n)
    );
    const transaction = await prove(unprovenTransaction);

    const merged = transaction.guaranteedOffer?.merge(transaction.fallibleOffer!.get(1)!);

    expect(merged!.inputs.length).toEqual(0);
    expect(merged!.outputs.length).toEqual(2);
    expect(merged!.deltas.get((shieldedToken() as ShieldedTokenType).raw)).toEqual(-300n);
    assertSerializationSuccess(merged!, undefined, ProofMarker.proof);
  });

  /**
   * Test offer serialization and deserialization with proofs.
   *
   * @given A proven transaction with offers
   * @when Serializing and deserializing the guaranteed offer
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize offers correctly', async () => {
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(0, shieldedToken() as ShieldedTokenType, 100n),
      Static.unprovenOfferFromOutput(1, shieldedToken() as ShieldedTokenType, 200n)
    );
    const transaction = await prove(unprovenTransaction);
    const offer = transaction.guaranteedOffer;

    const serialized = offer!.serialize();
    const deserialized = ZswapOffer.deserialize('proof', serialized);

    expect(deserialized.toString()).toEqual(offer!.toString());
  });
});
