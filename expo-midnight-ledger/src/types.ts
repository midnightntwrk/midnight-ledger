// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

/**
 * Type definitions for the Expo Midnight Ledger module.
 * These types mirror the ledger-v7 WASM API for drop-in replacement.
 */

// =============================================================================
// Primitive Types
// =============================================================================

/**
 * A Zswap nullifier, as a hex-encoded 256-bit bitstring
 */
export type Nullifier = string;

/**
 * A Zswap coin commitment, as a hex-encoded 256-bit bitstring
 */
export type CoinCommitment = string;

/**
 * A contract address, as a hex-encoded string
 */
export type ContractAddressString = string;

/**
 * A user public key address, as a hex-encoded string
 */
export type UserAddress = string;

/**
 * The internal identifier attached to a TokenType, as a hex-encoded string.
 */
export type RawTokenType = string;

/**
 * A user public key capable of receiving Zswap coins, as a hex-encoded string
 */
export type CoinPublicKey = string;

/**
 * An encryption public key, used to inform users of new coins sent to them
 */
export type EncPublicKey = string;

/**
 * A hex-encoded signature BIP-340 verifying key, with a 3-byte version prefix
 */
export type SignatureVerifyingKeyString = string;

/**
 * A hex-encoded signature BIP-340 signing key, with a 3-byte version prefix
 */
export type SigningKey = string;

/**
 * A hex-encoded signature BIP-340 signature, with a 3-byte version prefix
 */
export type Signature = string;

/**
 * A Zswap nonce, as a hex-encoded 256-bit string
 */
export type Nonce = string;

/**
 * The hash of a transaction, as a hex-encoded 256-bit bytestring
 */
export type TransactionHash = string;

/**
 * The hash of an intent, as a hex-encoded 256-bit bytestring
 */
export type IntentHash = string;

/**
 * A transaction identifier, used to index merged transactions
 */
export type TransactionId = string;

// =============================================================================
// Token Types
// =============================================================================

/**
 * Unshielded token type (or color), as a hex-encoded string
 */
export interface UnshieldedTokenType {
  tag: 'unshielded';
  raw: RawTokenType;
}

/**
 * Shielded token type (or color), as a hex-encoded string
 */
export interface ShieldedTokenType {
  tag: 'shielded';
  raw: RawTokenType;
}

/**
 * Dust token type
 */
export interface DustTokenType {
  tag: 'dust';
}

/**
 * A token type (or color)
 */
export type TokenType = UnshieldedTokenType | ShieldedTokenType | DustTokenType;

// =============================================================================
// Coin Types
// =============================================================================

/**
 * Information required to create a new coin
 */
export interface ShieldedCoinInfo {
  /**
   * The coin's type, identifying the currency it represents
   */
  type: RawTokenType;
  /**
   * The coin's randomness, preventing it from colliding with other coins
   */
  nonce: Nonce;
  /**
   * The coin's value, in atomic units dependent on the currency
   */
  value: bigint;
}

/**
 * Information required to spend an existing coin
 */
export interface QualifiedShieldedCoinInfo extends ShieldedCoinInfo {
  /**
   * The coin's location in the chain's Merkle tree of coin commitments
   */
  mt_index: bigint;
}

// =============================================================================
// UTXO Types
// =============================================================================

/**
 * An unspent transaction output
 */
export interface Utxo {
  /**
   * The amount of tokens this UTXO represents
   */
  value: bigint;
  /**
   * The address owning these tokens.
   */
  owner: UserAddress;
  /**
   * The token type of this UTXO
   */
  type: RawTokenType;
  /**
   * The hash of the intent outputting this UTXO
   */
  intentHash: IntentHash;
  /**
   * The output number of this UTXO in its parent Intent.
   */
  outputNo: number;
}

/**
 * An output appearing in an Intent.
 */
export interface UtxoOutputData {
  /**
   * The amount of tokens this UTXO represents
   */
  value: bigint;
  /**
   * The address owning these tokens.
   */
  owner: UserAddress;
  /**
   * The token type of this UTXO
   */
  type: RawTokenType;
}

/**
 * An input appearing in an Intent.
 */
export interface UtxoSpendData {
  /**
   * The amount of tokens this UTXO represents
   */
  value: bigint;
  /**
   * The signing key owning these tokens.
   */
  owner: SignatureVerifyingKeyString;
  /**
   * The token type of this UTXO
   */
  type: RawTokenType;
  /**
   * The hash of the intent outputting this UTXO
   */
  intentHash: IntentHash;
  /**
   * The output number of this UTXO in its parent Intent.
   */
  outputNo: number;
}

// =============================================================================
// Dust Types
// =============================================================================

export type DustPublicKeyType = bigint;
export type DustInitialNonce = string;
export type DustNonce = bigint;
export type DustCommitment = bigint;
export type DustNullifier = bigint;

export interface DustOutputData {
  initialValue: bigint;
  owner: DustPublicKeyType;
  nonce: DustNonce;
  seq: number;
  ctime: Date;
  backingNight: DustInitialNonce;
}

export interface QualifiedDustOutputData extends DustOutputData {
  mtIndex: bigint;
}

export interface DustGenerationInfoData {
  value: bigint;
  owner: DustPublicKeyType;
  nonce: DustInitialNonce;
  dtime: Date | undefined;
}

export interface DustParametersData {
  nightDustRatio: bigint;
  generationDecayRate: bigint;
  dustGracePeriodSeconds: bigint;
}

// =============================================================================
// Event Types
// =============================================================================

