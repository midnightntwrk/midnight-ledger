{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.CircuitFaithfulness (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.FieldProperties ⋯
open import zkir-v2.Encoding ⋯

------------------------------------------------------------------------
-- Per-instruction faithfulness
--
-- For each instruction, a forward faithfulness lemma, and — for every
-- instruction whose emitted constraints have inversion content — a
-- backward one:
--
--   fwd : R-instr pre s i s' ⇒ the constraints emitted by `circuit-instr`
--                              for `i` are satisfied by the canonical
--                              witness derived from s'.
--
--   bwd : constraints-of i satisfied by a witness whose mem has the
--         expected post-state shape ⇒ R-instr pre s i s'.
--
-- Instructions with no backward lemma here (`output`, `pi-skip`,
-- `public-input`, `private-input`, `div-mod-power-of-two`) are
-- dispatched directly to their operational rules by `CircuitProof`;
-- each absence is explained at the corresponding fwd lemma.
------------------------------------------------------------------------

open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
open import zkir-v2.Circuit ⋯
open import zkir-v2.SemanticsLemmas ⋯

open import Data.Bool      using (Bool; true; false; if_then_else_)
import Data.Bool as Bool
open import Data.List      using (List; []; _∷_; _++_; length; take; drop)
open import Data.List.Relation.Unary.All using (All)
  renaming ([] to []ᴬ; _∷_ to _∷ᴬ_)
open import Data.Maybe     using (Maybe; nothing; just; _>>=_)
open import Data.Maybe.Properties using (just-injective)
open import Data.Nat       using (ℕ; suc; zero; _+_; _∸_; _≤_; _<_)
open import Data.Product   using (_×_; _,_; ∃-syntax; proj₁; proj₂)
open import Data.Unit      using (⊤; tt)
open import Data.Sum       using (_⊎_; inj₁; inj₂)
open import Data.Empty     using (⊥-elim)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; cong₂; subst)
open import Relation.Nullary using (¬_)

------------------------------------------------------------------------
-- Axiomatic interface
--
-- The field-equality / `to-bool` reflection axioms, the BLS12-381
-- scalar-field equations, and the bit-decomposition / bit-arithmetic
-- facts that this module's per-instruction lemmas rest on are part of
-- the trust base.  They are fields of the `Assumptions` record
-- (`zkir-v2.Assumptions`) and come into scope via the module parameter.
------------------------------------------------------------------------

------------------------------------------------------------------------
-- Local helpers
--
-- The lookup toolkit these proofs run on lives in `SemanticsLemmas`
-- (opened above).  What remains here is specific to the backward
-- lemmas: multi-argument `cong`, bit-lookup decomposition, and the
-- `to-bool`/`is-bit` bridge.
------------------------------------------------------------------------

private

  -- Multi-argument `cong` helpers used by the cryptographic backward
  -- proofs (the chip primitives take 3 or 4 arguments).
  cong₃ : ∀ {A B C D : Set} (f : A → B → C → D)
          {a a' b b' c c'}
        → a ≡ a' → b ≡ b' → c ≡ c'
        → f a b c ≡ f a' b' c'
  cong₃ f refl refl refl = refl

  cong₄ : ∀ {A B C D E : Set} (f : A → B → C → D → E)
          {a a' b b' c c' d d'}
        → a ≡ a' → b ≡ b' → c ≡ c' → d ≡ d'
        → f a b c d ≡ f a' b' c' d'
  cong₄ f refl refl refl refl = refl

  -- Decompose a `>>=`-style bit lookup into the underlying field value
  -- plus the `to-bool` evidence on it.  Used wherever the operational
  -- rule's premise is in `mem-lookup … >>= to-bool` form (assert,
  -- cond-select's bit operand, constrain-to-boolean, not, public/
  -- private input guards).
  extract-bit-lookup : ∀ (mem : List Fr) b {sel}
    → (mem-lookup mem b >>= to-bool) ≡ just sel
    → ∃-syntax (λ bv →
        (mem-lookup mem b ≡ just bv) × (to-bool bv ≡ just sel))
  extract-bit-lookup mem b {sel} eq =
    aux (mem-lookup mem b) refl eq
    where
      aux : ∀ (m : Maybe Fr)
          → mem-lookup mem b ≡ m
          → (m >>= to-bool) ≡ just sel
          → ∃-syntax (λ bv →
              (mem-lookup mem b ≡ just bv) × (to-bool bv ≡ just sel))
      aux nothing   _    ()
      aux (just bv) m-eq eq' = bv , m-eq , eq'

  -- `to-bool` evidence yields the is-bit predicate required by constraints.
  to-bool→is-bit : ∀ {v sel} → to-bool v ≡ just sel → is-bit v
  to-bool→is-bit {sel = true}  eq = inj₂ (to-bool-true  eq)
  to-bool→is-bit {sel = false} eq = inj₁ (to-bool-false eq)

------------------------------------------------------------------------
-- Single-instruction emission
--
-- For every instruction except `declare-pub-input`, only `nr-wires`
-- matters in the synth state — `nr-declared-pi` and `output-wires` are
-- untouched.  This abbreviation captures the emitted constraints starting
-- from a fresh synth state with `nr-wires = n` (the `-with-decl`
-- variant below threads `nr-declared-pi` for `declare-pub-input`).
------------------------------------------------------------------------

-- Pull a pre-state lookup back from a post-state lookup, given the
-- index is within pre-state bounds.  Used by the backward dispatcher
-- to bridge between satisfies-constraints witnesses (which give post-state
-- lookups) and the per-instruction `*-bwd` lemmas (which take pre-state
-- lookups).
lookup-shrink : ∀ (mem suffix : List Fr) i {v}
  → mem-lookup (mem ++ suffix) i ≡ just v
  → suc i Data.Nat.≤ length mem
  → mem-lookup mem i ≡ just v
lookup-shrink []        _ _       _  ()
lookup-shrink (x ∷ xs)  _ zero    eq _  = eq
lookup-shrink (x ∷ xs)  s (suc i) eq (Data.Nat.s≤s lt) =
  lookup-shrink xs s i eq lt

-- Multi-index analogue of `lookup-shrink`.  Given that every index in
-- `is` is `< length mem` (`All (_< length mem) is`), a
-- `mem-lookups (mem ++ suffix) is ≡ just vs` collapses to
-- `mem-lookups mem is ≡ just vs`.  Used by the backward dispatcher for
-- the cryptographic-cluster cases.
mem-lookups-shrink : ∀ (mem suffix : List Fr) (is : List Index) {vs}
  → All (_< length mem) is
  → mem-lookups (mem ++ suffix) is ≡ just vs
  → mem-lookups mem is ≡ just vs
mem-lookups-shrink mem suffix []       _              refl = refl
mem-lookups-shrink mem suffix (i ∷ is) (i≤len ∷ᴬ rest) eq =
  aux (mem-lookup (mem ++ suffix) i) refl
      (mem-lookups (mem ++ suffix) is) refl
      eq
  where
    aux : ∀ (m : Maybe Fr) → mem-lookup (mem ++ suffix) i ≡ m
        → (ms : Maybe (List Fr)) → mem-lookups (mem ++ suffix) is ≡ ms
        → ∀ {vs} → (m >>= λ v → ms >>= λ vs' → just (v ∷ vs')) ≡ just vs
        → mem-lookups mem (i ∷ is) ≡ just vs
    aux nothing   _    _          _    ()
    aux (just _)  _    nothing    _    ()
    aux (just v)  m-eq (just vs') ms-eq refl
      rewrite lookup-shrink mem suffix i {v} m-eq i≤len
            | mem-lookups-shrink mem suffix is {vs'} rest ms-eq
      = refl

single-instr-constraints : Bool → ℕ → Instruction → List Constraint
single-instr-constraints hc n i =
  SynthState.constraints (circuit-instr hc i (mk-synth n [] 0 []))

-- Variant that exposes `nr-declared-pi`.  Used by `declare-pub-input`,
-- whose emitted constraint's `entry` index depends on the count of
-- previously-declared PIs.  Only `declare-pub-input` inspects
-- `nr-declared-pi`; for every other instruction this equals
-- `single-instr-constraints` at `d = 0`.
single-instr-constraints-with-decl : Bool → ℕ → ℕ → Instruction → List Constraint
single-instr-constraints-with-decl hc n d i =
  SynthState.constraints (circuit-instr hc i (mk-synth n [] d []))

------------------------------------------------------------------------
-- add(a, b)
--
-- Lowering (§5.2):  out = ⟦a⟧ + ⟦b⟧
-- Operational (§4.4): append M[a] + M[b]; Δmem = 1.
------------------------------------------------------------------------

add-fwd : ∀ {pre s s' a b hc} {rand : Maybe Fr}
  → R-instr pre s (add a b) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (add a b))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
add-fwd {s = s} {a = a} {b = b} (r-add {av = av} {bv = bv} la lb) =
  ≑⊕-fwd (Preprocessed.memory s ++ (av +ᶠ bv) ∷ [])
         (lookup-extends (Preprocessed.memory s) ((av +ᶠ bv) ∷ []) a la)
         (lookup-extends (Preprocessed.memory s) ((av +ᶠ bv) ∷ []) b lb)
         (lookup-new     (Preprocessed.memory s) (av +ᶠ bv))
   ∷ᴬ []ᴬ

add-bwd : ∀ {pre s a b av bv v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → mem-lookup (Preprocessed.memory s) b ≡ just bv
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (add a b))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ av +ᶠ bv) × R-instr pre s (add a b) (push-mem s v)
add-bwd {pre = pre} {s = s} {a = a} {b = b} {av = av} {bv = bv} {v = v}
        la lb (h ∷ᴬ _) =
  let (av' , bv' , ov' , la' , lb' , lout , eq) =
        ≑⊕-inv (Preprocessed.memory s ++ (v ∷ [])) h
      av≡av' = extend-uniq (Preprocessed.memory s) (v ∷ []) a la la'
      bv≡bv' = extend-uniq (Preprocessed.memory s) (v ∷ []) b lb lb'
      v≡ov'  = new-uniq (Preprocessed.memory s) v lout
      v≡sum  : v ≡ av +ᶠ bv
      v≡sum  = trans v≡ov' (trans eq (cong₂ _+ᶠ_ (sym av≡av') (sym bv≡bv')))
  in v≡sum
   , subst (R-instr pre s (add a b)) (cong (push-mem s) (sym v≡sum))
           (r-add la lb)

------------------------------------------------------------------------
-- copy(v)
--
-- Lowering: out = ⟦v⟧
-- Operational: append M[v]; Δmem = 1.
------------------------------------------------------------------------

copy-fwd : ∀ {pre s s' v hc} {rand : Maybe Fr}
  → R-instr pre s (copy v) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (copy v))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
copy-fwd {s = s} {v = v} (r-copy {v = v0} la) =
  ≑wire-fwd (Preprocessed.memory s ++ v0 ∷ [])
            (lookup-extends (Preprocessed.memory s) (v0 ∷ []) v la)
            (lookup-new     (Preprocessed.memory s) v0)
   ∷ᴬ []ᴬ

copy-bwd : ∀ {pre s v vv w hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) v ≡ just vv
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (copy v))
      (mk-witness (Preprocessed.memory s ++ (w ∷ []))
                  (Preprocessed.pis s) rand)
  → (w ≡ vv) × R-instr pre s (copy v) (push-mem s w)
copy-bwd {pre = pre} {s = s} {v = v} {vv = vv} {w = w}
         la (h ∷ᴬ _) =
  let (vv' , ov , la' , lout , eq) = ≑wire-inv (Preprocessed.memory s ++ (w ∷ [])) h
      mem    = Preprocessed.memory s
      vv≡vv' : vv ≡ vv'
      vv≡vv' = extend-uniq mem (w ∷ []) v la la'
      w≡ov   : w ≡ ov
      w≡ov   = new-uniq mem w lout
      w≡vv   : w ≡ vv
      w≡vv   = trans w≡ov (trans eq (sym vv≡vv'))
  in w≡vv
   , subst (R-instr pre s (copy v)) (cong (push-mem s) (sym w≡vv))
           (r-copy la)

------------------------------------------------------------------------
-- load-imm(k)
--
-- Lowering: out = k
-- Operational: append k; Δmem = 1.
------------------------------------------------------------------------

load-imm-fwd : ∀ {pre s s' k hc} {rand : Maybe Fr}
  → R-instr pre s (load-imm k) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (load-imm k))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
load-imm-fwd {s = s} {k = k} r-load-imm =
  ≑con-fwd (Preprocessed.memory s ++ k ∷ []) (lookup-new (Preprocessed.memory s) k) ∷ᴬ []ᴬ

load-imm-bwd : ∀ {pre s k w hc} {rand : Maybe Fr}
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (load-imm k))
      (mk-witness (Preprocessed.memory s ++ (w ∷ []))
                  (Preprocessed.pis s) rand)
  → (w ≡ k) × R-instr pre s (load-imm k) (push-mem s w)
load-imm-bwd {pre = pre} {s = s} {k = k} {w = w}
             (h ∷ᴬ _) =
  let (ov , lout , eq) = ≑con-inv (Preprocessed.memory s ++ (w ∷ [])) h
      w≡ov   : w ≡ ov
      w≡ov   = new-uniq (Preprocessed.memory s) w lout
      w≡k    : w ≡ k
      w≡k    = trans w≡ov eq
  in w≡k
   , subst (R-instr pre s (load-imm k)) (cong (push-mem s) (sym w≡k))
           r-load-imm

------------------------------------------------------------------------
-- constrain-eq(a, b)
--
-- Lowering: ⟦a⟧ = ⟦b⟧
-- Operational: precondition M[a] = M[b]; Δmem = 0.
------------------------------------------------------------------------

constrain-eq-fwd : ∀ {pre s s' a b hc} {rand : Maybe Fr}
  → R-instr pre s (constrain-eq a b) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (constrain-eq a b))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
constrain-eq-fwd {s = s} (r-constrain-eq {av = av} {bv = bv} la lb eq) =
  ≑vars-fwd (Preprocessed.memory s) la (trans lb (cong just (sym eq))) ∷ᴬ []ᴬ

constrain-eq-bwd : ∀ {pre s a b av bv hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → mem-lookup (Preprocessed.memory s) b ≡ just bv
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (constrain-eq a b))
      (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) rand)
  → R-instr pre s (constrain-eq a b) s
constrain-eq-bwd {s = s} {a = a} {b = b} {av = av} {bv = bv}
                 la lb (h ∷ᴬ _) =
  let (av' , bv' , la' , lb' , av'≡bv') = ≑vars-inv (Preprocessed.memory s) h
      mem    = Preprocessed.memory s
      av≡av' = lookup-uniq mem a la la'
      bv≡bv' = lookup-uniq mem b lb lb'
      -- av ≡ av' ≡ bv' ≡ bv  (constraint uses propositional equality)
      av≡bv  = trans av≡av' (trans av'≡bv' (sym bv≡bv'))
  in r-constrain-eq la lb av≡bv

