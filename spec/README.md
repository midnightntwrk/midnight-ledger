# Midnight ledger specification

This space is intended to specify formats and behaviours of Midnight, starting
with the Midnight ledger. The specification should eventually be in literate
agda, but is starting its life as rust sketches, providing both a prose
description of intention and reasoning, and a precise definition.

>  [!IMPORTANT]
>
> This spec *does not* cover the entire behaviour of the ledger. In particular,
> *events* are not yet specified. Further, while data formats are now defined
> for transactions, the onchain VM, and ZKIR, gaps remain elsewhere.
>
> These gaps in the specification are intended to be closed over time.

The parts of this specification are:
- [Preliminaries](./preliminaries.md), describing various preliminaries and
  primitives used in other sections.
- [Field-Aligned Binary](./field-aligned-binary.md), the byte and
  field-element format used for leaf values inside transactions and state.
- [Zswap](./zswap.md), describing shielded tokens on Midnight
- [Night](./night.md), describing Night and other unshielded tokens on Midnight
- [Dust](./dust.md), describing Dust payments and generation. This part of the
  spec is still in progress, although the key format may be treated as fixed.
- [Contracts](./contracts.md), abstractly describing contract states and
  interactions in transactions, without specifying the details of the structure
  of proofs and the onchain VM.
- [Intents & Transactions](./intents-transactions.md), describing Midnight's
  composite transaction format, and intents on Midnight (the narrative spec).
- [Transaction Format](./transaction-format.md), the concrete structure and
  wire-serialization (`midnight:<tag>:` containers, `[vN]` versioning) of
  every transaction type, reconciled against the implementation.
- [Transaction Type Catalog](./transaction-types.md), the per-variant
  reference for `Transaction::Standard`, `Transaction::ClaimRewards`, and
  the nine `SystemTransaction` variants.
- [Onchain Runtime](./onchain-runtime.md), describing the Impact VM —
  programs, `StateValue`s, kernel operations, context, and effects.
- [Impact Opcodes](./impact-opcodes.md), the per-opcode reference: binary
  encoding, semantics, error conditions, and gas / cost model.
- [ZKIR](./zkir.md), the zero-knowledge IR — the instruction set the Compact
  compiler emits for each circuit (v2 reference plus v3 changes).
- [Cost Model Architecture](./cost-model.md), the methodology behind the
  ledger's multi-dimensional cost model.
- [Storage I/O Cost Modeling](./storage-io-cost-modeling.md), how storage
  costs are modelled.
- [Cardano System Transactions](./cardano-system-transactions.md), the
  partner-chain / bridge integration.
- [Properties](./properties.md), describing the security and correctness
  properties of Midnight's transactions.
