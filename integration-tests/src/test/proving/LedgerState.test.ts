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
  type BlockContext,
  LedgerState,
  TransactionContext,
  Transaction,
  ZswapChainState,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';

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
      parentBlockHash: Static.parentBlockHash()
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
      parentBlockHash: Static.parentBlockHash()
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
   * Placeholder test for double spending scenarios.
   *
   * @given Transaction with double spent inputs
   * @when Applying transaction to ledger state
   * @then Should handle double spending correctly
   */
  test.todo('should handle double spent transactions correctly');
});
