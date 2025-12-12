# iOS Module Structure

This directory contains the iOS native code for the `react-native-ledger-ffi` module.

## Files

- **`LedgerFFI.swift`** - Main Swift wrapper class that provides React Native bridge interfaces
- **`LedgerFFI.m`** - Objective-C implementation file for React Native module registration
- **`LedgerFFI-Bridging-Header.h`** - Bridging header for Swift/Objective-C interoperability
- **`libledger_uniffi.a`** - Rust static library built for iOS (contains the actual ledger functionality)

## How it works

1. **Rust Library**: The `libledger_uniffi.a` file contains the compiled Rust code with UniFFI bindings
2. **Swift Wrapper**: `LedgerFFI.swift` provides React Native bridge methods that can be called from JavaScript
3. **Module Registration**: `LedgerFFI.m` registers the Swift class as a React Native module
4. **Bridging**: The bridging header allows Swift and Objective-C to work together

## Adding new functionality

To expose new ledger functions to React Native:

1. Add methods to `LedgerFFI.swift` with `@objc` annotations
2. Add corresponding `RCT_EXTERN_METHOD` declarations in `LedgerFFI.m`
3. Call the UniFFI-generated functions from the Rust library

## Example

```swift
// In LedgerFFI.swift
@objc
func createWallet(_ resolve: @escaping RCTPromiseResolveBlock, rejecter reject: @escaping RCTPromiseRejectBlock) {
    do {
        // Call UniFFI-generated function from Rust library
        let wallet = LedgerUniffi.createWallet()
        resolve(wallet)
    } catch {
        reject("ERROR", "Failed to create wallet", error)
    }
}
```

```objc
// In LedgerFFI.m
RCT_EXTERN_METHOD(createWallet:(RCTPromiseResolveBlock)resolve
                  rejecter:(RCTPromiseRejectBlock)reject)
```
