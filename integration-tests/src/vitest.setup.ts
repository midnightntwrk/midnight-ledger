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

import { webcrypto } from 'node:crypto';
import { beforeEach, expect } from 'vitest';
import { createDefaultTestLogger } from './logger';

// eslint-disable-next-line no-undef
Object.defineProperty(globalThis, 'crypto', {
  value: webcrypto,
  writable: false
});

const logger = await createDefaultTestLogger();
// eslint-disable-next-line @typescript-eslint/no-explicit-any, no-undef
(globalThis as any).logger = logger;

beforeEach(() => {
  logger.info(`Running: ${expect.getState().testPath} -> '${expect.getState().currentTestName}'`);
});
