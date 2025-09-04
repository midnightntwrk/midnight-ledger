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

import { Static, getNewUnshieldedOffer } from '@/test-objects';
import { assertSerializationSuccess, corruptSignature } from '@/test-utils';
import type { ContractDeploy } from '@midnight-ntwrk/ledger';
import {
  LedgerState,
  WellFormedStrictness,
  ZswapChainState,
  Transaction,
  Intent,
  UnshieldedOffer
} from '@midnight-ntwrk/ledger';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe('Ledger API - WellFormedStrictness', () => {
  let date: Date;
  let ledgerState: LedgerState;

  beforeEach(() => {
    date = new Date();
    const zSwapChainState = new ZswapChainState();
    ledgerState = new LedgerState('local-test', zSwapChainState);
  });

  describe('verifySignatures', () => {
    test('should pass when verifySignatures is false with invalid signature', () => {
      const unshieldedOffer = getNewUnshieldedOffer();
      const corruptedSignature = corruptSignature(unshieldedOffer.signatures[0]);
      const invalidSignatureOffer = UnshieldedOffer.new(unshieldedOffer.inputs, unshieldedOffer.outputs, [
        corruptedSignature
      ]);

      const intent = Intent.new(Static.calcBlockTime(date, 50));
      intent.guaranteedUnshieldedOffer = invalidSignatureOffer;
      const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);

      const strictness = new WellFormedStrictness();
      strictness.verifyContractProofs = false;
      strictness.enforceBalancing = false;
      strictness.verifyNativeProofs = false;
      strictness.enforceLimits = false;
      strictness.verifySignatures = false; // Should pass with invalid signature

      expect(() => transaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).not.toThrow();
      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });

    test('should pass when verifySignatures is true with valid signature', () => {
      const transaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();

      const strictness = new WellFormedStrictness();
      strictness.verifyContractProofs = false;
      strictness.enforceBalancing = false;
      strictness.verifyNativeProofs = false;
      strictness.enforceLimits = false;
      strictness.verifySignatures = true;

      expect(() => transaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).not.toThrow();
      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });
  });

  describe('enforceLimits', () => {
    test('should pass when enforceLimits is false with large transaction', () => {
      // Create a transaction that might exceed limits by adding many contract deploys
      const intent = Intent.new(Static.calcBlockTime(date, 50));

      // Add multiple contract deploys to make the transaction large

      for (let i = 0; i < 100; i++) {
        intent.addDeploy(
          Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls().intents!.get(1)!
            .actions[0] as ContractDeploy
        );
      }

      const transaction = Transaction.fromParts('local-test', undefined, undefined, intent);

      const strictness = new WellFormedStrictness();
      strictness.verifyContractProofs = false;
      strictness.enforceBalancing = false;
      strictness.verifyNativeProofs = false;
      strictness.enforceLimits = false; // Should pass even if large
      strictness.verifySignatures = false;

      expect(() => transaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).not.toThrow();
      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });

    test('should enforce limits when enforceLimits is true', () => {
      const transaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();

      const strictness = new WellFormedStrictness();
      strictness.verifyContractProofs = false;
      strictness.enforceBalancing = false;
      strictness.verifyNativeProofs = false;
      strictness.enforceLimits = true; // Should enforce limits
      strictness.verifySignatures = false;

      // This should not throw for a normal-sized transaction
      expect(() => transaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).not.toThrow();
      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });

    test('should pass when enforceLimits is false', () => {
      const transaction = Static.unprovenTransactionGuaranteedAndFallibleAndContractCalls();

      const strictness = new WellFormedStrictness();
      strictness.verifyContractProofs = false;
      strictness.enforceBalancing = false;
      strictness.verifyNativeProofs = false;
      strictness.enforceLimits = false; // Should not enforce limits
      strictness.verifySignatures = false;

      expect(() => transaction.wellFormed(ledgerState, strictness, new Date(+date - 15 * 1000))).not.toThrow();
      assertSerializationSuccess(
        transaction,
        SignatureMarker.signature,
        ProofMarker.preProof,
        BindingMarker.preBinding
      );
    });
  });
});
