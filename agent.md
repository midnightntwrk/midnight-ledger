# Midnight Ledger iOS Compilation - Agent Notes

## Overview

This document tracks the work on compiling the `midnight-ledger` Rust library (v7.0.0) for iOS using UniFFI.

**Goal:** Wallet-only iOS static library with UniFFI bindings (no proving functionality)

## Project Structure

```
midnight-ledger/
├── ledger-ios/                    # UniFFI-based iOS bindings crate
│   ├── Cargo.toml
│   ├── uniffi.toml
│   ├── build.rs
│   └── src/
│       ├── lib.rs                 # Main module, namespace functions
│       ├── ledger_ios.udl         # UniFFI interface definition
│       ├── error.rs               # LedgerError enum
│       ├── keys.rs                # ZswapSecretKeys, DustSecretKey, CoinSecretKey, etc.
│       ├── util.rs                # Hex encoding/decoding utilities
│       ├── events.rs              # Event wrapper
│       ├── zswap_state.rs         # ZswapLocalState, MerkleTreeCollapsedUpdate
│       ├── dust_state.rs          # DustLocalState, DustParameters
│       ├── dust.rs                # DustPublicKey, DustGenerationInfo, DustOutput, etc.
│       ├── ledger_state.rs        # LedgerState wrapper
│       ├── unshielded.rs          # UtxoSpend, UtxoOutput, UnshieldedOffer
│       ├── intent.rs              # Intent type with offers and TTL
│       ├── transaction.rs         # Transaction building and manipulation
│       ├── parameters.rs          # LedgerParameters (Phase 8)
│       ├── block_context.rs       # BlockContext (Phase 8)
│       └── addresses.rs           # ContractAddress, PublicAddress (Phase 9)
└── agent.md                       # This file
```

## Technical Approach

### Why UniFFI (over cbindgen/manual FFI):
- Handles complex types (Transaction, Intent, etc.) naturally
- Generates idiomatic Swift code automatically
- Memory-safe handling of secret keys with Zeroize
- Better type safety across FFI boundary

### Design Principles:
- **No proving functionality** - Proving will be handled separately
- **Immutable operations** - Methods return new `Arc<T>` instead of mutating
- **Simplified type states** - Limited enum variants for common use cases
- **String-based u128** - Since UniFFI doesn't support u128

## Implementation Phases

### Phase 1-3: Keys, Tokens, Coins ✅

**Keys (keys.rs):**
- `ZswapSecretKeys` - Generates from seed, provides coin/encryption keys
- `DustSecretKey` - For dust operations
- `CoinSecretKey` - Coin secret key with public key derivation
- `EncryptionSecretKey` - For coin encryption
- `SignatureVerifyingKey` - For address derivation

**Tokens (lib.rs):**
- `shielded_token()` - Returns shielded token type
- `unshielded_token()` / `native_token()` - Returns NIGHT token
- `fee_token()` - Returns fee token type

**Coins (lib.rs):**
- `create_shielded_coin_info()` - Creates coin info with random nonce
- `coin_commitment()` - Calculates coin commitment
- `coin_nullifier()` - Calculates nullifier from secret key

### Phase 4: Local State ✅

**ZswapLocalState (zswap_state.rs):**
- Track owned shielded coins
- `replay_events()` - Update state from blockchain events
- `apply_collapsed_update()` - Apply merkle tree updates
- `watch_for()` - Add coins to watch list
- `coins()` - Get all owned coins

**DustLocalState (dust_state.rs):**
- Track dust UTXOs
- `wallet_balance()` - Get balance at given time
- `process_ttls()` - Remove expired UTXOs
- `replay_events()` - Update from events

**LedgerState (ledger_state.rs):**
- Full ledger state wrapper
- `blank()` - Create empty state for network

**Supporting Types:**
- `Event` - Blockchain event wrapper
- `MerkleTreeCollapsedUpdate` - Merkle tree update
- `DustParameters` - Dust operation parameters

### Phase 5-6: Transactions and Offers ✅

**UtxoSpend (unshielded.rs):**
- UTXO input with value, owner, token_type, intent_hash, output_no
- All values as hex-encoded strings (owner, token_type, intent_hash)
- Value as decimal string (u128)

**UtxoOutput (unshielded.rs):**
- UTXO output with value, owner, token_type
- Similar encoding as UtxoSpend

