# expo-midnight-ledger Agent Notes

## TODOs

### ZswapLocalState

1. ~~**Implement proper coin data extraction from native module**~~ ✅ COMPLETED
   - Added `QualifiedShieldedCoinInfo` struct to Rust FFI (ledger-ios/src/zswap_state.rs)
   - Added `coins_data()` method to ZswapLocalState in UDL and Rust
   - Added `zswapLocalStateCoinsData` Swift binding in MidnightLedgerModule.swift
   - Updated TypeScript `coins` getter to use the new native method
   - Returns proper `Set<QualifiedShieldedCoinInfo>` with type, nonce, value, and mt_index

2. ~~**Implement pending outputs tracking**~~ ✅ COMPLETED
   - Added `ShieldedCoinInfo` struct to Rust FFI (ledger-ios/src/zswap_state.rs)
   - Added `PendingOutputEntry` struct with commitment and coin fields
   - Added `pending_outputs_data()` method to ZswapLocalState in UDL and Rust
   - Added `zswapLocalStatePendingOutputsData` Swift binding in MidnightLedgerModule.swift
   - Updated TypeScript `pendingOutputs` getter to use the new native method
   - Returns `HermesCompatibleMap<CoinCommitment, [ShieldedCoinInfo, Date | undefined]>`
   - Note: Date is always undefined (TTL tracking follows WASM implementation pattern)

3. ~~**Implement pending spends tracking**~~ ✅ COMPLETED
   - Added `PendingSpendEntry` struct with nullifier and coin fields
   - Added `pending_spends_data()` method to ZswapLocalState in UDL and Rust
   - Added `zswapLocalStatePendingSpendsData` Swift binding in MidnightLedgerModule.swift
   - Updated TypeScript `pendingSpends` getter to use the new native method
   - Returns `HermesCompatibleMap<Nullifier, [QualifiedShieldedCoinInfo, Date | undefined]>`
   - Note: Date is always undefined (TTL tracking follows WASM implementation pattern)

## Notes

- Uses `HermesCompatibleMap` wrapper because Hermes JS engine lacks Iterator helpers (`.map()` on iterators)
- The native module uses an ID-based resource management pattern (objects stored in native dictionaries, IDs passed to JS)
- API must match WASM ledger API signatures for SDK compatibility
- `DustSecretKey.publicKey` returns `bigint` (matching WASM API) - native Rust returns big-endian hex for direct BigInt conversion

## Recent Changes

### 2026-02-02: Implemented coins getter
- Extended ledger-ios FFI with `QualifiedShieldedCoinInfo` interface
- Added `coins_data()` method that returns structured coin data instead of hex strings
- Swift binding converts native coin data to JS-compatible dictionary format
- TypeScript `coins` getter now returns actual coin objects with proper types

### 2026-02-02: Implemented pending outputs and pending spends tracking
- Extended ledger-ios FFI with `ShieldedCoinInfo`, `PendingOutputEntry`, and `PendingSpendEntry` interfaces
- Added `pending_outputs_data()` and `pending_spends_data()` methods to ZswapLocalState
- Swift bindings convert native pending data to JS-compatible dictionary format
- TypeScript getters now return actual coin objects from native state
- Implementation mirrors WASM API pattern (Date field always undefined for now)

### 2026-02-02: Implemented spend() methods for ZswapLocalState and DustLocalState
- **ZswapLocalState.spend()**:
  - Added `ZswapInput` struct to Rust FFI wrapping `zswap::Input<ProofPreimage>`
  - Added `ZswapSpendResult` struct containing new state and input
  - Added `spend()` method to ZswapLocalState in Rust and UDL
  - Added Swift bindings for ZswapInput (nullifier, contractAddress, serialize, toDebugString, dispose)
  - Added TypeScript `ZswapInput` class with full API
  - Returns `[ZswapLocalState, ZswapInput]` tuple matching WASM API

- **DustLocalState.spend()**:
  - Added `DustSpend` struct to Rust FFI wrapping `ledger::dust::DustSpend<ProofPreimageMarker>`
  - Added `DustSpendResult` struct containing new state and spend
  - Added `spend()` method to DustLocalState in Rust and UDL
  - Added Swift bindings for DustSpend (vFee, oldNullifier, newCommitment, serialize, toDebugString, dispose)
  - Added TypeScript `DustSpend` class with full API
  - Modified `dustLocalStateUtxos` to store QualifiedDustOutput objects and return IDs for spend reference
  - Returns `[DustLocalState, DustSpend]` tuple matching WASM API
  - `QualifiedDustOutput` interface extended with `id` field for spend() method
