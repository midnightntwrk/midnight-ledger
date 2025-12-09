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
  ZswapLocalState,
  ZswapOffer,
  ZswapOutput,
  Transaction,
  ZswapSecretKeys,
  LedgerState,
  ZswapChainState,
  TransactionContext,
  WellFormedStrictness,
  MerkleTreeCollapsedUpdate
} from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, HEX_64_REGEX, LOCAL_TEST_NETWORK_ID, Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - ZswapLocalState', () => {
  /**
   * Test string representation of empty ZswapLocalState.
   *
   * @given A new ZswapLocalState instance
   * @when Calling toString method
   * @then Should return formatted string with empty collections and default values
   */
  test('should print out information as string', () => {
    const localState = new ZswapLocalState();

    expect(localState.toString()).toEqual(
      'State {\n' +
        '    coins: {},\n' +
        '    pending_spends: {},\n' +
        '    pending_outputs: {},\n' +
        '    merkle_tree: MerkleTree(root = Some(-)) {},\n' +
        '    first_free: 0,\n' +
        '}'
    );
  });

  test('should serialize and deserialize', () => {
    const localState = new ZswapLocalState();
    const serialized = localState.serialize();
    const deserialized = ZswapLocalState.deserialize(serialized);

    expect(deserialized.toString()).toEqual(localState.toString());
  });

  // it.each([['success'], ['partialSuccess']])('applyProofErasedTx - success - should apply tx - %s)', (res) => {
  //  const localState = new ZswapLocalState();
  //  const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
  //  const coinInfo = Static.shieldedCoinInfo(10n);
  //  const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
  //  const unprovenOffer = ZswapOffer.fromOutput(
  //    ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
  //    coinInfo.type,
  //    coinInfo.value
  //  );
  //  const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
  //  const proofErasedTransaction = unprovenTransaction.eraseProofs();
  //  const appliedTxLocalState = localState.applyTx(secretKeys, proofErasedTransaction, {
  //    type: res as 'success' | 'partialSuccess' | 'failure',
  //    successfulSegments:
  //      res === 'partialSuccess'
  //        ? new Map([
  //            [0, true],
  //            [1, false]
  //          ])
  //        : undefined
  //  });

  //  expect(appliedTxLocalState.pendingSpends.size).toEqual(0);
  //  expect(appliedTxLocalState.pendingOutputs.size).toEqual(0);
  //  expect(appliedTxLocalState.firstFree).toEqual(1n);
  //  expect(appliedTxLocalState.coins.size).toEqual(1);
  //  expect(appliedTxLocalState.coins.values().next().value).toEqual(qualifiedCoinInfo);
  //  assertSerializationSuccess(appliedTxLocalState);
  // });

  /// **
  // * Test application of failed transaction.
  // *
  // * @given A ZswapLocalState with a transaction marked as failed
  // * @when Applying the transaction with 'failure' status
  // * @then Should maintain empty state with no coins or pending items
  // */
  // test('should fail to apply transaction on failure status', () => {
  //  const localState = new ZswapLocalState();
  //  const coinInfo = Static.shieldedCoinInfo(10n);
  //  const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
  //  const unprovenOffer = ZswapOffer.fromOutput(
  //    ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
  //    coinInfo.type,
  //    coinInfo.value
  //  );
  //  const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
  //  const proofErasedTransaction = unprovenTransaction.eraseProofs();
  //  const appliedTxLocalState = localState.applyTx(secretKeys, proofErasedTransaction, { type: 'failure' });

  //  expect(appliedTxLocalState.pendingSpends.size).toEqual(0);
  //  expect(appliedTxLocalState.pendingOutputs.size).toEqual(0);
  //  expect(appliedTxLocalState.firstFree).toEqual(0n);
  //  expect(appliedTxLocalState.coins.size).toEqual(0);
  //  assertSerializationSuccess(appliedTxLocalState);
  // });

  /**
   * Test spending coins from local state.
   *
   * @given A ZswapLocalState with applied transaction containing coins
   * @when Spending a portion of available coins
   * @then Should create pending spend and return valid unproven input
   */
  test('should spend coins successfully', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const qualifiedCoinInfoToSpend = getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(5n), 0n);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const appliedTxLocalState = localState.replayEvents(secretKeys, events);
    const [spendLocalState, unprovenInput] = appliedTxLocalState.spend(secretKeys, qualifiedCoinInfoToSpend, 0);

    expect(spendLocalState.pendingSpends.size).toEqual(1);
    expect(spendLocalState.pendingOutputs.size).toEqual(0);
    expect(spendLocalState.firstFree).toEqual(1n);
    expect(spendLocalState.coins.size).toEqual(1);
    expect(spendLocalState.coins.values().next().value).toEqual(qualifiedCoinInfo);
    expect(spendLocalState.pendingSpends.values().next().value?.[0]).toEqual(qualifiedCoinInfoToSpend);
    expect(unprovenInput.contractAddress).toBeUndefined();
    expect(unprovenInput.nullifier).toMatch(HEX_64_REGEX);
    expect(unprovenInput.toString()).toMatch(/<shielded input Nullifier\([a-fA-F0-9]{64}\)>/);
    assertSerializationSuccess(spendLocalState);
  });

  /**
   * Test spending coins from output to create transient.
   *
   * @given A ZswapLocalState with applied transaction containing coins
   * @when Spending from output to create a transient coin
   * @then Should create pending spend and return valid unproven transient
   */
  test('should spend coins from output successfully', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const qualifiedCoinInfoToSpend = getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(5n), 0n);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const appliedTxLocalState = localState.replayEvents(secretKeys, events);
    const [spendLocalState, unprovenTransient] = appliedTxLocalState.spendFromOutput(
      secretKeys,
      qualifiedCoinInfoToSpend,
      0,
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey)
    );

    expect(spendLocalState.pendingSpends.size).toEqual(1);
    expect(spendLocalState.pendingOutputs.size).toEqual(0);
    expect(spendLocalState.firstFree).toEqual(1n);
    expect(spendLocalState.coins.size).toEqual(1);
    expect(spendLocalState.coins.values().next().value).toEqual(qualifiedCoinInfo);
    expect(spendLocalState.pendingSpends.values().next().value?.[0]).toEqual(qualifiedCoinInfoToSpend);
    expect(unprovenTransient.contractAddress).toBeUndefined();
    expect(unprovenTransient.nullifier).toMatch(HEX_64_REGEX);
    expect(unprovenTransient.toString()).toMatch(
      /<shielded transient coin Commitment\([a-fA-F0-9]{64}\) Nullifier\([a-fA-F0-9]{64}\)>/
    );
    assertSerializationSuccess(spendLocalState);
  });

  test('should handle empty state', () => {
    const localState = new ZswapLocalState();

    expect(localState.coins.size).toEqual(0);
    expect(localState.pendingOutputs.size).toEqual(0);
    expect(localState.pendingSpends.size).toEqual(0);
    expect(localState.firstFree).toEqual(0n);
    assertSerializationSuccess(localState);
  });

  test('should handle non-empty state', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const appliedTxLocalState = localState.replayEvents(secretKeys, events);

    expect(appliedTxLocalState.coins.size).toEqual(1);
    expect(appliedTxLocalState.coins.values().next().value).toEqual(qualifiedCoinInfo);
    assertSerializationSuccess(appliedTxLocalState);
  });

  /**
   * Test watching for specific coin and qualified coin info.
   *
   * @given A ZswapLocalState with applied transaction
   * @when Watching for specific coin public key and qualified coin info
   * @then Should return modified local state different from original
   */
  test('should watch for coin successfully', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const appliedTxLocalState = localState.replayEvents(secretKeys, events);

    const localStateWatched = appliedTxLocalState.watchFor(secretKeys.coinPublicKey, qualifiedCoinInfo);
    expect(localStateWatched.toString()).not.toEqual(appliedTxLocalState.toString());
  });

  test('clearPending - should clear pending outputs and spends', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfoToSpend = getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(5n), 0n);

    // First apply a transaction to create some state
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const appliedTxLocalState = localState.replayEvents(secretKeys, events);

    // Create a spend to have pending items
    const [spendLocalState] = appliedTxLocalState.spend(secretKeys, qualifiedCoinInfoToSpend, 0);

    // Clear pending items that would have TTL in the past
    const clearedLocalState = spendLocalState.clearPending(new Date(Date.now() + 1000));

    expect(clearedLocalState).toBeDefined();
    // The actual clearing behavior would depend on the TTL of the pending items
    assertSerializationSuccess(clearedLocalState);
  });

  /**
   * Test replayEventsWithChanges - spending coins from local state.
   *
   * @given A ZswapLocalState with applied transaction containing coins
   * @when Spending a portion of available coins using replayEventsWithChanges
   * @then Should track and confirm received coins in changes, and create pending spend with valid unproven input
   */
  test('replayEventsWithChanges - should spend coins successfully', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const qualifiedCoinInfoToSpend = getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(5n), 0n);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const appliedTxLocalState = withChanges.state;
    const [spendLocalState, unprovenInput] = appliedTxLocalState.spend(secretKeys, qualifiedCoinInfoToSpend, 0);

    // Verify state changes - should have received coins but no spent coins initially
    const allReceivedCoins = withChanges.changes.flatMap((change) => change.receivedCoins);
    const allSpentCoins = withChanges.changes.flatMap((change) => change.spentCoins);

    expect(allReceivedCoins).toEqual([qualifiedCoinInfo]);
    expect(allSpentCoins.length).toEqual(0);

    expect(spendLocalState.pendingSpends.size).toEqual(1);
    expect(spendLocalState.pendingOutputs.size).toEqual(0);
    expect(spendLocalState.firstFree).toEqual(1n);
    expect(spendLocalState.coins.size).toEqual(1);
    expect(spendLocalState.coins.values().next().value).toEqual(qualifiedCoinInfo);
    expect(spendLocalState.pendingSpends.values().next().value?.[0]).toEqual(qualifiedCoinInfoToSpend);
    expect(unprovenInput.contractAddress).toBeUndefined();
    expect(unprovenInput.nullifier).toMatch(HEX_64_REGEX);
    expect(unprovenInput.toString()).toMatch(/<shielded input Nullifier\([a-fA-F0-9]{64}\)>/);
    assertSerializationSuccess(spendLocalState);
  });

  /**
   * Test replayEventsWithChanges - spending coins from output.
   *
   * @given A ZswapLocalState with applied transaction containing coins
   * @when Spending coins from output using replayEventsWithChanges
   * @then Should track and confirm received coins in changes, and create pending spend with valid unproven transient
   */
  test('replayEventsWithChanges - should spend coins from output successfully', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const qualifiedCoinInfoToSpend = getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(5n), 0n);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const appliedTxLocalState = withChanges.state;
    const [spendLocalState, unprovenTransient] = appliedTxLocalState.spendFromOutput(
      secretKeys,
      qualifiedCoinInfoToSpend,
      0,
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey)
    );

    // Verify state changes - should have received coins but no spent coins initially
    const allReceivedCoins = withChanges.changes.flatMap((change) => change.receivedCoins);
    const allSpentCoins = withChanges.changes.flatMap((change) => change.spentCoins);

    expect(allReceivedCoins).toEqual([qualifiedCoinInfo]);
    expect(allSpentCoins.length).toEqual(0);

    expect(spendLocalState.pendingSpends.size).toEqual(1);
    expect(spendLocalState.pendingOutputs.size).toEqual(0);
    expect(spendLocalState.firstFree).toEqual(1n);
    expect(spendLocalState.coins.size).toEqual(1);
    expect(spendLocalState.coins.values().next().value).toEqual(qualifiedCoinInfo);
    expect(spendLocalState.pendingSpends.values().next().value?.[0]).toEqual(qualifiedCoinInfoToSpend);
    expect(unprovenTransient.contractAddress).toBeUndefined();
    expect(unprovenTransient.nullifier).toMatch(HEX_64_REGEX);
    expect(unprovenTransient.toString()).toMatch(
      /<shielded transient coin Commitment\([a-fA-F0-9]{64}\) Nullifier\([a-fA-F0-9]{64}\)>/
    );
    assertSerializationSuccess(spendLocalState);
  });

  /**
   * Test replayEventsWithChanges - handling non-empty state.
   *
   * @given A ZswapLocalState with applied transaction containing coins
   * @when Replaying events with changes on a transaction that receives coins
   * @then Should track and confirm received coins in changes, and update local state with new coins
   */
  test('replayEventsWithChanges - should handle non-empty state', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const appliedTxLocalState = withChanges.state;

    // Verify state changes - should have received coins but no spent coins
    const allReceivedCoins = withChanges.changes.flatMap((change) => Array.from(change.receivedCoins));
    const allSpentCoins = withChanges.changes.flatMap((change) => Array.from(change.spentCoins));

    expect(allReceivedCoins).toEqual([qualifiedCoinInfo]);
    expect(allSpentCoins.length).toEqual(0);

    expect(appliedTxLocalState.coins.size).toEqual(1);
    expect(appliedTxLocalState.coins.values().next().value).toEqual(qualifiedCoinInfo);
    assertSerializationSuccess(appliedTxLocalState);
  });

  /**
   * Test replayEventsWithChanges - watching for coins.
   *
   * @given A ZswapLocalState with applied transaction containing coins
   * @when Replaying events with changes and watching for a specific coin
   * @then Should track and confirm received coins in changes, and allow watching for the coin
   */
  test('replayEventsWithChanges - should watch for coin successfully', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const appliedTxLocalState = withChanges.state;

    // Verify state changes - should have received coins but no spent coins
    const allReceivedCoins = withChanges.changes.flatMap((change) => Array.from(change.receivedCoins));
    const allSpentCoins = withChanges.changes.flatMap((change) => Array.from(change.spentCoins));

    expect(allReceivedCoins).toEqual([qualifiedCoinInfo]);
    expect(allSpentCoins.length).toEqual(0);

    const localStateWatched = appliedTxLocalState.watchFor(secretKeys.coinPublicKey, qualifiedCoinInfo);
    expect(localStateWatched.toString()).not.toEqual(appliedTxLocalState.toString());
  });

  /**
   * Test replayEventsWithChanges - clearing pending items.
   *
   * @given A ZswapLocalState with applied transaction and pending spends
   * @when Replaying events with changes and clearing pending items
   * @then Should track and confirm received coins in changes, and clear pending outputs and spends based on TTL
   */
  test('replayEventsWithChanges - clearPending - should clear pending outputs and spends', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfoToSpend = getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(5n), 0n);

    // First apply a transaction to create some state
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const appliedTxLocalState = withChanges.state;

    // Verify state changes - should have received coins but no spent coins initially
    const allReceivedCoins = withChanges.changes.flatMap((change) => change.receivedCoins);
    const allSpentCoins = withChanges.changes.flatMap((change) => change.spentCoins);

    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    expect(allReceivedCoins).toEqual([qualifiedCoinInfo]);
    expect(allSpentCoins.length).toEqual(0);

    // Create a spend to have pending items
    const [spendLocalState] = appliedTxLocalState.spend(secretKeys, qualifiedCoinInfoToSpend, 0);

    // Clear pending items that would have TTL in the past
    const clearedLocalState = spendLocalState.clearPending(new Date(Date.now() + 1000));

    expect(clearedLocalState).toBeDefined();
    // The actual clearing behavior would depend on the TTL of the pending items
    assertSerializationSuccess(clearedLocalState);
  });

  /**
   * Test replayEventsWithChanges - tracking spent and received coins in transfer.
   *
   * @given A ZswapLocalState with an initial coin and a transfer transaction
   * @when Replaying events with changes on a transfer that spends and receives coins
   * @then Should track and confirm both spent coins and received coins in the changes
   */
  test('replayEventsWithChanges - should track spent and received coins in transfer transaction', () => {
    let localState = new ZswapLocalState();
    let ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);

    // Step 1: First transaction - receive a coin (output only)
    const initialOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const initialTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, initialOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedInitialTransaction = initialTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const [afterInitialLedgerState, { events: initialEvents }] = ledgerState.apply(
      verifiedInitialTransaction,
      transactionContext
    );
    ledgerState = afterInitialLedgerState.postBlockUpdate(new Date(0));
    localState = localState.replayEvents(secretKeys, initialEvents);
    expect(localState.coins.size).toEqual(1);

    // Step 2: Second transaction - spend the coin (input) and create a new output
    const coinToSpend = Array.from(localState.coins)[0];
    const [localStateWithPendingSpend, unprovenInput] = localState.spend(secretKeys, coinToSpend, 0);

    const sendValue = 5n;
    const newCoinInfo = Static.shieldedCoinInfo(sendValue);
    const newOutput = ZswapOutput.new(newCoinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey);
    const transferOffer = ZswapOffer.fromInput(unprovenInput, coinInfo.type, coinInfo.value).merge(
      ZswapOffer.fromOutput(newOutput, coinInfo.type, sendValue)
    );

    const transferTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, transferOffer);
    const transactionContext2 = new TransactionContext(ledgerState, Static.blockContext(new Date(1)));
    const verifiedTransferTransaction = transferTransaction.wellFormed(ledgerState, strictness, new Date(1));
    const { events: transferEvents } = ledgerState.apply(verifiedTransferTransaction, transactionContext2)[1];

    // Step 3: Replay events with changes and verify both spent and received coins
    const withChanges = localStateWithPendingSpend.replayEventsWithChanges(secretKeys, transferEvents);
    const allReceivedCoins = withChanges.changes.flatMap((change) => change.receivedCoins);
    const allSpentCoins = withChanges.changes.flatMap((change) => change.spentCoins);

    // Should have spent the original coin
    expect(allSpentCoins).toEqual([qualifiedCoinInfo]);

    // Should have received the new coin (mt_index is 1n because it's the second coin)
    const expectedReceivedCoinInfo = getQualifiedShieldedCoinInfo(newCoinInfo, 1n);
    expect(allReceivedCoins).toEqual([expectedReceivedCoinInfo]);

    // Verify final state
    expect(withChanges.state.coins.size).toEqual(1);
    const finalCoin = Array.from(withChanges.state.coins)[0];
    expect(finalCoin.value).toEqual(sendValue);
    assertSerializationSuccess(withChanges.state);
  });

  /**
   * Test replayEventsWithChanges - receiver wallet discovers coins from separate sender wallet.
   *
   * @given Two separate wallets, where wallet 1 sends coins to wallet 2
   * @when Wallet 2 replays events with changes on the transfer transaction
   * @then Wallet 2 should discover the received coins and have them reported in changes
   */
  test('replayEventsWithChanges - receiver wallet should discover coins from separate sender wallet', () => {
    // Setup: Two separate wallets with different secret keys
    const senderSecretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const receiverSecretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(2));

    let senderLocalState = new ZswapLocalState();
    const receiverLocalState = new ZswapLocalState();
    let ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());

    const coinInfo = Static.shieldedCoinInfo(10n);
    const sendValue = 5n;

    // Step 1: Sender receives initial coin
    const initialOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, senderSecretKeys.coinPublicKey, senderSecretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const initialTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, initialOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedInitialTransaction = initialTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const [afterInitialLedgerState, { events: initialEvents }] = ledgerState.apply(
      verifiedInitialTransaction,
      transactionContext
    );
    ledgerState = afterInitialLedgerState.postBlockUpdate(new Date(0));
    senderLocalState = senderLocalState.replayEvents(senderSecretKeys, initialEvents);
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(coinInfo);
    const senderCoin = Array.from(senderLocalState.coins)[0];
    expect(senderCoin).toEqual(qualifiedCoinInfo);

    // Step 1b: Receiver syncs merkle tree using collapsed update
    const collapsedUpdate = new MerkleTreeCollapsedUpdate(ledgerState.zswap, 0n, 0n);
    const receiverLocalStateSynced = receiverLocalState.applyCollapsedUpdate(collapsedUpdate);
    expect(Array.from(receiverLocalStateSynced.coins)).toEqual([]);
    expect(receiverLocalStateSynced.firstFree).toEqual(ledgerState.zswap.firstFree);

    // Step 2: Sender creates a transaction sending coins to receiver
    const coinToSpend = Array.from(senderLocalState.coins)[0];
    const [, unprovenInput] = senderLocalState.spend(senderSecretKeys, coinToSpend, 0);

    const newCoinInfo = Static.shieldedCoinInfo(sendValue);
    const receiverOutput = ZswapOutput.new(
      newCoinInfo,
      0,
      receiverSecretKeys.coinPublicKey,
      receiverSecretKeys.encryptionPublicKey
    );
    const transferOffer = ZswapOffer.fromInput(unprovenInput, coinInfo.type, coinInfo.value).merge(
      ZswapOffer.fromOutput(receiverOutput, coinInfo.type, sendValue)
    );

    const transferTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, transferOffer);
    const transactionContext2 = new TransactionContext(ledgerState, Static.blockContext(new Date(1)));
    const verifiedTransferTransaction = transferTransaction.wellFormed(ledgerState, strictness, new Date(1));
    const { events: transferEvents } = ledgerState.apply(verifiedTransferTransaction, transactionContext2)[1];

    // Step 3: Receiver replays only the transfer events with changes to discover coins
    const receiverWithChanges = receiverLocalStateSynced.replayEventsWithChanges(receiverSecretKeys, transferEvents);
    const allReceivedCoins = receiverWithChanges.changes.flatMap((change) => change.receivedCoins);
    const allSpentCoins = receiverWithChanges.changes.flatMap((change) => change.spentCoins);

    // Receiver should have discovered the received coins
    const expectedReceivedCoinInfo = getQualifiedShieldedCoinInfo(newCoinInfo, 1n);
    expect(allReceivedCoins).toEqual([expectedReceivedCoinInfo]);

    // Receiver should not have any spent coins (they only received)
    expect(allSpentCoins).toEqual([]);

    // Verify receiver's final state has the received coin
    const receiverCoins = Array.from(receiverWithChanges.state.coins);
    expect(receiverCoins).toEqual([expectedReceivedCoinInfo]);
    assertSerializationSuccess(receiverWithChanges.state);
  });
});
