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
  type Alignment,
  bigIntToValue,
  ChargedState,
  type CoinPublicKey,
  communicationCommitmentRandomness,
  type ContractAddress,
  ContractCallPrototype,
  ContractDeploy,
  ContractMaintenanceAuthority,
  ContractOperation,
  ContractState,
  createShieldedCoinInfo,
  decodeQualifiedShieldedCoinInfo,
  degradeToTransient,
  encodeCoinPublicKey,
  encodeContractAddress,
  encodeQualifiedShieldedCoinInfo,
  encodeShieldedCoinInfo,
  Intent,
  LedgerParameters,
  type LedgerState,
  type MaintenanceUpdate,
  type Nonce,
  type Op,
  partitionTranscripts,
  persistentCommit,
  type PreBinding,
  type PreProof,
  PreTranscript,
  type Proofish,
  type QualifiedShieldedCoinInfo,
  QueryContext,
  type RawTokenType,
  runtimeCoinCommitment,
  runtimeCoinNullifier,
  type ShieldedCoinInfo,
  type SignatureEnabled,
  StateBoundedMerkleTree,
  StateMap,
  StateValue,
  Transaction,
  transientHash,
  upgradeFromTransient,
  type Value,
  valueToBigInt,
  WellFormedStrictness,
  ZswapInput,
  ZswapOffer,
  ZswapOutput,
  ZswapTransient
} from '@midnight-ntwrk/ledger';
import { TestState } from '@/test/utils/TestState';
import {
  INITIAL_NIGHT_AMOUNT,
  LOCAL_TEST_NETWORK_ID,
  Random,
  type ShieldedTokenType,
  Static,
  TestResource
} from '@/test-objects';
import {
  cellRead,
  cellWrite,
  cellWriteCoin,
  counterIncrement,
  counterLessThan,
  counterRead,
  counterResetToDefault,
  getKey,
  historicMerkleTreeCheckRoot,
  historicMerkleTreeInsert,
  kernelClaimZswapCoinReceive,
  kernelClaimZswapCoinSpend,
  kernelClaimZswapNullfier,
  kernelSelf,
  merkleTreeCheckRoot,
  merkleTreeInsert,
  merkleTreeResetToDefault,
  setInsert,
  setMember,
  setResetToDefault
} from '@/test/utils/onchain-runtime-program-fragments';
import {
  ATOM_BYTES_1,
  ATOM_BYTES_16,
  ATOM_BYTES_32,
  ATOM_BYTES_8,
  ATOM_COMPRESS,
  ATOM_FIELD,
  EMPTY_VALUE,
  ONE_VALUE,
  THREE_VALUE,
  TWO_VALUE
} from '@/test/utils/value-alignment';