**UnshieldedOffer (unshielded.rs):**
- Collection of inputs, outputs, signatures
- `add_signatures()` - Add signatures to offer
- Inputs/outputs automatically sorted for deterministic ordering

**Intent (intent.rs):**
- User's intention to perform ledger actions
- TTL (time-to-live) in seconds since epoch
- Guaranteed and fallible unshielded offers
- `signature_data()` - Get data for signing
- `intent_hash()` - Get intent hash for segment
- `bind()` - Seal the intent with segment ID

**Transaction (transaction.rs):**
- `from_parts()` / `from_parts_randomized()` - Create transaction
- `bind()` - Seal transaction with random binding
- `mock_prove()` - For fee calculation (no real proving)
- `merge()` - Combine transactions
- `identifiers()` - Get transaction identifiers
- `erase_proofs()` / `erase_signatures()` - For inspection

### Phase 7: Dust Operations ✅

**DustPublicKey (dust.rs):**
- Wrapper for dust public key derived from secret key
- `to_hex()` - Convert to hex string
- `from_hex()` - Create from hex string
- Serialization/deserialization support

**InitialNonce (dust.rs):**
- Initial nonce for dust generation tracking
- Used to link dust to backing NIGHT UTXOs
- Serialization/deserialization support

**DustGenerationInfo (dust.rs):**
- Tracks dust generation for backing NIGHT
- `value` - Backing NIGHT value (u128 as string)
- `owner` - Dust public key owner
- `nonce` - Initial nonce
- `dtime_seconds` - Decay start time (u64::MAX for never)

**DustOutput (dust.rs):**
- Unqualified dust output
- `initial_value` - Initial dust value
- `owner` - Dust public key
- `nonce` - Random nonce (hex-encoded Fr)
- `seq` - Sequence number for re-spent dust
- `ctime_seconds` - Creation time
- `updated_value()` - Calculate current value with generation/decay

**QualifiedDustOutput (dust.rs):**
- Dust output with merkle tree index and backing NIGHT info
- All DustOutput fields plus:
- `backing_night` - Initial nonce linking to NIGHT
- `mt_index` - Merkle tree index
- `to_dust_output()` - Convert to unqualified form

**DustLocalState Updates (dust_state.rs):**
- `utxos()` - Returns all tracked QualifiedDustOutput objects
- `generation_info()` - Get generation info for a UTXO

**Namespace Functions (lib.rs):**
- `deserialize_dust_public_key(raw)` - Deserialize DustPublicKey
- `deserialize_initial_nonce(raw)` - Deserialize InitialNonce
- `deserialize_dust_generation_info(raw)` - Deserialize DustGenerationInfo
- `deserialize_dust_output(raw)` - Deserialize DustOutput
- `deserialize_qualified_dust_output(raw)` - Deserialize QualifiedDustOutput
- `create_dust_public_key(hex)` - Create from hex
- `create_initial_nonce(hex)` - Create from hex
- `dust_updated_value(...)` - Calculate dust value at time

### Phase 8: Parameters and Extended Events ✅

**LedgerParameters (parameters.rs):**
- Network-wide ledger parameters for fee calculation and limits
- `initial()` - Returns initial (genesis) parameters
- `dust_params()` - Returns dust parameters
- `global_ttl_seconds()` - Global TTL for transactions
- `transaction_byte_limit()` - Max transaction size
- `cardano_bridge_fee_basis_points()` - Bridge fee (0-10000)
- `cardano_bridge_min_amount()` - Min bridge amount (u128 as string)
- Fee price accessors (f64):
  - `fee_overall_price()` - Overall fee price (DUST per block)
  - `fee_read_factor()` - Read factor
  - `fee_compute_factor()` - Compute factor
  - `fee_block_usage_factor()` - Block usage factor
  - `fee_write_factor()` - Write factor
- `min_claimable_rewards()` - Min claimable rewards (u128 as string)

**BlockContext (block_context.rs):**
- Block context for transaction timing
- `new(tblock_seconds, tblock_err, parent_block_hash)` - Full constructor
- `default_context()` - Default with zero values
- `with_time(tblock_seconds)` - Simple time-only context
- `tblock_seconds()` - Block timestamp
- `tblock_err()` - Timestamp error margin
- `parent_block_hash()` - Parent hash (hex string)
- Serialization/deserialization support

