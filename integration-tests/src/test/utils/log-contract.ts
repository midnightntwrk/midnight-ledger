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
  type AlignedValue,
  communicationCommitmentRandomness,
  type ContractAddress,
  ContractCallPrototype,
  ContractDeploy,
  ContractOperation,
  ContractState,
  type EncodedStateValue,
  Intent,
  type Op,
  Transaction,
  type TransactionResult,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { LOCAL_TEST_NETWORK_ID, Random, Static, TestResource } from '@/test-objects';
import type { TestState } from '@/test/utils/TestState';

const EMPTY_EFFECTS = {
  claimedNullifiers: [],
  claimedShieldedReceives: [],
  claimedShieldedSpends: [],
  claimedContractCalls: [],
  shieldedMints: new Map(),
  unshieldedMints: new Map(),
  unshieldedInputs: new Map(),
  unshieldedOutputs: new Map(),
  claimedUnshieldedSpends: new Map()
};

const ENTRY_POINT = 'doLog';

export interface LogCallOutcome {
  result: TransactionResult;
  address: ContractAddress;
  entryPoint: string;
}

function unboundStrictness(): WellFormedStrictness {
  const strictness = new WellFormedStrictness();
  strictness.enforceBalancing = false;
  strictness.verifyContractProofs = false;
  return strictness;
}

export function applyDoLogCall(
  state: TestState,
  logValue: EncodedStateValue,
  gasBytes: bigint = 8192n
): LogCallOutcome {
  const operation = new ContractOperation();
  operation.verifierKey = TestResource.operationVerifierKey();
  const contractState = new ContractState();
  contractState.setOperation(ENTRY_POINT, operation);
  const deploy = new ContractDeploy(contractState);

  const strictness = unboundStrictness();

  const deployTx = Transaction.fromParts(
    LOCAL_TEST_NETWORK_ID,
    Random.unprovenOfferFromOutput(),
    undefined,
    Intent.new(Static.calcBlockTime(state.time, 50)).addDeploy(deploy)
  ).eraseProofs();
  state.assertApply(deployTx, strictness);

  const program: Op<AlignedValue>[] = [{ push: { storage: false, value: logValue } }, 'log'];
  const call = new ContractCallPrototype(
    deploy.address,
    ENTRY_POINT,
    operation,
    {
      gas: { readTime: 0n, computeTime: 10_000_000_000n, bytesWritten: gasBytes, bytesDeleted: gasBytes },
      effects: EMPTY_EFFECTS,
      program
    },
    undefined,
    [Static.alignedValue],
    Static.alignedValue,
    Static.alignedValue,
    communicationCommitmentRandomness(),
    'key_location'
  );

  const callTx = Transaction.fromParts(
    LOCAL_TEST_NETWORK_ID,
    Random.unprovenOfferFromOutput(),
    undefined,
    Intent.new(Static.calcBlockTime(state.time, 50)).addCall(call)
  ).eraseProofs();

  const result = state.apply(callTx, strictness);
  return { result, address: deploy.address, entryPoint: ENTRY_POINT };
}
