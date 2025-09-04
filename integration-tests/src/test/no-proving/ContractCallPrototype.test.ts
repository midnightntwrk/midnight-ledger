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
  type AlignedValue,
  communicationCommitmentRandomness,
  ContractCall,
  ContractCallPrototype,
  ContractOperation,
  type PreBinding,
  type Transcript
} from '@midnight-ntwrk/ledger';

import { Random } from '@/test-objects';

describe('Ledger API - ContractCallPrototype', () => {
  let transcript: Transcript<AlignedValue>;
  let alignedValue: AlignedValue;

  beforeEach(() => {
    transcript = {
      gas: {
        readTime: 0n,
        computeTime: 10n,
        bytesWritten: 0n,
        bytesDeleted: 0n
      },
      effects: {
        claimedNullifiers: [Random.hex(64)],
        claimedShieldedReceives: [Random.hex(64)],
        claimedShieldedSpends: [Random.hex(64)],
        claimedContractCalls: new Array([5n, Random.contractAddress(), Random.hex(64), new Uint8Array([0])]),
        shieldedMints: new Map([[Random.hex(64), 1n]]),
        unshieldedMints: new Map(),
        unshieldedInputs: new Map(),
        unshieldedOutputs: new Map(),
        claimedUnshieldedSpends: new Map()
      },
      program: ['new', { noop: { n: 5 } }]
    };
    alignedValue = {
      value: [new Uint8Array()],
      alignment: [
        {
          tag: 'atom',
          value: { tag: 'field' }
        }
      ]
    };
  });

  /**
   * Test successful creation of ContractCallPrototype.
   *
   * @given Valid contract address, entry point, operation, transcripts, and aligned values
   * @when Creating a ContractCallPrototype
   * @then Should not throw any errors
   */
  test('should create contract call prototype', () => {
    expect(
      () =>
        new ContractCallPrototype(
          Random.contractAddress(),
          'entry',
          new ContractOperation(),
          transcript,
          transcript,
          [alignedValue],
          alignedValue,
          alignedValue,
          communicationCommitmentRandomness(),
          'location'
        )
    ).not.toThrow();
  });

  /**
   * Test error handling for invalid contract address.
   *
   * @given An invalid contract address string
   * @when Creating a ContractCallPrototype
   * @then Should throw an error
   */
  test('should throw error when contract address is invalid', () => {
    expect(
      () =>
        new ContractCallPrototype(
          'invalid_address',
          'entry',
          new ContractOperation(),
          transcript,
          transcript,
          [alignedValue],
          alignedValue,
          alignedValue,
          communicationCommitmentRandomness(),
          'location'
        )
    ).toThrow();
  });

  /**
   * Test handling of empty entry point.
   *
   * @given An empty string as entry point
   * @when Creating a ContractCallPrototype
   * @then Should not throw any errors
   */
  test('should not throw error when entry point is empty', () => {
    expect(
      () =>
        new ContractCallPrototype(
          Random.contractAddress(),
          '',
          new ContractOperation(),
          transcript,
          transcript,
          [alignedValue],
          alignedValue,
          alignedValue,
          communicationCommitmentRandomness(),
          'location'
        )
    ).not.toThrow();
  });

  /**
   * Test creation with undefined transcripts.
   *
   * @given Undefined guaranteed and fallible transcripts
   * @when Creating a ContractCallPrototype
   * @then Should not throw any errors
   */
  test('should create contract call prototype with undefined transcripts', () => {
    expect(
      () =>
        new ContractCallPrototype(
          Random.contractAddress(),
          'entry',
          new ContractOperation(),
          undefined,
          undefined,
          [alignedValue],
          alignedValue,
          alignedValue,
          communicationCommitmentRandomness(),
          'location'
        )
    ).not.toThrow();
  });

  /**
   * Test creation with empty aligned values array.
   *
   * @given An empty array for aligned values
   * @when Creating a ContractCallPrototype
   * @then Should not throw any errors
   */
  test('should create contract call prototype with empty aligned values array', () => {
    expect(
      () =>
        new ContractCallPrototype(
          Random.contractAddress(),
          'entry',
          new ContractOperation(),
          transcript,
          transcript,
          [],
          alignedValue,
          alignedValue,
          communicationCommitmentRandomness(),
          'location'
        )
    ).not.toThrow();
  });

  /**
   * Test string representation of ContractCallPrototype.
   *
   * @given A ContractCallPrototype with valid parameters
   * @when Calling toString method
   * @then Should return a string matching the ContractCallPrototype pattern
   */
  test('should return proper string representation', () => {
    const contractCallPrototype = new ContractCallPrototype(
      Random.contractAddress(),
      'entry',
      new ContractOperation(),
      transcript,
      transcript,
      [alignedValue],
      alignedValue,
      alignedValue,
      communicationCommitmentRandomness(),
      'location'
    );

    expect(contractCallPrototype.toString()).toMatch(/ContractCallPrototype.*/);
  });

  /**
   * Test conversion from prototype to contract call.
   *
   * @given A ContractCallPrototype and a PreBinding object
   * @when Converting the prototype into a ContractCall
   * @then Should return a ContractCall with expected properties
   */
  test('intoCall - should convert prototype into contract call', () => {
    const contractAddress = Random.contractAddress();
    const contractCallPrototype = new ContractCallPrototype(
      contractAddress,
      'entry',
      new ContractOperation(),
      transcript,
      transcript,
      [alignedValue],
      alignedValue,
      alignedValue,
      communicationCommitmentRandomness(),
      'location'
    );

    const preBindingObject: { instance: string; type_: string } = {
      instance: 'pre-binding',
      type_: 'pre-binding'
    };
    const contractCall = contractCallPrototype.intoCall(preBindingObject as unknown as PreBinding);

    expect(contractCall).toBeInstanceOf(ContractCall);
    expect(contractCall.address).toEqual(contractAddress);
    expect(contractCall.guaranteedTranscript).toBeDefined();
    expect(contractCall.fallibleTranscript).toBeDefined();
    expect(contractCall.communicationCommitment).toBeDefined();
    expect(contractCall.proof).toBeDefined();
    expect(contractCall.entryPoint).toEqual('entry');
  });
});
