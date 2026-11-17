# ledger 9.1.0.1

## Metadata

- **Release type**: patch
  Every crate moves by a patch version, except `midnight-storage`, which moves to 3.0.0 so that `ledger-8` can stay on `storage` 2.x.
- **Date**: 2026-11-03
- **Sister-line note**: This release is on the `ledger-9` line. Nodes syncing from genesis replay pre-fork blocks with ledger 8.1.3, kept as legacy support for that sync.
- **Environment**: Mainnet
- **Released artifact(s)**: the Rust crates on crates.io, the npm packages `@midnight-ntwrk/ledger-v9` and `@midnight-ntwrk/onchain-runtime-v4`, and the proof server, listed under Artifacts below.
- **Component class**: `core/infra`
- **Upgrade scope**: binary + runtime
- **Reset required**: No
- **Governance action required**: Not applicable — handled by the node release

## High-level summary

Ledger 9.1.0.1 is a security patch release on the `ledger-9` line. It was released to node operators on 2026-11-03 and is already live on mainnet; its source and these notes are published after the fact. It bounds the work an attacker can force during transaction pricing, decoding and well-formedness checks over shared storage nodes, closes several fee and block-limit bypasses in contract call transcripts, and rejects repeated entries in a transaction. Several public Rust APIs change shape as a result; see Breaking changes.

## Audience

- Node developers who build on the `ledger-9` crates.
- Integrators who construct or price transactions with the ledger crates.
- Developers who depend on the `storage` crate directly.
- Developers using the ledger WASM bindings, which apply the same stricter validation.

## Dependencies

- `midnight-storage` moves to 3.0.0: `Map` keys must implement `serialize::EncodingShape`; code that keeps `ledger-8` stays on `storage` 2.x.
- `storage-core` 1.2.3 and `storage` 3.0.0 return to `sha2` 0.10 and `digest` 0.10, so the published `ledger-8` `storage` 2.0.x can share `storage-core` with this release.
- Node 2.1.100 is built against this release, and is the only component that must upgrade with it.

## Tested-with versions

Build pins, not QA-verified: read from the lockfile at the tag.

| Component | Tested-with version |
| --- | --- |
| `midnight-circuits` | `7.3.0` |
| `midnight-zk-stdlib` | `2.4.0` |
| `midnight-zkir` (zkir v3) | `3.2.0` |

## Deployment information

- **Upgrade scope**: binary + runtime
- **Reset required**: No
- **Governance action required**: Not applicable — handled by the node release
- **Downtime / coordination**: Not applicable — handled by the node release

## Artifacts

Crates on crates.io:

- `midnight-ledger-v9` 1.0.1
- `midnight-zswap` 9.0.1
- `midnight-onchain-runtime`, `midnight-onchain-state` and `midnight-onchain-vm` 4.0.1
- `midnight-storage` 3.0.0, `midnight-storage-core` 1.2.3 and `midnight-storage-macros` 1.0.1
- `midnight-base-crypto` 1.1.1, `midnight-transient-crypto` 3.0.1, `midnight-coin-structure` 3.0.1 and `midnight-serialize` 1.2.1

Packages on npm:

- `@midnight-ntwrk/ledger-v9` 1.0.1
- `@midnight-ntwrk/onchain-runtime-v4` 4.0.1

Proof server: `midnightntwrk/proof-server` 9.0.1.

`midnight-ledger-static`, `midnight-base-crypto-derive`, `midnight-zkir`, `zkir-wasm` and `zkir-v3-wasm` are unchanged from 9.1.0.0.

## What changed

- Transaction pricing and `well_formed` now take time linear in the encoding even when storage nodes are shared; repeated entries are rejected before fees are computed.
- Repeated committee signatures, repeated dust registrations and other repeated entries in a transaction are rejected.
- Contracts are capped at `MAX_CONTRACT_OPERATIONS` (256) operations.
- Each deploy is charged for its full state.
- Each segment's writes are priced separately, closing a fee and net-bytes-written bypass.
- Contract call transcripts declaring gas beyond the block limits, or transferring Dust through unshielded inputs or outputs, are rejected as malformed.
- `zswap` offers verify each distinct input, output and transient once.
- Decoding in `storage` and `storage-core` checks shared nodes once and counts in-lined and memoized nodes towards the recursion limit.
- `FixedPoint` division saturates instead of wrapping negative.
- Several Rust APIs change signature; see Breaking changes.
- Wallet event replay credits a Zswap output only if the recovered coin opens the event's commitment.
- `CardanoBridge` claims must claim at least `c_to_m_bridge_min_amount`, or the full, non-zero claimable balance.
- A cached `idx` succeeds only on a path the run has already read.
- Deserializing a storage graph hashes each node once rather than once per reference.
- Secret material is cleared more thoroughly: the seed hasher clears its state on drop, and Zswap witness vectors no longer leave copies on reallocation.

