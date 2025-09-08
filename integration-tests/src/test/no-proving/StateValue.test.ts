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

import { StateBoundedMerkleTree, StateMap, StateValue } from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';

describe('Ledger API - StateValue', () => {
  /**
   * Test creating StateValue with Map.
   *
   * @given A StateMap with a null value inserted
   * @when Creating a StateValue with the map
   * @then Should access the map value correctly
   */
  test('should create state with Map', () => {
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());
    const stateValue = StateValue.newMap(stateMap);

    expect(stateValue.asMap()?.get(Static.alignedValue)?.toString()).toEqual('null');
  });

  /**
   * Test creating StateValue with Array.
   *
   * @given An empty StateValue array with a null value pushed
   * @when Accessing the array
   * @then Should return the null value as string
   */
  test('should create state with Array', () => {
    let stateValue = StateValue.newArray();
    stateValue = stateValue.arrayPush(StateValue.newNull());

    expect(stateValue.asArray()?.toString()).toEqual('null');
  });

  /**
   * Test creating StateValue with Cell.
   *
   * @given A static aligned value
   * @when Creating a StateValue cell
   * @then Should preserve value and alignment properties
   */
  test('should create state with Cell', () => {
    const stateValue = StateValue.newCell(Static.alignedValue);

    expect(stateValue.asCell()?.value).toEqual(Static.alignedValue.value);
    expect(stateValue.asCell()?.alignment).toEqual(Static.alignedValue.alignment);
  });

  /**
   * Test creating StateValue with BoundedMerkleTree (currently failing).
   *
   * @given A StateBoundedMerkleTree with MAX_SAFE_INTEGER height
   * @when Creating a StateValue with the tree
   * @then Should have correct log size and height
   */
  test.fails('should create state with BoundedMerkleTree', () => {
    const stateValue = StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(Number.MAX_SAFE_INTEGER));

    expect(stateValue.logSize()).toEqual(1);
    expect(stateValue.asBoundedMerkleTree()?.height).toEqual(Number.MAX_SAFE_INTEGER);
  });

  /**
   * Test creating StateValue with BoundedMerkleTree of defined height.
   *
   * @given A StateBoundedMerkleTree with height 2
   * @when Creating a StateValue with the tree
   * @then Should have correct log size and height
   */
  test('should create state with BoundedMerkleTree of defined height', () => {
    const stateValue = StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(2));

    expect(stateValue.logSize()).toEqual(2);
    expect(stateValue.asBoundedMerkleTree()?.height).toEqual(2);
  });

  /**
   * Test encoding and decoding of different StateValue types.
   *
   * @given Various StateValue types (null, array, map, cell, boundedMerkleTree)
   * @when Encoding and then decoding each type
   * @then Should maintain identical string representation after round-trip
   */
  test.each([
    ['null', StateValue.newNull()],
    ['array', StateValue.newArray()],
    ['map', StateValue.newMap(new StateMap())],
    ['map', StateValue.newMap(new StateMap().insert(Static.alignedValue, StateValue.newNull()))],
    ['cell', StateValue.newCell(Static.alignedValue)],
    ['boundedMerkleTree', StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(4))]
  ])('should encode and decode state - %s', (encodedTag, stateValue) => {
    const encodedStateValue = stateValue.encode();
    const decodedValue = StateValue.decode(encodedStateValue);

    expect(encodedStateValue.tag).toEqual(encodedTag);
    expect(stateValue.toString()).toEqual(decodedValue.toString());
  });

  /**
   * Test array length limitation.
   *
   * @given A StateValue array with 15 null elements
   * @when Attempting to push another element
   * @then Should throw error about exceeding 15 elements limit
   */
  test('should limit array length', () => {
    let stateValue = StateValue.newArray();

    for (let i = 0; i < 15; i += 1) {
      stateValue = stateValue.arrayPush(StateValue.newNull());
    }

    expect(() => stateValue.arrayPush(StateValue.newNull())).toThrow('Push would cause array to exceed 15 elements');
    expect(stateValue.asArray()?.length).toEqual(15);
  });

  /**
   * Test creating array of arrays.
   *
   * @given Two nested arrays with maximum allowed size
   * @when Accessing nested array elements
   * @then Should navigate nested structure correctly
   */
  test('should create array of arrays', () => {
    const MAX_ARRAY_SIZE = 15;
    let stateValue = StateValue.newArray();
    for (let i = 0; i < MAX_ARRAY_SIZE; i += 1) {
      stateValue = stateValue.arrayPush(StateValue.newNull());
    }

    let sv = StateValue.newArray();
    for (let i = 0; i < MAX_ARRAY_SIZE; i += 1) {
      sv = sv.arrayPush(stateValue);
    }

    expect(sv.asArray()?.at(0)?.asArray()?.at(0)?.type()).toEqual(StateValue.newNull().type());
  });

  /**
   * Test creating StateValue with Map of different elements.
   *
   * @given A StateMap with 5 different keys
   * @when Creating StateValue and converting to string
   * @then Should not contain decode errors
   */
  test('should create state with Map of different elements', () => {
    let stateMap = new StateMap();
    for (let i = 0; i < 5; i += 1) {
      stateMap = stateMap.insert(
        {
          value: [new Uint8Array([i + 1])],
          alignment: [
            {
              tag: 'atom',
              value: { tag: 'compress' }
            }
          ]
        },
        StateValue.newNull()
      );
    }
    const stateValue = StateValue.newMap(stateMap);

    expect(stateValue.asMap()?.toString()).not.toMatch('decode error');
  });

  /**
   * Test encoding and decoding round-trip.
   *
   * @given A StateValue cell with aligned value
   * @when Encoding and then decoding
   * @then Should maintain identical string representation
   */
  test('should encode and decode correctly', () => {
    const stateValue = StateValue.newCell(Static.alignedValue);
    const stateValueEncoded = stateValue.encode();
    const stateValueDecoded = StateValue.decode(stateValueEncoded);

    expect(stateValueDecoded.toString()).toEqual(stateValue.toString());
  });

  /**
   * Test creating StateValue with nested Map.
   *
   * @given A nested map structure (map containing map)
   * @when Accessing nested map values
   * @then Should navigate nested structure correctly
   */
  test('should create state with nested Map', () => {
    let innerMap = new StateMap();
    innerMap = innerMap.insert(Static.alignedValue, StateValue.newNull());

    let outerMap = new StateMap();
    outerMap = outerMap.insert(Static.alignedValue, StateValue.newMap(innerMap));

    const stateValue = StateValue.newMap(outerMap);

    expect(stateValue.asMap()?.get(Static.alignedValue)?.asMap()?.get(Static.alignedValue)?.toString()).toEqual('null');
  });

  /**
   * Test creating StateValue with nested Array.
   *
   * @given A nested array structure (array containing array)
   * @when Accessing nested array values
   * @then Should navigate nested structure correctly
   */
  test('should create state with nested Array', () => {
    let innerArray = StateValue.newArray();
    innerArray = innerArray.arrayPush(StateValue.newNull());

    let outerArray = StateValue.newArray();
    outerArray = outerArray.arrayPush(innerArray);

    expect(outerArray.asArray()?.at(0)?.asArray()?.at(0)?.toString()).toEqual('null');
  });

  /**
   * Test creating StateValue with mixed Map and Array.
   *
   * @given A mixed structure with array containing map
   * @when Accessing nested values
   * @then Should navigate mixed structure correctly
   */
  test('should create state with mixed Map and Array', () => {
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());

    let stateArray = StateValue.newArray();
    stateArray = stateArray.arrayPush(StateValue.newMap(stateMap));

    expect(stateArray.asArray()?.at(0)?.asMap()?.get(Static.alignedValue)?.toString()).toEqual('null');
  });

  it('PM16013 - should allow creating an array containing a bounded merkle tree', () => {
    // The error in ticket PM-16013 was caused by the following line.
    StateValue.newArray().arrayPush(StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(100)));
  });
});
