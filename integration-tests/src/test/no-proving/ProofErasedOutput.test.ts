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

import { ZswapOutput } from '@midnight-ntwrk/ledger';
import { HEX_64_REGEX, Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe('Ledger API - ZswapOutput [@slow][@proving]', () => {
  /**
   * Test serialization and deserialization of proof-erased output.
   *
   * @given An unproven transaction with proof-erased outputs
   * @when Serializing and deserializing the first guaranteed output
   * @then Should maintain identical string representation
   */
  test('should serialize and deserialize correctly', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const output = proofErasedTransaction.guaranteedOffer?.outputs[0];
    const output2 = ZswapOutput.deserialize('no-proof', output!.serialize());

    expect(output2.toString()).toEqual(output?.toString());
  });

  /**
   * Test construction of proof-erased output.
   *
   * @given An unproven transaction with guaranteed and fallible offers
   * @when Erasing proofs and accessing the first guaranteed output
   * @then Should have valid commitment and undefined contract address
   */
  test('should construct proof-erased output correctly', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const output = proofErasedTransaction.guaranteedOffer?.outputs[0];

    expect(output?.commitment).toMatch(HEX_64_REGEX);
    expect(output?.contractAddress).toBeUndefined();
    assertSerializationSuccess(output!, undefined, ProofMarker.noProof);
  });
});
