import { NativeModules, Platform } from 'react-native';

const LINKING_ERROR =
  `The package 'react-native-ledger-ffi' doesn't seem to be linked. Make sure to: \n\n` +
  Platform.select({ ios: "- You have run 'cd ios && pod install'\n", default: '' }) +
  '- You rebuilt the app after installing the package\n' +
  '- You are not using Expo managed workflow\n';

const LedgerFFINative = NativeModules.LedgerFFI
  ? NativeModules.LedgerFFI
  : new Proxy(
      {},
      {
        get() {
          throw new Error(LINKING_ERROR);
        },
      }
    );

// Custom Types
export enum TokenType {
  Unshielded = 'Unshielded',
  Shielded = 'Shielded',
  Dust = 'Dust',
}

export enum ClaimKind {
  Reward = 'Reward',
  CardanoBridge = 'CardanoBridge',
}

export interface HashOutputWrapper {
  bytes: number[];
}

export interface PublicKey {
  hash: HashOutputWrapper;
}

export interface UserAddress {
  hash: HashOutputWrapper;
}

export interface ShieldedTokenType {
  hash: HashOutputWrapper;
}

export interface UnshieldedTokenType {
  hash: HashOutputWrapper;
}

export interface Commitment {
  hash: HashOutputWrapper;
}

export interface Nullifier {
  hash: HashOutputWrapper;
}

export interface Nonce {
  hash: HashOutputWrapper;
}

export interface ShieldedCoinInfo {
  nonce: Nonce;
  token_type: ShieldedTokenType;
  value: number;
}

export interface TransactionHash {
  hash: number[];
}

export interface IntentHash {
  hash: number[];
}

export interface UtxoOutput {
  value: number;
  owner: UserAddress;
  token_type: UnshieldedTokenType;
}

export interface UtxoSpend {
  value: number;
  owner: number[];
  token_type: UnshieldedTokenType;
  intent_hash: IntentHash;
  output_no: number;
}

export interface OutputInstructionShielded {
  amount: number;
  target_key: PublicKey;
}

export interface OutputInstructionUnshielded {
  amount: number;
  target_address: UserAddress;
  nonce: Nonce;
}

// Error types
export interface FfiError {
  InvalidInput?: { details: string };
  SerializationError?: { details: string };
  DeserializationError?: { details: string };
  CryptoError?: { details: string };
  LedgerError?: { details: string };
}

export interface LedgerFFIInterface {
  // Basic functions
  hello(): Promise<string>;
  
  // Token type functions
  nativeToken(): Promise<TokenType>;
  feeToken(): Promise<TokenType>;
  shieldedToken(): Promise<TokenType>;
  unshieldedToken(): Promise<TokenType>;
  
  // Sample data functions
  sampleCoinPublicKey(): Promise<PublicKey>;
  sampleEncryptionPublicKey(): Promise<PublicKey>;
  sampleIntentHash(): Promise<IntentHash>;
  
  // Type creation functions
  createShieldedCoinInfo(tokenType: ShieldedTokenType, value: number): Promise<ShieldedCoinInfo>;
  
  // Cryptographic functions
  coinNullifier(coinInfo: ShieldedCoinInfo, coinSecretKey: string): Promise<string>;
  coinCommitment(coinInfo: ShieldedCoinInfo, coinPublicKey: PublicKey): Promise<Commitment>;
  addressFromKey(key: string): Promise<UserAddress>;
  
  // Type conversion functions
  shieldedTokenTypeFromBytes(bytes: number[]): Promise<ShieldedTokenType>;
  unshieldedTokenTypeFromBytes(bytes: number[]): Promise<UnshieldedTokenType>;
  publicKeyFromBytes(bytes: number[]): Promise<PublicKey>;
  userAddressFromBytes(bytes: number[]): Promise<UserAddress>;
  commitmentFromBytes(bytes: number[]): Promise<Commitment>;
  nullifierFromBytes(bytes: number[]): Promise<Nullifier>;
  nonceFromBytes(bytes: number[]): Promise<Nonce>;
  transactionHashFromBytes(bytes: number[]): Promise<TransactionHash>;
  intentHashFromBytes(bytes: number[]): Promise<IntentHash>;
  
  // Type conversion to bytes
  shieldedTokenTypeToBytes(tokenType: ShieldedTokenType): Promise<number[]>;
  unshieldedTokenTypeToBytes(tokenType: UnshieldedTokenType): Promise<number[]>;
  publicKeyToBytes(publicKey: PublicKey): Promise<number[]>;
  userAddressToBytes(userAddress: UserAddress): Promise<number[]>;
  commitmentToBytes(commitment: Commitment): Promise<number[]>;
  nullifierToBytes(nullifier: Nullifier): Promise<number[]>;
  nonceToBytes(nonce: Nonce): Promise<number[]>;
  transactionHashToBytes(transactionHash: TransactionHash): Promise<number[]>;
  intentHashToBytes(intentHash: IntentHash): Promise<number[]>;
  
  // Transaction and proving functions
  createProvingTransactionPayload(transaction: any): Promise<any>;
  createProvingPayload(provingKeyMaterial: any): Promise<any>;
  createCheckPayload(wrappedIr: any): Promise<any>;
  parseCheckResult(result: any): Promise<any>;
}

export const LedgerFFI: LedgerFFIInterface = LedgerFFINative as LedgerFFIInterface;

// Helper functions for type validation and conversion
export const TypeHelpers = {
  /**
   * Validates that a byte array has exactly 32 bytes
   */
  validateHashBytes(bytes: number[]): boolean {
    return bytes.length === 32;
  },

  /**
   * Converts a hex string to byte array
   */
  hexToBytes(hex: string): number[] {
    const bytes = [];
    for (let i = 0; i < hex.length; i += 2) {
      bytes.push(parseInt(hex.substr(i, 2), 16));
    }
    return bytes;
  },

  /**
   * Converts a byte array to hex string
   */
  bytesToHex(bytes: number[]): string {
    return bytes.map(b => b.toString(16).padStart(2, '0')).join('');
  },

  /**
   * Creates a HashOutputWrapper from a byte array
   */
  createHashOutputWrapper(bytes: number[]): HashOutputWrapper {
    if (!this.validateHashBytes(bytes)) {
      throw new Error('Hash bytes must be exactly 32 bytes long');
    }
    return { bytes };
  },

  /**
   * Creates a PublicKey from a hex string
   */
  createPublicKeyFromHex(hex: string): PublicKey {
    const bytes = this.hexToBytes(hex);
    return { hash: this.createHashOutputWrapper(bytes) };
  },

  /**
   * Creates a UserAddress from a hex string
   */
  createUserAddressFromHex(hex: string): UserAddress {
    const bytes = this.hexToBytes(hex);
    return { hash: this.createHashOutputWrapper(bytes) };
  },

  /**
   * Creates a ShieldedTokenType from a hex string
   */
  createShieldedTokenTypeFromHex(hex: string): ShieldedTokenType {
    const bytes = this.hexToBytes(hex);
    return { hash: this.createHashOutputWrapper(bytes) };
  },

  /**
   * Creates an UnshieldedTokenType from a hex string
   */
  createUnshieldedTokenTypeFromHex(hex: string): UnshieldedTokenType {
    const bytes = this.hexToBytes(hex);
    return { hash: this.createHashOutputWrapper(bytes) };
  },
};

export default LedgerFFI;
