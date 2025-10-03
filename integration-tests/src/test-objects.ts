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
  type AlignedValue,
  type BlockContext,
  type CoinPublicKey,
  type ContractAddress,
  ContractDeploy,
  ContractState,
  createShieldedCoinInfo,
  dummyContractAddress,
  dummyUserAddress,
  DustParameters,
  type EncPublicKey,
  Intent,
  type IntentHash,
  type Nonce,
  type Op,
  type PreBinding,
  type PreProof,
  type PublicAddress,
  type QualifiedShieldedCoinInfo,
  type RawTokenType,
  sampleCoinPublicKey,
  sampleContractAddress,
  sampleEncryptionPublicKey,
  sampleIntentHash,
  sampleSigningKey,
  sampleUserAddress,
  type ShieldedCoinInfo,
  shieldedToken,
  type SignatureEnabled,
  signatureVerifyingKey,
  signData,
  type SigningKey,
  type TokenType,
  Transaction,
  UnshieldedOffer,
  ZswapOffer,
  ZswapOutput,
  ZswapSecretKeys
} from '@midnight-ntwrk/ledger';
import crypto from 'node:crypto';
import { generateHex, loadBinaryFile } from './test-utils';

export const VERSION_HEADER = '0200';
export const HEX_64_REGEX = /^[0-9a-fA-F]{64}$/;
export const PERSISTENT_HASH_BYTES = 32;
export const BOOLEAN_HASH_BYTES = 1;
export const U128_HASH_BYTES = 16;
export const LOCAL_TEST_NETWORK_ID = 'local-test';
export const DEFAULT_TOKEN_TYPE = Buffer.from(new Uint8Array(32)).toString('hex');
export const NIGHT_DUST_RATIO = 5_000_000_000n;
export const GENERATION_DECAY_RATE = 8_267n;
export const DUST_GRACE_PERIOD_IN_SECONDS = 3n * 60n * 60n;
export const BALANCING_OVERHEAD = 5_000_000_000_000_000n;
export const INITIAL_NIGHT_AMOUNT = 1_000_000n;
export const initialParameters = new DustParameters(
  NIGHT_DUST_RATIO,
  GENERATION_DECAY_RATE,
  DUST_GRACE_PERIOD_IN_SECONDS
);

export const ONE_KB = 1024;

export type ShieldedTokenType = { tag: 'shielded'; raw: RawTokenType };
export type UnshieldedTokenType = { tag: 'unshielded'; raw: RawTokenType };

export class Random {
  static hex = (len: number) => generateHex(len);

  static bigInt = () => {
    return (
      BigInt(Math.floor(Math.random() * Number.MAX_SAFE_INTEGER)) *
      BigInt(Math.floor(Math.random() * Number.MAX_SAFE_INTEGER))
    );
  };

  static nonce = (): Nonce => Random.hex(64);

  static parentBlockHash = () => Random.hex(64);

  static contractAddress = (): ContractAddress => sampleContractAddress();

  static userAddress = (): ContractAddress => sampleUserAddress();

  static coinPublicKey = (): CoinPublicKey => sampleCoinPublicKey();

  static encryptionPublicKey = (): EncPublicKey => sampleEncryptionPublicKey();

  static signingKey = (): SigningKey => sampleSigningKey();

  static signatureVerifyingKeyNew = () => signatureVerifyingKey(sampleSigningKey());

  static signature = () => signData(sampleSigningKey(), new Uint8Array(32));

  static tokenType = (tag: 'shielded' | 'unshielded' = 'shielded'): TokenType => ({
    tag,
    raw: Random.hex(64)
  });

  static generate32Bytes = (): Buffer => {
    const bytes = new Uint8Array(32);
    crypto.getRandomValues(bytes);
    return Buffer.from(bytes);
  };

  static shieldedTokenType = (): ShieldedTokenType => ({
    tag: 'shielded',
    raw: Random.hex(64)
  });

  static unshieldedTokenType = (): UnshieldedTokenType => ({
    tag: 'unshielded',
    raw: Random.hex(64)
  });

  static unprovenOfferFromOutput = (
    tokenType: ShieldedTokenType = Random.shieldedTokenType(),
    value: bigint = Random.bigInt(),
    targetCpk: string = Random.coinPublicKey(),
    targetEpk: string = Random.encryptionPublicKey()
  ) => {
    return ZswapOffer.fromOutput(
      ZswapOutput.new(createShieldedCoinInfo(tokenType.raw, value), 0, targetCpk, targetEpk),
      tokenType.raw,
      value
    );
  };
}

export class Static {
  static encodeFromHex = (hex: string) => Buffer.from(hex, 'hex');

  static encodeFromText = (text: string) => new TextEncoder().encode(text);

  static hex = (length: number = 64, seed: number = 42) =>
    Array.from({ length }, (_, i) => ((seed + i) % 16).toString(16)).join('');

  static bigInt = () => 124n;

  static nonce = (): Nonce => Static.hex(64, 1).replaceAll('0', 'a');

  static parentBlockHash = () => Static.hex(64, 2);

  static calcBlockTime = (initialTime: Date, addSeconds: number): Date => new Date(+initialTime + addSeconds * 1000);

  static blockTime = (blockTime: Date) => BigInt(Math.ceil(+blockTime / 1000));

  static coinPublicKey = (): CoinPublicKey => Static.hex(64, 3);

  static encryptionPublicKey = (): EncPublicKey => ZswapSecretKeys.fromSeed(new Uint8Array(32)).encryptionPublicKey;

  static contractAddress = () => dummyContractAddress();

  static userAddress = () => dummyUserAddress();

