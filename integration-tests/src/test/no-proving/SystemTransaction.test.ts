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

import { SystemTransaction } from '@midnight-ntwrk/ledger';

describe('Ledger API - SystemTransaction', () => {
  test('should fail on invalid system transaction deserialization', () => {
    expect(() => SystemTransaction.deserialize(new Uint8Array(1))).toThrow('Unable to deserialize SystemTransaction.');
  });
});
