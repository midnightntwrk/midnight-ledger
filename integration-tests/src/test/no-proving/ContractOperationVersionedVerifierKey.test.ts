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

import { ContractOperationVersionedVerifierKey } from '@midnight-ntwrk/ledger';
import { TestResource } from '@/test-objects';

describe('Ledger API - ContractOperationVersionedVerifierKey', () => {
  /**
   * Test constructor functionality.
   *
   * @given A version string 'v2' and an operation verifier key
   * @when Creating a ContractOperationVersionedVerifierKey
   * @then Should store version and verifier key correctly and format string representation
   */
  test('should construct with version and verifier key', () => {
    const contractOperationVersionedVerifierKey = new ContractOperationVersionedVerifierKey(
      'v2',
      TestResource.operationVerifierKey()
    );

    expect(contractOperationVersionedVerifierKey.version).toEqual('v2');
    expect(contractOperationVersionedVerifierKey.toString(true)).toMatch(/V2\(VerifierKey\(.*/);
  });
});
