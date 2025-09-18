// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

import {
  communicationCommitment,
  communicationCommitmentRandomness,
  CostModel,
  createShieldedCoinInfo,
  decodeShieldedCoinInfo,
  decodeCoinPublicKey,
  decodeContractAddress,
  decodeQualifiedShieldedCoinInfo,
  encodeShieldedCoinInfo,
  encodeRawTokenType,
  decodeRawTokenType,
  decodeUserAddress,
  encodeCoinPublicKey,
  encodeContractAddress,
  encodeQualifiedShieldedCoinInfo,
  encodeUserAddress,
  LedgerParameters,
  partitionTranscripts,
  PreTranscript,
  QueryContext,
  runProgram,
  sampleCoinPublicKey,
  sampleSigningKey,
  sampleRawTokenType,
  signatureVerifyingKey,
  signData,
  signingKeyFromBip340,
  StateValue,
  rawTokenType,
  verifySignature,
  VmStack,
  ZswapSecretKeys,
  coinNullifier,
  coinCommitment,
  addressFromKey,
  maxField,
  bigIntModFr,
  entryPointHash,
  leafHash,
  maxAlignedSize,
  valueToBigInt,
  bigIntToValue,
  transientHash,
  transientCommit,
  persistentHash,
  persistentCommit,
  degradeToTransient,
  upgradeFromTransient,
  hashToCurve,
  ecAdd,
  ecMul,
  ecMulGenerator,
  sampleUserAddress,
  ChargedState,
  type Value,
  type Alignment,
  type AlignedValue,
  sampleContractAddress,
  runtimeCoinCommitment
} from '@midnight-ntwrk/ledger';
import {
  BOOLEAN_HASH_BYTES,
  getQualifiedShieldedCoinInfo,
  HEX_64_REGEX,
  PERSISTENT_HASH_BYTES,
  Random,
  Static,
  U128_HASH_BYTES
} from '@/test-objects';
import { expect } from 'vitest';
import { RuntimeCoinCommitmentUtils } from '@/test/utils/RuntimeCoinCommitmentUtils';

