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

import { prove } from '@/proof-provider';
import { Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { LedgerState, WellFormedStrictness, ZswapChainState } from '@midnight-ntwrk/ledger';
import '@/setup-proving';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - WellFormedStrictness [@proving][@slow]', () => {
  test('wellFormed without enforceBalancing', async () => {
    const date = new Date();
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const zSwapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zSwapChainState);
    const strictness = new WellFormedStrictness();
    strictness.verifyContractProofs = true;
    strictness.enforceBalancing = false;
    strictness.verifyNativeProofs = true;

    expect(() => transaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).not.toThrow();
    expect(transaction.identifiers().length).toEqual(3);
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });

  test('wellFormed with enforceBalancing', async () => {
    const date = new Date();
    const transaction = await prove(Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls());
    const zSwapChainState = new ZswapChainState();
    const ledgerState = new LedgerState('local-test', zSwapChainState);
    const strictness = new WellFormedStrictness();
    strictness.verifyContractProofs = true;
    strictness.enforceBalancing = true;
    strictness.verifyNativeProofs = true;

    expect(() => transaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).toThrow(
      /invalid balance -\d+ for token .* in segment \d+; balance must be positive/
    );
    assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
  });
});
