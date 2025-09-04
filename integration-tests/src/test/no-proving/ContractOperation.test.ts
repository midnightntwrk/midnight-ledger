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

import { ContractOperation, ContractState } from '@midnight-ntwrk/ledger';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - ContractOperation', () => {
  /**
   * Test toString method output.
   *
   * @given A new ContractOperation instance
   * @when Calling toString method
   * @then Should return '<verifier key>' string
   */
  test('should print out verifier key string', () => {
    const contractOperation = new ContractOperation();
    expect(contractOperation.toString()).toEqual('<verifier key>');
  });

  /**
   * Test serialization and deserialization process.
   *
   * @given A ContractOperation instance
   * @when Serializing and then deserializing with same NetworkId
   * @then Should maintain object integrity and string representation
   */
  it('should serialize and deserialize', () => {
    const contractOperation = new ContractOperation();
    const array = contractOperation.serialize();

    expect(ContractOperation.deserialize(array).toString()).toEqual(contractOperation.toString());
  });

  /**
   * Test validation of verifier key updates.
   *
   * @given A ContractOperation instance
   * @when Setting verifier key to invalid data (1024 byte array)
   * @then Should throw error about unsupported version
   */
  test('should fail on invalid verifier key update', () => {
    const contractOperation = new ContractOperation();

    expect(() => {
      contractOperation.verifierKey = new Uint8Array(1024);
    }).toThrow(/expected header tag 'midnight:verifier-key/);
  });

  /**
   * Test serialization of operation within contract state.
   *
   * @given A ContractState with a named operation
   * @when Serializing the operation
   * @then Should complete successfully without errors
   */
  test('should serialize operation within contract state', () => {
    const OPERATION_NAME = 'abcdef';
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractState.setOperation(OPERATION_NAME, contractOperation);
    const operation = contractState.operation(OPERATION_NAME);

    assertSerializationSuccess(operation!);
  });
});
