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
  type BlockContext,
  DustActions,
  Intent,
  LedgerState,
  type PreBinding,
  type Proof,
  type SignatureEnabled,
  Transaction,
  TransactionContext,
  WellFormedStrictness,
  ZswapChainState
} from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { INITIAL_NIGHT_AMOUNT, LOCAL_TEST_NETWORK_ID, Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import { TestState } from '@/test/utils/TestState';

describe.concurrent('Ledger API - LedgerStateX [@slow][@proving]', () => {
  /**
   * Test ledger state remains unchanged when transaction application fails.
   *
   * @given A proven transaction with faerie-gold attempt
   * @when Applying transaction to ledger state
   * @then Should fail with faerie-gold error and leave ledger state unchanged
   */
  test('should leave ledger state unchanged when apply fails with faerie-gold', async () => {
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      Static.unprovenOfferFromOutput(),
      Static.unprovenOfferFromOutput(1)
    );
    const transaction = await prove(unprovenTransaction);
    const proofErasedTransaction = transaction.eraseProofs();
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const transactionContext = new TransactionContext(ledgerState, {
      secondsSinceEpoch: Static.blockTime(new Date()),
      secondsSinceEpochErr: 1_000_000,
      parentBlockHash: Static.parentBlockHash(),
      lastBlockTime: Static.blockTime(new Date()) - 6n
    } as BlockContext);

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const [ledgerStateAfter, transactionResult] = ledgerState.apply(verifiedTransaction, transactionContext);

    expect(transactionResult.type).toEqual('failure');
    expect(transactionResult.error).toMatch(/faerie-gold attempt with commitment Commitment\(.*\)/);
    expect(ledgerStateAfter.toString()).toEqual(ledgerState.toString());
    assertSerializationSuccess(ledgerStateAfter);
  });

  /**
   * Test ledger state updates correctly for successful transaction application.
   *
   * @given A proven transaction with guaranteed offer only
   * @when Applying transaction to ledger state
   * @then Should succeed and update ledger state with new zswap first free value
   */
  test('should update ledger state when transaction has guaranteed offer only', async () => {
    const unprovenTransaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
    const transaction = await prove(unprovenTransaction);
    const proofErasedTransaction = transaction.eraseProofs();
    const zswapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zswapChainState);
    const transactionContext = new TransactionContext(ledgerState, {
      secondsSinceEpoch: Static.blockTime(new Date(0)),
      secondsSinceEpochErr: 1_000_000,
      parentBlockHash: Static.parentBlockHash(),
      lastBlockTime: Static.blockTime(new Date(0))
    } as BlockContext);

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = proofErasedTransaction.wellFormed(ledgerState, strictness, new Date(0));
    const [ledgerStateAfter, transactionResult] = ledgerState.apply(verifiedTransaction, transactionContext);

    expect(transactionResult.type).toEqual('success');
    expect(transactionResult.error).toBeUndefined();
    expect(transactionResult.toString()).toMatch(/Success/);
    expect(ledgerStateAfter.toString()).not.toEqual(ledgerState.toString());
    expect(ledgerStateAfter.zswap.toString()).not.toEqual(ledgerState.zswap.toString());
    expect(ledgerStateAfter.zswap.firstFree).toEqual(1n);
    assertSerializationSuccess(ledgerStateAfter);
  });

  /**
   * Test that application charges the fees computed during verification.
   *
   * A wallet holding the proven transaction balances it against its real
   * fees. Application must charge the verified fees, not
   * re-estimate them from the proof-erased transaction, or exactly-balanced
   * transactions fail with "SPECKs of Dust not paid" (issue https://github.com/midnightntwrk/midnight-ledger/issues/298).
   *
   * @given A proven transaction paying exactly its real fees with a Dust spend
   * @when Verifying with enforced balancing and applying to the ledger state
   * @then Should succeed regardless of the proof-erased fee estimate
   */
  test('should charge verified fees on apply for an exactly-balanced proven transaction', async () => {
    const state = TestState.new();
    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);

    const buildProven = async (vFee: bigint) => {
      const [, spend] = state.dust.spend(state.dustKey.secretKey, state.dust.utxos[0], vFee, state.time);
      const intent = Intent.new(state.time);
      intent.dustActions = new DustActions(SignatureMarker.signature, ProofMarker.preProof, state.time, [spend], []);
      return prove(Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent));
    };

    // The real fees depend on the proven transaction's size and the Dust
    // spend's vFee feeds back into that size, so keep re-proving with the
    // latest fee until the fee paid matches the fee the tx actually costs
    const proveUntilFeesMatch = async (
      vFee: bigint,
      attemptsLeft: number
    ): Promise<{ tx: Transaction<SignatureEnabled, Proof, PreBinding>; vFee: bigint }> => {
      const tx = await buildProven(vFee);
      const fees = tx.fees(state.ledger.parameters, true);
      if (fees === vFee || attemptsLeft === 0) {
        return { tx, vFee };
      }
      return proveUntilFeesMatch(fees, attemptsLeft - 1);
    };

    const { tx, vFee } = await proveUntilFeesMatch(1n, 5);
    const realFees = tx.fees(state.ledger.parameters, true);
    expect(realFees).toEqual(vFee);

    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = true;
    const verifiedTransaction = tx.wellFormed(state.ledger, strictness, state.time);

    // Before https://github.com/midnightntwrk/midnight-ledger/pull/445 application re-estimated fees from the proof-erased
    // `verifiedTransaction.transaction` instead of charging `realFees`, and
    // rejected the transaction whenever that estimate came out higher.
    const [ledgerStateAfter, transactionResult] = state.ledger.apply(verifiedTransaction, state.context());

    expect(transactionResult.type, `error was: ${transactionResult.error}`).toEqual('success');
    assertSerializationSuccess(ledgerStateAfter);
  });

  /**
   * Placeholder test for double spending scenarios.
   *
   * @given Transaction with double spent inputs
   * @when Applying transaction to ledger state
   * @then Should handle double spending correctly
   */
  test.todo('should handle double spent transactions correctly');
});
