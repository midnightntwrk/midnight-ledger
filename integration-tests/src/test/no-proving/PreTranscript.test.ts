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
  communicationCommitment,
  ChargedState,
  PreTranscript,
  QueryContext,
  StateValue,
  communicationCommitmentRandomness
} from '@midnight-ntwrk/ledger';
import { Random, Static } from '@/test-objects';

describe('Ledger API - PreTranscript', () => {
  /**
   * Test error handling for invalid commitment.
   *
   * @given A QueryContext and an invalid commitment string
   * @when Creating a PreTranscript
   * @then Should throw 'failed to fill whole buffer' error
   */
  test('should throw error with invalid commitment', () => {
    expect(
      () =>
        new PreTranscript(
          new QueryContext(new ChargedState(StateValue.newNull()), Random.contractAddress()),
          [],
          communicationCommitment(Static.alignedValue, Static.alignedValueCompress, '')
        )
    ).toThrow('failed to fill whole buffer');
  });

  /**
   * Test string representation of PreTranscript.
   *
   * @given A PreTranscript with valid parameters
   * @when Calling toString method
   * @then Should return a string matching the PreTranscript pattern
   */
  test('should print out the class representation', () => {
    const preTranscript = new PreTranscript(
      new QueryContext(new ChargedState(StateValue.newNull()), Random.contractAddress()),
      [],
      '00'
    );

    expect(preTranscript.toString()).toMatch(/PreTranscript.*/);
  });

  /**
   * Test creation of PreTranscript with valid inputs.
   *
   * @given A QueryContext, empty operations array, and valid commitment
   * @when Creating a PreTranscript
   * @then Should create successfully and have proper string representation
   */
  test('should create PreTranscript with valid inputs', () => {
    const queryContext = new QueryContext(new ChargedState(StateValue.newNull()), Random.contractAddress());
    const commitment = communicationCommitment(
      Static.alignedValue,
      Static.alignedValueCompress,
      communicationCommitmentRandomness()
    );
    const preTranscript = new PreTranscript(queryContext, [], commitment);

    expect(preTranscript).toBeDefined();
    expect(preTranscript.toString()).toMatch(/PreTranscript.*/);
  });

  /**
   * Test handling of undefined commitment.
   *
   * @given A QueryContext and undefined commitment
   * @when Creating a PreTranscript
   * @then Should create successfully and have proper string representation
   */
  test('should handle undefined commitment', () => {
    const queryContext = new QueryContext(new ChargedState(StateValue.newNull()), Random.contractAddress());
    const preTranscript = new PreTranscript(queryContext, [], undefined);

    expect(preTranscript).toBeDefined();
    expect(preTranscript.toString()).toMatch(/PreTranscript.*/);
  });

  /**
   * Test handling of non-empty operations array.
   *
   * @given A QueryContext and non-empty operations array
   * @when Creating a PreTranscript
   * @then Should create successfully and have proper string representation
   */
  test('should handle non-empty operations', () => {
    const queryContext = new QueryContext(new ChargedState(StateValue.newNull()), Random.contractAddress());
    const preTranscript = new PreTranscript(queryContext, ['new'], '00');

    expect(preTranscript).toBeDefined();
    expect(preTranscript.toString()).toMatch(/PreTranscript.*/);
  });
});
