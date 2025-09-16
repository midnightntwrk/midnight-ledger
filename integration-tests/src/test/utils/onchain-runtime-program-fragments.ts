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
  type AlignedValue,
  type EncodedStateValue,
  type Key,
  leafHash,
  type Op,
  StateBoundedMerkleTree,
  StateValue,
  type Value
} from '@midnight-ntwrk/ledger';
import {
  ATOM_BYTES_1,
  ATOM_BYTES_32,
  ATOM_BYTES_8,
  EMPTY_VALUE,
  ONE_VALUE,
  TWO_VALUE
} from '@/test/utils/value-alignment';

function assertNonEmptyPath(path: Key[]): void {
  if (path.length === 0) {
    throw new Error('path must have at least one segment');
  }
}

function dropLast(keys: Key[]): Key[] {
  return keys.slice(0, Math.max(0, keys.length - 1));
}

function getLeafAlignedValue(keys: Key[]): AlignedValue {
  const leaf = keys[keys.length - 1];
  if (leaf.tag === 'stack') {
    throw Error('stack key type not supported');
  }
  return leaf.value;
}

export function kernelClaimZswapCoinSpend(coinCom: AlignedValue): Op<null>[] {
  return [
    { swap: { n: 0 } },
    {
      idx: {
        cached: true,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: { value: [TWO_VALUE], alignment: [ATOM_BYTES_1] }
          }
        ]
      }
    },
    {
      push: {
        storage: false,
        value: {
          tag: 'cell',
          content: coinCom
        }
      }
    },
    { push: { storage: false, value: { tag: 'null' } } },
    { ins: { cached: true, n: 2 } },
    { swap: { n: 0 } }
  ];
}

export function kernelClaimZswapNullfier(potNull: AlignedValue): Op<null>[] {
  return [
    { swap: { n: 0 } },
    {
      idx: {
        cached: true,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }
          }
        ]
      }
    },
    {
      push: {
        storage: false,
        value: {
          tag: 'cell',
          content: potNull
        }
      }
    },
    { push: { storage: false, value: { tag: 'null' } } },
    { ins: { cached: true, n: 2 } },
    { swap: { n: 0 } }
  ];
}

export function cellWriteCoin(path: Key[], cached: boolean, commitment: AlignedValue, coin: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  const parentPath = dropLast(path);
  const leaf = getLeafAlignedValue(path);

  return [
    { idx: { cached, pushPath: true, path: parentPath } },
    { push: { storage: false, value: { tag: 'cell', content: leaf } } },
    { dup: { n: 3 + (path.length - 1) * 2 } },
    { push: { storage: false, value: { tag: 'cell', content: commitment } } },
    {
      idx: {
        cached: true,
        pushPath: false,
        path: [
          {
            tag: 'value',
            value: { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }
          },
          {
            tag: 'stack'
          }
        ]
      }
    },
    { push: { storage: false, value: { tag: 'cell', content: coin } } },
    { swap: { n: 0 } },
    { concat: { cached: true, n: 91 } },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: path.length - 1 } }
  ];
}

export function cellWrite(path: Key[], cached: boolean, value: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  const parentPath = dropLast(path);
  const leaf = getLeafAlignedValue(path);

  return [
    { idx: { cached, pushPath: true, path: parentPath } },
    { push: { storage: false, value: { tag: 'cell', content: leaf } } },
    { push: { storage: true, value: { tag: 'cell', content: value } } },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: path.length - 1 } }
  ];
}

export function cellRead(path: Key[], cached: boolean): Op<null>[] {
  assertNonEmptyPath(path);

  return [{ dup: { n: 0 } }, { idx: { cached, pushPath: false, path } }, { popeq: { cached, result: null } }];
}

export function counterRead(path: Key[], cached: boolean): Op<null>[] {
  assertNonEmptyPath(path);

  return [{ dup: { n: 0 } }, { idx: { cached, pushPath: false, path } }, { popeq: { cached: true, result: null } }];
}

