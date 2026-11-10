# ledger 9.1.0.2-rc.0

## Metadata

- **Release type**: release candidate (patch)
  `midnight-ledger-v9` moves to 1.0.2, and `midnight-storage` and `midnight-transient-crypto` move to 3.1.0 for their new public items. The wasm packages and the proof server carry the `-rc.0` suffix.
- **Date**: 2026-11-10
- **Sister-line note**: This release is on the `ledger-9` line. Nodes syncing from genesis replay pre-fork blocks with ledger 8.1.3, kept as legacy support for that sync.
- **Environment**: Pre-release; not deployed to mainnet
- **Released artifact(s)**: the npm packages `@midnight-ntwrk/ledger-v9` and `@midnight-ntwrk/onchain-runtime-v4` under the `rc` dist-tag, the proof server, and isolated git tags for the Rust crates, listed under Artifacts below.
- **Component class**: `core/infra`
- **Upgrade scope**: client libraries
- **Reset required**: No
- **Governance action required**: No

## High-level summary

Ledger 9.1.0.2-rc.0 is a client-side release candidate on the `ledger-9` line. It does not change transaction validation, state hashes or anything a node computes.

It fixes two ways in which wallets and DApps built transactions that the node then rejected. Transcript partitioning now sizes the guaranteed section against the time to dismiss the final transaction will be allowed, and callers can pass the expected shape of that transaction through the new `TransactionShape`. Partitioning also rehearses each call with the widest commitment tree indices the chain could assign, so contracts that store a coin's `mt_index` declare enough written bytes.

It also lets the wasm API load contract state that an ordinary map write can leave in a non-canonical trie shape, which earlier releases rejected, leaving the contract unreadable to clients.

## Audience

- Developers using the ledger WASM bindings, in particular wallets and DApps that partition transcripts or call `Transaction.addCalls`.
- Integrators who build transactions with the `ledger-9` Rust crates.
- Node developers, only to note that the node needs no change and must not enable the new `storage` leniency.

## Dependencies

- `midnight-storage` 3.1.0 adds `merkle_patricia_trie::ALLOW_SHORT_EXTENSION_CHAINS` and depends on `scoped-tls`.
- `midnight-transient-crypto` 3.1.0 adds `PureGeneratorPedersen::smallest_representable`, which `midnight-ledger-v9` 1.0.2 requires.
- No node release is required.

## Tested-with versions

Build pins, not QA-verified: read from the lockfile at the tag.

| Component | Tested-with version |
| --- | --- |
| `midnight-circuits` | `7.3.0` |
| `midnight-zk-stdlib` | `2.4.0` |
| `midnight-zkir` (zkir v3) | `3.2.0` |

## Deployment information

- **Upgrade scope**: client libraries
- **Reset required**: No
- **Governance action required**: No
- **Downtime / coordination**: None

## Artifacts

Packages on npm, under the `rc` dist-tag:

- `@midnight-ntwrk/ledger-v9` 1.0.2-rc.0
- `@midnight-ntwrk/onchain-runtime-v4` 4.0.2-rc.0

Proof server: `midnightntwrk/proof-server` 9.0.2-rc.0. It is rebuilt against the new crates; its own behaviour is unchanged.

Rust crates are not published to crates.io for a release candidate. Patch them from these isolated git tags:

- `midnight-ledger-v9` 1.0.2: `crate-ledger-9.1.0.2-rc.0`
- `midnight-storage` 3.1.0: `storage-3.1.0-rc.0`
- `midnight-transient-crypto` 3.1.0: `transient-crypto-3.1.0-rc.0`

```toml
[patch.crates-io]
midnight-ledger-v9 = { git = "https://github.com/midnightntwrk/midnight-ledger", tag = "crate-ledger-9.1.0.2-rc.0" }
midnight-storage = { git = "https://github.com/midnightntwrk/midnight-ledger", tag = "storage-3.1.0-rc.0" }
midnight-transient-crypto = { git = "https://github.com/midnightntwrk/midnight-ledger", tag = "transient-crypto-3.1.0-rc.0" }
```

