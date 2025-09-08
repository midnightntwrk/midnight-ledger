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
  LedgerState,
  UnshieldedOffer,
  TransactionContext,
  Transaction,
  ZswapChainState,
  Intent,
  UtxoState,
  UtxoMeta,
  WellFormedStrictness,
  sampleUserAddress,
  sampleIntentHash,
  type Utxo,
  signatureVerifyingKey,
  sampleSigningKey,
  signData,
  addressFromKey
} from '@midnight-ntwrk/ledger';
import { Random, Static } from '@/test-objects';
import { compareBigIntArrays, sortBigIntArray } from '@/test-utils';

describe('Ledger API - UtxoState', () => {
  /**
   * Test storage of outputs in UTXO state.
   *
   * @given A ledger state and intent with guaranteed unshielded outputs
   * @when Applying a transaction with two outputs to the ledger
   * @then Should store both outputs in the UTXO state
   */
  test('should store the outputs in utxo state', () => {
    const ledgerState = new LedgerState('local-test', new ZswapChainState());
    const intent = Intent.new(Static.calcBlockTime(new Date(0), 50));
    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [],
      [
        {
          value: 100n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw
        },
        {
          value: 120n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw
        }
      ],
      []
    );
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, intent);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();

    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date(0)),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(ledgerState, strictness, new Date(0));
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const [ledgerStateAfter, _txResult] = ledgerState.apply(
      verifiedTransaction,
      new TransactionContext(ledgerState, blockContext, new Set())
    );

    expect(ledgerStateAfter.utxo.utxos.size).toEqual(2);
  });

  /**
   * Test filtering UTXOs by user address.
   *
   * @given A ledger state with outputs assigned to different user addresses
   * @when Filtering UTXOs by specific user addresses
   * @then Should return correct UTXO subsets for each address with expected values
   */
  test('should filter the utxos by user address', () => {
    const ledgerState = new LedgerState('local-test', new ZswapChainState());
    const intent = Intent.new(Static.calcBlockTime(new Date(0), 50));
    const address1 = sampleUserAddress();
    const address2 = sampleUserAddress();
    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [],
      [
        {
          value: 100n,
          owner: address1,
          type: Random.unshieldedTokenType().raw
        },
        {
          value: 120n,
          owner: address2,
          type: Random.unshieldedTokenType().raw
        }
      ],
      []
    );
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, intent);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();

    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date(0)),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(ledgerState, strictness, new Date(0));
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const [ledgerStateAfter, _txResult] = ledgerState.apply(
      verifiedTransaction,
      new TransactionContext(ledgerState, blockContext, new Set())
    );

    expect(ledgerStateAfter.utxo.utxos.size).toEqual(2);
    expect(ledgerStateAfter.utxo.filter(address1).size).toEqual(1);
    expect(ledgerStateAfter.utxo.filter(address1).size).toEqual(1);
    expect([...ledgerStateAfter.utxo.filter(address2)!.values()]?.[0]?.value).toEqual(120n);
  });

  /**
   * Test comparison between two UTXO states.
   *
   * @given Two UTXO states with different contents and delta filtering capabilities
   * @when Comparing states and applying various filter conditions
   * @then Should correctly identify differences and apply filters based on value thresholds
   */
  test('should compare two states', () => {
    const address1 = sampleUserAddress();
    const address2 = sampleUserAddress();
    const ledgerState = new LedgerState('local-test', new ZswapChainState());
    const intent = Intent.new(Static.calcBlockTime(new Date(0), 50));
    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [],
      [
        {
          value: 100n,
          owner: address1,
          type: Random.unshieldedTokenType().raw
        },
        {
          value: 120n,
          owner: address2,
          type: Random.unshieldedTokenType().raw
        }
      ],
      []
    );
    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, intent);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();

    const blockContext = {
      secondsSinceEpoch: Static.blockTime(new Date(0)),
      secondsSinceEpochErr: 0,
      parentBlockHash: Static.parentBlockHash()
    };

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(ledgerState, strictness, new Date(0));
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const [ledgerStateAfter, _txResult] = ledgerState.apply(
      verifiedTransaction,
      new TransactionContext(ledgerState, blockContext, new Set())
    );

    expect(ledgerStateAfter.utxo.utxos.size).toEqual(2);

    // compare with the same state
    expect(ledgerStateAfter.utxo.delta(ledgerStateAfter.utxo)).toEqual([new Set(), new Set()]);

    // add more records:
    const address3 = sampleUserAddress();
    const address4 = sampleUserAddress();
    const anotherUtxoState = UtxoState.new(
      new Map([
        [
          {
            value: 200n,
            owner: address3,
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 2
          },
          new UtxoMeta(new Date(0))
        ],
        [
          {
            value: 220n,
            owner: address4,
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 3
          },
          new UtxoMeta(new Date(0))
        ]
      ])
    );

    const delta = ledgerStateAfter.utxo.delta(anotherUtxoState);
    const values = delta.map((d) => [...d.values()].map(({ value }) => value).sort());
    expect(values[0]).toEqual([100n, 120n]);
    expect(values[1]).toEqual([200n, 220n]);

    // validate the filtering callback
    const delta2 = ledgerStateAfter.utxo.delta(anotherUtxoState, (utxo) => {
      return utxo.value > 200n;
    });
    const values2 = delta2.map((d) => [...d.values()].map(({ value }) => value).sort());
    expect(values2[0]).toEqual([]);
    expect(values2[1]).toEqual([220n]);

    // validate other filter`s condition
    const delta3 = ledgerStateAfter.utxo.delta(anotherUtxoState, (utxo) => {
      return utxo.value > 100n;
    });
    const values3 = delta3.map((d) => [...d.values()].map(({ value }) => value).sort());
    expect(values3[0]).toEqual([120n]);
    expect(values3[1]).toEqual([200n, 220n]);
  });

  describe('UtxoState.new() - Constructor scenarios', () => {
    /**
     * Test creation of empty UTXO state.
     *
     * @given An empty set of UTXOs
     * @when Creating new UtxoState
     * @then Should have zero UTXOs in the state
     */
    test('should create empty UTXO state from empty set', () => {
      const utxoState = UtxoState.new(new Map());

      expect(utxoState.utxos.size).toEqual(0);
    });

    /**
     * Test creation of UTXO state with single UTXO.
     *
     * @given A single UTXO with defined properties
     * @when Creating new UtxoState with this UTXO
     * @then Should contain exactly one UTXO with matching properties
     */
    test('should create UTXO state with single UTXO', () => {
      const utxo: Utxo = {
        value: 100n,
        owner: sampleUserAddress(),
        type: Random.unshieldedTokenType().raw,
        intentHash: sampleIntentHash(),
        outputNo: 0
      };

      const utxoState = UtxoState.new(new Map([[utxo, new UtxoMeta(new Date(0))]]));

      expect(utxoState.utxos.size).toEqual(1);
      const retrievedUtxo = [...utxoState.utxos][0];
      expect(retrievedUtxo.value).toEqual(utxo.value);
      expect(retrievedUtxo.owner).toEqual(utxo.owner);
      expect(retrievedUtxo.type).toEqual(utxo.type);
      expect(retrievedUtxo.intentHash).toEqual(utxo.intentHash);
      expect(retrievedUtxo.outputNo).toEqual(utxo.outputNo);
    });

    /**
     * Test creation of UTXO state with multiple UTXOs.
     *
     * @given Multiple UTXOs with different values and properties
     * @when Creating new UtxoState with all UTXOs
     * @then Should contain all UTXOs with preserved properties
     */
    test('should create UTXO state with multiple UTXOs', () => {
      const utxos: [Utxo, UtxoMeta][] = [
        [
          {
            value: 100n,
            owner: sampleUserAddress(),
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 0
          },
          new UtxoMeta(new Date(0))
        ],
        [
          {
            value: 200n,
            owner: sampleUserAddress(),
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 1
          },
          new UtxoMeta(new Date(0))
        ],
        [
          {
            value: 300n,
            owner: sampleUserAddress(),
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 2
          },
          new UtxoMeta(new Date(0))
        ]
      ];

      const utxoState = UtxoState.new(new Map(utxos));

      expect(utxoState.utxos.size).toEqual(3);
      utxos.forEach(([utxo, _meta]) => {
        const foundUtxo = [...utxoState.utxos].find(
          (u) =>
            u.value === utxo.value &&
            u.owner === utxo.owner &&
            u.type === utxo.type &&
            u.intentHash === utxo.intentHash &&
            u.outputNo === utxo.outputNo
        );
        expect(foundUtxo).toBeDefined();
      });
    });

    /**
     * Test handling of duplicate UTXOs in input set.
     *
     * @given The same UTXO added twice to the input set
     * @when Creating new UtxoState with duplicate UTXOs
     * @then Should contain only one instance due to Set deduplication
     */
    test('should handle duplicate UTXOs in input set', () => {
      const utxo: Utxo = {
        value: 100n,
        owner: sampleUserAddress(),
        type: Random.unshieldedTokenType().raw,
        intentHash: sampleIntentHash(),
        outputNo: 0
      };

      // Try to add the same UTXO twice
      const utxoState = UtxoState.new(
        new Map([
          [utxo, new UtxoMeta(new Date(0))],
          [utxo, new UtxoMeta(new Date(0))]
        ])
      );

      expect(utxoState.utxos.size).toEqual(1);
    });

    /**
     * Test handling of UTXOs with zero values.
     *
     * @given A UTXO with zero value
     * @when Creating new UtxoState with zero-value UTXO
     * @then Should properly store and preserve the zero value
     */
    test('should handle UTXOs with zero values', () => {
      const utxo: Utxo = {
        value: 0n,
        owner: sampleUserAddress(),
        type: Random.unshieldedTokenType().raw,
        intentHash: sampleIntentHash(),
        outputNo: 0
      };

      const utxoState = UtxoState.new(new Map([[utxo, new UtxoMeta(new Date(0))]]));

      expect(utxoState.utxos.size).toEqual(1);
      expect(Array.from(utxoState.utxos)[0].value).toEqual(0n);
    });

    /**
     * Test handling of UTXOs with large values.
     *
     * @given A UTXO with maximum safe BigInt value (2^64 - 1)
     * @when Creating new UtxoState with large-value UTXO
     * @then Should properly store and preserve the large value
     */
    test('should handle UTXOs with large values', () => {
      const largeValue = BigInt('18446744073709551615'); // 2^64 - 1
      const utxo: Utxo = {
        value: largeValue,
        owner: sampleUserAddress(),
        type: Random.unshieldedTokenType().raw,
        intentHash: sampleIntentHash(),
        outputNo: 0
      };

      const utxoState = UtxoState.new(new Map([[utxo, new UtxoMeta(new Date(0))]]));

      expect(utxoState.utxos.size).toEqual(1);
      expect(Array.from(utxoState.utxos)[0].value).toEqual(largeValue);
    });
  });

  describe('filter() - Advanced filtering scenarios', () => {
    let mixedUtxoState: UtxoState;
    let address1: string;
    let address2: string;
    let address3: string;

    beforeEach(() => {
      address1 = sampleUserAddress();
      address2 = sampleUserAddress();
      address3 = sampleUserAddress();

      const utxos: Utxo[] = [
        {
          value: 100n,
          owner: address1,
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 0
        },
        {
          value: 200n,
          owner: address1,
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 1
        },
        {
          value: 300n,
          owner: address2,
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 0
        },
        {
          value: 400n,
          owner: address2,
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 1
        },
        {
          value: 500n,
          owner: address3,
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 0
        }
      ];

      mixedUtxoState = UtxoState.new(new Map(utxos.map((utxo) => [utxo, new UtxoMeta(new Date(0))])));
    });

    /**
     * Test filtering UTXOs by first address.
     *
     * @given A mixed UTXO state with UTXOs from multiple addresses
     * @when Filtering by first address
     * @then Should return only UTXOs belonging to first address with correct values
     */
    test('should filter UTXOs by first address', () => {
      const filtered = mixedUtxoState.filter(address1);

      expect(filtered.size).toEqual(2);
      const values = Array.from(filtered).map((utxo) => utxo.value);
      const sortedValues = sortBigIntArray(values);
      expect(compareBigIntArrays(sortedValues, [100n, 200n])).toBe(true);
    });

    /**
     * Test filtering UTXOs by second address.
     *
     * @given A mixed UTXO state with UTXOs from multiple addresses
     * @when Filtering by second address
     * @then Should return only UTXOs belonging to second address with correct values
     */
    test('should filter UTXOs by second address', () => {
      const filtered = mixedUtxoState.filter(address2);

      expect(filtered.size).toEqual(2);
      const values = Array.from(filtered).map((utxo) => utxo.value);
      const sortedValues = sortBigIntArray(values);
      expect(compareBigIntArrays(sortedValues, [300n, 400n])).toBe(true);
    });

    /**
     * Test filtering UTXOs by third address.
     *
     * @given A mixed UTXO state with UTXOs from multiple addresses
     * @when Filtering by third address
     * @then Should return single UTXO belonging to third address
     */
    test('should filter UTXOs by third address', () => {
      const filtered = mixedUtxoState.filter(address3);

      expect(filtered.size).toEqual(1);
      expect(Array.from(filtered)[0].value).toEqual(500n);
    });

    /**
     * Test filtering with non-existent address.
     *
     * @given A mixed UTXO state and a non-existent address
     * @when Filtering by address that owns no UTXOs
     * @then Should return empty set
     */
    test('should return empty set for non-existent address', () => {
      const nonExistentAddress = sampleUserAddress();
      const filtered = mixedUtxoState.filter(nonExistentAddress);

      expect(filtered.size).toEqual(0);
    });

    /**
     * Test filtering from empty state.
     *
     * @given An empty UTXO state
     * @when Filtering by any address
     * @then Should return empty set
     */
    test('should filter from empty state', () => {
      const emptyState = UtxoState.new(new Map());
      const filtered = emptyState.filter(address1);

      expect(filtered.size).toEqual(0);
    });

    /**
     * Test preservation of UTXO properties in filtered results.
     *
     * @given A mixed UTXO state filtered by specific address
     * @when Examining filtered UTXOs
     * @then Should preserve all UTXO properties and validate constraints
     */
    test('should preserve UTXO properties in filtered results', () => {
      const filtered = mixedUtxoState.filter(address1);

      filtered.forEach((utxo) => {
        expect(utxo.owner).toEqual(address1);
        expect(utxo.value).toBeGreaterThan(0n);
        expect(utxo.type).toBeDefined();
        expect(utxo.intentHash).toBeDefined();
        expect(utxo.outputNo).toBeGreaterThanOrEqual(0);
      });
    });
  });

  describe('delta() - Comprehensive difference scenarios', () => {
    /**
     * Test delta between identical states.
     *
     * @given Two identical UTXO states
     * @when Computing delta between them
     * @then Should return empty added and removed sets
     */
    test('should handle identical states', () => {
      const utxos = new Map([
        [
          {
            value: 100n,
            owner: sampleUserAddress(),
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 0
          },
          new UtxoMeta(new Date(0))
        ]
      ]);

      const state1 = UtxoState.new(utxos);
      const state2 = UtxoState.new(utxos);

      const [added, removed] = state1.delta(state2);

      expect(added.size).toEqual(0);
      expect(removed.size).toEqual(0);
    });

    /**
     * Test delta between completely different states.
     *
     * @given Two UTXO states with completely different UTXOs
     * @when Computing delta between them
     * @then Should return all UTXOs from first state as added and all from second as removed
     */
    test('should handle completely different states', () => {
      const utxos1 = new Map([
        [
          {
            value: 100n,
            owner: sampleUserAddress(),
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 0
          },
          new UtxoMeta(new Date(0))
        ]
      ]);

      const utxos2 = new Map([
        [
          {
            value: 200n,
            owner: sampleUserAddress(),
            type: Random.unshieldedTokenType().raw,
            intentHash: sampleIntentHash(),
            outputNo: 1
          },
          new UtxoMeta(new Date(0))
        ]
      ]);

      const state1 = UtxoState.new(utxos1);
      const state2 = UtxoState.new(utxos2);

      const [added, removed] = state1.delta(state2);

      expect(added.size).toEqual(1);
      expect(removed.size).toEqual(1);
      expect(Array.from(added)[0].value).toEqual(100n);
      expect(Array.from(removed)[0].value).toEqual(200n);
    });

    /**
     * Test delta between states with partial overlap.
     *
     * @given Two UTXO states sharing some common UTXOs but having unique ones
     * @when Computing delta between them
     * @then Should return only the unique UTXOs from each state
     */
    test('should handle partial overlap states', () => {
      const commonUtxo: [Utxo, UtxoMeta] = [
        {
          value: 100n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 0
        },
        new UtxoMeta(new Date(0))
      ];

      const uniqueUtxo1: [Utxo, UtxoMeta] = [
        {
          value: 200n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 1
        },
        new UtxoMeta(new Date(0))
      ];

      const uniqueUtxo2: [Utxo, UtxoMeta] = [
        {
          value: 300n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 2
        },
        new UtxoMeta(new Date(0))
      ];

      const state1 = UtxoState.new(new Map([commonUtxo, uniqueUtxo1]));
      const state2 = UtxoState.new(new Map([commonUtxo, uniqueUtxo2]));

      const [added, removed] = state1.delta(state2);

      expect(added.size).toEqual(1);
      expect(removed.size).toEqual(1);
      expect(Array.from(added)[0].value).toEqual(200n);
      expect(Array.from(removed)[0].value).toEqual(300n);
    });

    /**
     * Test delta between empty and non-empty states.
     *
     * @given An empty state and a non-empty state
     * @when Computing delta in both directions
     * @then Should correctly identify additions and removals based on direction
     */
    test('should handle empty state comparisons', () => {
      const emptyState = UtxoState.new(new Map());
      const nonEmptyState = UtxoState.new(
        new Map([
          [
            {
              value: 100n,
              owner: sampleUserAddress(),
              type: Random.unshieldedTokenType().raw,
              intentHash: sampleIntentHash(),
              outputNo: 0
            },
            new UtxoMeta(new Date(0))
          ]
        ])
      );

      const [added, removed] = nonEmptyState.delta(emptyState);

      expect(added.size).toEqual(1);
      expect(removed.size).toEqual(0);

      const [added2, removed2] = emptyState.delta(nonEmptyState);

      expect(added2.size).toEqual(0);
      expect(removed2.size).toEqual(1);
    });
  });

  describe('delta() - Advanced filtering scenarios', () => {
    let state1: UtxoState;
    let state2: UtxoState;

    beforeEach(() => {
      const utxos1 = [
        {
          value: 100n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 0
        },
        {
          value: 200n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 1
        },
        {
          value: 300n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 2
        }
      ];

      const utxos2 = [
        {
          value: 150n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 3
        },
        {
          value: 250n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 4
        },
        {
          value: 350n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 5
        }
      ];

      state1 = UtxoState.new(new Map(utxos1.map((utxo) => [utxo, new UtxoMeta(new Date(0))])));
      state2 = UtxoState.new(new Map(utxos2.map((utxo) => [utxo, new UtxoMeta(new Date(0))])));
    });

    /**
     * Test value threshold filter application.
     *
     * @given Two states with UTXOs of different values and a value threshold filter
     * @when Computing delta with filter for values >= 200n
     * @then Should return only UTXOs meeting the threshold criteria
     */
    test('should apply value threshold filter', () => {
      const [added, removed] = state1.delta(state2, (utxo) => utxo.value >= 200n);

      const addedValues = Array.from(added).map((u) => u.value);
      const removedValues = Array.from(removed).map((u) => u.value);

      expect(compareBigIntArrays(sortBigIntArray(addedValues), [200n, 300n])).toBe(true);
      expect(compareBigIntArrays(sortBigIntArray(removedValues), [250n, 350n])).toBe(true);
    });

    /**
     * Test output number filter application.
     *
     * @given Two states with UTXOs having different output numbers
     * @when Computing delta with filter for outputNo <= 2
     * @then Should return only UTXOs from first state meeting the criteria
     */
    test('should apply output number filter', () => {
      const [added, removed] = state1.delta(state2, (utxo) => utxo.outputNo <= 2);

      const addedOutputNos = Array.from(added)
        .map((u) => u.outputNo)
        .sort((a, b) => a - b);
      const removedOutputNos = Array.from(removed)
        .map((u) => u.outputNo)
        .sort((a, b) => a - b);

      expect(addedOutputNos).toEqual([0, 1, 2]);
      expect(removedOutputNos).toEqual([]);
    });

    /**
     * Test owner-based filter application.
     *
     * @given States with UTXOs from different owners and an owner filter
     * @when Computing delta with filter for specific owner
     * @then Should return only UTXOs matching the owner criteria
     */
    test('should apply owner-based filter', () => {
      const specificOwner = sampleUserAddress();
      const utxosWithSpecificOwner = [
        {
          value: 100n,
          owner: specificOwner,
          type: Random.unshieldedTokenType().raw,
          intentHash: sampleIntentHash(),
          outputNo: 0
        }
      ];

      const stateWithSpecificOwner = UtxoState.new(
        new Map(utxosWithSpecificOwner.map((utxo) => [utxo, new UtxoMeta(new Date(0))]))
      );

      const [added, removed] = state1.delta(stateWithSpecificOwner, (utxo) => utxo.owner === specificOwner);

      expect(added.size).toEqual(0);
      expect(removed.size).toEqual(1);
      expect(Array.from(removed)[0].owner).toEqual(specificOwner);
    });

    /**
     * Test composite filter application.
     *
     * @given Two states with UTXOs and a composite filter for value and output number
     * @when Computing delta with filter for value >= 200n AND outputNo >= 1
     * @then Should return only UTXOs meeting both criteria
     */
    test('should apply composite filter (value and output number)', () => {
      const [added, removed] = state1.delta(state2, (utxo) => utxo.value >= 200n && utxo.outputNo >= 1);

      const addedValues = Array.from(added).map((u) => u.value);
      const removedValues = Array.from(removed).map((u) => u.value);

      expect(compareBigIntArrays(sortBigIntArray(addedValues), [200n, 300n])).toBe(true);
      expect(compareBigIntArrays(sortBigIntArray(removedValues), [250n, 350n])).toBe(true);
    });

    /**
     * Test filter that excludes all UTXOs.
     *
     * @given Two states and a filter that no UTXO can satisfy
     * @when Computing delta with filter for value > 1000n
     * @then Should return empty added and removed sets
     */
    test('should handle filter that excludes all UTXOs', () => {
      const [added, removed] = state1.delta(state2, (utxo) => utxo.value > 1000n); // No UTXOs have value > 1000

      expect(added.size).toEqual(0);
      expect(removed.size).toEqual(0);
    });

    /**
     * Test filter that includes all UTXOs.
     *
     * @given Two states and a filter that all UTXOs satisfy
     * @when Computing delta with filter for value >= 0n
     * @then Should return all UTXOs from both states
     */
    test('should handle filter that includes all UTXOs', () => {
      const [added, removed] = state1.delta(state2, (utxo) => utxo.value >= 0n); // All UTXOs have value >= 0

      expect(added.size).toEqual(3);
      expect(removed.size).toEqual(3);
    });
  });

  describe('Edge cases and error scenarios', () => {
    /**
     * Test UTXOs with same intent hash but different output numbers.
     *
     * @given Multiple UTXOs sharing the same intent hash but different output numbers
     * @when Creating UTXO state with these UTXOs
     * @then Should store all UTXOs and verify they share the same intent hash
     */
    test('should handle UTXOs with same intentHash but different outputNo', () => {
      const sharedIntentHash = sampleIntentHash();
      const utxos = [
        {
          value: 100n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sharedIntentHash,
          outputNo: 0
        },
        {
          value: 200n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sharedIntentHash,
          outputNo: 1
        },
        {
          value: 300n,
          owner: sampleUserAddress(),
          type: Random.unshieldedTokenType().raw,
          intentHash: sharedIntentHash,
          outputNo: 2
        }
      ];

      const utxoState = UtxoState.new(new Map(utxos.map((utxo) => [utxo, new UtxoMeta(new Date(0))])));

      expect(utxoState.utxos.size).toEqual(3);

      // Filter by intent hash would not be possible with current API,
      // but we can verify all UTXOs have the same intent hash
      Array.from(utxoState.utxos).forEach((utxo) => expect(utxo.intentHash).toEqual(sharedIntentHash));
    });

    /**
     * Test UTXOs with different token types.
     *
     * @given UTXOs with three different token types
     * @when Creating UTXO state with mixed token types
     * @then Should store all UTXOs and verify each has its correct token type
     */
    test('should handle UTXOs with different token types', () => {
      const tokenType1 = Random.unshieldedTokenType().raw;
      const tokenType2 = Random.unshieldedTokenType().raw;
      const tokenType3 = Random.unshieldedTokenType().raw;

      const utxos = [
        {
          value: 100n,
          owner: sampleUserAddress(),
          type: tokenType1,
          intentHash: sampleIntentHash(),
          outputNo: 0
        },
        {
          value: 200n,
          owner: sampleUserAddress(),
          type: tokenType2,
          intentHash: sampleIntentHash(),
          outputNo: 1
        },
        {
          value: 300n,
          owner: sampleUserAddress(),
          type: tokenType3,
          intentHash: sampleIntentHash(),
          outputNo: 2
        }
      ];

      const utxoState = UtxoState.new(new Map(utxos.map((utxo) => [utxo, new UtxoMeta(new Date(0))])));

      expect(utxoState.utxos.size).toEqual(3);

      const types = Array.from(utxoState.utxos).map((u) => u.type);
      expect(types).toContain(tokenType1);
      expect(types).toContain(tokenType2);
      expect(types).toContain(tokenType3);
    });

    /**
     * Test handling of very high output numbers.
     *
     * @given A UTXO with extremely high output number (999999)
     * @when Creating UTXO state with high output number
     * @then Should properly store and preserve the high output number
     */
    test('should handle very high output numbers', () => {
      const utxo: Utxo = {
        value: 100n,
        owner: sampleUserAddress(),
        type: Random.unshieldedTokenType().raw,
        intentHash: sampleIntentHash(),
        outputNo: 999999
      };

      const utxoState = UtxoState.new(new Map([[utxo, new UtxoMeta(new Date(0))]]));

      expect(utxoState.utxos.size).toEqual(1);
      expect(Array.from(utxoState.utxos)[0].outputNo).toEqual(999999);
    });
  });

  describe('Integration with transactions', () => {
    /**
     * Test UTXO tracking through spend and create cycle.
     *
     * @given A ledger state and transactions that create and spend UTXOs
     * @when Processing transactions that create initial UTXOs and then spend/create new ones
     * @then Should correctly track UTXO lifecycle with proper counts and delta calculation
     */
    test('should track UTXOs through spend and create cycle', () => {
      const ledgerState = new LedgerState('local-test', new ZswapChainState());
      const signingKey = sampleSigningKey();
      const verifyingKey = signatureVerifyingKey(signingKey);
      const address = addressFromKey(verifyingKey);

      // Create initial UTXOs
      const intent1 = Intent.new(Static.calcBlockTime(new Date(0), 50));
      intent1.guaranteedUnshieldedOffer = UnshieldedOffer.new(
        [],
        [
          {
            value: 1000n,
            owner: address,
            type: Random.unshieldedTokenType().raw
          }
        ],
        []
      );

      const tx1 = Transaction.fromParts('local-test', undefined, undefined, intent1);
      const proofErasedTx1 = tx1.eraseProofs();

      const blockContext = {
        secondsSinceEpoch: Static.blockTime(new Date(0)),
        secondsSinceEpochErr: 0,
        parentBlockHash: Static.parentBlockHash()
      };

      const strictness = new WellFormedStrictness();
      strictness.enforceBalancing = false;
      const verifiedTx1 = proofErasedTx1.wellFormed(ledgerState, strictness, new Date(0));
      const [ledgerStateAfter1, result1] = ledgerState.apply(
        verifiedTx1,
        new TransactionContext(ledgerState, blockContext, new Set())
      );

      expect(result1.type).toEqual('success');
      expect(ledgerStateAfter1.utxo.utxos.size).toEqual(1);
      expect(ledgerStateAfter1.utxo.filter(address).size).toEqual(1);

      // Now spend the UTXO and create new ones
      const createdUtxo = Array.from(ledgerStateAfter1.utxo.filter(address))[0];

      const intent2 = Intent.new(Static.calcBlockTime(new Date(0), 50));
      intent2.guaranteedUnshieldedOffer = UnshieldedOffer.new(
        [
          {
            value: createdUtxo.value,
            owner: verifyingKey,
            type: createdUtxo.type,
            intentHash: createdUtxo.intentHash,
            outputNo: createdUtxo.outputNo
          }
        ],
        [
          {
            value: 600n,
            owner: address,
            type: createdUtxo.type
          },
          {
            value: 400n,
            owner: sampleUserAddress(),
            type: createdUtxo.type
          }
        ],
        [signData(signingKey, new Uint8Array(32))]
      );

      const tx2 = Transaction.fromParts('local-test', undefined, undefined, intent2);
      const proofErasedTx2 = tx2.eraseProofs();

      const verifiedTx2 = proofErasedTx2.wellFormed(ledgerStateAfter1, strictness, new Date(0));
      const [ledgerStateAfter2, result2] = ledgerStateAfter1.apply(
        verifiedTx2,
        new TransactionContext(ledgerStateAfter1, blockContext, new Set())
      );

      expect(result2.type).toEqual('success');
      expect(ledgerStateAfter2.utxo.utxos.size).toEqual(2);
      expect(ledgerStateAfter2.utxo.filter(address).size).toEqual(1);

      // Verify the delta between states
      const [added, removed] = ledgerStateAfter2.utxo.delta(ledgerStateAfter1.utxo);
      expect(added.size).toEqual(2); // Two new UTXOs created
      expect(removed.size).toEqual(1); // One UTXO spent
    });
  });
});