export interface EventInfo {
  eventType: string;
  isZswapEvent: boolean;
  isDustEvent: boolean;
  isContractEvent: boolean;
  isParamChangeEvent: boolean;
  zswapInputNullifier?: string;
  zswapOutputCommitment?: string;
  zswapOutputMtIndex?: number;
}

// =============================================================================
// Block Context
// =============================================================================

export interface BlockContextData {
  /**
   * The seconds since the UNIX epoch that have elapsed
   */
  secondsSinceEpoch: bigint;
  /**
   * The maximum error on secondsSinceEpoch that should occur, as a
   * positive seconds value
   */
  secondsSinceEpochErr: number;
  /**
   * The hash of the block prior to this transaction, as a hex-encoded string
   */
  parentBlockHash: string;
}

// =============================================================================
// Cost Types
// =============================================================================

/**
 * A modelled cost of a transaction or block.
 */
export interface SyntheticCost {
  /**
   * The amount of (modelled) time spent reading from disk, measured in picoseconds.
   */
  readTime: bigint;
  /**
   * The amount of (modelled) time spent in single-threaded compute, measured in picoseconds.
   */
  computeTime: bigint;
  /**
   * The number of bytes of blockspace used
   */
  blockUsage: bigint;
  /**
   * The net number of (modelled) bytes written.
   */
  bytesWritten: bigint;
  /**
   * The number of (modelled) bytes written temporarily or overwritten.
   */
  bytesChurned: bigint;
}

/**
 * A running tally of synthetic resource costs.
 */
export interface RunningCost {
  readTime: bigint;
  computeTime: bigint;
  bytesWritten: bigint;
  bytesDeleted: bigint;
}

/**
 * The fee prices for transaction
 */
export interface FeePrices {
  overallPrice: number;
  readFactor: number;
  computeFactor: number;
  blockUsageFactor: number;
  writeFactor: number;
}

// =============================================================================
// Proving Types
// =============================================================================

/**
 * Interface for providing proofs.
 * Implement this to use a remote proving server or on-device proving.
 */
export interface ProvingProvider {
  /**
   * Check if a proof preimage is valid and return binding inputs.
   * @param preimage - The serialized proof preimage
   * @param keyLocation - Location/identifier for the proving key
   * @returns Array of binding inputs (undefined for invalid)
   */
  check(preimage: Uint8Array, keyLocation: string): Promise<(bigint | undefined)[]>;

  /**
   * Generate a proof from a preimage.
   * @param preimage - The serialized proof preimage
   * @param keyLocation - Location/identifier for the proving key
   * @param bindingInput - Optional binding input to override
   * @returns The serialized proof
   */
  prove(preimage: Uint8Array, keyLocation: string, bindingInput?: bigint): Promise<Uint8Array>;
}

/**
 * Contains the raw file contents required for proving
 */
export interface ProvingKeyMaterial {
  proverKey: Uint8Array;
  verifierKey: Uint8Array;
  ir: Uint8Array;
}

// =============================================================================
// Transaction Markers
// =============================================================================

/**
 * Signature state marker - signatures enabled
 */
export interface SignatureEnabledMarker {
  readonly instance: 'signature';
}

/**
 * Signature state marker - signatures erased
 */
export interface SignatureErasedMarker {
  readonly instance: 'signature-erased';
}

export type SignaturishMarker = SignatureEnabledMarker | SignatureErasedMarker;

/**
 * Proof state marker - actual proof
 */
export interface ProofMarker {
  readonly instance: 'proof';
}

/**
 * Proof state marker - pre-proof (proof preimage)
 */
export interface PreProofMarker {
  readonly instance: 'pre-proof';
}

/**
 * Proof state marker - no proof
 */
export interface NoProofMarker {
  readonly instance: 'no-proof';
}

export type ProofishMarker = ProofMarker | PreProofMarker | NoProofMarker;

/**
 * Binding state marker - bound
 */
export interface BindingMarker {
  readonly instance: 'binding';
}

/**
 * Binding state marker - pre-binding
 */
export interface PreBindingMarker {
  readonly instance: 'pre-binding';
}

/**
 * Binding state marker - no binding
 */
export interface NoBindingMarker {
  readonly instance: 'no-binding';
}

export type BindingishMarker = BindingMarker | PreBindingMarker | NoBindingMarker;

/**
 * Markers for transaction serialization/deserialization.
 */
export interface TransactionMarkers {
  signature: 'signature' | 'signature-erased';
  proof: 'pre-proof' | 'proof' | 'no-proof';
  binding: 'pre-binding' | 'binding' | 'no-binding';
}

// =============================================================================
// Transaction Result
// =============================================================================

export interface TransactionResultData {
  type: 'success' | 'partialSuccess' | 'failure';
  successfulSegments?: Map<number, boolean>;
  error?: string;
}

// =============================================================================
// Ledger Parameters
// =============================================================================

export interface LedgerParametersInfo {
  globalTtlSeconds: number;
  transactionByteLimit: number;
  cardanoBridgeFeeBasisPoints: number;
  cardanoBridgeMinAmount: string;
  feeOverallPrice: number;
  feeReadFactor: number;
  feeComputeFactor: number;
  feeBlockUsageFactor: number;
  feeWriteFactor: number;
  minClaimableRewards: string;
}

// =============================================================================
// Address Types
// =============================================================================

/**
 * A public address that an entity can be identified by
 */
export type PublicAddressData = {tag: 'user'; address: UserAddress} | {tag: 'contract'; address: ContractAddressString};
