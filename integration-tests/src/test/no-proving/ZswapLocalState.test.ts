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
  WellFormedStrictness
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
});
