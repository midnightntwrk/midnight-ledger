{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.Circuit (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.FieldProperties ⋯

------------------------------------------------------------------------
-- Circuit (Halo2 PLONKish) semantics for ZKIR v2 (V1 lowerings).
--
-- This module defines (spec §6.5):
--
--   • Section A defines a closed vocabulary of arithmetization
--     primitives (`Expr`, `Constraint`) and the structural synthesis
--     function (`circuit-instr`, `circuit`) that lowers each source
--     instruction to a *list of constraints*, mirroring §5.2's emission
--     contracts.  Synthesis is a deterministic function of the source
--     alone — independent of the prover's preimage (§5.4).
--
--   • Section B defines a wire-assignment model (`Witness`) and the
--     satisfaction relation (`holds`, `satisfies`).  `holds` is a single
--     interpreter over the closed `Constraint` vocabulary: every
--     constraint is either a polynomial gate (`gate`, a field
--     expression that evaluates to 0) or one of a fixed set of gadget
--     atoms (range, boolean, Poseidon, Jubjub, SHA-256, …).  Because
--     the only way to describe a constraint is through this
--     vocabulary, "the circuit emits valid constraints" is witnessed
--     by the synthesis *data*
--     (`circuit-instr`), not by reading a bespoke proposition per
--     instruction.
--
--     The gadget atoms interpret chip behaviour (Poseidon, Jubjub,
--     SHA-256, range checks) via the same canonical functions defined in
--     `zkir-v2.Assumptions`; this makes the chip "perfectly sound" by
--     construction and is the axiomatic interface to Halo2's chip layer.
--
--   • The closing section proves satisfaction decidable (`is-bit?`,
--     `holds?`) — an executable witness checker.  It is a deliverable
--     of this module; no proof in the development consumes it.
------------------------------------------------------------------------

-- Field/curve operations and the `Fits-in`/`bits-lt`/`to-bool`/`pow2-fr`/
-- `lt-bits` helpers come from the `Assumptions` parameter (opened by the
-- module telescope) and `FieldProperties`.  Only the genuinely
-- Semantics-level definitions are imported here.
open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
  using ( mem-lookup; mem-lookups; from-bool
        ; Preprocessed; ProofPreimage )

open import Data.Bool    using (Bool; true; false; if_then_else_)
open import Data.List    using (List; []; _∷_; _++_; _∷ʳ_; length; take)
open import Data.Maybe   using (Maybe; nothing; just; _>>=_; map)
open import Data.Nat     using (ℕ; suc; zero; _∸_; _+_)
open import Data.Product using (_×_; _,_; ∃-syntax)
open import Data.List.Relation.Unary.All using (All)
open import Data.Unit    using (⊤; tt)
open import Data.Empty   using (⊥)
open import Data.Sum     using (_⊎_)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; subst)
open import Relation.Nullary using (¬_; Dec; yes; no)
open import Relation.Nullary.Decidable using (map′; _×-dec_; _⊎-dec_; ¬?)
open import Function using (case_of_)
open import Data.Maybe.Properties using (just-injective)
  renaming (≡-dec to maybe-≡-dec)
open import Data.Product.Properties renaming (≡-dec to ×-≡-dec)

------------------------------------------------------------------------
-- Local helpers
--
-- These helpers are exported because `holds` (Section B) refers to them
-- in its result types, and downstream proofs need to construct/destruct
-- those types.
------------------------------------------------------------------------

-- "Wire bound to memory cell i has value v" — phrased as a
-- propositional equality, for readability in constraint definitions.
_at_↦_ : List Fr → Index → Fr → Set
mem at i ↦ v = mem-lookup mem i ≡ just v

infix 1 _at_↦_

-- A bit predicate: v ∈ {0, 1}.  Encoded as a sum.
is-bit : Fr → Set
is-bit v = (v ≡ 0ᶠ) ⊎ (v ≡ 1ᶠ)

------------------------------------------------------------------------
-- Section A.  Circuit syntax
------------------------------------------------------------------------

-- Field expressions over the wires.
--
-- A `wire` is a wire reference (by `Index`); `con` is an inline constant;
-- `_⊕_`, `_⊗_`, `⊝_` are the field operations.  Expressions are the
-- left-/right-hand sides of polynomial gates; they evaluate to a
-- field value via `eval` once a wire assignment is fixed (Section B).

data Expr : Set where
  wire : (i : Index) → Expr
  con : (k : Fr) → Expr
  _⊕_ : (l r : Expr) → Expr
  _⊗_ : (l r : Expr) → Expr
  ⊝_  : (e : Expr) → Expr

infixl 6 _⊕_
infixl 7 _⊗_
infix  8 ⊝_

-- Subtraction, the difference used by every polynomial gate `l − r = 0`.
_⊖_ : Expr → Expr → Expr
l ⊖ r = l ⊕ ⊝ r

infixl 6 _⊖_

-- Constraints
--
-- The closed vocabulary of arithmetization primitives.  `gate` is a
-- polynomial custom gate (a field expression constrained to 0).  Every
-- other constructor is a gadget atom whose meaning (Section B) is fixed
-- by the corresponding canonical chip function.  Wire references are by
-- `Index`; PI-vector positions (`entry`) count from zero across the
-- entire PI vector, including the structural preamble (binding-input,
-- optional comm.0).

data Constraint : Set where

  -- Polynomial custom gate: the field expression ⟦e⟧ evaluates to 0.
  -- Via the `_≑_` sugar (⟦l⟧ = ⟦r⟧ ≔ gate (l ⊖ r)) this covers add,
  -- mul, neg, copy, constrain_eq, and load_imm.
  gate
    : (e : Expr) → Constraint

  -- constrain_to_boolean(v): ⟦v⟧ ∈ {0, 1}
  boolean
    : (v : Index) → Constraint

  -- assert(c): ⟦c⟧ ≠ 0
  non-zero
    : (c : Index) → Constraint

  -- constrain_bits(v, n) / range chip: ⟦v⟧ < 2^n
  in-range
    : (v : Index) → (bits : ℕ) → Constraint

  -- cond_select(b, a, c): ⟦b⟧ ∈ {0,1} ∧ out = ⟦b⟧·⟦a⟧ + (1−⟦b⟧)·⟦c⟧
  select
    : (out b a c : Index) → Constraint

  -- div_mod_power_of_two(v, n):
  --   ⟦v⟧ = ⟦q⟧·2^n + ⟦r⟧  ∧  ⟦r⟧ < 2^n  ∧  ⟦q⟧ < 2^(FR_BITS − n)
  --   ∧  ⟦q⟧·2^n + ⟦r⟧ < |Fr|  (non-wrapping: pins the canonical split)
  div-mod
    : (q r v : Index) → (bits : ℕ) → Constraint

  -- reconstitute_field(d, m, n):
  --   ⟦d⟧ < 2^(FR_BITS − n)  ∧  ⟦m⟧ < 2^n  ∧  out = ⟦d⟧·2^n + ⟦m⟧
  -- (No overflow check — see §6.3 and obligation O3.)
  reconstitute
    : (out d m : Index) → (bits : ℕ) → Constraint

  -- not(a): out = is_zero(⟦a⟧)
  is-zero
    : (out a : Index) → Constraint

  -- test_eq(a, b): out = 1 iff ⟦a⟧ = ⟦b⟧
  test-eq
    : (out a b : Index) → Constraint

  -- less_than(a, b, n): out = 1 iff ⟦a⟧ < ⟦b⟧, using a range-check chip
  -- with padded bit-bound `lt-bits n`.
  less-than
    : (out a b : Index) → (bits : ℕ) → Constraint

  -- transient_hash(I): out = Poseidon(⟦I⟧)
  poseidon
    : (out : Index) → (inputs : List Index) → Constraint

  -- persistent_hash(α, I): (h₁, h₂) is the SHA-256 decomposition (high
  -- byte / low 31 bytes) of the FAB byte encoding of ⟦I⟧ under α.
  sha256
    : (h₁ h₂ : Index) → (alignment : Alignment)
    → (inputs : List Index) → Constraint

  -- ec_add: (⟦aₓ⟧,⟦aᵧ⟧) ∈ J ∧ (⟦bₓ⟧,⟦bᵧ⟧) ∈ J ∧ (cₓ,cᵧ) = sum
  ec-add
    : (c-x c-y a-x a-y b-x b-y : Index) → Constraint

  -- ec_mul: (⟦aₓ⟧,⟦aᵧ⟧) ∈ J ∧ (cₓ,cᵧ) = ⟦s⟧ · (⟦aₓ⟧,⟦aᵧ⟧)
  ec-mul
    : (c-x c-y a-x a-y scalar : Index) → Constraint

  -- ec_mul_generator: (cₓ,cᵧ) = ⟦s⟧ · G
  ec-gen
    : (c-x c-y scalar : Index) → Constraint

  -- hash_to_curve: (cₓ,cᵧ) = H2C(⟦I⟧)
  h2c
    : (c-x c-y : Index) → (inputs : List Index) → Constraint

  -- Declared PI binding: pis[entry] = ⟦wire⟧
  bind
    : (entry : ℕ) → (widx : Index) → Constraint

  -- Guarded input cell: ⟦i⟧ ∈ {0, 1} ∧ ((out = 0) ∨ (⟦i⟧ = 1)).
  -- Emitted when public/private_input has a guard; the guard wire is
  -- range-restricted to a boolean by a checked native→bit conversion.
  guard-disj
    : (out i : Index) → Constraint

  -- Communications-commitment constraint (§5.3):
  --   pis[1] = Poseidon(comm-rand ‖ inputs ‖ outputs)
  -- where `inputs = [0 .. num_inputs)` and `outputs` are the args of
  -- `output(v)` instructions in source order.
  comm
    : (inputs outputs : List Index) → Constraint

-- The constraint system: structurally a list of constraints, plus the
-- structural shape (number of wires, PI-vector length, comm-commitment
-- flag).  Determined by the source alone.
record Circuit : Set where
  constructor mk-circuit
  field
    nr-wires    : ℕ
    constraints : List Constraint
    pi-len      : ℕ          -- expected length of the verifier's PI vector
    has-comm    : Bool

------------------------------------------------------------------------
-- Synthesis
--
-- `circuit-instr` processes one instruction, growing the wire count and
-- appending constraints.  It mirrors the shape of `preprocess-instr` but
-- is total (synthesis cannot fail; §5.4).
--
-- The synthesis state additionally tracks the count of `DeclarePubInput`
-- emitted (for PI-entry indexing) and the indices of `output(v)`
-- arguments (in source order; consumed by the comm-commitment constraint).
------------------------------------------------------------------------

record SynthState : Set where
  constructor mk-synth
  field
    nr-wires       : ℕ                  -- next wire to be allocated
    constraints    : List Constraint    -- in emission order
    nr-declared-pi : ℕ                  -- # DeclarePubInput processed
    output-wires   : List Index         -- args of `output(v)` in order

-- Number of "preamble" PI entries (binding-input + optional comm.0).
preamble-pi-count : Bool → ℕ
preamble-pi-count true  = 2
preamble-pi-count false = 1

-- Update helpers.

private
  push : SynthState → Constraint → SynthState
  push st c = record st { constraints = SynthState.constraints st ∷ʳ c }

  bump-wires : SynthState → ℕ → SynthState
  bump-wires st n = record st { nr-wires = SynthState.nr-wires st + n }

-- Equality gate: ⟦l⟧ = ⟦r⟧, lowered to the polynomial gate l − r = 0.
_≑_ : Expr → Expr → Constraint
l ≑ r = gate (l ⊖ r)

infix 4 _≑_

-- One instruction's worth of synthesis.  `has-comm` is the source-level
-- flag, threaded in because `declare-pub-input` needs the PI-entry
-- offset.
circuit-instr : Bool → Instruction → SynthState → SynthState

circuit-instr _ (assert c) st =
  push st (non-zero c)

circuit-instr _ (cond-select b a c) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (select out b a c)

circuit-instr _ (constrain-bits v bits) st =
  -- WF2 forbids bits ≥ FR_BITS; for bits ≥ FR_BITS the lowering would
  -- emit no constraint.  We unconditionally emit the range atom and let
  -- WF2 guarantee its existence in the model.
  push st (in-range v bits)

circuit-instr _ (constrain-eq a b) st =
  push st (wire a ≑ wire b)

circuit-instr _ (constrain-to-boolean v) st =
  push st (boolean v)

circuit-instr _ (copy v) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (wire out ≑ wire v)

circuit-instr has-comm (declare-pub-input v) st =
  let entry = preamble-pi-count has-comm + SynthState.nr-declared-pi st
      st'   = record st { nr-declared-pi = suc (SynthState.nr-declared-pi st) }
  in push st' (bind entry v)

circuit-instr _ (pi-skip _ _) st =
  st  -- no in-circuit constraints

circuit-instr _ (ec-add a-x a-y b-x b-y) st =
  let cx = SynthState.nr-wires st
      cy = suc cx
  in push (bump-wires st 2) (ec-add cx cy a-x a-y b-x b-y)

circuit-instr _ (ec-mul a-x a-y scalar) st =
  let cx = SynthState.nr-wires st
      cy = suc cx
  in push (bump-wires st 2) (ec-mul cx cy a-x a-y scalar)

circuit-instr _ (ec-mul-generator scalar) st =
  let cx = SynthState.nr-wires st
      cy = suc cx
  in push (bump-wires st 2) (ec-gen cx cy scalar)

circuit-instr _ (hash-to-curve inputs) st =
  let cx = SynthState.nr-wires st
      cy = suc cx
  in push (bump-wires st 2) (h2c cx cy inputs)

circuit-instr _ (load-imm imm) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (wire out ≑ con imm)

circuit-instr _ (div-mod-power-of-two v bits) st =
  let q = SynthState.nr-wires st
      r = suc q
  in push (bump-wires st 2) (div-mod q r v bits)

circuit-instr _ (reconstitute-field d m bits) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (reconstitute out d m bits)

circuit-instr _ (output v) st =
  -- No constraints; the wire index is recorded for the comm-commitment
  -- constraint emitted at the end of synthesis (if has-comm).
  record st { output-wires = SynthState.output-wires st ∷ʳ v }

circuit-instr _ (transient-hash inputs) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (poseidon out inputs)

circuit-instr _ (persistent-hash alignment inputs) st =
  let h₁ = SynthState.nr-wires st
      h₂ = suc h₁
  in push (bump-wires st 2) (sha256 h₁ h₂ alignment inputs)

circuit-instr _ (test-eq a b) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (test-eq out a b)

circuit-instr _ (add a b) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (wire out ≑ wire a ⊕ wire b)

circuit-instr _ (mul a b) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (wire out ≑ wire a ⊗ wire b)

circuit-instr _ (neg a) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (wire out ≑ ⊝ wire a)

circuit-instr _ (not a) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (is-zero out a)

circuit-instr _ (less-than a b bits) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (less-than out a b bits)

circuit-instr _ (public-input nothing) st =
  bump-wires st 1                      -- a free witness wire, no constraint
circuit-instr _ (public-input (just g)) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (guard-disj out g)

circuit-instr _ (private-input nothing) st =
  bump-wires st 1
circuit-instr _ (private-input (just g)) st =
  let out = SynthState.nr-wires st in
  push (bump-wires st 1) (guard-disj out g)

-- Fold over an instruction list.

circuit-instrs : Bool → List Instruction → SynthState → SynthState
circuit-instrs _        []       st = st
circuit-instrs has-comm (i ∷ is) st =
  circuit-instrs has-comm is (circuit-instr has-comm i st)

-- Top-level synthesis.  Wires `[0 .. num_inputs)` are the
-- circuit-input wires (no constraints bind them; their values are part
-- of the witness).  After processing the instruction list, if has-comm,
-- append the comm-commitment constraint.

-- Natural-number range [0, 1, …, n-1].  Used as `cm-inputs` inside
-- `circuit`'s comm constraint, and by `CircuitProof.inputs-lookup-init`.
nat-range : ℕ → List Index
nat-range zero    = []
nat-range (suc k) = nat-range k ∷ʳ k

circuit : IrSource → Circuit
circuit src =
  let n   = IrSource.num-inputs src
      hc  = IrSource.do-communications-commitment src
      st₀ = mk-synth n [] 0 []
      st  = circuit-instrs hc (IrSource.instructions src) st₀
      cs  = SynthState.constraints st
      -- Built-in inputs to the comm-commitment: [0 .. n).
      cm-inputs = nat-range n
      cs' = if hc
        then cs ∷ʳ comm cm-inputs (SynthState.output-wires st)
        else cs
      pi-len = preamble-pi-count hc + SynthState.nr-declared-pi st
  in mk-circuit (SynthState.nr-wires st) cs' pi-len hc

------------------------------------------------------------------------
-- Section B.  Wire assignments and satisfaction
------------------------------------------------------------------------

-- Witness for a circuit
--
-- The Halo2 prover commits to:
--   • `mem` — values of the witness wires (one per allocated cell).
--   • `pis` — values of the public-input wires (verifier-supplied).
--   • `comm-rand` — the randomness `comm_commitment.1`, allocated as
--     an additional witness wire iff the circuit has-comm.
--
-- `CircuitProof.witness-of` builds a `Witness` from the operational
-- state and the preimage.

record Witness : Set where
  constructor mk-witness
  field
    mem       : List Fr
    pis       : List Fr
    comm-rand : Maybe Fr

-- The randomness component of a preimage's optional commitment.
comm-rand-of : ProofPreimage → Maybe Fr
comm-rand-of pre = map (λ (_ , r) → r) (ProofPreimage.comm-commitment pre)

-- The witness assignment produced by an operational execution: the
-- allocated wire values, the verifier-supplied PI entries, and the
-- commitment randomness.  Independent of the `IrSource`.
witness-of : Preprocessed → ProofPreimage → Witness
witness-of s pre = mk-witness
  (Preprocessed.memory s)
  (Preprocessed.pis    s)
  (comm-rand-of pre)

-- The i-th PI entry is looked up exactly like a memory cell: the pis
-- vector is a `List Fr` and out-of-range yields `nothing`.
pi-lookup : List Fr → ℕ → Maybe Fr
pi-lookup = mem-lookup

-- Evaluation of a field expression against a memory assignment.
-- `nothing` if any referenced wire is out of range.
eval : List Fr → Expr → Maybe Fr
eval mem (wire i)  = mem-lookup mem i
eval mem (con k)  = just k
eval mem (l ⊕ r)  = eval mem l >>= λ x → eval mem r >>= λ y → just (x +ᶠ y)
eval mem (l ⊗ r)  = eval mem l >>= λ x → eval mem r >>= λ y → just (x *ᶠ y)
eval mem (⊝ e)    = eval mem e >>= λ x → just (-ᶠ x)

------------------------------------------------------------------------
-- Constraint satisfaction
--
-- `holds w c` is the proposition "wire assignment `w` satisfies
-- constraint `c`".  It is the single interpreter over the closed
-- `Constraint` vocabulary: a polynomial gate (`gate e`) requires the
-- field expression to evaluate to 0, and each gadget atom requires the
-- looked-up wire values to stand in the relation given by the
-- corresponding canonical chip primitive (a field of the `Assumptions`
-- record — the module parameter; nothing is postulated).
--
-- Treating chip primitives as canonical functions (rather than as
-- separate axiomatic relations) implicitly bakes in the chips'
-- soundness.
------------------------------------------------------------------------

holds : Witness → Constraint → Set

holds w (gate e) = eval (Witness.mem w) e ≡ just 0ᶠ

holds w (boolean v) =
  ∃-syntax (λ vv →
    (Witness.mem w at v ↦ vv) × is-bit vv)

holds w (non-zero c) =
  ∃-syntax (λ v →
    (Witness.mem w at c ↦ v) × (¬ (v ≡ 0ᶠ)))

holds w (in-range v bits) =
  ∃-syntax (λ vv →
      (Witness.mem w at v ↦ vv)
    × (Fits-in vv bits))

holds w (select out b a c) =
  ∃-syntax (λ bv → ∃-syntax (λ av → ∃-syntax (λ cv → ∃-syntax (λ ov →
      (Witness.mem w at b ↦ bv)
    × (Witness.mem w at a ↦ av)
    × (Witness.mem w at c ↦ cv)
    × (Witness.mem w at out ↦ ov)
    × is-bit bv
    × (ov ≡ (bv *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ bv)) *ᶠ cv))))))

