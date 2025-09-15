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

import type { AlignmentSegment } from '@midnight-ntwrk/ledger';

function atomBytes(len: number): AlignmentSegment {
  return { tag: 'atom', value: { tag: 'bytes', length: len } };
}

export const ATOM_BYTES_1: AlignmentSegment = atomBytes(1);
export const ATOM_BYTES_8: AlignmentSegment = atomBytes(8);
export const ATOM_BYTES_16: AlignmentSegment = atomBytes(16);
export const ATOM_BYTES_32: AlignmentSegment = atomBytes(32);
export const ATOM_COMPRESS: AlignmentSegment = { tag: 'atom', value: { tag: 'compress' } };
export const ATOM_FIELD: AlignmentSegment = { tag: 'atom', value: { tag: 'field' } };

export const EMPTY_VALUE: Uint8Array = new Uint8Array(0);
export const ONE_VALUE: Uint8Array = new Uint8Array([1]);
export const TWO_VALUE: Uint8Array = new Uint8Array([2]);
export const THREE_VALUE: Uint8Array = new Uint8Array([3]);
