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
  MerkleTreeCollapsedUpdate,
  Transaction,
  ZswapChainState,
  ZswapSecretKeys,
  LedgerState,
  TransactionContext,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';

describe.concurrent('Ledger API - ZswapLocalStateX [@slow][@proving]', () => {
  const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));

  test('apply', async () => {
    const localState = new ZswapLocalState();
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
    const transaction = await prove(unprovenTransaction);
    const localStateAfter = localState.apply(secretKeys, transaction.guaranteedOffer!);

    expect(localStateAfter.toString()).not.toEqual(localState.toString());
    expect(localStateAfter.firstFree).toEqual(1n);
    assertSerializationSuccess(localStateAfter);
  });

  test('apply with proof-erased tx', async () => {
    const localState = new ZswapLocalState();
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
    const transaction = await prove(unprovenTransaction);
    const proofErasedTransaction = transaction.eraseProofs();
    const localStateAfter = localState.apply(secretKeys, proofErasedTransaction.guaranteedOffer!);

    expect(localStateAfter.toString()).not.toEqual(localState.toString());
    expect(localStateAfter.firstFree).toEqual(1n);
    assertSerializationSuccess(localStateAfter);
  });

  // test('applyProofErasedTx', async () => {
  //  const localState = new ZswapLocalState();
  //  const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
  //  const transaction = await prove(unprovenTransaction);
  //  const proofErasedTransaction = transaction.eraseProofs();
  //  const localStateAfter = localState.applyTx(secretKeys, proofErasedTransaction, { type: 'success' });

  //  expect(localStateAfter.toString()).not.toEqual(localState.toString());
  //  expect(localStateAfter.firstFree).toEqual(1n);
  //  assertSerializationSuccess(localStateAfter);
  // });

  // test('applyProofErasedTx - partialSuccess', async () => {
  //  const localState = new ZswapLocalState();
  //  const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
  //  const transaction = await prove(unprovenTransaction);
  //  const proofErasedTransaction = transaction.eraseProofs();
  //  const localStateAfter = localState.applyTx(secretKeys, proofErasedTransaction, {
  //    type: 'partialSuccess',
  //    successfulSegments: new Map([
  //      [0, true],
  //      [1, false]
  //    ])
  //  });

  //  expect(localStateAfter.toString()).not.toEqual(localState.toString());
  //  expect(localStateAfter.firstFree).toEqual(1n);
  //  assertSerializationSuccess(localStateAfter);
  // });

  // test('applyProofErasedTx - failure', async () => {
  //  const localState = new ZswapLocalState();
  //  const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
  //  const transaction = await prove(unprovenTransaction);
  //  const proofErasedTransaction = transaction.eraseProofs();
  //  const localStateAfter = localState.applyTx(secretKeys, proofErasedTransaction, { type: 'failure' });

  //  expect(localStateAfter.toString()).toEqual(localState.toString());
  //  expect(localStateAfter.firstFree).toEqual(0n);
  //  assertSerializationSuccess(localStateAfter);
  // });

  // test('applyFailedProofErased', async () => {
  //  const localState = new ZswapLocalState();
  //  const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
  //  const transaction = await prove(unprovenTransaction);
  //  const proofErasedTransaction = transaction.eraseProofs();
  //  const localStateAfter = localState.applyFailed(proofErasedTransaction.guaranteedOffer!);

  //  expect(localStateAfter.toString()).toEqual(localState.toString());
  //  expect(localStateAfter.firstFree).toEqual(0n);
  //  assertSerializationSuccess(localStateAfter);
  // });

  // test('applyFailed', async () => {
  //  const localState = new ZswapLocalState();
  //  const unprovenTransaction = Transaction.fromParts(
  //    'local-test',
  //    Static.unprovenOfferFromOutput(),
  //    Static.unprovenOfferFromOutput(1)
  //  );
  //  const transaction = await prove(unprovenTransaction);
  //  expect(transaction.fallibleOffer).toBeDefined();
  //  expect(transaction.fallibleOffer!.get(1)).toBeDefined();
  //  const localStateAfter = localState.applyFailed(transaction.fallibleOffer!.get(1)!);

  //  expect(localStateAfter.toString()).toEqual(localState.toString());
  //  expect(localStateAfter.firstFree).toEqual(0n);
  //  assertSerializationSuccess(localStateAfter);
  // });

  test('applyCollapsedUpdate', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const state = new ZswapChainState();

    const stateAfter = state.tryApply(proofErasedTransaction.guaranteedOffer!)[0].postBlockUpdate(new Date(0));
    const localState = new ZswapLocalState();

    const localStateAfter = localState.applyCollapsedUpdate(new MerkleTreeCollapsedUpdate(stateAfter, 0n, 1n));

    expect(localStateAfter.toString()).not.toEqual(localState.toString());
    expect(localStateAfter.firstFree).toEqual(2n);
    assertSerializationSuccess(localStateAfter);
  });

  test('replayEvents', async () => {
    const localState = new ZswapLocalState();
    const ledgerState = new LedgerState('local-test', new ZswapChainState());
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
    const transaction = await prove(unprovenTransaction);
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = transaction.wellFormed(ledgerState, strictness, new Date(0));

    const transactionContext = new TransactionContext(ledgerState, Static.blockContext(new Date(0)));
    const res = ledgerState.apply(verifiedTransaction, transactionContext)[1];
    console.log(res.toString());
    expect(res.type).toEqual('success');
    const { events } = res;
    const localStateAfter = localState.replayEvents(secretKeys, events);

    expect(localStateAfter.toString()).not.toEqual(localState.toString());
    expect(localStateAfter.firstFree).toEqual(1n);
    assertSerializationSuccess(localStateAfter);
  });
});
