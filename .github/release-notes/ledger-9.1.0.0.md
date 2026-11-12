# Ledger 9.1.0.0 Release Notes

**Version:** 9.1.0.0
**Date:** 2026-11-04

## High-level summary

Ledger 9.1.0.0 is the first full release of the `ledger-9` line, the ledger for
the next Midnight hardfork. It promotes `9.1.0.0-rc.7` after QA approval, and
collects everything shipped across the `9.1.0.0` release candidates.

The headline change is the move to `midnight-zk-stdlib` v2 and
`midnight-circuits` v7 for proving and verification. Proofs and verifier keys
made with zk-stdlib v1 remain verifiable. The release also adds support for
zkir v3 circuits, ECDSA signatures, the `UnlockToTreasury` system transaction,
an explicit price floor and separate cost factors for validation, guaranteed
and fallible application. It carries forward the security fixes released to
mainnet as `8.1.2` and `8.1.3`.

This is also the first release of the `ledger-9` crates on crates.io, including
the new `midnight-ledger-v9` crate.

## Audience

This release note is important for:

- Node developers
- Developers using the ledger WASM bindings
- Contract developers, because verifier keys change (see
  [Breaking changes](#breaking-changes-or-required-actions-for-developers))
- Proof server operators

## Released packages

| **Package** | **Version** |
|-------------|-------------|
| npm `@midnightntwrk/ledger-v9` | `1.0.0` |
| npm `@midnightntwrk/onchain-runtime-v4` | `4.0.0` |
| npm `@midnightntwrk/zkir-v2` | `2.2.0` |
| proof server | `9.0.0` |
| crates.io `midnight-ledger-v9` | `1.0.0` |
| crates.io `midnight-zswap` | `9.0.0` |
| crates.io `midnight-onchain-runtime`, `midnight-onchain-state`, `midnight-onchain-vm` | `4.0.0` |
| crates.io `midnight-coin-structure` | `3.0.0` |
| crates.io `midnight-transient-crypto` | `3.0.0` |
| crates.io `midnight-zkir` | `2.2.0` |
| crates.io `midnight-storage` | `2.0.4` |
| crates.io `midnight-storage-core` | `1.2.2` |
| crates.io `midnight-serialize` | `1.2.0` |
| crates.io `midnight-base-crypto`, `midnight-base-crypto-derive` | `1.1.0` |
| crates.io `midnight-ledger-static` | `10.0.0` |

The npm packages are also published under the legacy `@midnight-ntwrk` scope.
zkir v3 comes from the separate `midnight-zkir` repository, at `3.2.0`.

## Summary of updates

### Proving and verification

- feat: proofs are generated against zk-stdlib v2. `ProofVersioned::V3` carries
  them, and `proof_verify` dispatches on the proof version: V2 proofs verify
  through the zk-stdlib v1 pipeline, V3 proofs through the current one.
- feat: `ContractOperation` gains a `v3` field for zk-stdlib v2 verifier keys,
  under the new `ContractOperationVersion::V4` and
  `ContractOperationVersionedVerifierKey::V4`.
- feat: zswap verification keeps accepting v1 proofs.
- feat: `lookup_key` is exposed on the ledger-wasm proving path, so the prover
  uses the correct key version.
- feat: the proof server supports zkir v3 (through `midnight-zkir` `3.2.0`).
- chore: require `midnight-circuits` 7.3.0 and `midnight-zk-stdlib` 2.4.0.

### Contracts

- feat: maintenance updates can insert and remove the IR of a contract
  operation, through `SingleUpdate::IrInsert` and `SingleUpdate::IrRemove`
  (exposed in ledger-wasm as `IrInsert` and `IrRemove`, with `IrBuf` support).
- feat: ledger-wasm exposes the `contractLog` variant of `EventDetails`, matching
  the new on-chain event format.

### Transactions and fees

- feat: ECDSA signatures.
- feat: the `UnlockToTreasury` system transaction moves funds from the locked
  pool to the treasury.
- feat: an explicit price floor, denominated in full blocks and governed by
  ledger parameters.
- feat: `parallelism_factor` is replaced by `validation_factor`,
  `guaranteed_factor` and `fallible_factor` in the ledger parameters. They apply
  to compute cost only, and `validation_cost` now returns the cost with the
  factor applied.
- feat: `apply_guaranteed_only` and `GuaranteedApplyResult` allow split-phase
  transaction execution with deferred event generation.
- fix: Dust registration accounting uses block time rather than the declared
  transaction time.
- fix: the cost heuristic is tighter, and moves fewer transactions to the
  fallible section.
- fix: refine transcript gas accounting for transcripts that both write and
  delete state.

### Security

These fixes were released to mainnet as `8.1.2` and `8.1.3`, and reach the
`ledger-9` line in this release.

- security: hardening of low-level deserialization across `serialize`,
  `base-crypto`, `storage`, `onchain-state`, `onchain-vm` and
  `transient-crypto`. Encodings that are not canonical, and values violating
  their type's invariant, are rejected rather than decoded. See the per-crate
  changelogs for the individual rules.
- security: contract call transcripts must embed canonical field values, across
  `base-crypto`, `onchain-state` and `onchain-vm`. Non-canonical `field` atoms in
  `push`, `pushs` and `idx`-style path keys are rejected.
- security: `noop 0` in a contract call transcript is rejected as not
  normalized.

### Dependencies

- chore: `base-crypto` moves to `k256` 0.14 and `signature` 3;
  `transient-crypto` no longer depends on either.
- chore: zkir v2 verifies zk-stdlib v1 proofs through `midnight-transient-crypto`
  `2.2.0`, replacing the `zkir-old` dependency.

## New features requiring configuration updates

Ledger parameters gain the price floor and the `validation_factor`,
`guaranteed_factor` and `fallible_factor` fields, and lose
`parallelism_factor`. Tooling that constructs or serializes `LedgerParameters`
must supply the new fields. The initial `validation_factor` of 1/4 carries over
the previous `parallelism_factor` of 4.

## Deprecations

*None in this release.*

## Breaking changes or required actions for developers

- **Verifier keys change.** `midnight-circuits` 7.3.0 and `midnight-zk-stdlib`
  2.4.0 change the verifier keys of zkir V2 circuits that use `hash_to_curve`,
  and of zkir v3 circuits that `encode` a Jubjub scalar. V0 and V1 keys are
  unchanged. Affected contracts need their keys regenerated.
- **zkir defaults.** The default `IrMinorVersion` is now `V2`, and
  `VersionedInnerPK` is removed: `Zkir::ProverKey` is `MidnightPK<IrSource>`.
- **Signing envelopes.** Their construction is unified; code building them by
  hand must move to the shared construction.
- **Cost parameters.** `parallelism_factor` is removed, and `validation_cost`
  returns a factored cost (see
  [Transactions and fees](#transactions-and-fees)).
- **Stricter decoding and validation.** The security fixes reject data and
  contract calls that earlier `ledger-9` release candidates accepted.
- **`AlignedValueChecked`.** `onchain-vm`'s `Key::Value` and `Op::Push` hold
  `AlignedValueChecked` in place of `AlignedValue`. Rust code constructing them
  directly must convert with `AlignedValueChecked::try_from`.
- **npm scope.** Packages are published under `@midnightntwrk`, previously
  `@midnight-ntwrk`. The legacy scope still receives releases for now.

## Fixed defect list

| **Component** | **Description** |
|---------------|-----------------|
| ledger | Dust registration accounting uses block time rather than declared transaction time. |
| ledger | `DustParameters::time_to_cap` guards against a zero `generation_decay_rate` instead of dividing by zero. |
| ledger | Dust `seq` increments saturate. |
| ledger | Contract call cost accounting counts public inputs via `ContractCall::public_inputs_len`, with saturating arithmetic. |
| ledger | Dust event processing is associative. |
| ledger | `night_indices` are garbage collected, removing a potential panic in wallet state processing. |
| ledger | Bridge fee processing no longer risks a panic. |
| ledger | `communication_commitment` hashes the value-only representation of its inputs. |
| ledger | Transcript gas accounting is refined for transcripts that both write and delete state. |
| ledger | `ContractOperationVersionedVerifierKey::V3` and `V4` serialize independently. |
| zswap | Binding randomness extraction no longer panics on a proof preimage with no witness to extract from. |
| zswap | Delta accumulation in `normalize_deltas` saturates. |
| storage | MPT path removal no longer risks a panic. |
| transient-crypto | Coin ciphertext decryption correctly excludes the identity point. |
| zkir-wasm | `prove()` and `provingProvider().prove` apply `overwriteBindingInput` for `verifier-key[v6]` circuits, so in-process Dust spend proofs are no longer rejected with `InvalidDustSpendProof` (#832). |

## Known defects

| **Issue** | **Description** |
|-----------|-----------------|
| [#839](https://github.com/midnightntwrk/midnight-ledger/issues/839) | A contract call that spends a held shielded coin and writes the change back with `writeCoin` can be rejected with `Custom error: 104` (ran out of gas budget) on a chain whose commitment tree is non-empty. The node measures a few more `bytes_written` than the client declares, because the stored coin's `mt_index` is its real tree position rather than the output ordinal the client rehearsed with. |
| [#838](https://github.com/midnightntwrk/midnight-ledger/issues/838) | A contract call paid with exactly one DUST spend can fail the time-to-dismiss check with `OutsideTimeToDismiss`, while the same call with zero, two or more DUST spends is accepted. The pre-proof size estimate overstates the transaction's size, so the client can believe it fits when the node rejects it. Paying with two or more DUST spends avoids it. |
