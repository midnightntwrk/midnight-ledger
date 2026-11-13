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
  type Event,
  LedgerState,
  type ShieldedCoinInfo,
  Transaction,
  TransactionContext,
  type UnprovenOutput,
  WellFormedStrictness,
  ZswapChainState,
  ZswapLocalState,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys
} from '@midnightntwrk/ledger';
import { getQualifiedShieldedCoinInfo, LOCAL_TEST_NETWORK_ID, Static } from '@/test-objects';

/**
 * An output's ciphertext is not bound to its coin commitment by the output proof, so
 * a sender can commit a coin to one recipient while encrypting a coin to another.
 * Wallet replay must only credit decrypted coins that open the event commitment.
 */
describe('Ledger API - ZswapLocalState replay of mismatched output evidence', () => {
  const victimKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
  const attackerKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(2));

  const applyOutputs = (
    ledgerState: LedgerState,
    outputs: [UnprovenOutput, ShieldedCoinInfo][],
    blockTime: Date
  ): [LedgerState, Event[]] => {
    const offer = outputs
      .map(([output, coin]) => ZswapOffer.fromOutput(output, coin.type, coin.value))
      .reduce((acc, next) => acc.merge(next));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verified = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, offer).wellFormed(ledgerState, strictness, blockTime);
    const [nextState, { events }] = ledgerState.apply(
      verified,
      new TransactionContext(ledgerState, Static.blockContext(blockTime))
    );
    return [nextState.postBlockUpdate(blockTime), events];
  };

  /**
   * @given A consensus-valid output that commits a coin to the attacker but encrypts
   *        it to the victim, so the attacker keeps the funds on-chain
   * @when The victim replays the resulting events, followed by a genuine payment
   * @then The phantom coin is not credited and the genuine payment still is
   */
  test('should not credit a decrypted coin that does not open the output commitment', () => {
    const phantomCoin = Static.shieldedCoinInfo(1_000_000n);
    const genuineCoin = Static.shieldedCoinInfo(5n);

    const [, events] = applyOutputs(
      new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState()),
      [
        [ZswapOutput.new(phantomCoin, 0, attackerKeys.coinPublicKey, victimKeys.encryptionPublicKey), phantomCoin],
        [ZswapOutput.new(genuineCoin, 0, victimKeys.coinPublicKey, victimKeys.encryptionPublicKey), genuineCoin]
      ],
      new Date(0)
    );
    expect(events).toHaveLength(2);

    const replayed = new ZswapLocalState().replayEventsWithChanges(victimKeys, events);
    const receivedCoins = replayed.changes.flatMap((change) => change.receivedCoins);

    expect(replayed.state.firstFree).toEqual(2n);
    expect(receivedCoins).toHaveLength(1);
    expect(receivedCoins[0].value).toEqual(genuineCoin.value);
    expect(Array.from(replayed.state.coins).map((coin) => coin.value)).toEqual([genuineCoin.value]);
  });

  /**
   * @given A coin the victim genuinely received, and a later output that re-encrypts
   *        the same coin to the victim under a commitment owned by the attacker
   * @when The victim replays both transactions
   * @then The owned coin keeps its original Merkle tree index and remains the only coin
   */
  test('should not let a re-encrypted coin overwrite an owned coin', () => {
    const ownedCoin = Static.shieldedCoinInfo(10n);

    const [afterGenuine, genuineEvents] = applyOutputs(
      new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState()),
      [[ZswapOutput.new(ownedCoin, 0, victimKeys.coinPublicKey, victimKeys.encryptionPublicKey), ownedCoin]],
      new Date(0)
    );
    const [, replayEvents] = applyOutputs(
      afterGenuine,
      [[ZswapOutput.new(ownedCoin, 0, attackerKeys.coinPublicKey, victimKeys.encryptionPublicKey), ownedCoin]],
      new Date(1)
    );

    const afterGenuineReplay = new ZswapLocalState().replayEvents(victimKeys, genuineEvents);
    expect(Array.from(afterGenuineReplay.coins)).toEqual([getQualifiedShieldedCoinInfo(ownedCoin, 0n)]);

    const replayed = afterGenuineReplay.replayEventsWithChanges(victimKeys, replayEvents);

    expect(replayed.changes.flatMap((change) => change.receivedCoins)).toEqual([]);
    expect(Array.from(replayed.state.coins)).toEqual([getQualifiedShieldedCoinInfo(ownedCoin, 0n)]);
    expect(replayed.state.firstFree).toEqual(2n);
  });
});