holds w (div-mod q r v bits) =
  ∃-syntax (λ qv → ∃-syntax (λ rv → ∃-syntax (λ vv →
      (Witness.mem w at q ↦ qv)
    × (Witness.mem w at r ↦ rv)
    × (Witness.mem w at v ↦ vv)
    × (Fits-in rv bits)
    × (Fits-in qv (FR-BITS ∸ bits))
    × (NoWrap qv rv bits)
    × (vv ≡ (qv *ᶠ pow2-fr bits) +ᶠ rv))))
  -- The `NoWrap` conjunct forces the canonical (non-wrapping)
  -- decomposition, so (q, r) is the unique Euclidean split of `valFr v`
  -- (see `Encoding.div-mod-constraint-unique`).  Without it the bounds alone
  -- admit a second pair congruent mod |Fr|.

holds w (reconstitute out d m bits) =
  ∃-syntax (λ dv → ∃-syntax (λ mv → ∃-syntax (λ ov →
      (Witness.mem w at d ↦ dv)
    × (Witness.mem w at m ↦ mv)
    × (Witness.mem w at out ↦ ov)
    × (Fits-in dv (FR-BITS ∸ bits))
    × (Fits-in mv bits)
    × (ov ≡ (dv *ᶠ pow2-fr bits) +ᶠ mv))))
  -- N.B.: no overflow check.  A witness with dv·2^bits + mv ≥ |Fr| and
  -- ov the field-reduced value satisfies this but not the operational
  -- rule; producer obligation O3 closes the gap (§6.3).

