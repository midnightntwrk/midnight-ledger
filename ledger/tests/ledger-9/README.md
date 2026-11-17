# Bytes written by ledger 9

Used by `ledger/tests/ledger-9-keys.rs`. Do not regenerate them with this
ledger: they show what ledger 9 wrote.

They come from `ledger/tests/simple-merkle-tree.rs` on the `ledger-9` branch
(commit `b8f5e143`), run with `--features proving` against keys built from
`zkir-precompiles/simple-merkle-tree` by that branch's `zkir` tool. For the
first `store` call the test verifies, three values were written out with
`tagged_serialize` where the proof is verified (`ProofKind::proof_verify`):

| File | Value |
|---|---|
| `store.operation.bin` | the `ContractOperation` called, tagged `contract-operation[v6]`, holding a `verifier-key[v7]` key |
| `store.proof.bin` | the call's `ProofVersioned`, a `V3` proof |
| `store.public-inputs.bin` | the public inputs the proof was verified against |
