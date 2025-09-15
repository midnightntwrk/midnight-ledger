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

import { StateBoundedMerkleTree, valueToBigInt } from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';

describe('Ledger API - StateBoundedMerkleTree', () => {
  /**
   * Test height limitation to 255.
   *
   * @given StateBoundedMerkleTree with heights 0 and 256
   * @when Creating the trees
   * @then Both should have height 0 (invalid height above 255 defaults to 0)
   */
  test('should limit the height to 255', () => {
    const stateBoundedMerkleTree = new StateBoundedMerkleTree(0);
    expect(stateBoundedMerkleTree.height).toEqual(0);

    const stateBoundedMerkleTree2 = new StateBoundedMerkleTree(256);
    expect(stateBoundedMerkleTree2.height).toEqual(0);
  });

  /**
   * Test basic tree operations.
   *
   * @given A StateBoundedMerkleTree with height 2
   * @when Updating indices 0 and 1 with aligned values and collapsing
   * @then Should maintain correct height
   */
  test('should work with basic operations', () => {
    let stateBoundedMerkleTree = new StateBoundedMerkleTree(2);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(0n, Static.alignedValue);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(1n, Static.alignedValueCompress);
    stateBoundedMerkleTree = stateBoundedMerkleTree.collapse(0n, 1n);

    expect(stateBoundedMerkleTree.height).toEqual(2);
  });

  /**
   * Test invalid collapse operation.
   *
   * @given A StateBoundedMerkleTree with updated indices
   * @when Attempting to collapse with invalid range (start > end)
   * @then Should not change the tree and maintain height
   */
  test('should not change on invalid collapse', () => {
    let stateBoundedMerkleTree = new StateBoundedMerkleTree(2);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(0n, Static.alignedValue);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(1n, Static.alignedValueCompress);
    stateBoundedMerkleTree = stateBoundedMerkleTree.collapse(1n, 0n);

    expect(stateBoundedMerkleTree.height).toEqual(2);
    expect(stateBoundedMerkleTree.toString()).not.toContain('collapsed');
  });

  /**
   * Test finding path for existing leaf.
   *
   * @given A StateBoundedMerkleTree with an updated value
   * @when Finding the path for that value
   * @then Should return a valid path with value and alignment
   */
  test("'findPathForLeaf' should find path for given leaf if it exists", () => {
    let stateBoundedMerkleTree = new StateBoundedMerkleTree(3);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(0n, Static.alignedValue);

    const path = stateBoundedMerkleTree.findPathForLeaf(Static.alignedValue);

    expect(path).toBeDefined();
    expect(path!.value).toBeDefined();
    expect(path!.alignment).toBeDefined();
    expect(Array.isArray(path!.value)).toBe(true);
    expect(Array.isArray(path!.alignment)).toBe(true);
  });

  /**
   * Test finding path for non-existent leaf.
   *
   * @given A StateBoundedMerkleTree
   * @when Finding the path for a non-existent leaf
   * @then Should return undefined
   */
  test("'findPathForLeaf' should return undefined for non-existent leaf", () => {
    const stateBoundedMerkleTree = new StateBoundedMerkleTree(3);

    const found = stateBoundedMerkleTree.findPathForLeaf(Static.alignedValue);
    expect(found).toBeUndefined();
  });

  /**
   * Test computing path for non-existent index.
   *
   * @given A StateBoundedMerkleTree
   * @when Computing the path for a non-existent index
   * @then Should throw an error
   */
  test("'pathForLeaf' should throw an error for non-existent index", () => {
    const stateBoundedMerkleTree = new StateBoundedMerkleTree(3);
    expect(() => stateBoundedMerkleTree.pathForLeaf(0n, Static.alignedValue)).toThrow();
    expect(() => stateBoundedMerkleTree.pathForLeaf(100n, Static.alignedValue)).toThrow();
  });

  /**
   * Test path generation for specific leaf.
   *
   * @given A StateBoundedMerkleTree with an updated value at index 0
   * @when Getting the path for leaf at index 0
   * @then Should return a valid path with value and alignment
   */
  test("'pathForLeaf' should return path for given index and leaf", () => {
    let stateBoundedMerkleTree = new StateBoundedMerkleTree(3);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(0n, Static.alignedValue);

    const path = stateBoundedMerkleTree.pathForLeaf(0n, Static.alignedValue);

    expect(path).toBeDefined();
    expect(path!.value).toBeDefined();
    expect(path!.alignment).toBeDefined();
    expect(Array.isArray(path!.value)).toBe(true);
    expect(Array.isArray(path!.alignment)).toBe(true);
  });

  /**
   * Test computing the root for an unhashed empty tree.
   *
   * @given An unhashed empty StateBoundedMerkleTree
   * @when Computing the root
   * @then Should return the default field value
   */
  test("'root' should return default root for empty tree", () => {
    const stateBoundedMerkleTree = new StateBoundedMerkleTree(3);
    const root = stateBoundedMerkleTree.root();
    expect(root).toBeDefined();
    expect(valueToBigInt(root!.value)).toEqual(0n);
  });

  /**
   * Test computing the root for an unhashed non-empty tree.
   *
   * @given An unhashed non-empty StateBoundedMerkleTree
   * @when Computing the root
   * @then Should return undefined
   */
  test("'root' should return undefined for an unhashed non-empty tree", () => {
    let stateBoundedMerkleTree = new StateBoundedMerkleTree(3);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(0n, Static.alignedValue);
    const root = stateBoundedMerkleTree.root();
    expect(root).toBeUndefined();
  });

  /**
   * Test computing the root for a rehashed non-empty tree.
   *
   * @given A a rehashed non-empty StateBoundedMerkleTree
   * @when Computing the root
   * @then Should return a valid root with value and alignment
   */
  test("'root' should return undefined for an unhashed non-empty tree", () => {
    let stateBoundedMerkleTree = new StateBoundedMerkleTree(3);
    stateBoundedMerkleTree = stateBoundedMerkleTree.update(0n, Static.alignedValue);
    const root = stateBoundedMerkleTree.rehash().root();
    expect(root).toBeDefined();
    expect(Array.isArray(root!.value)).toBe(true);
    expect(root!.value.length).toBeGreaterThan(0);
    expect(root!.value[0]).toBeInstanceOf(Uint8Array);
  });
});
