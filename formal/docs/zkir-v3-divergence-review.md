# ZKIR v3 — in-circuit / off-circuit divergence review

A review, for the `zkir-v3` implementation team, of the places where the
in-circuit constraint system (`<IrSource as Relation>::circuit`,
`ir_vm.rs:705`) enforces strictly less than the off-circuit interpreter
(`IrSource::preprocess`, `ir_vm.rs:185`) requires. All line numbers refer to
`midnight-ledger` at `a1d1611c` — the `ledger-9` head at the 2026-07-24
re-pin, byte-identical in `zkir-v3/` to the earlier pin `b17df9d1`
(`midnight-zkir-v3 3.0.0-rc.2`), so all line numbers are valid at both.

Background: we maintain a machine-checked model of zkir-v3 (an Agda
mechanization in this repository) and have proved that any witness
satisfying the synthesized constraint system corresponds to an execution of
`preprocess` with the same public inputs — *under explicit side conditions*.
Each side condition marks a spot where a witness can satisfy the circuit
even though no `preprocess` run would produce it. We audited every such spot
against the Rust. Most turned out to be enforced by the real circuit and
need no attention (listed at the end); this memo reports the cases where
the divergence is real.

**Scope.** The model covers all seven `IrType`s — `Native`, `JubjubPoint`,
`JubjubScalar`, `Bytes32`, and (since 2026-07-15) the Secp256k1 triple
(`Secp256k1Point`, `Secp256k1Base`, `Secp256k1Scalar`) — and every
instruction over them. The Secp256k1 arms of the type-dispatching
instructions (`Add`, `Mul`, `Neg`, `Inv`, `EcMul`, `EcMulGenerator`,
`TestEq`, `ConstrainEq`, `CondSelect`, the coordinate and byte
conversions, `Encode`) are audited below alongside the rest; the
foreign-field and foreign-ECC *chips* themselves (`midnight-circuits`
`AssignedField`/`AssignedForeignPoint`) are modeled at the level of their
functional contracts and are trusted, not re-verified.

Why it matters: a witness that satisfies the circuit but corresponds to no
`preprocess` execution is exactly what a malicious prover can use. Whether a
given divergence is exploitable depends on whether the unconstrained value
can influence a public input or a circuit output; the findings below note
this per case. There is precedent for this class being real: the
DivModPowerOfTwo canonicity issue in an earlier ZKIR version, fixed
in-circuit by `enforce_canonical`, is the same shape of gap — and our
model of that version independently flags the same spot (its faithfulness
proof does not go through without the canonicity guard).

## Summary

| Finding | Assessment | Suggested action |
|---|---|---|
| 1. ReconstituteField result can wrap mod the field | genuine divergence | in-circuit no-overflow check |
| 2. Assert constrains `≠ 0`; semantics requires `= 1` | documented UB | constrain `= 1`, or keep as documented UB |
| 3. PublicInput/PrivateInput guard–value coupling | by design | document for producers |
| 4. LessThan bounds enforced at the evened width | minor divergence | document the effective bound |
| 5. Secp256k1 decode accepts non-canonical limb encodings | completeness gap + panic | reject non-canonical input; validate flag; error instead of panic |

Everything else we audited is already enforced in-circuit — see "Also
checked, found enforced" at the end.

## 1. ReconstituteField: result can wrap modulo the field

**What the circuit enforces.** For
`ReconstituteField { divisor, modulus, bits, output }`, the circuit asserts
two range bounds — `divisor < 2^(FR_BITS − bits)`
(`assert_lower_than_fixed`, `ir_vm.rs:1036-1040`) and `modulus < 2^bits`
(`ir_vm.rs:1041-1045`) — and then computes the output as the field
`linear_combination` `modulus + 2^bits · divisor` (`ir_vm.rs:1048-1055`),
which reduces modulo `FR_ORDER`.

**What preprocess requires.** The same two range checks, *plus* a
big-integer comparison against the field maximum:
`bail!("Reconstituted element overflows field")` when the integer sum
`modulus + 2^bits · divisor` exceeds it (`ir_vm.rs:445-447`).

**The gap.** The two range bounds do not imply no-overflow. The integer sum
ranges over `[0, 2^FR_BITS − 1]`, and `FR_ORDER < 2^FR_BITS`, so sums in
`[FR_ORDER, 2^FR_BITS − 1]` pass both range checks yet overflow. A prover
can therefore supply in-range `(divisor, modulus)` whose sum wraps: the
circuit accepts, with `output` bound to the *wrapped* field value — a value
no honest execution produces — while `preprocess` rejects the same inputs.

