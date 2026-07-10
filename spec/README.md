# Midnight ledger specification

This space is intended to specify formats and behaviours of Midnight, starting
with the Midnight ledger. The specification should eventually be in literate
agda, but is starting its life as rust sketches, providing both a prose
description of intention and reasoning, and a precise definition.

>  [!IMPORTANT]
>
> This spec *does not* cover the entire behaviour of the ledger. In particular,
> currently *system transactions*, the *cost model*, and *events* are either not
> specified or specified to a limited extent. Further, the spec currently *does
> not* define data formats in detail, and therefore is insufficient to reproduce
> the ledger's behaviour in isolation.
>
> These gaps in the specification are intended to be closed over time.

The parts of this specification are:
- [Preliminaries](./preliminaries.md), describing various preliminaries and
  primitives used in other sections.
- [Zswap](./zswap.md), describing shielded tokens on Midnight
- [Night](./night.md), describing Night and other unshielded tokens on Midnight
- [Dust](./dust.md), describing Dust payments and generation. This part of the
  spec is still in progress, although the key format may be treated as fixed.
- [Contracts](./contracts.md), abstractly describing contract states and
  interactions in transactions, without specifying the details of the structure
  of proofs and the onchain VM.
- [Intents & Transactions](./intents-transactions.md), describing Midnight's
  composite transaction format, and intents on Midnight.
- [Properties](./properties.md), describing the security and correctness
  properties of Midnight's transactions.
