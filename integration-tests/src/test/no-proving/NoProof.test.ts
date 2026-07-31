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

import { NoProof } from '@midnight-ntwrk/ledger';
import { ProofMarker } from '@/test/utils/Markers';

describe('Ledger API - NoProof', () => {
  /**
   * Test the instance discriminator.
   *
   * @given A NoProof marker
   * @when Reading its instance discriminator
   * @then It should report 'no-proof'
   */
  test('reports its instance discriminator', () => {
    const proof = new NoProof();

    expect(proof.instance).toEqual(ProofMarker.noProof);
    expect(proof.toString().length).toBeGreaterThan(0);
  });
});
