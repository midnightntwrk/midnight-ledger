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
  communicationCommitmentRandomness,
  type ContractAddress,
  ContractDeploy,
  ContractMaintenanceAuthority,
  ContractOperation,
  ContractState,
  createShieldedCoinInfo,
  encodeContractAddress,
  encodeShieldedCoinInfo,
  PrePartitionContractCall,
  PreTranscript,
  QueryContext,
  runtimeCoinCommitment,
  type SegmentSpecifier,
  type ShieldedCoinInfo,
  StateValue,
  Transaction,
  WellFormedStrictness,
  ZswapOutput,
  ZswapTransient
} from '@midnightntwrk/ledger';
import {
  getQualifiedShieldedCoinInfo,
  INITIAL_NIGHT_AMOUNT,
  LOCAL_TEST_NETWORK_ID,
  Random,
  type ShieldedTokenType,
  Static,
  TestResource
} from '@/test-objects';
import { plus1Hour, testIntents } from '@/test-utils';
import { TestState } from '@/test/utils/TestState';
import { ATOM_BYTES_1, ATOM_BYTES_16, ATOM_BYTES_32, EMPTY_VALUE, ONE_VALUE } from '@/test/utils/value-alignment';
import {
  cellRead,
  cellWrite,
  getKey,
  kernelClaimZswapCoinReceive,
  kernelSelf,
  programWithResults
} from '@/test/utils/onchain-runtime-program-fragments';

/**
 * `Transaction.addCalls` sorts the Zswap inputs, outputs and transients it is handed into
 * either the guaranteed offer or the fallible offer for the target segment, depending on
 * whether a call's fallible transcript claims them, and merges the result into whatever the
 * transaction already carried.
 *
 * These tests cover the sorting and merging itself. They deliberately do not assert on which
 * side a coin lands when a call *does* claim it fallibly: whether a transcript gets a fallible
 * section at all is decided by a gas budget inside `partitionTranscripts`, and
 * `LedgerParameters` cannot be narrowed from the API, so such a test would be pinned to the
 * current cost model rather than to the partitioning behaviour.
 */
