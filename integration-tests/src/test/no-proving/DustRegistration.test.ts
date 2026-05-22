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

import { expect } from 'vitest';
import {
  DustRegistration,
  sampleDustSecretKey,
  sampleSigningKey,
  SignatureEnabled,
  signatureVerifyingKey,
  signData
} from '@midnight-ntwrk/ledger';
import { SignatureKindMarker, SignatureMarker } from '@/test/utils/Markers';
import { BALANCING_OVERHEAD } from '@/test-objects';
import { assertSerializationSuccess, corruptSignature } from '@/test-utils';

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

  describe('ECDSA signature kind', () => {
    /**
     * Test construction with an ECDSA-keyed night key.
     *
     * @given An ECDSA signing key and a signature over arbitrary data
     * @when Constructing a DustRegistration with the matching verifying key
     * @then All getters should return the inputs with the 'ecdsa' tag intact
     */
    test('constructs with an ECDSA night key and ECDSA-signed signature', () => {
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const nightKey = signatureVerifyingKey(ecdsaSk);
      const dustAddress = sampleDustSecretKey().publicKey;
      const sig = signData(ecdsaSk, new Uint8Array(32));

      const reg = new DustRegistration(
        SignatureMarker.signature,
        nightKey,
        dustAddress,
        BALANCING_OVERHEAD,
        new SignatureEnabled(sig)
      );

      expect(reg.nightKey).toEqual(nightKey);
      expect(reg.nightKey.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(reg.signature.value).toEqual(sig);
      expect(reg.signature.value.tag).toEqual(SignatureKindMarker.ecdsa);
    });

    /**
     * Test that the nightKey setter accepts an ECDSA verifying key.
     *
     * @given A DustRegistration initially built with a Schnorr night key
     * @when Reassigning nightKey to an ECDSA verifying key
     * @then Reading nightKey back should yield the new ECDSA key
     */
    test('setter assigns an ECDSA night key and reads back equal', () => {
      const initialSk = sampleSigningKey(SignatureKindMarker.schnorr);
      const initialVk = signatureVerifyingKey(initialSk);
      const dustAddress = sampleDustSecretKey().publicKey;
      const reg = new DustRegistration(
        SignatureMarker.signature,
        initialVk,
        dustAddress,
        BALANCING_OVERHEAD,
        new SignatureEnabled(signData(initialSk, new Uint8Array(32)))
      );

      const ecdsaVk = signatureVerifyingKey(sampleSigningKey(SignatureKindMarker.ecdsa));
      reg.nightKey = ecdsaVk;

      expect(reg.nightKey).toEqual(ecdsaVk);
      expect(reg.nightKey.tag).toEqual(SignatureKindMarker.ecdsa);
    });

    /**
     * Test wire-format round-trip preserves the ECDSA night key kind.
     *
     * @given A DustRegistration with an ECDSA night key and matching signature
     * @when Serialising and deserialising it
     * @then The decoded value should match the original
     */
    test('serialization round-trip preserves the ECDSA night key kind', () => {
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const nightKey = signatureVerifyingKey(ecdsaSk);
      const reg = new DustRegistration(
        SignatureMarker.signature,
        nightKey,
        sampleDustSecretKey().publicKey,
        BALANCING_OVERHEAD,
        new SignatureEnabled(signData(ecdsaSk, new Uint8Array(32)))
      );

      assertSerializationSuccess(reg, SignatureMarker.signature);
    });

    /**
     * Test that corruptSignature (post-PR fix) preserves the kind tag.
     *
     * @given A valid ECDSA signature
     * @when Mutating it via corruptSignature
     * @then The tag must remain 'ecdsa' and the value must change
     */
    test('corruptSignature preserves the ECDSA tag', () => {
      const sig = signData(sampleSigningKey(SignatureKindMarker.ecdsa), new Uint8Array(32));
      const corrupted = corruptSignature(sig);

      expect(corrupted.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(corrupted.value).not.toEqual(sig.value);
    });
  });
});
