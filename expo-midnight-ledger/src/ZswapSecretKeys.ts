// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';

/**
 * Secret keys for ZSwap operations.
 *
 * This class holds sensitive cryptographic material (coin and encryption secret keys).
 * Always call `clear()` when done to securely erase the keys from memory.
 *
 * @example
 * ```typescript
 * // Create keys from a 32-byte seed
 * const seed = new Uint8Array(32);
 * crypto.getRandomValues(seed);
 * const keys = ZswapSecretKeys.fromSeed(seed);
 *
 * // Get public keys
 * console.log('Coin public key:', keys.coinPublicKey);
 * console.log('Encryption public key:', keys.encryptionPublicKey);
 *
 * // Clear when done
 * keys.clear();
 * ```
 */
export class ZswapSecretKeys {
  private _keyId: string | null;

  private constructor(keyId: string) {
    this._keyId = keyId;
  }

  /**
   * Creates secret keys from a 32-byte seed.
   * Uses the standard derivation function.
   *
   * @param seed - A 32-byte seed (e.g., from a wallet)
   * @returns A new ZswapSecretKeys instance
   */
  static fromSeed(seed: Uint8Array): ZswapSecretKeys {
    if (seed.length !== 32) {
      throw new Error('Seed must be exactly 32 bytes');
    }
    const keyId = MidnightLedger.createZswapSecretKeys(seed);
    return new ZswapSecretKeys(keyId);
  }

  /**
   * Gets the coin public key as a hex-encoded string.
   * Used for receiving shielded coins.
   */
  get coinPublicKey(): string {
    this.ensureNotCleared();
    return MidnightLedger.getZswapCoinPublicKey(this._keyId!);
  }

  /**
   * Gets the encryption public key as a hex-encoded string.
   * Used for encrypting data to this key holder.
   */
  get encryptionPublicKey(): string {
    this.ensureNotCleared();
    return MidnightLedger.getZswapEncryptionPublicKey(this._keyId!);
  }

  /**
   * Gets the coin secret key handle for use in nullifier computation.
   * @returns The coin secret key handle ID
   */
  get coinSecretKey(): string {
    this.ensureNotCleared();
    return MidnightLedger.getZswapCoinSecretKey(this._keyId!);
  }

  /**
   * Gets the coin secret key handle for use in nullifier computation.
   * @returns The coin secret key handle ID
   * @deprecated Use coinSecretKey getter instead
   */
  getCoinSecretKey(): string {
    return this.coinSecretKey;
  }

  /**
   * Gets the encryption secret key handle.
   * @returns The encryption secret key handle ID
   */
  get encryptionSecretKey(): string {
    this.ensureNotCleared();
    return MidnightLedger.getZswapEncryptionSecretKey(this._keyId!);
  }

  /**
   * Gets the encryption secret key handle.
   * @returns The encryption secret key handle ID
   * @deprecated Use encryptionSecretKey getter instead
   */
  getEncryptionSecretKey(): string {
    return this.encryptionSecretKey;
  }

  /**
   * Gets the internal key ID (for advanced usage with native functions).
   */
  get keyId(): string {
    this.ensureNotCleared();
    return this._keyId!;
  }

  /**
   * Checks if the keys have been cleared.
   */
  get isCleared(): boolean {
    return this._keyId === null;
  }

  /**
   * Securely clears the secret keys from memory.
   * After calling this method, the keys cannot be used anymore.
   */
  clear(): void {
    if (this._keyId !== null) {
      MidnightLedger.clearZswapSecretKeys(this._keyId);
      this._keyId = null;
    }
  }

  private ensureNotCleared(): void {
    if (this._keyId === null) {
      throw new Error('Secret keys have been cleared');
    }
  }
}
