{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.ObligationsSoundness (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.FieldProperties ⋯
open import zkir-v2.Encoding ⋯

------------------------------------------------------------------------
-- Soundness of producer obligations
--
-- For each obligation we have a static scan (in `Obligations.agda`) and
-- a witness-bearing trace predicate (`O2-Trace`/`O3-Trace`).  This
-- module proves the *connection* between those static facts and the
-- dynamic relational semantics `R-instr`/`R-instrs`:
--
--   • O2-sound  — if the scan adds `i` to bool-known, then for every
--                 reachable state with `mem-lookup mem i ≡ just v`, the
--                 value `v` is in {0, 1} (`is-bit v`).
--
--   • O3-sound  — analogous for the known partial map: if the scan
--                 has `lookupᵐ bm i ≡ just n`, then `Fits-in v n`.
--
--   • integration lemmas — `o2-known-is-bit` / `o3-known-fits` extract
--                 an `is-bit` / `Fits-in` witness for a specific operand,
--                 consumed by the per-instruction backward lemmas.
--
-- The proof is by induction along the `R-instrs` trace, threading an
-- invariant `O2-Inv (i, bk) s` saying:
--
--   • `i ≡ length (Preprocessed.memory s)`     (index alignment)
--   • every wire in `bk` is bound to an `is-bit` value in `mem s`.
--
-- The 29-constructor case analysis on `R-instr` (26 instructions, with
-- `pi-skip`/`public-input`/`private-input` split by guard) mirrors the
-- `O2-record` / `O3-record` discriminations.
------------------------------------------------------------------------

-- The `to-bool-*` and `fits-from-le-bits-*` facts are derived from the
-- `Assumptions` laws in `FieldProperties`/`Encoding`, not assumed.
open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
open import zkir-v2.SemanticsLemmas ⋯
  using ( init-state-memory; init-state-num-inputs
        ; consume-pub-out-mem; consume-priv-mem )
open import zkir-v2.Circuit ⋯ using (is-bit)
open import zkir-v2.Obligations ⋯

open import Data.Bool      using (Bool; true; false; _∧_; if_then_else_; T)
open import Data.Bool.Properties using (T-∧)
import Data.Bool as Bool
open import Data.List      using (List; []; _∷_; _++_; length; take; drop)
open import Data.List.Properties using (++-assoc)
open import Data.List.Relation.Unary.Any using (here; there)
open import Data.Maybe     using (Maybe; nothing; just; _>>=_)
open import Data.Maybe.Properties using (just-injective)
open import Data.Nat       using (ℕ; zero; suc; _+_; _∸_; _≟_; _≡ᵇ_; _<_; z<s; s<s)
open import Data.List.Membership.DecPropositional (_≟_) using (_∈_; _∈?_)
open import Data.Nat.Properties
  using (+-identityʳ; +-comm; ≡ᵇ⇒≡; m<n⇒m<1+n; n<1+n; <-irrefl; <⇒≯)
open import Function.Bundles using (Equivalence)
open import Data.Product   using (_×_; _,_; ∃-syntax; proj₁; proj₂)
open import Data.Sum       using (_⊎_; inj₁; inj₂)
open import Data.Empty     using (⊥; ⊥-elim)
open import Data.Unit      using (⊤; tt)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; subst)
open import Relation.Nullary using (¬_; yes; no)

------------------------------------------------------------------------
-- Common helpers
------------------------------------------------------------------------

private

  -- Length of an "appended ∷[]" memory.
  length-++-one : ∀ {A : Set} (xs : List A) (y : A)
    → length (xs ++ (y ∷ [])) ≡ suc (length xs)
  length-++-one [] y = refl
  length-++-one (x ∷ xs) y = cong suc (length-++-one xs y)

  length-++-two : ∀ {A : Set} (xs : List A) (y z : A)
    → length (xs ++ (y ∷ z ∷ [])) ≡ suc (suc (length xs))
  length-++-two [] y z = refl
  length-++-two (x ∷ xs) y z = cong suc (length-++-two xs y z)

  -- Lookup at exactly `length mem` returns the appended value.
  lookup-new : ∀ (mem : List Fr) v
    → mem-lookup (mem ++ (v ∷ [])) (length mem) ≡ just v
  lookup-new []       v = refl
  lookup-new (x ∷ xs) v = lookup-new xs v

  -- Two cells.
  lookup-new-fst : ∀ (mem : List Fr) x y
    → mem-lookup (mem ++ (x ∷ y ∷ [])) (length mem) ≡ just x
  lookup-new-fst []       x y = refl
  lookup-new-fst (z ∷ zs) x y = lookup-new-fst zs x y

  lookup-new-snd : ∀ (mem : List Fr) x y
    → mem-lookup (mem ++ (x ∷ y ∷ [])) (suc (length mem)) ≡ just y
  lookup-new-snd []       x y = refl
  lookup-new-snd (z ∷ zs) x y = lookup-new-snd zs x y

  ------------------------------------------------------------------
  -- Bool reasoning lemmas
  ------------------------------------------------------------------

  ∧-true : ∀ {a b : Bool} → T (a ∧ b) → T a × T b
  ∧-true {a} {b} = Equivalence.to (T-∧ {a} {b})

  ≡ᵇ-true : ∀ {m n : ℕ} → (m ≡ᵇ n) ≡ true → m ≡ n
  ≡ᵇ-true {m} {n} eq = ≡ᵇ⇒≡ m n (subst T (sym eq) tt)

------------------------------------------------------------------------
-- Set membership is the standard-library propositional `_∈_` on
-- `IndexSet = List Index`, decided by `_∈?_` (both from `Obligations`).
------------------------------------------------------------------------

------------------------------------------------------------------------
-- O2 step invariant
--
-- `O2-Inv (i, bk) s` says:
--   • `i ≡ length (memory s)`              — the scan's wire counter is
--                                            in sync with the program
--                                            memory length.
--   • every `j ∈ bk` looks up to an
--     `is-bit` value in `memory s`.
------------------------------------------------------------------------

-- An `n < i` follows from a successful mem-lookup at index n in a
-- memory of length matching i.
private
  lookup-bound : ∀ (mem : List Fr) (j : Index) {val : Fr}
    → mem-lookup mem j ≡ just val
    → ∀ {i₀} → i₀ ≡ length mem
    → j < i₀
  lookup-bound (x ∷ xs) zero    eq   refl = z<s
  lookup-bound (x ∷ xs) (suc j) eq   refl =
    s<s (lookup-bound xs j eq refl)
  lookup-bound []       _       ()   _

-- Generic per-wire-property invariant shared by O2 and O3.  The
-- known-structure `k : K` records, for some wires `j`, a payload `d : D`
-- (`Mem k j d`); the invariant says every such wire's value satisfies
-- the tracked property `P v d`, and every recorded index is `< i`.
record KInv {K D : Set} (Mem : K → Index → D → Set) (P : Fr → D → Set)
            (acc : ℕ × K) (s : Preprocessed) : Set where
  constructor mk-kinv
  field
    idx-sync : proj₁ acc ≡ length (Preprocessed.memory s)
    bound    : ∀ {j d} → Mem (proj₂ acc) j d → j < proj₁ acc
    known    : ∀ {j d v}
      → Mem (proj₂ acc) j d
      → mem-lookup (Preprocessed.memory s) j ≡ just v
      → P v d

open KInv public

-- O2 tracks a boolean-known set: payload is trivial, property is `is-bit`.
O2-Inv : ℕ × IndexSet → Preprocessed → Set
O2-Inv = KInv {IndexSet} {⊤} (λ bk j _ → j ∈ bk) (λ v _ → is-bit v)

------------------------------------------------------------------------
-- Base case: at the initial state with `acc = (num-inputs src, [])` the
-- invariant holds vacuously.
------------------------------------------------------------------------

o2-inv-init : ∀ {src pre s₀}
  → init-state src pre ≡ just s₀
  → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src
  → O2-Inv (IrSource.num-inputs src , []) s₀
o2-inv-init {src} {pre} {s₀} eq lenEq =
  mk-kinv idx-eq empty-bound empty-bk-bits
  where
    mem-eq : Preprocessed.memory s₀ ≡ ProofPreimage.inputs pre
    mem-eq = init-state-memory src pre s₀ eq

    len-eq : length (Preprocessed.memory s₀) ≡ length (ProofPreimage.inputs pre)
    len-eq = cong length mem-eq

    idx-eq : IrSource.num-inputs src ≡ length (Preprocessed.memory s₀)
    idx-eq = trans (sym lenEq) (sym len-eq)

    empty-bound : ∀ {j} → j ∈ [] → j < IrSource.num-inputs src
    empty-bound ()

    empty-bk-bits : ∀ {j v}
      → j ∈ []
      → mem-lookup (Preprocessed.memory s₀) j ≡ just v
      → is-bit v
    empty-bk-bits ()

------------------------------------------------------------------------
-- Per-step preservation
--
-- The workhorse:  if `R-instr pre s instr s'` and `O2-step instr acc ≡
-- just acc'` and the invariant held at `(acc, s)`, then it holds at
-- `(acc', s')`.
--
-- We case-split on `instr`.  For each case we know the shape of `s'`
-- (from the R-instr constructor) and the shape of `acc'` (from
-- `O2-step`).  Memory-preserving cases inherit the invariant; memory-
-- extending cases need a fresh witness for the new wire index when
-- the scan adds it to `bk`.
------------------------------------------------------------------------

private

  ------------------------------------------------------------------
  -- Push-mem and lookup helpers
  ------------------------------------------------------------------

  -- Positional characterization of a lookup into an extended memory:
  -- `mem-lookup (mem ++ (newv ∷ [])) j` returns:
  --   • the original `mem-lookup mem j` if defined,
  --   • else `just newv` at j = length mem,
  --   • else nothing.
  lookup-shrink-or-new : ∀ (mem : List Fr) newv j v
    → mem-lookup (mem ++ (newv ∷ [])) j ≡ just v
    → (mem-lookup mem j ≡ just v) ⊎ ((j ≡ length mem) × (v ≡ newv))
  lookup-shrink-or-new []       newv zero    .newv refl = inj₂ (refl , refl)
  lookup-shrink-or-new []       newv (suc j) v    ()
  lookup-shrink-or-new (x ∷ xs) newv zero    .x   refl = inj₁ refl
  lookup-shrink-or-new (x ∷ xs) newv (suc j) v    eq
    with lookup-shrink-or-new xs newv j v eq
  ... | inj₁ p        = inj₁ p
  ... | inj₂ (q , vr) = inj₂ (cong suc q , vr)

  -- Two-cell variant: lookup in mem ++ (x ∷ y ∷ []) decomposes into:
  --   • lookup mem j (if j < length mem)
  --   • j = length mem with value x
  --   • j = suc (length mem) with value y
  lookup-shrink-or-new-2 : ∀ (mem : List Fr) x y j v
    → mem-lookup (mem ++ (x ∷ y ∷ [])) j ≡ just v
    → (mem-lookup mem j ≡ just v)
    ⊎ ((j ≡ length mem) × (v ≡ x))
    ⊎ ((j ≡ suc (length mem)) × (v ≡ y))
  lookup-shrink-or-new-2 []       x y zero          .x refl = inj₂ (inj₁ (refl , refl))
  lookup-shrink-or-new-2 []       x y (suc zero)    .y refl = inj₂ (inj₂ (refl , refl))
  lookup-shrink-or-new-2 []       x y (suc (suc j)) v  ()
  lookup-shrink-or-new-2 (z ∷ zs) x y zero          .z refl = inj₁ refl
  lookup-shrink-or-new-2 (z ∷ zs) x y (suc j)       v  eq
    with lookup-shrink-or-new-2 zs x y j v eq
  ... | inj₁ p              = inj₁ p
  ... | inj₂ (inj₁ (q , vr)) = inj₂ (inj₁ (cong suc q , vr))
  ... | inj₂ (inj₂ (q , vr)) = inj₂ (inj₂ (cong suc q , vr))

------------------------------------------------------------------------
-- O2 step preservation
--
-- Given the invariant at (i, bk) and a successful one-step transition
-- `R-instr pre s instr s'` matched by `O2-step instr (i, bk) ≡ just
-- acc'`, the invariant holds at (acc', s').
--
-- The proof is a 29-case dispatch on `R-instr`.  Most cases are
-- trivial; the "boolean-producing" instructions (test-eq, less-than,
-- not, cond-select with both branches in bk, copy of bk member) need
-- the actual witness derivation.
------------------------------------------------------------------------

-- Witness lemma:  `from-bool b` is always is-bit.
from-bool-is-bit : ∀ (b : Bool) → is-bit (from-bool b)
from-bool-is-bit false = inj₁ refl
from-bool-is-bit true  = inj₂ refl

-- Witness lemma:  if `to-bool v ≡ just _`, then `is-bit v`.
to-bool→is-bit : ∀ {v sel} → to-bool v ≡ just sel → is-bit v
to-bool→is-bit {sel = true}  eq = inj₂ (to-bool-true  eq)
to-bool→is-bit {sel = false} eq = inj₁ (to-bool-false eq)

-- A `(mem-lookup mem a >>= to-bool) ≡ just sel` can be decomposed into
-- the underlying field value plus a `to-bool` evidence on it.
extract-to-bool : ∀ (mem : List Fr) (a : Index) {sel}
  → (mem-lookup mem a >>= to-bool) ≡ just sel
  → ∃-syntax λ av → mem-lookup mem a ≡ just av × to-bool av ≡ just sel
extract-to-bool mem a {sel} eq = aux (mem-lookup mem a) refl eq
  where
    aux : ∀ (m : Maybe Fr)
        → mem-lookup mem a ≡ m
        → (m >>= to-bool) ≡ just sel
        → ∃-syntax λ av → mem-lookup mem a ≡ just av × to-bool av ≡ just sel
    aux nothing  _    ()
    aux (just x) m-eq eq' = x , m-eq , eq'

------------------------------------------------------------------------
-- Step-preservation helpers (frame lemmas)
--
-- We package the three classes of memory transitions:
--   • frame-no-grow         : memory unchanged.
--   • frame-push-mem        : memory grows by one cell.
--   • frame-push-mem-2      : memory grows by two cells.
-- Each takes the (i, bk)-side accumulator update and produces the
-- post-state O2-Inv.
------------------------------------------------------------------------

private

  ------------------------------------------------------------------
  -- Generic frames: memory transitions that leave the known-structure
  -- untouched.  Shared by O2 and O3 (aliased below).
  ------------------------------------------------------------------

  -- Memory unchanged.
  gen-frame-no-grow : ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set}
                        {i k s s'}
    → Preprocessed.memory s' ≡ Preprocessed.memory s
    → KInv Mem P (i , k) s
    → KInv Mem P (i , k) s'
  gen-frame-no-grow mem-eq inv =
    mk-kinv
      (trans (idx-sync inv) (sym (cong length mem-eq)))
      (bound inv)
      (λ m? lookup-eq →
        known inv m?
          (trans (cong (λ m → mem-lookup m _) (sym mem-eq)) lookup-eq))

  -- Memory grows by one cell (push-mem).
  gen-frame-push-mem : ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set}
                         {i k s newv}
    → KInv Mem P (i , k) s
    → KInv Mem P (suc i , k) (push-mem s newv)
  gen-frame-push-mem {Mem = Mem} {P = P} {i = i} {k = k} {s = s} {newv = newv} inv =
    mk-kinv idx-eq bnd bits
    where
      mem = Preprocessed.memory s
      idx-eq : suc i ≡ length (Preprocessed.memory (push-mem s newv))
      idx-eq = trans (cong suc (idx-sync inv)) (sym (length-++-one mem newv))
      bnd : ∀ {j d} → Mem k j d → j < suc i
      bnd p = m<n⇒m<1+n (bound inv p)
      bits : ∀ {j d w} → Mem k j d
                       → mem-lookup (Preprocessed.memory (push-mem s newv)) j ≡ just w
                       → P w d
      bits {j} {d} {w} m? lookup-eq
        with lookup-shrink-or-new mem newv j w lookup-eq
      ... | inj₁ p          = known inv m? p
      ... | inj₂ (j≡L , _)  =
        ⊥-elim (<-irrefl refl
                  (subst (_< i) (trans j≡L (sym (idx-sync inv))) (bound inv m?)))

  -- Memory grows by two cells (push-mem2).
  gen-frame-push-mem-2 : ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set}
                           {i k s x y}
    → KInv Mem P (i , k) s
    → KInv Mem P (suc (suc i) , k) (push-mem2 s x y)
  gen-frame-push-mem-2 {Mem = Mem} {P = P} {i = i} {k = k} {s = s} {x = x} {y = y} inv =
    mk-kinv idx-eq bnd bits
    where
      mem = Preprocessed.memory s
      idx-eq : suc (suc i) ≡ length (Preprocessed.memory (push-mem2 s x y))
      idx-eq = trans (cong suc (cong suc (idx-sync inv))) (sym (length-++-two mem x y))
      bnd : ∀ {j d} → Mem k j d → j < suc (suc i)
      bnd p = m<n⇒m<1+n (m<n⇒m<1+n (bound inv p))
      bits : ∀ {j d w} → Mem k j d
                       → mem-lookup (Preprocessed.memory (push-mem2 s x y)) j ≡ just w
                       → P w d
      bits {j} {d} {w} m? lookup-eq
        with lookup-shrink-or-new-2 mem x y j w lookup-eq
      ... | inj₁ p                    = known inv m? p
      ... | inj₂ (inj₁ (j≡L , _))     =
        ⊥-elim (<-irrefl refl
                  (subst (_< i) (trans j≡L (sym (idx-sync inv))) (bound inv m?)))
      ... | inj₂ (inj₂ (j≡sucL , _))  =
        ⊥-elim (<⇒≯ (n<1+n _)
                  (subst (_< i)
                         (trans j≡sucL (cong suc (sym (idx-sync inv))))
                         (bound inv m?)))

  ------------------------------------------------------------------
  -- (a) Memory unchanged, bk extended by `v` (constrain-* cases).
  --     Requires v < i (operand lookup-bound) and an is-bit witness
  --     for the value at v.
  ------------------------------------------------------------------
  frame-no-grow-add-v : ∀ {i bk s s' v vv}
    → Preprocessed.memory s' ≡ Preprocessed.memory s
    → mem-lookup (Preprocessed.memory s) v ≡ just vv
    → is-bit vv
    → O2-Inv (i , bk) s
    → O2-Inv (i , insert v bk) s'
  frame-no-grow-add-v {i = i} {bk = bk} {s = s} {s' = s'} {v = v} {vv = vv}
                       mem-eq lv vv-bit inv =
    mk-kinv idx-eq bnd bits
    where
      idx-eq : i ≡ length (Preprocessed.memory s')
      idx-eq = trans (idx-sync inv) (sym (cong length mem-eq))

      v<i : v < i
      v<i = lookup-bound (Preprocessed.memory s) v lv (idx-sync inv)

      bnd : ∀ {j} → j ∈ (insert v bk) → j < i
      bnd {j} (here eq) = subst (_< i) (sym eq) v<i
      bnd (there p) = bound inv p

      bits : ∀ {j w} → j ∈ (insert v bk)
                     → mem-lookup (Preprocessed.memory s') j ≡ just w
                     → is-bit w
      bits {j} {w} (here j≡v) lookup-eq =
        let 
            lookup-at-v : mem-lookup (Preprocessed.memory s) v ≡ just w
            lookup-at-v =
              subst (λ k → mem-lookup (Preprocessed.memory s) k ≡ just w)
                    j≡v
                    (trans (cong (λ m → mem-lookup m j) (sym mem-eq)) lookup-eq)

            vv≡w : vv ≡ w
            vv≡w = just-injective (trans (sym lv) lookup-at-v)
        in subst is-bit vv≡w vv-bit
      bits (there p) lookup-eq =
        known inv p (trans (cong (λ m → mem-lookup m _) (sym mem-eq)) lookup-eq)

  ------------------------------------------------------------------
  -- (b) Memory grows by one cell, bk records the new index.
  --     Requires an is-bit witness for the appended value.
  ------------------------------------------------------------------
  frame-push-mem-record : ∀ {i bk s newv}
    → is-bit newv
    → O2-Inv (i , bk) s
    → O2-Inv (suc i , insert i bk) (push-mem s newv)
  frame-push-mem-record {i = i} {bk = bk} {s = s} {newv = newv}
                         newv-bit inv =
    mk-kinv idx-eq bnd bits
    where
      mem  = Preprocessed.memory s
      mem' = mem ++ (newv ∷ [])

      idx-eq : suc i ≡ length (Preprocessed.memory (push-mem s newv))
      idx-eq = trans (cong suc (idx-sync inv)) (sym (length-++-one mem newv))

      bnd : ∀ {j} → j ∈ (insert i bk) → j < suc i
      bnd {j} (here eq) = subst (_< suc i) (sym eq) (n<1+n i)
      bnd (there p) = m<n⇒m<1+n (bound inv p)

      bits : ∀ {j w} → j ∈ (insert i bk)
                     → mem-lookup (Preprocessed.memory (push-mem s newv)) j ≡ just w
                     → is-bit w
      bits {j} {w} (here j≡i) lookup-eq =
        let lookup-at-i : mem-lookup (mem ++ (newv ∷ [])) i ≡ just w
            lookup-at-i =
              subst (λ k → mem-lookup (mem ++ (newv ∷ [])) k ≡ just w)
                    j≡i lookup-eq

            lookup-at-i-new : mem-lookup (mem ++ (newv ∷ [])) i ≡ just newv
            lookup-at-i-new =
              subst (λ k → mem-lookup (mem ++ (newv ∷ [])) k ≡ just newv)
                    (sym (idx-sync inv))
                    (lookup-new mem newv)

            newv≡w : newv ≡ w
            newv≡w = just-injective (trans (sym lookup-at-i-new) lookup-at-i)
        in subst is-bit newv≡w newv-bit
      bits {j} {w} (there p) lookup-eq
        with lookup-shrink-or-new mem newv j w lookup-eq
      ... | inj₁ q          = known inv p q
      ... | inj₂ (j≡L , _)  =
        ⊥-elim (<-irrefl refl (subst (_< i) (trans j≡L (sym (idx-sync inv))) (bound inv p)))

------------------------------------------------------------------------
-- Frame helpers for the O2 preservation theorem
--
-- Coercion-bundled frame steps producing the invariant at the index
-- `O2-step` demands for each memory growth (Δmem = 0, 1, or 2).
------------------------------------------------------------------------

private
  -- Memory grows by two cells via iterated push-mem, known-structure
  -- untouched (div-mod-power-of-two's memory shape).  Shared by O2/O3.
  gen-frame-push-mem-push-mem :
    ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set} {i k s x y}
    → KInv Mem P (i , k) s
    → KInv Mem P (suc (suc i) , k) (push-mem (push-mem s x) y)
  gen-frame-push-mem-push-mem {i = i} {k = k} {s = s} {x = x} {y = y} inv =
    let mem    = Preprocessed.memory s
        mem-eq : Preprocessed.memory (push-mem (push-mem s x) y)
               ≡ Preprocessed.memory (push-mem2 s x y)
        mem-eq = ++-assoc mem (x ∷ []) (y ∷ [])
        inv2   = gen-frame-push-mem-2 inv
    in mk-kinv
         (trans (idx-sync inv2) (sym (cong length mem-eq)))
         (bound inv2)
         (λ look lookup-eq →
            known inv2 look
              (trans (cong (λ m → mem-lookup m _) (sym mem-eq)) lookup-eq))

private
  -- Sugar:  i + 0 ≡ i  and  i + 1 ≡ suc i  and  i + 2 ≡ suc (suc i).
  +-0-r : ∀ i → i + 0 ≡ i
  +-0-r = +-identityʳ

  +-1-r : ∀ i → i + 1 ≡ suc i
  +-1-r i = +-comm i 1

  +-2-r : ∀ i → i + 2 ≡ suc (suc i)
  +-2-r i = +-comm i 2

  -- Coerce a `KInv` along an idx-counter equality.  Shared by O2/O3;
  -- the `refl` match avoids the underscore-laden `subst (λ k → KInv …)`
  -- shape that confuses meta inference.
  gen-inv-coerce-idx : ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set}
                         {i i' k s}
    → i ≡ i'
    → KInv Mem P (i , k) s
    → KInv Mem P (i' , k) s
  gen-inv-coerce-idx refl inv = inv

  -- Coercion-bundled frame steps.  Each produces the invariant at the
  -- `i + Δmem` index that `O2-step` / `O3-step` demands, hiding the
  -- `+-Δ-r` index arithmetic that otherwise clutters every clause.
  step-0 : ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set} {i k s s'}
    → Preprocessed.memory s' ≡ Preprocessed.memory s
    → KInv Mem P (i , k) s → KInv Mem P (i + 0 , k) s'
  step-0 {i = i} eq inv =
    gen-inv-coerce-idx (sym (+-0-r i)) (gen-frame-no-grow eq inv)

  step-1 : ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set} {i k s newv}
    → KInv Mem P (i , k) s → KInv Mem P (i + 1 , k) (push-mem s newv)
  step-1 {i = i} inv =
    gen-inv-coerce-idx (sym (+-1-r i)) (gen-frame-push-mem inv)

  step-1-rec : ∀ {i bk s newv} → is-bit newv
    → O2-Inv (i , bk) s → O2-Inv (i + 1 , insert i bk) (push-mem s newv)
  step-1-rec {i = i} bit inv =
    gen-inv-coerce-idx (sym (+-1-r i)) (frame-push-mem-record bit inv)

  step-2 : ∀ {K D} {Mem : K → Index → D → Set} {P : Fr → D → Set} {i k s x y}
    → KInv Mem P (i , k) s → KInv Mem P (i + 2 , k) (push-mem2 s x y)
  step-2 {i = i} inv =
    gen-inv-coerce-idx (sym (+-2-r i)) (gen-frame-push-mem-2 inv)

------------------------------------------------------------------------
-- Main per-step preservation theorem (O2)
--
-- The result `O2-step instr (i, bk) ≡ just acc'` constrains `acc'` to
-- be `(i + Δmem instr , O2-record instr i bk₁)` where `bk₁` comes from
-- `O2-check`.  By case analysis on the constructor of `R-instr`, we
-- unfold `O2-step` and apply the matching frame helper.  Cases are
-- grouped by memory growth: Δmem = 0, Δmem = 1, then Δmem = 2.
------------------------------------------------------------------------

o2-preserve : ∀ {pre s s' instr i bk acc'}
  → O2-Inv (i , bk) s
  → R-instr pre s instr s'
  → O2-step instr (i , bk) ≡ just acc'
  → O2-Inv acc' s'
-- ============================================================
-- Δmem = 0 instructions
-- ============================================================

-- assert: bk-check requires `c ∈ bk`; record is a no-op.
o2-preserve {instr = assert c} {i = i} {bk = bk} inv
            (r-assert lookup-c-bool) step-eq
  with c ∈? bk | step-eq
... | yes _ | refl = step-0 refl inv
-- (the `c ∉ bk` case is excluded: O2-check returns nothing, so step-eq
--  would be `nothing ≡ just acc'`.)

-- constrain-eq: record is a no-op.
o2-preserve {instr = constrain-eq a b} {i = i} inv
            (r-constrain-eq _ _ _) refl =
  step-0 refl inv

-- constrain-bits: record is a no-op under the conservative under-
-- approximation (see Obligations.agda; we don't add v to bool-known
-- even when bits ≡ 1, because `Fits-in v 1 → is-bit v` is
-- outside the bit-arithmetic trust base).
o2-preserve {instr = constrain-bits v bits} {i = i} inv
            (r-constrain-bits _ _ _) refl =
  step-0 refl inv

-- constrain-to-boolean v: record adds `v` to bk.
o2-preserve {s = s} {instr = constrain-to-boolean v} {i = i} {bk = bk} inv
            (r-constrain-to-boolean {b = b} lookup-bool) refl =
  let av , lv , to-eq = extract-to-bool (Preprocessed.memory s) v lookup-bool
  in gen-inv-coerce-idx (sym (+-0-r i))
       (frame-no-grow-add-v refl lv (to-bool→is-bit to-eq) inv)

-- declare-pub-input: memory unchanged (push-pi), record is a no-op.
o2-preserve {instr = declare-pub-input v} {i = i} inv
            (r-declare-pub-input _) refl =
  step-0 refl inv

-- pi-skip active / inactive: memory unchanged (push-skip; pi-skip
-- inactive additionally modifies pub-in-idx, which doesn't touch
-- memory), no record.  A guarded pi-skip's check requires `g ∈ bk`
-- (the `g ∉ bk` case is excluded: `O2-check` returns nothing, so the
-- step equation would be `nothing ≡ just acc'`).
o2-preserve {instr = pi-skip nothing n} {i = i} inv
            (r-pi-skip-active _ _) refl =
  step-0 refl inv
o2-preserve {instr = pi-skip nothing n} {i = i} inv
            (r-pi-skip-inactive _) refl =
  step-0 refl inv
o2-preserve {instr = pi-skip (just g) n} {i = i} {bk = bk} inv
            (r-pi-skip-active _ _) step-eq
  with g ∈? bk | step-eq
... | yes _ | refl = step-0 refl inv
o2-preserve {instr = pi-skip (just g) n} {i = i} {bk = bk} inv
            (r-pi-skip-inactive _) step-eq
  with g ∈? bk | step-eq
... | yes _ | refl = step-0 refl inv

-- output: memory unchanged (push-output), record is a no-op.
o2-preserve {instr = output v} {i = i} inv
            (r-output _) refl =
  step-0 refl inv

-- ============================================================
-- Δmem = 1 instructions
-- ============================================================

-- cond-select bit a b: check requires `bit ∈ bk`; record adds new
-- index `i` to bk if BOTH `a ∈ bk` AND `b ∈ bk` (else no record).
o2-preserve {instr = cond-select bit a b} {i = i} {bk = bk} inv
            (r-cond-select {sel = sel} {av = av} {bv = bv}
                           lookup-bit la lb) step-eq
  with bit ∈? bk | step-eq
... | yes _ | step-eq'
  with a ∈? bk | b ∈? bk | step-eq'
... | yes a∈ | yes b∈ | refl =
  step-1-rec (if-sel-bit sel) inv
  where
    av-bit : is-bit av
    av-bit = known inv a∈ la

    bv-bit : is-bit bv
    bv-bit = known inv b∈ lb

    if-sel-bit : ∀ (s : Bool) → is-bit (if s then av else bv)
    if-sel-bit true  = av-bit
    if-sel-bit false = bv-bit
... | yes _ | no _ | refl =
  step-1 inv
... | no  _ | _    | refl =
  step-1 inv

-- copy v: record adds new index `i` to bk if `v ∈ bk`.
o2-preserve {instr = copy v} {i = i} {bk = bk} inv
            (r-copy {v = vv} lv) refl
  with v ∈? bk
... | yes v∈ =
  let vv-bit : is-bit vv
      vv-bit = known inv v∈ lv
  in step-1-rec vv-bit inv
... | no  _ =
  step-1 inv

-- load-imm k: under our conservative `is-bool-imm? _ = false`,
-- O2-record never adds for load-imm.  (See Obligations.agda.)
o2-preserve {instr = load-imm k} {i = i} inv r-load-imm refl =
  step-1 inv

-- add / mul / neg: push-mem, never record.
o2-preserve {instr = add a b} {i = i} inv (r-add _ _) refl =
  step-1 inv
o2-preserve {instr = mul a b} {i = i} inv (r-mul _ _) refl =
  step-1 inv
o2-preserve {instr = neg a} {i = i} inv (r-neg _) refl =
  step-1 inv

-- test-eq: push-mem (from-bool (av ≡ᶠ? bv)) — always records, value
-- is from-bool of a Bool, hence is-bit.
o2-preserve {instr = test-eq a b} {i = i} inv (r-test-eq {av = av} {bv = bv} _ _) refl =
  step-1-rec (from-bool-is-bit (av ≡ᶠ? bv)) inv

-- not: check requires `a ∈ bk`; always records (from-bool (Bool.not b)).
o2-preserve {instr = not a} {i = i} {bk = bk} inv (r-not {b = b} _) step-eq
  with a ∈? bk | step-eq
... | yes _ | refl =
  step-1-rec (from-bool-is-bit (Bool.not b)) inv

-- less-than: push-mem (from-bool (bits-lt …)) — always records.
o2-preserve {instr = less-than a b bits} {i = i} inv
            (r-less-than {av = av} {bv = bv} _ _ _ _) refl =
  step-1-rec
    (from-bool-is-bit
      (bits-lt (take bits (to-le-bits av)) (take bits (to-le-bits bv)))) inv

-- reconstitute-field: push-mem of a single combined value; Δmem = 1.
-- No record.
o2-preserve {instr = reconstitute-field d m bits} {i = i} inv
            (r-reconstitute-field _ _ _ _ _) refl =
  step-1 inv

-- public-input inactive: push-mem s 0ᶠ.  No record.
o2-preserve {instr = public-input g} {i = i} inv
            (r-public-input-inactive _) refl =
  step-1 inv

-- public-input active: push-mem s₁ v where s₁ has same memory as s.
-- After push, memory grows by one.  No record.
o2-preserve {s = s} {instr = public-input g} {i = i} inv
            (r-public-input-active {s₁ = s₁} _ s₁-eq) refl =
  let mem-s₁ : Preprocessed.memory s₁ ≡ Preprocessed.memory s
      mem-s₁ = consume-pub-out-mem s s₁-eq
      -- inv at s₁ (memory equal):
      inv-s₁ : O2-Inv (i , _) s₁
      inv-s₁ = gen-frame-no-grow mem-s₁ inv
  in step-1 inv-s₁

-- private-input inactive / active: same shape as public-input.
o2-preserve {instr = private-input g} {i = i} inv
            (r-private-input-inactive _) refl =
  step-1 inv
o2-preserve {s = s} {instr = private-input g} {i = i} inv
            (r-private-input-active {s₁ = s₁} _ s₁-eq) refl =
  let mem-s₁ : Preprocessed.memory s₁ ≡ Preprocessed.memory s
      mem-s₁ = consume-priv-mem s s₁-eq
      inv-s₁ : O2-Inv (i , _) s₁
      inv-s₁ = gen-frame-no-grow mem-s₁ inv
  in step-1 inv-s₁

-- transient-hash: push-mem (transient-hash-fn vs).  No record.
o2-preserve {instr = transient-hash xs} {i = i} inv
            (r-transient-hash _) refl =
  step-1 inv

-- ============================================================
-- Δmem = 2 instructions
-- ============================================================

-- ec-add, ec-mul, ec-mul-generator, hash-to-curve, persistent-hash:
-- push-mem2 s x y.  No record.
o2-preserve {instr = ec-add a_x a_y b_x b_y} {i = i} inv
            (r-ec-add _ _ _ _ _) refl =
  step-2 inv
o2-preserve {instr = ec-mul a_x a_y scalar} {i = i} inv
            (r-ec-mul _ _ _ _) refl =
  step-2 inv
o2-preserve {instr = ec-mul-generator scalar} {i = i} inv
            (r-ec-mul-generator _ _) refl =
  step-2 inv
o2-preserve {instr = hash-to-curve xs} {i = i} inv
            (r-hash-to-curve _ _) refl =
  step-2 inv
o2-preserve {instr = persistent-hash a xs} {i = i} inv
            (r-persistent-hash _ _) refl =
  step-2 inv

-- div-mod-power-of-two: push-mem (push-mem s x) y — iterated.  No
-- record.  Uses the special frame for iterated push-mem.
o2-preserve {instr = div-mod-power-of-two v bits} {i = i} inv
            (r-div-mod-power-of-two _ _) refl =
  gen-inv-coerce-idx (sym (+-2-r i)) (gen-frame-push-mem-push-mem inv)

------------------------------------------------------------------------
-- Multi-step preservation, indexed by O2-Trace
--
-- Given the invariant at (acc, s) and a parallel run of R-instrs and
-- O2-Trace, conclude the invariant at (final, s').
------------------------------------------------------------------------

o2-preserve* : ∀ {pre s s' acc final is}
  → O2-Inv acc s
  → R-instrs pre s is s'
  → O2-Trace is acc final
  → O2-Inv final s'
o2-preserve* inv r-done o2-done = inv
o2-preserve* inv (r-step r rs) (o2-step step-eq tr) =
  o2-preserve* (o2-preserve inv r step-eq) rs tr

------------------------------------------------------------------------
-- Whole-program O2 soundness
--
-- Given:
--   • `R src pre s` (the source is faithfully realised by state s),
--     which packages an initial state s₀ and an `R-instrs pre s₀ … s`
--     run;
--   • `O2-Runs src` (the O2 scan completed with final = (i, bk));
-- conclude: for every wire `j` in the final `bk`, looking up `j` in
-- `mem s` yields an `is-bit` value.
--
-- The WF1 arity fact `o2-inv-init` needs is extracted from the run's
-- own `init-state` equation (`init-state-num-inputs`).
------------------------------------------------------------------------

O2-sound : ∀ {src pre s}
  → R src pre s
  → (run : O2-Runs src)
  → ∀ {j v}
  → j ∈ (proj₂ (O2-Runs.final run))
  → mem-lookup (Preprocessed.memory s) j ≡ just v
  → is-bit v
O2-sound {src} {pre} {s} (s₀ , init-eq , r-instrs , _ , _)
         (mk-o2-runs final trace) =
  let inv₀ : O2-Inv (IrSource.num-inputs src , []) s₀
      inv₀ = o2-inv-init {src} {pre} {s₀} init-eq
               (init-state-num-inputs src pre s₀ init-eq)

      inv-final : O2-Inv final s
      inv-final = o2-preserve* inv₀ r-instrs trace
  in known inv-final

------------------------------------------------------------------------
-- Integration lemma: extracting `is-bit` evidence from the O2 scan
--
-- The program-level induction (`satisfies-constraints→R-instrs`) iterates
-- over the instruction list, threading a synth state and a preprocess
-- state.  At each obligation-bearing instruction (`assert`, `not`,
-- `cond-select`'s bit operand), it needs an `is-bit` witness for the
-- operand.  Given the invariant at `(i, bk)` paired with state `s` and
-- that the operand is `c ∈ bk`, `o2-known-is-bit` produces the witness
-- via `known inv`.
------------------------------------------------------------------------

-- Extract `is-bit v` for an operand `c` known to be in bool-known.
o2-known-is-bit : ∀ {i bk s c v}
  → O2-Inv (i , bk) s
  → c ∈ bk
  → mem-lookup (Preprocessed.memory s) c ≡ just v
  → is-bit v
o2-known-is-bit inv c∈ lookup-eq = known inv c∈ lookup-eq

------------------------------------------------------------------------
-- O3 soundness
--
-- O3 tracks a `known` partial map.  `O3-record` records only the
-- cases whose `Fits-in` justification is in the bit-arithmetic trust
-- base:
--
--   • `constrain-bits v n`            (premise `Fits-in v n`)
--   • `div-mod-power-of-two _ n`     (via `fits-from-le-bits-{take,drop}`)
--   • `copy v`                       (inherits from v)
--
-- The other record cases (test-eq, less-than, not, load-imm,
-- cond-select, reconstitute-field) are no-ops, since the backward
-- lemmas `reconstitute-field-bwd` / `less-than-bwd` take the relevant
-- `fits-in` premises directly from the satisfies-constraints data.
--
-- The invariant is analogous to O2's:
--   • i ≡ length mem
--   • for each (j, n) in the map (via lookupᵐ), the lookup at j
--     yields a value v with `Fits-in v n`.
------------------------------------------------------------------------

-- O3 step invariant: pairs (i, bm) where i = length mem, and for
-- every (j, n) in bm (via lookupᵐ), the memory entry at j fits in n
-- bits.  An instance of `KInv`: payload is the bit-bound `n`, recorded
-- via `lookupᵐ`, property is `Fits-in`.
O3-Inv : ℕ × PartialMap → Preprocessed → Set
O3-Inv = KInv {PartialMap} {ℕ} (λ bm j n → lookupᵐ j bm ≡ just n)
                                (λ v n → Fits-in v n)

-- Initial state: empty map vacuously satisfies the invariant.
o3-inv-init : ∀ {src pre s₀}
  → init-state src pre ≡ just s₀
  → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src
  → O3-Inv (IrSource.num-inputs src , []) s₀
o3-inv-init {src} {pre} {s₀} eq lenEq =
  mk-kinv idx-eq empty-bound empty
  where
    mem-eq : Preprocessed.memory s₀ ≡ ProofPreimage.inputs pre
    mem-eq = init-state-memory src pre s₀ eq

    idx-eq : IrSource.num-inputs src ≡ length (Preprocessed.memory s₀)
    idx-eq = trans (sym lenEq) (sym (cong length mem-eq))

    empty-bound : ∀ {j n} → lookupᵐ j [] ≡ just n → j < IrSource.num-inputs src
    empty-bound ()

    empty : ∀ {j n v}
          → lookupᵐ j [] ≡ just n
          → mem-lookup (Preprocessed.memory s₀) j ≡ just v
          → Fits-in v n
    empty ()

------------------------------------------------------------------------
-- O3 helpers for lookupᵐ / insertᵐ.
------------------------------------------------------------------------

private

  ------------------------------------------------------------------
  -- O3-record-supporting helpers
  ------------------------------------------------------------------

  -- After insertᵐ k n bm, a lookup at k returns n; at j ≠ k, it
  -- falls through to lookupᵐ j bm.
  -- Combined with the boolean condition, this gives a decomposition.
  lookupᵐ-insertᵐ-cases : ∀ (j k : Index) n bm {m}
    → lookupᵐ j (insertᵐ k n bm) ≡ just m
    → ((j ≡ k) × (m ≡ n)) ⊎ (lookupᵐ j bm ≡ just m)
  lookupᵐ-insertᵐ-cases j k n bm eq with j ≡ᵇ k in jk-eq
  ... | true  = inj₁ (≡ᵇ-true jk-eq , sym (just-injective eq))
  ... | false = inj₂ eq

  -- Frame: memory unchanged, map extended with `(v, n)`.  Requires:
  --   • v < i  (operand bound from a successful mem-lookup),
  --   • Fits-in vv n  (witness from the R-instr premise),
  --   • mem-lookup s v ≡ just vv.
  o3-frame-no-grow-insert-v : ∀ {i bm s s' v n vv}
    → Preprocessed.memory s' ≡ Preprocessed.memory s
    → mem-lookup (Preprocessed.memory s) v ≡ just vv
    → Fits-in vv n
    → O3-Inv (i , bm) s
    → O3-Inv (i , insertᵐ v n bm) s'
  o3-frame-no-grow-insert-v {i = i} {bm = bm} {s = s} {s' = s'}
                              {v = v} {n = n} {vv = vv}
                              mem-eq lv fits-vv inv =
    mk-kinv idx-eq bnd bits
    where
      idx-eq : i ≡ length (Preprocessed.memory s')
      idx-eq = trans (idx-sync inv) (sym (cong length mem-eq))

      v<i : v < i
      v<i = lookup-bound (Preprocessed.memory s) v lv (idx-sync inv)

      bnd : ∀ {j n'} → lookupᵐ j (insertᵐ v n bm) ≡ just n' → j < i
      bnd {j} look with lookupᵐ-insertᵐ-cases j v n bm look
      ... | inj₁ (j≡v , _)  = subst (_< i) (sym j≡v) v<i
      ... | inj₂ fall       = bound inv fall

      bits : ∀ {j n' v'} → lookupᵐ j (insertᵐ v n bm) ≡ just n'
                         → mem-lookup (Preprocessed.memory s') j ≡ just v'
                         → Fits-in v' n'
      bits {j} {n'} {v'} look lookup-eq
        with lookupᵐ-insertᵐ-cases j v n bm look
      ... | inj₁ (j≡v , n'≡n) =
        let lookup-at-v : mem-lookup (Preprocessed.memory s) v ≡ just v'
            lookup-at-v =
              subst (λ k → mem-lookup (Preprocessed.memory s) k ≡ just v')
                    j≡v
                    (trans (cong (λ m → mem-lookup m j) (sym mem-eq)) lookup-eq)
            vv≡v' : vv ≡ v'
            vv≡v' = just-injective (trans (sym lv) lookup-at-v)
        in subst (λ z → Fits-in z n') vv≡v'
                 (subst (λ z → Fits-in vv z) (sym n'≡n) fits-vv)
      ... | inj₂ fall =
        known inv fall
          (trans (cong (λ m → mem-lookup m _) (sym mem-eq)) lookup-eq)

  -- Frame for div-mod-power-of-two: memory grows by two cells (via
  -- iterated push-mem), and the map gets two new entries.  Requires
  -- `fits-in` witnesses for the two appended values.
  o3-frame-push-mem-2-insert-2 : ∀ {i bm s x y nx ny}
    → Fits-in x nx
    → Fits-in y ny
    → O3-Inv (i , bm) s
    → O3-Inv (suc (suc i) , insertᵐ (suc i) ny (insertᵐ i nx bm))
             (push-mem (push-mem s x) y)
  o3-frame-push-mem-2-insert-2 {i = i} {bm = bm} {s = s}
                                 {x = x} {y = y} {nx = nx} {ny = ny}
                                 fx fy inv =
    mk-kinv idx-eq bnd bits
    where
      mem    = Preprocessed.memory s
      mem-eq : Preprocessed.memory (push-mem (push-mem s x) y)
             ≡ mem ++ (x ∷ y ∷ [])
      mem-eq = ++-assoc mem (x ∷ []) (y ∷ [])

      idx-eq : suc (suc i)
             ≡ length (Preprocessed.memory (push-mem (push-mem s x) y))
      idx-eq = trans (cong suc (cong suc (idx-sync inv)))
                     (trans (sym (length-++-two mem x y))
                            (sym (cong length mem-eq)))

      bnd : ∀ {j n'} → lookupᵐ j (insertᵐ (suc i) ny (insertᵐ i nx bm)) ≡ just n'
                     → j < suc (suc i)
      bnd {j} look with lookupᵐ-insertᵐ-cases j (suc i) ny (insertᵐ i nx bm) look
      ... | inj₁ (j≡sucI , _) =
        subst (_< suc (suc i)) (sym j≡sucI) (n<1+n (suc i))
      ... | inj₂ fall1 with lookupᵐ-insertᵐ-cases j i nx bm fall1
      ...   | inj₁ (j≡i , _) =
        subst (_< suc (suc i)) (sym j≡i) (m<n⇒m<1+n (n<1+n i))
      ...   | inj₂ fall2     = m<n⇒m<1+n (m<n⇒m<1+n (bound inv fall2))

      bits : ∀ {j n' v'}
           → lookupᵐ j (insertᵐ (suc i) ny (insertᵐ i nx bm)) ≡ just n'
           → mem-lookup (Preprocessed.memory (push-mem (push-mem s x) y)) j
              ≡ just v'
           → Fits-in v' n'
      bits {j} {n'} {v'} look lookup-eq =
        let lookup-eq' : mem-lookup (mem ++ (x ∷ y ∷ [])) j ≡ just v'
            lookup-eq' = trans (cong (λ m → mem-lookup m j) (sym mem-eq))
                                lookup-eq
        in aux look lookup-eq'
        where
          aux : lookupᵐ j (insertᵐ (suc i) ny (insertᵐ i nx bm)) ≡ just n'
              → mem-lookup (mem ++ (x ∷ y ∷ [])) j ≡ just v'
              → Fits-in v' n'
          aux look' lookup-eq''
            with lookupᵐ-insertᵐ-cases j (suc i) ny (insertᵐ i nx bm) look'
          ... | inj₁ (j≡sucI , n'≡ny) =
            let -- lookup at suc i ≡ just y in mem ++ (x ∷ y ∷ [])
                lookup-at-sucI : mem-lookup (mem ++ (x ∷ y ∷ [])) (suc i) ≡ just y
                lookup-at-sucI =
                  subst (λ k → mem-lookup (mem ++ (x ∷ y ∷ [])) (suc k) ≡ just y)
                        (sym (idx-sync inv))
                        (lookup-new-snd mem x y)
                v'≡y : v' ≡ y
                v'≡y = just-injective
                         (trans (sym
                                  (subst (λ k → mem-lookup (mem ++ (x ∷ y ∷ [])) k
                                                ≡ just v')
                                          j≡sucI lookup-eq''))
                                lookup-at-sucI)
            in subst (λ z → Fits-in z n') (sym v'≡y)
                     (subst (λ z → Fits-in y z) (sym n'≡ny) fy)
          ... | inj₂ fall1 with lookupᵐ-insertᵐ-cases j i nx bm fall1
          ...   | inj₁ (j≡i , n'≡nx) =
            let lookup-at-i : mem-lookup (mem ++ (x ∷ y ∷ [])) i ≡ just x
                lookup-at-i =
                  subst (λ k → mem-lookup (mem ++ (x ∷ y ∷ [])) k ≡ just x)
                        (sym (idx-sync inv))
                        (lookup-new-fst mem x y)
                v'≡x : v' ≡ x
                v'≡x = just-injective
                         (trans (sym
                                  (subst (λ k → mem-lookup (mem ++ (x ∷ y ∷ [])) k
                                                ≡ just v')
                                          j≡i lookup-eq''))
                                lookup-at-i)
            in subst (λ z → Fits-in z n') (sym v'≡x)
                     (subst (λ z → Fits-in x z) (sym n'≡nx) fx)
          ...   | inj₂ fall2 with lookup-shrink-or-new-2 mem x y j v' lookup-eq''
          ...     | inj₁ p              = known inv fall2 p
          ...     | inj₂ (inj₁ (j≡L , _)) =
            ⊥-elim (<-irrefl refl
                     (subst (_< i) (trans j≡L (sym (idx-sync inv)))
                            (bound inv fall2)))
          ...     | inj₂ (inj₂ (j≡sL , _)) =
            ⊥-elim (<⇒≯ (n<1+n _)
                     (subst (_< i)
                            (trans j≡sL (cong suc (sym (idx-sync inv))))
                            (bound inv fall2)))

  -- Frame for copy v when `v ∈ dom(bm)`: memory grows by one cell;
  -- the appended value is `vv` (the value at v); the map gets a new
  -- entry `(i, k)` where k is the previous bound on v.  The witness:
  -- by IH, `fits-in vv k`.
  o3-frame-push-mem-copy : ∀ {i bm s v vv k}
    → lookupᵐ v bm ≡ just k
    → mem-lookup (Preprocessed.memory s) v ≡ just vv
    → O3-Inv (i , bm) s
    → O3-Inv (suc i , insertᵐ i k bm) (push-mem s vv)
  o3-frame-push-mem-copy {i = i} {bm = bm} {s = s} {v = v} {vv = vv} {k = k}
                           look-v lv inv =
    mk-kinv idx-eq bnd bits
    where
      mem  = Preprocessed.memory s
      idx-eq : suc i ≡ length (Preprocessed.memory (push-mem s vv))
      idx-eq = trans (cong suc (idx-sync inv)) (sym (length-++-one mem vv))

      fits-vv-k : Fits-in vv k
      fits-vv-k = known inv look-v lv

      bnd : ∀ {j n'} → lookupᵐ j (insertᵐ i k bm) ≡ just n' → j < suc i
      bnd {j} look with lookupᵐ-insertᵐ-cases j i k bm look
      ... | inj₁ (j≡i , _) = subst (_< suc i) (sym j≡i) (n<1+n i)
      ... | inj₂ fall      = m<n⇒m<1+n (bound inv fall)

      bits : ∀ {j n' v'} → lookupᵐ j (insertᵐ i k bm) ≡ just n'
                         → mem-lookup (Preprocessed.memory (push-mem s vv)) j ≡ just v'
                         → Fits-in v' n'
      bits {j} {n'} {v'} look lookup-eq
        with lookupᵐ-insertᵐ-cases j i k bm look
      ... | inj₁ (j≡i , n'≡k) =
        let lookup-at-i : mem-lookup (mem ++ (vv ∷ [])) i ≡ just vv
            lookup-at-i =
              subst (λ z → mem-lookup (mem ++ (vv ∷ [])) z ≡ just vv)
                    (sym (idx-sync inv)) (lookup-new mem vv)
            v'≡vv : v' ≡ vv
            v'≡vv = just-injective
                      (trans (sym (subst (λ z → mem-lookup (mem ++ (vv ∷ [])) z
                                                 ≡ just v')
                                          j≡i lookup-eq))
                             lookup-at-i)
        in subst (λ z → Fits-in z n') (sym v'≡vv)
                 (subst (λ z → Fits-in vv z) (sym n'≡k) fits-vv-k)
      ... | inj₂ fall with lookup-shrink-or-new mem vv j v' lookup-eq
      ...   | inj₁ p              = known inv fall p
      ...   | inj₂ (j≡L , _)      =
        ⊥-elim (<-irrefl refl (subst (_< i)
                                       (trans j≡L (sym (idx-sync inv)))
                                       (bound inv fall)))

------------------------------------------------------------------------
-- Main per-step preservation theorem (O3)
--
-- 29-case dispatch.  Memory-preserving cases use `step-0`; one-cell
-- push-mem cases use `step-1`; two-cell cases use `step-2`.  The
-- substantive record cases (constrain-bits, div-mod, copy) use the
-- specialised insert frames.
------------------------------------------------------------------------

o3-preserve : ∀ {pre s s' instr i bm acc'}
  → O3-Inv (i , bm) s
  → R-instr pre s instr s'
  → O3-step instr (i , bm) ≡ just acc'
  → O3-Inv acc' s'
-- Δmem = 0 cases (no record under our conservative O3-record)
o3-preserve {instr = assert c} {i = i} inv (r-assert _) refl =
  step-0 refl inv
o3-preserve {instr = constrain-eq a b} {i = i} inv (r-constrain-eq _ _ _) refl =
  step-0 refl inv
o3-preserve {instr = constrain-to-boolean v} {i = i} inv
            (r-constrain-to-boolean _) refl =
  step-0 refl inv
o3-preserve {instr = declare-pub-input v} {i = i} inv
            (r-declare-pub-input _) refl =
  step-0 refl inv
o3-preserve {instr = pi-skip g n} {i = i} inv (r-pi-skip-active _ _) refl =
  step-0 refl inv
o3-preserve {instr = pi-skip g n} {i = i} inv (r-pi-skip-inactive _) refl =
  step-0 refl inv
o3-preserve {instr = output v} {i = i} inv (r-output _) refl =
  step-0 refl inv

-- constrain-bits v n: record inserts (v, n).  Justified by r-constrain-bits.
o3-preserve {instr = constrain-bits v n} {i = i} inv
            (r-constrain-bits {v = vv} lv _ fits-eq) refl =
  gen-inv-coerce-idx (sym (+-0-r i))
    (o3-frame-no-grow-insert-v refl lv fits-eq inv)

-- Δmem = 1 cases (no record under conservative O3-record except copy)
o3-preserve {instr = cond-select bit a b} {i = i} inv
            (r-cond-select _ _ _) refl =
  step-1 inv
o3-preserve {instr = copy v} {i = i} {bm = bm} inv (r-copy {v = vv} lv) refl
  with lookupᵐ v bm in vbm-eq
... | just k  =
  gen-inv-coerce-idx (sym (+-1-r i)) (o3-frame-push-mem-copy vbm-eq lv inv)
... | nothing =
  step-1 inv
o3-preserve {instr = load-imm k} {i = i} inv r-load-imm refl =
  step-1 inv
o3-preserve {instr = add a b} {i = i} inv (r-add _ _) refl =
  step-1 inv
o3-preserve {instr = mul a b} {i = i} inv (r-mul _ _) refl =
  step-1 inv
o3-preserve {instr = neg a} {i = i} inv (r-neg _) refl =
  step-1 inv
o3-preserve {instr = test-eq a b} {i = i} inv (r-test-eq _ _) refl =
  step-1 inv
o3-preserve {instr = not a} {i = i} inv (r-not _) refl =
  step-1 inv
o3-preserve {instr = less-than a b bits} {i = i} {bm = bm} inv
            (r-less-than _ _ _ _) step-eq
  with o3OK? (less-than a b bits) bm | step-eq
... | yes _ | refl =
  step-1 inv
... | no  _ | ()
o3-preserve {instr = reconstitute-field d m bits} {i = i} {bm = bm} inv
            (r-reconstitute-field _ _ _ _ _) step-eq
  with o3OK? (reconstitute-field d m bits) bm | step-eq
... | yes _ | refl =
  step-1 inv
... | no  _ | ()
o3-preserve {instr = public-input g} {i = i} inv
            (r-public-input-inactive _) refl =
  step-1 inv
o3-preserve {s = s} {instr = public-input g} {i = i} inv
            (r-public-input-active {s₁ = s₁} _ s₁-eq) refl =
  let mem-eq : Preprocessed.memory s₁ ≡ Preprocessed.memory s
      mem-eq = consume-pub-out-mem s s₁-eq
      inv-s₁ = gen-frame-no-grow mem-eq inv
  in step-1 inv-s₁
o3-preserve {instr = private-input g} {i = i} inv
            (r-private-input-inactive _) refl =
  step-1 inv
o3-preserve {s = s} {instr = private-input g} {i = i} inv
            (r-private-input-active {s₁ = s₁} _ s₁-eq) refl =
  let mem-eq : Preprocessed.memory s₁ ≡ Preprocessed.memory s
      mem-eq = consume-priv-mem s s₁-eq
      inv-s₁ = gen-frame-no-grow mem-eq inv
  in step-1 inv-s₁
o3-preserve {instr = transient-hash xs} {i = i} inv (r-transient-hash _) refl =
  step-1 inv

-- Δmem = 2 cases.
o3-preserve {instr = ec-add ax ay bx by} {i = i} inv (r-ec-add _ _ _ _ _) refl =
  step-2 inv
o3-preserve {instr = ec-mul ax ay sc} {i = i} inv (r-ec-mul _ _ _ _) refl =
  step-2 inv
o3-preserve {instr = ec-mul-generator sc} {i = i} inv (r-ec-mul-generator _ _) refl =
  step-2 inv
o3-preserve {instr = hash-to-curve xs} {i = i} inv (r-hash-to-curve _ _) refl =
  step-2 inv
o3-preserve {instr = persistent-hash a xs} {i = i} inv (r-persistent-hash _ _) refl =
  step-2 inv

-- div-mod-power-of-two v n: 2 records.  divisor at i (fits in FR-BITS ∸ n);
-- modulus at suc i (fits in n).  Justified by the bit-arithmetic axioms.
o3-preserve {instr = div-mod-power-of-two v n} {i = i} inv
            (r-div-mod-power-of-two {v = vv} lv _) refl =
  let divisor = from-le-bits (drop n (to-le-bits vv))
      modulus = from-le-bits (take n (to-le-bits vv))
      fits-div : Fits-in divisor (FR-BITS ∸ n)
      fits-div = fits-from-le-bits-drop vv n
      fits-mod : Fits-in modulus n
      fits-mod = fits-from-le-bits-take (to-le-bits vv) n
  in gen-inv-coerce-idx (sym (+-2-r i))
       (o3-frame-push-mem-2-insert-2 {nx = FR-BITS ∸ n} {ny = n}
                                      fits-div fits-mod inv)

------------------------------------------------------------------------
-- Multi-step preservation, indexed by O3-Trace
--
-- Given the invariant at (acc, s) and a parallel run of R-instrs and
-- O3-Trace, conclude the invariant at (final, s').
------------------------------------------------------------------------

o3-preserve* : ∀ {pre s s' acc final is}
  → O3-Inv acc s
  → R-instrs pre s is s'
  → O3-Trace is acc final
  → O3-Inv final s'
o3-preserve* inv r-done o3-done = inv
o3-preserve* inv (r-step r rs) (o3-step step-eq tr) =
  o3-preserve* (o3-preserve inv r step-eq) rs tr

------------------------------------------------------------------------
-- Whole-program O3 soundness
--
-- Given:
--   • `R src pre s` (the source is faithfully realised by state s),
--     which packages an initial state s₀ and an `R-instrs pre s₀ … s`
--     run;
--   • `O3-Runs src` (the O3 scan completed with final = (i, bm));
--   • the WF1 hypothesis `length inputs ≡ num-inputs src`
-- conclude: for every wire `j` with `lookupᵐ j (proj₂ final) ≡ just n`,
-- the corresponding memory entry `v` satisfies `Fits-in v n`.
------------------------------------------------------------------------

O3-sound : ∀ {src pre s}
  → R src pre s
  → (run : O3-Runs src)
  → ∀ {j n v}
  → lookupᵐ j (proj₂ (O3-Runs.final run)) ≡ just n
  → mem-lookup (Preprocessed.memory s) j ≡ just v
  → Fits-in v n
O3-sound {src} {pre} {s} (s₀ , init-eq , r-instrs , _ , _)
         (mk-o3-runs final trace) =
  let inv₀ : O3-Inv (IrSource.num-inputs src , []) s₀
      inv₀ = o3-inv-init {src} {pre} {s₀} init-eq
               (init-state-num-inputs src pre s₀ init-eq)

      inv-final : O3-Inv final s
      inv-final = o3-preserve* inv₀ r-instrs trace
  in known inv-final

------------------------------------------------------------------------
-- Integration lemma: extracting `Fits-in` evidence from the O3 scan
--
-- `o3-known-fits`: at any intermediate (i, bm)/s pairing, if
-- `lookupᵐ a bm ≡ just n` and `mem-lookup mem a ≡ just v`, then
-- `Fits-in v n`.  This is the direct extractor used by
-- `reconstitute-field-bwd` and `less-than-bwd`.
------------------------------------------------------------------------

-- Extract `Fits-in v n` for an operand `a` known in bm.
o3-known-fits : ∀ {i bm s a n v}
  → O3-Inv (i , bm) s
  → lookupᵐ a bm ≡ just n
  → mem-lookup (Preprocessed.memory s) a ≡ just v
  → Fits-in v n
o3-known-fits inv look-eq lookup-eq = known inv look-eq lookup-eq

------------------------------------------------------------------------
-- Bool ⇒ Witness extractors for the producer-safe conjunction.
--
-- These let a caller holding `T (producer-safe src)` recover the
-- witness-bearing `O2-Runs` / `O3-Runs` needed to feed the soundness
-- theorems above.
--
-- `producer-safe src = O1 src ∧ O2 src ∧ O3 src ∧ wire-disc src`
-- decomposes via `∧-true` into the four conjunct checks.
------------------------------------------------------------------------

-- Project conjuncts of `producer-safe`.  Since `producer-safe =
-- O1 ∧ O2 ∧ O3 ∧ wire-disc`, each conjunct's `T` check is peeled off
-- the (right-nested) conjunction with `∧-true`.
producer-safe-O1 : ∀ {src} → T (producer-safe src) → T (O1 src)
producer-safe-O1 {src} eq = proj₁ (∧-true {O1 src} eq)

producer-safe-O2 : ∀ {src} → T (producer-safe src) → T (O2 src)
producer-safe-O2 {src} eq = proj₁ (∧-true {O2 src} (proj₂ (∧-true {O1 src} eq)))

producer-safe-O3 : ∀ {src} → T (producer-safe src) → T (O3 src)
producer-safe-O3 {src} eq =
  proj₁ (∧-true {O3 src} (proj₂ (∧-true {O2 src} (proj₂ (∧-true {O1 src} eq)))))

producer-safe-wire-disc : ∀ {src} → T (producer-safe src) → T (wire-disc src)
producer-safe-wire-disc {src} eq =
  proj₂ (∧-true {O3 src} (proj₂ (∧-true {O2 src} (proj₂ (∧-true {O1 src} eq)))))

