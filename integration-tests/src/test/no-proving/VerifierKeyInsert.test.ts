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

import { ContractOperationVersionedVerifierKey, VerifierKeyInsert } from '@midnight-ntwrk/ledger';
import { TestResource } from '@/test-objects';

describe('Ledger API - VerifierKeyInsert', () => {
  /**
   * Test constructor functionality.
   *
   * @given An operation name and a versioned verifier key
   * @when Creating a VerifierKeyInsert
   * @then Should store operation and verifier key correctly
   */
  test('should construct with operation and verifier key', () => {
    const operation = 'test_operation';
    const verifierKey = new ContractOperationVersionedVerifierKey('v2', TestResource.operationVerifierKey());

    const verifierKeyInsert = new VerifierKeyInsert(operation, verifierKey);

    expect(verifierKeyInsert.operation).toEqual(operation);
    expect(verifierKeyInsert.vk.toString()).toEqual(verifierKey.toString());
    expect(verifierKeyInsert.toString()).toEqual(operation);
  });
});
