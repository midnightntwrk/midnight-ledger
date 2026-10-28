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

import { TestState } from '@/test/utils/TestState';
import {
  ChargedState,
  communicationCommitmentRandomness,
  type ContractAddress,
  ContractOperation,
  encodeContractAddress,
  PrePartitionContractCall,
  PreTranscript,
  QueryContext
} from '@midnightntwrk/ledger';
import { TestResource } from '@/test-objects';
import { kernelSelf, programWithResults } from '@/test/utils/onchain-runtime-program-fragments';
import { deployOperation, firstCall, noopCallTx } from '@/test/utils/contracts';
import { ATOM_BYTES_32 } from '@/test/utils/value-alignment';
import { expect } from 'vitest';

describe('Ledger API - PrePartitionContractCall', () => {
  const STORE = 'store';
  let state: TestState;
  let op: ContractOperation;
  let addr: ContractAddress;

  beforeEach(() => {
    state = TestState.new();
    op = new ContractOperation();
    op.verifierKey = TestResource.operationVerifierKey();
    addr = deployOperation(state, STORE, op);
  });

  /**
   * Test string representation of PrePartitionContractCall.
   *
   * @given A new PrePartitionContractCall instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const program = programWithResults(
      [...kernelSelf()],
      [{ value: [encodeContractAddress(addr)], alignment: [ATOM_BYTES_32] }]
    );
    const context = new QueryContext(new ChargedState(state.ledger.index(addr)!.data.state), addr);
    const emptyAligned = { value: [], alignment: [] };

    const preCall = new PrePartitionContractCall(
      addr,
      STORE,
      op,
      new PreTranscript(context, program),
      [],
      emptyAligned,
      emptyAligned,
      communicationCommitmentRandomness(),
      STORE
    );

    expect(preCall.toString()).toMatch(/PrePartitionContractCall.*/);
  });

  /**
   * @given A pre-partition call given two inner proofs, one of them empty
   * @when Adding it to a transaction
   * @then Should list both in the call preimage in the given order
   */
  test('should carry the inner proofs through partitioning into the call preimage', () => {
    const tx = noopCallTx(state, addr, STORE, op, STORE, [new Uint8Array([1, 2, 3]), new Uint8Array()]);

    expect(firstCall(tx).proof.toString(true)).toContain('inner_proofs: [Direct([1, 2, 3]), Direct([])]');
  });

  /**
   * @given A pre-partition call given no inner proofs
   * @when Adding it to a transaction
   * @then Should have an empty inner proof list in the call preimage
   */
  test('should carry no inner proofs when none are given', () => {
    const tx = noopCallTx(state, addr, STORE, op, STORE);

    expect(firstCall(tx).proof.toString(true)).toContain('inner_proofs: []');
  });
});
