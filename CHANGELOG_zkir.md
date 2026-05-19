# `zkir` Changelog

## Unreleased

- feat: IR version 2.1, functionally identical to 2.0, but with additional optimizations
- feat: add JubjubPoint support to `TestEq`, `ConstrainEq`, and `CondSelect`
- test: pin prover/verifier key SHA-256 hashes for every precompile in
  `zkir-precompiles/` and add a V0/V1 smoke fixture in
  `zkir/tests/precompile_hashes.rs`, catching silent drift in `IrMinorVersion::V0`
  keys (which must stay byte-identical to pre-#154). Refresh after an
  intentional key change with
  `UPDATE_ZKIR_HASHES=1 cargo test -p midnight-zkir --test precompile_hashes`.

## Version `2.1.0`

- breaking: pull in breaking proof system changes
- feat: add ability to compute k value of a circuit in WASM

## Version `2.0.0`

- breaking: pull in breaking serialization changes.
- breaking: move the IR itself into the scope of `zkir`
- feat: add a wasm API to IR proving/checking
- addressed audit issues:
  - bugfix: correctly update the sliding window for in-circuit FAB bytes
    decoding only after the reversed iteration.

## Version `1.3.0`

- breaking: feat: Pull in breaking `transient-crypto` `0.5.0` change
- feat: Add `compile-many` and `mock-compile-many` subcommands
- feat: Provide better progress reporting during compilation

## Version `1.2.1`

- Update `base-crypto` to `0.4.2`, `transient-crypto` to `0.4.2`

## Version `1.2.0`

- Update base-crypto to `0.4.0`

## Version `1.1.1`

- Add `.bzkir` output, to be used in sending to the proof server.

## Version `1.1.0`

- Inherit breaking verifier key format change of `base-crypto-0.3.0`
- Add `mock-compile` subcommand

## Version `1.0.0`

- Initial tracked release
