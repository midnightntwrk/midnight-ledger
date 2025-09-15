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

import {
  type AlignedValue,
  bigIntToValue,
  ChargedState,
  CostModel,
  createShieldedCoinInfo,
  type Effects,
  encodeContractAddress,
  encodeRawTokenType,
  type Op,
  QueryContext,
  rawTokenType as createRawTokenType,
  sampleContractAddress,
  StateMap,
  StateValue,
  type Transcript,
  ZswapOutput
} from '@midnight-ntwrk/ledger';

import { addressToPublic, Random, Static } from '@/test-objects';
import { mapFindByKey } from '@/test-utils';
import { ATOM_BYTES_1, ATOM_BYTES_16, ATOM_BYTES_32, EMPTY_VALUE, ONE_VALUE } from '@/test/utils/value-alignment';

describe('Ledger API - QueryContext', () => {
  /**
   * Test creating QueryContext with a map-based StateValue.
   *
   * @given A contract address and a StateMap with a null value
   * @when Creating a QueryContext with a map StateValue
   * @then The context should be created with correct address and state
   */
  test('should create context - map', () => {
    const contractAddress = Random.contractAddress();
    let stateMap = new StateMap();
    stateMap = stateMap.insert(Static.alignedValue, StateValue.newNull());
    const state = new ChargedState(StateValue.newMap(stateMap));
    const queryContext = new QueryContext(state, contractAddress);

    expect(queryContext.address).toEqual(contractAddress);
    expect(queryContext.state.toString()).toEqual(`${state.toString()}`);
    expect(queryContext.toString()).toMatch(/QueryContext.*/);
  });

  /**
   * Test creating QueryContext with a cell-based StateValue.
   *
   * @given A contract address and a cell StateValue
   * @when Creating a QueryContext
   * @then The context should be created with correct address and state
   */
  test('should create context - cell', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);

    expect(queryContext.address).toEqual(contractAddress);
    expect(queryContext.state.toString()).toEqual(`${state.toString()}`);
  });

  /**
   * Test creating QueryContext with an array-based StateValue.
   *
   * @given A contract address and an array StateValue with a null element
   * @when Creating a QueryContext
   * @then The context should be created with correct address and state
   */
  test('should create context - array', () => {
    const contractAddress = Random.contractAddress();
    let stateValue = StateValue.newArray();
    stateValue = stateValue.arrayPush(StateValue.newNull());
    const chargedState = new ChargedState(stateValue);
    const queryContext = new QueryContext(chargedState, contractAddress);

    expect(queryContext.address).toEqual(contractAddress);
    expect(queryContext.state.toString()).toEqual(`${chargedState.toString()}`);
  });

  /**
   * Test string representation of QueryContext.
   *
   * @given A QueryContext with an array StateValue
   * @when Calling toString with verbose flag
   * @then Should return a string matching the QueryContext pattern
   */
  test('toString - should print out', () => {
    const contractAddress = Random.contractAddress();
    let stateValue = StateValue.newArray();
    stateValue = stateValue.arrayPush(StateValue.newNull());
    const chargedState = new ChargedState(stateValue);
    const queryContext = new QueryContext(chargedState, contractAddress);

    expect(queryContext.toString(true)).toMatch(/QueryContext.*/);
  });

  /**
   * Test querying a QueryContext with a cell StateValue.
   *
   * @given A QueryContext with a cell StateValue
   * @when Executing a query with operations and cost model
   * @then Should return query results with expected gas cost and modified context
   */
  test('query - should query if StateValue is cell', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);
    const queryResults = queryContext.query(['new'], CostModel.initialCostModel());

    expect(queryResults.gasCost.computeTime).toBeGreaterThanOrEqual(1n);
    expect(queryResults.events.length).toEqual(0);
    expect(queryResults.context.toString()).not.toEqual(queryContext.toString());
    expect(queryResults.toString()).toMatch(/QueryResults \{.*/);
  });

  /**
   * Test query failure when gas limit is exceeded.
   *
   * @given A QueryContext with a cell StateValue
   * @when Executing a query that exceeds the gas limit
   * @then Should throw 'ran out of gas budget' error
   */
  test('query - should fail is ran gas out of limit', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);

    expect(() =>
      queryContext.query(['new', 'or'], CostModel.initialCostModel(), {
        readTime: 1n,
        computeTime: 1n,
        bytesWritten: 1n,
        bytesDeleted: 1n
      })
    ).toThrow('ran out of gas budget');
  });

  /**
   * Test query failure with negative gas limit.
   *
   * @given A QueryContext with a cell StateValue
   * @when Executing a query with negative gas limit
   * @then Should throw type decoding error
   */
  test('query - should fail when negative gas limit set', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);

    expect(() =>
      queryContext.query(['new', 'or'], CostModel.initialCostModel(), {
        readTime: -1n,
        computeTime: -1n,
        bytesWritten: -1n,
        bytesDeleted: -1n
      })
    ).toThrow("Error: Couldn't deserialize u64 from a BigInt outside u64::MIN..u64::MAX bounds");
  });

  /**
   * Test query failure with non-cell StateValues.
   *
   * @given A QueryContext with non-cell StateValue (null, array, or map)
   * @when Executing a query
   * @then Should throw 'expected a cell' error
   */
  it.each([[StateValue.newNull()], [StateValue.newArray()], [StateValue.newMap(new StateMap())]])(
    'query - should fail if StateValue is not cell - %s)',
    (stateValue) => {
      const chargedState = new ChargedState(stateValue);
      const contractAddress = Random.contractAddress();
      const queryContext = new QueryContext(chargedState, contractAddress);

      expect(() => queryContext.query(['new'], CostModel.initialCostModel())).toThrow('expected a cell');
    }
  );

  /**
   * Test qualification functionality of QueryContext.
   *
   * @given A QueryContext with a cell StateValue
   * @when Attempting to qualify with byte arrays
   * @then Should return undefined (qualification not supported)
   */
  test('qualify - should not qualify context', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);
    const qualification = queryContext.qualify([
      new Uint8Array([1, 2, 3]),
      new Uint8Array([4, 5, 6]),
      new Uint8Array([7, 8, 9])
    ]);

    expect(qualification).toBeUndefined();
  });

  /**
   * Test setting and getting effects on QueryContext.
   *
   * @given A QueryContext and an Effects object with various claims
   * @when Setting effects on the context
   * @then Should be able to retrieve the same effects
   */
  test('effects - set and get effects', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);
    const effects: Effects = {
      claimedNullifiers: [Random.hex(64)],
      claimedShieldedReceives: [Random.hex(64)],
      claimedShieldedSpends: [Random.hex(64)],
      claimedContractCalls: [],
      shieldedMints: new Map(),
      unshieldedMints: new Map(),
      unshieldedInputs: new Map(),
      unshieldedOutputs: new Map(),
      claimedUnshieldedSpends: new Map()
    };
    queryContext.effects = effects;

    expect(queryContext.effects).toEqual(effects);
  });

  /**
   * Test setting and getting block context on QueryContext.
   *
   * @given A QueryContext and a call context with block information
   * @when Setting block context on the QueryContext
   * @then Should be able to retrieve the same block context
   */
  test('block - set and get block context', () => {
    const contractAddress = Random.contractAddress();
    const userAddress = Random.userAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);
    const callContext = {
      ownAddress: contractAddress,
      secondsSinceEpoch: Static.blockTime(new Date()),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash(),
      caller: addressToPublic(userAddress, 'user'),
      balance: new Map(),
      comIndices: new Map()
    };
    queryContext.block = callContext;

    expect(queryContext.block).toEqual(callContext);
  });

  /**
   * Test inserting commitments into QueryContext.
   *
   * @given A QueryContext and coin commitments
   * @when Inserting commitments with indices
   * @then Should store commitments with correct indices and maintain context properties
   */
  test('insertCommitment - should insert commitment', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    let queryContext = new QueryContext(state, contractAddress);
    const coinCommitment = ZswapOutput.newContractOwned(
      createShieldedCoinInfo(Random.shieldedTokenType().raw, 100n),
      0,
      contractAddress
    ).commitment;
    const coinCommitment2 = ZswapOutput.newContractOwned(
      createShieldedCoinInfo(Random.shieldedTokenType().raw, 100n),
      0,
      contractAddress
    ).commitment;
    queryContext = queryContext.insertCommitment(coinCommitment, 1n);
    queryContext = queryContext.insertCommitment(coinCommitment2, 2n);

    expect(queryContext.comIndices.get(coinCommitment)).toEqual(1n);
    expect(queryContext.comIndices.get(coinCommitment2)).toEqual(2n);
    expect(queryContext.comIndices.size).toEqual(2);
    expect(queryContext.address).toEqual(contractAddress);
  });

  /**
   * Test running a transcript on QueryContext.
   *
   * @given A QueryContext and a transcript with operations and effects
   * @when Running the transcript with a cost model
   * @then Should return a modified context with expected properties
   */
  test('runTranscript - should run transcript', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);
    const effects: Effects = {
      claimedNullifiers: [],
      claimedShieldedReceives: [],
      claimedShieldedSpends: [],
      claimedContractCalls: [],
      shieldedMints: new Map(),
      unshieldedMints: new Map(),
      unshieldedInputs: new Map(),
      unshieldedOutputs: new Map(),
      claimedUnshieldedSpends: new Map()
    };
    const program: Op<AlignedValue>[] = [
      {
        push: {
          storage: false,
          value: StateValue.newCell(Static.alignedValue).encode()
        }
      },
      'pop',
      { noop: { n: 1 } }
    ];
    const sampleTranscript: Transcript<AlignedValue> = {
      gas: {
        readTime: 0n,
        computeTime: 10000000000n,
        bytesWritten: 0n,
        bytesDeleted: 0n
      },
      effects,
      program
    };
    const queryContext1 = queryContext.runTranscript(sampleTranscript, CostModel.initialCostModel());

    expect(queryContext1).toBeDefined();
    expect(queryContext1.block.parentBlockHash).toEqual(
      '0000000000000000000000000000000000000000000000000000000000000000'
    );
    expect(queryContext1.block.secondsSinceEpoch).toEqual(0n);
    expect(queryContext1.block.secondsSinceEpochErr).toEqual(0);
    expect(queryContext1.comIndices).toEqual(new Map());
    expect(queryContext1.address).toEqual(contractAddress);
    expect(queryContext1.effects).toEqual(effects);
    expect(queryContext1.state.toString()).toEqual(state.toString());
  });

  /**
   * Test claiming unshielded spends functionality.
   *
   * @given A QueryContext with array state and various compact type descriptors
   * @and Amount, color, and recipient values for unshielded spend operation
   * @when Executing complex query operations with swap, idx, push, and arithmetic ops
   * @and Processing operations to claim unshielded spends with proper encoding
   * @then Should update effects with claimed unshielded spend amounts
   * @and Should properly handle color encoding and recipient addressing
   */
  test('should allow claiming unshielded spends', () => {
    const amount = 100n;
    const color = encodeRawTokenType(createRawTokenType(Random.generate32Bytes(), sampleContractAddress()));
    const recipient = encodeContractAddress(sampleContractAddress());

    const stateValue = new ChargedState(StateValue.newArray());
    const queryContext = new QueryContext(stateValue, sampleContractAddress());

    const ops: Op<null>[] = [
      { swap: { n: 0 } },
      {
        idx: {
          cached: true,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: {
                value: [new Uint8Array([8])],
                alignment: [ATOM_BYTES_1]
              }
            }
          ]
        }
      },
      {
        push: {
          storage: false,
          value: StateValue.newCell({
            value: [ONE_VALUE, color, EMPTY_VALUE, ONE_VALUE, recipient, EMPTY_VALUE],
            alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
          }).encode()
        }
      },
      { dup: { n: 1 } },
      { dup: { n: 1 } },
      'member',
      {
        push: {
          storage: false,
          value: StateValue.newCell({
            value: bigIntToValue(amount),
            alignment: [ATOM_BYTES_16]
          }).encode()
        }
      },
      { swap: { n: 0 } },
      'neg',
      { branch: { skip: 4 } },
      { dup: { n: 2 } },
      { dup: { n: 2 } },
      { idx: { cached: true, pushPath: false, path: [{ tag: 'stack' }] } },
      'add',
      { ins: { cached: true, n: 2 } },
      { swap: { n: 0 } }
    ];

    const res = queryContext.query(ops, CostModel.initialCostModel());
    const { effects } = res.context;
    expect([...effects.claimedUnshieldedSpends.values()]).toEqual([amount]);
  });

  /**
   * Test 'get' functionality for 'claimedUnshieldedSpends'.
   *
   * @given A QueryContext with multiple compact type descriptors for complex data structures
   * @and Specific amount, domain separator, and address values
   * @when Executing comprehensive query operations with descriptor-based encoding
   * @and Processing swap, idx, push, dup, member, and arithmetic operations
   * @then Should successfully claim unshielded spends with proper key mapping
   * @and Should handle complex nested data structure encodings correctly
   */
  test("'get' should work for 'claimedUnshieldedSpends'", () => {
    const amount = 100n;

    const selfRawAddress = sampleContractAddress();
    const stateValue = new ChargedState(StateValue.newArray());
    const queryContext = new QueryContext(stateValue, selfRawAddress);

    const domainSep = Random.generate32Bytes();
    const rawTokenType = createRawTokenType(domainSep, selfRawAddress);

    const queryResult = queryContext.query(
      [
        { swap: { n: 0 } },
        {
          idx: {
            cached: true,
            pushPath: true,
            path: [
              {
                tag: 'value',
                value: {
                  value: [new Uint8Array([8])],
                  alignment: [ATOM_BYTES_1]
                }
              }
            ]
          }
        },
        {
          push: {
            storage: false,
            value: StateValue.newCell({
              value: [
                ONE_VALUE,
                encodeRawTokenType(rawTokenType),
                EMPTY_VALUE,
                ONE_VALUE,
                encodeContractAddress(selfRawAddress),
                EMPTY_VALUE
              ],
              alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
            }).encode()
          }
        },
        { dup: { n: 1 } },
        { dup: { n: 1 } },
        'member',
        {
          push: {
            storage: false,
            value: StateValue.newCell({
              value: bigIntToValue(amount),
              alignment: [ATOM_BYTES_16]
            }).encode()
          }
        },
        { swap: { n: 0 } },
        'neg',
        { branch: { skip: 4 } },
        { dup: { n: 2 } },
        { dup: { n: 2 } },
        {
          idx: {
            cached: true,
            pushPath: false,
            path: [{ tag: 'stack' }]
          }
        },
        'add',
        { ins: { cached: true, n: 2 } },
        { swap: { n: 0 } }
      ],
      CostModel.initialCostModel()
    );

    const { claimedUnshieldedSpends } = queryResult.context.effects;

    const tokenTypeEnum = {
      tag: 'unshielded',
      raw: rawTokenType
    } as const;
    const publicAddressEnum = {
      tag: 'contract',
      address: selfRawAddress
    } as const;

    expect(mapFindByKey(claimedUnshieldedSpends, [tokenTypeEnum, publicAddressEnum])).toBeDefined();
  });

  /**
   * Test converting QueryContext to VmStack.
   *
   * @given A QueryContext with a cell StateValue
   * @when Converting to VmStack
   * @then Should return a VmStack with expected length and properties
   */
  test('toVmStack - should convert context to VmStack', () => {
    const contractAddress = Random.contractAddress();
    const state = new ChargedState(StateValue.newCell(Static.alignedValue));
    const queryContext = new QueryContext(state, contractAddress);

    const vmStack = queryContext.toVmStack();

    expect(vmStack).toBeDefined();
    expect(vmStack.length()).toEqual(3);
    expect(vmStack.get(0)).toBeDefined();
    expect(vmStack.isStrong(0)).toBe(false);
  });
});
