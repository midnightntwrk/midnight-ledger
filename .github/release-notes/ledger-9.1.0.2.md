# Ledger 9.1.0.2 Release Notes

**Version:** 9.1.0.2
**Date:** 2026-11-16

## High-level summary

Ledger 9.1.0.2 is a client-side patch release on the `ledger-9` line. It
promotes `9.1.0.2-rc.0` after QA approval. It does not change transaction
validation, state hashes or anything a node computes.

It fixes two ways in which wallets and DApps built transactions that the node
then rejected. Transcript partitioning now sizes the guaranteed section against
the time to dismiss the final transaction will be allowed, and callers can pass
the expected shape of that transaction through the new `TransactionShape`.
Partitioning also rehearses each call with the widest commitment tree indices
the chain could assign, so contracts that store a coin's `mt_index` declare
enough written bytes.

It also lets the wasm API load contract state that an ordinary map write can
leave in a non-canonical trie shape, which earlier releases rejected, leaving
the contract unreadable to clients.

Since the release candidate, the proof server moves to zkir v3 `3.2.1`, which
fixes proving for Compact circuits whose chip requirements
earlier versions missed, and its image passes `docker run` arguments
through to the server.

## Audience

This release note is important for:

- Developers using the ledger WASM bindings, in particular wallets and DApps
  that partition transcripts or call `Transaction.addCalls`
- Integrators who build transactions with the `ledger-9` Rust crates
- Proof server operators
- Node developers, only to note that the node needs no change and must not
  enable the new `storage` leniency

## Released packages

| **Package** | **Version** |
|-------------|-------------|
| npm `@midnightntwrk/ledger-v9` | `1.0.2` |
| npm `@midnightntwrk/onchain-runtime-v4` | `4.0.2` |
| proof server | `9.0.2` |
| crates.io `midnight-ledger-v9` | `1.0.2` |
| crates.io `midnight-storage` | `3.1.0` |
| crates.io `midnight-transient-crypto` | `3.1.0` |

The npm packages are also published under the legacy `@midnight-ntwrk` scope.
Every other package, including `@midnightntwrk/zkir-v2`, `midnight-zswap` and
the `onchain-*` crates, is unchanged from 9.1.0.1.

Build pins, read from the lockfile: `midnight-circuits` `7.3.0`,
`midnight-zk-stdlib` `2.4.0`, and zkir v3 (`midnight-zkir`, from crates.io)
`3.2.1`.

## Summary of updates

### Transcript partitioning

- fix: transcript partitioning sizes the guaranteed section against the
  predicted transaction's time-to-dismiss allowance,
  `max(min_time_to_dismiss, time_to_dismiss_per_byte * size)`, minus a reserve,
  instead of a fixed floor.
- fix: the predicted size and the reserve come from a mock transaction of the
  calls' `TransactionShape`. The reserve covers every call's proof
  verification, including calls that end up fully fallible, priced by each
  call's own public inputs. Partitioning therefore reserves more than before,
  and may keep fewer calls guaranteed for the same input.
- fix: partitioning rehearses each call with the widest commitment tree indices
  the chain could assign.
- feat: `TransactionShape` counts a transaction's non-call parts: intents,
  Night and non-Night unshielded spends and outputs, Dust spends, Zswap inputs,
  outputs and transients, and Dust registrations and deregistrations.
  `TransactionShape::for_calls` derives the counts of the balanced transaction
  from the calls.
- feat: `partition_transcripts_with_tx_shape` and
  `Transaction::add_calls_with_tx_shape` partition against a caller-supplied
  shape. Use them when the final transaction has more parts than the calls
  imply, such as a payment from several Night UTXOs. `add_calls` folds the
  Zswap parts it is given into the shape.
- feat: the wasm API exposes `TransactionShape` as a class obtained from
  `TransactionShape.forCalls`, with one accessor per count, and adds an
  optional `expectedTxShape` argument to `partitionTranscripts` and
  `Transaction.addCalls`.

### Contract state loading

- fix: the wasm API deserializes contract state, transactions and ledger state
  that contain a trie with a short `Extension` chain.
- feat: `storage::merkle_patricia_trie::ALLOW_SHORT_EXTENSION_CHAINS` is a
  scoped thread-local, unset by default. While it is set, decoding accepts a
  trie in which an `Extension` node shorter than 255 nibbles has an `Extension`
  child; every other invariant still applies. `onchain-runtime-wasm` sets it
  while deserializing, so both npm packages get the leniency. Nodes and
  consensus code must never set it.

### Proof server

- fix: zkir v3 moves to `3.2.1`, which derives a circuit's chip architecture
  from every instruction rather than only its inputs and entry points. Proving
  failed for Compact circuits that needed a chip only through another
  instruction, such as a foreign-field constant. Existing proving and
  verifying keys remain valid.
- fix: the image passes trailing `docker run` arguments to the server
  (`docker run <image> -v` now works; previously the `bash -c` entrypoint
  silently dropped them). The old `midnight-proof-server ...` command forms
  still work.
- feat: the image sets `MIDNIGHT_PP=/var/cache/midnight/zk-params`; mount a
  volume there to skip the parameter download on restart.

### Dependencies

- feat: `midnight-transient-crypto` 3.1.0 adds
  `PureGeneratorPedersen::smallest_representable`, which `midnight-ledger-v9`
  1.0.2 requires.
- `midnight-storage` 3.1.0 adds `ALLOW_SHORT_EXTENSION_CHAINS` and depends on
  `scoped-tls`.

## New features requiring configuration updates

*None in this release.* Mounting a volume at `MIDNIGHT_PP` for the proof
server is optional.

## Deprecations

*None in this release.*

## Breaking changes or required actions for developers

*None in this release.* The new wasm arguments are optional, and the new Rust
functions sit beside the existing ones.

## Fixed defect list

| **Component** | **Description** |
|---------------|-----------------|
| ledger | [#838](https://github.com/midnightntwrk/midnight-ledger/issues/838): a contract call with exactly one Dust spend could fail the node's time-to-dismiss check, because partitioning sized the guaranteed section against a floor rather than the transaction's allowance. |
| ledger | [#839](https://github.com/midnightntwrk/midnight-ledger/issues/839): a contract that stores a coin's `mt_index` under-declared its written bytes once the commitment tree had grown past 255 entries, and the node rejected the transaction for running out of gas budget. |
| ledger-wasm, onchain-runtime-wasm | [#856](https://github.com/midnightntwrk/midnight-ledger/pull/856): after a map write, a contract's state could fail to deserialize in the wasm API, so clients could neither read the contract nor build calls to it. |
| proof server | Proving failed for zkir v3 circuits that need a chip only through an instruction other than their inputs and entry points (fixed in `midnight-zkir` 3.2.1). |
| proof server | The image dropped trailing `docker run` arguments. |

## Known defects

| **Issue** | **Description** |
|-----------|-----------------|
| `insert` can produce a non-canonical trie | `MerklePatriciaTrie` insertion can still build the short `Extension` chain that strict decoding rejects. Nodes agree on the shape, and changing it changes state hashes, so the fix needs a hard fork. This release only lets clients load such state. Workaround: load affected state through the wasm API, or with `ALLOW_SHORT_EXTENSION_CHAINS` set, in client code only. |
