# Test fixtures

## `circuits/`

Hand-rolled zkir-v3 circuits keyed against `verifier-key[v8]`, served to provers by
`TestResource.circuit(name)` under the key location `name`.

- `noop`: declares the single public input a one-`noop` transcript field-reprs to, so a
  `PrePartitionContractCall` with program `[{ noop: { n: 1 } }]` proves against it.

Regenerate `*.bzkir`, `*.prover` and `*.verifier` from the `*.zkir` JSON with the zkir-v3 CLI:

```console
$ cargo run -p midnight-zkir-v3 --features binary -- compile noop.zkir noop.prover noop.verifier
```

## `verify-proof/`

Transactions proven in Rust the way `ledger/tests/verify-proof.rs` does, on the `local-test`
network at block time 0:

- `deploy.tx`: deploys a contract whose `verify` operation runs a `verify_proof` circuit.
- `call.tx`: calls it, carrying a `V4` proof with one deferred accumulator.

The circuit needs the 2^18 SRS, and its prover key is too large to commit, so only the proven
transactions are kept here.
