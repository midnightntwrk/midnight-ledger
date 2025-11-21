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

import { expect } from 'vitest';
import {
  DUST_GRACE_PERIOD_IN_SECONDS,
  GENERATION_DECAY_RATE,
  initialParameters,
  NIGHT_DUST_RATIO
} from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - DustParameters', () => {
  /**
   * Test string representation of DustParameters.
   *
   * @given A new DustParameters instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const expected = `DustParameters {
    night_dust_ratio: ${NIGHT_DUST_RATIO},
    generation_decay_rate: ${GENERATION_DECAY_RATE},
    dust_grace_period: Duration(
        ${DUST_GRACE_PERIOD_IN_SECONDS},
    ),
}`;

    expect(initialParameters.toString()).toEqual(expected);
  });

  /**
   * Test serialization and deserialization of DustParameters.
   *
   * @given A new DustParameters instance
   * @when Calling serialize method
   * @and Calling deserialize method
   * @then Should return formatted strings with the same values
   */
  test('should serialize and deserialize', () => {
    assertSerializationSuccess(initialParameters);
  });

  /**
   * Test all getters of DustParameters.
   *
   * @given A new DustParameters instance
   * @when Checking all getters
   * @then Should return the same values as initially set
   */
  test('should have all getters valid', () => {
    const timeToCapSeconds = divCeilBigInt(NIGHT_DUST_RATIO, GENERATION_DECAY_RATE);

    expect(initialParameters.nightDustRatio).toEqual(NIGHT_DUST_RATIO);
    expect(initialParameters.generationDecayRate).toEqual(GENERATION_DECAY_RATE);
    expect(initialParameters.dustGracePeriodSeconds).toEqual(DUST_GRACE_PERIOD_IN_SECONDS);
    expect(initialParameters.timeToCapSeconds).toEqual(timeToCapSeconds);
  });

  /**
   * Test all setters of DustParameters.
   *
   * @given A new DustParameters instance
   * @when Changing all setters
   * @then Should return updated values
   */
  test('should have all setters valid', () => {
    const updatedNightDustRatio = NIGHT_DUST_RATIO + 1n;
    const updatedGenerationDecayRate = GENERATION_DECAY_RATE + 1n;
    const updatedDustGracePeriodSeconds = DUST_GRACE_PERIOD_IN_SECONDS + 1n;

    initialParameters.nightDustRatio = updatedNightDustRatio;
    initialParameters.generationDecayRate = updatedGenerationDecayRate;
    initialParameters.dustGracePeriodSeconds = updatedDustGracePeriodSeconds;

    expect(initialParameters.nightDustRatio).toEqual(updatedNightDustRatio);
    expect(initialParameters.generationDecayRate).toEqual(updatedGenerationDecayRate);
    expect(initialParameters.dustGracePeriodSeconds).toEqual(updatedDustGracePeriodSeconds);
  });

  function divCeilBigInt(a: bigint, b: bigint): bigint {
    return (a + b - 1n) / b;
  }
});
