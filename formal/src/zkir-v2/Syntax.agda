{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.Syntax (⋯ : _) (open Assumptions ⋯) where

open import Data.Bool  using (Bool)
open import Data.List  using (List)
open import Data.Maybe using (Maybe)
open import Data.Nat   using (ℕ)

------------------------------------------------------------------------
-- External carrier types
--
-- `Fr` (BLS12-381 scalar field element, transient_crypto::curve::Fr) and
-- `Alignment` (byte-alignment descriptor, base_crypto::fab::Alignment)
-- are part of the trust base; they come from the `Assumptions` parameter.
------------------------------------------------------------------------
-- Index  (ir.rs: Index = u32)
--
-- All operands and outputs are references into the circuit memory by
-- position.  Named variables and types belong to the v3 IR.
------------------------------------------------------------------------

Index : Set
Index = ℕ

------------------------------------------------------------------------
-- Minor version  (ir.rs: IrMinorVersion)
------------------------------------------------------------------------

data IrMinorVersion : Set where
  V0 V1 : IrMinorVersion

------------------------------------------------------------------------
-- Instructions  (ir.rs: Instruction)
--
-- The 26 variants appear in the same order as in the Rust source.
-- Constructor names are the Rust CamelCase variant names in
-- kebab-case (`CondSelect` → `cond-select`); field names follow the
-- Rust names.
--
-- Outputs are implicit: each instruction appends a fixed number of
-- fresh values to the end of the circuit memory.  Arity information
-- belongs to the typing/semantics layer.
--
-- `bits` and `count` fields are u32 in Rust; we use ℕ here.
------------------------------------------------------------------------

data Instruction : Set where

  -- Assert that cond = 1.  UB if cond ∉ {0, 1}.
  -- No outputs.
  assert
    : (cond : Index)
    → Instruction

  -- Conditionally select a value.  UB if bit ∉ {0, 1}.
  -- Output = a when bit = 1, b when bit = 0.  1 output.
  cond-select
    : (bit : Index)
    → (a b : Index)
    → Instruction

  -- Constrain var to fit in the given number of bits.
  -- No outputs.
  constrain-bits
    : (var  : Index)
    → (bits : ℕ)
    → Instruction

  -- Constrain a = b.  No outputs.
  constrain-eq
    : (a b : Index)
    → Instruction

  -- Constrain var ∈ {0, 1}.  No outputs.
  constrain-to-boolean
    : (var : Index)
    → Instruction

  -- Copy a value into a fresh memory cell.  Emits no constraints
  -- in-circuit (a register-level alias).  1 output.
  copy
    : (var : Index)
    → Instruction

  -- Declare var as the next public input.  No outputs.
  declare-pub-input
    : (var : Index)
    → Instruction

  -- Mark the preceding DeclarePubInput group as conditionally active.
  -- No outputs.
  pi-skip
    : (guard : Maybe Index)
    → (count : ℕ)
    → Instruction

  -- Add two Jubjub curve points.  UB if either is not a valid point.
  -- 2 outputs: c_x, c_y.
  ec-add
    : (a_x a_y : Index)
    → (b_x b_y : Index)
    → Instruction

  -- Multiply a Jubjub curve point by a scalar.  UB if not a valid point.
  -- 2 outputs: c_x, c_y.
  ec-mul
    : (a_x a_y : Index)
    → (scalar  : Index)
    → Instruction

  -- Multiply the Jubjub group generator by a scalar.
  -- 2 outputs: c_x, c_y.
  ec-mul-generator
    : (scalar : Index)
    → Instruction

  -- Hash a sequence of field elements to a Jubjub point.
  -- 2 outputs: c_x, c_y.
  hash-to-curve
    : (inputs : List Index)
    → Instruction

  -- Load a constant field element into the circuit.  1 output.
  load-imm
    : (imm : Fr)
    → Instruction

  -- Divide by 2^bits: 2 outputs (val >> bits) and (val mod 2^bits).
  div-mod-power-of-two
    : (var  : Index)
    → (bits : ℕ)
    → Instruction

  -- Outputs divisor << bits | modulus, guaranteeing no field overflow
  -- and modulus < 2^bits.  Inverse of div-mod-power-of-two.  1 output.
  reconstitute-field
    : (divisor : Index)
    → (modulus : Index)
    → (bits    : ℕ)
    → Instruction

  -- Output var into the communications commitment.  No IR-level output.
  output
    : (var : Index)
    → Instruction

  -- Circuit-friendly (transient) hash: 1 output H(inputs).
  transient-hash
    : (inputs : List Index)
    → Instruction

  -- Long-term (persistent) hash with alignment metadata.
  -- 2 outputs: (h₁, h₂), with h₁ the high-order byte and h₂ the
  -- remaining 31 bytes assembled as a field element.
  persistent-hash
    : (alignment : Alignment)
    → (inputs    : List Index)
    → Instruction

  -- Test equality.  1 output: 1 iff a = b.
  test-eq
    : (a b : Index)
    → Instruction

  -- Field addition.  1 output a + b.
  add
    : (a b : Index)
    → Instruction

  -- Field multiplication.  1 output a * b.
  mul
    : (a b : Index)
    → Instruction

  -- Field negation.  1 output -a.
  neg
    : (a : Index)
    → Instruction

  -- Boolean NOT.  UB if a ∉ {0, 1}.  1 output !a.
  not
    : (a : Index)
    → Instruction

  -- Unsigned less-than in bits-bit precision.
  -- UB if a or b exceed 2^bits − 1.  1 output: 1 iff a < b.
  less-than
    : (a b  : Index)
    → (bits : ℕ)
    → Instruction

  -- Retrieve the next value from the *public-output* transcript
  -- (`pub-transcript-outputs` — values flowing *into* the circuit; the
  -- Rust names are historical, spec §4.4 sidebar.  Not
  -- `pub-transcript-inputs`, which `pi-skip` validation reads.)
  -- Active (guard absent, or guard evaluates to 1) consumes the next
  -- transcript entry and outputs it; inactive (guard evaluates to 0)
  -- outputs 0 and consumes nothing.  1 output.
  public-input
    : (guard : Maybe Index)
    → Instruction

  -- Retrieve the next value from the private witness transcript.
  -- Active (guard absent, or guard evaluates to 1) consumes the next
  -- transcript entry and outputs it; inactive (guard evaluates to 0)
  -- outputs 0 and consumes nothing.  1 output.
  private-input
    : (guard : Maybe Index)
    → Instruction

------------------------------------------------------------------------
-- Circuit source  (ir.rs: IrSource)
------------------------------------------------------------------------

record IrSource : Set where
  constructor mk-ir-source
  field
    -- Carried for record fidelity with the Rust `IrSource`; nothing in
    -- the development consumes it.  The preprocess semantics is
    -- version-independent, and the circuit model implements the V1
    -- lowering only (spec §5.2).
    version                       : IrMinorVersion
    num-inputs                    : ℕ
    do-communications-commitment  : Bool
    instructions                  : List Instruction
