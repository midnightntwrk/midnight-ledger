// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

import {requireNativeModule} from 'expo-modules-core';

/**
 * Native module interface - defines all functions exported from Swift
 */
export interface MidnightLedgerNativeModule {
  // Constants
  readonly nativeToken: string;
  readonly feeToken: string;
  readonly shieldedToken: string;
  readonly unshieldedToken: string;

  // Utility functions
  sampleCoinPublicKey(): string;
  sampleEncryptionPublicKey(): string;

  // Signing
  signData(signingKey: string, payload: Uint8Array): string;
  verifySignature(keyId: string, message: Uint8Array, signature: string): boolean;

  // SignatureVerifyingKey management
  signatureVerifyingKey(signingKey: string): string;
  createVerifyingKey(hex: string): string;
  verifyingKeyAddress(keyId: string): string;
  verifyingKeyToHex(keyId: string): string;
  verifyingKeyVerify(keyId: string, message: Uint8Array, signature: string): boolean;
  addressFromKey(keyId: string): string;
  disposeVerifyingKey(keyId: string): void;

  // Coin operations
  createShieldedCoinInfo(tokenType: string, value: number): Uint8Array;
  coinCommitment(coinInfo: Uint8Array, coinPublicKey: string): string;
  coinCommitmentFromFields(tokenType: string, nonce: string, value: number, coinPublicKey: string): string;
  coinNullifier(coinInfo: Uint8Array, coinSecretKeyId: string): string;
  coinNullifierFromFields(tokenType: string, nonce: string, value: number, coinSecretKeyId: string): string;

  // ZswapSecretKeys management
  createZswapSecretKeys(seed: Uint8Array): string;
  getZswapCoinPublicKey(keyId: string): string;
  getZswapEncryptionPublicKey(keyId: string): string;
  getZswapCoinSecretKey(keyId: string): string;
  getZswapEncryptionSecretKey(keyId: string): string;
  clearZswapSecretKeys(keyId: string): void;

  // CoinSecretKey management
  coinSecretKeyPublicKey(keyId: string): string;
  coinSecretKeySerialize(keyId: string): Uint8Array;
  clearCoinSecretKey(keyId: string): void;

  // EncryptionSecretKey management
  deserializeEncryptionSecretKey(raw: Uint8Array): string;
  encryptionSecretKeyPublicKey(keyId: string): string;
  encryptionSecretKeySerialize(keyId: string): Uint8Array;
  clearEncryptionSecretKey(keyId: string): void;

  // DustSecretKey management
  createDustSecretKey(seed: Uint8Array): string;
  getDustPublicKey(keyId: string): string;
  clearDustSecretKey(keyId: string): void;

  // Transaction operations
  createTransaction(networkId: string, intentId: string | null): string;
  createTransactionRandomized(networkId: string, intentId: string | null): string;
  deserializeTransaction(data: Uint8Array): string;
  deserializeTransactionTyped(
    signatureMarker: string,
    proofMarker: string,
    bindingMarker: string,
    data: Uint8Array
  ): string;
  transactionNetworkId(txId: string): string;
  bindTransaction(txId: string): string;
  mockProveTransaction(txId: string): string;
  mergeTransactions(txId1: string, txId2: string): string;
  transactionIdentifiers(txId: string): string[];
  eraseTransactionProofs(txId: string): string;
  eraseTransactionSignatures(txId: string): string;
  serializeTransaction(txId: string): Uint8Array;
  transactionToDebugString(txId: string): string;
  disposeTransaction(txId: string): void;

  // Intent operations
  createIntent(ttlSeconds: number): string;
  deserializeIntent(data: Uint8Array): string;
  intentTtlSeconds(intentId: string): number;
  intentSetTtl(intentId: string, ttlSeconds: number): string;
  intentSignatureData(intentId: string, segmentId: number): Uint8Array;
  intentIntentHash(intentId: string, segmentId: number): string;
  intentGuaranteedUnshieldedOffer(intentId: string): string | null;
  intentSetGuaranteedUnshieldedOffer(intentId: string, offerId: string | null): string;
  intentFallibleUnshieldedOffer(intentId: string): string | null;
  intentSetFallibleUnshieldedOffer(intentId: string, offerId: string | null): string;
  intentBind(intentId: string, segmentId: number): string;
  serializeIntent(intentId: string): Uint8Array;
  intentToDebugString(intentId: string): string;
  disposeIntent(intentId: string): void;