**Extended Event (events.rs):**
- `event_type()` - Returns event type string
- `source()` - Returns EventSource with transaction info
- Type checks: `is_zswap_event()`, `is_dust_event()`, `is_contract_event()`, `is_param_change_event()`
- ZSwap accessors: `zswap_input_nullifier()`, `zswap_output_commitment()`, `zswap_output_mt_index()`

**EventSource (events.rs):**
- `transaction_hash()` - Transaction hash (hex string)
- `logical_segment()` - Logical segment number
- `physical_segment()` - Physical segment number

**Namespace Functions (lib.rs):**
- `initial_ledger_parameters()` - Get genesis parameters
- `deserialize_ledger_parameters(raw)` - Deserialize parameters
- `create_block_context(tblock_seconds)` - Create simple block context
- `create_block_context_full(tblock_seconds, tblock_err, parent_block_hash)` - Full block context
- `deserialize_block_context(raw)` - Deserialize block context

### Phase 9: Addresses and Signature Verification ✅

**ContractAddress (addresses.rs):**
- Wrapper for contract addresses (32-byte hash)
- `from_hex(hex)` - Create from hex-encoded string
- `to_hex()` - Convert to hex string
- `custom_shielded_token(domain_sep)` - Create custom shielded token type
- `custom_unshielded_token(domain_sep)` - Create custom unshielded token type
- Serialization/deserialization support

**PublicAddress (addresses.rs):**
- Discriminated union: Contract or User address
- `from_contract(contract)` - Create from ContractAddress
- `from_user(user_address)` - Create from hex-encoded user address
- `is_contract()` / `is_user()` - Type checks
- `contract_address()` - Get ContractAddress if contract type
- `user_address()` - Get user address string if user type
- `to_hex()` - Convert to hex string
- Serialization/deserialization support

**SignatureVerifyingKey Updates (keys.rs):**
- `from_hex(hex)` - Create from hex-encoded verifying key (not signing key)
- `verify(message, signature)` - Verify a signature against a message

**Namespace Functions (lib.rs):**
- `create_contract_address(hex)` - Create ContractAddress from hex
- `deserialize_contract_address(raw)` - Deserialize ContractAddress
- `create_public_address_contract(contract)` - Create PublicAddress from ContractAddress
- `create_public_address_user(user_address)` - Create PublicAddress from user address
- `deserialize_public_address(raw)` - Deserialize PublicAddress
- `create_verifying_key(hex)` - Create SignatureVerifyingKey from hex
- `verify_signature(verifying_key, message, signature)` - Verify signature

### Phase 10: Remaining (Pending)

- Additional contract-related utilities if needed

## Key Design Decisions

### 1. u128/i128 → String
UniFFI doesn't support u128/i128 types. All large integers are passed as decimal strings:
```rust
pub fn value(&self) -> String {
    self.inner.value.to_string()
}
```

### 2. Secret Key Zeroization
Keys implement secure memory clearing via `clear()` methods:
```rust
pub fn clear(&self) {
    let mut inner = self.inner.lock().unwrap();
    *inner = None;
}
```

### 3. Transaction Type States
Transaction wraps an enum to handle different states:
```rust
pub(crate) enum TransactionTypes {
    UnprovenWithSignaturePreBinding(...),
    UnprovenWithSignatureBinding(...),
    ProvenWithSignatureBinding(...),
    ProofErasedNoBinding(...),
    ProofErasedSignatureErasedNoBinding(...),
}
```

### 4. No Proving
- `mock_prove()` available for fee calculation
- Real proving removed - to be handled externally
- Proof-erased types for deserialization/inspection

### 5. Immutable Operations
All mutating methods return new `Arc<T>`:
```rust
pub fn set_ttl(&self, ttl_seconds: u64) -> Arc<Intent> {
    Arc::new(Intent { inner: /* new inner */ })
}
```

### 6. Type Marker Deserialization
Transaction deserialization supports type markers:
```rust
pub fn deserialize_typed(
    signature_marker: String,  // "signature" or "erased"
    proof_marker: String,      // "unproven", "proven", or "erased"
    binding_marker: String,    // "pre", "bound", or "none"
    raw: Vec<u8>,
) -> Result<Self, LedgerError>
```

## UDL Interface Summary

