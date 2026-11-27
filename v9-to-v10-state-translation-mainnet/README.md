# v9 → v10 state translation on mainnet

Translates mainnet's ledger-9 state into ledger 10 and checks the result:

- every contract keeps its state, and the contract, bridge and block-reward
  counts are unchanged;
- every contract's `rc_0` is canonical;
- every node of the translated state passes the checks ledger 10's wire
  decoder makes, so all of its tries are canonical. The check walks the
  stored state a window of nodes at a time (`src/canonicity.rs`) rather than
  decoding it whole.

The state comes straight from a public mainnet node snapshot's databases. No
node runs and nothing is replayed.

## 1. Fetch a node snapshot

`https://snapshots.midnight.network/index.json` lists the snapshots per
network. A mainnet entry's `s3_path` names the archive; drop its
`s3://<bucket>/` prefix to get the download path:

```sh
curl -s https://snapshots.midnight.network/index.json \
  | jq -r '.mainnet[][] | "\(.timestamp) \(.size_bytes) \(.s3_path)"' | sort | tail -3

mkdir -p mainnet_cache/node-snapshot && cd mainnet_cache/node-snapshot
curl -fLO https://snapshots.midnight.network/mainnet/<host>/epoch-0/mainnet-epoch-0-block-<N>-v<version>.tar.gz
tar xzf mainnet-epoch-0-block-<N>-v<version>.tar.gz    # yields node/
```

The archive for block 3,602,507 is 113 GB and unpacks to 408 GB, most of it
`node/ledger_storage`, the node's ledger database.

## 2. Run

```sh
cargo run --release -- --node-dir mainnet_cache/node-snapshot/node
```

The test reads the finalized block's `Midnight::StateKey`
(storage key `twox128("Midnight") ++ twox128("StateKey")`) straight from the
chain database, `node/chains/midnight/paritydb/full`: finalized lookup key,
header, state root, then down the state trie (`src/chain.rs`). At block
3,602,505 the value tags the key `storage-key(ledger-state[v18])`, ledger 9's
state, with root `b4ab97b2…8bab5d`.

The first run copies the nodes reachable from that root out of
`node/ledger_storage` into `mainnet_cache/v9_state_db` and reuses the copy
afterwards; the translation and its garbage stay out of the node's database.
At block 3,602,505 that is 158,101 nodes, copied in about 90 s. The
translation takes under a second, and the canonicity walk about 7 minutes at
a peak of about 5 GB of memory.

Both node databases are opened read-only. If a node is running on the same
directory, stop it first: parity-db locks a database even for readers.