  // UnshieldedOffer operations
  createUnshieldedOffer(inputIds: string[], outputIds: string[], signatures: string[]): string;
  createUnshieldedOfferUnsigned(inputIds: string[], outputIds: string[]): string;
  deserializeUnshieldedOffer(data: Uint8Array): string;
  unshieldedOfferInputs(offerId: string): string[];
  unshieldedOfferOutputs(offerId: string): string[];
  unshieldedOfferSignatures(offerId: string): string[];
  unshieldedOfferAddSignatures(offerId: string, signatures: string[]): string;
  serializeUnshieldedOffer(offerId: string): Uint8Array;
  unshieldedOfferToDebugString(offerId: string): string;
  disposeUnshieldedOffer(offerId: string): void;

  // UtxoSpend operations
  createUtxoSpend(value: string, owner: string, tokenType: string, intentHash: string, outputNo: number): string;
  deserializeUtxoSpend(data: Uint8Array): string;
  utxoSpendValue(spendId: string): string;
  utxoSpendOwner(spendId: string): string;
  utxoSpendTokenType(spendId: string): string;
  utxoSpendIntentHash(spendId: string): string;
  utxoSpendOutputNo(spendId: string): number;
  serializeUtxoSpend(spendId: string): Uint8Array;
  disposeUtxoSpend(spendId: string): void;

  // UtxoOutput operations
  createUtxoOutput(value: string, owner: string, tokenType: string): string;
  deserializeUtxoOutput(data: Uint8Array): string;
  utxoOutputValue(outputId: string): string;
  utxoOutputOwner(outputId: string): string;
  utxoOutputTokenType(outputId: string): string;
  serializeUtxoOutput(outputId: string): Uint8Array;
  disposeUtxoOutput(outputId: string): void;

  // ZswapLocalState operations
  createZswapLocalState(): string;
  deserializeZswapLocalState(data: Uint8Array): string;
  zswapLocalStateFirstFree(stateId: string): number;
  zswapLocalStateCoinsCount(stateId: string): number;
  zswapLocalStateCoins(stateId: string): string[];
  zswapLocalStateCoinsData(stateId: string): Array<{
    type: string;
    nonce: string;
    value: string;
    mt_index: string;
  }>;
  zswapLocalStatePendingSpendsData(stateId: string): Array<{
    nullifier: string;
    type: string;
    nonce: string;
    value: string;
    mt_index: string;
  }>;
  zswapLocalStatePendingOutputsData(stateId: string): Array<{
    commitment: string;
    type: string;
    nonce: string;
    value: string;
  }>;
  zswapLocalStateReplayEvents(stateId: string, secretKeysId: string, eventIds: string[]): string;
  zswapLocalStateApplyCollapsedUpdate(stateId: string, updateId: string): string;
  zswapLocalStateWatchFor(stateId: string, coinPublicKey: string, coinInfo: Uint8Array): string;
  zswapLocalStateSpend(
    stateId: string,
    secretKeysId: string,
    coin: Uint8Array,
    segment: number | null
  ): {stateId: string; inputId: string};
  serializeZswapLocalState(stateId: string): Uint8Array;
  zswapLocalStateToDebugString(stateId: string): string;
  disposeZswapLocalState(stateId: string): void;

  // ZswapInput operations
  zswapInputNullifier(inputId: string): string;
  zswapInputContractAddress(inputId: string): string | null;
  serializeZswapInput(inputId: string): Uint8Array;
  zswapInputToDebugString(inputId: string): string;
  disposeZswapInput(inputId: string): void;

