{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.Semantics (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.FieldProperties ⋯
open import zkir-v2.Syntax ⋯

open import Data.Bool    using (Bool; true; false; if_then_else_; _∧_; T)
import Data.Bool as Bool
open import Data.List    using (List; []; _∷_; _++_; length; drop; take)
open import Data.Maybe   using (Maybe; nothing; just; _>>=_)
open import Data.Product using (_×_; _,_; ∃)
open import Data.Unit     using (⊤; tt)
open import Data.List.Relation.Unary.All using (All)
open import Relation.Binary.PropositionalEquality using (_≡_)
open import Relation.Nullary.Decidable using (does)
open import Data.Nat        using (ℕ; zero; suc; _∸_; _≡ᵇ_; _+_; _*_;
                                   _<_; _≤_; _<?_; _≤?_)
open import Data.Nat.DivMod using (_/_)

------------------------------------------------------------------------
-- Field and curve operations come from the `Assumptions` parameter; the
-- bit/boolean helpers (`to-bool`, `fits-in?`/`Fits-in`, `bits-lt`, …)
-- are *derived* from them in `FieldProperties`, not assumed.
------------------------------------------------------------------------

------------------------------------------------------------------------
-- Utilities
------------------------------------------------------------------------

-- `from-bool` is exported because `R-instr` constructor types
-- (`r-test-eq`, `r-not`, `r-less-than`, …) refer to it; downstream
-- modules need to refer to the *same* function in their proofs.
from-bool : Bool → Fr
from-bool false = 0ᶠ
from-bool true  = 1ᶠ

------------------------------------------------------------------------
-- Bit-bound constants
--
-- The byte width of a canonically-stored field element, derived from
-- `FR-BITS` exactly as in the Rust runtime (`transient_crypto::curve`):
--   FR_BYTES        = ⌈FR_BITS / 8⌉            (= FR_BITS.div_ceil(8))
--   FR_BYTES_STORED = FR_BYTES − 1
-- `FR-STORED-BITS = FR_BYTES_STORED · 8` is the bit ceiling that
-- `div-mod-power-of-two` and `reconstitute-field` must not exceed
-- (248 bits for BLS12-381).  No new assumption is needed: the bound is
-- a function of the existing `FR-BITS`.
------------------------------------------------------------------------

FR-BYTES : ℕ
FR-BYTES = (FR-BITS + 7) / 8

FR-BYTES-STORED : ℕ
FR-BYTES-STORED = FR-BYTES ∸ 1

FR-STORED-BITS : ℕ
FR-STORED-BITS = FR-BYTES-STORED * 8

private
  is-empty : {A : Set} → List A → Bool
  is-empty []      = true
  is-empty (_ ∷ _) = false

-- Element-wise Fr equality on lists.  Exposed (not private) because
-- the backward dispatcher D1 for `pi-skip` references it inside the
-- operational side-data ADT `op-side-data` (CircuitProof.agda), for
-- the same reason `from-bool` above is exposed.
_≡ᶠ-list?_ : List Fr → List Fr → Bool
[]       ≡ᶠ-list? []       = true
(x ∷ xs) ≡ᶠ-list? (y ∷ ys) = x ≡ᶠ? y ∧ xs ≡ᶠ-list? ys
_        ≡ᶠ-list? _        = false

mem-lookup : List Fr → Index → Maybe Fr
mem-lookup []       _       = nothing
mem-lookup (x ∷ _)  zero    = just x
mem-lookup (_ ∷ xs) (suc n) = mem-lookup xs n

mem-lookups : List Fr → List Index → Maybe (List Fr)
mem-lookups _   []       = just []
mem-lookups mem (i ∷ is) =
  mem-lookup mem i  >>= λ v  →
  mem-lookups mem is >>= λ vs →
  just (v ∷ vs)

------------------------------------------------------------------------
-- Proof preimage
-- Mirrors ProofPreimage in transient_crypto::proofs
------------------------------------------------------------------------

record ProofPreimage : Set where
  constructor mk-preimage
  field
    inputs                 : List Fr
    binding-input          : Fr
    comm-commitment        : Maybe (Fr × Fr)  -- (commitment, randomness)
    pub-transcript-inputs  : List Fr
    pub-transcript-outputs : List Fr
    priv-transcript        : List Fr

------------------------------------------------------------------------
-- Execution state
------------------------------------------------------------------------

record Preprocessed : Set where
  constructor mk-state
  field
    memory      : List Fr
    pis         : List Fr         -- public inputs: binding-input first, then
                                  --   DeclarePubInput values
    pi-skips    : List (Maybe ℕ)  -- nothing = active group, just n = skipped
    pub-in-idx  : ℕ               -- count of DeclarePubInput processed
    pub-out-rem : List Fr         -- remaining pub-transcript-outputs
    priv-rem    : List Fr         -- remaining priv-transcript
    outputs     : List Fr         -- values written by Output instructions

------------------------------------------------------------------------
-- Initial state from preimage
-- Fails if do-communications-commitment is set but no commitment is provided.
------------------------------------------------------------------------

-- Spec §4.2:  fail if |inputs| ≠ num-inputs (WF1 enforcement)
-- or if do-comm is set but the preimage carries no commitment.
init-state : IrSource → ProofPreimage → Maybe Preprocessed
init-state src pre
  with length (ProofPreimage.inputs pre) ≡ᵇ IrSource.num-inputs src
     | IrSource.do-communications-commitment src
     | ProofPreimage.comm-commitment pre
... | false | _     | _            = nothing
... | true  | false | _            = just (mk-state
      (ProofPreimage.inputs pre)
      (ProofPreimage.binding-input pre ∷ [])
      [] 0
      (ProofPreimage.pub-transcript-outputs pre)
      (ProofPreimage.priv-transcript pre)
      [])
... | true  | true  | just (c , _) = just (mk-state
      (ProofPreimage.inputs pre)
      (ProofPreimage.binding-input pre ∷ c ∷ [])
      [] 0
      (ProofPreimage.pub-transcript-outputs pre)
      (ProofPreimage.priv-transcript pre)
      [])
... | true  | true  | nothing      = nothing

------------------------------------------------------------------------
-- State helpers
------------------------------------------------------------------------

push-mem : Preprocessed → Fr → Preprocessed
push-mem s v = record s { memory = Preprocessed.memory s ++ (v ∷ []) }

push-mem2 : Preprocessed → Fr → Fr → Preprocessed
push-mem2 s v₁ v₂ = record s { memory = Preprocessed.memory s ++ (v₁ ∷ v₂ ∷ []) }

consume-pub-out : Preprocessed → Maybe (Fr × Preprocessed)
consume-pub-out s with Preprocessed.pub-out-rem s
... | []       = nothing
... | v ∷ rest = just (v , record s { pub-out-rem = rest })

consume-priv : Preprocessed → Maybe (Fr × Preprocessed)
consume-priv s with Preprocessed.priv-rem s
... | []       = nothing
... | v ∷ rest = just (v , record s { priv-rem = rest })

private
  push-pi : Preprocessed → Fr → Preprocessed
  push-pi s v = record s
    { pis        = Preprocessed.pis s ++ (v ∷ [])
    ; pub-in-idx = suc (Preprocessed.pub-in-idx s) }

  push-skip : Preprocessed → Maybe ℕ → Preprocessed
  push-skip s sk = record s { pi-skips = Preprocessed.pi-skips s ++ (sk ∷ []) }

  push-output : Preprocessed → Fr → Preprocessed
  push-output s v = record s { outputs = Preprocessed.outputs s ++ (v ∷ []) }

------------------------------------------------------------------------
-- Guard evaluation
-- nothing  → always active (true)
-- just idx → evaluate memory[idx] as a boolean
------------------------------------------------------------------------

eval-guard : List Fr → Maybe Index → Maybe Bool
eval-guard _   nothing    = just true
eval-guard mem (just idx) = mem-lookup mem idx >>= to-bool

------------------------------------------------------------------------
-- PiSkip
--
-- Active guard (true): validate the last `count` declared pub inputs
--   match the corresponding entries in the pub-transcript-inputs, then
--   record nothing (group is live).
-- Inactive guard (false): decrement pub-in-idx by count and record
--   just count (group is skipped).
------------------------------------------------------------------------

private
  preprocess-pi-skip : ProofPreimage → Preprocessed → Maybe Index → ℕ → Maybe Preprocessed
  preprocess-pi-skip pre s guard count =
    eval-guard (Preprocessed.memory s) guard >>= λ active →
    if active then validate else skip
    where
      validate : Maybe Preprocessed
      validate =
        let
          pis      = Preprocessed.pis s
          recent   = drop (length pis ∸ count) pis
          start    = Preprocessed.pub-in-idx s ∸ count
          expected = take count (drop start (ProofPreimage.pub-transcript-inputs pre))
        in
        if recent ≡ᶠ-list? expected
          then just (push-skip s nothing)
          else nothing

      skip : Maybe Preprocessed
      skip = just (push-skip
        (record s { pub-in-idx = Preprocessed.pub-in-idx s ∸ count })
        (just count))

------------------------------------------------------------------------
-- Single-instruction preprocessing
-- Returns nothing on out-of-bounds access, UB, or constraint failure.
------------------------------------------------------------------------

preprocess-instr : ProofPreimage → Preprocessed → Instruction → Maybe Preprocessed

preprocess-instr _ s (assert cond) =
  mem-lookup (Preprocessed.memory s) cond >>= to-bool >>= λ b →
  if b then just s else nothing

preprocess-instr _ s (cond-select bit a b) =
  let mem = Preprocessed.memory s in
  mem-lookup mem bit >>= to-bool >>= λ bv →
  mem-lookup mem a   >>= λ av   →
  mem-lookup mem b   >>= λ bv'  →
  just (push-mem s (if bv then av else bv'))

preprocess-instr _ s (constrain-bits var bits) =
  mem-lookup (Preprocessed.memory s) var >>= λ v →
  if does (bits <? FR-BITS) ∧ does (fits-in? v bits) then just s else nothing

preprocess-instr _ s (constrain-eq a b) =
  let mem = Preprocessed.memory s in
  mem-lookup mem a >>= λ av →
  mem-lookup mem b >>= λ bv →
  if av ≡ᶠ? bv then just s else nothing

preprocess-instr _ s (constrain-to-boolean var) =
  mem-lookup (Preprocessed.memory s) var >>= to-bool >>= λ _ →
  just s

preprocess-instr _ s (copy var) =
  mem-lookup (Preprocessed.memory s) var >>= λ v →
  just (push-mem s v)

preprocess-instr _ s (declare-pub-input var) =
  mem-lookup (Preprocessed.memory s) var >>= λ v →
  just (push-pi s v)

preprocess-instr pre s (pi-skip guard count) =
  preprocess-pi-skip pre s guard count

preprocess-instr _ s (ec-add a_x a_y b_x b_y) =
  let mem = Preprocessed.memory s in
  mem-lookup mem a_x >>= λ ax →
  mem-lookup mem a_y >>= λ ay →
  mem-lookup mem b_x >>= λ bx →
  mem-lookup mem b_y >>= λ by →
  ec-add-pts ax ay bx by >>= λ { (cx , cy) →
  just (push-mem2 s cx cy) }

preprocess-instr _ s (ec-mul a_x a_y scalar) =
  let mem = Preprocessed.memory s in
  mem-lookup mem a_x    >>= λ ax →
  mem-lookup mem a_y    >>= λ ay →
  mem-lookup mem scalar >>= λ sc →
  ec-mul-pt ax ay sc >>= λ { (cx , cy) →
  just (push-mem2 s cx cy) }

preprocess-instr _ s (ec-mul-generator scalar) =
  mem-lookup (Preprocessed.memory s) scalar >>= λ sc →
  let (cx , cy) = ec-mul-gen sc in
  just (push-mem2 s cx cy)

preprocess-instr _ s (hash-to-curve inputs) =
  mem-lookups (Preprocessed.memory s) inputs >>= λ vs →
  let (cx , cy) = hash-to-curve-fn vs in
  just (push-mem2 s cx cy)

preprocess-instr _ s (load-imm imm) =
  just (push-mem s imm)

preprocess-instr _ s (div-mod-power-of-two var bits) =
  mem-lookup (Preprocessed.memory s) var >>= λ v →
  let
    all-bits = to-le-bits v
    divisor  = from-le-bits (drop bits all-bits)
    modulus  = from-le-bits (take bits all-bits)
  in
  if does (bits ≤? FR-STORED-BITS)
    then just (push-mem (push-mem s divisor) modulus)
    else nothing

preprocess-instr _ s (persistent-hash alignment inputs) =
  mem-lookups (Preprocessed.memory s) inputs >>= λ vs →
  let (h₁ , h₂) = persistent-hash-fn alignment vs in
  just (push-mem2 s h₁ h₂)

preprocess-instr _ s (reconstitute-field divisor modulus bits) =
  let mem = Preprocessed.memory s in
  mem-lookup mem divisor >>= λ dv →
  mem-lookup mem modulus >>= λ mv →
  let
    mv-bits  = take bits             (to-le-bits mv)
    dv-bits  = take (FR-BITS ∸ bits) (to-le-bits dv)
    all-bits = mv-bits ++ dv-bits
  in
  if does (1 ≤? bits) ∧ does (bits ≤? FR-STORED-BITS) ∧
     does (fits-in? mv bits) ∧ does (fits-in? dv (FR-BITS ∸ bits)) ∧ does (bitsInField? all-bits)
    then just (push-mem s (from-le-bits all-bits))
    else nothing

preprocess-instr _ s (output var) =
  mem-lookup (Preprocessed.memory s) var >>= λ v →
  just (push-output s v)

preprocess-instr _ s (transient-hash inputs) =
  mem-lookups (Preprocessed.memory s) inputs >>= λ vs →
  just (push-mem s (transient-hash-fn vs))

preprocess-instr _ s (test-eq a b) =
  let mem = Preprocessed.memory s in
  mem-lookup mem a >>= λ av →
  mem-lookup mem b >>= λ bv →
  just (push-mem s (from-bool (av ≡ᶠ? bv)))

preprocess-instr _ s (add a b) =
  let mem = Preprocessed.memory s in
  mem-lookup mem a >>= λ av →
  mem-lookup mem b >>= λ bv →
  just (push-mem s (av +ᶠ bv))

preprocess-instr _ s (mul a b) =
  let mem = Preprocessed.memory s in
  mem-lookup mem a >>= λ av →
  mem-lookup mem b >>= λ bv →
  just (push-mem s (av *ᶠ bv))

preprocess-instr _ s (neg a) =
  mem-lookup (Preprocessed.memory s) a >>= λ av →
  just (push-mem s (-ᶠ av))

preprocess-instr _ s (not a) =
  mem-lookup (Preprocessed.memory s) a >>= to-bool >>= λ b →
  just (push-mem s (from-bool (Bool.not b)))

preprocess-instr _ s (less-than a b bits) =
  let mem = Preprocessed.memory s in
  mem-lookup mem a >>= λ av →
  mem-lookup mem b >>= λ bv →
  if does (bits <? FR-BITS) ∧ does (fits-in? av bits) ∧ does (fits-in? bv bits)
    then just (push-mem s (from-bool
           (bits-lt (take bits (to-le-bits av))
                    (take bits (to-le-bits bv)))))
    else nothing

preprocess-instr pre s (public-input guard) =
  eval-guard (Preprocessed.memory s) guard >>= λ active →
  if Bool.not active
    then just (push-mem s 0ᶠ)
    else consume-pub-out s >>= λ { (v , s') → just (push-mem s' v) }

preprocess-instr pre s (private-input guard) =
  eval-guard (Preprocessed.memory s) guard >>= λ active →
  if Bool.not active
    then just (push-mem s 0ᶠ)
    else consume-priv s >>= λ { (v , s') → just (push-mem s' v) }

------------------------------------------------------------------------
-- Circuit preprocessing: fold preprocess-instr over the instruction list
------------------------------------------------------------------------

preprocess-instrs : ProofPreimage → Preprocessed → List Instruction → Maybe Preprocessed
preprocess-instrs _   s []       = just s
preprocess-instrs pre s (i ∷ is) =
  preprocess-instr pre s i >>= λ s' →
  preprocess-instrs pre s' is

------------------------------------------------------------------------
-- Post-preprocessing validation
------------------------------------------------------------------------

-- All three transcripts must be fully consumed.
transcripts-consumed : ProofPreimage → Preprocessed → Bool
transcripts-consumed pre s =
  (length (ProofPreimage.pub-transcript-inputs pre) ≡ᵇ Preprocessed.pub-in-idx s)
  ∧ is-empty (Preprocessed.pub-out-rem s)
  ∧ is-empty (Preprocessed.priv-rem s)

-- If do-communications-commitment is set, verify the commitment.
comm-ok : IrSource → ProofPreimage → Preprocessed → Bool
comm-ok src pre s
  with IrSource.do-communications-commitment src
     | ProofPreimage.comm-commitment pre
... | false | _            = true
... | true  | nothing      = false
... | true  | just (c , r) =
  c ≡ᶠ? transient-commit (ProofPreimage.inputs pre ++ Preprocessed.outputs s) r

------------------------------------------------------------------------
-- Top-level: preprocess the circuit and validate
------------------------------------------------------------------------

preprocess : IrSource → ProofPreimage → Maybe Preprocessed
preprocess src pre =
  init-state src pre                           >>= λ s  →
  preprocess-instrs pre s (IrSource.instructions src) >>= λ s' →
  if transcripts-consumed pre s' ∧ comm-ok src pre s'
    then just s'
    else nothing

------------------------------------------------------------------------
-- Relational semantics
--
-- R-instr pre s i s' holds when instruction i can validly transition
-- from state s to state s', given preimage pre.  Defined independently
-- of preprocess-instr; faithfulness connects the two.
------------------------------------------------------------------------

data R-instr (pre : ProofPreimage)
    : Preprocessed → Instruction → Preprocessed → Set where

  r-assert : ∀ {s cond}
    → (mem-lookup (Preprocessed.memory s) cond >>= to-bool) ≡ just true
    → R-instr pre s (assert cond) s

  r-cond-select : ∀ {s bit a b sel av bv}
    → (mem-lookup (Preprocessed.memory s) bit >>= to-bool) ≡ just sel
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → mem-lookup (Preprocessed.memory s) b ≡ just bv
    → R-instr pre s (cond-select bit a b) (push-mem s (if sel then av else bv))

  r-constrain-bits : ∀ {s var bits v}
    → mem-lookup (Preprocessed.memory s) var ≡ just v
    → bits < FR-BITS
    → Fits-in v bits
    → R-instr pre s (constrain-bits var bits) s

  r-constrain-eq : ∀ {s a b av bv}
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → mem-lookup (Preprocessed.memory s) b ≡ just bv
    → av ≡ bv
    → R-instr pre s (constrain-eq a b) s

  r-constrain-to-boolean : ∀ {s var b}
    → (mem-lookup (Preprocessed.memory s) var >>= to-bool) ≡ just b
    → R-instr pre s (constrain-to-boolean var) s

  r-copy : ∀ {s var v}
    → mem-lookup (Preprocessed.memory s) var ≡ just v
    → R-instr pre s (copy var) (push-mem s v)

  r-declare-pub-input : ∀ {s var v}
    → mem-lookup (Preprocessed.memory s) var ≡ just v
    → R-instr pre s (declare-pub-input var) (push-pi s v)

  r-pi-skip-active : ∀ {s guard count}
    → eval-guard (Preprocessed.memory s) guard ≡ just true
    → T (drop (length (Preprocessed.pis s) ∸ count) (Preprocessed.pis s)
         ≡ᶠ-list? take count (drop (Preprocessed.pub-in-idx s ∸ count)
                                    (ProofPreimage.pub-transcript-inputs pre)))
    → R-instr pre s (pi-skip guard count) (push-skip s nothing)

  r-pi-skip-inactive : ∀ {s guard count}
    → eval-guard (Preprocessed.memory s) guard ≡ just false
    → R-instr pre s (pi-skip guard count)
        (push-skip (record s { pub-in-idx = Preprocessed.pub-in-idx s ∸ count }) (just count))

  r-ec-add : ∀ {s a_x a_y b_x b_y ax ay bx by cx cy}
    → mem-lookup (Preprocessed.memory s) a_x ≡ just ax
    → mem-lookup (Preprocessed.memory s) a_y ≡ just ay
    → mem-lookup (Preprocessed.memory s) b_x ≡ just bx
    → mem-lookup (Preprocessed.memory s) b_y ≡ just by
    → ec-add-pts ax ay bx by ≡ just (cx , cy)
    → R-instr pre s (ec-add a_x a_y b_x b_y) (push-mem2 s cx cy)

  r-ec-mul : ∀ {s a_x a_y scalar ax ay sc cx cy}
    → mem-lookup (Preprocessed.memory s) a_x ≡ just ax
    → mem-lookup (Preprocessed.memory s) a_y ≡ just ay
    → mem-lookup (Preprocessed.memory s) scalar ≡ just sc
    → ec-mul-pt ax ay sc ≡ just (cx , cy)
    → R-instr pre s (ec-mul a_x a_y scalar) (push-mem2 s cx cy)

  r-ec-mul-generator : ∀ {s scalar sc cx cy}
    → mem-lookup (Preprocessed.memory s) scalar ≡ just sc
    → ec-mul-gen sc ≡ (cx , cy)
    → R-instr pre s (ec-mul-generator scalar) (push-mem2 s cx cy)

  r-hash-to-curve : ∀ {s inputs vs cx cy}
    → mem-lookups (Preprocessed.memory s) inputs ≡ just vs
    → hash-to-curve-fn vs ≡ (cx , cy)
    → R-instr pre s (hash-to-curve inputs) (push-mem2 s cx cy)

  r-load-imm : ∀ {s imm}
    → R-instr pre s (load-imm imm) (push-mem s imm)

  r-div-mod-power-of-two : ∀ {s var bits v}
    → mem-lookup (Preprocessed.memory s) var ≡ just v
    → bits ≤ FR-STORED-BITS
    → R-instr pre s (div-mod-power-of-two var bits)
        (push-mem (push-mem s (from-le-bits (drop bits (to-le-bits v))))
                  (from-le-bits (take bits (to-le-bits v))))

  r-reconstitute-field : ∀ {s divisor modulus bits dv mv}
    → mem-lookup (Preprocessed.memory s) divisor ≡ just dv
    → mem-lookup (Preprocessed.memory s) modulus ≡ just mv
    → 1 ≤ bits
    → bits ≤ FR-STORED-BITS
    → (Fits-in mv bits × Fits-in dv (FR-BITS ∸ bits) ×
       BitsInField (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv)))
    → R-instr pre s (reconstitute-field divisor modulus bits)
        (push-mem s (from-le-bits (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))))

  r-output : ∀ {s var v}
    → mem-lookup (Preprocessed.memory s) var ≡ just v
    → R-instr pre s (output var) (push-output s v)

  r-transient-hash : ∀ {s inputs vs}
    → mem-lookups (Preprocessed.memory s) inputs ≡ just vs
    → R-instr pre s (transient-hash inputs) (push-mem s (transient-hash-fn vs))

  r-persistent-hash : ∀ {s alignment inputs vs h₁ h₂}
    → mem-lookups (Preprocessed.memory s) inputs ≡ just vs
    → persistent-hash-fn alignment vs ≡ (h₁ , h₂)
    → R-instr pre s (persistent-hash alignment inputs) (push-mem2 s h₁ h₂)

  r-test-eq : ∀ {s a b av bv}
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → mem-lookup (Preprocessed.memory s) b ≡ just bv
    → R-instr pre s (test-eq a b) (push-mem s (from-bool (av ≡ᶠ? bv)))

  r-add : ∀ {s a b av bv}
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → mem-lookup (Preprocessed.memory s) b ≡ just bv
    → R-instr pre s (add a b) (push-mem s (av +ᶠ bv))

  r-mul : ∀ {s a b av bv}
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → mem-lookup (Preprocessed.memory s) b ≡ just bv
    → R-instr pre s (mul a b) (push-mem s (av *ᶠ bv))

  r-neg : ∀ {s a av}
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → R-instr pre s (neg a) (push-mem s (-ᶠ av))

  r-not : ∀ {s a b}
    → (mem-lookup (Preprocessed.memory s) a >>= to-bool) ≡ just b
    → R-instr pre s (not a) (push-mem s (from-bool (Bool.not b)))

  r-less-than : ∀ {s a b bits av bv}
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → mem-lookup (Preprocessed.memory s) b ≡ just bv
    → bits < FR-BITS
    → (Fits-in av bits × Fits-in bv bits)
    → R-instr pre s (less-than a b bits)
        (push-mem s (from-bool (bits-lt (take bits (to-le-bits av))
                                        (take bits (to-le-bits bv)))))

  r-public-input-inactive : ∀ {s guard}
    → eval-guard (Preprocessed.memory s) guard ≡ just false
    → R-instr pre s (public-input guard) (push-mem s 0ᶠ)

  r-public-input-active : ∀ {s guard v s₁}
    → eval-guard (Preprocessed.memory s) guard ≡ just true
    → consume-pub-out s ≡ just (v , s₁)
    → R-instr pre s (public-input guard) (push-mem s₁ v)

  r-private-input-inactive : ∀ {s guard}
    → eval-guard (Preprocessed.memory s) guard ≡ just false
    → R-instr pre s (private-input guard) (push-mem s 0ᶠ)

  r-private-input-active : ∀ {s guard v s₁}
    → eval-guard (Preprocessed.memory s) guard ≡ just true
    → consume-priv s ≡ just (v , s₁)
    → R-instr pre s (private-input guard) (push-mem s₁ v)

-- Relational semantics for a sequence of instructions.
data R-instrs (pre : ProofPreimage)
    : Preprocessed → List Instruction → Preprocessed → Set where
  r-done : ∀ {s} → R-instrs pre s [] s
  r-step : ∀ {s s₁ s' i is}
    → R-instr  pre s  i  s₁
    → R-instrs pre s₁ is s'
    → R-instrs pre s (i ∷ is) s'

------------------------------------------------------------------------
-- Bit-bound well-formedness (WF2, §3.3)
--
-- The per-instruction bit-count bounds the Rust runtime enforces.  Both
-- the functional `preprocess-instr` and the relational `R-instr` reject
-- a bit count outside these bounds, so WF2 is the residual hypothesis
-- under which `circuit-faithful` recovers `R` from circuit satisfaction.
------------------------------------------------------------------------

WF2-instr : Instruction → Set
WF2-instr (constrain-bits _ bits)       = bits < FR-BITS
WF2-instr (less-than _ _ bits)          = bits < FR-BITS
WF2-instr (div-mod-power-of-two _ bits) = bits ≤ FR-STORED-BITS
WF2-instr (reconstitute-field _ _ bits) = (1 ≤ bits) × (bits ≤ FR-STORED-BITS)
WF2-instr _                             = ⊤

WF2 : IrSource → Set
WF2 src = All WF2-instr (IrSource.instructions src)

-- Top-level relational semantics: pre and s satisfy circuit src.
R : IrSource → ProofPreimage → Preprocessed → Set
R src pre s =
  ∃ λ s₀ →
    init-state src pre ≡ just s₀ ×
    R-instrs pre s₀ (IrSource.instructions src) s ×
    T (transcripts-consumed pre s) ×
    T (comm-ok src pre s)
