# ZKIR v3 — Agda formalization

This directory contains the Agda mechanization of **ZKIR major version 3**,
tracking the Rust `zkir-v3` crate in `midnight-ledger` (pinned at commit
`a1d1611c`, `midnight-zkir-v3 3.0.0-rc.2`; byte-identical to the earlier
pin `b17df9d1`). It covers the **full
seven-type surface** — Native, Bytes32, Jubjub point/scalar, and the
Secp256k1 point/base/scalar triple — with all 34 instructions over those
types.

Companion documents: the textual spec
[`docs/zkir-v3-spec.md`](../../docs/zkir-v3-spec.md) and the
implementer-facing divergence review
[`docs/zkir-v3-divergence-review.md`](../../docs/zkir-v3-divergence-review.md).
The ultimate source of truth is the Rust implementation.

## Headline results

The development relates the off-circuit interpreter (`preprocess`, a
deterministic function) to the synthesized constraint system (`synth`) at
two levels. All results are `--safe` with **no `postulate`s**; the trust
base is the [`Assumptions`](Assumptions.agda) record, threaded through
every module as a parameter.

**Canonical-witness faithfulness** (`circuit-faithful` in
[`CircuitProof.agda`](CircuitProof.agda)) — the v3 analogue of v2's P5.
For a well-typed producer, given the run-shape data (`BwdWalk` spine and
terminal `Consumed` facts — the analogue of v2's `preprocess-shaped`):

```
preprocess S P ≡ just s  ⇔  satisfies (synth S) (witness-of P s)
```

The shape data is projected from a successful run by
`preprocess→BwdWalk` (the v2 `R⇒preprocess-shaped` analogue): 34
per-instruction step inversions (`step→bwd`) folded by `run→BwdWalk`,
so a caller holding a run needs no hand-built spine.

**Statement soundness, pis-form** (`statement-sound` in
[`StatementSoundness.agda`](StatementSoundness.agda)). From an *arbitrary*
satisfying witness `w` of shape `WShape S w`, a preimage and run are
constructed whose canonical witness agrees with `w` on the public inputs,
whose memory is a sub-assignment of `w` (exact agreement on the run's
domain), and — under the commitment flag — whose commitment randomness
matches `w`'s:

```
producer-WT S → satisfies (synth S) w → WShape S w → SubRealizer S w
```

where a `SubRealizer S w` bundles a preimage `P'` and a final state `s`
with `run-shaped S P' s` and outright `preprocess S P' ≡ just s`,
`pis (witness-of P' s) ≡ pis w`, `mem s ⊑ᵂ w`, the
commitment-randomness agreement under the flag, and a `CommWF`
component (`do-comm S ≡ false → comm-commitment P' ≡ nothing`, excluding
vestigial commitment pairs when the flag is off).

An exact-equality extraction form (`witness-of P' s ≡ w`) is **false** for
v3: the Rust deliberately leaves transcript-input cells and their guards
unconstrained in-circuit, so `w` is free off the run's domain. The `⊑ᵂ`
component is the honest strengthening available.

**Extraction uniqueness** (`statement-unique` and
`statement-sound-unique` in
[`StatementUniqueness.agda`](StatementUniqueness.agda)). Any two
sub-realizers of the same witness have equal preimages and equal states,
and combined with `statement-sound`: **exactly one** `SubRealizer` per
satisfying `WShape` witness — the extractor is a function of the witness.

**Extractor completeness** (`extractor-complete` in
[`StatementSoundness.agda`](StatementSoundness.agda)). The extracted
preimage actually proves: the canonical witness of a `SubRealizer`'s
preimage/state pair itself satisfies the synthesized circuit
(`forward-sa` through the realizer's `preproc-ok` field).

**`WShape` non-vacuity** (`preprocess→WShape` in
[`StatementSoundness.agda`](StatementSoundness.agda)). The canonical
witness of every successful run is in the extraction class: each
`WSteps` conjunct is forced by its step's success (via the `step→bwd`
inversions), so `statement-sound` applies to every honestly-provable
statement.

Four producer checks are decidable and runnable: `producer-SA?`
(single-assignment well-formedness, [`Obligations.agda`](Obligations.agda)),
`producer-WT?` (adds the value-typing pass), `producer-WF2?` (the spec §5.3
bit-count bounds, which the Rust preprocess checks dynamically but the
mechanized `step` deliberately omits), and `WShape?` (witness shape,
[`StatementSoundness.agda`](StatementSoundness.agda)). The WF2 transfer
is itself a theorem (`preprocessʳ-agree`, Obligations): the Rust-faithful
`preprocessʳ`, whose step re-checks the bounds, agrees with `preprocess`
on every WF2-conforming source.

## Modules

Import [`Main.agda`](Main.agda) to type-check the entire development. The
dependency chain:

```
Assumptions → Types → Encoding → Syntax → Semantics → SemanticsProperties
  → Circuit → CircuitBridge → CircuitFaithfulness → CircuitBackward
  → Obligations → CircuitProof → StatementSoundness
  → StatementUniqueness → Main
```

| Module | Contents |
| --- | --- |
| [`Assumptions.agda`](Assumptions.agda) | The trust base: carriers (`Fr`, `Alignment`, `JubjubPoint`, `JubjubScalar`, `SecpPoint`, `SecpBase`, `SecpScalar`; concrete `Byte`/`Bytes32`), field/curve/byte/hash/commitment operations (incl. the Secp256k1 foreign-field and foreign-ECC chip contracts), the non-triviality law `1ᶠ≢0ᶠ`, the typed-encoding round-trip laws (Group C), and the derived valuation `valFr`. |
| [`Types.agda`](Types.agda) | `IrType` (7 constructors), `IrValue`, `encoded-len`, `typeof`, decidable type equality — mirrors `ir_types.rs`. |
| [`Encoding.agda`](Encoding.agda) | `encode`/`decode` between `IrValue` and `List Fr`, derived from the per-type trust-base primitives. |
| [`Syntax.agda`](Syntax.agda) | `Identifier`, `Operand`, the 34-variant `Instruction`, `IrSource` — mirrors `ir.rs`. |
| [`Semantics.agda`](Semantics.agda) | The off-circuit interpreter: `step`/`run`/`init`/`preprocess` over the typed named-register state; `ProofPreimage`. |
| [`SemanticsProperties.agda`](SemanticsProperties.agda) | Circuit-free consequences: store orders `_⊑_`/`_≼_`, freshness and domain lemmas, run inversion/extension, `Consumed`/`run-shaped`, and the `preprocess` inversions. |
| [`Circuit.agda`](Circuit.agda) | The constraint vocabulary and its semantics (`holds`), witness model (`CircuitWitness`), `satisfies`, and the total synthesis function `synth`. Chips are modeled at contract level (spec §7.0). |
| [`CircuitBridge.agda`](CircuitBridge.agda) | The witness/constraint bridge shared by both proof directions: `witness-of`, resolution transports, constraint monotonicity (`holds-mono`) and lowering (`Defd`/`holds-lower`), and the constraint-extraction family (`csOf`). |
| [`CircuitFaithfulness.agda`](CircuitFaithfulness.agda) | The forward direction: per-instruction `*-fwd` lemmas and the program-level induction `forward`. |
| [`CircuitBackward.agda`](CircuitBackward.agda) | The backward per-instruction layer: 39 `*-bwd` step-reconstruction lemmas (`from-bytes32` contributes three). Independent of the forward module. |
| [`Obligations.agda`](Obligations.agda) | Static producer checks, circuit-free: the single-assignment scan (`producer-SA`/`producer-SA?`), the value-typing pass (`producer-WT`/`producer-WT?`), the WF2 bit-bound check (`producer-WF2`/`producer-WF2?`), and the Rust-faithful semantics with its transfer theorem (`stepʳ`/`preprocessʳ`/`preprocessʳ-agree`). |
| [`CircuitProof.agda`](CircuitProof.agda) | Program-level assembly: the backward spine (`BwdWalk`) and fold (`bwd-go`), the run→spine projection (`step→bwd`/`run→BwdWalk`/`preprocess→BwdWalk`), `backward`, `forward-sa`, and the headline `circuit-faithful`. |
| [`StatementSoundness.agda`](StatementSoundness.agda) | Pis-form statement soundness: the witness-shape predicate `WShape`/`WShape?`, the w-only preimage pre-passes, the 34-clause `build` run-realizer, the `CommWF`/`SubRealizer` conclusion record, `statement-sound`, `extractor-complete`, and the `WShape` non-vacuity `preprocess→WShape`. |
| [`StatementUniqueness.agda`](StatementUniqueness.agda) | Extraction uniqueness: the transcript-pinning walk, `statement-unique`, `statement-sound-unique`. |
| [`Main.agda`](Main.agda) | Aggregator; type-checking it verifies the whole development. |

## Trust base and modeling level

The mechanization is parametric in the cryptography: the `Assumptions`
record collects the carriers and the functional contracts of the
`midnight-circuits` chips. Chips — including the range/decomposition
family (`div-mod`, `less-than`, `in-range`, `reconstitute`) — are modeled
at **contract level**: each constraint's `holds` clause states the
canonical semantic answer and trusts the chip that implements it (see the
decision record in spec §7.0). Faithfulness therefore catches
orchestration errors and off-/in-circuit divergences, but is sound only
insofar as `midnight-circuits` is. The Jubjub `from-coordinates`
subgroup contract was verified faithful against the chip source (the
chip enforces membership by in-circuit cofactor-clearing; see the note
in [`Assumptions.agda`](Assumptions.agda)). The `less-than` contract
states the chip's evened operand bound (`max(bits + bits%2, 4)` —
`even4` in [`Circuit.agda`](Circuit.agda)), matching the deployed
circuit; the exact `2^bits` bound the off-circuit run checks is carried
as a `WShape` conjunct and `BwdStep` side data instead — see spec §9
("Contract strength"). The Secp256k1 limb *decoders* are modeled as the
canonical partial inverses, which is stricter than the deployed
off-circuit `from_public_input` (audited 2026-07-24: it silently reduces
non-canonical encodings) — theorems transfer to the deployed decoder on
canonically-encoded data; see spec §9 ("Secp256k1 decode canonicity")
and the TRUST NOTE in [`Assumptions.agda`](Assumptions.agda).