  // DustLocalState operations
  createDustLocalState(paramsId: string): string;
  deserializeDustLocalState(data: Uint8Array): string;
  dustLocalStateWalletBalance(stateId: string, timeSeconds: number): string;
  dustLocalStateSyncTimeSeconds(stateId: string): number;
  dustLocalStateUtxosCount(stateId: string): number;
  dustLocalStateUtxos(stateId: string): Array<{
    id: string;
    nonce: string;
    initialValue: string;
    mtIndex: string;
    ctimeSeconds: number;
    seq: number;
    owner: string;
    backingNight: string;
  }>;
  disposeQualifiedDustOutput(utxoId: string): void;
  dustLocalStateParams(stateId: string): string;
  dustLocalStateProcessTtls(stateId: string, timeSeconds: number): string;
  dustLocalStateReplayEvents(stateId: string, secretKeyId: string, eventIds: string[]): string;
  dustLocalStateSpend(
    stateId: string,
    secretKeyId: string,
    utxoId: string,
    vFee: string,
    ctimeSeconds: number
  ): {stateId: string; spendId: string};
  serializeDustLocalState(stateId: string): Uint8Array;
  dustLocalStateToDebugString(stateId: string): string;
  disposeDustLocalState(stateId: string): void;

  // DustSpend operations
  dustSpendVFee(spendId: string): string;
  dustSpendOldNullifier(spendId: string): string;
  dustSpendNewCommitment(spendId: string): string;
  serializeDustSpend(spendId: string): Uint8Array;
  dustSpendToDebugString(spendId: string): string;
  disposeDustSpend(spendId: string): void;

  // LedgerState operations
  createLedgerState(networkId: string): string;
  deserializeLedgerState(data: Uint8Array): string;
  ledgerStateNetworkId(stateId: string): string;
  serializeLedgerState(stateId: string): Uint8Array;
  ledgerStateToDebugString(stateId: string): string;
  disposeLedgerState(stateId: string): void;

  // Event operations
  deserializeEvent(data: Uint8Array): string;
  eventType(eventId: string): string;
  eventIsZswapEvent(eventId: string): boolean;
  eventIsDustEvent(eventId: string): boolean;
  eventIsContractEvent(eventId: string): boolean;
  eventIsParamChangeEvent(eventId: string): boolean;
  eventZswapInputNullifier(eventId: string): string | null;
  eventZswapOutputCommitment(eventId: string): string | null;
  eventZswapOutputMtIndex(eventId: string): number | null;
  serializeEvent(eventId: string): Uint8Array;
  eventToDebugString(eventId: string): string;
  disposeEvent(eventId: string): void;

  // MerkleTreeCollapsedUpdate operations
  deserializeMerkleTreeCollapsedUpdate(data: Uint8Array): string;
  serializeMerkleTreeCollapsedUpdate(updateId: string): Uint8Array;
  disposeMerkleTreeCollapsedUpdate(updateId: string): void;

  // DustParameters operations
  createDustParameters(nightDustRatio: number, generationDecayRate: number, dustGracePeriodSeconds: number): string;
  deserializeDustParameters(data: Uint8Array): string;
  dustParametersNightDustRatio(paramsId: string): number;
  dustParametersGenerationDecayRate(paramsId: string): number;
  dustParametersDustGracePeriodSeconds(paramsId: string): number;
  serializeDustParameters(paramsId: string): Uint8Array;
  disposeDustParameters(paramsId: string): void;

  // LedgerParameters operations
  initialLedgerParameters(): string;
  deserializeLedgerParameters(data: Uint8Array): string;
  ledgerParametersDustParams(paramsId: string): string;
  ledgerParametersGlobalTtlSeconds(paramsId: string): number;
  ledgerParametersTransactionByteLimit(paramsId: string): number;
  ledgerParametersCardanoBridgeFeeBasisPoints(paramsId: string): number;
  ledgerParametersCardanoBridgeMinAmount(paramsId: string): string;
  ledgerParametersFeeOverallPrice(paramsId: string): number;
  ledgerParametersFeeReadFactor(paramsId: string): number;
  ledgerParametersFeeComputeFactor(paramsId: string): number;
  ledgerParametersFeeBlockUsageFactor(paramsId: string): number;
  ledgerParametersFeeWriteFactor(paramsId: string): number;
  ledgerParametersMinClaimableRewards(paramsId: string): string;
  serializeLedgerParameters(paramsId: string): Uint8Array;
  ledgerParametersToDebugString(paramsId: string): string;
  disposeLedgerParameters(paramsId: string): void;

