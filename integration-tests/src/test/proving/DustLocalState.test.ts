// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import {
  DustLocalState,
  DustActions,
  DustRegistration,
  Intent,
  type IntentHash,
  type PreProof,
  SignatureEnabled,
  signData,
  Transaction,
  TransactionContext,
  UnshieldedOffer,
  type UtxoOutput,
  type UtxoSpend,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import {
  BALANCING_OVERHEAD,
  DEFAULT_TOKEN_TYPE,
  INITIAL_NIGHT_AMOUNT,
  initialParameters,
  LOCAL_TEST_NETWORK_ID,
  Static
} from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';
import { TestState } from '@/test/utils/TestState';

describe.concurrent('Ledger API - DustLocalStateX [@slow][@proving]', () => {
  /**
   * Test replayEventsWithChanges with proven transactions.
   *
   * @given A DustLocalState and a proven transaction with Dust registration
   * @when Replaying events with changes on the transaction
   * @then Should track and confirm received UTXOs in changes, and update local state correctly
   */
  test('replayEventsWithChanges', async () => {
    const state = TestState.new();
    const localState = new DustLocalState(initialParameters);
    const { secretKey } = state.dustKey;

    // Create a transaction with Dust registration
    state.rewardNight(INITIAL_NIGHT_AMOUNT);
    state.fastForward(initialParameters.timeToCapSeconds);

    const utxoIh: IntentHash = state.ledger.utxo.utxos.values().next().value!.intentHash;
    const intent = Intent.new(state.time);
    const inputs: UtxoSpend[] = [
      {
        value: INITIAL_NIGHT_AMOUNT,
        owner: state.nightKey.verifyingKey(),
        type: DEFAULT_TOKEN_TYPE,
        intentHash: utxoIh,
        outputNo: 0
      }
    ];

    const outputs: UtxoOutput[] = [
      {
        owner: state.initialNightAddress,
        type: DEFAULT_TOKEN_TYPE,
        value: INITIAL_NIGHT_AMOUNT
      }
    ];

    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);

    const baseRegistrations: DustRegistration<SignatureEnabled>[] = [
      new DustRegistration(
        SignatureMarker.signature,
        state.nightKey.verifyingKey(),
        state.dustKey.publicKey(),
        BALANCING_OVERHEAD
      )
    ];

    intent.dustActions = new DustActions<SignatureEnabled, PreProof>(
      SignatureMarker.signature,
      ProofMarker.preProof,
      state.time,
      [],
      baseRegistrations
    );

    const intentSignatureData = intent.signatureData(1);
    const signatureEnabled = new SignatureEnabled(signData(state.nightKey.signingKey, intentSignatureData));

    intent.dustActions = new DustActions(
      SignatureMarker.signature,
      ProofMarker.preProof,
      state.time,
      [],
      baseRegistrations.map(
        (reg) =>
          new DustRegistration(
            SignatureMarker.signature,
            reg.nightKey,
            reg.dustAddress,
            reg.allowFeePayment,
            signatureEnabled
          )
      )
    );

    const unprovenTransaction = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const transaction = await prove(unprovenTransaction);
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = transaction.wellFormed(state.ledger, strictness, state.time);

    const transactionContext = new TransactionContext(state.ledger, Static.blockContext(state.time));
    const [, res] = state.ledger.apply(verifiedTransaction, transactionContext);
    expect(res.type).toEqual('success');
    const { events } = res;
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const localStateAfter = withChanges.state;

    expect(localStateAfter.toString()).not.toEqual(localState.toString());

    // Verify state changes - should have received UTXO but no spent UTXOs
    const allReceivedUtxos = withChanges.changes.flatMap((change) => change.receivedUtxos);
    const allSpentUtxos = withChanges.changes.flatMap((change) => change.spentUtxos);

    expect(allSpentUtxos).toEqual([]);

    // After registration, a Dust UTXO is created immediately
    // Get the actual UTXO from the state to use as expected value
    const actualUtxo = localStateAfter.utxos[0];

    // Verify we received exactly one UTXO matching the actual UTXO from the state
    // This verifies all properties including owner, initialValue, seq, mtIndex, nonce, backingNight, ctime
    expect(allReceivedUtxos).toEqual([actualUtxo]);

    assertSerializationSuccess(localStateAfter);
  });
});
