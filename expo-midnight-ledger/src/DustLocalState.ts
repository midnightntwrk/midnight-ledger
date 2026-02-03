// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';
import {DustParameters} from './LedgerParameters';

/**
 * Interface for objects that can be serialized to Uint8Array.
 * Compatible with LedgerEvent from @midnight-ntwrk/ledger-v7.
 */
export interface Serializable {
  serialize(): Uint8Array;
}

/**
 * Represents a qualified dust output (UTXO) with full information.
 */
export interface QualifiedDustOutput {
  /** The internal ID for this UTXO (for use with spend) */
  id: string;
  /** The nonce of the UTXO */
  nonce: string;
  /** The initial value of the UTXO */
  initialValue: string;
  /** The Merkle tree index */
  mtIndex: string;
  /** Creation time in seconds since epoch */
  ctimeSeconds: number;
  /** Sequence number */
  seq: number;
  /** Owner's dust public key (hex) */
  owner: string;
  /** Backing night nonce (hex) */
  backingNight: string;
}

/**
 * Interface for WASM-style DustParameters objects.
 */
interface WasmDustParameters {
  nightDustRatio: bigint | number;
  generationDecayRate: bigint | number;
  dustGracePeriodSeconds: bigint | number;
}

/**
 * Interface for secret keys (our DustSecretKey or WASM-style).
 */
interface DustSecretKeyLike {
  keyId?: string;
}

/**
 * Local state for Dust token operations.
 *
 * This tracks the user's dust tokens by maintaining local state
 * and watching for tokens sent to the user's public key.
 *
 * WASM-compatible: Accepts WASM-style DustParameters objects directly.
 *
 * @example
 * ```typescript
 * // Create a new local state with parameters
 * const params = LedgerParameters.initialParameters().dust;
 * const state = new DustLocalState(params);
 *
 * // Or create from a paramsId string
 * const state2 = new DustLocalState(paramsId);
 *
 * // Replay events to sync state
 * const syncedState = state.replayEvents(secretKey, events);
 *
 * // Process TTLs
 * const updatedState = syncedState.processTtls(Date.now() / 1000);
 *
 * // Get UTXOs
 * const utxos = updatedState.utxos;
 *
 * // Serialize for persistence
 * const serialized = updatedState.serialize();
 *
 * // Dispose when done
 * state.dispose();
 * ```
 */
export class DustLocalState {
  private _stateId: string | null;
  private _params: DustParameters | null = null;

  /**
   * Creates a new DustLocalState from parameters.
   *
   * WASM-compatible: Accepts DustParameters instance (native or WASM-style with
   * nightDustRatio, generationDecayRate, dustGracePeriodSeconds properties).
   *
   * @param params - DustParameters instance, WASM-style params object, or a paramsId string
   */
  constructor(params: DustParameters | WasmDustParameters | string) {
    let paramsId: string;

    if (typeof params === 'string') {
      // It's already a paramsId
      paramsId = params;
    } else if (params instanceof DustParameters) {
      // It's our DustParameters instance
      paramsId = params.paramsId;
      this._params = params;
    } else if (params && typeof (params as any).paramsId === 'string') {
      // It's an object with a paramsId property (our DustParameters or similar)
      paramsId = (params as any).paramsId;
    } else if (
      params &&
      ('nightDustRatio' in params || 'generationDecayRate' in params || 'dustGracePeriodSeconds' in params)
    ) {
      // WASM-style DustParameters object - create native params from values
      const wasmParams = params as WasmDustParameters;
      const ratio = Number(wasmParams.nightDustRatio ?? 0);
      const decay = Number(wasmParams.generationDecayRate ?? 0);
      const grace = Number(wasmParams.dustGracePeriodSeconds ?? 0);
      paramsId = MidnightLedger.createDustParameters(ratio, decay, grace);
    } else {
      throw new Error('DustLocalState requires DustParameters or a paramsId string');
    }

    this._stateId = MidnightLedger.createDustLocalState(paramsId);
  }

  /**
   * Creates a DustLocalState from an existing state ID.
   * @internal
   */
  private static fromStateId(stateId: string): DustLocalState {
    const instance = Object.create(DustLocalState.prototype);
    instance._stateId = stateId;
    instance._params = null;
    return instance;
  }