describe('Ledger API - Transaction.addCalls Zswap partitioning', () => {
  const STORE = 'store';
  const SEGMENT: SegmentSpecifier = { tag: 'specific', value: 2 };

  /**
   * Test that Zswap coins accumulate rather than replace each other over successive addCalls.
   *
   * @given A transaction that already carries a Zswap output from an earlier addCalls
   * @when Calling addCalls again with a second, distinct Zswap output
   * @then The guaranteed offer should contain both outputs, not just the most recent one
   */
  test('addCalls - merges Zswap coins into an offer the transaction already carries', () => {
    const { state, addr, encodedAddr, op } = setup();
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();
    const ttl = plus1Hour(state.time);

    const first = buildCallAndOutput({ state, addr, encodedAddr, op, token, baseKey: 0, value: 100_000n });
    const second = buildCallAndOutput({ state, addr, encodedAddr, op, token, baseKey: 3, value: 200_000n });

    const afterFirst = Transaction.fromParts(LOCAL_TEST_NETWORK_ID).addCalls(
      SEGMENT,
      [first.preCall],
      state.ledger.parameters,
      ttl,
      [],
      [first.zswapOutput],
      []
    );

    expect(afterFirst.guaranteedOffer, 'first addCalls should have produced a guaranteed offer').toBeDefined();
    expect(afterFirst.guaranteedOffer!.outputs).toHaveLength(1);

    const afterSecond = afterFirst.addCalls(
      SEGMENT,
      [second.preCall],
      state.ledger.parameters,
      ttl,
      [],
      [second.zswapOutput],
      []
    );

    // The merge branch: the second call must fold into the existing offer rather than
    // overwrite it or be dropped.
    expect(afterSecond.guaranteedOffer!.outputs).toHaveLength(2);

    const commitments = afterSecond.guaranteedOffer!.outputs.map((o) => o.commitment);
    expect(new Set(commitments).size, 'the two outputs should be distinct coins').toBe(2);
    expect(commitments).toContain(first.zswapOutput.commitment);
    expect(commitments).toContain(second.zswapOutput.commitment);
  });

  /**
   * Test that the three Zswap coin kinds are sorted independently of one another.
   *
   * @given An input, an output and a transient handed to a single addCalls
   * @when None of them is claimed by a fallible transcript
   * @then Each should appear exactly once in the guaranteed offer, under its own kind,
   *       with no fallible offer produced
   */
  test('addCalls - keeps inputs, outputs and transients distinct and loses none', () => {
    const { state, addr, encodedAddr, op } = setup();
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();
    const ttl = plus1Hour(state.time);

    const { preCall } = buildCallAndOutput({ state, addr, encodedAddr, op, token, baseKey: 0, value: 100_000n });

    // A user-owned coin to spend, giving us an input.
    const coinToSpend = Array.from(state.zswap.coins).find((c) => c.type === token.raw);
    expect(coinToSpend, 'expected a shielded coin to spend').toBeDefined();
    const [nextZswap, zswapInput] = state.zswap.spend(state.zswapKeys, coinToSpend!, 2);
    state.zswap = nextZswap;

    // A contract-owned output, kept out of the offer itself and used only to build a transient.
    const transientCoin: ShieldedCoinInfo = createShieldedCoinInfo(token.raw, 50_000n);
    const transientSource = ZswapOutput.newContractOwned(transientCoin, undefined, addr);
    const zswapTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(transientCoin),
      0,
      transientSource
    );

    // A separate contract-owned output that goes into the offer on its own.
    const outputCoin: ShieldedCoinInfo = createShieldedCoinInfo(token.raw, 70_000n);
    const zswapOutput = ZswapOutput.newContractOwned(outputCoin, undefined, addr);

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID).addCalls(
      SEGMENT,
      [preCall],
      state.ledger.parameters,
      ttl,
      [zswapInput],
      [zswapOutput],
      [zswapTransient]
    );

    expect(tx.guaranteedOffer, 'expected a guaranteed offer').toBeDefined();

    // Nothing was claimed fallibly, so every coin belongs to the guaranteed side, and each
    // must land under its own kind: a coin sorted against another kind's flags would be
    // dropped or misplaced here.
    expect(tx.guaranteedOffer!.inputs).toHaveLength(1);
    expect(tx.guaranteedOffer!.outputs).toHaveLength(1);
    expect(tx.guaranteedOffer!.transients).toHaveLength(1);
    expect(tx.fallibleOffer?.get(2)).toBeUndefined();

    expect(tx.guaranteedOffer!.inputs[0].nullifier).toBe(zswapInput.nullifier);
    expect(tx.guaranteedOffer!.outputs[0].commitment).toBe(zswapOutput.commitment);
    expect(tx.guaranteedOffer!.transients[0].commitment).toBe(zswapTransient.commitment);
  });

  function setup() {
    const state = TestState.new();
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, 5_000_000_000n);
    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);

    const op = new ContractOperation();
    op.verifierKey = TestResource.operationVerifierKey();

    const { addr, encodedAddr } = deployContract({ state, op });

    return { state, addr, encodedAddr, op };
  }

  function deployContract({ state, op }: { state: TestState; op: ContractOperation }): {
    addr: ContractAddress;
    encodedAddr: Uint8Array;
  } {
    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;

    const contract = new ContractState();
    contract.setOperation(STORE, op);

    // One slot per cell the call transcripts write, in the layout `buildCallAndOutput`
    // uses: commitment, coin payload, flag — once for `baseKey` 0 and once for 3.
    const slots = [0, 3].flatMap(() => [
      StateValue.newCell({ value: [EMPTY_VALUE], alignment: [ATOM_BYTES_32] }),
      StateValue.newCell({
        value: [EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      }),
      StateValue.newCell({ value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] })
    ]);
    contract.data = new ChargedState(slots.reduce((arr, cell) => arr.arrayPush(cell), StateValue.newArray()));

    contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);

    const deploy = new ContractDeploy(contract);
    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([], [], [deploy], state.time)
    );

    const addr: ContractAddress = tx.intents!.get(1)!.actions[0].address;
    const encodedAddr = encodeContractAddress(addr);

    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    state.assertApply(state.balanceTx(tx.eraseProofs()), new WellFormedStrictness());

    return { addr, encodedAddr };
  }

  function buildCallAndOutput(opts: {
    state: TestState;
    addr: ContractAddress;
    encodedAddr: Uint8Array;
    op: ContractOperation;
    token: ShieldedTokenType;
    baseKey: number;
    value: bigint;
  }) {
    const { state, addr, encodedAddr, op, token, baseKey, value } = opts;

    const coin: ShieldedCoinInfo = createShieldedCoinInfo(token.raw, value);
    const encodedCoin = encodeShieldedCoinInfo(coin);
    const value16 = bigIntToValue(encodedCoin.value)[0];

    const coinPayload: AlignedValue = {
      value: [Static.trimTrailingZeros(encodedCoin.nonce), Static.trimTrailingZeros(encodedCoin.color), value16],
      alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
    };

    const coinCom: AlignedValue = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(coinPayload.value[0]),
          Static.trimTrailingZeros(coinPayload.value[1]),
          Static.trimTrailingZeros(coinPayload.value[2])
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, Static.trimTrailingZeros(encodedAddr)],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    const transcriptOps = [
      ...kernelSelf(),
      ...kernelClaimZswapCoinReceive(coinCom),
      ...cellWrite(getKey(baseKey), true, coinCom),
      ...cellWrite(getKey(baseKey + 1), true, coinPayload),
      ...cellWrite(getKey(baseKey + 2), true, { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }),
      ...cellRead(getKey(baseKey + 2), false)
    ];

    const program = programWithResults(transcriptOps, [
      { value: [Static.trimTrailingZeros(encodedAddr)], alignment: [ATOM_BYTES_32] },
      { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }
    ]);

    const context = new QueryContext(new ChargedState(state.ledger.index(addr)!.data.state), addr);
    const preTranscript = new PreTranscript(context, program);

    const preCall = new PrePartitionContractCall(
      addr,
      STORE,
      op,
      preTranscript,
      [{ value: [Static.trimTrailingZeros(Random.generate32Bytes())], alignment: [ATOM_BYTES_32] }],
      coinPayload,
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      STORE
    );

    const zswapOutput = ZswapOutput.newContractOwned(coin, undefined, addr);

    return { preCall, zswapOutput, coinCom, coinPayload };
  }
});
