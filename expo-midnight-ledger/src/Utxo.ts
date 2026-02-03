// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';

/**
 * A UTXO spend input for an unshielded transaction.
 *
 * @example
 * ```typescript
 * const spend = UtxoSpend.create(
 *   '1000000', // value as string (for bigint support)
 *   ownerAddress,
 *   tokenType,
 *   intentHash,
 *   0 // output number
 * );
 *
 * console.log(spend.value);
 * spend.dispose();
 * ```
 */
export class UtxoSpend {
  private _spendId: string | null;

  private constructor(spendId: string) {
    this._spendId = spendId;
  }

  /**
   * Creates a new UTXO spend.
   *
   * @param value - The value as a string (for bigint support)
   * @param owner - The owner address
   * @param tokenType - The token type
   * @param intentHash - The intent hash
   * @param outputNo - The output number
   * @returns A new UtxoSpend instance
   */
  static create(
    value: string | bigint,
    owner: string,
    tokenType: string,
    intentHash: string,
    outputNo: number
  ): UtxoSpend {
    const valueStr = typeof value === 'bigint' ? value.toString() : value;
    const spendId = MidnightLedger.createUtxoSpend(valueStr, owner, tokenType, intentHash, outputNo);
    return new UtxoSpend(spendId);
  }

  /**
   * Deserializes a UTXO spend from bytes.
   *
   * @param data - The serialized spend data
   * @returns A new UtxoSpend instance
   */
  static deserialize(data: Uint8Array): UtxoSpend {
    const spendId = MidnightLedger.deserializeUtxoSpend(data);
    return new UtxoSpend(spendId);
  }

  /**
   * Gets the internal spend ID (for advanced usage).
   */
  get spendId(): string {
    this.ensureNotDisposed();
    return this._spendId!;
  }

  /**
   * Checks if the spend has been disposed.
   */
  get isDisposed(): boolean {
    return this._spendId === null;
  }

  /**
   * Gets the value as a string.
   */
  get value(): string {
    this.ensureNotDisposed();
    return MidnightLedger.utxoSpendValue(this._spendId!);
  }

  /**
   * Gets the value as a bigint.
   */
  get valueBigint(): bigint {
    return BigInt(this.value);
  }

  /**
   * Gets the owner address.
   */
  get owner(): string {
    this.ensureNotDisposed();
    return MidnightLedger.utxoSpendOwner(this._spendId!);
  }

  /**
   * Gets the token type.
   */
  get tokenType(): string {
    this.ensureNotDisposed();
    return MidnightLedger.utxoSpendTokenType(this._spendId!);
  }

  /**
   * Gets the intent hash.
   */
  get intentHash(): string {
    this.ensureNotDisposed();
    return MidnightLedger.utxoSpendIntentHash(this._spendId!);
  }

  /**
   * Gets the output number.
   */
  get outputNo(): number {
    this.ensureNotDisposed();
    return MidnightLedger.utxoSpendOutputNo(this._spendId!);
  }

  /**
   * Serializes the spend to bytes.
   *
   * @returns The serialized spend
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeUtxoSpend(this._spendId!);
  }

  /**
   * Disposes of the spend, freeing native resources.
   */
  dispose(): void {
    if (this._spendId !== null) {
      MidnightLedger.disposeUtxoSpend(this._spendId);
      this._spendId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._spendId === null) {
      throw new Error('UtxoSpend has been disposed');
    }
  }
}

/**
 * A UTXO output for an unshielded transaction.
 *
 * @example
 * ```typescript
 * const output = UtxoOutput.create(
 *   '1000000', // value as string (for bigint support)
 *   ownerAddress,
 *   tokenType
 * );
 *
 * console.log(output.value);
 * output.dispose();
 * ```
 */
export class UtxoOutput {
  private _outputId: string | null;

  private constructor(outputId: string) {
    this._outputId = outputId;
  }

  /**
   * Creates a new UTXO output.
   *
   * @param value - The value as a string (for bigint support)
   * @param owner - The owner address
   * @param tokenType - The token type
   * @returns A new UtxoOutput instance
   */
  static create(value: string | bigint, owner: string, tokenType: string): UtxoOutput {
    const valueStr = typeof value === 'bigint' ? value.toString() : value;
    const outputId = MidnightLedger.createUtxoOutput(valueStr, owner, tokenType);
    return new UtxoOutput(outputId);
  }

  /**
   * Deserializes a UTXO output from bytes.
   *
   * @param data - The serialized output data
   * @returns A new UtxoOutput instance
   */
  static deserialize(data: Uint8Array): UtxoOutput {
    const outputId = MidnightLedger.deserializeUtxoOutput(data);
    return new UtxoOutput(outputId);
  }

  /**
   * Gets the internal output ID (for advanced usage).
   */
  get outputId(): string {
    this.ensureNotDisposed();
    return this._outputId!;
  }

  /**
   * Checks if the output has been disposed.
   */
  get isDisposed(): boolean {
    return this._outputId === null;
  }

  /**
   * Gets the value as a string.
   */
  get value(): string {
    this.ensureNotDisposed();
    return MidnightLedger.utxoOutputValue(this._outputId!);
  }

  /**
   * Gets the value as a bigint.
   */
  get valueBigint(): bigint {
    return BigInt(this.value);
  }

  /**
   * Gets the owner address.
   */
  get owner(): string {
    this.ensureNotDisposed();
    return MidnightLedger.utxoOutputOwner(this._outputId!);
  }

  /**
   * Gets the token type.
   */
  get tokenType(): string {
    this.ensureNotDisposed();
    return MidnightLedger.utxoOutputTokenType(this._outputId!);
  }

  /**
   * Serializes the output to bytes.
   *
   * @returns The serialized output
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeUtxoOutput(this._outputId!);
  }

  /**
   * Disposes of the output, freeing native resources.
   */
  dispose(): void {
    if (this._outputId !== null) {
      MidnightLedger.disposeUtxoOutput(this._outputId);
      this._outputId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._outputId === null) {
      throw new Error('UtxoOutput has been disposed');
    }
  }
}
