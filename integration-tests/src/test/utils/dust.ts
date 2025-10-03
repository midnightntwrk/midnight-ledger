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

import { BALANCING_OVERHEAD, DEFAULT_TOKEN_TYPE, initialParameters, LOCAL_TEST_NETWORK_ID } from '@/test-objects';
import { TestState } from '@/test/utils/TestState';
import {
  DustActions,
  DustRegistration,
  Intent,
  type IntentHash,
  type PreProof,
  SignatureEnabled,
  signData,
  Transaction,
  UnshieldedOffer,
  type UtxoOutput,
  type UtxoSpend,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';

export function generateSampleDust(
  nightAmount: bigint,
  fastForwardDustGeneration: bigint = initialParameters.timeToCapSeconds
): TestState {
  const state = TestState.new();

  state.rewardNight(nightAmount);
  state.fastForward(initialParameters.timeToCapSeconds);

  const utxoIh: IntentHash = state.ledger.utxo.utxos.values().next().value!.intentHash;
  const intent = Intent.new(state.time);
  const inputs: UtxoSpend[] = [
    {
      value: nightAmount,
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
      value: nightAmount
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
  state.fastForward(fastForwardDustGeneration);

  return state;
}
