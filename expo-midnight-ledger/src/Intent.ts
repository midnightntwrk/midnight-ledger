// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';
import type {UnshieldedOffer} from './UnshieldedOffer';

/**
 * An intent for a Midnight transaction.
 *
 * Intents specify what a transaction should do, including:
 * - Time-to-live (TTL)
 * - Guaranteed unshielded offers
 * - Fallible unshielded offers
 *
 * @example
 * ```typescript
 * // Create an intent with 1 hour TTL
 * const intent = Intent.create(3600);
 *
 * // Set offers
 * intent.setGuaranteedUnshieldedOffer(offer);
 *
 * // Get signature data for signing
 * const sigData = intent.signatureData(0);
 *
 * // Dispose when done
 * intent.dispose();
 * ```
 */
export class Intent {
  private _intentId: string | null;

  private constructor(intentId: string) {
    this._intentId = intentId;
  }

  /**
   * Creates a new intent with the specified TTL.
   *
   * @param ttlSeconds - Time-to-live in seconds
   * @returns A new Intent instance
   */
  static create(ttlSeconds: number): Intent {
    const intentId = MidnightLedger.createIntent(ttlSeconds);
    return new Intent(intentId);
  }

  /**
   * Deserializes an intent from bytes.
   *
   * @param data - The serialized intent data
   * @returns A new Intent instance
   */
  static deserialize(data: Uint8Array): Intent {
    const intentId = MidnightLedger.deserializeIntent(data);
    return new Intent(intentId);
  }

  /**
   * Gets the internal intent ID (for advanced usage).
   */
  get intentId(): string {
    this.ensureNotDisposed();
    return this._intentId!;
  }

  /**
   * Checks if the intent has been disposed.
   */
  get isDisposed(): boolean {
    return this._intentId === null;
  }

  /**
   * Gets the TTL in seconds.
   */
  get ttlSeconds(): number {
    this.ensureNotDisposed();
    return MidnightLedger.intentTtlSeconds(this._intentId!);
  }

  /**
   * Creates a new intent with an updated TTL.
   *
   * @param ttlSeconds - The new TTL in seconds
   * @returns A new Intent instance with the updated TTL
   */
  setTtl(ttlSeconds: number): Intent {
    this.ensureNotDisposed();
    const newIntentId = MidnightLedger.intentSetTtl(this._intentId!, ttlSeconds);
    return new Intent(newIntentId);
  }

  /**
   * Gets the signature data for the specified segment.
   *
   * @param segmentId - The segment ID (typically 0)
   * @returns The signature data as bytes
   */
  signatureData(segmentId: number = 0): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.intentSignatureData(this._intentId!, segmentId);
  }

  /**
   * Gets the intent hash for the specified segment.
   *
   * @param segmentId - The segment ID (typically 0)
   * @returns The intent hash as a hex string
   */
  intentHash(segmentId: number = 0): string {
    this.ensureNotDisposed();
    return MidnightLedger.intentIntentHash(this._intentId!, segmentId);
  }

  /**
   * Gets the guaranteed unshielded offer ID, if any.
   *
   * @returns The offer ID or null
   */
  getGuaranteedUnshieldedOfferId(): string | null {
    this.ensureNotDisposed();
    return MidnightLedger.intentGuaranteedUnshieldedOffer(this._intentId!);
  }

  /**
   * Sets the guaranteed unshielded offer.
   *
   * @param offer - The offer to set, or null to clear
   * @returns A new Intent instance with the offer set
   */
  setGuaranteedUnshieldedOffer(offer: UnshieldedOffer | null): Intent {
    this.ensureNotDisposed();
    const offerId = offer?.offerId ?? null;
    const newIntentId = MidnightLedger.intentSetGuaranteedUnshieldedOffer(this._intentId!, offerId);
    return new Intent(newIntentId);
  }

  /**
   * Gets the fallible unshielded offer ID, if any.
   *
   * @returns The offer ID or null
   */
  getFallibleUnshieldedOfferId(): string | null {
    this.ensureNotDisposed();
    return MidnightLedger.intentFallibleUnshieldedOffer(this._intentId!);
  }

  /**
   * Sets the fallible unshielded offer.
   *
   * @param offer - The offer to set, or null to clear
   * @returns A new Intent instance with the offer set
   */
  setFallibleUnshieldedOffer(offer: UnshieldedOffer | null): Intent {
    this.ensureNotDisposed();
    const offerId = offer?.offerId ?? null;
    const newIntentId = MidnightLedger.intentSetFallibleUnshieldedOffer(this._intentId!, offerId);
    return new Intent(newIntentId);
  }

  /**
   * Binds the intent for the specified segment.
   *
   * @param segmentId - The segment ID (typically 0)
   * @returns A new bound Intent instance
   */
  bind(segmentId: number = 0): Intent {
    this.ensureNotDisposed();
    const newIntentId = MidnightLedger.intentBind(this._intentId!, segmentId);
    return new Intent(newIntentId);
  }

  /**
   * Serializes the intent to bytes.
   *
   * @returns The serialized intent
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeIntent(this._intentId!);
  }

  /**
   * Gets a debug string representation of this intent.
   *
   * @returns Debug string
   */
  toDebugString(): string {
    this.ensureNotDisposed();
    return MidnightLedger.intentToDebugString(this._intentId!);
  }

  /**
   * Disposes of the intent, freeing native resources.
   * After calling this method, the intent cannot be used anymore.
   */
  dispose(): void {
    if (this._intentId !== null) {
      MidnightLedger.disposeIntent(this._intentId);
      this._intentId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._intentId === null) {
      throw new Error('Intent has been disposed');
    }
  }
}
