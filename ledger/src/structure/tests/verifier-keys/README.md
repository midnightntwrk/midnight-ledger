# Verifier keys of every supported version

Used by `ledger/src/structure/tests/verifier_key_versions.rs`.

Never change the bytes of a released version. A new verifier key version
adds a directory, and an old one is removed only when the ledger stops
supporting that version.

Each directory is named after the tag of its key and holds three values,
written with `tagged_serialize`, for one verified call. For `v6` and `v7`
that is the first `store` call that `ledger/tests/simple-merkle-tree.rs`
verifies when run with `--features proving`; for `v8` it is the "IR 4.0"
call of `proving_versions.rs`:

| File | Value |
|---|---|
| `key.bin` | the verifier key of the call's circuit |
| `proof.bin` | the call's proof, as the `Proof` type of the key's proof system |
| `public-inputs.bin` | the public inputs the proof was verified against |

The values were written out where the proof is verified
(`ProofKind::proof_verify`).

| Directory | Key | Proof made by |
|---|---|---|
| `v6` | built from the `simple-merkle-tree` IR 2.0 circuit; `store.verifier.sha256` as pinned by commit `5d389e58` | this ledger (branch `ledger-10`, on top of `849bcae1`) |
| `v7` | `zkir-precompiles/simple-merkle-tree` built by the `zkir` tool of branch `ledger-9` at `b8f5e143`; matches that branch's `store.verifier.sha256` | ledger 9 at `b8f5e143` |
| `v8` | built from the "IR 4.0" circuit of `circuits_of_every_ir_version` | this ledger (branch `ledger-10`, on top of `a5996ebf`, with `midnight-zk-stdlib` 3.0.0-rc.2) |
