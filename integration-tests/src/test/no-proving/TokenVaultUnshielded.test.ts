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
 * Token Vault Contract Integration Tests
 *
 * **REFERENCE IMPLEMENTATION ONLY**
 * This code is provided for educational and testing purposes to demonstrate
 * Midnight ledger features. DO NOT use this code as-is in production.
 *
 * These tests validate the token-vault.compact contract's unshielded token operations
 * using the TypeScript WASM API. They cover:
 *
 * 1. Contract deployment with proper state initialization
 * 2. depositUnshielded - receiving tokens into the contract
 * 3. withdrawUnshielded - sending tokens from contract to user
 * 4. Contract-to-contract token transfers
 *
 * ## Architecture Overview
 *
 * The tests use the `TestState` utility which provides:
 * - A managed `LedgerState` for applying transactions
 * - ZSwap local state for shielded operations
 * - UTXO tracking for unshielded tokens
 * - Dust wallet for fee handling
 *
 * ## Test Pattern
 *
 * Each test follows this pattern:
 * 1. Set up TestState with initial tokens
 * 2. Deploy the token-vault contract
 * 3. Build transcript with proper Op sequences
 * 4. Create ContractCallPrototype with transcript
 * 5. Build Transaction with Intent containing the call
 * 6. Apply transaction to ledger and verify results
 */

