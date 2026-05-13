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

import { ContractMaintenanceAuthority, sampleSigningKey, signatureVerifyingKey } from '@midnight-ntwrk/ledger';
import { Random } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { SignatureKindMarker } from '@/test/utils/Markers';

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

  describe('ECDSA signature kind', () => {
    /**
     * Test construction with a homogeneous all-ECDSA committee.
     *
     * @given Two ECDSA verifying keys and a threshold of 2
     * @when Constructing a ContractMaintenanceAuthority
     * @then committee, threshold, and tags must round-trip through the getter
     */
    test('constructs with an all-ECDSA committee', () => {
      const vk1 = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.ecdsa));
      const vk2 = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.ecdsa));
      const cma = new ContractMaintenanceAuthority([vk1, vk2], 2, 0n);

      expect(cma.committee.length).toEqual(2);
      expect(cma.committee.at(0)?.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(cma.committee.at(1)?.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(cma.committee.at(0)).toEqual(vk1);
      expect(cma.committee.at(1)).toEqual(vk2);
      expect(cma.threshold).toEqual(2);
    });

    /**
     * Pin: the API doesn't enforce committee homogeneity. A CMA can mix
     * Schnorr and ECDSA verifying keys; downstream signature verification
     * is index-aligned and dispatches per member kind.
     *
     * @given A mixed [schnorr, ecdsa] committee
     * @when Reading committee back via the getter
     * @then Order and each member's tag must be preserved
     */
    test('preserves mixed schnorr/ecdsa committee order through the getter', () => {
      const schnorrVk = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.schnorr));
      const ecdsaVk = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.ecdsa));

      const cma = new ContractMaintenanceAuthority([schnorrVk, ecdsaVk], 2, 0n);

      expect(cma.committee.length).toEqual(2);
      expect(cma.committee.at(0)?.tag).toEqual(SignatureKindMarker.schnorr);
      expect(cma.committee.at(1)?.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(cma.committee.at(0)).toEqual(schnorrVk);
      expect(cma.committee.at(1)).toEqual(ecdsaVk);
    });

    /**
     * Test that the v2 wire format preserves committee kinds and order.
     *
     * @given A 3-member [schnorr, ecdsa, ecdsa] committee
     * @when Serialising and deserialising the CMA
     * @then The decoded committee must equal the original in order and kind
     */
    test('serialization round-trip preserves committee kinds and order', () => {
      const schnorrVk = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.schnorr));
      const ecdsaVk1 = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.ecdsa));
      const ecdsaVk2 = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.ecdsa));

      const cma = new ContractMaintenanceAuthority([schnorrVk, ecdsaVk1, ecdsaVk2], 2, 0n);
      const round = ContractMaintenanceAuthority.deserialize(cma.serialize());

      expect(round.committee.length).toEqual(3);
      expect(round.committee.at(0)).toEqual(schnorrVk);
      expect(round.committee.at(1)).toEqual(ecdsaVk1);
      expect(round.committee.at(2)).toEqual(ecdsaVk2);
      expect(round.toString()).toEqual(cma.toString());
      assertSerializationSuccess(cma);
    });
  });
});
