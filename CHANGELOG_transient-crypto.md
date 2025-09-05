# `transient-crypto` Changelog

## Version `1.0.0`

- version bump in preparation for full stablisation

## Version `0.6.0`

- breaking: pull in breaking serialization changes.
- breaking: move IR to `zkir` crate.
- breaking: parameterise prover keys by IR.
- breaking: update to `midnight-circuits` v4.
- feat: add new `Zkir` trait to allow custom IRs.
- feat: add `ProvingProvider` trait, abstracting the prover from over
  *arbitrary* IRs.

## Version `0.5.1`

- bug fix for conversion between `EmbeddedFr` and `Fr`

## Version `0.5.0`

- breaking: feat: Switch from Pluto-Eris to BLS12-381.
- breaking: feat: Switched to using data providers instead of direct prover
  keys and parameters.
- breaking: feat: Embed verifier parameters in the compiled library.
- added: Merkle trees (moved from `storage`)
- feat: Add prover key deserialization caching, to avoid churn if the same keys
  are reused, but not kept alive, as is the case in core usage patterns.

## Version `0.4.2`

- Split `base-crypto` into `base-crypto` and `transient-crypto`.