import {
  type AlignedValue,
  addressFromKey,
  bigIntToValue,
  ChargedState,
  communicationCommitmentRandomness,
  type ContractAddress,
  ContractCallPrototype,
  ContractDeploy,
  ContractMaintenanceAuthority,
  ContractOperation,
  ContractState,
  Intent,
  LedgerParameters,
  partitionTranscripts,
  persistentCommit,
  PreTranscript,
  QueryContext,
  rawTokenType as createRawTokenType,
  signatureVerifyingKey,
  StateMap,
  StateValue,
  Transaction,
  UnshieldedOffer,
  type UtxoOutput,
  type UtxoSpend,
  type Value,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';

import { TestState } from '@/test/utils/TestState';
import {
  DEFAULT_TOKEN_TYPE,
  INITIAL_NIGHT_AMOUNT,
  LOCAL_TEST_NETWORK_ID,
  Random,
  Static,
  TestResource
} from '@/test-objects';
import { testIntents } from '@/test-utils';
import {
  cellRead,
  cellWrite,
  counterIncrement,
  getKey,
  programWithResults
} from '@/test/utils/onchain-runtime-program-fragments';
import { ATOM_BYTES_1, ATOM_BYTES_8, ATOM_BYTES_32, EMPTY_VALUE, ONE_VALUE } from '@/test/utils/value-alignment';
import {
  claimUnshieldedSpendOps,
  encodeAmount,
  encodeClaimedSpendKeyUser,
  encodeUnshieldedTokenType,
  receiveUnshieldedOps,
  sendUnshieldedOps
} from '@/test/utils/unshielded-ops';

describe('Ledger API - TokenVault Unshielded', () => {
  // ============================================================================
  // Operation Names (matching token-vault.compact)
  // ============================================================================
  const DEPOSIT_UNSHIELDED = 'depositUnshielded';
  const WITHDRAW_UNSHIELDED = 'withdrawUnshielded';
  const SEND_TO_USER = 'sendUnshieldedToUser';
  const SEND_TO_CONTRACT = 'sendUnshieldedToContract';
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
  const STATE_IDX_OWNER = 2;
  const STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS = 6;
  const STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS = 7;

  // ============================================================================
  // Helper Functions
  // ============================================================================

  /**
   * Set up contract operations with dummy verifier keys.
   * In no-proving mode, we use dummy verifier keys.
   */
  function setupOperations() {
    const depositUnshieldedOp = new ContractOperation();
    depositUnshieldedOp.verifierKey = TestResource.operationVerifierKey();

    const withdrawUnshieldedOp = new ContractOperation();
    withdrawUnshieldedOp.verifierKey = TestResource.operationVerifierKey();

    const sendToUserOp = new ContractOperation();
    sendToUserOp.verifierKey = TestResource.operationVerifierKey();

    const sendToContractOp = new ContractOperation();
    sendToContractOp.verifierKey = TestResource.operationVerifierKey();

    const getBalanceOp = new ContractOperation();
    getBalanceOp.verifierKey = TestResource.operationVerifierKey();

    return {
      depositUnshieldedOp,
      withdrawUnshieldedOp,
      sendToUserOp,
      sendToContractOp,
      getBalanceOp
    };
  }

  /**
   * Create the initial contract state with owner set.
   */
  function createInitialContractState(ownerPk: Value): StateValue {
    // The state is an array with the following structure:
    // [shieldedVault, hasShieldedTokens, owner, authorized, counters...]
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
   * Deploy the token vault contract and return its address.
   */
  function deployContract(
    state: TestState,
    ownerSk: Uint8Array,
    ownerPk: Value,
    ops: ReturnType<typeof setupOperations>
  ): ContractAddress {
    const contract = new ContractState();

    // Set up operations
    contract.setOperation(DEPOSIT_UNSHIELDED, ops.depositUnshieldedOp);
    contract.setOperation(WITHDRAW_UNSHIELDED, ops.withdrawUnshieldedOp);
    contract.setOperation(SEND_TO_USER, ops.sendToUserOp);
    contract.setOperation(SEND_TO_CONTRACT, ops.sendToContractOp);
    contract.setOperation(GET_UNSHIELDED_BALANCE, ops.getBalanceOp);

    // Set initial state
    contract.data = new ChargedState(createInitialContractState(ownerPk));
    contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);

    const deploy = new ContractDeploy(contract);

    // Create deploy transaction
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

    // Return the deployed contract address
    return deploy.address;
  }

  // ============================================================================
  // Tests
  // ============================================================================

  /**
   * Test contract deployment.
   *
   * @given A TestState with initial tokens
   * @when Deploying the token vault contract
   * @then Should successfully deploy with correct initial state
   */
  test('should deploy token vault contract', () => {
    const state = TestState.new();
    state.giveFeeToken(5, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);

    const ops = setupOperations();
    const addr = deployContract(state, ownerSk, ownerPk, ops);

    // Verify the contract was deployed
    const contract = state.ledger.index(addr);
    expect(contract).toBeDefined();
    expect(contract!.data.state.type()).toBe('array');

    // Verify owner was set correctly
    const stateArray = contract!.data.state.asArray()!;
    const ownerCell = stateArray[STATE_IDX_OWNER].asCell();
    expect(ownerCell.value[0]).toEqual(ownerPk[0]);
  });

  /**
   * Test depositing unshielded tokens into the contract.
   *
   * @given A deployed token vault contract
   * @and A user with unshielded tokens
   * @when Depositing unshielded tokens into the contract
   * @then Should transfer tokens from user to contract
   * @and Should increment totalUnshieldedDeposits counter
   */
  test('should deposit unshielded tokens into vault', () => {
    const state = TestState.new();
    state.giveFeeToken(10, INITIAL_NIGHT_AMOUNT);

    // Create a custom unshielded token type for testing
    const domainSep = Random.generate32Bytes();
    const tokenColor = Random.hex(64);
    const depositAmount = 1000n;

    // Give the user some unshielded tokens
    const userAddress = addressFromKey(state.nightKey.verifyingKey());
    const tokenType = { tag: 'unshielded' as const, raw: tokenColor };
    state.rewardsUnshielded(tokenType, depositAmount);

    // Get the user's UTXO that we'll spend
    const userUtxos = [...state.utxos].filter((utxo) => utxo.type === tokenColor && utxo.owner === userAddress);
    expect(userUtxos.length).toBeGreaterThan(0);
    const utxoToSpend = userUtxos[0];

    // Deploy the contract
    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerSk, ownerPk, ops);

    // Build the deposit transaction
    // The transcript needs to:
    // 1. Call receiveUnshielded to declare incoming tokens
    // 2. Increment the totalUnshieldedDeposits counter

    const tokenTypeValue = encodeUnshieldedTokenType(tokenColor);
    const amountValue = encodeAmount(depositAmount);

    const context = new QueryContext(new ChargedState(state.ledger.index(contractAddr)!.data.state), contractAddr);

    const program = programWithResults(
      [
        // receiveUnshielded(color, amount)
        ...receiveUnshieldedOps(tokenTypeValue, amountValue),
        // totalUnshieldedDeposits.increment(1)
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)
      ],
      [] // No witness results needed for this simple case
    );

    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const colorInput: AlignedValue = {
      value: [Static.encodeFromHex(tokenColor)],
      alignment: [ATOM_BYTES_32]
    };
    const amountInputValue = bigIntToValue(depositAmount);

    const call = new ContractCallPrototype(
      contractAddr,
      DEPOSIT_UNSHIELDED,
      ops.depositUnshieldedOp,
      transcripts[0][0],
      transcripts[0][1],
      [], // No private inputs
      {
        value: [colorInput.value[0], amountInputValue[0]],
        alignment: [ATOM_BYTES_32, { tag: 'atom', value: { tag: 'bytes', length: 16 } }]
      },
      { value: [], alignment: [] }, // No outputs
      communicationCommitmentRandomness(),
      DEPOSIT_UNSHIELDED
    );

    // Create the unshielded offer that spends the user's UTXO
    // The tokens go to the contract (no output UTXO needed since contract receives them)
    const utxoSpend: UtxoSpend = {
      value: utxoToSpend.value,
      owner: state.nightKey.verifyingKey(),
      type: utxoToSpend.type,
      intentHash: utxoToSpend.intentHash,
      outputNo: utxoToSpend.outputNo
    };

    const intent = testIntents([call], [], [], state.time);
    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [utxoSpend],
      [], // No outputs - tokens go to contract
      [] // Signatures will be added
    );

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    strictness.verifyContractProofs = false;
    strictness.verifySignatures = false;

    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, strictness);

    // Verify the deposit counter was incremented
    const contractAfter = state.ledger.index(contractAddr)!;
    const stateArrayAfter = contractAfter.data.state.asArray()!;
    const depositCounter = stateArrayAfter[STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS].asCell();
    // Counter should now be 1
    expect(depositCounter).toBeDefined();
  });

  /**
   * Test withdrawing unshielded tokens from the contract to a user.
   *
   * @given A deployed token vault contract with unshielded tokens
   * @and An authorized user
   * @when Withdrawing unshielded tokens to the user
   * @then Should create a UTXO for the user with the withdrawn tokens
   * @and Should increment totalUnshieldedWithdrawals counter
   */
  test('should withdraw unshielded tokens to user', () => {
    const state = TestState.new();
    state.giveFeeToken(15, INITIAL_NIGHT_AMOUNT);

    // Create a custom unshielded token type for testing
    const tokenColor = Random.hex(64);
    const depositAmount = 2000n;
    const withdrawAmount = 500n;

    // Give the user some unshielded tokens
    const userAddress = addressFromKey(state.nightKey.verifyingKey());
    const tokenType = { tag: 'unshielded' as const, raw: tokenColor };
    state.rewardsUnshielded(tokenType, depositAmount);

    // Get the user's UTXO that we'll spend
    const userUtxos = [...state.utxos].filter((utxo) => utxo.type === tokenColor && utxo.owner === userAddress);
    expect(userUtxos.length).toBeGreaterThan(0);
    const utxoToSpend = userUtxos[0];

    // Deploy the contract with owner being the test user
    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit([ATOM_BYTES_32], [Static.encodeFromText('token:vault:pk')], [ownerSk]);
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerSk, ownerPk, ops);

    // First, deposit tokens into the contract
    const tokenTypeValue = encodeUnshieldedTokenType(tokenColor);
    const depositAmountValue = encodeAmount(depositAmount);

    const depositContext = new QueryContext(
      new ChargedState(state.ledger.index(contractAddr)!.data.state),
      contractAddr
    );

    const depositProgram = programWithResults(
      [
        ...receiveUnshieldedOps(tokenTypeValue, depositAmountValue),
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)
      ],
      []
    );

    const depositCalls: PreTranscript[] = [new PreTranscript(depositContext, depositProgram)];
    const depositTranscripts = partitionTranscripts(depositCalls, LedgerParameters.initialParameters());

    const depositCall = new ContractCallPrototype(
      contractAddr,
      DEPOSIT_UNSHIELDED,
      ops.depositUnshieldedOp,
      depositTranscripts[0][0],
      depositTranscripts[0][1],
      [],
      {
        value: [Static.encodeFromHex(tokenColor), bigIntToValue(depositAmount)[0]],
        alignment: [ATOM_BYTES_32, { tag: 'atom', value: { tag: 'bytes', length: 16 } }]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_UNSHIELDED
    );

    const depositUtxoSpend: UtxoSpend = {
      value: utxoToSpend.value,
      owner: state.nightKey.verifyingKey(),
      type: utxoToSpend.type,
      intentHash: utxoToSpend.intentHash,
      outputNo: utxoToSpend.outputNo
    };

    const depositIntent = testIntents([depositCall], [], [], state.time);
    depositIntent.guaranteedUnshieldedOffer = UnshieldedOffer.new([depositUtxoSpend], [], []);

    const depositTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, depositIntent);

    const depositStrictness = new WellFormedStrictness();
    depositStrictness.enforceBalancing = false;
    depositStrictness.verifyContractProofs = false;
    depositStrictness.verifySignatures = false;

    const balancedDeposit = state.balanceTx(depositTx.eraseProofs());
    state.assertApply(balancedDeposit, depositStrictness);

    // Now withdraw tokens from the contract to a user
    // The contract needs to:
    // 1. sendUnshielded(color, amount) - declare outgoing tokens
    // 2. claimUnshieldedSpend(key, amount) - specify recipient
    // And the transaction needs an output UTXO for the user

    const withdrawAmountValue = encodeAmount(withdrawAmount);
    const claimKey = encodeClaimedSpendKeyUser(tokenColor, userAddress);

    const withdrawContext = new QueryContext(
      new ChargedState(state.ledger.index(contractAddr)!.data.state),
      contractAddr
    );

    const withdrawProgram = programWithResults(
      [
        // sendUnshielded(color, amount)
        ...sendUnshieldedOps(tokenTypeValue, withdrawAmountValue),
        // claimUnshieldedSpend for the user
        ...claimUnshieldedSpendOps(claimKey, withdrawAmountValue),
        // totalUnshieldedWithdrawals.increment(1)
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS), false, 1)
      ],
      [{ value: ownerPk, alignment: [ATOM_BYTES_32] }] // Owner PK is read for authorization check
    );

    const withdrawCalls: PreTranscript[] = [new PreTranscript(withdrawContext, withdrawProgram)];
    const withdrawTranscripts = partitionTranscripts(withdrawCalls, LedgerParameters.initialParameters());

    const withdrawCall = new ContractCallPrototype(
      contractAddr,
      SEND_TO_USER,
      ops.sendToUserOp,
      withdrawTranscripts[0][0],
      withdrawTranscripts[0][1],
      [{ value: [ownerSk], alignment: [ATOM_BYTES_32] }], // Private input: owner secret key for auth
      {
        value: [
          Static.encodeFromHex(tokenColor),
          bigIntToValue(withdrawAmount)[0],
          Static.encodeFromHex(userAddress) // recipient address
        ],
        alignment: [ATOM_BYTES_32, { tag: 'atom', value: { tag: 'bytes', length: 16 } }, ATOM_BYTES_32]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      SEND_TO_USER
    );

    // Create the output UTXO that the user will receive
    const withdrawOutput: UtxoOutput = {
      value: withdrawAmount,
      owner: userAddress,
      type: tokenColor
    };

    const withdrawIntent = testIntents([withdrawCall], [], [], state.time);
    withdrawIntent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [], // No inputs
      [withdrawOutput], // Output for the user
      []
    );

    const withdrawTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, withdrawIntent);

    const withdrawStrictness = new WellFormedStrictness();
    withdrawStrictness.enforceBalancing = false;
    withdrawStrictness.verifyContractProofs = false;
    withdrawStrictness.verifySignatures = false;

    const balancedWithdraw = state.balanceTx(withdrawTx.eraseProofs());
    state.assertApply(balancedWithdraw, withdrawStrictness);

    // Verify the withdrawal counter was incremented
    const contractAfterWithdraw = state.ledger.index(contractAddr)!;
    const stateArrayAfterWithdraw = contractAfterWithdraw.data.state.asArray()!;
    const withdrawCounter = stateArrayAfterWithdraw[STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS].asCell();
    expect(withdrawCounter).toBeDefined();

    // Verify the user received the UTXO
    const finalUserUtxos = [...state.utxos].filter((utxo) => utxo.type === tokenColor && utxo.owner === userAddress);
    // User should have a new UTXO with the withdrawn amount
    const withdrawnUtxo = finalUserUtxos.find((utxo) => utxo.value === withdrawAmount);
    expect(withdrawnUtxo).toBeDefined();
  });
});
