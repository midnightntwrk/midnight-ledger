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
  ChargedState,
  ContractMaintenanceAuthority,
  ContractOperation,
  ContractState,
  CostModel,
  type EncodedStateValue,
  type LogEventType,
  type Op,
  shieldedToken,
  signatureVerifyingKey,
  StateBoundedMerkleTree,
  StateValue
} from '@midnight-ntwrk/ledger';
import { Random } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { arrayCell, intCell } from '@/test/utils/value-alignment';
import { LogEventTypeMarker } from '@/test/utils/Markers';

describe('Ledger API - ContractState', () => {
  /**
   * Test returning list of operations.
   *
   * @given A ContractState with a single operation set
   * @when Retrieving the operation and listing all operations
   * @then Should return correct operation details and operation list
   */
  test('should return list of operations', () => {
    const OPERATION_NAME = 'abcdef';
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractState.setOperation(OPERATION_NAME, contractOperation);
    const operation = contractState.operation(OPERATION_NAME);

    expect(operation?.verifierKey).toEqual(contractOperation.verifierKey);
    expect(contractState.operations()).toEqual([OPERATION_NAME]);
    expect(contractState.toString()).toMatch(/ContractState.*/);
  });

  /**
   * Test the balance accessor.
   *
   * @given A new contract state
   * @when Assigning a token balance map
   * @then The balance should be readable back
   */
  test('balance can be set and read', () => {
    const contractState = new ContractState();

    contractState.balance = new Map([[shieldedToken(), 5n]]);

    expect(contractState.balance.size).toEqual(1);
  });

  /**
   * Test query functionality.
   *
   * @given A ContractState and a initial cost model
   * @when Querying with a noop operation
   * @then Should return gather result with expected properties
   */
  test.fails('should pass query operation', () => {
    const contractState = new ContractState();
    const gatherResult = contractState.query([{ noop: { n: 15 } }], CostModel.initialCostModel());

    expect(gatherResult).toHaveLength(1);
    expect(gatherResult.at(0)?.toString()).toEqual(contractState.toString());
    expect(gatherResult.at(0)?.tag).toBeUndefined();
    expect(gatherResult.at(0)?.content).toBeUndefined();
  });

  /**
   * Test query error handling for invalid input.
   *
   * @given A ContractState and a initial cost model
   * @when Querying with a non-cell array
   * @then Should throw 'expected a cell' error
   */
  test('should fail query if not a cell', () => {
    const contractState = new ContractState();

    expect(() => contractState.query(['new'], CostModel.initialCostModel())).toThrow('expected a cell');
  });

  /**
   * Test handling of empty operation name.
   *
   * @given A ContractState with an empty string operation name
   * @when Setting and retrieving the operation
   * @then Should handle empty name correctly and serialize successfully
   */
  test('should handle empty operation name', () => {
    const OPERATION_NAME = '';
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractState.setOperation(OPERATION_NAME, contractOperation);
    const operation = contractState.operation(OPERATION_NAME);

    expect(operation?.verifierKey).toEqual(contractOperation.verifierKey);
    expect(contractState.operations()).toEqual([OPERATION_NAME]);
    assertSerializationSuccess(contractState);
  });

  /**
   * Test handling of Uint8Array operation name.
   *
   * @given A ContractState with a Uint8Array operation name
   * @when Setting and retrieving the operation
   * @then Should handle Uint8Array name correctly and serialize successfully
   */
  test('should handle Uint8Array operation name', () => {
    const OPERATION_NAME = new Uint8Array();
    const contractState = new ContractState();
    const contractOperation = new ContractOperation();
    contractState.setOperation(OPERATION_NAME, contractOperation);
    const operation = contractState.operation(OPERATION_NAME);

    expect(operation).toBeDefined();
    assertSerializationSuccess(contractState);
  });

  /**
   * Test handling of multiple operations.
   *
   * @given A ContractState with two different operations
   * @when Setting and retrieving both operations
   * @then Should handle both operations correctly and list them appropriately
   */
  test('should handle multiple operations', () => {
    const OPERATION_NAME_1 = 'op1';
    const OPERATION_NAME_2 = 'op2';
    const contractState = new ContractState();
    const contractOperation1 = new ContractOperation();
    const contractOperation2 = new ContractOperation();
    contractState.setOperation(OPERATION_NAME_1, contractOperation1);
    contractState.setOperation(OPERATION_NAME_2, contractOperation2);

    const operation1 = contractState.operation(OPERATION_NAME_1);
    const operation2 = contractState.operation(OPERATION_NAME_2);

    expect(operation1?.verifierKey).toEqual(contractOperation1.verifierKey);
    expect(operation2?.verifierKey).toEqual(contractOperation2.verifierKey);
    expect(contractState.operations().sort()).toEqual([OPERATION_NAME_1, OPERATION_NAME_2].sort());
    assertSerializationSuccess(contractState);
  });

  /**
   * Test query with empty array.
   *
   * @given A ContractState and a initial cost model
   * @when Querying with an empty array
   * @then Should return gather result with one element
   */
  test.fails('should handle query with empty array', () => {
    const contractState = new ContractState();
    const gatherResult = contractState.query([], CostModel.initialCostModel());

    expect(gatherResult).toHaveLength(1);
  });

  /**
   * Test setting contract state data.
   *
   * @given A ContractState and a null StateValue
   * @when Setting the data property
   * @then Should store data correctly and serialize successfully
   */
  test('should set contract state data', () => {
    const contractState = new ContractState();
    contractState.data = new ChargedState(StateValue.newNull());

    expect(contractState.data.toString()).toEqual(StateValue.newNull().toString());
    assertSerializationSuccess(contractState);
  });

  /**
   * Test setting contract state authority.
   *
   * @given A ContractState and a ContractMaintenanceAuthority
   * @when Setting the maintenanceAuthority property
   * @then Should store authority correctly and serialize successfully
   */
  test('should set contract state authority', () => {
    const newAuthority = Random.signingKey();
    const svk = signatureVerifyingKey(newAuthority);
    const contractMaintenanceAuthority = new ContractMaintenanceAuthority([svk], 1, undefined);
    const contractState = new ContractState();
    contractState.maintenanceAuthority = contractMaintenanceAuthority;

    expect(contractState.maintenanceAuthority.toString()).toEqual(contractMaintenanceAuthority.toString());
    assertSerializationSuccess(contractState);
  });

  describe('query - log events', () => {
    type LogContent = { version: number; eventType: LogEventType; data: EncodedStateValue };
    type GatherLog = { tag: 'log'; content: LogContent };

    // `query` returns [ContractState, GatherResult[]] despite the d.ts type.
    const events = (result: unknown): GatherLog[] => (result as [unknown, GatherLog[]])[1];

    const logProgram = (value: EncodedStateValue): Op<null>[] => [{ push: { storage: false, value } }, 'log'];

    const versionedTuple = (version: bigint, type: bigint, payload: EncodedStateValue): EncodedStateValue =>
      arrayCell([intCell(version, 4), intCell(type, 1), payload]);

    /**
     * Test that a bare cell value falls back to version 0 with eventType
     * `misc` when queried through `ContractState.query`
     *
     * @given A bare u64 cell pushed as the log payload
     * @when Querying with a program ending in `log`
     * @then The gathered event is `{version: 0, eventType: 'misc', data: <cell>}`
     */
    test('bare cell value falls back to version 0 / misc with the value as data', () => {
      const value = intCell(4n, 8);
      const logs = events(new ContractState().query(logProgram(value), CostModel.initialCostModel()));

      expect(logs).toHaveLength(1);
      expect(logs[0].tag).toBe('log');
      expect(logs[0].content.version).toBe(0);
      expect(logs[0].content.eventType).toBe(LogEventTypeMarker.misc);
      expect(logs[0].content.data).toEqual(value);
    });

    /**
     * Test that a well-formed [u32 version, u8 type, payload] tuple decodes
     * into the structured versioned event
     *
     * @given A 3-element tuple (version=2, type=8 Paused, payload=u64(4))
     * @when Querying with a program ending in `log`
     * @then The gathered event is `{version: 2, eventType: 'paused', data: <payload>}`
     */
    test('well-formed tuple decodes to versioned event', () => {
      const payload = intCell(4n, 8);
      const value = versionedTuple(2n, 8n, payload);
      const logs = events(new ContractState().query(logProgram(value), CostModel.initialCostModel()));

      expect(logs).toHaveLength(1);
      expect(logs[0].content.version).toBe(2);
      expect(logs[0].content.eventType).toBe(LogEventTypeMarker.paused);
      expect(logs[0].content.data).toEqual(payload);
    });

    /**
     * Test that an out-of-range event-type byte (>= 11) is rejected by the
     * decoder - the whole array surfaces as the misc payload.
     *
     * @given A tuple with type=11 (no such LogEventType)
     * @when Querying with a program ending in `log`
     * @then The event is `{version: 0, eventType: 'misc', data: <array>}`
     */
    test('out-of-range event type (>= 11) falls back to misc with the whole array as data', () => {
      const malformed = versionedTuple(0n, 11n, intCell(1n, 8));
      const logs = events(new ContractState().query(logProgram(malformed), CostModel.initialCostModel()));

      expect(logs).toHaveLength(1);
      expect(logs[0].content.version).toBe(0);
      expect(logs[0].content.eventType).toBe(LogEventTypeMarker.misc);
      expect(logs[0].content.data).toEqual(malformed);
    });

    /**
     * Test that a wrong-arity array (2 elements) is not decoded as a
     * versioned tuple and instead surfaces as a misc payload
     *
     * @given A 2-element array
     * @when Querying with a program ending in `log`
     * @then The event is `{version: 0, eventType: 'misc', data: <array>}`
     */
    test('wrong-arity array (2 elements) falls back to misc with the array as data', () => {
      const malformed = arrayCell([intCell(0n, 4), intCell(0n, 1)]);
      const logs = events(new ContractState().query(logProgram(malformed), CostModel.initialCostModel()));

      expect(logs).toHaveLength(1);
      expect(logs[0].content.version).toBe(0);
      expect(logs[0].content.eventType).toBe(LogEventTypeMarker.misc);
      expect(logs[0].content.data).toEqual(malformed);
    });

    /**
     * Test that a StateValue which is neither an array nor a cell (e.g. a
     * BoundedMerkleTree) falls back to the misc path
     *
     * @given A BoundedMerkleTree encoded as the log payload
     * @when Querying with a program ending in `log`
     * @then The event is `{version: 0, eventType: 'misc', data: <value>}`
     */
    test('non-array, non-cell value (BoundedMerkleTree) falls back to misc', () => {
      const value = StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(1)).encode();
      const logs = events(new ContractState().query(logProgram(value), CostModel.initialCostModel()));

      expect(logs).toHaveLength(1);
      expect(logs[0].content.version).toBe(0);
      expect(logs[0].content.eventType).toBe(LogEventTypeMarker.misc);
      expect(logs[0].content.data).toEqual(value);
    });
  });
});
