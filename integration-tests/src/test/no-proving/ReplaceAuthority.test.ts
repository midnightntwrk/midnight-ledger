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

import { ReplaceAuthority, ContractMaintenanceAuthority, signatureVerifyingKey } from '@midnight-ntwrk/ledger';
import { Random } from '@/test-objects';

describe('Ledger API - ReplaceAuthority', () => {
  /**
   * Test creation with undefined counter.
   *
   * @given A ContractMaintenanceAuthority with undefined counter
   * @when Creating a ReplaceAuthority
   * @then Should store authority correctly and have matching string representation
   */
  test('should handle undefined counter', () => {
    const newAuthority = Random.signingKey();
    const svk = signatureVerifyingKey(newAuthority);
    const contractMaintenanceAuthority = new ContractMaintenanceAuthority([svk], 1, undefined);
    const replaceAuthority = new ReplaceAuthority(contractMaintenanceAuthority);

    expect(replaceAuthority.authority.toString()).toEqual(contractMaintenanceAuthority.toString());
    expect(replaceAuthority.toString()).toEqual(contractMaintenanceAuthority.toString());
  });
});
