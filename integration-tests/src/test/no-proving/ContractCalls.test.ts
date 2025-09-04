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
  communicationCommitmentRandomness,
  type ContractCall,
  ContractCallPrototype,
  ContractOperation,
  ContractState,
  Intent,
  type PreProof,
  Transaction
} from '@midnight-ntwrk/ledger';
import { Random, Static } from '@/test-objects';

describe('Ledger API - ContractCalls', () => {
  /**
   * Test proper construction of ContractCall object.
   *
   * @given A contract call prototype with address, entry point, and operation
   * @when Creating an unproven transaction with the call prototype
   * @then The contract call should be constructed with correct properties
   */
  test('should construct object properly', () => {
    const commitmentRandomness = communicationCommitmentRandomness();
    const contractAddress = Random.contractAddress();
    const contractState = new ContractState();
    contractState.setOperation('operation', new ContractOperation());
    const contractCallPrototype = new ContractCallPrototype(
      contractAddress,
      'entry',
      new ContractOperation(),
      undefined,
      undefined,
      [Static.alignedValue],
      Static.alignedValue,
      Static.alignedValue,
      commitmentRandomness,
      'key_location'
    );
    const intent = Intent.new(new Date()).addCall(contractCallPrototype);
    const unprovenOfferGuaranteed = Static.unprovenOfferFromOutput();
    const unprovenOfferFallible = Static.unprovenOfferFromOutput(1);
    const unprovenTransaction = Transaction.fromParts(
      'local-test',
      unprovenOfferGuaranteed,
      unprovenOfferFallible,
      intent
    );

    const contractCall = unprovenTransaction.intents!.get(1)!.actions.at(0) as ContractCall<PreProof>;
    expect(contractCall.address).toEqual(contractAddress);
    expect(contractCall.communicationCommitment).not.toEqual(commitmentRandomness);
    expect(contractCall.entryPoint).toEqual('entry');
    expect(contractCall.fallibleTranscript).toEqual(undefined);
    expect(contractCall.guaranteedTranscript).toEqual(undefined);
    expect(contractCall.toString(true)).toMatch(/\{contract:.*/);
  });
});
