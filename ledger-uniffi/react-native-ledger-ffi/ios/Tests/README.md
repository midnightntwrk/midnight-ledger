# iOS Module Tests

This directory contains Swift tests for the `react-native-ledger-ffi` iOS module.

## Test Files

- **`LedgerFFIModuleTest.swift`** - Main test file that tests the `hello` function from the Rust library
- **`TestTarget.swift`** - Test target configuration file
- **`run_tests.sh`** - Test runner script

## Test Structure

The tests mirror the Android test structure and include:

### `testHello()`
- Tests the `hello()` function directly from the Rust library via UniFFI
- Verifies the function returns "hello" as expected
- Handles both success and error cases

### `testHelloFromSwiftWrapper()`
- Tests the Swift wrapper class instantiation
- Would test React Native bridge integration (requires bridge setup)

### `testNativeToken()`
- Tests another function from the Rust library (`nativeToken()`)
- Demonstrates testing multiple functions

## Running the Tests

### Option 1: Using the Test Runner Script
```bash
cd ios/Tests
./run_tests.sh
```

### Option 2: In Xcode
1. Open the project in Xcode
2. Create a new iOS Unit Test target
3. Add the test files to the target
4. Link against `libledger_uniffi.a`
5. Set up the module map for UniFFI bindings
6. Run the tests (Cmd+U)

### Option 3: Command Line
```bash
swift test --package-path .
```

## Test Dependencies

- **XCTest framework** - iOS testing framework
- **ledger_uniffi library** - Rust static library
- **UniFFI-generated Swift bindings** - Auto-generated Swift interface

## Expected Results

When tests pass, you should see:
```
Success: hello() returned: hello
Success: nativeToken() returned: [token value]
```

## Troubleshooting

If tests fail:
1. Ensure `libledger_uniffi.a` exists in the parent directory
2. Ensure UniFFI Swift bindings are generated
3. Check that the module map is properly configured
4. Verify the library is built for the correct architecture