  /**
   * Deserializes a DustLocalState from bytes.
   *
   * @param data - The serialized state data
   * @returns A new DustLocalState instance
   */
  static deserialize(data: Uint8Array): DustLocalState {
    const stateId = MidnightLedger.deserializeDustLocalState(data);
    return DustLocalState.fromStateId(stateId);
  }

  /**
   * Gets the internal state ID (for advanced usage).
   */
  get stateId(): string {
    this.ensureNotDisposed();
    return this._stateId!;
  }

  /**
   * Checks if the state has been disposed.
   */
  get isDisposed(): boolean {
    return this._stateId === null;
  }

  /**
   * Gets the dust parameters for this state.
   */
  get params(): DustParameters {
    this.ensureNotDisposed();
    if (!this._params) {
      const paramsId = MidnightLedger.dustLocalStateParams(this._stateId!);
      this._params = DustParameters.fromParamsId(paramsId);
    }
    return this._params;
  }

  /**
   * Gets the wallet balance at a given time.
   *
   * WASM-compatible: Accepts number, bigint, or Date object. Returns bigint.
   *
   * @param time - Time in seconds since epoch (number or bigint) or Date object
   * @returns The balance as a bigint
   */
  walletBalance(time: number | bigint | Date): bigint {
    this.ensureNotDisposed();
    const timeSeconds = time instanceof Date ? Math.floor(time.getTime() / 1000) : Number(time);
    const balanceStr = MidnightLedger.dustLocalStateWalletBalance(this._stateId!, timeSeconds);
    return BigInt(balanceStr);
  }

  /**
   * Gets the sync time in seconds.
   */
  get syncTimeSeconds(): number {
    this.ensureNotDisposed();
    return MidnightLedger.dustLocalStateSyncTimeSeconds(this._stateId!);
  }

  /**
   * Gets the number of UTXOs.
   */
  get utxosCount(): number {
    this.ensureNotDisposed();
    return MidnightLedger.dustLocalStateUtxosCount(this._stateId!);
  }

  /**
   * Gets all UTXOs in this state.
   */
  get utxos(): QualifiedDustOutput[] {
    this.ensureNotDisposed();
    return MidnightLedger.dustLocalStateUtxos(this._stateId!);
  }

  /**
   * Replays events to update the local state.
   *
   * WASM-compatible: Accepts DustSecretKey (with keyId property) and
   * Event objects (with serialize() method). Uses index-based iteration
   * for compatibility with array-like objects (matching wasm-bindgen behavior).
   *
   * @param secretKey - The DustSecretKey to use for decryption
   * @param events - Array-like of Event objects (with serialize() method) or event IDs (strings)
   * @returns A new DustLocalState instance with the events applied
   */
  replayEvents(secretKey: DustSecretKeyLike, events: ArrayLike<Serializable | string>): DustLocalState {
    this.ensureNotDisposed();

    // Get the key ID - support both our DustSecretKey and WASM-style
    const keyId = secretKey.keyId;
    if (!keyId) {
      throw new Error('DustSecretKey must have a keyId property');
    }

    // Convert events to event IDs if they are Event objects
    // Use index-based iteration for compatibility with array-like objects
    // (Effect's Chunk.toArray may not have proper Symbol.iterator in Hermes)
    const eventIds: string[] = [];
    const createdEventIds: string[] = [];
    const eventsLength = events.length;

    for (let i = 0; i < eventsLength; i++) {
      const event = events[i];
      if (typeof event === 'string') {
        // Already an event ID
        eventIds.push(event);
      } else {
        // It's an Event object - serialize and deserialize to get an ID
        const serialized = event.serialize();
        const eventId = MidnightLedger.deserializeEvent(serialized);
        eventIds.push(eventId);
        createdEventIds.push(eventId);
      }
    }

    try {
      const newStateId = MidnightLedger.dustLocalStateReplayEvents(this._stateId!, keyId, eventIds);
      return DustLocalState.fromStateId(newStateId);
    } finally {
      // Clean up the event IDs we created
      for (let i = 0; i < createdEventIds.length; i++) {
        MidnightLedger.disposeEvent(createdEventIds[i]);
      }
    }
  }