### Namespace Functions
```
// Tokens
shielded_token() -> string
unshielded_token() -> string
native_token() -> string
fee_token() -> string

// Key derivation
signature_verifying_key(signing_key) -> SignatureVerifyingKey
address_from_key(verifying_key) -> string

// Coin operations
create_shielded_coin_info(token_type, value) -> bytes
coin_commitment(coin_info, coin_public_key) -> string
coin_nullifier(coin_info, coin_secret_key) -> string

// Signing
sign_data(signing_key, payload) -> string

// Deserialization functions
deserialize_event(raw) -> Event
deserialize_merkle_tree_collapsed_update(raw) -> MerkleTreeCollapsedUpdate
deserialize_dust_parameters(raw) -> DustParameters
deserialize_zswap_local_state(raw) -> ZswapLocalState
deserialize_dust_local_state(raw) -> DustLocalState
deserialize_ledger_state(raw) -> LedgerState
deserialize_utxo_spend(raw) -> UtxoSpend
deserialize_utxo_output(raw) -> UtxoOutput
deserialize_unshielded_offer(raw) -> UnshieldedOffer
deserialize_intent(raw) -> Intent
deserialize_transaction(raw) -> Transaction
deserialize_transaction_typed(sig, proof, binding, raw) -> Transaction

// Creation functions
create_intent(ttl_seconds) -> Intent
create_transaction(network_id, intent?) -> Transaction
create_transaction_randomized(network_id, intent?) -> Transaction
create_utxo_spend(value, owner, token_type, intent_hash, output_no) -> UtxoSpend
create_utxo_output(value, owner, token_type) -> UtxoOutput
create_unshielded_offer(inputs, outputs, signatures) -> UnshieldedOffer
create_unshielded_offer_unsigned(inputs, outputs) -> UnshieldedOffer
```

// Phase 7: Dust Operations
deserialize_dust_public_key(raw) -> DustPublicKey
deserialize_initial_nonce(raw) -> InitialNonce
deserialize_dust_generation_info(raw) -> DustGenerationInfo
deserialize_dust_output(raw) -> DustOutput
deserialize_qualified_dust_output(raw) -> QualifiedDustOutput
create_dust_public_key(hex) -> DustPublicKey
create_initial_nonce(hex) -> InitialNonce
dust_updated_value(initial_value, ctime, gen_info, now, params) -> string

// Phase 8: Parameters and Extended Events
initial_ledger_parameters() -> LedgerParameters
deserialize_ledger_parameters(raw) -> LedgerParameters
create_block_context(tblock_seconds) -> BlockContext
create_block_context_full(tblock_seconds, tblock_err, parent_block_hash) -> BlockContext
deserialize_block_context(raw) -> BlockContext

// Phase 9: Addresses and Signature Verification
create_contract_address(hex) -> ContractAddress
deserialize_contract_address(raw) -> ContractAddress
create_public_address_contract(contract) -> PublicAddress
create_public_address_user(user_address) -> PublicAddress
deserialize_public_address(raw) -> PublicAddress
create_verifying_key(hex) -> SignatureVerifyingKey
verify_signature(verifying_key, message, signature) -> boolean
```

### Interfaces
```
ZswapSecretKeys, DustSecretKey, SignatureVerifyingKey
CoinSecretKey, EncryptionSecretKey
Event, EventSource, MerkleTreeCollapsedUpdate, DustParameters
ZswapLocalState, DustLocalState, LedgerState
UtxoSpend, UtxoOutput, UnshieldedOffer
Intent, Transaction
DustPublicKey, InitialNonce, DustGenerationInfo
DustOutput, QualifiedDustOutput
LedgerParameters, BlockContext
ContractAddress, PublicAddress
```

## Errors and Error Handling

```rust
pub enum LedgerError {
    InvalidData,
    InvalidSeed,
    KeysCleared,
    SerializationError(String),
    DeserializationError,
    CryptoError(String),
    TransactionError(String),
    InvalidState(String),
    NotImplemented(String),
}
```

## Build Configuration

### Dependencies (Cargo.toml)
```toml
[dependencies]
uniffi = "0.31"
hex = "0.4"
thiserror = "1"
rand = "0.8"

# Internal crates
base-crypto = { path = "../base-crypto" }
coin-structure = { path = "../coin-structure" }
ledger = { path = "../ledger" }
onchain-runtime = { path = "../onchain-runtime" }
serialize = { path = "../serialize" }
storage = { path = "../storage" }
transient-crypto = { path = "../transient-crypto" }
zswap = { path = "../zswap" }
```

### Build Commands
```bash
# Check compilation
cargo check --package midnight-ledger-ios

