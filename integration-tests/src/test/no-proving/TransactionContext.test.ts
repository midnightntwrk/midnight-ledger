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

import { LedgerState, TransactionContext, ZswapChainState } from '@midnight-ntwrk/ledger';

import { Random, Static } from '@/test-objects';

describe('Ledger API - TransactionContext', () => {
  /**
   * Test basic context creation.
   *
   * @given A LedgerState and valid block context
   * @when Creating a TransactionContext
   * @then Should create context without throwing errors
   */
  test('should create context', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date()),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    expect(() => new TransactionContext(ledgerState, blockContext).toString()).not.toThrow();
  });

  /**
   * Test context creation with whitelist.
   *
   * @given A LedgerState, block context, and contract address whitelist
   * @when Creating a TransactionContext with whitelist
   * @then Should create context without throwing errors
   */
  test('should create context with whitelist', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date()),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    expect(() =>
      new TransactionContext(ledgerState, blockContext, new Set([Random.contractAddress()])).toString()
    ).not.toThrow();
  });

  /**
   * Test error handling with invalid block context.
   *
   * @given A LedgerState and invalid block context with negative time
   * @when Creating a TransactionContext
   * @then Should throw error about u64 bounds validation
   */
  test('should throw error with invalid block context', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const invalidBlockContext = {
      secondsSinceEpoch: -1n,
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    expect(() => new TransactionContext(ledgerState, invalidBlockContext).toString(true)).toThrow(
      "Couldn't deserialize u64 from a BigInt outside u64::MIN..u64::MAX bounds"
    );
  });

  /**
   * Test string representation of TransactionContext.
   *
   * @given A valid TransactionContext
   * @when Converting to string with verbose flag
   * @then Should return string matching TransactionContext pattern
   */
  test('should return string representation', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date()),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };
    const transactionContext = new TransactionContext(ledgerState, blockContext);

    expect(transactionContext.toString(true)).toMatch(/TransactionContext.*/);
  });

  /**
   * Test context creation with multiple whitelist addresses.
   *
   * @given A LedgerState, block context, and multiple contract addresses
   * @when Creating a TransactionContext with multiple addresses in whitelist
   * @then Should create context without throwing errors
   */
  test('should create context with multiple whitelist addresses', () => {
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date()),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    expect(() =>
      new TransactionContext(
        ledgerState,
        blockContext,
        new Set([Random.contractAddress(), Random.contractAddress()])
      ).toString()
    ).not.toThrow();
  });
});
