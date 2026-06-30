# ZKIR Instruction Set Reference

ZKIR ("zero-knowledge intermediate representation") is the low-level,
serialisable representation of a Midnight circuit.  The Compact compiler lowers
each exported circuit to a ZKIR `IrSource`; the proving stack then turns that
`IrSource` into a concrete PLONK circuit, generates prover / verifier keys, and
produces proofs.

ZKIR sits between Compact and the proof system:

```
Compact circuit  ──compile──▶  ZKIR (IrSource)  ──synthesise──▶  PLONK circuit  ──prove──▶  Proof
```

The current stable IR is **v2** (`zkir` crate — `midnight-zkir` 2.1.0); the
upcoming **v3** is in release-candidate form (`zkir-v3` crate —
`midnight-zkir-v3` 3.0.0-rc.1).  This document primarily specifies v2 (what
the current Compact toolchain emits) and summarises the v3 changes in §7.

> A ZKIR program is **not** a stack machine and **not** a register machine in
> the usual sense.  It is a flat, append-only list of instructions over a
> growing **memory tape** of native field elements.  Each instruction reads
> its operands from existing memory positions and appends ("pushes") its
> output(s) to the end of the tape.  Operands are therefore plain indices
> into that tape — closer to SSA than to mutable registers: a memory slot is
> written once and never reassigned.

The same `IrSource` drives two passes:

1. **Off-circuit preprocessing** (`IrSource::preprocess`, used by both `prove`
   and `check`) — concretely evaluates the tape against a `ProofPreimage` to
   populate witness values and to derive the public inputs.  This is where
   most "this is undefined behaviour / this errors" rules are actually
   enforced at runtime.
2. **In-circuit synthesis** (`IrSource::circuit`) — emits the PLONK constraints
   that enforce each instruction, using the `midnight-zk` standard library
   (`ZkStdLib`).

## 1. The execution / circuit model

### 1.1 `IrSource` (v2)

A v2 circuit is described by this structure (`zkir/src/ir.rs`):

