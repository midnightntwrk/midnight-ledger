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

import { ContractDeploy, ContractOperation, ContractState } from '@midnight-ntwrk/ledger';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - ContractDeploy', () => {
  /**
   * Test creation of ContractDeploy with basic contract state.
   *
   * @given A basic ContractState
   * @when Creating a ContractDeploy
   * @then Should generate valid address and maintain state integrity
   */
  test('should create contract deploy', () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);

    expect(contractDeploy.address).toMatch(/[a-fA-F0-9]{64}/);
    // Nb. This is *not* pointer-equal, but *is* semantically equal.
    expect(contractDeploy.initialState).not.toEqual(contractState);
    expect(contractDeploy.initialState.serialize()).toEqual(contractState.serialize());
    expect(contractState.toString().length).toBeGreaterThan(0);
    expect(contractDeploy.toString().length).toBeGreaterThan(0);
    assertSerializationSuccess(contractState);
  });

  /**
   * Test creation of ContractDeploy with multiple operations.
   *
   * @given A ContractState with three operations (op1, op2, op3)
   * @when Creating a ContractDeploy
   * @then Should generate valid address and properly serialize state with all operations
   */
  test('should create contract deploy with state with 3 operations', () => {
    const contractState = new ContractState();
    contractState.setOperation('op1', new ContractOperation());
    contractState.setOperation('op2', new ContractOperation());
    contractState.setOperation('op3', new ContractOperation());
    const contractDeploy = new ContractDeploy(contractState);

    expect(contractDeploy.address).toMatch(/[a-fA-F0-9]{64}/);
    // Nb. This is *not* pointer-equal, but *is* semantically equal.
    expect(contractDeploy.initialState).not.toEqual(contractState);
    expect(contractDeploy.initialState.serialize()).toEqual(contractState.serialize());
    expect(contractState.toString().length).toBeGreaterThan(0);
    expect(contractDeploy.toString().length).toBeGreaterThan(0);
    assertSerializationSuccess(contractState);
  });
});
