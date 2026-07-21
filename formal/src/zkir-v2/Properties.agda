{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.Properties (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.FieldProperties ⋯
open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
open import zkir-v2.SemanticsLemmas ⋯
  using ( init-state-memory; init-state-num-inputs
        ; init-state-pub-in-idx; init-state-outputs
        ; consume-pub-out-mem; consume-priv-mem )

open import Data.Bool    using (Bool; true; false; if_then_else_; _∧_; T)
open import Data.Bool.Properties using (T-∧; T-≡)
open import Function.Bundles using (Equivalence)
open import Data.Unit    using (tt)
open import Data.List    using (List; []; _∷_; _++_; length; take)
open import Data.List.Properties using (length-++)
open import Data.List.Relation.Unary.All using (All) renaming ([] to ⟨⟩; _∷_ to _◂_)
open import Data.Maybe   using (Maybe; nothing; just; _>>=_)
open import Data.Nat     using (ℕ; zero; suc; _≤_; _<_; _<?_; _≤?_; _+_; _∸_; _≡ᵇ_; z≤n; s≤s)
open import Data.Nat.Properties
  using (≤-refl; ≤-trans; ≤-reflexive; m∸n≤m; m≤m+n; +-comm; +-assoc; +-monoʳ-≤
        ; +-identityʳ; ≡ᵇ⇒≡; <ᵇ⇒<; ≤ᵇ⇒≤)
open import Data.Product using (_×_; _,_; ∃; proj₁; proj₂)
open import Data.Maybe.Properties using (just-injective)
open import Relation.Nullary.Decidable using (does; dec-true)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; subst)
-- Re-exported (`public`): Properties is the façade through which spec-level
-- consumers reach the P5 vocabulary and statement.
open import zkir-v2.Circuit ⋯      using (circuit; satisfies) public
open import zkir-v2.Obligations ⋯  using (producer-safe; Δmem) public
open import zkir-v2.CircuitProof ⋯
  using (witness-of; preprocess-shaped; circuit-faithful) public

------------------------------------------------------------------------
-- Local proof helpers
------------------------------------------------------------------------

private

  >>=-just : ∀ {A B : Set} {f : A → Maybe B} {y : B}
    → (m : Maybe A) → (m >>= f) ≡ just y
    → ∃ λ x → m ≡ just x × f x ≡ just y
  >>=-just (just x) p = x , refl , p
  >>=-just nothing  ()

  if-just : ∀ {A : Set} (b : Bool) {x y : A}
    → (if b then just x else nothing) ≡ just y
    → T b × x ≡ y
  if-just true  refl = tt , refl
  if-just false ()

  -- Recover the `Fits-in`/`BitsInField` conjuncts of the reconstitute /
  -- less-than operational guards (Bool) as the `Set` product premise.
  lt-guard→ : ∀ av bv bits
    → T (does (fits-in? av bits) ∧ does (fits-in? bv bits))
    → Fits-in av bits × Fits-in bv bits
  lt-guard→ av bv bits g =
    let sa , sb = Equivalence.to T-∧ g
    in fits-in?-from-true av bits sa , fits-in?-from-true bv bits sb

  recon-guard→ : ∀ mv dv bits
    → T (does (fits-in? mv bits) ∧ does (fits-in? dv (FR-BITS ∸ bits)) ∧
       does (bitsInField? (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))))
    → Fits-in mv bits × Fits-in dv (FR-BITS ∸ bits) ×
      BitsInField (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))
  recon-guard→ mv dv bits g =
    let s1a , s1b = Equivalence.to T-∧ g
        s2a , s2b = Equivalence.to T-∧ s1b
    in fits-in?-from-true mv bits s1a
     , fits-in?-from-true dv (FR-BITS ∸ bits) s2a
     , bitsInField?-from-true
         (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv)) s2b

  from-just-R : ∀ {pre s i t s'} → R-instr pre s i t → just t ≡ just s' → R-instr pre s i s'
  from-just-R r refl = r

------------------------------------------------------------------------
-- 1. Transcript consumption
-- A successful preprocessing fully consumes all three transcript streams.
------------------------------------------------------------------------

preprocess-transcripts-consumed : ∀ src pre s
  → preprocess src pre ≡ just s
  → T (transcripts-consumed pre s)
preprocess-transcripts-consumed src pre s eq
  with >>=-just (init-state src pre) eq
... | s₀ , _ , eq₁
  with >>=-just (preprocess-instrs pre s₀ (IrSource.instructions src)) eq₁
... | s' , _ , eq₂
  with if-just _ eq₂
... | b-true , s'-eq
  = subst (λ x → T (transcripts-consumed pre x)) s'-eq
      (proj₁ (Equivalence.to T-∧ b-true))

------------------------------------------------------------------------
-- 2. Exact memory accounting (the exact-Δmem form of P2)
--
-- A successful step grows the memory by exactly `Δmem i` cells, so a
-- full run's memory length is `num-inputs` plus the per-instruction
-- `Δmem` sum.  Matched against the synthesis-side wire count, this pins
-- a run's memory length to the circuit's `nr-wires` — the witness-shape
-- fact the statement-soundness characterization consumes.
------------------------------------------------------------------------

Δmem-sum : List Instruction → ℕ
Δmem-sum []       = 0
Δmem-sum (i ∷ is) = Δmem i + Δmem-sum is

R-instr-Δmem : ∀ {pre s i s'} → R-instr pre s i s'
  → length (Preprocessed.memory s') ≡ length (Preprocessed.memory s) + Δmem i
R-instr-Δmem (r-assert _)                 = sym (+-identityʳ _)
R-instr-Δmem (r-constrain-bits _ _ _)       = sym (+-identityʳ _)
R-instr-Δmem (r-constrain-eq _ _ _)       = sym (+-identityʳ _)
R-instr-Δmem (r-constrain-to-boolean _)   = sym (+-identityʳ _)
R-instr-Δmem (r-declare-pub-input _)      = sym (+-identityʳ _)
R-instr-Δmem (r-pi-skip-active _ _)       = sym (+-identityʳ _)
R-instr-Δmem (r-pi-skip-inactive _)       = sym (+-identityʳ _)
R-instr-Δmem (r-output _)                 = sym (+-identityʳ _)
R-instr-Δmem {s = s} (r-cond-select _ _ _)        = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-copy _)                   = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} r-load-imm                   = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-reconstitute-field _ _ _ _ _) = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-transient-hash _)         = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-test-eq _ _)              = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-add _ _)                  = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-mul _ _)                  = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-neg _)                    = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-not _)                    = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-less-than _ _ _ _)          = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-public-input-inactive _)  = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-private-input-inactive _) = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-ec-add _ _ _ _ _)         = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-ec-mul _ _ _ _)           = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-ec-mul-generator _ _)     = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-hash-to-curve _ _)        = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-persistent-hash _ _)      = length-++ (Preprocessed.memory s)
R-instr-Δmem {s = s} (r-div-mod-power-of-two _ _)   =
  trans (length-++ (Preprocessed.memory s ++ _))
        (trans (cong (_+ 1) (length-++ (Preprocessed.memory s)))
               (+-assoc (length (Preprocessed.memory s)) 1 1))
