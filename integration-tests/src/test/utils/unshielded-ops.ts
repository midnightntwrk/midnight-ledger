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

/**
 * Helper functions for building VM Op sequences for unshielded token operations.
 *
 * These functions generate the exact VM operations that the Compact compiler produces
 * when a contract uses unshielded token primitives like receiveUnshielded, sendUnshielded,
 * and claiming spend recipients.
 *
 * ## Architecture Overview
 *
 * Unshielded tokens use a UTXO model. Contracts interact with unshielded tokens through
 * an "effects" structure that the VM tracks during execution:
 *
 * - Effects index 6: `unshielded_inputs` - Map of TokenType → u128 for tokens flowing INTO the contract
 * - Effects index 7: `unshielded_outputs` - Map of TokenType → u128 for tokens flowing OUT OF the contract
 * - Effects index 8: `claimed_unshielded_spends` - Map of (TokenType, Recipient) → u128 specifying who receives tokens
 *
 * ## TokenType Encoding
 *
 * For unshielded tokens:
 * - Byte 0: 1 (tag for unshielded variant)
 * - Bytes 1-32: color (the token type hash, e.g., all zeros for NIGHT)
 * - Bytes 33-64: padding (zeros)
 *
 * ## Usage Example
 *
 * ```typescript
 * // Contract receiving unshielded tokens (deposit)
 * const ops = receiveUnshieldedOps(tokenTypeValue, amountValue);
 *
 * // Contract sending unshielded tokens to a user (withdrawal)
 * const ops = [
 *   ...sendUnshieldedOps(tokenTypeValue, amountValue),
 *   ...claimUnshieldedSpendOps(tokenTypeValue, recipientValue, amountValue)
 * ];
 * ```
 */

import type { AlignedValue, Op } from '@midnight-ntwrk/ledger';
import { bigIntToValue } from '@midnight-ntwrk/ledger';
import { ATOM_BYTES_1, ATOM_BYTES_16, ATOM_BYTES_32, ONE_VALUE } from '@/test/utils/value-alignment';
import { Static } from '@/test-objects';

// Effects structure indices for unshielded operations
const EFFECTS_UNSHIELDED_INPUTS_IDX = 6;
const EFFECTS_UNSHIELDED_OUTPUTS_IDX = 7;
const EFFECTS_CLAIMED_UNSHIELDED_SPENDS_IDX = 8;

/**
 * Convert a hex string to Uint8Array.
 * @param hex - The hex string (64 chars = 32 bytes)
 */
function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

/**
 * Creates an AlignedValue representing an unshielded TokenType.
 *
 * TokenType::Unshielded is encoded as:
 * - Byte 0: 1 (tag for unshielded variant)
 * - Bytes 1-32: color (the raw token type hash)
 * - Bytes 33-64: padding (empty for unshielded, used for shielded contract address)
 *
 * Based on the pattern in QueryContext.test.ts and the Rust encoding.
 *
 * @param color - The raw token type as a hex string (64 hex chars = 32 bytes)
 * @returns AlignedValue representation of the unshielded token type
 */
export function encodeUnshieldedTokenType(color: string): AlignedValue {
  const colorBytes = hexToBytes(color);
  const emptyPadding = new Uint8Array(0); // Empty for unused variant
  return {
    value: [ONE_VALUE, Static.trimTrailingZeros(colorBytes), emptyPadding],
    alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
  };
}

/**
 * Creates an AlignedValue representing a u128 amount.
 *
 * @param amount - The amount as a bigint
 * @returns AlignedValue representation of the amount
 */
export function encodeAmount(amount: bigint): AlignedValue {
  // Use ledger's bigIntToValue which properly encodes as a Value
  const value = bigIntToValue(amount);
  return {
    value,
    alignment: [ATOM_BYTES_16]
  };
}

/**
 * Creates an AlignedValue representing a user recipient for claimed spends.
 *
 * The key for claimed_unshielded_spends is (TokenType, PublicAddress).
 *
 * PublicAddress encoding (from coin-structure/src/coin.rs):
 * - Contract: [true (1), contractAddress, empty]
 * - User: [false (0/empty), empty, userAddress]
 *
 * Full key structure:
 * [tokenTypeTag, color, padding32, addrTag, slot1, slot2]
 *
 * For User recipients:
 * - addrTag = EMPTY_VALUE (false/0)
 * - slot1 = EMPTY_VALUE (contract address slot, unused)
 * - slot2 = userAddress
 *
 * @param color - The raw token type as a hex string (64 hex chars = 32 bytes)
 * @param userAddress - The user address (verifying key hash) as a hex string
 * @returns AlignedValue representation of the claimed spend key
 */
export function encodeClaimedSpendKeyUser(color: string, userAddress: string): AlignedValue {
  const colorBytes = hexToBytes(color);
  const addressBytes = hexToBytes(userAddress);
  const emptyPadding = new Uint8Array(0);
  // User = false tag, empty slot1, address in slot2
  return {
    value: [ONE_VALUE, colorBytes, emptyPadding, emptyPadding, emptyPadding, addressBytes],
    alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
  };
}