describe('Ledger API - functions', () => {
  /**
   * Test signature verification with valid signature.
   *
   * @given A message, signing key, and signature created from them
   * @when Verifying the signature against the message and verifying key
   * @then Should return true for valid signature
   */
  test('should verify signature', () => {
    const data = new TextEncoder().encode('Hello world');
    const sk = sampleSigningKey();
    const signature = signData(sk, data);
    const vk = signatureVerifyingKey(sk);

    expect(verifySignature(vk, data, signature)).toEqual(true);
  });

  /**
   * Test signature verification with invalid signature.
   *
   * @given A signature created for different message data
   * @when Verifying the signature against the original message
   * @then Should return false for invalid signature
   */
  test('should not verify invalid signature', () => {
    const data = new TextEncoder().encode('Hello world');
    const data2 = new TextEncoder().encode('Hello world2');
    const sk = sampleSigningKey();
    const signature = signData(sk, data2);
    const vk = signatureVerifyingKey(sk);

    expect(verifySignature(vk, data, signature)).toEqual(false);
  });

  /**
   * Test signature verification with empty data.
   *
   * @given Empty byte array as message data
   * @when Creating and verifying signature for empty data
   * @then Should return true for valid signature of empty data
   */
  test('should verify empty data signature', () => {
    const data = new Uint8Array();
    const sk = sampleSigningKey();
    const signature = signData(sk, data);
    const vk = signatureVerifyingKey(sk);

    expect(verifySignature(vk, data, signature)).toEqual(true);
  });

  /**
   * Test encoding and decoding of shielded coin info.
   *
   * @given A shielded coin info object
   * @when Encoding to bytes and then decoding back
   * @then Should maintain data integrity through encode/decode cycle
   */
  test('should encode and decode coin info correctly', () => {
    const coinInfo = Static.shieldedCoinInfo();
    const encoded = encodeShieldedCoinInfo(coinInfo);
    const decoded = decodeShieldedCoinInfo(encoded);

    expect(encoded).not.toEqual(coinInfo);
    expect(decoded).toEqual(coinInfo);
  });

  /**
   * Test encoding and decoding of qualified coin info.
   *
   * @given A qualified shielded coin info object
   * @when Encoding to bytes and then decoding back
   * @then Should maintain data integrity through encode/decode cycle
   */
  test('should encode and decode qualified coin info correctly', () => {
    const qualifiedCoinInfo = getQualifiedShieldedCoinInfo(Static.shieldedCoinInfo());
    const encoded = encodeQualifiedShieldedCoinInfo(qualifiedCoinInfo);
    const decoded = decodeQualifiedShieldedCoinInfo(encoded);

    expect(encoded).not.toEqual(qualifiedCoinInfo);
    expect(decoded).toEqual(qualifiedCoinInfo);
  });

  /**
   * Test encoding and decoding of contract address.
   *
   * @given A contract address object
   * @when Encoding to bytes and then decoding back
   * @then Should maintain data integrity through encode/decode cycle
   */
  test('should encode and decode contract address correctly', () => {
    const contractAddress = Static.contractAddress();
    const encoded = encodeContractAddress(contractAddress);
    const decoded = decodeContractAddress(encoded);

    expect(encoded).not.toEqual(contractAddress);
    expect(decoded).toEqual(contractAddress);
  });

  /**
   * Test encoding and decoding of user address.
   *
   * @given A user address object
   * @when Encoding to bytes and then decoding back
   * @then Should maintain data integrity through encode/decode cycle
   */
  test('should encode and decode user address correctly', () => {
    const userAddress = Static.userAddress();
    const encoded = encodeUserAddress(userAddress);
    const decoded = decodeUserAddress(encoded);

    expect(encoded).not.toEqual(userAddress);
    expect(decoded).toEqual(userAddress);
  });

  /**
   * Test encoding and decoding of raw token type.
   *
   * @given A sample raw token type object
   * @when Encoding to bytes and then decoding back
   * @then Should maintain data integrity through encode/decode cycle
   */
  test('should encode and decode raw token type correctly', () => {
    const tokenType1 = sampleRawTokenType();
    const encoded = encodeRawTokenType(tokenType1);
    const decoded = decodeRawTokenType(encoded);

    expect(encoded).not.toEqual(tokenType1);
    expect(decoded).toEqual(tokenType1);
  });

  /**
   * Test encoding and decoding of coin public key.
   *
   * @given A sample coin public key
   * @when Encoding to bytes and then decoding back
   * @then Should maintain data integrity through encode/decode cycle
   */
  test('should encode and decode coin public key correctly', () => {
    const coinPublicKey = sampleCoinPublicKey();
    const encoded = encodeCoinPublicKey(coinPublicKey);
    const decoded = decodeCoinPublicKey(encoded);

    expect(encoded).not.toEqual(coinPublicKey);
    expect(decoded).toEqual(coinPublicKey);
  });

  /**
   * Test communication commitment length constraint.
   *
   * @given Aligned values and communication commitment randomness
   * @when Creating a communication commitment
   * @then Should have length less than or equal to 114 bytes
   */
  test('should generate communication commitment with valid length', () => {
    const communicationCommitment1 = communicationCommitment(
      Static.alignedValue,
      Static.alignedValueCompress,
      communicationCommitmentRandomness()
    );

    expect(communicationCommitment1.length).toBeLessThanOrEqual(114);
  });

  /**
   * Test transcript partitioning functionality.
   *
   * @given Two pre-transcripts with different query contexts and operations
   * @when Partitioning transcripts using ledger parameters
   * @then Should return exactly 2 transcripts with expected program operations and empty effects
   */
  test('should return 2 transcripts from partition transcripts', () => {
    const preTranscript = new PreTranscript(
      new QueryContext(new ChargedState(StateValue.newArray()), Random.contractAddress()),
      [{ noop: { n: 0 } }],
      communicationCommitment(Static.alignedValue, Static.alignedValueCompress, communicationCommitmentRandomness())
    );
    const preTranscript2 = new PreTranscript(
      new QueryContext(new ChargedState(StateValue.newNull()), Random.contractAddress()),
      [{ noop: { n: 1 } }],
      communicationCommitment(Static.alignedValueCompress, Static.alignedValue, communicationCommitmentRandomness())
    );
    const transcripts = partitionTranscripts([preTranscript, preTranscript2], LedgerParameters.initialParameters());

    expect(transcripts).toHaveLength(2);
    expect(transcripts.at(0)).toBeDefined();
    expect(transcripts.at(1)).toBeDefined();
    expect(transcripts.at(0)?.at(0)?.program).toEqual([{ noop: { n: 0 } }]);
    expect(transcripts.at(0)?.at(0)?.effects).toEqual({
      claimedContractCalls: [],
      claimedNullifiers: [],
      claimedShieldedReceives: [],
      claimedShieldedSpends: [],
      shieldedMints: new Map(),
      unshieldedMints: new Map(),
      unshieldedInputs: new Map(),
      unshieldedOutputs: new Map(),
      claimedUnshieldedSpends: new Map()
    });
    expect(transcripts.at(0)?.at(0)?.gas.computeTime).toBeGreaterThanOrEqual(1n);
    expect(transcripts.at(1)?.at(0)?.program).toEqual([{ noop: { n: 1 } }]);
    expect(transcripts.at(1)?.at(0)?.effects).toEqual({
      claimedContractCalls: [],
      claimedNullifiers: [],
      claimedShieldedReceives: [],
      claimedShieldedSpends: [],
      shieldedMints: new Map(),
      unshieldedMints: new Map(),
      unshieldedInputs: new Map(),
      unshieldedOutputs: new Map(),
      claimedUnshieldedSpends: new Map()
    });
    expect(transcripts.at(1)?.at(0)?.gas.computeTime).toBeGreaterThanOrEqual(1n);
  });

  /**
   * Test program execution on VM stack.
   *
   * @given A VM stack with an array state value and 'size' operation
   * @when Running the program with cost model
   * @then Should execute successfully with expected gas cost and stack state
   */
  test('should run program on VM stack successfully', () => {
    const vmStack = new VmStack();
    vmStack.push(StateValue.newArray(), true);
    const vmResults = runProgram(vmStack, ['size'], CostModel.initialCostModel(), undefined);

    expect(vmResults.stack.isStrong(0)).toEqual(true);
    expect(vmResults.stack.isStrong(1)).toEqual(undefined);
    expect(vmResults.gasCost.computeTime).toBeGreaterThanOrEqual(1n);
    expect(vmResults.toString()).toMatch(/VmResults.*/);
    expect(vmResults.events.length).toEqual(0);
  });

  /**
   * Test raw token type creation.
   *
   * @given A 32-byte array and random contract address
   * @when Creating a raw token type
   * @then Should return 64-character hexadecimal string
   */
  test('should create raw token type with valid format', () => {
    const tokenType1 = rawTokenType(new Uint8Array(32), Random.contractAddress());

    expect(tokenType1.toString().length).toEqual(64);
    expect(tokenType1).toMatch(/^[0-9a-fA-F]{64}$/);
  });

  /**
   * Test shielded coin info creation.
   *
   * @given A random shielded token type and value of 10,000
   * @when Creating shielded coin info
   * @then Should have correct value and type properties
   */
  test('should create shielded coin info with correct properties', () => {
    const type = Random.shieldedTokenType();
    const coinInfo = createShieldedCoinInfo(type.raw, 10_000n);

    expect(coinInfo.value).toEqual(10_000n);
    expect(coinInfo.type).toEqual(type.raw);
  });

  /**
   * Test coin nullifier generation.
   *
   * @given ZswapSecretKeys and shielded coin info
   * @when Generating coin nullifier
   * @then Should return valid 64-character hexadecimal nullifier
   */
  test('should generate valid coin nullifier', () => {
    const secretKeys = ZswapSecretKeys.fromSeedRng(new Uint8Array(32).fill(1));
    const coinInfo = Static.shieldedCoinInfo();
    const nullifier = coinNullifier(coinInfo, secretKeys.coinSecretKey);

    expect(nullifier.toString()).toMatch(HEX_64_REGEX);
  });

  /**
   * Test address derivation from signing key.
   *
   * @given A signature verifying key
   * @when Deriving address from the key
   * @then Should return address different from the original key
   */
  test('should derive address from key', () => {
    const svk = signatureVerifyingKey(sampleSigningKey());
    const address = addressFromKey(svk);

    expect(address).not.toEqual(svk);
  });

  /**
   * Test maximum field value constant.
   *
   * @given The maxField function
   * @when Calling maxField
   * @then Should return the expected maximum prime field value
   */
  test('should return correct maximum field value', () => {
    const maxFieldValue = maxField();

    expect(maxFieldValue).toEqual(52435875175126190479447740508185965837690552500527637822603658699938581184512n);
  });

  /**
   * Test BigInt modular arithmetic for prime field.
   *
   * @given Values within and outside prime field bounds
   * @when Applying modular arithmetic operation
   * @then Should return valid value for in-bounds input and throw for out-of-bounds
   */
  test('should handle BigInt modular arithmetic correctly', () => {
    expect(bigIntModFr(maxField())).toEqual(maxField());
    expect(() => bigIntModFr(maxField() + 1n)).toThrow('out of bounds for prime field');
    expect(() => bigIntModFr(-1n)).toThrow("Invalid character '-' at position 0");
  });

  /**
   * Test entry point hash generation from string.
   *
   * @given An entry point string
   * @when Generating hash for the entry point
   * @then Should return 64-character hexadecimal hash
   */
  test('should generate entry point hash from string', () => {
    const entryPoint = 'testEntryPoint';
    const hash = entryPointHash(entryPoint);

    expect(hash).toMatch(HEX_64_REGEX);
    expect(hash.length).toEqual(64);
  });

  /**
   * Test entry point hash generation from Uint8Array.
   *
   * @given An entry point as encoded Uint8Array
   * @when Generating hash for the entry point
   * @then Should return 64-character hexadecimal hash
   */
  test('should generate entry point hash from Uint8Array', () => {
    const entryPoint = new TextEncoder().encode('testEntryPoint');
    const hash = entryPointHash(entryPoint);

    expect(hash).toMatch(HEX_64_REGEX);
    expect(hash.length).toEqual(64);
  });

  /**
   * Test degradation from persistent to transient value.
   *
   * @given A persistent value representation
   * @when Degrading to transient form
   * @then Should return different transient value with same structure
   */
  test('should degrade persistent to transient value', () => {
    const persistent: Value = [new Uint8Array(32)];

    const transient = degradeToTransient(persistent);
    expect(transient).toBeInstanceOf(Array);
    expect(transient[0]).toBeInstanceOf(Uint8Array);
    expect(transient[0]).not.toEqual(persistent[0]);
  });

  /**
   * Test upgrade from transient to persistent value.
   *
   * @given A transient value representation
   * @when Upgrading to persistent form
   * @then Should return different persistent value with same structure
   */
  test('should upgrade transient to persistent value', () => {
    const transient: Value = [new Uint8Array(32)];

    const persistent = upgradeFromTransient(transient);
    expect(persistent).toBeInstanceOf(Array);
    expect(persistent[0]).toBeInstanceOf(Uint8Array);
    expect(persistent[0]).not.toEqual(transient[0]);
  });

  /**
   * Test leaf hash generation.
   *
   * @given An aligned value
   * @when Generating leaf hash
   * @then Should return valid hash with proper structure
   */
  test('should generate leaf hash', () => {
    const value = Static.alignedValue;
    const hash = leafHash(value);

    expect(hash).toBeDefined();
    expect(hash.value).toBeInstanceOf(Array);
    expect(hash.alignment).toBeInstanceOf(Array);
  });

  /**
   * Test maximum aligned size calculation.
   *
   * @given An alignment specification
   * @when Calculating maximum aligned size
   * @then Should return positive BigInt size value
   */
  test('should calculate maximum aligned size', () => {
    const alignment: Alignment = [
      {
        tag: 'atom',
        value: { tag: 'field' }
      }
    ];
    const size = maxAlignedSize(alignment);

    expect(typeof size).toBe('bigint');
    expect(size).toBeGreaterThan(0n);
  });

  /**
   * Test bidirectional conversion between Value and BigInt.
   *
   * @given A Value representing field element
   * @when Converting to BigInt and back to Value
   * @then Should maintain proper structure through conversion cycle
   */
  test('should convert between Value and BigInt bidirectionally', () => {
    const testValue: Value = [new Uint8Array(32)]; // Field element representation

    const bigIntVal = valueToBigInt(testValue);
    expect(typeof bigIntVal).toBe('bigint');

    const backToValue = bigIntToValue(bigIntVal);
    expect(backToValue).toBeInstanceOf(Array);
    expect(backToValue[0]).toBeInstanceOf(Uint8Array);
  });

  /**
   * Test BigInt to Value conversion.
   *
   * @given A BigInt value
   * @when Converting to Value format
   * @then Should return proper Value structure with Uint8Array
   */
  test('should convert BigInt to Value format', () => {
    const testBigInt = 42n;
    const value = bigIntToValue(testBigInt);

    expect(value).toBeInstanceOf(Array);
    expect(value[0]).toBeInstanceOf(Uint8Array);
  });

  /**
   * Test transient hash generation.
   *
   * @given Alignment and value specifications
   * @when Generating transient hash
   * @then Should return hash as Uint8Array structure
   */
  test('should generate transient hash', () => {
    const alignment: Alignment = [{ tag: 'atom', value: { tag: 'bytes', length: 16 } }];
    const value = [new Uint8Array([...Array(16).keys()])];

    const hash = transientHash(alignment, value);
    expect(hash).toBeInstanceOf(Array);
    expect(hash[0]).toBeInstanceOf(Uint8Array);
  });

  test('transientCommit', () => {
    const alignment: Alignment = [{ tag: 'atom', value: { tag: 'bytes', length: 16 } }];
    const value = [new Uint8Array([...Array(16).keys()])];
    const opening = [new Uint8Array(32).fill(99)];

    const commitment = transientCommit(alignment, value, opening);
    expect(commitment).toBeInstanceOf(Array);
    expect(commitment[0]).toBeInstanceOf(Uint8Array);
  });

  test('persistentHash', () => {
    const alignment: Alignment = [{ tag: 'atom', value: { tag: 'bytes', length: 16 } }];
    const value = [new Uint8Array([...Array(16).keys()])];

    const hash = persistentHash(alignment, value);
    expect(hash).toBeInstanceOf(Array);
    expect(hash[0]).toBeInstanceOf(Uint8Array);
  });

  test('persistentCommit', () => {
    const alignment: Alignment = [{ tag: 'atom', value: { tag: 'bytes', length: 16 } }];
    const value = [new Uint8Array([...Array(16).keys()])];
    const opening = [new Uint8Array(32).fill(99)];

    const commitment = persistentCommit(alignment, value, opening);
    expect(commitment).toBeInstanceOf(Array);
    expect(commitment[0]).toBeInstanceOf(Uint8Array);
  });

  test('hashToCurve', () => {
    const alignment: Alignment = [{ tag: 'atom', value: { tag: 'bytes', length: 16 } }];
    const value = [new Uint8Array([...Array(16).keys()])];

    const curvePoint = hashToCurve(alignment, value);
    expect(curvePoint).toBeInstanceOf(Array);
    expect(curvePoint[0]).toBeInstanceOf(Uint8Array);
  });

  test('ecAdd', () => {
    const scalar1: Value = [new Uint8Array(32).fill(1)];
    const scalar2: Value = [new Uint8Array(32).fill(2)];

    const pointA = ecMulGenerator(scalar1);
    const pointB = ecMulGenerator(scalar2);

    const result = ecAdd(pointA, pointB);

    expect(result).toHaveLength(2);
    expect(result[0]).toBeInstanceOf(Uint8Array);
    expect(result[0]).toHaveLength(32); // EC point size
    expect(result[0]).not.toEqual(pointA[0]); // Result differs from inputs
    expect(result[0]).not.toEqual(pointB[0]);
  });

  test('ecMul', () => {
    const scalar1: Value = [new Uint8Array(32).fill(3)];
    const point = ecMulGenerator(scalar1);

    const scalar2: Value = [new Uint8Array(32).fill(4)];

    const result = ecMul(point, scalar2);

    expect(result).toHaveLength(2);
    expect(result[0]).toBeInstanceOf(Uint8Array);
    expect(result[0]).toHaveLength(32);
    expect(result[0]).not.toEqual(point[0]);
  });

  test('ecMulGenerator', () => {
    const scalar: Value = [new Uint8Array(32).fill(3)];

    const result = ecMulGenerator(scalar);
    expect(result).toBeInstanceOf(Array);
    expect(result[0]).toBeInstanceOf(Uint8Array);
  });

  test('coinCommitment', () => {
    const coin = Static.shieldedCoinInfo();
    const coinPublicKey = sampleCoinPublicKey();

    const commitment = coinCommitment(coin, coinPublicKey);

    expect(typeof commitment).toBe('string');
    expect(commitment).toMatch(HEX_64_REGEX);
    expect(commitment.length).toEqual(64);
  });

  test('coinCommitment with different coins produces different commitments', () => {
    const coin1 = createShieldedCoinInfo(sampleRawTokenType(), 1000n);
    const coin2 = createShieldedCoinInfo(sampleRawTokenType(), 2000n);
    const coinPublicKey = sampleCoinPublicKey();

    const commitment1 = coinCommitment(coin1, coinPublicKey);
    const commitment2 = coinCommitment(coin2, coinPublicKey);

    expect(commitment1).not.toEqual(commitment2);
    expect(commitment1).toMatch(HEX_64_REGEX);
    expect(commitment2).toMatch(HEX_64_REGEX);
  });

  test('coinCommitment with same coin and different keys produces different commitments', () => {
    const coin = Static.shieldedCoinInfo();
    const coinPublicKey1 = sampleCoinPublicKey();
    const coinPublicKey2 = sampleCoinPublicKey();

    const commitment1 = coinCommitment(coin, coinPublicKey1);
    const commitment2 = coinCommitment(coin, coinPublicKey2);

    expect(commitment1).not.toEqual(commitment2);
    expect(commitment1).toMatch(HEX_64_REGEX);
    expect(commitment2).toMatch(HEX_64_REGEX);
  });

  test('coinCommitment deterministic for same inputs', () => {
    const coin = Static.shieldedCoinInfo();
    const coinPublicKey = sampleCoinPublicKey();

    const commitment1 = coinCommitment(coin, coinPublicKey);
    const commitment2 = coinCommitment(coin, coinPublicKey);

    expect(commitment1).toEqual(commitment2);
  });

  test('signingKeyFromBip340', () => {
    // Test with valid 32-byte private key
    const validPrivateKey = new Uint8Array(32).fill(1);

    const signingKey = signingKeyFromBip340(validPrivateKey);

    // Verify it's a valid signing key by testing sign/verify cycle
    const testData = new TextEncoder().encode('test message');
    const signature = signData(signingKey, testData);
    const verifyingKey = signatureVerifyingKey(signingKey);

    expect(verifySignature(verifyingKey, testData, signature)).toBe(true);
    expect(typeof signingKey).toBe('string');
    expect(signingKey.length).toBeGreaterThan(0);
  });

  test('signingKeyFromBip340 with different inputs produces different keys', () => {
    const privateKey1 = new Uint8Array(32).fill(1);
    const privateKey2 = new Uint8Array(32).fill(2);

    const signingKey1 = signingKeyFromBip340(privateKey1);
    const signingKey2 = signingKeyFromBip340(privateKey2);

    expect(signingKey1).not.toEqual(signingKey2);
  });

  test('signingKeyFromBip340 is deterministic', () => {
    const privateKey = new Uint8Array(32).fill(42);

    const signingKey1 = signingKeyFromBip340(privateKey);
    const signingKey2 = signingKeyFromBip340(privateKey);

    expect(signingKey1).toEqual(signingKey2);
  });

  test('signingKeyFromBip340 with invalid key size should throw', () => {
    const invalidKey = new Uint8Array(31); // Wrong size

    expect(() => signingKeyFromBip340(invalidKey)).toThrow('signature error');
  });

  /**
   * Test for generating a runtime coin commitment for a contract address
   *
   * @given An aligned value for coin and recipient with a contract address
   * @when Generating a runtime coin commitment
   * @then Should return valid commitment as AlignedValue
   */
  test('should generate runtime coin commitment for a contract address', () => {
    const coin = RuntimeCoinCommitmentUtils.getShieldedCoinInfoAsAlignedValue();
    const recipient = RuntimeCoinCommitmentUtils.getContractRecipient();

    RuntimeCoinCommitmentUtils.assertOutcomes(coin, recipient);
  });

  /**
   * Test for generating a runtime coin commitment for a user address
   *
   * @given An aligned value for coin and recipient with a user address
   * @when Generating a runtime coin commitment
   * @then Should return valid commitment as AlignedValue
   */
  test('should generate runtime coin commitment for a user address', () => {
    const coin = RuntimeCoinCommitmentUtils.getShieldedCoinInfoAsAlignedValue();

    const userAddress = sampleUserAddress();
    const encodedUserAddress = encodeUserAddress(userAddress);
    const isUserAddress = true;

    const recipient: AlignedValue = {
      value: [
        RuntimeCoinCommitmentUtils.getArrayForIsLeft(isUserAddress),
        Static.trimTrailingZeros(encodedUserAddress),
        new Uint8Array([])
      ],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: BOOLEAN_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } }
      ]
    };

    RuntimeCoinCommitmentUtils.assertOutcomes(coin, recipient);
  });

  /**
   * Test for generating a runtime coin commitment for a contract address when a user address is not empty
   *
   * @given An aligned value for coin and recipient with a contract address and a not empty user address
   * @when Generating a runtime coin commitment
   * @then Should throw an error
   */
  test('should not generate runtime coin commitment for a contract address when a user address is not empty', () => {
    const coin = RuntimeCoinCommitmentUtils.getShieldedCoinInfoAsAlignedValue();

    const contractAddress = sampleContractAddress();
    const encodedContractAddress = encodeContractAddress(contractAddress);
    const isUserAddress = false;

    const recipient: AlignedValue = {
      value: [
        RuntimeCoinCommitmentUtils.getArrayForIsLeft(isUserAddress),
        new Uint8Array(PERSISTENT_HASH_BYTES),
        Static.trimTrailingZeros(encodedContractAddress)
      ],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: BOOLEAN_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } }
      ]
    };

    expect(() => runtimeCoinCommitment(coin, recipient)).toThrow(
      'Error: value deserialized as aligned failed alignment check'
    );
  });

  /**
   * Test for generating a runtime coin commitment for a contract address when is_left suggests user address
   *
   * @given An aligned value for coin and recipient with a contract address and is_left suggesting user address
   * @when Generating a runtime coin commitment
   * @then Should throw an error
   */
  test('should not generate runtime coin commitment for a contract address when is_left suggests user address', () => {
    const coin = RuntimeCoinCommitmentUtils.getShieldedCoinInfoAsAlignedValue();

    const contractAddress = sampleContractAddress();
    const encodedContractAddress = encodeContractAddress(contractAddress);
    const invalidIsUserAddress = true;

    const recipient: AlignedValue = {
      value: [
        RuntimeCoinCommitmentUtils.getArrayForIsLeft(invalidIsUserAddress),
        new Uint8Array(),
        Static.trimTrailingZeros(encodedContractAddress)
      ],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: BOOLEAN_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } }
      ]
    };

    expect(() => runtimeCoinCommitment(coin, recipient)).toThrow(
      'failed to decode for built-in type () after successful typecheck'
    );
  });

  /**
   * Test for generating a runtime coin commitment for a contract address when the length of contract address is incorrect
   *
   * @given An aligned value for coin and recipient with a contract address and incorrect length of contract address
   * @when Generating a runtime coin commitment
   * @then Should throw an error
   */
  test('should not generate runtime coin commitment for a contract address when the length of contract address is incorrect', () => {
    const invalidContractAddressBytes = 10;
    const coin = RuntimeCoinCommitmentUtils.getShieldedCoinInfoAsAlignedValue();

    const contractAddress = sampleContractAddress();
    const encodedContractAddress = encodeContractAddress(contractAddress);
    const isUserAddress = false;

    const recipient: AlignedValue = {
      value: [
        RuntimeCoinCommitmentUtils.getArrayForIsLeft(isUserAddress),
        new Uint8Array(),
        Static.trimTrailingZeros(encodedContractAddress)
      ],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: BOOLEAN_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: invalidContractAddressBytes } }
      ]
    };

    expect(() => runtimeCoinCommitment(coin, recipient)).toThrow(
      'Error: value deserialized as aligned failed alignment check'
    );
  });

  /**
   * Test for generating a runtime coin commitment without trimming trailing zeros from color
   *
   * @given A malformed aligned value for coin (without trimming trailing zeros from color) and recipient with a contract address
   * @when Generating a runtime coin commitment
   * @then Should throw an error
   */
  test('should not generate runtime coin commitment without trimming trailing zeros from color', () => {
    const coinInfo = Static.shieldedCoinInfo(10_000n);
    const encoded = encodeShieldedCoinInfo(coinInfo);
    const value = bigIntToValue(encoded.value);
    const coin: AlignedValue = {
      value: [encoded.nonce, encoded.color, value[0]],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: U128_HASH_BYTES } }
      ]
    };
    const recipient: AlignedValue = RuntimeCoinCommitmentUtils.getContractRecipient();

    expect(() => runtimeCoinCommitment(coin, recipient)).toThrow(
      'Error: value deserialized as aligned failed alignment check'
    );
  });

  /**
   * Test for generating a runtime coin commitment without proper length of bytes for nonce
   *
   * @given A malformed aligned value for coin (without proper length of bytes for nonce) and recipient with a contract address
   * @when Generating a runtime coin commitment
   * @then Should throw an error
   */
  test('should not generate runtime coin commitment without proper length of bytes for nonce', () => {
    const invalidCoinNonceBytes = 10;
    const coinInfo = Static.shieldedCoinInfo(10_000n);
    const encoded = encodeShieldedCoinInfo(coinInfo);
    const value = bigIntToValue(encoded.value);
    const coin: AlignedValue = {
      value: [encoded.nonce, Static.trimTrailingZeros(encoded.color), value[0]],
      alignment: [
        { tag: 'atom', value: { tag: 'bytes', length: invalidCoinNonceBytes } },
        { tag: 'atom', value: { tag: 'bytes', length: PERSISTENT_HASH_BYTES } },
        { tag: 'atom', value: { tag: 'bytes', length: U128_HASH_BYTES } }
      ]
    };
    const recipient: AlignedValue = RuntimeCoinCommitmentUtils.getContractRecipient();

    expect(() => runtimeCoinCommitment(coin, recipient)).toThrow(
      'Error: value deserialized as aligned failed alignment check'
    );
  });

  test.todo('createZswapInput');
});
