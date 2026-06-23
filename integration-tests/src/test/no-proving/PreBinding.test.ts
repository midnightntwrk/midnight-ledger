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

import { Intent, PreBinding } from '@midnight-ntwrk/ledger';
import { BindingMarker } from '@/test/utils/Markers';

describe('Ledger API - PreBinding', () => {
  /**
   * Test the PreBinding marker carried by an unbound Intent.
   *
   * @given A freshly created Intent
   * @when Reading its PreBinding and serializing it
   * @then It should report its instance, render a string, and round-trip through deserialize
   */
  test('serializes, round-trips, and reports its instance', () => {
    const { binding } = Intent.new(new Date(1000));

    expect(binding.instance).toEqual(BindingMarker.preBinding);
    expect(binding.toString().length).toBeGreaterThan(0);

    const serialized = binding.serialize();
    expect(PreBinding.deserialize(serialized).serialize()).toEqual(serialized);
  });
});
