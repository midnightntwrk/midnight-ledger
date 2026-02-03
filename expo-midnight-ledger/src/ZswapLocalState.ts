// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';
import type {Serializable} from './DustLocalState';
import type {QualifiedShieldedCoinInfo, ShieldedCoinInfo, CoinCommitment, Nullifier} from './types';

/**
 * A Map-like class that provides Hermes-compatible iteration.
 * Standard Map.values() returns an iterator without .map() in Hermes.
 * This class returns an array from values() to support .map() calls.
 */
class HermesCompatibleMap<K, V> {
  private _map: Map<K, V>;

  constructor() {
    this._map = new Map<K, V>();
  }

  get size(): number {
    return this._map.size;
  }

  get(key: K): V | undefined {
    return this._map.get(key);
  }

  set(key: K, value: V): this {
    this._map.set(key, value);
    return this;
  }

  has(key: K): boolean {
    return this._map.has(key);
  }

  delete(key: K): boolean {
    return this._map.delete(key);
  }

  clear(): void {
    this._map.clear();
  }

  // Return array instead of iterator for Hermes compatibility
  values(): V[] {
    return Array.from(this._map.values());
  }

  keys(): K[] {
    return Array.from(this._map.keys());
  }

  entries(): [K, V][] {
    return Array.from(this._map.entries());
  }

  forEach(callback: (value: V, key: K, map: Map<K, V>) => void): void {
    this._map.forEach(callback);
  }

  [Symbol.iterator](): IterableIterator<[K, V]> {
    return this._map[Symbol.iterator]();
  }
}

/**
 * Interface for secret keys (our ZswapSecretKeys or WASM-style).
 */
interface ZswapSecretKeysLike {
  keyId?: string;
}

/**
 * Local state for ZSwap (shielded) transactions.
 *
 * This tracks the user's shielded coins by maintaining a local Merkle tree
 * and watching for coins sent to the user's public keys.
 *
 * WASM-compatible: Supports `new ZswapLocalState()` constructor pattern.
 *
 * @example
 * ```typescript
 * // Create a new local state (WASM-compatible)
 * const state = new ZswapLocalState();
 *
 * // Or use static factory method
 * const state2 = ZswapLocalState.create();
 *
 * // Watch for coins sent to a public key
 * const newState = state.watchFor(coinPublicKey, coinInfo);
 *
 * // Replay events to sync state
 * const syncedState = newState.replayEvents(secretKeys, events);
 *
 * // Serialize for persistence
 * const serialized = syncedState.serialize();
 *
 * // Dispose when done
 * state.dispose();
 * newState.dispose();
 * syncedState.dispose();
 * ```
 */
export class ZswapLocalState {
  private _stateId: string | null;

  /**
   * Creates a new empty ZswapLocalState.
   *
   * WASM-compatible: Can be called with `new ZswapLocalState()`.
   *
   * @param stateId - Internal state ID (optional, for internal use)
   */
  constructor(stateId?: string) {
    if (stateId !== undefined) {
      // Internal use: created from existing state ID
      this._stateId = stateId;
    } else {
      // WASM-compatible: create new state
      this._stateId = MidnightLedger.createZswapLocalState();
    }
  }

  /**
   * Creates a new empty ZswapLocalState.
   *
   * @returns A new ZswapLocalState instance
   */
  static create(): ZswapLocalState {
    return new ZswapLocalState();
  }

