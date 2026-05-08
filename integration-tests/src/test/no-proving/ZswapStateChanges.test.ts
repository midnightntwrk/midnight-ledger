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
  LedgerState,
  Transaction,
  TransactionContext,
  WellFormedStrictness,
  ZswapChainState,
  ZswapLocalState,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys,
  ZswapStateChanges
} from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, HEX_64_REGEX, LOCAL_TEST_NETWORK_ID, Static } from '@/test-objects';

describe('Ledger API - ZswapStateChanges', () => {
  const buildLedgerAndEvents = () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const txCtx = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTx = tx.wellFormed(ledgerState, strictness, new Date(0));
    const { events } = ledgerState.apply(verifiedTx, txCtx)[1];
    return { localState, secretKeys, coinInfo, events };
  };

  /**
   * Test construction with empty coin arrays.
   *
   * @given A valid source hex string and empty coin arrays
   * @when Constructing a ZswapStateChanges instance
   * @then Should return an instance with matching source and empty coin arrays
   */
  test('should construct with empty received and spent coin arrays', () => {
    const { localState, secretKeys, events } = buildLedgerAndEvents();
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const [firstChange] = withChanges.changes;
    const { source } = firstChange;

    const changes = new ZswapStateChanges(source, [], []);

    expect(changes.source).toEqual(source);
    expect(changes.receivedCoins).toEqual([]);
    expect(changes.spentCoins).toEqual([]);
  });

  /**
   * Test construction with received coins.
   *
   * @given A valid source hex string and a list of received QualifiedShieldedCoinInfo
   * @when Constructing a ZswapStateChanges instance
   * @then Should return an instance whose receivedCoins getter returns the provided coins
   */
  test('should construct with received coins and expose them via getter', () => {
    const { localState, secretKeys, events, coinInfo } = buildLedgerAndEvents();
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const [{ source }] = withChanges.changes;
    const coin = getQualifiedShieldedCoinInfo(coinInfo, 0n);

    const changes = new ZswapStateChanges(source, [coin], []);

    expect(changes.receivedCoins).toHaveLength(1);
    expect(changes.receivedCoins[0]).toEqual(coin);
    expect(changes.spentCoins).toEqual([]);
  });

  /**
   * Test construction with spent coins.
   *
   * @given A valid source hex string and a list of spent QualifiedShieldedCoinInfo
   * @when Constructing a ZswapStateChanges instance
   * @then Should return an instance whose spentCoins getter returns the provided coins
   */
  test('should construct with spent coins and expose them via getter', () => {
    const { localState, secretKeys, events, coinInfo } = buildLedgerAndEvents();
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const [{ source }] = withChanges.changes;
    const coin = getQualifiedShieldedCoinInfo(coinInfo, 1n);

    const changes = new ZswapStateChanges(source, [], [coin]);

    expect(changes.spentCoins).toHaveLength(1);
    expect(changes.spentCoins[0]).toEqual(coin);
    expect(changes.receivedCoins).toEqual([]);
  });

  /**
   * Test construction with multiple received and spent coins.
   *
   * @given A valid source hex string and multiple coins in each category
   * @when Constructing a ZswapStateChanges instance
   * @then Should preserve all coins in both received and spent arrays
   */
  test('should construct with multiple received and spent coins', () => {
    const { localState, secretKeys, events } = buildLedgerAndEvents();
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const [{ source }] = withChanges.changes;
    const receivedCoins = [
      getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(5n), 0n),
      getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(10n), 1n)
    ];
    const spentCoins = [getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(3n), 2n)];

    const changes = new ZswapStateChanges(source, receivedCoins, spentCoins);

    expect(changes.receivedCoins).toHaveLength(2);
    expect(changes.spentCoins).toHaveLength(1);
    expect(changes.receivedCoins).toEqual(receivedCoins);
    expect(changes.spentCoins).toEqual(spentCoins);
  });

  /**
   * Test round-trip: reconstruct ZswapStateChanges from values obtained via replayEventsWithChanges.
   *
   * @given A ZswapStateChanges obtained by replaying real transaction events
   * @when Reconstructing it using the public constructor with the same source and coins
   * @then The reconstructed instance should have an identical source, receivedCoins, and spentCoins
   */
  test('should round-trip: reconstruct from replayEventsWithChanges values', () => {
    const { localState, secretKeys, events } = buildLedgerAndEvents();
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const [original] = withChanges.changes;

    const reconstructed = new ZswapStateChanges(original.source, [...original.receivedCoins], [...original.spentCoins]);

    expect(reconstructed.source).toEqual(original.source);
    expect(reconstructed.receivedCoins).toEqual(original.receivedCoins);
    expect(reconstructed.spentCoins).toEqual(original.spentCoins);
  });

  /**
   * Test that the source getter returns a valid hex string.
   *
   * @given A ZswapStateChanges obtained by replaying real transaction events
   * @when Reading the source after reconstructing it via the public constructor
   * @then Should return a hex-encoded string matching the expected pattern
   */
  test('should expose source as a hex-encoded string', () => {
    const { localState, secretKeys, events } = buildLedgerAndEvents();
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const [{ source }] = withChanges.changes;

    const changes = new ZswapStateChanges(source, [], []);

    expect(changes.source).toMatch(HEX_64_REGEX);
  });

  /**
   * Test error thrown on invalid source hex string.
   *
   * @given A non-hex source string
   * @when Constructing a ZswapStateChanges instance
   * @then Should throw an error indicating the source is invalid
   */
  test('should throw on invalid source hex string', () => {
    expect(() => new ZswapStateChanges('not-a-hex-string', [], [])).toThrow();
  });

  /**
   * Test error thrown on hex string with the wrong length.
   *
   * @given A hex source string that is too short to represent a TransactionHash
   * @when Constructing a ZswapStateChanges instance
   * @then Should throw an error
   */
  test('should throw on source hex string with wrong length', () => {
    expect(() => new ZswapStateChanges('deadbeef', [], [])).toThrow();
  });

  /**
   * Test error thrown on a malformed coin object.
   *
   * @given A valid source but a coin object with missing required fields
   * @when Constructing a ZswapStateChanges instance
   * @then Should throw an error indicating the coin is invalid
   */
  test('should throw on malformed coin object in receivedCoins', () => {
    const { localState, secretKeys, events } = buildLedgerAndEvents();
    const withChanges = localState.replayEventsWithChanges(secretKeys, events);
    const [{ source }] = withChanges.changes;
    const invalidCoin = { type: 'not-valid-hex', nonce: 'also-invalid', value: 10n, mt_index: 0n };

    expect(() => new ZswapStateChanges(source, [invalidCoin as never], [])).toThrow();
  });
});