export function counterLessThan(path: Key[], cached: boolean, decrement: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  return [
    { dup: { n: 0 } },
    { idx: { cached, pushPath: false, path } },
    { push: { storage: false, value: { tag: 'cell', content: decrement } } },
    'lt',
    { popeq: { cached: true, result: null } }
  ];
}

export function setMember(path: Key[], cached: boolean, nul: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  return [
    { dup: { n: 0 } },
    { idx: { cached, pushPath: false, path } },
    { push: { storage: false, value: { tag: 'cell', content: nul } } },
    'member',
    { popeq: { cached: true, result: null } }
  ];
}

export function setInsert(path: Key[], cached: boolean, nul: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  return [
    { idx: { cached, pushPath: true, path } },
    { push: { storage: false, value: { tag: 'cell', content: nul } } },
    { push: { storage: true, value: { tag: 'null' } } },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: path.length } }
  ];
}

export function kernelSelf(): Op<null>[] {
  return [
    { dup: { n: 2 } },
    {
      idx: {
        cached: true,
        pushPath: false,
        path: [
          {
            tag: 'value',
            value: { value: [EMPTY_VALUE], alignment: [ATOM_BYTES_1] }
          }
        ]
      }
    },
    { popeq: { cached: true, result: null } }
  ];
}

export function kernelClaimZswapCoinReceive(coinCom: AlignedValue): Op<null>[] {
  return [
    { swap: { n: 0 } },
    {
      idx: {
        cached: true,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: { value: [ONE_VALUE], alignment: [ATOM_BYTES_1] }
          }
        ]
      }
    },
    {
      push: {
        storage: false,
        value: {
          tag: 'cell',
          content: coinCom
        }
      }
    },
    { push: { storage: false, value: { tag: 'null' } } },
    { ins: { cached: true, n: 2 } },
    { swap: { n: 0 } }
  ];
}

export function counterIncrement(path: Key[], cached: boolean, increment: number): Op<null>[] {
  assertNonEmptyPath(path);
  return [
    { idx: { cached, pushPath: true, path } },
    { addi: { immediate: increment } },
    { ins: { cached: true, n: path.length } }
  ];
}

export function counterResetToDefault(path: Key[], cached: boolean): Op<null>[] {
  assertNonEmptyPath(path);

  const parentPath = dropLast(path);
  const leaf = getLeafAlignedValue(path);
  return [
    { idx: { cached, pushPath: true, path: parentPath } },
    {
      push: { storage: false, value: { tag: 'cell', content: leaf } }
    },
    {
      push: {
        storage: true,
        value: {
          tag: 'cell',
          content: {
            value: [EMPTY_VALUE],
            alignment: [ATOM_BYTES_8]
          }
        }
      }
    },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: path.length - 1 } }
  ];
}

export function setResetToDefault(path: Key[], cached: boolean): Op<null>[] {
  assertNonEmptyPath(path);

  const parentPath = dropLast(path);
  const leaf = getLeafAlignedValue(path);
  return [
    {
      idx: { cached, pushPath: true, path: parentPath }
    },
    {
      push: { storage: false, value: { tag: 'cell', content: leaf } }
    },
    { push: { storage: true, value: { tag: 'map', content: new Map<AlignedValue, EncodedStateValue>() } } },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: path.length - 1 } }
  ];
}

export function merkleTreeResetToDefault(path: Key[], cached: boolean, height: number): Op<null>[] {
  assertNonEmptyPath(path);

  const tree = new StateBoundedMerkleTree(height);
  const stateTree = StateValue.newBoundedMerkleTree(tree).encode();

  const parentPath = dropLast(path);
  const leaf = getLeafAlignedValue(path);

  return [
    { idx: { cached, pushPath: true, path: parentPath } },
    { push: { storage: false, value: { tag: 'cell', content: leaf } } },
    {
      push: {
        storage: true,
        value: {
          tag: 'array',
          content: [
            stateTree,
            {
              tag: 'cell',
              content: {
                value: [EMPTY_VALUE],
                alignment: [ATOM_BYTES_8]
              }
            }
          ]
        }
      }
    },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: path.length - 1 } }
  ];
}