  /**
   * Deserializes a ZswapLocalState from bytes.
   *
   * @param data - The serialized state data
   * @returns A new ZswapLocalState instance
   */
  static deserialize(data: Uint8Array): ZswapLocalState {
    const stateId = MidnightLedger.deserializeZswapLocalState(data);
    return new ZswapLocalState(stateId);
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
   * Gets the first free index in the Merkle tree.
   */
  get firstFree(): number {
    this.ensureNotDisposed();
    return MidnightLedger.zswapLocalStateFirstFree(this._stateId!);
  }

  /**
   * Gets the number of coins being tracked.
   */
  get coinsCount(): number {
    this.ensureNotDisposed();
    return MidnightLedger.zswapLocalStateCoinsCount(this._stateId!);
  }

  /**
   * Gets the set of spendable coins.
   * WASM-compatible: Returns a Set<QualifiedShieldedCoinInfo>.
   */
  get coins(): Set<QualifiedShieldedCoinInfo> {
    this.ensureNotDisposed();
    const coinsData = MidnightLedger.zswapLocalStateCoinsData(this._stateId!);
    const result = new Set<QualifiedShieldedCoinInfo>();
    for (const coin of coinsData) {
      result.add({
        type: coin.type,
        nonce: coin.nonce,
        value: BigInt(coin.value),
        mt_index: BigInt(coin.mt_index),
      });
    }
    return result;
  }

  /**
   * Gets the raw coin commitments as hex-encoded strings.
   * Use this for debugging or when you need the raw data.
   *
   * @returns Array of coin commitments (hex-encoded)
   */
  getCoinsRaw(): string[] {
    this.ensureNotDisposed();
    return MidnightLedger.zswapLocalStateCoins(this._stateId!);
  }

  /**
   * Gets the pending outputs (coins expected to be received).
   * WASM-compatible: Returns a Map-like object with Hermes-compatible iteration.
   *
   * Map key is the coin commitment (hex string).
   * Map value is a tuple of [ShieldedCoinInfo, Date | undefined].
   * The Date is currently always undefined (TTL tracking not yet implemented).
   */
  get pendingOutputs(): HermesCompatibleMap<CoinCommitment, [ShieldedCoinInfo, Date | undefined]> {
    this.ensureNotDisposed();
    const pendingData = MidnightLedger.zswapLocalStatePendingOutputsData(this._stateId!);
    const result = new HermesCompatibleMap<CoinCommitment, [ShieldedCoinInfo, Date | undefined]>();
    for (const entry of pendingData) {
      const coinInfo: ShieldedCoinInfo = {
        type: entry.type,
        nonce: entry.nonce,
        value: BigInt(entry.value),
      };
      result.set(entry.commitment, [coinInfo, undefined]);
    }
    return result;
  }

  /**
   * Gets the pending spends (coins being spent).
   * WASM-compatible: Returns a Map-like object with Hermes-compatible iteration.
   *
   * Map key is the nullifier (hex string).
   * Map value is a tuple of [QualifiedShieldedCoinInfo, Date | undefined].
   * The Date is currently always undefined (TTL tracking not yet implemented).
   */
  get pendingSpends(): HermesCompatibleMap<Nullifier, [QualifiedShieldedCoinInfo, Date | undefined]> {
    this.ensureNotDisposed();
    const pendingData = MidnightLedger.zswapLocalStatePendingSpendsData(this._stateId!);
    const result = new HermesCompatibleMap<Nullifier, [QualifiedShieldedCoinInfo, Date | undefined]>();
    for (const entry of pendingData) {
      const coinInfo: QualifiedShieldedCoinInfo = {
        type: entry.type,
        nonce: entry.nonce,
        value: BigInt(entry.value),
        mt_index: BigInt(entry.mt_index),
      };
      result.set(entry.nullifier, [coinInfo, undefined]);
    }
    return result;
  }

  /**
   * Replays events to update the local state.
   *
   * WASM-compatible: Accepts ZswapSecretKeys (with keyId property) and
   * Event objects (with serialize() method). Uses index-based iteration
   * for compatibility with array-like objects (matching wasm-bindgen behavior).
   *
   * @param secretKeys - The ZswapSecretKeys to use for decryption
   * @param events - Array-like of Event objects (with serialize() method) or event IDs (strings)
   * @returns A new ZswapLocalState instance with the events applied
   */
  replayEvents(secretKeys: ZswapSecretKeysLike, events: ArrayLike<Serializable | string>): ZswapLocalState {
    this.ensureNotDisposed();

    // Get the key ID - support both our ZswapSecretKeys and WASM-style
    const keyId = secretKeys.keyId;
    if (!keyId) {
      throw new Error('ZswapSecretKeys must have a keyId property');
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
      const newStateId = MidnightLedger.zswapLocalStateReplayEvents(this._stateId!, keyId, eventIds);
      return new ZswapLocalState(newStateId);
    } finally {
      // Clean up the event IDs we created
      for (let i = 0; i < createdEventIds.length; i++) {
        MidnightLedger.disposeEvent(createdEventIds[i]);
      }
    }
  }

  /**
   * Applies a collapsed Merkle tree update.
   *
   * @param updateId - The update ID
   * @returns A new ZswapLocalState instance with the update applied
   */
  applyCollapsedUpdate(updateId: string): ZswapLocalState {
    this.ensureNotDisposed();
    const newStateId = MidnightLedger.zswapLocalStateApplyCollapsedUpdate(this._stateId!, updateId);
    return new ZswapLocalState(newStateId);
  }

  /**
   * Watches for coins sent to a public key.
   *
   * @param coinPublicKey - The coin public key to watch (hex-encoded)
   * @param coinInfo - The coin info to watch for
   * @returns A new ZswapLocalState instance with the watch added
   */
  watchFor(coinPublicKey: string, coinInfo: Uint8Array): ZswapLocalState {
    this.ensureNotDisposed();
    const newStateId = MidnightLedger.zswapLocalStateWatchFor(this._stateId!, coinPublicKey, coinInfo);
    return new ZswapLocalState(newStateId);
  }

  /**
   * Spends a coin from this local state.
   *
   * WASM-compatible: Returns [ZswapLocalState, ZswapInput] tuple.
   *
   * @param secretKeys - The ZswapSecretKeys to use for signing (with keyId property)
   * @param coin - The qualified coin info to spend (serialized bytes or object with serialize() method)
   * @param segment - Optional segment ID for the spend
   * @param _ttl - Optional TTL (currently ignored, for WASM compatibility)
   * @returns A tuple of [new_state, input]
   */
  spend(
    secretKeys: ZswapSecretKeysLike,
    coin: QualifiedShieldedCoinInfo | Uint8Array | Serializable,
    segment?: number,
    _ttl?: Date
  ): [ZswapLocalState, ZswapInput] {
    this.ensureNotDisposed();

    const keyId = secretKeys.keyId;
    if (!keyId) {
      throw new Error('ZswapSecretKeys must have a keyId property');
    }

    // Convert coin to bytes if needed
    let coinBytes: Uint8Array;
    if (coin instanceof Uint8Array) {
      coinBytes = coin;
    } else if ('serialize' in coin && typeof coin.serialize === 'function') {
      coinBytes = coin.serialize();
    } else {
      // It's a QualifiedShieldedCoinInfo object - we need to serialize it
      throw new Error(
        'QualifiedShieldedCoinInfo object serialization not yet implemented. Pass serialized bytes instead.'
      );
    }

    const result = MidnightLedger.zswapLocalStateSpend(this._stateId!, keyId, coinBytes, segment ?? null);

    return [new ZswapLocalState(result.stateId), new ZswapInput(result.inputId)];
  }

  /**
   * Serializes the state to bytes.
   *
   * @returns The serialized state
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeZswapLocalState(this._stateId!);
  }

  /**
   * Gets a debug string representation of this state.
   *
   * @returns Debug string
   */
  toDebugString(): string {
    this.ensureNotDisposed();
    return MidnightLedger.zswapLocalStateToDebugString(this._stateId!);
  }

  /**
   * Disposes of the state, freeing native resources.
   */
  dispose(): void {
    if (this._stateId !== null) {
      MidnightLedger.disposeZswapLocalState(this._stateId);
      this._stateId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._stateId === null) {
      throw new Error('ZswapLocalState has been disposed');
    }
  }
}

/**
 * A ZSwap input (spend proof preimage).
 *
 * This is returned by ZswapLocalState.spend() and contains the data needed
 * to create a ZSwap transaction.
 */
export class ZswapInput {
  private _inputId: string | null;

