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

import {
  Intent,
  sampleIntentHash,
  sampleSigningKey,
  sampleUserAddress,
  type Signature,
  signatureVerifyingKey,
  signData,
  UnshieldedOffer,
  type UtxoOutput,
  type UtxoSpend
} from '@midnight-ntwrk/ledger';
import { getNewUnshieldedOffer, Random } from '@/test-objects';
import { BindingMarker, ProofMarker, SignatureKindMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Ledger API - UnshieldedOffer', () => {
  /**
   * Test creating unshielded offer with valid parameters.
   *
   * @given Valid UTXO spend, output, and signature
   * @when Creating a new UnshieldedOffer
   * @then Should create offer with correct properties
   */
  test('should create a new unshielded offer with valid parameters', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = signData(sampleSigningKey(), new Uint8Array(32));
    const utxoSpend: UtxoSpend = {
      value: 100n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };
    const utxoOutput: UtxoOutput = {
      value: 100n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const unshieldedOffer = UnshieldedOffer.new([utxoSpend], [utxoOutput], [signature]);

    expect(unshieldedOffer.signatures.at(0)).toEqual(signature);
    expect(unshieldedOffer.inputs.at(0)).toEqual(utxoSpend);
    expect(unshieldedOffer.inputs.length).toEqual(1);
    expect(unshieldedOffer.outputs.at(0)).toEqual(utxoOutput);
    expect(unshieldedOffer.outputs.length).toEqual(1);
    expect(unshieldedOffer.toString()).toMatch(/UnshieldedOffer.*/);
  });

  /**
   * Test creating empty unshielded offer.
   *
   * @given Empty arrays for inputs, outputs, and signatures
   * @when Creating a new UnshieldedOffer
   * @then Should create empty offer with zero-length arrays
   */
  test('should create an empty unshielded offer', () => {
    const unshieldedOffer = UnshieldedOffer.new([], [], []);

    expect(unshieldedOffer.signatures.length).toEqual(0);
    expect(unshieldedOffer.inputs.length).toEqual(0);
    expect(unshieldedOffer.outputs.length).toEqual(0);
  });

  /**
   * Test handling empty array of signatures.
   *
   * @given An existing unshielded offer
   * @when Adding empty array of signatures
   * @then Should maintain existing signatures without adding new ones
   */
  test('should handle adding empty array of signatures', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const unshieldedOffer = getNewUnshieldedOffer(intentHash, token, svk);

    const updated = unshieldedOffer.addSignatures([]);
    expect(updated.signatures.length).toEqual(1);
    expect(updated.signatures.at(0)).toBeDefined();
  });

  /**
   * Test adding multiple signatures to an offer.
   *
   * @given An unshielded offer with existing signatures
   * @when Adding multiple new signatures
   * @then Should append all new signatures to existing ones
   */
  test('should add signatures to an offer', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = signData(sampleSigningKey(), new Uint8Array(32));
    const utxoSpend: UtxoSpend = {
      value: 100n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };
    const utxoOutput: UtxoOutput = {
      value: 100n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const unshieldedOffer = UnshieldedOffer.new([utxoSpend], [utxoOutput], [signature, signature]);

    const signatures: Signature[] = Array(64).fill(signature);

    const updated = unshieldedOffer.addSignatures(signatures);
    expect(updated.signatures.length).toEqual(64 + 2);
  });

  /**
   * Test erasing all signatures from an offer.
   *
   * @given An unshielded offer with signatures
   * @when Erasing all signatures
   * @then Should return offer with empty signatures array
   */
  test('should erase all signatures from an offer', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const unshieldedOffer = getNewUnshieldedOffer(intentHash, token, svk);

    const erased = unshieldedOffer.eraseSignatures();
    expect(erased).toBeInstanceOf(UnshieldedOffer);
    expect(erased.signatures).toEqual([]);
  });

  /**
   * Test creating offer with multiple inputs and outputs.
   *
   * @given Multiple UTXO spends, outputs, and signatures
   * @when Creating UnshieldedOffer
   * @then Should handle multiple inputs and outputs correctly
   */
  test('should create an offer with multiple inputs and outputs', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature1 = signData(sampleSigningKey(), new Uint8Array(32));
    const signature2 = signData(sampleSigningKey(), new Uint8Array(32));

    const utxoSpend1: UtxoSpend = {
      value: 100n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };

    const utxoSpend2: UtxoSpend = {
      value: 50n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 1
    };

    const utxoOutput1: UtxoOutput = {
      value: 75n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const utxoOutput2: UtxoOutput = {
      value: 75n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const unshieldedOffer = UnshieldedOffer.new(
      [utxoSpend1, utxoSpend2],
      [utxoOutput1, utxoOutput2],
      [signature1, signature2]
    );

    expect(unshieldedOffer.signatures.length).toEqual(2);
    expect(unshieldedOffer.inputs.length).toEqual(2);
    expect(unshieldedOffer.outputs.length).toEqual(2);
    expect(unshieldedOffer.inputs).toContainEqual(utxoSpend1);
    expect(unshieldedOffer.inputs).toContainEqual(utxoSpend2);
    expect(unshieldedOffer.outputs).toContainEqual(utxoOutput1);
    expect(unshieldedOffer.outputs).toContainEqual(utxoOutput2);
  });

  /**
   * Test handling different token types in inputs and outputs.
   *
   * @given UTXO spend and output with different token types
   * @when Creating UnshieldedOffer
   * @then Should preserve different token types correctly
   */
  test('should handle different token types in inputs and outputs', () => {
    const intentHash = sampleIntentHash();
    const token1 = Random.unshieldedTokenType();
    const token2 = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = signData(sampleSigningKey(), new Uint8Array(32));

    const utxoSpend: UtxoSpend = {
      value: 100n,
      owner: svk,
      type: token1.raw,
      intentHash,
      outputNo: 0
    };

    const utxoOutput: UtxoOutput = {
      value: 100n,
      owner: sampleUserAddress(),
      type: token2.raw
    };

    const unshieldedOffer = UnshieldedOffer.new([utxoSpend], [utxoOutput], [signature]);

    expect(unshieldedOffer.inputs.at(0)?.type).toEqual(token1.raw);
    expect(unshieldedOffer.outputs.at(0)?.type).toEqual(token2.raw);
  });

  /**
   * Test appending new signatures to existing ones.
   *
   * @given An unshielded offer with existing signatures
   * @when Adding new signatures multiple times
   * @then Should append signatures in correct order
   */
  test('should append new signatures to existing ones', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature1 = signData(sampleSigningKey(), new Uint8Array(32));
    const signature2 = signData(sampleSigningKey(), new Uint8Array(32));

    const unshieldedOffer = UnshieldedOffer.new(
      [
        {
          value: 100n,
          owner: svk,
          type: token.raw,
          intentHash,
          outputNo: 0
        }
      ],
      [
        {
          value: 100n,
          owner: sampleUserAddress(),
          type: token.raw
        }
      ],
      [signature1]
    );
    const unshieldedOfferWithSignature = unshieldedOffer.addSignatures([signature1]);
    const updated = unshieldedOfferWithSignature.addSignatures([signature2]);

    expect(updated.signatures).toEqual([signature1, signature1, signature2]);
  });

  /**
   * Test handling zero value inputs and outputs.
   *
   * @given UTXO spend and output with zero values
   * @when Creating UnshieldedOffer
   * @then Should handle zero values correctly
   */
  test('should handle zero value inputs and outputs', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = signData(sampleSigningKey(), new Uint8Array(32));

    const utxoSpend: UtxoSpend = {
      value: 0n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };

    const utxoOutput: UtxoOutput = {
      value: 0n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const unshieldedOffer = UnshieldedOffer.new([utxoSpend], [utxoOutput], [signature]);

    expect(unshieldedOffer.inputs.at(0)?.value).toEqual(0n);
    expect(unshieldedOffer.outputs.at(0)?.value).toEqual(0n);
  });

  /**
   * Test handling mismatched array lengths.
   *
   * @given Arrays of different lengths for inputs, outputs, and signatures
   * @when Creating UnshieldedOffer
   * @then Should handle mismatched lengths gracefully
   */
  test('should handle mismatched inputs/outputs/signatures lengths', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = signData(sampleSigningKey(), new Uint8Array(32));

    const utxoSpend: UtxoSpend = {
      value: 100n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };

    const utxoOutput: UtxoOutput = {
      value: 100n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const unshieldedOffer = UnshieldedOffer.new([utxoSpend], [utxoOutput], [signature, signature]);

    expect(unshieldedOffer.signatures.length).toEqual(2);
    expect(unshieldedOffer.inputs.length).toEqual(1);
  });

  /**
   * Test string representation generation.
   *
   * @given A valid UnshieldedOffer with all components
   * @when Converting to string
   * @then Should return string matching UnshieldedOffer pattern
   */
  test('should generate a string representation with valid data', () => {
    const intentHash = sampleIntentHash();
    const token = Random.unshieldedTokenType();
    const svk = signatureVerifyingKey(sampleSigningKey());
    const signature = signData(sampleSigningKey(), new Uint8Array(32));

    const utxoSpend: UtxoSpend = {
      value: 100n,
      owner: svk,
      type: token.raw,
      intentHash,
      outputNo: 0
    };

    const utxoOutput: UtxoOutput = {
      value: 100n,
      owner: sampleUserAddress(),
      type: token.raw
    };

    const unshieldedOffer = UnshieldedOffer.new([utxoSpend], [utxoOutput], [signature]);

    expect(unshieldedOffer.toString()).toMatch(/UnshieldedOffer.*/);
  });

  describe('ECDSA signature kind', () => {
    /**
     * Test construction of an offer with an ECDSA-keyed UTXO input.
     *
     * @given An ecdsa signing key, its verifying key, and an ecdsa signature
     * @when Building an UnshieldedOffer with the verifying key as owner
     *   and the ecdsa signature attached
     * @then The owner and signature should round-trip through their getters
     *   with the 'ecdsa' tag intact
     */
    test('should build an offer whose owner is ECDSA and whose signature is ECDSA', () => {
      const intentHash = sampleIntentHash();
      const token = Random.unshieldedTokenType();
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const sig = signData(ecdsaSk, new Uint8Array(32));

      const offer = UnshieldedOffer.new(
        [
          {
            value: 100n,
            owner: ecdsaVk,
            type: token.raw,
            intentHash,
            outputNo: 0
          }
        ],
        [
          {
            value: 100n,
            owner: sampleUserAddress(),
            type: token.raw
          }
        ],
        [sig]
      );

      expect(offer.inputs.at(0)?.owner).toEqual(ecdsaVk);
      expect(offer.inputs.at(0)?.owner.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(offer.signatures.at(0)).toEqual(sig);
      expect(offer.signatures.at(0)?.tag).toEqual(SignatureKindMarker.ecdsa);
    });

    /**
     * Test that an offer with mixed-kind inputs preserves signature order.
     *
     * @given Two UTXO inputs — one Schnorr-owned, one ECDSA-owned — and a
     *   signature for each in declaration order
     * @when Building the UnshieldedOffer
     * @then The signatures and owner tags must appear in the same order via
     *   the getters
     */
    test('should preserve mixed schnorr + ecdsa signatures in declaration order', () => {
      const intentHash = sampleIntentHash();
      const token = Random.unshieldedTokenType();
      const schnorrSk = sampleSigningKey(SignatureKindMarker.schnorr);
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const schnorrVk = signatureVerifyingKey(schnorrSk);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const schnorrSig = signData(schnorrSk, new Uint8Array(32));
      const ecdsaSig = signData(ecdsaSk, new Uint8Array(32));

      const offer = UnshieldedOffer.new(
        [
          { value: 50n, owner: schnorrVk, type: token.raw, intentHash, outputNo: 0 },
          { value: 50n, owner: ecdsaVk, type: token.raw, intentHash, outputNo: 1 }
        ],
        [{ value: 100n, owner: sampleUserAddress(), type: token.raw }],
        [schnorrSig, ecdsaSig]
      );

      expect(offer.inputs.at(0)?.owner.tag).toEqual(SignatureKindMarker.schnorr);
      expect(offer.inputs.at(1)?.owner.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(offer.signatures.length).toEqual(2);
      expect(offer.signatures.at(0)?.tag).toEqual(SignatureKindMarker.schnorr);
      expect(offer.signatures.at(1)?.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(offer.signatures.at(0)).toEqual(schnorrSig);
      expect(offer.signatures.at(1)).toEqual(ecdsaSig);
    });

    /**
     * Test that addSignatures appends ECDSA signatures correctly.
     *
     * @given A signed offer with one ecdsa signature and a second ecdsa signature
     * @when Calling addSignatures with the second signature
     * @then The offer should contain both signatures in append order
     */
    test('should round-trip ECDSA signatures through addSignatures on a signed offer', () => {
      const intentHash = sampleIntentHash();
      const token = Random.unshieldedTokenType();
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const firstSig = signData(ecdsaSk, new Uint8Array(32).fill(1));
      const secondSig = signData(ecdsaSk, new Uint8Array(32).fill(2));

      const base = UnshieldedOffer.new(
        [{ value: 10n, owner: ecdsaVk, type: token.raw, intentHash, outputNo: 0 }],
        [{ value: 10n, owner: sampleUserAddress(), type: token.raw }],
        [firstSig]
      );

      const updated = base.addSignatures([secondSig]);

      expect(updated.signatures.length).toEqual(2);
      expect(updated.signatures.at(0)).toEqual(firstSig);
      expect(updated.signatures.at(1)).toEqual(secondSig);
    });

    /**
     * Pin the surprising behaviour documented in
     * ledger-wasm/src/unshielded.rs: the TS signature requires Signature[]
     * even on a signature-erased offer, but the rust side requires unit, so
     * the shim silently discards anything you hand it.
     *
     * @given An offer with a real ecdsa signature that's been erased, then
     *   re-given an ecdsa signature via addSignatures
     * @when Reading back the signatures via the getter
     * @then The slot should be `undefined` — the signature was discarded
     */
    test('addSignatures on signature-erased offer silently erases ECDSA signatures', () => {
      const intentHash = sampleIntentHash();
      const token = Random.unshieldedTokenType();
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);
      const ecdsaSig = signData(ecdsaSk, new Uint8Array(32));

      const signed = UnshieldedOffer.new(
        [{ value: 10n, owner: ecdsaVk, type: token.raw, intentHash, outputNo: 0 }],
        [{ value: 10n, owner: sampleUserAddress(), type: token.raw }],
        [ecdsaSig]
      );

      const erased = signed.eraseSignatures();
      const reAdded = erased.addSignatures([ecdsaSig]);

      // Getter returns undefined slots for the erased variant.
      expect(reAdded.signatures.length).toEqual(1);
      expect(reAdded.signatures.at(0)).toBeUndefined();
    });

    /**
     * UnshieldedOffer has no direct serialize() — round-trip it through its
     * containing Intent, which is the realistic wire scenario.
     *
     * @given An Intent carrying an ECDSA-owned, ECDSA-signed UnshieldedOffer
     * @when Serialising the Intent and deserialising it back
     * @then The decoded offer's owner and signature must be byte-equal to
     *   the originals
     */
    test('serialization round-trip via Intent preserves ECDSA owner and signature tags', () => {
      const ttl = new Date(Date.now() + 60 * 60 * 1000);
      const token = Random.unshieldedTokenType();
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);

      const intent = Intent.new(ttl);
      const intentHash = intent.intentHash(0);
      const sig = signData(ecdsaSk, intent.signatureData(0));

      intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
        [{ value: 100n, owner: ecdsaVk, type: token.raw, intentHash, outputNo: 0 }],
        [{ value: 100n, owner: sampleUserAddress(), type: token.raw }],
        [sig]
      );

      const wire = intent.serialize();
      const round = Intent.deserialize(SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding, wire);
      const roundOffer = round.guaranteedUnshieldedOffer!;

      expect(roundOffer.inputs.at(0)?.owner).toEqual(ecdsaVk);
      expect(roundOffer.signatures.at(0)).toEqual(sig);
    });

    /**
     * Same as above but with both signature kinds in a single offer.
     *
     * @given An Intent carrying an UnshieldedOffer with one schnorr- and
     *   one ecdsa-owned UTXO input, signed by the respective keys
     * @when Serialising the Intent and deserialising it back
     * @then Owner and signature tags must be preserved in original order
     */
    test('serialization round-trip via Intent preserves a mixed schnorr/ecdsa offer', () => {
      const ttl = new Date(Date.now() + 60 * 60 * 1000);
      const token = Random.unshieldedTokenType();
      const schnorrSk = sampleSigningKey(SignatureKindMarker.schnorr);
      const ecdsaSk = sampleSigningKey(SignatureKindMarker.ecdsa);
      const schnorrVk = signatureVerifyingKey(schnorrSk);
      const ecdsaVk = signatureVerifyingKey(ecdsaSk);

      const intent = Intent.new(ttl);
      const intentHash = intent.intentHash(0);
      const schnorrSig = signData(schnorrSk, intent.signatureData(0));
      const ecdsaSig = signData(ecdsaSk, intent.signatureData(0));

      intent.guaranteedUnshieldedOffer = UnshieldedOffer.new(
        [
          { value: 1n, owner: schnorrVk, type: token.raw, intentHash, outputNo: 0 },
          { value: 1n, owner: ecdsaVk, type: token.raw, intentHash, outputNo: 1 }
        ],
        [{ value: 2n, owner: sampleUserAddress(), type: token.raw }],
        [schnorrSig, ecdsaSig]
      );

      const wire = intent.serialize();
      const round = Intent.deserialize(SignatureMarker.signature, ProofMarker.preProof, BindingMarker.preBinding, wire);
      const roundOffer = round.guaranteedUnshieldedOffer!;

      expect(roundOffer.inputs.at(0)?.owner.tag).toEqual(SignatureKindMarker.schnorr);
      expect(roundOffer.inputs.at(1)?.owner.tag).toEqual(SignatureKindMarker.ecdsa);
      expect(roundOffer.signatures.at(0)?.tag).toEqual(SignatureKindMarker.schnorr);
      expect(roundOffer.signatures.at(1)?.tag).toEqual(SignatureKindMarker.ecdsa);
    });
  });
});
