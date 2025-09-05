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

import { StateBoundedMerkleTree, StateValue, VmStack } from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';

describe('Ledger API - VmStack', () => {
  /**
   * Test basic VmStack operations.
   *
   * @given A new VmStack
   * @when Pushing values and checking properties
   * @then Should handle push, get, isStrong, and removeLast operations correctly
   */
  test('should work as expected', () => {
    const vmStack = new VmStack();

    expect(vmStack.toString()).toEqual('[]');

    vmStack.push(StateValue.newArray(), true);
    vmStack.push(StateValue.newNull(), false);

    expect(vmStack.isStrong(0)).toEqual(true);
    expect(vmStack.isStrong(1)).toEqual(false);
    expect(vmStack.get(0)?.asArray()).toEqual(StateValue.newArray().asArray());
    expect(vmStack.get(1)?.type()).toEqual('null');
    expect(vmStack.get(2)).toBeUndefined();

    vmStack.removeLast();
    vmStack.removeLast();

    expect(vmStack.length()).toEqual(0);
  });

  /**
   * Test pushing and popping multiple values.
   *
   * @given A VmStack with multiple values pushed
   * @when Removing values one by one
   * @then Should maintain correct length and values at each step
   */
  test('should handle pushing and popping multiple values', () => {
    const vmStack = new VmStack();

    vmStack.push(StateValue.newArray(), true);
    vmStack.push(StateValue.newNull(), false);
    vmStack.push(StateValue.newCell(Static.alignedValue), true);

    expect(vmStack.length()).toEqual(3);

    vmStack.removeLast();
    expect(vmStack.length()).toEqual(2);
    expect(vmStack.get(1)?.type()).toEqual('null');

    vmStack.removeLast();
    expect(vmStack.length()).toEqual(1);
    expect(vmStack.get(0)?.asArray()).toEqual(StateValue.newArray().asArray());

    vmStack.removeLast();
    expect(vmStack.length()).toEqual(0);
  });

  /**
   * Test handling mixed strong and weak values.
   *
   * @given A VmStack with both strong and weak values
   * @when Checking strength properties and removing values
   * @then Should correctly track strength properties of remaining values
   */
  test('should handle mixed strong and weak values', () => {
    const vmStack = new VmStack();

    vmStack.push(StateValue.newArray(), true);
    vmStack.push(StateValue.newNull(), false);
    vmStack.push(StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(0)), false);

    expect(vmStack.isStrong(0)).toEqual(true);
    expect(vmStack.isStrong(1)).toEqual(false);
    expect(vmStack.isStrong(2)).toEqual(false);

    vmStack.removeLast();
    expect(vmStack.isStrong(1)).toEqual(false);

    vmStack.removeLast();
    expect(vmStack.isStrong(0)).toEqual(true);
  });

  /**
   * Test out of bounds access handling.
   *
   * @given A VmStack with one value
   * @when Accessing indices outside valid range
   * @then Should return undefined for invalid indices
   */
  test('should return undefined for out of bounds access', () => {
    const vmStack = new VmStack();

    vmStack.push(StateValue.newArray(), true);

    expect(vmStack.get(1)).toBeUndefined();
    expect(vmStack.get(-1)).toBeUndefined();
  });
});
