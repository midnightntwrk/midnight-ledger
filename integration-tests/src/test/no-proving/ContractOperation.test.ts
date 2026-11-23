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

import { ContractOperation, ContractState } from '@midnightntwrk/ledger';
import { TestResource } from '@/test-objects';
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
   * @when Setting verifier key to invalid data (1024 byte array), then to a key with an unknown tag
   * @then Should throw error about unsupported version, naming the unknown tag
   */
  test('should fail on invalid verifier key update', () => {
    const contractOperation = new ContractOperation();

    expect(() => {
      contractOperation.verifierKey = new Uint8Array(1024);
    }).toThrow(/tagged data does not begin with/);
    expect(() => {
      contractOperation.verifierKey = TestResource.taggedVerifierKey(9 as never);
    }).toThrow("unknown verifier key tag: 'verifier-key[v9]'");
  });

  /**
   * Test that a verifier key of every version is taken.
   *
   * @given A ContractOperation instance
   * @when Setting a verifier key of each version, oldest first
   * @then Should return the newest key held, byte for byte
   */
  test('should take a verifier key of each version and return the newest', () => {
    const contractOperation = new ContractOperation();

    ([6, 7, 8] as const).forEach((keyVersion) => {
      const key = TestResource.taggedVerifierKey(keyVersion);
      contractOperation.verifierKey = key;
      expect(contractOperation.verifierKey).toEqual(key);
    });
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
