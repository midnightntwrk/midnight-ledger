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

import { IrInsert } from '@midnight-ntwrk/ledger';

describe('Ledger API - IrInsert', () => {
  /**
   * Test exposing operation and ir.
   *
   * @given An IrInsert built from an operation name and IR bytes
   * @when Reading its operation and ir properties
   * @then They should match the constructor arguments and the value should render a string
   */
  test('exposes operation and ir from the constructor', () => {
    const ir = new Uint8Array([1, 2, 3]);
    const irInsert = new IrInsert('op', ir);

    expect(irInsert.operation).toEqual('op');
    expect(irInsert.ir).toEqual(ir);
    expect(irInsert.toString().length).toBeGreaterThan(0);
  });
});
