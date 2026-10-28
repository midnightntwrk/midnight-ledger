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

import '@/setup-proving';
import { type ContractAddress, ContractOperation, Transaction } from '@midnightntwrk/ledger';
import { prove } from '@/proof-provider';
import { TestResource } from '@/test-objects';
import { TestState } from '@/test/utils/TestState';
import { deployOperation, firstCall, noopCallTx, unbalancedStrictness } from '@/test/utils/contracts';

/**
 * Proves against `resources/circuits/noop`, whose only public input is what a `noop` transcript
 * field-reprs to. The WASM ledger has no `proof-verifying`, so applying checks the proof's shape only.
 */
describe('Ledger API - ContractCalls [@slow][@proving]', () => {
  const CIRCUIT = 'noop';
  let state: TestState;
  let op: ContractOperation;
  let addr: ContractAddress;

  beforeEach(() => {
    state = TestState.new();
    op = new ContractOperation();
    op.verifierKey = TestResource.circuit(CIRCUIT)!.verifierKey;
    addr = deployOperation(state, CIRCUIT, op);
  });

  /**
   * @given A deployed operation keyed with a verifier-key[v8] and an unproven noop call
   * @when Proving it through the proof server and applying it
   * @then Should yield a V4 proof without accumulators and succeed
   */
  test('should prove a call against a v8 key and apply it', async () => {
    const proven = await prove(noopCallTx(state, addr, CIRCUIT, op, CIRCUIT));

    expect(firstCall(proven).proof.toString(true)).toMatch(/^V4\(Proof \{ bytes: \[[^\]]+\], accumulators: \[\] \}\)$/);
    state.assertApply(proven, unbalancedStrictness());
  });

  /**
   * @given A proven call
   * @when Serializing and deserializing it
   * @then Should keep its string representation and still apply
   */
  test('should survive a serialization round trip', async () => {
    const proven = await prove(noopCallTx(state, addr, CIRCUIT, op, CIRCUIT));

    const deserialized = Transaction.deserialize('signature', 'proof', 'pre-binding', proven.serialize());

    expect(deserialized.toString()).toEqual(proven.toString());
    state.assertApply(deserialized, unbalancedStrictness());
  });

  /**
   * @given An unproven call carrying an inner proof for a circuit without verify_proof
   * @when Proving it
   * @then Should be rejected by the prover
   */
  test('should refuse to prove a call whose inner proofs the circuit does not consume', async () => {
    const withInnerProof = noopCallTx(state, addr, CIRCUIT, op, CIRCUIT, [new Uint8Array([1, 2, 3])]);

    await expect(prove(withInnerProof)).rejects.toThrow();
  });
});
