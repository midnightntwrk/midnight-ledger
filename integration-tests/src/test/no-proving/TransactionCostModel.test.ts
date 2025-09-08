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

import { TransactionCostModel } from '@midnight-ntwrk/ledger';

describe('Ledger API - TransactionCostModel', () => {
  /**
   * Test serialization and deserialization process.
   *
   * @given A initial TransactionCostModel
   * @when Serializing and then deserializing
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize correctly', () => {
    const transactionCostModel = TransactionCostModel.initialTransactionCostModel();
    const array = transactionCostModel.serialize();

    expect(TransactionCostModel.deserialize(array).toString()).toEqual(transactionCostModel.toString());
  });

  /**
   * Test error handling for invalid data during deserialization.
   *
   * @given Invalid byte array data
   * @when Attempting to deserialize
   * @then Should throw error about unsupported version
   */
  test('should throw error on deserialize with invalid data', () => {
    const invalidArray = new Uint8Array([1, 2, 3]);

    expect(() => TransactionCostModel.deserialize(invalidArray)).toThrow(
      /expected header tag 'midnight:transaction-cost-model/
    );
  });
});
