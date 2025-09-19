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
import {
  DustRegistration,
  sampleDustSecretKey,
  sampleSigningKey,
  SignatureEnabled,
  signatureVerifyingKey,
  signData
} from '@midnight-ntwrk/ledger';
import { SignatureMarker } from '@/test/utils/Markers';
import { BALANCING_OVERHEAD } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';

describe('Ledger API - DustRegistration', () => {
  /**
   * Test string representation of DustRegistration.
   *
   * @given A new DustRegistration instance
   * @when Calling toString method
   * @then Should return formatted string with default values
   */
  test('should print out information as string', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;

    const signature = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));
    const dustRegistration = new DustRegistration(
      SignatureMarker.signature,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );

    expect(dustRegistration.toString()).toMatch(/DustRegistration.*/);
  });

  /**
   * Test serialization and deserialization of DustRegistration.
   *
   * @given A new DustRegistration instance
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
      SignatureMarker.signature,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );
    assertSerializationSuccess(dustRegistration, SignatureMarker.signature);
  });

  /**
   * Test all getters of DustRegistration.
   *
   * @given A new DustRegistration instance
   * @when Checking all getters
   * @then Should return the same values as initially set
   */
  test('should have all getters valid', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;

    const signature = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));

    const dustRegistration = new DustRegistration(
      SignatureMarker.signature,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );

    expect(dustRegistration.nightKey).toEqual(nightKey);
    expect(dustRegistration.dustAddress).toEqual(dustAddress);
    expect(dustRegistration.allowFeePayment).toEqual(BALANCING_OVERHEAD);
    expect(dustRegistration.signature.toString()).toEqual(signature.toString());
  });

  /**
   * Test all setters of DustRegistration.
   *
   * @given A new DustRegistration instance
   * @when Setting new setters
   * @then Should return the same values as where set
   */
  test('should have all setters valid', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;

    const signature = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));

    const dustRegistration = new DustRegistration(
      SignatureMarker.signature,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signature
    );

    const signingKeyUpdated = sampleSigningKey();
    const nightKeyUpdated = signatureVerifyingKey(signingKeyUpdated);
    const dustAddressUpdated = sampleDustSecretKey().publicKey;
    const allowFeePaymentUpdated = 0n;
    const signatureUpdated = new SignatureEnabled(signData(signingKeyUpdated, new Uint8Array(32)));

    dustRegistration.nightKey = nightKeyUpdated;
    dustRegistration.dustAddress = dustAddressUpdated;
    dustRegistration.allowFeePayment = allowFeePaymentUpdated;
    dustRegistration.signature = signatureUpdated;

    expect(dustRegistration.nightKey).toEqual(nightKeyUpdated);
    expect(dustRegistration.dustAddress).toEqual(dustAddressUpdated);
    expect(dustRegistration.allowFeePayment).toEqual(allowFeePaymentUpdated);
    expect(dustRegistration.signature.toString()).toEqual(signatureUpdated.toString());
  });

  /**
   * Test signatures should only have 'signature' or 'signature-erased' markers.
   *
   * @given A new DustRegistration instance
   * @when Checking all signatures
   * @then Should work fine only for 'signature' or 'signature-erased' markers
   * @and other should be falling back to 'signature-erased'
   */
  test('should accept only signature or signature-erased as signature marker', () => {
    const signingKey = sampleSigningKey();
    const nightKey = signatureVerifyingKey(signingKey);
    const dustAddress = sampleDustSecretKey().publicKey;

    const signatureEnabled = new SignatureEnabled(signData(signingKey, new Uint8Array(32)));
    const signatureErased = undefined;

    const dustRegistrationSignature = new DustRegistration(
      SignatureMarker.signature,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signatureEnabled
    );

    const dustRegistrationSignatureErased = new DustRegistration(
      SignatureMarker.signatureErased,
      nightKey,
      dustAddress,
      BALANCING_OVERHEAD,
      signatureErased
    );

    expect(dustRegistrationSignature).toBeDefined();
    expect(dustRegistrationSignatureErased).toBeDefined();
    expect(dustRegistrationSignatureErased.signature).toBeUndefined();
  });
});