## New features

### Bounded public-input counting

- `Transcript::public_inputs_len` counts the field elements a transcript adds to a contract call's public inputs without materializing them, and `transcript::MAX_CALL_PUBLIC_INPUTS` exposes the per-call bound to consumers outside the ledger.
- `onchain-vm`'s cost model gains `tree_copy_bounded`, which prices a copy and stops walking once a byte budget is exceeded.

### Linear-time walks over shared structures

- `storage` adds `fold` on `MerklePatriciaTrie`, `Array` and `HashMap`, and `distinct_values` and `distinct_values_across` on `MerklePatriciaTrie` and `Array`.
- `storage-core` adds `DagMemo` and `Monoid` for walks over shared DAGs, and `Storable::check_invariant_memoized`.
- `transient-crypto` adds `MerkleTree::fold`.
- `serialize` adds `EncodingShape`, and `coin-structure` implements it for `Nullifier`, `Commitment`, `TokenType`, `UserAddress` and `ContractAddress`.

## New features requiring configuration updates

None

## Improvements

### Storage hashing dependency

**Description**: `storage-core` and `storage` return to `sha2` 0.10 and `digest` 0.10, which `storage-core` 1.2.2 had replaced with `digest` 0.11, which broke published `storage` 2.0.x.

### State translation progress

**Description**: a state-translation run that exhausts its budget continues until it has persisted at least one result.

## Deprecations

None

## Breaking changes

### Fallible transaction cost functions

**What changed**: `Transaction::application_cost` and `Transaction::time_to_dismiss` take `&LedgerParameters` and return `Result<_, FeeCalculationError>`.
**What breaks**: callers of either function that pass no ledger parameters or use the result as a plain value.
**Required actions**: Pass the ledger parameters and handle `FeeCalculationError`.

**Code example**: —

### `FeeCalculationError` is non-exhaustive

**What changed**: `FeeCalculationError` gains `RepeatedEntries` and is `#[non_exhaustive]`.
**What breaks**: exhaustive matches on `FeeCalculationError`.
**Required actions**: Add a wildcard arm to matches on `FeeCalculationError`.

**Code example**: —

### Fallible public inputs

**What changed**: `ContractCall::public_inputs` returns a `Result`, failing with `TooManyPublicInputs` instead of materializing more than `MAX_CALL_PUBLIC_INPUTS` field elements.
**What breaks**: callers using the return value of `ContractCall::public_inputs` directly.
**Required actions**: Handle the `TooManyPublicInputs` error.

**Code example**: —

### `storage` 3: `Map` keys need `EncodingShape`

**What changed**: `Map` requires its keys to implement `serialize::EncodingShape`, through `MapKey`.
**What breaks**: `Map`s keyed by types that do not implement `EncodingShape`.
**Required actions**: Implement `serialize::EncodingShape` for custom `Map` key types, or stay on `storage` 2.x as `ledger-8` does.

**Code example**: —

### Stricter validation

**What changed**: transactions with repeated entries, contracts with more than 256 operations, and contract calls over the gas, public-input or Dust-transfer limits are now rejected.
**What breaks**: transactions that earlier `ledger-9` releases accepted under those conditions.
**Required actions**: keep each committee signature and Dust registration unique within a transaction, deploy at most 256 operations per contract, declare transcript gas within the block limits, and move Dust only through Dust spends, never as an unshielded contract input or output.

**Code example**: —

## Known issues

### Contract call with exactly one DUST spend can fail the time-to-dismiss check

