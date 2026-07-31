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

import { NoBinding } from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';
import { BindingMarker } from '@/test/utils/Markers';

describe('Ledger API - NoBinding', () => {
  /**
   * Test the NoBinding marker carried by a proof-erased transaction's intent.
   *
   * @given A proof-erased transaction with an intent
   * @when Reading the intent's NoBinding and serializing it
   * @then It should report its instance, render a string, and round-trip through deserialize
   */
  test('serializes, round-trips, and reports its instance', () => {
    const txErased = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls().eraseProofs();
    const { binding } = txErased.intents!.get(1)!;

    expect(binding.instance).toEqual(BindingMarker.noBinding);
    expect(binding.toString().length).toBeGreaterThan(0);

    const serialized = binding.serialize();
    expect(NoBinding.deserialize(serialized).serialize()).toEqual(serialized);
  });
});
