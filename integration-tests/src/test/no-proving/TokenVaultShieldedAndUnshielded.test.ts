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
 * Token Vault Contract - Shielded & Unshielded Token Integration Tests
 *
 * **REFERENCE IMPLEMENTATION ONLY**
 * This code is provided for educational and testing purposes to demonstrate
 * Midnight ledger features. DO NOT use this code as-is in production.
 *
 * These tests validate the token-vault.compact contract's shielded & unshielded token operations
 * using the TypeScript WASM API. They cover:
 *
 * 1. Contract deployment with proper state initialization
 * 2. depositShielded & depositUnshielded in one call - first deposit (empty vault → new coin)
 * 3. withdrawShielded & withdrawUnshielded in one call - partial withdrawal (split vault into user coin + change)
 *
 * ## Shielded Token Concepts
 *
 * - **Commitments**: Hide the value and owner of coins
 * - **Nullifiers**: Prevent double-spending without revealing which coin was spent
 * - **ZSwap Offers**: Bundle inputs, outputs, transients, and deltas for atomic ops
 */

import {
  type AlignedValue,
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
  encodeCoinPublicKey,
  encodeContractAddress,
  encodeQualifiedShieldedCoinInfo,
  encodeShieldedCoinInfo,
  LedgerParameters,
  type Op,
  partitionTranscripts,
  persistentCommit,
  PreTranscript,
  type QualifiedShieldedCoinInfo,
  runtimeCoinCommitment,
  runtimeCoinNullifier,
  type ShieldedCoinInfo,
  StateMap,
  StateValue,
  Transaction,
  type Value,
  valueToBigInt,
  WellFormedStrictness,
  ZswapInput,
  ZswapOffer,
  ZswapOutput,
  addressFromKey,
  QueryContext,
  UnshieldedOffer,
  type UtxoOutput,
  type UtxoSpend
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
  EMPTY_VALUE,
  ONE_VALUE
} from '@/test/utils/value-alignment';
import { evolveFrom, getContextWithOffer } from '@/test/utils/zswap';
import {
  claimUnshieldedSpendOps,
  encodeAmount,
  encodeClaimedSpendKeyUser,
  encodeUnshieldedTokenType,
  receiveUnshieldedOps,
  sendUnshieldedOps,
  unshieldedBalanceLtOps
} from '@/test/utils/unshielded-ops';