# Build
cargo build --package midnight-ledger-ios

# Test
cargo test --package midnight-ledger-ios
```

## Current Status

### ledger-ios (Rust → iOS)
- [x] Phase 1-3: Keys, Tokens, Coins
- [x] Phase 4: Local State (ZswapLocalState, DustLocalState, LedgerState)
- [x] Phase 5-6: Transactions and Offers (UtxoSpend, UtxoOutput, UnshieldedOffer, Intent, Transaction)
- [x] Phase 7: Dust Operations (DustPublicKey, DustGenerationInfo, DustOutput, QualifiedDustOutput)
- [x] Phase 8: Parameters and Extended Events (LedgerParameters, BlockContext, EventSource)
- [x] Phase 9: Addresses and Signature Verification (ContractAddress, PublicAddress, verify_signature)
- [x] Phase 10: Skipped (no additional utilities needed)
- [x] UDL interface definition complete for implemented phases
- [x] Cargo check passes
- [x] Cargo build passes
- [x] iOS cross-compilation (Rust 1.89 toolchain)
- [x] Swift bindings generation
- [x] XCFramework creation

### expo-midnight-ledger (iOS → React Native)
- [x] Swift bridge implementation (MidnightLedgerModule.swift, ~180 functions)
- [x] TypeScript native module interface (MidnightLedger.ts)
- [x] Wrapper classes (ZswapSecretKeys, Transaction, Intent, UnshieldedOffer, Utxo, ZswapLocalState)
- [x] Type exports (types.ts)
- [x] Module exports (index.ts)
- [x] Handle-based object management pattern
- [x] API compatibility with ledger-v7.template.d.ts (minimal subset)

## Build Artifacts

**XCFramework:** `ledger-ios/MidnightLedger.xcframework` (~207MB)
- `ios-arm64` - Physical iOS devices
- `ios-arm64_x86_64-simulator` - iOS Simulator (Apple Silicon + Intel)

**Swift Bindings:** `ledger-ios/generated/`
- `ledger_ios.swift` - Swift wrapper code (209KB)
- `ledger_iosFFI.h` - C header (162KB)
- `ledger_iosFFI.modulemap` - Module map

**Static Libraries:**
- `target/aarch64-apple-ios/release/libmidnight_ledger_ios.a` (~70MB)
- `target/aarch64-apple-ios-sim/release/libmidnight_ledger_ios.a` (~69MB)
- `target/x86_64-apple-ios/release/libmidnight_ledger_ios.a` (~69MB)

## Build Commands

```bash
# Required: Rust 1.89+ toolchain with iOS targets
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios --toolchain 1.89-aarch64-apple-darwin

# Build for iOS device
CC_aarch64_apple_ios=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang \
SDKROOT=/Applications/Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk \
IPHONEOS_DEPLOYMENT_TARGET=13.0 \
PATH="$HOME/.rustup/toolchains/1.89-aarch64-apple-darwin/bin:$PATH" \
cargo build --package midnight-ledger-ios --target aarch64-apple-ios --release

# Build for iOS simulator (arm64)
CC_aarch64_apple_ios_sim=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang \
SDKROOT=/Applications/Xcode.app/Contents/Developer/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk \
IPHONEOS_DEPLOYMENT_TARGET=13.0 \
PATH="$HOME/.rustup/toolchains/1.89-aarch64-apple-darwin/bin:$PATH" \
cargo build --package midnight-ledger-ios --target aarch64-apple-ios-sim --release

# Build for iOS simulator (x86_64)
CC_x86_64_apple_ios=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang \
SDKROOT=/Applications/Xcode.app/Contents/Developer/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk \
IPHONEOS_DEPLOYMENT_TARGET=13.0 \
PATH="$HOME/.rustup/toolchains/1.89-aarch64-apple-darwin/bin:$PATH" \
cargo build --package midnight-ledger-ios --target x86_64-apple-ios --release

# Generate Swift bindings
cargo build --package midnight-ledger-ios --bin uniffi-bindgen --release
./target/release/uniffi-bindgen generate \
    --library target/aarch64-apple-ios/release/libmidnight_ledger_ios.a \
    --language swift \
    --out-dir ledger-ios/generated