------------------------------------------------------------------------
-- constrain-bits(v, n)
--
-- Lowering: ⟦v⟧ < 2^n  (range chip; vacuous when n ≥ FR_BITS)
-- Operational: precondition M[v] < 2^n; Δmem = 0.
------------------------------------------------------------------------

constrain-bits-fwd : ∀ {pre s s' v n hc} {rand : Maybe Fr}
  → R-instr pre s (constrain-bits v n) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (constrain-bits v n))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
constrain-bits-fwd (r-constrain-bits {v = vv} la _ fits) =
  (vv , la , fits) ∷ᴬ []ᴬ

constrain-bits-bwd : ∀ {pre s v n vv hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) v ≡ just vv
  → n < FR-BITS
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (constrain-bits v n))
      (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) rand)
  → R-instr pre s (constrain-bits v n) s
constrain-bits-bwd {pre = pre} {s = s} {v = v} {n = n} {vv = vv}
                   la bound ((vv' , la' , fits) ∷ᴬ _) =
  let mem    = Preprocessed.memory s
      vv≡vv' = lookup-uniq mem v la la'
      fits-vv : Fits-in vv n
      fits-vv = subst (λ z → Fits-in z n) (sym vv≡vv') fits
  in r-constrain-bits la bound fits-vv

------------------------------------------------------------------------
-- mul(a, b), neg(a)         (identical pattern to add)
------------------------------------------------------------------------

mul-fwd : ∀ {pre s s' a b hc} {rand : Maybe Fr}
  → R-instr pre s (mul a b) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (mul a b))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
mul-fwd {s = s} {a = a} {b = b} (r-mul {av = av} {bv = bv} la lb) =
  ≑⊗-fwd (Preprocessed.memory s ++ (av *ᶠ bv) ∷ [])
         (lookup-extends (Preprocessed.memory s) ((av *ᶠ bv) ∷ []) a la)
         (lookup-extends (Preprocessed.memory s) ((av *ᶠ bv) ∷ []) b lb)
         (lookup-new     (Preprocessed.memory s) (av *ᶠ bv))
   ∷ᴬ []ᴬ

mul-bwd : ∀ {pre s a b av bv v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → mem-lookup (Preprocessed.memory s) b ≡ just bv
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (mul a b))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ av *ᶠ bv) × R-instr pre s (mul a b) (push-mem s v)
mul-bwd {pre = pre} {s = s} {a = a} {b = b} {av = av} {bv = bv} {v = v}
        la lb (h ∷ᴬ _) =
  let (av' , bv' , ov' , la' , lb' , lout , eq) =
        ≑⊗-inv (Preprocessed.memory s ++ (v ∷ [])) h
      av≡av' = extend-uniq (Preprocessed.memory s) (v ∷ []) a la la'
      bv≡bv' = extend-uniq (Preprocessed.memory s) (v ∷ []) b lb lb'
      v≡ov'  = new-uniq (Preprocessed.memory s) v lout
      v≡prod : v ≡ av *ᶠ bv
      v≡prod = trans v≡ov' (trans eq (cong₂ _*ᶠ_ (sym av≡av') (sym bv≡bv')))
  in v≡prod
   , subst (R-instr pre s (mul a b)) (cong (push-mem s) (sym v≡prod))
           (r-mul la lb)

neg-fwd : ∀ {pre s s' a hc} {rand : Maybe Fr}
  → R-instr pre s (neg a) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (neg a))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
neg-fwd {s = s} {a = a} (r-neg {av = av} la) =
  ≑⊝-fwd (Preprocessed.memory s ++ (-ᶠ av) ∷ [])
         (lookup-extends (Preprocessed.memory s) ((-ᶠ av) ∷ []) a la)
         (lookup-new     (Preprocessed.memory s) (-ᶠ av))
   ∷ᴬ []ᴬ

neg-bwd : ∀ {pre s a av v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (neg a))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ (-ᶠ av)) × R-instr pre s (neg a) (push-mem s v)
neg-bwd {pre = pre} {s = s} {a = a} {av = av} {v = v}
        la (h ∷ᴬ _) =
  let (av' , ov' , la' , lout , eq) = ≑⊝-inv (Preprocessed.memory s ++ (v ∷ [])) h
      av≡av' : av ≡ av'
      av≡av' = extend-uniq (Preprocessed.memory s) (v ∷ []) a la la'
      v≡ov'  : v ≡ ov'
      v≡ov'  = new-uniq (Preprocessed.memory s) v lout
      v≡neg  : v ≡ (-ᶠ av)
      v≡neg  = trans v≡ov' (trans eq (cong -ᶠ_ (sym av≡av')))
  in v≡neg
   , subst (R-instr pre s (neg a)) (cong (push-mem s) (sym v≡neg))
           (r-neg la)

------------------------------------------------------------------------
-- test-eq(a, b)
--
-- Lowering: out = 1 iff ⟦a⟧ = ⟦b⟧, expressed as `out ≡ from-bool (a ≡ᶠ? b)`.
-- Operational: append `from-bool (av ≡ᶠ? bv)`.
------------------------------------------------------------------------

test-eq-fwd : ∀ {pre s s' a b hc} {rand : Maybe Fr}
  → R-instr pre s (test-eq a b) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (test-eq a b))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
test-eq-fwd {s = s} {a = a} {b = b} (r-test-eq {av = av} {bv = bv} la lb) =
  ( av , bv , from-bool (av ≡ᶠ? bv)
  , lookup-extends (Preprocessed.memory s) (from-bool (av ≡ᶠ? bv) ∷ []) a la
  , lookup-extends (Preprocessed.memory s) (from-bool (av ≡ᶠ? bv) ∷ []) b lb
  , lookup-new     (Preprocessed.memory s) (from-bool (av ≡ᶠ? bv))
  , refl
  ) ∷ᴬ []ᴬ

test-eq-bwd : ∀ {pre s a b av bv v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → mem-lookup (Preprocessed.memory s) b ≡ just bv
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (test-eq a b))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ from-bool (av ≡ᶠ? bv))
  × R-instr pre s (test-eq a b) (push-mem s v)
