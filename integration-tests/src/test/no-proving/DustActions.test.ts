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

import {
  type PreProof,
  sampleSigningKey,
  signData,
  DustActions,
  DustRegistration,
  type DustSpend,
  type Proofish,
  type Signaturish,
  signatureVerifyingKey,
  sampleDustSecretKey,
  SignatureEnabled
} from '@midnight-ntwrk/ledger';
import { expect } from 'vitest';
import { ProofMarker, SignatureMarker } from '@/test/utils/Markers';
import { BALANCING_OVERHEAD } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - DustActions', () => {
  /**
   * Test string representation of DustActions.
   *
   * @given A new DustActions instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;

    const signature = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));
    const dustRegistration = new DustRegistration(
      signature.instance,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );
    const spends: DustSpend<PreProof>[] = [];
    const registrations: DustRegistration<SignatureEnabled>[] = [dustRegistration];

    const action = new DustActions(SignatureMarker.signature, ProofMarker.preProof, new Date(0), spends, registrations);
    expect(action.toString()).toMatch(/DustActions.*/);
  });

  /**
   * Test serialization and deserialization of DustActions.
   *
   * @given A new DustActions instance
   * @when Calling serialize method
   * @and Calling deserialize method
   * @then Should return formatted strings with the same values
   */
  test('should serialize and deserialize', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;

    const signature = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));
    const dustRegistration = new DustRegistration(
      signature.instance,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );
    const spends: DustSpend<Proofish>[] = [];
    const registrations: DustRegistration<Signaturish>[] = [dustRegistration];

    const action = new DustActions(SignatureMarker.signature, ProofMarker.preProof, new Date(0), spends, registrations);
    assertSerializationSuccess(action, SignatureMarker.signature, ProofMarker.preProof);
  });

  /**
   * Test all getters of DustActions.
   *
   * @given A new DustActions instance
   * @when Checking all getters
   * @then Should return the same values as initially set
   */
  test('should have all getters valid', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;
    const cTime = new Date(0);

    const signature = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));
    const dustRegistration = new DustRegistration(
      signature.instance,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );
    const spends: DustSpend<Proofish>[] = [];
    const registrations: DustRegistration<Signaturish>[] = [dustRegistration];

    const action = new DustActions(SignatureMarker.signature, ProofMarker.preProof, cTime, spends, registrations);
    expect(action.registrations.length).toEqual(1);
    expect(action.registrations[0].toString()).toEqual(dustRegistration.toString());
    expect(action.spends).toEqual(spends);
    expect(action.ctime).toEqual(cTime);
  });

  /**
   * Test all setters of DustActions.
   *
   * @given A new DustActions instance
   * @when Setting new setters
   * @then Should return the same values as where set
   */
  test('should have all getters valid', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;
    const cTime = new Date(0);

    const signature = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));
    const dustRegistration = new DustRegistration(
      signature.instance,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );
    const spends: DustSpend<Proofish>[] = [];
    const registrations: DustRegistration<Signaturish>[] = [dustRegistration];

    const action = new DustActions(SignatureMarker.signature, ProofMarker.preProof, cTime, spends, registrations);

    const updatedSpends: DustSpend<Proofish>[] = [];
    const updatedRegistrations: DustRegistration<Signaturish>[] = [];
    const updatedCtime = new Date(0);
    updatedCtime.setSeconds(1);

    action.spends = updatedSpends;
    action.registrations = updatedRegistrations;
    action.ctime = updatedCtime;

    expect(action.registrations.length).toEqual(0);
    expect(action.spends).toEqual(updatedSpends);
    expect(action.ctime).toEqual(updatedCtime);
  });
});
