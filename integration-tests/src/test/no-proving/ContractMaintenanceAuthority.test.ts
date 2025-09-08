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

import { ContractMaintenanceAuthority, signatureVerifyingKey } from '@midnight-ntwrk/ledger';
import { Random } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - ContractMaintenanceAuthority', () => {
  /**
   * Test that maximum counter value throws an error.
   *
   * @given A signing key and counter value set to MAX_SAFE_INTEGER
   * @when Creating a ContractMaintenanceAuthority
   * @then Should throw 'counter out of range' error
   */
  test('should throw error when counter exceeds maximum value', () => {
    const newAuthority = Random.signingKey();
    const svk = signatureVerifyingKey(newAuthority);

    expect(() => new ContractMaintenanceAuthority([svk], 1, BigInt(Number.MAX_SAFE_INTEGER))).toThrow(
      'counter out of range'
    );
  });

  /**
   * Test creation with undefined counter.
   *
   * @given A signing key and undefined counter
   * @when Creating a ContractMaintenanceAuthority
   * @then Should default counter to 0 and maintain other properties correctly
   */
  test('should default counter to 0 when undefined', () => {
    const newAuthority = Random.signingKey();
    const svk = signatureVerifyingKey(newAuthority);
    const contractMaintenanceAuthority = new ContractMaintenanceAuthority([svk], 1, undefined);

    expect(contractMaintenanceAuthority.counter).toEqual(0n);
    expect(contractMaintenanceAuthority.threshold).toEqual(1);
    expect(contractMaintenanceAuthority.committee.at(0)).toEqual(svk);
    expect(contractMaintenanceAuthority.toString()).toMatch(/ContractMaintenanceAuthority.*/);
    assertSerializationSuccess(contractMaintenanceAuthority);
  });

  /**
   * Test basic constructor functionality.
   *
   * @given A signing key, threshold value, and counter value
   * @when Creating a ContractMaintenanceAuthority
   * @then Should initialize all properties correctly
   */
  test('should construct with valid properties', () => {
    const authority = Random.signingKey();
    const svk = signatureVerifyingKey(authority);
    const contractMaintenanceAuthority = new ContractMaintenanceAuthority([svk], 1, 0n);

    expect(contractMaintenanceAuthority.counter).toEqual(0n);
    expect(contractMaintenanceAuthority.threshold).toEqual(1);
    expect(contractMaintenanceAuthority.committee.at(0)).toEqual(svk);
    expect(contractMaintenanceAuthority.toString()).toMatch(/ContractMaintenanceAuthority.*/);
  });

  /**
   * Test serialization and deserialization.
   *
   * @given A ContractMaintenanceAuthority with valid properties
   * @when Serializing and then deserializing the object
   * @then Should maintain object integrity and equality
   */
  test('should serialize and deserialize correctly', () => {
    const authority = Random.signingKey();
    const svk = signatureVerifyingKey(authority);
    const contractMaintenanceAuthority = new ContractMaintenanceAuthority([svk], 1, 0n);

    expect(ContractMaintenanceAuthority.deserialize(contractMaintenanceAuthority.serialize()).toString()).toEqual(
      contractMaintenanceAuthority.toString()
    );
  });
});
