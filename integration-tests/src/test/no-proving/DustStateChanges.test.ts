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
  DustActions,
  DustLocalState,
  DustRegistration,
  DustStateChanges,
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
import { expect } from 'vitest';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import {
  BALANCING_OVERHEAD,
  DEFAULT_TOKEN_TYPE,
  HEX_64_REGEX,
  INITIAL_NIGHT_AMOUNT,
  initialParameters,
  LOCAL_TEST_NETWORK_ID,
  Static
} from '@/test-objects';
import { TestState } from '@/test/utils/TestState';

const buildRegistrationEvents = () => {
  const state = TestState.new();
  const localState = new DustLocalState(initialParameters);
  const { secretKey } = state.dustKey;

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
    { owner: state.initialNightAddress, type: DEFAULT_TOKEN_TYPE, value: INITIAL_NIGHT_AMOUNT }
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

  const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
  const txCtx = new TransactionContext(state.ledger, Static.blockContext(state.time));
  const strictness = new WellFormedStrictness();
  strictness.enforceBalancing = false;
  const verifiedTx = tx.wellFormed(state.ledger, strictness, state.time);
  const { events } = state.ledger.apply(verifiedTx, txCtx)[1];

  return { localState, secretKey, events };
};

describe('Ledger API - DustStateChanges', () => {
  /**
   * Test construction with empty UTXO arrays.
   *
   * @given A valid source hex string and empty UTXO arrays
   * @when Constructing a DustStateChanges instance
   * @then Should return an instance with matching source and UTXO arrays
   */
  test('should construct with empty received and spent UTXO arrays', () => {
    const { localState, secretKey, events } = buildRegistrationEvents();
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const [{ source, receivedUtxos, spentUtxos }] = withChanges.changes;

    const changes = new DustStateChanges(source, receivedUtxos, spentUtxos);

    expect(changes.source).toEqual(source);
    expect(changes.receivedUtxos).toEqual(receivedUtxos);
    expect(changes.spentUtxos).toEqual(spentUtxos);
  });

  /**
   * Test construction with received UTXOs.
   *
   * @given A valid source hex string and a QualifiedDustOutput received in a real transaction
   * @when Constructing a DustStateChanges instance
   * @then Should return an instance whose receivedUtxos getter returns the provided UTXOs
   */
  test('should construct with received UTXOs and expose them via getter', () => {
    const { localState, secretKey, events } = buildRegistrationEvents();
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const [{ source }] = withChanges.changes;
    const receivedUtxo = withChanges.state.utxos[0];

    const changes = new DustStateChanges(source, [receivedUtxo], []);

    expect(changes.receivedUtxos).toHaveLength(1);
    expect(changes.receivedUtxos[0]).toEqual(receivedUtxo);
    expect(changes.spentUtxos).toEqual([]);
  });

  /**
   * Test construction with spent UTXOs.
   *
   * @given A valid source hex string and a QualifiedDustOutput as a spent UTXO
   * @when Constructing a DustStateChanges instance
   * @then Should return an instance whose spentUtxos getter returns the provided UTXOs
   */
  test('should construct with spent UTXOs and expose them via getter', () => {
    const { localState, secretKey, events } = buildRegistrationEvents();
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const [{ source }] = withChanges.changes;
    const utxo = withChanges.state.utxos[0];

    const changes = new DustStateChanges(source, [], [utxo]);

    expect(changes.spentUtxos).toHaveLength(1);
    expect(changes.spentUtxos[0]).toEqual(utxo);
    expect(changes.receivedUtxos).toEqual([]);
  });

  /**
   * Test construction with multiple received and spent UTXOs.
   *
   * @given A valid source and multiple UTXOs in both received and spent arrays
   * @when Constructing a DustStateChanges instance
   * @then Should preserve all UTXOs in both arrays
   */
  test('should construct with multiple received and spent UTXOs', () => {
    const { localState, secretKey, events } = buildRegistrationEvents();
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const [{ source }] = withChanges.changes;
    const utxo = withChanges.state.utxos[0];

    const changes = new DustStateChanges(source, [utxo, utxo], [utxo]);

    expect(changes.receivedUtxos).toHaveLength(2);
    expect(changes.spentUtxos).toHaveLength(1);
  });

  /**
   * Test that source getter returns a valid hex string.
   *
   * @given A DustStateChanges obtained by replaying real transaction events
   * @when Reading the source after reconstructing it via the public constructor
   * @then Should return a hex-encoded string matching the expected pattern
   */
  test('should expose source as a hex-encoded string', () => {
    const { localState, secretKey, events } = buildRegistrationEvents();
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const [{ source }] = withChanges.changes;

    const changes = new DustStateChanges(source, [], []);

    expect(changes.source).toMatch(HEX_64_REGEX);
  });

  /**
   * Test error thrown on invalid source hex string.
   *
   * @given A non-hex source string
   * @when Constructing a DustStateChanges instance
   * @then Should throw an error indicating the source is invalid
   */
  test('should throw on invalid source hex string', () => {
    expect(() => new DustStateChanges('not-a-hex-string', [], [])).toThrow();
  });

  /**
   * Test error thrown on a hex source with the wrong length.
   *
   * @given A hex source string too short to represent a TransactionHash
   * @when Constructing a DustStateChanges instance
   * @then Should throw an error
   */
  test('should throw on source hex string with wrong length', () => {
    expect(() => new DustStateChanges('deadbeef', [], [])).toThrow();
  });

  /**
   * Test error thrown on a malformed UTXO object.
   *
   * @given A valid source but a UTXO object with invalid field values
   * @when Constructing a DustStateChanges instance
   * @then Should throw an error indicating the UTXO is invalid
   */
  test('should throw on malformed UTXO object in receivedUtxos', () => {
    const { localState, secretKey, events } = buildRegistrationEvents();
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const [{ source }] = withChanges.changes;
    const invalidUtxo = {
      initialValue: 100n,
      owner: 999999999999999999999999999999999999999999999999999999999999n,
      nonce: -1n,
      seq: 0,
      ctime: new Date(),
      backingNight: 'not-a-hex',
      mtIndex: 0n
    };

    expect(() => new DustStateChanges(source, [invalidUtxo as never], [])).toThrow();
  });
});
