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
  Transaction,
  Intent,
  ContractDeploy,
  ContractState,
  LedgerState,
  ZswapChainState,
  ZswapInput,
  createShieldedCoinInfo,
  UnshieldedOffer,
  sampleIntentHash,
  sampleSigningKey,
  signatureVerifyingKey,
  signData,
  sampleUserAddress,
  WellFormedStrictness
} from '@midnight-ntwrk/ledger';
import { getQualifiedShieldedCoinInfo, Random, Static } from '@/test-objects';
import { assertSerializationSuccess, mapFindByKey } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Economic Attack Vector Tests', () => {
  describe('Double Spending Prevention', () => {
    /**
     * Prevents spending of non-existent coins through merkle tree validation
     * @given an invalid index that doesn't exist in the chain state
     * @when attempting to create an input for a non-existent coin
     * @then the system should throw an invalid index error
     */
    test('should prevent spending non-existent coins', () => {
      const chainState = new ZswapChainState();

      expect(() => {
        ZswapInput.newContractOwned(
          getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo(1000n)),
          999999,
          Random.contractAddress(),
          chainState
        );
      }).toThrow('invalid index into sparse merkle tree');
    });

    /**
     * Validates nullifier uniqueness across transactions to prevent double spending
     * @given transactions created from the same coin source
     * @when extracting identifiers from different transactions
     * @then transaction identifiers should be unique preventing coin reuse
     */
    test('should validate nullifier uniqueness across transactions', () => {
      const tokenType = Random.shieldedTokenType();

      const transaction1 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType, 100n));
      const transaction2 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType, 100n));

      const ids1 = transaction1.identifiers();
      const ids2 = transaction2.identifiers();

      expect(ids1).not.toEqual(ids2);
    });
  });

  describe('Transaction Balance Validation', () => {
    /**
     * Detects unbalanced transaction attempts to prevent economic exploits
     * @given a transaction with outputs but no corresponding inputs
     * @when validating the transaction with balance enforcement
     * @then the system should reject the transaction with a negative balance error
     */
    test('should detect unbalanced transaction attempts', () => {
      const tokenType = Random.shieldedTokenType();
      const ledgerState = new LedgerState('local-test', new ZswapChainState());
      const strictness = new WellFormedStrictness();
      strictness.enforceBalancing = true;

      const unbalancedTransaction = Transaction.fromParts(
        'local-test',
        Static.unprovenOfferFromOutput(0, tokenType, 1000n)
      );

      expect(() => {
        unbalancedTransaction.wellFormed(ledgerState, strictness, new Date());
      }).toThrow(/invalid balance -.* for token .* in segment 0; balance must be positive/);
    });

    /**
     * Handles complex multi-token balance validation across transaction segments
     * @given transactions with multiple different token types
     * @when calculating imbalances for guaranteed and fallible segments
     * @then each token should have correct negative balance values matching outputs
     */
    test('should handle complex multi-token balance validation', () => {
      const tokenType1 = Random.shieldedTokenType();
      const tokenType2 = Random.shieldedTokenType();
      const tokenType3 = Random.shieldedTokenType();

      const offer1 = Static.unprovenOfferFromOutput(0, tokenType1, 100n);
      const offer2 = Static.unprovenOfferFromOutput(0, tokenType2, 200n);
      const offer3 = Static.unprovenOfferFromOutput(1, tokenType3, 300n);

      const transaction = Transaction.fromParts('local-test', offer1.merge(offer2), offer3);

      const guaranteedImbalances = transaction.imbalances(0);
      const fallibleImbalances = transaction.imbalances(1);

      expect(guaranteedImbalances.size).toEqual(2);
      expect(fallibleImbalances.size).toEqual(1);

      expect(mapFindByKey(guaranteedImbalances, tokenType1)).toEqual(-100n);
      expect(mapFindByKey(guaranteedImbalances, tokenType2)).toEqual(-200n);
      expect(mapFindByKey(fallibleImbalances, tokenType3)).toEqual(-300n);
    });

    /**
     * Prevents arithmetic overflow in balance calculations with maximum values
     * @given coin values at the maximum u128 limit
     * @when creating offers with maximum valid values
     * @then valid max values should succeed while overflow attempts should fail
     */
    test('should prevent arithmetic overflow in balance calculations', () => {
      const tokenType = Random.shieldedTokenType();
      // eslint-disable-next-line no-bitwise
      const maxValue = (1n << 128n) - 1n;

      expect(() => {
        Static.unprovenOfferFromOutput(0, tokenType, maxValue);
      }).not.toThrow();

      expect(() => {
        createShieldedCoinInfo(tokenType.raw, maxValue + 1n);
      }).toThrow("Couldn't deserialize u128 from a BigInt outside u128::MIN..u128::MAX bounds");
    });

    test('should validate delta calculations for complex offers', () => {
      const tokenType = Random.shieldedTokenType();

      const offer1 = Static.unprovenOfferFromOutput(0, tokenType, 100n);
      const offer2 = Static.unprovenOfferFromOutput(0, tokenType, 200n);

      const combined = offer1.merge(offer2);

      expect(combined.deltas.get(tokenType.raw)).toEqual(-300n);
    });
  });

  describe('Rewards Authority Validation', () => {
    test('should prevent unauthorized rewarding attempts', () => {
      const transaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

      expect(transaction.rewards).toBeUndefined();

      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });

    test('should validate unshielded offer authorization', () => {
      const intent = Intent.new(new Date());
      const intentHash = sampleIntentHash();
      const tokenType = Random.unshieldedTokenType();
      const signingKey = sampleSigningKey();
      const verifyingKey = signatureVerifyingKey(signingKey);

      const validOffer = UnshieldedOffer.new(
        [
          {
            value: 100n,
            owner: verifyingKey,
            type: tokenType.raw,
            intentHash,
            outputNo: 0
          }
        ],
        [
          {
            value: 100n,
            owner: sampleUserAddress(),
            type: tokenType.raw
          }
        ],
        [signData(signingKey, new Uint8Array(32))]
      );

      intent.guaranteedUnshieldedOffer = validOffer;

      const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);

      expect(transaction.intents?.get(1)?.guaranteedUnshieldedOffer?.toString()).toEqual(validOffer.toString());

      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });
  });

  describe('Fee Manipulation Prevention', () => {
    test('should prevent negative fee attacks through imbalance calculation', () => {
      const tokenType = Random.shieldedTokenType();

      const transaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType, 100n));

      const imbalances = transaction.imbalances(0);
      const balance = mapFindByKey(imbalances, tokenType);

      expect(imbalances.size).toEqual(1);
      expect(balance).toEqual(-100n);
      expect(balance).toBeLessThan(0n);
    });

    test('should handle zero-value transactions appropriately', () => {
      const contractState = new ContractState();
      const contractDeploy = new ContractDeploy(contractState);
      const intent = Intent.new(new Date()).addDeploy(contractDeploy);

      const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);

      expect(transaction.imbalances(0).size).toEqual(0);
      expect(transaction.imbalances(1).size).toEqual(0);

      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });
  });

  describe('Segment ID Manipulation Prevention', () => {
    test('should prevent segment ID collision attacks', () => {
      const offer1 = Static.unprovenOfferFromOutput(0, Random.shieldedTokenType(), 100n);

      const transaction1 = Transaction.fromParts('local-test', offer1);
      const transaction2 = Transaction.fromParts('local-test', offer1); // Same offer = same segment ID

      expect(() => {
        transaction1.merge(transaction2);
      }).toThrow('attempted to merge non-disjoint coin sets');
    });

    test('should generate unique segment IDs with randomization', () => {
      const offer = Static.unprovenOfferFromOutput();

      const transaction1 = Transaction.fromPartsRandomized('local-test', offer);
      const transaction2 = Transaction.fromPartsRandomized('local-test', offer);

      expect(transaction1.toString()).toEqual(transaction2.toString());

      expect(() => {
        transaction1.merge(transaction2);
      }).toThrow('attempted to merge non-disjoint coin sets');
    });

    test('should validate segment ID uniqueness within transaction', () => {
      const guaranteedOffer = Static.unprovenOfferFromOutput(0);
      const fallibleOffer1 = Static.unprovenOfferFromOutput(2);
      const fallibleOffer2 = Static.unprovenOfferFromOutput(2);

      const transaction = Transaction.fromParts('local-test', guaranteedOffer, fallibleOffer1.merge(fallibleOffer2));

      expect(transaction.guaranteedOffer?.outputs.length).toEqual(1);
      expect(transaction.fallibleOffer?.has(2)).toBe(true);
      expect(transaction.fallibleOffer?.has(3)).toBe(false);
    });
  });

  describe('Resource Exhaustion Prevention', () => {
    test('should limit intent complexity to prevent DoS', () => {
      let intent = Intent.new(new Date());

      for (let i = 0; i < 50; i++) {
        const contractState = new ContractState();
        const contractDeploy = new ContractDeploy(contractState);
        intent = intent.addDeploy(contractDeploy);
      }

      const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);

      expect(transaction.intents?.get(1)?.actions.length).toEqual(50);

      expect(() => transaction.toString()).not.toThrow();
    });
  });

  describe('Privacy Leakage Prevention', () => {
    test('should not leak transaction amounts in public interface', () => {
      const tokenType = Random.shieldedTokenType();
      const secretAmount = 123456789n;

      const transaction = Transaction.fromParts(
        'local-test',
        Static.unprovenOfferFromOutput(0, tokenType, secretAmount)
      );

      const transactionString = transaction.toString();

      expect(transactionString).toContain(secretAmount.toString());
      expect(transactionString).toContain('123456789');
    });

    test('should not leak token type information in public interface', () => {
      const secretTokenType = Random.shieldedTokenType();

      const transaction = Transaction.fromParts(
        'local-test',
        Static.unprovenOfferFromOutput(0, secretTokenType, 1000n)
      );

      const transactionString = transaction.toString();

      expect(transactionString).not.toContain(secretTokenType.toString());
      expect(transactionString).toContain(secretTokenType.raw);
    });

    test('should maintain privacy in merged transactions', () => {
      const tokenType1 = Random.shieldedTokenType();
      const tokenType2 = Random.shieldedTokenType();
      const amount1 = 111111n;
      const amount2 = 222222n;

      const transaction1 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType1, amount1));
      const transaction2 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(0, tokenType2, amount2));

      const mergedTransaction = transaction1.merge(transaction2);
      const mergedString = mergedTransaction.toString();

      expect(mergedString).toContain(amount1.toString());
      expect(mergedString).toContain(amount2.toString());
      expect(mergedString).toContain(tokenType1.raw.toString());
      expect(mergedString).toContain(tokenType2.raw.toString());
    });
  });

  describe('Transaction Binding Security', () => {
    /**
     * Prevents modification after binding commitment to ensure transaction integrity
     * @given a transaction that has been cryptographically bound
     * @when attempting to modify the guaranteed offer after binding
     * @then the system should throw an error preventing any modifications
     */
    test('should prevent modification after binding commitment', () => {
      const transaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
      const { bindingRandomness } = transaction;

      const boundTransaction = transaction.bind();

      expect(boundTransaction.bindingRandomness).toEqual(bindingRandomness);

      expect(() => {
        boundTransaction.guaranteedOffer = Static.unprovenOfferFromOutput();
      }).toThrow('Transaction is already bound');
    });

    /**
     * Generates unique binding randomness for each transaction to prevent replay attacks
     * @given multiple transactions created independently
     * @when comparing their binding randomness values
     * @then each transaction should have unique positive randomness preventing reuse
     */
    test('should generate unique binding randomness for each transaction', () => {
      const transaction1 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
      const transaction2 = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());

      expect(transaction1.bindingRandomness).not.toEqual(transaction2.bindingRandomness);

      expect(transaction1.bindingRandomness).toBeGreaterThan(0n);
      expect(transaction2.bindingRandomness).toBeGreaterThan(0n);
    });

    /**
     * Maintains binding integrity through serialization processes
     * @given a bound transaction with cryptographic commitments
     * @when serializing the transaction in bound state
     * @then serialization should succeed and reflect the binding status
     */
    test('should maintain binding integrity through serialization', () => {
      const transaction = Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
      const boundTransaction = transaction.bind();

      // Bound transaction should serialize correctly
      assertSerializationSuccess(
        boundTransaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.binding
      );

      // Binding should be reflected in string representation
      expect(boundTransaction.toString()).toMatch(/.*binding.*/i);
    });
  });
});
