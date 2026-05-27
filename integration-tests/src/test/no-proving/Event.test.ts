// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import type { EncodedStateValue, Event, LogEventType } from '@midnight-ntwrk/ledger';
import { arrayCell, intCell } from '@/test/utils/value-alignment';
import { LogEventTypeMarker } from '@/test/utils/Markers';
import { applyDoLogCall, type LogCallOutcome } from '@/test/utils/log-contract';
import { TestState } from '@/test/utils/TestState';

interface ContractLogEvent {
  tag: 'contractLog';
  address: string;
  entryPoint: Uint8Array | string;
  loggedItem: { version: number; eventType: LogEventType; data: EncodedStateValue };
}

const versionedTuple = (version: bigint, type: bigint, payload: EncodedStateValue): EncodedStateValue =>
  arrayCell([intCell(version, 4), intCell(type, 1), payload]);

const findContractLog = (events: Event[]): ContractLogEvent => {
  const log = events.find((e) => (e.content as { tag: string }).tag === 'contractLog');
  expect(log, 'expected a contractLog event in result.events').toBeDefined();
  return log!.content as ContractLogEvent;
};

const emitLog = (logValue: EncodedStateValue, gasBytes?: bigint): LogCallOutcome =>
  applyDoLogCall(TestState.new(), logValue, gasBytes);

describe('Ledger API - Event - ContractLog', () => {
  /**
   * Test that applying a contract call which executes a `log` opcode produces
   * an Event of variant `contractLog` carrying the call's address, entry
   * point, and the decoded loggedItem
   *
   * @given A deployed contract with a `doLog` entry point that logs a
   *        versioned tuple (version=1, type=8 Paused, payload=u64(42))
   * @when Applying a transaction that calls the entry point
   * @then `result.events` contains a `contractLog` event whose `address`,
   *       `entryPoint`, and `loggedItem` match the call and payload
   */
  test('apply emits a contractLog event with address, entryPoint, and decoded loggedItem', () => {
    const payload = intCell(42n, 8);
    const value = versionedTuple(1n, 8n, payload);
    const { result, address, entryPoint } = emitLog(value);

    expect(result.type).toBe('success');
    const log = findContractLog(result.events);

    expect(log.address).toBe(address);
    expect(log.entryPoint).toBe(entryPoint);
    expect(log.loggedItem.version).toBe(1);
    expect(log.loggedItem.eventType).toBe(LogEventTypeMarker.paused);
    expect(log.loggedItem.data).toEqual(payload);
  });

  /**
   * Test that a bare value (not a versioned tuple) surfaces e2e as a
   * version-0 / misc event.
   *
   * @given A bare u64 cell logged from a contract call
   * @when Applying the call transaction
   * @then The contractLog event has version=0, eventType='misc', and the raw
   *       value as data
   */
  test('bare value (non-tuple) surfaces as version 0 / misc with the raw value as data', () => {
    const value = intCell(7n, 8);
    const { result } = emitLog(value);

    const log = findContractLog(result.events);
    expect(log.loggedItem.version).toBe(0);
    expect(log.loggedItem.eventType).toBe(LogEventTypeMarker.misc);
    expect(log.loggedItem.data).toEqual(value);
  });

  /**
   * Test that every defined LogEventType discriminant byte round-trips to its
   * matching string when emitted from a real contract call
   *
   * @given A deployed contract emitting a versioned tuple with each of the 11
   *        valid type bytes (0..10)
   * @when Applying the call transaction
   * @then The contractLog event's `loggedItem.eventType` matches the expected
   *       marker, and the version and data round-trip through the ledger
   */
  test.each<[bigint, LogEventType]>([
    [0n, LogEventTypeMarker.shieldedSpend],
    [1n, LogEventTypeMarker.shieldedReceive],
    [2n, LogEventTypeMarker.shieldedMint],
    [3n, LogEventTypeMarker.shieldedBurn],
    [4n, LogEventTypeMarker.unshieldedSpend],
    [5n, LogEventTypeMarker.unshieldedReceive],
    [6n, LogEventTypeMarker.unshieldedMint],
    [7n, LogEventTypeMarker.unshieldedBurn],
    [8n, LogEventTypeMarker.paused],
    [9n, LogEventTypeMarker.unpaused],
    [10n, LogEventTypeMarker.misc]
  ])('type %i surfaces as eventType=%s end-to-end', (type, expected) => {
    const payload = intCell(type + 100n, 8);
    const { result } = emitLog(versionedTuple(3n, type, payload));

    const log = findContractLog(result.events);
    expect(log.loggedItem.version).toBe(3);
    expect(log.loggedItem.eventType).toBe(expected);
    expect(log.loggedItem.data).toEqual(payload);
  });

  /**
   * Test that an Event whose content is a contractLog survives a full
   * serialize/deserialize round-trip without losing any field.
   *
   * @given An Event with `contractLog` content captured from a real call
   * @when Calling `event.serialize()` then `Event.deserialize(bytes)`
   * @then All `content` fields (address, entryPoint, loggedItem.{version,
   *       eventType, data}) and all `source` fields match the original
   */
  test('Event with contractLog content survives serialize/deserialize round-trip', () => {
    const payload = intCell(99n, 8);
    const { result } = emitLog(versionedTuple(5n, 1n, payload));

    const original = result.events.find((e) => (e.content as { tag: string }).tag === 'contractLog')!;
    const EventClass = original.constructor as unknown as { deserialize(raw: Uint8Array): Event };
    const roundTripped = EventClass.deserialize(original.serialize());

    const originalContent = original.content as ContractLogEvent;
    const roundTrippedContent = roundTripped.content as ContractLogEvent;

    expect(roundTrippedContent.tag).toBe('contractLog');
    expect(roundTrippedContent.address).toBe(originalContent.address);
    expect(roundTrippedContent.entryPoint).toEqual(originalContent.entryPoint);
    expect(roundTrippedContent.loggedItem.version).toBe(originalContent.loggedItem.version);
    expect(roundTrippedContent.loggedItem.eventType).toBe(originalContent.loggedItem.eventType);
    expect(roundTrippedContent.loggedItem.data).toEqual(originalContent.loggedItem.data);

    expect(roundTripped.source.transactionHash).toBe(original.source.transactionHash);
    expect(roundTripped.source.logicalSegment).toBe(original.source.logicalSegment);
    expect(roundTripped.source.physicalSegment).toBe(original.source.physicalSegment);
  });
});