R-instr-Δmem {s = s} (r-public-input-active {v = v} {s₁ = s₁} _ cp) =
  trans (length-++ (Preprocessed.memory s₁))
        (cong (_+ 1) (cong length (consume-pub-out-mem s cp)))
R-instr-Δmem {s = s} (r-private-input-active {v = v} {s₁ = s₁} _ cp) =
  trans (length-++ (Preprocessed.memory s₁))
        (cong (_+ 1) (cong length (consume-priv-mem s cp)))

R-instrs-Δmem : ∀ {pre s is s'} → R-instrs pre s is s'
  → length (Preprocessed.memory s')
    ≡ length (Preprocessed.memory s) + Δmem-sum is
R-instrs-Δmem r-done = sym (+-identityʳ _)
R-instrs-Δmem {s = s} {is = i ∷ is} (r-step step rest) =
  trans (R-instrs-Δmem rest)
    (trans (cong (_+ Δmem-sum is) (R-instr-Δmem step))
           (+-assoc (length (Preprocessed.memory s)) (Δmem i) (Δmem-sum is)))

R-memory-length : ∀ src pre s → R src pre s
  → length (Preprocessed.memory s)
    ≡ IrSource.num-inputs src + Δmem-sum (IrSource.instructions src)
R-memory-length src pre s (s₀ , init-eq , Rs , _ , _) =
  trans (R-instrs-Δmem Rs)
    (cong (_+ Δmem-sum (IrSource.instructions src))
      (trans (cong length (init-state-memory src pre s₀ init-eq))
             (init-state-num-inputs src pre s₀ init-eq)))

------------------------------------------------------------------------
-- 3. Faithfulness
-- The computational and relational semantics agree.
------------------------------------------------------------------------

-- Per-instruction: computational → relational

preprocess-instr→R-instr : ∀ pre s i s'
  → preprocess-instr pre s i ≡ just s' → R-instr pre s i s'
preprocess-instr→R-instr _ s (assert cond) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) cond >>= to-bool) eq
... | b , lb , eq₁
  with if-just b eq₁
... | b-true , s-eq
  = subst (R-instr _ s (assert cond)) s-eq
      (r-assert (subst (λ x → _ ≡ just x) (Equivalence.to T-≡ b-true) lb))

preprocess-instr→R-instr _ s (cond-select bit a b) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) bit >>= to-bool) eq
... | sel , lsel , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) a) eq₁
... | av , la , eq₂
  with >>=-just (mem-lookup (Preprocessed.memory s) b) eq₂
... | bv , lb , eq₃
  = from-just-R (r-cond-select lsel la lb) eq₃

preprocess-instr→R-instr _ s (constrain-bits var bits) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) var) eq
... | v , lv , eq₁
  with if-just (does (bits <? FR-BITS) ∧ does (fits-in? v bits)) eq₁
... | guard , s-eq
  = let bnd , ft = Equivalence.to T-∧ guard
    in subst (R-instr _ s (constrain-bits var bits)) s-eq
         (r-constrain-bits lv (<ᵇ⇒< bits FR-BITS bnd) (fits-in?-from-true v bits ft))

preprocess-instr→R-instr _ s (constrain-eq a b) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a) eq
... | av , la , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) b) eq₁
... | bv , lb , eq₂
  with if-just (av ≡ᶠ? bv) eq₂
... | eq? , s-eq
  = subst (R-instr _ s (constrain-eq a b)) s-eq (r-constrain-eq la lb (≡ᶠ?-true eq?))

preprocess-instr→R-instr _ s (constrain-to-boolean var) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) var >>= to-bool) eq
... | b , lb , eq₁
  = subst (R-instr _ s (constrain-to-boolean var)) (just-injective eq₁) (r-constrain-to-boolean lb)

preprocess-instr→R-instr _ s (copy var) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) var) eq
... | v , lv , eq₁ = from-just-R (r-copy lv) eq₁

preprocess-instr→R-instr _ s (declare-pub-input var) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) var) eq
... | v , lv , eq₁ = from-just-R (r-declare-pub-input lv) eq₁

preprocess-instr→R-instr pre s (pi-skip guard count) s' eq
  with >>=-just (eval-guard (Preprocessed.memory s) guard) eq
... | false , gf , eq₁ = from-just-R (r-pi-skip-inactive gf) eq₁
... | true  , gt , eq₁
  with if-just _ eq₁
... | chk , s-eq
  = subst (R-instr pre s (pi-skip guard count)) s-eq (r-pi-skip-active gt chk)

