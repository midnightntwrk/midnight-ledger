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
  coinNullifier,
  createShieldedCoinInfo,
  type Event,
  LedgerState,
  MerkleTreeCollapsedUpdate,
  Transaction,
  TransactionContext,
  WellFormedStrictness,
  ZswapChainState,
  ZswapLocalState,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys
} from '@midnight-ntwrk/ledger';
import { TestState } from '@/test/utils/TestState';
import { LOCAL_TEST_NETWORK_ID, type ShieldedTokenType, Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Confirmed bugs', () => {
  test('replayRawEvents drops every event after the first when given concatenated bytes', () => {
    const sk = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const ledger = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;

    const coin1 = createShieldedCoinInfo(Static.defaultShieldedTokenType().raw, 10n);
    const coin2 = createShieldedCoinInfo(Static.defaultShieldedTokenType().raw, 20n);
    const offer = ZswapOffer.fromOutput(
      ZswapOutput.new(coin1, 0, sk.coinPublicKey, sk.encryptionPublicKey),
      coin1.type,
      coin1.value
    ).merge(
      ZswapOffer.fromOutput(
        ZswapOutput.new(coin2, 0, sk.coinPublicKey, sk.encryptionPublicKey),
        coin2.type,
        coin2.value
      )
    );

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer);
    const ctx = new TransactionContext(ledger, Static.blockContext(new Date(0)));
    const vtx = tx.wellFormed(ledger, strictness, new Date(0));
    const [, result] = ledger.apply(vtx, ctx);

    expect(result.events.length).toEqual(2);

    const parts = result.events.map((e: Event) => e.serialize());
    const totalLength = parts.reduce((acc: number, p: Uint8Array) => acc + p.length, 0);
    const rawBytes = new Uint8Array(totalLength);
    let offset = 0;
    parts.forEach((part: Uint8Array) => {
      rawBytes.set(part, offset);
      offset += part.length;
    });

    const raw = new ZswapLocalState().replayRawEvents(sk, rawBytes);

    expect(raw.state.coins.size).toEqual(2);
    expect(raw.state.firstFree).toEqual(2n);
  });
});

