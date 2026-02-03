// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

/**
 * Expo Midnight Ledger - React Native bindings for the Midnight ledger library.
 *
 * This module provides a native implementation of the Midnight ledger API
 * that can be used as a drop-in replacement for the WASM version in React Native.
 *
 * @example
 * ```typescript
 * import {
 *   ZswapSecretKeys,
 *   Transaction,
 *   Intent,
 *   nativeToken,
 *   createShieldedCoinInfo,
 *   coinCommitment,
 * } from '@midnight-ntwrk/expo-midnight-ledger';
 *
 * // Create keys from seed
 * const keys = ZswapSecretKeys.fromSeed(seed);
 *
 * // Create a shielded coin
 * const coinInfo = createShieldedCoinInfo(nativeToken, 1000000n);
 * const commitment = coinCommitment(coinInfo, keys.coinPublicKey);
 *
 * // Create a transaction
 * const intent = Intent.create(3600);
 * const tx = Transaction.fromIntent('mainnet', intent);
 * const provenTx = tx.mockProve();
 * const boundTx = provenTx.bind();
 *
 * // Serialize for submission
 * const serialized = boundTx.serialize();
 *
 * // Clean up
 * keys.clear();
 * intent.dispose();
 * tx.dispose();
 * provenTx.dispose();
 * boundTx.dispose();
 * ```
 */

// Native module and token constants
export {
  MidnightLedger,
  nativeToken,
  feeToken,
  shieldedToken,
  unshieldedToken,
  sampleCoinPublicKey,
  sampleEncryptionPublicKey,
  signData,
  signatureVerifyingKey,
  createVerifyingKey,
  verifyingKeyAddress,
  verifyingKeyToHex,
  verifyingKeyVerify,
  addressFromKey,
  disposeVerifyingKey,
  verifySignature,
  createShieldedCoinInfo,
  coinCommitment,
  coinNullifier,
} from './MidnightLedger';

export type {MidnightLedgerNativeModule} from './MidnightLedger';

// Key management
export {ZswapSecretKeys} from './ZswapSecretKeys';
export {DustSecretKey} from './DustSecretKey';

// Ledger parameters
export {LedgerParameters, DustParameters} from './LedgerParameters';

// Transaction building
export {Transaction} from './Transaction';
export {Intent} from './Intent';
export {UnshieldedOffer} from './UnshieldedOffer';
export {UtxoSpend, UtxoOutput} from './Utxo';

// Local state management
export {ZswapLocalState} from './ZswapLocalState';
export {DustLocalState} from './DustLocalState';
export type {QualifiedDustOutput, Serializable} from './DustLocalState';

// Events
export {Event} from './Event';

// Types
export type {
  // Primitive types
  Nullifier,
  CoinCommitment,
  ContractAddressString,
  UserAddress,
  RawTokenType,
  CoinPublicKey,
  EncPublicKey,
  SignatureVerifyingKeyString,
  SigningKey,
  Signature,
  Nonce,
  TransactionHash,
  IntentHash,
  TransactionId,

  // Token types
  UnshieldedTokenType,
  ShieldedTokenType,
  DustTokenType,
  TokenType,

  // Coin types
  ShieldedCoinInfo,
  QualifiedShieldedCoinInfo,

  // UTXO types
  Utxo,
  UtxoOutputData,
  UtxoSpendData,

  // Dust types
  DustPublicKeyType,
  DustInitialNonce,
  DustNonce,
  DustCommitment,
  DustNullifier,
  DustOutputData,
  QualifiedDustOutputData,
  DustGenerationInfoData,
  DustParametersData,

  // Event types
  EventInfo,

  // Block context
  BlockContextData,

  // Cost types
  SyntheticCost,
  RunningCost,
  FeePrices,

  // Proving types
  ProvingProvider,
  ProvingKeyMaterial,

  // Transaction markers
  SignatureEnabledMarker,
  SignatureErasedMarker,
  SignaturishMarker,
  ProofMarker,
  PreProofMarker,
  NoProofMarker,
  ProofishMarker,
  BindingMarker,
  PreBindingMarker,
  NoBindingMarker,
  BindingishMarker,
  TransactionMarkers,

  // Result types
  TransactionResultData,

  // Ledger parameters
  LedgerParametersInfo,

  // Address types
  PublicAddressData,
} from './types';
