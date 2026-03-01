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
  ContractDeploy,
  ContractMaintenanceAuthority,
  ContractOperation,
  ContractState,
  encodeContractAddress,
  PrePartitionContractCall,
  PreTranscript,
  QueryContext,
  Transaction,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import {
  INITIAL_NIGHT_AMOUNT,
  LOCAL_TEST_NETWORK_ID,
  type ShieldedTokenType,
  Static,
  TestResource
} from '@/test-objects';
import { kernelSelf, programWithResults } from '@/test/utils/onchain-runtime-program-fragments';
import { testIntents } from '@/test-utils';
import { ATOM_BYTES_32 } from '@/test/utils/value-alignment';
import { expect } from 'vitest';

describe('Ledger API - PrePartitionContractCall', () => {
  /**
   * Test string representation of PrePartitionContractCall.
   *
   * @given A new PrePartitionContractCall instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const STORE = 'store';
    const state = TestState.new();
    const token: ShieldedTokenType = Static.defaultShieldedTokenType();

    state.rewardsShielded(token, 5_000_000_000n);
    state.giveFeeToken(1, INITIAL_NIGHT_AMOUNT);

    const op = new ContractOperation();
    op.verifierKey = TestResource.operationVerifierKey();

    const { addr, encodedAddr } = deployContract({
      state,
      op,
      store: STORE
    });

    const program = programWithResults([...kernelSelf()], [{ value: [encodedAddr], alignment: [ATOM_BYTES_32] }]);
    const context = new QueryContext(new ChargedState(state.ledger.index(addr)!.data.state), addr);
    const preTranscript = new PreTranscript(context, program);

    const emptyAligned = { value: [], alignment: [] };

    const preCall = new PrePartitionContractCall(
      addr,
      STORE,
      op,
      preTranscript,
      [],
      emptyAligned,
      emptyAligned,
      communicationCommitmentRandomness(),
      STORE
    );

    expect(preCall.toString()).toMatch(/PrePartitionContractCall.*/);
  });

  function deployContract({ state, op, store }: { state: TestState; op: ContractOperation; store: string }): {
    addr: ContractAddress;
    encodedAddr: Uint8Array;
  } {
    const unbalancedStrictness = new WellFormedStrictness();
    unbalancedStrictness.enforceBalancing = false;

    const contract = new ContractState();
    contract.setOperation(store, op);
    contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);

    const deploy = new ContractDeploy(contract);
    const tx = Transaction.fromParts(
      LOCAL_TEST_NETWORK_ID,
      undefined,
      undefined,
      testIntents([], [], [deploy], state.time)
    );
    const addr: ContractAddress = tx.intents!.get(1)!.actions[0].address;
    const encodedAddr = encodeContractAddress(addr);

    tx.wellFormed(state.ledger, unbalancedStrictness, state.time);
    const balanced = state.balanceTx(tx.eraseProofs());
    state.assertApply(balanced, new WellFormedStrictness());

    return { addr, encodedAddr };
  }
});