  /**
   * Spends a dust UTXO.
   *
   * WASM-compatible: Returns [DustLocalState, DustSpend] tuple.
   *
   * @param secretKey - The DustSecretKey to use for signing (with keyId property)
   * @param utxo - The qualified dust output to spend (must have an `id` field from utxos getter)
   * @param vFee - The fee value as bigint or string
   * @param ctime - The creation time as Date or seconds since epoch
   * @returns A tuple of [new_state, dust_spend]
   */
  spend(
    secretKey: DustSecretKeyLike,
    utxo: QualifiedDustOutput,
    vFee: bigint | string,
    ctime: Date | number
  ): [DustLocalState, DustSpend] {
    this.ensureNotDisposed();

    const keyId = secretKey.keyId;
    if (!keyId) {
      throw new Error('DustSecretKey must have a keyId property');
    }

    if (!utxo.id) {
      throw new Error('QualifiedDustOutput must have an id field (obtained from utxos getter)');
    }

    const vFeeStr = typeof vFee === 'bigint' ? vFee.toString() : vFee;
    const ctimeSeconds = ctime instanceof Date ? Math.floor(ctime.getTime() / 1000) : ctime;

    const result = MidnightLedger.dustLocalStateSpend(this._stateId!, keyId, utxo.id, vFeeStr, ctimeSeconds);

    return [DustLocalState.fromStateId(result.stateId), new DustSpend(result.spendId)];
  }

  /**
   * Processes TTLs (time-to-live) for the state.
   *
   * WASM-compatible: Accepts either a number (seconds since epoch) or a Date object.
   *
   * @param time - Current time as seconds since epoch (number) or Date object
   * @returns A new DustLocalState instance with TTLs processed
   */
  processTtls(time: number | Date): DustLocalState {
    this.ensureNotDisposed();
    const timeSeconds = time instanceof Date ? Math.floor(time.getTime() / 1000) : time;
    const newStateId = MidnightLedger.dustLocalStateProcessTtls(this._stateId!, timeSeconds);
    return DustLocalState.fromStateId(newStateId);
  }

  /**
   * Serializes the state to bytes.
   *
   * @returns The serialized state
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeDustLocalState(this._stateId!);
  }

  /**
   * Gets a debug string representation of this state.
   *
   * @returns Debug string
   */
  toDebugString(): string {
    this.ensureNotDisposed();
    return MidnightLedger.dustLocalStateToDebugString(this._stateId!);
  }

  /**
   * Disposes of the state, freeing native resources.
   */
  dispose(): void {
    if (this._stateId !== null) {
      MidnightLedger.disposeDustLocalState(this._stateId);
      this._stateId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._stateId === null) {
      throw new Error('DustLocalState has been disposed');
    }
  }
}

/**
 * A Dust spend (spend proof preimage).
 *
 * This is returned by DustLocalState.spend() and contains the data needed
 * to create a Dust transaction.
 */
export class DustSpend {
  private _spendId: string | null;

  /**
   * Creates a DustSpend from an existing spend ID.
   * @internal
   */
  constructor(spendId: string) {
    this._spendId = spendId;
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
   * Gets the fee value as a string.
   */
  get vFee(): string {
    this.ensureNotDisposed();
    return MidnightLedger.dustSpendVFee(this._spendId!);
  }

  /**
   * Gets the old nullifier as a hex-encoded string.
   */
  get oldNullifier(): string {
    this.ensureNotDisposed();
    return MidnightLedger.dustSpendOldNullifier(this._spendId!);
  }

  /**
   * Gets the new commitment as a hex-encoded string.
   */
  get newCommitment(): string {
    this.ensureNotDisposed();
    return MidnightLedger.dustSpendNewCommitment(this._spendId!);
  }

  /**
   * Serializes the spend to bytes.
   *
   * @returns The serialized spend
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeDustSpend(this._spendId!);
  }

  /**
   * Gets a debug string representation of this spend.
   *
   * @returns Debug string
   */
  toDebugString(): string {
    this.ensureNotDisposed();
    return MidnightLedger.dustSpendToDebugString(this._spendId!);
  }

  /**
   * Disposes of the spend, freeing native resources.
   */
  dispose(): void {
    if (this._spendId !== null) {
      MidnightLedger.disposeDustSpend(this._spendId);
      this._spendId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._spendId === null) {
      throw new Error('DustSpend has been disposed');
    }
  }
}
