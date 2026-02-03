// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {MidnightLedger} from './MidnightLedger';

/**
 * A ledger event that can be used to update local state.
 *
 * Events are emitted by the ledger when transactions are processed
 * and can be used to update the local wallet state.
 *
 * @example
 * ```typescript
 * // Deserialize an event from bytes (e.g., from indexer)
 * const event = Event.deserialize(eventBytes);
 *
 * // Check event type
 * if (event.isZswapEvent) {
 *   console.log('ZSwap event:', event.eventType);
 * }
 *
 * // Serialize back to bytes
 * const bytes = event.serialize();
 *
 * // Dispose when done
 * event.dispose();
 * ```
 */
export class Event {
  private _eventId: string | null;

  private constructor(eventId: string) {
    this._eventId = eventId;
  }

  /**
   * Deserializes an event from bytes.
   *
   * @param raw - The serialized event bytes
   * @returns A new Event instance
   * @throws If deserialization fails
   */
  static deserialize(raw: Uint8Array): Event {
    const eventId = MidnightLedger.deserializeEvent(raw);
    return new Event(eventId);
  }

  /**
   * Serializes the event to bytes.
   *
   * @returns The serialized event bytes
   */
  serialize(): Uint8Array {
    this.ensureNotDisposed();
    return MidnightLedger.serializeEvent(this._eventId!);
  }

  /**
   * Returns the event type as a string.
   * Possible values: "zswap_input", "zswap_output", "contract_deploy",
   * "contract_log", "param_change", "dust_initial_utxo",
   * "dust_generation_dtime_update", "dust_spend_processed", "unknown"
   */
  get eventType(): string {
    this.ensureNotDisposed();
    return MidnightLedger.eventType(this._eventId!);
  }

  /**
   * Returns true if this is a zswap (shielded coin) event.
   */
  get isZswapEvent(): boolean {
    this.ensureNotDisposed();
    return MidnightLedger.eventIsZswapEvent(this._eventId!);
  }

  /**
   * Returns true if this is a dust event.
   */
  get isDustEvent(): boolean {
    this.ensureNotDisposed();
    return MidnightLedger.eventIsDustEvent(this._eventId!);
  }

  /**
   * Returns true if this is a contract event.
   */
  get isContractEvent(): boolean {
    this.ensureNotDisposed();
    return MidnightLedger.eventIsContractEvent(this._eventId!);
  }

  /**
   * Returns true if this is a parameter change event.
   */
  get isParamChangeEvent(): boolean {
    this.ensureNotDisposed();
    return MidnightLedger.eventIsParamChangeEvent(this._eventId!);
  }

  /**
   * For ZswapInput events, returns the nullifier as hex string.
   * Returns null for other event types.
   */
  get zswapInputNullifier(): string | null {
    this.ensureNotDisposed();
    return MidnightLedger.eventZswapInputNullifier(this._eventId!);
  }

  /**
   * For ZswapOutput events, returns the commitment as hex string.
   * Returns null for other event types.
   */
  get zswapOutputCommitment(): string | null {
    this.ensureNotDisposed();
    return MidnightLedger.eventZswapOutputCommitment(this._eventId!);
  }

  /**
   * For ZswapOutput events, returns the merkle tree index.
   * Returns null for other event types.
   */
  get zswapOutputMtIndex(): number | null {
    this.ensureNotDisposed();
    return MidnightLedger.eventZswapOutputMtIndex(this._eventId!);
  }

  /**
   * Gets the internal event ID (for advanced usage with native functions).
   */
  get eventId(): string {
    this.ensureNotDisposed();
    return this._eventId!;
  }

  /**
   * Checks if the event has been disposed.
   */
  get isDisposed(): boolean {
    return this._eventId === null;
  }

  /**
   * Returns a string representation of the event.
   * @param compact - If true, returns a compact representation (ignored, for API compatibility)
   */
  toString(compact?: boolean): string {
    this.ensureNotDisposed();
    return MidnightLedger.eventToDebugString(this._eventId!);
  }

  /**
   * Disposes the event, releasing native resources.
   * After calling this method, the event cannot be used anymore.
   */
  dispose(): void {
    if (this._eventId !== null) {
      MidnightLedger.disposeEvent(this._eventId);
      this._eventId = null;
    }
  }

  private ensureNotDisposed(): void {
    if (this._eventId === null) {
      throw new Error('Event has been disposed');
    }
  }
}
