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

import {
  ContractOperationVersion,
  ContractOperationVersionedVerifierKey,
  MaintenanceUpdate,
  sampleContractAddress,
  sampleSigningKey,
  signData,
  VerifierKeyInsert,
  VerifierKeyRemove
} from '@midnight-ntwrk/ledger';
import { Random } from '@/test-objects';
import { SignatureKindMarker } from '@/test/utils/Markers';

describe('Ledger API - MaintenanceUpdate', () => {
  /**
   * Test removal of verifier key.
   *
   * @given A contract address and a VerifierKeyRemove operation
   * @when Creating a MaintenanceUpdate
   * @then Should successfully create the update object
   */
  test('should remove verifier key', () => {
    const maintenanceUpdate = new MaintenanceUpdate(
      Random.contractAddress(),
      [new VerifierKeyRemove('operation', new ContractOperationVersion('v3'))],
      0n
    );

    expect(maintenanceUpdate).not.toBeNull();
  });

  /**
   * Test string representation of MaintenanceUpdate.
   *
   * @given A MaintenanceUpdate with a VerifierKeyRemove operation
   * @when Calling toString method
   * @then Should return a string matching the MaintenanceUpdate pattern
   */
  test('should return a string representation', () => {
    const maintenanceUpdate = new MaintenanceUpdate(
      Random.contractAddress(),
      [new VerifierKeyRemove('operation', new ContractOperationVersion('v3'))],
      0n
    );

    expect(maintenanceUpdate.toString()).toMatch(/MaintenanceUpdate .*/);
  });

  /**
   * Test adding multiple signatures to MaintenanceUpdate.
   *
   * @given A MaintenanceUpdate and multiple signed data
   * @when Adding signatures with different indices
   * @then Should store signatures correctly and maintain update properties
   */
  test('should add multiple signatures', () => {
    const contractAddress = Random.contractAddress();
    const operation = 'test_operation';
    const sk1 = signData(sampleSigningKey(), new Uint8Array(32));
    const sk2 = signData(sampleSigningKey(), new Uint8Array(64));
    let maintenanceUpdate = new MaintenanceUpdate(
      contractAddress,
      [new VerifierKeyRemove(operation, new ContractOperationVersion('v3'))],
      0n
    );
    maintenanceUpdate = maintenanceUpdate.addSignature(0n, sk1);
    maintenanceUpdate = maintenanceUpdate.addSignature(1n, sk2);

    expect(maintenanceUpdate.signatures).toHaveLength(2);
    expect(maintenanceUpdate.signatures[0]?.at(1)).toEqual(sk1);
    expect(maintenanceUpdate.signatures[1]?.at(1)).toEqual(sk2);
    expect(maintenanceUpdate.address).toEqual(contractAddress);
    expect(maintenanceUpdate.counter).toEqual(0n);
    expect(maintenanceUpdate.dataToSign.length).toBeGreaterThan(0);
    expect(maintenanceUpdate.updates.toString()).toEqual(operation);
  });

  /**
   * Test failure on empty verifier key insertion.
   *
   * @given A contract address and VerifierKeyInsert with empty verifier key
   * @when Creating a MaintenanceUpdate
   * @then Should throw invalid input data error with version information
   */
  test('should fail on empty verifier key insertion', () => {
    expect(
      () =>
        new MaintenanceUpdate(
          Random.contractAddress(),
          [new VerifierKeyInsert('operation', new ContractOperationVersionedVerifierKey('v3', new Uint8Array(1024)))],
          0n
        )
    ).toThrow(/expected header tag 'midnight:verifier-key/);
  });

  /**
   * Test failure on verifier key with invalid length.
   *
   * @given A contract address and VerifierKeyInsert with zero-length verifier key
   * @when Creating a MaintenanceUpdate
   * @then Should throw 'Unexpected length of input' error
   */
  test('should fail on insert verifier key of invalid length', () => {
    expect(
      () =>
        new MaintenanceUpdate(
          Random.contractAddress(),
          [new VerifierKeyInsert('operation', new ContractOperationVersionedVerifierKey('v3', new Uint8Array(0)))],
          0n
        )
    ).toThrow(/expected header tag 'midnight:verifier-key/);
  });

  /**
   * Test creation with multiple operations.
   *
   * @given A contract address and multiple VerifierKeyRemove operations
   * @when Creating a MaintenanceUpdate
   * @then Should successfully create the update with all operations
   */
  test('should create MaintenanceUpdate with multiple operations', () => {
    const maintenanceUpdate = new MaintenanceUpdate(
      Random.contractAddress(),
      [
        new VerifierKeyRemove('operation1', new ContractOperationVersion('v3')),
        new VerifierKeyRemove('operation2', new ContractOperationVersion('v3')),
        new VerifierKeyRemove('operation3', new ContractOperationVersion('v3'))
      ],
      0n
    );

    expect(maintenanceUpdate).not.toBeNull();
  });

  describe('ECDSA signature kind', () => {
    /**
     * Test addSignature with an ECDSA signature.
     *
     * @given A MaintenanceUpdate and an ECDSA signature
     * @when Calling addSignature with the ECDSA signature at index 0
     * @then The getter should report the ECDSA signature with tag intact
     */
    test('addSignature accepts an ECDSA-tagged signature and round-trips through the getter', () => {
      const contractAddress = Random.contractAddress();
      const ecdsaSig = signData(sampleSigningKey(SignatureKindMarker.ecdsa), new Uint8Array(32));

      let update = new MaintenanceUpdate(
        contractAddress,
        [new VerifierKeyRemove('op', new ContractOperationVersion('v3'))],
        0n
      );
      update = update.addSignature(0n, ecdsaSig);

      expect(update.signatures).toHaveLength(1);
      expect(update.signatures[0]?.at(1)).toEqual(ecdsaSig);
      expect((update.signatures[0]?.at(1) as { tag: string }).tag).toEqual(SignatureKindMarker.ecdsa);
    });

    /**
     * Pin: signature kinds at different indices are independent of each
     * other; mixing them is allowed.
     *
     * @given A MaintenanceUpdate with a schnorr sig at index 0 and an ecdsa sig at index 1
     * @when Reading the signatures back via the getter
     * @then Each index must report its original tag
     */
    test('preserves mixed schnorr/ecdsa signatures across indices', () => {
      const contractAddress = sampleContractAddress();
      const schnorrSig = signData(sampleSigningKey(SignatureKindMarker.schnorr), new Uint8Array(32));
      const ecdsaSig = signData(sampleSigningKey(SignatureKindMarker.ecdsa), new Uint8Array(32));

      let update = new MaintenanceUpdate(
        contractAddress,
        [new VerifierKeyRemove('op', new ContractOperationVersion('v3'))],
        0n
      );
      update = update.addSignature(0n, schnorrSig);
      update = update.addSignature(1n, ecdsaSig);

      expect(update.signatures).toHaveLength(2);
      expect((update.signatures[0]?.at(1) as { tag: string }).tag).toEqual(SignatureKindMarker.schnorr);
      expect((update.signatures[1]?.at(1) as { tag: string }).tag).toEqual(SignatureKindMarker.ecdsa);
    });

    /**
     * MaintenanceUpdate has no direct serialize() - use toString as a structural-equality proxy.
     *
     * @given Two MaintenanceUpdates built identically with the same mixed schnorr+ecdsa signatures at the same indices
     * @when Comparing their toString outputs
     * @then They must be byte-equal
     */
    test('toString stable for a maintenance update carrying both signature kinds', () => {
      const contractAddress = sampleContractAddress();
      const schnorrSig = signData(sampleSigningKey(SignatureKindMarker.schnorr), new Uint8Array(32));
      const ecdsaSig = signData(sampleSigningKey(SignatureKindMarker.ecdsa), new Uint8Array(32));

      let update = new MaintenanceUpdate(
        contractAddress,
        [new VerifierKeyRemove('op', new ContractOperationVersion('v3'))],
        0n
      );
      update = update.addSignature(0n, schnorrSig);
      update = update.addSignature(1n, ecdsaSig);

      let rebuilt = new MaintenanceUpdate(
        contractAddress,
        [new VerifierKeyRemove('op', new ContractOperationVersion('v3'))],
        0n
      );
      rebuilt = rebuilt.addSignature(0n, schnorrSig);
      rebuilt = rebuilt.addSignature(1n, ecdsaSig);

      expect(rebuilt.toString()).toEqual(update.toString());
    });
  });
});