preprocess-instr→R-instr _ s (ec-add a_x a_y b_x b_y) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a_x) eq
... | ax , lax , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) a_y) eq₁
... | ay , lay , eq₂
  with >>=-just (mem-lookup (Preprocessed.memory s) b_x) eq₂
... | bx , lbx , eq₃
  with >>=-just (mem-lookup (Preprocessed.memory s) b_y) eq₃
... | by , lby , eq₄
  with >>=-just (ec-add-pts ax ay bx by) eq₄
... | (cx , cy) , add-eq , eq₅ = from-just-R (r-ec-add lax lay lbx lby add-eq) eq₅

preprocess-instr→R-instr _ s (ec-mul a_x a_y scalar) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a_x) eq
... | ax , lax , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) a_y) eq₁
... | ay , lay , eq₂
  with >>=-just (mem-lookup (Preprocessed.memory s) scalar) eq₂
... | sc , lsc , eq₃
  with >>=-just (ec-mul-pt ax ay sc) eq₃
... | (cx , cy) , mul-eq , eq₄ = from-just-R (r-ec-mul lax lay lsc mul-eq) eq₄

preprocess-instr→R-instr _ s (ec-mul-generator scalar) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) scalar) eq
... | sc , lsc , eq₁
  = subst (R-instr _ s (ec-mul-generator scalar)) (just-injective eq₁) (r-ec-mul-generator lsc refl)

preprocess-instr→R-instr _ s (hash-to-curve inputs) s' eq
  with >>=-just (mem-lookups (Preprocessed.memory s) inputs) eq
... | vs , lvs , eq₁
  = subst (R-instr _ s (hash-to-curve inputs)) (just-injective eq₁) (r-hash-to-curve lvs refl)

preprocess-instr→R-instr _ s (load-imm imm) s' eq = from-just-R r-load-imm eq

preprocess-instr→R-instr _ s (div-mod-power-of-two var bits) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) var) eq
... | v , lv , eq₁
  with if-just (does (bits ≤? FR-STORED-BITS)) eq₁
... | le , s-eq
  = subst (R-instr _ s (div-mod-power-of-two var bits)) s-eq
      (r-div-mod-power-of-two lv (≤ᵇ⇒≤ bits FR-STORED-BITS le))

preprocess-instr→R-instr _ s (reconstitute-field divisor modulus bits) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) divisor) eq
... | dv , ldv , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) modulus) eq₁
... | mv , lmv , eq₂
  with if-just (does (1 ≤? bits) ∧ does (bits ≤? FR-STORED-BITS) ∧
                does (fits-in? mv bits) ∧ does (fits-in? dv (FR-BITS ∸ bits)) ∧
                does (bitsInField? (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv)))) eq₂
... | cond , s-eq
  = let ge1b , r1 = Equivalence.to T-∧ cond
        leb  , r2 = Equivalence.to T-∧ r1
    in subst (R-instr _ s (reconstitute-field divisor modulus bits)) s-eq
         (r-reconstitute-field ldv lmv (≤ᵇ⇒≤ 1 bits ge1b) (≤ᵇ⇒≤ bits FR-STORED-BITS leb)
            (recon-guard→ mv dv bits r2))

preprocess-instr→R-instr _ s (output var) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) var) eq
... | v , lv , eq₁ = from-just-R (r-output lv) eq₁

preprocess-instr→R-instr _ s (transient-hash inputs) s' eq
  with >>=-just (mem-lookups (Preprocessed.memory s) inputs) eq
... | vs , lvs , eq₁ = from-just-R (r-transient-hash lvs) eq₁

preprocess-instr→R-instr _ s (persistent-hash alignment inputs) s' eq
  with >>=-just (mem-lookups (Preprocessed.memory s) inputs) eq
... | vs , lvs , eq₁
  = subst (R-instr _ s (persistent-hash alignment inputs)) (just-injective eq₁) (r-persistent-hash lvs refl)

preprocess-instr→R-instr _ s (test-eq a b) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a) eq
... | av , la , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) b) eq₁
... | bv , lb , eq₂ = from-just-R (r-test-eq la lb) eq₂

preprocess-instr→R-instr _ s (add a b) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a) eq
... | av , la , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) b) eq₁
... | bv , lb , eq₂ = from-just-R (r-add la lb) eq₂

preprocess-instr→R-instr _ s (mul a b) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a) eq
... | av , la , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) b) eq₁
... | bv , lb , eq₂ = from-just-R (r-mul la lb) eq₂

preprocess-instr→R-instr _ s (neg a) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a) eq
... | av , la , eq₁ = from-just-R (r-neg la) eq₁

preprocess-instr→R-instr _ s (not a) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a >>= to-bool) eq
... | b , lb , eq₁ = from-just-R (r-not lb) eq₁

preprocess-instr→R-instr _ s (less-than a b bits) s' eq
  with >>=-just (mem-lookup (Preprocessed.memory s) a) eq
... | av , la , eq₁
  with >>=-just (mem-lookup (Preprocessed.memory s) b) eq₁
... | bv , lb , eq₂
  with if-just (does (bits <? FR-BITS) ∧ does (fits-in? av bits) ∧ does (fits-in? bv bits)) eq₂
... | guard , s-eq
  = let bnd , fts = Equivalence.to T-∧ guard
    in subst (R-instr _ s (less-than a b bits)) s-eq
         (r-less-than la lb (<ᵇ⇒< bits FR-BITS bnd) (lt-guard→ av bv bits fts))

preprocess-instr→R-instr _ s (public-input guard) s' eq
  with >>=-just (eval-guard (Preprocessed.memory s) guard) eq
... | false , gf , eq₁ = from-just-R (r-public-input-inactive gf) eq₁
... | true  , gt , eq₁
  with >>=-just (consume-pub-out s) eq₁
... | (v , s₁) , cp , eq₂ = from-just-R (r-public-input-active gt cp) eq₂

