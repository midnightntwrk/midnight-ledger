# Test fixtures

## Test circuits

Hand-rolled zkir-v3 circuits live with the other precompiled circuits in `zkir-precompiles/`, keyed
against `verifier-key[v8]` by the `test-artifacts` Nix package. `TestResource.circuit(name)` serves them
to provers from `MIDNIGHT_LEDGER_TEST_STATIC_DIR`, which the `default` and `ci` dev shells point at that
package.

- `noop` (`zkir-precompiles/noop/noop.zkir`): declares the single public input a one-`noop` transcript
  field-reprs to, so a `PrePartitionContractCall` with program `[{ noop: { n: 1 } }]` proves against it.

## `verify-proof/`

Transactions proven in Rust by `ledger/tests/verify-proof.rs`, on the `local-test` network at block time 0:

- `deploy.tx`: deploys a contract whose `verify` operation runs a `verify_proof` circuit.
- `call.tx`: calls it, carrying a `V4` proof with one deferred accumulator.

The circuit's source is `outer_ir` in that file. It is a handful of instructions; its k=18 comes from the
in-circuit verifier a `verify_proof` expands to. Its prover key is about 90 MB, so only the proven
transactions are kept here. Regenerate them after a change to the transaction, proof or key formats, with
`MIDNIGHT_PP` holding the parameters up to 2^18 (the `public-params` Nix package) and the dust keys:

```console
$ cargo test --release -p midnight-ledger-v10 --features proving --test verify-proof \
    -- --ignored write_integration_test_fixtures
```