describe('Wallet Control - Finer ZswapLocalState control', () => {
  function setupWithCoin(value: bigint = 10n) {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    let ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const coinInfo = Static.shieldedCoinInfo(value);
    const offer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer);
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const ctx = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const vtx = tx.wellFormed(ledgerState, strictness, new Date(0));
    const [newLedger, result] = ledgerState.apply(vtx, ctx);
    ledgerState = newLedger.postBlockUpdate(new Date(0));
    return { secretKeys, ledgerState, coinInfo, events: result.events };
  }

  function serializeEvents(events: Event[]): Uint8Array {
    const parts = events.map((e) => e.serialize());
    const totalLength = parts.reduce((acc, p) => acc + p.length, 0);
    const result = new Uint8Array(totalLength);
    let currentOffset = 0;
    parts.forEach((part) => {
      result.set(part, currentOffset);
      currentOffset += part.length;
    });
    return result;
  }

  test('merkleTreeRoot - blank tree has a defined bigint root', () => {
    const localState = new ZswapLocalState();
    expect(localState.merkleTreeRoot).toBeDefined();
    expect(typeof localState.merkleTreeRoot).toBe('bigint');
  });

  test('merkleTreeRoot - changes after each new coin', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    let ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    let localState = new ZswapLocalState();

    const coin1 = Static.shieldedCoinInfo(100n);
    const offer1 = ZswapOffer.fromOutput(
      ZswapOutput.new(coin1, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coin1.type,
      coin1.value
    );
    const tx1 = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer1);
    const ctx1 = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const vtx1 = tx1.wellFormed(ledgerState, strictness, new Date(0));
    const [ledger2, result1] = ledgerState.apply(vtx1, ctx1);
    localState = localState.replayEvents(secretKeys, result1.events);
    const root1 = localState.merkleTreeRoot;

    ledgerState = ledger2.postBlockUpdate(new Date(0));
    const coin2 = Static.shieldedCoinInfo(200n);
    const offer2 = ZswapOffer.fromOutput(
      ZswapOutput.new(coin2, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coin2.type,
      coin2.value
    );
    const tx2 = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer2);
    const ctx2 = new TransactionContext(ledgerState, Static.blockContext(new Date(1)));
    const vtx2 = tx2.wellFormed(ledgerState, strictness, new Date(1));
    const { events: events2 } = ledgerState.apply(vtx2, ctx2)[1];
    localState = localState.replayEvents(secretKeys, events2);
    const root2 = localState.merkleTreeRoot;

    expect(root1).toBeDefined();
    expect(root2).toBeDefined();
    expect(root1).not.toEqual(root2);
  });

  test('insertCoin - adds coin to wallet, advances firstFree, serializes', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const localState = new ZswapLocalState();

    const coinInfo = Static.shieldedCoinInfo(42n);
    const updated = localState.insertCoin(secretKeys, coinInfo);

    expect(updated.coins.size).toEqual(1);
    expect(updated.firstFree).toEqual(1n);
    expect(updated.merkleTreeRoot).toBeDefined();
    assertSerializationSuccess(updated);
  });

  test('insertCoin - assigns mt_index = firstFree at the moment of insertion', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    let localState = new ZswapLocalState();

    localState = localState.insertCoin(secretKeys, Static.shieldedCoinInfo(10n));
    localState = localState.insertCoin(secretKeys, Static.shieldedCoinInfo(20n));

    expect(localState.coins.size).toEqual(2);
    expect(localState.firstFree).toEqual(2n);

    const storedIndices = Array.from(localState.coins)
      .map((c) => c.mt_index)
      .sort();
    expect(storedIndices).toEqual([0n, 1n]);
  });

  test('insertCoin - inserted coin is immediately spendable', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const localState = new ZswapLocalState();

    const coinInfo = Static.shieldedCoinInfo(42n);
    const withCoin = localState.insertCoin(secretKeys, coinInfo);
    const storedCoin = Array.from(withCoin.coins)[0];
    expect(storedCoin.mt_index).toEqual(0n);

    const [spentState, spendInput] = withCoin.spend(secretKeys, storedCoin, 0);
    expect(spentState.pendingSpends.size).toEqual(1);
    expect(spendInput.nullifier).toBeDefined();
  });

  test('removeCoinByNullifier - removes the coin from tracking; firstFree stays advanced because the tree slot is permanent', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(42n);

    const withCoin = new ZswapLocalState().insertCoin(secretKeys, coinInfo);
    const nullifier = coinNullifier(coinInfo, secretKeys.coinSecretKey);
    const withoutCoin = withCoin.removeCoinByNullifier(nullifier);

    expect(withoutCoin.coins.size).toEqual(0);
    expect(withoutCoin.firstFree).toEqual(1n);
    assertSerializationSuccess(withoutCoin);
  });

  test('removeCoinByNullifier - is a no-op for unknown nullifier', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const withCoin = new ZswapLocalState().insertCoin(secretKeys, Static.shieldedCoinInfo(42n));

    const afterRemove = withCoin.removeCoinByNullifier('0'.repeat(64));

    expect(afterRemove.coins.size).toEqual(1);
  });

  test('removeCoinByNullifier - leaves other coins untouched', () => {
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coin1 = Static.shieldedCoinInfo(10n);
    const coin2 = Static.shieldedCoinInfo(20n);

    let localState = new ZswapLocalState().insertCoin(secretKeys, coin1).insertCoin(secretKeys, coin2);

    localState = localState.removeCoinByNullifier(coinNullifier(coin1, secretKeys.coinSecretKey));

    expect(localState.coins.size).toEqual(1);
    expect(Array.from(localState.coins)[0].value).toEqual(20n);
  });

  test('removeCoinByNullifier - also clears matching pending spend', () => {
    const { secretKeys, events } = setupWithCoin(100n);
    const localState = new ZswapLocalState().replayEvents(secretKeys, events);

    const coin = Array.from(localState.coins)[0];
    const [stateWithPending] = localState.spend(secretKeys, coin, 0);
    expect(stateWithPending.pendingSpends.size).toEqual(1);

    const nullifier = coinNullifier(
      { type: coin.type, nonce: coin.nonce, value: coin.value },
      secretKeys.coinSecretKey
    );
    const afterRemove = stateWithPending.removeCoinByNullifier(nullifier);

    expect(afterRemove.pendingSpends.size).toEqual(0);
  });

  test('replayRawEvents - produces same state as replayEventsWithChanges', () => {
    const { secretKeys, events } = setupWithCoin(50n);
    const localState = new ZswapLocalState();

    const rawBytes = serializeEvents(events);

    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const rawResult = localState.replayRawEvents(secretKeys, rawBytes);

    expect(rawResult.state.firstFree).toEqual(withChanges.state.firstFree);
    expect(rawResult.state.coins.size).toEqual(withChanges.state.coins.size);
    expect(rawResult.state.merkleTreeRoot).toEqual(withChanges.state.merkleTreeRoot);

    const normalReceived = withChanges.changes.flatMap((c) => c.receivedCoins);
    const rawReceived = rawResult.changes.flatMap((c) => c.receivedCoins);
    expect(rawReceived).toEqual(normalReceived);
  });

  test('replayRawEvents - empty raw bytes return unchanged state', () => {
    const { secretKeys, events } = setupWithCoin(100n);
    const localState = new ZswapLocalState().replayEvents(secretKeys, events);

    const emptyResult = localState.replayRawEvents(secretKeys, new Uint8Array(0));

    expect(emptyResult.state.firstFree).toEqual(localState.firstFree);
    expect(emptyResult.state.coins.size).toEqual(localState.coins.size);
    expect(emptyResult.changes.length).toEqual(0);
  });

  test('applyWithChanges - output-only offer matches apply and reports received coin', () => {
    const { secretKeys, events } = setupWithCoin(100n);
    const localState = new ZswapLocalState().replayEvents(secretKeys, events);

    const newCoin = Static.shieldedCoinInfo(50n);
    const offer = ZswapOffer.fromOutput(
      ZswapOutput.new(newCoin, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      newCoin.type,
      newCoin.value
    );

    const afterApply = localState.apply(secretKeys, offer);
    const withChanges = localState.applyWithChanges(secretKeys, offer);

    expect(withChanges.state.firstFree).toEqual(afterApply.firstFree);
    expect(withChanges.state.coins.size).toEqual(afterApply.coins.size);
    expect(withChanges.state.merkleTreeRoot).toEqual(afterApply.merkleTreeRoot);

    const receivedCoins = withChanges.changes.flatMap((c) => c.receivedCoins);
    expect(receivedCoins.length).toEqual(1);
    expect(receivedCoins[0].value).toEqual(50n);
  });

  test('applyWithChanges - transfer reports both spent and received coins', () => {
    const { secretKeys, events } = setupWithCoin(100n);
    const localState = new ZswapLocalState().replayEvents(secretKeys, events);

    const coinToSpend = Array.from(localState.coins)[0];
    const [stateWithPending, input] = localState.spend(secretKeys, coinToSpend, 0);

    const changeCoin = Static.shieldedCoinInfo(30n);
    const changeOutput = ZswapOutput.new(changeCoin, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey);
    const transferOffer = ZswapOffer.fromInput(input, coinToSpend.type, coinToSpend.value).merge(
      ZswapOffer.fromOutput(changeOutput, coinToSpend.type, 30n)
    );

    const withChanges = stateWithPending.applyWithChanges(secretKeys, transferOffer);

    const receivedCoins = withChanges.changes.flatMap((c) => c.receivedCoins);
    const spentCoins = withChanges.changes.flatMap((c) => c.spentCoins);

    expect(spentCoins.length).toEqual(1);
    expect(spentCoins[0].value).toEqual(coinToSpend.value);

    expect(receivedCoins.length).toEqual(1);
    expect(receivedCoins[0].value).toEqual(30n);
  });

  test('applyWithChanges - input-only offer matches apply', () => {
    const { secretKeys, events } = setupWithCoin(100n);
    const localState = new ZswapLocalState().replayEvents(secretKeys, events);

    const coin = Array.from(localState.coins)[0];
    const [stateWithPending, input] = localState.spend(secretKeys, coin, 0);
    const spendOnlyOffer = ZswapOffer.fromInput(input, coin.type, coin.value);

    const afterApply = stateWithPending.apply(secretKeys, spendOnlyOffer);
    const afterApplyWithChanges = stateWithPending.applyWithChanges(secretKeys, spendOnlyOffer);

    expect(afterApplyWithChanges.state.coins.size).toEqual(afterApply.coins.size);
    expect(afterApplyWithChanges.state.firstFree).toEqual(afterApply.firstFree);
    expect(afterApplyWithChanges.state.pendingSpends.size).toEqual(afterApply.pendingSpends.size);
  });

  test('applyWithChanges - transient (input + output of same coin) matches apply', () => {
    const { secretKeys, events } = setupWithCoin(100n);
    const localState = new ZswapLocalState().replayEvents(secretKeys, events);

    const coin = Array.from(localState.coins)[0];
    const [stateWithPending, transient] = localState.spendFromOutput(
      secretKeys,
      coin,
      0,
      ZswapOutput.new(Static.shieldedCoinInfo(50n), 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey)
    );

    const newOutput = ZswapOutput.new(
      Static.shieldedCoinInfo(30n),
      0,
      secretKeys.coinPublicKey,
      secretKeys.encryptionPublicKey
    );
    const offer = ZswapOffer.fromTransient(transient).merge(ZswapOffer.fromOutput(newOutput, coin.type, 30n));

    const afterApply = stateWithPending.apply(secretKeys, offer);
    const afterApplyWithChanges = stateWithPending.applyWithChanges(secretKeys, offer);

    expect(afterApplyWithChanges.state.coins.size).toEqual(afterApply.coins.size);
    expect(afterApplyWithChanges.state.firstFree).toEqual(afterApply.firstFree);
    expect(afterApplyWithChanges.state.merkleTreeRoot).toEqual(afterApply.merkleTreeRoot);
  });

  test('applyWithChanges - mixed 1 input + 2 outputs assigns sequential mt_indices', () => {
    const { secretKeys, events } = setupWithCoin(100n);
    const localState = new ZswapLocalState().replayEvents(secretKeys, events);

    const coin = Array.from(localState.coins)[0];
    const [stateWithPending, input] = localState.spend(secretKeys, coin, 0);

    const out1 = ZswapOutput.new(
      createShieldedCoinInfo(coin.type, 40n),
      0,
      secretKeys.coinPublicKey,
      secretKeys.encryptionPublicKey
    );
    const out2 = ZswapOutput.new(
      createShieldedCoinInfo(coin.type, 60n),
      0,
      secretKeys.coinPublicKey,
      secretKeys.encryptionPublicKey
    );
    const offer = ZswapOffer.fromInput(input, coin.type, coin.value)
      .merge(ZswapOffer.fromOutput(out1, coin.type, 40n))
      .merge(ZswapOffer.fromOutput(out2, coin.type, 60n));

    const afterApply = stateWithPending.apply(secretKeys, offer);
    const afterApplyWithChanges = stateWithPending.applyWithChanges(secretKeys, offer);

    expect(afterApplyWithChanges.state.coins.size).toEqual(afterApply.coins.size);
    expect(afterApplyWithChanges.state.firstFree).toEqual(afterApply.firstFree);
    expect(afterApplyWithChanges.state.merkleTreeRoot).toEqual(afterApply.merkleTreeRoot);

    const receivedCoins = afterApplyWithChanges.changes.flatMap((c) => c.receivedCoins);
    expect(receivedCoins.length).toEqual(2);
    const mtIndices = receivedCoins.map((c) => c.mt_index).sort();
    expect(mtIndices[1] - mtIndices[0]).toEqual(1n);
  });

  test('Event.source - exposes transaction metadata', () => {
    const { events } = setupWithCoin();

    expect(events.length).toBeGreaterThan(0);

    const { source } = events[0];

    expect(source).toBeDefined();
    expect(source.transactionHash).toBeDefined();
    expect(typeof source.transactionHash).toBe('string');
    expect(typeof source.logicalSegment).toBe('number');
    expect(typeof source.physicalSegment).toBe('number');
  });

  test('Event.source - all events from the same transaction share transactionHash', () => {
    const { events } = setupWithCoin();

    if (events.length > 1) {
      const { transactionHash } = events[0].source;
      events.forEach((event) => {
        expect(event.source.transactionHash).toEqual(transactionHash);
      });
    }
  });

  test('Event.content - exposes a tagged union (zswapInput / zswapOutput / etc.)', () => {
    const { events } = setupWithCoin();

    expect(events.length).toBeGreaterThan(0);
    events.forEach((event) => {
      const { content } = event as Event & { content: { tag: string } };
      expect(content).toBeDefined();
      expect(typeof content.tag).toBe('string');
    });

    const tags = events.map((e) => (e as Event & { content: { tag: string } }).content.tag);
    expect(tags).toContain('zswapOutput');
  });

  test('Event - serialize and deserialize round-trip', () => {
    const { events } = setupWithCoin();

    expect(events.length).toBeGreaterThan(0);
    events.forEach((event) => {
      const serialized = event.serialize();
      expect(serialized).toBeInstanceOf(Uint8Array);
      expect(serialized.length).toBeGreaterThan(0);

      const EventClass = event.constructor as unknown as { deserialize(raw: Uint8Array): Event };
      const deserialized = EventClass.deserialize(serialized);

      expect(deserialized.source).toBeDefined();
      expect(deserialized.source.transactionHash).toEqual(event.source.transactionHash);
      expect(deserialized.source.logicalSegment).toEqual(event.source.logicalSegment);
      expect(deserialized.source.physicalSegment).toEqual(event.source.physicalSegment);
    });
  });

  test('realistic scenario - rewardsShielded + insertCoin + removeCoinByNullifier on a TestState wallet', () => {
    const state = TestState.new();
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    const firstReward = 100_000n;
    state.rewardsShielded(token, firstReward);

    const rootAfterFirstReward = state.zswap.merkleTreeRoot;
    expect(rootAfterFirstReward).toBeDefined();
    expect(state.zswap.coins.size).toBeGreaterThan(0);

    const secondReward = 250_000n;
    state.rewardsShielded(token, secondReward);

    expect(state.zswap.merkleTreeRoot).not.toEqual(rootAfterFirstReward);

    const totalFunds = Array.from(state.zswap.coins)
      .filter((c) => c.type === token.raw)
      .reduce((acc, c) => acc + c.value, 0n);
    expect(totalFunds).toBeGreaterThanOrEqual(firstReward + secondReward);

    const manualCoin = createShieldedCoinInfo(token.raw, 999n);
    const coinsBeforeInsert = state.zswap.coins.size;
    const firstFreeBeforeInsert = state.zswap.firstFree;
    const stateAfterInsert = state.zswap.insertCoin(state.zswapKeys, manualCoin);

    expect(stateAfterInsert.coins.size).toEqual(coinsBeforeInsert + 1);
    expect(stateAfterInsert.firstFree).toEqual(firstFreeBeforeInsert + 1n);

    const nullifier = coinNullifier(manualCoin, state.zswapKeys.coinSecretKey);
    const stateAfterRemove = stateAfterInsert.removeCoinByNullifier(nullifier);

    expect(stateAfterRemove.coins.size).toEqual(coinsBeforeInsert);
    expect(stateAfterRemove.firstFree).toEqual(stateAfterInsert.firstFree);

    assertSerializationSuccess(stateAfterRemove);
  });

  test('realistic scenario - sender/receiver transfer with collapsed update and event replay', () => {
    const senderKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const receiverKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(2));

    let ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    let senderState = new ZswapLocalState();
    let receiverState = new ZswapLocalState();
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;

    const initialCoin = Static.shieldedCoinInfo(1000n);
    const initialOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(initialCoin, 0, senderKeys.coinPublicKey, senderKeys.encryptionPublicKey),
      initialCoin.type,
      initialCoin.value
    );
    const tx1 = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, initialOffer);
    const ctx1 = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const vtx1 = tx1.wellFormed(ledgerState, strictness, new Date(0));
    const [ledger2, result1] = ledgerState.apply(vtx1, ctx1);
    ledgerState = ledger2.postBlockUpdate(new Date(0));
    senderState = senderState.replayEvents(senderKeys, result1.events);

    const collapsedUpdate = new MerkleTreeCollapsedUpdate(ledgerState.zswap, 0n, 0n);
    receiverState = receiverState.applyCollapsedUpdate(collapsedUpdate);

    const senderCoin = Array.from(senderState.coins)[0];
    const [, input] = senderState.spend(senderKeys, senderCoin, 0);

    const transferValue = 300n;
    const transferCoin = Static.shieldedCoinInfo(transferValue);
    const receiverOutput = ZswapOutput.new(
      transferCoin,
      0,
      receiverKeys.coinPublicKey,
      receiverKeys.encryptionPublicKey
    );
    const changeCoin = Static.shieldedCoinInfo(senderCoin.value - transferValue);
    const changeOutput = ZswapOutput.new(changeCoin, 0, senderKeys.coinPublicKey, senderKeys.encryptionPublicKey);

    const transferOffer = ZswapOffer.fromInput(input, senderCoin.type, senderCoin.value)
      .merge(ZswapOffer.fromOutput(receiverOutput, senderCoin.type, transferValue))
      .merge(ZswapOffer.fromOutput(changeOutput, senderCoin.type, senderCoin.value - transferValue));

    const tx2 = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, transferOffer);
    const ctx2 = new TransactionContext(ledgerState, Static.blockContext(new Date(1)));
    const vtx2 = tx2.wellFormed(ledgerState, strictness, new Date(1));
    const { events: transferEvents } = ledgerState.apply(vtx2, ctx2)[1];

    expect(transferEvents.length).toBeGreaterThan(0);
    const { source } = transferEvents[0];
    expect(source.transactionHash).toBeDefined();
    expect(source.logicalSegment).toEqual(0);

    const receiverResult = receiverState.replayEventsWithChanges(receiverKeys, transferEvents);

    const receivedCoins = receiverResult.changes.flatMap((c) => c.receivedCoins);
    expect(receivedCoins.length).toEqual(1);
    expect(receivedCoins[0].value).toEqual(transferValue);

    const receiverCoin = Array.from(receiverResult.state.coins)[0];
    expect(receiverCoin.value).toEqual(transferValue);
    expect(receiverCoin.mt_index).toBeDefined();
  });
});
