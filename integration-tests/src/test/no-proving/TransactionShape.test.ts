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
  type AlignedValue,
  ChargedState,
  communicationCommitmentRandomness,
  type ContractCall,
  ContractCallPrototype,
  ContractOperation,
  Intent,
  LedgerParameters,
  nativeToken,
  partitionTranscripts,
  type PreProof,
  PrePartitionContractCall,
  PreTranscript,
  QueryContext,
  sampleIntentHash,
  sampleSigningKey,
  sampleUserAddress,
  SignatureEnabled,
  signatureVerifyingKey,
  signData,
  StateValue,
  Transaction,
  TransactionShape,
  UnshieldedOffer
} from '@midnightntwrk/ledger';
import { LOCAL_TEST_NETWORK_ID, Random } from '@/test-objects';

const EMPTY: AlignedValue = { value: [], alignment: [] };
const PARAMS = LedgerParameters.initialParameters();
const ttl = () => new Date(Date.now() + 3_600_000);

/** A call doing a single no-op, which fits in the guaranteed section alone. */
const noopCall = () =>
  new PreTranscript(new QueryContext(new ChargedState(StateValue.newArray()), Random.contractAddress()), [
    { noop: { n: 1 } }
  ]);

/** An unshielded offer of `spends` UTXOs of `rawType` and one output. */
const offer = (spends: number, rawType: string) => {
  const owner = signatureVerifyingKey(sampleSigningKey());
  return UnshieldedOffer.new(
    Array.from({ length: spends }, () => ({
      value: 100n,
      owner,
      type: rawType,
      intentHash: sampleIntentHash(),
      outputNo: 0
    })),
    [{ value: 100n, owner: sampleUserAddress(), type: rawType }],
    Array.from({ length: spends }, () => new SignatureEnabled(signData(sampleSigningKey(), new Uint8Array(32))))
  );
};

/** A transaction of one intent spending `spends` NIGHT UTXOs. */
const nightSpendsTx = (spends: number) => {
  const intent = Intent.new(ttl());
  intent.guaranteedUnshieldedOffer = offer(spends, nativeToken().raw);
  return Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
};

/** Whether `partitionTranscripts`' only call kept a guaranteed transcript. */
const keptGuaranteed = (partitioned: ReturnType<typeof partitionTranscripts>) => partitioned[0][0] !== undefined;

