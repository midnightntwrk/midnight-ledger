# `transient-crypto` Changelog

## Version `2.1.1`

- security: `VerifierKey` serialization is now independent of whether the key has
  been initialized: the bytes a key was decoded from are preserved, so
  initializing in place (which is shared across clones) no longer changes what
  the key serializes to.
- security: reject a verifier key with trailing bytes after the encoded key, so
  that two encodings cannot map to the same key.
- fix: rehashing serde deserialized `MerkleTree`s

## Version `2.1.0`

- feat: add new `try_update` and `try_update_hash` Merkle tree insertion variants that do not panic on collapsed trees
- feat: add new variants to `find_path_for_leaf` for finding hashes and scanning index ranges

## Version `2.0.0`

- breaking: pull in breaking midnight-zk changes
- breaking: bugfix: correctly exclude identity point in elliptic curve
  encryption

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
