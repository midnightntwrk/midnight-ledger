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
  ZswapOffer,
  ZswapOutput,
  Intent,
  ContractState,
  ContractDeploy,
  createShieldedCoinInfo,
  shieldedToken,
  MaintenanceUpdate,
  VerifierKeyRemove,
  ContractOperationVersion
} from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';
import { Static, type ShieldedTokenType } from '@/test-objects';
import '@/setup-proving';
import { assertSerializationSuccess, createValidZSwapInput } from '@/test-utils';
import { BindingMarker, ProofMarker, SignatureMarker } from '@/test/utils/Markers';

describe.concurrent('Ledger API - TransactionBig [@slow][@proving]', () => {
  /**
   * Test creating large transaction with 10 ZSwapInputs, 10 ZSwapOutputs, and single intent with 10 calls, 10 deploys, 10 maintenance updates.
   *
   * @given Large transaction structure with multiple inputs, outputs and complex single intent
   * @when Creating and proving transaction
   * @then Should successfully process all components and maintain transaction integrity
   */
  test(
    'should create transaction with 10 ZSwapInputs, 10 ZSwapOutputs and single intent with 10 calls/deploys/updates',
    async () => {
      const tokenType = shieldedToken() as ShieldedTokenType;

      // Create 10 ZSwapInputs
      const zswapInputs = [];
      for (let i = 0; i < 10; i++) {
        const { zswapInput } = createValidZSwapInput(BigInt(100 + i), tokenType.raw, i < 5 ? 0 : 1);
        zswapInputs.push(zswapInput);
      }

      // Create 10 ZSwapOutputs
      const zswapOutputs = [];
      for (let i = 0; i < 10; i++) {
        const coinInfo = createShieldedCoinInfo(tokenType.raw, BigInt(50 + i));
        const output = ZswapOutput.new(
          coinInfo,
          i < 5 ? 0 : 1, // Alternate between segments 0 and 1
          Static.coinPublicKey(),
          Static.encryptionPublicKey()
        );
        zswapOutputs.push(output);
      }

      // Create guaranteed offer with first 5 inputs and outputs
      let guaranteedOffer = ZswapOffer.fromInput(zswapInputs[0], tokenType.raw, 100n);
      for (let i = 1; i < 5; i++) {
        guaranteedOffer = guaranteedOffer.merge(ZswapOffer.fromInput(zswapInputs[i], tokenType.raw, 100n + BigInt(i)));
      }
      for (let i = 0; i < 5; i++) {
        guaranteedOffer = guaranteedOffer.merge(ZswapOffer.fromOutput(zswapOutputs[i], tokenType.raw, 50n + BigInt(i)));
      }

      // Create fallible offer with last 5 inputs and outputs
      let fallibleOffer = ZswapOffer.fromInput(zswapInputs[5], tokenType.raw, 105n);
      for (let i = 6; i < 10; i++) {
        fallibleOffer = fallibleOffer.merge(ZswapOffer.fromInput(zswapInputs[i], tokenType.raw, 100n + BigInt(i)));
      }
      for (let i = 5; i < 10; i++) {
        fallibleOffer = fallibleOffer.merge(ZswapOffer.fromOutput(zswapOutputs[i], tokenType.raw, 50n + BigInt(i)));
      }

      // Create single intent with 10 deploys, 10 maintenance updates
      let intent = Intent.new(Static.calcBlockTime(new Date(), 50));

      for (let deployIndex = 0; deployIndex < 10; deployIndex++) {
        const contractState = new ContractState();
        const contractDeploy = new ContractDeploy(contractState);
        intent = intent.addDeploy(contractDeploy);
      }

      for (let updateIndex = 0; updateIndex < 10; updateIndex++) {
        const maintenanceUpdate = new MaintenanceUpdate(
          Static.contractAddress(),
          [new VerifierKeyRemove(`operation_${updateIndex}`, new ContractOperationVersion('v2'))],
          BigInt(updateIndex)
        );
        intent = intent.addMaintenanceUpdate(maintenanceUpdate);
      }

      // TODO: Add contract calls (require to update prover to pass configuration to proving)

      const unprovenTransaction = Transaction.fromParts('local-test', guaranteedOffer, fallibleOffer, intent);

      const transaction = await prove(unprovenTransaction);

      expect(transaction.guaranteedOffer?.inputs).toHaveLength(5);
      expect(transaction.guaranteedOffer?.outputs).toHaveLength(5);
      expect(transaction.fallibleOffer?.get(1)?.inputs).toHaveLength(5);
      expect(transaction.fallibleOffer?.get(1)?.outputs).toHaveLength(5);
      expect(transaction.intents?.size).toEqual(1);

      const singleIntent = transaction.intents?.get(1);
      expect(singleIntent?.actions).toHaveLength(20);

      expect(transaction.identifiers()).toHaveLength(21); // 10 inputs + 10 outputs + 1 intent

      assertSerializationSuccess(transaction, SignatureMarker.signature, ProofMarker.proof, BindingMarker.preBinding);
    },
    15 * 60000
  );
});
