# `midnight-onchain-runtime` Changelog

## Version `1.0.0`

- version bump in preparation for full stablisation
- breaking: contract states are now wrapped in `ChargedState`, which does
  accounting for state space that has been paid for.
- breaking: major refactor of runtime cost-model
- feat: add `key_location` parameter to proof data serializer.

## Version `0.5.0`

- breaking: pull in breaking serialization changes.
- breaking: remove `checkProofData`. This has been moved to the `zkir` wasm
  crate in a different format.
  - `check` in this module requires a `KeyMaterialProvider`, and a serialized
    preimage as inputs instead, and is an async function.
  - The `KeyMaterialProvider` can provide dummy prover/verifier keys, and must
    provide the binary format IR, rather than the JSON IR.
  - New helper methods have been provided to allow constructing the required
    inputs, including `proofDataIntoSerializedPreimage` in this module, and
    `jsonIrToBinary` in the new zkir module.
- feat: add `proofDataIntoSerializedPreimage`, outputting the right format for
  using the above mentioned replacement for `checkProofData`.

## Version `0.4.0`

- Feature: Added `balance` field to `ContractState`, and added logic
  to constrain values of this map to `0`.

  Also extended `Effects` to contain unshielded token information, and
  added `CallContext` struct.

- Made `Op` and `Key` `Storable`
- Reexported breaking changes in `midnight-coin-structure`

## Version `0.3.0`

- breaking: chore: bump to pull in breaking serialization format changes.
- breaking: chore: bump to pull in breaking `transient-crypto` `0.5.0` changes.

## Version `0.2.6`

- Bug fix: Browser compatibility

## Version `0.2.5`

- Versioning updates to contract state.

## Version `0.2.4`

- Updated to use `transient-crypto`.

## Version `0.2.3`

- Updated to `coin-structure-0.3`, catching the breaking change in a backwards
  compatible way

## Version `0.2.2`

- Added missing sequence number to effects type.
- Fix incorrect JS conversion of contract addresses in effects.
- Remove conditions for null pointers from WASM API.

## Version `0.2.1`

- Pulled in storage changes
- Fixed network id parameter in serialize functions

## Version `0.2.0`

- Pull in serialization format changes
- Pull in base data structure performance improvements
- Make network ID a serialization parameter, and remove it from string-form
  values, such as `ContractAddress`
- Deprecate `QueryContext.intoTranscript` in favor of ledger's new
  `partitionTranscripts`.

## Version `0.2.0-beta.3`

- Expose various API endpoints that were missed properly.

## Version `0.2.0-beta.2`

- Add counter to contract maintenance authorities
- Bump various dependencies

## Version `0.2.0-beta.1`

- Add contract maintenance authorities to contract states
  - These consist of a sequence of signature verifying keys, and a theshold
  - Semantics are not defined here.
- Expose signing keys to wasm
- Prepare `ContractOperation` for the ability to support containing multiple
  versions of keys simultaneously.
- Inherit breaking upstream changes.

## Version `0.2.0-alpha.2`

Initial tracked release.
