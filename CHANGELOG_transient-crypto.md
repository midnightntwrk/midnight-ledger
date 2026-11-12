# `transient-crypto` Changelog

## Unreleased

## Version `3.1.0`

- feat: add `PureGeneratorPedersen::smallest_representable`, for bounding
  transaction sizes from below.

## Version `3.0.1`

- feat: `MerkleTree::fold` combines a function over a tree's leaves in time
  linear in its distinct nodes, which `onchain-state` uses to size shared trees.

## Version `3.0.0`

- chore: require `midnight-circuits` 7.3.0 and `midnight-zk-stdlib` 2.4.0.
- breaking: upgrade to `midnight-zk-stdlib` v2 / `midnight-circuits` v7
- breaking: `Zkir` trait reworked: removed `Relation + Tagged + Deserializable`
  supertraits, added `type ProverKey` associated type, `read_raw_pk`,
  `write_raw_pk`, `load_ir_from_tagged`, `load_prover_key_from_tagged` as
  required methods, `k`/`keygen`/`keygen_vk` moved from defaults to required
- feat: `VerifierKey` preserves `original_bytes()` across initialization for
  v1/v2 round-tripping
- feat: `VerifierKey::Initialized` variant now retains the original raw bytes

## Version `2.2.0`

- breaking: `Zkir` no longer requires `Tagged + Deserializable`. Implementors
  provide `load_ir_from_tagged` and `load_prover_key_from_tagged` instead, so
  old IR and prover-key encodings can still be decoded. `ProverKey<T>` is
  `Tagged` only when `T: Tagged`.
- feat: add a `schnorr` module, providing Schnorr signatures over the embedded
  curve.
- feat: add `EmbeddedFr::from_le_bytes_wide`.
- change: decoding an `EmbeddedFr` from its field-aligned encoding reduces
  values at or above the embedded scalar field order, instead of rejecting
  them.
- fix: reject out-of-bounds `MerkleTree` update indices with
  `InvalidUpdate::IndexOutOfTree`, instead of updating the rightmost leaf
- fix: do not panic on indexing into a collapsed `MerkleTree`, but return `None` instead
- chore: drop the `k256` and `signature` dependencies; `CtOption` now comes
  from `subtle`.

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
