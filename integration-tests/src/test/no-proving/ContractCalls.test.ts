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
  type Bindingish,
  communicationCommitmentRandomness,
  type ContractCall,
  ContractCallPrototype,
  ContractOperation,
  ContractState,
  Intent,
  type PreProof,
  type Proof,
  type Signaturish,
  Transaction
} from '@midnightntwrk/ledger';
import { Random, Static, TestResource } from '@/test-objects';
import { TestState } from '@/test/utils/TestState';
import { firstCall, unbalancedStrictness } from '@/test/utils/contracts';

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

  /**
   * The WASM ledger is built without `proof-verifying`, so the accumulator's pairing check is Rust-only
   * (`ledger/tests/verify-proof.rs`). This API still refuses a point off the curve or outside the subgroup.
   */
  describe('with a proof carrying a deferred accumulator', () => {
    const PROOF_TAG_LENGTH = 'midnight:proof-versioned:'.length;

    let state: TestState;

    beforeEach(() => {
      state = TestState.new();
      state.assertApply(TestResource.verifyProofTx('deploy'), unbalancedStrictness());
    });

    const withLastProofByteFlipped = <S extends Signaturish, B extends Bindingish>(
      tx: Transaction<S, Proof, B>
    ): Uint8Array => {
      const raw = Buffer.from(tx.serialize());
      const proof = Buffer.from(firstCall(tx).proof.serialize()).subarray(PROOF_TAG_LENGTH);
      const proofEnd = raw.indexOf(proof) + proof.length;
      expect(proofEnd).toBeGreaterThan(proof.length);
      raw[proofEnd - 1] = (raw[proofEnd - 1] + 1) % 256;
      return raw;
    };

    /**
     * @given The Rust-proven call against a verify_proof circuit
     * @when Reading its proof
     * @then Should be a V4 proof with exactly one deferred accumulator
     */
    test('should carry exactly one accumulator in a V4 proof', () => {
      const proof = firstCall(TestResource.verifyProofTx('call')).proof.toString(true);

      expect(proof).toMatch(/^V4\(Proof \{ bytes: \[/);
      expect(proof.match(/DeferredAccumulator/g)).toHaveLength(1);
    });

    /**
     * @given A ledger with the verify_proof contract deployed
     * @when Applying the proven call
     * @then Should succeed
     */
    test('should be well-formed and apply', () => {
      state.assertApply(TestResource.verifyProofTx('call'), unbalancedStrictness());
    });

    /**
     * @given The proven call
     * @when Serializing and deserializing it
     * @then Should keep its string representation
     */
    test('should survive a serialization round trip', () => {
      const call = TestResource.verifyProofTx('call');

      const deserialized = Transaction.deserialize('signature', 'proof', 'pre-binding', call.serialize());

      expect(deserialized.toString()).toEqual(call.toString());
    });

    /**
     * @given The proven call with the last byte of its accumulator changed
     * @when Deserializing it
     * @then Should throw naming the curve and subgroup check
     */
    test('should refuse to deserialize once an accumulator point is corrupted', () => {
      const tampered = withLastProofByteFlipped(TestResource.verifyProofTx('call'));

      expect(() => Transaction.deserialize('signature', 'proof', 'pre-binding', tampered)).toThrow(
        /accumulator point is not on the curve, or outside the prime-order subgroup/
      );
    });
  });
});
