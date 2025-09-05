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

import { ContractOperationVersion, VerifierKeyRemove } from '@midnight-ntwrk/ledger';

describe('Ledger API - VerifierKeyRemove', () => {
  /**
   * Test constructor functionality.
   *
   * @given An operation name and a contract operation version
   * @when Creating a VerifierKeyRemove
   * @then Should store operation and version correctly
   */
  test('should construct with operation and version', () => {
    const operation = 'test_operation';
    const version = new ContractOperationVersion('v2');

    const verifierKeyRemove = new VerifierKeyRemove(operation, version);

    expect(verifierKeyRemove.version.toString()).toEqual(version.toString());
    expect(verifierKeyRemove.operation).toEqual(operation);
    expect(verifierKeyRemove.toString()).toEqual(operation);
  });
});