## Type-checking

See [`../README.md`](../README.md) for the toolchain; checking
[`Main.agda`](Main.agda) verifies the entire `--safe` development:

```sh
agda --safe -i src src/zkir-v3/Main.agda
```

A cold check takes a few minutes (CircuitFaithfulness and
StatementSoundness dominate).

## Proof idioms / gotchas (learned the hard way)

- A local variable named `eq` clashes with the `Constraint` constructor
  `eq` (from `constrain-eq`). Name equality variables `ieq` / `e`.
- Implicits don't infer through the non-injective `init` / `synth` /
  `run` / `step` / `out1` — **pin them explicitly** (e.g.
  `init-pi0 {S}{P}{st0}`, `run-extends {P}{S}{s}`).
- To make `do-communications-commitment S` *definitionally* `false`/`true`
  where `synth`/`init` must reduce, use a helper whose arguments contain no
  `do-comm S` (a top-level `with do-comm S` orphans a `where`-block's
  implicits).
- `_⊑_` is a Π over functions `{id v}`; the unifier stalls on
  `⊑-refl`/`⊑-trans` implicits — give them explicitly or η-expand
  (`λ {id}{v} p → g (f p)`).
- The `*-fwd`/`*-bwd` lemmas are stated at the **immediate post-step
  witness** `witness-of P (out1 st out v)`; the program-level lift to the
  final witness is `holds-mono` + `run-extends` (forward) and
  `holds-lower` + the spine's monotonicity data (backward).
- Multi-instruction inductions must **enumerate all 34 instructions**; a
  catch-all leaves `i` opaque and blocks the needed reductions.
- `grep` for lemma names with `[a-zA-Z-]*` misses `bytes32`/`keccak256`
  names (digits) — include `0-9`.
