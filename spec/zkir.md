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

This document specifies **IR v3** — the IR defined by the `zkir-v3` crate
(`midnight-zkir-v3` 3.0.0-rc.2), which is the version integrated into Ledger 9
(consumed by `proof-server` and published to JS / TS as
`@midnight-ntwrk/zkir-v3`).  The previous stable IR, **v2** (`zkir` crate —
`midnight-zkir` 2.2.0), is retained for reference in
[Appendix B](#appendix-b--ir-v2-legacy); [§8](#8-migrating-from-v2) summarises
the v2 → v3 deltas.

> A ZKIR program is **not** a stack machine and **not** a register machine in
> the usual sense.  It is a flat list of instructions over a **named memory**
> of typed values.  Each producing instruction reads its operands and binds
> its output(s) to fresh, explicitly-named variables; a variable is written
> once and never reassigned (SSA by name).  Operands are therefore either a
> variable reference (`%name`) or an inline immediate (`0x…`) — not positional
> tape indices as in v2.

The same `IrSource` drives two passes:

1. **Off-circuit preprocessing** (`IrSource::preprocess`, used by both `prove`
   and `check`) — concretely evaluates the program against a `ProofPreimage` to
   populate witness values and to derive the public inputs.  This is where
   most "this is undefined behaviour / this errors" rules are actually
   enforced at runtime.
2. **In-circuit synthesis** (`IrSource::circuit`) — emits the PLONK constraints
   that enforce each instruction, using the `midnight-zk` standard library
   (`ZkStdLib`).

## 1. The execution / circuit model

### 1.1 `IrSource` (v3)

A v3 circuit is described by the following structure:
* `version`: Major and minor IR version.
* `inputs`: The circuit's input variables, each a `(name, type)` pair. These are bound in memory before execution, decoded from `ProofPreimage::inputs`.
* `outputs`: The circuit's return signature - an explicit, positional list of result types. The `Output` terminator is type-checked against this.
* `do_communications_commitment`: Whether the circuit binds a commitment over its inputs and outputs ([§1.5](#15-communications-commitment)).
* `instructions`: The instruction list, executed in order.

Note the change from v2's single `num_inputs: u32` to a fully-typed signature
(`inputs` + `outputs`).

### 1.2 Named memory

* Memory is a map from **identifier** to **typed value** (`HashMap<Identifier, IrValue>`).
* It starts populated with the `inputs`, decoded from `ProofPreimage::inputs`.
  Values are encoded as native field elements, with IrType::encoded_len()
  specifying the number of elements required for each type (see §2.1); a 
  shortfall errors with `Not enough raw inputs`,
  and a leftover with `Expected N raw inputs, received M`.
* Instructions execute in order. Each producing instruction reads its operands
  and binds its result(s) to the variable name(s) it declares (`output` /
  `outputs`). Referencing a name that has not been bound yet is a runtime error
  (`variable not found`).
* Because a name is bound once, an instruction can only reference values
  produced **before** it. The number of values each instruction produces is
  fixed and listed per-instruction below.

### 1.4 Public inputs, transcripts, and guards

A ZKIR proof is verified against a list of **public inputs (PIs)**. The PI
vector is assembled during preprocessing in this order
(`zkir-v3/src/ir_vm.rs`):

1. `preimage.binding_input` (always first).
2. The communications-commitment value, **iff**
   `do_communications_commitment`.
3. One entry per value declared by each `Impact` instruction, in execution
   order.

The `Impact` entries are how the circuit binds itself to a specific sequence of
**Impact VM operations**: each operation's arguments are pinned into the PI
vector, and the proof attests that the circuit produced exactly that sequence.
`Impact` also produces a parallel **`pi_skips`** vector (one entry per `Impact`)
recording, for each instruction, whether it was guarded off (`Some(n)`, meaning
`n` zeros were declared) or active (`None`); `check` / `prove` return this so
the on-chain VM can tell which operations to skip.

> Intuitively, the public inputs *are* the Impact VM operations the proof
> commits to: a valid proof attests that some ZKIR execution produced
> exactly that sequence, and the Impact VM then executes those operations
> on chain. The proof therefore gates *which* instructions reach the VM —
> they are run only if the proof verifies.

ZKIR also consumes three **transcript** streams from the `ProofPreimage`, each
with its own cursor that **must be fully consumed** by the end of the program
(otherwise preprocessing fails with `Transcripts not fully consumed`):

| Stream | Consumed by | Notes |
|---|---|---|
| `public_transcript_outputs` | `PublicInput` | The next public output value witnessed into memory. |
| `private_transcript` | `PrivateInput` | The next private (witness-only) value into memory. |
| `public_transcript_inputs` | `Impact` | The declared public inputs; `Impact` reconciles them against this stream. |

`PublicInput` / `PrivateInput` consume `val_t.encoded_len()` raw elements per
(active) read and decode them into a typed value.

### 1.5 Communications commitment

When `do_communications_commitment` is set, the circuit binds a Poseidon
commitment over `randomness ‖ inputs ‖ outputs` (where `inputs` are the
circuit's typed inputs and `outputs` are the values produced by the `Output`
terminator, each encoded to raw field elements). Preprocessing recomputes this
commitment from the preimage's `communications_commitment` `(value, randomness)`
using `transient_commit` and fails with `Communications commitment mismatch` if
it disagrees. The commitment value is added as a public input (position 2
above).

### 1.6 Minor versions

`IrMinorVersion` is `#[non_exhaustive]` and currently defines only `V0` (the
default), tag `ir-minor-version[v3]`. v3 starts fresh at `V0`; it does not
carry over v2's minor-version history (which has since grown to `V0`/`V1`/`V2`).
The top-level loader (`IrSource::load`) accepts only major version `3`, minor
`0`.

## 2. Operand and value conventions

### 2.1 Types (`IrType`)

Values are typed. `IrType` (`zkir-v3/src/ir_types.rs`, tag `ir-type[v1]`) has
**seven** variants. Each type is represented by a number of native field elements.
* `Scalar<BLS12-381>` — the BLS12-381 scalar field. This is the native type 
   for midnight-zk proofs and, therefore, it is represented as a native field element.
* `Point<Jubjub>` — A point of the embedded curve Jubjub, whose base field 
  is the BLS12-381 scalar field. Its raw encoding is two native elements (x, y), 
  representing its coordinates in affine form.
* `Scalar<Jubjub>` — Jubjub scalar field, used as the scalar argumenti to 
  `EcMul` / `EcMulGenerator`. Represented as a single native element.
* `Scalar<Secp256k1>` — SECP256k1 scalar field, emulated over the native
  field. Its raw encoding is two native elements.
* `Base<Secp256k1>` — SECP256k1 base field, emulated over the native
  field. Its raw encoding is two native elements.
* `Point<Secp256k1>` — A point from the SECP256k1 curve, emulated over the 
  native field. It is defined by the tuple `(x, y, is_id)`, where `x` and `y` are its
  coordinates (represented as `Base<Secp256k1>`) and `is_id` is a flag 
  determining whether the point is the identity or not. Its raw encoding is five 
  native elements. 
* **`Bytes32`** — a 32-byte value. Its raw encoding is 32 native elements, 
  range constrained to `[0, 256)`.

### 2.2 Operands (`Operand`)

An operand (tag `zkir-operand[v1]`) is one of:

* `Variable(Identifier)` — serialised as a `%`-prefixed string (e.g. `"%t3"`).
  An identifier that is not `%`-prefixed (and is not a `0x` immediate) is
  rejected at deserialisation.
* `Immediate(Fr)` — serialised as a `0x`-prefixed (optionally `-0x` for
  negation) little-endian hex field value (e.g. `"0x01"`, `"-0x01"`). This
  replaces v2's `LoadImm` instruction — native constants are now inline. An
  immediate always resolves to a `Native` value.

| Convention | Meaning |
|---|---|
| variable | A `%`-prefixed identifier naming a value already bound in memory. |
| immediate | A `0x…` / `-0x…` inline native field constant. |
| boolean operand | An operand whose value **must** be `0` or `1`. |
| `output` / `outputs` | The identifier(s) a producing instruction binds. Bare `Identifier` strings (`%`-prefixed); these are *declarations*, not operands. |
| `bits` | A `u32` immediate (compile-time constant), not an operand. |
| `type` / `val_t` | An `IrType`, naming the type an instruction reads or produces. |
| `alignment` | A field-aligned-binary `Alignment` describing how inputs pack into bytes (`PersistentHash` / `Keccak256`). |

**Serialised form.** Instructions serialise (serde) as a tagged object with
key `"op"` and the snake-cased variant name, e.g.
`Add { a, b, output }` → `{"op": "add", "a": "%3", "b": "%7", "output": "%8"}`
(instruction tag `ir-instruction[v3]`). A `TypedIdentifier` serialises as
`{"name": "%a", "type": "Scalar<BLS12-381>"}`. The top-level `IrSource` is
authored with an explicit version object, e.g.
`{"version": {"major": 3, "minor": 0}, "inputs": […], "outputs": […], "do_communications_commitment": false, "instructions": […]}`.
Examples below use this form.

## 3. Instruction reference (IR v3)

There are **33** v3 instructions.  Each entry lists operands, the number of
values it binds ("outputs"), semantics, the value types it supports, and
preconditions / undefined behaviour (UB).  "UB" means the instruction does not
itself constrain the precondition — the caller must guarantee it, or the proof /
preprocessing may be unsound or error.

Many arithmetic/equality/selection instructions are **type-polymorphic**: they
dispatch on the runtime type of their operands and error
(`Unsupported …: {ty} …`) on an unsupported type or a type mismatch between
operands.

### 3.1 Field / curve arithmetic

#### `Add { a, b, output }`
* **Operands:** `a`, `b`. **Outputs:** 1 — `a + b`.
* **Types:** `Native`, `JubjubPoint`, `Secp256k1Point`, `Secp256k1Base`,
  `Secp256k1Scalar`. For the two point types this is **point addition** (this
  subsumes v2's separate `EcAdd`). Both operands must share the same type.

#### `Mul { a, b, output }`
* **Operands:** `a`, `b`. **Outputs:** 1 — `a * b`.
* **Types:** `Native`, `Secp256k1Base`, `Secp256k1Scalar`. Field multiplication.

#### `Neg { a, output }`
* **Operands:** `a`. **Outputs:** 1 — `-a`.
* **Types:** `Native`, `JubjubPoint`, `Secp256k1Point`, `Secp256k1Base`,
  `Secp256k1Scalar`. Additive negation (point negation for the point types).

#### `Inv { a, output }`
* **Operands:** `a`. **Outputs:** 1 — `a⁻¹`.
* **Types:** `Native`, `Secp256k1Base`, `Secp256k1Scalar`.
* **Errors:** inverting zero (`cannot invert zero of type …`); in-circuit the
  zero case is unsatisfiable.

### 3.2 Boolean and selection

#### `Not { a, output }`
* **Operands:** `a ∈ {0,1}`. **Outputs:** 1 — `!a`.
* **Errors:** if `a ∉ {0,1}`. In-circuit that case is unsatisfiable.

#### `TestEq { a, b, output }`
* **Operands:** `a`, `b`. **Outputs:** 1 — boolean `a == b` (a `Native` `0`/`1`).
* **Types:** `Native`, `JubjubPoint`, `Secp256k1Point`, `Secp256k1Base`,
  `Secp256k1Scalar`. Different from `ConstrainEq`, which *asserts* equality
  rather than computing a boolean.

#### `CondSelect { bit, a, b, output }`
* **Operands:** `bit ∈ {0,1}`, `a`, `b`. **Outputs:** 1.
* **Semantics:** binds `a` if `bit == 1`, else `b`.
* **Types:** `Native`, `JubjubPoint`, `Secp256k1Point`, `Secp256k1Base`,
  `Secp256k1Scalar`.
* **Errors:** if `bit ∉ {0,1}`. In-circuit that case is unsatisfiable.

### 3.3 Constraints and assertions (bind **no** outputs)

#### `Assert { cond }`
* **Operands:** `cond ∈ {0,1}`. **Outputs:** none.
* **Semantics:** asserts `cond == 1`.
* **Errors:** Fails preprocessing with `Failed direct assertion` if 
  `cond ∉ {0,1}`. In-circuit that case is unsatisfiable.

#### `ConstrainEq { a, b }`
* **Operands:** `a`, `b`. **Outputs:** none.
* **Semantics:** constrains `a == b`.
* **Types:** `Native`, `JubjubPoint`, `Secp256k1Point`, `Secp256k1Base`,
  `Secp256k1Scalar`.

#### `ConstrainToBoolean { val }`
* **Operands:** `val`. **Outputs:** none.
* **Semantics:** constrains `val ∈ {0,1}` (`Native`).

#### `ConstrainBits { val, bits }`
* **Operands:** `val`, `bits: u32`. **Outputs:** none.
* **Semantics:** constrains `val` to fit in `bits` bits (i.e. `val < 2^bits`).
* **Errors:** `bits >= FR_BITS`. A bound larger than FR_BITS errors 
  with `Excessive bit bound`.

### 3.4 Bit decomposition and comparison

#### `DivModPowerOfTwo { val, bits, outputs }`
* **Operands:** `val`, `bits: u32`. **Outputs:** 2 — declared in this order:
  1. `val >> bits` (the quotient / high bits)
  2. `val & ((1 << bits) - 1)` (the remainder / low `bits` bits)
* **Errors:** if `bits >= FR_BYTES_STORED * 8`, with `Excessive bit count`.

#### `ReconstituteField { divisor, modulus, bits, output }`
* **Operands:** `divisor`, `modulus`, `bits: u32`. **Outputs:**
  1 — `(divisor << bits) | modulus`.
* **Semantics:** inverse of `DivModPowerOfTwo`. Guarantees `modulus < 2^bits`
  and that the reconstituted value does not overflow the field.
  * **Errors:** if if `bits > 248`, fails with `Reconstituted element overflows field`, 
    or `Excessive bit count`.

#### `LessThan { a, b, bits, output }`
* **Operands:** `a`, `b`, `bits: u32`. **Outputs:** 1 — boolean `a < b` (`Native`).
* **Semantics:** compares `a` and `b` as `bits`-bit unsigned integers.
* **Errors:** if `a` or `b` do not fit in `bits` bits. In-circuit that case is unsatisfiable.

### 3.5 Copies

#### `Copy { val, output }`
* **Operands:** `val`. **Outputs:** 1 — a duplicate of `val`.
* **Semantics:** superfluous to the underlying circuit (does not add real
  constraints) but useful for IR-level bookkeeping / aliasing. (Native constants
  no longer need a dedicated instruction — use an `Immediate` operand directly.)

### 3.6 Encoding and type conversions

#### `Encode { input, outputs }`
* **Operands:** `input` (any typed value). **Outputs:** one per raw element of
  the input's type — `encoded_len` from [§2.1](#21-types-irtype) (`Native` → 1,
  `Bytes32` → 2, `JubjubPoint` → 2, `JubjubScalar` → 1, `Secp256k1Point` → 5,
  `Secp256k1Base` / `Secp256k1Scalar` → 2). A wrong output count errors
  (`Unexpected output length of encode instruction`).
* **Semantics:** lowers a typed value to its raw `Native` field-element
  representation (the same encoding used for circuit inputs and the
  communications commitment).

#### `IntoCoordinates { point, outputs }`
* **Operands:** `point`. **Outputs:** 2 — the affine coordinates `(x, y)` as base
  field elements.
* **Types:** `JubjubPoint`, `Secp256k1Point`.
* **Errors:** extracting coordinates of the identity of a Weierstrass curve
  (`Secp256k1`) fails off-circuit and is unsatisfiable in-circuit.

#### `FromCoordinates { inputs, output }`
* **Operands:** `inputs` — a pair `(x, y)`. **Outputs:** 1 — the reconstructed point.
* **Types:** `(Native, Native)` → `JubjubPoint`;
  `(Secp256k1Base, Secp256k1Base)` → `Secp256k1Point`.
* **Errors:** coordinates not on the curve; the identity of a Weierstrass curve
  cannot be constructed this way.

#### `IntoBytes32 { input, output }`
* **Operands:** `input`. **Outputs:** 1 — a `Bytes32` (little-endian byte
  encoding of the underlying canonical integer).
* **Types:** `Native`, `Secp256k1Base`, `Secp256k1Scalar`.

#### `FromBytes32 { bytes, type, output }`
* **Operands:** `bytes` (`Bytes32`), `type: IrType`. **Outputs:** 1 — a value of
  `type` built from the little-endian bytes.
* **Types:** `Native`, `Secp256k1Base`, `Secp256k1Scalar`.
* **Semantics:** inverse of `IntoBytes32`; also accepts non-canonical 32-byte
  inputs by applying the relevant modular reduction.

#### `Bytes32IntoLowHigh { bytes, outputs }`
* **Operands:** `bytes` (`Bytes32`). **Outputs:** 2 — `(low, high)` as `Native`:
  `low` is the first 31 bytes (little-endian), `high` is the 32nd (most
  significant) byte.
* **Semantics:** a temporary bridge for Compact, which cannot yet handle
  `Bytes32` directly. Imposes no off-circuit errors and no in-circuit
  constraints. Inverse of `Bytes32FromLowHigh`.

#### `Bytes32FromLowHigh { inputs, output }`
* **Operands:** `inputs` — a pair `(low, high)`. **Outputs:** 1 — a `Bytes32`
  concatenating the first 31 bytes of `low` with byte `high`.
* **Precondition:** `low < 2^248` (fits in 31 bytes) and `high < 256` (one byte);
  off-circuit violation errors, in-circuit it is unsatisfiable.
* **Note:** the inverse of `Bytes32IntoLowHigh`; same temporary-bridge caveat.

#### `JubjubScalarFromNative { native, output }`
* **Operands:** `native` (`Native`). **Outputs:** 1 — a `JubjubScalar`.
* **Semantics:** casts a native value to a Jubjub scalar, reducing modulo the
  Jubjub scalar field order if necessary.
* **Note:** intended to be removed once a `BigUint` type is available.

### 3.7 Hashing

#### `TransientHash { inputs, output }`
* **Operands:** `inputs: Vec<Operand>` (all `Native`). **Outputs:** 1 — `H(inputs)`.
* **Semantics:** **Poseidon** hash (over the native scalar field) of the listed
  values. Circuit-friendly and cheap in-circuit; use for transient / ephemeral
  hashing.

#### `PersistentHash { alignment, inputs, outputs }`
* **Operands:** `alignment: Alignment`, `inputs: Vec<Operand>` (all `Native`).
  **Outputs:** 1 — `Bytes32`.
* **Semantics:** **SHA-256** hash. Inputs are first parsed according to
  `alignment` into a field-aligned-binary (FAB) byte representation
  ([field-aligned-binary.md](./field-aligned-binary.md)), then SHA-256 is taken
  over those bytes. 
  * **Errors:** If `inputs` don't conform to `alignment`, errors with 
    `Inputs did not match alignment`.
* **Note:** much more expensive in-circuit than `TransientHash` (SHA-256 vs
  Poseidon); use where the digest must be a standard, stable hash across
  contexts (e.g. on-chain identifiers, interop).

#### `Keccak256 { alignment, inputs, outputs }`
* **Operands:** `alignment: Alignment`, `inputs: Vec<Operand>` (all `Native`).
  **Outputs:** 1 — `Bytes32`.
* **Semantics:** as `PersistentHash`, but computes the **Keccak-256** digest.

### 3.8 Elliptic-curve operations

In v3 a point is a single point-typed value, not a raw coordinate pair. (Point
*addition* is `Add` on point operands; there is no separate `EcAdd`.)

#### `EcMul { a, scalar, output }`
* **Operands:** `a` (point), `scalar`. **Outputs:** 1 — `scalar · a`.
* **Types:** `(JubjubPoint, JubjubScalar)` → `JubjubPoint`;
  `(Secp256k1Point, Secp256k1Scalar)` → `Secp256k1Point`. Other combinations
  error.

#### `EcMulGenerator { scalar, output }`
* **Operands:** `scalar`. **Outputs:** 1 — `scalar · G` (the curve's fixed
  generator).
* **Types:** `JubjubScalar` → `JubjubPoint`; `Secp256k1Scalar` →
  `Secp256k1Point`. Any other scalar type errors with
  `Unsupported EcMulGenerator for scalar of type …`.

#### `HashToCurve { inputs, output }`
* **Operands:** `inputs: Vec<Operand>` (all `Native`). **Outputs:** 1 — a
  `JubjubPoint` derived by hashing the inputs onto the curve. Non-`Native`
  inputs error.

### 3.9 I/O, transcript, and public inputs

#### `PublicInput { guard, type, output }`
* **Operands:** `guard: Option<Operand>`, `type: IrType` (serialised `type`).
  **Outputs:** 1.
* **Semantics:** witnesses the next value (of type `val_t`) from the **public
  transcript outputs** into memory, consuming `val_t.encoded_len()` raw
  elements. If `guard` is present and `false`, binds the type's default and does
  **not** advance the transcript cursor. In-circuit the value is constrained
  only to respect `val_t`; the guard does not add in-circuit constraints.

#### `PrivateInput { guard, type, output }`
* **Operands:** `guard: Option<Operand>`, `type: IrType`. **Outputs:** 1.
* **Semantics:** as `PublicInput`, but pulls from the **private transcript**
  (witness-only) stream.

#### `Impact { guard, inputs }`
* **Operands:** `guard: Operand` (boolean), `inputs: Vec<Operand>`. **Outputs:**
  none.
* **Semantics:** declares its `inputs` as public inputs (appended to the PI
  vector) and records activity information (`pi_skips`). When `guard` is `false`,
  it instead declares `n` zeros (for its `n` inputs) and records a skip
  (`Some(n)`); the declared PIs are otherwise reconciled against
  `public_transcript_inputs` (mismatch errors with
  `Public transcript input mismatch`). In-circuit the guard is enforced as
  `select(guard, x, 0)` per input. This single instruction subsumes v2's
  `DeclarePubInput` + `PiSkip` pairing — one `Impact` expresses one guarded
  Impact-VM operation.
* **Precondition:** all `inputs` must be `Native`.

#### `Output { vals }`
* **Operands:** `vals: Vec<Operand>` — one per `IrSource::outputs` entry.
* **Semantics:** the **circuit terminator** (conventionally the final
  instruction). Produces the circuit's return values in signature order. The
  operand list is type-checked against `IrSource::outputs` (length and
  per-position type — mismatches error with `Output: signature declares N return
  values but instruction has M` / `Output position i: signature declares … but
  operand has runtime type …`); each operand is then encoded and pushed into the
  outputs accumulator (which feeds the communications commitment,
  [§1.5](#15-communications-commitment)).

## 4. Worked example: assert `a · b = c` over private inputs

A minimal circuit that takes three private witnesses, multiplies two, and
constrains the product to equal the third. It returns nothing, so `outputs` is
empty and the `Output` terminator carries no values:

```jsonc
{
  "version": { "major": 3, "minor": 0 },
  "inputs": [],
  "outputs": [],
  "do_communications_commitment": false,
  "instructions": [
    {"op": "private_input", "type": "Scalar<BLS12-381>", "output": "%a"},
    {"op": "private_input", "type": "Scalar<BLS12-381>", "output": "%b"},
    {"op": "private_input", "type": "Scalar<BLS12-381>", "output": "%c"},
    {"op": "mul",          "a": "%a", "b": "%b", "output": "%p"},
    {"op": "constrain_eq", "a": "%p", "b": "%c"},
    {"op": "output",       "vals": []}
  ]
}
```

Note how operands are named (`%a`) rather than positional indices, each
producing instruction declares its `output`, the constraint instruction binds
nothing, and execution ends at `Output`. The three `private_input`s must
exactly consume `ProofPreimage::private_transcript`, or preprocessing fails with
`Transcripts not fully consumed`.

## 5. Failure modes (runtime, during preprocessing)

These are the explicit error conditions enforced when evaluating an `IrSource`
against a preimage — useful when debugging a circuit that proves locally but
fails to preprocess (`zkir-v3/src/ir_vm.rs` and the per-instruction modules):

| Error | Cause |
|---|---|
| `variable not found: {id}` | An operand referenced a name not yet bound in memory. |
| `Not enough raw inputs: ran out at index …` / `Expected {n} raw inputs, received {m}` | `ProofPreimage::inputs` too short / wrong length for the typed `inputs` signature. |
| `Expected boolean, found: …` | A boolean operand held a value other than `0` / `1`. |
| `Failed direct assertion` | `Assert` saw `cond != 1`. |
| `Bit bound failed: … is not n-bit` | A value exceeded its declared bit width. |
| `Excessive bit bound` / `Excessive bit count` | `bits ≥ 255` (`ConstrainBits`) or `bits > 248` (`DivModPowerOfTwo` / `ReconstituteField`). |
| `Reconstituted element overflows field` | `ReconstituteField` result ≥ field modulus. |
| `Unsupported addition / multiplication / negation / inversion …` | A type-polymorphic arithmetic instruction got an unsupported or mismatched operand type. |
| `cannot invert zero of type …` | `Inv` applied to zero. |
| `Cannot extract coordinates of the Secp256k1 identity` / coordinates not on curve | `IntoCoordinates` / `FromCoordinates` on the identity or an off-curve pair. |
| `Bytes32FromLowHigh: low operand must fit in 31 bytes … and high … in a single byte` | `Bytes32FromLowHigh` precondition violated. |
| `Inputs did not match alignment` | `PersistentHash` / `Keccak256` inputs didn't conform to `alignment`. |
| `DivModPowerOfTwo requires exactly 2 outputs` / `PersistentHash requires exactly 2 outputs` / `Unexpected output length of encode instruction` | Wrong number of declared output names for the instruction. |
| `Failed to decode … as …` | A raw input / transcript value could not be decoded as its declared type. |
| `Transcripts not fully consumed` | Some transcript stream had leftover values at the end. |
| `Public transcript input mismatch …` | `Impact` reconciliation: declared PI ≠ transcript value. |
| `Communications commitment mismatch` | Recomputed `transient_commit(randomness ‖ inputs ‖ outputs)` ≠ preimage commitment. |
| `Output: signature declares N return values but instruction has M` / `Output position i: signature declares … but operand has runtime type …` | `Output` operands didn't match the `IrSource::outputs` signature. |

## 6. The `zkir-v3` tooling

The `zkir-v3` crate ships a CLI binary (`zkir`, built with the `binary` feature)
that operates on serialised `IrSource` files (`.zkir` JSON, and `.bzkir` for the
tagged binary form sent to the proof server). Its subcommands are `mock-compile`
and `compile` (validate / generate keys for a single IR file) and the
directory-batch variants `mock-compile-many` / `compile-many`. When given a
`.zkir`, it also writes the corresponding `.bzkir`.

`zkir-v3-wasm` exposes the v3 IR proving / checking surface to JS / TS
(published as `@midnight-ntwrk/zkir-v3`). The earlier v2 surface is published as
`@midnight-ntwrk/zkir-v2` from the `zkir-wasm` crate.

## 7. Relationship to the Impact VM

ZKIR and the Impact VM are different layers and should not be confused (see
[impact-opcodes.md](./impact-opcodes.md)). In brief: the `Impact` instruction
(and the `PublicInput` / `PrivateInput` transcript witnesses) are the channel
between the proof and the on-chain VM — the public inputs a ZKIR proof commits
to *are* the guarded Impact-VM operations the chain will execute if the proof
verifies.

## 8. Migrating from v2

v3 is a substantial restructuring of v2, not just a few new opcodes. v2 has
**26** instructions; v3 has **33**. The model, the type system, and the
instruction membership all changed.

**Model changes:**

* **Named SSA instead of an index tape.** v2's positional `Index` (`u32`)
  memory tape is replaced by named variables (`%name`) and inline immediates
  (`0x…`). Each producing instruction names its result(s) explicitly.
* **A type system.** Values are typed; in addition to `Native`, v3 adds
  `Bytes32`, first-class `JubjubPoint` / `JubjubScalar`, and the emulated
  `Secp256k1Point` / `Secp256k1Base` / `Secp256k1Scalar` family. `Add`, `Neg`,
  `TestEq`, `ConstrainEq`, and `CondSelect` are type-polymorphic.
* **Typed circuit signature.** `num_inputs: u32` becomes
  `inputs: Vec<TypedIdentifier>`, plus a new `outputs: Vec<IrType>` return
  signature that the `Output` terminator is type-checked against.
* **Minor versions reset.** v3 starts at `V0` and does not inherit v2's
  minor-version history.

**Instruction-set delta:**

| | v2 | v3 |
|---|---|---|
| **Added** | — | `Encode`, `Impact`, `Keccak256`, `Inv`, `IntoCoordinates`, `FromCoordinates`, `IntoBytes32`, `FromBytes32`, `Bytes32IntoLowHigh`, `Bytes32FromLowHigh`, `JubjubScalarFromNative` |
| **Removed / folded in** | `EcAdd`, `LoadImm`, `DeclarePubInput`, `PiSkip` | `EcAdd` → `Add` on point types; `LoadImm` → inline `Immediate` operands; `DeclarePubInput` + `PiSkip` → `Impact` |
| **Changed shape** | `EcMul` / `EcMulGenerator` / `HashToCurve` push 2 raw coords; `Output` records a single value | now bind **1** point; `Output` is a typed **terminator** taking one operand per `outputs` entry; `PublicInput` / `PrivateInput` now carry a result `type` |

(There is **no `Decode` instruction** in v3; raw-element decoding for inputs and
transcript reads is handled internally, and typed reconstruction is done by the
specific conversions in [§3.6](#36-encoding-and-type-conversions).)

## Appendix B — IR v2 (legacy)

v2 is the prior stable IR (`zkir` crate — `midnight-zkir` 2.2.0); it is what the
current stable Compact toolchain emits and is wrapped by
`@midnight-ntwrk/zkir-v2`. Structurally it differs from v3 as summarised in
[§8](#8-migrating-from-v2): a v2 `IrSource` is `{ version, num_inputs: u32,
do_communications_commitment: bool, instructions }`, memory is an append-only
tape of native field elements, and operands are `Index` (`u32`) positions into
that tape (outputs are *appended*, not named).

The 26 v2 instructions:

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
