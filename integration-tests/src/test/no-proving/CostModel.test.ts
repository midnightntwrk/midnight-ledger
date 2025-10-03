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
  type DustSpend,
  Intent,
  LedgerParameters,
  type PreProof,
  type QualifiedDustOutput,
  type SignatureEnabled,
  Transaction,
  UnshieldedOffer,
  type UtxoOutput,
  type UtxoSpend,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { expect } from 'vitest';
import { TestState } from '@/test/utils/TestState';
import { DEFAULT_TOKEN_TYPE, LOCAL_TEST_NETWORK_ID, Random, Static, type UnshieldedTokenType } from '@/test-objects';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Ledger API - CostModel', () => {
  /**
   * Test string representation of CostModel.
   *
   * @given A new CostModel instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const costModel = CostModel.initialCostModel();

    const expected = `CostModel {
    noop_constant: 58.049ns,
    noop_coeff_arg: 0.000s,
    lt: 1.005μs,
    eq: 1.006μs,
    type_null: 647.378ns,
    type_cell: 757.159ns,
    type_map: 627.432ns,
    type_bmt: 749.668ns,
    type_array: 583.371ns,
    size_map: 1.600μs,
    size_bmt: 725.665ns,
    size_array: 1.350μs,
    new_null: 374.871ns,
    new_cell: 814.951ns,
    new_map: 978.652ns,
    new_bmt: 845.753ns,
    new_array: 842.371ns,
    and: 784.624ns,
    or: 781.084ns,
    neg: 836.890ns,
    log_null_constant: 155.124ns,
    log_null_coeff_value_size: 0.000s,
    log_cell_constant: 314.563ns,
    log_cell_coeff_value_size: 0.000s,
    log_map_constant: 1.820μs,
    log_map_coeff_value_size: 0.000s,
    log_bmt_constant: 303.821ns,
    log_bmt_coeff_value_size: 1ps,
    log_array_constant: 610.437ns,
    log_array_coeff_value_size: 18.027ns,
    root: 777.264ns,
    pop: 574.527ns,
    popeq_constant: 385.050ns,
    popeq_coeff_value_size: 15ps,
    popeqc_constant: 383.608ns,
    popeqc_coeff_value_size: 14ps,
    addi: 816.807ns,
    subi: 817.066ns,
    push_null: 624.886ns,
    push_cell: 235.744ns,
    push_map: 5.764μs,
    push_bmt: 238.126ns,
    push_array: 737.159ns,
    pushs_null: 146.496ns,
    pushs_cell: 235.886ns,
    pushs_map: 1.006μs,
    pushs_bmt: 245.902ns,
    pushs_array: 742.982ns,
    branch_constant: 314.875ns,
    branch_coeff_arg: 8ps,
    jmp_constant: 54.347ns,
    jmp_coeff_arg: 3ps,
    add: 1.061μs,
    sub: 1.058μs,
    concat_constant: 1.283μs,
    concat_coeff_total_size: 446ps,
    concatc_constant: 1.294μs,
    concatc_coeff_total_size: 447ps,
    member_constant: 2.187μs,
    member_coeff_key_size: 1.196ns,
    member_coeff_container_log_size: 1.451ns,
    rem_map_constant: 2.637μs,
    rem_map_coeff_key_size: 2.244ns,
    rem_map_coeff_container_log_size: 927.384ns,
    rem_bmt_constant: 1.233μs,
    rem_bmt_coeff_key_size: 0.000s,
    rem_bmt_coeff_container_log_size: 51.495μs,
    remc_map_constant: 2.530μs,
    remc_map_coeff_key_size: 2.176ns,
    remc_map_coeff_container_log_size: 927.358ns,
    remc_bmt_constant: 1.771μs,
    remc_bmt_coeff_key_size: 0.000s,
    remc_bmt_coeff_container_log_size: 51.449μs,
    dup_constant: 1.544μs,
    dup_coeff_arg: 455.427ns,
    swap_constant: 1.076μs,
    swap_coeff_arg: 452.216ns,
    idx_map_constant: 3.950μs,
    idx_map_coeff_key_size: 2.710ns,
    idx_map_coeff_container_log_size: 771ps,
    idx_bmt_constant: 3.371μs,
    idx_bmt_coeff_key_size: 0.000s,
    idx_bmt_coeff_container_log_size: 4.721ns,
    idx_array: 3.499μs,
    idxp_map_constant: 4.790μs,
    idxp_map_coeff_key_size: 2.942ns,
    idxp_map_coeff_container_log_size: 0.000s,
    idxp_bmt_constant: 3.583μs,
    idxp_bmt_coeff_key_size: 0.000s,
    idxp_bmt_coeff_container_log_size: 5.227ns,
    idxp_array: 4.206μs,
    idxc_map_constant: 3.789μs,
    idxc_map_coeff_key_size: 2.676ns,
    idxc_map_coeff_container_log_size: 1.408ns,
    idxc_bmt_constant: 3.043μs,
    idxc_bmt_coeff_key_size: 0.000s,
    idxc_bmt_coeff_container_log_size: 6.069ns,
    idxc_array: 3.404μs,
    idxpc_map_constant: 4.634μs,
    idxpc_map_coeff_key_size: 2.953ns,
    idxpc_map_coeff_container_log_size: 1.779ns,
    idxpc_bmt_constant: 3.275μs,
    idxpc_bmt_coeff_key_size: 0.000s,
    idxpc_bmt_coeff_container_log_size: 6.234ns,
    idxpc_array: 4.172μs,
    ins_map_constant: 12.019μs,
    ins_map_coeff_key_size: 1.268ns,
    ins_map_coeff_container_log_size: 1.191μs,
    ins_bmt_constant: 2.961μs,
    ins_bmt_coeff_key_size: 0.000s,
    ins_bmt_coeff_container_log_size: 51.304μs,
    ins_array: 8.290μs,
    insc_map_constant: 12.219μs,
    insc_map_coeff_key_size: 2.177ns,
    insc_map_coeff_container_log_size: 1.113μs,
    insc_bmt_constant: 2.866μs,
    insc_bmt_coeff_key_size: 0.000s,
    insc_bmt_coeff_container_log_size: 51.323μs,
    insc_array: 8.278μs,
    ckpt: 69.432ns,
    signature_verify_constant: 59.433μs,
    signature_verify_coeff_size: 391ps,
    pedersen_valid: 178.190μs,
    verifier_key_load: 3.407ms,
    proof_verify_constant: 5.825ms,
    proof_verify_coeff_size: 3.309μs,
    hash_to_curve: 236.027μs,
    ec_add: 232.259ns,
    ec_mul: 85.080μs,
    transient_hash: 52.808μs,
    get_writes_constant: 19.262μs,
    get_writes_coeff_keys_added_size: 789.174ns,
    update_rcmap_constant: 1.055ms,
    update_rcmap_coeff_keys_added_size: 24.634μs,
    gc_rcmap_constant: 0.000s,
    gc_rcmap_coeff_keys_removed_size: 40.272μs,
    read_time_batched_4k: 2.000μs,
    read_time_synchronous_4k: 85.000μs,
}`;

    expect(costModel.toString()).toEqual(expected);
  });

  test('should max price adjustment be between 1.045 and 1.046', () => {
    const maxPriceAdjustment = LedgerParameters.initialParameters().maxPriceAdjustment();

    expect(maxPriceAdjustment).toBeGreaterThanOrEqual(1.045);
    expect(maxPriceAdjustment).toBeLessThanOrEqual(1.046);
  });

  test('should price adjustments adjust down on less than half-full blocks - UNSHIELDED NIGHT', () => {
    const defaultParams = LedgerParameters.initialParameters();
    const iterations = 2;
    const REWARDS_AMOUNT = 5_000_000n;

    const state = TestState.new();
    state.giveFeeToken(iterations, REWARDS_AMOUNT);

    const txs = Array.from(state.ledger.utxo.utxos).map((utxo) => {
      const intent = Intent.new(state.time);
      const utxoIh = utxo.intentHash;
      const inputs: UtxoSpend[] = [
        {
          value: REWARDS_AMOUNT,
          owner: state.nightKey.verifyingKey(),
          type: utxo.type,
          intentHash: utxoIh,
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

      intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);
      return Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    });

    if (txs.length === 0) throw new Error('no UTXOs to spend');
    const [first, ...rest] = txs;
    const tx = rest.reduce((acc, t) => acc.merge(t), first);

    console.log(tx.toString());
    const balancedTx = state.balanceTx(tx.eraseProofs());

    state.assertApply(balancedTx, new WellFormedStrictness());
    console.log(balancedTx.fees(defaultParams));
    console.log(balancedTx.feesWithMargin(defaultParams, 10000));
    console.log(balancedTx.cost(defaultParams));
    console.log(defaultParams.toString());
    console.log(defaultParams.maxPriceAdjustment().toString());
  });

  test('should price adjustments adjust down on less than half-full blocks - UNSHIELDED RANDOM (guaranteed)', () => {
    const defaultParams = LedgerParameters.initialParameters();
    const iterations = 6;
    const REWARDS_AMOUNT = 5_000_000n;
    const randomUnshieldedToken: UnshieldedTokenType = Random.unshieldedTokenType();

    const state = TestState.new();

    for (let i = 0; i < iterations; i++) {
      state.rewardsUnshielded(randomUnshieldedToken, REWARDS_AMOUNT);
    }
    state.giveFeeToken(iterations, REWARDS_AMOUNT);

    const txs = Array.from(state.ledger.utxo.utxos)
      .filter((utxo) => utxo.type !== DEFAULT_TOKEN_TYPE)
      .map((utxo) => {
        const intent = Intent.new(state.time);
        const utxoIh = utxo.intentHash;
        const inputs: UtxoSpend[] = [
          {
            value: REWARDS_AMOUNT,
            owner: state.nightKey.verifyingKey(),
            type: utxo.type,
            intentHash: utxoIh,
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

        intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);
        return Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
      });

    if (txs.length === 0) throw new Error('no UTXOs to spend');
    const [first, ...rest] = txs;
    const tx = rest.reduce((acc, t) => acc.merge(t), first);

    const balancedTx = state.balanceTx(tx.eraseProofs());

    state.assertApply(balancedTx, new WellFormedStrictness());
    console.log(balancedTx.fees(defaultParams));
    console.log(balancedTx.feesWithMargin(defaultParams, 10000));
    console.log(balancedTx.cost(defaultParams));
    console.log(defaultParams.toString());
    console.log(defaultParams.maxPriceAdjustment().toString());
  });

  test('should price adjustments adjust down on less than half-full blocks - UNSHIELDED RANDOM (fallible - multiple intents)', () => {
    const defaultParams = LedgerParameters.initialParameters();
    const iterations = 9;
    const REWARDS_AMOUNT = 5_000_000n;
    const randomUnshieldedToken: UnshieldedTokenType = Random.unshieldedTokenType();

    const state = TestState.new();

    for (let i = 0; i < iterations; i++) {
      state.rewardsUnshielded(randomUnshieldedToken, REWARDS_AMOUNT);
    }
    state.giveFeeToken(iterations, REWARDS_AMOUNT);

    const txs = Array.from(state.ledger.utxo.utxos)
      .filter((utxo) => utxo.type !== DEFAULT_TOKEN_TYPE)
      .map((utxo) => {
        const intent = Intent.new(state.time);
        const utxoIh = utxo.intentHash;
        const inputs: UtxoSpend[] = [
          {
            value: REWARDS_AMOUNT,
            owner: state.nightKey.verifyingKey(),
            type: utxo.type,
            intentHash: utxoIh,
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

        intent.fallibleUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);
        return Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
      });

    if (txs.length === 0) throw new Error('no UTXOs to spend');
    const [first, ...rest] = txs;
    const tx = rest.reduce((acc, t) => acc.merge(t), first);

    const balancedTx = state.balanceTx(tx.eraseProofs());
    // console.log(balancedTx.toString());
    // console.log(defaultParams.toString());
    console.log('cost:', balancedTx.cost(defaultParams));
    console.log('fees:', balancedTx.fees(defaultParams));
    state.assertApply(balancedTx, new WellFormedStrictness());

    console.log(balancedTx.feesWithMargin(defaultParams, 10000));
    console.log(balancedTx.cost(defaultParams));
    console.log(defaultParams.toString());
    console.log(defaultParams.maxPriceAdjustment().toString());
  });

  test('should price adjustments adjust down on less than half-full blocks - UNSHIELDED RANDOM (fallible - single long intent)', () => {
    const defaultParams = LedgerParameters.initialParameters();
    const iterations = 118;
    const REWARDS_AMOUNT = 5_000_000n;
    const randomUnshieldedToken: UnshieldedTokenType = Random.unshieldedTokenType();

    const state = TestState.new();

    for (let i = 0; i < iterations; i++) {
      state.rewardsUnshielded(randomUnshieldedToken, REWARDS_AMOUNT);
    }
    state.giveFeeToken(iterations, REWARDS_AMOUNT);

    const inputs: UtxoSpend[] = [];
    const outputs: UtxoOutput[] = [];

    Array.from(state.ledger.utxo.utxos)
      .filter((utxo) => utxo.type !== DEFAULT_TOKEN_TYPE)
      .forEach((utxo) => {
        const utxoIh = utxo.intentHash;
        const input: UtxoSpend = {
          value: REWARDS_AMOUNT,
          owner: state.nightKey.verifyingKey(),
          type: utxo.type,
          intentHash: utxoIh,
          outputNo: 0
        };
        const output: UtxoOutput = {
          owner: state.initialNightAddress,
          type: utxo.type,
          value: REWARDS_AMOUNT
        };

        inputs.push(input);
        outputs.push(output);
      });
    const intent = Intent.new(state.time);

    intent.fallibleUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);
    const tx = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

    const balancedTx = state.balanceTx(tx.eraseProofs());
    // console.log(balancedTx.toString());
    // console.log(defaultParams.toString());
    console.log('cost:', balancedTx.cost(defaultParams));
    console.log('fees:', balancedTx.fees(defaultParams));
    state.assertApply(balancedTx, new WellFormedStrictness());

    console.log(balancedTx.feesWithMargin(defaultParams, 10000));
    console.log(balancedTx.cost(defaultParams));
    console.log(defaultParams.toString());
    console.log(defaultParams.maxPriceAdjustment().toString());
  });

  test('should price adjustments adjust down on less than half-full blocks - NIGHT (intent with proofs)', () => {
    const HALF_FULL_BLOCK_LIMIT = 100000n;
    const LOWER_LIMIT = 30;
    const UPPER_LIMIT = 40;

    const results = [];
    let isPostHalfBlock = false;
    for (let i = LOWER_LIMIT; i < UPPER_LIMIT; i++) {
      results.push(getAppliedTransactionDetails(i));
    }

    for (let i = 1; i < results.length; i++) {
      const previous = results[i - 1];
      const current = results[i];
      if (current.cost.blockUsage > HALF_FULL_BLOCK_LIMIT && !isPostHalfBlock) {
        console.log('POST HALF-FULL BLOCK');
        console.log('--------------');
        isPostHalfBlock = true;
      }
      console.log(`iter: ${i}`);
      console.log('time:', current.time);
      console.log('spends:', current.spends);
      console.log('cost:', current.cost);
      console.log('fees:', current.fees);
      console.log(`diff in readTime`, current.cost.readTime - previous.cost.readTime);
      console.log(`diff in computeTime`, current.cost.computeTime - previous.cost.computeTime);
      console.log(`diff in blockUsage`, current.cost.blockUsage - previous.cost.blockUsage);
      console.log(`diff in bytesWritten`, current.cost.bytesWritten - previous.cost.bytesWritten);
      console.log(`diff in bytesChurned`, current.cost.bytesChurned - previous.cost.bytesChurned);
      console.log(`diff in fee`, current.fees - previous.fees);
      console.log('--------------');
    }

    console.log(results[results.length - 1].ledgerState.toString());
  });
});

function getAppliedTransactionDetails(iterations: number) {
  const REWARDS_AMOUNT = 5_000_000n;
  const DUST_UTXOS_WE_WANT_TO_KEEP = 10;
  const defaultParams = LedgerParameters.initialParameters();
  const nightToken: UnshieldedTokenType = Static.defaultUnshieldedTokenType();

  const state = TestState.new();

  for (let i = 0; i < iterations; i++) {
    state.rewardsUnshielded(nightToken, REWARDS_AMOUNT);
  }
  state.giveFeeToken(iterations, REWARDS_AMOUNT);
  const spends: DustSpend<PreProof>[] = [];
  while (state.dust.utxos.length > DUST_UTXOS_WE_WANT_TO_KEEP) {
    const qdo: QualifiedDustOutput = state.dust.utxos[0];
    const vFee = 0n;

    const [newDust, spend] = state.dust.spend(state.dustKey.secretKey, qdo, vFee, state.time);
    state.dust = newDust;
    spends.push(spend);
  }
  const intent = Intent.new(state.time);
  intent.dustActions = new DustActions<SignatureEnabled, PreProof>(
    SignatureMarker.signature,
    ProofMarker.preProof,
    state.time,
    spends,
    []
  );
  intent.signatureData(0xfeed);
  const tx = Transaction.fromPartsRandomized(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

  const balancedTx = state.balanceTx(tx.eraseProofs());
  state.assertApply(balancedTx, new WellFormedStrictness(), balancedTx.cost(defaultParams));
  const spendsNumber = Array.from(balancedTx.intents!.values()).reduce(
    (acc, currIntent) => acc + currIntent.dustActions!.spends.length,
    0
  );
  return {
    spends: spendsNumber,
    cost: balancedTx.cost(defaultParams),
    fees: balancedTx.fees(defaultParams),
    time: state.time.getTime(),
    ledgerState: state.ledger
  };
}
