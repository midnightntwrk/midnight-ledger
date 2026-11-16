# Ledger 9.1.0.2 Release Notes

**Version:** 9.1.0.2
**Date:** <YYYY-MM-DD>

## High-level summary

<One or two paragraphs: what kind of release this is and the headline changes.>

## Audience

This release note is important for:

- <e.g. Node developers>
- <e.g. Developers using the ledger WASM bindings>

## Summary of updates

### <Component or area> Updates

- <feat/fix/breaking bullet>

## New features requiring configuration updates

*None in this release.*

## Deprecations

*None in this release.*

## Breaking changes or required actions for developers

*None in this release.*

## Fixed defect list

| **Component** | **Description** |
|---------------|-----------------|
| <component>   | <description>   |

<!-- Draft bullets from commits since the last tag: -->
- fix: build after merging ledger-9's transcript partitioning
- fix: let clients load tries with short extension chains
- fix: release storage 2.0.5 on sha2 0.10
- fix: compilation after merge
- fix: rehearse calls with the widest commitment tree indices
- feat: take an expected TransactionShape when partitioning transcripts
- fix: partition transcripts against a mock transaction's size and time-to-dismiss
- feat: add TransactionShape, the counts of a transaction's non-call parts
- fix: require base-crypto-derive 1.1.0
- fix: require base-crypto-derive 1.1.0 for the Envelope derive
- fix: let isolate.py tag path-dependency workspace members
- fix: guarantee each translation run persists progress
- fix: revert storage and storage-core to sha2 0.10
- fix: key repeated dust registrations without their signatures
- fix: don't mark slots read on ins and rem
- fix: saturate FixedPoint::from_u128_div instead of wrapping
- fix: cap contracts at 256 operations
- fix: count shared call effects in the time to dismiss
- fix: bound a call's program length by its public-input count
- fix: serialize registration signing data once per dust actions
- fix: reject committee signatures repeated in a transaction
- feat: count distinct values across several weighted arrays
- fix: charge each deploy for its state, within a bounded walk
- fix: saturate entry counts summed for pricing
- fix: check keys stored with their values once per shared node
- fix: price nested shared structure in time linear in the encoding
- feat: charge copies of values that share nodes once per shared node
- fix: reject repeated entries before computing fees
- fix: check fees before stateless transaction checks
- fix: price transactions in time linear in their encoding
- fix: compute StateValue field sizes in linear time
- feat: fold Merkle trees in time linear in distinct nodes
- fix: check TimeFilterMap consistency in linear time
- fix: check Map keys in time linear in distinct nodes
- fix: check Array indices in time linear in distinct nodes
- fix: check MPT canonicity per node instead of per trie
- feat: fold MPT-backed collections in time linear in distinct nodes
- feat: add DagMemo for linear-time walks over shared DAGs
- fix: make isolate.py push to a selectable remote
- fix: report the real threshold in BridgeClaimTooSmall
- fix: track cache state through ins and cached bmt idx
- fix: bump rustls to 0.23.46 to resolve the RUSTSEC-2026-0346/0347/0348/0349 cargo audit failures.
- fix: failed Sp::downcast no longer leaks a metadata refcount
- fix: reject zero-value ClaimRewards transactions
- feat: remove unnessisary specified lifetime bounds
- fix: reject contract calls that receive or send Dust
- fix: replace deprecated atomic fetch_update calls with try_update to satisfy clippy on newer Rust toolchains
- fix: restore original runner labels and required check names
- fix: update vulnerable transport dependencies
- fix: replace deprecated atomic update calls
- fix: use the nondeprecated atomic update API
- feat: allow args to be passed through
- fix: use the inline runner cleanup convention
- fix: reclaim unused SDK space before Nix builds
- fix: isolate metadata checks from proving artifacts
- fix: verify output evidence opens commitment on event replay
- fix: price oversized transcript gas instead of wrapping to zero
- fix: count in-lined children towards the deserialization recursion limit
- fix: remove quadratic when deserialising
- fix: let the release resume after the npm publish approval
- fix: release the proof server on its own tag, idempotently
- fix: flatten proof-server public key into manifest artifact
- fix: stop expanding untrusted input into workflow shell (GHSA-w369-9rqw-8hq8)
- fix: drop the unused http2 feature (RUSTSEC-2026-0258) (#722)
- fix: stop purging the Nix store cache on every run (#724)
