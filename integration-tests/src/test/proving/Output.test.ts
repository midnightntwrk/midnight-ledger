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
import { prove } from '@/proof-provider';
import { HEX_64_REGEX, Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - Output [@slow][@proving]', () => {
  /**
   * Test output serialization and deserialization with proofs.
   *
   * @given A proven transaction with guaranteed offer outputs
   * @when Serializing and deserializing the first output
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize outputs correctly', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const output = transaction.guaranteedOffer?.outputs[0];
    const output2 = ZswapOutput.deserialize('proof', output!.serialize());

    expect(output2.toString()).toEqual(output?.toString());
  });

  /**
   * Test output construction and properties verification.
   *
   * @given A proven transaction with guaranteed and fallible offers
   * @when Accessing the first output from guaranteed offer
   * @then Should have valid commitment format and undefined contract address
   */
  test('should construct outputs with correct properties', async () => {
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const output = transaction.guaranteedOffer?.outputs[0];

    expect(output?.commitment).toMatch(HEX_64_REGEX);
    expect(output?.contractAddress).toBeUndefined();
    assertSerializationSuccess(output!, undefined, ProofMarker.proof);
  });
});