preprocess-instr→R-instr _ s (private-input guard) s' eq
  with >>=-just (eval-guard (Preprocessed.memory s) guard) eq
... | false , gf , eq₁ = from-just-R (r-private-input-inactive gf) eq₁
... | true  , gt , eq₁
  with >>=-just (consume-priv s) eq₁
... | (v , s₁) , cp , eq₂ = from-just-R (r-private-input-active gt cp) eq₂


-- Per-instruction: relational → computational

R-instr→preprocess-instr : ∀ pre s i s'
  → R-instr pre s i s' → preprocess-instr pre s i ≡ just s'
R-instr→preprocess-instr _ s (assert cond) _ (r-assert la) rewrite la = refl
R-instr→preprocess-instr _ s (cond-select bit a b) _ (r-cond-select lsel la lb) rewrite lsel | la | lb = refl
R-instr→preprocess-instr _ s (constrain-bits var bits) _ (r-constrain-bits {v = v} lv bound fits)
  rewrite lv | dec-true (bits <? FR-BITS) bound | fits-in?-true v bits fits = refl
R-instr→preprocess-instr _ s (constrain-eq a b) _ (r-constrain-eq {av = av} {bv = bv} la lb eq)
  rewrite la | lb | subst (λ z → (av ≡ᶠ? z) ≡ true) eq ≡ᶠ?-refl = refl
R-instr→preprocess-instr _ s (constrain-to-boolean var) _ (r-constrain-to-boolean lb) rewrite lb = refl
R-instr→preprocess-instr _ s (copy var) _ (r-copy lv) rewrite lv = refl
R-instr→preprocess-instr _ s (declare-pub-input var) _ (r-declare-pub-input lv) rewrite lv = refl
R-instr→preprocess-instr pre s (pi-skip guard count) _ (r-pi-skip-active gt chk)
  rewrite gt | Equivalence.to T-≡ chk = refl
R-instr→preprocess-instr pre s (pi-skip guard count) _ (r-pi-skip-inactive gf)   rewrite gf       = refl
R-instr→preprocess-instr _ s (ec-add a_x a_y b_x b_y) _ (r-ec-add lax lay lbx lby add-eq)
  rewrite lax | lay | lbx | lby | add-eq = refl
R-instr→preprocess-instr _ s (ec-mul a_x a_y scalar) _ (r-ec-mul lax lay lsc mul-eq)
  rewrite lax | lay | lsc | mul-eq = refl
R-instr→preprocess-instr _ s (ec-mul-generator scalar) _ (r-ec-mul-generator lsc gen-eq)
  rewrite lsc | gen-eq = refl
R-instr→preprocess-instr _ s (hash-to-curve inputs) _ (r-hash-to-curve lvs htc-eq)
  rewrite lvs | htc-eq = refl
R-instr→preprocess-instr _ s (load-imm imm) _ r-load-imm = refl
R-instr→preprocess-instr _ s (div-mod-power-of-two var bits) _ (r-div-mod-power-of-two lv le)
  rewrite lv | dec-true (bits ≤? FR-STORED-BITS) le = refl
R-instr→preprocess-instr _ s (reconstitute-field divisor modulus bits) _
  (r-reconstitute-field {dv = dv} {mv = mv} ldv lmv ge1 le (fmv , fdv , inf))
  rewrite ldv | lmv
        | dec-true (1 ≤? bits) ge1
        | dec-true (bits ≤? FR-STORED-BITS) le
        | fits-in?-true mv bits fmv
        | fits-in?-true dv (FR-BITS ∸ bits) fdv
        | bitsInField?-true (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv)) inf = refl
R-instr→preprocess-instr _ s (output var) _ (r-output lv) rewrite lv = refl
R-instr→preprocess-instr _ s (transient-hash inputs) _ (r-transient-hash lvs) rewrite lvs = refl
R-instr→preprocess-instr _ s (persistent-hash alignment inputs) _ (r-persistent-hash lvs ph-eq)
  rewrite lvs | ph-eq = refl
R-instr→preprocess-instr _ s (test-eq a b) _ (r-test-eq la lb) rewrite la | lb = refl
R-instr→preprocess-instr _ s (add a b) _ (r-add la lb) rewrite la | lb = refl
R-instr→preprocess-instr _ s (mul a b) _ (r-mul la lb) rewrite la | lb = refl
R-instr→preprocess-instr _ s (neg a) _ (r-neg la) rewrite la = refl
R-instr→preprocess-instr _ s (not a) _ (r-not lb) rewrite lb = refl
R-instr→preprocess-instr _ s (less-than a b bits) _ (r-less-than {av = av} {bv = bv} la lb bound (fav , fbv))
  rewrite la | lb | dec-true (bits <? FR-BITS) bound | fits-in?-true av bits fav | fits-in?-true bv bits fbv = refl
R-instr→preprocess-instr _ s (public-input guard) _ (r-public-input-inactive gf) rewrite gf       = refl
R-instr→preprocess-instr _ s (public-input guard) _ (r-public-input-active gt cp) rewrite gt | cp = refl
R-instr→preprocess-instr _ s (private-input guard) _ (r-private-input-inactive gf) rewrite gf       = refl
R-instr→preprocess-instr _ s (private-input guard) _ (r-private-input-active gt cp) rewrite gt | cp = refl


-- Lift faithfulness from instructions to instruction sequences.

preprocess-instrs→R-instrs : ∀ pre s is s'
  → preprocess-instrs pre s is ≡ just s' → R-instrs pre s is s'
preprocess-instrs→R-instrs _ s [] s' eq
  = subst (R-instrs _ s []) (just-injective eq) r-done
preprocess-instrs→R-instrs pre s (i ∷ is) s' eq
  with >>=-just (preprocess-instr pre s i) eq
