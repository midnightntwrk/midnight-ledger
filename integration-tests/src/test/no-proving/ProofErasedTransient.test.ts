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

import { ZswapOffer, ZswapOutput, Transaction, ZswapTransient } from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, HEX_64_REGEX, Random, Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe('Ledger API - ZswapTransient [@slow][@proving]', () => {
  /**
   * Test serialization and deserialization of proof-erased transient.
   *
   * @given A proof-erased transaction with transients
   * @when Serializing and deserializing the first guaranteed transient
   * @then Should maintain identical string representation
   */
  test('should serialize and deserialize correctly', async () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );
    const unprovenOffer = ZswapOffer.fromTransient(unprovenTransient);
    const unprovenTransaction = Transaction.fromParts('local-test', unprovenOffer);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const transient = proofErasedTransaction.guaranteedOffer?.transients[0];
    const transient2 = ZswapTransient.deserialize('no-proof', transient!.serialize());

    expect(transient2.toString()).toEqual(transient?.toString());
  });

  /**
   * Test construction of proof-erased transient.
   *
   * @given A transaction with contract-owned output transient
   * @when Erasing proofs and accessing the first guaranteed transient
   * @then Should have valid commitment, contract address, and nullifier
   */
  test('should construct proof-erased transient correctly', async () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );
    const unprovenOffer = ZswapOffer.fromTransient(unprovenTransient);
    const unprovenTransaction = Transaction.fromParts('local-test', unprovenOffer);
    const proofErasedTransaction = unprovenTransaction.eraseProofs();
    const output = proofErasedTransaction.guaranteedOffer?.transients[0];

    expect(output?.commitment).toMatch(HEX_64_REGEX);
    expect(output?.contractAddress).toEqual(contractAddress);
    expect(output?.nullifier).toMatch(HEX_64_REGEX);
    assertSerializationSuccess(output!, undefined, ProofMarker.noProof);
  });
});