# Create universal simulator library
lipo -create \
    target/aarch64-apple-ios-sim/release/libmidnight_ledger_ios.a \
    target/x86_64-apple-ios/release/libmidnight_ledger_ios.a \
    -output ledger-ios/xcframework/libmidnight_ledger_ios-sim.a

# Create XCFramework
xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/libmidnight_ledger_ios.a \
    -headers ledger-ios/generated \
    -library ledger-ios/xcframework/libmidnight_ledger_ios-sim.a \
    -headers ledger-ios/generated \
    -output ledger-ios/MidnightLedger.xcframework
```

## Next Steps

1. Test integration in iOS app
2. Add XCFramework to Swift Package Manager or CocoaPods

---

## Expo Midnight Ledger Module

The `expo-midnight-ledger` package provides React Native bindings for the iOS library, enabling the wallet SDK to use native iOS implementation as a drop-in replacement for the WASM version.

### Project Structure

```
expo-midnight-ledger/
├── ios/
│   ├── MidnightLedger.podspec
│   ├── MidnightLedgerModule.swift    # Expo module bridge (~1970 lines)
│   └── MidnightLedger.xcframework/   # iOS framework (symlink or copy)
├── src/
│   ├── index.ts                      # Main exports
│   ├── MidnightLedger.ts             # Native module interface
│   ├── types.ts                      # TypeScript type definitions
│   ├── ZswapSecretKeys.ts            # Key management wrapper
│   ├── Transaction.ts                # Transaction wrapper
│   ├── Intent.ts                     # Intent wrapper
│   ├── UnshieldedOffer.ts            # Unshielded offer wrapper
│   ├── Utxo.ts                       # UtxoSpend/UtxoOutput wrappers
│   └── ZswapLocalState.ts            # Local state wrapper
├── expo-module.config.json
└── package.json
```

### Handle-Based Object Management

Native objects are managed via UUID strings stored in dictionaries on the Swift side:

```swift
// Swift side - MidnightLedgerModule.swift
private var zswapSecretKeys: [String: ZswapSecretKeys] = [:]
private var transactions: [String: Transaction] = [:]
private var intents: [String: Intent] = [:]
// ... ~20+ more dictionaries for each object type

Function("createZswapSecretKeys") { (seed: Data) -> String in
    let keys = try ZswapSecretKeys.fromSeed(seed: seed)
    let id = UUID().uuidString
    self.zswapSecretKeys[id] = keys
    return id
}
```

```typescript
// TypeScript side - ZswapSecretKeys.ts
export class ZswapSecretKeys {
  private _keyId: string | null;

  static fromSeed(seed: Uint8Array): ZswapSecretKeys {
    const keyId = MidnightLedger.createZswapSecretKeys(seed);
    return new ZswapSecretKeys(keyId);
  }

  clear(): void {
    if (this._keyId !== null) {
      MidnightLedger.clearZswapSecretKeys(this._keyId);
      this._keyId = null;
    }
  }
}
```

### TypeScript Wrapper Classes

| Class | Purpose |
|-------|---------|
| `ZswapSecretKeys` | Key derivation from seed, provides coin/encryption keys |
| `Transaction` | Transaction building, merging, binding |
| `Intent` | User intention with TTL and offers |
| `UnshieldedOffer` | Collection of UTXO inputs/outputs with signatures |
| `UtxoSpend` | UTXO input for spending |
| `UtxoOutput` | UTXO output for receiving |
| `ZswapLocalState` | Track owned shielded coins via Merkle tree |

### Native Module Interface (MidnightLedger.ts)

```typescript
export interface MidnightLedgerNativeModule {
  // Constants
  readonly nativeToken: string;
  readonly feeToken: string;
  readonly shieldedToken: string;
  readonly unshieldedToken: string;

  // Keys
  createZswapSecretKeys(seed: Uint8Array): string;
  zswapSecretKeysCoinPublicKey(keyId: string): string;
  zswapSecretKeysEncryptionPublicKey(keyId: string): string;
  clearZswapSecretKeys(keyId: string): void;

  // Transactions
  createTransaction(networkId: string, intentId: string | null): string;
  transactionBind(txId: string): string;
  transactionMockProve(txId: string): string;
  transactionMerge(txId: string, otherTxId: string): string;
  transactionSerialize(txId: string): Uint8Array;