test-eq-bwd {pre = pre} {s = s} {a = a} {b = b} {av = av} {bv = bv} {v = v}
            la lb ((av' , bv' , ov' , la' , lb' , lout , eq) ∷ᴬ _) =
  let av≡av' : av ≡ av'
      av≡av' = extend-uniq (Preprocessed.memory s) (v ∷ []) a la la'
      bv≡bv' : bv ≡ bv'
      bv≡bv' = extend-uniq (Preprocessed.memory s) (v ∷ []) b lb lb'
      v≡ov'  : v ≡ ov'
      v≡ov'  = new-uniq (Preprocessed.memory s) v lout
      v≡teq  : v ≡ from-bool (av ≡ᶠ? bv)
      v≡teq  = trans v≡ov' (trans eq (cong₂ (λ x y → from-bool (x ≡ᶠ? y))
                                              (sym av≡av') (sym bv≡bv')))
  in v≡teq
   , subst (R-instr pre s (test-eq a b)) (cong (push-mem s) (sym v≡teq))
           (r-test-eq la lb)

------------------------------------------------------------------------
-- output(v), pi-skip(g, n)     — no constraints; forward proof is trivial.
------------------------------------------------------------------------

output-fwd : ∀ {pre s s' v hc} {rand : Maybe Fr}
  → R-instr pre s (output v) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (output v))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
output-fwd _ = []ᴬ

pi-skip-fwd : ∀ {pre s s' g n hc} {rand : Maybe Fr}
  → R-instr pre s (pi-skip g n) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (pi-skip g n))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
pi-skip-fwd _ = []ᴬ

-- `output`'s backward direction is intentionally NOT exposed here: it
-- emits no constraints, so there is nothing to invert — `CircuitProof`
-- fires `r-output` directly from the `mem-lookup` in scope.  The same
-- goes for pi-skip:
-- - active branch needs the transcript-prefix-match precondition that
--   uses the private `_≡ᶠ-list?_`;
-- - inactive branch needs `eval-guard ≡ just false`.
-- `CircuitProof.agda` dispatches directly to
-- `r-pi-skip-{active,inactive}`, which has the side data in scope.

------------------------------------------------------------------------
-- constrain-to-boolean(v)
--
-- Lowering: ⟦v⟧ ∈ {0, 1}
-- Operational: precondition bool(M[v]) ∈ {false, true}; Δmem = 0.
------------------------------------------------------------------------

constrain-to-boolean-fwd : ∀ {pre s s' v hc} {rand : Maybe Fr}
  → R-instr pre s (constrain-to-boolean v) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (constrain-to-boolean v))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
constrain-to-boolean-fwd {s = s} {v = v} (r-constrain-to-boolean la-bind) =
  let (vv , lvv , to-vv) = extract-bit-lookup (Preprocessed.memory s) v la-bind
  in (vv , lvv , to-bool→is-bit to-vv) ∷ᴬ []ᴬ

-- Backward: the constraint's `is-bit vv` gives us `vv ∈ {0, 1}`, which
-- determines `to-bool vv`.  Combined with `mem-lookup mem v ≡ just vv`
-- (from the constraint), we can fire `r-constrain-to-boolean`.
constrain-to-boolean-bwd : ∀ {pre s v hc} {rand : Maybe Fr}
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (constrain-to-boolean v))
      (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) rand)
  → R-instr pre s (constrain-to-boolean v) s
constrain-to-boolean-bwd {pre = pre} {s = s} {v = v}
                         ((vv , lvv , inj₁ vv≡0) ∷ᴬ _) =
  let mem = Preprocessed.memory s
      to-bind : (mem-lookup mem v >>= to-bool) ≡ just false
      to-bind = trans (cong (λ m → m >>= to-bool) lvv)
                      (subst (λ z → to-bool z ≡ just false)
                             (sym vv≡0) to-bool-of-0ᶠ)
  in r-constrain-to-boolean to-bind
constrain-to-boolean-bwd {pre = pre} {s = s} {v = v}
                         ((vv , lvv , inj₂ vv≡1) ∷ᴬ _) =
  let mem = Preprocessed.memory s
      to-bind : (mem-lookup mem v >>= to-bool) ≡ just true
      to-bind = trans (cong (λ m → m >>= to-bool) lvv)
                      (subst (λ z → to-bool z ≡ just true)
                             (sym vv≡1) to-bool-of-1ᶠ)
  in r-constrain-to-boolean to-bind

------------------------------------------------------------------------
-- not(a)                   (§6.5)
--
-- Lowering:    out = is_zero(⟦a⟧) ≡ from-bool (⟦a⟧ ≡ᶠ? 0ᶠ)
-- Operational: append from-bool (¬ bool(M[a]))
--              precondition: bool(M[a]) ∈ {false, true}.
--
-- Forward: the operational rule provides the bit precondition.
-- Backward: requires `is-bit av` (producer obligation O2) on the
-- operand.
------------------------------------------------------------------------

private
  -- For av ∈ {0ᶠ, 1ᶠ}: ¬ b = (av ≡ᶠ? 0ᶠ) in the boolean lattice.
  not-equation : ∀ av (b : Bool)
    → to-bool av ≡ just b
    → from-bool (Bool.not b) ≡ from-bool (av ≡ᶠ? 0ᶠ)
  not-equation av true to-av =
    let av≡1 : av ≡ 1ᶠ
        av≡1 = to-bool-true to-av
        bool-eq : (av ≡ᶠ? 0ᶠ) ≡ false
        bool-eq = subst (λ z → (z ≡ᶠ? 0ᶠ) ≡ false) (sym av≡1) (≡ᶠ?-false 1ᶠ≢0ᶠ)
    in sym (cong from-bool bool-eq)
  not-equation av false to-av =
    let av≡0 : av ≡ 0ᶠ
        av≡0 = to-bool-false to-av
        bool-eq : (av ≡ᶠ? 0ᶠ) ≡ true
        bool-eq = subst (λ z → (z ≡ᶠ? 0ᶠ) ≡ true) (sym av≡0) ≡ᶠ?-refl
    in sym (cong from-bool bool-eq)

not-fwd : ∀ {pre s s' a hc} {rand : Maybe Fr}
  → R-instr pre s (not a) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (not a))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
not-fwd {s = s} {a = a} (r-not {b = b} la-bind) =
  let mem    = Preprocessed.memory s
      (av , lav , to-av) = extract-bit-lookup mem a la-bind
      out-val = from-bool (Bool.not b)
  in ( av , out-val
     , lookup-extends mem (out-val ∷ []) a lav
     , lookup-new     mem out-val
     , not-equation av b to-av
     ) ∷ᴬ []ᴬ

-- Backward direction.
--
-- Requires `is-bit av` (producer obligation O2 on the operand).
-- With av ∈ {0ᶠ, 1ᶠ} we can run `to-bool av` deterministically and the
-- constraint's `from-bool (av ≡ᶠ? 0ᶠ)` collapses to `from-bool (Bool.not b)`.

private
  -- Operational rule firing for `not a`, split on the is-bit case.
  -- Pre-computes the value `from-bool (av ≡ᶠ? 0ᶠ)` which is what the
  -- constraint forces the output to be.
  not-fire : ∀ {pre} (s : Preprocessed) (a : Index) (av : Fr)
    → mem-lookup (Preprocessed.memory s) a ≡ just av
    → is-bit av
    → R-instr pre s (not a) (push-mem s (from-bool (av ≡ᶠ? 0ᶠ)))
  not-fire {pre} s a av la (inj₁ av≡0) =
    let to-bind : (mem-lookup (Preprocessed.memory s) a >>= to-bool) ≡ just false
        to-bind = trans (cong (λ m → m >>= to-bool) la)
                        (subst (λ z → to-bool z ≡ just false)
                               (sym av≡0) to-bool-of-0ᶠ)
        ≡ᶠ-true : (av ≡ᶠ? 0ᶠ) ≡ true
        ≡ᶠ-true = subst (λ z → (z ≡ᶠ? 0ᶠ) ≡ true) (sym av≡0) ≡ᶠ?-refl
        target-eq : from-bool (Bool.not false) ≡ from-bool (av ≡ᶠ? 0ᶠ)
        target-eq = cong from-bool (sym ≡ᶠ-true)
    in subst (R-instr pre s (not a)) (cong (push-mem s) target-eq)
             (r-not to-bind)
  not-fire {pre} s a av la (inj₂ av≡1) =
    let to-bind : (mem-lookup (Preprocessed.memory s) a >>= to-bool) ≡ just true
        to-bind = trans (cong (λ m → m >>= to-bool) la)
                        (subst (λ z → to-bool z ≡ just true)
                               (sym av≡1) to-bool-of-1ᶠ)
        ≡ᶠ-false : (av ≡ᶠ? 0ᶠ) ≡ false
        ≡ᶠ-false = subst (λ z → (z ≡ᶠ? 0ᶠ) ≡ false)
                         (sym av≡1) (≡ᶠ?-false 1ᶠ≢0ᶠ)
        target-eq : from-bool (Bool.not true) ≡ from-bool (av ≡ᶠ? 0ᶠ)
        target-eq = cong from-bool (sym ≡ᶠ-false)
    in subst (R-instr pre s (not a)) (cong (push-mem s) target-eq)
             (r-not to-bind)

not-bwd : ∀ {pre s a av v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → is-bit av
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (not a))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ from-bool (av ≡ᶠ? 0ᶠ))
  × R-instr pre s (not a) (push-mem s (from-bool (av ≡ᶠ? 0ᶠ)))
