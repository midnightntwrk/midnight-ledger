# ZKIR v2 — Agda formalization

This directory contains the Agda mechanization of **ZKIR major version 2**
(minor version V1, the current default lowering). It formalizes the abstract
syntax of the IR, its two semantics — the *preprocess* (witness-generation)
semantics and the Halo2 PLONKish *circuit* semantics — and the central
correctness theorem connecting them, together with the producer obligations
(spec §6.4): checkable circuit well-formedness conditions that guarantee
the two semantics coincide.

The companion textual specification is [`docs/zkir-v2-spec.md`](../../docs/zkir-v2-spec.md);
section references below (e.g. §5.2, §6.2) point into it. The ultimate
source of truth is the Rust implementation referenced from that spec.

## Headline result

The development discharges **P5** (spec §6.2): *circuit faithfulness*. For a
*producer-safe* (spec §6.4) source with the WF1/WF2 well-formedness bounds
(spec §3.3), and for every *preprocess-shaped* state `s`, the operational
relation `R src pre s` holds **iff** the synthesized circuit is satisfied by
the canonical witness:

```
R src pre s  ⇔  satisfies (circuit src) (witness-of s pre)
```

The `preprocess-shaped` hypothesis restricts `s` to states with the arity
shape of a run: `public_input`/`private_input` emit no constraint pinning
their output wire (spec §5.2), so satisfaction alone cannot see transcript
reads. It pins only suffix *arities*, never computed values, so `satisfies`
remains load-bearing; all four hypotheses constrain only the backward
direction and hold automatically for any state arising from `R`.

This was previously a postulate in the spec; it is now fully mechanized
(`circuit-faithful` in [`CircuitProof.agda`](CircuitProof.agda), re-exported
from [`Properties.agda`](Properties.agda)). No axioms are introduced by the
proof itself — it rests only on the cryptographic trust base collected in
the [`Assumptions`](Assumptions.agda) record described below.

On top of P5 the development proves **statement soundness**
(`circuit-statement-sound` in
[`StatementSoundness.agda`](StatementSoundness.agda)): P5 speaks only about
the *canonical* witness of a given run, while statement soundness is the
universal direction — for a producer-safe source, **every** satisfying
witness of the synthesized circuit is the
canonical witness of some genuine preprocess run:

```
satisfies (circuit src) w  →  Realizer src w
```

where `Realizer src w` is a record packaging a run `(pre , s)` together
with the proofs `R src pre s` and `witness-of s pre ≡ w`.

Two refinements complete the picture. `circuit-witness-characterization`
(same module) combines this with P5's forward direction into an *iff*: the
satisfying witnesses are **exactly** the
canonical witnesses of preprocess runs. And **extraction uniqueness**
(`circuit-statement-unique` in
[`StatementUniqueness.agda`](StatementUniqueness.agda)) shows the realizing
run is *unique* — for commitment-well-shaped preimages (`CommWF`; when the
commitment flag is off, a vestigial `comm-commitment` is invisible to the
semantics, so it must be excluded) — making the extractor a function of the
witness (`circuit-statement-sound-unique`: exactly one `CommWFRealizer`,
the `Realizer` record extended with the `CommWF` proof).

The development contains **no `postulate`s**: the trust base is a structured
record taken as a module parameter, so every module type-checks under Agda's
`--safe` flag.

## Modules

Import [`Main.agda`](Main.agda) to type-check the entire development at once.
Every module takes the [`Assumptions`](Assumptions.agda) record as a module
parameter (`module M (⋯ : _) (open Assumptions ⋯) where`), so `Assumptions`
sits at the root of the dependency layering:

```
Assumptions → FieldProperties → Syntax → { Semantics → Circuit, Obligations }
FieldProperties → Encoding            (consumed by the proof modules)
Semantics → SemanticsLemmas           (consumed by the proof modules)
Circuit → CircuitFaithfulness → CircuitProof → { Properties, StatementSoundness }
Obligations → ObligationsSoundness → { CircuitProof, StatementSoundness }
{ Properties, StatementSoundness } → StatementUniqueness
```

