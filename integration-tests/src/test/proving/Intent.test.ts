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

import '@/setup-proving';
import { ContractDeploy, ContractState, Intent, Transaction } from '@midnight-ntwrk/ledger';
import { prove } from '@/proof-provider';

describe.concurrent('Ledger API - Intent [@slow][@proving]', () => {
  /**
   * Test proof erasure from proven intents.
   *
   * @given A proven transaction with contract deploy intent
   * @when Erasing proofs from transaction intents
   * @then Should maintain string representation after proof erasure
   */
  test('should erase proofs from intents correctly', async () => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);

    const intent = Intent.new(new Date());
    const updated = intent.addDeploy(contractDeploy);

    const unprovenTransaction = Transaction.fromParts('local-test', undefined, undefined, updated);

    const transaction = await prove(unprovenTransaction);

    transaction.intents?.forEach((txIntent) => {
      const noProof = txIntent.eraseProofs();
      expect(noProof.toString()).toEqual(txIntent.toString());
    });
  });
});