not-bwd {pre = pre} {s = s} {a = a} {av = av} {v = v}
        la is-bit-av ((av' , ov , la' , lout , ov-eq) ∷ᴬ _) =
  let mem    = Preprocessed.memory s
      av≡av' = extend-uniq mem (v ∷ []) a la la'
      v≡ov   = new-uniq mem v lout
      v≡target : v ≡ from-bool (av ≡ᶠ? 0ᶠ)
      v≡target = trans v≡ov
                  (trans ov-eq (cong (λ z → from-bool (z ≡ᶠ? 0ᶠ))
                                     (sym av≡av')))
  in v≡target , not-fire s a av la is-bit-av

------------------------------------------------------------------------
-- cond-select(b, a, c)              (§6.5 sketch case)
--
-- Lowering: ⟦b⟧ ∈ {0,1}  ∧  out = ⟦b⟧·⟦a⟧ + (1−⟦b⟧)·⟦c⟧
-- Operational: precondition bool(M[b]) ∈ {false, true}; append M[a]
--              when true, else M[c]; Δmem = 1.
--
-- §6.5 forward sketch:  case on `sel`:
--   sel = true:  bv = 1ᶠ.  RHS = 1·av + (1+(-1))·cv = av + 0·cv = av.
--   sel = false: bv = 0ᶠ.  RHS = 0·av + (1+(-0))·cv = 0 + 1·cv = cv.
------------------------------------------------------------------------

private
  -- Field-arithmetic lemma for the select equation, true-branch:
  --   1·av + (1 + (-1))·cv  ≡  av
  select-eq-true : ∀ av cv
    → (1ᶠ *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ 1ᶠ)) *ᶠ cv) ≡ av
  select-eq-true av cv =
    trans (cong₂ _+ᶠ_ (*-one-l av)
                       (trans (cong (_*ᶠ cv) (+-inv-r 1ᶠ)) (*-zero-l cv)))
          (+-zero-r av)

  -- Field-arithmetic lemma, false-branch:
  --   0·av + (1 + (-0))·cv  ≡  cv
  select-eq-false : ∀ av cv
    → (0ᶠ *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ 0ᶠ)) *ᶠ cv) ≡ cv
  select-eq-false av cv =
    trans (cong₂ _+ᶠ_ (*-zero-l av)
                       (trans (cong (λ z → (1ᶠ +ᶠ z) *ᶠ cv) -ᶠ-zero)
                              (trans (cong (_*ᶠ cv) (+-zero-r 1ᶠ))
                                     (*-one-l cv))))
          (+-zero-l cv)

  -- The select equation holds in both branches of `sel`.
  -- `to-bool bv ≡ just sel` already pins `bv` to 0ᶠ / 1ᶠ.
  select-equation : ∀ (sel : Bool) bv av cv
    → to-bool bv ≡ just sel
    → (if sel then av else cv)
      ≡ (bv *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ bv)) *ᶠ cv)
  select-equation true  bv av cv to-bv =
    subst (λ z → av ≡ (z *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ z)) *ᶠ cv))
          (sym (to-bool-true to-bv))
          (sym (select-eq-true av cv))
  select-equation false bv av cv to-bv =
    subst (λ z → cv ≡ (z *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ z)) *ᶠ cv))
          (sym (to-bool-false to-bv))
          (sym (select-eq-false av cv))

cond-select-fwd : ∀ {pre s s' b a c hc} {rand : Maybe Fr}
  → R-instr pre s (cond-select b a c) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (cond-select b a c))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
cond-select-fwd {s = s} {b = b} {a = a} {c = c}
                (r-cond-select {sel = sel} {av = av-spec} {bv = cv-spec}
                                lb-bind la lc) =
  let mem     = Preprocessed.memory s
      out-val = if sel then av-spec else cv-spec
      (bv , lbv-pre , to-bv) = extract-bit-lookup mem b lb-bind
  in ( bv , av-spec , cv-spec , out-val
     , lookup-extends mem (out-val ∷ []) b lbv-pre
     , lookup-extends mem (out-val ∷ []) a la
     , lookup-extends mem (out-val ∷ []) c lc
     , lookup-new     mem out-val
     , to-bool→is-bit to-bv
     , select-equation sel bv av-spec cv-spec to-bv
     ) ∷ᴬ []ᴬ