  /**
   * Creates a ZswapInput from an existing input ID.
   * @internal
   */
  constructor(inputId: string) {
    this._inputId = inputId;
  }

  /**
   * Gets the internal input ID (for advanced usage).
   */
  get inputId(): string {
    this.ensureNotDisposed();
    return this._inputId!;
  }

  /**
   * Checks if the input has been disposed.
   */
  get isDisposed(): boolean {
    return this._inputId === null;
  }

  /**
   * Gets the nullifier as a hex-encoded string.
   */
  get nullifier(): string {
    this.ensureNotDisposed();
    return MidnightLedger.zswapInputNullifier(this._inputId!);
  }

  /**
   * Gets the contract address as a hex-encoded string, if any.
   */
  get contractAddress(): string | null {
    this.ensureNotDisposed();
    return MidnightLedger.zswapInputContractAddress(this._inputId!);
  }

  /**
   * Serializes the input to bytes.
   *
   * @returns The serialized input
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeZswapInput(this._inputId!);
  }

  /**
   * Gets a debug string representation of this input.
   *
   * @returns Debug string
   */
  toDebugString(): string {
    this.ensureNotDisposed();
    return MidnightLedger.zswapInputToDebugString(this._inputId!);
  }

  /**
   * Disposes of the input, freeing native resources.
   */
  dispose(): void {
    if (this._inputId !== null) {
      MidnightLedger.disposeZswapInput(this._inputId);
      this._inputId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._inputId === null) {
      throw new Error('ZswapInput has been disposed');
    }
  }
}
