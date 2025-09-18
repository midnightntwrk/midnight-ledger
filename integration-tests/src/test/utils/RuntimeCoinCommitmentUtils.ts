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

import { BOOLEAN_HASH_BYTES, PERSISTENT_HASH_BYTES, Static, U128_HASH_BYTES } from '@/test-objects';
import { expect } from 'vitest';
import {
  type AlignedValue,
  bigIntToValue,
  encodeContractAddress,
  encodeShieldedCoinInfo,
  runtimeCoinCommitment,
  sampleContractAddress
} from '@midnight-ntwrk/ledger';

export class RuntimeCoinCommitmentUtils {
  static getShieldedCoinInfoAsAlignedValue(): AlignedValue {
    const coinInfo = Static.shieldedCoinInfo(10_000n);
    const encoded = encodeShieldedCoinInfo(coinInfo);
    const value = bigIntToValue(encoded.value);
    return {
      value: [Static.trimTrailingZeros(encoded.nonce), Static.trimTrailingZeros(encoded.color), value[0]],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: U128_HASH_BYTES } }
      ]
    };
  }

  static getContractRecipient(): AlignedValue {
    const contractAddress = sampleContractAddress();
    const encodedContractAddress = encodeContractAddress(contractAddress);
    const isUserAddress = false;

    return {
      value: [
        RuntimeCoinCommitmentUtils.getArrayForIsLeft(isUserAddress),
        new Uint8Array([]),
        Static.trimTrailingZeros(encodedContractAddress)
      ],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: BOOLEAN_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } }
      ]
    };
  }

  static getArrayForIsLeft(isUserAddress: boolean): Uint8Array {
    /*
       When a Recipient is created, we need to specify is_left, which is a boolean value.
       If the Recipient is a User, is_left must be set to true, which corresponds to Uint8Array([1])
       If the Recipient is a Contract, is_left must be set to false, which corresponds to Uint8Array(0)
      */
    if (isUserAddress) {
      return new Uint8Array([1]);
    }
    return new Uint8Array(0);
  }

  static assertOutcomes(coin: AlignedValue, recipient: AlignedValue) {
    console.log(JSON.stringify(coin, null, 4), JSON.stringify(recipient, null, 4));
    const commitment = runtimeCoinCommitment(coin, recipient);

    expect(commitment).toBeDefined();
    expect(commitment.value).toBeInstanceOf(Array);
    expect(commitment.value.length).toEqual(1);
    expect(commitment.value[0]).toBeInstanceOf(Uint8Array);
    expect(commitment.value[0].length).toBeLessThanOrEqual(PERSISTENT_HASH_BYTES);

    expect(commitment.alignment).toBeInstanceOf(Array);
    expect(commitment.alignment.length).toEqual(1);
    expect(commitment.alignment[0]).toEqual({ tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } });
  }
}