holds w (is-zero out a) =
  -- is_zero(a): returns 1 iff a = 0, else 0.
  ∃-syntax (λ av → ∃-syntax (λ ov →
      (Witness.mem w at a ↦ av)
    × (Witness.mem w at out ↦ ov)
    × (ov ≡ from-bool (av ≡ᶠ? 0ᶠ))))

holds w (test-eq out a b) =
  ∃-syntax (λ av → ∃-syntax (λ bv → ∃-syntax (λ ov →
      (Witness.mem w at a ↦ av)
    × (Witness.mem w at b ↦ bv)
    × (Witness.mem w at out ↦ ov)
    × (ov ≡ from-bool (av ≡ᶠ? bv)))))

holds w (less-than out a b bits) =
  -- Range checks use the *padded* bit count (cf. §5.2 footnote).
  ∃-syntax (λ av → ∃-syntax (λ bv → ∃-syntax (λ ov →
      (Witness.mem w at a ↦ av)
    × (Witness.mem w at b ↦ bv)
    × (Witness.mem w at out ↦ ov)
    × (Fits-in av (lt-bits bits))
    × (Fits-in bv (lt-bits bits))
    × (ov ≡ from-bool (bits-lt (take (lt-bits bits) (to-le-bits av))
                               (take (lt-bits bits) (to-le-bits bv)))))))