export function merkleTreeCheckRoot(path: Key[], cached: boolean, pathRoot: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  return [
    { dup: { n: 0 } },
    { idx: { cached, pushPath: false, path } },
    {
      idx: {
        cached: false,
        pushPath: false,
        path: [
          {
            tag: 'value',
            value: {
              value: [EMPTY_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    'root',
    { push: { storage: false, value: { tag: 'cell', content: pathRoot } } },
    'eq',
    { popeq: { cached: true, result: null } }
  ];
}

export function historicMerkleTreeInsert(path: Key[], cached: boolean, pk: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  return [
    { idx: { cached, pushPath: true, path } },
    {
      idx: {
        cached: false,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: {
              value: [EMPTY_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    { dup: { n: 2 } },
    {
      idx: {
        cached: false,
        pushPath: false,
        path: [
          {
            tag: 'value',
            value: {
              value: [ONE_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    { push: { storage: true, value: { tag: 'cell', content: leafHash(pk) } } },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: 1 } },
    {
      idx: {
        cached: false,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: {
              value: [ONE_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    { addi: { immediate: 1 } },
    { ins: { cached: true, n: 1 } },
    {
      idx: {
        cached: false,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: {
              value: [TWO_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    { dup: { n: 2 } },
    {
      idx: {
        cached: false,
        pushPath: false,
        path: [
          {
            tag: 'value',
            value: {
              value: [EMPTY_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    'root',
    { push: { storage: true, value: { tag: 'null' } } },
    { ins: { cached: false, n: 1 } },
    { ins: { cached: true, n: path.length + 1 } }
  ];
}

export function historicMerkleTreeCheckRoot(path: Key[], cached: boolean, root: AlignedValue): Op<null>[] {
  assertNonEmptyPath(path);

  return [
    {
      dup: { n: 0 }
    },
    { idx: { cached, pushPath: false, path } },
    {
      idx: {
        cached: false,
        pushPath: false,
        path: [
          {
            tag: 'value',
            value: {
              value: [TWO_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    { push: { storage: false, value: { tag: 'cell', content: root } } },
    'member',
    { popeq: { cached: true, result: null } }
  ];
}

export function merkleTreeInsert(path: Key[], cached: boolean, cm: Value): Op<null>[] {
  assertNonEmptyPath(path);

  const cmLeafHash = leafHash({ value: cm, alignment: [ATOM_BYTES_32] });
  return [
    {
      idx: { cached, pushPath: true, path }
    },
    {
      idx: {
        cached: false,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: {
              value: [EMPTY_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    { dup: { n: 2 } },
    {
      idx: {
        cached: false,
        pushPath: false,
        path: [
          {
            tag: 'value',
            value: {
              value: [ONE_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    {
      push: { storage: true, value: { tag: 'cell', content: cmLeafHash } }
    },
    {
      ins: { cached: false, n: 1 }
    },
    {
      ins: { cached: true, n: 1 }
    },
    {
      idx: {
        cached: false,
        pushPath: true,
        path: [
          {
            tag: 'value',
            value: {
              value: [ONE_VALUE],
              alignment: [ATOM_BYTES_1]
            }
          }
        ]
      }
    },
    { addi: { immediate: 1 } },
    { ins: { cached: true, n: path.length + 1 } }
  ];
}

export function getKey(keyNr: number): Key[] {
  return [
    {
      tag: 'value',
      value: {
        value: [keyNr === 0 ? EMPTY_VALUE : new Uint8Array([keyNr])],
        alignment: [ATOM_BYTES_1]
      }
    }
  ];
}
