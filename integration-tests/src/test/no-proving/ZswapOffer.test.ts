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
  createShieldedCoinInfo,
  ZswapOffer,
  ZswapOutput,
  ZswapTransient,
  Transaction,
  ZswapLocalState,
  ZswapSecretKeys,
  LedgerState,
  TransactionContext,
  ZswapChainState,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, LOCAL_TEST_NETWORK_ID, Random, Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Ledger API - ZswapOffer', () => {
  test('fromOutput - should create from UnprovenOutput', () => {
    const unprovenOutputGuaranteed = ZswapOutput.new(
      createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
      0,
      Static.coinPublicKey(),
      Static.encryptionPublicKey()
    );
    const unprovenOutputFallible = ZswapOutput.new(
      createShieldedCoinInfo(Random.shieldedTokenType().raw, 10_000n),
      1,
      Static.coinPublicKey(),
      Static.encryptionPublicKey()
    );
    const unprovenOfferGuaranteed = ZswapOffer.fromOutput(
      unprovenOutputGuaranteed,
      Random.shieldedTokenType().raw,
      10_001n
    );
    const unprovenOfferFallible = ZswapOffer.fromOutput(
      unprovenOutputFallible,
      Random.shieldedTokenType().raw,
      10_001n
    );

    expect(unprovenOfferGuaranteed.outputs[0]?.contractAddress).toEqual(unprovenOutputGuaranteed.contractAddress);
    expect(unprovenOfferGuaranteed.outputs[0]?.commitment).toEqual(unprovenOutputGuaranteed.commitment);
    expect(unprovenOfferGuaranteed.inputs).toHaveLength(0);
    expect(unprovenOfferGuaranteed.outputs).toHaveLength(1);
    expect(unprovenOfferGuaranteed.transients).toHaveLength(0);

    expect(unprovenOfferFallible.outputs[0]?.contractAddress).toEqual(unprovenOutputFallible.contractAddress);
    expect(unprovenOfferFallible.outputs[0]?.commitment).toEqual(unprovenOutputFallible.commitment);
    expect(unprovenOfferFallible.inputs).toHaveLength(0);
    expect(unprovenOfferFallible.outputs).toHaveLength(1);
    expect(unprovenOfferFallible.transients).toHaveLength(0);

    const unprovenTransaction = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      unprovenOfferGuaranteed,
      unprovenOfferFallible
    );
    expect(unprovenTransaction.fallibleOffer?.get(1)?.outputs).toHaveLength(1);
    expect(unprovenTransaction.guaranteedOffer?.outputs).toHaveLength(1);
    expect(unprovenTransaction.fallibleOffer?.get(1)?.inputs).toHaveLength(0);
    expect(unprovenTransaction.guaranteedOffer?.inputs).toHaveLength(0);
    expect(unprovenTransaction.fallibleOffer?.get(1)?.transients).toHaveLength(0);
    expect(unprovenTransaction.guaranteedOffer?.transients).toHaveLength(0);
    expect(unprovenTransaction.intents).toBeUndefined();
    assertSerializationSuccess(
      unprovenTransaction,
      SignatureMarker.signature,
      ProofMarker.preProof,
      BindingMarker.preBinding
    );
  });

  test('fromOutput - fails on invalid segment', () => {
    const guaranteedOffer = Static.unprovenOfferFromOutput(0);
    const fallibleOffer = Static.unprovenOfferFromOutput(0);
    expect(() => Transaction.fromParts(LOCAL_TEST_NETWORK_ID, guaranteedOffer, fallibleOffer)).toThrow(
      'Segment ID cannot be 0 in a fallible offer'
    );
  });

  test('fromTransient - should create from UnprovenTransient', () => {
    const value = 10_000n;
    const coinInfo = Static.shieldedCoinInfo(value);
    const unprovenOutput = ZswapOutput.newContractOwned(coinInfo, 0, Static.contractAddress());

    const unprovenOffer = ZswapOffer.fromTransient(
      ZswapTransient.newFromContractOwnedOutput(getQualifiedShieldedCoinInfo(coinInfo), 0, unprovenOutput)
    );

    expect(unprovenOffer.transients[0]?.contractAddress).toEqual(unprovenOutput.contractAddress);
    expect(unprovenOffer.transients[0]?.commitment).toEqual(unprovenOutput.commitment);
    expect(unprovenOffer.inputs).toHaveLength(0);
    expect(unprovenOffer.outputs).toHaveLength(0);
    expect(unprovenOffer.transients).toHaveLength(1);
    assertSerializationSuccess(unprovenOffer, undefined, ProofMarker.preProof);
  });

  test('fromInput - should create from UnprovenInput', () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo(10n);
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
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const [_, unprovenInput] = appliedTxLocalState.spend(secretKeys, qualifiedCoinInfoToSpend, 0);
    const unprovenOffer2 = ZswapOffer.fromInput(unprovenInput, coinInfo.type, coinInfo.value);

    expect(unprovenOffer2.inputs).toHaveLength(1);
    expect(unprovenOffer2.inputs.at(0)?.contractAddress).toBeUndefined();
    expect(unprovenOffer2.inputs.at(0)?.nullifier).toBeDefined();
    expect(unprovenOffer2.deltas.size).toEqual(1);
    expect(unprovenOffer2.outputs).toHaveLength(0);
    assertSerializationSuccess(unprovenOffer2, undefined, ProofMarker.preProof);
  });

  test('send token', () => {
    let localStateAlice = new ZswapLocalState();
    let ledgerState = new LedgerState(LOCAL_TEST_NETWORK_ID, new ZswapChainState());
    const secretKeysAlice = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
    const secretKeysBob = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(2));

    // step 1: preload Alice's account with a shielded coin
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenOffer = ZswapOffer.fromOutput(
      ZswapOutput.new(coinInfo, 0, secretKeysAlice.coinPublicKey, secretKeysAlice.encryptionPublicKey),
      coinInfo.type,
      coinInfo.value
    );
    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, unprovenOffer);
    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;

    expect(ledgerState.zswap.firstFree).toEqual(0n);
    const verifiedTransaction = unprovenTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const [afterTxLegerState, { events }] = ledgerState.apply(verifiedTransaction, transactionContext);
    ledgerState = afterTxLegerState.postBlockUpdate(new Date(0));
    expect(ledgerState.zswap.firstFree).toEqual(1n);

    localStateAlice = localStateAlice.replayEvents(secretKeysAlice, events);
    expect(localStateAlice.coins.size).toEqual(1);
    expect(localStateAlice.pendingSpends.size).toEqual(0);

    // step 2: select a token to send
    const sendValue = 5n;
    const qualifiedCoinInfoToSpend = getQualifiedShieldedCoinInfo(coinInfo, 0n);
    const [updatedLocalState, unprovenInput] = localStateAlice.spend(secretKeysAlice, qualifiedCoinInfoToSpend, 0);
    expect(updatedLocalState.pendingSpends.size).toEqual(1);
    localStateAlice = updatedLocalState;

    // step 3: create a transfer tx
    const unprovenOutput1 = ZswapOutput.new(
      Static.shieldedCoinInfo(sendValue),
      0,
      secretKeysBob.coinPublicKey,
      secretKeysBob.encryptionPublicKey
    );
    // return the change back to Alice
    const unprovenOutput2 = ZswapOutput.new(
      Static.shieldedCoinInfo(sendValue),
      0,
      secretKeysAlice.coinPublicKey,
      secretKeysAlice.encryptionPublicKey
    );
    const sendOffer = ZswapOffer.fromInput(unprovenInput, coinInfo.type, coinInfo.value)
      .merge(ZswapOffer.fromOutput(unprovenOutput1, coinInfo.type, sendValue))
      .merge(ZswapOffer.fromOutput(unprovenOutput2, coinInfo.type, sendValue));

    const transferTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, sendOffer);
    const transactionContext2 = new TransactionContext(ledgerState, Static.blockContext(new Date(1)));
    const verifiedTransaction2 = transferTransaction.wellFormed(ledgerState, strictness, new Date(1));

    // step 4: apply tx
    const [afterTx2LedgerState, { events: events2 }] = ledgerState.apply(verifiedTransaction2, transactionContext2);
    ledgerState = afterTx2LedgerState.postBlockUpdate(new Date(1));
    expect(ledgerState.zswap.firstFree).toEqual(3n);

    // step 5: replay events and check the state
    localStateAlice = localStateAlice.replayEvents(secretKeysAlice, events2);
    expect(localStateAlice.coins.size).toEqual(1);
    const newCoin = [...localStateAlice.coins.values()][0];
    expect(newCoin.value).toEqual(sendValue);
  });

  test('merge - cannot merge to itself', () => {
    const tokenType = Random.shieldedTokenType();
    const unprovenOffer1 = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(tokenType.raw, 10_000n),
        0,
        Static.coinPublicKey(),
        Static.encryptionPublicKey()
      ),
      tokenType.raw,
      1n
    );

    expect(() => unprovenOffer1.merge(unprovenOffer1)).toThrow('attempted to merge non-disjoint coin sets');
  });

  test('merge - two offers with same token type', () => {
    const tokenType = Random.shieldedTokenType();
    const unprovenOffer1 = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(tokenType.raw, 10_000n),
        0,
        Static.coinPublicKey(),
        Static.encryptionPublicKey()
      ),
      tokenType.raw,
      1n
    );
    const unprovenOffer2 = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(tokenType.raw, 10_000n),
        0,
        Static.coinPublicKey(),
        Static.encryptionPublicKey()
      ),
      tokenType.raw,
      2n
    );
    const mergedOffer = unprovenOffer1.merge(unprovenOffer2);

    expect(mergedOffer.inputs).toHaveLength(0);
    expect(mergedOffer.outputs).toHaveLength(2);
    expect(mergedOffer.deltas.size).toEqual(1);
    expect(mergedOffer.deltas.get(tokenType.raw)).toEqual(-3n);
    assertSerializationSuccess(mergedOffer, undefined, ProofMarker.preProof);
  });

  test('merge - two offers with different token type', () => {
    const tokenType = Random.shieldedTokenType();
    const tokenType2 = Random.shieldedTokenType();
    const unprovenOffer1 = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(tokenType.raw, 10_000n),
        0,
        Static.coinPublicKey(),
        Static.encryptionPublicKey()
      ),
      tokenType.raw,
      1n
    );
    const unprovenOffer2 = ZswapOffer.fromOutput(
      ZswapOutput.new(
        createShieldedCoinInfo(tokenType2.raw, 10_000n),
        0,
        Static.coinPublicKey(),
        Static.encryptionPublicKey()
      ),
      tokenType2.raw,
      2n
    );
    const mergedOffer = unprovenOffer1.merge(unprovenOffer2);

    expect(mergedOffer.inputs).toHaveLength(0);
    expect(mergedOffer.outputs).toHaveLength(2);
    expect(mergedOffer.deltas.size).toEqual(2);
    expect(mergedOffer.deltas.get(tokenType.raw)).toEqual(-1n);
    expect(mergedOffer.deltas.get(tokenType2.raw)).toEqual(-2n);
    assertSerializationSuccess(mergedOffer, undefined, ProofMarker.preProof);
  });

  test('fromOutput - should create from UnprovenOutput with contract address', () => {
    const coinInfo = Static.shieldedCoinInfo(10_000n);
    const unprovenOutput = ZswapOutput.newContractOwned(coinInfo, 0, Static.contractAddress());
    const unprovenOffer = ZswapOffer.fromOutput(unprovenOutput, Random.shieldedTokenType().raw, 10_001n);

    expect(unprovenOffer.outputs[0]?.contractAddress).toEqual(unprovenOutput.contractAddress);
    expect(unprovenOffer.outputs[0]?.commitment).toEqual(unprovenOutput.commitment);
    expect(unprovenOffer.inputs).toHaveLength(0);
    expect(unprovenOffer.outputs).toHaveLength(1);
    expect(unprovenOffer.transients).toHaveLength(0);
    assertSerializationSuccess(unprovenOffer, undefined, ProofMarker.preProof);
  });

  test('fromTransient - should create from UnprovenTransient with contract address', () => {
    const value = 10_000n;
    const coinInfo = Static.shieldedCoinInfo(value);
    const unprovenOutput = ZswapOutput.newContractOwned(coinInfo, 0, Static.contractAddress());

    const unprovenOffer = ZswapOffer.fromTransient(
      ZswapTransient.newFromContractOwnedOutput(getQualifiedShieldedCoinInfo(coinInfo), 0, unprovenOutput)
    );

    expect(unprovenOffer.transients[0]?.contractAddress).toEqual(unprovenOutput.contractAddress);
    expect(unprovenOffer.transients[0]?.commitment).toEqual(unprovenOutput.commitment);
    expect(unprovenOffer.inputs).toHaveLength(0);
    expect(unprovenOffer.outputs).toHaveLength(0);
    expect(unprovenOffer.transients).toHaveLength(1);
    assertSerializationSuccess(unprovenOffer, undefined, ProofMarker.preProof);
  });

  test('merge - two offers with same token type and contract address', () => {
    const value = 10_000n;
    const coinInfo = Static.shieldedCoinInfo(value);
    const contractAddress = Static.contractAddress();
    const unprovenOffer1 = ZswapOffer.fromOutput(
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress),
      coinInfo.type,
      1n
    );
    const unprovenOffer2 = ZswapOffer.fromOutput(
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress),
      coinInfo.type,
      2n
    );
    const mergedOffer = unprovenOffer1.merge(unprovenOffer2);

    expect(mergedOffer.inputs).toHaveLength(0);
    expect(mergedOffer.outputs).toHaveLength(2);
    expect(mergedOffer.deltas.size).toEqual(1);
    expect(mergedOffer.deltas.get(coinInfo.type)).toEqual(-3n);
    expect(mergedOffer.outputs[0]?.contractAddress).toEqual(contractAddress);
    expect(mergedOffer.outputs[1]?.contractAddress).toEqual(contractAddress);
    assertSerializationSuccess(mergedOffer, undefined, ProofMarker.preProof);
  });

  test('merge - two offers with different token type and contract address', () => {
    const value = 10_000n;
    const coinInfo = Static.shieldedCoinInfo(value);
    const tokenType2 = Random.shieldedTokenType();
    const coinInfo2 = createShieldedCoinInfo(tokenType2.raw, value);
    const contractAddress = Static.contractAddress();
    const unprovenOffer1 = ZswapOffer.fromOutput(
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress),
      coinInfo.type,
      1n
    );
    const unprovenOffer2 = ZswapOffer.fromOutput(
      ZswapOutput.newContractOwned(coinInfo2, 0, contractAddress),
      coinInfo2.type,
      2n
    );
    const mergedOffer = unprovenOffer1.merge(unprovenOffer2);

    expect(mergedOffer.inputs).toHaveLength(0);
    expect(mergedOffer.outputs).toHaveLength(2);
    expect(mergedOffer.deltas.size).toEqual(2);
    expect(mergedOffer.deltas.get(coinInfo.type)).toEqual(-1n);
    expect(mergedOffer.deltas.get(coinInfo2.type)).toEqual(-2n);
    expect(mergedOffer.outputs[0]?.contractAddress).toEqual(contractAddress);
    expect(mergedOffer.outputs[1]?.contractAddress).toEqual(contractAddress);
    assertSerializationSuccess(mergedOffer, undefined, ProofMarker.preProof);
  });

  test('merge - two offers with different segment ids', () => {
    const value = 10_000n;
    const coinInfo = Static.shieldedCoinInfo(value);
    const tokenType2 = Random.shieldedTokenType();
    const coinInfo2 = createShieldedCoinInfo(tokenType2.raw, value);
    const contractAddress = Static.contractAddress();
    const unprovenOffer1 = ZswapOffer.fromOutput(
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress),
      coinInfo.type,
      1n
    );
    const unprovenOffer2 = ZswapOffer.fromOutput(
      ZswapOutput.newContractOwned(coinInfo2, 1, contractAddress),
      coinInfo2.type,
      2n
    );
    expect(() => unprovenOffer1.merge(unprovenOffer2)).toThrow('Mismatched output segments.');
  });

  test('serialize and deserialize', () => {
    const value = 10_000n;
    const coinInfo = Static.shieldedCoinInfo(value);
    const unprovenOutput = ZswapOutput.new(coinInfo, 0, Static.coinPublicKey(), Static.encryptionPublicKey());
    const unprovenOffer = ZswapOffer.fromOutput(unprovenOutput, Random.shieldedTokenType().raw, 10_001n);

    expect(ZswapOffer.deserialize('pre-proof', unprovenOffer.serialize()).toString()).toEqual(unprovenOffer.toString());
  });
});
