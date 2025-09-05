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
});
