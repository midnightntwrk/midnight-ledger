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

import { PreProof } from '@midnight-ntwrk/ledger';
import { createValidZSwapInput } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe('Ledger API - PreProof', () => {
  /**
   * Test the PreProof marker carried by an unproven ZswapInput.
   *
   * @given A valid unproven ZswapInput
   * @when Reading its PreProof and serializing it
   * @then It should report its instance, render a string, and round-trip through deserialize
   */
  test('serializes, round-trips, and reports its instance', () => {
    const { zswapInput } = createValidZSwapInput(100n);
    const { proof } = zswapInput;

    expect(proof.instance).toEqual(ProofMarker.preProof);
    expect(proof.toString().length).toBeGreaterThan(0);

    const serialized = proof.serialize();
    expect(PreProof.deserialize(serialized).serialize()).toEqual(serialized);
  });
});