  static blockContext = (blockTime: Date): BlockContext => ({
    secondsSinceEpoch: Static.blockTime(blockTime),
    secondsSinceEpochErr: 0,
    parentBlockHash: Static.parentBlockHash()
  });

  static shieldedCoinInfo = (value: bigint = Static.bigInt()): ShieldedCoinInfo => {
    const token = shieldedToken() as ShieldedTokenType;
    return createShieldedCoinInfo(token.raw, value);
  };

  static alignedValue: AlignedValue = {
    value: [new Uint8Array()],
    alignment: [
      {
        tag: 'atom',
        value: { tag: 'field' }
      }
    ]
  };

  static alignedValueBytes: AlignedValue = {
    value: [new Uint8Array(64 * ONE_KB).fill(255)],
    alignment: [
      {
        tag: 'atom',
        value: { tag: 'bytes', length: 256 * ONE_KB }
      }
    ]
  };

  static alignedValueCompress: AlignedValue = {
    value: [new Uint8Array([1, 2])],
    alignment: [
      {
        tag: 'atom',
        value: { tag: 'compress' }
      }
    ]
  };

  static unprovenOfferFromOutput = (
    segment: number = 0,
    tokenType: ShieldedTokenType = shieldedToken() as ShieldedTokenType,
    value: bigint = Static.bigInt(),
    targetCpk: string = Static.coinPublicKey(),
    targetEpk: string = Static.encryptionPublicKey()
  ) => {
    return ZswapOffer.fromOutput(
      ZswapOutput.new(
        {
          type: tokenType.raw,
          nonce: Static.nonce(),
          value
        },
        segment,
        targetCpk,
        targetEpk
      ),
      tokenType.raw,
      value
    );
  };

  static unprovenTransactionGuaranteed = () => {
    return Transaction.fromParts('local-test', Static.unprovenOfferFromOutput());
  };

  static unprovenTransactionGuaranteedAndFallible = () => {
    return Transaction.fromParts('local-test', Static.unprovenOfferFromOutput(), Static.unprovenOfferFromOutput(1));
  };

  static unprovenTransactionGuaranteedAndFallibleAndContractCalls = (): Transaction<
    SignatureEnabled,
    PreProof,
    PreBinding
  > => {
    const contractState = new ContractState();
    const contractDeploy = new ContractDeploy(contractState);
    const contractDeploy2 = new ContractDeploy(contractState);
    const intent = Intent.new(Static.calcBlockTime(new Date(), 50))
      .addDeploy(contractDeploy)
      .addDeploy(contractDeploy2);
    const unprovenOfferGuaranteed = Static.unprovenOfferFromOutput();
    const unprovenOfferFallible = Static.unprovenOfferFromOutput(1);
    return Transaction.fromParts('local-test', unprovenOfferGuaranteed, unprovenOfferFallible, intent);
  };

  static operationsArray: Op<null>[] = [
    { noop: { n: 1 } },
    'lt',
    'eq',
    'type',
    'size',
    'new',
    'and',
    'or',
    'neg',
    'log',
    'root',
    'pop',
    { popeq: { cached: true, result: null } },
    { addi: { immediate: 42 } },
    { subi: { immediate: -1 } },
    { branch: { skip: 5 } },
    { jmp: { skip: 10 } },
    'add',
    'sub',
    { concat: { cached: true, n: 3 } },
    'member',
    { dup: { n: 2 } },
    { swap: { n: 1 } },
    { ins: { cached: true, n: 5 } },
    'ckpt'
  ];

  static trimTrailingZeros(value: Uint8Array): Uint8Array {
    let end = value.length;
    while (end > 0 && value[end - 1] === 0) {
      end -= 1;
    }
    return value.slice(0, end);
  }

  static defaultShieldedTokenType = (): ShieldedTokenType => ({
    tag: 'shielded',
    raw: DEFAULT_TOKEN_TYPE
  });

  static defaultUnshieldedTokenType = (): UnshieldedTokenType => ({
    tag: 'unshielded',
    raw: DEFAULT_TOKEN_TYPE
  });
}

export const addressToPublic = (address: string, tag: 'user' | 'contract'): PublicAddress => ({
  address,
  tag
});

export const getQualifiedShieldedCoinInfo = (
  coinInfo: ShieldedCoinInfo,
  mtIndex: bigint = 0n
): QualifiedShieldedCoinInfo => {
  return {
    type: coinInfo.type,
    nonce: coinInfo.nonce,
    value: coinInfo.value,
    mt_index: mtIndex
  };
};

export const getNewUnshieldedOffer = (
  intentHash: IntentHash = sampleIntentHash(),
  token: UnshieldedTokenType = Random.unshieldedTokenType(),
  svk: CoinPublicKey = Random.signatureVerifyingKeyNew()
): UnshieldedOffer<SignatureEnabled> =>
  UnshieldedOffer.new(
    [
      {
        value: 100n,
        owner: svk,
        type: token.raw,
        intentHash,
        outputNo: 0
      }
    ],
    [
      {
        value: 100n,
        owner: sampleUserAddress(),
        type: token.raw
      }
    ],
    [signData(sampleSigningKey(), new Uint8Array(32))]
  );

export class TestTransactionContext {
  intentHash = sampleIntentHash();
  token = Random.unshieldedTokenType();
  svk = Random.signatureVerifyingKeyNew();
  signature = Random.signature();
  userAddress = Random.userAddress();
}

export class TestResource {
  static operationVerifierKey = (): Uint8Array => {
    return loadBinaryFile('../resources/sample_vk.verifier');
  };
}
