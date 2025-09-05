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

import { ZswapChainState } from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';
import { prove } from '@/proof-provider';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';

describe.concurrent('Ledger API - ZSwapChainStateX [@slow][@proving]', () => {
  test('tryApply - no whitelist', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const transaction = await prove(unprovenTransaction);
    const state = new ZswapChainState();

    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const [stateAfter, _] = state.tryApply(transaction.guaranteedOffer!);

    expect(stateAfter.toString()).not.toEqual(state.toString());
    expect(stateAfter.firstFree).toEqual(1n);
    assertSerializationSuccess(stateAfter);
  });

  test('tryApply - whitelist', async () => {
    const unprovenTransaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();
    const transaction = await prove(unprovenTransaction);
    const state = new ZswapChainState();
    const contractAddress = Static.contractAddress();
    const whitelist: Set<string> = new Set<string>([contractAddress]);

    const [stateAfter, mapResult] = state.tryApply(transaction.guaranteedOffer!, whitelist);

    expect(stateAfter.toString()).not.toEqual(state.toString());
    expect(stateAfter.firstFree).toEqual(1n);
    expect(mapResult.size).toEqual(1);
    const next = mapResult.keys().next();
    expect(next.done).toEqual(false);
    assertSerializationSuccess(stateAfter);
  });
});
