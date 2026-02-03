// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';

/**
 * Secret key for Dust operations.
 *
 * This class holds sensitive cryptographic material for dust token operations.
 * Always call `clear()` when done to securely erase the key from memory.
 *
 * @example
 * ```typescript
 * // Create key from a 32-byte seed
 * const seed = new Uint8Array(32);
 * crypto.getRandomValues(seed);
 * const dustKey = DustSecretKey.fromSeed(seed);
 *
 * // Get public key
 * console.log('Dust public key:', dustKey.publicKey);
 *
 * // Clear when done
 * dustKey.clear();
 * ```
 */
export class DustSecretKey {
  private _keyId: string | null;

  private constructor(keyId: string) {
    this._keyId = keyId;
  }

  /**
   * Creates a dust secret key from a 32-byte seed.
   * Uses the standard derivation function.
   *
   * @param seed - A 32-byte seed (e.g., from HD wallet derivation)
   * @returns A new DustSecretKey instance
   */
  static fromSeed(seed: Uint8Array): DustSecretKey {
    if (seed.length !== 32) {
      throw new Error('Seed must be exactly 32 bytes');
    }
    const keyId = MidnightLedger.createDustSecretKey(seed);
    return new DustSecretKey(keyId);
  }

  /**
   * Gets the dust public key as a bigint.
   * WASM-compatible: Returns bigint for use with DustAddress encoding.
   * Used for receiving dust tokens.
   */
  get publicKey(): bigint {
    this.ensureNotCleared();
    const hexString = MidnightLedger.getDustPublicKey(this._keyId!);
    // Native returns big-endian hex string for direct BigInt conversion
    return BigInt('0x' + hexString);
  }

  /**
   * Gets the dust public key as a hex-encoded string (for debugging/display).
   */
  get publicKeyHex(): string {
    this.ensureNotCleared();
    return MidnightLedger.getDustPublicKey(this._keyId!);
  }

  /**
   * Gets the internal key ID (for advanced usage with native functions).
   */
  get keyId(): string {
    this.ensureNotCleared();
    return this._keyId!;
  }

  /**
   * Checks if the key has been cleared.
   */
  get isCleared(): boolean {
    return this._keyId === null;
  }

  /**
   * Securely clears the secret key from memory.
   * After calling this method, the key cannot be used anymore.
   */
  clear(): void {
    if (this._keyId !== null) {
      MidnightLedger.clearDustSecretKey(this._keyId);
      this._keyId = null;
    }
  }

  private ensureNotCleared(): void {
    if (this._keyId === null) {
      throw new Error('Dust secret key has been cleared');
    }
  }
}