/**
 * Creates an AlignedValue representing a contract recipient for claimed spends.
 *
 * The key for claimed_unshielded_spends is (TokenType, PublicAddress).
 *
 * PublicAddress encoding (from coin-structure/src/coin.rs):
 * - Contract: [true (1), contractAddress, empty]
 * - User: [false (0/empty), empty, userAddress]
 *
 * Full key structure:
 * [tokenTypeTag, color, padding32, addrTag, slot1, slot2]
 *
 * For Contract recipients:
 * - addrTag = ONE_VALUE (true/1)
 * - slot1 = contractAddress
 * - slot2 = EMPTY_VALUE (user address slot, unused)
 *
 * @param color - The raw token type as a hex string
 * @param contractAddress - The contract address as a hex string
 * @returns AlignedValue representation of the claimed spend key
 */
export function encodeClaimedSpendKeyContract(color: string, contractAddress: string): AlignedValue {
  const colorBytes = hexToBytes(color);
  const addressBytes = hexToBytes(contractAddress);
  const emptyPadding = new Uint8Array(0);
  // Contract = true tag, address in slot1, empty slot2
  return {
    value: [ONE_VALUE, colorBytes, emptyPadding, ONE_VALUE, addressBytes, emptyPadding],
    alignment: [ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32, ATOM_BYTES_1, ATOM_BYTES_32, ATOM_BYTES_32]
  };
}

/**
 * Create the Op sequence for receiveUnshielded (effects index 6: unshielded_inputs).
 *
 * This function generates the exact VM operations that the Compact compiler produces
 * when a contract calls `receiveUnshielded(color, amount)`. The ledger uses these
 * operations to track incoming unshielded tokens.
 *
 * ## What this does:
 * 1. Accesses the effects structure at index 6 (unshielded_inputs map)
 * 2. Uses the token type as the map key
 * 3. If the key exists, adds the amount to the existing value
 * 4. If the key doesn't exist, inserts the amount as a new entry
 *
 * @param tokenType - The token type as an AlignedValue (use encodeUnshieldedTokenType)
 * @param amount - The amount as an AlignedValue (use encodeAmount)
 * @returns Array of VM operations
 */
export function receiveUnshieldedOps(tokenType: AlignedValue, amount: AlignedValue): Op<null>[] {
  const indexValue: AlignedValue = {
    value: [new Uint8Array([EFFECTS_UNSHIELDED_INPUTS_IDX])],
    alignment: [ATOM_BYTES_1]
  };

  return [
    // Swap to access effects on stack
    { swap: { n: 0 } },
    // Index into effects at position 6 (unshielded_inputs map), push path for later insert
    {
      idx: {
        cached: true,
        pushPath: true,
        path: [{ tag: 'value', value: indexValue }]
      }
    },
    // Push the token type as key
    { push: { storage: false, value: { tag: 'cell', content: tokenType } } },
    // Duplicate for member check
    { dup: { n: 1 } },
    { dup: { n: 1 } },
    // Check if key exists in map
    'member',
    // Push the amount
    { push: { storage: false, value: { tag: 'cell', content: amount } } },
    // Swap and negate for branching
    { swap: { n: 0 } },
    'neg',
    // Branch: skip 4 ops if key doesn't exist
    { branch: { skip: 4 } },
    // If exists: get current value and add amount
    { dup: { n: 2 } },
    { dup: { n: 2 } },
    {
      idx: {
        cached: true,
        pushPath: false,
        path: [{ tag: 'stack' }]
      }
    },
    'add',
    // Insert the value
    { ins: { cached: true, n: 2 } },
    // Swap back
    { swap: { n: 0 } }
  ];
}

/**
 * Create the Op sequence for sendUnshielded (effects index 7: unshielded_outputs).
 *
 * This function generates the VM operations for a contract sending unshielded tokens.
 * It mirrors what the Compact compiler generates for `sendUnshielded(color, amount, recipient)`.
 *
 * ## Important:
 * This function only handles the OUTPUT side. For a complete withdrawal,
 * you also need `claimUnshieldedSpendOps` to specify WHO receives the tokens.
 *
 * @param tokenType - The token type as an AlignedValue
 * @param amount - The amount as an AlignedValue
 * @returns Array of VM operations
 */
export function sendUnshieldedOps(tokenType: AlignedValue, amount: AlignedValue): Op<null>[] {
  const indexValue: AlignedValue = {
    value: [new Uint8Array([EFFECTS_UNSHIELDED_OUTPUTS_IDX])],
    alignment: [ATOM_BYTES_1]
  };

  return [
    { swap: { n: 0 } },
    {
      idx: {
        cached: true,
        pushPath: true,
        path: [{ tag: 'value', value: indexValue }]
      }
    },
    { push: { storage: false, value: { tag: 'cell', content: tokenType } } },
    { dup: { n: 1 } },
    { dup: { n: 1 } },
    'member',
    { push: { storage: false, value: { tag: 'cell', content: amount } } },
    { swap: { n: 0 } },
    'neg',
    { branch: { skip: 4 } },
    { dup: { n: 2 } },
    { dup: { n: 2 } },
    {
      idx: {
        cached: true,
        pushPath: false,
        path: [{ tag: 'stack' }]
      }
    },
    'add',
    { ins: { cached: true, n: 2 } },
    { swap: { n: 0 } }
  ];
}

