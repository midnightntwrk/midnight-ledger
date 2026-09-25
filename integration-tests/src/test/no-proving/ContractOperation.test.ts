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
import { assertSerializationSuccess } from '@/test-utils';
import { TestResource } from '@/test-objects';

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
    }).toThrow(/tagged data does not begin with/);
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

  describe('verifier key versions', () => {
    const v6Key = TestResource.operationVerifierKey();
    const v8Key = TestResource.circuit('noop')!.verifierKey;

    const V6_TAG = 'midnight:verifier-key[v6]:';
    const retagged = (key: Uint8Array, tag: string): Uint8Array =>
      Buffer.concat([Buffer.from(`midnight:${tag}:`, 'latin1'), key.subarray(V6_TAG.length)]);

    /**
     * @given A verifier-key[v8] from a zkir-v3 circuit
     * @when Setting it as the operation verifier key
     * @then Should read back the same bytes
     */
    test('should accept a verifier-key[v8] and expose it unchanged', () => {
      const contractOperation = new ContractOperation();
      contractOperation.verifierKey = v8Key;

      expect(contractOperation.verifierKey).toEqual(v8Key);
    });

    /**
     * @given An operation holding a v8 key
     * @when Setting a v6 key as well
     * @then Should still expose the v8 key as the latest version
     */
    test('should expose the v8 key over a v6 one, whichever was set last', () => {
      const contractOperation = new ContractOperation();
      contractOperation.verifierKey = v8Key;
      contractOperation.verifierKey = v6Key;

      expect(contractOperation.verifierKey).toEqual(v8Key);
    });

    /**
     * @given An operation holding a v8 key
     * @when Serializing and deserializing it
     * @then Should keep the key and string representation
     */
    test('should serialize and deserialize with a v8 key', () => {
      const contractOperation = new ContractOperation();
      contractOperation.verifierKey = v8Key;

      const deserialized = ContractOperation.deserialize(contractOperation.serialize());

      expect(deserialized.verifierKey).toEqual(v8Key);
      expect(deserialized.toString()).toEqual(contractOperation.toString());
    });

    /**
     * @given A key retagged as verifier-key[v7]
     * @when Setting it as the operation verifier key
     * @then Should throw an error naming the rejected tag
     */
    test('should reject the retired verifier-key[v7], naming the tag', () => {
      const contractOperation = new ContractOperation();

      expect(() => {
        contractOperation.verifierKey = retagged(v6Key, 'verifier-key[v7]');
      }).toThrow("unknown verifier key tag: 'verifier-key[v7]'");
    });
  });
});
