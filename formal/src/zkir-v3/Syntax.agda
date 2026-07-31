{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Abstract syntax of zkir-v3  (ir.rs)
--
-- `Identifier`, `Operand`, the 34 `Instruction`s (in ir.rs order), and
-- the `IrSource` record.  `Fr` and `Alignment` come from the trust base;
-- `IrType` from the Types module.
------------------------------------------------------------------------

module zkir-v3.Syntax (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯

open import Data.Bool    using (Bool)
open import Data.List    using (List)
open import Data.Maybe   using (Maybe)
open import Data.Nat     using (ℕ)
open import Data.Product using (_×_)
open import Data.String  using (String)

------------------------------------------------------------------------
-- Identifiers  (ir.rs: Identifier)
--
-- In the concrete syntax variables are '%'-prefixed strings; the
-- leading-'%' invariant belongs to the well-formedness layer.
------------------------------------------------------------------------

Identifier : Set
Identifier = String

------------------------------------------------------------------------
-- Operands  (ir.rs: Operand)
--
-- A reference to a bound variable, or an immediate.  An immediate always
-- denotes a `Native` value.
------------------------------------------------------------------------

data Operand : Set where
  var : Identifier → Operand
  imm : Fr         → Operand

------------------------------------------------------------------------
-- Typed identifier  (ir.rs: TypedIdentifier) — a declared circuit input.
------------------------------------------------------------------------

record TypedIdentifier : Set where
  constructor _⦂_
  field
    name  : Identifier
    val-t : IrType

------------------------------------------------------------------------
-- Minor version  (ir.rs: IrMinorVersion) — only V0 at present.
------------------------------------------------------------------------

data IrMinorVersion : Set where
  V0 : IrMinorVersion

------------------------------------------------------------------------
-- Instructions  (ir.rs: Instruction)
--
-- The 34 variants in ir.rs order.  `bits` fields are u32 in Rust (ℕ
-- here).  Operand/identifier lists are `List`; pairs are `_×_`.  Arity
-- and type-compatibility constraints belong to the semantics layer.
------------------------------------------------------------------------

data Instruction : Set where

  -- Encode a typed value as its raw Fr elements.
  encode
    : (input   : Operand)
    → (outputs : List Identifier)
    → Instruction

  -- Assert cond = 1.  UB if cond ∉ {0,1}.
  assert
    : (cond : Operand)
    → Instruction

  -- Select a when bit = 1, else b.  Requires a, b of the same type.
  cond-select
    : (bit    : Operand)
    → (a b    : Operand)
    → (output : Identifier)
    → Instruction

  -- Constrain val to fit in `bits` bits.
  constrain-bits
    : (val  : Operand)
    → (bits : ℕ)
    → Instruction

  -- Constrain a = b.
  constrain-eq
    : (a b : Operand)
    → Instruction

  -- Constrain val ∈ {0,1}.
  constrain-to-boolean
    : (val : Operand)
    → Instruction

  -- Copy a value.
  copy
    : (val    : Operand)
    → (output : Identifier)
    → Instruction

  -- Declare public inputs under a guard (fused declare + pi-skip).
  impact
    : (guard  : Operand)
    → (inputs : List Operand)
    → Instruction

  -- Multiply a point by a scalar.
  ec-mul
    : (a      : Operand)
    → (scalar : Operand)
    → (output : Identifier)
    → Instruction

  -- Multiply the group generator by a scalar.
  ec-mul-generator
    : (scalar : Operand)
    → (output : Identifier)
    → Instruction

  -- Hash native elements to a Jubjub point.
  hash-to-curve
    : (inputs : List Operand)
    → (output : Identifier)
    → Instruction

  -- Affine coordinates (x, y) of a point.
  into-coordinates
    : (point   : Operand)
    → (outputs : Identifier × Identifier)
    → Instruction

  -- Reconstruct a point from affine coordinates.
  from-coordinates
    : (inputs : Operand × Operand)
    → (output : Identifier)
    → Instruction

  -- 32-byte representation of a value.
  into-bytes32
    : (input  : Operand)
    → (output : Identifier)
    → Instruction

  -- Value of the given type from a 32-byte representation.
  from-bytes32
    : (bytes  : Operand)
    → (val-t  : IrType)
    → (output : Identifier)
    → Instruction

  -- Reverse the byte order of a Bytes32 value.  Bytes32 → Bytes32.
  reverse-bytes
    : (bytes  : Operand)
    → (output : Identifier)
    → Instruction

  -- Decompose Bytes32 into (low 31 bytes, high byte) as Native.
  bytes32-into-low-high
    : (bytes   : Operand)
    → (outputs : Identifier × Identifier)
    → Instruction

  -- Reassemble Bytes32 from (low, high).  Requires low < 2²⁴⁸, high < 256.
  bytes32-from-low-high
    : (inputs : Operand × Operand)
    → (output : Identifier)
    → Instruction

  -- Divide by 2^bits: outputs [quotient, remainder].
  div-mod-power-of-two
    : (val     : Operand)
    → (bits    : ℕ)
    → (outputs : List Identifier)
    → Instruction

  -- divisor·2^bits + modulus, no field overflow, modulus < 2^bits.
  reconstitute-field
    : (divisor : Operand)
    → (modulus : Operand)
    → (bits    : ℕ)
    → (output  : Identifier)
    → Instruction

  -- Circuit-friendly (transient) hash.
  transient-hash
    : (inputs : List Operand)
    → (output : Identifier)
    → Instruction

  -- Long-term (persistent, SHA-256) hash with alignment.  Output Bytes32.
  persistent-hash
    : (alignment : Alignment)
    → (inputs    : List Operand)
    → (output    : Identifier)
    → Instruction

  -- Keccak-256 hash with alignment.  Output Bytes32.
  keccak256
    : (alignment : Alignment)
    → (inputs    : List Operand)
    → (output    : Identifier)
    → Instruction

  -- Equality test.  Output is 1 iff a = b.
  test-eq
    : (a b    : Operand)
    → (output : Identifier)
    → Instruction

  -- Addition (field or point).
  add
    : (a b    : Operand)
    → (output : Identifier)
    → Instruction

  -- Prime-field multiplication.
  mul
    : (a b    : Operand)
    → (output : Identifier)
    → Instruction

  -- Negation (field or point).
  neg
    : (a      : Operand)
    → (output : Identifier)
    → Instruction

  -- Prime-field inverse.  Errors if a = 0.
  inv
    : (a      : Operand)
    → (output : Identifier)
    → Instruction

  -- Boolean NOT.  UB if a ∉ {0,1}.
  not
    : (a      : Operand)
    → (output : Identifier)
    → Instruction

  -- Unsigned less-than in `bits`-bit precision.
  less-than
    : (a b    : Operand)
    → (bits   : ℕ)
    → (output : Identifier)
    → Instruction

  -- Cast a Native value to a Jubjub scalar (modular reduction).
  jubjub-scalar-from-native
    : (a      : Operand)
    → (output : Identifier)
    → Instruction

  -- Next value from the public transcript (or default if guard fails).
  public-input
    : (guard  : Maybe Operand)
    → (val-t  : IrType)
    → (output : Identifier)
    → Instruction

  -- Next value from the private transcript (or default if guard fails).
  private-input
    : (guard  : Maybe Operand)
    → (val-t  : IrType)
    → (output : Identifier)
    → Instruction

  -- Terminator: produce the circuit's return values, in signature order.
  -- (ir.rs: Output; named `circuit-output` here so that `output` is free
  -- as a per-instruction output-register binder.)
  circuit-output
    : (vals : List Operand)
    → Instruction

------------------------------------------------------------------------
-- Circuit source  (ir.rs: IrSource)
------------------------------------------------------------------------

record IrSource : Set where
  constructor mk-ir-source
  field
    version                       : IrMinorVersion
    inputs                        : List TypedIdentifier
    outputs                       : List IrType
    do-communications-commitment  : Bool
    instructions                  : List Instruction