... | s₁ , eq₁ , eq₂
  = r-step (preprocess-instr→R-instr pre s i s₁ eq₁)
           (preprocess-instrs→R-instrs pre s₁ is s' eq₂)

R-instrs→preprocess-instrs : ∀ pre s is s'
  → R-instrs pre s is s' → preprocess-instrs pre s is ≡ just s'
R-instrs→preprocess-instrs _ _ [] _ r-done = refl
R-instrs→preprocess-instrs pre s (i ∷ is) s' (r-step ri ris)
  with R-instr→preprocess-instr pre s i _ ri
... | eq₁ rewrite eq₁ = R-instrs→preprocess-instrs pre _ is s' ris

-- Top-level faithfulness.

preprocess→R : ∀ src pre s → preprocess src pre ≡ just s → R src pre s
preprocess→R src pre s eq
  with >>=-just (init-state src pre) eq
... | s₀ , eq₀ , eq₁
  with >>=-just (preprocess-instrs pre s₀ (IrSource.instructions src)) eq₁
... | s' , eq₂ , eq₃
  with if-just _ eq₃
... | tc-co , s'-eq =
  let tc , co = Equivalence.to T-∧ tc-co in
    s₀ , eq₀ ,
    subst (R-instrs pre s₀ (IrSource.instructions src)) s'-eq
      (preprocess-instrs→R-instrs pre s₀ (IrSource.instructions src) s' eq₂) ,
    subst (λ x → T (transcripts-consumed pre x)) s'-eq tc ,
    subst (λ x → T (comm-ok src pre x)) s'-eq co

R→preprocess : ∀ src pre s → R src pre s → preprocess src pre ≡ just s
R→preprocess src pre s (s₀ , init-eq , ris , tc , co)
  with R-instrs→preprocess-instrs pre s₀ (IrSource.instructions src) s ris
... | instrs-eq
  rewrite init-eq | instrs-eq | Equivalence.to T-≡ tc | Equivalence.to T-≡ co
  = refl

------------------------------------------------------------------------
-- 4. Memory monotonicity (the `≤` form of P2)
--
-- A successful step, sequence, or run never shrinks the memory.  Each
-- form follows from the exact `Δmem` accounting of section 2 (a step's
-- post-length is its pre-length plus a nonnegative delta) composed with
-- the faithfulness bridge of section 3.
------------------------------------------------------------------------

-- A successful step never shrinks the memory.
preprocess-instr-mem-≤ : ∀ pre s i s'
  → preprocess-instr pre s i ≡ just s'
  → length (Preprocessed.memory s) ≤ length (Preprocessed.memory s')
preprocess-instr-mem-≤ pre s i s' eq =
  subst (length (Preprocessed.memory s) ≤_)
    (sym (R-instr-Δmem (preprocess-instr→R-instr pre s i s' eq)))
    (m≤m+n _ _)

-- Executing a sequence of instructions does not shrink the memory.
preprocess-instrs-mono : ∀ pre s is s'
  → preprocess-instrs pre s is ≡ just s'
  → length (Preprocessed.memory s) ≤ length (Preprocessed.memory s')
preprocess-instrs-mono pre s is s' eq =
  subst (length (Preprocessed.memory s) ≤_)
    (sym (R-instrs-Δmem (preprocess-instrs→R-instrs pre s is s' eq)))
    (m≤m+n _ _)

-- A successful top-level preprocessing grows (or preserves) the memory.
preprocess-memory-mono : ∀ src pre s
  → preprocess src pre ≡ just s
  → length (ProofPreimage.inputs pre) ≤ length (Preprocessed.memory s)
preprocess-memory-mono src pre s eq =
  let r = preprocess→R src pre s eq
      (s₀ , init-eq , _) = r
  in subst (_≤ length (Preprocessed.memory s))
       (sym (init-state-num-inputs src pre s₀ init-eq))
       (subst (IrSource.num-inputs src ≤_)
         (sym (R-memory-length src pre s r))
         (m≤m+n _ _))

------------------------------------------------------------------------
-- 5. Circuit correctness (Property P5, spec §6.2)
--
-- The Halo2 constraint-synthesis function `circuit` is faithful to the
-- relational semantics `R`.
--
-- The faithful statement carries the following preconditions:
--
--   • producer-safety  `T (producer-safe src)`                  (§6.4)
--   • input arity      `length (inputs pre) ≡ num-inputs src`    (§3.3, WF1)
--   • bit bounds       `WF2 src`                                 (§3.3, WF2)
--   • shape            `preprocess-shaped src pre s`             (§5.3)
--
-- and concludes the spec's biconditional as a logical equivalence
-- (`_⇔_`):  `R src pre s ⇔ satisfies (circuit src) (witness-of s pre)`.
--
-- `circuit-faithful` is re-exported from `CircuitProof` (see the import
-- list above); its full statement is:
--
--   circuit-faithful : ∀ src pre s
--     → T (producer-safe src)
--     → length (inputs pre) ≡ num-inputs src
--     → WF2 src
--     → preprocess-shaped src pre s
--     → R src pre s ⇔ satisfies (circuit src) (witness-of s pre)
------------------------------------------------------------------------

------------------------------------------------------------------------
-- 6. Well-formedness preservation (Property P4, spec §6.1)
--
-- The spec's P4 makes three claims about every reachable intermediate
-- state of a preprocess run.  We mechanise all three over the relational
-- trace `R-instrs` (equivalently `preprocess`, via section 3's bridge):
--
--   (a) in-bounds.    A successful step references only memory indices
--                     `< |memory|` of its pre-state.  This is the
--                     contrapositive of the spec's "… < |Σᵢ.M| — or the
--                     transition fails": a step that does *not* fail
--                     (i.e. `R-instr` relates it) has all its operand
--                     indices in range.  (`R-instr→in-bounds`)
--   (b) PI bound.     `pub-in-idx` never exceeds the number of
--                     `declare-pub-input`s executed so far.
--                     (`pub-idx-trace`; corollary `preprocess-pub-in-idx-bound`)
--   (c) output count. `|outputs|` equals the number of `output`s executed
--                     so far — the cardinality of the spec's bijection
--                     between `Σ.ω` and the executed `Output`s.
--                     (`out-trace`; corollary `preprocess-output-count`)
--
-- None of the three needs the §3.3 well-formedness hypotheses: they hold
-- for every trace the semantics admits.  (Well-formedness is what makes
-- such traces *exist* for honest producers; the invariants themselves are
-- structural.)  The top-level corollaries specialise (b)/(c) to a full
-- `preprocess` run, where the initial `pub-in-idx`/`outputs` are `0`/`[]`.
------------------------------------------------------------------------

-- A successful memory lookup proves its index is in range.
mem-lookup-< : ∀ (mem : List Fr) idx {v}
  → mem-lookup mem idx ≡ just v → idx < length mem
mem-lookup-< (x ∷ _)  zero    _  = s≤s z≤n
mem-lookup-< (_ ∷ xs) (suc n) eq = s≤s (mem-lookup-< xs n eq)
mem-lookup-< []       _       ()

-- Likewise for a lookup followed by a boolean coercion.
lookup-bool-< : ∀ (mem : List Fr) idx {b}
  → (mem-lookup mem idx >>= to-bool) ≡ just b → idx < length mem
lookup-bool-< mem idx eq with >>=-just (mem-lookup mem idx) eq
... | _ , lk , _ = mem-lookup-< mem idx lk

-- A successful multi-lookup proves every index is in range.
mem-lookups-all : ∀ (mem : List Fr) is {vs}
  → mem-lookups mem is ≡ just vs → All (_< length mem) is
mem-lookups-all _   []       _  = ⟨⟩
mem-lookups-all mem (i ∷ is) eq with >>=-just (mem-lookup mem i) eq
... | _ , lk , eq₁ with >>=-just (mem-lookups mem is) eq₁
...   | _ , lks , _ = mem-lookup-< mem i lk ◂ mem-lookups-all mem is lks

-- The memory indices a guard reads (empty guard reads nothing).
idx-of : Maybe Index → List Index
idx-of nothing  = []
idx-of (just i) = i ∷ []

eval-guard-all : ∀ (mem : List Fr) g {b}
  → eval-guard mem g ≡ just b → All (_< length mem) (idx-of g)
eval-guard-all _   nothing    _  = ⟨⟩
eval-guard-all mem (just idx) eq = lookup-bool-< mem idx eq ◂ ⟨⟩

-- The memory indices an instruction reads.
refs : Instruction → List Index
refs (assert cond)               = cond ∷ []
refs (cond-select bit a b)       = bit ∷ a ∷ b ∷ []
refs (constrain-bits var _)      = var ∷ []
refs (constrain-eq a b)          = a ∷ b ∷ []
refs (constrain-to-boolean var)  = var ∷ []
refs (copy var)                  = var ∷ []
refs (declare-pub-input var)     = var ∷ []
refs (pi-skip guard _)           = idx-of guard
refs (ec-add a_x a_y b_x b_y)    = a_x ∷ a_y ∷ b_x ∷ b_y ∷ []
refs (ec-mul a_x a_y scalar)     = a_x ∷ a_y ∷ scalar ∷ []
refs (ec-mul-generator scalar)   = scalar ∷ []
refs (hash-to-curve inputs)      = inputs
refs (load-imm _)                = []
refs (div-mod-power-of-two var _) = var ∷ []
refs (reconstitute-field d m _)  = d ∷ m ∷ []
refs (output var)                = var ∷ []
refs (transient-hash inputs)     = inputs
refs (persistent-hash _ inputs)  = inputs
refs (test-eq a b)               = a ∷ b ∷ []
refs (add a b)                   = a ∷ b ∷ []
refs (mul a b)                   = a ∷ b ∷ []
refs (neg a)                     = a ∷ []
refs (not a)                     = a ∷ []
refs (less-than a b _)           = a ∷ b ∷ []
refs (public-input guard)        = idx-of guard
refs (private-input guard)       = idx-of guard

-- P4(a): every index a non-failing step references is `< |memory|`.
R-instr→in-bounds : ∀ {pre i s'} s → R-instr pre s i s'
  → All (_< length (Preprocessed.memory s)) (refs i)
R-instr→in-bounds s (r-assert p)                = lookup-bool-< M _ p ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-cond-select ps pa pb)    = lookup-bool-< M _ ps ◂ mem-lookup-< M _ pa ◂ mem-lookup-< M _ pb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-constrain-bits lv _ _)   = mem-lookup-< M _ lv ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-constrain-eq la lb _)    = mem-lookup-< M _ la ◂ mem-lookup-< M _ lb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-constrain-to-boolean lb) = lookup-bool-< M _ lb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-copy lv)                 = mem-lookup-< M _ lv ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-declare-pub-input lv)    = mem-lookup-< M _ lv ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-pi-skip-active {guard = guard} g _) = eval-guard-all (Preprocessed.memory s) guard g
R-instr→in-bounds s (r-pi-skip-inactive {guard = guard} g) = eval-guard-all (Preprocessed.memory s) guard g
R-instr→in-bounds s (r-ec-add lax lay lbx lby _) = mem-lookup-< M _ lax ◂ mem-lookup-< M _ lay ◂ mem-lookup-< M _ lbx ◂ mem-lookup-< M _ lby ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-ec-mul lax lay lsc _)    = mem-lookup-< M _ lax ◂ mem-lookup-< M _ lay ◂ mem-lookup-< M _ lsc ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-ec-mul-generator lsc _)  = mem-lookup-< M _ lsc ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-hash-to-curve lvs _)     = mem-lookups-all (Preprocessed.memory s) _ lvs
R-instr→in-bounds s r-load-imm                  = ⟨⟩
R-instr→in-bounds s (r-div-mod-power-of-two lv _) = mem-lookup-< M _ lv ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-reconstitute-field ldv lmv _ _ _) = mem-lookup-< M _ ldv ◂ mem-lookup-< M _ lmv ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-output lv)               = mem-lookup-< M _ lv ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-transient-hash lvs)      = mem-lookups-all (Preprocessed.memory s) _ lvs
R-instr→in-bounds s (r-persistent-hash lvs _)   = mem-lookups-all (Preprocessed.memory s) _ lvs
R-instr→in-bounds s (r-test-eq la lb)           = mem-lookup-< M _ la ◂ mem-lookup-< M _ lb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-add la lb)               = mem-lookup-< M _ la ◂ mem-lookup-< M _ lb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-mul la lb)               = mem-lookup-< M _ la ◂ mem-lookup-< M _ lb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-neg la)                  = mem-lookup-< M _ la ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-not lb)                  = lookup-bool-< M _ lb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-less-than la lb _ _)     = mem-lookup-< M _ la ◂ mem-lookup-< M _ lb ◂ ⟨⟩  where M = Preprocessed.memory s
R-instr→in-bounds s (r-public-input-inactive {guard = guard} g)  = eval-guard-all (Preprocessed.memory s) guard g
R-instr→in-bounds s (r-public-input-active {guard = guard} g _)  = eval-guard-all (Preprocessed.memory s) guard g
R-instr→in-bounds s (r-private-input-inactive {guard = guard} g) = eval-guard-all (Preprocessed.memory s) guard g
R-instr→in-bounds s (r-private-input-active {guard = guard} g _) = eval-guard-all (Preprocessed.memory s) guard g