describe('Ledger API - MicroDao', () => {
  const ADVANCE = 'advance';
  const BUY_IN = 'buyIn';
  const CASH_OUT = 'cashOut';
  const SET_TOPIC = 'setTopic';
  const VOTE_COMMIT = 'voteCommit';
  const VOTE_REVEAL = 'voteReveal';

  test('should simulate DAO operations', () => {
    const orgSk = Random.generate32Bytes();
    const sep = [Static.encodeFromText('lares:udao:pk')];
    const orgPk = persistentCommit([ATOM_BYTES_32], sep, [orgSk]);

    const ops = setupOperations();

    const state = TestState.new();
    const REWARDS_AMOUNT = 5_000_000_000n;
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, REWARDS_AMOUNT);
    state.giveFeeToken(25, 5n * INITIAL_NIGHT_AMOUNT);

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;
    const balancedStrictness = new WellFormedStrictness();

    const partSks = [Random.generate32Bytes(), Random.generate32Bytes()];
    const partPks = [
      persistentCommit([ATOM_BYTES_32], sep, [partSks[0]]),
      persistentCommit([ATOM_BYTES_32], sep, [partSks[1]])
    ];
    const partNames = ['red', 'blue'];
    const partVotes = [true, false];

    const fundsBefore = getCurrentFunds(state.zswap.coins, token);

    // part 1
    const { addr, encodedAddr } = deployPhase({
      state,
      orgPk,
      ops,
      unbalancedStrictness,
      balancedStrictness
    });

    // part 2
    setTopicPhase({
      state,
      addr,
      orgSk,
      orgPk,
      setTopicOp: ops.setTopicOp,
      unbalancedStrictness,
      balancedStrictness
    });

    // part 3
    buyInPhase({
      state,
      addr,
      encodedAddr,
      token,
      partSks,
      partPks,
      partNames,
      buyInOp: ops.buyInOp,
      unbalancedStrictness,
      balancedStrictness
    });

    // part 4
    voteCommitmentPhase({
      state,
      addr,
      partSks,
      partPks,
      partVotes,
      partNames,
      voteCommitOp: ops.voteCommitOp,
      unbalancedStrictness,
      balancedStrictness
    });

    // part 5
    advanceToRevealPhase({
      state,
      addr,
      orgSk,
      orgPk,
      advanceOp: ops.advanceOp,
      unbalancedStrictness,
      balancedStrictness
    });

    // part 6
    voteRevealPhase({
      state,
      addr,
      partSks,
      partVotes,
      partNames,
      voteRevealOp: ops.voteRevealOp,
      unbalancedStrictness,
      balancedStrictness
    });

    // part 7
    advanceToFinalPhase({
      state,
      addr,
      orgSk,
      orgPk,
      advanceOp: ops.advanceOp,
      unbalancedStrictness,
      balancedStrictness
    });

    // part 8
    cashOutPhase({
      state,
      addr,
      encodedAddr,
      token,
      cashOutOp: ops.cashOutOp,
      beneficiary: state.zswapKeys.coinPublicKey,
      unbalancedStrictness,
      balancedStrictness
    });

    const fundsAfter = getCurrentFunds(state.zswap.coins, token);

    console.log(
      `We started with ${fundsBefore} tokens, and ended with ${fundsAfter}. ${fundsBefore - fundsAfter} lost to fees, and hopefully not the contract`
    );
    expect(fundsBefore - fundsAfter).toBe(0n);
  });

  function getCurrentFunds(coins: Set<QualifiedShieldedCoinInfo>, token: ShieldedTokenType) {
    return Array.from(coins)
      .filter((c) => c.type === token.raw)
      .map((c) => c.value)
      .reduce((acc, val) => acc + val, 0n);
  }

  function setupOperations() {
    const advanceOp = new ContractOperation();
    advanceOp.verifierKey = TestResource.operationVerifierKey();

    const buyInOp = new ContractOperation();
    buyInOp.verifierKey = TestResource.operationVerifierKey();

    const cashOutOp = new ContractOperation();
    cashOutOp.verifierKey = TestResource.operationVerifierKey();

    const setTopicOp = new ContractOperation();
    setTopicOp.verifierKey = TestResource.operationVerifierKey();

    const voteCommitOp = new ContractOperation();
    voteCommitOp.verifierKey = TestResource.operationVerifierKey();

    const voteRevealOp = new ContractOperation();
    voteRevealOp.verifierKey = TestResource.operationVerifierKey();

    return { advanceOp, buyInOp, cashOutOp, setTopicOp, voteCommitOp, voteRevealOp };
  }

  function deployPhase({
    state,
    orgPk,
    ops,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    orgPk: Value;
    ops: ReturnType<typeof setupOperations>;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }): { addr: ContractAddress; encodedAddr: Uint8Array } {
    console.log(':: Part 1: Deploy');

    const contract = new ContractState();
    contract.setOperation(ADVANCE, ops.advanceOp);
    contract.setOperation(BUY_IN, ops.buyInOp);
    contract.setOperation(CASH_OUT, ops.cashOutOp);
    contract.setOperation(SET_TOPIC, ops.setTopicOp);
    contract.setOperation(VOTE_COMMIT, ops.voteCommitOp);
    contract.setOperation(VOTE_REVEAL, ops.voteRevealOp);

    contract.data = getChargedState(orgPk);
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
    const balanced = state.balanceTx(tx.eraseProofs());
    const proved = balanced.eraseProofs();
    proved.wellFormed(state.ledger, balancedStrictness, state.time);
    state.assertApply(proved, balancedStrictness);

    return { addr, encodedAddr };
  }

  function setTopicPhase({
    state,
    addr,
    orgSk,
    orgPk,
    setTopicOp,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    addr: ContractAddress;
    orgSk: Uint8Array;
    orgPk: Value;
    setTopicOp: ContractOperation;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }) {
    console.log(':: Part 2: Setting topic');

    const context = new QueryContext(new ChargedState(state.ledger.index(addr)!.data.state), addr);
    const program = programWithResults(
      [
        ...cellRead(getKey(0), false),
        ...cellRead(getKey(1), false),
        ...cellWrite(getKey(2), false, {
          value: [ONE_VALUE, Static.encodeFromText('test topic')],
          alignment: [ATOM_BYTES_1, ATOM_COMPRESS]
        }),
        ...cellWrite(getKey(3), false, {
          value: [ONE_VALUE, encodeCoinPublicKey(state.zswapKeys.coinPublicKey)],
          alignment: [ATOM_BYTES_1, ATOM_BYTES_32]
        }),
        ...cellWrite(getKey(1), true, {
          value: [ONE_VALUE],
          alignment: [ATOM_BYTES_1]
        })
      ],
      [
        {
          value: [orgPk[0]],
          alignment: [ATOM_BYTES_32]
        },
        {
          value: [EMPTY_VALUE],
          alignment: [ATOM_BYTES_1]
        }
      ]
    );

    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const call = new ContractCallPrototype(
      addr,
      SET_TOPIC,
      setTopicOp,
      transcripts[0][0],
      transcripts[0][1],
      [{ value: [orgSk], alignment: [ATOM_BYTES_32] }],
      {
        value: [Static.encodeFromText('test topic'), encodeCoinPublicKey(state.zswapKeys.coinPublicKey)],
        alignment: [ATOM_COMPRESS, ATOM_BYTES_32]
      },
      {
        value: [],
        alignment: []
      },
      communicationCommitmentRandomness(),
      SET_TOPIC
    );

    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);

    const proved = tx.eraseProofs();
    const balanced = state.balanceTx(proved);
    state.assertApply(balanced, balancedStrictness);
  }

  function buyInPhase({
    state,
    addr,
    encodedAddr,
    token,
    partSks,
    partPks,
    partNames,
    buyInOp,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    addr: ContractAddress;
    encodedAddr: Uint8Array;
    token: ShieldedTokenType;
    partSks: Uint8Array[];
    partPks: Value[];
    partNames: string[];
    buyInOp: ContractOperation;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }) {
    console.log(':: Part 3: Buy-in');

    for (let i = 0; i < partSks.length; i++) {
      const sk = partSks[i];
      const pk = partPks[i];
      const name = partNames[i];

      console.log(`  :: Part ${name}`);

      const coin = createShieldedCoinInfo(token.raw, 100_000n);
      let out = ZswapOutput.newContractOwned(coin, 0, addr);

      const encodedCoin = encodeShieldedCoinInfo(coin);
      const encodedCoinValue = bigIntToValue(encodedCoin.value);

      let coinCom: AlignedValue = runtimeCoinCommitment(
        {
          value: [
            Static.trimTrailingZeros(encodedCoin.nonce),
            Static.trimTrailingZeros(encodedCoin.color),
            encodedCoinValue[0]
          ],
          alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
        },
        {
          value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
          alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
        }
      );
      const coinValue = bigIntToValue(encodedCoin.value);

      const potHasCoin = name !== 'red';
      const publicTranscript: Op<null>[] = [
        ...kernelSelf(),
        ...kernelClaimZswapCoinReceive(coinCom),
        ...cellRead(getKey(12), false)
      ];
      const publicTranscriptResults: AlignedValue[] = [
        {
          value: [encodedAddr],
          alignment: [ATOM_BYTES_32]
        },
        {
          value: [potHasCoin ? ONE_VALUE : EMPTY_VALUE],
          alignment: [ATOM_BYTES_1]
        }
      ];

      let offer;

      if (potHasCoin) {
        const cstate: ContractState = state.ledger.index(addr)!;
        const arr = cstate.data.state;
        expect(arr.type()).toBe('array');

        const potCell = arr.asArray()![11];
        expect(potCell.type()).toBe('cell');

        const { value } = potCell.asCell();
        const valueAsBigInt = valueToBigInt([value[2]]);
        const mtIndexAsBigInt = valueToBigInt([value[3]]);

        const pot: QualifiedShieldedCoinInfo = decodeQualifiedShieldedCoinInfo({
          nonce: Static.trimTrailingZeros(value[0]),
          color: value[1].length === 0 ? Static.encodeFromHex(token.raw) : value[1],
          value: valueAsBigInt,
          mt_index: mtIndexAsBigInt
        });

        const encodedPot = encodeQualifiedShieldedCoinInfo(pot);
        const potNull = runtimeCoinNullifier(
          {
            value: [
              Static.trimTrailingZeros(encodedPot.nonce),
              Static.trimTrailingZeros(encodedPot.color),
              bigIntToValue(encodedPot.value)[0]
            ],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
          },
          {
            value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
            alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
          }
        );
        const coinNull = runtimeCoinNullifier(
          {
            value: [
              Static.trimTrailingZeros(encodedCoin.nonce),
              Static.trimTrailingZeros(encodedCoin.color),
              bigIntToValue(encodedCoin.value)[0]
            ],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
          },
          {
            value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
            alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
          }
        );
        const potIn = ZswapInput.newContractOwned(pot, 0, addr, state.ledger.zswap);
        const transient = ZswapTransient.newFromContractOwnedOutput(
          {
            type: coin.type,
            nonce: coin.nonce,
            value: coin.value,
            mt_index: 0n
          },
          0,
          out
        );
        const newCoin: ShieldedCoinInfo = evolveFrom(
          Static.encodeFromText('midnight:kernel:nonce_evolve'),
          pot.value + coin.value,
          pot.type,
          pot.nonce
        );
        out = ZswapOutput.newContractOwned(newCoin, 0, addr);
        const encodedNewCoin = encodeShieldedCoinInfo(newCoin);
        const encodedNewCoinValue = bigIntToValue(encodedNewCoin.value);

        coinCom = runtimeCoinCommitment(
          {
            value: [
              Static.trimTrailingZeros(encodedNewCoin.nonce),
              Static.trimTrailingZeros(encodedNewCoin.color),
              encodedNewCoinValue[0]
            ],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
          },
          {
            value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
            alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
          }
        );

        const potEncoded = encodeQualifiedShieldedCoinInfo(pot);
        publicTranscriptResults.push(
          {
            value: [
              Static.trimTrailingZeros(potEncoded.nonce),
              Static.trimTrailingZeros(potEncoded.color),
              bigIntToValue(potEncoded.value)[0],
              bigIntToValue(potEncoded.mt_index)[0]
            ],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16, ATOM_BYTES_8]
          },
          {
            value: [encodedAddr],
            alignment: [ATOM_BYTES_32]
          },
          {
            value: [encodedAddr],
            alignment: [ATOM_BYTES_32]
          }
        );

        publicTranscript.push(
          ...cellRead(getKey(11), false),
          ...kernelSelf(),
          ...kernelClaimZswapNullfier(potNull),
          ...kernelClaimZswapNullfier(coinNull),
          ...kernelClaimZswapCoinSpend(coinCom),
          ...kernelClaimZswapCoinReceive(coinCom),
          ...kernelSelf(),
          ...cellWriteCoin(getKey(11), true, coinCom, {
            value: [
              Static.trimTrailingZeros(encodedNewCoin.nonce),
              Static.trimTrailingZeros(encodedNewCoin.color),
              encodedNewCoinValue[0]
            ],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
          })
        );

        offer = ZswapOffer.fromInput(potIn, pot.type, 0n);
        offer = offer.merge(ZswapOffer.fromOutput(out, pot.type, 100_000n));
        offer = offer.merge(ZswapOffer.fromTransient(transient));
      } else {
        publicTranscriptResults.push({
          value: [encodeContractAddress(addr)],
          alignment: [ATOM_BYTES_32]
        });
        publicTranscript.push(
          ...kernelSelf(),
          ...cellWriteCoin(getKey(11), true, coinCom, {
            value: [
              Static.trimTrailingZeros(encodedCoin.nonce),
              Static.trimTrailingZeros(encodedCoin.color),
              coinValue[0]
            ],
            alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
          }),
          ...cellWrite(getKey(12), true, {
            value: [ONE_VALUE],
            alignment: [ATOM_BYTES_1]
          })
        );

        offer = ZswapOffer.fromOutput(out, token.raw, 100_000n);
      }

      publicTranscript.push(
        ...historicMerkleTreeInsert(getKey(8), false, {
          value: pk,
          alignment: [ATOM_BYTES_32]
        })
      );

      const program = programWithResults(publicTranscript, publicTranscriptResults);
      const context = getContextWithOffer(state.ledger, addr, offer);
      const calls: PreTranscript[] = [new PreTranscript(context, program)];
      const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

      const call = new ContractCallPrototype(
        addr,
        BUY_IN,
        buyInOp,
        transcripts[0][0],
        transcripts[0][1],
        [{ value: [sk], alignment: [ATOM_BYTES_32] }],
        {
          value: [
            Static.trimTrailingZeros(encodedCoin.nonce),
            Static.trimTrailingZeros(encodedCoin.color),
            coinValue[0]
          ],
          alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
        },
        {
          value: [],
          alignment: []
        },
        communicationCommitmentRandomness(),
        BUY_IN
      );

      const tx = Transaction.fromParts(
        LOCAL_TEST_NETWORK_ID,
        offer,
        undefined,
        testIntents([call], [], [], state.time)
      );
      tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
      const balanced = state.balanceTx(tx.eraseProofs());
      state.assertApply(balanced, balancedStrictness);
    }
  }

  function voteCommitmentPhase({
    state,
    addr,
    partSks,
    partPks,
    partVotes,
    partNames,
    voteCommitOp,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    addr: ContractAddress;
    partSks: Uint8Array[];
    partPks: Value[];
    partVotes: boolean[];
    partNames: string[];
    voteCommitOp: ContractOperation;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }) {
    console.log(':: Part 4: Vote commitment');

    for (let i = 0; i < partSks.length; i++) {
      const sk = partSks[i];
      const pk = partPks[i];
      const vote = partVotes[i];
      const name = partNames[i];
      console.log(`  :: ${name}`);

      const contract = state.ledger.index(addr)!;
      expect(contract.data.state.type()).toBe('array');

      let arr = contract.data.state.asArray()!;
      const eligibleVoters = arr[8];

      expect(eligibleVoters.type()).toBe('array');
      arr = eligibleVoters.asArray()!;
      const mtreeVal = arr[0];

      expect(mtreeVal.type()).toBe('boundedMerkleTree');
      const tree = mtreeVal.asBoundedMerkleTree()!;
      const pathRoot = tree.root();
      const path = tree.findPathForLeaf({ value: pk, alignment: [ATOM_BYTES_32] });
      expect(path).toBeDefined();

      const nul = persistentCommit(
        [ATOM_BYTES_32],
        [Static.encodeFromText('\\0\\0\\0\\0\\0\\0\\0\\0udao:cn\\0')],
        [sk]
      );
      const cm = persistentCommit(
        [ATOM_BYTES_32],
        [
          vote
            ? Static.encodeFromText('\\0\\0\\0\\0\\0\\0\\0\\0yes\\0\\0\\0\\0\\0')
            : Static.encodeFromText('\\0\\0\\0\\0\\0\\0\\0\\0no\\0\\0\\0\\0\\0\\0')
        ],
        [sk]
      );

      const privateTranscriptOutputs: AlignedValue[] = [
        { value: [EMPTY_VALUE], alignment: [ATOM_FIELD] },
        { value: [sk], alignment: [ATOM_BYTES_32] },
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
        path!
      ];

      const program = programWithResults(
        [
          ...cellRead(getKey(1), false),
          ...counterRead(getKey(6), false),
          ...setMember(getKey(9), false, {
            value: nul,
            alignment: [ATOM_BYTES_32]
          }),
          ...historicMerkleTreeCheckRoot(getKey(8), false, pathRoot!),
          ...counterRead(getKey(6), false),
          ...merkleTreeInsert(getKey(7), false, cm),
          ...setInsert(getKey(9), false, {
            value: nul,
            alignment: [ATOM_BYTES_32]
          })
        ],
        [
          { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
          { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_8] },
          { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] },
          { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
          { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_8] }
        ]
      );

      const context = getContextWithOffer(state.ledger, addr);
      const calls: PreTranscript[] = [new PreTranscript(context, program)];
      const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

      const call = new ContractCallPrototype(
        addr,
        VOTE_COMMIT,
        voteCommitOp,
        transcripts[0][0],
        transcripts[0][1],
        privateTranscriptOutputs,
        {
          value: [vote ? ONE_VALUE : EMPTY_VALUE],
          alignment: [ATOM_BYTES_1]
        },
        {
          value: [],
          alignment: []
        },
        communicationCommitmentRandomness(),
        VOTE_COMMIT
      );

      const tx = Transaction.fromParts(
        LOCAL_TEST_NETWORK_ID,
        undefined,
        undefined,
        testIntents([call], [], [], state.time)
      );
      tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
      const balanced = state.balanceTx(tx.eraseProofs());
      state.assertApply(balanced, balancedStrictness);
    }
  }

  function advanceToRevealPhase({
    state,
    addr,
    orgSk,
    orgPk,
    advanceOp,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    addr: ContractAddress;
    orgSk: Uint8Array;
    orgPk: Value;
    advanceOp: ContractOperation;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }) {
    console.log(':: Part 5: Advance to reveal phase');

    const program = programWithResults(
      [
        ...cellRead(getKey(1), false),
        ...cellRead(getKey(0), false),
        ...cellRead(getKey(1), false),
        ...cellWrite(getKey(1), false, {
          value: [TWO_VALUE],
          alignment: [ATOM_BYTES_1]
        }),
        ...cellRead(getKey(1), false)
      ],
      [
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
        { value: orgPk, alignment: [ATOM_BYTES_32] },
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
        { value: [TWO_VALUE], alignment: [ATOM_BYTES_1] }
      ]
    );

    const context = getContextWithOffer(state.ledger, addr);
    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const call = new ContractCallPrototype(
      addr,
      ADVANCE,
      advanceOp,
      transcripts[0][0],
      transcripts[0][1],
      [{ value: [orgSk], alignment: [ATOM_BYTES_32] }],
      { value: [], alignment: [] },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      ADVANCE
    );

    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);
  }

  function voteRevealPhase({
    state,
    addr,
    partSks,
    partVotes,
    partNames,
    voteRevealOp,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    addr: ContractAddress;
    partSks: Uint8Array[];
    partVotes: boolean[];
    partNames: string[];
    voteRevealOp: ContractOperation;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }) {
    console.log(':: Part 6: Vote revealing');

    for (let i = 0; i < partSks.length; i++) {
      const sk = partSks[i];
      const vote = partVotes[i];
      const name = partNames[i];

      console.log(`  :: Part ${name}`);

      const cm = persistentCommit(
        [ATOM_BYTES_32],
        [
          vote
            ? Static.encodeFromText('\\0\\0\\0\\0\\0\\0\\0\\0yes\\0\\0\\0\\0\\0')
            : Static.encodeFromText('\\0\\0\\0\\0\\0\\0\\0\\0no\\0\\0\\0\\0\\0\\0')
        ],
        [sk]
      );

      const contract = state.ledger.index(addr)!;
      expect(contract.data.state.type()).toBe('array');

      let arr = contract.data.state.asArray()!;
      const commitedVotes = arr[7];

      expect(commitedVotes.type(), 'array');
      arr = commitedVotes.asArray()!;
      const mtreeValue = arr[0];

      expect(mtreeValue.type(), 'boundedMerkleTree');
      const tree = mtreeValue.asBoundedMerkleTree()!;
      const pathRoot = tree.root();
      const path = tree.findPathForLeaf({ value: cm, alignment: [ATOM_BYTES_32] });
      expect(path).toBeDefined();

      const nul = persistentCommit(
        [ATOM_BYTES_32],
        [Static.encodeFromText('\\0\\0\\0\\0\\0\\0\\0\\0udao:rn\\0')],
        [sk]
      );
      const privateTranscriptOutputs: AlignedValue[] = [
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_8] },
        { value: [sk], alignment: [ATOM_BYTES_32] },
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
        { value: [vote ? ONE_VALUE : EMPTY_VALUE], alignment: [ATOM_BYTES_1] },
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
        path!
      ];

      const program = programWithResults(
        [
          ...cellRead(getKey(1), false),
          ...counterRead(getKey(6), false),
          ...setMember(getKey(10), false, { value: nul, alignment: [ATOM_BYTES_32] }),
          ...counterRead(getKey(6), false),
          ...merkleTreeCheckRoot(getKey(7), false, pathRoot!),
          ...counterIncrement(vote ? getKey(4) : getKey(5), false, 1),
          ...setInsert(getKey(10), false, { value: nul, alignment: [ATOM_BYTES_32] })
        ],
        [
          { value: [TWO_VALUE], alignment: [ATOM_BYTES_1] },
          { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_8] },
          { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] },
          { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_8] },
          { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }
        ]
      );

      const context = getContextWithOffer(state.ledger, addr);
      const calls: PreTranscript[] = [new PreTranscript(context, program)];
      const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

      const call = new ContractCallPrototype(
        addr,
        VOTE_REVEAL,
        voteRevealOp,
        transcripts[0][0],
        transcripts[0][1],
        privateTranscriptOutputs,
        { value: [], alignment: [] },
        { value: [], alignment: [] },
        communicationCommitmentRandomness(),
        VOTE_REVEAL
      );

      const tx = Transaction.fromParts(
        LOCAL_TEST_NETWORK_ID,
        undefined,
        undefined,
        testIntents([call], [], [], state.time)
      );
      tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
      const balanced = state.balanceTx(tx.eraseProofs());
      state.assertApply(balanced, balancedStrictness);
    }
  }

  function advanceToFinalPhase({
    state,
    addr,
    orgSk,
    orgPk,
    advanceOp,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    addr: ContractAddress;
    orgSk: Uint8Array;
    orgPk: Value;
    advanceOp: ContractOperation;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }) {
    console.log(':: Part 7: Advance to final phase');

    const program = programWithResults(
      [
        ...cellRead(getKey(1), false),
        ...cellRead(getKey(0), false),
        ...cellRead(getKey(1), false),
        ...cellWrite(getKey(1), false, { value: [THREE_VALUE], alignment: [ATOM_BYTES_1] }),
        ...cellRead(getKey(1), false),
        ...counterRead(getKey(5), false),
        ...counterLessThan(getKey(4), false, { value: [ONE_VALUE], alignment: [ATOM_BYTES_8] })
      ],
      [
        { value: [TWO_VALUE], alignment: [ATOM_BYTES_1] },
        { value: orgPk, alignment: [ATOM_BYTES_32] },
        { value: [TWO_VALUE], alignment: [ATOM_BYTES_1] },
        { value: [THREE_VALUE], alignment: [ATOM_BYTES_1] },
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_8] },
        { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }
      ]
    );

    const context = getContextWithOffer(state.ledger, addr);
    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const call = new ContractCallPrototype(
      addr,
      ADVANCE,
      advanceOp,
      transcripts[0][0],
      transcripts[0][1],
      [{ value: [orgSk], alignment: [ATOM_BYTES_32] }],
      { value: [], alignment: [] },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      ADVANCE
    );

    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);
  }

  function cashOutPhase({
    state,
    addr,
    encodedAddr,
    token,
    cashOutOp,
    beneficiary,
    unbalancedStrictness,
    balancedStrictness
  }: {
    state: TestState;
    addr: ContractAddress;
    encodedAddr: Uint8Array;
    token: ShieldedTokenType;
    cashOutOp: ContractOperation;
    beneficiary: CoinPublicKey;
    unbalancedStrictness: WellFormedStrictness;
    balancedStrictness: WellFormedStrictness;
  }) {
    console.log(':: Part 8: Cash Out');

    const contract = state.ledger.index(addr)!;
    expect(contract.data.state.type(), 'array');

    const arr = contract.data.state.asArray()!;
    const potVal = arr[11];
    expect(potVal.type(), 'cell');

    const { value } = potVal.asCell();
    const valueAsBigInt = valueToBigInt([value[2]]);
    const mtIndexAsBigInt = valueToBigInt([value[3]]);
    const nonceWithoutZeroAtTheEnd = value[0];
    const nonce = new Uint8Array(nonceWithoutZeroAtTheEnd.length + 1);
    nonce.set(nonceWithoutZeroAtTheEnd, 0);
    nonce[nonceWithoutZeroAtTheEnd.length] = 0;

    const pot: QualifiedShieldedCoinInfo = decodeQualifiedShieldedCoinInfo({
      nonce,
      color: value[1].length === 0 ? Static.encodeFromHex(token.raw) : value[1],
      value: valueAsBigInt,
      mt_index: mtIndexAsBigInt
    });

    const newCoin = evolveFrom(Static.encodeFromText('midnight:kernel:nonce_evolve'), pot.value, pot.type, pot.nonce);
    const encodedNewCoin = encodeShieldedCoinInfo(newCoin);
    const encodedPot = encodeQualifiedShieldedCoinInfo(pot);
    const encodedNewCoinValue = bigIntToValue(encodedNewCoin.value);

    const potNull = runtimeCoinNullifier(
      {
        value: [
          Static.trimTrailingZeros(encodedPot.nonce),
          Static.trimTrailingZeros(encodedPot.color),
          bigIntToValue(encodedPot.value)[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    const coinCom = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(encodedNewCoin.nonce),
          Static.trimTrailingZeros(encodedNewCoin.color),
          encodedNewCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [ONE_VALUE, encodeCoinPublicKey(state.zswapKeys.coinPublicKey), EMPTY_VALUE],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    const program = programWithResults(
      [
        ...cellRead(getKey(1), false),
        ...cellRead(getKey(3), false),
        ...cellRead(getKey(3), false),
        ...counterRead(getKey(4), false),
        ...counterLessThan(getKey(5), false, { value: [TWO_VALUE], alignment: [ATOM_BYTES_8] }),
        ...cellRead(getKey(11), false),
        ...cellRead(getKey(11), false),
        ...kernelSelf(),
        ...kernelClaimZswapNullfier(potNull),
        ...kernelClaimZswapCoinSpend(coinCom),
        ...cellWrite(getKey(1), false, { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }),
        ...cellWrite(getKey(2), false, { value: [EMPTY_VALUE, EMPTY_VALUE], alignment: [ATOM_BYTES_1, ATOM_COMPRESS] }),
        ...counterResetToDefault(getKey(4), false),
        ...counterResetToDefault(getKey(5), false),
        ...cellWrite(getKey(3), false, { value: [EMPTY_VALUE, EMPTY_VALUE], alignment: [ATOM_BYTES_1, ATOM_BYTES_32] }),
        ...merkleTreeResetToDefault(getKey(7), false, 10),
        ...setResetToDefault(getKey(9), false),
        ...setResetToDefault(getKey(10), false),
        ...cellWrite(getKey(11), false, {
          value: [EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE],
          alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16, ATOM_BYTES_8]
        }),
        ...cellWrite(getKey(12), false, { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }),
        ...counterIncrement(getKey(6), false, 1)
      ],
      [
        { value: [THREE_VALUE], alignment: [ATOM_BYTES_1] },
        {
          value: [ONE_VALUE, Static.encodeFromHex(beneficiary)],
          alignment: [ATOM_BYTES_1, ATOM_BYTES_32]
        },
        {
          value: [ONE_VALUE, Static.encodeFromHex(beneficiary)],
          alignment: [ATOM_BYTES_1, ATOM_BYTES_32]
        },
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_8] },
        { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] },
        {
          value: [
            Static.trimTrailingZeros(encodedPot.nonce),
            Static.trimTrailingZeros(encodedPot.color),
            bigIntToValue(encodedPot.value)[0],
            bigIntToValue(encodedPot.mt_index)[0]
          ],
          alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16, ATOM_BYTES_8]
        },
        {
          value: [
            Static.trimTrailingZeros(encodedPot.nonce),
            Static.trimTrailingZeros(encodedPot.color),
            bigIntToValue(encodedPot.value)[0],
            bigIntToValue(encodedPot.mt_index)[0]
          ],
          alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16, ATOM_BYTES_8]
        },
        { value: [encodedAddr], alignment: [ATOM_BYTES_32] }
      ]
    );

    const context = getContextWithOffer(state.ledger, addr);
    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const call = new ContractCallPrototype(
      addr,
      CASH_OUT,
      cashOutOp,
      transcripts[0][0],
      transcripts[0][1],
      [{ value: [encodeCoinPublicKey(state.zswapKeys.coinPublicKey)], alignment: [ATOM_BYTES_32] }],
      { value: [], alignment: [] },
      {
        value: [
          Static.trimTrailingZeros(encodedNewCoin.nonce),
          Static.trimTrailingZeros(encodedNewCoin.color),
          bigIntToValue(encodedNewCoin.value)[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      communicationCommitmentRandomness(),
      CASH_OUT
    );

    let offer = ZswapOffer.fromInput(
      ZswapInput.newContractOwned(pot, 0, addr, state.ledger.zswap),
      pot.type,
      pot.value
    );
    offer = offer.merge(
      ZswapOffer.fromOutput(
        ZswapOutput.new(newCoin, 0, state.zswapKeys.coinPublicKey, state.zswapKeys.encryptionPublicKey),
        newCoin.type,
        newCoin.value
      )
    );
    const s = state;
    s.zswap = s.zswap.watchFor(s.zswapKeys.coinPublicKey, newCoin);

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer, undefined, testIntents([call], [], [], s.time));
    tx.wellFormed(s.ledger, unbalancedStrictness, s.time);
    const balanced = s.balanceTx(tx.eraseProofs());
    s.assertApply(balanced, balancedStrictness);
  }

  function evolveFrom(domainSep: Uint8Array, value: bigint, type: RawTokenType, nonce: Nonce): ShieldedCoinInfo {
    const degrade = degradeToTransient([Static.encodeFromHex(nonce)])[0];
    const thAlignment: Alignment = [ATOM_FIELD, ATOM_FIELD];
    const thValue: Value = transientHash(thAlignment, [domainSep, degrade]);
    const evolvedNonce = upgradeFromTransient(thValue)[0];
    const updatedEvolvedNonce = new Uint8Array(evolvedNonce.length + 1);
    updatedEvolvedNonce.set(evolvedNonce, 0);
    updatedEvolvedNonce[updatedEvolvedNonce.length] = 0;
    const evolvedNonceAsNonce: Nonce = Buffer.from(updatedEvolvedNonce).toString('hex');
    return {
      nonce: evolvedNonceAsNonce,
      type,
      value
    };
  }

  function getContextWithOffer(ledger: LedgerState, addr: ContractAddress, offer?: ZswapOffer<Proofish>) {
    const res = new QueryContext(new ChargedState(ledger.index(addr)!.data.state), addr);
    if (offer) {
      const [, indices] = ledger.zswap.tryApply(offer);
      const { block } = res;
      block.comIndices = new Map(Array.from(indices, ([k, v]) => [k, Number(v)]));
      res.block = block;
    }
    return res;
  }

  function translateOp(op: Op<null>, nextResult: () => AlignedValue): Op<AlignedValue> {
    if (typeof op === 'string') {
      return op;
    }
    if ('popeq' in op) {
      return { popeq: { cached: op.popeq.cached, result: nextResult() } };
    }
    return op;
  }

  function programWithResults(prog: Op<null>[], results: AlignedValue[]): Op<AlignedValue>[] {
    let i = 0;
    const next = () => {
      if (i >= results.length) throw new Error('programWithResults: not enough results to fill popeq ops');
      return results[i++];
    };
    return prog
      .map((op) => translateOp(op as Op<null>, next))
      .filter((op) => {
        if (typeof op === 'string') {
          return op;
        }
        if ('idx' in op) {
          return op.idx.path.length !== 0;
        }
        if ('ins' in op) {
          return op.ins.n !== 0;
        }
        return true;
      });
  }

  function testIntents(
    calls: ContractCallPrototype[],
    updates: MaintenanceUpdate[],
    deploys: ContractDeploy[],
    tblock: Date
  ): Intent<SignatureEnabled, PreProof, PreBinding> {
    const fastForward = new Date(0);
    fastForward.setSeconds(3600);
    const updatedTtl = new Date(tblock.getTime() + fastForward.getTime());
    let intent = Intent.new(updatedTtl);
    calls.forEach((call) => {
      intent = intent.addCall(call);
    });
    updates.forEach((update) => {
      intent = intent.addMaintenanceUpdate(update);
    });
    deploys.forEach((deploy) => {
      intent = intent.addDeploy(deploy);
    });
    return intent;
  }

  function getChargedState(orgPk: Value): ChargedState {
    let stateValue = StateValue.newArray();
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: orgPk,
        alignment: [ATOM_BYTES_32]
      })
    );
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_1]
      })
    );
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE, EMPTY_VALUE],
        alignment: [ATOM_BYTES_1, ATOM_COMPRESS]
      })
    );
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE, EMPTY_VALUE],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32]
      })
    );
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_8]
      })
    );
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_8]
      })
    );
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_8]
      })
    );
    const pair = StateValue.newArray()
      .arrayPush(StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(10)))
      .arrayPush(
        StateValue.newCell({
          value: [EMPTY_VALUE],
          alignment: [ATOM_BYTES_8]
        })
      );
    stateValue = stateValue.arrayPush(pair);

    let m = new StateMap();
    m = m.insert(
      {
        value: [ONE_VALUE, EMPTY_VALUE],
        alignment: [ATOM_BYTES_1, ATOM_FIELD]
      },
      StateValue.newNull()
    );
    const triple = StateValue.newArray()
      .arrayPush(StateValue.newBoundedMerkleTree(new StateBoundedMerkleTree(10)))
      .arrayPush(
        StateValue.newCell({
          value: [EMPTY_VALUE],
          alignment: [ATOM_BYTES_8]
        })
      )
      .arrayPush(StateValue.newMap(m));
    stateValue = stateValue.arrayPush(triple);
    stateValue = stateValue.arrayPush(StateValue.newMap(new StateMap()));
    stateValue = stateValue.arrayPush(StateValue.newMap(new StateMap()));

    const qualifiedCoinInfoDefault: AlignedValue = {
      value: [EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE],
      alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16, ATOM_BYTES_8]
    };
    stateValue = stateValue.arrayPush(StateValue.newCell(qualifiedCoinInfoDefault));
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_1]
      })
    );
    return new ChargedState(stateValue);
  }
});
