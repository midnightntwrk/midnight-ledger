// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';
import type {UtxoSpend, UtxoOutput} from './Utxo';

/**
 * An unshielded offer for a Midnight transaction.
 *
 * Unshielded offers represent transfers of unshielded tokens (UTXO-based).
 *
 * @example
 * ```typescript
 * // Create inputs and outputs
 * const input = UtxoSpend.create(value, owner, tokenType, intentHash, outputNo);
 * const output = UtxoOutput.create(value, owner, tokenType);
 *
 * // Create an unsigned offer
 * const offer = UnshieldedOffer.createUnsigned([input], [output]);
 *
 * // Add signatures
 * const signedOffer = offer.addSignatures([signature]);
 *
 * // Dispose when done
 * offer.dispose();
 * signedOffer.dispose();
 * ```
 */
export class UnshieldedOffer {
  private _offerId: string | null;

  private constructor(offerId: string) {
    this._offerId = offerId;
  }

  /**
   * Creates an unshielded offer with signatures.
   *
   * @param inputs - Array of UtxoSpend inputs
   * @param outputs - Array of UtxoOutput outputs
   * @param signatures - Array of signatures (hex-encoded)
   * @returns A new UnshieldedOffer instance
   */
  static create(inputs: UtxoSpend[], outputs: UtxoOutput[], signatures: string[]): UnshieldedOffer {
    const inputIds = inputs.map((i) => i.spendId);
    const outputIds = outputs.map((o) => o.outputId);
    const offerId = MidnightLedger.createUnshieldedOffer(inputIds, outputIds, signatures);
    return new UnshieldedOffer(offerId);
  }

  /**
   * Creates an unsigned unshielded offer.
   *
   * @param inputs - Array of UtxoSpend inputs
   * @param outputs - Array of UtxoOutput outputs
   * @returns A new UnshieldedOffer instance
   */
  static createUnsigned(inputs: UtxoSpend[], outputs: UtxoOutput[]): UnshieldedOffer {
    const inputIds = inputs.map((i) => i.spendId);
    const outputIds = outputs.map((o) => o.outputId);
    const offerId = MidnightLedger.createUnshieldedOfferUnsigned(inputIds, outputIds);
    return new UnshieldedOffer(offerId);
  }

  /**
   * Deserializes an unshielded offer from bytes.
   *
   * @param data - The serialized offer data
   * @returns A new UnshieldedOffer instance
   */
  static deserialize(data: Uint8Array): UnshieldedOffer {
    const offerId = MidnightLedger.deserializeUnshieldedOffer(data);
    return new UnshieldedOffer(offerId);
  }

  /**
   * Gets the internal offer ID (for advanced usage).
   */
  get offerId(): string {
    this.ensureNotDisposed();
    return this._offerId!;
  }

  /**
   * Checks if the offer has been disposed.
   */
  get isDisposed(): boolean {
    return this._offerId === null;
  }

  /**
   * Gets the input IDs for this offer.
   *
   * @returns Array of input IDs
   */
  getInputIds(): string[] {
    this.ensureNotDisposed();
    return MidnightLedger.unshieldedOfferInputs(this._offerId!);
  }

  /**
   * Gets the output IDs for this offer.
   *
   * @returns Array of output IDs
   */
  getOutputIds(): string[] {
    this.ensureNotDisposed();
    return MidnightLedger.unshieldedOfferOutputs(this._offerId!);
  }

  /**
   * Gets the signatures for this offer.
   *
   * @returns Array of signatures (hex-encoded)
   */
  signatures(): string[] {
    this.ensureNotDisposed();
    return MidnightLedger.unshieldedOfferSignatures(this._offerId!);
  }

  /**
   * Adds signatures to this offer.
   *
   * @param signatures - Array of signatures to add (hex-encoded)
   * @returns A new UnshieldedOffer instance with the signatures added
   */
  addSignatures(signatures: string[]): UnshieldedOffer {
    this.ensureNotDisposed();
    const newOfferId = MidnightLedger.unshieldedOfferAddSignatures(this._offerId!, signatures);
    return new UnshieldedOffer(newOfferId);
  }

  /**
   * Serializes the offer to bytes.
   *
   * @returns The serialized offer
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeUnshieldedOffer(this._offerId!);
  }

  /**
   * Gets a debug string representation of this offer.
   *
   * @returns Debug string
   */
  toDebugString(): string {
    this.ensureNotDisposed();
    return MidnightLedger.unshieldedOfferToDebugString(this._offerId!);
  }

  /**
   * Disposes of the offer, freeing native resources.
   * After calling this method, the offer cannot be used anymore.
   */
  dispose(): void {
    if (this._offerId !== null) {
      MidnightLedger.disposeUnshieldedOffer(this._offerId);
      this._offerId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._offerId === null) {
      throw new Error('UnshieldedOffer has been disposed');
    }
  }
}