-- Backward direction.  Case-splits on the bit value witnessed by
-- `is-bit bv'` and applies the corresponding select-equation lemma to
-- recover the output value.  No producer obligation needed: the
-- §6.5 footnote observes that the V1 lowering for cond-select's bit
-- operand silently rejects non-bit values, so the constraint itself
-- enforces what's needed for the backward direction.
cond-select-bwd : ∀ {pre s b a c bv av cv v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) b ≡ just bv
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → mem-lookup (Preprocessed.memory s) c ≡ just cv
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (cond-select b a c))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → R-instr pre s (cond-select b a c) (push-mem s v)
cond-select-bwd {pre = pre} {s = s} {b = b} {a = a} {c = c}
                {bv = bv} {av = av} {cv = cv} {v = v}
                lb la lc
                ((bv' , av' , cv' , ov , lb' , la' , lc' , lout
                                       , inj₁ bv'≡0 , eq) ∷ᴬ _) =
  -- Case bv' ≡ 0ᶠ ⇒ sel = false ⇒ output = cv.
  let mem    = Preprocessed.memory s
      bv≡bv' = extend-uniq mem (v ∷ []) b lb lb'
      cv≡cv' = extend-uniq mem (v ∷ []) c lc lc'
      v≡ov   = new-uniq mem v lout
      bv≡0   = trans bv≡bv' bv'≡0
      ov≡cv' = trans (subst (λ z → ov ≡ (z *ᶠ av') +ᶠ ((1ᶠ +ᶠ (-ᶠ z)) *ᶠ cv'))
                             bv'≡0 eq)
                      (select-eq-false av' cv')
      v≡cv : v ≡ cv
      v≡cv   = trans v≡ov (trans ov≡cv' (sym cv≡cv'))
      to-bv  = subst (λ z → to-bool z ≡ just false) (sym bv≡0) to-bool-of-0ᶠ
      lb-bind : (mem-lookup mem b >>= to-bool) ≡ just false
      lb-bind = trans (cong (λ m → m >>= to-bool) lb) to-bv
      r-fired : R-instr pre s (cond-select b a c) (push-mem s cv)
      r-fired = r-cond-select {sel = false} lb-bind la lc
  in subst (R-instr pre s (cond-select b a c))
           (cong (push-mem s) (sym v≡cv))
           r-fired
cond-select-bwd {pre = pre} {s = s} {b = b} {a = a} {c = c}
                {bv = bv} {av = av} {cv = cv} {v = v}
                lb la lc
                ((bv' , av' , cv' , ov , lb' , la' , lc' , lout
                                       , inj₂ bv'≡1 , eq) ∷ᴬ _) =
  -- Case bv' ≡ 1ᶠ ⇒ sel = true ⇒ output = av.
  let mem    = Preprocessed.memory s
      bv≡bv' = extend-uniq mem (v ∷ []) b lb lb'
      av≡av' = extend-uniq mem (v ∷ []) a la la'
      v≡ov   = new-uniq mem v lout
      bv≡1   = trans bv≡bv' bv'≡1
      ov≡av' = trans (subst (λ z → ov ≡ (z *ᶠ av') +ᶠ ((1ᶠ +ᶠ (-ᶠ z)) *ᶠ cv'))
                             bv'≡1 eq)
                      (select-eq-true av' cv')
      v≡av : v ≡ av
      v≡av   = trans v≡ov (trans ov≡av' (sym av≡av'))
      to-bv  = subst (λ z → to-bool z ≡ just true) (sym bv≡1) to-bool-of-1ᶠ
      lb-bind : (mem-lookup mem b >>= to-bool) ≡ just true
      lb-bind = trans (cong (λ m → m >>= to-bool) lb) to-bv
      r-fired : R-instr pre s (cond-select b a c) (push-mem s av)
      r-fired = r-cond-select {sel = true} lb-bind la lc
  in subst (R-instr pre s (cond-select b a c))
           (cong (push-mem s) (sym v≡av))
           r-fired

------------------------------------------------------------------------
-- declare-pub-input(v)             (state-dependent: nr-declared-pi)
--
-- Lowering: emits `constraint-pi-from-wire entry v` where
--   entry = preamble-pi-count hc + nr-declared-pi  (synth-state field).
-- Operational: append M[v] to `pis`; Δmem = 0.
--
-- The forward lemma threads the synth-state's `nr-declared-pi`
-- explicitly via `single-instr-constraints-with-decl`, and requires the
-- consistency precondition that the operational `pis` length matches
-- the synth-state's PI count, which the program-level inductive
-- invariant discharges.
------------------------------------------------------------------------

declare-pub-input-fwd : ∀ {pre s s' v hc d} {rand : Maybe Fr}
  → length (Preprocessed.pis s) ≡ preamble-pi-count hc + d
  → R-instr pre s (declare-pub-input v) s'
  → satisfies-constraints
      (single-instr-constraints-with-decl hc (length (Preprocessed.memory s)) d
         (declare-pub-input v))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
declare-pub-input-fwd {s = s} {v = v} {hc = hc} {d = d}
                      pi-len (r-declare-pub-input {v = wv} la) =
  -- post-state: memory unchanged; pis = s.pis ++ (wv ∷ []).
  let entry  = preamble-pi-count hc + d
      pis-eq : pi-lookup (Preprocessed.pis s ++ (wv ∷ [])) entry ≡ just wv
      pis-eq = subst (λ k → pi-lookup (Preprocessed.pis s ++ (wv ∷ [])) k
                              ≡ just wv)
                     pi-len (lookup-new (Preprocessed.pis s) wv)
  in (wv , wv , la , pis-eq , refl) ∷ᴬ []ᴬ

-- Backward direction.  From the constraint we extract: the PI vector
-- extends `s.pis` with exactly the value bound to wire `v`, and the
-- `entry` index points at it via `pi-lookup`.  Uniqueness of
-- `mem-lookup` then identifies `wv` with the operational value.
declare-pub-input-bwd : ∀ {pre s v wv hc d ext} {rand : Maybe Fr}
  → length (Preprocessed.pis s) ≡ preamble-pi-count hc + d
  → mem-lookup (Preprocessed.memory s) v ≡ just wv
  → satisfies-constraints
      (single-instr-constraints-with-decl hc (length (Preprocessed.memory s)) d
         (declare-pub-input v))
      (mk-witness (Preprocessed.memory s)
                  (Preprocessed.pis s ++ (ext ∷ [])) rand)
  → (ext ≡ wv) × R-instr pre s (declare-pub-input v)
                                (record s
                                  { pis        = Preprocessed.pis s ++ (ext ∷ [])
                                  ; pub-in-idx = suc (Preprocessed.pub-in-idx s) })
declare-pub-input-bwd {pre = pre} {s = s} {v = v} {wv = wv} {hc = hc} {d = d}
                      {ext = ext} pi-len lv
                      ((wv' , pv , lv' , pi-eq , pv≡wv') ∷ᴬ _) =
  let wv≡wv' = lookup-uniq (Preprocessed.memory s) v lv lv'
      entry  = preamble-pi-count hc + d
      pis-new : pi-lookup (Preprocessed.pis s ++ (ext ∷ [])) (length (Preprocessed.pis s))
                  ≡ just ext
      pis-new = lookup-new (Preprocessed.pis s) ext
      -- Transport `pis-new` along `pi-len : length (pis s) ≡ entry`.
      pis-at-entry : pi-lookup (Preprocessed.pis s ++ (ext ∷ [])) entry ≡ just ext
      pis-at-entry = subst (λ k → pi-lookup (Preprocessed.pis s ++ (ext ∷ [])) k
                                    ≡ just ext)
                           pi-len pis-new
      pv≡ext = just-injective (trans (sym pi-eq) pis-at-entry)
      ext≡wv : ext ≡ wv
      ext≡wv = trans (sym pv≡ext) (trans pv≡wv' (sym wv≡wv'))
      r-fired : R-instr pre s (declare-pub-input v)
                  (record s
                    { pis        = Preprocessed.pis s ++ (wv ∷ [])
                    ; pub-in-idx = suc (Preprocessed.pub-in-idx s) })
      r-fired = r-declare-pub-input lv
  in ext≡wv
   , subst (λ z → R-instr pre s (declare-pub-input v)
                    (record s
                      { pis        = Preprocessed.pis s ++ (z ∷ [])
                      ; pub-in-idx = suc (Preprocessed.pub-in-idx s) }))
           (sym ext≡wv) r-fired

------------------------------------------------------------------------
-- public-input nothing                  (no constraints)
--
-- Operational: r-public-input-active fires with guard ≡ just true.
-- Lowering: emits no constraint (`bump-wires` only).
------------------------------------------------------------------------

public-input-nothing-fwd : ∀ {pre s s' hc} {rand : Maybe Fr}
  → R-instr pre s (public-input nothing) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (public-input nothing))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
public-input-nothing-fwd _ = []ᴬ

-- Backward: no constraints to invert — `CircuitProof` fires
-- `r-public-input-active` directly (`eval-guard _ nothing ≡ just true`
-- by definition; the transcript is the source of `v`).

------------------------------------------------------------------------
-- public-input (just g)                 (guard-disj constraint)
--
-- Operational: two rules — active (guard = true, output from transcript)
-- and inactive (guard = false, output = 0ᶠ).
-- Lowering: emits `constraint-guard-disj out g`, satisfied by either
--   (out = 0) ∨ (⟦g⟧ = 1).
--
-- Forward needs the active/inactive split; the active case must
-- characterize `consume-pub-out` to compute the post-state's memory.
------------------------------------------------------------------------

public-input-just-fwd : ∀ {pre s s' g hc} {rand : Maybe Fr}
  → R-instr pre s (public-input (just g)) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (public-input (just g)))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
public-input-just-fwd {s = s} {g = g} {hc = hc}
                      (r-public-input-inactive eg) =
  -- Inactive: post-state memory = s.memory ++ [0ᶠ]; out value is 0ᶠ.
  let mem  = Preprocessed.memory s
      (gv , lg , to-gv) = extract-bit-lookup mem g eg
  in ( 0ᶠ , gv
     , lookup-new mem 0ᶠ
     , lookup-extends mem (0ᶠ ∷ []) g lg
     , to-bool→is-bit to-gv
     , inj₁ refl
     ) ∷ᴬ []ᴬ
public-input-just-fwd {s = s} {g = g} {hc = hc} {rand = rand}
                      (r-public-input-active {v = v} {s₁ = s₁} eg cp) =
  -- Active: consume-pub-out yields v; post-state memory = s.memory ++ [v].
  let mem    = Preprocessed.memory s
      mem-eq : Preprocessed.memory s₁ ≡ mem
      mem-eq = consume-pub-out-mem s cp
      (gv , lg , to-gv) = extract-bit-lookup mem g eg
      gv≡1   : gv ≡ 1ᶠ
      gv≡1   = to-bool-true to-gv
      -- Rewrite `push-mem s₁ v` so its memory shape is `mem ++ (v ∷ [])`.
      mem'   = mem ++ (v ∷ [])
      mem₁-eq : Preprocessed.memory (push-mem s₁ v) ≡ mem'
      mem₁-eq = cong (_++ (v ∷ [])) mem-eq
  in subst (λ m → satisfies-constraints
             (single-instr-constraints hc (length mem) (public-input (just g)))
             (mk-witness m (Preprocessed.pis (push-mem s₁ v)) rand))
           (sym mem₁-eq)
           (( v , gv
            , lookup-new mem v
            , lookup-extends mem (v ∷ []) g lg
            , to-bool→is-bit to-gv
            , inj₂ gv≡1
            ) ∷ᴬ []ᴬ)

-- Backward direction for `public-input (just g)`: the guard-disj
-- constraint alone does not determine which operational rule (active /
-- inactive) fits — the *operational* `consume-pub-out` shape does — so
-- `CircuitProof` fires `r-public-input-{active,inactive}` directly
-- from the side data in scope.

------------------------------------------------------------------------
-- private-input nothing / (just g)
--
-- Identical pattern to `public-input`, swapping `consume-pub-out` for
-- `consume-priv` and the active rule accordingly.
------------------------------------------------------------------------

private-input-nothing-fwd : ∀ {pre s s' hc} {rand : Maybe Fr}
  → R-instr pre s (private-input nothing) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (private-input nothing))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
private-input-nothing-fwd _ = []ᴬ

private-input-just-fwd : ∀ {pre s s' g hc} {rand : Maybe Fr}
  → R-instr pre s (private-input (just g)) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (private-input (just g)))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
private-input-just-fwd {s = s} {g = g}
                       (r-private-input-inactive eg) =
  let mem  = Preprocessed.memory s
      (gv , lg , to-gv) = extract-bit-lookup mem g eg
  in ( 0ᶠ , gv
     , lookup-new mem 0ᶠ
     , lookup-extends mem (0ᶠ ∷ []) g lg
     , to-bool→is-bit to-gv
     , inj₁ refl
     ) ∷ᴬ []ᴬ
private-input-just-fwd {s = s} {g = g} {hc = hc} {rand = rand}
                       (r-private-input-active {v = v} {s₁ = s₁} eg cp) =
  let mem    = Preprocessed.memory s
      mem-eq : Preprocessed.memory s₁ ≡ mem
      mem-eq = consume-priv-mem s cp
      (gv , lg , to-gv) = extract-bit-lookup mem g eg
      gv≡1   : gv ≡ 1ᶠ
      gv≡1   = to-bool-true to-gv
      mem'   = mem ++ (v ∷ [])
      mem₁-eq : Preprocessed.memory (push-mem s₁ v) ≡ mem'
      mem₁-eq = cong (_++ (v ∷ [])) mem-eq
  in subst (λ m → satisfies-constraints
             (single-instr-constraints hc (length mem) (private-input (just g)))
             (mk-witness m (Preprocessed.pis (push-mem s₁ v)) rand))
           (sym mem₁-eq)
           (( v , gv
            , lookup-new mem v
            , lookup-extends mem (v ∷ []) g lg
            , to-bool→is-bit to-gv
            , inj₂ gv≡1
            ) ∷ᴬ []ᴬ)

------------------------------------------------------------------------
-- assert(c)
--
-- Lowering: ⟦c⟧ ≠ 0
-- Operational: precondition `bool(M[c]) = true`, i.e. M[c] = 1; Δmem = 0.
--
-- Forward: the operational rule witnesses M[c] = 1ᶠ via `to-bool`, and
-- `1ᶠ ≢ 0ᶠ` discharges the constraint.
--
-- Backward: the constraint only gives `v ≠ 0ᶠ`, while the operational rule
-- needs `v ∈ {0, 1} ∧ v ≠ 0`, so it requires `is-bit v` (producer
-- obligation O2).
------------------------------------------------------------------------

assert-fwd : ∀ {pre s s' c hc} {rand : Maybe Fr}
  → R-instr pre s (assert c) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (assert c))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
assert-fwd {s = s} {c = c} (r-assert la-bind) =
  let mem    = Preprocessed.memory s
      (vv , lvv , to-vv) = extract-bit-lookup mem c la-bind
      vv≡1   : vv ≡ 1ᶠ
      vv≡1   = to-bool-true to-vv
      vv≢0   : ¬ (vv ≡ 0ᶠ)
      vv≢0   = λ vv≡0 → 1ᶠ≢0ᶠ (trans (sym vv≡1) vv≡0)
  in (vv , lvv , vv≢0) ∷ᴬ []ᴬ

-- Backward direction.
--
-- Requires `is-bit v` (Circuit.is-bit, i.e. (v ≡ 0ᶠ) ⊎ (v ≡ 1ᶠ)), the
-- per-instruction O2 hypothesis guaranteeing the operand lies in
-- {0, 1}.  Combined with the constraint's `v ≠ 0ᶠ`, we case-split and rule
-- out `inj₁`, then discharge the operational rule using `to-bool-of-1ᶠ`.
assert-bwd : ∀ {pre s c v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) c ≡ just v
  → is-bit v
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s)) (assert c))
      (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) rand)
  → R-instr pre s (assert c) s
