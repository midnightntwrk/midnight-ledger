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

import { type AlignedValue, StateMap, StateValue } from '@midnight-ntwrk/ledger';
import { ONE_KB, Static } from '@/test-objects';

describe('Ledger API - StateMap', () => {
  /**
   * Test key override behavior when inserting duplicate keys.
   *
   * @given A StateMap with a key inserted twice with different values
   * @when Retrieving the value for that key
   * @then Should return the most recently inserted value
   */
  test('should override key if insert 2 elements with same key', () => {
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newArray());
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());

    expect(stateMap.keys().length).toEqual(1);
    expect(stateMap.get(Static.alignedValue)?.toString()).toEqual(StateValue.newNull().toString());
  });

  /**
   * Test element removal functionality.
   *
   * @given A StateMap with one element
   * @when Removing that element
   * @then Should result in empty map
   */
  test('should remove element', () => {
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());
    stateMap = stateMap.remove(Static.alignedValue);

    expect(stateMap.keys().length).toEqual(0);
    expect(stateMap.toString()).toEqual('{}');
  });

  /**
   * Test removal of non-existing element.
   *
   * @given A StateMap with one element and a different key
   * @when Attempting to remove the non-existing key
   * @then Should not affect the existing element
   */
  test('should not remove on not existing element', () => {
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());
    stateMap = stateMap.remove(Static.alignedValueCompress);

    expect(stateMap.keys().length).toEqual(1);
    expect(stateMap.toString()).not.toEqual('{}');
  });

  /**
   * Test validation of aligned value format.
   *
   * @given An invalid aligned value with trailing zero bytes
   * @when Attempting to insert it into StateMap
   * @then Should throw error about normal form validation
   */
  test('should not allow inserting an invalid value', () => {
    const alignedValue: AlignedValue = {
      value: [new Uint8Array(2)],
      alignment: [
        {
          tag: 'atom',
          value: { tag: 'compress' }
        }
      ]
    };
    const stateMap = new StateMap();
    expect(() => stateMap.insert(alignedValue, StateValue.newNull())).toThrow(
      'aligned value is not in normal form (has trailing zero bytes)'
    );
  });

  /**
   * Test string representation of StateMap.
   *
   * @given Empty and populated StateMaps
   * @when Converting to string
   * @then Should return appropriate string representations
   */
  test('should print out correct string representation', () => {
    let stateMap = new StateMap();
    expect(stateMap.toString()).toEqual('{}');

    stateMap = stateMap.insert(Static.alignedValue, StateValue.newArray());
    expect(stateMap.toString()).toEqual('{\n    <[-]: f>: Array(0) [],\n}');
  });

  /**
   * Test insertion with different key types.
   *
   * @given StateMap and different aligned value types (compress and field)
   * @when Inserting values with these keys
   * @then Should handle both key types correctly
   */
  it.each([
    ['compress', Static.alignedValueCompress],
    ['field', Static.alignedValue]
  ])('should insert different keys (%s)', (_testCase, alignedValue: AlignedValue) => {
    let stateMap = new StateMap();
    stateMap = stateMap.insert(alignedValue, StateValue.newNull());

    expect(stateMap.keys().length).toEqual(1);
    expect(stateMap.get(alignedValue)?.toString()).toEqual(StateValue.newNull().toString());
  });

  /**
   * Test handling of empty map.
   *
   * @given A new StateMap
   * @when Checking its properties
   * @then Should have zero keys and empty string representation
   */
  test('should handle empty map', () => {
    const stateMap = new StateMap();
    expect(stateMap.keys().length).toEqual(0);
    expect(stateMap.toString()).toEqual('{}');
  });

  /**
   * Test handling of map with multiple elements.
   *
   * @given A StateMap with two different key-value pairs
   * @when Retrieving values for both keys
   * @then Should return correct values for each key
   */
  test('should handle map with multiple elements', () => {
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newArray());
    stateMap = stateMap.insert(Static.alignedValueCompress, StateValue.newNull());

    expect(stateMap.keys().length).toEqual(2);
    expect(stateMap.get(Static.alignedValue)?.toString()).toEqual(StateValue.newArray().toString());
    expect(stateMap.get(Static.alignedValueCompress)?.toString()).toEqual(StateValue.newNull().toString());
  });

  /**
   * Test handling of map with nested maps.
   *
   * @given A StateMap containing another StateMap as a value
   * @when Accessing nested map values
   * @then Should navigate nested structure correctly
   */
  test('should handle map with nested maps', () => {
    let innerMap = new StateMap();
    innerMap = innerMap.insert(Static.alignedValue, StateValue.newNull());

    let outerMap = new StateMap();
    outerMap = outerMap.insert(Static.alignedValue, StateValue.newMap(innerMap));

    expect(outerMap.keys().length).toEqual(1);
    expect(outerMap.get(Static.alignedValue)?.asMap()?.get(Static.alignedValue)?.toString()).toEqual(
      StateValue.newNull().toString()
    );
  });

  /**
   * Test handling of map with nested arrays.
   *
   * @given A StateMap containing an array as a value
   * @when Accessing nested array values
   * @then Should navigate nested structure correctly
   */
  test('should handle map with nested arrays', () => {
    let innerArray = StateValue.newArray();
    innerArray = innerArray.arrayPush(StateValue.newNull());

    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, innerArray);

    expect(stateMap.keys().length).toEqual(1);
    expect(stateMap.get(Static.alignedValue)?.asArray()?.at(0)?.toString()).toEqual(StateValue.newNull().toString());
  });

  /**
   * Test inserting large key multiple times.
   *
   * @given A large aligned value (128KB) and StateValue
   * @when Inserting the same key twice
   * @then Should handle large keys without memory access errors
   */
  test('should insert key with allowed big size twice', () => {
    let stateMap = new StateMap();
    const stateValueArray = StateValue.newArray();
    const alignedValue: AlignedValue = {
      value: [new Uint8Array(128 * ONE_KB).fill(255)],
      alignment: [
        {
          tag: 'atom',
          value: { tag: 'bytes', length: 128 * ONE_KB }
        }
      ]
    };
    stateMap = stateMap.insert(alignedValue, stateValueArray);
    stateMap.insert(alignedValue, stateValueArray);
  });

  /**
   * Test validation for oversized keys (currently skipped due to bug).
   *
   * @given A StateMap and an oversized key (512KB+)
   * @when Attempting to insert the oversized key
   * @then Should throw error about key size limit
   */
  it.skip('should not allow inserting a key that is too large', () => {
    const stateMap = new StateMap();
    expect(() =>
      stateMap.insert(
        {
          value: [new Uint8Array(512 * ONE_KB).fill(255)],
          alignment: [
            {
              tag: 'atom',
              value: { tag: 'bytes', length: 1024 * ONE_KB }
            }
          ]
        },
        StateValue.newArray()
      )
    ).toThrow('big key exceeding limit of 512KB');
  });
});