holds w (poseidon out inputs) =
  ∃-syntax (λ vs → ∃-syntax (λ ov →
      (mem-lookups (Witness.mem w) inputs ≡ just vs)
    × (Witness.mem w at out ↦ ov)
    × (ov ≡ transient-hash-fn vs)))

holds w (sha256 h₁ h₂ alignment inputs) =
  ∃-syntax (λ vs → ∃-syntax (λ v1 → ∃-syntax (λ v2 →
      (mem-lookups (Witness.mem w) inputs ≡ just vs)
    × (Witness.mem w at h₁ ↦ v1)
    × (Witness.mem w at h₂ ↦ v2)
    × (persistent-hash-fn alignment vs ≡ (v1 , v2)))))

holds w (ec-add c-x c-y a-x a-y b-x b-y) =
  ∃-syntax (λ ax → ∃-syntax (λ ay → ∃-syntax (λ bx → ∃-syntax (λ by →
   ∃-syntax (λ cx → ∃-syntax (λ cy →
        (Witness.mem w at a-x ↦ ax)
      × (Witness.mem w at a-y ↦ ay)
      × (Witness.mem w at b-x ↦ bx)
      × (Witness.mem w at b-y ↦ by)
      × (Witness.mem w at c-x ↦ cx)
      × (Witness.mem w at c-y ↦ cy)
      × (ec-add-pts ax ay bx by ≡ just (cx , cy))))))))

holds w (ec-mul c-x c-y a-x a-y scalar) =
  ∃-syntax (λ ax → ∃-syntax (λ ay → ∃-syntax (λ sc →
   ∃-syntax (λ cx → ∃-syntax (λ cy →
        (Witness.mem w at a-x ↦ ax)
      × (Witness.mem w at a-y ↦ ay)
      × (Witness.mem w at scalar ↦ sc)
      × (Witness.mem w at c-x ↦ cx)
      × (Witness.mem w at c-y ↦ cy)
      × (ec-mul-pt ax ay sc ≡ just (cx , cy)))))))

holds w (ec-gen c-x c-y scalar) =
  ∃-syntax (λ sc → ∃-syntax (λ cx → ∃-syntax (λ cy →
      (Witness.mem w at scalar ↦ sc)
    × (Witness.mem w at c-x ↦ cx)
    × (Witness.mem w at c-y ↦ cy)
    × (ec-mul-gen sc ≡ (cx , cy)))))

holds w (h2c c-x c-y inputs) =
  ∃-syntax (λ vs → ∃-syntax (λ cx → ∃-syntax (λ cy →
      (mem-lookups (Witness.mem w) inputs ≡ just vs)
    × (Witness.mem w at c-x ↦ cx)
    × (Witness.mem w at c-y ↦ cy)
    × (hash-to-curve-fn vs ≡ (cx , cy)))))

holds w (bind entry widx) =
  ∃-syntax (λ wv → ∃-syntax (λ pv →
      (Witness.mem w at widx ↦ wv)
    × (pi-lookup (Witness.pis w) entry ≡ just pv)
    × (pv ≡ wv)))

