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

import { Proof } from '@midnightntwrk/ledger';

/**
 * A serialized Proof is a `proof-versioned` discriminant byte, then the proof: for `V4` a `proof[v6]`
 * of PLONK bytes plus one accumulator (two compressed G1 points) per `verify_proof`. Lengths are SCALE compact.
 */
const TAG = Buffer.from('midnight:proof-versioned:', 'latin1');
const V2 = 1;
const V3 = 2;
const V4 = 3;
const POINT_BYTES = 48;
const G1_GENERATOR = Buffer.from(
  '97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb',
  'hex'
);

const length = (n: number): Buffer => Buffer.from([n * 4]);
const versioned = (...parts: Uint8Array[]): Buffer => Buffer.concat([TAG, ...parts]);
const v4 = (bytes: Uint8Array, ...accumulators: Uint8Array[]): Buffer =>
  versioned(Buffer.from([V4]), length(bytes.length), bytes, length(accumulators.length), ...accumulators);
const accumulator = (lhs: Uint8Array, rhs: Uint8Array): Buffer => Buffer.concat([lhs, rhs]);

describe('Ledger API - Proof', () => {
  /**
   * @given A V4 proof[v6] with PLONK bytes and no accumulators
   * @when Deserializing and serializing it again
   * @then Should show the bytes and an empty accumulator list, and reproduce the input
   */
  test('should deserialize a proof[v6] without accumulators and serialize it back unchanged', () => {
    const raw = v4(Buffer.from([9, 8, 7]));
    const proof = Proof.deserialize(raw);

    expect(proof.toString(true)).toEqual('V4(Proof { bytes: [9, 8, 7], accumulators: [] })');
    expect(Buffer.from(proof.serialize())).toEqual(raw);
  });

  /**
   * @given The hex of a V4 proof-versioned body, without the tag
   * @when Constructing a Proof from it
   * @then Should serialize to the tagged form of the same bytes
   */
  test('should construct from the hex of an untagged proof-versioned body', () => {
    const raw = v4(Buffer.from([9, 8, 7]));
    const body = raw.subarray(TAG.length).toString('hex');

    expect(Buffer.from(new Proof(body).serialize())).toEqual(raw);
  });

  /**
   * @given A V4 proof[v6] with one accumulator made of two valid G1 points
   * @when Deserializing and serializing it again
   * @then Should show one DeferredAccumulator and reproduce the input
   */
  test('should deserialize a proof[v6] carrying a deferred accumulator', () => {
    const raw = v4(Buffer.from([1]), accumulator(G1_GENERATOR, G1_GENERATOR));
    const proof = Proof.deserialize(raw);

    expect(proof.toString(true)).toMatch(
      /^V4\(Proof \{ bytes: \[1\], accumulators: \[DeferredAccumulator \{ .* \}\] \}\)$/
    );
    expect(Buffer.from(proof.serialize())).toEqual(raw);
  });

  /**
   * @given A V4 proof[v6] whose accumulator has an all-0xff point
   * @when Deserializing it
   * @then Should throw naming the curve and subgroup check
   */
  test('should reject an accumulator point that is not on the curve', () => {
    const offCurve = Buffer.alloc(POINT_BYTES, 0xff);

    expect(() => Proof.deserialize(v4(Buffer.from([1]), accumulator(G1_GENERATOR, offCurve)))).toThrow(
      /not on the curve, or outside the prime-order subgroup/
    );
  });

  /**
   * @given A V4 proof[v6] whose accumulator holds a single point
   * @when Deserializing it
   * @then Should throw
   */
  test('should reject an accumulator shorter than two points', () => {
    expect(() => Proof.deserialize(v4(Buffer.from([1]), G1_GENERATOR))).toThrow();
  });

  /**
   * @given A proof in the pre-accumulator V2 or V3 layout
   * @when Deserializing and serializing it again
   * @then Should keep its version and reproduce the input
   */
  test.each([
    ['V2', V2],
    ['V3', V3]
  ])('should still deserialize a legacy %s proof', (name, discriminant) => {
    const raw = versioned(Buffer.from([discriminant]), length(1), Buffer.from([9]));
    const proof = Proof.deserialize(raw);

    expect(proof.toString(true)).toEqual(`${name}(Proof([9]))`);
    expect(Buffer.from(proof.serialize())).toEqual(raw);
  });

  /**
   * @given A proof-versioned blob with the retired discriminant 0 or the unknown 4
   * @when Deserializing it
   * @then Should throw about the discriminant
   */
  test.each([0, 4])('should reject proof version discriminant %d', (discriminant) => {
    expect(() => Proof.deserialize(versioned(Buffer.from([discriminant]), length(0), length(0)))).toThrow(
      /discriminant for ProofVersioned/
    );
  });

  /**
   * @given A bare proof[v5] blob
   * @when Deserializing it as a Proof
   * @then Should throw about the header tag
   */
  test('should reject a proof[v5] outside the versioned wrapper', () => {
    expect(() => Proof.deserialize(Buffer.from('midnight:proof[v5]:\0', 'latin1'))).toThrow(/expected header tag/);
  });
});