**Why it is worth attention.** ReconstituteField is the documented inverse
of DivModPowerOfTwo (`ir.rs:643`), and this is the same shape as the
DivModPowerOfTwo canonicity issue that motivated `enforce_canonical`: the
in-circuit relation admits non-canonical (wrapping) witnesses that the
off-circuit function never emits. If a reconstituted output can flow into a
public input or circuit output, the prover controls a value the semantics
says cannot exist.

**Suggested action.** Add an in-circuit no-overflow assertion mirroring the
off-circuit comparison — e.g. a bound on the reconstituted result against
`FR_ORDER`, as `enforce_canonical` does for DivModPowerOfTwo. Cost: one
comparison constraint per ReconstituteField. Alternatively, first assess
whether a reconstituted output can reach a public input under the intended
producer discipline, and if not, document no-overflow as an explicit
producer obligation.

## 2. Assert: constrains `≠ 0` where the semantics requires `= 1`

**What the circuit enforces.** `std.assert_non_zero(layouter, &cond)`
(`ir_vm.rs:833`): the condition is non-zero. Neither `cond = 1` nor
booleanity is constrained.

**What preprocess requires.** The condition is resolved as a bool and must
be `true` (`ir_vm.rs:337-338`); any value other than `0` or `1` is rejected
outright (`resolve_operand_bool`, `ir_vm.rs:240-251`). So off-circuit,
Assert passes only on exactly `1`.

**The gap.** A witness with `cond ∈ {2, …, p−1}` satisfies the circuit but
is rejected off-circuit. The instruction documentation already acknowledges
this: "Assert that `cond` has value `1`. UB if `cond` is not `0` or `1`"
(`ir.rs:380`).

**Why it is (only) moderate.** Assert has no output, so it is not directly
a value-forgery vector. The divergence matters when the asserted wire is
*not* produced by an instruction that already constrains it boolean
(TestEq, LessThan, Not all do). For a wire coming from arbitrary arithmetic
or a free witness, "assert" in-circuit means only "non-zero", which is
weaker than the intended "true".

**Suggested action.** Either keep the documented-UB contract (no code
change) and rely on producers asserting only boolean-producing wires, or
strengthen the constraint to `cond = 1` (one equality constraint per
Assert), which removes the UB surface entirely.

## 3. PublicInput / PrivateInput: guard and value are uncoupled in-circuit

**What the circuit enforces.** Both instructions witness a fresh value and
allocate it well-typed via `assign_incircuit` (`ir_vm.rs:999-1003`), so the
*type* of the cell is constrained. The guard deliberately takes no part in
the constraints — the instruction documentation is explicit: "the `guard`
DOES NOT participate in in-circuit constraints" (`ir.rs:804-805`,
`827-828`).

**What preprocess requires.** The guard is read as a bool (rejecting
non-`0`/`1`, `ir_vm.rs:240-251`) and branches: guard-false binds the
default value for the type; guard-true decodes the next transcript element
(`ir_vm.rs:353-365` public, `367-383` private).

**The gap.** In-circuit, nothing ties the witnessed value to the guard: a
witness may put any well-typed value in an "inactive" cell (where
preprocess would bind the default), and the guard wire itself need not be
boolean unless constrained elsewhere. Several distinct witnesses therefore
satisfy the circuit for the same public inputs.

**Why this is by design.** These cells are the prover's inputs — the
witnessed value is *supposed* to be free, and the public inputs are pinned
independently (via Impact). The divergence does not let the circuit accept
a false statement; it only means the in-circuit relation does not
distinguish active from inactive input cells. This is the intended slack of
any circuit with private inputs.

**Suggested action.** Document for producers and auditors: transcript-input
cells are free in-circuit (only their type is constrained), and the guard
is not a circuit input — any guard/value coupling must come from how the
value is used downstream. No code change suggested unless that coupling is
ever made load-bearing.

## 4. LessThan: operand bounds enforced at the evened bit width

**What the circuit enforces.** `LessThan { a, b, bits, output }` lowers to
`std.lower_than(layouter, &a, &b, max(bits + bits%2, 4))` (`ir_vm.rs:966`;
the library requires an even bound ≥ 4). The wrapper converts each operand
via `bounded_of_element`, which **does** emit an enforced range check —
`assert_lower_than_fixed(x, 2^n)` (`midnight-circuits 7.2.2`,
`field/native/native_gadget.rs:332-348`, verified 2026-07-24) — but at the
*evened* bound `n = max(bits + bits%2, 4)`.