holds w (guard-disj out i) =
  ∃-syntax (λ ov → ∃-syntax (λ iv →
      (Witness.mem w at out ↦ ov)
    × (Witness.mem w at i ↦ iv)
    × is-bit iv
    × ((ov ≡ 0ᶠ) ⊎ (iv ≡ 1ᶠ))))
  -- The guard wire is lowered through a checked native→bit conversion,
  -- so `⟦i⟧ ∈ {0, 1}` in addition to the disjunction.

holds w (comm inputs outputs) =
  ∃-syntax (λ ivs → ∃-syntax (λ ovs → ∃-syntax (λ rv → ∃-syntax (λ pv →
      (mem-lookups (Witness.mem w) inputs ≡ just ivs)
    × (mem-lookups (Witness.mem w) outputs ≡ just ovs)
    × (Witness.comm-rand w ≡ just rv)
    × (pi-lookup (Witness.pis w) 1 ≡ just pv)
    × (pv ≡ transient-commit (ivs ++ ovs) rv)))))

------------------------------------------------------------------------
-- Decidability of constraint satisfaction
--
-- This section is a *deliverable* of the module in its own right: it
-- makes `satisfies` executable as a concrete witness checker (given an
-- instantiation of `Assumptions` with decidable primitives).  No proof
-- elsewhere in the development consumes it — do not mistake it for
-- dead code.
--
-- `holds w c` is decidable.  Each constraint is an existential block whose
-- bound values are pinned by a *functional* lookup
-- (`mem-lookup`/`mem-lookups`/`pi-lookup`/`comm-rand`): once the lookup
-- resolves, the value is unique (`just`-injectivity), so the existential
-- collapses to deciding the residual predicate, which is built from the
-- decidable atoms `_≟ᶠ_`, `fits-in?`, `noWrap?` and `is-bit?`.
--
-- The proofs decide the *chained* form (each lookup immediately followed
-- by the rest, as an existential) and `map′` it onto the `holds` shape
-- (all lookups grouped before the residual predicate); the two are
-- logically equivalent and the transport functions just reassociate the
-- tuple.
------------------------------------------------------------------------

-- A field value is a bit exactly when it equals `0ᶠ` or `1ᶠ`.
is-bit? : ∀ v → Dec (is-bit v)
is-bit? v = (v ≟ᶠ 0ᶠ) ⊎-dec (v ≟ᶠ 1ᶠ)

private
  -- `∃ x. (m ≡ just x) × P x` is decidable when `P` is: `nothing` has no
  -- witness, and `just y` forces `x ≡ y` by injectivity.
  dec-just : ∀ {a p} {A : Set a} (m : Maybe A) (P : A → Set p)
    → (∀ x → Dec (P x))
    → Dec (∃-syntax (λ x → (m ≡ just x) × P x))
  dec-just nothing  P P? = no λ { (_ , () , _) }
  dec-just (just y) P P? =
    map′ (λ p → y , refl , p)
         (λ (_ , eq , p) → subst P (just-injective (sym eq)) p)
         (P? y)

  -- The same, specialized to a wire lookup `mem at i ↦ v`.
  dec-↦ : ∀ {p} mem i (P : Fr → Set p) → (∀ v → Dec (P v))
    → Dec (∃-syntax (λ v → (mem at i ↦ v) × P v))
  dec-↦ mem i = dec-just (mem-lookup mem i)

holds? : ∀ w c → Dec (holds w c)

holds? w (gate e) = maybe-≡-dec _≟ᶠ_ (eval (Witness.mem w) e) (just 0ᶠ)

holds? w (boolean v) = dec-↦ (Witness.mem w) v is-bit is-bit?

holds? w (non-zero c) =
  dec-↦ (Witness.mem w) c (λ v → ¬ (v ≡ 0ᶠ)) (λ v → ¬? (v ≟ᶠ 0ᶠ))

holds? w (in-range v bits) =
  dec-↦ (Witness.mem w) v (λ vv → Fits-in vv bits) (λ vv → fits-in? vv bits)

holds? w (select out b a c) = map′ to from
  (dec-↦ mem b _
    (λ bv → dec-↦ mem a _
      (λ av → dec-↦ mem c _
        (λ cv → dec-↦ mem out _
          (λ ov → is-bit? bv ×-dec
                  (ov ≟ᶠ ((bv *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ bv)) *ᶠ cv))))))))
  where
    mem = Witness.mem w
    to : _ → holds w (select out b a c)
    to (bv , lb , av , la , cv , lc , ov , lo , ib , eq) =
      bv , av , cv , ov , lb , la , lc , lo , ib , eq
    from : holds w (select out b a c) → _
    from (bv , av , cv , ov , lb , la , lc , lo , ib , eq) =
      bv , lb , av , la , cv , lc , ov , lo , ib , eq

holds? w (div-mod q r v bits) = map′ to from
  (dec-↦ mem q _
    (λ qv → dec-↦ mem r _
      (λ rv → dec-↦ mem v _
        (λ vv → fits-in? rv bits
                ×-dec fits-in? qv (FR-BITS ∸ bits)
                ×-dec noWrap? qv rv bits
                ×-dec (vv ≟ᶠ ((qv *ᶠ pow2-fr bits) +ᶠ rv))))))
  where
    mem = Witness.mem w
    to : _ → holds w (div-mod q r v bits)
    to (qv , lq , rv , lr , vv , lv , fr , fq , nw , eq) =
      qv , rv , vv , lq , lr , lv , fr , fq , nw , eq
    from : holds w (div-mod q r v bits) → _
    from (qv , rv , vv , lq , lr , lv , fr , fq , nw , eq) =
      qv , lq , rv , lr , vv , lv , fr , fq , nw , eq

holds? w (reconstitute out d m bits) = map′ to from
  (dec-↦ mem d _
    (λ dv → dec-↦ mem m _
      (λ mv → dec-↦ mem out _
        (λ ov → fits-in? dv (FR-BITS ∸ bits)
                ×-dec fits-in? mv bits
                ×-dec (ov ≟ᶠ ((dv *ᶠ pow2-fr bits) +ᶠ mv))))))
  where
    mem = Witness.mem w
    to : _ → holds w (reconstitute out d m bits)
    to (dv , ld , mv , lm , ov , lo , fd , fm , eq) =
      dv , mv , ov , ld , lm , lo , fd , fm , eq
    from : holds w (reconstitute out d m bits) → _
    from (dv , mv , ov , ld , lm , lo , fd , fm , eq) =
      dv , ld , mv , lm , ov , lo , fd , fm , eq

holds? w (is-zero out a) = map′ to from
  (dec-↦ mem a _
    (λ av → dec-↦ mem out _
      (λ ov → ov ≟ᶠ from-bool (av ≡ᶠ? 0ᶠ))))
  where
    mem = Witness.mem w
    to : _ → holds w (is-zero out a)
    to (av , la , ov , lo , eq) = av , ov , la , lo , eq
    from : holds w (is-zero out a) → _
    from (av , ov , la , lo , eq) = av , la , ov , lo , eq

