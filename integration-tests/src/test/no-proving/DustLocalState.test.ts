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
  addressFromKey,
  DustActions,
  DustLocalState,
  type DustPublicKey,
  DustRegistration,
  Intent,
  type IntentHash,
  type PreProof,
  sampleDustSecretKey,
  sampleSigningKey,
  SignatureEnabled,
  signatureVerifyingKey,
  type SignatureVerifyingKey,
  signData,
  Transaction,
  TransactionContext,
  UnshieldedOffer,
  type UserAddress,
  type UtxoOutput,
  type UtxoSpend,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { expect } from 'vitest';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import {
  BALANCING_OVERHEAD,
  DEFAULT_TOKEN_TYPE,
  DUST_GRACE_PERIOD_IN_SECONDS,
  GENERATION_DECAY_RATE,
  INITIAL_NIGHT_AMOUNT,
  initialParameters,
  LOCAL_TEST_NETWORK_ID,
  NIGHT_DUST_RATIO,
  Static
} from '@/test-objects';
import { TestState } from '@/test/utils/TestState';
import { assertSerializationSuccess } from '@/test-utils';
import { generateSampleDust } from '@/test/utils/dust';

describe('Ledger API - DustLocalState', () => {
  /**
   * Test string representation of empty DustLocalState.
   *
   * @given A new DustLocalState instance
   * @when Calling toString method
   * @then Should return formatted string with empty collections and default values
   */
  test('should print out information as string', () => {
    const localState = new DustLocalState(initialParameters);
    const expected = `DustLocalState {
    generating_tree: MerkleTree(root = Some(-)) {},
    generating_tree_first_free: 0,
    commitment_tree: MerkleTree(root = Some(-)) {},
    commitment_tree_first_free: 0,
    night_indices: {},
    dust_utxos: {},
    sync_time: Timestamp(
        0,
    ),
    params: DustParameters {
        night_dust_ratio: ${NIGHT_DUST_RATIO},
        generation_decay_rate: ${GENERATION_DECAY_RATE},
        dust_grace_period: Duration(
            ${DUST_GRACE_PERIOD_IN_SECONDS},
        ),
    },
}`;

    expect(localState.toString()).toEqual(expected);
  });

  /**
   * Test dust parameters getter of LocalDustState.
   *
   * @given A new LocalDustState instance
   * @when Calling params attribute
   * @then Should return the initial dust params values
   */
  test('should return dust parameters', () => {
    const localState = new DustLocalState(initialParameters);
    expect(localState.params.nightDustRatio).toEqual(NIGHT_DUST_RATIO);
    expect(localState.params.generationDecayRate).toEqual(GENERATION_DECAY_RATE);
    expect(localState.params.dustGracePeriodSeconds).toEqual(DUST_GRACE_PERIOD_IN_SECONDS);
  });

  /**
   * Test serialization and deserialization of LocalDustState.
   *
   * @given A new LocalDustState instance
   * @when Calling serialize method
   * @and Calling deserialize method
   * @then Should return formatted strings with the same values
   */
  test('should serialize and deserialize', () => {
    const localState = new DustLocalState(initialParameters);
    assertSerializationSuccess(localState);
  });

  /**
   * Test Dust generation when Night is given, but not registered yet
   *
   * @given A new TestState
   * @when Calling reward night method
   * @and Calling fast-forward method
   * @then Dust should not be generated despite having Night
   */
  test('should generate 0 Dust for unregistered Night', () => {
    const state = TestState.new();

    state.rewardNight(INITIAL_NIGHT_AMOUNT);
    state.fastForward(initialParameters.timeToCapSeconds);

    expect(state.dust.utxos).toEqual([]);
    expect(state.dust.walletBalance(state.time)).toEqual(0n);
  });

  /**
   * Test Dust generation when Night is given and properly registered
   *
   * By Thomas Kerber:
   *
   * There is a bootstrapping challenge of how a user can pay for transactions if they do not own dust yet; the bootstrapping conditions are:
   * You must own a Night UTXO that is not generating Dust
   * You must spend this UTXO to yourself
   * You must do a Dust registration in the same intent
   * You must declare an allowed fee payment for this Dust registrations
   * If all of these are true, then the registration will act as if your Night UTXO has been generating dust, and let you use that to fund your transaction, as well as a new Dust UTXO coming from it.
   *
   * @given A new TestState
   * @when Calling reward night method
   * @and Calling fast-forward method
   * @and Spending UTXO to oneself
   * @and Registering it
   * @and Waiting time to cap
   * @then Dust should be generated for the maximum amount
   */
  test('should generate maximum of Dust for a given registered Night', () => {
    const state = generateSampleDust(INITIAL_NIGHT_AMOUNT);

    const { dust } = state;

    expect(dust.utxos.length).toEqual(1);
    expect(dust.walletBalance(state.time)).toEqual(NIGHT_DUST_RATIO * INITIAL_NIGHT_AMOUNT);
  });

  /**
   * Test Dust generation when Night is given and properly registered for just a half of the initial amount
   *
   * @given A new TestState
   * @when Calling reward night method
   * @and Calling fast-forward method
   * @and Spending UTXO to 2 addresses (splitting the UTXO to half)
   * @and Registering it
   * @and Waiting time to cap
   * @then Dust should be generated only for the half of maximum amount
   */
  test('should generate Dust proportionally', () => {
    const halfAmount = INITIAL_NIGHT_AMOUNT / 2n;
    const signingKey = sampleSigningKey();
    const verifyingKey = signatureVerifyingKey(signingKey);
    const bobAddress: UserAddress = addressFromKey(verifyingKey);
    const state = TestState.new();

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
        value: halfAmount
      },
      {
        owner: bobAddress,
        type: DEFAULT_TOKEN_TYPE,
        value: halfAmount
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

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    state.assertApply(tx, new WellFormedStrictness());
    state.fastForward(initialParameters.timeToCapSeconds);

    expect(state.dust.utxos.length).toEqual(1);
    expect(state.dust.walletBalance(state.time)).toEqual(NIGHT_DUST_RATIO * halfAmount);
  });

  test('should set the generation info dtime correctly', () => {
    const signingKey = sampleSigningKey();
    const verifyingKey = signatureVerifyingKey(signingKey);
    const bobAddress: UserAddress = addressFromKey(verifyingKey);
    const state = TestState.new();

    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);
    expect(state.dust.utxos.length).toEqual(1);
    expect(state.dust.generationInfo(state.dust.utxos[0])!.dtime).toBeUndefined();

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
        owner: bobAddress,
        type: DEFAULT_TOKEN_TYPE,
        value: INITIAL_NIGHT_AMOUNT
      }
    ];

    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);
    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const balancedTx = state.balanceTx(tx.eraseProofs());
    const normalizedCost = state.ledger.parameters.normalizeFullness(balancedTx.cost(state.ledger.parameters));
    const overallCost = Math.max(
      normalizedCost.readTime,
      normalizedCost.computeTime,
      normalizedCost.blockUsage,
      normalizedCost.bytesWritten,
      normalizedCost.bytesChurned
    );
    state.assertApply(balancedTx, new WellFormedStrictness(), normalizedCost, overallCost);

    expect(state.dust.utxos.length).toEqual(1);
    expect(state.dust.generationInfo(state.dust.utxos[0])!.dtime).toBeInstanceOf(Date);
  });

  /**
   * Test Dust generation when Night is given and properly registered after just a half of the initialCap
   *
   * @given A new TestState
   * @when Calling reward night method
   * @and Calling fast-forward method
   * @and Spending UTXO to 2 addresses
   * @and Registering it
   * @and Calling fast-forward method with just a half of initialCap
   * @then Dust should be generated only in ~half of the maximum amount
   */
  test('should generated only around half of Dust in the middle', () => {
    const two = 2n;
    const initialCap = initialParameters.timeToCapSeconds;
    const halfTimeToFullRegistration = initialCap / two;
    const state = generateSampleDust(INITIAL_NIGHT_AMOUNT, halfTimeToFullRegistration);

    expect(state.dust.walletBalance(state.time)).toBeGreaterThan((NIGHT_DUST_RATIO * INITIAL_NIGHT_AMOUNT) / two);
  });

  /**
   * Stress Test wallet's utxo management
   *
   * By Thomas Kerber:
   *
   * Test Night UTXOs being cycled through Y participants
   * Each participant gets one UTXO to start, then each participant takes turn to move their
   * current UTXO one participant to the right.
   *
   * We end when one full 'cycle' has been completed.
   * This stress-tests the wallet's utxo management, and tree sparsity, by ensuring plenty of
   * sparse insertions and deletions need to take place. We only track the first participant
   * (Alice)'s wallet state, but this will be sparse, as it doesn't see most interactions, and
   * further, interactions do not spend the most recent UTXOs.
   *
   * @given A new TestState
   * @and Length of a cycle
   * @when Calling reward night method
   * @and Calling fast-forward method
   * @and Creating cycle with needed values
   * @and Registering it
   * @and Applying the transaction
   * @then The empty transaction should be valid and well-formed
   */
  test('should test cycle transfers', () => {
    const NIGHT_VAL = 1_000_000_000n;
    const CYCLE_LEN = 128;
    const state = TestState.new();
    const aliceVk: SignatureVerifyingKey = state.nightKey.verifyingKey();
    const aliceAddr: UserAddress = addressFromKey(aliceVk);
    const aliceDust: DustPublicKey = state.dustKey.publicKey();

    const cycle: [[SignatureVerifyingKey, UserAddress, DustPublicKey]] = [[aliceVk, aliceAddr, aliceDust]];
    for (let i = 1; i < CYCLE_LEN; i++) {
      const sk = sampleSigningKey();
      const vk = signatureVerifyingKey(sk);
      const addr: UserAddress = addressFromKey(vk);
      const dust: DustPublicKey = sampleDustSecretKey().publicKey;
      cycle.push([vk, addr, dust]);
    }

    state.rewardNight(BigInt(CYCLE_LEN) * NIGHT_VAL);
    state.fastForward(initialParameters.timeToCapSeconds);

    let utxoIh: IntentHash = state.ledger.utxo.utxos.values().next().value!.intentHash;

    let intent = Intent.new(state.time);
    const outputsWithNumbers: Array<[UtxoOutput, number]> = cycle.map(([, addr], i) => [
      {
        owner: addr,
        type: DEFAULT_TOKEN_TYPE,
        value: NIGHT_VAL
      },
      i
    ]);

    outputsWithNumbers.sort(([a], [b]) => a.owner.localeCompare(b.owner));

    intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
      [
        {
          intentHash: utxoIh,
          value: BigInt(CYCLE_LEN) * NIGHT_VAL,
          owner: aliceVk,
          type: DEFAULT_TOKEN_TYPE,
          outputNo: 0
        }
      ],
      outputsWithNumbers.map(([out]) => out),
      []
    );

    const registrations: DustRegistration<SignatureEnabled>[] = cycle.map(
      ([night, , dust]) => new DustRegistration(SignatureMarker.signature, night, dust, 0n)
    );

    intent.dustActions = new DustActions(
      SignatureMarker.signature,
      ProofMarker.preProof,
      state.time,
      [],
      registrations
    );

    utxoIh = intent.intentHash(0);
    const utxos: Array<Array<[IntentHash, number]>> = Array.from({ length: cycle.length }, () => []);
    outputsWithNumbers.forEach(([, i], j) => {
      utxos[i].push([utxoIh, j]);
    });

    let tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;
    unbalancedStrictness.verifySignatures = false;
    state.assertApply(tx, unbalancedStrictness);

    const N = 4;

    for (let i = 0; i < CYCLE_LEN * N; i++) {
      const sender: SignatureVerifyingKey = cycle[i % CYCLE_LEN][0];
      const recipient: UserAddress = cycle[(i + 1) % CYCLE_LEN][1];
      const utxo = utxos[i % CYCLE_LEN].shift()!;
      intent = Intent.new(state.time);
      const inputs: UtxoSpend[] = [
        {
          value: NIGHT_VAL,
          owner: sender,
          type: DEFAULT_TOKEN_TYPE,
          intentHash: utxo[0],
          outputNo: utxo[1]
        }
      ];

      const outputs = [
        {
          value: NIGHT_VAL,
          owner: recipient,
          type: DEFAULT_TOKEN_TYPE
        }
      ];
      intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(inputs, outputs, []);
      utxoIh = intent.intentHash(0);
      utxos[(i + 1) % CYCLE_LEN].push([utxoIh, 0]);
      tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);

      state.assertApply(tx, unbalancedStrictness);
    }
    state.fastForward(initialParameters.timeToCapSeconds);

    const emptyTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID);
    const balancedTx = state.balanceTx(emptyTx.eraseProofs());
    state.assertApply(balancedTx, new WellFormedStrictness());
  });

  /**
   * Test replayEventsWithChanges - handling Dust registration.
   *
   * @given A DustLocalState with a transaction containing Dust registration
   * @when Replaying events with changes on a transaction that registers Dust
   * @then Should track and confirm received UTXOs in changes, and update local state correctly
   */
  test('replayEventsWithChanges - should handle Dust registration', () => {
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

    const tx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const transactionContext = new TransactionContext(state.ledger, Static.blockContext(state.time));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedTransaction = tx.wellFormed(state.ledger, strictness, state.time);
    const { events } = state.ledger.apply(verifiedTransaction, transactionContext)[1];
    const withChanges = localState.replayEventsWithChanges(secretKey, events);
    const appliedTxLocalState = withChanges.state;

    // Verify state changes - should have received UTXO but no spent UTXOs
    const allReceivedUtxos = withChanges.changes.flatMap((change) => change.receivedUtxos);
    const allSpentUtxos = withChanges.changes.flatMap((change) => change.spentUtxos);

    expect(allSpentUtxos).toEqual([]);
    expect(allReceivedUtxos.length).toEqual(1);

    // After registration, a Dust UTXO is created immediately
    // Verify the received UTXO matches what's in the state
    const actualUtxo = appliedTxLocalState.utxos[0];
    expect(allReceivedUtxos).toEqual([actualUtxo]);

    // Verify state was updated
    expect(appliedTxLocalState.utxos).toEqual([actualUtxo]);
    assertSerializationSuccess(appliedTxLocalState);
  });

  /**
   * Test replayEventsWithChanges - handling non-empty state with empty transaction.
   *
   * @given A DustLocalState with generated Dust UTXOs
   * @when Replaying events with changes on an empty transaction (no Dust actions)
   * @then Should have no spent or received UTXOs in changes, and state should remain unchanged
   */
  test('replayEventsWithChanges - should handle non-empty state', () => {
    const state = TestState.new();
    let localState = new DustLocalState(initialParameters);
    const { secretKey } = state.dustKey;

    // First, register and generate some Dust
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

    const registrationTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const transactionContext = new TransactionContext(state.ledger, Static.blockContext(state.time));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedRegistrationTx = registrationTx.wellFormed(state.ledger, strictness, state.time);
    const { events: registrationEvents } = state.ledger.apply(verifiedRegistrationTx, transactionContext)[1];
    const withRegistrationChanges = localState.replayEventsWithChanges(secretKey, registrationEvents);
    localState = withRegistrationChanges.state;

    // Fast forward to generate Dust
    state.fastForward(initialParameters.timeToCapSeconds);

    const initialUtxoCount = localState.utxos.length;
    expect(initialUtxoCount).toEqual(1);

    // Create a new empty transaction
    const emptyIntent = Intent.new(state.time);
    const emptyTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, emptyIntent);
    const emptyTransactionContext = new TransactionContext(state.ledger, Static.blockContext(state.time));
    strictness.verifySignatures = false;
    const verifiedEmptyTx = emptyTx.wellFormed(state.ledger, strictness, state.time);
    const { events: emptyEvents } = state.ledger.apply(verifiedEmptyTx, emptyTransactionContext)[1];
    const withEmptyChanges = localState.replayEventsWithChanges(secretKey, emptyEvents);
    const appliedTxLocalState = withEmptyChanges.state;

    // Verify state changes - empty transaction should not change UTXOs
    const allReceivedUtxos = withEmptyChanges.changes.flatMap((change) => change.receivedUtxos);
    const allSpentUtxos = withEmptyChanges.changes.flatMap((change) => change.spentUtxos);

    expect(allSpentUtxos).toEqual([]);
    expect(allReceivedUtxos).toEqual([]);

    // State should remain the same (empty transaction doesn't generate new UTXOs)
    expect(appliedTxLocalState.utxos.length).toEqual(initialUtxoCount);
    assertSerializationSuccess(appliedTxLocalState);
  });

  /**
   * Test replayEventsWithChanges - tracking spent and received UTXOs in transfer.
   *
   * @given A DustLocalState with an initial Dust UTXO and a transfer transaction
   * @when Replaying events with changes on a transfer that spends and receives UTXOs
   * @then Should track and confirm both spent UTXOs and received UTXOs in the changes
   */
  test('replayEventsWithChanges - should track spent and received UTXOs in transfer transaction', () => {
    const state = TestState.new();
    let localState = new DustLocalState(initialParameters);
    const { secretKey } = state.dustKey;

    // Step 1: Register and generate Dust
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

    const registrationTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, intent);
    const transactionContext = new TransactionContext(state.ledger, Static.blockContext(state.time));
    const strictness = new WellFormedStrictness();
    strictness.enforceBalancing = false;
    const verifiedRegistrationTx = registrationTx.wellFormed(state.ledger, strictness, state.time);
    const [afterRegistrationLedgerState, { events: registrationEvents }] = state.ledger.apply(
      verifiedRegistrationTx,
      transactionContext
    );
    state.ledger = afterRegistrationLedgerState.postBlockUpdate(state.time);
    const withRegistrationChanges = localState.replayEventsWithChanges(secretKey, registrationEvents);
    localState = withRegistrationChanges.state;

    // Fast forward to generate Dust
    state.fastForward(initialParameters.timeToCapSeconds);

    // Apply an empty transaction to sync local state with ledger after fast-forward
    const syncIntent = Intent.new(state.time);
    const syncTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, syncIntent);
    const syncTransactionContext = new TransactionContext(state.ledger, Static.blockContext(state.time));
    strictness.verifySignatures = false;
    const verifiedSyncTx = syncTx.wellFormed(state.ledger, strictness, state.time);
    const { events: syncEvents } = state.ledger.apply(verifiedSyncTx, syncTransactionContext)[1];
    const withSyncChanges = localState.replayEventsWithChanges(secretKey, syncEvents);
    localState = withSyncChanges.state;

    expect(localState.utxos.length).toEqual(1);
    const utxoToSpend = localState.utxos[0];

    // Step 2: Spend the Dust UTXO and create a new one (transfer)
    const vFee = 0n;
    const [localStateWithSpend, dustSpend] = localState.spend(secretKey, utxoToSpend, vFee, state.time);

    const transferIntent = Intent.new(state.time);
    transferIntent.dustActions = new DustActions<SignatureEnabled, PreProof>(
      SignatureMarker.signature,
      ProofMarker.preProof,
      state.time,
      [dustSpend],
      []
    );

    const transferTx = Transaction.fromParts(LOCAL_TEST_NETWORK_ID, undefined, undefined, transferIntent);
    const transferTransactionContext = new TransactionContext(state.ledger, Static.blockContext(state.time));
    strictness.verifySignatures = false;
    const verifiedTransferTx = transferTx.wellFormed(state.ledger, strictness, state.time);
    const { events: transferEvents } = state.ledger.apply(verifiedTransferTx, transferTransactionContext)[1];

    // Step 3: Replay events with changes and verify both spent and received UTXOs
    // Use localStateWithSpend because it has the pending spend tracked
    const withTransferChanges = localStateWithSpend.replayEventsWithChanges(secretKey, transferEvents);
    const allReceivedUtxos = withTransferChanges.changes.flatMap((change) => change.receivedUtxos);
    const allSpentUtxos = withTransferChanges.changes.flatMap((change) => change.spentUtxos);

    // Should have spent the original UTXO
    expect(allSpentUtxos.length).toEqual(1);
    expect(allSpentUtxos).toEqual([utxoToSpend]);

    // Dust spend creates a new commitment (for change or output)
    // When spending with 0 fee, we should receive change back
    expect(allReceivedUtxos.length).toEqual(1);
    const receivedUtxo = allReceivedUtxos[0];

    // Verify final state - should have the received UTXO (change)
    expect(withTransferChanges.state.utxos.length).toEqual(1);
    expect(withTransferChanges.state.utxos[0]).toEqual(receivedUtxo);
    assertSerializationSuccess(withTransferChanges.state);
  });
});