**What preprocess requires.** Both operands strictly below the exact
`2^bits` (`resolve_operand_bits(_, Some(*bits))`, bailing with "Bit bound
failed" otherwise).

**The gap.** For odd `bits`, the circuit admits operands in
`[2^bits, 2^(bits+1))`; for `bits < 4` the slack widens to `2^4` (e.g.
`bits = 1`: preprocess demands operands in `{0, 1}`, the circuit accepts
up to 15). The comparison *result* is still correct for whatever in-bound
values are supplied — the divergence is only in which operand values are
admissible.

**Why it is minor.** The output bit is boolean and correct for the admitted
values, so no wrong comparison can be proven; the slack only widens the
witness set beyond what preprocess accepts, in a bounded, predictable way.

**Suggested action.** Document the evened bound in the `LessThan` doc
comment (producers choosing odd or tiny `bits` should know the effective
in-circuit bound), or round the bound up on the preprocess side to match.

## 5. Secp256k1 decode: non-canonical limb encodings accepted off-circuit

Unlike findings 1–4, the lax side here is the *off-circuit* decoder: it
accepts prover-supplied data the canonical encoder never emits. The
functions are `midnight-circuits 7.2.2` (the version resolved by
`zkir-v3` at the pin), reached from `decode_offcircuit` (`encode.rs`) on
preimage inputs and transcript reads (`ir_vm.rs:201, 360-363, 379-383`).

**What decode accepts.** `AssignedField::from_public_input`
(`field_chip.rs:141-160`) reconstructs an integer from the two native
limbs and maps it into the foreign field via `bigint_to_fe`
(`utils/util.rs:91-100`), which **silently reduces modulo the foreign
modulus** — there is no canonicity check, so every integer in
`[0, 2²⁵⁶)` is accepted (~2³² non-canonical encodings per `Fp` element,
~1.27·2¹²⁸ per `Fq` element). `AssignedForeignPoint::from_public_input`
(`weierstrass_chip.rs:249-268`) short-circuits on identity flag `= 1`
**before any coordinate or length check** (arbitrary coordinate limbs
accepted; the canonical identity encoding is the limbs of `p−1` twice
plus flag `1`, so even zeroed coordinates are non-canonical), and never
inspects flag values other than `1` (flag `42` decodes like flag `0`).
Separately, limb values ≥ `2¹⁹²` make `bi_to_limbs` (`util.rs:113-118`)
**panic** rather than return an error — prover-supplied data crashing
`preprocess`, the same defect class as the `Bytes32` decode
`assert_eq!` (`encode.rs:126-131`).

**What the circuit does.** The in-circuit side re-encodes the *decoded*
(hence canonical-valued) inputs — for the communications commitment, the
Poseidon preimage is built from the re-encoded memory values, while
`preprocess` commits the **raw** input stream (`ir_vm.rs:662-681`).

**The gap.** For a preimage containing a non-canonical Secp256k1
encoding, `preprocess` succeeds (decoding reduces silently, TC2 checks
the commitment over the raw stream) but the synthesized circuit pins the
commitment to the canonical re-encoding — the two disagree, so proof
generation fails. This is a *completeness* gap (an accepted preimage
that cannot prove), not a soundness one, and it is reachable only
through the declared `inputs` with the communications commitment
enabled.

**Ledger-level scope (traced through the ledger crates at the pin).**
The two transcript vectors are prover-local: `ProofPreimage`s are
discarded after proving (`structure.rs:546-554`, `prove.rs:253+`), and
validators rebuild every pis cell from the *typed* on-chain `Transcript`
(`verify.rs:1946-1961`; `Popeq` results re-encoded canonically via
`field_repr`, `ops.rs:476-479`). The raw `public_transcript_outputs`
vector is never hashed, committed, or serialized into the proven
transaction, so a non-canonical encoding there is protocol-invisible —
it decodes to the same typed values, the same witnessed cells, the same
pis, the same statement. The panic region is the exception: any service
that runs `preprocess` on third-party preimages (`ProvingProvider::
check/prove`, `transient-crypto/proofs.rs:683-697`; `zkir-v3-wasm`) can
be crashed by an oversized limb — an availability concern for proof
servers, though not for validators, who only encode typed values and
never decode raw Fr.

**Model status.** Our mechanization's decoders are the canonical partial
inverses (they reject what the encoder cannot emit), so its theorems
transfer to the deployed decoder exactly on canonically-encoded data;
the round-trip laws on canonical encodings, off-curve rejection, and the
coordinate conversions were verified faithful.

**Suggested action.** In `from_public_input`: reject reconstructed
integers ≥ the foreign modulus, validate the identity flag as boolean
(and check the coordinate limbs on the identity path or require the
canonical identity encoding), and return an error instead of panicking
on oversized limbs. Each is a cheap host-side check in a function that
already returns `Option`.

## Also checked, found enforced (no action needed)

For completeness, the remaining candidate divergences from the audit are
all enforced by the real circuit:

- **Declared-input typing** — every declared circuit input is allocated
  through `assign_incircuit` (`ir_vm.rs:729-730`;
  `ir_instructions/assign.rs:36-106`), whose type-specific chips constrain
  well-formedness (on-curve/subgroup for JubjubPoint, per-byte range for
  Bytes32, canonical scalars).
- **FromCoordinates subgroup membership (Jubjub)** — enforced in-circuit:
  `jubjub().point_from_coordinates` (`midnight-circuits 7.2.2`,
  `src/ecc/native/edwards_chip.rs:783`) assigns a fresh point under the
  curve-equation gate, multiplies it by the cofactor **in-circuit**
  (`assign` → `clear_cofactor`, annihilating any 8-torsion), and constrains
  the input coordinates equal to the result — so on-curve, non-subgroup
  coordinates are unsatisfiable, matching the off-circuit `into_subgroup`
  rejection (`from_coordinates.rs:40-41`). The chip's
  `point_from_coordinates_unsafe` / `assign_without_subgroup_check` paths
  skip this check, but zkir-v3 does not call them. The guarantee is not
  stated in the chip's doc comment — worth an upstream documentation
  request. (Verified 2026-07-24 against the crates.io sources.)
- **Impact guard booleanity, including empty input lists** — the guard is
  converted to an `AssignedBit` before the inputs loop (`ir_vm.rs:871-875`),
  so booleanity is constrained even when there is nothing to push.
- **DivModPowerOfTwo output arity** — wrong arity is rejected on both
  sides: `Error::Synthesis` in-circuit (`ir_vm.rs:1006-1010`), `bail!`
  off-circuit (`ir_vm.rs:396-398`).
- **FromBytes32 unsupported result types** — rejected on both sides
  (`ir_instructions/from_bytes32.rs:101-103`, `from_bytes32.rs:58-60`).
- **FromBytes32 result-type ↔ `val_t` coupling** — in-circuit the chip is
  selected by the instruction's `val_t` (`from_bytes32.rs:79-105`), so the
  output cell's type is pinned by the source. (Our model's synthesized
  constraint is looser — it does not record `val_t` — and compensates with
  a witness-shape side condition tying the output cell's type to `val_t`;
  the audit confirms the real circuit enforces this by construction.)