holds? w (test-eq out a b) = map′ to from
  (dec-↦ mem a _
    (λ av → dec-↦ mem b _
      (λ bv → dec-↦ mem out _
        (λ ov → ov ≟ᶠ from-bool (av ≡ᶠ? bv)))))
  where
    mem = Witness.mem w
    to : _ → holds w (test-eq out a b)
    to (av , la , bv , lb , ov , lo , eq) = av , bv , ov , la , lb , lo , eq
    from : holds w (test-eq out a b) → _
    from (av , bv , ov , la , lb , lo , eq) = av , la , bv , lb , ov , lo , eq

holds? w (less-than out a b bits) = map′ to from
  (dec-↦ mem a _
    (λ av → dec-↦ mem b _
      (λ bv → dec-↦ mem out _
        (λ ov → fits-in? av (lt-bits bits)
                ×-dec fits-in? bv (lt-bits bits)
                ×-dec (ov ≟ᶠ from-bool
                  (bits-lt (take (lt-bits bits) (to-le-bits av))
                           (take (lt-bits bits) (to-le-bits bv))))))))
  where
    mem = Witness.mem w
    to : _ → holds w (less-than out a b bits)
    to (av , la , bv , lb , ov , lo , fa , fb , eq) =
      av , bv , ov , la , lb , lo , fa , fb , eq
    from : holds w (less-than out a b bits) → _
    from (av , bv , ov , la , lb , lo , fa , fb , eq) =
      av , la , bv , lb , ov , lo , fa , fb , eq

holds? w (poseidon out inputs) = map′ to from
  (dec-just (mem-lookups mem inputs) _
    (λ vs → dec-↦ mem out _
      (λ ov → ov ≟ᶠ transient-hash-fn vs)))
  where
    mem = Witness.mem w
    to : _ → holds w (poseidon out inputs)
    to (vs , lvs , ov , lo , eq) = vs , ov , lvs , lo , eq
    from : holds w (poseidon out inputs) → _
    from (vs , ov , lvs , lo , eq) = vs , lvs , ov , lo , eq

holds? w (sha256 h₁ h₂ alignment inputs) = map′ to from
  (dec-just (mem-lookups mem inputs) _
    (λ vs → dec-↦ mem h₁ _
      (λ v1 → dec-↦ mem h₂ _
        (λ v2 → ×-≡-dec _≟ᶠ_ _≟ᶠ_
                  (persistent-hash-fn alignment vs) (v1 , v2)))))
  where
    mem = Witness.mem w
    to : _ → holds w (sha256 h₁ h₂ alignment inputs)
    to (vs , lvs , v1 , l1 , v2 , l2 , eq) = vs , v1 , v2 , lvs , l1 , l2 , eq
    from : holds w (sha256 h₁ h₂ alignment inputs) → _
    from (vs , v1 , v2 , lvs , l1 , l2 , eq) = vs , lvs , v1 , l1 , v2 , l2 , eq

holds? w (ec-add c-x c-y a-x a-y b-x b-y) = map′ to from
  (dec-↦ mem a-x _
    (λ ax → dec-↦ mem a-y _
      (λ ay → dec-↦ mem b-x _
        (λ bx → dec-↦ mem b-y _
          (λ by → dec-↦ mem c-x _
            (λ cx → dec-↦ mem c-y _
              (λ cy → maybe-≡-dec (×-≡-dec _≟ᶠ_ _≟ᶠ_)
                        (ec-add-pts ax ay bx by) (just (cx , cy)))))))))
  where
    mem = Witness.mem w
    to : _ → holds w (ec-add c-x c-y a-x a-y b-x b-y)
    to (ax , lax , ay , lay , bx , lbx , by , lby , cx , lcx , cy , lcy , eq) =
      ax , ay , bx , by , cx , cy , lax , lay , lbx , lby , lcx , lcy , eq
    from : holds w (ec-add c-x c-y a-x a-y b-x b-y) → _
    from (ax , ay , bx , by , cx , cy , lax , lay , lbx , lby , lcx , lcy , eq) =
      ax , lax , ay , lay , bx , lbx , by , lby , cx , lcx , cy , lcy , eq

holds? w (ec-mul c-x c-y a-x a-y scalar) = map′ to from
  (dec-↦ mem a-x _
    (λ ax → dec-↦ mem a-y _
      (λ ay → dec-↦ mem scalar _
        (λ sc → dec-↦ mem c-x _
          (λ cx → dec-↦ mem c-y _
            (λ cy → maybe-≡-dec (×-≡-dec _≟ᶠ_ _≟ᶠ_)
                      (ec-mul-pt ax ay sc) (just (cx , cy))))))))
  where
    mem = Witness.mem w
    to : _ → holds w (ec-mul c-x c-y a-x a-y scalar)
    to (ax , lax , ay , lay , sc , ls , cx , lcx , cy , lcy , eq) =
      ax , ay , sc , cx , cy , lax , lay , ls , lcx , lcy , eq
    from : holds w (ec-mul c-x c-y a-x a-y scalar) → _
    from (ax , ay , sc , cx , cy , lax , lay , ls , lcx , lcy , eq) =
      ax , lax , ay , lay , sc , ls , cx , lcx , cy , lcy , eq

holds? w (ec-gen c-x c-y scalar) = map′ to from
  (dec-↦ mem scalar _
    (λ sc → dec-↦ mem c-x _
      (λ cx → dec-↦ mem c-y _
        (λ cy → ×-≡-dec _≟ᶠ_ _≟ᶠ_ (ec-mul-gen sc) (cx , cy)))))
  where
    mem = Witness.mem w
    to : _ → holds w (ec-gen c-x c-y scalar)
    to (sc , ls , cx , lcx , cy , lcy , eq) = sc , cx , cy , ls , lcx , lcy , eq
    from : holds w (ec-gen c-x c-y scalar) → _
    from (sc , cx , cy , ls , lcx , lcy , eq) = sc , ls , cx , lcx , cy , lcy , eq

holds? w (h2c c-x c-y inputs) = map′ to from
  (dec-just (mem-lookups mem inputs) _
    (λ vs → dec-↦ mem c-x _
      (λ cx → dec-↦ mem c-y _
        (λ cy → ×-≡-dec _≟ᶠ_ _≟ᶠ_ (hash-to-curve-fn vs) (cx , cy)))))
  where
    mem = Witness.mem w
    to : _ → holds w (h2c c-x c-y inputs)
    to (vs , lvs , cx , lcx , cy , lcy , eq) = vs , cx , cy , lvs , lcx , lcy , eq
    from : holds w (h2c c-x c-y inputs) → _
    from (vs , cx , cy , lvs , lcx , lcy , eq) = vs , lvs , cx , lcx , cy , lcy , eq

