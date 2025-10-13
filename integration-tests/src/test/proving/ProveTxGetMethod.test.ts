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

import axios from 'axios';
import '@/setup-proving';
import { getProofServerUrl } from '@/proof-provider';

/**
 * Integration tests covering the proof server's /prove-tx endpoint.
 */
describe.concurrent('Proof Server - /prove-tx [@slow][@proof-server]', () => {
  describe('when a GET request is sent to the POST-only endpoint', () => {
    test('should respond with 404 Not Found to indicate the method is unsupported', async () => {
      const response = await axios.get(`${getProofServerUrl()}/prove-tx`, {
        // Allow axios to resolve promises for error HTTP status codes so assertions can run.
        validateStatus: () => true
      });

      expect(response.status).toBe(404);
    });
  });
});
