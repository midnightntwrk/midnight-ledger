// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';

/**
 * Dust parameters for the ledger.
 *
 * Compatible with both native ID-based pattern and WASM-style constructor:
 * - `new DustParameters(nightDustRatio, generationDecayRate, dustGracePeriodSeconds)`
 * - `DustParameters.create(nightDustRatio, generationDecayRate, dustGracePeriodSeconds)`
 * - `DustParameters.fromParamsId(paramsId)`
 */
export class DustParameters {
  private _paramsId: string;

  /**
   * Creates dust parameters.
   *
   * WASM-compatible: Accepts either BigInt or number values for dust parameters,
   * or a paramsId string for internal use.
   *
   * @param nightDustRatioOrParamsId - Night dust ratio (BigInt/number) or internal paramsId (string)
   * @param generationDecayRate - Generation decay rate (optional if paramsId provided)
   * @param dustGracePeriodSeconds - Dust grace period in seconds (optional if paramsId provided)
   */
  constructor(
    nightDustRatioOrParamsId: bigint | number | string,
    generationDecayRate?: bigint | number,
    dustGracePeriodSeconds?: bigint | number
  ) {
    if (typeof nightDustRatioOrParamsId === 'string' && generationDecayRate === undefined) {
      // Internal pattern: paramsId string
      this._paramsId = nightDustRatioOrParamsId;
    } else {
      // WASM-compatible pattern: create from values
      const ratio =
        typeof nightDustRatioOrParamsId === 'bigint'
          ? Number(nightDustRatioOrParamsId)
          : Number(nightDustRatioOrParamsId);
      const decay =
        typeof generationDecayRate === 'bigint' ? Number(generationDecayRate) : Number(generationDecayRate ?? 0);
      const grace =
        typeof dustGracePeriodSeconds === 'bigint'
          ? Number(dustGracePeriodSeconds)
          : Number(dustGracePeriodSeconds ?? 0);

      this._paramsId = MidnightLedger.createDustParameters(ratio, decay, grace);
    }
  }

  /**
   * Creates dust parameters with the specified values.
   */
  static create(nightDustRatio: number, generationDecayRate: number, dustGracePeriodSeconds: number): DustParameters {
    const paramsId = MidnightLedger.createDustParameters(nightDustRatio, generationDecayRate, dustGracePeriodSeconds);
    return new DustParameters(paramsId);
  }

  /**
   * Creates dust parameters from an existing params ID.
   * @internal
   */
  static fromParamsId(paramsId: string): DustParameters {
    return new DustParameters(paramsId);
  }

  /**
   * Deserializes dust parameters from bytes.
   */
  static deserialize(data: Uint8Array): DustParameters {
    const paramsId = MidnightLedger.deserializeDustParameters(data);
    return new DustParameters(paramsId);
  }

  /** Night to dust ratio */
  get nightDustRatio(): number {
    return MidnightLedger.dustParametersNightDustRatio(this._paramsId);
  }

  /** Generation decay rate */
  get generationDecayRate(): number {
    return MidnightLedger.dustParametersGenerationDecayRate(this._paramsId);
  }

  /** Dust grace period in seconds */
  get dustGracePeriodSeconds(): number {
    return MidnightLedger.dustParametersDustGracePeriodSeconds(this._paramsId);
  }

  /** Gets the internal params ID */
  get paramsId(): string {
    return this._paramsId;
  }

  /** Serializes the parameters to bytes */
  serialize(): Uint8Array {
    return MidnightLedger.serializeDustParameters(this._paramsId);
  }

  /** Disposes the native resources */
  dispose(): void {
    MidnightLedger.disposeDustParameters(this._paramsId);
  }
}

/**
 * Ledger parameters containing all configuration for the ledger.
 */
export class LedgerParameters {
  private _paramsId: string;

  private constructor(paramsId: string) {
    this._paramsId = paramsId;
  }

  /**
   * Gets the initial (genesis) ledger parameters.
   */
  static initialParameters(): LedgerParameters {
    const paramsId = MidnightLedger.initialLedgerParameters();
    return new LedgerParameters(paramsId);
  }

  /**
   * Deserializes ledger parameters from bytes.
   */
  static deserialize(data: Uint8Array): LedgerParameters {
    const paramsId = MidnightLedger.deserializeLedgerParameters(data);
    return new LedgerParameters(paramsId);
  }

  /** Gets the dust parameters */
  get dust(): DustParameters {
    const dustParamsId = MidnightLedger.ledgerParametersDustParams(this._paramsId);
    return DustParameters.fromParamsId(dustParamsId);
  }

  /** Gets the dust parameters ID directly */
  getDustParamsId(): string {
    return MidnightLedger.ledgerParametersDustParams(this._paramsId);
  }

  /** Global TTL in seconds */
  get globalTtlSeconds(): number {
    return MidnightLedger.ledgerParametersGlobalTtlSeconds(this._paramsId);
  }

  /** Transaction byte limit */
  get transactionByteLimit(): number {
    return MidnightLedger.ledgerParametersTransactionByteLimit(this._paramsId);
  }

  /** Cardano bridge fee basis points */
  get cardanoBridgeFeeBasisPoints(): number {
    return MidnightLedger.ledgerParametersCardanoBridgeFeeBasisPoints(this._paramsId);
  }

  /** Cardano bridge minimum amount */
  get cardanoBridgeMinAmount(): string {
    return MidnightLedger.ledgerParametersCardanoBridgeMinAmount(this._paramsId);
  }

  /** Fee overall price */
  get feeOverallPrice(): number {
    return MidnightLedger.ledgerParametersFeeOverallPrice(this._paramsId);
  }

  /** Fee read factor */
  get feeReadFactor(): number {
    return MidnightLedger.ledgerParametersFeeReadFactor(this._paramsId);
  }

  /** Fee compute factor */
  get feeComputeFactor(): number {
    return MidnightLedger.ledgerParametersFeeComputeFactor(this._paramsId);
  }

  /** Fee block usage factor */
  get feeBlockUsageFactor(): number {
    return MidnightLedger.ledgerParametersFeeBlockUsageFactor(this._paramsId);
  }

  /** Fee write factor */
  get feeWriteFactor(): number {
    return MidnightLedger.ledgerParametersFeeWriteFactor(this._paramsId);
  }

  /** Minimum claimable rewards */
  get minClaimableRewards(): string {
    return MidnightLedger.ledgerParametersMinClaimableRewards(this._paramsId);
  }

  /** Gets the internal params ID */
  get paramsId(): string {
    return this._paramsId;
  }

  /** Serializes the parameters to bytes */
  serialize(): Uint8Array {
    return MidnightLedger.serializeLedgerParameters(this._paramsId);
  }

  /** Gets a debug string representation */
  toDebugString(): string {
    return MidnightLedger.ledgerParametersToDebugString(this._paramsId);
  }

  /** Disposes the native resources */
  dispose(): void {
    MidnightLedger.disposeLedgerParameters(this._paramsId);
  }
}