holds? w (bind entry widx) = map′ to from
  (dec-↦ (Witness.mem w) widx _
    (λ wv → dec-just (pi-lookup (Witness.pis w) entry) _
      (λ pv → pv ≟ᶠ wv)))
  where
    to : _ → holds w (bind entry widx)
    to (wv , lw , pv , lp , eq) = wv , pv , lw , lp , eq
    from : holds w (bind entry widx) → _
    from (wv , pv , lw , lp , eq) = wv , lw , pv , lp , eq

holds? w (guard-disj out i) = map′ to from
  (dec-↦ mem out _
    (λ ov → dec-↦ mem i _
      (λ iv → is-bit? iv ×-dec ((ov ≟ᶠ 0ᶠ) ⊎-dec (iv ≟ᶠ 1ᶠ)))))
  where
    mem = Witness.mem w
    to : _ → holds w (guard-disj out i)
    to (ov , lo , iv , li , ib , d) = ov , iv , lo , li , ib , d
    from : holds w (guard-disj out i) → _
    from (ov , iv , lo , li , ib , d) = ov , lo , iv , li , ib , d

holds? w (comm inputs outputs) = map′ to from
  (dec-just (mem-lookups mem inputs) _
    (λ ivs → dec-just (mem-lookups mem outputs) _
      (λ ovs → dec-just (Witness.comm-rand w) _
        (λ rv → dec-just (pi-lookup (Witness.pis w) 1) _
          (λ pv → pv ≟ᶠ transient-commit (ivs ++ ovs) rv)))))
  where
    mem = Witness.mem w
    to : _ → holds w (comm inputs outputs)
    to (ivs , li , ovs , lo , rv , lr , pv , lp , eq) =
      ivs , ovs , rv , pv , li , lo , lr , lp , eq
    from : holds w (comm inputs outputs) → _
    from (ivs , ovs , rv , pv , li , lo , lr , lp , eq) =
      ivs , li , ovs , lo , rv , lr , pv , lp , eq

------------------------------------------------------------------------
-- Polynomial-gate bridges
--
-- The arithmetic instructions (add, mul, neg, copy, constrain_eq,
-- load_imm) lower to equality gates `l ≑ r`.  These lemmas relate
-- `holds w (l ≑ r)` to the field-equation shape on the looked-up wire
-- values that the per-instruction faithfulness proofs reason with.
------------------------------------------------------------------------

-- Inversion for a binary-operator expression: if `l ⊕ r` evaluates,
-- both operands evaluate and the result is their sum.
eval-⊕ : ∀ mem l r {v} → eval mem (l ⊕ r) ≡ just v
  → ∃-syntax (λ x → ∃-syntax (λ y →
      (eval mem l ≡ just x) × (eval mem r ≡ just y) × (v ≡ x +ᶠ y)))
eval-⊕ mem l r ev with eval mem l
... | nothing = case ev of λ ()
... | just x with eval mem r
...   | nothing = case ev of λ ()
...   | just y = x , y , refl , refl , sym (just-injective ev)

-- Likewise for products and negations.
eval-⊗ : ∀ mem l r {v} → eval mem (l ⊗ r) ≡ just v
  → ∃-syntax (λ x → ∃-syntax (λ y →
      (eval mem l ≡ just x) × (eval mem r ≡ just y) × (v ≡ x *ᶠ y)))
eval-⊗ mem l r ev with eval mem l
... | nothing = case ev of λ ()
... | just x with eval mem r
...   | nothing = case ev of λ ()
...   | just y = x , y , refl , refl , sym (just-injective ev)

eval-⊝ : ∀ mem e {v} → eval mem (⊝ e) ≡ just v
  → ∃-syntax (λ x → (eval mem e ≡ just x) × (v ≡ -ᶠ x))
eval-⊝ mem e ev with eval mem e
... | nothing = case ev of λ ()
... | just x = x , refl , sym (just-injective ev)

-- An equality gate holds exactly when both sides evaluate to a common
-- value.  Forward: from a shared value; inverse: to the shared value.
-- The memory list is an *explicit* argument: every occurrence of it sits
-- under `mem-lookup` (not injective) or `eval`/`holds` (which reduce it
-- away), so it cannot be recovered by unification and must be supplied.
-- `eval mem (l ⊖ r) ≡ just 0ᶠ` is precisely `holds w (l ≑ r)` for a
-- witness `w` with `Witness.mem w = mem`.
≑-fwd : ∀ mem (l r : Expr) {x}
  → eval mem l ≡ just x → eval mem r ≡ just x → eval mem (l ⊖ r) ≡ just 0ᶠ
≑-fwd _ l r {x} el er rewrite el | er = cong just (+-inv-r x)

≑-inv : ∀ mem (l r : Expr) → eval mem (l ⊖ r) ≡ just 0ᶠ
  → ∃-syntax (λ x → ∃-syntax (λ y →
      (eval mem l ≡ just x) × (eval mem r ≡ just y) × (x ≡ y)))
≑-inv mem l r h
  with x , z , el , e⊝ , 0≡x+z ← eval-⊕ mem l (⊝ r) h
  with y , er , z≡-y          ← eval-⊝ mem r e⊝
  = x , y , el , er
  , +ᶠ-cancel (trans (cong (x +ᶠ_) (sym z≡-y)) (sym 0≡x+z))

-- Output-gate bridges.  An instruction whose result wire `out` is
-- constrained to a small expression over input wires lowers to a single
-- equality gate.  Each lemma exposes the wire-lookups and the field
-- equation the per-instruction faithfulness proofs build and destruct.
-- The memory list is explicit for the reason given above.

-- out = ⟦a⟧ + ⟦b⟧
≑⊕-fwd : ∀ mem {out a b av bv}
  → (mem at a ↦ av) → (mem at b ↦ bv) → (mem at out ↦ (av +ᶠ bv))
  → eval mem (wire out ⊖ (wire a ⊕ wire b)) ≡ just 0ᶠ
≑⊕-fwd mem {out} {a} {b} {av} {bv} la lb lo =
  ≑-fwd mem (wire out) (wire a ⊕ wire b) lo e
  where e : eval mem (wire a ⊕ wire b) ≡ just (av +ᶠ bv)
        e rewrite la | lb = refl

≑⊕-inv : ∀ mem {out a b} → eval mem (wire out ⊖ (wire a ⊕ wire b)) ≡ just 0ᶠ
  → ∃-syntax (λ av → ∃-syntax (λ bv → ∃-syntax (λ ov →
      (mem at a ↦ av) × (mem at b ↦ bv)
    × (mem at out ↦ ov) × (ov ≡ av +ᶠ bv))))