  // ... ~180+ more functions
}

export const MidnightLedger = requireNativeModule<MidnightLedgerNativeModule>('MidnightLedger');
```

### Key API Differences from WASM Version

1. **No on-device proving** - Use `mockProve()` for testing; real proving requires a remote server
2. **Handle-based API** - Native objects referenced by UUID strings, not direct object references
3. **Immutable operations** - Methods like `bind()`, `merge()`, `setTtl()` return new instances
4. **Manual cleanup required** - Call `dispose()` or `clear()` to free native resources
5. **bigint as strings** - Large values passed as strings to avoid JavaScript number limitations

### Usage Example

```typescript
import {
  ZswapSecretKeys,
  Transaction,
  Intent,
  nativeToken,
  createShieldedCoinInfo,
  coinCommitment,
} from '@midnight-ntwrk/expo-midnight-ledger';

// Create keys from seed
const keys = ZswapSecretKeys.fromSeed(seed);

// Create a shielded coin
const coinInfo = createShieldedCoinInfo(nativeToken, 1000000n);
const commitment = coinCommitment(coinInfo, keys.coinPublicKey);

// Create a transaction
const intent = Intent.create(3600);
const tx = Transaction.fromIntent('mainnet', intent);
const provenTx = tx.mockProve();
const boundTx = provenTx.bind();

// Serialize for submission
const serialized = boundTx.serialize();

// Clean up - IMPORTANT!
keys.clear();
intent.dispose();
tx.dispose();
provenTx.dispose();
boundTx.dispose();
```

### Type Exports (types.ts)

All necessary types are exported for TypeScript consumers:

- **Primitives:** `Nullifier`, `CoinCommitment`, `UserAddress`, `RawTokenType`, etc.
- **Token types:** `UnshieldedTokenType`, `ShieldedTokenType`, `DustTokenType`, `TokenType`
- **Coin types:** `ShieldedCoinInfo`, `QualifiedShieldedCoinInfo`
- **UTXO types:** `Utxo`, `UtxoOutputData`, `UtxoSpendData`
- **Dust types:** `DustPublicKeyType`, `DustNonce`, `DustNullifier`, etc.
- **Transaction markers:** `SignatureEnabledMarker`, `ProofMarker`, `BindingMarker`, etc.
- **Result types:** `TransactionResultData`, `LedgerParametersInfo`
- **Address types:** `PublicAddressData`

### Swift Module Structure

The `MidnightLedgerModule.swift` follows Expo modules conventions:

```swift
public class MidnightLedgerModule: Module {
    // Private storage for all native object handles
    private var verifyingKeys: [String: SignatureVerifyingKey] = [:]
    private var zswapSecretKeys: [String: ZswapSecretKeys] = [:]
    private var transactions: [String: Transaction] = [:]
    private var intents: [String: Intent] = [:]
    private var unshieldedOffers: [String: UnshieldedOffer] = [:]
    private var utxoSpends: [String: UtxoSpend] = [:]
    private var utxoOutputs: [String: UtxoOutput] = [:]
    private var zswapLocalStates: [String: ZswapLocalState] = [:]
    private var events: [String: Event] = [:]
    private var merkleTreeUpdates: [String: MerkleTreeCollapsedUpdate] = [:]
    // ... more dictionaries

    public func definition() -> ModuleDefinition {
        Name("MidnightLedger")

        // Token constants
        Constants([
            "nativeToken": nativeToken(),
            "feeToken": feeToken(),
            "shieldedToken": shieldedToken(),
            "unshieldedToken": unshieldedToken()
        ])

        // Cleanup on module destroy
        OnDestroy {
            self.clearAllHandles()
        }

        // ~180+ Function definitions...
    }

    private func clearAllHandles() {
        // Secure cleanup of all stored objects
        for keys in zswapSecretKeys.values { keys.clear() }
        zswapSecretKeys.removeAll()
        // ... clear all dictionaries
    }
}
```

## Considerations

- **Memory:** No proving means lower memory requirements
- **Size:** XCFramework ~207MB (includes debug symbols, can be stripped)
- **iOS Minimum:** iOS 13+ for Swift compatibility
- **Threading:** All types are `Arc`-wrapped for thread safety
- **Rust Toolchain:** Requires Rust 1.89+ for edition 2024 support
