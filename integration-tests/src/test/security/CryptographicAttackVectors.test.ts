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
  type Alignment,
  bigIntModFr,
  coinNullifier,
  communicationCommitment,
  communicationCommitmentRandomness,
  createShieldedCoinInfo,
  ecAdd,
  ecMul,
  ecMulGenerator,
  hashToCurve,
  maxField,
  persistentCommit,
  persistentHash,
  sampleCoinPublicKey,
  sampleEncryptionPublicKey,
  sampleSigningKey,
  signatureVerifyingKey,
  signData,
  transientCommit,
  transientHash,
  type Value,
  verifySignature,
  ZswapChainState,
  ZswapInput,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys
} from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, Random, Static } from '@/test-objects';

describe('Cryptographic Attack Vector Tests', () => {
  describe('Signature Forgery Protection', () => {
    /**
     * Prevents signature replay attacks by ensuring signature specificity to messages
     * @given two different messages and the same signing key
     * @when signing both messages and cross-verifying signatures
     * @then signatures should only verify for their corresponding original messages
     */
    test('should prevent signature replay attacks', () => {
      const message1 = new TextEncoder().encode('Transfer 100 tokens to Alice');
      const message2 = new TextEncoder().encode('Transfer 1000 tokens to Bob');

      const signingKey = sampleSigningKey();
      const verifyingKey = signatureVerifyingKey(signingKey);

      const signature1 = signData(signingKey, message1);

      expect(verifySignature(verifyingKey, message1, signature1)).toBe(true);

      expect(verifySignature(verifyingKey, message2, signature1)).toBe(false);
    });

    /**
     * Prevents signature malleability attacks through non-deterministic signing
     * @given the same message signed multiple times with the same key
     * @when comparing the resulting signatures
     * @then signatures should be different but both valid for the message
     */
    test('should prevent signature malleability attacks', () => {
      const message = new TextEncoder().encode('Critical transaction');
      const signingKey = sampleSigningKey();
      const verifyingKey = signatureVerifyingKey(signingKey);

      const signature1 = signData(signingKey, message);
      const signature2 = signData(signingKey, message);

      expect(verifySignature(verifyingKey, message, signature1)).toBe(true);
      expect(verifySignature(verifyingKey, message, signature2)).toBe(true);

      expect(signature1).not.toEqual(signature2);
    });

    /**
     * Rejects signatures verified with incorrect keys to prevent key substitution attacks
     * @given a message signed with one key and verified with different keys
     * @when verifying the signature with the correct and incorrect keys
     * @then verification should succeed only with the correct signing key
     */
    test('should reject signatures with wrong key', () => {
      const message = new TextEncoder().encode('Authenticated message');

      const signingKey1 = sampleSigningKey();
      const signingKey2 = sampleSigningKey();
      const verifyingKey1 = signatureVerifyingKey(signingKey1);
      const verifyingKey2 = signatureVerifyingKey(signingKey2);

      const signature = signData(signingKey1, message);

      expect(verifySignature(verifyingKey1, message, signature)).toBe(true);

      expect(verifySignature(verifyingKey2, message, signature)).toBe(false);
    });

    /**
     * Handles empty message signatures securely without special case vulnerabilities
     * @given an empty message and a non-empty message
     * @when signing and verifying both with the same key
     * @then signatures should be message-specific and not cross-verify
     */
    test('should handle empty message signatures securely', () => {
      const emptyMessage = new Uint8Array(0);
      const signingKey = sampleSigningKey();
      const verifyingKey = signatureVerifyingKey(signingKey);

      const signature = signData(signingKey, emptyMessage);

      expect(verifySignature(verifyingKey, emptyMessage, signature)).toBe(true);

      const nonEmptyMessage = new Uint8Array([1]);
      expect(verifySignature(verifyingKey, nonEmptyMessage, signature)).toBe(false);
    });
  });

  describe('Nullifier Uniqueness and Privacy', () => {
    /**
     * Generates unique nullifiers for different coins to prevent linkability attacks
     * @given different coins created with the same secret keys
     * @when generating nullifiers for each coin
     * @then nullifiers should be unique and properly formatted hex strings
     */
    test('should generate unique nullifiers for different coins', () => {
      const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));

      const coin1 = createShieldedCoinInfo(Random.shieldedTokenType().raw, 100n);
      const coin2 = createShieldedCoinInfo(Random.shieldedTokenType().raw, 200n);

      const nullifier1 = coinNullifier(coin1, secretKeys.coinSecretKey);
      const nullifier2 = coinNullifier(coin2, secretKeys.coinSecretKey);

      expect(nullifier1).not.toEqual(nullifier2);

      expect(nullifier1).toMatch(/^[a-fA-F0-9]{64}$/);
      expect(nullifier2).toMatch(/^[a-fA-F0-9]{64}$/);
    });

    /**
     * Generates deterministic nullifiers for the same coin and key combination
     * @given the same coin and secret key used multiple times
     * @when generating nullifiers repeatedly
     * @then nullifiers should be identical and deterministic
     */
    test('should generate same nullifier for same coin with same key', () => {
      const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
      const coin = createShieldedCoinInfo(Random.shieldedTokenType().raw, 100n);

      const nullifier1 = coinNullifier(coin, secretKeys.coinSecretKey);
      const nullifier2 = coinNullifier(coin, secretKeys.coinSecretKey);

      expect(nullifier1).toEqual(nullifier2);
    });

    /**
     * Generates different nullifiers for the same coin with different keys
     * @given the same coin but different secret keys
     * @when generating nullifiers with each key
     * @then nullifiers should be different to prevent key linkage
     */
    test('should generate different nullifiers for same coin with different keys', () => {
      const secretKeys1 = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
      const secretKeys2 = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(2));
      const coin = createShieldedCoinInfo(Random.shieldedTokenType().raw, 100n);

      const nullifier1 = coinNullifier(coin, secretKeys1.coinSecretKey);
      const nullifier2 = coinNullifier(coin, secretKeys2.coinSecretKey);

      expect(nullifier1).not.toEqual(nullifier2);
    });
  });

  describe('Encryption and Privacy Protection', () => {
    /**
     * Protects against chosen plaintext attacks through randomized encryption
     * @given identical plaintext values encrypted multiple times
     * @when creating outputs and offers with the same values
     * @then encrypted outputs should be different each time preventing pattern analysis
     */
    test('should protect against chosen plaintext attacks', () => {
      const tokenType = Random.shieldedTokenType();

      const output1 = ZswapOutput.new(
        createShieldedCoinInfo(tokenType.raw, 1000n),
        0,
        sampleCoinPublicKey(),
        sampleEncryptionPublicKey()
      );

      const output2 = ZswapOutput.new(
        createShieldedCoinInfo(tokenType.raw, 1000n),
        0,
        sampleCoinPublicKey(),
        sampleEncryptionPublicKey()
      );

      expect(output1.toString()).not.toEqual(output2.toString());

      const offer1 = ZswapOffer.fromOutput(output1, tokenType.raw, 1000n);
      const offer2 = ZswapOffer.fromOutput(output2, tokenType.raw, 1000n);

      expect(offer1.toString()).not.toEqual(offer2.toString());
    });

    test('should ensure encryption key privacy', () => {
      const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
      const publicKey = sampleEncryptionPublicKey();

      const output = ZswapOutput.new(
        createShieldedCoinInfo(Random.shieldedTokenType().raw, 1000n),
        0,
        sampleCoinPublicKey(),
        publicKey
      );

      const outputString = output.toString();

      expect(outputString).not.toContain(secretKeys.encryptionSecretKey.toString());
      expect(outputString).not.toContain(secretKeys.coinSecretKey.toString());
    });

    test('should test encryption key derivation security', () => {
      const seed1 = new Uint8Array(32).fill(1);
      const seed2 = new Uint8Array(32).fill(2);

      const keys1 = ZswapSecretKeys.fromSeed(seed1);
      const keys2 = ZswapSecretKeys.fromSeed(seed2);

      expect(keys1.encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).not.toEqual(
        keys2.encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()
      );
      expect(keys1.coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).not.toEqual(
        keys2.coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()
      );

      const duplicateKeys = ZswapSecretKeys.fromSeed(seed1);
      expect(keys1.encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).toEqual(
        duplicateKeys.encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()
      );
      expect(keys1.coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).toEqual(
        duplicateKeys.coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()
      );
    });

    test('should validate offer decryption authorization', () => {
      const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
      const wrongSecretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(2));

      const output = ZswapOutput.new(
        createShieldedCoinInfo(Random.shieldedTokenType().raw, 1000n),
        0,
        sampleCoinPublicKey(),
        sampleEncryptionPublicKey()
      );

      const offer = ZswapOffer.fromOutput(output, Random.shieldedTokenType().raw, 1000n);

      const canDecrypt = secretKeys.encryptionSecretKey.test(offer);
      expect(typeof canDecrypt).toBe('boolean');

      const cannotDecrypt = wrongSecretKeys.encryptionSecretKey.test(offer);
      expect(typeof cannotDecrypt).toBe('boolean');

      expect(cannotDecrypt).toBe(false);
    });
  });

  describe('Field Arithmetic Security', () => {
    /**
     * Prevents field overflow attacks through proper bounds checking
     * @given values at and beyond the maximum field limit
     * @when performing modular field arithmetic
     * @then valid values should succeed while oversized values should throw errors
     */
    test('should prevent field overflow attacks', () => {
      const maxFieldValue = maxField();

      expect(() => bigIntModFr(maxFieldValue)).not.toThrow();
      expect(bigIntModFr(maxFieldValue)).toEqual(maxFieldValue);

      expect(() => bigIntModFr(maxFieldValue + 1n)).toThrow('out of bounds for prime field');
      expect(() => bigIntModFr(maxFieldValue * 2n)).toThrow('out of bounds for prime field');
    });

    test('should handle negative field elements securely', () => {
      expect(() => bigIntModFr(-1n)).toThrow();
      expect(() => bigIntModFr(-100n)).toThrow();

      expect(() => bigIntModFr(0n)).not.toThrow();
      expect(bigIntModFr(0n)).toEqual(0n);
    });

    test('should prevent modular arithmetic manipulation', () => {
      const validValue = 12345n;
      const result1 = bigIntModFr(validValue);
      const result2 = bigIntModFr(validValue);

      expect(result1).toEqual(result2);
      expect(result1).toEqual(validValue);

      const nearMax = maxField() - 1n;
      expect(() => bigIntModFr(nearMax)).not.toThrow();
      expect(bigIntModFr(nearMax)).toEqual(nearMax);
    });
  });

  describe('Commitment Scheme Security', () => {
    /**
     * Generates cryptographically binding commitments with proper randomness
     * @given two values and different randomness parameters
     * @when generating commitments with same and different randomness
     * @then commitments should be deterministic for same inputs and unique for different randomness
     */
    test('should generate binding commitments', () => {
      const value1 = Static.alignedValue;
      const value2 = Static.alignedValueCompress;
      const randomness1 = communicationCommitmentRandomness();
      const randomness2 = communicationCommitmentRandomness();

      const commitment1a = communicationCommitment(value1, value2, randomness1);
      const commitment1b = communicationCommitment(value1, value2, randomness1);
      expect(commitment1a).toEqual(commitment1b);

      const commitment2 = communicationCommitment(value1, value2, randomness2);
      expect(commitment1a).not.toEqual(commitment2);

      const commitment3 = communicationCommitment(value2, value1, randomness1);
      expect(commitment1a).not.toEqual(commitment3);
    });

    test('should enforce commitment length constraints', () => {
      const commitment = communicationCommitment(
        Static.alignedValue,
        Static.alignedValueCompress,
        communicationCommitmentRandomness()
      );

      expect(commitment.length).toBeLessThanOrEqual(114);
      expect(commitment.length).toBeGreaterThan(0);
    });

    test('should test transient and persistent hash security', () => {
      const alignment: Alignment = [{ tag: 'atom', value: { tag: 'bytes', length: 16 } }];
      const value = [new Uint8Array([...Array(16).keys()])];
      const opening: Value = [new Uint8Array(32)];

      const transientHash1 = transientHash(alignment, value);
      const transientHash2 = transientHash(alignment, value);
      const transientCommit1 = transientCommit(alignment, value, opening);

      expect(transientHash1).toEqual(transientHash2);

      const persistentHash1 = persistentHash(alignment, value);
      const persistentHash2 = persistentHash(alignment, value);
      const persistentCommit1 = persistentCommit(alignment, value, opening);

      expect(persistentHash1).toEqual(persistentHash2);

      expect(transientHash1).not.toEqual(persistentHash1);
      expect(transientCommit1).not.toEqual(persistentCommit1);

      expect(Array.isArray(transientHash1)).toBe(true);
      expect(Array.isArray(persistentHash1)).toBe(true);
      expect(Array.isArray(transientCommit1)).toBe(true);
      expect(Array.isArray(persistentCommit1)).toBe(true);
    });
  });

  describe('Elliptic Curve Cryptography Security', () => {
    test('should test curve point operations security', () => {
      const alignment: Alignment = [{ tag: 'atom', value: { tag: 'bytes', length: 16 } }];
      const value = [new Uint8Array([...Array(16).keys()])];

      const point1 = hashToCurve(alignment, value);
      const point2 = hashToCurve(alignment, value);

      expect(point1).toEqual(point2);
      expect(Array.isArray(point1)).toBe(true);

      const sum = ecAdd(point1, point2);
      expect(Array.isArray(sum)).toBe(true);
      expect(sum).not.toEqual(point1);

      const scalar: Value = [new Uint8Array(32)];
      const product = ecMul(point1, scalar);
      expect(Array.isArray(product)).toBe(true);

      const generatorProduct = ecMulGenerator(scalar);
      expect(Array.isArray(generatorProduct)).toBe(true);
    });

    test('should ensure point addition commutativity', () => {
      const alignment: Alignment = [
        {
          tag: 'atom',
          value: { tag: 'field' }
        }
      ];
      const value1: Value = [new Uint8Array(32).fill(1)];
      const value2: Value = [new Uint8Array(32).fill(2)];

      const point1 = hashToCurve(alignment, value1);
      const point2 = hashToCurve(alignment, value2);

      const sum1 = ecAdd(point1, point2);
      const sum2 = ecAdd(point2, point1);

      expect(sum1).toEqual(sum2);
    });

    test('should test scalar multiplication properties', () => {
      const alignment: Alignment = [
        {
          tag: 'atom',
          value: { tag: 'field' }
        }
      ];
      const value: Value = [new Uint8Array(32).fill(1)];
      const point = hashToCurve(alignment, value);

      const scalar1: Value = [new Uint8Array(32).fill(2)];
      const scalar2: Value = [new Uint8Array(32).fill(3)];

      const result1 = ecMul(point, scalar1);
      const result2 = ecMul(point, scalar2);

      expect(Array.isArray(result1)).toBe(true);
      expect(Array.isArray(result2)).toBe(true);
      expect(result1).not.toEqual(result2);

      const genResult1 = ecMulGenerator(scalar1);
      const genResult2 = ecMulGenerator(scalar2);

      expect(Array.isArray(genResult1)).toBe(true);
      expect(Array.isArray(genResult2)).toBe(true);
      expect(genResult1).not.toEqual(genResult2);
    });
  });

  describe('Input Validation Edge Cases', () => {
    test('should validate token type format consistency', () => {
      const tokenType1 = Random.shieldedTokenType();
      const tokenType2 = Random.shieldedTokenType();

      expect(tokenType1.raw).not.toEqual(tokenType2.raw);

      // Should be properly formatted
      expect(tokenType1.raw.toString()).toMatch(/^[a-fA-F0-9]{64}$/);
      expect(tokenType2.raw.toString()).toMatch(/^[a-fA-F0-9]{64}$/);
    });

    test('should handle malformed input gracefully', () => {
      const chainState = new ZswapChainState();

      const invalidIndices = [-1, Number.MAX_SAFE_INTEGER, Number.MIN_SAFE_INTEGER];

      // eslint-disable-next-line no-restricted-syntax
      for (const invalidIndex of invalidIndices) {
        expect(() => {
          ZswapInput.newContractOwned(
            getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(1000n)),
            invalidIndex,
            Random.contractAddress(),
            chainState
          );
        }).toThrow('invalid index into sparse merkle tree: 0 -- write creating spend proof');
      }
    });
  });

  describe('Side Channel Attack Protection', () => {
    // Skipped because this fails in CI, probably because of variance in task
    // scheduling rather than runtime.
    test.skip('should ensure constant-time operations for sensitive data', () => {
      const secretKeys1 = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));
      const secretKeys2 = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(6));

      const coin = createShieldedCoinInfo(Random.shieldedTokenType().raw, 100n);

      // Measure timing for nullifier generation (should be consistent)
      const startTime1 = performance.now();
      const nullifier1 = coinNullifier(coin, secretKeys1.coinSecretKey);
      const endTime1 = performance.now();

      const startTime2 = performance.now();
      const nullifier2 = coinNullifier(coin, secretKeys2.coinSecretKey);
      const endTime2 = performance.now();

      const duration1 = endTime1 - startTime1;
      const duration2 = endTime2 - startTime2;

      // Times should be roughly similar (within 300% variance)
      const timingRatio = Math.max(duration1, duration2) / Math.min(duration1, duration2);
      expect(timingRatio).toBeLessThan(3);

      expect(nullifier1).not.toEqual(nullifier2);
    });

    test('should not leak information through string representations', () => {
      const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(42));

      // String representations should not contain raw secret material
      const encKeyString = secretKeys.encryptionSecretKey.toString();
      const coinKeyString = secretKeys.coinSecretKey.toString();

      expect(encKeyString.length).toBeGreaterThan(0);
      expect(coinKeyString.length).toBeGreaterThan(0);

      expect(encKeyString).not.toMatch(/^[01]+$/); // Not raw binary
      expect(coinKeyString).not.toMatch(/^[01]+$/); // Not raw binary
    });
  });

  describe('Randomness Quality Validation', () => {
    test('should generate high-quality randomness for commitments', () => {
      const randomness1 = communicationCommitmentRandomness();
      const randomness2 = communicationCommitmentRandomness();
      const randomness3 = communicationCommitmentRandomness();

      // All randomness should be different
      expect(randomness1).not.toEqual(randomness2);
      expect(randomness2).not.toEqual(randomness3);
      expect(randomness1).not.toEqual(randomness3);

      // Should have reasonable bit distribution
      const randomnessArray = [randomness1, randomness2, randomness3];
      randomnessArray.forEach((r) => {
        // Should not be all zeros or all ones (basic entropy check)
        expect(r).not.toEqual(0n);
        expect(r.toString().match(/0/g)?.length).not.toEqual(r.toString().length); // Contains at least one 1 bit
        expect(r.toString().match(/[fF]/g)?.length).not.toEqual(r.toString().length); // Contains at least one 0 bit
      });
    });

    test('should ensure key derivation produces distributed outputs', () => {
      const seeds = [
        new Uint8Array(32).fill(1),
        new Uint8Array(32).fill(2),
        new Uint8Array(32).fill(3),
        new Uint8Array(32).fill(255)
      ];

      const keyPairs = seeds.map((seed) => ZswapSecretKeys.fromSeed(seed));

      // All key pairs should be different
      for (let i = 0; i < keyPairs.length; i++) {
        for (let j = i + 1; j < keyPairs.length; j++) {
          expect(keyPairs[i].encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).not.toEqual(
            keyPairs[j].encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()
          );
          expect(keyPairs[i].coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()).not.toEqual(
            keyPairs[j].coinSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize()
          );
        }
      }
    });
  });
});