| Field | Type | Meaning |
|---|---|---|
| `version` | `IrMinorVersion` | Minor IR version: `V0` or `V1` (default `V1`). Affects a few instructions' bit-bound handling — see [§1.6](#16-minor-versions-v0-vs-v1). |
| `num_inputs` | `u32` | Number of initial memory elements. Memory indices `0 .. num_inputs` are the circuit inputs, taken from `ProofPreimage::inputs`. |
| `do_communications_commitment` | `bool` | Whether the circuit binds a commitment over its inputs and outputs ([§1.5](#15-communications-commitment)). |
| `instructions` | `Arc<Vec<Instruction>>` | The instruction list, executed in order. |

### 1.2 The memory tape

* Memory is a `Vec` of **native field elements** (`Fr`).
* It starts as a copy of `ProofPreimage::inputs` (length `num_inputs`).
* Instructions execute in order. Each instruction reads operands by index and
  appends its output(s) to the end of memory. Outputs therefore occupy the
  next consecutive indices.
* An operand of type **`Index`** is just a `u32` position into this tape.
  Reading an out-of-range index is a runtime error (`index out of bounds`).

Because outputs are appended, an instruction can only reference values produced
**before** it. The number of values each instruction appends is fixed and
listed per-instruction below.

### 1.3 The field and the embedded curve

* **Native field (`Fr`)** — the **BLS12-381 scalar field** (`outer::Scalar` in
  `transient-crypto`). Every memory element is a native field element.
* **Embedded curve** — **Jubjub** (`EmbeddedGroupAffine`; the Jubjub base field
  is the BLS12-381 scalar field, which is why curve coordinates fit directly
  in native memory elements). In v2, a curve point is represented as **two**
  memory elements: its affine `x` and `y` coordinates.
* **Embedded scalar field** — the Jubjub scalar field (`EmbeddedFr`), used as
  the scalar argument to `EcMul` / `EcMulGenerator`.

### 1.4 Public inputs, transcripts, and guards

A ZKIR proof is verified against a list of **public inputs (PIs)**. The PI
vector is assembled during preprocessing in this order:

1. `preimage.binding_input` (always first).
2. The communications-commitment value, **iff**
   `do_communications_commitment`.
3. One entry per `DeclarePubInput` instruction, in execution order.

The `DeclarePubInput` entries are how the circuit binds itself to a specific sequence of **Impact VM operations**: each operation's arguments are pinned into the PI vector, and the proof attests that the circuit produced exactly that sequence. 

  > Intuitively, the public inputs *are* the Impact VM operations the proof
  > commits to: a valid proof attests that some ZKIR execution produced
  > exactly that sequence, and the Impact VM then executes those operations
  > on chain. The proof therefore gates *which* instructions reach the VM -
  > they are run only if the proof verifies.
  
ZKIR also consumes three **transcript** streams from the `ProofPreimage`, each
with its own cursor that **must be fully consumed** by the end of the program
(otherwise preprocessing fails with `Transcripts not fully consumed`):

| Stream | Consumed by | Notes |
|---|---|---|
| `public_transcript_outputs` | `PublicInput` | The next public output value witnessed into memory. |
| `private_transcript` | `PrivateInput` | The next private (witness-only) value into memory. |
| `public_transcript_inputs` | `DeclarePubInput` / `PiSkip` | The declared public inputs; `PiSkip` reconciles them against computed PIs. |

**Guards.** `PublicInput`, `PrivateInput`, and `PiSkip` take an optional
`guard` index (a boolean memory value). When a guard is present and `false`,
the instruction takes its "inactive" path: `PublicInput` / `PrivateInput` push
`0` without advancing the transcript cursor, and `PiSkip` records a skip.
This is how conditional (guarded) Impact operations are encoded.

### 1.5 Communications commitment

When `do_communications_commitment` is set, the circuit binds a
Pedersen-style `transientCommit` over the concatenation `inputs ‖ outputs`
(where `outputs` are the values collected by `Output` instructions).
Preprocessing recomputes this commitment from the preimage's
`communications_commitment` `(value, randomness)` and fails with
`Communications commitment mismatch` if it disagrees. The commitment value is
added as a public input (position 2 above).

### 1.6 Minor versions (V0 vs V1)

`IrMinorVersion` is `V0` or `V1` (default `V1`). The two differ only in how a
couple of instructions enforce bit bounds in-circuit. For example, `V0`
`CondSelect` derives the selector via `is_zero` (negating the bit to force the
bound), and `V0` `ConstrainBits` lowers slightly differently from `V1`.
Functional results are equivalent; `V1` is the optimised lowering. New
circuits use `V1`.

## 2. Operand and value conventions

| Convention | Meaning |
|---|---|
| `Index` | A `u32` memory position. The value stored there is a native field element. |
| boolean operand | An `Index` whose value **must** be `0` or `1`. Reading a non-boolean where a boolean is required is a runtime error (`Expected boolean`); in-circuit, callers are responsible for the `0/1` precondition (see "UB" notes). |
| point operand | A **pair** of indices `(x, y)`. The pair must be a valid Jubjub point or preprocessing errors (`point not on curve`). |
| scalar operand | An `Index` interpreted as an embedded-curve (Jubjub) scalar. |
| `bits` | A `u32` immediate (compile-time constant), not a memory index. |
| `imm` | A native field constant embedded in the instruction (`LoadImm`). |
| `alignment` | A field-aligned-binary `Alignment` describing how inputs pack into bytes (`PersistentHash`). |

**Serialised form.**  Instructions serialise (serde) as a tagged object with
key `"op"` and snake-cased variant name, e.g. `Add { a, b }` →
`{"op": "add", "a": 3, "b": 7}`.  Examples below use this form.

## 3. Instruction reference (IR v2)

There are **26** v2 instructions.  Each entry lists operands, the number of
values pushed to memory ("outputs"), semantics, and preconditions / undefined
behaviour (UB).  "UB" means the instruction does not itself constrain the
precondition — the caller must guarantee it, or the proof / preprocessing may
be unsound or error.

### 3.1 Field arithmetic

#### `Add { a, b }`
* **Operands:** `a: Index`, `b: Index`. **Outputs:** 1 — `a + b`.
* **Semantics:** addition in the native prime field.

#### `Mul { a, b }`
* **Operands:** `a: Index`, `b: Index`. **Outputs:** 1 — `a * b`.
* **Semantics:** multiplication in the native prime field.

#### `Neg { a }`
* **Operands:** `a: Index`. **Outputs:** 1 — `-a`.
* **Semantics:** additive negation in the native prime field.

### 3.2 Boolean and selection

#### `Not { a }`
* **Operands:** `a: Index` (boolean). **Outputs:** 1 — `!a`.
* **Precondition:** `a ∈ {0,1}` (runtime-checked as boolean during preprocessing).

#### `TestEq { a, b }`
* **Operands:** `a: Index`, `b: Index`. **Outputs:** 1 — boolean `a == b`.
* **Semantics:** returns `1` if equal, else `0`. (Compare with `ConstrainEq`,
  which *asserts* equality rather than computing a boolean.)

#### `CondSelect { bit, a, b }`
* **Operands:** `bit: Index` (boolean), `a: Index`, `b: Index`. **Outputs:** 1.
* **Semantics:** pushes `a` if `bit == 1`, else `b`.
* **UB:** `bit` must be `0` or `1`. (Lowering differs between `V0` / `V1`;
  see §1.6.)

### 3.3 Constraints and assertions (push **no** outputs)

#### `Assert { cond }`
* **Operands:** `cond: Index` (boolean). **Outputs:** none.
* **Semantics:** requires `cond == 1`. Fails preprocessing with
  `Failed direct assertion` otherwise; in-circuit, asserts non-zero.
* **UB:** `cond` must be `0` or `1`.

#### `ConstrainEq { a, b }`
* **Operands:** `a: Index`, `b: Index`. **Outputs:** none.
* **Semantics:** constrains `a == b`. Fails with `Failed equality constraint`
  otherwise.

#### `ConstrainToBoolean { var }`
* **Operands:** `var: Index`. **Outputs:** none.
* **Semantics:** constrains `var ∈ {0,1}`.

#### `ConstrainBits { var, bits }`
* **Operands:** `var: Index`, `bits: u32`. **Outputs:** none.
* **Semantics:** constrains `var` to fit in `bits` bits (i.e. `var < 2^bits`).
* **Precondition:** `bits < FR_BITS` (255). A larger bound errors with
  `Excessive bit bound`.

### 3.4 Bit decomposition and reconstruction

#### `DivModPowerOfTwo { var, bits }`
* **Operands:** `var: Index`, `bits: u32`. **Outputs:** 2 — pushed in this
  order:
  1. `var >> bits` (the quotient / high bits)
  2. `var & ((1 << bits) - 1)` (the remainder / low `bits` bits)
* **Precondition:** `bits ≤ FR_BYTES_STORED * 8` (248). Larger errors with
  `Excessive bit count`.

#### `ReconstituteField { divisor, modulus, bits }`
* **Operands:** `divisor: Index`, `modulus: Index`, `bits: u32`. **Outputs:**
  1 — `(divisor << bits) | modulus`.
* **Semantics:** inverse of `DivModPowerOfTwo`. Guarantees `modulus < 2^bits`
  and that the reconstituted value does not overflow the field (errors:
  `Reconstituted element overflows field`, or `Excessive bit count` if
  `bits > 248`).
* **Example (round-trip):**

  ```jsonc
  // split memory[5] into high/low at 8 bits, then rebuild it
  {"op": "div_mod_power_of_two", "var": 5, "bits": 8}      // pushes hi@N, lo@N+1
  {"op": "reconstitute_field", "divisor": N, "modulus": N+1, "bits": 8}  // == memory[5]
  ```

#### `LessThan { a, b, bits }`
* **Operands:** `a: Index`, `b: Index`, `bits: u32`. **Outputs:** 1 — boolean
  `a < b`.
* **Semantics:** compares `a` and `b` as `bits`-bit unsigned integers.
* **UB:** `a` and `b` must each fit in `bits` bits; otherwise the comparison
  is meaningless (inputs are bit-constrained to `bits` during evaluation).

### 3.5 Constants and copies

#### `LoadImm { imm }`
* **Operands:** `imm: Fr` (an embedded constant, serialised as a hex field
  value). **Outputs:** 1 — `imm`.
* **Semantics:** loads a compile-time constant into memory.

#### `Copy { var }`
* **Operands:** `var: Index`. **Outputs:** 1 — a duplicate of `var`.
* **Semantics:** superfluous to the underlying circuit (does not add real
  constraints) but useful for IR-level bookkeeping / aliasing.

### 3.6 Hashing

#### `TransientHash { inputs }`
* **Operands:** `inputs: Vec<Index>`. **Outputs:** 1 — `H(inputs)`.
* **Semantics:** **Poseidon** hash (over the native scalar field) of the
  listed memory elements. Circuit-friendly and cheap in-circuit; use for
  transient / ephemeral hashing.

#### `PersistentHash { alignment, inputs }`
* **Operands:** `alignment: Alignment`, `inputs: Vec<Index>`. **Outputs:** 2 —
  the hash digest in the binary (field-aligned) format.
* **Semantics:** **SHA-256** hash. Inputs are first parsed according to
  `alignment` into a field-aligned-binary (FAB) byte representation
  ([field-aligned-binary.md](./field-aligned-binary.md)), then SHA-256 is
  taken over those bytes; the 32-byte digest is returned as 2 field elements.
  Errors with `Inputs did not match alignment` if `inputs` don't conform to
  `alignment`.
* **Note:** much more expensive in-circuit than `TransientHash` (SHA-256 vs
  Poseidon); use where the digest must be a standard, stable hash across
  contexts (e.g. on-chain identifiers, interop).

### 3.7 Elliptic-curve operations (Jubjub)

Curve points are `(x, y)` index pairs; each of these pushes **2** outputs
(`c_x`, `c_y`).

#### `EcAdd { a_x, a_y, b_x, b_y }`
* **Outputs:** 2 — point `a + b`.
* **UB:** `(a_x, a_y)` and `(b_x, b_y)` must each be valid Jubjub points
  (else `point not on curve`).

#### `EcMul { a_x, a_y, scalar }`
* **Operands:** point `(a_x, a_y)`, `scalar: Index`. **Outputs:** 2 —
  `scalar · a`.
* **UB:** `(a_x, a_y)` must be a valid curve point.

#### `EcMulGenerator { scalar }`
* **Operands:** `scalar: Index`. **Outputs:** 2 — `scalar · G` (the fixed
  Jubjub generator).

#### `HashToCurve { inputs }`
* **Operands:** `inputs: Vec<Index>`. **Outputs:** 2 — a Jubjub point derived
  by hashing the inputs onto the curve.

### 3.8 I/O, transcript, and public inputs

#### `PublicInput { guard }`
* **Operands:** `guard: Option<Index>`. **Outputs:** 1.
* **Semantics:** witnesses the next value from the **public transcript
  outputs** into memory. If `guard` is present and `false`, pushes `0` and
  does **not** advance the transcript cursor.

#### `PrivateInput { guard }`
* **Operands:** `guard: Option<Index>`. **Outputs:** 1.
* **Semantics:** as `PublicInput`, but pulls from the **private transcript**
  (witness-only) stream.

#### `DeclarePubInput { var }`
* **Operands:** `var: Index`. **Outputs:** none.
* **Semantics:** appends `memory[var]` to the public-input vector and
  advances the public-transcript-inputs cursor. Should be **followed** by a
  `PiSkip` covering it (see below).

#### `PiSkip { guard, count }`
* **Operands:** `guard: Option<Index>`, `count: u32`. **Outputs:** none.
* **Semantics:** a marker that groups the `count` immediately-preceding
  declared public inputs into one logical unit (typically one Impact
  operation) and records whether they are active. If `guard` is present and
  `false`, the group is skipped (recorded in `pi_skips`, and the
  public-transcript-inputs cursor is rewound by `count`); otherwise the
  declared PIs are reconciled against `public_transcript_inputs` (mismatch
  errors with `Public transcript input mismatch`).
* **Pairing rule:** every `DeclarePubInput` should be covered by a following
  `PiSkip`.  This pair is how a guarded set of public inputs (an Impact
  write) is expressed.

#### `Output { var }`
* **Operands:** `var: Index`. **Outputs:** none *at the IR-VM level* (despite
  the name — it does not push to memory).
* **Semantics:** records `memory[var]` as a circuit output. Outputs are
  included in the communications commitment ([§1.5](#15-communications-commitment)).

## 4. Worked example: assert `a · b = c` over private inputs

A minimal circuit that takes two private witnesses, multiplies them, and
constrains the product to equal a third private value:

```jsonc
// IrSource { version: V1, num_inputs: 0, do_communications_commitment: false,
//            instructions: [ ... ] }
[
  {"op": "private_input", "guard": null},   // memory[0] = a   (witness)
  {"op": "private_input", "guard": null},   // memory[1] = b   (witness)
  {"op": "private_input", "guard": null},   // memory[2] = c   (witness)
  {"op": "mul", "a": 0, "b": 1},            // memory[3] = a * b
  {"op": "constrain_eq", "a": 3, "b": 2}    // require a*b == c   (no output)
]
```

Note how operands are positional memory indices, outputs append to the end,
and the constraint instruction pushes nothing. The three `private_input`s
must exactly match the length of `ProofPreimage::private_transcript`, or
preprocessing fails with `Transcripts not fully consumed`.

## 5. Failure modes (runtime, during preprocessing)

These are the explicit error conditions enforced when evaluating an
`IrSource` against a preimage — useful when debugging a circuit that proves
locally but fails to preprocess:

| Error | Cause |
|---|---|
| `index out of bounds: {i}` | An operand referenced a memory slot that doesn't exist yet. |
| `Expected boolean, found: …` | A boolean operand held a value other than `0` / `1`. |
| `Elliptic curve point not on curve` | A point operand `(x,y)` is not a valid Jubjub point. |
| `Failed direct assertion` | `Assert` saw `cond != 1`. |
| `Failed equality constraint` | `ConstrainEq` operands differed. |
| `Bit bound failed: … is not n-bit` | A value exceeded its declared bit width. |
| `Excessive bit bound` / `Excessive bit count` | `bits ≥ 255` (`ConstrainBits`) or `bits > 248` (`DivModPowerOfTwo` / `ReconstituteField`). |
| `Reconstituted element overflows field` | `ReconstituteField` result ≥ field modulus. |
| `Inputs did not match alignment` | `PersistentHash` inputs didn't conform to `alignment`. |
| `Ran out of public/private transcript outputs` | A `PublicInput` / `PrivateInput` had no value left to consume. |
| `Transcripts not fully consumed` | Some transcript stream had leftover values at the end. |
| `Public transcript input mismatch` | `PiSkip` reconciliation: declared PI ≠ transcript value. |
| `Communications commitment mismatch` | Recomputed `transientCommit(inputs ‖ outputs)` ≠ preimage commitment. |

## 6. The `zkir` tool

The `zkir` crate ships a CLI binary ("ZKIR v2 compiler", `zkir/src/main.rs`)
that operates on serialised `IrSource` files (`.zkir`, and `.bzkir` for the
binary form sent to the proof server). Subcommands:

| Subcommand | Purpose |
|---|---|
| `mock-compile` | Compile a single IR source without generating real keys (fast, for validation). |
| `compile` | Generate prover **and** verifier keys for a single IR source. |
| `compile-many` / `mock-compile-many` | The directory-batch variants. |

`zkir-wasm` exposes the v2 IR proving / checking surface to JS / TS (published
as `@midnight-ntwrk/zkir-v2`); `zkir-v3-wasm` does the same for v3.

## 7. IR v3 (`3.0.0-rc.1`) — what's changing

> **Status:** release candidate, under active development. The v3 VM and
> constraint details below are summarised from the `zkir-v3` instruction
> definitions and types (`zkir-v3/src/ir.rs`, `ir_types.rs`); treat exact
> semantics as subject to change until v3 ships.

v3 is a substantial restructuring, not just new opcodes. The headline changes:

**1. Typed, named operands (SSA by name).** v2's positional `Index` (`u32`)
memory tape is replaced by named variables and inline immediates. An
`Operand` is now either:

* `Variable(Identifier)` — serialised as a `%`-prefixed name (e.g. `%t3`), or
* `Immediate(Fr)` — serialised as a `0x`-prefixed hex field value.

Each producing instruction names its result(s) via an `output: Identifier`
(or `outputs: Vec<Identifier>`) field, instead of implicitly appending to a
tape.

**2. A type system (`IrType`).** Values are typed:

| `IrType` | Serialised name | Raw `Fr` elements |
|---|---|---|
| `Native` | `Scalar<BLS12-381>` | 1 |
| `JubjubPoint` | `Point<Jubjub>` | 2 (x, y) |
| `JubjubScalar` | `Scalar<Jubjub>` | 1 |

Points and embedded scalars become **first-class typed values** rather than
raw coordinate pairs. Consequently several instructions become
type-polymorphic: `Add`, `TestEq`, `ConstrainEq`, and `CondSelect` are
documented as supporting both `Native` and `JubjubPoint`.

**3. Typed circuit signature.** `IrSource` changes from
`num_inputs: u32` to `inputs: Vec<TypedIdentifier>` and gains
`outputs: Vec<IrType>` — an explicit, positional return signature that the
new `Output` terminator is type-checked against. `IrMinorVersion` for v3
currently has only `V0`.

**4. Instruction set delta.** v3 also has 26 instructions, but the membership
changed:

* **Added:**
  * `Encode` / `Decode` — convert between a typed value and its raw
    `Fr`-element representation (replacing implicit coordinate handling; e.g.
    `JubjubPoint` ↔ 2 `Native`s).
  * `Impact` — a single guarded "declare these public inputs" instruction
    that subsumes v2's `DeclarePubInput` + `PiSkip` pairing.
  * `Keccak256` — Keccak-256 hashing with an `Alignment` (alongside
    `PersistentHash`).
* **Removed / folded in:**
  * `EcAdd` — point addition is now expressed via `Add` on `JubjubPoint`
    operands.
  * `LoadImm` — constants are now inline `Operand::Immediate` values.
  * `DeclarePubInput` + `PiSkip` — replaced by `Impact`.
* **Changed shape:**
  * `EcMul` / `EcMulGenerator` / `HashToCurve` now output **1** value (a typed
    `JubjubPoint`) instead of 2 raw coordinates.
  * `Output` becomes a **circuit terminator**: it takes one operand per
    `IrSource::outputs` entry, type-checks them against the signature,
    encodes each, and ends execution.
  * `PublicInput` / `PrivateInput` now carry the result type `val_t`.

**Illustrative v3 form** of the §4 example (named, typed):

```jsonc
{"op": "private_input",  "type": "Scalar<BLS12-381>", "output": "%a"}
{"op": "private_input",  "type": "Scalar<BLS12-381>", "output": "%b"}
{"op": "private_input",  "type": "Scalar<BLS12-381>", "output": "%c"}
{"op": "mul",            "a": "%a", "b": "%b", "output": "%p"}
{"op": "constrain_eq",   "a": "%p", "b": "%c"}
{"op": "output",         "vals": []}
```

## Appendix A — v2 instruction index

| Instruction | Operands | Outputs pushed |
|---|---|---|
| `Add` | `a, b` | 1 |
| `Mul` | `a, b` | 1 |
| `Neg` | `a` | 1 |
| `Not` | `a` (bool) | 1 |
| `TestEq` | `a, b` | 1 (bool) |
| `CondSelect` | `bit` (bool), `a, b` | 1 |
| `Assert` | `cond` (bool) | 0 |
| `ConstrainEq` | `a, b` | 0 |
| `ConstrainToBoolean` | `var` | 0 |
| `ConstrainBits` | `var, bits` | 0 |
| `DivModPowerOfTwo` | `var, bits` | 2 |
| `ReconstituteField` | `divisor, modulus, bits` | 1 |
| `LessThan` | `a, b, bits` | 1 (bool) |
| `LoadImm` | `imm` | 1 |
| `Copy` | `var` | 1 |
| `TransientHash` | `inputs[]` | 1 |
| `PersistentHash` | `alignment, inputs[]` | 2 |
| `EcAdd` | `a_x, a_y, b_x, b_y` | 2 |
| `EcMul` | `a_x, a_y, scalar` | 2 |
| `EcMulGenerator` | `scalar` | 2 |
| `HashToCurve` | `inputs[]` | 2 |
| `PublicInput` | `guard?` | 1 |
| `PrivateInput` | `guard?` | 1 |
| `DeclarePubInput` | `var` | 0 |
| `PiSkip` | `guard?, count` | 0 |
| `Output` | `var` | 0 (records output) |