Every other crate, including `midnight-zswap` and the `onchain-*` crates, is unchanged from 9.1.0.1 and resolves from crates.io.

## What changed

- Transcript partitioning sizes the guaranteed section against the predicted transaction's time-to-dismiss allowance, `max(min_time_to_dismiss, time_to_dismiss_per_byte * size)`, minus a reserve, instead of a fixed floor.
- The predicted size and the reserve come from a mock transaction of the calls' `TransactionShape`. The reserve covers every call's proof verification, including calls that end up fully fallible, priced by each call's own public inputs.
- Partitioning rehearses each call with the widest commitment tree indices the chain could assign.
- The wasm API deserializes contract state, transactions and ledger state that contain a trie with a short `Extension` chain.

Partitioning reserves more than before, so it may keep fewer calls guaranteed for the same input.

## New features

### Expected transaction shape for partitioning

- `TransactionShape` counts a transaction's non-call parts: intents, Night and non-Night unshielded spends and outputs, Dust spends, Zswap inputs, outputs and transients, and Dust registrations and deregistrations. `TransactionShape::for_calls` derives the counts of the balanced transaction from the calls.
- `partition_transcripts_with_tx_shape` and `Transaction::add_calls_with_tx_shape` partition against a caller-supplied shape. Use them when the final transaction has more parts than the calls imply, such as a payment from several Night UTXOs. `add_calls` folds the Zswap parts it is given into the shape.
- The wasm API exposes `TransactionShape` as a class obtained from `TransactionShape.forCalls`, with one accessor per count, and adds an optional `expectedTxShape` argument to `partitionTranscripts` and `Transaction.addCalls`.

### Lenient trie loading for clients

- `storage::merkle_patricia_trie::ALLOW_SHORT_EXTENSION_CHAINS` is a scoped thread-local, unset by default. While it is set, decoding accepts a trie in which an `Extension` node shorter than 255 nibbles has an `Extension` child; every other invariant still applies. `onchain-runtime-wasm` sets it while deserializing, so both npm packages get the leniency. Nodes and consensus code must never set it.

## New features requiring configuration updates

None

## Improvements

None

## Deprecations

None

## Breaking changes

None. The new wasm arguments are optional, and the new Rust functions sit beside the existing ones.

## Known issues

### `insert` can produce a non-canonical trie

**Description**: `MerklePatriciaTrie` insertion can still build the short `Extension` chain that strict decoding rejects. Nodes agree on the shape, and changing it changes state hashes, so the fix needs a hard fork. This release only lets clients load such state.
**Workaround (if any)**: load affected state through the wasm API or with `ALLOW_SHORT_EXTENSION_CHAINS` set, in client code only.

## Links and references

- **Engineering docs**: [CHANGELOG at the tag](https://github.com/midnightntwrk/midnight-ledger/blob/ledger-9.1.0.2-rc.0/CHANGELOG.md)
- **Pull requests**: [#845](https://github.com/midnightntwrk/midnight-ledger/pull/845), [#846](https://github.com/midnightntwrk/midnight-ledger/pull/846), [#856](https://github.com/midnightntwrk/midnight-ledger/pull/856)

## Fixed defect list

The following defects were fixed in ledger 9.1.0.2-rc.0.

| Defect number | Description |
| --- | --- |
| [#838](https://github.com/midnightntwrk/midnight-ledger/issues/838) | A contract call with exactly one Dust spend could fail the node's time-to-dismiss check, because partitioning sized the guaranteed section against a floor rather than the transaction's allowance. |
| [#839](https://github.com/midnightntwrk/midnight-ledger/issues/839) | A contract that stores a coin's `mt_index` under-declared its written bytes once the commitment tree had grown past 255 entries, and the node rejected the transaction for running out of gas budget. |
| [#856](https://github.com/midnightntwrk/midnight-ledger/pull/856) | After a map write, a contract's state could fail to deserialize in the wasm API, so clients could neither read the contract nor build calls to it. |
