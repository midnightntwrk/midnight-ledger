# Ledger 9.1.0.0-rc.5 Release Notes

**Version:** 9.1.0.0-rc.5
**Date:** 2026-09-08

## High-level summary

Ledger 9.1.0.0-rc.5 carries the encoding strictness fixes released to mainnet as
`8.1.2` (see PR #743) forward onto the `ledger-9` line. There are no other
changes: everything else in this release candidate is unchanged from
`9.1.0.0-rc.4`.

## Audience

This release note is important for:

- Node developers
- Developers using the ledger WASM bindings

## Summary of updates

### Ledger Updates

Security patch. Internal dependency requirements are pinned to the new exact
versions so that consumers cannot resolve past the fix.

- security: hardening of low-level deserialization across `serialize`,
  `base-crypto`, `storage`, `onchain-state`, `onchain-vm` and
  `transient-crypto`. Encodings that are not canonical, and values violating
  their type's invariant, are now rejected rather than decoded. This narrows what
  deserializes: an rc.5 node rejects data an rc.4 node accepts. See the
  per-crate changelogs for the individual rules.
- fix: `DustParameters::time_to_cap` guards against a zero
  `generation_decay_rate` instead of dividing by zero.
- fix: Dust `seq` increments saturate.
- fix: Zswap binding randomness extraction no longer panics on a proof preimage
  with no witness to extract from.
- fix: delta accumulation in `normalize_deltas` saturates.
- fix: contract call cost accounting counts public inputs via
  `ContractCall::public_inputs_len`, with saturating arithmetic, rather than
  materializing the inputs to take their length.

### Crate versions

Only two crate versions move. `midnight-storage` goes to `2.0.4` and
`midnight-storage-core` to `1.2.2`, skipping the `2.0.3` and `1.2.1` numbers that
`8.1.2` published to crates.io from the `ledger-8` line. Every other crate keeps
its version and ships under a new isolated `-rc` tag.

## New features requiring configuration updates

*None in this release.*

## Deprecations

*None in this release.*

## Breaking changes or required actions for developers

The security changes are breaking to an environment containing maliciously
formed transactions.

Rust consumers patching these crates by git tag must move to the new isolated
tags, `midnight-storage-core` included: with `storage-core` now at `1.2.2`, an
unpatched `storage-core` resolves to the `ledger-8` release on crates.io.

## Fixed defect list

| **Component** | **Description** |
|---------------|-----------------|
| serialize | `HashMap` and `HashSet` deserialization requires a normalized encoding (sorted, no duplicate keys); the non-canonical encodings that previously decoded to the same value are rejected. |
| serialize | Non-canonical `ScaleBigInt` encodings, which use the 4-byte form for a value that fits a smaller form, are rejected. |
| serialize | `Box<T>` deserialization counts against the recursion depth budget. |
| serialize | `Vec::with_bounded_capacity` no longer divides by zero for zero-sized types. |
| serialize | `serialized_size` for `HashMap` and `HashSet` accounts for the actual width of the length prefix. |
| base-crypto | Non-canonical `Value` and `ValueAtom` encodings are rejected: a singleton value encoded in the multi-entry form, and an atom that fits the single-byte form encoded as multiple bytes. |
| base-crypto | `AlignedValue` deserialization rejects a value that does not fit its declared alignment. |
| base-crypto | `Duration::from_hours` saturates instead of overflowing. |
| storage | `MerklePatriciaTrie` deserialization enforces full structural canonicity rather than annotation consistency alone; a trie whose structure is not uniquely determined by its contents no longer decodes. |
| storage | An `Extension` node whose declared nibble length disagrees with the length of its encoded path is rejected. |
| storage | `MultiSet` rejects zero-count entries. |
| storage | `TimeFilterMap` rejects an encoding whose set and time-map representations disagree. |
| storage | Nibble-encoded keys reject trailing bytes left over after decoding. |
| onchain-runtime | `Op` deserialization rejects operands outside their legal encoding bound: `dup`, `swap` and `ins` with `n >= 16`, and `idx` with a path length outside `1..=16`. |
| onchain-runtime | serde `StateValue` deserialization enforces the type's invariant. |
| onchain-runtime | Taking the `type` of an array with more than 16 entries is a type error instead of producing an out-of-range tag byte. |
| onchain-runtime | The Merkle tree bound checks in `idx` and `ins` no longer overflow for large tree heights. |
| transient-crypto | A verifier key with trailing bytes after the encoded key is rejected, so two encodings cannot map to the same key. |
| ledger | `DustParameters::time_to_cap` guards against a zero `generation_decay_rate` instead of dividing by zero. |
| ledger | Dust `seq` increments saturate. |
| ledger | Contract call cost accounting counts public inputs via `ContractCall::public_inputs_len`, with saturating arithmetic, rather than materializing the inputs to take their length. |
| zswap | Binding randomness extraction no longer panics on a proof preimage with no witness to extract from. |
| zswap | Delta accumulation in `normalize_deltas` saturates. |
| storage-core | Pulls in the hardened `serialize` and `base-crypto` deserialization. |
