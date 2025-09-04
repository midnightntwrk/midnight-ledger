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
  ContractDeploy,
  ContractOperation,
  ContractState,
  EncryptionSecretKey,
  Intent,
  StateBoundedMerkleTree,
  Transaction,
  ZswapChainState,
  ZswapOutput,
  ZswapSecretKeys,
  createShieldedCoinInfo,
  sampleCoinPublicKey,
  sampleEncryptionPublicKey,
  bigIntModFr,
  maxField,
  ZswapInput
} from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, Random, Static } from '@/test-objects';
import { assertSerializationSuccess } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Security Boundary Validation Tests', () => {
  describe('Input Validation and Bounds Checking', () => {
    /**
     * Rejects zero coin values in specific security contexts to prevent economic attacks
     * @given a contract-owned zswap input with zero value coin
     * @when attempting to create the input with zero value
     * @then the system should throw an error rejecting the zero value
     */
    test('should reject zero coin values in specific contexts', () => {
      expect(() => {
        ZswapInput.newContractOwned(
          getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(0n)),
          0,
          Random.contractAddress(),
          new ZswapChainState()
        );
      }).toThrow();
    });

    /**
     * Rejects extremely large coin values that exceed cryptographic field limits
     * @given a coin value exceeding the maximum field value
     * @when attempting to create shielded coin info with oversized value
     * @then the system should throw a bounds error preventing overflow
     */
    test('should reject extremely large coin values that exceed field limits', () => {
      const maxFieldValue = maxField();

      expect(() => {
        createShieldedCoinInfo(Random.shieldedTokenType().raw, maxFieldValue + 1n);
      }).toThrow("Couldn't deserialize u128 from a BigInt outside u128::MIN..u128::MAX bounds");
    });

    /**
     * Handles field arithmetic overflow protection to prevent cryptographic attacks
     * @given values at and beyond the maximum field boundary
     * @when performing modular field arithmetic operations
     * @then valid values should succeed while invalid values should throw errors
     */
    test('should handle field arithmetic overflow protection', () => {
      const maxFieldValue = maxField();

      expect(() => bigIntModFr(maxFieldValue)).not.toThrow();

      expect(() => bigIntModFr(maxFieldValue + 1n)).toThrow('out of bounds for prime field');

      expect(() => bigIntModFr(-1n)).toThrow();
    });

    /**
     * Limits merkle tree height to prevent denial-of-service attacks
     * @given different merkle tree height values including extreme cases
     * @when creating bounded merkle trees with various heights
     * @then height should be capped at maximum safe value of 255
     */
    test('should limit merkle tree height to prevent DoS', () => {
      const validTree = new StateBoundedMerkleTree(255);
      expect(validTree.height).toEqual(255);

      const invalidTree = new StateBoundedMerkleTree(256);
      expect(invalidTree.height).toEqual(0);

      const extremeTree = new StateBoundedMerkleTree(Number.MAX_SAFE_INTEGER);
      expect(extremeTree.height).toEqual(255);
    });
  });

  describe('Cryptographic Key Security', () => {
    /**
     * Handles malformed key data gracefully to prevent system compromise
     * @given truncated and corrupted encryption key serialization data
     * @when attempting to deserialize invalid key data
     * @then system should throw appropriate errors without crashing
     */
    test('should handle malformed key data gracefully', () => {
      const validSeed = new Uint8Array(32).fill(1);
      const secretKeys = ZswapSecretKeys.fromSeed(validSeed);
      const validSerialized = secretKeys.encryptionSecretKey.yesIKnowTheSecurityImplicationsOfThis_serialize();

      const truncated = validSerialized.slice(0, 10);
      expect(() => {
        EncryptionSecretKey.deserialize(truncated);
      }).toThrow('Unable to deserialize EncryptionSecretKey');
    });
  });

  describe('Transaction Integrity Validation', () => {
    /**
     * Prevents modification of bound transactions to ensure cryptographic integrity
     * @given a transaction that has been bound with cryptographic commitments
     * @when attempting to modify offer or intent components
     * @then all modification attempts should throw binding violation errors
     */
    test('should prevent modification of bound transactions', () => {
      const transaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
      const boundTransaction = transaction.bind();

      expect(() => {
        boundTransaction.guaranteedOffer = Static.unprovenOfferFromOutput();
      }).toThrow('Transaction is already bound');

      expect(() => {
        const newFallibleOffer = new Map();
        newFallibleOffer.set(1, Static.unprovenOfferFromOutput(1));
        boundTransaction.fallibleOffer = newFallibleOffer;
      }).toThrow('Transaction is already bound');

      expect(() => {
        const newIntents = new Map();
        newIntents.set(1, Intent.new(new Date()));
        boundTransaction.intents = newIntents;
      }).toThrow('Transaction is already bound');
    });

    /**
     * Rejects transactions with conflicting coin identifiers to prevent double spending
     * @given transactions with overlapping coin sets
     * @when attempting to merge transactions with conflicting coins
     * @then the system should reject the merge with a non-disjoint error
     */
    test('should reject transactions with conflicting coin identifiers', () => {
      const transaction1 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

      expect(() => {
        transaction1.merge(transaction1);
      }).toThrow('attempted to merge non-disjoint coin sets');
    });
  });

  describe('Contract Security Boundaries', () => {
    /**
     * Limits contract operation complexity to prevent resource exhaustion attacks
     * @given a contract state with maximum allowed operations
     * @when deploying the contract with 255 operations
     * @then deployment should succeed and generate a valid contract address
     */
    test('should limit contract operation complexity', () => {
      const contractState = new ContractState();

      const maxOperations = 255;

      for (let i = 0; i < maxOperations; i++) {
        contractState.setOperation(`op${i}`, new ContractOperation());
      }

      expect(() => {
        // eslint-disable-next-line no-new
        new ContractDeploy(contractState);
      }).not.toThrow();

      const deploy = new ContractDeploy(contractState);
      expect(deploy.address).toMatch(/[a-fA-F0-9]{64}/);
    });

    /**
     * Validates contract address format to ensure proper cryptographic properties
     * @given deployed contracts from the same state
     * @when examining generated contract addresses
     * @then addresses should be 64-character hex strings and unique per deployment
     */
    test('should validate contract address format', () => {
      const contractState = new ContractState();
      const deploy = new ContractDeploy(contractState);

      expect(deploy.address.length).toEqual(64);
      expect(deploy.address).toMatch(/^[a-fA-F0-9]{64}$/);

      const deploy2 = new ContractDeploy(contractState);
      expect(deploy.address).not.toEqual(deploy2.address);
    });

    /**
     * Protects against contract state tampering through immutable deployment copies
     * @given a contract state modified after deployment
     * @when comparing deployed state with modified original state
     * @then deployed state should remain unchanged while original reflects modifications
     */
    test('should protect against contract state tampering', () => {
      const contractState = new ContractState();
      contractState.setOperation('test', new ContractOperation());

      const deploy = new ContractDeploy(contractState);
      expect(deploy.initialState).not.toBe(contractState);

      expect(deploy.initialState.serialize()).toEqual(contractState.serialize());

      contractState.setOperation('malicious', new ContractOperation());
      expect(deploy.initialState.serialize()).not.toEqual(contractState.serialize());
    });
  });

  describe('Serialization Security', () => {
    /**
     * Maintains serialization determinism for security-critical objects
     * @given a transaction with proven offers
     * @when serializing the transaction multiple times
     * @then serialization output should be identical and succeed validation
     */
    test('should maintain serialization determinism for security-critical objects', () => {
      const transaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

      const serialized1 = transaction.toString();
      const serialized2 = transaction.toString();
      expect(serialized1).toEqual(serialized2);

      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });
  });

  describe('Randomness and Nonce Security', () => {
    /**
     * Generates unique binding randomness for each transaction to prevent replay attacks
     * @given multiple transactions created independently
     * @when comparing their binding randomness values
     * @then each transaction should have unique, positive randomness values
     */
    test('should generate unique binding randomness for each transaction', () => {
      const transaction1 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
      const transaction2 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

      expect(transaction1.bindingRandomness).not.toEqual(transaction2.bindingRandomness);

      expect(transaction1.bindingRandomness).toBeGreaterThan(0n);
      expect(transaction2.bindingRandomness).toBeGreaterThan(0n);
    });

    /**
     * Generates unique identifiers for transaction components to prevent collisions
     * @given a transaction with multiple components
     * @when extracting component identifiers
     * @then all identifiers should be unique within the transaction
     */
    test('should generate unique identifiers for transaction components', () => {
      const transaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
      const identifiers = transaction.identifiers();

      expect(identifiers.length).toBeGreaterThan(0);

      // All identifiers should be unique
      const uniqueIdentifiers = new Set(identifiers);
      expect(uniqueIdentifiers.size).toEqual(identifiers.length);
    });
  });

  describe('Privacy and Information Leakage Protection', () => {
    /**
     * Ensures error messages do not leak sensitive cryptographic key information
     * @given an error condition triggered during coin creation
     * @when examining the error message content
     * @then message should not contain any secret key material
     */
    test('should not leak sensitive information in error messages', () => {
      const secretKeys = ZswapSecretKeys.fromSeed(new Uint8Array(32).fill(1));

      try {
        createShieldedCoinInfo(Random.shieldedTokenType().raw, -1n);
      } catch (error) {
        const errorMessage = error instanceof Error ? error.message : String(error);

        // Error message should not contain sensitive key material
        expect(errorMessage).not.toContain(secretKeys.coinSecretKey.toString());
        expect(errorMessage).not.toContain(secretKeys.encryptionSecretKey.toString());
      }
    });

    /**
     * Protects offer privacy through encryption to prevent plaintext leakage
     * @given a zswap output with specific token type and amount
     * @when converting the output to string representation
     * @then output should not reveal plaintext amount or token type information
     */
    test('should protect offer privacy through encryption', () => {
      const tokenType = Random.shieldedTokenType();
      const amount = 1000n;

      const output = ZswapOutput.new(
        createShieldedCoinInfo(tokenType.raw, amount),
        0,
        sampleCoinPublicKey(),
        sampleEncryptionPublicKey()
      );

      // Output should not reveal a plaintext amount or token type
      const outputString = output.toString();
      expect(outputString).not.toContain(amount.toString());
      expect(outputString).not.toContain(tokenType.toString());
    });
  });

  describe('Resource Exhaustion Protection', () => {
    /**
     * Handles large but valid merkle tree operations efficiently under load
     * @given a merkle tree with 32 height and multiple update operations
     * @when performing sequential tree updates within time constraints
     * @then operations should complete efficiently and maintain tree integrity
     */
    test('should handle large but valid merkle tree operations efficiently', () => {
      const size = 32;
      let tree = new StateBoundedMerkleTree(size);

      const startTime = Date.now();

      for (let i = 0; i < size; i++) {
        tree = tree.update(BigInt(i), {
          value: [new Uint8Array()],
          alignment: [
            {
              tag: 'atom',
              value: { tag: 'field' }
            }
          ]
        });
      }

      const endTime = Date.now();
      const duration = endTime - startTime;

      // Tree should still function correctly
      expect(tree.height).toEqual(size);
      const root = tree.rehash().root()!;
      expect(root).toBeDefined();
      expect(Array.isArray(root.value)).toBe(true);

      expect(duration).toBeLessThan(size * 50); // 50ms per operation is reasonable
    });

    /**
     * Limits transaction component counts to prevent denial-of-service attacks
     * @given a transaction with 255 contract deployments in a single intent
     * @when creating and serializing the complex transaction
     * @then transaction should be successfully processed and serialized
     */
    test('should limit transaction component counts to prevent DoS', () => {
      const guaranteedOffer = Static.unprovenOfferFromOutput();
      const fallibleOffer = Static.unprovenOfferFromOutput(1);

      let intent = Intent.new(new Date());

      for (let i = 0; i < 255; i++) {
        const contractState = new ContractState();
        const contractDeploy = new ContractDeploy(contractState);
        intent = intent.addDeploy(contractDeploy);
      }
      const transaction = Transaction.fromParts('local-test', guaranteedOffer, fallibleOffer, intent);

      expect(transaction.guaranteedOffer).toBeDefined();
      expect(transaction.fallibleOffer).toBeDefined();
      expect(transaction.intents).toBeDefined();

      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });
  });
});
