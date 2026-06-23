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

import { SignatureEnabled, signData, sampleSigningKey } from '@midnight-ntwrk/ledger';

describe('Ledger API - SignatureEnabled', () => {
  /**
   * Test serialization round-trip of a wrapped signature.
   *
   * @given A SignatureEnabled wrapping a real signature
   * @when Serializing and deserializing it
   * @then The deserialized value should re-serialize to identical bytes
   */
  test('serializes and deserializes a wrapped signature', () => {
    const signature = new SignatureEnabled(signData(sampleSigningKey(), new Uint8Array(32)));
    const serialized = signature.serialize();

    expect(SignatureEnabled.deserialize(serialized).serialize()).toEqual(serialized);
  });
});