**Description**: still open, listed as a known defect of 9.1.0.0 ([issue](https://github.com/midnightntwrk/midnight-ledger/issues/838)); it still applies to 9.1.0.1.

### Coin overwrite under-declares bytes written

**Description**: still open, listed as a known defect of 9.1.0.0 ([issue](https://github.com/midnightntwrk/midnight-ledger/issues/839)); it still applies to 9.1.0.1.

### Decoding needs a large stack

**Description**: decoding a storage graph up to the recursion limit still needs about 1.5 MB of stack.
**Workaround (if any)**: decode on a thread with at least 1.5 MB of stack.

## Links and references

- **Security advisories**: [GHSA-5j5j-538c-75vp](https://github.com/midnightntwrk/midnight-ledger/security/advisories/GHSA-5j5j-538c-75vp), [GHSA-xrv9-p5gx-5547](https://github.com/midnightntwrk/midnight-ledger/security/advisories/GHSA-xrv9-p5gx-5547), [GHSA-r8v5-mmq4-5qfm](https://github.com/midnightntwrk/midnight-node/security/advisories/GHSA-r8v5-mmq4-5qfm), [GHSA-cjcc-56g7-33x4](https://github.com/midnightntwrk/midnight-ledger/security/advisories/GHSA-cjcc-56g7-33x4), [GHSA-rgvh-4fg3-ff6f](https://github.com/midnightntwrk/midnight-ledger/security/advisories/GHSA-rgvh-4fg3-ff6f), [GHSA-4r8v-645m-5wpc](https://github.com/midnightntwrk/midnight-ledger/security/advisories/GHSA-4r8v-645m-5wpc), [GHSA-8xrv-7vxq-x8g4](https://github.com/midnightntwrk/midnight-ledger/security/advisories/GHSA-8xrv-7vxq-x8g4), [GHSA-qjrc-wmgr-7wr2](https://github.com/midnightntwrk/midnight-ledger/security/advisories/GHSA-qjrc-wmgr-7wr2), [GHSA-4x6f-973h-mmqf](https://github.com/midnightntwrk/midnight-node/security/advisories/GHSA-4x6f-973h-mmqf)
- **Engineering docs**: [CHANGELOG at the tag](https://github.com/midnightntwrk/midnight-ledger/blob/ledger-9.1.0.1/CHANGELOG.md)

## Fixed defect list

The following defects were fixed in ledger 9.1.0.1.

| Defect number | Description |
| --- | --- |
| GHSA-5j5j-538c-75vp | Pricing, `well_formed` and decoding walked storage nodes shared by hash once per path, so a few hundred bytes could force exponential work before fees were checked. The fix also rejects repeated committee signatures, Dust registrations and priced entries, caps contracts at 256 operations, charges deploys for their full state, and counts every path to a call's effects in the time to dismiss. |
| GHSA-xrv9-p5gx-5547 | Fee estimation sized shared transcript programs and state values once per path rather than once per distinct node. |
| GHSA-r8v5-mmq4-5qfm, Taran C-4 | Contract calls could declare unbounded public inputs, and `noop n` allocated a buffer proportional to the untrusted `n`. |
| GHSA-cjcc-56g7-33x4 | A contract could absorb the surplus Dust credited by a registration's `allow_fee_payment` while the registrant still received the full entitlement. |
| GHSA-rgvh-4fg3-ff6f, Taran H-3 | Wallet event replay credited a Zswap output whose ciphertext did not open its commitment, so wallets reported coins they could not spend. |
| GHSA-4r8v-645m-5wpc, Taran H-6 | Cached `idx` reads of unread paths, including through bounded Merkle trees and slots overwritten by `ins` or `rem`, skipped their read charge. |
| GHSA-8xrv-7vxq-x8g4 | Deserializing a storage graph hashed a node once per reference to it, costing time quadratic in the encoding. |
| GHSA-qjrc-wmgr-7wr2, GHSA-4x6f-973h-mmqf, Taran H-4 | Deeply nested in-lined or shared nodes could exceed the decoding recursion limit and overflow the stack. |
| Taran H-7 | Summing guaranteed and fallible costs before netting writes against deletions let a fallible transcript lower other segments' fees and bypass the net-bytes-written block limit. |
| Taran H-5 | A transcript declaring `2^63` ps or more of compute or read time was priced as zero compute, and `FixedPoint` division wrapped negative for very large quotients, zeroing `CostDuration * FixedPoint` and `min_claimable_rewards`. |
| Taran C-2 | Zero-value `CardanoBridge` claims passed validation without paying fees. |
| Ledger audit issue D | `addCalls` drew the illegal segment 0 for the `random` and `guaranteedOnly` segment specifiers. |
| Ledger audit issue E | The hasher deriving coin keys from the wallet seed kept its state after use, and Zswap preimage witness vectors could leave copies of the witness in freed memory on reallocation. |
