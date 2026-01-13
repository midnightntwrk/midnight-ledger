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

/**
 * Token Vault Contract - Shielded Token Integration Tests
 *
 * **REFERENCE IMPLEMENTATION ONLY**
 * This code is provided for educational and testing purposes to demonstrate
 * Midnight ledger features. DO NOT use this code as-is in production.
 *
 * These tests validate the token-vault.compact contract's shielded token operations
 * using the TypeScript WASM API. They cover:
 *
 * 1. Contract deployment with proper state initialization
 * 2. depositShielded - first deposit (empty vault → new coin)
 * 3. depositShielded with merge (existing coin + new coin → merged coin)
 * 4. withdrawShielded - partial withdrawal (split vault into user coin + change)
 *
 * ## Shielded Token Concepts
 *
 * - **Commitments**: Hide the value and owner of coins
 * - **Nullifiers**: Prevent double-spending without revealing which coin was spent
 * - **ZSwap Offers**: Bundle inputs, outputs, transients, and deltas for atomic ops
 */

import {
  type AlignedValue,
  type Alignment,
  bigIntToValue,
  ChargedState,
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
  LedgerParameters,
  type LedgerState,
  type Nonce,
  type Op,
  partitionTranscripts,
  persistentCommit,
  PreTranscript,
  type Proofish,
  type QualifiedShieldedCoinInfo,
  QueryContext,
  type RawTokenType,
  runtimeCoinCommitment,
  runtimeCoinNullifier,
  type ShieldedCoinInfo,
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
import { testIntents } from '@/test-utils';
import {
  cellRead,
  cellWrite,
  cellWriteCoin,
  counterIncrement,
  getKey,
  kernelClaimZswapCoinReceive,
  kernelClaimZswapCoinSpend,
  kernelClaimZswapNullfier,
  kernelSelf,
  programWithResults,
  setMember
} from '@/test/utils/onchain-runtime-program-fragments';
import {
  ATOM_BYTES_1,
  ATOM_BYTES_16,
  ATOM_BYTES_32,
  ATOM_BYTES_8,
  ATOM_FIELD,
  EMPTY_VALUE,
  ONE_VALUE
} from '@/test/utils/value-alignment';

describe('Ledger API - TokenVault Shielded', () => {
  // ============================================================================
  // Operation Names (matching token-vault.compact)
  // ============================================================================
  const DEPOSIT_SHIELDED = 'depositShielded';
  const WITHDRAW_SHIELDED = 'withdrawShielded';
  const GET_SHIELDED_BALANCE = 'getShieldedBalance';

  // ============================================================================
  // Contract State Layout (matching token-vault.compact)
  // ============================================================================
  // Index 0: shieldedVault (QualifiedShieldedCoinInfo)
  // Index 1: hasShieldedTokens (Boolean)
  // Index 2: owner (Bytes<32>)
  // Index 3: authorized (Set<Bytes<32>>)
  // Index 4: totalShieldedDeposits (Counter)
  // Index 5: totalShieldedWithdrawals (Counter)
  // Index 6: totalUnshieldedDeposits (Counter)
  // Index 7: totalUnshieldedWithdrawals (Counter)
  const STATE_IDX_SHIELDED_VAULT = 0;
  const STATE_IDX_HAS_SHIELDED_TOKENS = 1;
  const STATE_IDX_OWNER = 2;
  const STATE_IDX_AUTHORIZED = 3;
  const STATE_IDX_TOTAL_SHIELDED_DEPOSITS = 4;
  const STATE_IDX_TOTAL_SHIELDED_WITHDRAWALS = 5;

  // ============================================================================
  // Helper Functions
  // ============================================================================

  /**
   * Set up contract operations with dummy verifier keys.
   */
  function setupOperations() {
    const depositShieldedOp = new ContractOperation();
    depositShieldedOp.verifierKey = TestResource.operationVerifierKey();

    const withdrawShieldedOp = new ContractOperation();
    withdrawShieldedOp.verifierKey = TestResource.operationVerifierKey();

    const getBalanceOp = new ContractOperation();
    getBalanceOp.verifierKey = TestResource.operationVerifierKey();

    return {
      depositShieldedOp,
      withdrawShieldedOp,
      getBalanceOp
    };
  }

  /**
   * Create the initial contract state with owner set.
   */
  function createInitialContractState(ownerPk: Value): StateValue {
    let stateValue = StateValue.newArray();
    // Index 0: shieldedVault - empty initially
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE, EMPTY_VALUE],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_8, ATOM_BYTES_8]
      })
    );
    // Index 1: hasShieldedTokens - false
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_1]
      })
    );
    // Index 2: owner - the owner's public key
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: ownerPk,
        alignment: [ATOM_BYTES_32]
      })
    );
    // Index 3: authorized - empty set (Map)
    stateValue = stateValue.arrayPush(StateValue.newMap(new StateMap()));
    // Index 4: totalShieldedDeposits - 0
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_8]
      })
    );
    // Index 5: totalShieldedWithdrawals - 0
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_8]
      })
    );
    // Index 6: totalUnshieldedDeposits - 0
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_8]
      })
    );
    // Index 7: totalUnshieldedWithdrawals - 0
    stateValue = stateValue.arrayPush(
      StateValue.newCell({
        value: [EMPTY_VALUE],
        alignment: [ATOM_BYTES_8]
      })
    );

    return stateValue;
  }

  /**
   * Deploy the token vault contract.
   */
  function deployContract(
    state: TestState,
    ownerPk: Value,
    ops: ReturnType<typeof setupOperations>
  ): ContractAddress {
    const contract = new ContractState();

    contract.setOperation(DEPOSIT_SHIELDED, ops.depositShieldedOp);
    contract.setOperation(WITHDRAW_SHIELDED, ops.withdrawShieldedOp);
    contract.setOperation(GET_SHIELDED_BALANCE, ops.getBalanceOp);

    contract.data = new ChargedState(createInitialContractState(ownerPk));
    contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);

    const deploy = new ContractDeploy(contract);

    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([], [], [deploy], state.time)
    );

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    strictness.verifyContractProofs = false;

    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, strictness);

    return deploy.address;
  }

  /**
   * Get query context with ZSwap offer applied.
   */
  function getContextWithOffer(
    ledger: LedgerState,
    addr: ContractAddress,
    offer?: ZswapOffer<Proofish>
  ): QueryContext {
    const res = new QueryContext(new ChargedState(ledger.index(addr)!.data.state), addr);
    if (offer) {
      const [, indices] = ledger.zswap.tryApply(offer);
      const { block } = res;
      block.comIndices = new Map(Array.from(indices, ([k, v]) => [k, Number(v)]));
      res.block = block;
    }
    return res;
  }

  /**
   * Evolve a coin's nonce using domain separation.
   */
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

  // ============================================================================
  // Tests
  // ============================================================================

  /**
   * Test contract deployment.
   */
  test('should deploy token vault contract', () => {
    const state = TestState.new();
    state.giveFeeToken(5, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);

    const ops = setupOperations();
    const addr = deployContract(state, ownerPk, ops);

    const contract = state.ledger.index(addr);
    expect(contract).toBeDefined();
    expect(contract!.data.state.type()).toBe('array');

    const stateArray = contract!.data.state.asArray()!;
    const ownerCell = stateArray[STATE_IDX_OWNER].asCell();
    expect(ownerCell.value[0]).toEqual(ownerPk[0]);

    const hasTokensCell = stateArray[STATE_IDX_HAS_SHIELDED_TOKENS].asCell();
    expect(hasTokensCell.value[0]).toEqual(EMPTY_VALUE);
  });

  /**
   * Test first shielded deposit into empty vault.
   *
   * @given A deployed token vault with no shielded tokens
   * @when Depositing shielded tokens
   * @then Should store the coin in shieldedVault and set hasShieldedTokens=true
   */
  test('should deposit shielded tokens into empty vault', () => {
    console.log(':: Shielded Token Vault Test - First Deposit');

    const state = TestState.new();
    const REWARDS_AMOUNT = 5_000_000_000n;
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, REWARDS_AMOUNT);
    state.giveFeeToken(10, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);

    const ops = setupOperations();
    const addr = deployContract(state, ownerPk, ops);
    const encodedAddr = encodeContractAddress(addr);

    console.log('   Contract deployed');

    // Create shielded coin to deposit
    const DEPOSIT_AMOUNT = 1_000_000n;
    const coin = createShieldedCoinInfo(token.raw, DEPOSIT_AMOUNT);
    const out = ZswapOutput.newContractOwned(coin, undefined, addr);
    const encodedCoin = encodeShieldedCoinInfo(coin);
    const encodedCoinValue = bigIntToValue(encodedCoin.value);

    // Compute coin commitment
    const coinCom: AlignedValue = runtimeCoinCommitment(
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

    // Build public transcript - order: kernel_self, claim_receive, cell_read, kernel_self, cell_write_coin, cell_write, counter_increment
    const publicTranscript: Op<null>[] = [
      ...kernelSelf(),
      ...kernelClaimZswapCoinReceive(coinCom),
      ...cellRead(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), false),
      ...kernelSelf(),
      ...cellWriteCoin(getKey(STATE_IDX_SHIELDED_VAULT), true, coinCom, {
        value: [
          Static.trimTrailingZeros(encodedCoin.nonce),
          Static.trimTrailingZeros(encodedCoin.color),
          encodedCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      }),
      ...cellWrite(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), true, {
        value: [ONE_VALUE],
        alignment: [ATOM_BYTES_1]
      }),
      ...counterIncrement(getKey(STATE_IDX_TOTAL_SHIELDED_DEPOSITS), false, 1)
    ];

    // Results in order: kernel_self, Cell_read(hasShieldedTokens), kernel_self
    const publicTranscriptResults: AlignedValue[] = [
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] },
      { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }, // hasShieldedTokens was false
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] }
    ];

    // Create ZSwap offer with negative delta (user spends tokens)
    const offer = ZswapOffer.fromOutput(out, token.raw, DEPOSIT_AMOUNT);

    const program = programWithResults(publicTranscript, publicTranscriptResults);
    const context = getContextWithOffer(state.ledger, addr, offer);
    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const call = new ContractCallPrototype(
      addr,
      DEPOSIT_SHIELDED,
      ops.depositShieldedOp,
      transcripts[0][0],
      transcripts[0][1],
      [],
      {
        value: [
          Static.trimTrailingZeros(encodedCoin.nonce),
          Static.trimTrailingZeros(encodedCoin.color),
          encodedCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_SHIELDED
    );

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;
    const balancedStrictness = new WellFormedStrictness();

    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      offer,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    console.log(`   Deposited ${DEPOSIT_AMOUNT} tokens`);

    // Verify hasShieldedTokens is now true
    const contract = state.ledger.index(addr)!;
    const stateArray = contract.data.state.asArray()!;
    const hasTokensCell = stateArray[STATE_IDX_HAS_SHIELDED_TOKENS].asCell();
    expect(hasTokensCell.value[0]).toEqual(ONE_VALUE);

    // Verify vault has the coin
    const vaultCell = stateArray[STATE_IDX_SHIELDED_VAULT].asCell();
    expect(vaultCell.value[0]).toBeDefined();
    expect(vaultCell.value[0].length).toBeGreaterThan(0);

    console.log('   First deposit complete - vault has tokens');
  });

  /**
   * Test shielded deposit with merge operation.
   *
   * @given A deployed token vault with existing shielded tokens
   * @when Depositing additional shielded tokens
   * @then Should merge with existing coin and update vault
   */
  test('should merge shielded deposit with existing vault', () => {
    console.log(':: Shielded Token Vault Test - Merge Deposit');

    const state = TestState.new();
    const REWARDS_AMOUNT = 5_000_000_000n;
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, REWARDS_AMOUNT);
    state.giveFeeToken(15, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);

    const ops = setupOperations();
    const addr = deployContract(state, ownerPk, ops);
    const encodedAddr = encodeContractAddress(addr);

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;
    const balancedStrictness = new WellFormedStrictness();

    // ========== First Deposit ==========
    const FIRST_DEPOSIT = 1_000_000n;
    const firstCoin = createShieldedCoinInfo(token.raw, FIRST_DEPOSIT);
    let out = ZswapOutput.newContractOwned(firstCoin, undefined, addr);
    const encodedFirstCoin = encodeShieldedCoinInfo(firstCoin);
    const encodedFirstCoinValue = bigIntToValue(encodedFirstCoin.value);

    let coinCom: AlignedValue = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(encodedFirstCoin.nonce),
          Static.trimTrailingZeros(encodedFirstCoin.color),
          encodedFirstCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    let publicTranscript: Op<null>[] = [
      ...kernelSelf(),
      ...kernelClaimZswapCoinReceive(coinCom),
      ...cellRead(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), false),
      ...kernelSelf(),
      ...cellWriteCoin(getKey(STATE_IDX_SHIELDED_VAULT), true, coinCom, {
        value: [
          Static.trimTrailingZeros(encodedFirstCoin.nonce),
          Static.trimTrailingZeros(encodedFirstCoin.color),
          encodedFirstCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      }),
      ...cellWrite(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), true, {
        value: [ONE_VALUE],
        alignment: [ATOM_BYTES_1]
      }),
      ...counterIncrement(getKey(STATE_IDX_TOTAL_SHIELDED_DEPOSITS), false, 1)
    ];

    let publicTranscriptResults: AlignedValue[] = [
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] },
      { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] },
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] }
    ];

    let offer = ZswapOffer.fromOutput(out, token.raw, FIRST_DEPOSIT);
    let program = programWithResults(publicTranscript, publicTranscriptResults);
    let context = getContextWithOffer(state.ledger, addr, offer);
    let calls: PreTranscript[] = [new PreTranscript(context, program)];
    let transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    let call = new ContractCallPrototype(
      addr,
      DEPOSIT_SHIELDED,
      ops.depositShieldedOp,
      transcripts[0][0],
      transcripts[0][1],
      [],
      {
        value: [
          Static.trimTrailingZeros(encodedFirstCoin.nonce),
          Static.trimTrailingZeros(encodedFirstCoin.color),
          encodedFirstCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_SHIELDED
    );

    let tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      offer,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    let balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    console.log(`   First deposit: ${FIRST_DEPOSIT} tokens`);

    // ========== Second Deposit with Merge ==========
    const SECOND_DEPOSIT = 500_000n;

    // Read existing pot from contract state
    const cstate = state.ledger.index(addr)!;
    const arr = cstate.data.state.asArray()!;
    const potCell = arr[STATE_IDX_SHIELDED_VAULT].asCell();
    const { value } = potCell;
    const valueAsBigInt = valueToBigInt([value[2]]);
    const mtIndexAsBigInt = valueToBigInt([value[3]]);

    const pot: QualifiedShieldedCoinInfo = decodeQualifiedShieldedCoinInfo({
      nonce: Static.trimTrailingZeros(value[0]),
      color: value[1].length === 0 ? Static.encodeFromHex(token.raw) : value[1],
      value: valueAsBigInt,
      mt_index: mtIndexAsBigInt
    });

    // Create new deposit coin
    const newCoin = createShieldedCoinInfo(token.raw, SECOND_DEPOSIT);
    out = ZswapOutput.newContractOwned(newCoin, undefined, addr);
    const encodedNewCoin = encodeShieldedCoinInfo(newCoin);
    const encodedNewCoinValue = bigIntToValue(encodedNewCoin.value);

    const newCoinCom = runtimeCoinCommitment(
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

    // Compute nullifiers
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

    // Create merged coin with combined value
    const mergedCoin: ShieldedCoinInfo = evolveFrom(
      Static.encodeFromText('midnight:kernel:nonce_evolve'),
      pot.value + newCoin.value,
      pot.type,
      pot.nonce
    );
    const mergedOut = ZswapOutput.newContractOwned(mergedCoin, undefined, addr);
    const encodedMergedCoin = encodeShieldedCoinInfo(mergedCoin);
    const encodedMergedCoinValue = bigIntToValue(encodedMergedCoin.value);

    const mergedCoinCom = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(encodedMergedCoin.nonce),
          Static.trimTrailingZeros(encodedMergedCoin.color),
          encodedMergedCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    // Create ZSwap inputs/outputs
    const potIn = ZswapInput.newContractOwned(pot, undefined, addr, state.ledger.zswap);
    const transient = ZswapTransient.newFromContractOwnedOutput(
      { type: newCoin.type, nonce: newCoin.nonce, value: newCoin.value, mt_index: 0n },
      0,
      out
    );

    // Build merge transcript
    // Order: kernel_self, claim_receive(newCoin), cell_read(hasShielded), cell_read(pot),
    //        kernel_self, claim_nullifier(pot), claim_nullifier(coin), claim_spend(merged), claim_receive(merged),
    //        kernel_self, cell_write_coin(merged), counter_increment
    publicTranscript = [
      ...kernelSelf(),
      ...kernelClaimZswapCoinReceive(newCoinCom),
      ...cellRead(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), false),
      ...cellRead(getKey(STATE_IDX_SHIELDED_VAULT), false),
      ...kernelSelf(),
      ...kernelClaimZswapNullfier(potNull),
      ...kernelClaimZswapNullfier(coinNull),
      ...kernelClaimZswapCoinSpend(mergedCoinCom),
      ...kernelClaimZswapCoinReceive(mergedCoinCom),
      ...kernelSelf(),
      ...cellWriteCoin(getKey(STATE_IDX_SHIELDED_VAULT), true, mergedCoinCom, {
        value: [
          Static.trimTrailingZeros(encodedMergedCoin.nonce),
          Static.trimTrailingZeros(encodedMergedCoin.color),
          encodedMergedCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      }),
      ...counterIncrement(getKey(STATE_IDX_TOTAL_SHIELDED_DEPOSITS), false, 1)
    ];

    // Results: kernel_self, hasShielded=true, pot, kernel_self (for nullifiers), kernel_self (for write)
    publicTranscriptResults = [
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] },
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
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] },
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] }
    ];

    // Build offer with pot input, merged output, and transient
    offer = ZswapOffer.fromInput(potIn, pot.type, 0n);
    offer = offer.merge(ZswapOffer.fromOutput(mergedOut, pot.type, SECOND_DEPOSIT));
    offer = offer.merge(ZswapOffer.fromTransient(transient));

    program = programWithResults(publicTranscript, publicTranscriptResults);
    context = getContextWithOffer(state.ledger, addr, offer);
    calls = [new PreTranscript(context, program)];
    transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    call = new ContractCallPrototype(
      addr,
      DEPOSIT_SHIELDED,
      ops.depositShieldedOp,
      transcripts[0][0],
      transcripts[0][1],
      [],
      {
        value: [
          Static.trimTrailingZeros(encodedNewCoin.nonce),
          Static.trimTrailingZeros(encodedNewCoin.color),
          encodedNewCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_SHIELDED
    );

    tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      offer,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    console.log(`   Second deposit (merge): ${SECOND_DEPOSIT} tokens`);
    console.log(`   Total in vault: ${FIRST_DEPOSIT + SECOND_DEPOSIT} tokens`);

    // Verify vault has merged coin with combined value
    const finalContract = state.ledger.index(addr)!;
    const finalStateArray = finalContract.data.state.asArray()!;
    const finalVaultCell = finalStateArray[STATE_IDX_SHIELDED_VAULT].asCell();
    const finalValue = valueToBigInt([finalVaultCell.value[2]]);
    expect(finalValue).toBe(FIRST_DEPOSIT + SECOND_DEPOSIT);

    console.log('   Merge deposit complete');
  });

  /**
   * Test partial shielded withdrawal.
   *
   * @given A deployed token vault with shielded tokens
   * @when Withdrawing a partial amount
   * @then Should split the vault coin: user receives withdrawn amount, change stays in vault
   */
  test('should withdraw shielded tokens partially', () => {
    console.log(':: Shielded Token Vault Test - Partial Withdrawal');

    const state = TestState.new();
    const REWARDS_AMOUNT = 5_000_000_000n;
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, REWARDS_AMOUNT);
    state.giveFeeToken(20, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);

    const ops = setupOperations();
    const addr = deployContract(state, ownerPk, ops);
    const encodedAddr = encodeContractAddress(addr);

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;
    const balancedStrictness = new WellFormedStrictness();

    // ========== First Deposit (to have tokens in vault) ==========
    const DEPOSIT_AMOUNT = 1_000_000n;
    const depositCoin = createShieldedCoinInfo(token.raw, DEPOSIT_AMOUNT);
    const depositOut = ZswapOutput.newContractOwned(depositCoin, undefined, addr);
    const encodedDepositCoin = encodeShieldedCoinInfo(depositCoin);
    const encodedDepositCoinValue = bigIntToValue(encodedDepositCoin.value);

    const depositCoinCom: AlignedValue = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(encodedDepositCoin.nonce),
          Static.trimTrailingZeros(encodedDepositCoin.color),
          encodedDepositCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    let publicTranscript: Op<null>[] = [
      ...kernelSelf(),
      ...kernelClaimZswapCoinReceive(depositCoinCom),
      ...cellRead(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), false),
      ...kernelSelf(),
      ...cellWriteCoin(getKey(STATE_IDX_SHIELDED_VAULT), true, depositCoinCom, {
        value: [
          Static.trimTrailingZeros(encodedDepositCoin.nonce),
          Static.trimTrailingZeros(encodedDepositCoin.color),
          encodedDepositCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      }),
      ...cellWrite(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), true, {
        value: [ONE_VALUE],
        alignment: [ATOM_BYTES_1]
      }),
      ...counterIncrement(getKey(STATE_IDX_TOTAL_SHIELDED_DEPOSITS), false, 1)
    ];

    let publicTranscriptResults: AlignedValue[] = [
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] },
      { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] },
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] }
    ];

    let offer = ZswapOffer.fromOutput(depositOut, token.raw, DEPOSIT_AMOUNT);
    let program = programWithResults(publicTranscript, publicTranscriptResults);
    let context = getContextWithOffer(state.ledger, addr, offer);
    let calls: PreTranscript[] = [new PreTranscript(context, program)];
    let transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    let call = new ContractCallPrototype(
      addr,
      DEPOSIT_SHIELDED,
      ops.depositShieldedOp,
      transcripts[0][0],
      transcripts[0][1],
      [],
      {
        value: [
          Static.trimTrailingZeros(encodedDepositCoin.nonce),
          Static.trimTrailingZeros(encodedDepositCoin.color),
          encodedDepositCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_SHIELDED
    );

    let tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      offer,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    let balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    console.log(`   Deposited ${DEPOSIT_AMOUNT} tokens into vault`);

    // ========== Partial Withdrawal ==========
    const WITHDRAW_AMOUNT = 300_000n;
    const CHANGE_AMOUNT = DEPOSIT_AMOUNT - WITHDRAW_AMOUNT;

    // Read existing pot from contract state
    const cstate = state.ledger.index(addr)!;
    const arr = cstate.data.state.asArray()!;
    const potCell = arr[STATE_IDX_SHIELDED_VAULT].asCell();
    const { value } = potCell;
    const valueAsBigInt = valueToBigInt([value[2]]);
    // mt_index is stored in value[3] after the coin commitment is added to the zswap tree
    const mtIndexAsBigInt = value[3] ? valueToBigInt([value[3]]) : 0n;

    const pot: QualifiedShieldedCoinInfo = decodeQualifiedShieldedCoinInfo({
      nonce: Static.trimTrailingZeros(value[0]),
      color: value[1].length === 0 ? Static.encodeFromHex(token.raw) : value[1],
      value: valueAsBigInt,
      mt_index: mtIndexAsBigInt
    });

    // Create withdrawal coin (goes to user) and change coin (stays in contract)
    const withdrawCoin: ShieldedCoinInfo = evolveFrom(
      Static.encodeFromText('midnight:kernel:nonce_evolve'),
      WITHDRAW_AMOUNT,
      pot.type,
      pot.nonce
    );
    const changeCoin: ShieldedCoinInfo = evolveFrom(
      Static.encodeFromText('midnight:kernel:nonce_evolve'),
      CHANGE_AMOUNT,
      pot.type,
      pot.nonce
    );

    const encodedWithdrawCoin = encodeShieldedCoinInfo(withdrawCoin);
    const encodedChangeCoin = encodeShieldedCoinInfo(changeCoin);
    const encodedPot = encodeQualifiedShieldedCoinInfo(pot);
    const encodedWithdrawCoinValue = bigIntToValue(encodedWithdrawCoin.value);
    const encodedChangeCoinValue = bigIntToValue(encodedChangeCoin.value);

    // Compute nullifier for pot
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

    // Compute coin commitments
    // Withdrawal coin goes to user (left<ZswapCoinPublicKey, ContractAddress>)
    const withdrawCom = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(encodedWithdrawCoin.nonce),
          Static.trimTrailingZeros(encodedWithdrawCoin.color),
          encodedWithdrawCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [ONE_VALUE, encodeCoinPublicKey(state.zswapKeys.coinPublicKey), EMPTY_VALUE],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    // Change coin goes to contract (right<ZswapCoinPublicKey, ContractAddress>)
    const changeCom = runtimeCoinCommitment(
      {
        value: [
          Static.trimTrailingZeros(encodedChangeCoin.nonce),
          Static.trimTrailingZeros(encodedChangeCoin.color),
          encodedChangeCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      {
        value: [EMPTY_VALUE, EMPTY_VALUE, encodedAddr],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    // Build withdrawal transcript
    // Order: isAuthorized() -> Set_member(authorized), Cell_read(owner),
    //        Cell_read(hasShieldedTokens), Cell_read(vault),
    //        kernel_self, claim_nullifier(pot), claim_spend(withdraw), claim_spend(change), claim_receive(change),
    //        kernel_self, cell_write_coin(change), counter_increment
    publicTranscript = [
      ...setMember(getKey(STATE_IDX_AUTHORIZED), false, { value: ownerPk, alignment: [ATOM_BYTES_32] }),
      ...cellRead(getKey(STATE_IDX_OWNER), false),
      ...cellRead(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), false),
      ...cellRead(getKey(STATE_IDX_SHIELDED_VAULT), false),
      ...kernelSelf(),
      ...kernelClaimZswapNullfier(potNull),
      ...kernelClaimZswapCoinSpend(withdrawCom),
      ...kernelClaimZswapCoinSpend(changeCom),
      ...kernelClaimZswapCoinReceive(changeCom),
      ...kernelSelf(),
      ...cellWriteCoin(getKey(STATE_IDX_SHIELDED_VAULT), true, changeCom, {
        value: [
          Static.trimTrailingZeros(encodedChangeCoin.nonce),
          Static.trimTrailingZeros(encodedChangeCoin.color),
          encodedChangeCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      }),
      ...counterIncrement(getKey(STATE_IDX_TOTAL_SHIELDED_WITHDRAWALS), false, 1)
    ];

    // Results: Set_member(false), owner(ownerPk), hasShielded(true), pot, kernel_self, kernel_self
    publicTranscriptResults = [
      { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }, // authorized.member(pk) = false
      { value: ownerPk, alignment: [ATOM_BYTES_32] }, // owner = ownerPk (so pk == owner is true)
      { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }, // hasShieldedTokens = true
      {
        value: [
          Static.trimTrailingZeros(encodedPot.nonce),
          Static.trimTrailingZeros(encodedPot.color),
          bigIntToValue(encodedPot.value)[0],
          bigIntToValue(encodedPot.mt_index)[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16, ATOM_BYTES_8]
      },
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] }, // kernel_self for nullifier/spend
      { value: [encodedAddr], alignment: [ATOM_BYTES_32] } // kernel_self for write
    ];

    // Build ZSwap offer: pot input → withdraw output (to user) + change output (to contract)
    const potIn = ZswapInput.newContractOwned(pot, undefined, addr, state.ledger.zswap);
    const withdrawOut = ZswapOutput.new(
      withdrawCoin,
      undefined,
      state.zswapKeys.coinPublicKey,
      state.zswapKeys.encryptionPublicKey
    );
    const changeOut = ZswapOutput.newContractOwned(changeCoin, undefined, addr);

    // Track the withdrawn coin so user can receive it
    state.zswap = state.zswap.watchFor(state.zswapKeys.coinPublicKey, withdrawCoin);

    offer = ZswapOffer.fromInput(potIn, pot.type, 0n);
    offer = offer.merge(ZswapOffer.fromOutput(withdrawOut, withdrawCoin.type, WITHDRAW_AMOUNT));
    offer = offer.merge(ZswapOffer.fromOutput(changeOut, changeCoin.type, CHANGE_AMOUNT));

    program = programWithResults(publicTranscript, publicTranscriptResults);
    context = getContextWithOffer(state.ledger, addr, offer);
    calls = [new PreTranscript(context, program)];
    transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    call = new ContractCallPrototype(
      addr,
      WITHDRAW_SHIELDED,
      ops.withdrawShieldedOp,
      transcripts[0][0],
      transcripts[0][1],
      [{ value: [ownerSk], alignment: [ATOM_BYTES_32] }], // Private: owner secret key for auth
      {
        value: [bigIntToValue(WITHDRAW_AMOUNT)[0]],
        alignment: [ATOM_BYTES_16]
      },
      {
        value: [
          Static.trimTrailingZeros(encodedWithdrawCoin.nonce),
          Static.trimTrailingZeros(encodedWithdrawCoin.color),
          encodedWithdrawCoinValue[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16]
      },
      communicationCommitmentRandomness(),
      WITHDRAW_SHIELDED
    );

    tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      offer,
      undefined,
      testIntents([call], [], [], state.time)
    );
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    console.log(`   Withdrew ${WITHDRAW_AMOUNT} tokens`);
    console.log(`   Change remaining in vault: ${CHANGE_AMOUNT} tokens`);

    // Verify vault has change coin
    const finalContract = state.ledger.index(addr)!;
    const finalStateArray = finalContract.data.state.asArray()!;
    const finalVaultCell = finalStateArray[STATE_IDX_SHIELDED_VAULT].asCell();
    const finalValue = valueToBigInt([finalVaultCell.value[2]]);
    expect(finalValue).toBe(CHANGE_AMOUNT);

    // Verify hasShieldedTokens is still true (since there's change)
    const finalHasTokens = finalStateArray[STATE_IDX_HAS_SHIELDED_TOKENS].asCell();
    expect(finalHasTokens.value[0]).toEqual(ONE_VALUE);

    console.log('   Partial withdrawal complete');
  });
});
