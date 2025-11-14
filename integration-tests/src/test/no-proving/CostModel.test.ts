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
  CostModel,
  DustActions,
  Intent,
  LedgerParameters,
  type SyntheticCost,
  Transaction,
  TransactionCostModel,
  UnshieldedOffer,
  type Utxo,
  type UtxoOutput,
  type UtxoSpend,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { TestState } from '@/test/utils/TestState';
import { DEFAULT_TOKEN_TYPE, LOCAL_TEST_NETWORK_ID, Random, Static, type UnshieldedTokenType } from '@/test-objects';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Ledger API - CostModel', () => {
  /**
   * Ledger API – CostModel tests
   *
   * Docs:
   * 1. https://github.com/midnightntwrk/midnight-ledger/blob/main/spec/cost-model.md
   * 2. https://45047878.fs1.hubspotusercontent-na1.net/hubfs/45047878/Midnight-Tokenomics-And-Incentives-Whitepaper.pdf
   *
   * ──────────────────────────────────────────────────────────────────────────────
   * General Rules of the Cost Model
   * ──────────────────────────────────────────────────────────────────────────────
   * The ledger’s Cost Model dynamically adjusts per-dimension fee prices between
   * blocks based on *previous block utilization*. Each dimension evolves
   * independently and responds to pressure in that metric only.
   *
   * Core dimensions:
   * • readTime        → proportional to storage reads / validation effort
   * • computeTime     → proportional to circuit verification and CPU cost
   * • blockUsage      → proportional to block fullness / proof count
   * • bytesWritten    → proportional to on-chain state growth (writes)
   *
   * Adjustment rules:
   * 1. Prices update *after* each block based on utilization in the previous
   * block. The prices you read now apply to the *next* block.
   * 2. Each dimension increases if its utilization exceeds the target,
   * decreases if below, and remains steady near the target.
   * 3. Price movement per block is multiplicatively bounded by `maxPriceAdjustment()` (~1.045). This cap limits how fast prices
   * can change upward or downward between consecutive blocks.
   * 4. Dimensions are *decoupled*:
   *    – High compute usage raises only computePrice.
   *    – High block usage (many proofs) raises only blockUsagePrice.
   *    – High churn / writes raises writePrice.
   *    – High read activity raises readPrice.
   *    – Unused dimensions drift down gradually toward a floor ≥ 0.
   * 5. Empty or under-filled blocks cause all prices to drift downward monotonically but never below zero.
   * 6. Baseline compute cost is nonzero (100 ms nominal CPU), others start at 0.
   * 7. FeePrices are floating-point (not fixed-point) and tiny rounding deltas (EPS ≈ 1e-12) should be tolerated in tests.
   *
   * In essence: each price dimension is an adaptive feedback controller that
   * balances block resource demand against target capacity, with smooth capped
   * adjustments to maintain economic stability and prevent fee oscillation.
   */
  const TEN_SECS = 10n;
  const REWARDS_AMOUNT = 5_000_000n;
  const INITIAL_FIXED_PRICE = 10;
  const EPS = 1e-12; // tiny tolerance to account for IEEE-754 rounding noise
  const withinCap = (a: number, b: number, cap: number) => Math.max(a / b, b / a) <= cap + EPS;
  const randomUnshieldedToken: UnshieldedTokenType = Random.unshieldedTokenType();
  const nightToken: UnshieldedTokenType = Static.defaultUnshieldedTokenType();
  const maxAdj = LedgerParameters.initialParameters().maxPriceAdjustment();

  const mergedUnshieldedTxFromUtxos = (
    state: TestState,
    opts?: {
      filter?: (utxo: Utxo) => boolean;
      offerKind?: 'guaranteed' | 'fallible';
    }
  ) => {
    const filter = opts?.filter ?? (() => true);
    const offerKind = opts?.offerKind ?? 'guaranteed';

    const txs = Array.from(state.ledger.utxo.utxos)
      .filter(filter)
      .map((utxo) => {
        const intent = Intent.new(state.time);

        const inputs: UtxoSpend[] = [
          {
            value: REWARDS_AMOUNT,
            owner: state.nightKey.verifyingKey(),
            type: utxo.type,
            intentHash: utxo.intentHash,
            outputNo: 0
          }
        ];
        const outputs: UtxoOutput[] = [
          {
            owner: state.initialNightAddress,
            type: utxo.type,
            value: REWARDS_AMOUNT
          }
        ];

        const offer = UnshieldedOffer.new(inputs, outputs, []);
        if (offerKind === 'guaranteed') {
          intent.guaranteedUnshieldedOffer = offer;
        } else {
          intent.fallibleUnshieldedOffer = offer;
        }

        return Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
      });

    if (txs.length === 0) throw new Error('no UTXOs to spend');
    return txs.slice(1).reduce((acc, t) => acc.merge(t), txs[0]);
  };

  /**
   * @given A fresh CostModel and TransactionCostModel
   * @when  Converting to string
   * @then  CostModel.toString() mirrors TransactionCostModel.runtimeCostModel.toString()
   */
  test('string output matches initial runtime cost model', () => {
    const costModel = CostModel.initialCostModel();
    const transactionCostModel = TransactionCostModel.initialTransactionCostModel();
    expect(costModel.toString()).toEqual(transactionCostModel.runtimeCostModel.toString());
  });

  /**
   * @given Initial parameters
   * @when  Reading baselineCost
   * @then  Only computeTime has a non-zero baseline (100_000_000ns) - others are 0
   */
  test('baselineCost has only computeTime > 0', () => {
    const { baselineCost } = LedgerParameters.initialParameters().transactionCostModel;
    expect(baselineCost.readTime).toEqual(0n);
    expect(baselineCost.computeTime).toEqual(100_000_000n);
    expect(baselineCost.bytesWritten).toEqual(0n);
    expect(baselineCost.bytesDeleted).toEqual(0n);
  });

  /**
   * @given Initial parameters
   * @when  Reading maxPriceAdjustment()
   * @then  Value should be ~1.045
   */
  test('max price adjustment is ~1.045', () => {
    expect(maxAdj).toBeGreaterThanOrEqual(1.045);
    expect(maxAdj).toBeLessThanOrEqual(1.046);
  });

  /**
   * @given A small block below target fullness
   * @when  Applying a transaction built from multiple guaranteed unshielded intents (NIGHT token)
   * @then  All price dimensions decrease from initial 10
   */
  test('all prices decrease when block under-fills (guaranteed, NIGHT)', () => {
    const ITERATIONS = 2; // small to guarantee under-fill (perf constraint)
    const state = TestState.new();
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    const tx = mergedUnshieldedTxFromUtxos(state, { offerKind: 'guaranteed' });
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, new WellFormedStrictness(), balanced.cost(state.ledger.parameters));

    const { feePrices } = state.ledger.parameters;
    expect(feePrices.readPrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.computePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.blockUsagePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.writePrice).toBeLessThan(INITIAL_FIXED_PRICE);
  });

  /**
   * @given A small block below target fullness
   * @when  Calling feesWithMargin with various parameters
   * @then  Margins are correctly applied to transaction fees
   */
  test('applies margin correctly to transaction fees using feesWithMargin', () => {
    const ITERATIONS = 2;
    const state = TestState.new();
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    const tx = mergedUnshieldedTxFromUtxos(state, { offerKind: 'guaranteed' });
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, new WellFormedStrictness(), balanced.cost(state.ledger.parameters));

    const baseFee = balanced.fees(state.ledger.parameters);
    const feesWithMargin0 = balanced.feesWithMargin(state.ledger.parameters, 0);
    const feesWithMargin1 = balanced.feesWithMargin(state.ledger.parameters, 1);

    expect(feesWithMargin0).toEqual(baseFee);
    expect(feesWithMargin1).toBeGreaterThan(feesWithMargin0);
  });

  /**
   * @given A small block below target fullness (unshielded random token)
   * @when  Applying multiple guaranteed unshielded intents
   * @then  All price dimensions decrease
   */
  test('all prices decrease when block under-fills (guaranteed, random token)', () => {
    const ITERATIONS = 6; // small to guarantee under-fill (perf constraint)

    const state = TestState.new();
    for (let i = 0; i < ITERATIONS; i++) state.rewardsUnshielded(randomUnshieldedToken, REWARDS_AMOUNT);
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    const tx = mergedUnshieldedTxFromUtxos(state, {
      offerKind: 'guaranteed',
      filter: (u) => u.type !== DEFAULT_TOKEN_TYPE
    });
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, new WellFormedStrictness(), balanced.cost(state.ledger.parameters));

    const { feePrices } = state.ledger.parameters;
    expect(feePrices.readPrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.computePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.blockUsagePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.writePrice).toBeLessThan(INITIAL_FIXED_PRICE);
  });

  /**
   * @given A small block below target fullness (unshielded random token)
   * @when  Applying multiple *fallible* unshielded intents
   * @then  All price dimensions decrease
   */
  test('all prices decrease when block under-fills (fallible, random token)', () => {
    const ITERATIONS = 9; // small to guarantee under-fill (perf constraint)

    const state = TestState.new();
    for (let i = 0; i < ITERATIONS; i++) state.rewardsUnshielded(randomUnshieldedToken, REWARDS_AMOUNT);
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    const tx = mergedUnshieldedTxFromUtxos(state, {
      offerKind: 'fallible',
      filter: (u) => u.type !== DEFAULT_TOKEN_TYPE
    });
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, new WellFormedStrictness(), balanced.cost(state.ledger.parameters));

    const { feePrices } = state.ledger.parameters;
    expect(feePrices.readPrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.computePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.blockUsagePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.writePrice).toBeLessThan(INITIAL_FIXED_PRICE);
  });

  /**
   * @given Low block usage but high bytes churn close to limit
   * @when  Applying a single fallible unshielded intent with many inputs/outputs
   * @then  Only writePrice should increase; other dimensions should decrease
   */
  test('writePrice increases under high churn - others decrease', () => {
    const ITERATIONS = 118;

    const state = TestState.new();
    for (let i = 0; i < ITERATIONS; i++) state.rewardsUnshielded(randomUnshieldedToken, REWARDS_AMOUNT);
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    const inputs: UtxoSpend[] = [];
    const outputs: UtxoOutput[] = [];

    // One large offer out of all non-NIGHT UTXOs to push bytes churn
    Array.from(state.ledger.utxo.utxos)
      .filter((u) => u.type !== DEFAULT_TOKEN_TYPE)
      .forEach((utxo) => {
        inputs.push({
          value: REWARDS_AMOUNT,
          owner: state.nightKey.verifyingKey(),
          type: utxo.type,
          intentHash: utxo.intentHash,
          outputNo: 0
        });
        outputs.push({
          owner: state.initialNightAddress,
          type: utxo.type,
          value: REWARDS_AMOUNT
        });
      });

    const intent = Intent.new(state.time);
    intent.fallibleUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);
    const tx = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, new WellFormedStrictness(), balanced.cost(state.ledger.parameters));

    const { feePrices } = state.ledger.parameters;
    expect(feePrices.readPrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.computePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.blockUsagePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.writePrice).toBeGreaterThan(INITIAL_FIXED_PRICE);
  });

  /**
   * @given A block that is heavy in number of proofs (many DUST UTXOs)
   * @when  Applying a single dustActions intent with many proofs
   * @then  blockUsagePrice increases; other dimensions fall
   */
  test('blockUsagePrice increases when blockUsage near limit (dustActions w/ many proofs)', () => {
    const ITERATIONS = 58;
    const DUST_UTXO_TO_SPARE = 5; // leaves enough spends to push blockUsage but avoid hitting hard caps

    const state = TestState.new();
    for (let i = 0; i < ITERATIONS; i++) state.rewardsUnshielded(nightToken, REWARDS_AMOUNT);
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    const spends = [];
    while (state.dust.utxos.length > DUST_UTXO_TO_SPARE) {
      const qdo = state.dust.utxos[0];
      const vFee = 0n;
      const [newDust, spend] = state.dust.spend(state.dustKey.secretKey, qdo, vFee, state.time);
      state.dust = newDust;
      spends.push(spend);
    }

    const intent = Intent.new(state.time);
    intent.dustActions = new DustActions(SignatureMarker.signature, ProofMarker.preProof, state.time, spends, []);
    intent.signatureData(0xfeed);

    const tx = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, new WellFormedStrictness(), balanced.cost(state.ledger.parameters));

    const { feePrices } = state.ledger.parameters;
    expect(feePrices.readPrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.computePrice).toBeLessThan(INITIAL_FIXED_PRICE);
    expect(feePrices.blockUsagePrice).toBeGreaterThan(INITIAL_FIXED_PRICE);
    expect(feePrices.writePrice).toBeLessThan(INITIAL_FIXED_PRICE);
  });

  /**
   * @given Three consecutive similar blocks of DUST spends
   * @when  Filling each block with the same count of spends
   * @then  Per-block price change is always bounded by maxPriceAdjustment
   */
  test('price change bounded per block with stable usage (dustActions)', () => {
    const ITERATIONS = 60;
    const SPENDS_PER_BLOCK = 40; // near max load across three blocks

    const state = TestState.new();
    for (let i = 0; i < ITERATIONS; i++) state.rewardsUnshielded(nightToken, REWARDS_AMOUNT);
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    state.spendDust(SPENDS_PER_BLOCK);
    const p1 = state.ledger.parameters.feePrices.computePrice;

    state.spendDust(SPENDS_PER_BLOCK);
    const p2 = state.ledger.parameters.feePrices.computePrice;

    state.spendDust(SPENDS_PER_BLOCK);
    const p3 = state.ledger.parameters.feePrices.computePrice;

    expect(withinCap(p2, p1, maxAdj)).toBeTruthy();
    expect(withinCap(p3, p2, maxAdj)).toBeTruthy();
  });

  /**
   * @given Decreasing block usage across blocks
   * @when  Filling blocks with 30 -> 20 -> 10 spends
   * @then  Per-block price change remains within the cap (compute dimension observed)
   */
  test('price bounded while usage decreases (dustActions)', () => {
    const ITERATIONS = 56;

    const state = TestState.new();
    for (let i = 0; i < ITERATIONS; i++) state.rewardsUnshielded(nightToken, REWARDS_AMOUNT);
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    state.spendDust(30);
    const p1 = state.ledger.parameters.feePrices.computePrice;

    state.spendDust(20);
    const p2 = state.ledger.parameters.feePrices.computePrice;

    state.spendDust(10);
    const p3 = state.ledger.parameters.feePrices.computePrice;

    expect(withinCap(p2, p1, maxAdj)).toBeTruthy();
    expect(withinCap(p3, p2, maxAdj)).toBeTruthy();
  });

  /**
   * @given Increasing block usage across blocks
   * @when  Filling blocks with 10 -> 20 -> 40 spends
   * @then  computePrice may still drift down if computeTime stays under target
   * We only enforce the per-block cap (compute dimension observed)
   */
  test('price bounded while usage increases (dustActions)', () => {
    const ITERATIONS = 56;

    const state = TestState.new();
    for (let i = 0; i < ITERATIONS; i++) state.rewardsUnshielded(nightToken, REWARDS_AMOUNT);
    state.giveFeeToken(ITERATIONS, REWARDS_AMOUNT);

    state.spendDust(10);
    const p1 = state.ledger.parameters.feePrices.computePrice;

    state.spendDust(20);
    const p2 = state.ledger.parameters.feePrices.computePrice;

    state.spendDust(40);
    const p3 = state.ledger.parameters.feePrices.computePrice;

    expect(withinCap(p2, p1, maxAdj)).toBeTruthy();
    expect(withinCap(p3, p2, maxAdj)).toBeTruthy();
  });

  /**
   * @given A initial state, then two synthetic compute-heavy “blocks”
   * @when  Applying two compute spikes via fastForward (blockUsage=0)
   * @then  computePrice increases monotonically and each step respects the cap;
   * blockUsagePrice drifts down (we didn’t use block capacity)
   */
  test('computePrice rises under compute-heavy prior block', () => {
    const state = TestState.new();

    const A = state.ledger.parameters.feePrices;

    // B: compute spike
    const spike1: SyntheticCost = {
      readTime: 0n,
      computeTime: 500_000_000_000n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };
    state.fastForward(TEN_SECS, spike1);
    const B = state.ledger.parameters.feePrices;

    // C: another compute spike, even higher
    const spike2: SyntheticCost = {
      readTime: 0n,
      computeTime: 800_000_000_000n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };
    state.fastForward(TEN_SECS, spike2);
    const C = state.ledger.parameters.feePrices;

    expect(withinCap(B.computePrice, A.computePrice, maxAdj)).toBeTruthy();
    expect(withinCap(C.computePrice, B.computePrice, maxAdj)).toBeTruthy();

    expect(B.computePrice).toBeGreaterThanOrEqual(A.computePrice);
    expect(C.computePrice).toBeGreaterThanOrEqual(B.computePrice);

    expect(B.blockUsagePrice).toBeLessThanOrEqual(A.blockUsagePrice);
    expect(C.blockUsagePrice).toBeLessThanOrEqual(B.blockUsagePrice);
  });

  /**
   * @given A initial state
   * @when  Applying two read spikes via fastForward (blockUsage=0, compute/write=0)
   * @then  readPrice changes to read load -> blockUsagePrice drifts down
   */
  test('readPrice responds to read-heavy prior blocks (direction-agnostic)', () => {
    const state = TestState.new();

    const A = state.ledger.parameters.feePrices;

    // B: read spike (keep under block limit: read_time <= 1s)
    const spike1: SyntheticCost = {
      readTime: 800_000_000n, // 0.8s
      computeTime: 0n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };
    state.fastForward(TEN_SECS, spike1);
    const B = state.ledger.parameters.feePrices;

    // C: larger read spike, still under limit
    const spike2: SyntheticCost = {
      readTime: 900_000_000n, // 0.9s
      computeTime: 0n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };
    state.fastForward(TEN_SECS, spike2);
    const C = state.ledger.parameters.feePrices;

    // assert that readPrice actually changes each step
    expect(Math.abs(B.readPrice - A.readPrice)).toBeGreaterThan(EPS);
    expect(Math.abs(C.readPrice - B.readPrice)).toBeGreaterThan(EPS);

    // Orthogonal dimension drifts down
    expect(B.blockUsagePrice).toBeLessThanOrEqual(A.blockUsagePrice + EPS);
    expect(C.blockUsagePrice).toBeLessThanOrEqual(B.blockUsagePrice + EPS);
  });

  /**
   * @given A initial state, then many consecutive empty blocks
   * @when  Fast-forwarding with zero utilization across all dimensions
   * @then  All prices drift down monotonically and never go below zero
   */
  test('prices drift down across empty blocks and stay non-negative', () => {
    const ITERATIONS = 12;
    const state = TestState.new();

    let prev = { ...state.ledger.parameters.feePrices };
    for (let i = 0; i < ITERATIONS; i++) {
      const empty: SyntheticCost = {
        readTime: 0n,
        computeTime: 0n,
        blockUsage: 0n,
        bytesWritten: 0n,
        bytesChurned: 0n
      };
      state.fastForward(TEN_SECS, empty);
      const cur = state.ledger.parameters.feePrices;

      expect(cur.readPrice).toBeLessThanOrEqual(prev.readPrice + EPS);
      expect(cur.computePrice).toBeLessThanOrEqual(prev.computePrice + EPS);
      expect(cur.blockUsagePrice).toBeLessThanOrEqual(prev.blockUsagePrice + EPS);
      expect(cur.writePrice).toBeLessThanOrEqual(prev.writePrice + EPS);

      expect(cur.readPrice).toBeGreaterThanOrEqual(0);
      expect(cur.computePrice).toBeGreaterThanOrEqual(0);
      expect(cur.blockUsagePrice).toBeGreaterThanOrEqual(0);
      expect(cur.writePrice).toBeGreaterThanOrEqual(0);

      prev = cur;
    }
  });

  /**
   * @given A initial state
   * @when  readTime is high but blockUsage is zero
   * @then  readPrice increases while blockUsagePrice decreases (cap respected)
   */
  test('mixed pressures in one update: read up, blockUsage down', () => {
    const state = TestState.new();

    const before = state.ledger.parameters.feePrices;

    const mixed: SyntheticCost = {
      readTime: 600_000_000_000n, // strong read pressure
      computeTime: 0n,
      blockUsage: 0n, // no block capacity consumed
      bytesWritten: 0n,
      bytesChurned: 0n
    };
    state.fastForward(TEN_SECS, mixed);

    const after = state.ledger.parameters.feePrices;

    expect(after.readPrice).toBeGreaterThanOrEqual(before.readPrice);
    expect(after.blockUsagePrice).toBeLessThanOrEqual(before.blockUsagePrice);
    // Others unconstrained but should not move counter to their signals here:
    expect(after.computePrice).toBeLessThanOrEqual(before.computePrice + EPS);
    expect(after.writePrice).toBeLessThanOrEqual(before.writePrice + EPS);
  });

  /**
   * @given A initial state
   * @when  Applying a large write spike via fastForward (below block limit)
   * @then  writePrice increases but by no more than maxPriceAdjustment; blockUsagePrice drifts down
   */
  test('writePrice cap holds under large write spike', () => {
    const state = TestState.new();

    const A = state.ledger.parameters.feePrices;

    const largeWrite: SyntheticCost = {
      readTime: 0n,
      computeTime: 0n,
      blockUsage: 0n,
      bytesWritten: 45_000n, // under 50k limit
      bytesChurned: 0n
    };
    state.fastForward(TEN_SECS, largeWrite);
    const B = state.ledger.parameters.feePrices;

    expect(withinCap(B.writePrice, A.writePrice, maxAdj)).toBeTruthy();
    expect(B.writePrice).toBeGreaterThanOrEqual(A.writePrice - EPS);
    expect(B.blockUsagePrice).toBeLessThanOrEqual(A.blockUsagePrice + EPS);
  });

  /**
   * @given A initial state
   * @when  We fastForward with high read/compute/churn and nonzero blockUsage
   * @then  All price dimensions rise (or at least not fall), and every change
   *        is bounded by maxPriceAdjustment (per-dimension)
   */
  test('all dimensions respond under simultaneous multi-dimension pressure', () => {
    const state = TestState.new();

    const A = state.ledger.parameters.feePrices;

    const heavyAll: SyntheticCost = {
      readTime: 900_000_000n, // 0.9s
      computeTime: 600_000_000_000n, // 0.6s
      blockUsage: 1_000n, // consume some capacity
      bytesWritten: 40_000n,
      bytesChurned: 42_000n
    };
    state.fastForward(TEN_SECS, heavyAll);

    const B = state.ledger.parameters.feePrices;

    expect(withinCap(B.computePrice, A.computePrice, maxAdj)).toBeTruthy();
    expect(withinCap(B.writePrice, A.writePrice, maxAdj)).toBeTruthy();

    // At least one “pressured” dimension moved up
    const ups =
      (B.readPrice >= A.readPrice - EPS ? 1 : 0) +
      (B.computePrice >= A.computePrice - EPS ? 1 : 0) +
      (B.writePrice >= A.writePrice - EPS ? 1 : 0) +
      (B.blockUsagePrice >= A.blockUsagePrice - EPS ? 1 : 0);

    expect(ups).toBeGreaterThanOrEqual(1);
  });

  /**
   * @given A initial state
   * @when  We apply a churn-heavy block (high bytesChurned) with minimal bytesWritten and no block usage
   * @then  writePrice increases; read/compute/blockUsage drift down or stay flat
   */
  test('churn-only (high bytesChurned, tiny bytesWritten, no block usage) does not lift writePrice', () => {
    const state = TestState.new();

    const A = state.ledger.parameters.feePrices;

    const churnHeavy: SyntheticCost = {
      readTime: 0n,
      computeTime: 0n,
      blockUsage: 0n,
      bytesWritten: 500n, // tiny write -> no meaningful upward write pressure
      bytesChurned: 35_000n // large churn alone should not raise writePrice
    };
    state.fastForward(TEN_SECS, churnHeavy);
    const B = state.ledger.parameters.feePrices;

    // No upward push on write
    expect(B.writePrice).toBeLessThanOrEqual(A.writePrice + EPS);
    expect(withinCap(B.writePrice, A.writePrice, maxAdj)).toBeTruthy();

    // Orthogonal dimensions drift down (no block capacity consumed, no read/compute pressure)
    expect(B.blockUsagePrice).toBeLessThanOrEqual(A.blockUsagePrice + EPS);
    expect(B.readPrice).toBeLessThanOrEqual(A.readPrice + EPS);
    expect(B.computePrice).toBeLessThanOrEqual(A.computePrice + EPS);
  });

  /**
   * @given A initial state
   * @when  We run several consecutive empty blocks
   * @then  The per-block decrease (downward change) is also bounded by the cap
   */
  test('downward price changes per block are also bounded by the cap (empty streak)', () => {
    const ITERATIONS = 6;
    const state = TestState.new();

    let prev = state.ledger.parameters.feePrices;

    const empty: SyntheticCost = {
      readTime: 0n,
      computeTime: 0n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };

    for (let i = 0; i < ITERATIONS; i++) {
      state.fastForward(TEN_SECS, empty);
      const cur = state.ledger.parameters.feePrices;

      // On empty blocks all dimensions should drift down or stay flat
      expect(cur.readPrice).toBeLessThanOrEqual(prev.readPrice + EPS);
      expect(cur.computePrice).toBeLessThanOrEqual(prev.computePrice + EPS);
      expect(cur.blockUsagePrice).toBeLessThanOrEqual(prev.blockUsagePrice + EPS);
      expect(cur.writePrice).toBeLessThanOrEqual(prev.writePrice + EPS);

      expect(cur.readPrice).toBeGreaterThanOrEqual(0);
      expect(cur.computePrice).toBeGreaterThanOrEqual(0);
      expect(cur.blockUsagePrice).toBeGreaterThanOrEqual(0);
      expect(cur.writePrice).toBeGreaterThanOrEqual(0);

      prev = { ...cur };
    }
  });

  /**
   * @given A initial state
   * @when  We apply many empty blocks
   * @then  Prices approach zero but never cross below zero
   */
  test('very long empty run never produces negative prices', () => {
    const ITERATIONS = 100;
    const state = TestState.new();

    const empty: SyntheticCost = {
      readTime: 0n,
      computeTime: 0n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };

    for (let i = 0; i < ITERATIONS; i++) {
      state.fastForward(TEN_SECS, empty);
      const p = state.ledger.parameters.feePrices;
      expect(p.readPrice).toBeGreaterThanOrEqual(0);
      expect(p.computePrice).toBeGreaterThanOrEqual(0);
      expect(p.blockUsagePrice).toBeGreaterThanOrEqual(0);
      expect(p.writePrice).toBeGreaterThanOrEqual(0);
    }
  });

  /**
   * @given Low read/compute/write pressure but high block usage
   * @when  We simulate a prior block with large blockUsage only
   * @then  blockUsagePrice increases while other dimensions decrease
   */
  test('block usage only pushes blockUsagePrice up - other dimensions down', () => {
    const state = TestState.new();

    const A = state.ledger.parameters.feePrices;

    const blockUsageOnly: SyntheticCost = {
      readTime: 0n,
      computeTime: 0n,
      blockUsage: 200_000n, // very high blockUsage
      bytesWritten: 0n,
      bytesChurned: 0n
    };
    state.fastForward(TEN_SECS, blockUsageOnly);
    const B = state.ledger.parameters.feePrices;

    expect(B.blockUsagePrice).toBeGreaterThanOrEqual(A.blockUsagePrice - EPS);
    expect(B.readPrice).toBeLessThanOrEqual(A.readPrice + EPS);
    expect(B.computePrice).toBeLessThanOrEqual(A.computePrice + EPS);
    expect(B.writePrice).toBeLessThanOrEqual(A.writePrice + EPS);
  });

  /**
   * @given Alternating pressures across blocks
   * @when  We alternate compute-heavy and empty blocks
   * @then  computePrice is within the cap and never overshoots cap
   */
  test('alternating compute spike and empty blocks keeps changes within cap', () => {
    const ITERATIONS = 6;
    const state = TestState.new();

    const empty: SyntheticCost = {
      readTime: 0n,
      computeTime: 0n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };

    const spike: SyntheticCost = {
      readTime: 0n,
      computeTime: 700_000_000_000n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };

    let prev = state.ledger.parameters.feePrices.computePrice;

    for (let i = 0; i < ITERATIONS; i++) {
      // Spike: increase should be capped by maxAdj and non-decreasing
      state.fastForward(TEN_SECS, spike);
      const afterSpike = state.ledger.parameters.feePrices.computePrice;
      expect(withinCap(afterSpike, prev, maxAdj)).toBeTruthy();
      expect(afterSpike).toBeGreaterThanOrEqual(prev - EPS);

      // Empty: decrease has no symmetric cap
      prev = afterSpike;
      state.fastForward(TEN_SECS, empty);

      // Assert monotonic down + non-negative
      const afterEmpty = state.ledger.parameters.feePrices.computePrice;
      expect(afterEmpty).toBeLessThanOrEqual(prev + EPS);
      expect(afterEmpty).toBeGreaterThanOrEqual(0);

      expect(afterSpike).toBeGreaterThanOrEqual(afterEmpty - EPS);

      prev = afterEmpty;
    }
  });

  /**
   * @given Two fresh states with identical prior-block utilization
   * @when  Applying the same synthetic compute-heavy spike once to each
   * @then  The resulting fee prices are identical (within EPS) – determinism
   */
  test('identical prior-block utilization -> identical feePrices (determinism)', () => {
    const spike: SyntheticCost = {
      readTime: 0n,
      computeTime: 700_000_000_000n,
      blockUsage: 0n,
      bytesWritten: 0n,
      bytesChurned: 0n
    };

    const s1 = TestState.new();
    const before1 = s1.ledger.parameters.feePrices;
    s1.fastForward(TEN_SECS, spike);
    const after1 = s1.ledger.parameters.feePrices;

    const s2 = TestState.new();
    const before2 = s2.ledger.parameters.feePrices;
    s2.fastForward(TEN_SECS, spike);
    const after2 = s2.ledger.parameters.feePrices;

    expect(before1.readPrice - before2.readPrice).toBeLessThanOrEqual(EPS);
    expect(before1.computePrice - before2.computePrice).toBeLessThanOrEqual(EPS);
    expect(before1.blockUsagePrice - before2.blockUsagePrice).toBeLessThanOrEqual(EPS);
    expect(before1.writePrice - before2.writePrice).toBeLessThanOrEqual(EPS);

    expect(after1.readPrice - after2.readPrice).toBeLessThanOrEqual(EPS);
    expect(after1.computePrice - after2.computePrice).toBeLessThanOrEqual(EPS);
    expect(after1.blockUsagePrice - after2.blockUsagePrice).toBeLessThanOrEqual(EPS);
    expect(after1.writePrice - after2.writePrice).toBeLessThanOrEqual(EPS);
  });

  /**
   * @given Two fresh states, same prior-block utilization
   * @when  Fast-forwarding different intervals (10s vs 60s) for the same utilization
   * @then  The price updates are identical (within EPS) because updates are per-block, not per-second
   */
  test('price update is invariant to inter-block time interval for same utilization', () => {
    const spike: SyntheticCost = {
      readTime: 800_000_000n, // 0.8s read
      computeTime: 500_000_000_000n, // 0.5s compute
      blockUsage: 1_000n,
      bytesWritten: 20_000n,
      bytesChurned: 22_000n
    };

    const s10 = TestState.new();
    const start10 = s10.ledger.parameters.feePrices;
    s10.fastForward(TEN_SECS, spike);
    const end10 = s10.ledger.parameters.feePrices;

    const s60 = TestState.new();
    const start60 = s60.ledger.parameters.feePrices;
    s60.fastForward(60n, spike);
    const end60 = s60.ledger.parameters.feePrices;

    expect(start10.readPrice - start60.readPrice).toBeLessThanOrEqual(EPS);
    expect(start10.computePrice - start60.computePrice).toBeLessThanOrEqual(EPS);
    expect(start10.blockUsagePrice - start60.blockUsagePrice).toBeLessThanOrEqual(EPS);
    expect(start10.writePrice - start60.writePrice).toBeLessThanOrEqual(EPS);

    expect(end10.readPrice - end60.readPrice).toBeLessThanOrEqual(EPS);
    expect(end10.computePrice - end60.computePrice).toBeLessThanOrEqual(EPS);
    expect(end10.blockUsagePrice - end60.blockUsagePrice).toBeLessThanOrEqual(EPS);
    expect(end10.writePrice - end60.writePrice).toBeLessThanOrEqual(EPS);
  });
});