- **FromBytes32 non-canonical bytes on the foreign fields** — off-circuit
  reduces via `from_le_bytes_with_reduction` (`from_bytes32.rs:109-114`);
  in-circuit `assigned_from_le_bytes` (upstream `midnight-zk`,
  `circuits/src/field/foreign/field_chip.rs:1264-1290`) computes the byte
  integer by a linear combination *in the emulated field* and normalizes,
  i.e. also reduces mod the order. The two sides agree on all inputs,
  canonical or not. (The `Native`-target analogue remains unverified —
  see the spec §9.)
- **IntoCoordinates on the Secp256k1 identity** — rejected on both sides:
  off-circuit errors on `is_identity`
  (`ir_instructions/into_coordinates.rs:39-61`); in-circuit
  `assert_non_zero` makes the constraint unsatisfiable there
  (`into_coordinates.rs:75-102`).
- **Output (terminator) arity and per-position types** — checked in-circuit
  (`ir_vm.rs:1151-1167`) and off-circuit (`ir_vm.rs:626-647`).

One observation that may still interest the team: there is no separate
validation/well-formedness pass over an `IrSource` — malformed instructions
(wrong arity, unsupported types) surface as errors only when `preprocess`
or `circuit` runs. That is sound (no proof is produced), but an up-front
validation pass would fail earlier and more uniformly.