------------------------------------------------------------------------
-- P4(b): public-input-index bound.
------------------------------------------------------------------------

-- consume-pub-out / consume-priv preserve pub-in-idx and outputs.
consume-pub-out-idx : ∀ s v s₁ → consume-pub-out s ≡ just (v , s₁)
  → Preprocessed.pub-in-idx s₁ ≡ Preprocessed.pub-in-idx s
consume-pub-out-idx s v s₁ eq with Preprocessed.pub-out-rem s | eq
... | []    | ()
... | _ ∷ _ | p = sym (cong Preprocessed.pub-in-idx (cong proj₂ (just-injective p)))

consume-priv-idx : ∀ s v s₁ → consume-priv s ≡ just (v , s₁)
  → Preprocessed.pub-in-idx s₁ ≡ Preprocessed.pub-in-idx s
consume-priv-idx s v s₁ eq with Preprocessed.priv-rem s | eq
... | []    | ()
... | _ ∷ _ | p = sym (cong Preprocessed.pub-in-idx (cong proj₂ (just-injective p)))

consume-pub-out-out : ∀ s v s₁ → consume-pub-out s ≡ just (v , s₁)
  → Preprocessed.outputs s₁ ≡ Preprocessed.outputs s
consume-pub-out-out s v s₁ eq with Preprocessed.pub-out-rem s | eq
... | []    | ()
... | _ ∷ _ | p = sym (cong Preprocessed.outputs (cong proj₂ (just-injective p)))