≑⊕-inv mem {out} {a} {b} h
  with ov , _ , lo , er , ov≡y ← ≑-inv mem (wire out) (wire a ⊕ wire b) h
  with av , bv , la , lb , y≡sum ← eval-⊕ mem (wire a) (wire b) er
  = av , bv , ov , la , lb , lo , trans ov≡y y≡sum

-- out = ⟦a⟧ · ⟦b⟧
≑⊗-fwd : ∀ mem {out a b av bv}
  → (mem at a ↦ av) → (mem at b ↦ bv) → (mem at out ↦ (av *ᶠ bv))
  → eval mem (wire out ⊖ (wire a ⊗ wire b)) ≡ just 0ᶠ
≑⊗-fwd mem {out} {a} {b} {av} {bv} la lb lo =
  ≑-fwd mem (wire out) (wire a ⊗ wire b) lo e
  where e : eval mem (wire a ⊗ wire b) ≡ just (av *ᶠ bv)
        e rewrite la | lb = refl

≑⊗-inv : ∀ mem {out a b} → eval mem (wire out ⊖ (wire a ⊗ wire b)) ≡ just 0ᶠ
  → ∃-syntax (λ av → ∃-syntax (λ bv → ∃-syntax (λ ov →
      (mem at a ↦ av) × (mem at b ↦ bv)
    × (mem at out ↦ ov) × (ov ≡ av *ᶠ bv))))
≑⊗-inv mem {out} {a} {b} h
  with ov , _ , lo , er , ov≡y ← ≑-inv mem (wire out) (wire a ⊗ wire b) h
  with av , bv , la , lb , y≡prod ← eval-⊗ mem (wire a) (wire b) er
  = av , bv , ov , la , lb , lo , trans ov≡y y≡prod

-- out = − ⟦a⟧
≑⊝-fwd : ∀ mem {out a av}
  → (mem at a ↦ av) → (mem at out ↦ (-ᶠ av))
  → eval mem (wire out ⊖ ⊝ wire a) ≡ just 0ᶠ
≑⊝-fwd mem {out} {a} {av} la lo =
  ≑-fwd mem (wire out) (⊝ wire a) lo e
  where e : eval mem (⊝ wire a) ≡ just (-ᶠ av)
        e rewrite la = refl

≑⊝-inv : ∀ mem {out a} → eval mem (wire out ⊖ ⊝ wire a) ≡ just 0ᶠ
  → ∃-syntax (λ av → ∃-syntax (λ ov →
      (mem at a ↦ av) × (mem at out ↦ ov) × (ov ≡ -ᶠ av)))
≑⊝-inv mem {out} {a} h
  with ov , _ , lo , er , ov≡y ← ≑-inv mem (wire out) (⊝ wire a) h
  with av , la , y≡neg ← eval-⊝ mem (wire a) er
  = av , ov , la , lo , trans ov≡y y≡neg

-- out = ⟦v⟧  (copy)
≑wire-fwd : ∀ mem {out v vv}
  → (mem at v ↦ vv) → (mem at out ↦ vv)
  → eval mem (wire out ⊖ wire v) ≡ just 0ᶠ
≑wire-fwd mem {out} {v} lv lo = ≑-fwd mem (wire out) (wire v) lo lv

≑wire-inv : ∀ mem {out v} → eval mem (wire out ⊖ wire v) ≡ just 0ᶠ
  → ∃-syntax (λ vv → ∃-syntax (λ ov →
      (mem at v ↦ vv) × (mem at out ↦ ov) × (ov ≡ vv)))
≑wire-inv mem {out} {v} h
  with ov , vv , lo , lv , ov≡vv ← ≑-inv mem (wire out) (wire v) h
  = vv , ov , lv , lo , ov≡vv

-- out = k  (load_imm)
≑con-fwd : ∀ mem {out k}
  → (mem at out ↦ k) → eval mem (wire out ⊖ con k) ≡ just 0ᶠ
≑con-fwd mem {out} {k} lo = ≑-fwd mem (wire out) (con k) lo refl

≑con-inv : ∀ mem {out k} → eval mem (wire out ⊖ con k) ≡ just 0ᶠ
  → ∃-syntax (λ ov → (mem at out ↦ ov) × (ov ≡ k))
≑con-inv mem {out} {k} h
  with ov , _ , lo , ek , ov≡k ← ≑-inv mem (wire out) (con k) h
  = ov , lo , trans ov≡k (sym (just-injective ek))

-- ⟦a⟧ = ⟦b⟧  (constrain_eq)
≑vars-fwd : ∀ mem {a b va}
  → (mem at a ↦ va) → (mem at b ↦ va)
  → eval mem (wire a ⊖ wire b) ≡ just 0ᶠ
≑vars-fwd mem {a} {b} la lb = ≑-fwd mem (wire a) (wire b) la lb

≑vars-inv : ∀ mem {a b} → eval mem (wire a ⊖ wire b) ≡ just 0ᶠ
  → ∃-syntax (λ av → ∃-syntax (λ bv →
      (mem at a ↦ av) × (mem at b ↦ bv) × (av ≡ bv)))
≑vars-inv mem {a} {b} h
  with av , bv , la , lb , av≡bv ← ≑-inv mem (wire a) (wire b) h
  = av , bv , la , lb , av≡bv

-- Satisfaction of a constraint list: `holds w` for every constraint (`All`).
satisfies-constraints : List Constraint → Witness → Set
satisfies-constraints cs w = All (holds w) cs

-- "If the circuit uses a comm-commitment, the witness must carry the
-- randomness; otherwise the witness's comm-rand is unconstrained
-- (the constraint that would consume it is not emitted, and the prover
-- may carry a spurious randomness that is simply ignored)."
Maybe-shape : Bool → Maybe Fr → Set
Maybe-shape true  (just _) = ⊤
Maybe-shape true  nothing  = ⊥
Maybe-shape false _        = ⊤

-- Top-level satisfaction.  An assignment satisfies a circuit when:
--   • all constraints hold;
--   • the witness has comm-rand iff the circuit has-comm;
--   • the memory vector has exactly the allocated wire count;
--   • the PI vector has the structural length recorded in the circuit
--     (= preamble + #declared-PIs).
record satisfies (c : Circuit) (w : Witness) : Set where
  constructor mk-sat
  field
    pi-length     : length (Witness.pis w) ≡ Circuit.pi-len c
    mem-length    : length (Witness.mem w) ≡ Circuit.nr-wires c
    rand-shape    : Maybe-shape (Circuit.has-comm c) (Witness.comm-rand w)
    constraint-ok : satisfies-constraints (Circuit.constraints c) w