  // BlockContext operations
  createBlockContext(tblockSeconds: number): string;
  createBlockContextFull(tblockSeconds: number, tblockErr: number, parentBlockHash: string): string;
  deserializeBlockContext(data: Uint8Array): string;
  blockContextTblockSeconds(ctxId: string): number;
  blockContextTblockErr(ctxId: string): number;
  blockContextParentBlockHash(ctxId: string): string;
  serializeBlockContext(ctxId: string): Uint8Array;
  blockContextToDebugString(ctxId: string): string;
  disposeBlockContext(ctxId: string): void;

  // ContractAddress operations
  createContractAddress(hex: string): string;
  deserializeContractAddress(data: Uint8Array): string;
  contractAddressToHex(addrId: string): string;
  contractAddressCustomShieldedToken(addrId: string, domainSep: string): string;
  contractAddressCustomUnshieldedToken(addrId: string, domainSep: string): string;
  serializeContractAddress(addrId: string): Uint8Array;
  disposeContractAddress(addrId: string): void;

  // PublicAddress operations
  createPublicAddressContract(contractAddrId: string): string;
  createPublicAddressUser(userAddress: string): string;
  deserializePublicAddress(data: Uint8Array): string;
  publicAddressIsContract(addrId: string): boolean;
  publicAddressIsUser(addrId: string): boolean;
  publicAddressContractAddress(addrId: string): string | null;
  publicAddressUserAddress(addrId: string): string | null;
  publicAddressToHex(addrId: string): string;
  serializePublicAddress(addrId: string): Uint8Array;
  disposePublicAddress(addrId: string): void;

  // Dust operations
  dustUpdatedValue(
    initialValue: string,
    ctimeSeconds: number,
    genInfoId: string,
    nowSeconds: number,
    paramsId: string
  ): string;
  deserializeDustGenerationInfo(data: Uint8Array): string;
  dustGenerationInfoValue(infoId: string): string;
  dustGenerationInfoDtimeSeconds(infoId: string): number;
  serializeDustGenerationInfo(infoId: string): Uint8Array;
  disposeDustGenerationInfo(infoId: string): void;

  // DustPublicKey operations
  createDustPublicKeyFromHex(hex: string): string;
  deserializeDustPublicKey(data: Uint8Array): string;
  dustPublicKeyToHex(keyId: string): string;
  serializeDustPublicKey(keyId: string): Uint8Array;
  disposeDustPublicKey(keyId: string): void;

  // InitialNonce operations
  createInitialNonce(hex: string): string;
  deserializeInitialNonce(data: Uint8Array): string;
  initialNonceToHex(nonceId: string): string;
  serializeInitialNonce(nonceId: string): Uint8Array;
  disposeInitialNonce(nonceId: string): void;
}

// Load the native module
export const MidnightLedger = requireNativeModule<MidnightLedgerNativeModule>('MidnightLedger');

// Re-export constants
export const nativeToken = MidnightLedger.nativeToken;
export const feeToken = MidnightLedger.feeToken;
export const shieldedToken = MidnightLedger.shieldedToken;
export const unshieldedToken = MidnightLedger.unshieldedToken;

// Re-export utility functions
export const sampleCoinPublicKey = () => MidnightLedger.sampleCoinPublicKey();
export const sampleEncryptionPublicKey = () => MidnightLedger.sampleEncryptionPublicKey();

// Signing functions
export const signData = (signingKey: string, payload: Uint8Array): string =>
  MidnightLedger.signData(signingKey, payload);

// SignatureVerifyingKey functions
export const signatureVerifyingKey = (signingKey: string): string => MidnightLedger.signatureVerifyingKey(signingKey);

export const createVerifyingKey = (hex: string): string => MidnightLedger.createVerifyingKey(hex);

