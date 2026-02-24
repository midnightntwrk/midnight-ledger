
## Overview

- Goal: Provide a mobile-friendly FFI surface for the Rust ledger library, using Mozilla UniFFI for type-safe bindings and a React Native module for consumption in iOS/Android apps.
- Analogy: This is conceptually similar to ledger-wasm (which targets Web via WASM), but targeting mobile (React Native) instead.
- Status: Experimental and work-in-progress.
    - The Rust wrapper code in ledger-uniffi/src is incomplete.
    - The public API is not finalized and will change.
    - Data types exposed through the FFI are not finalized.
    - There are currently no tests for this surface.
- React Native module: react-native-ledger-ffi is a React Native package intended to bridge to the Rust API.
- Demo: rn-demo-app is a sample React Native application meant to demonstrate/use the module.


## Current state and scope

- Rust UniFFI wrapper (ledger-uniffi):
    - Several modules and object wrappers exist to model ledger concepts (transactions, parameters, token types, proofs, cost models, etc.).
    - Implementations are partial; some functions are placeholders or not yet implemented.
    - Serialization/formatting helpers exist for certain objects.
    - Error handling and type conversions are in place in parts but are not comprehensive.
    - No test suite yet (unit/integration/proptests pending).

- Public API and datatypes:
    - Not final.

- React Native module (react-native-ledger-ffi):
    - Contains scaffolding for iOS/Android and the JS/TS bridge code.
    - Intended to load and call into the UniFFI-generated bindings from React Native.

- Demo app (rn-demo-app):
    - Provides a place to manually exercise the RN module during development.
    - Meant for iterative testing and developer feedback; not production-ready.

## What works

- End-to-end greeting string from Rust (hello function) is surfaced via the UniFFI bindings and displayed in the demo app.
- The rn-demo-app illustrates the data flow from the Rust library -> UniFFI bindings -> React Native module -> mobile UI.

## What’s missing / planned work

- Finalize native module API:
- Complete implementations
- Testing:
    - Rust-side unit and integration tests for all exported functionality.
    - React Native module tests (JS/TS) and basic E2E paths with the demo app.