assert-bwd {pre = pre} {s = s} {c = c} {v = v}
           lv (inj₁ v≡0) ((v' , lv' , v'≢0) ∷ᴬ _) =
  -- v ≡ 0 contradicts the constraint's `v' ≢ 0` once we identify v with v'.
  let mem    = Preprocessed.memory s
      v≡v'   = lookup-uniq mem c lv lv'
      v'≡0   = trans (sym v≡v') v≡0
  in ⊥-elim (v'≢0 v'≡0)
assert-bwd {pre = pre} {s = s} {c = c} {v = v}
           lv (inj₂ v≡1) _ =
  -- v ≡ 1 — fire `r-assert` with the `to-bool-of-1ᶠ` evidence.
  let to-bind : (mem-lookup (Preprocessed.memory s) c >>= to-bool) ≡ just true
      to-bind = trans (cong (λ m → m >>= to-bool) lv)
                      (subst (λ z → to-bool z ≡ just true)
                             (sym v≡1) to-bool-of-1ᶠ)
  in r-assert to-bind

------------------------------------------------------------------------
-- div-mod-power-of-two(v, n)
--
-- Lowering: emits `constraint-div-mod q r v bits` with q = nr-wires,
--           r = nr-wires + 1, bits = n.
-- Operational: append `divisor := from-le-bits (drop bits (to-le-bits v))`
--              then `modulus := from-le-bits (take bits (to-le-bits v))`;
--              Δmem = 2.
--
-- The forward direction relies on four bit-decomposition facts:
--   • `bits-decomp-split`         — the arithmetic identity (canonical
--                                   decomposition satisfies the constraint);
--   • `fits-from-le-bits-take`    — modulus fits in `bits` bits;
--   • `fits-from-le-bits-drop`    — divisor fits in `FR_BITS − bits` bits;
--   • `divmod-canonical-noWrap`   — the canonical recombination value is
--                                   `valFr v < |Fr|`, so it does not wrap.
-- The backward direction lives in `CircuitProof` (reads the canonical
-- decomposition off the operational side-data); see its note below.
------------------------------------------------------------------------

div-mod-power-of-two-fwd : ∀ {pre s s' var bits hc} {rand : Maybe Fr}
  → R-instr pre s (div-mod-power-of-two var bits) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (div-mod-power-of-two var bits))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
div-mod-power-of-two-fwd {s = s} {var = var} {bits = bits}
  (r-div-mod-power-of-two {v = vv} la _) =
  let mem      = Preprocessed.memory s
      divisor  = from-le-bits (drop bits (to-le-bits vv))
      modulus  = from-le-bits (take bits (to-le-bits vv))
      -- post-mem = (mem ++ (divisor ∷ [])) ++ (modulus ∷ [])
      mem'     = (mem ++ (divisor ∷ [])) ++ (modulus ∷ [])
      -- q = length mem, r = suc (length mem).
      lq       : mem-lookup mem' (length mem) ≡ just divisor
      lq       = lookup-new-fst mem divisor modulus
      lr       : mem-lookup mem' (suc (length mem)) ≡ just modulus
      lr       = lookup-new-snd mem divisor modulus
      la-ext   : mem-lookup mem' var ≡ just vv
      la-ext   = lookup-extends (mem ++ (divisor ∷ [])) (modulus ∷ []) var
                   (lookup-extends mem (divisor ∷ []) var la)
  in ( divisor , modulus , vv
     , lq , lr , la-ext
     , fits-from-le-bits-take (to-le-bits vv) bits
     , fits-from-le-bits-drop vv bits
     , divmod-canonical-noWrap vv bits
     , bits-decomp-split vv bits
     ) ∷ᴬ []ᴬ

-- Backward direction.
--
-- Handled directly in `CircuitProof.satisfies→R-instr-step`: the
-- canonical decomposition (`x ≡ canon-q`, `y ≡ canon-r`) is read off the
-- operational side-data recorded by `R-instr→tr-step`, then the rule
-- fires via `r-div-mod-power-of-two`.  No separate backward lemma — and,
-- crucially, no uniqueness-of-division axiom: `constraint-div-mod` is
-- underconstrained (no in-field check, like `constraint-reconstitute`), so
-- distinct (q, r) pairs can satisfy it under modular wraparound; the
-- side-data, not the constraint, is what pins the values.

------------------------------------------------------------------------
-- reconstitute-field(d, m, n)         (§6.3)
--
-- Lowering: emits `constraint-reconstitute out d m bits` with no overflow
--           check.
-- Operational: requires `Fits-in mv bits`, `Fits-in dv (FR_BITS − bits)`,
--              and `BitsInField (mv-bits ++ dv-bits)`.
--              Output: `from-le-bits (mv-bits ++ dv-bits)`.
--
-- Forward direction uses `reconstitute-no-overflow` to extract the
-- field equation from the operational premise.  Backward requires
-- producer obligation O3 to recover the in-field check.
------------------------------------------------------------------------

reconstitute-field-fwd : ∀ {pre s s' d m bits hc} {rand : Maybe Fr}
  → R-instr pre s (reconstitute-field d m bits) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (reconstitute-field d m bits))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
reconstitute-field-fwd {s = s} {d = d} {m = m} {bits = bits}
  (r-reconstitute-field {dv = dv} {mv = mv} ld lm _ _ fits-and-in-field) =
  let mem    = Preprocessed.memory s
      ov     = from-le-bits (take bits (to-le-bits mv) ++
                              take (FR-BITS ∸ bits) (to-le-bits dv))
      (fits-mv , fits-dv , _) = fits-and-in-field
      -- ov ≡ dv · 2^bits + mv.
      ov-eq : ov ≡ (dv *ᶠ pow2-fr bits) +ᶠ mv
      ov-eq = reconstitute-no-overflow dv mv bits fits-mv fits-dv
  in ( dv , mv , ov
     , lookup-extends mem (ov ∷ []) d ld
     , lookup-extends mem (ov ∷ []) m lm
     , lookup-new     mem ov
     , fits-dv , fits-mv , ov-eq
     ) ∷ᴬ []ᴬ

-- Backward direction.
--
-- Requires `BitsInField (mv-bits ++ dv-bits)` (producer obligation
-- O3).  The constraint supplies the fits-in bounds and the
-- arithmetic equation; combined with the no-overflow hypothesis, we
-- can identify the constraint's output with the canonical
-- `from-le-bits (mv-bits ++ dv-bits)` and fire `r-reconstitute-field`.
reconstitute-field-bwd : ∀ {pre s d m bits dv mv v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) d ≡ just dv
  → mem-lookup (Preprocessed.memory s) m ≡ just mv
  → 1 ≤ bits
  → bits ≤ FR-STORED-BITS
  → BitsInField
      (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (reconstitute-field d m bits))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ from-le-bits
           (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv)))
  × R-instr pre s (reconstitute-field d m bits)
      (push-mem s (from-le-bits
                    (take bits (to-le-bits mv) ++
                     take (FR-BITS ∸ bits) (to-le-bits dv))))
reconstitute-field-bwd {pre = pre} {s = s} {d = d} {m = m} {bits = bits}
                       {dv = dv} {mv = mv} {v = v}
  ld lm ge1 le in-field
  ((dv' , mv' , ov , ld' , lm' , lout , fits-dv , fits-mv , ov-eq) ∷ᴬ _) =
  let mem    = Preprocessed.memory s
      dv≡dv' = extend-uniq mem (v ∷ []) d ld ld'
      mv≡mv' = extend-uniq mem (v ∷ []) m lm lm'
      v≡ov   = new-uniq mem v lout
      -- Canonical reconstitution.
      canon  = from-le-bits
                 (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))
      -- Pull `fits-mv`, `fits-dv` back to `mv`, `dv`.
      fits-mv-mv : Fits-in mv bits
      fits-mv-mv = subst (λ z → Fits-in z bits) (sym mv≡mv') fits-mv
      fits-dv-dv : Fits-in dv (FR-BITS ∸ bits)
      fits-dv-dv = subst (λ z → Fits-in z (FR-BITS ∸ bits))
                         (sym dv≡dv') fits-dv
      -- canon ≡ dv · 2^bits + mv.
      canon-eq : canon ≡ (dv *ᶠ pow2-fr bits) +ᶠ mv
      canon-eq = reconstitute-no-overflow dv mv bits fits-mv-mv fits-dv-dv
      -- v ≡ ov ≡ dv' · 2^bits + mv' ≡ dv · 2^bits + mv ≡ canon.
      ov≡sum : ov ≡ (dv *ᶠ pow2-fr bits) +ᶠ mv
      ov≡sum = trans ov-eq (cong₂ _+ᶠ_ (cong (_*ᶠ pow2-fr bits) (sym dv≡dv'))
                                        (sym mv≡mv'))
      v≡canon : v ≡ canon
      v≡canon = trans v≡ov (trans ov≡sum (sym canon-eq))
      r-fired : R-instr pre s (reconstitute-field d m bits)
                  (push-mem s canon)
      r-fired = r-reconstitute-field ld lm ge1 le (fits-mv-mv , fits-dv-dv , in-field)
  in v≡canon , r-fired

------------------------------------------------------------------------
-- less-than(a, b, n)                  (§5.2-footnote)
--
-- Lowering: emits `constraint-less-than out a b bits` using the *padded*
--           bit count `lt-bits bits`.
-- Operational: requires `Fits-in av bits` and `Fits-in bv bits`,
--              outputs `from-bool (bits-lt (take bits …) (take bits …))`.
--
-- Forward direction: pad the bit-bounds via `fits-in-lt-bits`, and
-- transport the comparison via `bits-lt-pad`.  Backward requires the
-- operand bit-bounds (the in-circuit constraint is strictly weaker than
-- the operational rule).
------------------------------------------------------------------------

less-than-fwd : ∀ {pre s s' a b bits hc} {rand : Maybe Fr}
  → R-instr pre s (less-than a b bits) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (less-than a b bits))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
less-than-fwd {s = s} {a = a} {b = b} {bits = bits}
  (r-less-than {av = av} {bv = bv} la lb _ fits) =
  let mem        = Preprocessed.memory s
      (fits-av , fits-bv) = fits
      -- Operational output value.
      op-out     = from-bool (bits-lt (take bits (to-le-bits av))
                                       (take bits (to-le-bits bv)))
      -- Padded output value (what the constraint refers to).
      padded-lt  = bits-lt (take (lt-bits bits) (to-le-bits av))
                            (take (lt-bits bits) (to-le-bits bv))
      -- Padding preserves the comparison.
      pad-eq : padded-lt
             ≡ bits-lt (take bits (to-le-bits av))
                       (take bits (to-le-bits bv))
      pad-eq = bits-lt-pad av bv bits fits-av fits-bv
      -- ov ≡ from-bool padded-lt, derived from op-out ≡ from-bool padded-lt.
      out-eq : op-out ≡ from-bool padded-lt
      out-eq = sym (cong from-bool pad-eq)
  in ( av , bv , op-out
     , lookup-extends mem (op-out ∷ []) a la
     , lookup-extends mem (op-out ∷ []) b lb
     , lookup-new     mem op-out
     , fits-in-lt-bits av bits fits-av
     , fits-in-lt-bits bv bits fits-bv
     , out-eq
     ) ∷ᴬ []ᴬ

-- Backward direction.
--
-- Requires the operand bit-bounds `Fits-in av bits` and `Fits-in bv
-- bits` (the *unpadded* bounds; the constraint only carries `lt-bits
-- bits`).  These let us apply `bits-lt-pad` to bridge the padded
-- constraint-side comparison to the unpadded operational one.
less-than-bwd : ∀ {pre s a b bits av bv v hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a ≡ just av
  → mem-lookup (Preprocessed.memory s) b ≡ just bv
  → bits < FR-BITS
  → Fits-in av bits
  → Fits-in bv bits
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (less-than a b bits))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ from-bool (bits-lt (take bits (to-le-bits av))
                             (take bits (to-le-bits bv))))
  × R-instr pre s (less-than a b bits)
      (push-mem s (from-bool (bits-lt (take bits (to-le-bits av))
                                       (take bits (to-le-bits bv)))))
less-than-bwd {pre = pre} {s = s} {a = a} {b = b} {bits = bits}
              {av = av} {bv = bv} {v = v}
  la lb bound fits-av fits-bv
  ((av' , bv' , ov , la' , lb' , lout , _ , _ , ov-eq) ∷ᴬ _) =
  let mem    = Preprocessed.memory s
      av≡av' = extend-uniq mem (v ∷ []) a la la'
      bv≡bv' = extend-uniq mem (v ∷ []) b lb lb'
      v≡ov   = new-uniq mem v lout
      -- Operational output value (the canonical, unpadded one).
      op-out = from-bool (bits-lt (take bits (to-le-bits av))
                                   (take bits (to-le-bits bv)))
      -- bits-lt-pad bridges the padded constraint-side comparison to the
      -- unpadded operational one.
      pad-eq : bits-lt (take (lt-bits bits) (to-le-bits av))
                       (take (lt-bits bits) (to-le-bits bv))
             ≡ bits-lt (take bits (to-le-bits av))
                       (take bits (to-le-bits bv))
      pad-eq = bits-lt-pad av bv bits fits-av fits-bv
      -- ov = from-bool (bits-lt-padded(av', bv'))
      --    = from-bool (bits-lt-padded(av,  bv))    (subst on av, bv)
      --    = from-bool (bits-lt(av, bv))            (pad-eq)
      --    = op-out.
      ov-padded : ov ≡ from-bool (bits-lt (take (lt-bits bits) (to-le-bits av))
                                           (take (lt-bits bits) (to-le-bits bv)))
      ov-padded = trans ov-eq
                   (cong₂ (λ x y → from-bool (bits-lt (take (lt-bits bits) (to-le-bits x))
                                                       (take (lt-bits bits) (to-le-bits y))))
                          (sym av≡av') (sym bv≡bv'))
      ov≡op : ov ≡ op-out
      ov≡op = trans ov-padded (cong from-bool pad-eq)
      v≡op  : v ≡ op-out
      v≡op  = trans v≡ov ov≡op
      r-fired : R-instr pre s (less-than a b bits) (push-mem s op-out)
      r-fired = r-less-than la lb bound (fits-av , fits-bv)
  in v≡op , r-fired

------------------------------------------------------------------------
-- transient-hash(inputs)
--
-- Lowering: emits `constraint-transient-hash out inputs` with out = nr-wires.
-- Operational: append `transient-hash-fn vs`, where
--   `mem-lookups (Preprocessed.memory s) inputs ≡ just vs`; Δmem = 1.
--
-- Mechanical lookup-plumbing — the constraint references the same
-- `transient-hash-fn` as the operational rule.
------------------------------------------------------------------------

transient-hash-fwd : ∀ {pre s s' inputs hc} {rand : Maybe Fr}
  → R-instr pre s (transient-hash inputs) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (transient-hash inputs))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
transient-hash-fwd {s = s} {inputs = inputs}
  (r-transient-hash {vs = vs} lvs) =
  let mem = Preprocessed.memory s
      ov  = transient-hash-fn vs
  in ( vs , ov
     , mem-lookups-extends mem (ov ∷ []) inputs lvs
     , lookup-new mem ov
     , refl
     ) ∷ᴬ []ᴬ

transient-hash-bwd : ∀ {pre s inputs vs v hc} {rand : Maybe Fr}
  → mem-lookups (Preprocessed.memory s) inputs ≡ just vs
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (transient-hash inputs))
      (mk-witness (Preprocessed.memory s ++ (v ∷ []))
                  (Preprocessed.pis s) rand)
  → (v ≡ transient-hash-fn vs)
  × R-instr pre s (transient-hash inputs) (push-mem s (transient-hash-fn vs))
transient-hash-bwd {pre = pre} {s = s} {inputs = inputs} {vs = vs} {v = v}
  lvs ((vs' , ov , lvs' , lout , ov-eq) ∷ᴬ _) =
  let mem      = Preprocessed.memory s
      lvs-ext  = mem-lookups-extends mem (v ∷ []) inputs lvs
      vs≡vs'   = just-injective (trans (sym lvs-ext) lvs')
      v≡ov     = new-uniq mem v lout
      v≡hash   : v ≡ transient-hash-fn vs
      v≡hash   = trans v≡ov (trans ov-eq (cong transient-hash-fn (sym vs≡vs')))
  in v≡hash , r-transient-hash lvs

------------------------------------------------------------------------
-- persistent-hash(alignment, inputs)
--
-- Lowering: emits `constraint-persistent-hash h₁ h₂ α inputs` with
--           h₁ = nr-wires, h₂ = suc nr-wires.
-- Operational: append `(h₁ , h₂) = persistent-hash-fn α vs` with
--   `mem-lookups (Preprocessed.memory s) inputs ≡ just vs`; Δmem = 2.
------------------------------------------------------------------------

persistent-hash-fwd : ∀ {pre s s' α inputs hc} {rand : Maybe Fr}
  → R-instr pre s (persistent-hash α inputs) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (persistent-hash α inputs))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
persistent-hash-fwd {s = s} {α = α} {inputs = inputs} {hc = hc} {rand = rand}
  (r-persistent-hash {vs = vs} {h₁ = h₁} {h₂ = h₂} lvs hash-eq) =
  let mem    = Preprocessed.memory s
      assoc  = push-mem2-assoc mem h₁ h₂  -- mem ++ h₁ ∷ h₂ ∷ [] ≡ (mem ++ h₁ ∷ []) ++ h₂ ∷ []
      lvs-ext = mem-lookups-extends (mem ++ (h₁ ∷ [])) (h₂ ∷ []) inputs
                  (mem-lookups-extends mem (h₁ ∷ []) inputs lvs)
  in subst (λ m → satisfies-constraints
             (single-instr-constraints hc (length mem) (persistent-hash α inputs))
             (mk-witness m (Preprocessed.pis s) rand))
           (sym assoc)
           (( vs , h₁ , h₂
            , lvs-ext
            , lookup-new-fst mem h₁ h₂
            , lookup-new-snd mem h₁ h₂
            , hash-eq
            ) ∷ᴬ []ᴬ)

persistent-hash-bwd : ∀ {pre s α inputs vs x y hc} {rand : Maybe Fr}
  → mem-lookups (Preprocessed.memory s) inputs ≡ just vs
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (persistent-hash α inputs))
      (mk-witness ((Preprocessed.memory s ++ (x ∷ [])) ++ (y ∷ []))
                  (Preprocessed.pis s) rand)
  → persistent-hash-fn α vs ≡ (x , y)
  × R-instr pre s (persistent-hash α inputs) (push-mem2 s x y)
persistent-hash-bwd {pre = pre} {s = s} {α = α} {inputs = inputs}
  {vs = vs} {x = x} {y = y}
  lvs ((vs' , v1 , v2 , lvs' , lh₁ , lh₂ , hash-eq) ∷ᴬ _) =
  let mem      = Preprocessed.memory s
      lvs-ext  = mem-lookups-extends (mem ++ (x ∷ [])) (y ∷ []) inputs
                   (mem-lookups-extends mem (x ∷ []) inputs lvs)
      vs≡vs'   = just-injective (trans (sym lvs-ext) lvs')
      x≡v1     = new2-uniq-fst mem x y lh₁
      y≡v2     = new2-uniq-snd mem x y lh₂
      hash-eq' : persistent-hash-fn α vs ≡ (x , y)
      hash-eq' = trans (cong (persistent-hash-fn α) vs≡vs')
                       (trans hash-eq
                              (cong₂ _,_ (sym x≡v1) (sym y≡v2)))
  in hash-eq' , r-persistent-hash lvs hash-eq'

------------------------------------------------------------------------
-- hash-to-curve(inputs)
--
-- Lowering: emits `constraint-hash-to-curve c-x c-y inputs` with
--           c-x = nr-wires, c-y = suc nr-wires.
-- Operational: append `(cx, cy) = hash-to-curve-fn vs` with mem-lookups;
--              Δmem = 2.
------------------------------------------------------------------------

hash-to-curve-fwd : ∀ {pre s s' inputs hc} {rand : Maybe Fr}
  → R-instr pre s (hash-to-curve inputs) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (hash-to-curve inputs))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
hash-to-curve-fwd {s = s} {inputs = inputs} {hc = hc} {rand = rand}
  (r-hash-to-curve {vs = vs} {cx = cx} {cy = cy} lvs hash-eq) =
  let mem    = Preprocessed.memory s
      assoc  = push-mem2-assoc mem cx cy
      lvs-ext = mem-lookups-extends (mem ++ (cx ∷ [])) (cy ∷ []) inputs
                  (mem-lookups-extends mem (cx ∷ []) inputs lvs)
  in subst (λ m → satisfies-constraints
             (single-instr-constraints hc (length mem) (hash-to-curve inputs))
             (mk-witness m (Preprocessed.pis s) rand))
           (sym assoc)
           (( vs , cx , cy
            , lvs-ext
            , lookup-new-fst mem cx cy
            , lookup-new-snd mem cx cy
            , hash-eq
            ) ∷ᴬ []ᴬ)

hash-to-curve-bwd : ∀ {pre s inputs vs x y hc} {rand : Maybe Fr}
  → mem-lookups (Preprocessed.memory s) inputs ≡ just vs
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (hash-to-curve inputs))
      (mk-witness ((Preprocessed.memory s ++ (x ∷ [])) ++ (y ∷ []))
                  (Preprocessed.pis s) rand)
  → hash-to-curve-fn vs ≡ (x , y)
  × R-instr pre s (hash-to-curve inputs) (push-mem2 s x y)
hash-to-curve-bwd {pre = pre} {s = s} {inputs = inputs}
  {vs = vs} {x = x} {y = y}
  lvs ((vs' , cx , cy , lvs' , lcx , lcy , hash-eq) ∷ᴬ _) =
  let mem      = Preprocessed.memory s
      lvs-ext  = mem-lookups-extends (mem ++ (x ∷ [])) (y ∷ []) inputs
                   (mem-lookups-extends mem (x ∷ []) inputs lvs)
      vs≡vs'   = just-injective (trans (sym lvs-ext) lvs')
      x≡cx     = new2-uniq-fst mem x y lcx
      y≡cy     = new2-uniq-snd mem x y lcy
      hash-eq' : hash-to-curve-fn vs ≡ (x , y)
      hash-eq' = trans (cong hash-to-curve-fn vs≡vs')
                       (trans hash-eq
                              (cong₂ _,_ (sym x≡cx) (sym y≡cy)))
  in hash-eq' , r-hash-to-curve lvs hash-eq'

------------------------------------------------------------------------
-- ec-add(a-x, a-y, b-x, b-y)
--
-- Lowering: emits `constraint-ec-add c-x c-y a-x a-y b-x b-y` with
--           c-x = nr-wires, c-y = suc nr-wires.
-- Operational: requires `ec-add-pts ax ay bx by ≡ just (cx , cy)`;
--              Δmem = 2.
--
-- The chip primitive `ec-add-pts` is partial (returns `nothing` for
-- off-curve inputs).  Constraint and operational rule both carry the
-- `≡ just (cx , cy)` premise.
------------------------------------------------------------------------

ec-add-fwd : ∀ {pre s s' a-x a-y b-x b-y hc} {rand : Maybe Fr}
  → R-instr pre s (ec-add a-x a-y b-x b-y) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (ec-add a-x a-y b-x b-y))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
ec-add-fwd {s = s} {a-x = a-x} {a-y = a-y} {b-x = b-x} {b-y = b-y}
           {hc = hc} {rand = rand}
  (r-ec-add {ax = ax} {ay = ay} {bx = bx} {by = by}
            {cx = cx} {cy = cy} lax lay lbx lby add-eq) =
  let mem    = Preprocessed.memory s
      assoc  = push-mem2-assoc mem cx cy
  in subst (λ m → satisfies-constraints
             (single-instr-constraints hc (length mem) (ec-add a-x a-y b-x b-y))
             (mk-witness m (Preprocessed.pis s) rand))
           (sym assoc)
           (( ax , ay , bx , by , cx , cy
            , lookup-extend2 mem cx cy a-x lax
            , lookup-extend2 mem cx cy a-y lay
            , lookup-extend2 mem cx cy b-x lbx
            , lookup-extend2 mem cx cy b-y lby
            , lookup-new-fst mem cx cy
            , lookup-new-snd mem cx cy
            , add-eq
            ) ∷ᴬ []ᴬ)

ec-add-bwd : ∀ {pre s a-x a-y b-x b-y ax ay bx by x y hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a-x ≡ just ax
  → mem-lookup (Preprocessed.memory s) a-y ≡ just ay
  → mem-lookup (Preprocessed.memory s) b-x ≡ just bx
  → mem-lookup (Preprocessed.memory s) b-y ≡ just by
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (ec-add a-x a-y b-x b-y))
      (mk-witness ((Preprocessed.memory s ++ (x ∷ [])) ++ (y ∷ []))
                  (Preprocessed.pis s) rand)
  → ec-add-pts ax ay bx by ≡ just (x , y)
  × R-instr pre s (ec-add a-x a-y b-x b-y) (push-mem2 s x y)
ec-add-bwd {pre = pre} {s = s} {a-x = a-x} {a-y = a-y} {b-x = b-x} {b-y = b-y}
           {ax = ax} {ay = ay} {bx = bx} {by = by} {x = x} {y = y}
  lax lay lbx lby
  ((ax' , ay' , bx' , by' , cx , cy
    , lax' , lay' , lbx' , lby' , lcx , lcy , add-eq) ∷ᴬ _) =
  let mem    = Preprocessed.memory s
      mem'   = (mem ++ (x ∷ [])) ++ (y ∷ [])
      ax≡ax' = lookup-uniq mem' a-x (lookup-extend2 mem x y a-x lax) lax'
      ay≡ay' = lookup-uniq mem' a-y (lookup-extend2 mem x y a-y lay) lay'
      bx≡bx' = lookup-uniq mem' b-x (lookup-extend2 mem x y b-x lbx) lbx'
      by≡by' = lookup-uniq mem' b-y (lookup-extend2 mem x y b-y lby) lby'
      x≡cx   = new2-uniq-fst mem x y lcx
      y≡cy   = new2-uniq-snd mem x y lcy
      add-eq' : ec-add-pts ax ay bx by ≡ just (x , y)
      add-eq' = trans (cong₄ ec-add-pts ax≡ax' ay≡ay' bx≡bx' by≡by')
                      (trans add-eq (cong just (cong₂ _,_ (sym x≡cx) (sym y≡cy))))
  in add-eq' , r-ec-add lax lay lbx lby add-eq'

------------------------------------------------------------------------
-- ec-mul(a-x, a-y, scalar)
--
-- Same shape as ec-add but with 3 input wires.
------------------------------------------------------------------------

ec-mul-fwd : ∀ {pre s s' a-x a-y scalar hc} {rand : Maybe Fr}
  → R-instr pre s (ec-mul a-x a-y scalar) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (ec-mul a-x a-y scalar))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
ec-mul-fwd {s = s} {a-x = a-x} {a-y = a-y} {scalar = scalar}
           {hc = hc} {rand = rand}
  (r-ec-mul {ax = ax} {ay = ay} {sc = sc} {cx = cx} {cy = cy}
            lax lay lsc mul-eq) =
  let mem    = Preprocessed.memory s
      assoc  = push-mem2-assoc mem cx cy
  in subst (λ m → satisfies-constraints
             (single-instr-constraints hc (length mem) (ec-mul a-x a-y scalar))
             (mk-witness m (Preprocessed.pis s) rand))
           (sym assoc)
           (( ax , ay , sc , cx , cy
            , lookup-extend2 mem cx cy a-x lax
            , lookup-extend2 mem cx cy a-y lay
            , lookup-extend2 mem cx cy scalar lsc
            , lookup-new-fst mem cx cy
            , lookup-new-snd mem cx cy
            , mul-eq
            ) ∷ᴬ []ᴬ)

ec-mul-bwd : ∀ {pre s a-x a-y scalar ax ay sc x y hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) a-x ≡ just ax
  → mem-lookup (Preprocessed.memory s) a-y ≡ just ay
  → mem-lookup (Preprocessed.memory s) scalar ≡ just sc
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (ec-mul a-x a-y scalar))
      (mk-witness ((Preprocessed.memory s ++ (x ∷ [])) ++ (y ∷ []))
                  (Preprocessed.pis s) rand)
  → ec-mul-pt ax ay sc ≡ just (x , y)
  × R-instr pre s (ec-mul a-x a-y scalar) (push-mem2 s x y)
ec-mul-bwd {pre = pre} {s = s} {a-x = a-x} {a-y = a-y} {scalar = scalar}
           {ax = ax} {ay = ay} {sc = sc} {x = x} {y = y}
  lax lay lsc
  ((ax' , ay' , sc' , cx , cy
    , lax' , lay' , lsc' , lcx , lcy , mul-eq) ∷ᴬ _) =
  let mem    = Preprocessed.memory s
      mem'   = (mem ++ (x ∷ [])) ++ (y ∷ [])
      ax≡ax' = lookup-uniq mem' a-x   (lookup-extend2 mem x y a-x   lax) lax'
      ay≡ay' = lookup-uniq mem' a-y   (lookup-extend2 mem x y a-y   lay) lay'
      sc≡sc' = lookup-uniq mem' scalar (lookup-extend2 mem x y scalar lsc) lsc'
      x≡cx   = new2-uniq-fst mem x y lcx
      y≡cy   = new2-uniq-snd mem x y lcy
      mul-eq' : ec-mul-pt ax ay sc ≡ just (x , y)
      mul-eq' = trans (cong₃ ec-mul-pt ax≡ax' ay≡ay' sc≡sc')
                      (trans mul-eq (cong just (cong₂ _,_ (sym x≡cx) (sym y≡cy))))
  in mul-eq' , r-ec-mul lax lay lsc mul-eq'

------------------------------------------------------------------------
-- ec-mul-generator(scalar)
--
-- Lowering: emits `constraint-ec-mul-generator c-x c-y scalar` with
--           c-x = nr-wires, c-y = suc nr-wires.
-- Operational: append `(cx, cy) = ec-mul-gen sc` (total function);
--              Δmem = 2.
------------------------------------------------------------------------

ec-mul-generator-fwd : ∀ {pre s s' scalar hc} {rand : Maybe Fr}
  → R-instr pre s (ec-mul-generator scalar) s'
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (ec-mul-generator scalar))
      (mk-witness (Preprocessed.memory s') (Preprocessed.pis s') rand)
ec-mul-generator-fwd {s = s} {scalar = scalar} {hc = hc} {rand = rand}
  (r-ec-mul-generator {sc = sc} {cx = cx} {cy = cy} lsc gen-eq) =
  let mem    = Preprocessed.memory s
      assoc  = push-mem2-assoc mem cx cy
  in subst (λ m → satisfies-constraints
             (single-instr-constraints hc (length mem) (ec-mul-generator scalar))
             (mk-witness m (Preprocessed.pis s) rand))
           (sym assoc)
           (( sc , cx , cy
            , lookup-extend2 mem cx cy scalar lsc
            , lookup-new-fst mem cx cy
            , lookup-new-snd mem cx cy
            , gen-eq
            ) ∷ᴬ []ᴬ)

ec-mul-generator-bwd : ∀ {pre s scalar sc x y hc} {rand : Maybe Fr}
  → mem-lookup (Preprocessed.memory s) scalar ≡ just sc
  → satisfies-constraints
      (single-instr-constraints hc (length (Preprocessed.memory s))
         (ec-mul-generator scalar))
      (mk-witness ((Preprocessed.memory s ++ (x ∷ [])) ++ (y ∷ []))
                  (Preprocessed.pis s) rand)
  → ec-mul-gen sc ≡ (x , y)
  × R-instr pre s (ec-mul-generator scalar) (push-mem2 s x y)
ec-mul-generator-bwd {pre = pre} {s = s} {scalar = scalar}
                     {sc = sc} {x = x} {y = y}
  lsc ((sc' , cx , cy , lsc' , lcx , lcy , gen-eq) ∷ᴬ _) =
  let mem    = Preprocessed.memory s
      mem'   = (mem ++ (x ∷ [])) ++ (y ∷ [])
      sc≡sc' = lookup-uniq mem' scalar (lookup-extend2 mem x y scalar lsc) lsc'
      x≡cx   = new2-uniq-fst mem x y lcx
      y≡cy   = new2-uniq-snd mem x y lcy
      gen-eq' : ec-mul-gen sc ≡ (x , y)
      gen-eq' = trans (cong ec-mul-gen sc≡sc')
                      (trans gen-eq (cong₂ _,_ (sym x≡cx) (sym y≡cy)))
  in gen-eq' , r-ec-mul-generator lsc gen-eq'
