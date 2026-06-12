# `zkir` Changelog

## Version `2.2.0`

- feat: dual v1/v2 proving and verification pipeline
  - `VersionedInnerPK` enum holds either v1 (zk-stdlib v1) or v2 (zk-stdlib v2)
    prover keys; `read_raw_pk` defaults to v1
  - `IrSource::v2_keygen` for explicit v2 key generation (tests only)
  - Default `keygen`/`keygen_vk`/`k` delegate to v1 via pinned old crates
  - `LocalProvingProvider` uses `ir_v1::v1_prove` (old pipeline end-to-end)
- feat: `ir_v1` module with `v1_prove`, `v1_verify`, `v1_mock_verify` and
  adapters (`V1Params`, `V1Resolver`, `preimage_to_v1`)

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