// Context indices for reading from CallContext
const CONTEXT_IDX_BALANCE = 5;

/**
 * Create the Op sequence for checking if balance < amount (unshieldedBalanceLt).
 *
 * This function generates the VM operations that check if the contract's unshielded
 * balance for a token type is less than a given amount. The balance is stored in the
 * CallContext at index 5 (the balance map).
 *
 * ## How it works:
 * 1. Duplicates the context from stack position 2
 * 2. Indexes into the balance map (context index 5)
 * 3. Checks if the token type exists in the balance map
 * 4. If exists: reads the value and compares with amount using 'lt'
 * 5. If not exists: uses 0 as the balance (which is always < amount if amount > 0)
 *
 * ## Result:
 * Returns a boolean indicating whether balance < amount (true) or balance >= amount (false)
 *
 * ## Usage:
 * For `unshieldedBalanceGte`, the result should be false (meaning balance >= amount)
 *
 * @param tokenType - The token type as an AlignedValue
 * @param amount - The amount as an AlignedValue
 * @returns Array of VM operations
 */
export function unshieldedBalanceLtOps(tokenType: AlignedValue, amount: AlignedValue): Op<null>[] {
  const balanceIndexValue: AlignedValue = {
    value: [new Uint8Array([CONTEXT_IDX_BALANCE])],
    alignment: [ATOM_BYTES_1]
  };
  // Use the same encoding as amount values for consistency
  const zeroValue: AlignedValue = {
    value: bigIntToValue(0n),
    alignment: [ATOM_BYTES_16]
  };

  return [
    // Duplicate context from stack position 2
    { dup: { n: 2 } },
    // Index into balance map (context index 5)
    {
      idx: {
        cached: true,
        pushPath: false,
        path: [{ tag: 'value', value: balanceIndexValue }]
      }
    },
    // Duplicate for member check
    { dup: { n: 0 } },
    // Push token type as key
    { push: { storage: false, value: { tag: 'cell', content: tokenType } } },
    // Check if key exists in balance map
    'member',
    // Branch: skip 3 ops if key doesn't exist (member returns false)
    { branch: { skip: 3 } },
    // Key doesn't exist path: pop the balance map, push 0
    'pop',
    { push: { storage: false, value: { tag: 'cell', content: zeroValue } } },
    // Jump past the "key exists" path
    { jmp: { skip: 1 } },
    // Key exists path: index into map to get the value
    {
      idx: {
        cached: true,
        pushPath: false,
        path: [{ tag: 'value', value: tokenType }]
      }
    },
    // Push amount to compare
    { push: { storage: false, value: { tag: 'cell', content: amount } } },
    // Less than comparison: balance < amount?
    'lt',
    // Pop result (leaves boolean on stack which becomes transcript output)
    { popeq: { cached: true, result: null } }
  ];
}

/**
 * Create the Op sequence for claiming unshielded spend (effects index 8: claimed_unshielded_spends).
 *
 * This function specifies WHO should receive the tokens being sent via sendUnshielded.
 *
 * ## Critical for verification:
 * The ledger performs a SUBSET CHECK during transaction verification:
 * - For user recipients: The `claimed_unshielded_spends` must be a subset of
 *   the `UnshieldedOffer.outputs`. This ensures the user actually receives a UTXO.
 * - For contract recipients: The `claimed_unshielded_spends` must be a subset of
 *   the recipient contract's `unshielded_inputs`.
 *
 * @param claimKey - The claim key as an AlignedValue (use encodeClaimedSpendKeyUser or encodeClaimedSpendKeyContract)
 * @param amount - The amount as an AlignedValue
 * @returns Array of VM operations
 */
export function claimUnshieldedSpendOps(claimKey: AlignedValue, amount: AlignedValue): Op<null>[] {
  const indexValue: AlignedValue = {
    value: [new Uint8Array([EFFECTS_CLAIMED_UNSHIELDED_SPENDS_IDX])],
    alignment: [ATOM_BYTES_1]
  };

  return [
    { swap: { n: 0 } },
    {
      idx: {
        cached: true,
        pushPath: true,
        path: [{ tag: 'value', value: indexValue }]
      }
    },
    { push: { storage: false, value: { tag: 'cell', content: claimKey } } },
    { dup: { n: 1 } },
    { dup: { n: 1 } },
    'member',
    { push: { storage: false, value: { tag: 'cell', content: amount } } },
    { swap: { n: 0 } },
    'neg',
    { branch: { skip: 4 } },
    { dup: { n: 2 } },
    { dup: { n: 2 } },
    {
      idx: {
        cached: true,
        pushPath: false,
        path: [{ tag: 'stack' }]
      }
    },
    'add',
    { ins: { cached: true, n: 2 } },
    { swap: { n: 0 } }
  ];
}
