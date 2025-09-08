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

import { expect } from 'vitest';
import { type QualifiedDustOutput } from '@midnight-ntwrk/ledger';
import { ProofMarker } from '@/test/utils/Markers';
import { INITIAL_NIGHT_AMOUNT, initialParameters, NIGHT_DUST_RATIO } from '@/test-objects';
import { generateSampleDust } from '@/test/utils/dust';

describe('Ledger API - DustSpend', () => {
  /**
   * Test string representation of DustSpend.
   *
   * @given A new DustSpend instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const vFee = 0n;
    const expected = `DustSpend {
    v_fee: ${vFee},
    old_nullifier: <dust nullifier>,
    new_commitment: <dust commitment>,
    proof: <proof>,
}`;
    const state = generateSampleDust(INITIAL_NIGHT_AMOUNT);

    const { dust } = state;

    expect(dust.utxos.length).toEqual(1);

    const qdo: QualifiedDustOutput = dust.utxos[0];
    const [, dustSpend] = dust.spend(state.dustKey.secretKey, qdo, vFee, state.time);

    expect(dustSpend.toString()).toEqual(expected);
  });
  /**
   * Test spend Dust once it's generated
   *
   * @given Generated sample Dust
   * @when Calling spend() method
   * @then Dust should be spent and no UTXOs should be available
   */
  test('should spend Dust UTXO', () => {
    const state = generateSampleDust(INITIAL_NIGHT_AMOUNT);

    const { dust } = state;

    expect(dust.utxos.length).toEqual(1);

    const vFee = 0n;
    const qdo: QualifiedDustOutput = dust.utxos[0];
    const [dustLocalState, dustSpend] = dust.spend(state.dustKey.secretKey, qdo, vFee, state.time);

    expect(dustLocalState.utxos.length).toEqual(0);
    expect(dustSpend.vFee).toEqual(vFee);
    expect(dustSpend.oldNullifier).toBeDefined();
    expect(dustSpend.newCommitment).toBeDefined();
    expect(dustSpend.proof.instance).toEqual(ProofMarker.preProof);
  });

  /**
   * Test regenerate Dust once it's spent
   *
   * @given Generated sample Dust
   * @when Calling spend() method
   * @then Dust should be spent and no UTXOs should be available
   */
  test('should regenerate Dust UTXO once its spent', () => {
    const state = generateSampleDust(INITIAL_NIGHT_AMOUNT);

    const { dust } = state;

    expect(dust.utxos.length).toEqual(1);

    const vFee = 0n;
    const qdo: QualifiedDustOutput = dust.utxos[0];
    const [dustLocalState] = dust.spend(state.dustKey.secretKey, qdo, vFee, state.time);
    state.dust = dustLocalState;

    expect(state.dust.utxos.length).toEqual(0);
    state.fastForward(initialParameters.timeToCapSeconds);

    expect(state.dust.utxos.length).toEqual(1);
    expect(state.dust.walletBalance(state.time)).toEqual(NIGHT_DUST_RATIO * INITIAL_NIGHT_AMOUNT);
  });
});
