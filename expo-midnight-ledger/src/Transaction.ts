// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';
import type {TransactionMarkers} from './types';
import type {Intent} from './Intent';

/**
 * A Midnight ledger transaction.
 *
 * Transactions go through several states:
 * - Unproven: Contains proof preimages, ready to be proven
 * - Proven: Contains actual proofs (via mockProve or remote proving)
 * - Bound: Sealed with binding randomness, ready for submission
 *
 * @example
 * ```typescript
 * // Create a transaction with an intent
 * const intent = Intent.create(3600); // 1 hour TTL
 * const tx = Transaction.fromIntent('mainnet', intent);
 *
 * // Mock prove the transaction (for testing)
 * const provenTx = tx.mockProve();
 *
 * // Bind the transaction
 * const boundTx = provenTx.bind();
 *
 * // Serialize for submission
 * const serialized = boundTx.serialize();
 *
 * // Clean up
 * tx.dispose();
 * provenTx.dispose();
 * boundTx.dispose();
 * intent.dispose();
 * ```
 */
export class Transaction {
  private _txId: string | null;

  private constructor(txId: string) {
    this._txId = txId;
  }

  /**
   * Creates a transaction from an optional intent.
   *
   * @param networkId - The network ID (e.g., 'mainnet', 'testnet')
   * @param intent - Optional intent for the transaction
   * @returns A new Transaction instance
   */
  static fromIntent(networkId: string, intent?: Intent): Transaction {
    const intentId = intent?.intentId ?? null;
    const txId = MidnightLedger.createTransaction(networkId, intentId);
    return new Transaction(txId);
  }

  /**
   * Creates a randomized transaction from an optional intent.
   *
   * @param networkId - The network ID (e.g., 'mainnet', 'testnet')
   * @param intent - Optional intent for the transaction
   * @returns A new Transaction instance
   */
  static fromIntentRandomized(networkId: string, intent?: Intent): Transaction {
    const intentId = intent?.intentId ?? null;
    const txId = MidnightLedger.createTransactionRandomized(networkId, intentId);
    return new Transaction(txId);
  }

  /**
   * Deserializes a transaction from bytes.
   *
   * @param data - The serialized transaction data
   * @returns A new Transaction instance
   */
  static deserialize(data: Uint8Array): Transaction {
    const txId = MidnightLedger.deserializeTransaction(data);
    return new Transaction(txId);
  }

  /**
   * Deserializes a typed transaction from bytes.
   *
   * @param markers - The transaction type markers
   * @param data - The serialized transaction data
   * @returns A new Transaction instance
   */
  static deserializeTyped(markers: TransactionMarkers, data: Uint8Array): Transaction {
    const txId = MidnightLedger.deserializeTransactionTyped(markers.signature, markers.proof, markers.binding, data);
    return new Transaction(txId);
  }

  /**
   * Gets the internal transaction ID (for advanced usage).
   */
  get txId(): string {
    this.ensureNotDisposed();
    return this._txId!;
  }

  /**
   * Checks if the transaction has been disposed.
   */
  get isDisposed(): boolean {
    return this._txId === null;
  }

  /**
   * Gets the network ID of this transaction.
   */
  get networkId(): string {
    this.ensureNotDisposed();
    return MidnightLedger.transactionNetworkId(this._txId!);
  }

  /**
   * Mock proves the transaction (for testing only).
   * Uses pre-generated mock proofs instead of real proving.
   *
   * @returns A new proven Transaction instance
   */
  mockProve(): Transaction {
    this.ensureNotDisposed();
    const newTxId = MidnightLedger.mockProveTransaction(this._txId!);
    return new Transaction(newTxId);
  }

  /**
   * Binds the transaction (seals with binding randomness).
   *
   * After binding, the transaction cannot be modified and is ready
   * for submission to the network.
   *
   * @returns A new bound Transaction instance
   */
  bind(): Transaction {
    this.ensureNotDisposed();
    const newTxId = MidnightLedger.bindTransaction(this._txId!);
    return new Transaction(newTxId);
  }

  /**
   * Merges this transaction with another transaction.
   *
   * @param other - The other transaction to merge with
   * @returns A new merged Transaction instance
   */
  merge(other: Transaction): Transaction {
    this.ensureNotDisposed();
    other.ensureNotDisposed();
    const newTxId = MidnightLedger.mergeTransactions(this._txId!, other._txId!);
    return new Transaction(newTxId);
  }

  /**
   * Gets the transaction identifiers.
   *
   * @returns Array of transaction identifiers
   */
  identifiers(): string[] {
    this.ensureNotDisposed();
    return MidnightLedger.transactionIdentifiers(this._txId!);
  }

  /**
   * Erases the proofs from this transaction.
   *
   * @returns A new Transaction instance with proofs erased
   */
  eraseProofs(): Transaction {
    this.ensureNotDisposed();
    const newTxId = MidnightLedger.eraseTransactionProofs(this._txId!);
    return new Transaction(newTxId);
  }

  /**
   * Erases the signatures from this transaction.
   *
   * @returns A new Transaction instance with signatures erased
   */
  eraseSignatures(): Transaction {
    this.ensureNotDisposed();
    const newTxId = MidnightLedger.eraseTransactionSignatures(this._txId!);
    return new Transaction(newTxId);
  }

  /**
   * Serializes the transaction to bytes.
   *
   * @returns The serialized transaction
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeTransaction(this._txId!);
  }

  /**
   * Gets a debug string representation of this transaction.
   *
   * @returns Debug string
   */
  toDebugString(): string {
    this.ensureNotDisposed();
    return MidnightLedger.transactionToDebugString(this._txId!);
  }

  /**
   * Disposes of the transaction, freeing native resources.
   * After calling this method, the transaction cannot be used anymore.
   */
  dispose(): void {
    if (this._txId !== null) {
      MidnightLedger.disposeTransaction(this._txId);
      this._txId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._txId === null) {
      throw new Error('Transaction has been disposed');
    }
  }
}