describe('Ledger API - TokenVault Shielded And Unshielded', () => {
  // ============================================================================
  // Operation Names (matching token-vault.compact)
  // ============================================================================
  const DEPOSIT_SHIELDED = 'depositShielded';
  const WITHDRAW_SHIELDED = 'withdrawShielded';
  const GET_SHIELDED_BALANCE = 'getShieldedBalance';
  const DEPOSIT_UNSHIELDED = 'depositUnshielded';
  const WITHDRAW_UNSHIELDED = 'withdrawUnshielded';
  const GET_UNSHIELDED_BALANCE = 'getUnshieldedBalance';

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
  const STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS = 6;
  const STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS = 7;

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

    const depositUnshieldedOp = new ContractOperation();
    depositUnshieldedOp.verifierKey = TestResource.operationVerifierKey();

    const withdrawUnshieldedOp = new ContractOperation();
    withdrawUnshieldedOp.verifierKey = TestResource.operationVerifierKey();

    const getBalanceOp = new ContractOperation();
    getBalanceOp.verifierKey = TestResource.operationVerifierKey();

    return {
      depositShieldedOp,
      withdrawShieldedOp,
      depositUnshieldedOp,
      withdrawUnshieldedOp,
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
  function deployContract(state: TestState, ownerPk: Value, ops: ReturnType<typeof setupOperations>): ContractAddress {
    const contract = new ContractState();

    // Set up operations
    contract.setOperation(DEPOSIT_SHIELDED, ops.depositShieldedOp);
    contract.setOperation(WITHDRAW_SHIELDED, ops.withdrawShieldedOp);
    contract.setOperation(GET_SHIELDED_BALANCE, ops.getBalanceOp);
    contract.setOperation(DEPOSIT_UNSHIELDED, ops.depositUnshieldedOp);
    contract.setOperation(WITHDRAW_UNSHIELDED, ops.withdrawUnshieldedOp);
    contract.setOperation(GET_UNSHIELDED_BALANCE, ops.getBalanceOp);

    // Set initial state
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

    // Verify the contract was deployed
    const contract = state.ledger.index(addr);
    expect(contract).toBeDefined();
    expect(contract!.data.state.type()).toBe('array');

    // Verify an owner was set correctly
    const stateArray = contract!.data.state.asArray()!;
    const ownerCell = stateArray[STATE_IDX_OWNER].asCell();
    expect(ownerCell.value[0]).toEqual(ownerPk[0]);

    const hasShieldedTokensCell = stateArray[STATE_IDX_HAS_SHIELDED_TOKENS].asCell();
    expect(hasShieldedTokensCell.value[0]).toEqual(EMPTY_VALUE);
    const hasUnshieldedTokensCell = stateArray[STATE_IDX_HAS_SHIELDED_TOKENS].asCell();
    expect(hasUnshieldedTokensCell.value[0]).toEqual(EMPTY_VALUE);
  });

  /**
   * Test first shielded deposit into empty vault.
   *
   * @given A deployed token vault with no shielded tokens
   * @when Depositing shielded tokens
   * @then Should store the coin in shieldedVault and set hasShieldedTokens=true
   */
  test('should deposit shielded and unshielded tokens into an empty vault', () => {
    console.log(':: Shielded Token Vault Test - Part 1: Deposit');

    const state = TestState.new();

    // reward shielded token
    const REWARDS_SHIELDED_AMOUNT = 5_000_000_000n;
    const shieldedToken: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(shieldedToken, REWARDS_SHIELDED_AMOUNT);
    state.giveFeeToken(10, INITIAL_NIGHT_AMOUNT);

    // reward unshielded token
    const unshieldedTokenColor = Random.hex(64);
    const REWARDS_UNSHIELDED_AMOUNT = 2000n; // User has 2000 tokens in UTXO
    const DEPOSIT_UNSHIELDED_AMOUNT = 1500n; // User deposits 1500 to contract
    const CHANGE_UNSHIELDED_AMOUNT = REWARDS_UNSHIELDED_AMOUNT - DEPOSIT_UNSHIELDED_AMOUNT; // 500 goes back to the user

    // Give the user a UTXO with 2000 tokens
    const userAddress = addressFromKey(state.nightKey.verifyingKey());
    const tokenType = { tag: 'unshielded' as const, raw: unshieldedTokenColor };
    state.rewardsUnshielded(tokenType, REWARDS_UNSHIELDED_AMOUNT);

    // Get the user's UTXO that we'll spend
    const userUtxos = [...state.utxos].filter(
      (utxo) => utxo.type === unshieldedTokenColor && utxo.owner === userAddress
    );
    expect(userUtxos.length).toBeGreaterThan(0);
    const unshieldedUtxoToSpend = userUtxos[0];
    expect(unshieldedUtxoToSpend.value).toBe(REWARDS_UNSHIELDED_AMOUNT);

    // Deploy the contract
    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerPk, ops);
    const encodedAddr = encodeContractAddress(contractAddr);
    console.log('   Contract deployed at:', contractAddr);

    // Create a shielded coin to deposit
    const DEPOSIT_SHIELDED_AMOUNT = 1_000_000n;
    const coin = createShieldedCoinInfo(shieldedToken.raw, DEPOSIT_SHIELDED_AMOUNT);
    const out = ZswapOutput.newContractOwned(coin, undefined, contractAddr);
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
        value: [EMPTY_VALUE, EMPTY_VALUE, Static.trimTrailingZeros(encodedAddr)],
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
    const offer = ZswapOffer.fromOutput(out, shieldedToken.raw, DEPOSIT_SHIELDED_AMOUNT);

    const program = programWithResults(publicTranscript, publicTranscriptResults);
    const context = getContextWithOffer(state.ledger, contractAddr, offer);
    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const call = new ContractCallPrototype(
      contractAddr,
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

    // Build the unshielded token deposit transaction
    // The transcript declares how much the contract receives
    const unshieldedTokenTypeValue = encodeUnshieldedTokenType(unshieldedTokenColor);
    const depositUnshieldedAmountValue = encodeAmount(DEPOSIT_UNSHIELDED_AMOUNT);

    const contextUnshielded = new QueryContext(
      new ChargedState(state.ledger.index(contractAddr)!.data.state),
      contractAddr
    );

    const programUnshielded = programWithResults(
      [
        // receiveUnshielded(color, amount) - contract receives DEPOSIT_AMOUNT
        ...receiveUnshieldedOps(unshieldedTokenTypeValue, depositUnshieldedAmountValue),
        // totalUnshieldedDeposits.increment(1)
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)
      ],
      [] // No witness results needed
    );

    const callsUnshielded: PreTranscript[] = [new PreTranscript(contextUnshielded, programUnshielded)];
    const transcriptsUnshielded = partitionTranscripts(callsUnshielded, LedgerParameters.initialParameters());

    const callUnshielded = new ContractCallPrototype(
      contractAddr,
      DEPOSIT_UNSHIELDED,
      ops.depositUnshieldedOp,
      transcriptsUnshielded[0][0],
      transcriptsUnshielded[0][1],
      [], // No private inputs
      {
        value: [Static.encodeFromHex(unshieldedTokenColor), bigIntToValue(DEPOSIT_UNSHIELDED_AMOUNT)[0]],
        alignment: [ATOM_BYTES_32, { tag: 'atom', value: { tag: 'bytes', length: 16 } }]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_UNSHIELDED
    );

    // Spend full UTXO, create change output for the remainder
    const utxoSpend: UtxoSpend = {
      value: unshieldedUtxoToSpend.value, // Spend the full UTXO (2000)
      owner: state.nightKey.verifyingKey(),
      type: unshieldedUtxoToSpend.type,
      intentHash: unshieldedUtxoToSpend.intentHash,
      outputNo: unshieldedUtxoToSpend.outputNo
    };

    // Change output: the remaining tokens go back to the user
    const changeOutput: UtxoOutput = {
      value: CHANGE_UNSHIELDED_AMOUNT,
      owner: userAddress,
      type: unshieldedTokenColor
    };

    const intent = testIntents([call, callUnshielded], [], [], state.time);
    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [utxoSpend], // Input: spend the full UTXO
      [changeOutput], // Output: change back to user
      [] // Signatures added later
    );

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer, undefined, intent);
    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    console.log(tx.toString());

    const balancedStrictness = new WellFormedStrictness();
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, balancedStrictness);

    console.log(`   Deposited ${DEPOSIT_SHIELDED_AMOUNT} shielded tokens`);
    console.log(`   Deposited ${DEPOSIT_UNSHIELDED_AMOUNT} unshielded tokens`);

    // Verify hasShieldedTokens is now true
    const contract = state.ledger.index(contractAddr)!;
    const stateArray = contract.data.state.asArray()!;
    const hasShieldedTokensCell = stateArray[STATE_IDX_HAS_SHIELDED_TOKENS].asCell();
    expect(hasShieldedTokensCell.value[0]).toEqual(ONE_VALUE);

    // Verify vault has the coin
    const vaultCell = stateArray[STATE_IDX_SHIELDED_VAULT].asCell();
    expect(vaultCell.value[0]).toBeDefined();
    expect(vaultCell.value[0].length).toBeGreaterThan(0);

    // Verify the deposit counter was incremented
    const depositCounter = stateArray[STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS].asCell();
    expect(depositCounter).toBeDefined();

    // Verify the user received their change UTXO
    const afterTxUserUtxos = [...state.utxos].filter(
      (utxo) => utxo.type === unshieldedTokenColor && utxo.owner === userAddress
    );
    const changeUnshieldedUtxo = afterTxUserUtxos.find((utxo) => utxo.value === CHANGE_UNSHIELDED_AMOUNT);
    expect(changeUnshieldedUtxo).toBeDefined();
    console.log(`   User received unshielded change UTXO: ${changeUnshieldedUtxo!.value}`);

    // Verify the original unshielded UTXO was consumed (no longer in state)
    const originalStillExists = afterTxUserUtxos.find((utxo) => utxo.value === REWARDS_UNSHIELDED_AMOUNT);
    expect(originalStillExists).toBeUndefined();
    console.log('   Original unshielded UTXO consumed ✓');

    /*
     * PART 2: Withdraw deposited tokens
     */
    console.log(':: Shielded Token Vault Test - Part 2: Withdrawal');

    // First, withdraw the shielded token
    const WITHDRAW_SHIELDED_AMOUNT = 300_000n;
    const CHANGE_SHIELDED_AMOUNT = DEPOSIT_SHIELDED_AMOUNT - WITHDRAW_SHIELDED_AMOUNT;
    const WITHDRAW_UNSHIELDED_AMOUNT = 500n; // Withdraw a partial amount
    const REMAINING_UNSHIELDED_IN_CONTRACT = DEPOSIT_UNSHIELDED_AMOUNT - WITHDRAW_UNSHIELDED_AMOUNT; // 1500 stays in contract

    // Read existing pot from the contract state
    const cstate = state.ledger.index(contractAddr)!;
    const arr = cstate.data.state.asArray()!;
    const potCell = arr[STATE_IDX_SHIELDED_VAULT].asCell();
    const { value } = potCell;
    const valueAsBigInt = valueToBigInt([value[2]]);
    // mt_index is stored in value[3] after the coin commitment is added to the zswap tree
    const mtIndexAsBigInt = value[3] ? valueToBigInt([value[3]]) : 0n;

    const shieldedPot: QualifiedShieldedCoinInfo = decodeQualifiedShieldedCoinInfo({
      nonce: Static.trimTrailingZeros(value[0]),
      color: value[1].length === 0 ? Static.encodeFromHex(shieldedToken.raw) : value[1],
      value: valueAsBigInt,
      mt_index: mtIndexAsBigInt
    });

    // Create a withdrawal coin (goes to user) and a change coin (stays in contract)
    const withdrawShieldedCoin: ShieldedCoinInfo = evolveFrom(
      Static.encodeFromText('midnight:kernel:nonce_evolve'),
      WITHDRAW_SHIELDED_AMOUNT,
      shieldedPot.type,
      shieldedPot.nonce
    );
    const changeShieldedCoin: ShieldedCoinInfo = evolveFrom(
      Static.encodeFromText('midnight:kernel:nonce_evolve'),
      CHANGE_SHIELDED_AMOUNT,
      shieldedPot.type,
      shieldedPot.nonce
    );

    const encodedWithdrawCoin = encodeShieldedCoinInfo(withdrawShieldedCoin);
    const encodedChangeCoin = encodeShieldedCoinInfo(changeShieldedCoin);
    const encodedPot = encodeQualifiedShieldedCoinInfo(shieldedPot);
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
        value: [EMPTY_VALUE, EMPTY_VALUE, Static.trimTrailingZeros(encodedAddr)],
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
        value: [EMPTY_VALUE, EMPTY_VALUE, Static.trimTrailingZeros(encodedAddr)],
        alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
      }
    );

    // Build withdrawal transcript
    // Note: shieldedVault is read twice - once for the value check (shieldedVault.value >= amount)
    // and once for the sendShielded operation. This is how the Compact circuit works.
    const publicShieldedWithdrawalTranscript = [
      ...setMember(getKey(STATE_IDX_AUTHORIZED), false, { value: ownerPk, alignment: [ATOM_BYTES_32] }),
      ...cellRead(getKey(STATE_IDX_OWNER), false),
      ...cellRead(getKey(STATE_IDX_HAS_SHIELDED_TOKENS), false),
      ...cellRead(getKey(STATE_IDX_SHIELDED_VAULT), false), // First vault read (for value check)
      ...cellRead(getKey(STATE_IDX_SHIELDED_VAULT), false), // Second vault read (for sendShielded)
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

    // Results: Set_member(false), owner(ownerPk), hasShielded(true), pot (x2 for double read), kernel_self (x2)
    const publicShieldedWithdrawalTranscriptResults = [
      { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }, // authorized.member(pk) = false
      { value: ownerPk, alignment: [ATOM_BYTES_32] }, // owner = ownerPk (so pk == owner is true)
      { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }, // hasShieldedTokens = true
      {
        // First vault read (for value check)
        value: [
          Static.trimTrailingZeros(encodedPot.nonce),
          Static.trimTrailingZeros(encodedPot.color),
          bigIntToValue(encodedPot.value)[0],
          bigIntToValue(encodedPot.mt_index)[0]
        ],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_16, ATOM_BYTES_8]
      },
      {
        // Second vault read (for sendShielded)
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
    const potIn = ZswapInput.newContractOwned(shieldedPot, undefined, contractAddr, state.ledger.zswap);
    const withdrawOut = ZswapOutput.new(
      withdrawShieldedCoin,
      undefined,
      state.zswapKeys.coinPublicKey,
      state.zswapKeys.encryptionPublicKey
    );
    const changeOut = ZswapOutput.newContractOwned(changeShieldedCoin, undefined, contractAddr);

    // Track the withdrawn coin so user can receive it
    state.zswap = state.zswap.watchFor(state.zswapKeys.coinPublicKey, withdrawShieldedCoin);

    let withdrawalShieldedOffer = ZswapOffer.fromInput(potIn, shieldedPot.type, 0n);
    withdrawalShieldedOffer = withdrawalShieldedOffer.merge(
      ZswapOffer.fromOutput(withdrawOut, withdrawShieldedCoin.type, WITHDRAW_SHIELDED_AMOUNT)
    );
    withdrawalShieldedOffer = withdrawalShieldedOffer.merge(
      ZswapOffer.fromOutput(changeOut, changeShieldedCoin.type, CHANGE_SHIELDED_AMOUNT)
    );

    const withdrawShieldedProgram = programWithResults(
      publicShieldedWithdrawalTranscript,
      publicShieldedWithdrawalTranscriptResults
    );
    const withdrawShieldedContext = getContextWithOffer(state.ledger, contractAddr, withdrawalShieldedOffer);
    const withdrawShieldedCalls = [new PreTranscript(withdrawShieldedContext, withdrawShieldedProgram)];
    const withdrawShieldedTranscripts = partitionTranscripts(
      withdrawShieldedCalls,
      LedgerParameters.initialParameters()
    );

    const withdrawShieldedCall = new ContractCallPrototype(
      contractAddr,
      WITHDRAW_SHIELDED,
      ops.withdrawShieldedOp,
      withdrawShieldedTranscripts[0][0],
      withdrawShieldedTranscripts[0][1],
      [{ value: [ownerSk], alignment: [ATOM_BYTES_32] }], // Private: owner secret key for auth
      {
        value: [bigIntToValue(WITHDRAW_SHIELDED_AMOUNT)[0]],
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

    // Next, withdraw the unshielded token
    const withdrawUnshieldedAmountValue = encodeAmount(WITHDRAW_UNSHIELDED_AMOUNT);
    const claimKey = encodeClaimedSpendKeyUser(unshieldedTokenColor, userAddress);

    // Create context WITH the contract's unshielded balance
    // This is necessary because unshieldedBalanceGte reads from CallContext.balance
    const withdrawUnshieldedContext = new QueryContext(
      new ChargedState(state.ledger.index(contractAddr)!.data.state),
      contractAddr
    );
    // Set the contract's balance in the call context for balance checks
    const contractBalance = state.ledger.index(contractAddr)!.balance;
    const { block } = withdrawUnshieldedContext;
    block.balance = new Map(contractBalance);
    withdrawUnshieldedContext.block = block;

    const withdrawUnshieldedProgram = programWithResults(
      [
        // === isAuthorized() ===
        // Check if public key is in authorized set (state index 3)
        ...setMember(getKey(STATE_IDX_AUTHORIZED), false, { value: ownerPk, alignment: [ATOM_BYTES_32] }),
        // Read owner from state (state index 2)
        ...cellRead(getKey(STATE_IDX_OWNER), false),
        // === unshieldedBalanceGte() ===
        // Check if balance >= amount (implemented as !(balance < amount))
        ...unshieldedBalanceLtOps(unshieldedTokenTypeValue, withdrawUnshieldedAmountValue),
        // === sendUnshielded() ===
        // sendUnshielded increments unshielded_outputs (effects index 7)
        ...sendUnshieldedOps(unshieldedTokenTypeValue, withdrawUnshieldedAmountValue),
        // Also claim the unshielded spend (effects index 8)
        // This specifies: "User with public key X should receive these tokens"
        ...claimUnshieldedSpendOps(claimKey, withdrawUnshieldedAmountValue),
        // Counter_increment for totalUnshieldedWithdrawals (state index 7)
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS), false, 1)
      ],
      [
        // Results for operations that have Popeq { cached: true }
        // 1. Set_member returns false (pk is NOT in the authorized set)
        { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] },
        // 2. Cell_read returns owner_pk (the owner's public key - pk == owner succeeds)
        { value: ownerPk, alignment: [ATOM_BYTES_32] },
        // 3. unshieldedBalanceLtOps returns false (balance >= amount, NOT less than)
        { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }
      ]
    );

    const withdrawUnshieldedCalls: PreTranscript[] = [
      new PreTranscript(withdrawUnshieldedContext, withdrawUnshieldedProgram)
    ];
    const withdrawUnshieldedTranscripts = partitionTranscripts(
      withdrawUnshieldedCalls,
      LedgerParameters.initialParameters()
    );

    const withdrawUnshieldedCall = new ContractCallPrototype(
      contractAddr,
      WITHDRAW_UNSHIELDED,
      ops.withdrawUnshieldedOp,
      withdrawUnshieldedTranscripts[0][0],
      withdrawUnshieldedTranscripts[0][1],
      [{ value: [ownerSk], alignment: [ATOM_BYTES_32] }], // Private: owner sk for auth
      {
        // Input: (color, amount, recipient: Either<ContractAddress, UserAddress>)
        // PublicAddress encoding in Rust: vec![is_contract, contract_addr, user_addr]
        // For PublicAddress::User: vec![false, (), user_address]
        // - false is encoded as EMPTY_VALUE (empty Uint8Array), not new Uint8Array([0])
        // - () is unit type with NO bytes and NO alignment entry
        value: [
          Static.encodeFromHex(unshieldedTokenColor),
          bigIntToValue(WITHDRAW_UNSHIELDED_AMOUNT)[0],
          EMPTY_VALUE, // false = User address (not Contract)
          // Unit value for empty contract address - NO bytes added
          Static.encodeFromHex(userAddress)
        ],
        alignment: [
          ATOM_BYTES_32, // tokenColor
          { tag: 'atom', value: { tag: 'bytes', length: 16 } }, // amount
          ATOM_BYTES_1, // boolean (is_contract)
          // Unit has no alignment entry
          ATOM_BYTES_32 // user address
        ]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      WITHDRAW_UNSHIELDED
    );

    // User creates output UTXO to receive withdrawn tokens
    const withdrawUnshieldedOutput: UtxoOutput = {
      value: WITHDRAW_UNSHIELDED_AMOUNT,
      owner: userAddress,
      type: unshieldedTokenColor
    };

    const withdrawIntent = testIntents([withdrawShieldedCall, withdrawUnshieldedCall], [], [], state.time);
    withdrawIntent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [], // No inputs - tokens come from contract
      [withdrawUnshieldedOutput], // Output: new UTXO for user
      []
    );

    const withdrawTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, withdrawalShieldedOffer, undefined, withdrawIntent);
    withdrawTx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    console.log('::::withdrawTx::::');
    console.log(withdrawTx.toString());

    const balancedWithdrawTx = state.balanceTx(withdrawTx.eraseProofs());
    state.assertApply(balancedWithdrawTx, balancedStrictness);

    console.log(
      `   Withdrew ${WITHDRAW_SHIELDED_AMOUNT} shielded tokens / ${WITHDRAW_UNSHIELDED_AMOUNT} unshielded tokens`
    );
    console.log(
      `   Change remaining in vault: ${CHANGE_SHIELDED_AMOUNT} shielded tokens / ${CHANGE_SHIELDED_AMOUNT} unshielded tokens`
    );

    // Verify vault has change coin
    const finalContract = state.ledger.index(contractAddr)!;
    const finalStateArray = finalContract.data.state.asArray()!;
    const finalVaultCell = finalStateArray[STATE_IDX_SHIELDED_VAULT].asCell();
    const finalValue = valueToBigInt([finalVaultCell.value[2]]);
    expect(finalValue).toBe(CHANGE_SHIELDED_AMOUNT);

    const withdrawCounter = finalStateArray[STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS].asCell();
    expect(withdrawCounter).toBeDefined();

    // Verify hasShieldedTokens is still true (since there's a change)
    const finalHasTokens = finalStateArray[STATE_IDX_HAS_SHIELDED_TOKENS].asCell();
    expect(finalHasTokens.value[0]).toEqual(ONE_VALUE);

    // Verify the user received the withdrawn unshielded UTXO
    const finalUserUtxos = [...state.utxos].filter(
      (utxo) => utxo.type === unshieldedTokenColor && utxo.owner === userAddress
    );
    const withdrawnUtxo = finalUserUtxos.find((utxo) => utxo.value === WITHDRAW_UNSHIELDED_AMOUNT);
    expect(withdrawnUtxo).toBeDefined();
    console.log(`   User received UTXO: ${withdrawnUtxo!.value} unshielded tokens`);
    console.log(`   Contract retains: ${REMAINING_UNSHIELDED_IN_CONTRACT} unshielded tokens (in balance)`);

    console.log('   Partial withdrawal complete');
  });
});
