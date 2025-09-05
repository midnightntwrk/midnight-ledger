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

import { ZswapOutput, ZswapTransient } from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, HEX_64_REGEX, Random, Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { ProofMarker } from '@/test/utils/Markers';

describe('Ledger API - ZswapTransient', () => {
  /**
   * Test prevention of contract-owned transients from user outputs.
   *
   * @given A user-owned ZswapOutput
   * @when Attempting to create a transient from contract-owned output
   * @then Should throw error about attempting to spend user-owned output as contract owned
   */
  test('should not allow contract-owned transients from user outputs', () => {
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenOutput = ZswapOutput.new(coinInfo, 0, Static.coinPublicKey(), Static.encryptionPublicKey());
    expect(() =>
      ZswapTransient.newFromContractOwnedOutput(getQualifiedShieldedCoinInfo(coinInfo), 0, unprovenOutput)
    ).toThrow('attempted to spend a user-owned output as contract owned');
  });

  /**
   * Test creation of unproven transient.
   *
   * @given A contract address and contract-owned output
   * @when Creating a ZswapTransient from contract-owned output
   * @then Should create transient with valid contract address, nullifier, and commitment
   */
  test('should create unproven transient', () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );

    expect(unprovenTransient.contractAddress).toEqual(contractAddress);
    expect(unprovenTransient.nullifier).toMatch(HEX_64_REGEX);
    expect(unprovenTransient.commitment).toMatch(HEX_64_REGEX);
    expect(unprovenTransient.toString()).toMatch(
      /<shielded transient coin Commitment(.*) Nullifier(.*) for: ContractAddress(.*)>/
    );
  });

  /**
   * Test serialization and deserialization of ZswapTransient.
   *
   * @given An unproven ZswapTransient
   * @when Serializing and then deserializing
   * @then Should maintain identical string representation
   */
  test('should serialize and deserialize', () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );
    const serialized = unprovenTransient.serialize();
    const unprovenTransientDeserialized = ZswapTransient.deserialize('pre-proof', serialized);

    expect(unprovenTransientDeserialized.toString()).toEqual(unprovenTransient.toString());
  });

  /**
   * Test error handling for invalid serialization.
   *
   * @given Invalid serialized data (single byte array)
   * @when Attempting to deserialize ZswapTransient
   * @then Should throw 'Unexpected length of input' error
   */
  test('should throw an error for invalid serialization', () => {
    const invalidSerialized = new Uint8Array([100]);
    expect(() => {
      ZswapTransient.deserialize('pre-proof', invalidSerialized);
    }).toThrow(/expected header tag 'midnight:zswap-transient/);
  });

  /**
   * Test handling of zero-value coin info.
   *
   * @given A contract address and coin info with zero value
   * @when Creating a ZswapTransient
   * @then Should create valid transient with proper address, nullifier, and commitment
   */
  test('should handle empty coin info', () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(0n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );

    expect(unprovenTransient.contractAddress).toEqual(contractAddress);
    expect(unprovenTransient.nullifier).toMatch(HEX_64_REGEX);
    expect(unprovenTransient.commitment).toMatch(HEX_64_REGEX);
    assertSerializationSuccess(unprovenTransient, undefined, ProofMarker.preProof);
  });

  /**
   * Test that input proof is undefined before proving.
   *
   * @given An unproven ZswapTransient
   * @when Checking input and output proof instances
   * @then Both should be of pre-proof type before proving
   */
  test('inputProof - should be of pre-proof type before proving', () => {
    const contractAddress = Random.contractAddress();
    const coinInfo = Static.shieldedCoinInfo(10n);
    const unprovenTransient = ZswapTransient.newFromContractOwnedOutput(
      getQualifiedShieldedCoinInfo(coinInfo),
      0,
      ZswapOutput.newContractOwned(coinInfo, 0, contractAddress)
    );

    expect(unprovenTransient.outputProof.instance).toEqual('pre-proof');
    expect(unprovenTransient.inputProof.instance).toEqual('pre-proof');
  });
});
