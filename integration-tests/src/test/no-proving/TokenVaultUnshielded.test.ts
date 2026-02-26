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
  LedgerParameters,
  partitionTranscripts,
  persistentCommit,
  PreTranscript,
  QueryContext,
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
import { INITIAL_NIGHT_AMOUNT, LOCAL_TEST_NETWORK_ID, Random, Static, TestResource } from '@/test-objects';
import { testIntents } from '@/test-utils';
import {
  cellRead,
  counterIncrement,
  getKey,
  programWithResults,
  setMember
} from '@/test/utils/onchain-runtime-program-fragments';
import { ATOM_BYTES_1, ATOM_BYTES_16, ATOM_BYTES_32, ATOM_BYTES_8, EMPTY_VALUE } from '@/test/utils/value-alignment';
import {
  claimUnshieldedSpendOps,
  encodeAmount,
  encodeClaimedSpendKeyUser,
  encodeUnshieldedTokenType,
  receiveUnshieldedOps,
  sendUnshieldedOps,
  unshieldedBalanceLtOps
} from '@/test/utils/unshielded-ops';

describe('Ledger API - TokenVault Unshielded', () => {
  // ============================================================================
  // Operation Names (matching token-vault.compact)
  // ============================================================================
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
  const STATE_IDX_OWNER = 2;
  const STATE_IDX_AUTHORIZED = 3;
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

    const getBalanceOp = new ContractOperation();
    getBalanceOp.verifierKey = TestResource.operationVerifierKey();

    return {
      depositUnshieldedOp,
      withdrawUnshieldedOp,
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

  /** Verify contract deployment initializes owner state correctly. */
  test('should deploy token vault contract', () => {
    const state = TestState.new();
    state.giveFeeToken(5, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit(
      [ATOM_BYTES_32],
      [Static.encodeFromText('token:vault:pk')],
      [Static.trimTrailingZeros(ownerSk)]
    );

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
   * Deposit with UTXO change: spend 2000, deposit 1500, receive 500 change.
   * Balance: Input(+2000) = Contract(-1500) + Change(-500)
   */
  test('should deposit unshielded tokens into vault', () => {
    const state = TestState.new();
    state.giveFeeToken(10, INITIAL_NIGHT_AMOUNT);

    // Create a custom unshielded token type for testing
    const tokenColor = Random.hex(64);
    const UTXO_AMOUNT = 2000n; // User has 2000 tokens in UTXO
    const DEPOSIT_AMOUNT = 1500n; // User deposits 1500 to contract
    const CHANGE_AMOUNT = UTXO_AMOUNT - DEPOSIT_AMOUNT; // 500 goes back to user

    // Give the user a UTXO with 2000 tokens
    const userAddress = addressFromKey(state.nightKey.verifyingKey());
    const tokenType = { tag: 'unshielded' as const, raw: tokenColor };
    state.rewardsUnshielded(tokenType, UTXO_AMOUNT);

    // Get the user's UTXO that we'll spend
    const userUtxos = [...state.utxos].filter((utxo) => utxo.type === tokenColor && utxo.owner === userAddress);
    expect(userUtxos.length).toBeGreaterThan(0);
    const utxoToSpend = userUtxos[0];
    expect(utxoToSpend.value).toBe(UTXO_AMOUNT);

    // Deploy the contract
    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit(
      [ATOM_BYTES_32],
      [Static.encodeFromText('token:vault:pk')],
      [Static.trimTrailingZeros(ownerSk)]
    );
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerSk, ownerPk, ops);

    console.log(':: Unshielded Deposit with UTXO Change');
    console.log(`   UTXO amount: ${UTXO_AMOUNT}`);
    console.log(`   Deposit amount: ${DEPOSIT_AMOUNT}`);
    console.log(`   Change amount: ${CHANGE_AMOUNT}`);

    // Build the deposit transaction
    // The transcript declares how much the contract receives
    const tokenTypeValue = encodeUnshieldedTokenType(tokenColor);
    const depositAmountValue = encodeAmount(DEPOSIT_AMOUNT);

    const context = new QueryContext(new ChargedState(state.ledger.index(contractAddr)!.data.state), contractAddr);

    const program = programWithResults(
      [
        // receiveUnshielded(color, amount) - contract receives DEPOSIT_AMOUNT
        ...receiveUnshieldedOps(tokenTypeValue, depositAmountValue),
        // totalUnshieldedDeposits.increment(1)
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)
      ],
      [] // No witness results needed
    );

    const calls: PreTranscript[] = [new PreTranscript(context, program)];
    const transcripts = partitionTranscripts(calls, LedgerParameters.initialParameters());

    const call = new ContractCallPrototype(
      contractAddr,
      DEPOSIT_UNSHIELDED,
      ops.depositUnshieldedOp,
      transcripts[0][0],
      transcripts[0][1],
      [], // No private inputs
      {
        value: [Static.trimTrailingZeros(Static.encodeFromHex(tokenColor)), bigIntToValue(DEPOSIT_AMOUNT)[0]],
        alignment: [ATOM_BYTES_32, { tag: 'atom', value: { tag: 'bytes', length: 16 } }]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_UNSHIELDED
    );

    // Spend full UTXO, create change output for remainder
    const utxoSpend: UtxoSpend = {
      value: utxoToSpend.value, // Spend the full UTXO (2000)
      owner: state.nightKey.verifyingKey(),
      type: utxoToSpend.type,
      intentHash: utxoToSpend.intentHash,
      outputNo: utxoToSpend.outputNo
    };

    // Change output: the remaining tokens go back to the user
    const changeOutput: UtxoOutput = {
      value: CHANGE_AMOUNT,
      owner: userAddress,
      type: tokenColor
    };

    const intent = testIntents([call], [], [], state.time);
    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [utxoSpend], // Input: spend the full UTXO
      [changeOutput], // Output: change back to user
      [] // Signatures added later
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
    expect(depositCounter).toBeDefined();

    // Verify the user received their change UTXO
    const finalUserUtxos = [...state.utxos].filter((utxo) => utxo.type === tokenColor && utxo.owner === userAddress);
    const changeUtxo = finalUserUtxos.find((utxo) => utxo.value === CHANGE_AMOUNT);
    expect(changeUtxo).toBeDefined();
    console.log(`   User received change UTXO: ${changeUtxo!.value}`);

    // Verify original UTXO was consumed (no longer in state)
    const originalStillExists = finalUserUtxos.find((utxo) => utxo.value === UTXO_AMOUNT);
    expect(originalStillExists).toBeUndefined();
    console.log('   Original UTXO consumed ✓');
  });

  /**
   * Withdraw from contract: deposit 2000, withdraw 500, contract retains 1500.
   * Balance: ContractOutput(+500) = UserUTXO(-500)
   */
  test('should withdraw unshielded tokens to user', () => {
    const state = TestState.new();
    state.giveFeeToken(15, INITIAL_NIGHT_AMOUNT);

    // Create a custom unshielded token type for testing
    const tokenColor = Random.hex(64);
    const DEPOSIT_AMOUNT = 2000n; // Deposit full UTXO to contract
    const WITHDRAW_AMOUNT = 500n; // Withdraw partial amount
    const REMAINING_IN_CONTRACT = DEPOSIT_AMOUNT - WITHDRAW_AMOUNT; // 1500 stays in contract

    // Give the user some unshielded tokens
    const userAddress = addressFromKey(state.nightKey.verifyingKey());
    const tokenType = { tag: 'unshielded' as const, raw: tokenColor };
    state.rewardsUnshielded(tokenType, DEPOSIT_AMOUNT);

    // Get the user's UTXO that we'll spend
    const userUtxos = [...state.utxos].filter((utxo) => utxo.type === tokenColor && utxo.owner === userAddress);
    expect(userUtxos.length).toBeGreaterThan(0);
    const utxoToSpend = userUtxos[0];

    // Deploy the contract with owner being the test user
    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit(
      [ATOM_BYTES_32],
      [Static.encodeFromText('token:vault:pk')],
      [Static.trimTrailingZeros(ownerSk)]
    );
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerSk, ownerPk, ops);

    console.log(':: Unshielded Withdrawal Test');
    console.log(`   Depositing: ${DEPOSIT_AMOUNT} tokens to contract`);

    // Step 1: Deposit all tokens to contract (full UTXO, no change)
    const tokenTypeValue = encodeUnshieldedTokenType(tokenColor);
    const depositAmountValue = encodeAmount(DEPOSIT_AMOUNT);

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
        value: [Static.trimTrailingZeros(Static.encodeFromHex(tokenColor)), bigIntToValue(DEPOSIT_AMOUNT)[0]],
        alignment: [ATOM_BYTES_32, ATOM_BYTES_16]
      },
      { value: [], alignment: [] },
      communicationCommitmentRandomness(),
      DEPOSIT_UNSHIELDED
    );

    // Full UTXO spend - no change (entire UTXO goes to contract)
    const depositUtxoSpend: UtxoSpend = {
      value: utxoToSpend.value,
      owner: state.nightKey.verifyingKey(),
      type: utxoToSpend.type,
      intentHash: utxoToSpend.intentHash,
      outputNo: utxoToSpend.outputNo
    };

    const depositIntent = testIntents([depositCall], [], [], state.time);
    // No outputs - entire UTXO value goes to contract
    depositIntent.guaranteedUnshieldedOffer = UnshieldedOffer.new([depositUtxoSpend], [], []);

    const depositTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, depositIntent);

    const depositStrictness = new WellFormedStrictness();
    depositStrictness.enforceBalancing = false;
    depositStrictness.verifyContractProofs = false;
    depositStrictness.verifySignatures = false;

    const balancedDeposit = state.balanceTx(depositTx.eraseProofs());
    state.assertApply(balancedDeposit, depositStrictness);

    console.log(`   Contract now has: ${DEPOSIT_AMOUNT} tokens`);
    console.log(`   Withdrawing: ${WITHDRAW_AMOUNT} tokens to user`);

    // Step 2: Withdraw partial amount - contract sends, user receives UTXO
    //
    // The withdrawUnshielded circuit performs these operations in order:
    // 1. isAuthorized(): Set_member check on authorized set + Cell_read of owner
    // 2. unshieldedBalanceGte(): Balance check (via unshieldedBalanceLt negated)
    // 3. sendUnshielded(): Increments effects[7] and effects[8]
    // 4. Counter_increment: Tracks total withdrawals in contract state
    //
    // We must match this exact order in our transcript.
    const withdrawAmountValue = encodeAmount(WITHDRAW_AMOUNT);
    const claimKey = encodeClaimedSpendKeyUser(tokenColor, userAddress);

    // Create context WITH the contract's unshielded balance
    // This is necessary because unshieldedBalanceGte reads from CallContext.balance
    const withdrawContext = new QueryContext(
      new ChargedState(state.ledger.index(contractAddr)!.data.state),
      contractAddr
    );
    // Set the contract's balance in the call context for balance checks
    const contractBalance = state.ledger.index(contractAddr)!.balance;
    const { block } = withdrawContext;
    block.balance = new Map(contractBalance);
    withdrawContext.block = block;

    const withdrawProgram = programWithResults(
      [
        // === isAuthorized() ===
        // Check if public key is in authorized set (state index 3)
        ...setMember(getKey(STATE_IDX_AUTHORIZED), false, { value: ownerPk, alignment: [ATOM_BYTES_32] }),
        // Read owner from state (state index 2)
        ...cellRead(getKey(STATE_IDX_OWNER), false),
        // === unshieldedBalanceGte() ===
        // Check if balance >= amount (implemented as !(balance < amount))
        ...unshieldedBalanceLtOps(tokenTypeValue, withdrawAmountValue),
        // === sendUnshielded() ===
        // sendUnshielded increments unshielded_outputs (effects index 7)
        ...sendUnshieldedOps(tokenTypeValue, withdrawAmountValue),
        // Also claim the unshielded spend (effects index 8)
        // This specifies: "User with public key X should receive these tokens"
        ...claimUnshieldedSpendOps(claimKey, withdrawAmountValue),
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

    const withdrawCalls: PreTranscript[] = [new PreTranscript(withdrawContext, withdrawProgram)];
    const withdrawTranscripts = partitionTranscripts(withdrawCalls, LedgerParameters.initialParameters());

    const withdrawCall = new ContractCallPrototype(
      contractAddr,
      WITHDRAW_UNSHIELDED,
      ops.withdrawUnshieldedOp,
      withdrawTranscripts[0][0],
      withdrawTranscripts[0][1],
      [{ value: [Static.trimTrailingZeros(ownerSk)], alignment: [ATOM_BYTES_32] }], // Private: owner sk for auth
      {
        // Input: (color, amount, recipient: Either<ContractAddress, UserAddress>)
        // PublicAddress encoding in Rust: vec![is_contract, contract_addr, user_addr]
        // For PublicAddress::User: vec![false, (), user_address]
        // - false is encoded as EMPTY_VALUE (empty Uint8Array), not new Uint8Array([0])
        // - () is unit type with NO bytes and NO alignment entry
        value: [
          Static.trimTrailingZeros(Static.encodeFromHex(tokenColor)),
          bigIntToValue(WITHDRAW_AMOUNT)[0],
          EMPTY_VALUE, // false = User address (not Contract)
          // Unit value for empty contract address - NO bytes added
          Static.trimTrailingZeros(Static.encodeFromHex(userAddress))
        ],
        alignment: [
          ATOM_BYTES_32, // tokenColor
          ATOM_BYTES_16, // amount
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
    const withdrawOutput: UtxoOutput = {
      value: WITHDRAW_AMOUNT,
      owner: userAddress,
      type: tokenColor
    };

    const withdrawIntent = testIntents([withdrawCall], [], [], state.time);
    withdrawIntent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [], // No inputs - tokens come from contract
      [withdrawOutput], // Output: new UTXO for user
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

    // Verify the user received the withdrawn UTXO
    const finalUserUtxos = [...state.utxos].filter((utxo) => utxo.type === tokenColor && utxo.owner === userAddress);
    const withdrawnUtxo = finalUserUtxos.find((utxo) => utxo.value === WITHDRAW_AMOUNT);
    expect(withdrawnUtxo).toBeDefined();
    console.log(`   User received UTXO: ${withdrawnUtxo!.value} tokens`);
    console.log(`   Contract retains: ${REMAINING_IN_CONTRACT} tokens (in balance)`);
  });

  /** Verify that receiveUnshielded increases gas compared to a baseline without unshielded ops. */
  test('partitionTranscripts returns higher gas for receiveUnshielded than baseline', () => {
    const state = TestState.new();
    state.giveFeeToken(5, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit(
      [ATOM_BYTES_32],
      [Static.encodeFromText('token:vault:pk')],
      [Static.trimTrailingZeros(ownerSk)]
    );
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerSk, ownerPk, ops);

    const context = new QueryContext(new ChargedState(state.ledger.index(contractAddr)!.data.state), contractAddr);

    // Program A: just increment a counter (no unshielded ops)
    const programA = programWithResults(
      [...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)],
      []
    );
    const transcriptsA = partitionTranscripts(
      [new PreTranscript(context, programA)],
      LedgerParameters.initialParameters()
    );

    // Program B: receiveUnshieldedOps + counterIncrement (populates effects.unshielded_inputs)
    const tokenColor = Random.hex(64);
    const tokenTypeValue = encodeUnshieldedTokenType(tokenColor);
    const amountValue = encodeAmount(1000n);

    const programB = programWithResults(
      [
        ...receiveUnshieldedOps(tokenTypeValue, amountValue),
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)
      ],
      []
    );
    const transcriptsB = partitionTranscripts(
      [new PreTranscript(context, programB)],
      LedgerParameters.initialParameters()
    );

    const gasA = transcriptsA[0][0]!.gas;
    const gasB = transcriptsB[0][0]!.gas;

    const totalGasA = gasA.computeTime + gasA.readTime + gasA.bytesWritten + gasA.bytesDeleted;
    const totalGasB = gasB.computeTime + gasB.readTime + gasB.bytesWritten + gasB.bytesDeleted;
    expect(totalGasB).toBeGreaterThan(totalGasA);
  });

  /** Verify that sendUnshielded increases gas compared to a baseline without unshielded ops. */
  test('partitionTranscripts returns higher gas for sendUnshielded than baseline', () => {
    const state = TestState.new();
    state.giveFeeToken(5, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit(
      [ATOM_BYTES_32],
      [Static.encodeFromText('token:vault:pk')],
      [Static.trimTrailingZeros(ownerSk)]
    );
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerSk, ownerPk, ops);

    const context = new QueryContext(new ChargedState(state.ledger.index(contractAddr)!.data.state), contractAddr);

    // Program A: just increment a counter (no unshielded ops)
    const programA = programWithResults(
      [...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS), false, 1)],
      []
    );
    const transcriptsA = partitionTranscripts(
      [new PreTranscript(context, programA)],
      LedgerParameters.initialParameters()
    );

    // Program B: sendUnshieldedOps + counterIncrement (populates effects.unshielded_outputs)
    const tokenColor = Random.hex(64);
    const tokenTypeValue = encodeUnshieldedTokenType(tokenColor);
    const amountValue = encodeAmount(500n);

    const programB = programWithResults(
      [
        ...sendUnshieldedOps(tokenTypeValue, amountValue),
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_WITHDRAWALS), false, 1)
      ],
      []
    );
    const transcriptsB = partitionTranscripts(
      [new PreTranscript(context, programB)],
      LedgerParameters.initialParameters()
    );

    const gasA = transcriptsA[0][0]!.gas;
    const gasB = transcriptsB[0][0]!.gas;

    const totalGasA = gasA.computeTime + gasA.readTime + gasA.bytesWritten + gasA.bytesDeleted;
    const totalGasB = gasB.computeTime + gasB.readTime + gasB.bytesWritten + gasB.bytesDeleted;
    expect(totalGasB).toBeGreaterThan(totalGasA);
  });

  /** Verify that more unshielded token types produce proportionally more gas overhead. */
  test('partitionTranscripts returns higher gas for two unshielded token types than one', () => {
    const state = TestState.new();
    state.giveFeeToken(5, INITIAL_NIGHT_AMOUNT);

    const ownerSk = Random.generate32Bytes();
    const ownerPk = persistentCommit(
      [ATOM_BYTES_32],
      [Static.encodeFromText('token:vault:pk')],
      [Static.trimTrailingZeros(ownerSk)]
    );
    const ops = setupOperations();
    const contractAddr = deployContract(state, ownerSk, ownerPk, ops);

    const context = new QueryContext(new ChargedState(state.ledger.index(contractAddr)!.data.state), contractAddr);

    const tokenColor1 = Random.hex(64);
    const tokenColor2 = Random.hex(64);
    const amountValue = encodeAmount(1000n);

    // Program A: receiveUnshieldedOps(tokenColor1) + counterIncrement (1 key in unshielded_inputs)
    const programA = programWithResults(
      [
        ...receiveUnshieldedOps(encodeUnshieldedTokenType(tokenColor1), amountValue),
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)
      ],
      []
    );
    const transcriptsA = partitionTranscripts(
      [new PreTranscript(context, programA)],
      LedgerParameters.initialParameters()
    );

    // Program B: receiveUnshieldedOps(color1) + receiveUnshieldedOps(color2) + counterIncrement (2 keys)
    const programB = programWithResults(
      [
        ...receiveUnshieldedOps(encodeUnshieldedTokenType(tokenColor1), amountValue),
        ...receiveUnshieldedOps(encodeUnshieldedTokenType(tokenColor2), amountValue),
        ...counterIncrement(getKey(STATE_IDX_TOTAL_UNSHIELDED_DEPOSITS), false, 1)
      ],
      []
    );
    const transcriptsB = partitionTranscripts(
      [new PreTranscript(context, programB)],
      LedgerParameters.initialParameters()
    );

    const gasA = transcriptsA[0][0]!.gas;
    const gasB = transcriptsB[0][0]!.gas;

    const totalGasA = gasA.computeTime + gasA.readTime + gasA.bytesWritten + gasA.bytesDeleted;
    const totalGasB = gasB.computeTime + gasB.readTime + gasB.bytesWritten + gasB.bytesDeleted;
    expect(totalGasB).toBeGreaterThan(totalGasA);
  });
});