describe('Ledger API - TransactionShape', () => {
  /**
   * @given Calls with no effects
   * @when Estimating their shape
   * @then It is the calls' intent and a fee intent with one Dust spend
   * @and The deprecated forCalls gives the same estimate
   */
  test('estimateFromCalls counts the calls intent and a fee intent', () => {
    const calls = [noopCall()];
    const estimate = TransactionShape.estimateFromCalls(calls, PARAMS);

    expect(estimate.intents).toEqual(2);
    expect(estimate.dustSpends).toEqual(1);
    expect(estimate.nightSpends).toEqual(0);
    expect(estimate.fallibleNightSpends).toEqual(0);
    expect(TransactionShape.forCalls(calls, PARAMS).toString()).toEqual(estimate.toString());
  });

  /**
   * @given A shape
   * @when Assigning counts that are not non-negative integers
   * @then Each assignment throws and leaves the count as it was
   * @and Non-negative integers are assigned
   */
  test('counts must be non-negative integers', () => {
    const shape = TransactionShape.estimateFromCalls([], PARAMS);
    [-1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, 2 ** 32].forEach((invalid) => {
      expect(() => {
        shape.nightSpends = invalid;
      }).toThrow('must be a non-negative integer');
      expect(() => {
        shape.fallibleZswapInputs = invalid;
      }).toThrow('must be a non-negative integer');
    });
    expect(shape.nightSpends).toEqual(0);

    shape.nightSpends = 7;
    shape.fallibleZswapInputs = 0;
    shape.intents = 2 ** 32 - 1;
    expect(shape.nightSpends).toEqual(7);
    expect(shape.fallibleZswapInputs).toEqual(0);
    expect(shape.intents).toEqual(2 ** 32 - 1);
  });

  /**
   * @given A transaction with a guaranteed NIGHT offer and a fallible offer of another token
   * @when Counting its shape
   * @then Each offer's parts are counted in their own section and by token
   */
  test('of counts each section and token apart', () => {
    const intent = Intent.new(ttl());
    intent.guaranteedUnshieldedOffer = offer(2, nativeToken().raw);
    intent.fallibleUnshieldedOffer = offer(1, Random.unshieldedTokenType().raw);
    const shape = TransactionShape.of(Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent));

    expect(shape.intents).toEqual(1);
    expect(shape.nightSpends).toEqual(2);
    expect(shape.nightOutputs).toEqual(1);
    expect(shape.nonNightUnshieldedSpends).toEqual(0);
    expect(shape.fallibleNightSpends).toEqual(0);
    expect(shape.fallibleNonNightUnshieldedSpends).toEqual(1);
    expect(shape.fallibleNonNightUnshieldedOutputs).toEqual(1);
    expect(shape.dustSpends).toEqual(0);
  });

  /**
   * @given A shape counting one NIGHT spend
   * @when Checking a transaction with one spend, and one with eight
   * @then The first passes
   * @and The second throws, naming the count that differs, as the node's own check does
   */
  test('checkTx passes a transaction that fits and names the counts of one that does not', () => {
    const expected = TransactionShape.of(nightSpendsTx(1));
    expect(() => expected.checkTx(nightSpendsTx(1), PARAMS)).not.toThrow();

    const tooMany = nightSpendsTx(8);
    expect(() => expected.checkTx(tooMany, PARAMS)).toThrow('night_spends: expected 1, got 8');
    expect(() => tooMany.cost(PARAMS, true)).toThrow('exceeded the maximum time to dismiss');
  });

  /**
   * @given A call that fits as guaranteed for its estimated shape
   * @when Partitioning for a range up to many more NIGHT spends, in either order
   * @then The call is partitioned for the largest counts, as fallible
   * @and A range is exactly two shapes
   */
  test('a range of two shapes partitions for its largest counts', () => {
    const calls = [noopCall()];
    const estimate = TransactionShape.estimateFromCalls(calls, PARAMS);
    const many = TransactionShape.estimateFromCalls(calls, PARAMS);
    many.nightSpends = 16;

    expect(keptGuaranteed(partitionTranscripts(calls, PARAMS, estimate))).toBe(true);
    expect(keptGuaranteed(partitionTranscripts(calls, PARAMS, [estimate, many]))).toBe(false);
    expect(keptGuaranteed(partitionTranscripts(calls, PARAMS, [many, estimate]))).toBe(false);

    expect(() =>
      partitionTranscripts(calls, PARAMS, [estimate] as unknown as [TransactionShape, TransactionShape])
    ).toThrow('exactly two');
    expect(() =>
      partitionTranscripts(calls, PARAMS, [estimate, 3] as unknown as [TransactionShape, TransactionShape])
    ).toThrow('Expected TransactionShape');
  });

  /**
   * @given A transaction holding a guaranteed call that declares 5 ms of extra gas,
   * which fits alone
   * @when Partitioning a new call with and without the transaction, and adding it with addCalls
   * @then Without the transaction the new call stays guaranteed
   * @and With it, or through addCalls, the new call is fallible
   */
  test('partitioning reserves the other calls of the transaction', () => {
    const calls = [noopCall()];
    const estimate = TransactionShape.estimateFromCalls(calls, PARAMS);
    const [guaranteed] = partitionTranscripts([noopCall()], PARAMS)[0];
    const costly = {
      ...guaranteed!,
      gas: { ...guaranteed!.gas, computeTime: guaranteed!.gas.computeTime + 5_000_000_000n }
    };
    const costlyCall = new ContractCallPrototype(
      Random.contractAddress(),
      'op',
      new ContractOperation(),
      costly,
      undefined,
      [],
      EMPTY,
      EMPTY,
      communicationCommitmentRandomness(),
      'op'
    );
    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      Intent.new(ttl()).addCall(costlyCall)
    );
    expect(() => estimate.checkTx(tx, PARAMS)).not.toThrow();

    expect(keptGuaranteed(partitionTranscripts(calls, PARAMS, estimate))).toBe(true);
    expect(keptGuaranteed(partitionTranscripts(calls, PARAMS, estimate, tx))).toBe(false);
    expect(keptGuaranteed(partitionTranscripts(calls, PARAMS, undefined, tx))).toBe(false);
    expect(() =>
      partitionTranscripts(calls, PARAMS, estimate, {} as unknown as Transaction<SignatureEnabled, PreProof, never>)
    ).toThrow('Expected Transaction');

    const op = new ContractOperation();
    const added = tx.addCalls(
      { tag: 'first' },
      [
        new PrePartitionContractCall(
          Random.contractAddress(),
          'op',
          op,
          noopCall(),
          [],
          EMPTY,
          EMPTY,
          communicationCommitmentRandomness(),
          'op'
        )
      ],
      PARAMS,
      ttl()
    );
    const actions = added.intents!.get(1)!.actions as ContractCall<PreProof>[];
    expect(actions.map((call) => call.guaranteedTranscript !== undefined)).toEqual([true, false]);
  });

  /**
   * @given A call that fits as guaranteed alone
   * @when Partitioning for a shape no transaction within the byte limit could have
   * @then The call is fallible, and partitioning does not build that shape
   */
  test('a shape beyond the byte limit keeps every call fallible', () => {
    const calls = [noopCall()];
    const absurd = TransactionShape.estimateFromCalls(calls, PARAMS);
    absurd.nightOutputs = 2 ** 32 - 1;

    const start = Date.now();
    expect(keptGuaranteed(partitionTranscripts(calls, PARAMS, absurd))).toBe(false);
    expect(Date.now() - start).toBeLessThan(5_000);
  });
});
