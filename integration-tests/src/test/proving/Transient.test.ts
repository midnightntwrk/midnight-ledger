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

import { ZswapTransient, ZswapOffer, ZswapOutput, Transaction } from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { getQualifiedShieldedCoinInfo, HEX_64_REGEX, Random, Static } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - Transient [@slow][@proving]', () => {
  /**
   * Test transient serialization and deserialization with proofs.
   *
   * @given A proven transaction with contract-owned transient output
   * @when Serializing and deserializing the transient
   * @then Should maintain object integrity and string representation
   */
  test('should serialize and deserialize transients correctly', async () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );
    const unprovenOffer = ZswapOffer.fromTransient(unprovenTransient);
    const unprovenTransaction = Transaction.fromParts('local-test', unprovenOffer);
    const transaction = await prove(unprovenTransaction);
    const transient = transaction.guaranteedOffer?.transients[0];
    const output2 = ZswapTransient.deserialize('proof', transient!.serialize());

    expect(output2.toString()).toEqual(transient?.toString());
  });

  /**
   * Test transient construction and properties verification.
   *
   * @given A proven transaction with contract-owned transient
   * @when Accessing the transient from guaranteed offer
   * @then Should have valid commitment, contract address, and nullifier in hex format
   */
  test('should construct transients with correct properties', async () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );
    const unprovenOffer = ZswapOffer.fromTransient(unprovenTransient);
    const unprovenTransaction = Transaction.fromParts('local-test', unprovenOffer);
    const transaction = await prove(unprovenTransaction);
    const transient = transaction.guaranteedOffer?.transients[0];

    expect(transient?.commitment).toMatch(HEX_64_REGEX);
    expect(transient?.contractAddress).toEqual(contractAddress);
    expect(transient?.nullifier).toMatch(HEX_64_REGEX);
    assertSerializationSuccess(transient!, undefined, ProofMarker.proof);
  });
});
