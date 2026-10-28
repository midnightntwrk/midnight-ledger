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

import { ContractOperationVersionedVerifierKey } from '@midnightntwrk/ledger';
import { TestResource } from '@/test-objects';

describe('Ledger API - ContractOperationVersionedVerifierKey', () => {
  /**
   * Test constructor functionality.
   *
   * @given A version string 'v3' and an operation verifier key
   * @when Creating a ContractOperationVersionedVerifierKey
   * @then Should store version and verifier key correctly and format string representation
   */
  test('should construct with version and verifier key', () => {
    const contractOperationVersionedVerifierKey = new ContractOperationVersionedVerifierKey(
      'v3',
      TestResource.operationVerifierKey()
    );

    expect(contractOperationVersionedVerifierKey.version).toEqual('v3');
    expect(contractOperationVersionedVerifierKey.toString(true)).toMatch(/V3\(VerifierKey\(.*/);
  });

  /**
   * @given A version string 'v4' and a verifier-key[v8]
   * @when Creating a ContractOperationVersionedVerifierKey
   * @then Should store the version and format as V4
   */
  test('should construct v4 from a verifier-key[v8]', () => {
    const v8Key = TestResource.circuit('noop')!.verifierKey;
    const versionedKey = new ContractOperationVersionedVerifierKey('v4', v8Key);

    expect(versionedKey.version).toEqual('v4');
    expect(versionedKey.toString(true)).toMatch(/^V4\(VerifierKey\(.*/);
  });

  /**
   * @given A version string 'v4' and a verifier-key[v6]
   * @when Creating a ContractOperationVersionedVerifierKey
   * @then Should throw about the expected verifier-key[v8] tag
   */
  test('should reject a verifier-key[v6] as v4', () => {
    expect(() => new ContractOperationVersionedVerifierKey('v4', TestResource.operationVerifierKey())).toThrow(
      /expected header tag 'midnight:verifier-key\[v8\]:'/
    );
  });

  /**
   * @given A version string 'v3' and a verifier-key[v8]
   * @when Creating a ContractOperationVersionedVerifierKey
   * @then Should throw
   */
  test('should reject a verifier-key[v8] as v3', () => {
    expect(() => new ContractOperationVersionedVerifierKey('v3', TestResource.circuit('noop')!.verifierKey)).toThrow();
  });
});