export const verifyingKeyAddress = (keyId: string): string => MidnightLedger.verifyingKeyAddress(keyId);

export const verifyingKeyToHex = (keyId: string): string => MidnightLedger.verifyingKeyToHex(keyId);

export const verifyingKeyVerify = (keyId: string, message: Uint8Array, signature: string): boolean =>
  MidnightLedger.verifyingKeyVerify(keyId, message, signature);

export const addressFromKey = (keyId: string): string => MidnightLedger.addressFromKey(keyId);

export const disposeVerifyingKey = (keyId: string): void => MidnightLedger.disposeVerifyingKey(keyId);

export const verifySignature = (keyId: string, message: Uint8Array, signature: string): boolean =>
  MidnightLedger.verifySignature(keyId, message, signature);

// Coin operations
export const createShieldedCoinInfo = (tokenType: string, value: bigint): Uint8Array =>
  MidnightLedger.createShieldedCoinInfo(tokenType, Number(value));

// Type for coin info object from wallet SDK
interface CoinInfoObject {
  type: string;
  nonce: string;
  value: bigint | number | string;
  mt_index?: bigint | number | string;
}

export const coinCommitment = (coinInfo: Uint8Array | CoinInfoObject, coinPublicKey: string): string => {
  // Handle object form (from wallet SDK) by calling coinCommitmentFromFields
  if (coinInfo && typeof coinInfo === 'object' && 'type' in coinInfo && 'nonce' in coinInfo && 'value' in coinInfo) {
    const obj = coinInfo as CoinInfoObject;
    // Convert value to number (handle bigint, string, or number)
    const valueNum =
      typeof obj.value === 'bigint' ? Number(obj.value) : typeof obj.value === 'string' ? Number(obj.value) : obj.value;
    return MidnightLedger.coinCommitmentFromFields(obj.type, obj.nonce, valueNum, coinPublicKey);
  }

  // Handle Uint8Array (serialized bytes)
  if (coinInfo instanceof Uint8Array) {
    return MidnightLedger.coinCommitment(coinInfo, coinPublicKey);
  }

  throw new Error(`Invalid coinInfo type: expected Uint8Array or {type, nonce, value} object`);
};

// Type for coin secret key - can be a string ID or an object with keyId
type CoinSecretKeyInput = string | {keyId?: string; _keyId?: string};

export const coinNullifier = (coinInfo: Uint8Array | CoinInfoObject, coinSecretKey: CoinSecretKeyInput): string => {
  // Extract the key ID from the coinSecretKey
  let keyId: string;
  if (typeof coinSecretKey === 'string') {
    keyId = coinSecretKey;
  } else if (coinSecretKey && typeof coinSecretKey === 'object') {
    // Handle object with keyId or _keyId property (e.g., ZswapSecretKeys.coinSecretKey result)
    keyId = (coinSecretKey as {keyId?: string}).keyId || (coinSecretKey as {_keyId?: string})._keyId || '';
  } else {
    throw new Error(`Invalid coinSecretKey: expected string or object with keyId, got ${typeof coinSecretKey}`);
  }

  if (!keyId) {
    throw new Error(`Invalid coinSecretKey: no key ID found. Make sure to pass a valid CoinSecretKey handle.`);
  }

  // Handle object form (from wallet SDK) by calling coinNullifierFromFields
  if (coinInfo && typeof coinInfo === 'object' && 'type' in coinInfo && 'nonce' in coinInfo && 'value' in coinInfo) {
    const obj = coinInfo as CoinInfoObject;
    // Convert value to number (handle bigint, string, or number)
    const valueNum =
      typeof obj.value === 'bigint' ? Number(obj.value) : typeof obj.value === 'string' ? Number(obj.value) : obj.value;
    return MidnightLedger.coinNullifierFromFields(obj.type, obj.nonce, valueNum, keyId);
  }

  // Handle Uint8Array (serialized bytes)
  if (coinInfo instanceof Uint8Array) {
    return MidnightLedger.coinNullifier(coinInfo, keyId);
  }

  throw new Error(`Invalid coinInfo type: expected Uint8Array or {type, nonce, value} object`);
};