consume-priv-out : ∀ s v s₁ → consume-priv s ≡ just (v , s₁)
  → Preprocessed.outputs s₁ ≡ Preprocessed.outputs s
consume-priv-out s v s₁ eq with Preprocessed.priv-rem s | eq
... | []    | ()
... | _ ∷ _ | p = sym (cong Preprocessed.outputs (cong proj₂ (just-injective p)))

-- Rearrangement used by both trace folds: b + (a + c) ≡ (a + b) + c.
+-rearr : ∀ a b c → b + (a + c) ≡ (a + b) + c
+-rearr a b c = trans (sym (+-assoc b a c)) (cong (_+ c) (+-comm b a))

-- How much each instruction can advance pub-in-idx (only DeclarePubInput).
declare-count : Instruction → ℕ
declare-count (declare-pub-input _) = 1
declare-count _                     = 0

count-declares : List Instruction → ℕ
count-declares []       = 0
count-declares (i ∷ is) = declare-count i + count-declares is

-- Per step: pub-in-idx grows by at most `declare-count i` (PiSkip may
-- shrink it; everything but DeclarePubInput leaves it untouched).
pub-idx-step : ∀ {pre s i s'} → R-instr pre s i s'
  → Preprocessed.pub-in-idx s' ≤ declare-count i + Preprocessed.pub-in-idx s
pub-idx-step (r-assert _)                 = ≤-refl
pub-idx-step (r-cond-select _ _ _)        = ≤-refl
pub-idx-step (r-constrain-bits _ _ _)       = ≤-refl
pub-idx-step (r-constrain-eq _ _ _)       = ≤-refl
pub-idx-step (r-constrain-to-boolean _)   = ≤-refl
pub-idx-step (r-copy _)                   = ≤-refl
pub-idx-step (r-declare-pub-input _)      = ≤-refl
pub-idx-step (r-pi-skip-active _ _)       = ≤-refl
pub-idx-step (r-pi-skip-inactive {s = s} {count = count} _) = m∸n≤m (Preprocessed.pub-in-idx s) count
pub-idx-step (r-ec-add _ _ _ _ _)         = ≤-refl
pub-idx-step (r-ec-mul _ _ _ _)           = ≤-refl
pub-idx-step (r-ec-mul-generator _ _)     = ≤-refl
pub-idx-step (r-hash-to-curve _ _)        = ≤-refl
pub-idx-step r-load-imm                   = ≤-refl
pub-idx-step (r-div-mod-power-of-two _ _)   = ≤-refl
pub-idx-step (r-reconstitute-field _ _ _ _ _) = ≤-refl
pub-idx-step (r-output _)                 = ≤-refl
pub-idx-step (r-transient-hash _)         = ≤-refl
pub-idx-step (r-persistent-hash _ _)      = ≤-refl
pub-idx-step (r-test-eq _ _)              = ≤-refl
pub-idx-step (r-add _ _)                  = ≤-refl
pub-idx-step (r-mul _ _)                  = ≤-refl
pub-idx-step (r-neg _)                    = ≤-refl
pub-idx-step (r-not _)                    = ≤-refl
pub-idx-step (r-less-than _ _ _ _)          = ≤-refl
pub-idx-step (r-public-input-inactive _)  = ≤-refl
pub-idx-step (r-public-input-active _ cp) = ≤-reflexive (consume-pub-out-idx _ _ _ cp)
pub-idx-step (r-private-input-inactive _) = ≤-refl
pub-idx-step (r-private-input-active _ cp) = ≤-reflexive (consume-priv-idx _ _ _ cp)

-- Over a whole trace: pub-in-idx ≤ #DeclarePubInputs executed (+ initial).
pub-idx-trace : ∀ {pre s₀ is s} → R-instrs pre s₀ is s
  → Preprocessed.pub-in-idx s ≤ count-declares is + Preprocessed.pub-in-idx s₀
pub-idx-trace r-done = ≤-refl
pub-idx-trace {s₀ = s₀} (r-step {s₁ = s₁} {i = i} {is = is} ri ris) =
  ≤-trans (pub-idx-trace ris)
    (≤-trans (+-monoʳ-≤ (count-declares is) (pub-idx-step ri))
             (≤-reflexive (+-rearr (declare-count i) (count-declares is) (Preprocessed.pub-in-idx s₀))))

------------------------------------------------------------------------
-- P4(c): output count.
------------------------------------------------------------------------

output-count : Instruction → ℕ
output-count (output _) = 1
output-count _          = 0

count-outputs : List Instruction → ℕ
count-outputs []       = 0
count-outputs (i ∷ is) = output-count i + count-outputs is

-- Per step: |outputs| grows by exactly `output-count i`.
out-step : ∀ {pre s i s'} → R-instr pre s i s'
  → length (Preprocessed.outputs s') ≡ output-count i + length (Preprocessed.outputs s)
out-step (r-assert _)                 = refl
out-step (r-cond-select _ _ _)        = refl
out-step (r-constrain-bits _ _ _)       = refl
out-step (r-constrain-eq _ _ _)       = refl
out-step (r-constrain-to-boolean _)   = refl
out-step (r-copy _)                   = refl
out-step (r-declare-pub-input _)      = refl
out-step (r-pi-skip-active _ _)       = refl
out-step (r-pi-skip-inactive _)       = refl
out-step (r-ec-add _ _ _ _ _)         = refl
out-step (r-ec-mul _ _ _ _)           = refl
out-step (r-ec-mul-generator _ _)     = refl
out-step (r-hash-to-curve _ _)        = refl
out-step r-load-imm                   = refl
out-step (r-div-mod-power-of-two _ _)   = refl
out-step (r-reconstitute-field _ _ _ _ _) = refl
out-step (r-output {s = s} _)         = trans (length-++ (Preprocessed.outputs s))
                                              (+-comm (length (Preprocessed.outputs s)) 1)
out-step (r-transient-hash _)         = refl
out-step (r-persistent-hash _ _)      = refl
out-step (r-test-eq _ _)              = refl
out-step (r-add _ _)                  = refl
out-step (r-mul _ _)                  = refl
out-step (r-neg _)                    = refl
out-step (r-not _)                    = refl
out-step (r-less-than _ _ _ _)          = refl
out-step (r-public-input-inactive _)  = refl
out-step (r-public-input-active _ cp) = cong length (consume-pub-out-out _ _ _ cp)
out-step (r-private-input-inactive _) = refl
out-step (r-private-input-active _ cp) = cong length (consume-priv-out _ _ _ cp)

-- Over a whole trace: |outputs| = #Outputs executed (+ initial).
out-trace : ∀ {pre s₀ is s} → R-instrs pre s₀ is s
  → length (Preprocessed.outputs s) ≡ count-outputs is + length (Preprocessed.outputs s₀)
out-trace r-done = refl
out-trace {s₀ = s₀} (r-step {s₁ = s₁} {i = i} {is = is} ri ris) =
  trans (out-trace ris)
    (trans (cong (count-outputs is +_) (out-step ri))
           (+-rearr (output-count i) (count-outputs is) (length (Preprocessed.outputs s₀))))

------------------------------------------------------------------------
-- Top-level corollaries over a full `preprocess` run.
------------------------------------------------------------------------

-- P4(b): a successful preprocess has pub-in-idx ≤ #DeclarePubInputs.
preprocess-pub-in-idx-bound : ∀ src pre s
  → preprocess src pre ≡ just s
  → Preprocessed.pub-in-idx s ≤ count-declares (IrSource.instructions src)
preprocess-pub-in-idx-bound src pre s eq
  with preprocess→R src pre s eq
... | s₀ , init-eq , ris , _ , _ =
  ≤-trans
    (subst (λ n → Preprocessed.pub-in-idx s ≤ count-declares (IrSource.instructions src) + n)
           (init-state-pub-in-idx src pre s₀ init-eq)
           (pub-idx-trace ris))
    (≤-reflexive (+-identityʳ (count-declares (IrSource.instructions src))))

-- P4(c): a successful preprocess records exactly #Outputs values.
preprocess-output-count : ∀ src pre s
  → preprocess src pre ≡ just s
  → length (Preprocessed.outputs s) ≡ count-outputs (IrSource.instructions src)
preprocess-output-count src pre s eq
  with preprocess→R src pre s eq
... | s₀ , init-eq , ris , _ , _ =
  trans (out-trace ris)
    (trans (cong (count-outputs (IrSource.instructions src) +_)
                 (cong length (init-state-outputs src pre s₀ init-eq)))
           (+-identityʳ (count-outputs (IrSource.instructions src))))
