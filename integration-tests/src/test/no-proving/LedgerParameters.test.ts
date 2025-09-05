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

import { LedgerParameters } from '@midnight-ntwrk/ledger';

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
});
