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

import { LedgerParameters } from '@midnightntwrk/ledger';

describe('Ledger API - LedgerParameters', () => {
  /**
   * Test serialization and deserialization process.
   *
   * @given LedgerParameters instance
   * @when Serializing and then deserializing
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize correctly', () => {
    const ledgerParameters = LedgerParameters.initialParameters();
    const array = ledgerParameters.serialize();

    expect(LedgerParameters.deserialize(array).toString()).toEqual(ledgerParameters.toString());
  });

  /**
   * Test ledger parameters getters
   *
   * @given LedgerParameters with initial values
   * @when Accessing getters
   * @then Should have defined both cost model and dust with valid values
   */
  test('should have transaction cost model with valid properties', () => {
    const ledgerParameters = LedgerParameters.initialParameters();

    expect(ledgerParameters.transactionCostModel).toBeDefined();
    expect(ledgerParameters.dust).toBeDefined();
  });

  /**
   * Test the transaction limits getter
   *
   * @given LedgerParameters with initial values
   * @when Accessing limits
   * @then Should return the initial transaction and block limits, times in picoseconds
   */
  test('should expose the initial transaction limits', () => {
    const { limits } = LedgerParameters.initialParameters();

    expect(limits.transactionByteLimit).toEqual(1_048_576n);
    expect(limits.timeToDismissPerByte).toEqual(2_000_000n);
    expect(limits.minTimeToDismiss).toEqual(15_000_000_000n);
    expect(limits.blockLimits).toEqual({
      readTime: 1_000_000_000_000n,
      computeTime: 1_000_000_000_000n,
      blockUsage: 200_000n,
      bytesWritten: 50_000n,
      bytesChurned: 1_000_000n
    });
    expect(limits.blockWithdrawalMinimumMultiple).toBeCloseTo(0.5, 12);
    expect(limits.maxContractMetadataSize).toEqual(50_000n);
  });

  /**
   * Test the transaction limits string representation
   *
   * @given LedgerParameters with initial values
   * @when Converting their limits to a string, compact and not
   * @then Should name the limits with their initial values
   */
  test('should print the transaction limits', () => {
    const { limits } = LedgerParameters.initialParameters();

    expect(limits.toString(true)).toMatch(/^TransactionLimits \{ transaction_byte_limit: 1048576, /);
    expect(limits.toString()).toContain('min_time_to_dismiss: 15.000ms');
  });

  /**
   * Test the block limits match the ones fullness is normalized against
   *
   * @given LedgerParameters with initial values
   * @when Normalizing a fullness equal to the block limits
   * @then Should normalize every dimension to a full block
   */
  test('should normalize the block limits to a full block', () => {
    const ledgerParameters = LedgerParameters.initialParameters();

    expect(ledgerParameters.normalizeFullness(ledgerParameters.limits.blockLimits)).toEqual({
      readTime: 1,
      computeTime: 1,
      blockUsage: 1,
      bytesWritten: 1,
      bytesChurned: 1
    });
  });
});