| Module | Contents |
| --- | --- |
| [`Assumptions.agda`](Assumptions.agda) | The entire trust base as one record: carrier types (`Fr`, `Alignment`), field/curve/hash/commitment operations, derived helpers (`to-bool`, `fits-in`, `bits-lt`, `pow2-fr`, `lt-bits`), and the field's prime-order valuation laws + the LE encoding round-trip law (the higher-level bit-arithmetic identities are *derived* here, not assumed). Downstream modules take it as a parameter; nothing is `postulate`d. |
| [`FieldProperties.agda`](FieldProperties.agda) | Derived field/bit helpers — the boolean equality test `_≡ᶠ?_` and its reflection laws, `to-bool`, the `Fits-in`/`NoWrap` predicates, `bits-lt`, `pow2-fr`, the padded bit-count `lt-bits`, and the pure LE bit-value machinery (`bits-to-ℕ` splits and bounds). Everything here is a consequence of the `Assumptions` operations. |
| [`Encoding.agda`](Encoding.agda) | Consequences of the encoding/valuation axioms: the operational characterization of `to-bool`, the `fits-from-le-bits-*` truncation facts, and the bit-arithmetic identities (`bits-decomp-split`, `reconstitute-no-overflow`, `bits-lt-pad`, `div-mod-constraint-unique`, …) the faithfulness and soundness proofs consume. |
| [`Syntax.agda`](Syntax.agda) | Abstract syntax (spec §3): the 26-variant `Instruction` datatype, `Index` operands, IR minor versions, and an `IrSource`. Mirrors the Rust `ir.rs` in source order. |
| [`Semantics.agda`](Semantics.agda) | Preprocess / operational semantics (spec §4): the small-step relation over the witness-population state. The field, Jubjub-curve, hashing, and commitment operations come from the `Assumptions` parameter. |
| [`SemanticsLemmas.agda`](SemanticsLemmas.agda) | Inversion and framing principles for the `Semantics` definitions: `mem-lookup` stability under memory extension, inversion of the transcript consumers (`consume-pub-out`/`consume-priv`), and the `init-state` inversion record (`InitInv`). The single home for this lemma layer; the proof modules import it rather than re-proving these facts privately. |
| [`Properties.agda`](Properties.agda) | Top-level correctness properties (spec §6). Re-exports `circuit-faithful` (P5) and bundles the spec's stated guarantees. |
| [`Circuit.agda`](Circuit.agda) | Halo2 PLONKish circuit semantics (spec §5). Defines `Constraint`/`Circuit`, the deterministic synthesis function `circuit` (§5.2 emission contracts), the `Witness` assignment model, and the `satisfies` relation. Chip behaviour is interpreted via the same canonical functions used by the preprocess semantics. |
| [`CircuitFaithfulness.agda`](CircuitFaithfulness.agda) | Per-instruction faithfulness lemmas: forward and backward bridging between `R-instr` and the emitted constraints, instruction by instruction. |
| [`CircuitProof.agda`](CircuitProof.agda) | Program-level induction that assembles the per-instruction lemmas into the full `circuit-faithful` equivalence. Contains no postulates. |
| [`Obligations.agda`](Obligations.agda) | Producer obligations (spec §6.4) as linear-scan checker functions: O0 (operand-index discipline, `wire-disc`), O1 (PiSkip discipline), O2 (Boolean-UB freedom), O3 (ReconstituteField / no field overflow). `producer-safe` is their four-way conjunction. |
| [`ObligationsSoundness.agda`](ObligationsSoundness.agda) | Soundness of the obligation checkers: connects the static scans (`O2`/`O3`) to the dynamic relational semantics, by induction along the `R-instrs` trace. Feeds the backward bridging proofs. |
| [`StatementSoundness.agda`](StatementSoundness.agda) | Statement soundness (`circuit-statement-sound`): every satisfying witness of `circuit src` is *realizable* — it is the canonical witness `witness-of s pre` of a genuine preprocess run, for every producer-safe source. The universal property P5 lacks; builds the realizing run from the witness and closes via P5's backward direction. Also `circuit-witness-characterization`: with P5's forward direction, the satisfying witnesses are *exactly* the canonical run witnesses. |
| [`StatementUniqueness.agda`](StatementUniqueness.agda) | Extraction uniqueness (`circuit-statement-unique`): two commitment-well-shaped (`CommWF`) preimage/state pairs realizing the same witness are equal — a parallel induction over the two `R-instrs` traces, anchored on the common final memory/pis, pins every preimage field (the verifier transcript via O1's full bracketing). `circuit-statement-sound-unique` packages existence + uniqueness: exactly one `CommWF` realizer per satisfying witness (with flag-respecting `comm-rand`). |
| [`Main.agda`](Main.agda) | Aggregator that imports every module above; type-checking it verifies the whole development. |

## Trust base (`Assumptions`)

The mechanization is parametric in the underlying cryptography. Rather than
`postulate`s, the trust base is a single structured record,
[`Assumptions`](Assumptions.agda), threaded through every module as a
parameter. It collects:

- field types and arithmetic over the BLS12-381 scalar field `Fr`,
- bit decomposition and in-field range predicates,
- Jubjub elliptic-curve operations,
- the Poseidon / persistent hash and commitment functions,
- byte-`Alignment` descriptors,
- the field's prime-order (ℤ/|Fr|) valuation laws and the canonical
  little-endian encoding round-trip law (from which the higher-level
  bit-arithmetic identities — reconstitution, range/`fits` monotonicity,
  quotient/remainder splitting, padded comparison — are *derived*, not
  assumed).

No concrete instantiation of `Assumptions` is provided yet; the development
is intentionally abstract over this interface. Because nothing is
`postulate`d, the whole development type-checks under `--safe`. All other
results — including P5 — are proved, not assumed.

## Type-checking

See [`../README.md`](../README.md) for the toolchain and commands; checking
[`Main.agda`](Main.agda) verifies the entire `--safe` development.
