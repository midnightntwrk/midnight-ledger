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

import path, { dirname } from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

import {
  type ContractCallPrototype,
  type ContractDeploy,
  createShieldedCoinInfo,
  Intent,
  type MaintenanceUpdate,
  type PreBinding,
  type PreProof,
  type RawTokenType,
  type Signature,
  type SignatureEnabled,
  type ZswapInput,
  ZswapLocalState,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys
} from '@midnight-ntwrk/ledger';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

export const TEST_SEED = Number(process.env.MN_TEST_SEED) || 0x42;

// Seedable PRNG (Park-Miller) so randomized fixtures are reproducible.
let prngState = TEST_SEED % 2147483647 || 1;
export const seededRandom = (): number => {
  prngState = (prngState * 48271) % 2147483647;
  return prngState / 2147483647;
};

export const delay = (ms: number) => {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
};

export const loadBinaryFile = (pathToFile: string) => {
  const filePath = path.join(__dirname, pathToFile);
  logger.info(`Loading binary file from: ${filePath}`);
  const fileBuffer = fs.readFileSync(filePath);
  return new Uint8Array(fileBuffer);
};

interface Serializable {
  serialize: () => Uint8Array;
}

type Deserializer<T> = { deserialize: (...args: unknown[]) => T };

export function assertSerializationSuccess<T extends Serializable>(serializable: T, ...markers: unknown[]) {
  const serialized = serializable.serialize();
  const ctor = serializable.constructor as unknown as Deserializer<T>;
  const deserialized = ctor.deserialize(...markers.filter(Boolean), serialized);
  expect(deserialized.toString()).toEqual(serializable.toString());
  expect(deserialized.serialize()).toEqual(serialized);
}

export function mapFindByKey<K, V>(map: Map<K, V>, key: K): V | undefined {
  const foundKey = [...map.keys()].find((k: K) => JSON.stringify(k) === JSON.stringify(key));
  if (foundKey !== undefined) {
    return map.get(foundKey);
  }
  return undefined;
}

export const compareBigIntArrays = (arr1: bigint[], arr2: bigint[]): boolean => {
  if (arr1.length !== arr2.length) return false;

  for (let i = 0; i < arr1.length; i++) {
    if (arr1[i] !== arr2[i]) return false;
  }
  return true;
};

export const sortBigIntArray = (arr: bigint[]): bigint[] => {
  return arr.sort((a, b) => Number(a - b));
};

export const corruptSignature = (signature: Signature): Signature => {
  const bytes = Buffer.from(signature.value, 'hex');
  const randomIndex = Math.floor(seededRandom() * (bytes.length - 4));
  const randomBit = Math.floor(seededRandom() * 8);

  const bitMask = 2 ** randomBit;
  // eslint-disable-next-line no-bitwise
  bytes[4 + randomIndex] ^= bitMask;
  return { tag: signature.tag, value: bytes.toString('hex') };
};

export const generateHex = (len: number) =>
  [...Array(len)].map(() => Math.floor(seededRandom() * 16).toString(16)).join('');

/**
 * Creates a valid ZSwapInput using wallet spend method
 */
export const createValidZSwapInput = (
  value: bigint,
  tokenType: RawTokenType = generateHex(64),
  segment: number = 0,
  localState: ZswapLocalState = new ZswapLocalState()
): {
  outputLocalState: ZswapLocalState;
  zswapInput: ZswapInput<PreProof>;
} => {
  const seed = new Uint8Array(32).fill(42);
  const secretKeys = ZswapSecretKeys.fromSeed(seed);

  // Create and add coin to local state
  const coinInfo = createShieldedCoinInfo(tokenType, value);

  // Create output and apply to both states
  const output = ZswapOutput.new(coinInfo, segment, secretKeys.coinPublicKey, secretKeys.encryptionPublicKey);
  const offer = ZswapOffer.fromOutput(output, tokenType, value);

  // Apply to local state (simulates receiving the coin)
  const updatedLocalState = localState.apply(secretKeys, offer);

  // Get the coin from local state
  const coins = Array.from(updatedLocalState.coins);
  if (coins.length === 0) {
    throw new Error('No coins available to spend');
  }

  const coin = coins[0];

  // Use wallet's spend method to create ZSwapInput
  const [outputLocalState, zswapInput] = updatedLocalState.spend(secretKeys, coin, segment);

  return { outputLocalState, zswapInput };
};

export const testIntents = (
  calls: ContractCallPrototype[],
  updates: MaintenanceUpdate[],
  deploys: ContractDeploy[],
  tblock: Date
): Intent<SignatureEnabled, PreProof, PreBinding> => {
  const ttl = plus1Hour(tblock);
  let intent = Intent.new(ttl);
  calls.forEach((call) => {
    intent = intent.addCall(call);
  });
  updates.forEach((update) => {
    intent = intent.addMaintenanceUpdate(update);
  });
  deploys.forEach((deploy) => {
    intent = intent.addDeploy(deploy);
  });
  return intent;
};

export const plus1Hour = (d: Date): Date => {
  return new Date(d.getTime() + 3600 * 1000);
};
