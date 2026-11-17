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
  ChargedState,
  communicationCommitmentRandomness,
  type ContractAddress,
  type ContractCall,
  ContractDeploy,
  ContractMaintenanceAuthority,
  type ContractOperation,
  ContractState,
  type PreBinding,
  PrePartitionContractCall,
  type PreProof,
  PreTranscript,
  type Proofish,
  QueryContext,
  type SignatureEnabled,
  type Signaturish,
  Transaction,
  WellFormedStrictness
} from '@midnightntwrk/ledger';
import { LOCAL_TEST_NETWORK_ID } from '@/test-objects';
import { plus1Hour, testIntents } from '@/test-utils';
import { type TestState } from '@/test/utils/TestState';

const EMPTY_ALIGNED = { value: [], alignment: [] };

export const unbalancedStrictness = (): WellFormedStrictness => {
  const strictness = new WellFormedStrictness();
  strictness.enforceBalancing = false;
  return strictness;
};

export const deployOperation = (state: TestState, entryPoint: string, op: ContractOperation): ContractAddress => {
  const contract = new ContractState();
  contract.setOperation(entryPoint, op);
  contract.maintenanceAuthority = new ContractMaintenanceAuthority([], 1, 0n);
  const deploy = new ContractDeploy(contract);
  const tx = Transaction.fromParts(
    LOCAL_TEST_NETWORK_ID,
    undefined,
    undefined,
    testIntents([], [], [deploy], state.time)
  );
  state.assertApply(tx.eraseProofs(), unbalancedStrictness());
  return deploy.address;
};

/** An unproven transaction calling `entryPoint` with a single `noop`, the least a call can carry. */
export const noopCallTx = (
  state: TestState,
  address: ContractAddress,
  entryPoint: string,
  op: ContractOperation,
  keyLocation: string,
  innerProofs?: Uint8Array[]
): Transaction<SignatureEnabled, PreProof, PreBinding> => {
  const context = new QueryContext(new ChargedState(state.ledger.index(address)!.data.state), address);
  const call = new PrePartitionContractCall(
    address,
    entryPoint,
    op,
    new PreTranscript(context, [{ noop: { n: 1 } }]),
    [],
    EMPTY_ALIGNED,
    EMPTY_ALIGNED,
    communicationCommitmentRandomness(),
    keyLocation,
    innerProofs
  );
  return Transaction.fromParts(LOCAL_TEST_NETWORK_ID).addCalls(
    { tag: 'first' },
    [call],
    state.ledger.parameters,
    plus1Hour(state.time)
  );
};

export const firstCall = <S extends Signaturish, P extends Proofish, B extends Bindingish>(
  tx: Transaction<S, P, B>
): ContractCall<P> => tx.intents!.get(1)!.actions[0] as ContractCall<P>;
