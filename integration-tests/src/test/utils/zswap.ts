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
  type Alignment,
  ChargedState,
  type ContractAddress,
  degradeToTransient,
  type LedgerState,
  type Nonce,
  type Proofish,
  QueryContext,
  type RawTokenType,
  type ShieldedCoinInfo,
  transientHash,
  upgradeFromTransient,
  type Value,
  type ZswapOffer
} from '@midnight-ntwrk/ledger';
import { Static } from '@/test-objects';
import { ATOM_FIELD } from '@/test/utils/value-alignment';

/**
 * Creates a QueryContext from a ledger state and contract address,
 * optionally applying a ZSwap offer to populate commitment indices.
 *
 * @param ledger - The current ledger state
 * @param addr - The contract address to query
 * @param offer - Optional ZSwap offer to apply for commitment indices
 * @returns QueryContext with applied offer indices if provided
 */
export function getContextWithOffer(
  ledger: LedgerState,
  addr: ContractAddress,
  offer?: ZswapOffer<Proofish>
): QueryContext {
  const res = new QueryContext(new ChargedState(ledger.index(addr)!.data.state), addr);
  if (offer) {
    const [, indices] = ledger.zswap.tryApply(offer);
    const { block } = res;
    block.comIndices = new Map(Array.from(indices, ([k, v]) => [k, Number(v)]));
    res.block = block;
  }
  return res;
}

/**
 * Evolves a coin's nonce using domain separation.
 *
 * This function creates a new ShieldedCoinInfo with an evolved nonce by hashing
 * the domain separator with the degraded (transient) version of the original nonce.
 *
 * @param domainSep - Domain separator bytes (e.g., 'midnight:kernel:nonce_evolve')
 * @param value - The coin value
 * @param type - The raw token type
 * @param nonce - The original nonce to evolve
 * @returns ShieldedCoinInfo with the evolved nonce
 */
export function evolveFrom(domainSep: Uint8Array, value: bigint, type: RawTokenType, nonce: Nonce): ShieldedCoinInfo {
  const degrade = degradeToTransient([Static.encodeFromHex(nonce)])[0];
  const thAlignment: Alignment = [ATOM_FIELD, ATOM_FIELD];
  const thValue: Value = transientHash(thAlignment, [domainSep, degrade]);
  const evolvedNonce = upgradeFromTransient(thValue)[0];
  const updatedEvolvedNonce = new Uint8Array(evolvedNonce.length + 1);
  updatedEvolvedNonce.set(evolvedNonce, 0);
  updatedEvolvedNonce[updatedEvolvedNonce.length] = 0;
  const evolvedNonceAsNonce: Nonce = Buffer.from(updatedEvolvedNonce).toString('hex');
  return {
    nonce: evolvedNonceAsNonce,
    type,
    value
  };
}
