{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.CircuitProof (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.FieldProperties ⋯
open import zkir-v2.Encoding ⋯

------------------------------------------------------------------------
-- Circuit-faithfulness bridging
--
-- This module carries the program-level induction that connects the
-- operational relation `R src pre s` to the in-circuit satisfaction
-- relation `satisfies (circuit src) (witness-of s pre)` (spec §6.2).
-- `circuit-faithful` is exported as a logical equivalence (`_⇔_`) and
-- re-exported from `Properties`.  It comprises:
--
--   • the forward direction: R-instrs ⇒ satisfies-constraints;
--   • soundness of O2 / O3 over R-instrs traces;
--   • the backward direction: satisfies-constraints ⇒ R-instrs (D1/D2),
--     and the top-level `circuit-faithful-bwd` (D3), quantified over
--     the §5.3 `preprocess-shaped` states with the §3.3 WF1 arity
--     hypothesis and a `transcripts-consumed` shape conjunct (both
--     required — see the notes at D3).
--
-- No axioms are introduced here: every lemma is discharged by an
-- inductive or equational proof, resting only on the field/crypto
-- assumptions in the `Assumptions` record.
------------------------------------------------------------------------

open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
open import zkir-v2.Circuit ⋯ hiding (witness-of; comm-rand-of)
open import zkir-v2.Circuit ⋯ public using (witness-of; comm-rand-of)
open import zkir-v2.SemanticsLemmas ⋯
  using ( lookup-extends; mem-lookups-extends; push-mem2-assoc
        ; consume-pub-out-mem; consume-pub-out-pis
        ; consume-priv-mem; consume-priv-pis
        ; consume-pub-out-outputs; consume-priv-outputs
        ; init-state-memory; init-state-num-inputs
        ; init-state-outputs; init-state-inv; InitInv )
open import zkir-v2.CircuitFaithfulness ⋯
open import zkir-v2.Obligations ⋯
  using ( producer-safe
        ; wire-disc; WireOK; wireOK?; wire-step; wire-scan
        ; Wire-Trace; wire-done; wire-cons
        ; Δmem; GuardOK
        ; InstrShape; shape-Δmem; shape-Δpis
        ; ShapeView; shapeView; shape-of
        ; sv-pure0; sv-pure1; sv-pure2; sv-declare
        ; sv-output; sv-skip; sv-pub-in; sv-priv-in
        ; IndexSet; PartialMap; lookupᵐ
        ; O2-check; O3OK; o3OK?; require-∈
        ; O2-step; O3-step
        ; O2-Trace; o2-done; o2-step
        ; O3-Trace; o3-done; o3-step
        ; O2-Runs; O3-Runs
        ; O2-bool→Runs; O3-bool→Runs
        )
open import zkir-v2.ObligationsSoundness ⋯
  using ( producer-safe-wire-disc
        ; producer-safe-O2; producer-safe-O3
        ; o2-inv-init; o3-inv-init
        ; O2-Inv; O3-Inv
        ; o2-known-is-bit; o3-known-fits
        ; o2-preserve; o3-preserve
        )

open import Data.Bool    using (Bool; true; false; if_then_else_; T)
import Data.Bool as Bool
import Data.Bool.Properties
open import Data.List    using (List; []; _∷_; _++_; _∷ʳ_; length; map; take; drop)
open import Data.List.Properties using (++-assoc; ++-identityʳ)
open import Data.Maybe   using (Maybe; nothing; just; _>>=_)
open import Data.Maybe.Properties using (just-injective)
open import Data.Nat     using (ℕ; suc; zero; _+_; _∸_; _<_; _≡ᵇ_)
import Data.Nat
-- Decidable propositional membership on `IndexSet = List Index`, matching
-- the instance used in `Obligations`/`ObligationsSoundness`.
open import Data.List.Membership.DecPropositional (Data.Nat._≟_) using (_∈_; _∈?_)
-- `All` for the list-operand wire-discipline predicate (`WireOK`'s hash
-- cases).  Constructors renamed to avoid clashing with `Data.List`.
open import Data.List.Relation.Unary.All using (All)
  renaming ([] to []ᴬ; _∷_ to _∷ᴬ_; map to mapᴬ; head to headᴬ)
open import Data.List.Relation.Unary.All.Properties using (++⁻)
  renaming (++⁺ to ++ᴬ)
open import Data.Nat.Properties using (+-suc; +-identityʳ; +-comm; m≤n⇒m≤n+o; <-≤-trans)
import Data.Nat.Properties
open import Data.Product using (_×_; _,_; proj₁; proj₂; ∃-syntax; Σ)
open import Data.Sum     using (_⊎_; inj₁; inj₂)
open import Data.Unit    using (⊤; tt)
open import Data.Empty   using (⊥; ⊥-elim)
open import Function.Bundles using (_⇔_; mk⇔)
import Function.Bundles
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; cong₂; subst; subst₂)
open import Relation.Nullary using (¬_; yes; no)

------------------------------------------------------------------------
-- Section A.  Witness-of:  Preprocessed × ProofPreimage  →  Witness.
--
-- The witness assignment produced by an operational execution.  Three
-- fields:
--
--   • mem        : Preprocessed.memory s    (all allocated wire values)
--   • pis        : Preprocessed.pis    s    (verifier-supplied entries)
--   • comm-rand  : the randomness portion of the optional commitment.
--
-- Note: `witness-of` does NOT depend on the `IrSource`.  The
-- *circuit*'s `has-comm` flag determines the expected shape of
-- `comm-rand`, and the `Maybe-shape` predicate in `satisfies` enforces
-- the match.  Producer-safety (+ the operational `init-state`
-- precondition) is what guarantees the shapes line up at the top level.
------------------------------------------------------------------------

-- Bridge from the propositional equality `b ≡ true` to the `T b` check,
-- used where a `with`/`subst` that drives shape computation off the
-- equality must hand its result to a Bool obligation phrased as `T`.
private
  ≡→T : ∀ {b} → b ≡ true → T b
  ≡→T refl = tt

------------------------------------------------------------------------
-- Section B.  Synth-state invariants.
--
-- During the induction along an `R-instrs pre s₀ is s_end` trace, we
-- maintain a *parallel* synth-state evolved by `circuit-instrs hc`.
-- The forward lemma asserts that the synth-state's accumulated constraints
-- are all satisfied by the assignment derived from the *post*-trace
-- preprocessed state.
--
-- Two structural invariants get threaded:
--
--   I-mem  :  SynthState.nr-wires   ≡  length (Preprocessed.memory)
--   I-pi   :  preamble-pi-count hc + SynthState.nr-declared-pi
--             ≡  length (Preprocessed.pis)
--
-- I-mem says every wire allocated by synthesis corresponds to a memory
-- cell in the operational state; I-pi says the PI-vector length that
-- synthesis tracks (preamble plus declares) matches the operational
-- `pis` — needed so that `bind` references a valid PI entry.
--
-- The forward induction discharges these from the per-instruction
-- faithfulness lemmas in `CircuitFaithfulness.agda`.
------------------------------------------------------------------------

-- Memory-length invariant.
mem-inv : Preprocessed → SynthState → Set
mem-inv s st = SynthState.nr-wires st ≡ length (Preprocessed.memory s)

-- PI-length invariant.  Parameterized on `has-comm`.
pi-inv : Bool → Preprocessed → SynthState → Set
pi-inv hc s st =
  length (Preprocessed.pis s) ≡ preamble-pi-count hc + SynthState.nr-declared-pi st

------------------------------------------------------------------------
-- Section B.5.  Memory/PI monotonicity along R-instrs.
--
-- Auxiliary lemmas used by both the forward dispatcher and the
-- top-level comm-commitment gluing.
------------------------------------------------------------------------

private

  -- Evaluation of a field expression is preserved by appending a suffix
  -- to the memory: every wire reference resolves through `mem-lookup`,
  -- which is monotone (`lookup-extends`).  Used by the `gate` case of
  -- `holds-mem-extends`.
  eval-mem-extends : ∀ {mem suffix} (e : Expr) {v}
    → eval mem e ≡ just v
    → eval (mem ++ suffix) e ≡ just v
  eval-mem-extends {mem} {suffix} (wire i) ev = lookup-extends mem suffix i ev
  eval-mem-extends (con k) ev = ev
  eval-mem-extends {mem} {suffix} (l ⊕ r) ev
    with x , y , el , er , v≡ ← eval-⊕ mem l r ev
    rewrite eval-mem-extends {mem} {suffix} l el
          | eval-mem-extends {mem} {suffix} r er = cong just (sym v≡)
  eval-mem-extends {mem} {suffix} (l ⊗ r) ev
    with x , y , el , er , v≡ ← eval-⊗ mem l r ev
    rewrite eval-mem-extends {mem} {suffix} l el
          | eval-mem-extends {mem} {suffix} r er = cong just (sym v≡)
  eval-mem-extends {mem} {suffix} (⊝ e) ev
    with x , ee , v≡ ← eval-⊝ mem e ev
    rewrite eval-mem-extends {mem} {suffix} e ee = cong just (sym v≡)

  -- Every wire referenced by `e` is `< n`.  The bound premise for the
  -- `gate` case of `holds-mem-shrink`.
  expr-mem-fits : Expr → ℕ → Set
  expr-mem-fits (wire i)  n = i < n
  expr-mem-fits (con _)   _ = ⊤
  expr-mem-fits (l ⊕ r)   n = expr-mem-fits l n × expr-mem-fits r n
  expr-mem-fits (l ⊗ r)   n = expr-mem-fits l n × expr-mem-fits r n
  expr-mem-fits (⊝ e)     n = expr-mem-fits e n

  -- Dual of `eval-mem-extends`: when every wire of `e` fits within
  -- `length mem`, an evaluation over `mem ++ suffix` pulls back to `mem`.
  eval-mem-shrink : ∀ (mem suffix : List Fr) (e : Expr) {v}
    → expr-mem-fits e (length mem)
    → eval (mem ++ suffix) e ≡ just v
    → eval mem e ≡ just v
  eval-mem-shrink mem suf (wire i) i< ev = lookup-shrink mem suf i ev i<
  eval-mem-shrink mem suf (con k) _ ev = ev
  eval-mem-shrink mem suf (l ⊕ r) (lf , rf) ev
    with x , y , el , er , v≡ ← eval-⊕ (mem ++ suf) l r ev
    rewrite eval-mem-shrink mem suf l lf el
          | eval-mem-shrink mem suf r rf er = cong just (sym v≡)
  eval-mem-shrink mem suf (l ⊗ r) (lf , rf) ev
    with x , y , el , er , v≡ ← eval-⊗ (mem ++ suf) l r ev
    rewrite eval-mem-shrink mem suf l lf el
          | eval-mem-shrink mem suf r rf er = cong just (sym v≡)
  eval-mem-shrink mem suf (⊝ e) ef ev
    with x , ee , v≡ ← eval-⊝ (mem ++ suf) e ev
    rewrite eval-mem-shrink mem suf e ef ee = cong just (sym v≡)

  -- One R-instr step's memory only grows: post-mem = pre-mem ++ suffix
  -- for some suffix.  This is a single existential lemma; we package
  -- the suffix as a List Fr.
  mem-extends-R-instr : ∀ {pre s i s'}
    → R-instr pre s i s'
    → Σ (List Fr) λ suf →
        Preprocessed.memory s' ≡ Preprocessed.memory s ++ suf
  mem-extends-R-instr (r-assert _)                       = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-cond-select {av = av} {bv = bv} _ _ _) =
    _ ∷ [] , refl
  mem-extends-R-instr (r-constrain-bits _ _ _)             = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-constrain-eq _ _ _)             = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-constrain-to-boolean _)         = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-copy {v = v} _)                 = v ∷ [] , refl
  mem-extends-R-instr (r-declare-pub-input _)            = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-pi-skip-active _ _)             = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-pi-skip-inactive _)             = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-ec-add {cx = cx} {cy = cy} _ _ _ _ _) =
    cx ∷ cy ∷ [] , refl
  mem-extends-R-instr (r-ec-mul {cx = cx} {cy = cy} _ _ _ _) =
    cx ∷ cy ∷ [] , refl
  mem-extends-R-instr (r-ec-mul-generator {cx = cx} {cy = cy} _ _) =
    cx ∷ cy ∷ [] , refl
  mem-extends-R-instr (r-hash-to-curve {cx = cx} {cy = cy} _ _) =
    cx ∷ cy ∷ [] , refl
  mem-extends-R-instr (r-load-imm {imm = imm})          = imm ∷ [] , refl
  mem-extends-R-instr {s = s} (r-div-mod-power-of-two {bits = bits} {v = vv} _ _) =
    let mem = Preprocessed.memory s
        divisor = from-le-bits (drop bits (to-le-bits vv))
        modulus = from-le-bits (take bits (to-le-bits vv))
    in divisor ∷ modulus ∷ [] , ++-assoc mem (divisor ∷ []) (modulus ∷ [])
  mem-extends-R-instr (r-reconstitute-field _ _ _ _ _)       = _ ∷ [] , refl
  mem-extends-R-instr (r-output _)                       = [] , sym (++-identityʳ _)
  mem-extends-R-instr (r-transient-hash {vs = vs} _)     = transient-hash-fn vs ∷ [] , refl
  mem-extends-R-instr (r-persistent-hash {h₁ = h₁} {h₂ = h₂} _ _) =
    h₁ ∷ h₂ ∷ [] , refl
  mem-extends-R-instr (r-test-eq _ _)                    = _ ∷ [] , refl
  mem-extends-R-instr (r-add _ _)                        = _ ∷ [] , refl
  mem-extends-R-instr (r-mul _ _)                        = _ ∷ [] , refl
  mem-extends-R-instr (r-neg _)                          = _ ∷ [] , refl
  mem-extends-R-instr (r-not _)                          = _ ∷ [] , refl
  mem-extends-R-instr (r-less-than _ _ _ _)                = _ ∷ [] , refl
  mem-extends-R-instr (r-public-input-inactive _)        = 0ᶠ ∷ [] , refl
  mem-extends-R-instr {s = s} (r-public-input-active {v = v} {s₁ = s₁} _ cp) =
    v ∷ [] , cong (_++ (v ∷ [])) (consume-pub-out-mem s cp)
  mem-extends-R-instr (r-private-input-inactive _)       = 0ᶠ ∷ [] , refl
  mem-extends-R-instr {s = s} (r-private-input-active {v = v} {s₁ = s₁} _ cp) =
    v ∷ [] , cong (_++ (v ∷ [])) (consume-priv-mem s cp)

  -- Holds is preserved by extending the witness's memory with a suffix.
  -- The pis and comm-rand fields are unchanged.  By case analysis on the
  -- constraint: every constraint uses `mem-lookup` (or `mem-lookups`) on the
  -- witness's memory, and these are monotone.
  holds-mem-extends : ∀ {mem suffix pis rand} (cl : Constraint)
    → holds (mk-witness mem pis rand) cl
    → holds (mk-witness (mem ++ suffix) pis rand) cl
  holds-mem-extends {mem} {suffix} (non-zero c)
    (v , lv , v≢0) =
    v , lookup-extends mem suffix c lv , v≢0
  holds-mem-extends {mem} {suffix} (select out b a c)
    (bv , av , cv , ov , lb , la , lc , lout , bit , eq) =
    bv , av , cv , ov
    , lookup-extends mem suffix b lb
    , lookup-extends mem suffix a la
    , lookup-extends mem suffix c lc
    , lookup-extends mem suffix out lout
    , bit , eq
  holds-mem-extends {mem} {suffix} (in-range v bits)
    (vv , lv , fits) =
    vv , lookup-extends mem suffix v lv , fits
  holds-mem-extends {mem} {suffix} (gate e) h = eval-mem-extends e h
  holds-mem-extends {mem} {suffix} (boolean v)
    (vv , lv , bit) =
    vv , lookup-extends mem suffix v lv , bit
  holds-mem-extends {mem} {suffix} (ec-add c-x c-y a-x a-y b-x b-y)
    (ax , ay , bx , by , cx , cy , lax , lay , lbx , lby , lcx , lcy , add-eq) =
    ax , ay , bx , by , cx , cy
    , lookup-extends mem suffix a-x lax
    , lookup-extends mem suffix a-y lay
    , lookup-extends mem suffix b-x lbx
    , lookup-extends mem suffix b-y lby
    , lookup-extends mem suffix c-x lcx
    , lookup-extends mem suffix c-y lcy
    , add-eq
  holds-mem-extends {mem} {suffix} (ec-mul c-x c-y a-x a-y scalar)
    (ax , ay , sc , cx , cy , lax , lay , lsc , lcx , lcy , mul-eq) =
    ax , ay , sc , cx , cy
    , lookup-extends mem suffix a-x lax
    , lookup-extends mem suffix a-y lay
    , lookup-extends mem suffix scalar lsc
    , lookup-extends mem suffix c-x lcx
    , lookup-extends mem suffix c-y lcy
    , mul-eq
  holds-mem-extends {mem} {suffix} (ec-gen c-x c-y scalar)
    (sc , cx , cy , lsc , lcx , lcy , gen-eq) =
    sc , cx , cy
    , lookup-extends mem suffix scalar lsc
    , lookup-extends mem suffix c-x lcx
    , lookup-extends mem suffix c-y lcy
    , gen-eq
  holds-mem-extends {mem} {suffix} (h2c c-x c-y inputs)
    (vs , cx , cy , lvs , lcx , lcy , hash-eq) =
    vs , cx , cy
    , mem-lookups-extends mem suffix inputs lvs
    , lookup-extends mem suffix c-x lcx
    , lookup-extends mem suffix c-y lcy
    , hash-eq
  holds-mem-extends {mem} {suffix} (div-mod q r v bits)
    (qv , rv , vv , lq , lr , lv , fr , fq , nw , eq) =
    qv , rv , vv
    , lookup-extends mem suffix q lq
    , lookup-extends mem suffix r lr
    , lookup-extends mem suffix v lv
    , fr , fq , nw , eq
  holds-mem-extends {mem} {suffix} (reconstitute out d m bits)
    (dv , mv , ov , ld , lm , lout , fd , fm , eq) =
    dv , mv , ov
    , lookup-extends mem suffix d ld
    , lookup-extends mem suffix m lm
    , lookup-extends mem suffix out lout
    , fd , fm , eq
  holds-mem-extends {mem} {suffix} (poseidon out inputs)
    (vs , ov , lvs , lout , eq) =
    vs , ov
    , mem-lookups-extends mem suffix inputs lvs
    , lookup-extends mem suffix out lout
    , eq
  holds-mem-extends {mem} {suffix} (sha256 h₁ h₂ alignment inputs)
    (vs , v1 , v2 , lvs , lh₁ , lh₂ , hash-eq) =
    vs , v1 , v2
    , mem-lookups-extends mem suffix inputs lvs
    , lookup-extends mem suffix h₁ lh₁
    , lookup-extends mem suffix h₂ lh₂
    , hash-eq
  holds-mem-extends {mem} {suffix} (test-eq out a b)
    (av , bv , ov , la , lb , lout , eq) =
    av , bv , ov
    , lookup-extends mem suffix a la
    , lookup-extends mem suffix b lb
    , lookup-extends mem suffix out lout
    , eq
  holds-mem-extends {mem} {suffix} (is-zero out a)
    (av , ov , la , lout , eq) =
    av , ov
    , lookup-extends mem suffix a la
    , lookup-extends mem suffix out lout
    , eq
  holds-mem-extends {mem} {suffix} (less-than out a b bits)
    (av , bv , ov , la , lb , lout , fa , fb , eq) =
    av , bv , ov
    , lookup-extends mem suffix a la
    , lookup-extends mem suffix b lb
    , lookup-extends mem suffix out lout
    , fa , fb , eq
  holds-mem-extends {mem} {suffix} (guard-disj out i)
    (ov , iv , lout , li , bit , disj) =
    ov , iv
    , lookup-extends mem suffix out lout
    , lookup-extends mem suffix i li
    , bit , disj
  holds-mem-extends {mem} {suffix} (bind entry widx)
    (wv , pv , lw , lpi , eq) =
    wv , pv , lookup-extends mem suffix widx lw , lpi , eq
  holds-mem-extends {mem} {suffix} (comm inputs outputs)
    (ivs , ovs , rv , pv , livs , lovs , crv , lpv , eq) =
    ivs , ovs , rv , pv
    , mem-lookups-extends mem suffix inputs livs
    , mem-lookups-extends mem suffix outputs lovs
    , crv , lpv , eq

  -- Satisfaction of a list of constraints is preserved under mem extension.
  satisfies-constraints-mem-extends : ∀ {mem suffix pis rand} (cls : List Constraint)
    → satisfies-constraints cls (mk-witness mem pis rand)
    → satisfies-constraints cls (mk-witness (mem ++ suffix) pis rand)
  satisfies-constraints-mem-extends _ = mapᴬ (λ {c} → holds-mem-extends c)

  -- Holds is preserved by extending the witness's pis with a suffix.
  -- Only `bind` and `comm` mention pis.
  holds-pis-extends : ∀ {mem pis suffix rand} (cl : Constraint)
    → holds (mk-witness mem pis rand) cl
    → holds (mk-witness mem (pis ++ suffix) rand) cl
  holds-pis-extends (gate _) h = h
  holds-pis-extends (non-zero c) h = h
  holds-pis-extends (select _ _ _ _) h = h
  holds-pis-extends (in-range _ _) h = h
  holds-pis-extends (boolean _) h = h
  holds-pis-extends (ec-add _ _ _ _ _ _) h = h
  holds-pis-extends (ec-mul _ _ _ _ _) h = h
  holds-pis-extends (ec-gen _ _ _) h = h
  holds-pis-extends (h2c _ _ _) h = h
  holds-pis-extends (div-mod _ _ _ _) h = h
  holds-pis-extends (reconstitute _ _ _ _) h = h
  holds-pis-extends (poseidon _ _) h = h
  holds-pis-extends (sha256 _ _ _ _) h = h
  holds-pis-extends (test-eq _ _ _) h = h
  holds-pis-extends (is-zero _ _) h = h
  holds-pis-extends (less-than _ _ _ _) h = h
  holds-pis-extends (guard-disj _ _) h = h
  holds-pis-extends {pis = pis} {suffix = suffix} (bind entry widx)
    (wv , pv , lw , lpi , eq) =
    wv , pv , lw , lookup-extends pis suffix entry lpi , eq
  holds-pis-extends {pis = pis} {suffix = suffix} (comm inputs outputs)
    (ivs , ovs , rv , pv , livs , lovs , crv , lpv , eq) =
    ivs , ovs , rv , pv
    , livs , lovs , crv
    , lookup-extends pis suffix 1 lpv
    , eq

  satisfies-constraints-pis-extends : ∀ {mem pis suffix rand} (cls : List Constraint)
    → satisfies-constraints cls (mk-witness mem pis rand)
    → satisfies-constraints cls (mk-witness mem (pis ++ suffix) rand)
  satisfies-constraints-pis-extends _ = mapᴬ (λ {c} → holds-pis-extends c)

  ------------------------------------------------------------------------
  -- Mem / pis SHRINK direction.
  --
  -- `holds-mem-shrink`/`-pis-shrink` are the duals of `-extends`.  Given
  -- a constraint whose referenced indices all fit within `length mem`
  -- (resp. `length pis`), and a `holds` at the extended witness, derive
  -- `holds` at the pre-extension witness.  Used by D2 (the per-list
  -- backward dispatcher) to peel off the satisfies-constraints for the
  -- head instruction from the satisfies-constraints over the *full*
  -- accumulated extension.
  ------------------------------------------------------------------------

  -- Predicate: every index referenced by `cl` is strictly less
  -- than `n`.  For constraints with PI references (pi-from-wire,
  -- comm-commitment), this is only about memory indices.
  constraint-mem-fits : Constraint → ℕ → Set
  constraint-mem-fits (gate e)                     n = expr-mem-fits e n
  constraint-mem-fits (non-zero c)               n = c < n
  constraint-mem-fits (select out b a c)           n =
    (out < n) × (b < n) × (a < n) × (c < n)
  constraint-mem-fits (in-range v _)                  n = v < n
  constraint-mem-fits (boolean v)                          n = v < n
  constraint-mem-fits (ec-add cx cy ax ay bx by)        n =
    (cx < n) × (cy < n) × (ax < n) × (ay < n) × (bx < n) × (by < n)
  constraint-mem-fits (ec-mul cx cy ax ay s)            n =
    (cx < n) × (cy < n) × (ax < n) × (ay < n) × (s < n)
  constraint-mem-fits (ec-gen cx cy s)        n =
    (cx < n) × (cy < n) × (s < n)
  constraint-mem-fits (h2c cx cy inputs)      n =
    (cx < n) × (cy < n) × All (_< n) inputs
  constraint-mem-fits (div-mod q r v _)                 n =
    (q < n) × (r < n) × (v < n)
  constraint-mem-fits (reconstitute out d m _)          n =
    (out < n) × (d < n) × (m < n)
  constraint-mem-fits (poseidon out inputs)       n =
    (out < n) × All (_< n) inputs
  constraint-mem-fits (sha256 h₁ h₂ _ inputs)  n =
    (h₁ < n) × (h₂ < n) × All (_< n) inputs
  constraint-mem-fits (test-eq out a b)                 n =
    (out < n) × (a < n) × (b < n)
  constraint-mem-fits (is-zero out a)                       n =
    (out < n) × (a < n)
  constraint-mem-fits (less-than out a b _)             n =
    (out < n) × (a < n) × (b < n)
  constraint-mem-fits (guard-disj out i)                n =
    (out < n) × (i < n)
  constraint-mem-fits (bind _ widx)             n = widx < n
  constraint-mem-fits (comm inputs outputs)  n =
    All (_< n) inputs × All (_< n) outputs

  -- The dual of `holds-mem-extends`.  All lookups in `cl` are at indices
  -- `< length mem` (encoded as `constraint-mem-fits cl (length mem)`),
  -- so they pull back from `mem ++ suffix` to `mem`.
  --
  -- Implementation note:  each case mirrors `holds-mem-extends`, except
  -- it calls `lookup-shrink` (not `lookup-extends`) and threads the
  -- bound premise extracted from `constraint-mem-fits cl (length mem)`.
  holds-mem-shrink : ∀ {pis rand} (mem suffix : List Fr) (cl : Constraint)
    → constraint-mem-fits cl (length mem)
    → holds (mk-witness (mem ++ suffix) pis rand) cl
    → holds (mk-witness mem pis rand) cl
  holds-mem-shrink mem suf (gate e) e< h = eval-mem-shrink mem suf e e< h
  holds-mem-shrink mem suf (non-zero c) c<
    (v , lv , v≢0) =
    v , lookup-shrink mem suf c lv c< , v≢0
  holds-mem-shrink mem suf (select out b a c) (out< , b< , a< , c<)
    (bv , av , cv , ov , lb , la , lc , lout , bit , eq) =
    bv , av , cv , ov
    , lookup-shrink mem suf b lb b<
    , lookup-shrink mem suf a la a<
    , lookup-shrink mem suf c lc c<
    , lookup-shrink mem suf out lout out<
    , bit , eq
  holds-mem-shrink mem suf (in-range v _) v<
    (vv , lv , fits-eq) =
    vv , lookup-shrink mem suf v lv v< , fits-eq
  holds-mem-shrink mem suf (boolean v) v<
    (vv , lv , bit) =
    vv , lookup-shrink mem suf v lv v< , bit
  holds-mem-shrink mem suf (ec-add cx cy ax ay bx by)
    (cx< , cy< , ax< , ay< , bx< , by<)
    (axv , ayv , bxv , byv , cxv , cyv ,
     lax , lay , lbx , lby , lcx , lcy , add-eq) =
    axv , ayv , bxv , byv , cxv , cyv
    , lookup-shrink mem suf ax lax ax<
    , lookup-shrink mem suf ay lay ay<
    , lookup-shrink mem suf bx lbx bx<
    , lookup-shrink mem suf by lby by<
    , lookup-shrink mem suf cx lcx cx<
    , lookup-shrink mem suf cy lcy cy<
    , add-eq
  holds-mem-shrink mem suf (ec-mul cx cy ax ay sc)
    (cx< , cy< , ax< , ay< , sc<)
    (axv , ayv , scv , cxv , cyv ,
     lax , lay , lsc , lcx , lcy , mul-eq) =
    axv , ayv , scv , cxv , cyv
    , lookup-shrink mem suf ax lax ax<
    , lookup-shrink mem suf ay lay ay<
    , lookup-shrink mem suf sc lsc sc<
    , lookup-shrink mem suf cx lcx cx<
    , lookup-shrink mem suf cy lcy cy<
    , mul-eq
  holds-mem-shrink mem suf (ec-gen cx cy sc) (cx< , cy< , sc<)
    (scv , cxv , cyv , lsc , lcx , lcy , gen-eq) =
    scv , cxv , cyv
    , lookup-shrink mem suf sc lsc sc<
    , lookup-shrink mem suf cx lcx cx<
    , lookup-shrink mem suf cy lcy cy<
    , gen-eq
  holds-mem-shrink mem suf (h2c cx cy inputs) (cx< , cy< , in<)
    (vs , cxv , cyv , lvs , lcx , lcy , hash-eq) =
    vs , cxv , cyv
    , mem-lookups-shrink mem suf inputs in< lvs
    , lookup-shrink mem suf cx lcx cx<
    , lookup-shrink mem suf cy lcy cy<
    , hash-eq
  holds-mem-shrink mem suf (div-mod q r v _) (q< , r< , v<)
    (qv , rv , vv , lq , lr , lv , fr , fq , nw , eq) =
    qv , rv , vv
    , lookup-shrink mem suf q lq q<
    , lookup-shrink mem suf r lr r<
    , lookup-shrink mem suf v lv v<
    , fr , fq , nw , eq
  holds-mem-shrink mem suf (reconstitute out d m _) (out< , d< , m<)
    (dv , mv , ov , ld , lm , lout , fd , fm , eq) =
    dv , mv , ov
    , lookup-shrink mem suf d ld d<
    , lookup-shrink mem suf m lm m<
    , lookup-shrink mem suf out lout out<
    , fd , fm , eq
  holds-mem-shrink mem suf (poseidon out inputs) (out< , in<)
    (vs , ov , lvs , lout , eq) =
    vs , ov
    , mem-lookups-shrink mem suf inputs in< lvs
    , lookup-shrink mem suf out lout out<
    , eq
  holds-mem-shrink mem suf (sha256 h₁ h₂ _ inputs) (h1< , h2< , in<)
    (vs , v1 , v2 , lvs , lh₁ , lh₂ , hash-eq) =
    vs , v1 , v2
    , mem-lookups-shrink mem suf inputs in< lvs
    , lookup-shrink mem suf h₁ lh₁ h1<
    , lookup-shrink mem suf h₂ lh₂ h2<
    , hash-eq
  holds-mem-shrink mem suf (test-eq out a b) (out< , a< , b<)
    (av , bv , ov , la , lb , lout , eq) =
    av , bv , ov
    , lookup-shrink mem suf a la a<
    , lookup-shrink mem suf b lb b<
    , lookup-shrink mem suf out lout out<
    , eq
  holds-mem-shrink mem suf (is-zero out a) (out< , a<)
    (av , ov , la , lout , eq) =
    av , ov
    , lookup-shrink mem suf a la a<
    , lookup-shrink mem suf out lout out<
    , eq
  holds-mem-shrink mem suf (less-than out a b _) (out< , a< , b<)
    (av , bv , ov , la , lb , lout , fa , fb , eq) =
    av , bv , ov
    , lookup-shrink mem suf a la a<
    , lookup-shrink mem suf b lb b<
    , lookup-shrink mem suf out lout out<
    , fa , fb , eq
  holds-mem-shrink mem suf (guard-disj out i) (out< , i<)
    (ov , iv , lout , li , bit , disj) =
    ov , iv
    , lookup-shrink mem suf out lout out<
    , lookup-shrink mem suf i li i<
    , bit , disj
  holds-mem-shrink mem suf (bind entry widx) widx<
    (wv , pv , lw , lpi , eq) =
    wv , pv
    , lookup-shrink mem suf widx lw widx<
    , lpi , eq
  holds-mem-shrink mem suf (comm inputs outputs) (in< , out<)
    (ivs , ovs , rv , pv , livs , lovs , crv , lpv , eq) =
    ivs , ovs , rv , pv
    , mem-lookups-shrink mem suf inputs in< livs
    , mem-lookups-shrink mem suf outputs out< lovs
    , crv , lpv , eq

  -- All constraints fit: pointwise predicate, AND'd over the list.
  constraints-mem-fit : List Constraint → ℕ → Set
  constraints-mem-fit []       _ = ⊤
  constraints-mem-fit (c ∷ cs) n = constraint-mem-fits c n × constraints-mem-fit cs n

  -- Satisfaction shrinks under suffix removal, given pointwise bounds.
  satisfies-constraints-mem-shrink : ∀ {pis rand}
    (cls : List Constraint) (mem suf : List Fr)
    → constraints-mem-fit cls (length mem)
    → satisfies-constraints cls (mk-witness (mem ++ suf) pis rand)
    → satisfies-constraints cls (mk-witness mem pis rand)
  satisfies-constraints-mem-shrink []       _   _   _    _            = []ᴬ
  satisfies-constraints-mem-shrink (c ∷ cs) mem suf (hd-fit , tl-fit) (hd-h ∷ᴬ tl-s) =
    holds-mem-shrink mem suf c hd-fit hd-h
    ∷ᴬ satisfies-constraints-mem-shrink cs mem suf tl-fit tl-s

  ------------------------------------------------------------------------
  -- Pis shrink direction.
  --
  -- Dual of `satisfies-constraints-mem-shrink`.  Only `bind`
  -- and `comm` mention pis; for all others the
  -- "fit" predicate is trivially `true` and the shrink is the identity.
  ------------------------------------------------------------------------

  -- Per-constraint "fits in pis of length n" predicate.  Only the two
  -- pis-referencing constraints are non-trivial; all others hold trivially.
  constraint-pis-fit : Constraint → ℕ → Set
  constraint-pis-fit (bind entry _)  n = entry < n
  constraint-pis-fit (comm _ _)   n = 1 < n
  constraint-pis-fit _                              _ = ⊤

  -- Shrink direction for `holds`:  if all pi-references in `cl` are
  -- < length pis, then `holds (mem, pis ++ suf, rand) cl` implies
  -- `holds (mem, pis, rand) cl`.
  holds-pis-shrink : ∀ {mem rand} (pis suf : List Fr) (cl : Constraint)
    → constraint-pis-fit cl (length pis)
    → holds (mk-witness mem (pis ++ suf) rand) cl
    → holds (mk-witness mem pis            rand) cl
  holds-pis-shrink _ _ (gate _) _ h = h
  holds-pis-shrink _ _ (non-zero _) _ h = h
  holds-pis-shrink _ _ (select _ _ _ _) _ h = h
  holds-pis-shrink _ _ (in-range _ _) _ h = h
  holds-pis-shrink _ _ (boolean _) _ h = h
  holds-pis-shrink _ _ (ec-add _ _ _ _ _ _) _ h = h
  holds-pis-shrink _ _ (ec-mul _ _ _ _ _) _ h = h
  holds-pis-shrink _ _ (ec-gen _ _ _) _ h = h
  holds-pis-shrink _ _ (h2c _ _ _) _ h = h
  holds-pis-shrink _ _ (div-mod _ _ _ _) _ h = h
  holds-pis-shrink _ _ (reconstitute _ _ _ _) _ h = h
  holds-pis-shrink _ _ (poseidon _ _) _ h = h
  holds-pis-shrink _ _ (sha256 _ _ _ _) _ h = h
  holds-pis-shrink _ _ (test-eq _ _ _) _ h = h
  holds-pis-shrink _ _ (is-zero _ _) _ h = h
  holds-pis-shrink _ _ (less-than _ _ _ _) _ h = h
  holds-pis-shrink _ _ (guard-disj _ _) _ h = h
  holds-pis-shrink pis suf (bind entry widx) fits
    (wv , pv , lw , lpi , eq) =
    wv , pv , lw
    , lookup-shrink pis suf entry lpi fits
    , eq
  holds-pis-shrink pis suf (comm inputs outputs) fits
    (ivs , ovs , rv , pv , livs , lovs , crv , lpv , eq) =
    ivs , ovs , rv , pv , livs , lovs , crv
    , lookup-shrink pis suf 1 lpv fits
    , eq

  -- All constraints fit (pis-side): pointwise predicate AND'd over the list.
  constraints-pis-fit : List Constraint → ℕ → Set
  constraints-pis-fit []       _ = ⊤
  constraints-pis-fit (c ∷ cs) n = constraint-pis-fit c n × constraints-pis-fit cs n

  -- List-level shrink for pis suffix.
  satisfies-constraints-pis-shrink : ∀ {mem rand}
    (cls : List Constraint) (pis suf : List Fr)
    → constraints-pis-fit cls (length pis)
    → satisfies-constraints cls (mk-witness mem (pis ++ suf) rand)
    → satisfies-constraints cls (mk-witness mem pis            rand)
  satisfies-constraints-pis-shrink []       _   _   _    _            = []ᴬ
  satisfies-constraints-pis-shrink (c ∷ cs) pis suf (hd-fit , tl-fit) (hd-h ∷ᴬ tl-s) =
    holds-pis-shrink pis suf c hd-fit hd-h
    ∷ᴬ satisfies-constraints-pis-shrink cs pis suf tl-fit tl-s

  -- Distributivity of satisfies-constraints over list concatenation.
  satisfies-constraints-++ : ∀ {w} (xs ys : List Constraint)
    → satisfies-constraints xs w
    → satisfies-constraints ys w
    → satisfies-constraints (xs ++ ys) w
  satisfies-constraints-++ _ _ = ++ᴬ

  -- Splitting direction of `satisfies-constraints-++`.  Used by the
  -- backward dispatcher D1 to peel off the prior constraints (which are
  -- satisfied at the post-state witness as a consequence of monotonicity)
  -- from the new constraints (which the dispatcher actually inverts).
  satisfies-constraints-split : ∀ {w} (xs ys : List Constraint)
    → satisfies-constraints (xs ++ ys) w
    → satisfies-constraints xs w × satisfies-constraints ys w
  satisfies-constraints-split xs _ = ++⁻ xs

  -- length-of-append for explicit nat arithmetic on mem-inv.
  length-++-1 : ∀ (xs : List Fr) y → length (xs ++ (y ∷ [])) ≡ suc (length xs)
  length-++-1 []       y = refl
  length-++-1 (x ∷ xs) y = cong suc (length-++-1 xs y)

  length-++-2 : ∀ (xs : List Fr) y z → length (xs ++ (y ∷ z ∷ [])) ≡ suc (suc (length xs))
  length-++-2 []       y z = refl
  length-++-2 (x ∷ xs) y z = cong suc (length-++-2 xs y z)

  -- Build the post-state mem-inv from the pre-state one for a Δmem = 1 instruction.
  mem-inv-step-1 : ∀ {st : SynthState} {mem : List Fr} {v : Fr}
    → SynthState.nr-wires st ≡ length mem
    → SynthState.nr-wires st + 1 ≡ length (mem ++ (v ∷ []))
  mem-inv-step-1 {st} {mem} {v} mi =
    trans (+-comm (SynthState.nr-wires st) 1)
          (trans (cong suc mi) (sym (length-++-1 mem v)))

  -- Δmem = 2 instruction (push-mem2 form: mem ++ (x ∷ y ∷ [])).
  mem-inv-step-2 : ∀ {st : SynthState} {mem : List Fr} {x y : Fr}
    → SynthState.nr-wires st ≡ length mem
    → SynthState.nr-wires st + 2 ≡ length (mem ++ (x ∷ y ∷ []))
  mem-inv-step-2 {st} {mem} {x} {y} mi =
    trans (+-comm (SynthState.nr-wires st) 2)
          (trans (cong (suc ∘ suc) mi) (sym (length-++-2 mem x y)))
    where open import Function using (_∘_)

  -- Δmem = 2 instruction (iterated push-mem form: (mem ++ (x ∷ [])) ++ (y ∷ [])).
  mem-inv-step-2' : ∀ {st : SynthState} {mem : List Fr} {x y : Fr}
    → SynthState.nr-wires st ≡ length mem
    → SynthState.nr-wires st + 2 ≡ length ((mem ++ (x ∷ [])) ++ (y ∷ []))
  mem-inv-step-2' {st} {mem} {x} {y} mi =
    trans (mem-inv-step-2 {st} {mem} {x} {y} mi)
          (cong length (push-mem2-assoc mem x y))

  -- Discharge pattern shared (hand-expanded) by the
  -- "memory-only-extends" dispatcher cases below.  An instruction `i`
  -- whose `R-instr` evidence yields a memory suffix `suf` and whose
  -- `circuit-instr hc i st` only appends new constraints (no
  -- `nr-declared-pi` or `output-wires` change) is discharged by:
  --
  --   1. Lift prior-sat to (mem ++ suf, pis, rand) via mem-extends.
  --   2. Apply the per-instruction forward lemma to get satisfaction
  --      of the *new* constraints at (mem ++ suf, pis, rand).
  --   3. Concatenate via `satisfies-constraints-++`.
  --
  -- The pattern's preconditions ensure
  -- `constraints (circuit-instr hc i st) = constraints st ++ newcls`, which
  -- holds by definition for all instructions except `pi-skip` and
  -- `output` (which emit no constraints) and `public-input nothing` and
  -- `private-input nothing` (which also emit no constraints but still
  -- bump `nr-wires`).

  -- The PI vector along an R-instr step extends pre-pis with a suffix.
  -- Only `declare-pub-input` extends the pis (with one cell); all others
  -- leave them unchanged.
  pis-extends-R-instr : ∀ {pre s i s'}
    → R-instr pre s i s'
    → Σ (List Fr) λ suf →
        Preprocessed.pis s' ≡ Preprocessed.pis s ++ suf
  pis-extends-R-instr (r-assert _)                   = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-cond-select _ _ _)          = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-constrain-bits _ _ _)         = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-constrain-eq _ _ _)         = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-constrain-to-boolean _)     = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-copy _)                     = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-declare-pub-input {v = v} _) = v ∷ [] , refl
  pis-extends-R-instr (r-pi-skip-active _ _)         = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-pi-skip-inactive _)         = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-ec-add _ _ _ _ _)           = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-ec-mul _ _ _ _)             = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-ec-mul-generator _ _)       = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-hash-to-curve _ _)          = [] , sym (++-identityʳ _)
  pis-extends-R-instr r-load-imm                     = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-div-mod-power-of-two _ _)     = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-reconstitute-field _ _ _ _ _)   = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-output _)                   = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-transient-hash _)           = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-persistent-hash _ _)        = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-test-eq _ _)                = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-add _ _)                    = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-mul _ _)                    = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-neg _)                      = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-not _)                      = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-less-than _ _ _ _)            = [] , sym (++-identityʳ _)
  pis-extends-R-instr (r-public-input-inactive _)    = [] , sym (++-identityʳ _)
  pis-extends-R-instr {s = s} (r-public-input-active {s₁ = s₁} _ cp) =
    [] , trans (consume-pub-out-pis s cp) (sym (++-identityʳ _))
  pis-extends-R-instr (r-private-input-inactive _)   = [] , sym (++-identityʳ _)
  pis-extends-R-instr {s = s} (r-private-input-active {s₁ = s₁} _ cp) =
    [] , trans (consume-priv-pis s cp) (sym (++-identityʳ _))

------------------------------------------------------------------------
-- Section C.  Forward direction.
--
-- Two layers:
--
--   • `R-instrs→satisfies-constraints`     instruction-list induction
--   • `circuit-faithful-fwd`           top-level (incl. comm-commitment)
------------------------------------------------------------------------

-- Sub-lemma 1: a single R-instr step preserves the satisfaction of any
-- prior constraint list, and the new constraints emitted by `circuit-instr` for
-- that step are satisfied by the post-state assignment.
--
-- This is essentially the conjunction of the 26 forward lemmas in
-- `CircuitFaithfulness.agda`, generalized to thread:
--
--   • the constraints accumulated by earlier instructions (they stay
--     satisfied because they only mention indices < length pre-mem, and
--     pre-mem is a prefix of post-mem);
--   • the invariants I-mem and I-pi (which the synthesis-state record
--     also obeys clause-by-clause).
--
-- The `prior-sat` hypothesis is what lets the per-step proof lift the
-- earlier-emitted constraints to the post-state's larger memory.  The
-- forward direction needs no producer-obligation hypotheses; those are
-- only required by the backward direction of the four §6.5 cases
-- (assert, not, reconstitute-field, less-than).

-- Concrete-state version of `single-instr-constraints-with-decl`: applies
-- the synthesis function to an arbitrary `st`, returning the *new*
-- constraints emitted (i.e. `constraints st`-suffix).  Definitionally equal to
-- `single-instr-constraints-with-decl hc (nr-wires st) (nr-declared-pi st) i`
-- (this is what `circuit-instr` does, up to the prior constraints prefix).
--
-- The actual proof discharges via direct case analysis on `i`.

-- Shared closing moves for the per-instruction cases, isolating the
-- otherwise copy-pasted `subst`/`++`/lift plumbing.  Each takes the
-- per-instruction `*-fwd` result (`sat-new`) already stated at
-- `single-instr-constraints hc (length (memory s)) i`, together with the
-- memory / pi invariants, and assembles the post-state triple.  The
-- output's constraint list is written as
-- `constraints st ++ single-instr-constraints hc (nr-wires st) i`, which
-- coincides definitionally with `constraints (circuit-instr hc i st)` once
-- `i` is a concrete instruction at the call site.
private

  -- Instruction that appends a memory suffix `suf` and no PI entries.
  fwd-mem-step
    : ∀ {hc} (pre : ProofPreimage) (s : Preprocessed) (i : Instruction)
        (st : SynthState) (suf : List Fr)
    → SynthState.nr-wires st ≡ length (Preprocessed.memory s)
    → SynthState.nr-wires (circuit-instr hc i st)
        ≡ length (Preprocessed.memory s ++ suf)
    → length (Preprocessed.pis s)
        ≡ preamble-pi-count hc + SynthState.nr-declared-pi (circuit-instr hc i st)
    → satisfies-constraints (SynthState.constraints st)
        (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) (comm-rand-of pre))
    → satisfies-constraints
        (single-instr-constraints hc (length (Preprocessed.memory s)) i)
        (mk-witness (Preprocessed.memory s ++ suf) (Preprocessed.pis s)
                    (comm-rand-of pre))
    →   (SynthState.nr-wires (circuit-instr hc i st)
           ≡ length (Preprocessed.memory s ++ suf))
      × (length (Preprocessed.pis s)
           ≡ preamble-pi-count hc
             + SynthState.nr-declared-pi (circuit-instr hc i st))
      × satisfies-constraints
          (SynthState.constraints st
             ++ single-instr-constraints hc (SynthState.nr-wires st) i)
          (mk-witness (Preprocessed.memory s ++ suf) (Preprocessed.pis s)
                      (comm-rand-of pre))
  fwd-mem-step {hc} pre s i st suf mi mi-res pi-res prior-sat sat-new =
      mi-res , pi-res ,
      satisfies-constraints-++ (SynthState.constraints st) _
        (satisfies-constraints-mem-extends (SynthState.constraints st) prior-sat)
        (subst (λ cls → satisfies-constraints cls
                  (mk-witness (Preprocessed.memory s ++ suf)
                              (Preprocessed.pis s) (comm-rand-of pre)))
               (sym (cong (λ n → single-instr-constraints hc n i) mi))
               sat-new)

  -- Instruction that leaves memory and pis unchanged.
  fwd-nogrow-step
    : ∀ {hc} (pre : ProofPreimage) (s : Preprocessed) (i : Instruction)
        (st : SynthState)
    → SynthState.nr-wires st ≡ length (Preprocessed.memory s)
    → satisfies-constraints (SynthState.constraints st)
        (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) (comm-rand-of pre))
    → satisfies-constraints
        (single-instr-constraints hc (length (Preprocessed.memory s)) i)
        (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) (comm-rand-of pre))
    → satisfies-constraints
        (SynthState.constraints st
           ++ single-instr-constraints hc (SynthState.nr-wires st) i)
        (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) (comm-rand-of pre))
  fwd-nogrow-step {hc} pre s i st mi prior-sat sat-new =
      satisfies-constraints-++ (SynthState.constraints st) _ prior-sat
        (subst (λ cls → satisfies-constraints cls
                  (mk-witness (Preprocessed.memory s) (Preprocessed.pis s)
                              (comm-rand-of pre)))
               (sym (cong (λ n → single-instr-constraints hc n i) mi))
               sat-new)

  -- Transcript-active input instruction: `s'` differs from a state that
  -- shares memory / pis with `s` only by appending the read value `v`.
  -- Rebuilds the invariants and lifts prior-sat across the two equations.
  input-active-frame
    : ∀ {hc} (pre : ProofPreimage) (s s' : Preprocessed) (st : SynthState)
        {v : Fr}
    → Preprocessed.memory s' ≡ Preprocessed.memory s ++ (v ∷ [])
    → Preprocessed.pis s' ≡ Preprocessed.pis s
    → SynthState.nr-wires st ≡ length (Preprocessed.memory s)
    → pi-inv hc s st
    → satisfies-constraints (SynthState.constraints st)
        (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) (comm-rand-of pre))
    →   (SynthState.nr-wires st + 1 ≡ length (Preprocessed.memory s'))
      × (length (Preprocessed.pis s')
           ≡ preamble-pi-count hc + SynthState.nr-declared-pi st)
      × satisfies-constraints (SynthState.constraints st)
          (mk-witness (Preprocessed.memory s') (Preprocessed.pis s')
                      (comm-rand-of pre))
  input-active-frame {hc} pre s s' st {v} mem-s' pis-s' mi pi prior-sat =
      subst (λ m → SynthState.nr-wires st + 1 ≡ length m) (sym mem-s')
            (mem-inv-step-1 {st} {Preprocessed.memory s} {v} mi)
    , subst (λ p → length p
              ≡ preamble-pi-count hc + SynthState.nr-declared-pi st)
            (sym pis-s') pi
    , subst (λ p → satisfies-constraints (SynthState.constraints st)
              (mk-witness (Preprocessed.memory s') p (comm-rand-of pre)))
            (sym pis-s')
            (subst (λ m → satisfies-constraints (SynthState.constraints st)
                     (mk-witness m (Preprocessed.pis s) (comm-rand-of pre)))
                   (sym mem-s')
                   (satisfies-constraints-mem-extends {suffix = v ∷ []}
                      (SynthState.constraints st) prior-sat))

R-instr→satisfies-step
  : ∀ {hc} (pre : ProofPreimage) (s s' : Preprocessed) (i : Instruction)
  → (st : SynthState)
  → mem-inv s st
  → pi-inv  hc s st
  → satisfies-constraints (SynthState.constraints st)
      (mk-witness (Preprocessed.memory s)
                  (Preprocessed.pis    s)
                  (comm-rand-of pre))
  → R-instr pre s i s'
  →   mem-inv s' (circuit-instr hc i st)
    × pi-inv  hc s' (circuit-instr hc i st)
    × satisfies-constraints
        (SynthState.constraints (circuit-instr hc i st))
        (mk-witness (Preprocessed.memory s')
                    (Preprocessed.pis    s')
                    (comm-rand-of pre))
-- For each case, we use the helpers built up above:
--   • `mem-extends-R-instr` to identify the suffix appended to memory;
--   • the corresponding `*-fwd` lemma from CircuitFaithfulness;
--   • `satisfies-constraints-mem-extends` / `-pis-extends` to lift prior-sat;
--   • `satisfies-constraints-++` to combine.

-- Pattern-matching helper: each case applies the appropriate forward
-- lemma to the new constraints and concatenates with the lifted prior-sat.

-- assert(c): mem and pis unchanged.  newcls = [non-zero c].
R-instr→satisfies-step {hc} pre s .s (assert c) st mi pi prior-sat r@(r-assert _) =
  mi , pi ,
  fwd-nogrow-step {hc} pre s (assert c) st mi prior-sat
    (assert-fwd {pre = pre} {s = s} {s' = s} {c = c} {hc = hc}
                {rand = comm-rand-of pre} r)

-- constrain-bits(v, n): mem and pis unchanged.
R-instr→satisfies-step {hc} pre s .s (constrain-bits v n) st mi pi prior-sat r@(r-constrain-bits _ _ _) =
  mi , pi ,
  fwd-nogrow-step {hc} pre s (constrain-bits v n) st mi prior-sat
    (constrain-bits-fwd {pre = pre} {s = s} {s' = s}
                        {v = v} {n = n} {hc = hc} {rand = comm-rand-of pre} r)

-- constrain-eq(a, b): mem and pis unchanged.
R-instr→satisfies-step {hc} pre s .s (constrain-eq a b) st mi pi prior-sat r@(r-constrain-eq _ _ _) =
  mi , pi ,
  fwd-nogrow-step {hc} pre s (constrain-eq a b) st mi prior-sat
    (constrain-eq-fwd {pre = pre} {s = s} {s' = s}
                      {a = a} {b = b} {hc = hc} {rand = comm-rand-of pre} r)

-- constrain-to-boolean(v): mem and pis unchanged.
R-instr→satisfies-step {hc} pre s .s (constrain-to-boolean v) st mi pi prior-sat r@(r-constrain-to-boolean _) =
  mi , pi ,
  fwd-nogrow-step {hc} pre s (constrain-to-boolean v) st mi prior-sat
    (constrain-to-boolean-fwd {pre = pre} {s = s} {s' = s}
                              {v = v} {hc = hc} {rand = comm-rand-of pre} r)

-- copy(v): Δmem = 1, pis unchanged.
R-instr→satisfies-step {hc} pre s s' (copy v) st mi pi prior-sat r@(r-copy {v = v0} _) =
  fwd-mem-step {hc} pre s (copy v) st (v0 ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {v0} mi) pi prior-sat
    (copy-fwd {pre = pre} {s = s} {s' = s'} {v = v} {hc = hc}
              {rand = comm-rand-of pre} r)

-- load-imm(imm): Δmem = 1, pis unchanged.
R-instr→satisfies-step {hc} pre s s' (load-imm imm) st mi pi prior-sat r@r-load-imm =
  fwd-mem-step {hc} pre s (load-imm imm) st (imm ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {imm} mi) pi prior-sat
    (load-imm-fwd {pre = pre} {s = s} {s' = s'} {k = imm} {hc = hc}
                  {rand = comm-rand-of pre} r)

-- add(a, b): Δmem = 1, pis unchanged.
R-instr→satisfies-step {hc} pre s s' (add a b) st mi pi prior-sat r@(r-add {av = av} {bv = bv} _ _) =
  fwd-mem-step {hc} pre s (add a b) st ((av +ᶠ bv) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {av +ᶠ bv} mi) pi prior-sat
    (add-fwd {pre = pre} {s = s} {s' = s'} {a = a} {b = b} {hc = hc}
             {rand = comm-rand-of pre} r)

-- mul(a, b)
R-instr→satisfies-step {hc} pre s s' (mul a b) st mi pi prior-sat r@(r-mul {av = av} {bv = bv} _ _) =
  fwd-mem-step {hc} pre s (mul a b) st ((av *ᶠ bv) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {av *ᶠ bv} mi) pi prior-sat
    (mul-fwd {pre = pre} {s = s} {s' = s'} {a = a} {b = b} {hc = hc}
             {rand = comm-rand-of pre} r)

-- neg(a)
R-instr→satisfies-step {hc} pre s s' (neg a) st mi pi prior-sat r@(r-neg {av = av} _) =
  fwd-mem-step {hc} pre s (neg a) st ((-ᶠ av) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} { -ᶠ av } mi) pi prior-sat
    (neg-fwd {pre = pre} {s = s} {s' = s'} {a = a} {hc = hc}
             {rand = comm-rand-of pre} r)

-- test-eq(a, b)
R-instr→satisfies-step {hc} pre s s' (test-eq a b) st mi pi prior-sat r@(r-test-eq {av = av} {bv = bv} _ _) =
  fwd-mem-step {hc} pre s (test-eq a b) st (from-bool (av ≡ᶠ? bv) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {from-bool (av ≡ᶠ? bv)} mi)
    pi prior-sat
    (test-eq-fwd {pre = pre} {s = s} {s' = s'} {a = a} {b = b} {hc = hc}
                 {rand = comm-rand-of pre} r)

-- not(a)
R-instr→satisfies-step {hc} pre s s' (not a) st mi pi prior-sat r@(r-not {b = b0} _) =
  fwd-mem-step {hc} pre s (not a) st (from-bool (Bool.not b0) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {from-bool (Bool.not b0)} mi)
    pi prior-sat
    (not-fwd {pre = pre} {s = s} {s' = s'} {a = a} {hc = hc}
             {rand = comm-rand-of pre} r)

-- cond-select(b, a, c): Δmem = 1.  Output value is `if sel then av else bv`.
R-instr→satisfies-step {hc} pre s s' (cond-select b a c) st mi pi prior-sat
  r@(r-cond-select {sel = sel} {av = av} {bv = bv} _ _ _) =
  fwd-mem-step {hc} pre s (cond-select b a c) st ((if sel then av else bv) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {if sel then av else bv} mi)
    pi prior-sat
    (cond-select-fwd {pre = pre} {s = s} {s' = s'} {b = b} {a = a} {c = c}
                     {hc = hc} {rand = comm-rand-of pre} r)

-- less-than(a, b, n): Δmem = 1.  Output value: `from-bool (bits-lt ...)`.
R-instr→satisfies-step {hc} pre s s' (less-than a b bits) st mi pi prior-sat
  r@(r-less-than {av = av} {bv = bv} _ _ _ _) =
  fwd-mem-step {hc} pre s (less-than a b bits) st
    (from-bool (bits-lt (take bits (to-le-bits av)) (take bits (to-le-bits bv))) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s}
         {from-bool (bits-lt (take bits (to-le-bits av)) (take bits (to-le-bits bv)))} mi)
    pi prior-sat
    (less-than-fwd {pre = pre} {s = s} {s' = s'} {a = a} {b = b} {bits = bits}
                   {hc = hc} {rand = comm-rand-of pre} r)

-- transient-hash(inputs): Δmem = 1.  Output value: transient-hash-fn vs.
R-instr→satisfies-step {hc} pre s s' (transient-hash inputs) st mi pi prior-sat
  r@(r-transient-hash {vs = vs} _) =
  fwd-mem-step {hc} pre s (transient-hash inputs) st (transient-hash-fn vs ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {transient-hash-fn vs} mi)
    pi prior-sat
    (transient-hash-fwd {pre = pre} {s = s} {s' = s'} {inputs = inputs}
                        {hc = hc} {rand = comm-rand-of pre} r)

-- reconstitute-field(d, m, bits): Δmem = 1.
R-instr→satisfies-step {hc} pre s s' (reconstitute-field d m bits) st mi pi prior-sat
  r@(r-reconstitute-field {dv = dv} {mv = mv} _ _ _ _ _) =
  fwd-mem-step {hc} pre s (reconstitute-field d m bits) st
    (from-le-bits (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv)) ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s}
         {from-le-bits (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))} mi)
    pi prior-sat
    (reconstitute-field-fwd {pre = pre} {s = s} {s' = s'} {d = d} {m = m} {bits = bits}
                            {hc = hc} {rand = comm-rand-of pre} r)

-- ec-add: Δmem = 2 (push-mem2 form, mem ++ x ∷ y ∷ []).
R-instr→satisfies-step {hc} pre s s' (ec-add a-x a-y b-x b-y) st mi pi prior-sat
  r@(r-ec-add {cx = cx} {cy = cy} _ _ _ _ _) =
  fwd-mem-step {hc} pre s (ec-add a-x a-y b-x b-y) st (cx ∷ cy ∷ [])
    mi (mem-inv-step-2 {st} {Preprocessed.memory s} {cx} {cy} mi) pi prior-sat
    (ec-add-fwd {pre = pre} {s = s} {s' = s'} {a-x = a-x} {a-y = a-y}
                {b-x = b-x} {b-y = b-y} {hc = hc} {rand = comm-rand-of pre} r)

-- ec-mul
R-instr→satisfies-step {hc} pre s s' (ec-mul a-x a-y scalar) st mi pi prior-sat
  r@(r-ec-mul {cx = cx} {cy = cy} _ _ _ _) =
  fwd-mem-step {hc} pre s (ec-mul a-x a-y scalar) st (cx ∷ cy ∷ [])
    mi (mem-inv-step-2 {st} {Preprocessed.memory s} {cx} {cy} mi) pi prior-sat
    (ec-mul-fwd {pre = pre} {s = s} {s' = s'} {a-x = a-x} {a-y = a-y}
                {scalar = scalar} {hc = hc} {rand = comm-rand-of pre} r)

-- ec-mul-generator
R-instr→satisfies-step {hc} pre s s' (ec-mul-generator scalar) st mi pi prior-sat
  r@(r-ec-mul-generator {cx = cx} {cy = cy} _ _) =
  fwd-mem-step {hc} pre s (ec-mul-generator scalar) st (cx ∷ cy ∷ [])
    mi (mem-inv-step-2 {st} {Preprocessed.memory s} {cx} {cy} mi) pi prior-sat
    (ec-mul-generator-fwd {pre = pre} {s = s} {s' = s'} {scalar = scalar}
                          {hc = hc} {rand = comm-rand-of pre} r)

-- hash-to-curve
R-instr→satisfies-step {hc} pre s s' (hash-to-curve inputs) st mi pi prior-sat
  r@(r-hash-to-curve {cx = cx} {cy = cy} _ _) =
  fwd-mem-step {hc} pre s (hash-to-curve inputs) st (cx ∷ cy ∷ [])
    mi (mem-inv-step-2 {st} {Preprocessed.memory s} {cx} {cy} mi) pi prior-sat
    (hash-to-curve-fwd {pre = pre} {s = s} {s' = s'} {inputs = inputs}
                       {hc = hc} {rand = comm-rand-of pre} r)

-- persistent-hash
R-instr→satisfies-step {hc} pre s s' (persistent-hash alignment inputs) st mi pi prior-sat
  r@(r-persistent-hash {h₁ = h₁} {h₂ = h₂} _ _) =
  fwd-mem-step {hc} pre s (persistent-hash alignment inputs) st (h₁ ∷ h₂ ∷ [])
    mi (mem-inv-step-2 {st} {Preprocessed.memory s} {h₁} {h₂} mi) pi prior-sat
    (persistent-hash-fwd {pre = pre} {s = s} {s' = s'} {α = alignment}
                         {inputs = inputs} {hc = hc} {rand = comm-rand-of pre} r)

-- div-mod-power-of-two: Δmem = 2, iterated push-mem form.  The
-- post-state's memory is (mem ++ (divisor ∷ [])) ++ (modulus ∷ []) per
-- `r-div-mod-power-of-two`.  The forward lemma matches that shape.
R-instr→satisfies-step {hc} pre s s' (div-mod-power-of-two var bits) st mi pi prior-sat
  r@(r-div-mod-power-of-two {v = vv} _ _) =
  let mem  = Preprocessed.memory s ; pis = Preprocessed.pis s
      rand = comm-rand-of pre
      divisor = from-le-bits (drop bits (to-le-bits vv))
      modulus = from-le-bits (take bits (to-le-bits vv))
      mem' = (mem ++ (divisor ∷ [])) ++ (modulus ∷ [])
      w'   = mk-witness mem' pis rand
      newcls-eq : single-instr-constraints hc (SynthState.nr-wires st) (div-mod-power-of-two var bits)
                 ≡ single-instr-constraints hc (length mem) (div-mod-power-of-two var bits)
      newcls-eq = cong (λ k → single-instr-constraints hc k (div-mod-power-of-two var bits)) mi
      sat-new = subst (λ cls → satisfies-constraints cls w') (sym newcls-eq)
                       (div-mod-power-of-two-fwd {pre = pre} {s = s} {s' = s'}
                                                  {var = var} {bits = bits}
                                                  {hc = hc} {rand = rand} r)
      lifted-prior = satisfies-constraints-mem-extends
                       {suffix = modulus ∷ []}
                       (SynthState.constraints st)
                       (satisfies-constraints-mem-extends
                          {suffix = divisor ∷ []}
                          (SynthState.constraints st) prior-sat)
  in mem-inv-step-2' {st} {mem} {divisor} {modulus} mi , pi ,
     satisfies-constraints-++ (SynthState.constraints st) _ lifted-prior sat-new

-- output(v): no constraints.  push-output appends to s.outputs but leaves
-- memory and pis unchanged.  Only `output-wires` changes in the synth
-- state — irrelevant to `constraints`/`nr-wires`/`nr-declared-pi`.
R-instr→satisfies-step {hc} pre s s' (output v) st mi pi prior-sat r@(r-output _) =
  -- s' = push-output s _, whose memory ≡ memory s and pis ≡ pis s
  -- (push-output only modifies the `outputs` field).
  -- circuit-instr _ (output v) st = record st { output-wires = … },
  -- so its constraints ≡ st.constraints, nr-wires ≡ st.nr-wires, nr-declared-pi ≡ st.nr-declared-pi.
  mi , pi , prior-sat

-- pi-skip(guard, count): no constraints, no Δmem.  But the operational rule
-- does change the synth state's record (pi-skips, possibly pub-in-idx).
-- All effects on `Preprocessed` happen via `push-skip` which doesn't
-- change `memory` or `pis`.  The synth state is left entirely untouched
-- by `circuit-instr _ (pi-skip _ _) st = st`.
R-instr→satisfies-step {hc} pre s s' (pi-skip g n) st mi pi prior-sat
  (r-pi-skip-active _ _) =
  -- Post-state: push-skip s nothing.  push-skip leaves memory/pis unchanged.
  -- nr-wires st unchanged ≡ length (push-skip-memory) = length mem.
  mi , pi , prior-sat
R-instr→satisfies-step {hc} pre s s' (pi-skip g n) st mi pi prior-sat
  (r-pi-skip-inactive _) =
  mi , pi , prior-sat

-- public-input nothing: Δmem = 1, no constraints.  Fires r-public-input-active
-- since `eval-guard _ nothing ≡ just true` by definition.
R-instr→satisfies-step {hc} pre s s' (public-input nothing) st mi pi prior-sat
  (r-public-input-active {v = v} {s₁ = s₁} _ cp) =
  input-active-frame {hc} pre s s' st {v}
    (cong (_++ (v ∷ [])) (consume-pub-out-mem s cp))
    (consume-pub-out-pis s cp) mi pi prior-sat

-- private-input nothing: identical pattern to public-input nothing.
R-instr→satisfies-step {hc} pre s s' (private-input nothing) st mi pi prior-sat
  (r-private-input-active {v = v} {s₁ = s₁} _ cp) =
  input-active-frame {hc} pre s s' st {v}
    (cong (_++ (v ∷ [])) (consume-priv-mem s cp))
    (consume-priv-pis s cp) mi pi prior-sat

-- public-input (just g) — inactive: Δmem = 1, push-mem s 0ᶠ.
R-instr→satisfies-step {hc} pre s s' (public-input (just g)) st mi pi prior-sat
  r@(r-public-input-inactive _) =
  fwd-mem-step {hc} pre s (public-input (just g)) st (0ᶠ ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {0ᶠ} mi) pi prior-sat
    (public-input-just-fwd {pre = pre} {s = s} {s' = s'} {g = g} {hc = hc}
                           {rand = comm-rand-of pre} r)

-- public-input (just g) — active: Δmem = 1, push-mem s₁ v where s₁ shares mem/pis with s.
R-instr→satisfies-step {hc} pre s s' (public-input (just g)) st mi pi prior-sat
  r@(r-public-input-active {v = v} {s₁ = s₁} _ cp) =
  let mem-s' = cong (_++ (v ∷ [])) (consume-pub-out-mem s cp)
      pis-s' = consume-pub-out-pis s cp
      (mi' , pi' , lifted-prior) =
        input-active-frame {hc} pre s s' st {v} mem-s' pis-s' mi pi prior-sat
      sat-new-st =
        subst (λ cls → satisfies-constraints cls
                (mk-witness (Preprocessed.memory s') (Preprocessed.pis s')
                            (comm-rand-of pre)))
              (sym (cong (λ k → single-instr-constraints hc k (public-input (just g))) mi))
              (public-input-just-fwd {pre = pre} {s = s} {s' = s'} {g = g}
                                     {hc = hc} {rand = comm-rand-of pre} r)
  in mi' , pi' ,
     satisfies-constraints-++ (SynthState.constraints st) _ lifted-prior sat-new-st

-- private-input (just g) — inactive
R-instr→satisfies-step {hc} pre s s' (private-input (just g)) st mi pi prior-sat
  r@(r-private-input-inactive _) =
  fwd-mem-step {hc} pre s (private-input (just g)) st (0ᶠ ∷ [])
    mi (mem-inv-step-1 {st} {Preprocessed.memory s} {0ᶠ} mi) pi prior-sat
    (private-input-just-fwd {pre = pre} {s = s} {s' = s'} {g = g} {hc = hc}
                            {rand = comm-rand-of pre} r)

-- private-input (just g) — active
R-instr→satisfies-step {hc} pre s s' (private-input (just g)) st mi pi prior-sat
  r@(r-private-input-active {v = v} {s₁ = s₁} _ cp) =
  let mem-s' = cong (_++ (v ∷ [])) (consume-priv-mem s cp)
      pis-s' = consume-priv-pis s cp
      (mi' , pi' , lifted-prior) =
        input-active-frame {hc} pre s s' st {v} mem-s' pis-s' mi pi prior-sat
      sat-new-st =
        subst (λ cls → satisfies-constraints cls
                (mk-witness (Preprocessed.memory s') (Preprocessed.pis s')
                            (comm-rand-of pre)))
              (sym (cong (λ k → single-instr-constraints hc k (private-input (just g))) mi))
              (private-input-just-fwd {pre = pre} {s = s} {s' = s'} {g = g}
                                      {hc = hc} {rand = comm-rand-of pre} r)
  in mi' , pi' ,
     satisfies-constraints-++ (SynthState.constraints st) _ lifted-prior sat-new-st

-- declare-pub-input(v): pis grows by 1 cell with the value of wire v.
-- Mem unchanged.  Uses `single-instr-constraints-with-decl`.  nr-declared-pi
-- in synth state increments by 1.
R-instr→satisfies-step {hc} pre s s' (declare-pub-input v) st mi pi prior-sat
  r@(r-declare-pub-input {v = wv} _) =
  -- s' = push-pi s wv, whose memory ≡ memory s, pis ≡ pis s ++ (wv ∷ []).
  let mem  = Preprocessed.memory s ; pis-s = Preprocessed.pis s
      rand = comm-rand-of pre
      pis' = pis-s ++ (wv ∷ [])
      w'   = mk-witness mem pis' rand
      newcls-st = single-instr-constraints-with-decl hc (SynthState.nr-wires st)
                    (SynthState.nr-declared-pi st) (declare-pub-input v)
      newcls'   = single-instr-constraints-with-decl hc (length mem)
                    (SynthState.nr-declared-pi st) (declare-pub-input v)
      newcls-eq : newcls-st ≡ newcls'
      newcls-eq = cong (λ k → single-instr-constraints-with-decl hc k
                                  (SynthState.nr-declared-pi st) (declare-pub-input v)) mi
      sat-new' = declare-pub-input-fwd {pre = pre} {s = s} {s' = s'} {v = v} {hc = hc}
                                        {d = SynthState.nr-declared-pi st} {rand = rand} pi r
      sat-new = subst (λ cls → satisfies-constraints cls w') (sym newcls-eq) sat-new'
      lifted-prior = satisfies-constraints-pis-extends {suffix = wv ∷ []}
                       (SynthState.constraints st) prior-sat
      -- mem unchanged so mem-inv straightforward.
      mi' : mem-inv s' (circuit-instr hc (declare-pub-input v) st)
      mi' = mi  -- nr-wires unchanged, mem unchanged
      -- pis grows by 1; nr-declared-pi grows by 1.
      -- length (pis ++ wv ∷ []) = suc (length pis)
      -- = suc (preamble-pi-count hc + nr-declared-pi st)
      -- = preamble-pi-count hc + suc (nr-declared-pi st)
      pi' : length (pis-s ++ (wv ∷ [])) ≡ preamble-pi-count hc + suc (SynthState.nr-declared-pi st)
      pi' = trans (length-++-1 pis-s wv)
                  (trans (cong suc pi) (sym (+-suc (preamble-pi-count hc) (SynthState.nr-declared-pi st))))
  in mi' , pi' ,
     satisfies-constraints-++ (SynthState.constraints st) _ lifted-prior sat-new

-- Sub-lemma 2: iteration of the per-step lemma along an `R-instrs` trace.
-- Yields satisfaction of *all* constraints accumulated by `circuit-instrs`
-- against the final state's assignment.  Straightforward induction on
-- the `R-instrs` derivation tree, calling `R-instr→satisfies-step` at
-- each `r-step`.
R-instrs→satisfies-constraints
  : ∀ {hc} (pre : ProofPreimage) (s₀ s : Preprocessed)
    (is : List Instruction) (st₀ : SynthState)
  → mem-inv s₀ st₀
  → pi-inv  hc s₀ st₀
  → satisfies-constraints (SynthState.constraints st₀)
      (mk-witness (Preprocessed.memory s₀)
                  (Preprocessed.pis    s₀)
                  (comm-rand-of pre))
  → R-instrs pre s₀ is s
  →   mem-inv s (circuit-instrs hc is st₀)
    × pi-inv  hc s (circuit-instrs hc is st₀)
    × satisfies-constraints
        (SynthState.constraints (circuit-instrs hc is st₀))
        (mk-witness (Preprocessed.memory s)
                    (Preprocessed.pis    s)
                    (comm-rand-of pre))
R-instrs→satisfies-constraints pre s₀ .s₀ [] st₀ mi pi sat r-done =
  mi , pi , sat
R-instrs→satisfies-constraints {hc} pre s₀ s (i ∷ is) st₀ mi pi sat (r-step {s₁ = s₁} r-head r-tail) =
  let mi₁ , pi₁ , sat₁ = R-instr→satisfies-step {hc = hc} pre s₀ s₁ i st₀ mi pi sat r-head
  in R-instrs→satisfies-constraints {hc = hc} pre s₁ s is
       (circuit-instr hc i st₀) mi₁ pi₁ sat₁ r-tail

------------------------------------------------------------------------
-- Top-level comm-commitment alignment.
--
-- If `do-communications-commitment src ≡ true`, then `R src pre s`
-- carries `T (comm-ok src pre s)`, i.e. the *operational*
-- commitment satisfies
--
--   pre.comm-commitment = just (c , r)
--   c ≡ transient-commit (pre.inputs ++ s.outputs) r
--
-- and `init-state` puts `c` at index 1 of `pis`.  The circuit's
-- `comm cm-inputs out-wires` requires
--
--   pis[1] ≡ transient-commit (ivs ++ ovs) rv
--
-- where `ivs = lookup mem [0..num-inputs)` and
-- `ovs = lookup mem out-wires`.  The match rests on three facts:
--
--   • `out-wires` (the indices recorded by `circuit-instr (output v)`)
--     evaluate via `mem-lookups` to exactly `Preprocessed.outputs s`;
--
--   • the wires `[0 .. num-inputs)` evaluate to `pre.inputs`
--     (consequence of `init-state-memory` + structure of `mem`);
--
--   • `transient-commit` and the in-circuit Poseidon are definitionally
--     the same canonical function (spec §5.3 trust boundary; already
--     baked in by `holds` using `transient-commit` directly).
--
-- The first two are the auxiliary lemmas below; the third holds
-- definitionally.
------------------------------------------------------------------------

-- Memory monotonicity for `mem-lookups` along R-instrs.  Discharged by
-- induction; each step's memory is a suffix extension of the prior.
mem-lookups-mono-R-instrs
  : ∀ (pre : ProofPreimage) (s s' : Preprocessed)
    (is : List Instruction) (xs : List Index) (vs : List Fr)
  → R-instrs pre s is s'
  → mem-lookups (Preprocessed.memory s)  xs ≡ just vs
  → mem-lookups (Preprocessed.memory s') xs ≡ just vs
mem-lookups-mono-R-instrs pre s .s [] xs vs r-done lookup-eq = lookup-eq
mem-lookups-mono-R-instrs pre s s' (i ∷ is) xs vs (r-step {s₁ = s₁} r-head r-tail) lookup-eq =
  let suf , eq = mem-extends-R-instr r-head   -- eq : mem s₁ ≡ mem s ++ suf
      lookup-s₁ : mem-lookups (Preprocessed.memory s₁) xs ≡ just vs
      lookup-s₁ = subst (λ m → mem-lookups m xs ≡ just vs)
                         (sym eq)
                         (mem-lookups-extends (Preprocessed.memory s) suf xs lookup-eq)
  in mem-lookups-mono-R-instrs pre s₁ s' is xs vs r-tail lookup-s₁

-- pi-lookup monotonicity along R-instrs.  Each step's `pis` is either
-- unchanged or extended (only `r-declare-pub-input` extends it).
pi-lookup-mono-R-instrs
  : ∀ (pre : ProofPreimage) (s s' : Preprocessed)
    (is : List Instruction) (idx : ℕ) (v : Fr)
  → R-instrs pre s is s'
  → pi-lookup (Preprocessed.pis s)  idx ≡ just v
  → pi-lookup (Preprocessed.pis s') idx ≡ just v
pi-lookup-mono-R-instrs pre s .s [] idx v r-done lk = lk
pi-lookup-mono-R-instrs pre s s' (i ∷ is) idx v
  (r-step {s₁ = s₁} r-head r-tail) lk =
  let suf , eq = pis-extends-R-instr r-head   -- eq : pis s₁ ≡ pis s ++ suf
      lk-s₁ : pi-lookup (Preprocessed.pis s₁) idx ≡ just v
      lk-s₁ = subst (λ p → pi-lookup p idx ≡ just v)
                     (sym eq)
                     (lookup-extends (Preprocessed.pis s) suf idx lk)
  in pi-lookup-mono-R-instrs pre s₁ s' is idx v r-tail lk-s₁

------------------------------------------------------------------------
-- Section C.2.  Helpers for the top-level forward proof.
--
-- These bridge the per-step iteration result (`R-instrs→satisfies-constraints`)
-- with the top-level comm-commitment constraint and the initial invariants.
------------------------------------------------------------------------

private

  -- mem-lookups distributes over snoc on the index list.
  mem-lookups-snoc : ∀ (mem : List Fr) (is : List Index) (i : Index) {vs v}
    → mem-lookups mem is ≡ just vs
    → mem-lookup mem i ≡ just v
    → mem-lookups mem (is ∷ʳ i) ≡ just (vs ∷ʳ v)
  mem-lookups-snoc mem [] i {vs} {v} lk lkv
    rewrite just-injective (sym lk) | lkv = refl
  mem-lookups-snoc mem (j ∷ js) i {vs} {v} lk lkv =
    aux (mem-lookup mem j)    refl
        (mem-lookups mem js)  refl
        lk
    where
      aux : ∀ (m : Maybe Fr) → mem-lookup mem j ≡ m
          → (ms : Maybe (List Fr)) → mem-lookups mem js ≡ ms
          → (m >>= λ v' → ms >>= λ vs' → just (v' ∷ vs')) ≡ just vs
          → mem-lookups mem ((j ∷ js) ∷ʳ i) ≡ just (vs ∷ʳ v)
      aux nothing   _    _          _    ()
      aux (just _)  _    nothing    _    ()
      aux (just w)  m-eq (just ws)  ms-eq refl
        rewrite m-eq | ms-eq
              | mem-lookups-snoc mem js i {ws} {v} ms-eq lkv
        = refl

  -- length-respecting decomposition of a non-empty list into snoc form.
  -- Used to enable snoc-induction over `nat-range`.
  snoc-of-length : ∀ (xs : List Fr) (n : ℕ)
    → length xs ≡ suc n
    → Σ (List Fr) (λ xs' → Σ Fr (λ x →
        (length xs' ≡ n) × (xs ≡ xs' ∷ʳ x)))
  snoc-of-length []           n  ()
  snoc-of-length (x ∷ [])     zero    refl =
    [] , x , refl , refl
  snoc-of-length (x ∷ [])     (suc _) ()
  snoc-of-length (x ∷ y ∷ ys) zero    ()
  snoc-of-length (x ∷ y ∷ ys) (suc n) p =
    let xs' , z , q , eq = snoc-of-length (y ∷ ys) n (Data.Nat.Properties.suc-injective p)
                           -- eq : y ∷ ys ≡ xs' ∷ʳ z
    in x ∷ xs' , z , cong suc q , cong (x ∷_) eq

  -- mem-lookup at exactly `length xs` in `xs ∷ʳ y` is `just y`.
  -- Used inductively to feed `mem-lookups-snoc`.
  mem-lookup-snoc-at-len : ∀ (xs : List Fr) (y : Fr)
    → mem-lookup (xs ∷ʳ y) (length xs) ≡ just y
  mem-lookup-snoc-at-len []       y = refl
  mem-lookup-snoc-at-len (x ∷ xs) y = mem-lookup-snoc-at-len xs y

  -- The wires `[0 .. length xs)` of `xs` look up to exactly `xs`.
  -- Proved by induction on `length xs`, decomposed via `snoc-of-length`.
  mem-lookups-nat-range-len : ∀ (n : ℕ) (xs : List Fr)
    → length xs ≡ n
    → mem-lookups xs (nat-range n) ≡ just xs
  mem-lookups-nat-range-len zero []       refl = refl
  mem-lookups-nat-range-len zero (_ ∷ _)  ()
  mem-lookups-nat-range-len (suc n) xs    p
    with snoc-of-length xs n p
  ... | xs' , y , len' , refl
    -- Goal: mem-lookups (xs' ∷ʳ y) (nat-range n ∷ʳ n) ≡ just (xs' ∷ʳ y)
    -- Use mem-lookups-nat-range-len n xs' len' (terminating: n < suc n).
    = mem-lookups-snoc (xs' ∷ʳ y) (nat-range n) n
        {vs = xs'} {v = y}
        (mem-lookups-extends xs' (y ∷ []) (nat-range n)
           {vs = xs'}
           (mem-lookups-nat-range-len n xs' len'))
        (subst (λ k → mem-lookup (xs' ∷ʳ y) k ≡ just y) len'
               (mem-lookup-snoc-at-len xs' y))

  -- Specialised: when `length xs ≡ n`, the wires `[0 .. n)` look up to `xs`.
  mem-lookups-nat-range : ∀ (xs : List Fr)
    → mem-lookups xs (nat-range (length xs)) ≡ just xs
  mem-lookups-nat-range xs = mem-lookups-nat-range-len (length xs) xs refl

  -- Initial `pis` length: `1` (hc = false, `[binding-input]`) or `2`
  -- (hc = true, `[binding-input, c]`).  Read off `InitInv.pis-shape`.
  init-state-pis-length : ∀ src pre s₀
    → init-state src pre ≡ just s₀
    → length (Preprocessed.pis s₀)
       ≡ preamble-pi-count (IrSource.do-communications-commitment src)
  init-state-pis-length src pre s₀ eq
    with InitInv.pis-shape (init-state-inv src pre eq)
  ... | inj₁ (hc≡ , pis≡) =
        trans (cong length pis≡) (sym (cong preamble-pi-count hc≡))
  ... | inj₂ (_ , _ , hc≡ , _ , pis≡) =
        trans (cong length pis≡) (sym (cong preamble-pi-count hc≡))

  -- For each R-instr step, the outputs grow by 0 (for non-output instructions)
  -- or by 1 (for output instructions).  We need a uniform lift of the
  -- IH from `s` to `s₁` when output-wires (and outputs) don't change.

  -- Specialised step lemma for the "non-output" cases.  The caller
  -- discharges `out-eq` (definitional for all but r-output) and uses the
  -- fact that, for non-output instructions, `circuit-instr` doesn't
  -- modify `output-wires` (so `output-wires (circuit-instr ...) ≡
  -- output-wires st` reduces definitionally to `refl`).
  --
  -- Both `i` and `st` are explicit so reduction of `circuit-instr` at the
  -- call site triggers normalisation of `output-wires (circuit-instr …)`.
  output-wires-non-output-step
    : ∀ {pre s s₁} (i : Instruction) (st : SynthState)
    → R-instr pre s i s₁
    → Preprocessed.outputs s₁ ≡ Preprocessed.outputs s
    → mem-lookups (Preprocessed.memory s) (SynthState.output-wires st)
        ≡ just (Preprocessed.outputs s)
    → mem-lookups (Preprocessed.memory s₁) (SynthState.output-wires st)
      ≡ just (Preprocessed.outputs s₁)
  output-wires-non-output-step {pre} {s} {s₁} i st r-head out-eq H =
    let suf , mem-eq = mem-extends-R-instr r-head
                       -- mem-eq : memory s₁ ≡ memory s ++ suf
    in subst (λ m → mem-lookups m (SynthState.output-wires st)
                      ≡ just (Preprocessed.outputs s₁))
              (sym mem-eq)
              (subst (λ ov → mem-lookups (Preprocessed.memory s ++ suf)
                               (SynthState.output-wires st)
                               ≡ just ov)
                      (sym out-eq)
                      (mem-lookups-extends (Preprocessed.memory s) suf
                         (SynthState.output-wires st) H))

-- The generalized version of `output-wires-coincide`, allowing arbitrary
-- starting `output-wires st₀`.
output-wires-coincide-gen
  : ∀ {hc} (pre : ProofPreimage) (s₀ s : Preprocessed)
    (is : List Instruction) (st₀ : SynthState)
  → R-instrs pre s₀ is s
  → mem-lookups (Preprocessed.memory s₀) (SynthState.output-wires st₀)
      ≡ just (Preprocessed.outputs s₀)
  → mem-lookups (Preprocessed.memory s)
      (SynthState.output-wires (circuit-instrs hc is st₀))
    ≡ just (Preprocessed.outputs s)
output-wires-coincide-gen pre s₀ .s₀ [] st₀ r-done H = H
output-wires-coincide-gen {hc} pre s₀ s (i ∷ is) st₀
  (r-step {s₁ = s₁} r-head r-tail) H =
  output-wires-coincide-gen {hc} pre s₁ s is (circuit-instr hc i st₀) r-tail
    (step-IH i r-head)
  where
    -- For each non-output instruction, `circuit-instr hc i st₀`'s
    -- `output-wires` field reduces to `SynthState.output-wires st₀`
    -- definitionally; the helper's result is the IH at `s₁`.
    step-IH : ∀ (i : Instruction) {s₁} → R-instr pre s₀ i s₁
      → mem-lookups (Preprocessed.memory s₁)
          (SynthState.output-wires (circuit-instr hc i st₀))
        ≡ just (Preprocessed.outputs s₁)
    step-IH (assert cond)            r@(r-assert _)          =
      output-wires-non-output-step (assert cond) st₀ r refl H
    step-IH (cond-select b a c)      r@(r-cond-select _ _ _) =
      output-wires-non-output-step (cond-select b a c) st₀ r refl H
    step-IH (constrain-bits v n)     r@(r-constrain-bits _ _ _) =
      output-wires-non-output-step (constrain-bits v n) st₀ r refl H
    step-IH (constrain-eq a b)       r@(r-constrain-eq _ _ _) =
      output-wires-non-output-step (constrain-eq a b) st₀ r refl H
    step-IH (constrain-to-boolean v) r@(r-constrain-to-boolean _) =
      output-wires-non-output-step (constrain-to-boolean v) st₀ r refl H
    step-IH (copy v)                 r@(r-copy _)            =
      output-wires-non-output-step (copy v) st₀ r refl H
    step-IH (declare-pub-input v)    r@(r-declare-pub-input _) =
      output-wires-non-output-step (declare-pub-input v) st₀ r refl H
    step-IH (pi-skip g n)            r@(r-pi-skip-active _ _) =
      output-wires-non-output-step (pi-skip g n) st₀ r refl H
    step-IH (pi-skip g n)            r@(r-pi-skip-inactive _) =
      output-wires-non-output-step (pi-skip g n) st₀ r refl H
    step-IH (ec-add a-x a-y b-x b-y) r@(r-ec-add _ _ _ _ _)  =
      output-wires-non-output-step (ec-add a-x a-y b-x b-y) st₀ r refl H
    step-IH (ec-mul a-x a-y sc)      r@(r-ec-mul _ _ _ _)    =
      output-wires-non-output-step (ec-mul a-x a-y sc) st₀ r refl H
    step-IH (ec-mul-generator sc)    r@(r-ec-mul-generator _ _) =
      output-wires-non-output-step (ec-mul-generator sc) st₀ r refl H
    step-IH (hash-to-curve inputs)   r@(r-hash-to-curve _ _) =
      output-wires-non-output-step (hash-to-curve inputs) st₀ r refl H
    step-IH (load-imm imm)           r@r-load-imm            =
      output-wires-non-output-step (load-imm imm) st₀ r refl H
    step-IH (div-mod-power-of-two v bits) r@(r-div-mod-power-of-two _ _) =
      output-wires-non-output-step (div-mod-power-of-two v bits) st₀ r refl H
    step-IH (reconstitute-field d m bits) r@(r-reconstitute-field _ _ _ _ _) =
      output-wires-non-output-step (reconstitute-field d m bits) st₀ r refl H
    -- output v: the synth state pushes `v` to output-wires, the operational
    -- state pushes its value to outputs.  Combine via mem-lookups-snoc.
    step-IH (output var) (r-output {v = v} la) =
      -- circuit-instr _ (output var) st₀ = record st₀ { output-wires = ow ∷ʳ var }
      -- s₁ = push-output s₀ v, memory unchanged, outputs s₁ = outputs s₀ ∷ʳ v.
      mem-lookups-snoc (Preprocessed.memory s₀)
                        (SynthState.output-wires st₀) var
                        {vs = Preprocessed.outputs s₀} {v = v}
                        H la
    step-IH (transient-hash inputs)  r@(r-transient-hash _)  =
      output-wires-non-output-step (transient-hash inputs) st₀ r refl H
    step-IH (persistent-hash al inputs) r@(r-persistent-hash _ _) =
      output-wires-non-output-step (persistent-hash al inputs) st₀ r refl H
    step-IH (test-eq a b)            r@(r-test-eq _ _)       =
      output-wires-non-output-step (test-eq a b) st₀ r refl H
    step-IH (add a b)                r@(r-add _ _)           =
      output-wires-non-output-step (add a b) st₀ r refl H
    step-IH (mul a b)                r@(r-mul _ _)           =
      output-wires-non-output-step (mul a b) st₀ r refl H
    step-IH (neg a)                  r@(r-neg _)             =
      output-wires-non-output-step (neg a) st₀ r refl H
    step-IH (not a)                  r@(r-not _)             =
      output-wires-non-output-step (not a) st₀ r refl H
    step-IH (less-than a b bits)     r@(r-less-than _ _ _ _)   =
      output-wires-non-output-step (less-than a b bits) st₀ r refl H
    step-IH (public-input nothing)   r@(r-public-input-inactive _) =
      output-wires-non-output-step (public-input nothing) st₀ r refl H
    step-IH (public-input nothing)   r@(r-public-input-active _ cp) =
      output-wires-non-output-step (public-input nothing) st₀ r
        (consume-pub-out-outputs s₀ cp) H
    step-IH (public-input (just g))  r@(r-public-input-inactive _) =
      output-wires-non-output-step (public-input (just g)) st₀ r refl H
    step-IH (public-input (just g))  r@(r-public-input-active _ cp) =
      output-wires-non-output-step (public-input (just g)) st₀ r
        (consume-pub-out-outputs s₀ cp) H
    step-IH (private-input nothing)  r@(r-private-input-inactive _) =
      output-wires-non-output-step (private-input nothing) st₀ r refl H
    step-IH (private-input nothing)  r@(r-private-input-active _ cp) =
      output-wires-non-output-step (private-input nothing) st₀ r
        (consume-priv-outputs s₀ cp) H
    step-IH (private-input (just g)) r@(r-private-input-inactive _) =
      output-wires-non-output-step (private-input (just g)) st₀ r refl H
    step-IH (private-input (just g)) r@(r-private-input-active _ cp) =
      output-wires-non-output-step (private-input (just g)) st₀ r
        (consume-priv-outputs s₀ cp) H

-- Top-level specialisation: when synth state starts with no recorded
-- output wires, the looked-up output wires match the operational outputs.
output-wires-coincide
  : ∀ {hc} (pre : ProofPreimage) (s₀ s : Preprocessed)
    (is : List Instruction) (st₀ : SynthState)
  → R-instrs pre s₀ is s
  → SynthState.output-wires st₀ ≡ []
  → Preprocessed.outputs s₀ ≡ []
  → mem-lookups (Preprocessed.memory s)
      (SynthState.output-wires (circuit-instrs hc is st₀))
    ≡ just (Preprocessed.outputs s)
output-wires-coincide {hc} pre s₀ s is st₀ Rs ow-empty out-empty =
  output-wires-coincide-gen {hc} pre s₀ s is st₀ Rs
    (subst (λ ows → mem-lookups (Preprocessed.memory s₀) ows
                      ≡ just (Preprocessed.outputs s₀))
            (sym ow-empty)
            (subst (λ os → mem-lookups (Preprocessed.memory s₀) [] ≡ just os)
                    (sym out-empty)
                    refl))

-- The wires `[0 .. n)` of the initial memory look up to exactly
-- `inputs pre`.  Uses `mem-lookups-nat-range` + `init-state-memory`
-- + the WF1 length enforcement extracted by `init-state-num-inputs`.
inputs-lookup-init
  : ∀ (src : IrSource) (pre : ProofPreimage) (s₀ : Preprocessed)
  → init-state src pre ≡ just s₀
  → mem-lookups (Preprocessed.memory s₀) (nat-range (IrSource.num-inputs src))
    ≡ just (ProofPreimage.inputs pre)
inputs-lookup-init src pre s₀ eq =
  let mem≡       = init-state-memory src pre s₀ eq
      len-eq     = init-state-num-inputs src pre s₀ eq  -- length inputs ≡ num-inputs
      -- Step 1: mem-lookups (inputs pre) (nat-range (length (inputs pre))) ≡ just (inputs pre)
      lk : mem-lookups (ProofPreimage.inputs pre)
                       (nat-range (length (ProofPreimage.inputs pre)))
           ≡ just (ProofPreimage.inputs pre)
      lk = mem-lookups-nat-range (ProofPreimage.inputs pre)
      -- Step 2: rewrite (length inputs) to (num-inputs src) via len-eq.
      lk' : mem-lookups (ProofPreimage.inputs pre)
                        (nat-range (IrSource.num-inputs src))
            ≡ just (ProofPreimage.inputs pre)
      lk' = subst (λ n → mem-lookups (ProofPreimage.inputs pre) (nat-range n)
                          ≡ just (ProofPreimage.inputs pre))
                   len-eq lk
      -- Step 3: rewrite (inputs pre) to (memory s₀) via mem≡.
  in subst (λ m → mem-lookups m (nat-range (IrSource.num-inputs src))
                    ≡ just (ProofPreimage.inputs pre))
            (sym mem≡) lk'

------------------------------------------------------------------------
-- Top-level forward.
--
-- The proof decomposes
-- `R src pre s` into its `init-eq`, body trace `Rs`, transcript-
-- consumption, and comm-ok components, runs `R-instrs→satisfies-constraints`
-- to discharge the bulk of the constraints, and glues in the top-level
-- `comm` (when `has-comm = true`).
------------------------------------------------------------------------

-- Helper: from `init-state src pre ≡ just s₀` with `hc = true` and
-- `comm-commitment pre ≡ just (c, r)`, the initial pis has `c` at index 1.
private
  init-state-pi-1 : ∀ src pre s₀ c r
    → IrSource.do-communications-commitment src ≡ true
    → ProofPreimage.comm-commitment pre ≡ just (c , r)
    → init-state src pre ≡ just s₀
    → pi-lookup (Preprocessed.pis s₀) 1 ≡ just c
  init-state-pi-1 src pre s₀ c r hc-true cc-just eq
    with InitInv.pis-shape (init-state-inv src pre eq)
  ... | inj₂ (_ , _ , _ , cc≡ , pis≡) =
        trans (cong (λ p → pi-lookup p 1) pis≡)
              (cong just
                (cong proj₁ (just-injective (trans (sym cc≡) cc-just))))
  ... | inj₁ (hc≡ , _) with trans (sym hc≡) hc-true
  ...   | ()

-- Forward direction, hc=false branch.
private
  circuit-faithful-fwd-false
    : ∀ (src : IrSource) (pre : ProofPreimage) (s s₀ : Preprocessed)
    → IrSource.do-communications-commitment src ≡ false
    → init-state src pre ≡ just s₀
    → R-instrs pre s₀ (IrSource.instructions src) s
    → satisfies (circuit src) (witness-of s pre)
  circuit-faithful-fwd-false src pre s s₀ hc-false init-eq Rs =
    mk-sat pi-length-eq mem-length-eq rand-shape-eq constraints-ok
    where
      n  = IrSource.num-inputs src
      st₀ : SynthState
      st₀ = mk-synth n [] 0 []
      instrs = IrSource.instructions src
      mem≡    = init-state-memory src pre s₀ init-eq
      len-eq  = init-state-num-inputs src pre s₀ init-eq
      mi₀ : SynthState.nr-wires st₀ ≡ length (Preprocessed.memory s₀)
      mi₀ = sym (trans (cong length mem≡) len-eq)
      -- pi₀: length (pis s₀) ≡ preamble + nr-declared-pi st₀.
      -- preamble false = 1, nr-declared-pi st₀ = 0.
      pi₀-pre : length (Preprocessed.pis s₀)
                  ≡ preamble-pi-count (IrSource.do-communications-commitment src)
      pi₀-pre = init-state-pis-length src pre s₀ init-eq
      pi₀ : length (Preprocessed.pis s₀)
              ≡ preamble-pi-count false + SynthState.nr-declared-pi st₀
      pi₀ = subst (λ b → length (Preprocessed.pis s₀) ≡ preamble-pi-count b + 0)
                   hc-false
                   (trans pi₀-pre
                          (sym (+-identityʳ (preamble-pi-count
                                  (IrSource.do-communications-commitment src)))))
      result = R-instrs→satisfies-constraints {hc = false} pre s₀ s instrs st₀
                 mi₀ pi₀ []ᴬ Rs
      pi-end  = proj₁ (proj₂ result)
      sat-end = proj₂ (proj₂ result)
      -- Now we need to transport pi-end and sat-end through the
      -- (currently-abstract) hc.  The `Circuit.pi-len (circuit src)` and
      -- `Circuit.constraints (circuit src)` both depend on hc; using `hc-false`
      -- we substitute.
      circuit-eq : circuit src ≡
        mk-circuit
          (SynthState.nr-wires (circuit-instrs false instrs st₀))
          (SynthState.constraints (circuit-instrs false instrs st₀))
          (1 + SynthState.nr-declared-pi (circuit-instrs false instrs st₀))
          false
      circuit-eq = circuit-instantiate-false hc-false
        where
          -- Substitute `hc = false` into `circuit src`'s definition.
          -- The `if false then ... else cls` reduces to `cls`,
          -- `preamble-pi-count false = 1`, so we get the matching record.
          circuit-instantiate-false :
            IrSource.do-communications-commitment src ≡ false
            → circuit src ≡
              mk-circuit
                (SynthState.nr-wires (circuit-instrs false instrs st₀))
                (SynthState.constraints (circuit-instrs false instrs st₀))
                (1 + SynthState.nr-declared-pi (circuit-instrs false instrs st₀))
                false
          circuit-instantiate-false refl = refl
      mem-length-eq : length (Preprocessed.memory s)
                        ≡ Circuit.nr-wires (circuit src)
      mem-length-eq = trans (sym (proj₁ result))
                            (sym (cong Circuit.nr-wires circuit-eq))
      pi-length-eq : length (Preprocessed.pis s) ≡ Circuit.pi-len (circuit src)
      pi-length-eq = trans pi-end (cong Circuit.pi-len (sym circuit-eq))
      rand-shape-eq : Maybe-shape (Circuit.has-comm (circuit src))
                                   (Witness.comm-rand (witness-of s pre))
      rand-shape-eq =
        subst (λ c → Maybe-shape (Circuit.has-comm c) (comm-rand-of pre))
              (sym circuit-eq) tt
      constraints-ok : satisfies-constraints (Circuit.constraints (circuit src))
                                       (witness-of s pre)
      constraints-ok = subst (λ c → satisfies-constraints (Circuit.constraints c)
                                                    (witness-of s pre))
                         (sym circuit-eq) sat-end

-- Forward direction, hc=true branch with comm-commitment = just (c, r).
private
  circuit-faithful-fwd-true
    : ∀ (src : IrSource) (pre : ProofPreimage) (s s₀ : Preprocessed) c r
    → IrSource.do-communications-commitment src ≡ true
    → ProofPreimage.comm-commitment pre ≡ just (c , r)
    → init-state src pre ≡ just s₀
    → R-instrs pre s₀ (IrSource.instructions src) s
    → T (c ≡ᶠ? transient-commit (ProofPreimage.inputs pre ++ Preprocessed.outputs s) r)
    → satisfies (circuit src) (witness-of s pre)
  circuit-faithful-fwd-true src pre s s₀ c r hc-true cc-just init-eq Rs co-eq =
    mk-sat pi-length-eq mem-length-eq rand-shape-eq constraints-ok
    where
      n  = IrSource.num-inputs src
      st₀ : SynthState
      st₀ = mk-synth n [] 0 []
      instrs = IrSource.instructions src
      mem≡    = init-state-memory src pre s₀ init-eq
      len-eq  = init-state-num-inputs src pre s₀ init-eq
      mi₀ : SynthState.nr-wires st₀ ≡ length (Preprocessed.memory s₀)
      mi₀ = sym (trans (cong length mem≡) len-eq)
      pi₀-pre : length (Preprocessed.pis s₀)
                  ≡ preamble-pi-count (IrSource.do-communications-commitment src)
      pi₀-pre = init-state-pis-length src pre s₀ init-eq
      pi₀ : length (Preprocessed.pis s₀)
              ≡ preamble-pi-count true + SynthState.nr-declared-pi st₀
      pi₀ = subst (λ b → length (Preprocessed.pis s₀) ≡ preamble-pi-count b + 0)
                   hc-true
                   (trans pi₀-pre
                          (sym (+-identityʳ (preamble-pi-count
                                  (IrSource.do-communications-commitment src)))))
      result = R-instrs→satisfies-constraints {hc = true} pre s₀ s instrs st₀
                 mi₀ pi₀ []ᴬ Rs
      pi-end  = proj₁ (proj₂ result)
      sat-end = proj₂ (proj₂ result)
      st-end  = circuit-instrs true instrs st₀
      cm-inputs = nat-range n
      out-wires = SynthState.output-wires st-end
      -- The comm-constraint witness:
      ivs-lookup : mem-lookups (Preprocessed.memory s) cm-inputs
                    ≡ just (ProofPreimage.inputs pre)
      ivs-lookup = mem-lookups-mono-R-instrs pre s₀ s instrs cm-inputs
                     (ProofPreimage.inputs pre) Rs
                     (inputs-lookup-init src pre s₀ init-eq)
      ovs-lookup : mem-lookups (Preprocessed.memory s) out-wires
                    ≡ just (Preprocessed.outputs s)
      ovs-lookup = output-wires-coincide {hc = true} pre s₀ s instrs st₀ Rs
                     refl
                     (init-state-outputs src pre s₀ init-eq)
      pi-1-init : pi-lookup (Preprocessed.pis s₀) 1 ≡ just c
      pi-1-init = init-state-pi-1 src pre s₀ c r hc-true cc-just init-eq
      pi-1-final : pi-lookup (Preprocessed.pis s) 1 ≡ just c
      pi-1-final = pi-lookup-mono-R-instrs pre s₀ s instrs 1 c Rs pi-1-init
      c≡tc : c ≡ transient-commit (ProofPreimage.inputs pre ++ Preprocessed.outputs s) r
      c≡tc = ≡ᶠ?-true co-eq
      w = witness-of s pre
      rand≡ : Witness.comm-rand w ≡ just r
      rand≡ = cong (Data.Maybe.map (λ (_ , r) → r)) cc-just
      holds-comm : holds w (comm cm-inputs out-wires)
      holds-comm =
        ProofPreimage.inputs pre
        , Preprocessed.outputs s
        , r
        , c
        , ivs-lookup
        , ovs-lookup
        , rand≡
        , pi-1-final
        , c≡tc
      body-constraints = SynthState.constraints st-end
      -- circuit src reduces, under hc-true, to its hc=true shape.
      circuit-eq : circuit src ≡
        mk-circuit
          (SynthState.nr-wires st-end)
          (body-constraints ∷ʳ comm cm-inputs out-wires)
          (2 + SynthState.nr-declared-pi st-end)
          true
      circuit-eq = circuit-instantiate-true hc-true
        where
          circuit-instantiate-true :
            IrSource.do-communications-commitment src ≡ true
            → circuit src ≡
              mk-circuit
                (SynthState.nr-wires st-end)
                (body-constraints ∷ʳ comm cm-inputs out-wires)
                (2 + SynthState.nr-declared-pi st-end)
                true
          circuit-instantiate-true refl = refl
      mem-length-eq : length (Preprocessed.memory s)
                        ≡ Circuit.nr-wires (circuit src)
      mem-length-eq = trans (sym (proj₁ result))
                            (sym (cong Circuit.nr-wires circuit-eq))
      pi-length-eq : length (Preprocessed.pis s) ≡ Circuit.pi-len (circuit src)
      pi-length-eq = trans pi-end (cong Circuit.pi-len (sym circuit-eq))
      rand-shape-eq : Maybe-shape (Circuit.has-comm (circuit src))
                                   (Witness.comm-rand w)
      rand-shape-eq =
        subst (λ cc → Maybe-shape (Circuit.has-comm cc) (Witness.comm-rand w))
              (sym circuit-eq)
              (subst (λ rd → Maybe-shape true rd) (sym rand≡) tt)
      constraints-ok-body++ : satisfies-constraints
        (body-constraints ∷ʳ comm cm-inputs out-wires) w
      constraints-ok-body++ = satisfies-constraints-++ body-constraints
        (comm cm-inputs out-wires ∷ [])
        sat-end
        (holds-comm ∷ᴬ []ᴬ)
      constraints-ok : satisfies-constraints (Circuit.constraints (circuit src)) w
      constraints-ok = subst (λ c' → satisfies-constraints (Circuit.constraints c') w)
                          (sym circuit-eq) constraints-ok-body++

-- Reconstitute the comm-ok equality at the hc=true / just (c,r) branch.
private
  extract-comm-ok-eq : ∀ src pre s c r
    → IrSource.do-communications-commitment src ≡ true
    → ProofPreimage.comm-commitment pre ≡ just (c , r)
    → T (comm-ok src pre s)
    → T (c ≡ᶠ? transient-commit (ProofPreimage.inputs pre ++ Preprocessed.outputs s) r)
  extract-comm-ok-eq src pre s c r hc-eq cc-eq co
    with IrSource.do-communications-commitment src
       | ProofPreimage.comm-commitment pre
       | hc-eq | cc-eq
  ... | true | just .(c , r) | _ | refl = co

  -- comm-ok with hc=true and comm-commitment=nothing is impossible.
  no-comm-contra : ∀ src pre s
    → IrSource.do-communications-commitment src ≡ true
    → ProofPreimage.comm-commitment pre ≡ nothing
    → T (comm-ok src pre s)
    → ⊥
  no-comm-contra src pre s hc-eq cc-eq co
    with IrSource.do-communications-commitment src
       | ProofPreimage.comm-commitment pre
       | hc-eq | cc-eq
  ... | true | nothing | _ | _ = co

  -- Discriminate on Bool (for use after extracting `do-comm src`).
  bool-cases : (b : Bool) → (b ≡ true) ⊎ (b ≡ false)
  bool-cases true  = inj₁ refl
  bool-cases false = inj₂ refl

  -- Discriminate on Maybe (Fr × Fr).
  maybe-cases : (m : Maybe (Fr × Fr))
    → (m ≡ nothing) ⊎ (Σ Fr λ c → Σ Fr λ r → m ≡ just (c , r))
  maybe-cases nothing         = inj₁ refl
  maybe-cases (just (c , r))  = inj₂ (c , r , refl)

-- The top-level forward lemma.  Note there is no producer-safety
-- hypothesis: the forward direction (completeness of the lowering)
-- holds for ALL circuits, producer-safe or not (spec §6.2).
circuit-faithful-fwd
  : ∀ (src : IrSource) (pre : ProofPreimage) (s : Preprocessed)
  → R src pre s
  → satisfies (circuit src) (witness-of s pre)
circuit-faithful-fwd src pre s (s₀ , init-eq , Rs , _tc , co)
  with bool-cases (IrSource.do-communications-commitment src)
... | inj₂ hc-false =
  circuit-faithful-fwd-false src pre s s₀ hc-false init-eq Rs
... | inj₁ hc-true with maybe-cases (ProofPreimage.comm-commitment pre)
...   | inj₁ cc-none =
        ⊥-elim (no-comm-contra src pre s hc-true cc-none co)
...   | inj₂ (c , r , cc-just) =
        circuit-faithful-fwd-true src pre s s₀ c r
          hc-true cc-just init-eq Rs
          (extract-comm-ok-eq src pre s c r hc-true cc-just co)

------------------------------------------------------------------------
-- Section C.5.  Wire-discipline soundness.
--
-- The backward dispatcher needs, for each per-instruction case, a bound
-- `operand < length (memory s)` to pull pre-state lookups back from
-- post-state lookups in the satisfies-constraints witness.  The producer
-- obligation `wire-disc` (Obligations.agda) supplies this as a static
-- linear scan: each instruction's operand indices are checked against
-- the current wire count, which is `length (memory s)` on the
-- operational side under `mem-inv`.
--
-- `wire-disc-sound` lifts `T (wire-disc src)` to a `Wire-Trace`
-- predicate threaded along the instruction list; pairing this with
-- `mem-inv` then gives per-step `WireOK instr (length mem)`,
-- which `lookup-shrink` from CircuitFaithfulness then converts into the
-- needed pre-state lookups.
------------------------------------------------------------------------

private

  -- Reconstruct a Wire-Trace from the Bool scan.
  wire-scan→trace : ∀ is n {final}
    → wire-scan is n ≡ just final
    → Wire-Trace is n final
  wire-scan→trace []       n refl = wire-done
  wire-scan→trace (i ∷ is) n eq
    with wire-step i n in step-eq
  ... | just n' = wire-cons step-eq (wire-scan→trace is n' eq)

  -- Bool → Wire-Trace witness extractor (mirrors O2-bool→Runs).
  wire-bool→trace : ∀ {src} → T (wire-disc src)
    → ∃-syntax λ final →
        Wire-Trace (IrSource.instructions src) (IrSource.num-inputs src) final
  wire-bool→trace {src} eq
    with wire-scan (IrSource.instructions src) (IrSource.num-inputs src)
         in scan-eq
  ... | just final =
        final , wire-scan→trace (IrSource.instructions src)
                                 (IrSource.num-inputs src) scan-eq

  -- Soundness: `producer-safe` gives a Wire-Trace.
  wire-disc-sound : ∀ {src} → T (producer-safe src)
    → ∃-syntax λ final →
        Wire-Trace (IrSource.instructions src) (IrSource.num-inputs src) final
  wire-disc-sound {src} ps = wire-bool→trace {src} (producer-safe-wire-disc {src} ps)

  -- Per-step extractor: a Wire-Trace covering `instr ∷ rest` gives both
  -- the `WireOK instr n` premise and the residual trace at the bumped
  -- counter.  Splitting `wireOK? instr n` reduces `wire-step instr n` and
  -- refines the just-equation simultaneously; the `no` case is impossible
  -- (`wire-step` would be `nothing`, contradicting the cons step).
  wire-trace-head : ∀ {instr is n final}
    → Wire-Trace (instr ∷ is) n final
    → WireOK instr n
        × Wire-Trace is (n + Δmem instr) final
  wire-trace-head {instr} {is} {n} (wire-cons {n' = n'} step-eq tail)
    with wireOK? instr n | step-eq
  ... | yes ok | refl = ok , tail

------------------------------------------------------------------------
-- Section D.  Backward direction (statements only).
--
-- The backward direction needs the same
-- invariants threaded the other way: from a satisfying assignment +
-- `T (producer-safe src)`, recover an `R-instrs` derivation.
--
-- The four obligation-bearing backward proofs in
-- `CircuitFaithfulness.agda` (`assert-bwd`, `not-bwd`,
-- `reconstitute-field-bwd`, `less-than-bwd`) each take an
-- obligation-evidence hypothesis explicitly.
--
-- The wire-discipline obligation is threaded in `Obligations.agda` and
-- discharged by `wire-disc-sound` above; D1's signature takes
-- `WireOK instr (nr-wires st)` as a per-step premise.  D1's body
-- (`satisfies→R-instr-step` below) dispatches all 26 instruction cases
-- directly to the corresponding `*-bwd` lemma in
-- `CircuitFaithfulness.agda`.  The signature uses explicit suffix
-- decomposition:
--   • `mem-suf : List Fr`  — memory extension
--   • `pis-suf : List Fr`  — pis extension (= [] for non-pi cases)
-- and outputs Σ s' with `memory s' ≡ memory s ++ mem-suf` and
-- `pis s' ≡ pis s ++ pis-suf`.
------------------------------------------------------------------------

-- Signature shape: the dispatcher *produces* the post-state `s'` rather
-- than consuming it.  The witness in the input satisfies-constraints is
-- over `(memory s ++ suf, pis')` — the suffix is the concrete memory
-- extension committed to by the satisfying witness.
--
-- Output Σ packages:
--   • `s'`              — the recovered post-state,
--   • `mem-eq`          — `memory s' ≡ memory s ++ suf`,
--   • `pis-eq`          — `pis s' ≡ pis'`,
--   • `R-instr pre s i s'`  — the operational reconstruction.
--
-- `WireOK i (nr-wires st)` is the per-step consequence of
-- the producer's `wire-disc` obligation (threaded by D2 via
-- `wire-trace-head`).  Combined with `mem-inv : nr-wires st ≡ length
-- (memory s)` this lets each case extract pre-state lookups from
-- post-state ones via `lookup-shrink`.
--
-- The dispatcher discharges all 26 instruction cases directly, using
-- the corresponding `*-bwd` lemma in CircuitFaithfulness.agda.  The
-- obligation-bearing cases (assert, not, reconstitute-field, less-than)
-- consume the O2/O3 evidence threaded through the signature; the
-- side-data cases (output, pi-skip, public-input, private-input) read
-- the transcript/skip data off the `op-side-data` payload `sd`.

private

  -- O2 obligation-check extraction.  For the obligation-bearing
  -- instructions (`assert`, `not`, `cond-select`), the check is
  -- `require-∈ c bk`; a `≡ just bk` premise forces the membership.
  o2-check-mem? : ∀ (c : Index) (bk : IndexSet)
    → require-∈ c bk ≡ just bk
    → c ∈ bk
  o2-check-mem? c bk eq with c ∈? bk
  ... | yes p = p
  ... | no  _ with eq
  ...           | ()



-- Per-instruction operational side data supplied to the backward
-- dispatcher.  For most instructions this is `⊤` (no side data needed);
-- for the four "side-data instructions" (`output`, `pi-skip`,
-- `public-input`, `private-input`) it carries the per-step evidence
-- that the in-circuit witness alone doesn't determine — namely a
-- memory lookup (for `output`), a guard evaluation result (for
-- `pi-skip`, `public-input`, `private-input`), a transcript-prefix
-- match (for active `pi-skip`), and the consumed transcript entry
-- (for active `public-input` / `private-input`).
--
-- Shape encoding by instruction:
--   • Δmem=0, Δpis=0   →  (mem-suf ≡ []) × (pis-suf ≡ [])
--   • Δmem=1, Δpis=0   →  Σ Fr (λ w → mem-suf ≡ w ∷ []) × (pis-suf ≡ [])
--   • Δmem=2, Δpis=0   →  Σ Fr (λ x → Σ Fr (λ y → mem-suf ≡ x ∷ y ∷ [])) × (pis-suf ≡ [])
--   • Δmem=0, Δpis=1 (declare-pub-input)
--                      →  (mem-suf ≡ []) × Σ Fr (λ wv → pis-suf ≡ wv ∷ [])
-- For the four "side-data instructions" the shape Σ wraps the
-- operational payload (mem-lookup / eval-guard / consume-pub-out /
-- consume-priv).
-- `osd-of` gives the side-data Set for each bookkeeping shape; the pure
-- shapes only pin the suffix lengths, the four side-channel shapes carry
-- the channel payload.  `op-side-data` routes each instruction through
-- its `shapeView`, so `op-side-data (add a b) …` reduces to the pure1
-- shape exactly as a per-instruction table would.
osd-of : ∀ {i} → ShapeView i → ProofPreimage → Preprocessed
       → (mem-suf pis-suf : List Fr) → Set
-- Δmem=0, Δpis=0 (no payload).
osd-of sv-pure0 _ _ ms ps = (ms ≡ []) × (ps ≡ [])
-- Δmem=1, Δpis=0 (push-mem).
osd-of sv-pure1 _ _ ms ps = Σ Fr (λ w → ms ≡ w ∷ []) × (ps ≡ [])
-- Δmem=2, Δpis=0 (push-mem2).  `div-mod-power-of-two` shares this free
-- pushed pair; its constraint pins (q, r) to the unique non-wrapping
-- decomposition (`Encoding.divmod-canon`), which the backward dispatcher
-- recovers from the constraint satisfaction rather than from `sd`.
osd-of sv-pure2 _ _ ms ps =
  Σ Fr (λ x → Σ Fr (λ y → ms ≡ x ∷ y ∷ [])) × (ps ≡ [])
-- Δmem=0, Δpis=1 (declare-pub-input).
osd-of sv-declare _ _ ms ps =
  (ms ≡ []) × Σ Fr (λ wv → ps ≡ wv ∷ [])
-- output v: Δmem=0, Δpis=0; carries a mem-lookup proof producing `val`.
osd-of (sv-output {v}) _ s ms ps =
  Σ Fr (λ val → mem-lookup (Preprocessed.memory s) v ≡ just val)
  × (ms ≡ []) × (ps ≡ [])
-- pi-skip: Δmem=0, Δpis=0; payload = guard's truth value and
-- (if active) the transcript prefix-match check.
osd-of (sv-skip {g} {c}) pre s ms ps =
  (ms ≡ []) × (ps ≡ [])
  × Σ Bool (λ active →
        eval-guard (Preprocessed.memory s) g ≡ just active
      × (if active
         then T (drop (length (Preprocessed.pis s) ∸ c) (Preprocessed.pis s)
                  ≡ᶠ-list?
                take c (drop (Preprocessed.pub-in-idx s ∸ c)
                                  (ProofPreimage.pub-transcript-inputs pre)))
         else ⊤))
-- public-input g: Δmem=1, Δpis=0; the single memory cell `w` is bound
-- by the outer Σ so the payload (consume-pub-out producing `w`) can
-- reference it.
osd-of (sv-pub-in {g}) pre s ms ps =
  Σ Fr (λ w → (ms ≡ w ∷ []) × (ps ≡ [])
    × Σ Bool (λ active →
          eval-guard (Preprocessed.memory s) g ≡ just active
        × (if active
           then Σ Preprocessed (λ s₁ → consume-pub-out s ≡ just (w , s₁))
           else (w ≡ 0ᶠ))))
-- private-input g: symmetric to public-input.
osd-of (sv-priv-in {g}) pre s ms ps =
  Σ Fr (λ w → (ms ≡ w ∷ []) × (ps ≡ [])
    × Σ Bool (λ active →
          eval-guard (Preprocessed.memory s) g ≡ just active
        × (if active
           then Σ Preprocessed (λ s₁ → consume-priv s ≡ just (w , s₁))
           else (w ≡ 0ᶠ))))

op-side-data : Instruction → ProofPreimage → Preprocessed
             → (mem-suf pis-suf : List Fr) → Set
op-side-data i = osd-of (shapeView i)

------------------------------------------------------------------------
-- `next-state-from-osd`
--
-- Computes the canonical post-state produced by each D1 case directly
-- from the inputs (`i`, `pre`, `s`, `mem-suf`, `pis-suf`, `sd`).  This
-- is the post-state that the corresponding `satisfies→R-instr-step`
-- branch returns (as the existential `s'`).  Because `op-side-data-list`
-- threads this *computed* next state into the recursive call, D2's
-- cons-case reconciles the mid-state with D1's output by definitional
-- equality.
--
-- The function pattern-matches on the same shape as D1 (instruction +
-- suffixes + side-data).  Ill-shaped inputs fall through to `s` (this
-- branch is never reached in practice — D2 only invokes it on the
-- shapes that `op-side-data-list` guarantees).
------------------------------------------------------------------------
-- `nso′` computes the post-state for each shape; `next-state-from-osd`
-- routes each instruction through its `shapeView`.  Pattern-matching on
-- the view exposes the shape's payload (the pushed cell(s), the declared
-- input, the transcript split) directly.
nso′ : ∀ {i} (v : ShapeView i) (pre : ProofPreimage) (s : Preprocessed)
       (mem-suf pis-suf : List Fr)
     → osd-of v pre s mem-suf pis-suf → Preprocessed
-- Δmem=0, Δpis=0 (state unchanged).
nso′ sv-pure0 _ s _ _ _ = s
-- Δmem=1 (push-mem).  Extract `w` from sd.
nso′ sv-pure1 _ s _ _ ((w , _) , _) = push-mem s w
-- Δmem=2 (push-mem2).  Extract `x` and `y` from sd.
nso′ sv-pure2 _ s _ _ ((x , y , _) , _) = push-mem2 s x y
-- Δpis=1 (declare-pub-input).  Extract `wv` from sd.
nso′ sv-declare _ s _ _ (_ , wv , _) =
  record s
    { pis        = Preprocessed.pis s ++ (wv ∷ [])
    ; pub-in-idx = suc (Preprocessed.pub-in-idx s)
    }
-- output v: outputs += val.
nso′ sv-output _ s _ _ ((val , _) , _ , _) =
  record s { outputs = Preprocessed.outputs s ++ (val ∷ []) }
-- pi-skip g count: splits on `active`.
nso′ sv-skip _ s _ _ (_ , _ , (true  , _ , _)) =
  record s { pi-skips = Preprocessed.pi-skips s ++ (nothing ∷ []) }
nso′ (sv-skip {c = count}) _ s _ _ (_ , _ , (false , _ , _)) =
  record s
    { pi-skips   = Preprocessed.pi-skips s ++ (just count ∷ [])
    ; pub-in-idx = Preprocessed.pub-in-idx s ∸ count
    }
-- public-input g: splits on `active`.
nso′ sv-pub-in _ s _ _ (w , _ , _ , (true , _ , (s₁ , _))) =
  record s₁ { memory = Preprocessed.memory s₁ ++ (w ∷ []) }
nso′ sv-pub-in _ s _ _ (w , _ , _ , (false , _ , _)) =
  record s { memory = Preprocessed.memory s ++ (w ∷ []) }
-- private-input g: symmetric.
nso′ sv-priv-in _ s _ _ (w , _ , _ , (true , _ , (s₁ , _))) =
  record s₁ { memory = Preprocessed.memory s₁ ++ (w ∷ []) }
nso′ sv-priv-in _ s _ _ (w , _ , _ , (false , _ , _)) =
  record s { memory = Preprocessed.memory s ++ (w ∷ []) }

next-state-from-osd
  : (i : Instruction) (pre : ProofPreimage) (s : Preprocessed)
    (mem-suf pis-suf : List Fr)
  → op-side-data i pre s mem-suf pis-suf
  → Preprocessed
next-state-from-osd i = nso′ (shapeView i)

-- Reshape the split-off new-constraint satisfaction (`sat-new`, from
-- `satisfies-constraints-split`) into the exact shape every per-instruction
-- `*-bwd` lemma expects.  Two orthogonal rewrites, shared verbatim by
-- every push-mem / push-mem2 case of D1 below:
--   • shift the constraint's wire index `n` to `length (memory s)`, via the
--     memory invariant `mi : n ≡ length (memory s)`; and
--   • drop the trailing `++ []` on the witness's pis (every non-pis
--     instruction has `pis-suf = []`).
-- `ms` is the memory suffix the instruction appends (`w ∷ []` for Δmem=1,
-- `x ∷ y ∷ []` for Δmem=2).
private
  reshape-core
    : ∀ {hc} {rand : Maybe Fr} (s : Preprocessed) (instr : Instruction)
        (ms : List Fr) {n : ℕ}
    → n ≡ length (Preprocessed.memory s)
    → satisfies-constraints (single-instr-constraints hc n instr)
        (mk-witness (Preprocessed.memory s ++ ms)
                    (Preprocessed.pis s ++ []) rand)
    → satisfies-constraints
        (single-instr-constraints hc (length (Preprocessed.memory s)) instr)
        (mk-witness (Preprocessed.memory s ++ ms)
                    (Preprocessed.pis s) rand)
  reshape-core {hc} {rand} s instr ms mi sat =
    subst (λ p → satisfies-constraints
                   (single-instr-constraints hc (length (Preprocessed.memory s)) instr)
                   (mk-witness (Preprocessed.memory s ++ ms) p rand))
          (++-identityʳ (Preprocessed.pis s))
          (subst (λ k → satisfies-constraints
                          (single-instr-constraints hc k instr)
                          (mk-witness (Preprocessed.memory s ++ ms)
                                      (Preprocessed.pis s ++ []) rand))
                 mi sat)

  -- Δmem=2 variant: additionally bridge the witness memory from the
  -- `mem ++ (x ∷ y ∷ [])` form (produced by `op-side-data`) to the
  -- iterated `(mem ++ (x ∷ [])) ++ (y ∷ [])` form the `*-bwd` lemmas use.
  reshape-push2
    : ∀ {hc} {rand : Maybe Fr} (s : Preprocessed) (instr : Instruction)
        (x y : Fr) {n : ℕ}
    → n ≡ length (Preprocessed.memory s)
    → satisfies-constraints (single-instr-constraints hc n instr)
        (mk-witness (Preprocessed.memory s ++ (x ∷ y ∷ []))
                    (Preprocessed.pis s ++ []) rand)
    → satisfies-constraints
        (single-instr-constraints hc (length (Preprocessed.memory s)) instr)
        (mk-witness ((Preprocessed.memory s ++ (x ∷ [])) ++ (y ∷ []))
                    (Preprocessed.pis s) rand)
  reshape-push2 {hc} {rand} s instr x y mi sat =
    subst (λ m → satisfies-constraints
                   (single-instr-constraints hc (length (Preprocessed.memory s)) instr)
                   (mk-witness m (Preprocessed.pis s) rand))
          (push-mem2-assoc (Preprocessed.memory s) x y)
          (reshape-core {hc} {rand} s instr (x ∷ y ∷ []) mi sat)

  -- Δmem=0 variant: the instruction grows neither memory nor pis, so in
  -- addition to the index shift and pis-drop we also drop the trailing
  -- `++ []` on the witness memory.
  reshape-nogrow
    : ∀ {hc} {rand : Maybe Fr} (s : Preprocessed) (instr : Instruction) {n : ℕ}
    → n ≡ length (Preprocessed.memory s)
    → satisfies-constraints (single-instr-constraints hc n instr)
        (mk-witness (Preprocessed.memory s ++ [])
                    (Preprocessed.pis s ++ []) rand)
    → satisfies-constraints
        (single-instr-constraints hc (length (Preprocessed.memory s)) instr)
        (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) rand)
  reshape-nogrow {hc} {rand} s instr mi sat =
    subst (λ m → satisfies-constraints
                   (single-instr-constraints hc (length (Preprocessed.memory s)) instr)
                   (mk-witness m (Preprocessed.pis s) rand))
          (++-identityʳ (Preprocessed.memory s))
          (reshape-core {hc} {rand} s instr [] mi sat)

  -- The two cells a push-mem2 step appends sit at indices `length xs`
  -- and `suc (length xs)` of `xs ++ (a ∷ b ∷ [])`.  Used by the div-mod
  -- backward step to identify the constraint's (q, r) output values with the
  -- appended cells.
  lookup-skip1 : ∀ (xs : List Fr) (a b : Fr)
    → mem-lookup (xs ++ (a ∷ b ∷ [])) (length xs) ≡ just a
  lookup-skip1 []       a b = refl
  lookup-skip1 (x ∷ xs) a b = lookup-skip1 xs a b
  lookup-skip2 : ∀ (xs : List Fr) (a b : Fr)
    → mem-lookup (xs ++ (a ∷ b ∷ [])) (suc (length xs)) ≡ just b
  lookup-skip2 []       a b = refl
  lookup-skip2 (x ∷ xs) a b = lookup-skip2 xs a b

-- Per-instruction backward step.  Returns a Σ existential because the
-- post-state `s'` is recovered from the satisfaction witness's memory
-- shape; each case applies the appropriate `*-bwd` lemma.
-- `mem-suf` and `pis-suf` are the memory/pis extensions committed to
-- by the witness; D2 threads them per-instruction.
--
-- The four extra premises `O2-Inv`, `O3-Inv`, `O2-check i bk ≡ just bk`,
-- `O3OK i bm` are the per-step shadows of the producer-safety
-- conditions; D2 threads them via `o2-preserve` / `o3-preserve` and
-- extracts the `_∈_` / `lookupᵐ` facts from the corresponding
-- `O2-Trace` / `O3-Trace` step.  For the 18 non-obligation-bearing
-- cases (arithmetic, EC, hashing, copy, declare-pub-input, etc.) the
-- premises are trivially `refl` and unused; the four obligation-
-- bearing cases (`assert`, `not`, `reconstitute-field`, `less-than`)
-- consume O2-Inv via `o2-known-is-bit` and / or O3-Inv via
-- `o3-known-fits`.
satisfies→R-instr-step
  : ∀ {hc} (pre : ProofPreimage) (s : Preprocessed) (i : Instruction)
    (st : SynthState) (mem-suf : List Fr) (pis-suf : List Fr)
    {wf2 : WF2-instr i}            -- WF2 bit-bound for this instruction
  → mem-inv s st
  → pi-inv  hc s st
  → WireOK i (SynthState.nr-wires st)
  → ∀ {bk : IndexSet} {bm : PartialMap}
  → O2-Inv (SynthState.nr-wires st , bk) s
  → O3-Inv (SynthState.nr-wires st , bm) s
  → O2-check i bk ≡ just bk
  → O3OK i bm
  → (sd : op-side-data i pre s mem-suf pis-suf)
  → satisfies-constraints
      (SynthState.constraints (circuit-instr hc i st))
      (mk-witness (Preprocessed.memory s ++ mem-suf)
                  (Preprocessed.pis    s ++ pis-suf)
                  (comm-rand-of pre))
  → let s' = next-state-from-osd i pre s mem-suf pis-suf sd in
        (Preprocessed.memory s' ≡ Preprocessed.memory s ++ mem-suf)
      × (Preprocessed.pis    s' ≡ Preprocessed.pis    s ++ pis-suf)
      × R-instr pre s i s'
-- D1 dispatcher cases.  Each instruction's case follows this template:
--   1. Pattern-match `i` and `suf`; unfold `circuit-instr hc i st`.
--   2. Apply `satisfies-constraints-split` to peel off prior-constraint sat.
--   3. Project the `WireOK` premise `wc` and `subst` with `mi` to get
--      per-operand bounds `suc operand ≤ length (memory s)`.
--   4. Destructure the new-constraints satisfaction; pull post-state
--      lookups via `lookup-shrink` to get pre-state lookups.
--   5. Call the `*-bwd` lemma to produce `R-instr pre s i s'`.
--   6. Package the Σ output (s', mem-eq, pis-eq, R-instr).

-- ─── add(a, b) ─────────────────────────────────────────────────────
-- Δmem = 1; pis unchanged.
satisfies→R-instr-step {hc} pre s (add a b) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat
  with wc
... | a<n , b<n =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      a≤len  = subst (suc a Data.Nat.≤_) mi a<n
      b≤len  = subst (suc b Data.Nat.≤_) mi b<n
      -- Peel the new constraint off `sat`:  constraints (circuit-instr hc (add a b) st)
      -- = constraints st ++ [the add gate].
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      ((wire n ≑ wire a ⊕ wire b) ∷ [])
                      sat
      gate-h = headᴬ sat-new
      (av , bv , _ , la-post , lb-post , _ , _) =
        ≑⊕-inv (mem ++ (w ∷ [])) {n} {a} {b} gate-h
      la-pre  : mem-lookup mem a ≡ just av
      la-pre  = lookup-shrink mem (w ∷ []) a la-post a≤len
      lb-pre  : mem-lookup mem b ≡ just bv
      lb-pre  = lookup-shrink mem (w ∷ []) b lb-post b≤len
      _ , r-add-ev = add-bwd {pre = pre} {s = s} {a = a} {b = b}
                              {av = av} {bv = bv} {v = w} {hc = hc}
                              {rand = comm-rand-of pre}
                              la-pre lb-pre
                              (reshape-core {hc} {comm-rand-of pre} s (add a b) (w ∷ []) mi sat-new)
      s' = push-mem s w
      pis-eq : Preprocessed.pis s' ≡ Preprocessed.pis s ++ []
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-add-ev

-- ─── mul(a, b) ─────────────────────────────────────────────────────
satisfies→R-instr-step {hc} pre s (mul a b) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat
  with wc
... | a<n , b<n =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      a≤len  = subst (suc a Data.Nat.≤_) mi a<n
      b≤len  = subst (suc b Data.Nat.≤_) mi b<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      ((wire n ≑ wire a ⊗ wire b) ∷ []) sat
      gate-h = headᴬ sat-new
      (av , bv , _ , la-post , lb-post , _ , _) =
        ≑⊗-inv (mem ++ (w ∷ [])) {n} {a} {b} gate-h
      la-pre  = lookup-shrink mem (w ∷ []) a la-post a≤len
      lb-pre  = lookup-shrink mem (w ∷ []) b lb-post b≤len
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (mul a b) (w ∷ []) mi sat-new
      _ , r-ev = mul-bwd {pre = pre} {s = s} {a = a} {b = b}
                          {av = av} {bv = bv} {v = w} {hc = hc}
                          {rand = comm-rand-of pre}
                          la-pre lb-pre sat-pis
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── neg(a) ────────────────────────────────────────────────────────
satisfies→R-instr-step {hc} pre s (neg a) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      a≤len  = subst (suc a Data.Nat.≤_) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      ((wire n ≑ ⊝ wire a) ∷ []) sat
      gate-h = headᴬ sat-new
      (av , _ , la-post , _ , _) =
        ≑⊝-inv (mem ++ (w ∷ [])) {n} {a} gate-h
      la-pre  = lookup-shrink mem (w ∷ []) a la-post a≤len
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (neg a) (w ∷ []) mi sat-new
      _ , r-ev = neg-bwd {pre = pre} {s = s} {a = a}
                          {av = av} {v = w} {hc = hc}
                          {rand = comm-rand-of pre}
                          la-pre sat-pis
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── test-eq(a, b) ─────────────────────────────────────────────────
satisfies→R-instr-step {hc} pre s (test-eq a b) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat
  with wc
... | a<n , b<n =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      a≤len  = subst (suc a Data.Nat.≤_) mi a<n
      b≤len  = subst (suc b Data.Nat.≤_) mi b<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st) (test-eq n a b ∷ []) sat
      (av , bv , _ , la-post , lb-post , _) = headᴬ sat-new
      la-pre  = lookup-shrink mem (w ∷ []) a la-post a≤len
      lb-pre  = lookup-shrink mem (w ∷ []) b lb-post b≤len
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (test-eq a b) (w ∷ []) mi sat-new
      _ , r-ev = test-eq-bwd {pre = pre} {s = s} {a = a} {b = b}
                              {av = av} {bv = bv} {v = w} {hc = hc}
                              {rand = comm-rand-of pre}
                              la-pre lb-pre sat-pis
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── copy(v) ───────────────────────────────────────────────────────
satisfies→R-instr-step {hc} pre s (copy v) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      v≤len  = subst (suc v Data.Nat.≤_) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      ((wire n ≑ wire v) ∷ []) sat
      gate-h = headᴬ sat-new
      (vv , _ , la-post , _ , _) =
        ≑wire-inv (mem ++ (w ∷ [])) {n} {v} gate-h
      la-pre  = lookup-shrink mem (w ∷ []) v la-post v≤len
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (copy v) (w ∷ []) mi sat-new
      _ , r-ev = copy-bwd {pre = pre} {s = s} {v = v} {vv = vv} {w = w} {hc = hc}
                           {rand = comm-rand-of pre}
                           la-pre sat-pis
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── constrain-eq(a, b) ───────────────────────────────────────────
-- Δmem = 0; mem/pis unchanged.  Suffix is [].
satisfies→R-instr-step {hc} pre s (constrain-eq a b) st _ _ mi pii wc _ _ _ _ (refl , refl) sat
  with wc
... | a<n , b<n =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      a≤len  = subst (suc a Data.Nat.≤_) mi a<n
      b≤len  = subst (suc b Data.Nat.≤_) mi b<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st) ((wire a ≑ wire b) ∷ []) sat
      gate-h = headᴬ sat-new
      (av , bv , la-post , lb-post , _) =
        ≑vars-inv (mem ++ []) {a} {b} gate-h
      la-eq : mem-lookup mem a ≡ just av
      la-eq = subst (λ m → mem-lookup m a ≡ just av)
                    (++-identityʳ mem) la-post
      lb-eq : mem-lookup mem b ≡ just bv
      lb-eq = subst (λ m → mem-lookup m b ≡ just bv)
                    (++-identityʳ mem) lb-post
      sat-pis = reshape-nogrow {hc} {comm-rand-of pre} s (constrain-eq a b) mi sat-new
      r-ev = constrain-eq-bwd {pre = pre} {s = s} {a = a} {b = b}
                               {av = av} {bv = bv} {hc = hc}
                               {rand = comm-rand-of pre}
                               la-eq lb-eq sat-pis
      mem-eq : Preprocessed.memory s ≡ mem ++ []
      mem-eq = sym (++-identityʳ mem)
      pis-eq : Preprocessed.pis s ≡ Preprocessed.pis s ++ []
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in mem-eq , pis-eq , r-ev

-- ─── constrain-bits(v, n) ─────────────────────────────────────────
satisfies→R-instr-step {hc} pre s (constrain-bits v bits) st _ _ {wf2 = wf2} mi pii wc _ _ _ _ (refl , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      v≤len  = subst (suc v Data.Nat.≤_) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st) (in-range v bits ∷ []) sat
      (vv , la-post , _) = headᴬ sat-new
      la-eq : mem-lookup mem v ≡ just vv
      la-eq = subst (λ m → mem-lookup m v ≡ just vv)
                    (++-identityʳ mem) la-post
      sat-pis = reshape-nogrow {hc} {comm-rand-of pre} s (constrain-bits v bits) mi sat-new
      r-ev = constrain-bits-bwd {pre = pre} {s = s} {v = v} {n = bits}
                                 {vv = vv} {hc = hc} {rand = comm-rand-of pre}
                                 la-eq wf2 sat-pis
      mem-eq = sym (++-identityʳ mem)
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in mem-eq , pis-eq , r-ev

-- ─── constrain-to-boolean(v) ──────────────────────────────────────
satisfies→R-instr-step {hc} pre s (constrain-to-boolean v) st _ _ mi pii wc _ _ _ _ (refl , refl) sat =
  let mem    = Preprocessed.memory s
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st) (boolean v ∷ []) sat
      sat-pis = reshape-nogrow {hc} {comm-rand-of pre} s (constrain-to-boolean v) mi sat-new
      r-ev = constrain-to-boolean-bwd {pre = pre} {s = s} {v = v} {hc = hc}
                                       {rand = comm-rand-of pre} sat-pis
      mem-eq = sym (++-identityʳ mem)
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in mem-eq , pis-eq , r-ev

-- ─── load-imm(imm) ─────────────────────────────────────────────────
-- No operand wire discipline; `WireOK` always holds for load-imm.
satisfies→R-instr-step {hc} pre s (load-imm imm) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat =
  let n      = SynthState.nr-wires st
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      ((wire n ≑ con imm) ∷ []) sat
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (load-imm imm) (w ∷ []) mi sat-new
      _ , r-ev = load-imm-bwd {pre = pre} {s = s} {k = imm} {w = w} {hc = hc}
                               {rand = comm-rand-of pre}
                               sat-pis
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── transient-hash(inputs) ──────────────────────────────────────────
-- Δmem = 1; pis unchanged.  Inputs witnessed via `mem-lookups`.
satisfies→R-instr-step {hc} pre s (transient-hash inputs) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      -- `wc : All (_< n) inputs`; retarget the bound to `length mem`.
      wc-len : All (_< length mem) inputs
      wc-len = subst (λ k → All (_< k) inputs) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (poseidon n inputs ∷ []) sat
      (vs , ov , lvs-post , _) = headᴬ sat-new
      -- Convert the input-vector lookup back to pre-state via mem-lookups-shrink.
      lvs-pre  : mem-lookups mem inputs ≡ just vs
      lvs-pre  = mem-lookups-shrink mem (w ∷ []) inputs wc-len lvs-post
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (transient-hash inputs) (w ∷ []) mi sat-new
      w≡hash , r-ev = transient-hash-bwd {pre = pre} {s = s} {inputs = inputs}
                                     {vs = vs} {v = w} {hc = hc}
                                     {rand = comm-rand-of pre}
                                     lvs-pre sat-pis
      r-ev' : R-instr pre s (transient-hash inputs) (push-mem s w)
      r-ev' = subst (λ z → R-instr pre s (transient-hash inputs) (push-mem s z))
                    (sym w≡hash) r-ev
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev'

-- ─── cond-select(b, a, c) ────────────────────────────────────────────
-- Δmem = 1; pis unchanged.
satisfies→R-instr-step {hc} pre s (cond-select b a c) st _ _ mi pii wc _ _ _ _ ((w , refl) , refl) sat
  with wc
... | b<n , a<n , c<n =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      b≤len  = subst (suc b Data.Nat.≤_) mi b<n
      a≤len  = subst (suc a Data.Nat.≤_) mi a<n
      c≤len  = subst (suc c Data.Nat.≤_) mi c<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (select n b a c ∷ []) sat
      (bv , av , cv , _ , lb-post , la-post , lc-post , _) = headᴬ sat-new
      lb-pre   = lookup-shrink mem (w ∷ []) b lb-post b≤len
      la-pre   = lookup-shrink mem (w ∷ []) a la-post a≤len
      lc-pre   = lookup-shrink mem (w ∷ []) c lc-post c≤len
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (cond-select b a c) (w ∷ []) mi sat-new
      r-ev = cond-select-bwd {pre = pre} {s = s} {b = b} {a = a} {c = c}
                              {bv = bv} {av = av} {cv = cv} {v = w} {hc = hc}
                              {rand = comm-rand-of pre}
                              lb-pre la-pre lc-pre sat-pis
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── hash-to-curve(inputs) ──────────────────────────────────────────
-- Δmem = 2; pis unchanged.  Inputs via `mem-lookups`; output 2 cells.
satisfies→R-instr-step {hc} pre s (hash-to-curve inputs) st _ _ mi pii wc _ _ _ _ ((x , y , refl) , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      wc-len : All (_< length mem) inputs
      wc-len = subst (λ k → All (_< k) inputs) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (h2c n (suc n) inputs ∷ []) sat
      (vs , _ , _ , lvs-post , _) = headᴬ sat-new
      lvs-pre  : mem-lookups mem inputs ≡ just vs
      lvs-pre  = mem-lookups-shrink mem (x ∷ y ∷ []) inputs wc-len lvs-post
      -- Shift n → length mem in the constraint.
      sat-assoc = reshape-push2 {hc} {comm-rand-of pre} s (hash-to-curve inputs) x y mi sat-new
      _ , r-ev = hash-to-curve-bwd {pre = pre} {s = s} {inputs = inputs}
                                    {vs = vs} {x = x} {y = y} {hc = hc}
                                    {rand = comm-rand-of pre}
                                    lvs-pre sat-assoc
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── persistent-hash(α, inputs) ─────────────────────────────────────
satisfies→R-instr-step {hc} pre s (persistent-hash α inputs) st _ _ mi pii wc _ _ _ _ ((x , y , refl) , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      wc-len : All (_< length mem) inputs
      wc-len = subst (λ k → All (_< k) inputs) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (sha256 n (suc n) α inputs ∷ []) sat
      (vs , _ , _ , lvs-post , _) = headᴬ sat-new
      lvs-pre  : mem-lookups mem inputs ≡ just vs
      lvs-pre  = mem-lookups-shrink mem (x ∷ y ∷ []) inputs wc-len lvs-post
      sat-assoc = reshape-push2 {hc} {comm-rand-of pre} s (persistent-hash α inputs) x y mi sat-new
      _ , r-ev = persistent-hash-bwd {pre = pre} {s = s} {α = α} {inputs = inputs}
                                      {vs = vs} {x = x} {y = y} {hc = hc}
                                      {rand = comm-rand-of pre}
                                      lvs-pre sat-assoc
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── ec-add(a-x, a-y, b-x, b-y) ─────────────────────────────────────
satisfies→R-instr-step {hc} pre s (ec-add a-x a-y b-x b-y) st _ _ mi pii wc _ _ _ _ ((x , y , refl) , refl) sat
  with wc
... | ax<n , ay<n , bx<n , by<n =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      ax≤len = subst (suc a-x Data.Nat.≤_) mi ax<n
      ay≤len = subst (suc a-y Data.Nat.≤_) mi ay<n
      bx≤len = subst (suc b-x Data.Nat.≤_) mi bx<n
      by≤len = subst (suc b-y Data.Nat.≤_) mi by<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (ec-add n (suc n) a-x a-y b-x b-y ∷ []) sat
      (ax , ay , bx , by , _ , _ , lax-post , lay-post , lbx-post , lby-post , _) = headᴬ sat-new
      lax-pre  = lookup-shrink mem (x ∷ y ∷ []) a-x lax-post ax≤len
      lay-pre  = lookup-shrink mem (x ∷ y ∷ []) a-y lay-post ay≤len
      lbx-pre  = lookup-shrink mem (x ∷ y ∷ []) b-x lbx-post bx≤len
      lby-pre  = lookup-shrink mem (x ∷ y ∷ []) b-y lby-post by≤len
      sat-assoc = reshape-push2 {hc} {comm-rand-of pre} s (ec-add a-x a-y b-x b-y) x y mi sat-new
      _ , r-ev = ec-add-bwd {pre = pre} {s = s}
                             {a-x = a-x} {a-y = a-y} {b-x = b-x} {b-y = b-y}
                             {ax = ax} {ay = ay} {bx = bx} {by = by}
                             {x = x} {y = y} {hc = hc}
                             {rand = comm-rand-of pre}
                             lax-pre lay-pre lbx-pre lby-pre sat-assoc
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── ec-mul(a-x, a-y, scalar) ───────────────────────────────────────
satisfies→R-instr-step {hc} pre s (ec-mul a-x a-y scalar) st _ _ mi pii wc _ _ _ _ ((x , y , refl) , refl) sat
  with wc
... | ax<n , ay<n , sc<n =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      ax≤len = subst (suc a-x Data.Nat.≤_) mi ax<n
      ay≤len = subst (suc a-y Data.Nat.≤_) mi ay<n
      sc≤len = subst (suc scalar Data.Nat.≤_) mi sc<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (ec-mul n (suc n) a-x a-y scalar ∷ []) sat
      (ax , ay , sc , _ , _ , lax-post , lay-post , lsc-post , _) = headᴬ sat-new
      lax-pre  = lookup-shrink mem (x ∷ y ∷ []) a-x lax-post ax≤len
      lay-pre  = lookup-shrink mem (x ∷ y ∷ []) a-y lay-post ay≤len
      lsc-pre  = lookup-shrink mem (x ∷ y ∷ []) scalar lsc-post sc≤len
      sat-assoc = reshape-push2 {hc} {comm-rand-of pre} s (ec-mul a-x a-y scalar) x y mi sat-new
      _ , r-ev = ec-mul-bwd {pre = pre} {s = s}
                             {a-x = a-x} {a-y = a-y} {scalar = scalar}
                             {ax = ax} {ay = ay} {sc = sc}
                             {x = x} {y = y} {hc = hc}
                             {rand = comm-rand-of pre}
                             lax-pre lay-pre lsc-pre sat-assoc
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── ec-mul-generator(scalar) ───────────────────────────────────────
satisfies→R-instr-step {hc} pre s (ec-mul-generator scalar) st _ _ mi pii wc _ _ _ _ ((x , y , refl) , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      sc≤len = subst (suc scalar Data.Nat.≤_) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (ec-gen n (suc n) scalar ∷ []) sat
      (sc , _ , _ , lsc-post , _) = headᴬ sat-new
      lsc-pre  = lookup-shrink mem (x ∷ y ∷ []) scalar lsc-post sc≤len
      sat-assoc = reshape-push2 {hc} {comm-rand-of pre} s (ec-mul-generator scalar) x y mi sat-new
      _ , r-ev = ec-mul-generator-bwd {pre = pre} {s = s} {scalar = scalar}
                                       {sc = sc} {x = x} {y = y} {hc = hc}
                                       {rand = comm-rand-of pre}
                                       lsc-pre sat-assoc
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev

-- ─── div-mod-power-of-two(var, bits) ────────────────────────────────
-- Δmem = 2.  The side-data supplies the appended cells (x , y); the
-- div-mod constraint (peeled off `sat`) supplies the field equation
-- *plus* the `NoWrap` canonicity guard (faithful to the Rust
-- `enforce_canonical` bit decomposition).  The field equation alone
-- would be underconstrained — distinct (q , r) satisfy it under
-- modular wraparound — but together with `NoWrap`, `divmod-canon` pins
-- (q , r) to the canonical split of `vv`, forcing x and y to the
-- values `r-div-mod-power-of-two` produces; we then subst to match
-- `push-mem2 s x y`.
satisfies→R-instr-step {hc} pre s (div-mod-power-of-two var bits) st _ _ {wf2 = wf2} mi _ var<n _ _ _ _ ((x , y , refl) , refl) sat =
  let mem     = Preprocessed.memory s
      n       = SynthState.nr-wires st
      var≤len = subst (suc var Data.Nat.≤_) mi var<n
      -- Peel the div-mod constraint off `sat`.
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (div-mod n (suc n) var bits ∷ []) sat
      (qv , rv , vv , lq , lr , lv , frv , fqv , nw , veq) = headᴬ sat-new
      -- The constraint's (q, r) output wires `n`, `suc n` hold the appended
      -- cells x, y; `var` is in range, so its lookup shrinks to `mem`.
      qv≡x : qv ≡ x
      qv≡x = just-injective
        (trans (sym (subst (λ k → mem-lookup (mem ++ (x ∷ y ∷ [])) k ≡ just qv) mi lq))
               (lookup-skip1 mem x y))
      rv≡y : rv ≡ y
      rv≡y = just-injective
        (trans (sym (subst (λ k → mem-lookup (mem ++ (x ∷ y ∷ [])) (suc k) ≡ just rv) mi lr))
               (lookup-skip2 mem x y))
      la-sd : mem-lookup mem var ≡ just vv
      la-sd = lookup-shrink mem (x ∷ y ∷ []) var lv var≤len
      -- The constraint + `NoWrap` pin (q, r) to the canonical split of `vv`.
      cqv , crv = divmod-canon {qv = qv} {rv = rv} {vv = vv} {bits = bits} frv nw veq
      xeq : x ≡ from-le-bits (drop bits (to-le-bits vv))
      xeq = trans (sym qv≡x) cqv
      yeq : y ≡ from-le-bits (take bits (to-le-bits vv))
      yeq = trans (sym rv≡y) crv
      -- R fires producing the canonical decomposition of `vv`.
      r-ev : R-instr pre s (div-mod-power-of-two var bits)
               (push-mem (push-mem s (from-le-bits (drop bits (to-le-bits vv))))
                         (from-le-bits (take bits (to-le-bits vv))))
      r-ev = r-div-mod-power-of-two la-sd wf2
      -- We want : R-instr ... (push-mem2 s x y).  Subst x≡canon-q, y≡canon-r.
      r-ev1 : R-instr pre s (div-mod-power-of-two var bits)
                (push-mem (push-mem s x) (from-le-bits (take bits (to-le-bits vv))))
      r-ev1 = subst (λ z → R-instr pre s (div-mod-power-of-two var bits)
                              (push-mem (push-mem s z) (from-le-bits (take bits (to-le-bits vv)))))
                    (sym xeq) r-ev
      r-ev2 : R-instr pre s (div-mod-power-of-two var bits)
                (push-mem (push-mem s x) y)
      r-ev2 = subst (λ z → R-instr pre s (div-mod-power-of-two var bits)
                              (push-mem (push-mem s x) z))
                    (sym yeq) r-ev1
      -- push-mem (push-mem s x) y ≡ push-mem2 s x y propositionally —
      -- their memories differ only by `mem ++ (x ∷ []) ++ (y ∷ [])` vs
      -- `mem ++ (x ∷ y ∷ [])`.  Convert via cong on Preprocessed.
      pm-eq : push-mem (push-mem s x) y ≡ push-mem2 s x y
      pm-eq = cong (λ m → record s { memory = m }) (sym (push-mem2-assoc (Preprocessed.memory s) x y))
      r-ev3 : R-instr pre s (div-mod-power-of-two var bits) (push-mem2 s x y)
      r-ev3 = subst (R-instr pre s (div-mod-power-of-two var bits)) pm-eq r-ev2
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev3

-- ─── declare-pub-input(v) ───────────────────────────────────────────
-- Δmem = 0; pis grows by exactly one cell (wv).
satisfies→R-instr-step {hc} pre s (declare-pub-input v) st _ _ mi pii wc _ _ _ _ (refl , wv , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      v≤len  = subst (suc v Data.Nat.≤_) mi wc
      d      = SynthState.nr-declared-pi st
      -- The dispatcher emits constraints for declare-pub-input via
      -- `single-instr-constraints-with-decl hc (length mem) d`.  But
      -- `circuit-instr hc (declare-pub-input v) st` emits
      -- `bind (preamble-pi-count hc + d) v` at the synth
      -- state, where d = nr-declared-pi st.  Convert.
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (bind (preamble-pi-count hc + d) v ∷ []) sat
      (wv' , _ , lv-post , _) = headᴬ sat-new
      -- Pre-state lookup: mem unchanged so mem-suf = [] gives `mem ++ [] = mem`.
      lv-pre : mem-lookup mem v ≡ just wv'
      lv-pre = subst (λ m → mem-lookup m v ≡ just wv')
                     (++-identityʳ mem) lv-post
      -- Reshape sat-new to use `length mem` for nr-wires.
      sat-shifted = subst (λ k → satisfies-constraints
                                    (bind (preamble-pi-count hc + d) v ∷ [])
                                    (mk-witness (mem ++ []) (Preprocessed.pis s ++ (wv ∷ []))
                                                (comm-rand-of pre)))
                          mi sat-new
      -- Convert to single-instr-constraints-with-decl form (no wire-count
      -- subst needed since bind doesn't depend on n).
      sat-mem = subst (λ m → satisfies-constraints
                                (single-instr-constraints-with-decl hc (length mem) d
                                   (declare-pub-input v))
                                (mk-witness m (Preprocessed.pis s ++ (wv ∷ []))
                                            (comm-rand-of pre)))
                      (++-identityʳ mem) sat-shifted
      -- pi-inv : length (pis s) ≡ preamble-pi-count hc + nr-declared-pi st
      pi-len : length (Preprocessed.pis s) ≡ preamble-pi-count hc + d
      pi-len = pii
      ext≡wv , r-ev = declare-pub-input-bwd
                        {pre = pre} {s = s} {v = v} {wv = wv'} {hc = hc} {d = d}
                        {ext = wv} {rand = comm-rand-of pre}
                        pi-len lv-pre sat-mem
      mem-eq : Preprocessed.memory s ≡ mem ++ []
      mem-eq = sym (++-identityʳ mem)
      s' = record s
             { pis        = Preprocessed.pis s ++ (wv ∷ [])
             ; pub-in-idx = suc (Preprocessed.pub-in-idx s)
             }
  in mem-eq , refl , r-ev

-- ─── assert(c) ────────────────────────────────────────────────────
-- Δmem = 0; pis unchanged.  Needs `is-bit v` (O2-Inv) on the operand.
-- The constraint's `v ≢ 0ᶠ` combines with `is-bit v` (which forces
-- v ∈ {0, 1}) to imply v ≡ 1ᶠ; then `r-assert` fires.
satisfies→R-instr-step {hc} pre s (assert c) st _ _ mi pii wc
                       {bk = bk} o2-inv _ o2-chk _ (refl , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      c≤len  = subst (suc c Data.Nat.≤_) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (non-zero c ∷ []) sat
      (v , lv-post , v≢0) = headᴬ sat-new
      -- Pre-state lookup for `c`.
      lv-pre : mem-lookup mem c ≡ just v
      lv-pre = subst (λ m → mem-lookup m c ≡ just v)
                     (++-identityʳ mem) lv-post
      -- Extract `c ∈ bk` from `O2-check ≡ just bk`.
      c∈bk : c ∈ bk
      c∈bk = o2-check-mem? c bk o2-chk
      -- Apply `o2-known-is-bit`.
      is-bit-v = o2-known-is-bit {bk = bk} o2-inv c∈bk lv-pre
      -- Build sat suitable for `assert-bwd`:  shape  (mem, pis, rand)
      -- with the constraints re-shaped to use `length mem`.
      sat-pis = reshape-nogrow {hc} {comm-rand-of pre} s (assert c) mi sat-new
      r-ev = assert-bwd {pre = pre} {s = s} {c = c} {v = v} {hc = hc}
                         {rand = comm-rand-of pre}
                         lv-pre is-bit-v sat-pis
      mem-eq : Preprocessed.memory s ≡ mem ++ []
      mem-eq = sym (++-identityʳ mem)
      pis-eq : Preprocessed.pis s ≡ Preprocessed.pis s ++ []
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in mem-eq , pis-eq , r-ev

-- ─── not(a) ───────────────────────────────────────────────────────
-- Δmem = 1; pis unchanged.  Needs `is-bit av` (O2-Inv) on the operand
-- `a`.  `not-bwd` packages constraint data into `r-not` once the boolean
-- precondition is satisfied.
satisfies→R-instr-step {hc} pre s (not a) st _ _ mi pii wc
                       {bk = bk} o2-inv _ o2-chk _ ((w , refl) , refl) sat =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      a≤len  = subst (suc a Data.Nat.≤_) mi wc
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (is-zero n a ∷ []) sat
      (av , _ , la-post , _) = headᴬ sat-new
      la-pre    : mem-lookup mem a ≡ just av
      la-pre    = lookup-shrink mem (w ∷ []) a la-post a≤len
      -- Extract `a ∈ bk` from `O2-check ≡ just bk`.
      a∈bk : a ∈ bk
      a∈bk = o2-check-mem? a bk o2-chk
      -- Apply `o2-known-is-bit` to get `is-bit av`.
      is-bit-av = o2-known-is-bit {bk = bk} o2-inv a∈bk la-pre
      -- Re-shape the new-constraints satisfaction for `not-bwd`.
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (not a) (w ∷ []) mi sat-new
      w≡target , r-ev = not-bwd {pre = pre} {s = s} {a = a} {av = av} {v = w} {hc = hc}
                          {rand = comm-rand-of pre}
                          la-pre is-bit-av sat-pis
      r-ev' : R-instr pre s (not a) (push-mem s w)
      r-ev' = subst (λ z → R-instr pre s (not a) (push-mem s z))
                    (sym w≡target) r-ev
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev'

-- ─── less-than(a, b, bits) ───────────────────────────────────────
-- Δmem = 1; pis unchanged.  `less-than-bwd` needs `Fits-in av bits`
-- and `Fits-in bv bits`.  We extract these via O3:
--   O3OK guarantees `lookupᵐ a bm ≡ just ka` with `ka ≤ bits`
--   (and similarly for b).  `o3-known-fits` lifts to
--   `Fits-in av ka`.  `fits-in-mono` then pads to
--   `Fits-in av bits`.
satisfies→R-instr-step {hc} pre s (less-than a b bits) st _ _ {wf2 = wf2} mi pii wc
                       {bm = bm} _ o3-inv _ o3-chk ((w , refl) , refl) sat
  with lookupᵐ a bm in eqa | lookupᵐ b bm in eqb
... | just ka | just kb
  with o3-chk | wc
... | (ka≤b , kb≤b) | (a<n , b<n) =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      a≤len  = subst (suc a Data.Nat.≤_) mi a<n
      b≤len  = subst (suc b Data.Nat.≤_) mi b<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (less-than n a b bits ∷ []) sat
      (av , bv , _ , la-post , lb-post , _) = headᴬ sat-new
      la-pre  = lookup-shrink mem (w ∷ []) a la-post a≤len
      lb-pre  = lookup-shrink mem (w ∷ []) b lb-post b≤len
      -- Extract fits-in av ka via O3-Inv, then pad to fits-in av bits.
      fits-av-ka : Fits-in av ka
      fits-av-ka = o3-known-fits {bm = bm} o3-inv eqa la-pre
      fits-bv-kb : Fits-in bv kb
      fits-bv-kb = o3-known-fits {bm = bm} o3-inv eqb lb-pre
      fits-av : Fits-in av bits
      fits-av = fits-in-mono fits-av-ka ka≤b
      fits-bv : Fits-in bv bits
      fits-bv = fits-in-mono fits-bv-kb kb≤b
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (less-than a b bits) (w ∷ []) mi sat-new
      w≡target , r-ev = less-than-bwd
                          {pre = pre} {s = s} {a = a} {b = b} {bits = bits}
                          {av = av} {bv = bv} {v = w} {hc = hc}
                          {rand = comm-rand-of pre}
                          la-pre lb-pre wf2 fits-av fits-bv sat-pis
      r-ev' : R-instr pre s (less-than a b bits) (push-mem s w)
      r-ev' = subst (λ z → R-instr pre s (less-than a b bits) (push-mem s z))
                    (sym w≡target) r-ev
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev'

-- ─── reconstitute-field(d, m, bits) ─────────────────────────────────
-- Δmem = 1; pis unchanged.  `reconstitute-field-bwd` needs
-- `BitsInField (mv-bits ++ dv-bits)`.  We extract it via O3:
--   O3-check guarantees `lookupᵐ d bm ≡ just kd ∧ kd ≤ᵇ (FR-BITS ∸ bits ∸ 1)`
--   and `lookupᵐ m bm ≡ just km ∧ km ≤ᵇ bits`.  `o3-known-fits` gives
--   `fits-in dv kd` and `fits-in mv km`; `fits-in-mono` pads them to
--   the bounds expected by `bits-in-field-from-strict-bound`, which
--   then supplies the `BitsInField` premise.
satisfies→R-instr-step {hc} pre s (reconstitute-field d m bits) st _ _ {wf2 = wf2} mi pii wc
                       {bm = bm} _ o3-inv _ o3-chk ((w , refl) , refl) sat
  with lookupᵐ d bm in eqd | lookupᵐ m bm in eqm
... | just kd | just km
  with o3-chk | wc
... | (kd≤ , km≤) | (d<n , m<n) =
  let mem    = Preprocessed.memory s
      n      = SynthState.nr-wires st
      d≤len  = subst (suc d Data.Nat.≤_) mi d<n
      m≤len  = subst (suc m Data.Nat.≤_) mi m<n
      _ , sat-new = satisfies-constraints-split
                      (SynthState.constraints st)
                      (reconstitute n d m bits ∷ []) sat
      (dv , mv , _ , ld-post , lm-post , _) = headᴬ sat-new
      ld-pre  = lookup-shrink mem (w ∷ []) d ld-post d≤len
      lm-pre  = lookup-shrink mem (w ∷ []) m lm-post m≤len
      -- Extract fits-in dv kd and fits-in mv km via O3-Inv.
      fits-dv-kd : Fits-in dv kd
      fits-dv-kd = o3-known-fits {bm = bm} o3-inv eqd ld-pre
      fits-mv-km : Fits-in mv km
      fits-mv-km = o3-known-fits {bm = bm} o3-inv eqm lm-pre
      -- Pad to the bounds required by `bits-in-field-from-strict-bound`:
      --   fits-in mv bits        (from km ≤ bits)
      --   fits-in dv (FR-BITS ∸ bits ∸ 1)   (from kd ≤ FR-BITS ∸ bits ∸ 1).
      fits-mv : Fits-in mv bits
      fits-mv = fits-in-mono fits-mv-km km≤
      fits-dv : Fits-in dv (FR-BITS ∸ bits ∸ 1)
      fits-dv = fits-in-mono fits-dv-kd kd≤
      -- BitsInField premise.
      in-field : BitsInField
                   (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))
      in-field = bits-in-field-from-strict-bound {dv = dv} {mv = mv} {n = bits}
                   fits-mv fits-dv
      sat-pis = reshape-core {hc} {comm-rand-of pre} s (reconstitute-field d m bits) (w ∷ []) mi sat-new
      w≡target , r-ev = reconstitute-field-bwd
                          {pre = pre} {s = s} {d = d} {m = m} {bits = bits}
                          {dv = dv} {mv = mv} {v = w} {hc = hc}
                          {rand = comm-rand-of pre}
                          ld-pre lm-pre (proj₁ wf2) (proj₂ wf2) in-field sat-pis
      r-ev' : R-instr pre s (reconstitute-field d m bits) (push-mem s w)
      r-ev' = subst (λ z → R-instr pre s (reconstitute-field d m bits) (push-mem s z))
                    (sym w≡target) r-ev
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in refl , pis-eq , r-ev'

-- ─── output(v) ────────────────────────────────────────────────────
-- Δmem = 0; pis unchanged.  Emits no constraints (the wire index is
-- recorded for the comm-commitment constraint emitted at end of synthesis
-- if has-comm).  Operational side data: `mem-lookup mem v ≡ just val`
-- — the value pushed onto `outputs`.
satisfies→R-instr-step {hc} pre s (output v) st _ _ mi pii wc _ _ _ _ ((val , lv-pre) , refl , refl) sat =
  let mem = Preprocessed.memory s
      s'  = record s { outputs = Preprocessed.outputs s ++ (val ∷ []) }
      r-ev : R-instr pre s (output v) s'
      r-ev = r-output {pre = pre} {s = s} {var = v} {v = val} lv-pre
      mem-eq : Preprocessed.memory s' ≡ mem ++ []
      mem-eq = sym (++-identityʳ mem)
      pis-eq : Preprocessed.pis s' ≡ Preprocessed.pis s ++ []
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in mem-eq , pis-eq , r-ev

-- ─── pi-skip(g, count) ───────────────────────────────────────────
-- Δmem = 0; pis unchanged.  Emits no constraints (the pi-skip group is
-- pure side data for the verifier).  Operational side data: the
-- guard's evaluation and, if active, the transcript-prefix match.
satisfies→R-instr-step {hc} pre s (pi-skip g count) st _ _ mi pii wc _ _ _ _
                       (refl , refl , (true , ev-guard , prefix-match)) sat =
  let s' = record s { pi-skips = Preprocessed.pi-skips s ++ (nothing ∷ []) }
      r-ev : R-instr pre s (pi-skip g count) s'
      r-ev = r-pi-skip-active {pre = pre} {s = s} {guard = g} {count = count}
                              ev-guard prefix-match
      mem-eq = sym (++-identityʳ (Preprocessed.memory s))
      pis-eq = sym (++-identityʳ (Preprocessed.pis    s))
  in mem-eq , pis-eq , r-ev
satisfies→R-instr-step {hc} pre s (pi-skip g count) st _ _ mi pii wc _ _ _ _
                       (refl , refl , (false , ev-guard , _)) sat =
  let s' = record s
             { pi-skips    = Preprocessed.pi-skips s ++ (just count ∷ [])
             ; pub-in-idx  = Preprocessed.pub-in-idx s ∸ count
             }
      r-ev : R-instr pre s (pi-skip g count) s'
      r-ev = r-pi-skip-inactive {pre = pre} {s = s} {guard = g} {count = count}
                                ev-guard
      mem-eq = sym (++-identityʳ (Preprocessed.memory s))
      pis-eq = sym (++-identityʳ (Preprocessed.pis    s))
  in mem-eq , pis-eq , r-ev

-- ─── public-input(g) ─────────────────────────────────────────────
-- Δmem = 1; pis unchanged.  Either emits no constraints (`g = nothing`)
-- or a single guard-disj constraint (`g = just _`).  Operational side
-- data fixes whether active (consume from `pub-out-rem`) or inactive
-- (push 0ᶠ).  The witness's memory cell `w` is reconciled to either
-- the consumed value (active) or 0ᶠ (inactive) via the side data.
satisfies→R-instr-step {hc} pre s (public-input g) st _ _ mi pii wc _ _ _ _
                       (w , refl , refl , (true , ev-guard , (s₁ , consume-eq))) sat =
  let s' = record s₁ { memory = Preprocessed.memory s₁ ++ (w ∷ []) }
      r-ev : R-instr pre s (public-input g) s'
      r-ev = r-public-input-active {pre = pre} {s = s} {guard = g}
                                   {v = w} {s₁ = s₁}
                                   ev-guard consume-eq
      -- memory s₁ ≡ memory s definitionally (consume-pub-out only
      -- touches pub-out-rem); but Agda needs a proof — pin it via the
      -- `consume-eq` premise's structure.
      mem-eq : Preprocessed.memory s' ≡ Preprocessed.memory s ++ (w ∷ [])
      mem-eq = cong (λ m → m ++ (w ∷ []))
                    (consume-pub-out-mem s consume-eq)
      pis-eq : Preprocessed.pis s' ≡ Preprocessed.pis s ++ []
      pis-eq = trans (consume-pub-out-pis s consume-eq)
                     (sym (++-identityʳ (Preprocessed.pis s)))
  in mem-eq , pis-eq , r-ev
satisfies→R-instr-step {hc} pre s (public-input g) st _ _ mi pii wc _ _ _ _
                       (w , refl , refl , (false , ev-guard , w≡0)) sat =
  let s' = record s { memory = Preprocessed.memory s ++ (w ∷ []) }
      r-ev₀ : R-instr pre s (public-input g) (record s { memory = Preprocessed.memory s ++ (0ᶠ ∷ []) })
      r-ev₀ = r-public-input-inactive {pre = pre} {s = s} {guard = g} ev-guard
      r-ev : R-instr pre s (public-input g) s'
      r-ev = subst (λ z → R-instr pre s (public-input g)
                              (record s { memory = Preprocessed.memory s ++ (z ∷ []) }))
                   (sym w≡0) r-ev₀
      mem-eq : Preprocessed.memory s' ≡ Preprocessed.memory s ++ (w ∷ [])
      mem-eq = refl
      pis-eq : Preprocessed.pis s' ≡ Preprocessed.pis s ++ []
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in mem-eq , pis-eq , r-ev

-- ─── private-input(g) ────────────────────────────────────────────
-- Symmetric to public-input but consumes from `priv-rem`.
satisfies→R-instr-step {hc} pre s (private-input g) st _ _ mi pii wc _ _ _ _
                       (w , refl , refl , (true , ev-guard , (s₁ , consume-eq))) sat =
  let s' = record s₁ { memory = Preprocessed.memory s₁ ++ (w ∷ []) }
      r-ev : R-instr pre s (private-input g) s'
      r-ev = r-private-input-active {pre = pre} {s = s} {guard = g}
                                    {v = w} {s₁ = s₁}
                                    ev-guard consume-eq
      mem-eq : Preprocessed.memory s' ≡ Preprocessed.memory s ++ (w ∷ [])
      mem-eq = cong (λ m → m ++ (w ∷ []))
                    (consume-priv-mem s consume-eq)
      pis-eq : Preprocessed.pis s' ≡ Preprocessed.pis s ++ []
      pis-eq = trans (consume-priv-pis s consume-eq)
                     (sym (++-identityʳ (Preprocessed.pis s)))
  in mem-eq , pis-eq , r-ev
satisfies→R-instr-step {hc} pre s (private-input g) st _ _ mi pii wc _ _ _ _
                       (w , refl , refl , (false , ev-guard , w≡0)) sat =
  let s' = record s { memory = Preprocessed.memory s ++ (w ∷ []) }
      r-ev₀ : R-instr pre s (private-input g) (record s { memory = Preprocessed.memory s ++ (0ᶠ ∷ []) })
      r-ev₀ = r-private-input-inactive {pre = pre} {s = s} {guard = g} ev-guard
      r-ev : R-instr pre s (private-input g) s'
      r-ev = subst (λ z → R-instr pre s (private-input g)
                              (record s { memory = Preprocessed.memory s ++ (z ∷ []) }))
                   (sym w≡0) r-ev₀
      mem-eq : Preprocessed.memory s' ≡ Preprocessed.memory s ++ (w ∷ [])
      mem-eq = refl
      pis-eq : Preprocessed.pis s' ≡ Preprocessed.pis s ++ []
      pis-eq = sym (++-identityʳ (Preprocessed.pis s))
  in mem-eq , pis-eq , r-ev


------------------------------------------------------------------------
-- D2 helpers
--
-- Two structural facts about `circuit-instr` / `circuit-instrs`:
--   (a) one step extends `constraints` by appending a (possibly empty) list;
--   (b) iterated steps extend `constraints` by an appended list as well.
--
-- These are pure computations: (a) follows by case analysis on `i`,
-- (b) by induction.  Used by D2 to peel off the head step's constraints
-- from the accumulated constraints list.
------------------------------------------------------------------------

private
  -- Per-instruction constraint delta.  Symbolically, the list of constraints
  -- that `circuit-instr hc i st` appends to `constraints st`.  We use the
  -- empty list for instructions that emit no constraints
  -- (output, pi-skip, public-input nothing, private-input nothing) and
  -- a one-element list for the rest.  `declare-pub-input` is the only
  -- instruction whose new constraint's content depends on `nr-declared-pi
  -- st` (the PI-entry index), so the delta is parameterised by that
  -- field as well as `nr-wires st`.
  instr-new-constraints : Bool → SynthState → Instruction → List Constraint
  instr-new-constraints _  st (assert c)               = non-zero c ∷ []
  instr-new-constraints _  st (cond-select b a c)      =
    select (SynthState.nr-wires st) b a c ∷ []
  instr-new-constraints _  st (constrain-bits v bits)  = in-range v bits ∷ []
  instr-new-constraints _  st (constrain-eq a b)       = (wire a ≑ wire b) ∷ []
  instr-new-constraints _  st (constrain-to-boolean v) = boolean v ∷ []
  instr-new-constraints _  st (copy v)                 =
    (wire (SynthState.nr-wires st) ≑ wire v) ∷ []
  instr-new-constraints hc st (declare-pub-input v)    =
    bind (preamble-pi-count hc + SynthState.nr-declared-pi st) v ∷ []
  instr-new-constraints _  st (pi-skip _ _)            = []
  instr-new-constraints _  st (ec-add ax ay bx by)     =
    ec-add (SynthState.nr-wires st) (suc (SynthState.nr-wires st)) ax ay bx by ∷ []
  instr-new-constraints _  st (ec-mul ax ay s)         =
    ec-mul (SynthState.nr-wires st) (suc (SynthState.nr-wires st)) ax ay s ∷ []
  instr-new-constraints _  st (ec-mul-generator s)     =
    ec-gen (SynthState.nr-wires st) (suc (SynthState.nr-wires st)) s ∷ []
  instr-new-constraints _  st (hash-to-curve inputs)   =
    h2c (SynthState.nr-wires st) (suc (SynthState.nr-wires st)) inputs ∷ []
  instr-new-constraints _  st (load-imm imm)           =
    (wire (SynthState.nr-wires st) ≑ con imm) ∷ []
  instr-new-constraints _  st (div-mod-power-of-two v bits) =
    div-mod (SynthState.nr-wires st) (suc (SynthState.nr-wires st)) v bits ∷ []
  instr-new-constraints _  st (reconstitute-field d m bits) =
    reconstitute (SynthState.nr-wires st) d m bits ∷ []
  instr-new-constraints _  st (output v)               = []
  instr-new-constraints _  st (transient-hash inputs)  =
    poseidon (SynthState.nr-wires st) inputs ∷ []
  instr-new-constraints _  st (persistent-hash α inputs) =
    sha256 (SynthState.nr-wires st) (suc (SynthState.nr-wires st)) α inputs ∷ []
  instr-new-constraints _  st (test-eq a b)            =
    test-eq (SynthState.nr-wires st) a b ∷ []
  instr-new-constraints _  st (add a b)                =
    (wire (SynthState.nr-wires st) ≑ wire a ⊕ wire b) ∷ []
  instr-new-constraints _  st (mul a b)                =
    (wire (SynthState.nr-wires st) ≑ wire a ⊗ wire b) ∷ []
  instr-new-constraints _  st (neg a)                  =
    (wire (SynthState.nr-wires st) ≑ ⊝ wire a) ∷ []
  instr-new-constraints _  st (not a)                  =
    is-zero (SynthState.nr-wires st) a ∷ []
  instr-new-constraints _  st (less-than a b bits)     =
    less-than (SynthState.nr-wires st) a b bits ∷ []
  instr-new-constraints _  st (public-input nothing)   = []
  instr-new-constraints _  st (public-input (just g))  =
    guard-disj (SynthState.nr-wires st) g ∷ []
  instr-new-constraints _  st (private-input nothing)  = []
  instr-new-constraints _  st (private-input (just g)) =
    guard-disj (SynthState.nr-wires st) g ∷ []

  -- Decomposition: `constraints (circuit-instr hc i st) ≡ constraints st ++
  -- instr-new-constraints hc st i`.  By case analysis on `i`.  The
  -- definition of `circuit-instr` makes each case definitionally
  -- equal to its push-constraint form, so each case proof is `refl`.
  -- We use a single helper lemma `++-cl` that produces `xs ∷ʳ x ≡ xs ++ (x ∷ [])`
  -- (which is already definitional).
  constraints-after-instr-eq : ∀ {hc} (i : Instruction) (st : SynthState)
    → SynthState.constraints (circuit-instr hc i st)
      ≡ SynthState.constraints st ++ instr-new-constraints hc st i
  constraints-after-instr-eq (assert c) st                = refl
  constraints-after-instr-eq (cond-select b a c) st       = refl
  constraints-after-instr-eq (constrain-bits v bits) st   = refl
  constraints-after-instr-eq (constrain-eq a b) st        = refl
  constraints-after-instr-eq (constrain-to-boolean v) st  = refl
  constraints-after-instr-eq (copy v) st                  = refl
  constraints-after-instr-eq (declare-pub-input v) st     = refl
  constraints-after-instr-eq (pi-skip g count) st         = sym (++-identityʳ _)
  constraints-after-instr-eq (ec-add ax ay bx by) st      = refl
  constraints-after-instr-eq (ec-mul ax ay s) st          = refl
  constraints-after-instr-eq (ec-mul-generator s) st      = refl
  constraints-after-instr-eq (hash-to-curve inputs) st    = refl
  constraints-after-instr-eq (load-imm imm) st            = refl
  constraints-after-instr-eq (div-mod-power-of-two v bits) st = refl
  constraints-after-instr-eq (reconstitute-field d m bits) st = refl
  constraints-after-instr-eq (output v) st                = sym (++-identityʳ _)
  constraints-after-instr-eq (transient-hash inputs) st   = refl
  constraints-after-instr-eq (persistent-hash α inputs) st = refl
  constraints-after-instr-eq (test-eq a b) st             = refl
  constraints-after-instr-eq (add a b) st                 = refl
  constraints-after-instr-eq (mul a b) st                 = refl
  constraints-after-instr-eq (neg a) st                   = refl
  constraints-after-instr-eq (not a) st                   = refl
  constraints-after-instr-eq (less-than a b bits) st      = refl
  constraints-after-instr-eq (public-input nothing) st    = sym (++-identityʳ _)
  constraints-after-instr-eq (public-input (just g)) st   = refl
  constraints-after-instr-eq (private-input nothing) st   = sym (++-identityʳ _)
  constraints-after-instr-eq (private-input (just g)) st  = refl

  -- Iterated decomposition.  `constraints (circuit-instrs hc is st) ≡
  -- constraints st ++ <tail>` for some explicit tail computed by
  -- `instrs-new-constraints`.  We do not need the explicit form of `tail`;
  -- just its existence is enough for the satisfies-split.
  constraints-after-instrs-extends
    : ∀ {hc} (is : List Instruction) (st : SynthState)
    → Σ (List Constraint) λ tail →
        SynthState.constraints (circuit-instrs hc is st)
          ≡ SynthState.constraints st ++ tail
  constraints-after-instrs-extends []       st =
    [] , sym (++-identityʳ _)
  constraints-after-instrs-extends {hc} (i ∷ is) st =
    let head-new      = instr-new-constraints hc st i
        st₁           = circuit-instr hc i st
        head-eq       = constraints-after-instr-eq {hc} i st
        tail , tl-eq  = constraints-after-instrs-extends {hc} is st₁
        combined-eq   : SynthState.constraints (circuit-instrs hc is st₁)
                      ≡ SynthState.constraints st ++ (head-new ++ tail)
        combined-eq   = trans tl-eq
                          (trans (cong (_++ tail) head-eq)
                                 (++-assoc (SynthState.constraints st) head-new tail))
    in head-new ++ tail , combined-eq

------------------------------------------------------------------------
-- Helpers for D2's cons-case discharge.
------------------------------------------------------------------------

private
  -- nr-wires after a single instruction = nr-wires before + Δmem.
  -- Proved by case analysis on the instruction.  Each case is `refl`
  -- because `circuit-instr` updates `nr-wires` as exactly
  -- `nr-wires st + Δmem i`, but spelled with explicit `+1`/`+2`
  -- shorthands rather than `+ Δmem i`.  We bridge via `+1-suc`/`+2-ss`.
  nr-wires-step : ∀ {hc} (i : Instruction) (st : SynthState)
    → SynthState.nr-wires (circuit-instr hc i st)
      ≡ SynthState.nr-wires st + Δmem i
  nr-wires-step (assert _)                 st = sym (+-identityʳ _)
  nr-wires-step (constrain-bits _ _)       st = sym (+-identityʳ _)
  nr-wires-step (constrain-eq _ _)         st = sym (+-identityʳ _)
  nr-wires-step (constrain-to-boolean _)   st = sym (+-identityʳ _)
  nr-wires-step (declare-pub-input _)      st = sym (+-identityʳ _)
  nr-wires-step (pi-skip _ _)              st = sym (+-identityʳ _)
  nr-wires-step (output _)                 st = sym (+-identityʳ _)
  nr-wires-step (cond-select _ _ _)        st = refl
  nr-wires-step (copy _)                   st = refl
  nr-wires-step (load-imm _)               st = refl
  nr-wires-step (reconstitute-field _ _ _) st = refl
  nr-wires-step (transient-hash _)         st = refl
  nr-wires-step (test-eq _ _)              st = refl
  nr-wires-step (add _ _)                  st = refl
  nr-wires-step (mul _ _)                  st = refl
  nr-wires-step (neg _)                    st = refl
  nr-wires-step (not _)                    st = refl
  nr-wires-step (less-than _ _ _)          st = refl
  nr-wires-step (public-input nothing)     st = refl
  nr-wires-step (public-input (just _))    st = refl
  nr-wires-step (private-input nothing)    st = refl
  nr-wires-step (private-input (just _))   st = refl
  nr-wires-step (ec-add _ _ _ _)           st = refl
  nr-wires-step (ec-mul _ _ _)             st = refl
  nr-wires-step (ec-mul-generator _)       st = refl
  nr-wires-step (hash-to-curve _)          st = refl
  nr-wires-step (persistent-hash _ _)      st = refl
  nr-wires-step (div-mod-power-of-two _ _) st = refl

  -- O2-check extraction.  From `O2-step i (n , bk) ≡ just acc'`
  -- recover `O2-check i bk ≡ just bk`.  Mechanical case analysis on `i`.
  --
  -- The three obligation cases (`assert`, `not`, `cond-select`) split
  -- on `c ∈? bk`:  the `yes` branch gives `O2-check i bk ≡ just bk`
  -- directly; the `no` branch makes `O2-step ≡ nothing`, contradicting
  -- `eq`.  All other 23 cases unfold to `O2-check i bk = just bk` and
  -- the result is `refl`.
  o2-check-from-step
    : ∀ (i : Instruction) {n : ℕ} (bk : IndexSet) {acc'}
    → O2-step i (n , bk) ≡ just acc'
    → O2-check i bk ≡ just bk
  o2-check-from-step (assert c) {n} bk eq with c ∈? bk
  ... | yes _ = refl
  ... | no  _ with eq
  ...            | ()
  o2-check-from-step (not a) {n} bk eq with a ∈? bk
  ... | yes _ = refl
  ... | no  _ with eq
  ...            | ()
  o2-check-from-step (cond-select b _ _) {n} bk eq with b ∈? bk
  ... | yes _ = refl
  ... | no  _ with eq
  ...            | ()
  o2-check-from-step (constrain-bits _ _)     bk eq = refl
  o2-check-from-step (constrain-eq _ _)       bk eq = refl
  o2-check-from-step (constrain-to-boolean _) bk eq = refl
  o2-check-from-step (copy _)                 bk eq = refl
  o2-check-from-step (declare-pub-input _)    bk eq = refl
  o2-check-from-step (pi-skip nothing _)      bk eq = refl
  o2-check-from-step (pi-skip (just g) _)     bk eq with g ∈? bk
  ... | yes _ = refl
  ... | no  _ with eq
  ...            | ()
  o2-check-from-step (ec-add _ _ _ _)         bk eq = refl
  o2-check-from-step (ec-mul _ _ _)           bk eq = refl
  o2-check-from-step (ec-mul-generator _)     bk eq = refl
  o2-check-from-step (hash-to-curve _)        bk eq = refl
  o2-check-from-step (load-imm _)             bk eq = refl
  o2-check-from-step (div-mod-power-of-two _ _) bk eq = refl
  o2-check-from-step (reconstitute-field _ _ _) bk eq = refl
  o2-check-from-step (output _)               bk eq = refl
  o2-check-from-step (transient-hash _)       bk eq = refl
  o2-check-from-step (persistent-hash _ _)    bk eq = refl
  o2-check-from-step (test-eq _ _)            bk eq = refl
  o2-check-from-step (add _ _)                bk eq = refl
  o2-check-from-step (mul _ _)                bk eq = refl
  o2-check-from-step (neg _)                  bk eq = refl
  o2-check-from-step (less-than _ _ _)        bk eq = refl
  o2-check-from-step (public-input _)         bk eq = refl
  o2-check-from-step (private-input _)        bk eq = refl

  -- O3-obligation extraction:  from `O3-step i (n , bm) ≡ just acc'`
  -- recover the `O3OK i bm` witness.  `O3-step` branches on `o3OK? i bm`.
  o3-check-from-step
    : ∀ (i : Instruction) {n : ℕ} (bm : PartialMap) {acc'}
    → O3-step i (n , bm) ≡ just acc'
    → O3OK i bm
  o3-check-from-step i bm eq with o3OK? i bm | eq
  ... | yes ok | _  = ok
  ... | no  _  | ()

  -- pis-suf = wv ∷ [] after a declare; given `length pis-suf ≡ 1`,
  -- refine the suffix into its canonical shape and account for the +1
  -- in nr-declared-pi.
  pi-inv-add-1-declare : ∀ (hc : Bool) (st : SynthState) (pis : List Fr)
    → (pis-suf : List Fr)
    → length pis-suf ≡ 1
    → length pis ≡ preamble-pi-count hc + SynthState.nr-declared-pi st
    → length (pis ++ pis-suf)
      ≡ preamble-pi-count hc + suc (SynthState.nr-declared-pi st)
  pi-inv-add-1-declare _  _  _   []              ()   _
  pi-inv-add-1-declare hc st pis (wv ∷ [])       refl pii =
    trans (length-++-1 pis wv)
          (trans (cong suc pii)
                 (sym (+-suc (preamble-pi-count hc)
                              (SynthState.nr-declared-pi st))))
  pi-inv-add-1-declare _  _  _   (_ ∷ _ ∷ _)     ()   _

  ------------------------------------------------------------------------
  -- The new constraints emitted by `circuit-instr hc i st` fit
  -- in the post-state pis (`length pis-suf` cells extension).
  --
  -- For every instruction except `declare-pub-input`, the new constraints
  -- mention no pis (so `constraints-pis-fit` is trivially `true`).
  -- For `declare-pub-input v`, the only new constraint is
  -- `bind (preamble-pi-count hc + nr-declared-pi st) v`;
  -- with `pi-inv`, the entry is `≡ length (pis s)`, hence
  -- `entry <ᵇ length (pis s ++ pis-suf)` once `length pis-suf ≡ 1`.
  ------------------------------------------------------------------------

  -- Δpis is 1 for `declare-pub-input` and 0 for every other instruction.
  Δpis-of : Instruction → ℕ
  Δpis-of i = shape-Δpis (shape-of (shapeView i))

  -- Auxiliary:  if `length pis-s ≡ entry` and `length pis-suf ≡ 1`,
  -- then `length (pis-s ++ pis-suf) ≡ suc entry`.
  pis-len-succ
    : ∀ (pis-s : List Fr) (pis-suf : List Fr) (entry : ℕ)
    → length pis-suf ≡ 1
    → length pis-s ≡ entry
    → length (pis-s ++ pis-suf) ≡ suc entry
  pis-len-succ _      []              _     ()   _
  pis-len-succ pis-s  (w ∷ [])        entry refl eq =
    trans (length-++-1 pis-s w) (cong suc eq)
  pis-len-succ _      (_ ∷ _ ∷ _)     _     ()   _

  constraints-pis-fit-instr
    : ∀ (hc : Bool) (st : SynthState) (s : Preprocessed) (i : Instruction)
        (pis-suf : List Fr)
    → length pis-suf ≡ Δpis-of i
    → length (Preprocessed.pis s)
        ≡ preamble-pi-count hc + SynthState.nr-declared-pi st
    → constraints-pis-fit (instr-new-constraints hc st i)
                       (length (Preprocessed.pis s ++ pis-suf))
  -- All but `declare-pub-input` emit constraints with no pi references.
  constraints-pis-fit-instr hc st s (assert c)               _ _ _ = _
  constraints-pis-fit-instr hc st s (cond-select _ _ _)      _ _ _ = _
  constraints-pis-fit-instr hc st s (constrain-bits _ _)     _ _ _ = _
  constraints-pis-fit-instr hc st s (constrain-eq _ _)       _ _ _ = _
  constraints-pis-fit-instr hc st s (constrain-to-boolean _) _ _ _ = _
  constraints-pis-fit-instr hc st s (copy _)                 _ _ _ = _
  constraints-pis-fit-instr hc st s (pi-skip _ _)            _ _ _ = _
  constraints-pis-fit-instr hc st s (output _)               _ _ _ = _
  constraints-pis-fit-instr hc st s (ec-add _ _ _ _)         _ _ _ = _
  constraints-pis-fit-instr hc st s (ec-mul _ _ _)           _ _ _ = _
  constraints-pis-fit-instr hc st s (ec-mul-generator _)     _ _ _ = _
  constraints-pis-fit-instr hc st s (hash-to-curve _)        _ _ _ = _
  constraints-pis-fit-instr hc st s (load-imm _)             _ _ _ = _
  constraints-pis-fit-instr hc st s (div-mod-power-of-two _ _) _ _ _ = _
  constraints-pis-fit-instr hc st s (reconstitute-field _ _ _) _ _ _ = _
  constraints-pis-fit-instr hc st s (transient-hash _)       _ _ _ = _
  constraints-pis-fit-instr hc st s (persistent-hash _ _)    _ _ _ = _
  constraints-pis-fit-instr hc st s (test-eq _ _)            _ _ _ = _
  constraints-pis-fit-instr hc st s (add _ _)                _ _ _ = _
  constraints-pis-fit-instr hc st s (mul _ _)                _ _ _ = _
  constraints-pis-fit-instr hc st s (neg _)                  _ _ _ = _
  constraints-pis-fit-instr hc st s (not _)                  _ _ _ = _
  constraints-pis-fit-instr hc st s (less-than _ _ _)        _ _ _ = _
  constraints-pis-fit-instr hc st s (public-input nothing)   _ _ _ = _
  constraints-pis-fit-instr hc st s (public-input (just _))  _ _ _ = _
  constraints-pis-fit-instr hc st s (private-input nothing)  _ _ _ = _
  constraints-pis-fit-instr hc st s (private-input (just _)) _ _ _ = _
  -- declare-pub-input: the entry index is `preamble-pi-count hc +
  -- nr-declared-pi st`.  By `pi-inv`, this equals `length (pis s)`.
  -- The post-state pis has length `length (pis s) + 1 = suc (length pis s)`.
  -- So `entry < length (pis s ++ pis-suf) = entry < suc entry`.
  constraints-pis-fit-instr hc st s (declare-pub-input v) pis-suf lh pii =
    let entry  = preamble-pi-count hc + SynthState.nr-declared-pi st
        pis-s  = Preprocessed.pis s
        len-eq : length (pis-s ++ pis-suf) ≡ suc entry
        len-eq = pis-len-succ pis-s pis-suf entry lh pii
        ent< : entry < length (pis-s ++ pis-suf)
        ent< = subst (λ m → entry < m) (sym len-eq) Data.Nat.Properties.≤-refl
    in ent< , tt

  ------------------------------------------------------------------------
  -- mem-inv-next / pi-inv-next.
  --
  -- For each of the 26 instructions, given:
  --   • the shape hypotheses on `mem-suf` and `pis-suf`
  --     (`length mem-suf ≡ Δmem i`, `length pis-suf ≡ Δpis-of i`),
  --   • the pre-state invariants (`mem-inv s st`, `pi-inv hc s st`),
  -- the post-state invariants hold for
  -- `next-state-from-osd i pre s mem-suf pis-suf sd` and
  -- `circuit-instr hc i st`.
  --
  -- Each case mirrors the corresponding clause of `next-state-from-osd`.
  -- Ill-shaped combinations are absurd by the shape hypotheses.
  ------------------------------------------------------------------------

  mem-inv-next
    : ∀ {hc} (i : Instruction) (pre : ProofPreimage)
        (s : Preprocessed) (st : SynthState)
        (mem-suf pis-suf : List Fr)
        (sd : op-side-data i pre s mem-suf pis-suf)
    → mem-inv s st
    → length mem-suf ≡ Δmem i
    → length pis-suf ≡ Δpis-of i
    → mem-inv (next-state-from-osd i pre s mem-suf pis-suf sd)
              (circuit-instr hc i st)
  -- Δmem = 1, "push-mem" cases.
  mem-inv-next (add _ _)             _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (mul _ _)             _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (neg _)               _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (copy _)              _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (load-imm _)          _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (test-eq _ _)         _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (transient-hash _)    _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (cond-select _ _ _)   _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (not _)               _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (less-than _ _ _)     _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (reconstitute-field _ _ _) _ s st (w ∷ []) [] ((_ , refl) , _) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  -- Δmem = 2, "push-mem2" cases.
  mem-inv-next (ec-add _ _ _ _)      _ s st (x ∷ y ∷ []) [] ((_ , _ , refl) , _) mi refl refl =
    mem-inv-step-2 {st = st} {mem = Preprocessed.memory s} {x = x} {y = y} mi
  mem-inv-next (ec-mul _ _ _)        _ s st (x ∷ y ∷ []) [] ((_ , _ , refl) , _) mi refl refl =
    mem-inv-step-2 {st = st} {mem = Preprocessed.memory s} {x = x} {y = y} mi
  mem-inv-next (ec-mul-generator _)  _ s st (x ∷ y ∷ []) [] ((_ , _ , refl) , _) mi refl refl =
    mem-inv-step-2 {st = st} {mem = Preprocessed.memory s} {x = x} {y = y} mi
  mem-inv-next (hash-to-curve _)     _ s st (x ∷ y ∷ []) [] ((_ , _ , refl) , _) mi refl refl =
    mem-inv-step-2 {st = st} {mem = Preprocessed.memory s} {x = x} {y = y} mi
  mem-inv-next (persistent-hash _ _) _ s st (x ∷ y ∷ []) [] ((_ , _ , refl) , _) mi refl refl =
    mem-inv-step-2 {st = st} {mem = Preprocessed.memory s} {x = x} {y = y} mi
  mem-inv-next (div-mod-power-of-two _ _) _ s st (x ∷ y ∷ []) [] ((_ , _ , refl) , _) mi refl refl =
    mem-inv-step-2 {st = st} {mem = Preprocessed.memory s} {x = x} {y = y} mi
  -- Δmem = 0, state unchanged.  `circuit-instr` for these does not
  -- bump `nr-wires`; the `next-state` for these has memory unchanged.
  mem-inv-next (constrain-eq _ _)      _ s st [] [] _ mi refl refl = mi
  mem-inv-next (constrain-bits _ _)    _ s st [] [] _ mi refl refl = mi
  mem-inv-next (constrain-to-boolean _) _ s st [] [] _ mi refl refl = mi
  mem-inv-next (assert _)              _ s st [] [] _ mi refl refl = mi
  -- declare-pub-input:  Δmem = 0; pis += wv (handled via record-update;
  -- memory unchanged).
  mem-inv-next (declare-pub-input _) _ s st [] (wv ∷ []) _ mi refl refl = mi
  -- output v:  Δmem = 0; memory unchanged.
  mem-inv-next (output _)            _ s st [] [] _ mi refl refl = mi
  -- pi-skip:  Δmem = 0; memory unchanged (regardless of `active`).
  mem-inv-next (pi-skip _ _)         _ s st [] [] (_ , _ , (true  , _ , _)) mi refl refl = mi
  mem-inv-next (pi-skip _ _)         _ s st [] [] (_ , _ , (false , _ , _)) mi refl refl = mi
  -- public-input / private-input:  Δmem = 1.  Active branch:
  --   next-state = record s₁ { memory = memory s₁ ++ (w ∷ []) }
  -- with `consume-pub-out s ≡ just (w, s₁)`, so `memory s₁ ≡ memory s`.
  -- Inactive branch: memory ++ (w ∷ []) directly.
  mem-inv-next (public-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) mi refl refl =
    let mem-s₁≡mem-s = consume-pub-out-mem s {v = w} {s′ = s₁} cp
        step₁ : SynthState.nr-wires st + 1 ≡ length (Preprocessed.memory s ++ (w ∷ []))
        step₁ = mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
    in subst (λ m → SynthState.nr-wires st + 1 ≡ length (m ++ (w ∷ [])))
             (sym mem-s₁≡mem-s) step₁
  mem-inv-next (public-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) mi refl refl =
    let mem-s₁≡mem-s = consume-pub-out-mem s {v = w} {s′ = s₁} cp
        step₁ : SynthState.nr-wires st + 1 ≡ length (Preprocessed.memory s ++ (w ∷ []))
        step₁ = mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
    in subst (λ m → SynthState.nr-wires st + 1 ≡ length (m ++ (w ∷ [])))
             (sym mem-s₁≡mem-s) step₁
  mem-inv-next (public-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (public-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (private-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) mi refl refl =
    let mem-s₁≡mem-s = consume-priv-mem s {v = w} {s′ = s₁} cp
        step₁ : SynthState.nr-wires st + 1 ≡ length (Preprocessed.memory s ++ (w ∷ []))
        step₁ = mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
    in subst (λ m → SynthState.nr-wires st + 1 ≡ length (m ++ (w ∷ [])))
             (sym mem-s₁≡mem-s) step₁
  mem-inv-next (private-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) mi refl refl =
    let mem-s₁≡mem-s = consume-priv-mem s {v = w} {s′ = s₁} cp
        step₁ : SynthState.nr-wires st + 1 ≡ length (Preprocessed.memory s ++ (w ∷ []))
        step₁ = mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
    in subst (λ m → SynthState.nr-wires st + 1 ≡ length (m ++ (w ∷ [])))
             (sym mem-s₁≡mem-s) step₁
  mem-inv-next (private-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi
  mem-inv-next (private-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) mi refl refl =
    mem-inv-step-1 {st = st} {mem = Preprocessed.memory s} {v = w} mi

  pi-inv-next
    : ∀ {hc} (i : Instruction) (pre : ProofPreimage)
        (s : Preprocessed) (st : SynthState)
        (mem-suf pis-suf : List Fr)
        (sd : op-side-data i pre s mem-suf pis-suf)
    → pi-inv hc s st
    → length mem-suf ≡ Δmem i
    → length pis-suf ≡ Δpis-of i
    → pi-inv hc (next-state-from-osd i pre s mem-suf pis-suf sd)
                (circuit-instr hc i st)
  -- All non-pi instructions:  pis unchanged in both next-state-from-osd
  -- and circuit-instr.  We feed `pii` (or a small length-rewrite for
  -- record-update preservations) directly.
  pi-inv-next (add _ _)              _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (mul _ _)              _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (neg _)                _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (copy _)               _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (load-imm _)           _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (test-eq _ _)          _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (transient-hash _)     _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (cond-select _ _ _)    _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (not _)                _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (less-than _ _ _)      _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (reconstitute-field _ _ _) _ s st (w ∷ []) [] _ pii refl refl = pii
  pi-inv-next (ec-add _ _ _ _)       _ s st (x ∷ y ∷ []) [] _ pii refl refl = pii
  pi-inv-next (ec-mul _ _ _)         _ s st (x ∷ y ∷ []) [] _ pii refl refl = pii
  pi-inv-next (ec-mul-generator _)   _ s st (x ∷ y ∷ []) [] _ pii refl refl = pii
  pi-inv-next (hash-to-curve _)      _ s st (x ∷ y ∷ []) [] _ pii refl refl = pii
  pi-inv-next (persistent-hash _ _)  _ s st (x ∷ y ∷ []) [] _ pii refl refl = pii
  pi-inv-next (div-mod-power-of-two _ _) _ s st (x ∷ y ∷ []) [] _ pii refl refl = pii
  pi-inv-next (constrain-eq _ _)      _ s st [] [] _ pii refl refl = pii
  pi-inv-next (constrain-bits _ _)    _ s st [] [] _ pii refl refl = pii
  pi-inv-next (constrain-to-boolean _) _ s st [] [] _ pii refl refl = pii
  pi-inv-next (assert _)              _ s st [] [] _ pii refl refl = pii
  pi-inv-next (output _)              _ s st [] [] _ pii refl refl = pii
  pi-inv-next (pi-skip _ _)           _ s st [] [] (_ , _ , (true  , _ , _)) pii refl refl = pii
  pi-inv-next (pi-skip _ _)           _ s st [] [] (_ , _ , (false , _ , _)) pii refl refl = pii
  -- public-input / private-input: pis unchanged.  Active branch
  -- requires `pis s₁ ≡ pis s`.
  pi-inv-next (public-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) pii refl refl =
    subst (λ p → length p ≡ _) (sym (consume-pub-out-pis s {v = w} {s′ = s₁} cp)) pii
  pi-inv-next (public-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) pii refl refl =
    subst (λ p → length p ≡ _) (sym (consume-pub-out-pis s {v = w} {s′ = s₁} cp)) pii
  pi-inv-next (public-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) pii refl refl = pii
  pi-inv-next (public-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) pii refl refl = pii
  pi-inv-next (private-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) pii refl refl =
    subst (λ p → length p ≡ _) (sym (consume-priv-pis s {v = w} {s′ = s₁} cp)) pii
  pi-inv-next (private-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (true , _ , (s₁ , cp))) pii refl refl =
    subst (λ p → length p ≡ _) (sym (consume-priv-pis s {v = w} {s′ = s₁} cp)) pii
  pi-inv-next (private-input nothing) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) pii refl refl = pii
  pi-inv-next (private-input (just _)) _ s st (w ∷ []) [] (_ , refl , _ , (false , _ , _)) pii refl refl = pii
  -- declare-pub-input:  Δpis = 1; the post-state's nr-declared-pi is
  -- `suc (nr-declared-pi st)`; the post-state pis is `pis s ++ (wv ∷ [])`.
  pi-inv-next {hc} (declare-pub-input _) _ s st [] (wv ∷ []) (_ , _ , refl) pii refl refl =
    pi-inv-add-1-declare hc st (Preprocessed.pis s) (wv ∷ []) refl pii

------------------------------------------------------------------------
-- Constraints-fit invariant along `circuit-instrs`.
--
-- Three pieces of infrastructure that together establish:
--
--   `constraints-mem-fit (constraints (circuit-instrs hc is st₀))
--                    (nr-wires (circuit-instrs hc is st₀))`
--
-- given an analogous fact on `st₀` and a `Wire-Trace`.  This is the
-- invariant that lets D2's cons case shrink the satisfies-constraints
-- witness back to the operationally-relevant memory at each step.
------------------------------------------------------------------------

private
  ------------------------------------------------------------------------
  -- Monotonicity of `constraint-mem-fits` / `constraints-mem-fit` along
  -- `n ↦ n + k` (each operand bound lifted by `m≤n⇒m≤n+o`).
  ------------------------------------------------------------------------

  -- Monotonicity of `expr-mem-fits` along `n ↦ n + k`.  Each wire
  -- bound is lifted by `m≤n⇒m≤n+o`; the `gate` case of
  -- `constraint-mem-fits-mono` defers to it.
  expr-mem-fits-mono : ∀ (e : Expr) n k
    → expr-mem-fits e n → expr-mem-fits e (n + k)
  expr-mem-fits-mono (wire i) n k h = m≤n⇒m≤n+o k h
  expr-mem-fits-mono (con _)  _ _ h = h
  expr-mem-fits-mono (l ⊕ r)  n k (hl , hr) =
    expr-mem-fits-mono l n k hl , expr-mem-fits-mono r n k hr
  expr-mem-fits-mono (l ⊗ r)  n k (hl , hr) =
    expr-mem-fits-mono l n k hl , expr-mem-fits-mono r n k hr
  expr-mem-fits-mono (⊝ e)    n k h = expr-mem-fits-mono e n k h

  -- `constraint-mem-fits cl n → constraint-mem-fits cl (n+k)`.  Each operand
  -- bound `x < n` is lifted by `m≤n⇒m≤n+o`; list operands by `mapᴬ`.
  constraint-mem-fits-mono : ∀ (cl : Constraint) n k
    → constraint-mem-fits cl n → constraint-mem-fits cl (n + k)
  constraint-mem-fits-mono (gate e) n k h = expr-mem-fits-mono e n k h
  constraint-mem-fits-mono (non-zero c) n k h = m≤n⇒m≤n+o k h
  constraint-mem-fits-mono (select out b a c) n k (hout , hb , ha , hc) =
    m≤n⇒m≤n+o k hout , m≤n⇒m≤n+o k hb , m≤n⇒m≤n+o k ha , m≤n⇒m≤n+o k hc
  constraint-mem-fits-mono (in-range v _) n k h = m≤n⇒m≤n+o k h
  constraint-mem-fits-mono (boolean v) n k h = m≤n⇒m≤n+o k h
  constraint-mem-fits-mono (ec-add cx cy ax ay bx by) n k
    (hcx , hcy , hax , hay , hbx , hby) =
    m≤n⇒m≤n+o k hcx , m≤n⇒m≤n+o k hcy , m≤n⇒m≤n+o k hax
    , m≤n⇒m≤n+o k hay , m≤n⇒m≤n+o k hbx , m≤n⇒m≤n+o k hby
  constraint-mem-fits-mono (ec-mul cx cy ax ay s) n k
    (hcx , hcy , hax , hay , hs) =
    m≤n⇒m≤n+o k hcx , m≤n⇒m≤n+o k hcy , m≤n⇒m≤n+o k hax
    , m≤n⇒m≤n+o k hay , m≤n⇒m≤n+o k hs
  constraint-mem-fits-mono (ec-gen cx cy s) n k (hcx , hcy , hs) =
    m≤n⇒m≤n+o k hcx , m≤n⇒m≤n+o k hcy , m≤n⇒m≤n+o k hs
  constraint-mem-fits-mono (h2c cx cy inputs) n k (hcx , hcy , hin) =
    m≤n⇒m≤n+o k hcx , m≤n⇒m≤n+o k hcy , mapᴬ (m≤n⇒m≤n+o k) hin
  constraint-mem-fits-mono (div-mod q r v _) n k (hq , hr , hv) =
    m≤n⇒m≤n+o k hq , m≤n⇒m≤n+o k hr , m≤n⇒m≤n+o k hv
  constraint-mem-fits-mono (reconstitute out d m _) n k (hout , hd , hm) =
    m≤n⇒m≤n+o k hout , m≤n⇒m≤n+o k hd , m≤n⇒m≤n+o k hm
  constraint-mem-fits-mono (poseidon out inputs) n k (hout , hin) =
    m≤n⇒m≤n+o k hout , mapᴬ (m≤n⇒m≤n+o k) hin
  constraint-mem-fits-mono (sha256 h₁ h₂ _ inputs) n k (hh1 , hh2 , hin) =
    m≤n⇒m≤n+o k hh1 , m≤n⇒m≤n+o k hh2 , mapᴬ (m≤n⇒m≤n+o k) hin
  constraint-mem-fits-mono (test-eq out a b) n k (hout , ha , hb) =
    m≤n⇒m≤n+o k hout , m≤n⇒m≤n+o k ha , m≤n⇒m≤n+o k hb
  constraint-mem-fits-mono (is-zero out a) n k (hout , ha) =
    m≤n⇒m≤n+o k hout , m≤n⇒m≤n+o k ha
  constraint-mem-fits-mono (less-than out a b _) n k (hout , ha , hb) =
    m≤n⇒m≤n+o k hout , m≤n⇒m≤n+o k ha , m≤n⇒m≤n+o k hb
  constraint-mem-fits-mono (guard-disj out i) n k (hout , hi) =
    m≤n⇒m≤n+o k hout , m≤n⇒m≤n+o k hi
  constraint-mem-fits-mono (bind _ widx) n k h = m≤n⇒m≤n+o k h
  constraint-mem-fits-mono (comm inputs outputs) n k (hin , hout) =
    mapᴬ (m≤n⇒m≤n+o k) hin , mapᴬ (m≤n⇒m≤n+o k) hout

  -- The list-level monotonicity:  pointwise from `constraint-mem-fits-mono`.
  constraints-mem-fits-mono : ∀ (cs : List Constraint) n k
    → constraints-mem-fit cs n → constraints-mem-fit cs (n + k)
  constraints-mem-fits-mono []       n k _ = tt
  constraints-mem-fits-mono (c ∷ cs) n k (hc , htl) =
    constraint-mem-fits-mono c n k hc , constraints-mem-fits-mono cs n k htl

  ------------------------------------------------------------------------
  -- Per-step fit: the new constraints emitted by `circuit-instr hc i st`
  -- fit in `nr-wires st + Δmem i`.  26-case proof.
  --
  -- The basic strategy in each case is:
  --   • project each operand's `_< nr-wires st` bound from `WireOK`;
  --   • lift it via `m≤n⇒m≤n+o` to `_< nr-wires st + Δmem i`;
  --   • for the "new output wire" cases (`nr-wires st`, `suc (nr-wires st)`),
  --     use the explicit `n<n+k` facts below.
  ------------------------------------------------------------------------

  -- Output-wire fit facts: the freshly-emitted output wire(s) sit below
  -- the bumped wire count.
  n<n+1 : ∀ n → n < n + 1
  n<n+1 zero    = Data.Nat.s≤s Data.Nat.z≤n
  n<n+1 (suc n) = Data.Nat.s≤s (n<n+1 n)

  n<n+2 : ∀ n → n < n + 2
  n<n+2 zero    = Data.Nat.s≤s Data.Nat.z≤n
  n<n+2 (suc n) = Data.Nat.s≤s (n<n+2 n)

  sn<n+2 : ∀ n → suc n < n + 2
  sn<n+2 zero    = Data.Nat.s≤s (Data.Nat.s≤s Data.Nat.z≤n)
  sn<n+2 (suc n) = Data.Nat.s≤s (sn<n+2 n)

  constraints-new-fit-step
    : ∀ (hc : Bool) (st : SynthState) (i : Instruction)
    → WireOK i (SynthState.nr-wires st)
    → constraints-mem-fit (instr-new-constraints hc st i)
                      (SynthState.nr-wires st + Δmem i)
  -- Δmem=0 cases (constraints use only existing wires; sometimes empty).
  constraints-new-fit-step hc st (assert c) wc =
    m≤n⇒m≤n+o 0 wc , tt
  constraints-new-fit-step hc st (constrain-bits v bits) wc =
    m≤n⇒m≤n+o 0 wc , tt
  constraints-new-fit-step hc st (constrain-eq a b) (ha , hb) =
    (m≤n⇒m≤n+o 0 ha , m≤n⇒m≤n+o 0 hb) , tt
  constraints-new-fit-step hc st (constrain-to-boolean v) wc =
    m≤n⇒m≤n+o 0 wc , tt
  constraints-new-fit-step hc st (declare-pub-input v) wc =
    m≤n⇒m≤n+o 0 wc , tt
  constraints-new-fit-step hc st (pi-skip g count) wc = tt
  constraints-new-fit-step hc st (output v) wc = tt
  -- Δmem=1 cases that introduce a new output wire at `nr-wires st`.
  constraints-new-fit-step hc st (cond-select b a c) (hb , ha , hcw) =
    ( n<n+1 (SynthState.nr-wires st)
    , m≤n⇒m≤n+o 1 hb , m≤n⇒m≤n+o 1 ha , m≤n⇒m≤n+o 1 hcw ) , tt
  constraints-new-fit-step hc st (copy v) wc =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 wc ) , tt
  constraints-new-fit-step hc st (load-imm imm) wc =
    (n<n+1 (SynthState.nr-wires st) , tt) , tt
  constraints-new-fit-step hc st (reconstitute-field d m bits) (hd , hm) =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 hd , m≤n⇒m≤n+o 1 hm ) , tt
  constraints-new-fit-step hc st (transient-hash inputs) wc =
    ( n<n+1 (SynthState.nr-wires st) , mapᴬ (m≤n⇒m≤n+o 1) wc ) , tt
  constraints-new-fit-step hc st (test-eq a b) (ha , hb) =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 ha , m≤n⇒m≤n+o 1 hb ) , tt
  constraints-new-fit-step hc st (add a b) (ha , hb) =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 ha , m≤n⇒m≤n+o 1 hb ) , tt
  constraints-new-fit-step hc st (mul a b) (ha , hb) =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 ha , m≤n⇒m≤n+o 1 hb ) , tt
  constraints-new-fit-step hc st (neg a) wc =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 wc ) , tt
  constraints-new-fit-step hc st (not a) wc =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 wc ) , tt
  constraints-new-fit-step hc st (less-than a b bits) (ha , hb) =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 ha , m≤n⇒m≤n+o 1 hb ) , tt
  -- Δmem=1 cases with optional guard:  `public-input` / `private-input`.
  constraints-new-fit-step hc st (public-input nothing) wc = tt
  constraints-new-fit-step hc st (public-input (just g)) wc =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 wc ) , tt
  constraints-new-fit-step hc st (private-input nothing) wc = tt
  constraints-new-fit-step hc st (private-input (just g)) wc =
    ( n<n+1 (SynthState.nr-wires st) , m≤n⇒m≤n+o 1 wc ) , tt
  -- Δmem=2 cases that introduce two new output wires.
  constraints-new-fit-step hc st (ec-add ax ay bx by) (hax , hay , hbx , hby) =
    ( n<n+2 (SynthState.nr-wires st)
    , sn<n+2 (SynthState.nr-wires st)
    , m≤n⇒m≤n+o 2 hax , m≤n⇒m≤n+o 2 hay , m≤n⇒m≤n+o 2 hbx , m≤n⇒m≤n+o 2 hby ) , tt
  constraints-new-fit-step hc st (ec-mul ax ay s) (hax , hay , hs) =
    ( n<n+2 (SynthState.nr-wires st)
    , sn<n+2 (SynthState.nr-wires st)
    , m≤n⇒m≤n+o 2 hax , m≤n⇒m≤n+o 2 hay , m≤n⇒m≤n+o 2 hs ) , tt
  constraints-new-fit-step hc st (ec-mul-generator s) wc =
    ( n<n+2 (SynthState.nr-wires st)
    , sn<n+2 (SynthState.nr-wires st)
    , m≤n⇒m≤n+o 2 wc ) , tt
  constraints-new-fit-step hc st (hash-to-curve inputs) wc =
    ( n<n+2 (SynthState.nr-wires st)
    , sn<n+2 (SynthState.nr-wires st)
    , mapᴬ (m≤n⇒m≤n+o 2) wc ) , tt
  constraints-new-fit-step hc st (persistent-hash α inputs) wc =
    ( n<n+2 (SynthState.nr-wires st)
    , sn<n+2 (SynthState.nr-wires st)
    , mapᴬ (m≤n⇒m≤n+o 2) wc ) , tt
  constraints-new-fit-step hc st (div-mod-power-of-two v bits) wc =
    ( n<n+2 (SynthState.nr-wires st)
    , sn<n+2 (SynthState.nr-wires st)
    , m≤n⇒m≤n+o 2 wc ) , tt

  ------------------------------------------------------------------------
  -- The iterated constraints-fit invariant.
  --
  -- Given:
  --   • `constraints-mem-fit (constraints st₀) (nr-wires st₀)`;
  --   • a `Wire-Trace is (nr-wires st₀) final-w` (which witnesses that
  --     every prefix satisfies `WireOK`).
  --
  -- Conclude that
  --   `constraints-mem-fit (constraints (circuit-instrs hc is st₀))
  --                    (nr-wires (circuit-instrs hc is st₀))`.
  --
  -- The induction step combines `constraints-after-instr-eq` (decomposes
  -- the post-step constraints), `nr-wires-step` (relates post- and pre-step
  -- `nr-wires`), and the two monotonicity lemmas.
  ------------------------------------------------------------------------

  -- Extract the head step's `WireOK` premise and the residual trace from
  -- a `Wire-Trace`.  Both are projections of `wire-trace-head`.
  wire-trace-head-ok : ∀ {i is n final}
    → Wire-Trace (i ∷ is) n final
    → WireOK i n
  wire-trace-head-ok wt = proj₁ (wire-trace-head wt)

  wire-trace-tail : ∀ {i is n final}
    → (wt : Wire-Trace (i ∷ is) n final)
    → Wire-Trace is (n + Δmem i) final
  wire-trace-tail wt = proj₂ (wire-trace-head wt)

  -- ∧ combiner concatenated with `constraints-mem-fit-++`:  the fit of
  -- `xs ++ ys` at `n` decomposes into fit of `xs` at `n` and fit of
  -- `ys` at `n`.
  constraints-mem-fit-++
    : ∀ (xs ys : List Constraint) n
    → constraints-mem-fit xs n
    → constraints-mem-fit ys n
    → constraints-mem-fit (xs ++ ys) n
  constraints-mem-fit-++ []       ys n _        hy = hy
  constraints-mem-fit-++ (x ∷ xs) ys n (hx , htl) hy =
    hx , constraints-mem-fit-++ xs ys n htl hy

  constraints-st-fit-invariant
    : ∀ {hc} (st₀ : SynthState) (is : List Instruction)
      {final-w : ℕ}
    → constraints-mem-fit (SynthState.constraints st₀) (SynthState.nr-wires st₀)
    → Wire-Trace is (SynthState.nr-wires st₀) final-w
    → constraints-mem-fit
        (SynthState.constraints (circuit-instrs hc is st₀))
        (SynthState.nr-wires (circuit-instrs hc is st₀))
  -- Empty list:  `circuit-instrs hc [] st₀ = st₀`.
  constraints-st-fit-invariant {hc} st₀ [] base _ = base
  -- Cons:  recurse on the post-step state `circuit-instr hc i st₀`,
  -- threading the lifted base via `constraints-after-instr-eq`,
  -- `nr-wires-step`, `constraints-mem-fits-mono`, and `constraints-new-fit-step`.
  constraints-st-fit-invariant {hc} st₀ (i ∷ is) {final-w} base wt =
    let n₀ = SynthState.nr-wires st₀
        st₁ = circuit-instr hc i st₀
        n₁ = SynthState.nr-wires st₁
        -- Head's WireOK.
        wc-head = wire-trace-head-ok wt
        -- Tail's wire trace at `n₀ + Δmem i`.
        wt-tail : Wire-Trace is (n₀ + Δmem i) final-w
        wt-tail = wire-trace-tail wt
        -- Reshape the tail's wire trace from `n₀ + Δmem i` to
        -- `nr-wires st₁`, using `nr-wires-step`.
        nw-eq : n₁ ≡ n₀ + Δmem i
        nw-eq = nr-wires-step {hc} i st₀
        wt-tail' : Wire-Trace is n₁ final-w
        wt-tail' = subst (λ m → Wire-Trace is m final-w) (sym nw-eq) wt-tail
        -- Prior constraints lifted to `n₀ + Δmem i`.
        prior-lift : constraints-mem-fit (SynthState.constraints st₀)
                                     (n₀ + Δmem i)
        prior-lift = constraints-mem-fits-mono (SynthState.constraints st₀) n₀
                                           (Δmem i) base
        -- New head constraints fit in `n₀ + Δmem i`.
        new-fit : constraints-mem-fit (instr-new-constraints hc st₀ i)
                                  (n₀ + Δmem i)
        new-fit = constraints-new-fit-step hc st₀ i wc-head
        -- Combined fit at `n₀ + Δmem i`.
        combined-at-n0+ : constraints-mem-fit
                            (SynthState.constraints st₀ ++ instr-new-constraints hc st₀ i)
                            (n₀ + Δmem i)
        combined-at-n0+ = constraints-mem-fit-++
                            (SynthState.constraints st₀)
                            (instr-new-constraints hc st₀ i)
                            (n₀ + Δmem i)
                            prior-lift new-fit
        -- Rewrite combined fit at `n₁` using `nw-eq`.
        combined-at-n1 : constraints-mem-fit
                            (SynthState.constraints st₀ ++ instr-new-constraints hc st₀ i)
                            n₁
        combined-at-n1 =
          subst (λ m → constraints-mem-fit
                         (SynthState.constraints st₀ ++ instr-new-constraints hc st₀ i)
                         m)
                (sym nw-eq) combined-at-n0+
        -- Rewrite the constraint list using `constraints-after-instr-eq`.
        post-eq : SynthState.constraints st₁
                  ≡ SynthState.constraints st₀ ++ instr-new-constraints hc st₀ i
        post-eq = constraints-after-instr-eq {hc} i st₀
        base-at-st1 : constraints-mem-fit (SynthState.constraints st₁) n₁
        base-at-st1 = subst (λ cs → constraints-mem-fit cs n₁)
                            (sym post-eq) combined-at-n1
    in constraints-st-fit-invariant {hc} st₁ is base-at-st1 wt-tail'

------------------------------------------------------------------------
-- Shape extractors and the pis-fit list invariant.
--
-- `osd-mem-len` / `osd-pis-len`:  from a per-step `op-side-data`
-- payload recover the canonical suffix lengths
-- `length mem-step ≡ Δmem i` and `length pis-step ≡ Δpis-of i`.
-- These feed `mem-inv-next` / `pi-inv-next` / `constraints-pis-fit-instr`
-- in the D2 cons-case.  Each constraint matches the shape Σ that
-- `op-side-data` pins for the instruction; matching the embedded
-- equalities as `refl` collapses the suffix to its canonical form so
-- the length is `refl`.
--
-- `constraints-pis-fit-invariant`:  the pis-side dual of
-- `constraints-st-fit-invariant`, maintaining
-- `constraints-pis-fit (constraints (circuit-instrs hc is st₀))
--                  (length (pis s'))` along the trace.  Unlike
-- the mem-side invariant (bounded by the synthesis `nr-wires`), the
-- pis bound is the *running* `length (pis s)` of the operational
-- state; only `declare-pub-input` emits a pi-referencing constraint, and
-- `constraints-pis-fit-instr` shows it fits once the step has appended its
-- pis cell.  The invariant is threaded through the `op-side-data-list`
-- structure so the per-step `pi-inv` and the suffix lengths are
-- available at each node.
------------------------------------------------------------------------

private
  -- Δmem suffix length from the side data.
  osd-mem-len
    : ∀ (i : Instruction) (pre : ProofPreimage) (s : Preprocessed)
        (ms ps : List Fr)
    → op-side-data i pre s ms ps
    → length ms ≡ Δmem i
  osd-mem-len i pre s ms ps = go (shapeView i)
    where
    go : ∀ {j} (v : ShapeView j) → osd-of v pre s ms ps
       → length ms ≡ shape-Δmem (shape-of v)
    go sv-pure0   (refl , _)           = refl
    go sv-pure1   ((_ , refl) , _)     = refl
    go sv-pure2   ((_ , _ , refl) , _) = refl
    go sv-declare (refl , _)           = refl
    go sv-output  (_ , refl , _)       = refl
    go sv-skip    (refl , _)           = refl
    go sv-pub-in  (_ , refl , _)       = refl
    go sv-priv-in (_ , refl , _)       = refl

  -- Δpis suffix length from the side data.
  osd-pis-len
    : ∀ (i : Instruction) (pre : ProofPreimage) (s : Preprocessed)
        (ms ps : List Fr)
    → op-side-data i pre s ms ps
    → length ps ≡ Δpis-of i
  osd-pis-len i pre s ms ps = go (shapeView i)
    where
    go : ∀ {j} (v : ShapeView j) → osd-of v pre s ms ps
       → length ps ≡ shape-Δpis (shape-of v)
    go sv-pure0   (_ , refl)         = refl
    go sv-pure1   (_ , refl)         = refl
    go sv-pure2   (_ , refl)         = refl
    go sv-declare (_ , _ , refl)     = refl
    go sv-output  (_ , _ , refl)     = refl
    go sv-skip    (_ , refl , _)     = refl
    go sv-pub-in  (_ , _ , refl , _) = refl
    go sv-priv-in (_ , _ , refl , _) = refl

  -- pis-side list-level fit invariant, threaded through the operational
  -- trace structure.  At each node, `pi-inv` supplies the base index
  -- `length (pis s) ≡ preamble-pi-count hc + nr-declared-pi st`, which
  -- `constraints-pis-fit-instr` turns into the head-step fit at the
  -- *post-step* pis length; the prior constraints' fit is preserved because
  -- pis only ever grows (so a `constraint-pis-fit cl n` still holds at
  -- the larger length — proved by `constraints-pis-fit-mono` below).
  constraint-pis-fit-mono : ∀ (cl : Constraint) m n
    → m Data.Nat.≤ n
    → constraint-pis-fit cl m → constraint-pis-fit cl n
  constraint-pis-fit-mono (gate _)           _ _ _ h = h
  constraint-pis-fit-mono (non-zero _)       _ _ _ h = h
  constraint-pis-fit-mono (select _ _ _ _)     _ _ _ h = h
  constraint-pis-fit-mono (in-range _ _)          _ _ _ h = h
  constraint-pis-fit-mono (boolean _)                  _ _ _ h = h
  constraint-pis-fit-mono (ec-add _ _ _ _ _ _)      _ _ _ h = h
  constraint-pis-fit-mono (ec-mul _ _ _ _ _)        _ _ _ h = h
  constraint-pis-fit-mono (ec-gen _ _ _)  _ _ _ h = h
  constraint-pis-fit-mono (h2c _ _ _)     _ _ _ h = h
  constraint-pis-fit-mono (div-mod _ _ _ _)         _ _ _ h = h
  constraint-pis-fit-mono (reconstitute _ _ _ _)    _ _ _ h = h
  constraint-pis-fit-mono (poseidon _ _)      _ _ _ h = h
  constraint-pis-fit-mono (sha256 _ _ _ _) _ _ _ h = h
  constraint-pis-fit-mono (test-eq _ _ _)           _ _ _ h = h
  constraint-pis-fit-mono (is-zero _ _)                 _ _ _ h = h
  constraint-pis-fit-mono (less-than _ _ _ _)       _ _ _ h = h
  constraint-pis-fit-mono (guard-disj _ _)          _ _ _ h = h
  constraint-pis-fit-mono (bind entry _)    m n m≤n h = <-≤-trans h m≤n
  constraint-pis-fit-mono (comm _ _)     m n m≤n h = <-≤-trans h m≤n

  constraints-pis-fit-mono : ∀ (cs : List Constraint) m n
    → m Data.Nat.≤ n
    → constraints-pis-fit cs m → constraints-pis-fit cs n
  constraints-pis-fit-mono []       _ _ _   _ = tt
  constraints-pis-fit-mono (c ∷ cs) m n m≤n (hc , htl) =
    constraint-pis-fit-mono c m n m≤n hc , constraints-pis-fit-mono cs m n m≤n htl

  constraints-pis-fit-++
    : ∀ (xs ys : List Constraint) n
    → constraints-pis-fit xs n
    → constraints-pis-fit ys n
    → constraints-pis-fit (xs ++ ys) n
  constraints-pis-fit-++ []       ys n _        hy = hy
  constraints-pis-fit-++ (x ∷ xs) ys n (hx , htl) hy =
    hx , constraints-pis-fit-++ xs ys n htl hy

  -- `length xs ≤ length (xs ++ ys)`.
  len-≤-++ : ∀ (xs ys : List Fr) → length xs Data.Nat.≤ length (xs ++ ys)
  len-≤-++ []       ys = Data.Nat.z≤n
  len-≤-++ (x ∷ xs) ys = Data.Nat.s≤s (len-≤-++ xs ys)

  -- `length (xs ++ ys) ≡ length xs + length ys`.
  len-++ : ∀ (xs ys : List Fr) → length (xs ++ ys) ≡ length xs + length ys
  len-++ []       ys = refl
  len-++ (x ∷ xs) ys = cong suc (len-++ xs ys)

  -- First components of the per-step accumulators bump by `Δmem i`.
  o2-step-fst : ∀ (i : Instruction) {n bk acc'}
    → O2-step i (n , bk) ≡ just acc'
    → proj₁ acc' ≡ n + Δmem i
  o2-step-fst i {n} {bk} eq with O2-check i bk | eq
  ... | just _  | refl = refl
  ... | nothing | ()

  o3-step-fst : ∀ (i : Instruction) {n bm acc'}
    → O3-step i (n , bm) ≡ just acc'
    → proj₁ acc' ≡ n + Δmem i
  o3-step-fst i {n} {bm} eq with o3OK? i bm | eq
  ... | yes _ | refl = refl
  ... | no  _ | ()

  wire-step-fst : ∀ (i : Instruction) (n : ℕ) {n'}
    → wire-step i n ≡ just n'
    → n' ≡ n + Δmem i
  wire-step-fst i n eq with wireOK? i n | eq
  ... | yes _ | refl = refl
  ... | no  _ | ()

------------------------------------------------------------------------
-- D2  —  per-list backward dispatcher.
--
-- Signature:
--
--   • the post-state `s` is *existential* (mirrors D1), so the
--     dispatcher can rebuild it from the witness's mem/pis decomposition;
--   • the witness's memory is `memory s₀ ++ mem-suf` for an explicit
--     suffix `mem-suf` (and analogously for pis);
--   • `O2-Inv`, `O3-Inv`, `O2-Trace`, `O3-Trace`, `Wire-Trace` are
--     threaded as separate hypotheses (D3 derives them from
--     `T (producer-safe src)`);
--   • `op-side-data-list`  is the structural trace that supplies the
--     per-step side data D1 needs for the four "side-data instructions".
--
-- The side-data list also decomposes `mem-suf` and `pis-suf` into
-- per-step pieces.
------------------------------------------------------------------------

-- Trace of per-step operational side data, jointly threading the
-- preprocessed-state evolution.  Each `osd-cons` provides:
--   • the side-data for the head instruction;
--   • the tail side-data list at the computed next state
--     (`next-state-from-osd i pre s mem-step pis-step sd`).
--
-- The "next state" is computed by `next-state-from-osd` from the head
-- instruction and its side data (rather than supplied by the caller as
-- an arbitrary `s'` with separate mem/pis equations).  This makes D2's
-- cons-case definitional: D1's output and `next-state-from-osd ...`
-- coincide for each of the 26 instructions.
--
-- The list "linearises" the operational trace structure without
-- carrying the `R-instr` constructors themselves (which D2 reconstructs).
data op-side-data-list (pre : ProofPreimage) :
       (s : Preprocessed) (is : List Instruction)
       (mem-suf pis-suf : List Fr) → Set where
  osd-nil  : ∀ {s} → op-side-data-list pre s [] [] []
  osd-cons : ∀ {s i is mem-step pis-step mem-tail pis-tail}
    → (sd : op-side-data i pre s mem-step pis-step)
    → op-side-data-list pre
        (next-state-from-osd i pre s mem-step pis-step sd)
        is mem-tail pis-tail
    → op-side-data-list pre s (i ∷ is) (mem-step ++ mem-tail) (pis-step ++ pis-tail)

-- The endpoint reached by folding `next-state-from-osd` along an
-- `op-side-data-list`.  D2's existential output `s'` is provably this
-- fold (see the `fold-eq` field added to D2's result below); and a
-- `Tr-shaped` trace's endpoint index coincides with it too (since
-- `tr-next ≡ next-state-from-osd`).  This is the bridge that pins D2's
-- `s'` to the GIVEN state `s` in `circuit-faithful-bwd`.
osd-fold : ∀ {pre s is ms ps} → op-side-data-list pre s is ms ps → Preprocessed
osd-fold {s = s} osd-nil = s
osd-fold (osd-cons {i = i} {mem-step = mem-step} {pis-step = pis-step} sd rest) =
  osd-fold rest

-- D2 itself.
--
-- The four bool-traces and `O2-Inv` / `O3-Inv` are explicit so the
-- inductive step can refine them via `o2-step ≡ just _` / `o3-step ≡ just _`
-- and `o2-preserve` / `o3-preserve` (already proven).
--
-- Two constraint-fit preconditions (`fit-mem`, `fit-pis`) state that the
-- synthesis state's *current* constraints fit in the memory / pis lengths
-- reached so far.  They are
-- threaded inductively (each step's post-state fit is re-established by
-- `constraints-st-fit-invariant` / `constraints-pis-fit-instr`) and are
-- discharged trivially at the top-level call site (`circuit-faithful-bwd`),
-- where `st₀` is the initial synthesis state with `constraints ≡ []` (so both
-- fits are `refl`).
satisfies-constraints→R-instrs
  : ∀ {hc} (pre : ProofPreimage) (s₀ : Preprocessed)
    (is : List Instruction) (st₀ : SynthState)
    (mem-suf pis-suf : List Fr)
  → mem-inv s₀ st₀
  → pi-inv  hc s₀ st₀
  → constraints-mem-fit (SynthState.constraints st₀) (SynthState.nr-wires st₀)
  → constraints-pis-fit (SynthState.constraints st₀) (length (Preprocessed.pis s₀))
  → ∀ {bk₀ : IndexSet} {bm₀ : PartialMap}
  → O2-Inv (SynthState.nr-wires st₀ , bk₀) s₀
  → O3-Inv (SynthState.nr-wires st₀ , bm₀) s₀
  → ∀ {final-o2 : ℕ × IndexSet} {final-o3 : ℕ × PartialMap} {final-w : ℕ}
  → O2-Trace is (SynthState.nr-wires st₀ , bk₀) final-o2
  → O3-Trace is (SynthState.nr-wires st₀ , bm₀) final-o3
  → Wire-Trace is (SynthState.nr-wires st₀) final-w
  → All WF2-instr is                                   -- WF2 (§3.3)
  → (osd : op-side-data-list pre s₀ is mem-suf pis-suf)
  → satisfies-constraints
      (SynthState.constraints (circuit-instrs hc is st₀))
      (mk-witness (Preprocessed.memory s₀ ++ mem-suf)
                  (Preprocessed.pis    s₀ ++ pis-suf)
                  (comm-rand-of pre))
  → Σ Preprocessed (λ s' →
        (Preprocessed.memory s' ≡ Preprocessed.memory s₀ ++ mem-suf)
      × (Preprocessed.pis    s' ≡ Preprocessed.pis    s₀ ++ pis-suf)
      × R-instrs pre s₀ is s'
      × (s' ≡ osd-fold osd))

-- The body inducts on the `op-side-data-list` structure.  For `i ∷ is'`
-- with `osd-cons sd osd-tail`:
--
--   1. Peel the head step from the O2 / O3 / wire traces (constructor
--      match), recovering each per-step `*-step ≡ just acc'` and the
--      residual tail trace.
--   2. Establish the head's constraint-fit facts at the post-step memory /
--      pis lengths (`constraints-st-fit-invariant` for mem,
--      `constraints-pis-fit-instr` + `constraints-pis-fit-mono`/`-++` for pis)
--      and shrink the satisfaction witness from the full
--      `mem-step ++ mem-tail` / `pis-step ++ pis-tail` down to just the
--      head's `mem-step` / `pis-step` (`satisfies-constraints-mem-shrink`,
--      `satisfies-constraints-pis-shrink`).  These fits are stated purely in
--      terms of `mem₀ ++ mem-step` / `pis₀ ++ pis-step` (no reference to
--      D1's output `s₁`), so there is no circular dependency on D1.
--   3. Apply D1 (`satisfies→R-instr-step`) to the shrunk head witness,
--      obtaining `s₁ = next-state-from-osd …`, `memory s₁ ≡ memory s₀ ++
--      mem-step`, `pis s₁ ≡ pis s₀ ++ pis-step`, and `R-instr pre s₀ i s₁`.
--   4. Advance the invariants to `s₁` (`mem-inv-next`, `pi-inv-next`,
--      `o2-preserve`, `o3-preserve`) and recurse on `is'` from `st₁`,
--      feeding the residual traces (re-indexed by `nr-wires-step` and the
--      per-step first-component bumps) and the full witness rewritten by
--      `++-assoc` so its memory / pis read as `memory s₁ ++ mem-tail` /
--      `pis s₁ ++ pis-tail`.
--   5. Assemble `R-instrs pre s₀ (i ∷ is') s'` with `r-step`, chaining
--      the memory / pis equations through `++-assoc`.

-- Empty list:  the witness's mem-suf and pis-suf are forced by
-- `osd-nil` to be `[]`.  Produce `r-done` and the trivial equations.
satisfies-constraints→R-instrs pre s₀ [] st₀ .[] .[]
                            mi pii fit-mem fit-pis _ _ _ _ _ _ osd-nil _ =
  s₀ , sym (++-identityʳ _) , sym (++-identityʳ _) , r-done , refl

-- Cons case.
satisfies-constraints→R-instrs {hc} pre s₀ (i ∷ is') st₀ ._ ._
    mi pii fit-mem fit-pis {bk₀} {bm₀} o2-inv o3-inv {final-o2} {final-o3} {final-w}
    (o2-step {acc' = o2acc'} o2se o2-tail)
    (o3-step {acc' = o3acc'} o3se o3-tail)
    (wire-cons {n' = wn'} wse w-tail)
    (wf2-head ∷ᴬ wf2-tail)
    (osd-cons {mem-step = mem-step} {pis-step = pis-step}
              {mem-tail = mem-tail} {pis-tail = pis-tail} sd osd-tail)
    sat =
  let
    n₀  = SynthState.nr-wires st₀
    st₁ = circuit-instr hc i st₀
    n₁  = SynthState.nr-wires st₁
    mem₀ = Preprocessed.memory s₀
    pis₀ = Preprocessed.pis s₀

    -- Suffix length facts from the side data.
    ms-len : length mem-step ≡ Δmem i
    ms-len = osd-mem-len i pre s₀ mem-step pis-step sd
    ps-len : length pis-step ≡ Δpis-of i
    ps-len = osd-pis-len i pre s₀ mem-step pis-step sd

    -- Per-step obligation premises for D1.
    o2chk : O2-check i bk₀ ≡ just bk₀
    o2chk = o2-check-from-step i {n = n₀} bk₀ o2se
    o3chk : O3OK i bm₀
    o3chk = o3-check-from-step i {n = n₀} bm₀ o3se

    -- Head's `WireOK` from `wse : wire-step i n₀ ≡ just wn'`.
    wc : WireOK i n₀
    wc = proj₁ (wire-trace-head (wire-cons wse wire-done))

    -- `nr-wires` after the head step.
    nw-eq : n₁ ≡ n₀ + Δmem i
    nw-eq = nr-wires-step {hc} i st₀

    -- `length (mem₀ ++ mem-step) ≡ n₁`.
    len-mem-eq : length (mem₀ ++ mem-step) ≡ n₁
    len-mem-eq =
      trans (len-++ mem₀ mem-step)
        (trans (cong₂ _+_ (sym mi) ms-len) (sym nw-eq))

    -- mem-fit for `constraints st₁`, stated at `length (mem₀ ++ mem-step)`.
    fit-mem-st₁-nw : constraints-mem-fit (SynthState.constraints st₁) n₁
    fit-mem-st₁-nw =
      constraints-st-fit-invariant {hc} st₀ (i ∷ []) fit-mem
        (wire-cons wse wire-done)
    fit-mem-head : constraints-mem-fit (SynthState.constraints st₁)
                                   (length (mem₀ ++ mem-step))
    fit-mem-head = subst (λ k → constraints-mem-fit (SynthState.constraints st₁) k)
                         (sym len-mem-eq) fit-mem-st₁-nw

    -- pis-fit for `constraints st₁`, stated at `length (pis₀ ++ pis-step)`.
    pis-le : length pis₀ Data.Nat.≤ length (pis₀ ++ pis-step)
    pis-le = len-≤-++ pis₀ pis-step
    fit-pis-prior : constraints-pis-fit (SynthState.constraints st₀)
                                    (length (pis₀ ++ pis-step))
    fit-pis-prior = constraints-pis-fit-mono (SynthState.constraints st₀)
                      (length pis₀) (length (pis₀ ++ pis-step)) pis-le fit-pis
    fit-pis-new : constraints-pis-fit (instr-new-constraints hc st₀ i)
                                  (length (pis₀ ++ pis-step))
    fit-pis-new = constraints-pis-fit-instr hc st₀ s₀ i pis-step ps-len pii
    fit-pis-head : constraints-pis-fit (SynthState.constraints st₁)
                                   (length (pis₀ ++ pis-step))
    fit-pis-head =
      subst (λ cs → constraints-pis-fit cs (length (pis₀ ++ pis-step)))
            (sym (constraints-after-instr-eq {hc} i st₀))
            (constraints-pis-fit-++ (SynthState.constraints st₀)
              (instr-new-constraints hc st₀ i) (length (pis₀ ++ pis-step))
              fit-pis-prior fit-pis-new)

    -- The full witness satisfies `constraints st₁` (split off the tail
    -- constraints), re-associated so the head suffix is exposed.
    sat-full-st₁ : satisfies-constraints (SynthState.constraints st₁)
        (mk-witness ((mem₀ ++ mem-step) ++ mem-tail)
                    ((pis₀ ++ pis-step) ++ pis-tail)
                    (comm-rand-of pre))
    sat-full-st₁ =
      let tail , tl-eq = constraints-after-instrs-extends {hc} is' st₁
          sat-decomp : satisfies-constraints (SynthState.constraints st₁ ++ tail)
                         (mk-witness (mem₀ ++ (mem-step ++ mem-tail))
                                     (pis₀ ++ (pis-step ++ pis-tail))
                                     (comm-rand-of pre))
          sat-decomp = subst (λ cs → satisfies-constraints cs _) tl-eq sat
      in subst₂ (λ m p → satisfies-constraints (SynthState.constraints st₁)
                           (mk-witness m p (comm-rand-of pre)))
                (sym (++-assoc mem₀ mem-step mem-tail))
                (sym (++-assoc pis₀ pis-step pis-tail))
                (proj₁ (satisfies-constraints-split (SynthState.constraints st₁)
                          tail sat-decomp))

    -- Shrink off the tail suffixes to obtain the head witness D1 wants.
    sat-head-mem : satisfies-constraints (SynthState.constraints st₁)
        (mk-witness (mem₀ ++ mem-step) ((pis₀ ++ pis-step) ++ pis-tail)
                    (comm-rand-of pre))
    sat-head-mem =
      satisfies-constraints-mem-shrink (SynthState.constraints st₁)
        (mem₀ ++ mem-step) mem-tail fit-mem-head sat-full-st₁
    sat-head : satisfies-constraints (SynthState.constraints st₁)
        (mk-witness (mem₀ ++ mem-step) (pis₀ ++ pis-step) (comm-rand-of pre))
    sat-head =
      satisfies-constraints-pis-shrink (SynthState.constraints st₁)
        (pis₀ ++ pis-step) pis-tail fit-pis-head sat-head-mem

    -- D1 applied to the head.
    s₁ = next-state-from-osd i pre s₀ mem-step pis-step sd
    d1-out :
        (Preprocessed.memory s₁ ≡ mem₀ ++ mem-step)
      × (Preprocessed.pis    s₁ ≡ pis₀ ++ pis-step)
      × R-instr pre s₀ i s₁
    d1-out = satisfies→R-instr-step {hc} pre s₀ i st₀ mem-step pis-step {wf2 = wf2-head}
               mi pii wc {bk = bk₀} {bm = bm₀} o2-inv o3-inv o2chk o3chk sd
               sat-head

    mem-eq₁ , pis-eq₁ , r-head = d1-out

    -- Post-step invariants for the recursion.
    mi₁ : mem-inv s₁ st₁
    mi₁ = mem-inv-next {hc} i pre s₀ st₀ mem-step pis-step sd mi ms-len ps-len
    pii₁ : pi-inv hc s₁ st₁
    pii₁ = pi-inv-next {hc} i pre s₀ st₀ mem-step pis-step sd pii ms-len ps-len

    -- The recorded O2 / O3 accumulators after the step, transported to `s₁`
    -- and re-indexed so the first component reads `n₁`.
    o2-fst : proj₁ o2acc' ≡ n₀ + Δmem i
    o2-fst = o2-step-fst i {n = n₀} {bk = bk₀} o2se
    o3-fst : proj₁ o3acc' ≡ n₀ + Δmem i
    o3-fst = o3-step-fst i {n = n₀} {bm = bm₀} o3se
    o2acc-eq : o2acc' ≡ (n₁ , proj₂ o2acc')
    o2acc-eq = cong (_, proj₂ o2acc') (trans o2-fst (sym nw-eq))
    o3acc-eq : o3acc' ≡ (n₁ , proj₂ o3acc')
    o3acc-eq = cong (_, proj₂ o3acc') (trans o3-fst (sym nw-eq))

    o2-inv₁ : O2-Inv (n₁ , proj₂ o2acc') s₁
    o2-inv₁ = subst (λ a → O2-Inv a s₁) o2acc-eq (o2-preserve o2-inv r-head o2se)
    o3-inv₁ : O3-Inv (n₁ , proj₂ o3acc') s₁
    o3-inv₁ = subst (λ a → O3-Inv a s₁) o3acc-eq (o3-preserve o3-inv r-head o3se)

    o2-tail₁ : O2-Trace is' (n₁ , proj₂ o2acc') final-o2
    o2-tail₁ = subst (λ a → O2-Trace is' a final-o2) o2acc-eq o2-tail
    o3-tail₁ : O3-Trace is' (n₁ , proj₂ o3acc') final-o3
    o3-tail₁ = subst (λ a → O3-Trace is' a final-o3) o3acc-eq o3-tail
    w-tail₁ : Wire-Trace is' n₁ final-w
    w-tail₁ = subst (λ k → Wire-Trace is' k final-w)
                (trans (wire-step-fst i n₀ wse) (sym nw-eq)) w-tail

    -- The full witness over `s₁`'s mem / pis plus the tails.
    sat-rec : satisfies-constraints
        (SynthState.constraints (circuit-instrs hc is' st₁))
        (mk-witness (Preprocessed.memory s₁ ++ mem-tail)
                    (Preprocessed.pis    s₁ ++ pis-tail)
                    (comm-rand-of pre))
    sat-rec =
      subst₂ (λ m p → satisfies-constraints
                        (SynthState.constraints (circuit-instrs hc is' st₁))
                        (mk-witness m p (comm-rand-of pre)))
             (sym (trans (cong (_++ mem-tail) mem-eq₁)
                         (++-assoc mem₀ mem-step mem-tail)))
             (sym (trans (cong (_++ pis-tail) pis-eq₁)
                         (++-assoc pis₀ pis-step pis-tail)))
             sat

    -- Recurse.
    fit-pis-rec : constraints-pis-fit (SynthState.constraints st₁)
                                  (length (Preprocessed.pis s₁))
    fit-pis-rec = subst (λ p → constraints-pis-fit (SynthState.constraints st₁)
                                 (length p))
                        (sym pis-eq₁) fit-pis-head

    -- Recurse.
    rec = satisfies-constraints→R-instrs {hc} pre s₁ is' st₁ mem-tail pis-tail
            mi₁ pii₁ fit-mem-st₁-nw fit-pis-rec
            {bk₀ = proj₂ o2acc'} {bm₀ = proj₂ o3acc'}
            o2-inv₁ o3-inv₁ o2-tail₁ o3-tail₁ w-tail₁ wf2-tail osd-tail sat-rec

    s' , mem-eq' , pis-eq' , r-tail , fold-eq' = rec
  in
    s'
    , trans mem-eq'
        (trans (cong (_++ mem-tail) mem-eq₁)
               (++-assoc mem₀ mem-step mem-tail))
    , trans pis-eq'
        (trans (cong (_++ pis-tail) pis-eq₁)
               (++-assoc pis₀ pis-step pis-tail))
    , r-step r-head r-tail
    -- `osd-fold (osd-cons sd osd-tail)` reduces to `osd-fold osd-tail`,
    -- which is `rec`'s fold endpoint.  But `rec` was taken at `s₁ =
    -- next-state-from-osd i …` whereas the osd-tail in THIS list is
    -- rooted at the same `s₁` — the two `osd-fold`s coincide definitionally.
    , fold-eq'

------------------------------------------------------------------------
-- Part 2′.  The transcript-consistency predicate `preprocess-shaped`.
--
-- The backward direction (`circuit-faithful-bwd`) over an *arbitrary*
-- `s : Preprocessed` is unprovable because in-circuit satisfaction
-- (`satisfies`) is *blind* to the transcript-read wires:
-- `public-input` / `private-input` emit no
-- constraint for the value read off the transcript, and `pi-skip`'s
-- transcript-match check has no in-circuit shadow.  The spec (§5.3,
-- line 809) sidesteps this by quantifying `Σ` over "preprocess-state-
-- shaped assignments"; the predicate below is the Agda rendering of
-- that quantifier restriction.
--
-- `preprocess-shaped src pre s` asserts the existence of an operational
-- *shape* trace from the initial state to `s` in which:
--
--   • transcript-read instructions (`public-input` / `private-input`
--     when active, `pi-skip` when active) consume the preimage
--     transcripts in order and pass their guard / prefix-match checks;
--   • EVERY OTHER instruction merely appends some *free* memory / pis
--     cells of the correct arity — their VALUES are left UNCONSTRAINED.
--
-- Design notes.
--
--   • NON-VACUOUS / NON-CIRCULAR.  This is strictly WEAKER than
--     `R-instrs pre s₀ instrs s`: a `tr-step` for `add` / `mul` /
--     `ec-add` / `hash` / `declare-pub-input` / … pins only the
--     *length* of the appended suffix (`w ∷ []`, `x ∷ y ∷ []`), NEVER
--     the computed value (`av +ᶠ bv`, `ec-add-pts …`, …).  Those values
--     are pinned by `satisfies` via D1/D2.  So `satisfies` remains fully
--     load-bearing.  It does NOT hand back an `R-instrs` trace.
--
--   • IMPLEMENTATION-INDEPENDENT.  The predicate is stated purely in
--     `Semantics` vocabulary (`Preprocessed`, `mem-lookup`, `eval-guard`,
--     `consume-pub-out`, `consume-priv`, `≡ᶠ-list?`).  It mentions no
--     in-circuit construct (`SynthState`, constraints, `op-side-data`).
--
--   • STRONG ENOUGH.  The per-step `tr-step` payloads are arranged to
--     coincide *definitionally* with the corresponding `op-side-data`
--     payloads and `tr-next` with `next-state-from-osd`, so the internal
--     bridge converts a `Tr-shaped` trace to an `op-side-data-list`
--     structurally (two cases), supplying exactly the transcript gaps
--     D2 needs.
------------------------------------------------------------------------

-- Per-step shape obligation and post-state, as definitional aliases of
-- `op-side-data` / `next-state-from-osd` (same payloads, same
-- `Semantics` vocabulary).  Every downstream reference (`Tr-shaped`,
-- `R-instr→tr-step`, `tr-step-mem`/`-pis`, the fold) reduces through the
-- alias to the original, so the internal bridge to `op-side-data` is
-- the identity.
tr-step : Instruction → ProofPreimage → Preprocessed
        → (mem-suf pis-suf : List Fr) → Set
tr-step = op-side-data

tr-next : (i : Instruction) (pre : ProofPreimage) (s : Preprocessed)
          (mem-suf pis-suf : List Fr) → tr-step i pre s mem-suf pis-suf → Preprocessed
tr-next = next-state-from-osd

-- The list closure.  Structurally identical to `op-side-data-list` but
-- built from the clean `tr-step` / `tr-next` (which coincide
-- definitionally with `op-side-data` / `next-state-from-osd`), and
-- additionally ENDPOINT-INDEXED: `Tr-shaped pre s₀ is s ms ps` says the
-- shape walk from `s₀` over `is`, appending `ms` / `ps`, ends exactly at
-- `s`.  The endpoint index pins ALL of `s`'s fields (memory, pis, and the
-- transcript bookkeeping) — this is what the backward direction needs to
-- recover `R src pre s` for the GIVEN `s` (not merely a state with the
-- same memory/pis prefix).
data Tr-shaped (pre : ProofPreimage) :
       (s₀ : Preprocessed) (is : List Instruction) (s : Preprocessed)
       (mem-suf pis-suf : List Fr) → Set where
  tr-nil  : ∀ {s} → Tr-shaped pre s [] s [] []
  tr-cons : ∀ {s₀ i is s mem-step pis-step mem-tail pis-tail}
    → (sd : tr-step i pre s₀ mem-step pis-step)
    → Tr-shaped pre (tr-next i pre s₀ mem-step pis-step sd) is s mem-tail pis-tail
    → Tr-shaped pre s₀ (i ∷ is) s (mem-step ++ mem-tail) (pis-step ++ pis-tail)

-- A shape walk with existential suffixes: some memory / pis suffixes
-- together with a `Tr-shaped` walk appending exactly those.
record Shape-walk (pre : ProofPreimage) (s₀ : Preprocessed)
                  (is : List Instruction) (s : Preprocessed) : Set where
  constructor mk-shape-walk
  field
    mem-suf : List Fr
    pis-suf : List Fr
    walk    : Tr-shaped pre s₀ is s mem-suf pis-suf

-- The user-facing predicate: there is an initial state `start` and a
-- shape walk from `start` over the instruction stream that ends EXACTLY
-- at `s`, consuming the transcripts.  The suffixes are existential
-- (fields of the `Shape-walk`).
--
-- The conjunct `T (transcripts-consumed pre s)` is required because the
-- `Tr-shaped` walk alone does NOT force the three transcript cursors of
-- `s` to be fully consumed (a walk in which every `public-input` /
-- `private-input` / `pi-skip` guard is inactive consumes nothing), and
-- `satisfies` is blind to the cursors.  Yet `R src pre s` carries
-- `T (transcripts-consumed pre s)` as a top-level conjunct
-- (Semantics.agda:632), so the backward direction must reproduce it.  It
-- is not derivable from `satisfies` + the trace + producer-safety
-- (none of O1/O2/O3/wire-disc constrains the transcript cursors), so —
-- like the WF1 arity hypothesis and the transcript-read-wire blindness
-- handled by the trace — it must be supplied.  This is faithful to the
-- spec §5.3 "preprocess-state-shaped Σ": those states are reached by a
-- SUCCESSFUL `preprocess`, which by definition passed
-- `transcripts-consumed` (Semantics.agda:452).  `comm-ok` is NOT folded
-- in: it is recoverable from `satisfies` (the `comm`
-- constraint; see part 4 below), so it stays derived to keep `satisfies`
-- load-bearing.
record preprocess-shaped (src : IrSource) (pre : ProofPreimage)
                         (s : Preprocessed) : Set where
  constructor mk-shaped
  field
    start    : Preprocessed
    init≡    : init-state src pre ≡ just start
    shaped   : Shape-walk pre start (IrSource.instructions src) s
    consumed : T (transcripts-consumed pre s)

------------------------------------------------------------------------
-- `R ⇒ preprocess-shaped`.
--
-- The forward field of the bundled `circuit-faithful` `↔` produces
-- `satisfies` from `R`; the extra `preprocess-shaped` hypothesis it is
-- handed is then redundant.  But the *bundle's* statement carries
-- `preprocess-shaped` as a top-level hypothesis (so the two directions
-- share preconditions), and we want it derivable from `R` directly so
-- callers in possession of `R` need not supply it separately.  This
-- lemma provides that: an `R-instrs` trace is a fortiori a `Tr-shaped`
-- trace (it pins strictly more — including the computed values).
------------------------------------------------------------------------

private
  -- `push-mem2 s x y ≡ push-mem (push-mem s x) y`.  Both set only the
  -- `memory` field; they differ by `++`-associativity on the suffix.
  push-mem2-iter : ∀ (s : Preprocessed) x y
    → push-mem2 s x y ≡ push-mem (push-mem s x) y
  push-mem2-iter s x y =
    cong (λ m → record s { memory = m })
         (push-mem2-assoc (Preprocessed.memory s) x y)

  -- Per-step conversion.  From `R-instr pre s i s'` build the shape
  -- payload `sd` plus the suffix decomposition and a proof that
  -- `tr-next … sd ≡ s'`.
  R-instr→tr-step
    : ∀ (pre : ProofPreimage) (s : Preprocessed) (i : Instruction) (s' : Preprocessed)
    → R-instr pre s i s'
    → Σ (List Fr) (λ ms → Σ (List Fr) (λ ps →
          (Preprocessed.memory s' ≡ Preprocessed.memory s ++ ms)
        × (Preprocessed.pis    s' ≡ Preprocessed.pis    s ++ ps)
        × Σ (tr-step i pre s ms ps) (λ sd →
            tr-next i pre s ms ps sd ≡ s')))
  R-instr→tr-step pre s .(assert _) .s (r-assert _) =
    [] , [] , sym (++-identityʳ _) , sym (++-identityʳ _) , (refl , refl) , refl
  R-instr→tr-step pre s .(constrain-bits _ _) .s (r-constrain-bits _ _ _) =
    [] , [] , sym (++-identityʳ _) , sym (++-identityʳ _) , (refl , refl) , refl
  R-instr→tr-step pre s .(constrain-eq _ _) .s (r-constrain-eq _ _ _) =
    [] , [] , sym (++-identityʳ _) , sym (++-identityʳ _) , (refl , refl) , refl
  R-instr→tr-step pre s .(constrain-to-boolean _) .s (r-constrain-to-boolean _) =
    [] , [] , sym (++-identityʳ _) , sym (++-identityʳ _) , (refl , refl) , refl
  -- push-mem cases (Δmem=1): ms = [the appended value], ps = [].
  R-instr→tr-step pre s (cond-select _ _ _) _ (r-cond-select {sel = sel} {av} {bv} _ _ _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (copy _) _ (r-copy {v = v} _) =
    v ∷ [] , [] , refl , sym (++-identityʳ _) , ((v , refl) , refl) , refl
  R-instr→tr-step pre s (load-imm _) _ (r-load-imm {imm = imm}) =
    imm ∷ [] , [] , refl , sym (++-identityʳ _) , ((imm , refl) , refl) , refl
  R-instr→tr-step pre s (test-eq _ _) _ (r-test-eq {av = av} {bv} _ _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (transient-hash _) _ (r-transient-hash {vs = vs} _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (add _ _) _ (r-add {av = av} {bv} _ _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (mul _ _) _ (r-mul {av = av} {bv} _ _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (neg _) _ (r-neg {av = av} _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (not _) _ (r-not {b = b} _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (less-than _ _ _) _ (r-less-than {av = av} {bv} _ _ _ _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  R-instr→tr-step pre s (reconstitute-field _ _ _) _ (r-reconstitute-field {dv = dv} {mv} _ _ _ _ _) =
    _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , refl) , refl) , refl
  -- push-mem2 cases (Δmem=2): ms = [x, y], ps = [].
  R-instr→tr-step pre s (ec-add _ _ _ _) _ (r-ec-add {cx = cx} {cy} _ _ _ _ _) =
    _ ∷ _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , _ , refl) , refl) , refl
  R-instr→tr-step pre s (ec-mul _ _ _) _ (r-ec-mul {cx = cx} {cy} _ _ _ _) =
    _ ∷ _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , _ , refl) , refl) , refl
  R-instr→tr-step pre s (ec-mul-generator _) _ (r-ec-mul-generator {cx = cx} {cy} _ _) =
    _ ∷ _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , _ , refl) , refl) , refl
  R-instr→tr-step pre s (hash-to-curve _) _ (r-hash-to-curve {cx = cx} {cy} _ _) =
    _ ∷ _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , _ , refl) , refl) , refl
  R-instr→tr-step pre s (persistent-hash _ _) _ (r-persistent-hash {h₁ = h₁} {h₂} _ _) =
    _ ∷ _ ∷ [] , [] , refl , sym (++-identityʳ _) , ((_ , _ , refl) , refl) , refl
  R-instr→tr-step pre s (div-mod-power-of-two var bits) _ (r-div-mod-power-of-two {v = v} la _) =
    let d = from-le-bits (drop bits (to-le-bits v))
        m = from-le-bits (take bits (to-le-bits v))
    in d ∷ m ∷ [] , []
       , sym (push-mem2-assoc (Preprocessed.memory s) d m)
       , sym (++-identityʳ _)
       , ((d , m , refl) , refl)
       , push-mem2-iter s d m
  -- declare-pub-input (Δpis=1): ms = [], ps = [the value].
  R-instr→tr-step pre s (declare-pub-input _) _ (r-declare-pub-input {v = v} _) =
    [] , v ∷ [] , sym (++-identityʳ _) , refl , (refl , v , refl) , refl
  -- output: no suffix; carry the lookup evidence.
  R-instr→tr-step pre s (output _) _ (r-output {v = v} lk) =
    [] , [] , sym (++-identityʳ _) , sym (++-identityʳ _)
    , ((v , lk) , refl , refl) , refl
  -- pi-skip active / inactive.
  R-instr→tr-step pre s (pi-skip _ _) _ (r-pi-skip-active g-eq match) =
    [] , [] , sym (++-identityʳ _) , sym (++-identityʳ _)
    , (refl , refl , (true , g-eq , match)) , refl
  R-instr→tr-step pre s (pi-skip _ count) _ (r-pi-skip-inactive g-eq) =
    [] , [] , sym (++-identityʳ _) , sym (++-identityʳ _)
    , (refl , refl , (false , g-eq , tt)) , refl
  -- public-input active / inactive.
  R-instr→tr-step pre s (public-input _) _ (r-public-input-active {v = v} {s₁} g-eq c-eq) =
    v ∷ [] , []
    , cong (_++ (v ∷ [])) (consume-pub-out-mem s c-eq)
    , trans (consume-pub-out-pis s c-eq) (sym (++-identityʳ _))
    , (v , refl , refl , (true , g-eq , (s₁ , c-eq))) , refl
  R-instr→tr-step pre s (public-input _) _ (r-public-input-inactive g-eq) =
    0ᶠ ∷ [] , [] , refl , sym (++-identityʳ _)
    , (0ᶠ , refl , refl , (false , g-eq , refl)) , refl
  -- private-input active / inactive.
  R-instr→tr-step pre s (private-input _) _ (r-private-input-active {v = v} {s₁} g-eq c-eq) =
    v ∷ [] , []
    , cong (_++ (v ∷ [])) (consume-priv-mem s c-eq)
    , trans (consume-priv-pis s c-eq) (sym (++-identityʳ _))
    , (v , refl , refl , (true , g-eq , (s₁ , c-eq))) , refl
  R-instr→tr-step pre s (private-input _) _ (r-private-input-inactive g-eq) =
    0ᶠ ∷ [] , [] , refl , sym (++-identityʳ _)
    , (0ᶠ , refl , refl , (false , g-eq , refl)) , refl

  -- Fold the per-step conversion along an `R-instrs` trace.  The
  -- endpoint `s` is pinned by the trace, so the result carries it as the
  -- `Tr-shaped` endpoint index (no memory/pis equations needed here).
  R-instrs→Shape-walk
    : ∀ (pre : ProofPreimage) (s₀ s : Preprocessed) (is : List Instruction)
    → R-instrs pre s₀ is s
    → Shape-walk pre s₀ is s
  R-instrs→Shape-walk pre s₀ .s₀ [] r-done = mk-shape-walk [] [] tr-nil
  R-instrs→Shape-walk pre s₀ s (i ∷ is) (r-step {s₁ = s₁} r-head r-tail) =
    let ms , ps , _mem-eq₁ , _pis-eq₁ , sd , tn-eq = R-instr→tr-step pre s₀ i s₁ r-head
        -- The tail trace starts from `s₁`; rewrite it to start from
        -- `tr-next … sd` (which equals `s₁` by `tn-eq`).
        r-tail' : R-instrs pre (tr-next i pre s₀ ms ps sd) is s
        r-tail' = subst (λ z → R-instrs pre z is s) (sym tn-eq) r-tail
        (mk-shape-walk ms-t ps-t tr-tail) =
          R-instrs→Shape-walk pre (tr-next i pre s₀ ms ps sd) s is r-tail'
    in mk-shape-walk (ms ++ ms-t) (ps ++ ps-t) (tr-cons sd tr-tail)

-- `R ⇒ preprocess-shaped`.
R⇒preprocess-shaped : ∀ (src : IrSource) (pre : ProofPreimage) (s : Preprocessed)
  → R src pre s → preprocess-shaped src pre s
R⇒preprocess-shaped src pre s (s₀ , init-eq , Rs , tc , _co) =
  mk-shaped s₀ init-eq
    (R-instrs→Shape-walk pre s₀ s (IrSource.instructions src) Rs) tc

------------------------------------------------------------------------
-- Internal bridge, step 0:  `Tr-shaped` ⇒ `op-side-data-list`.
--
-- Because `tr-step` / `tr-next` are definitional aliases of
-- `op-side-data` / `next-state-from-osd`, the two relations have
-- definitionally-equal index expressions.  The
-- conversion is therefore a trivial structural map (two cases): a
-- `tr-step` payload IS an `op-side-data` payload, and
-- `tr-next … sd` reduces to `next-state-from-osd … sd`.
------------------------------------------------------------------------

private
  -- After matching `i` to a concrete constructor, `tr-step i` and
  -- `op-side-data i` reduce to the SAME RHS (they are aliases), and
  -- likewise `tr-next i` ≡ `next-state-from-osd i`.  So in each case the
  -- `tr-step` payload `sd` IS an `op-side-data` payload and the tail's
  -- start state coincides — `osd-cons sd (rec tr-tail)` typechecks
  -- directly with no transport (for `pi-skip` / `public-input` /
  -- `private-input` the recursion is transported by `tr-next≡nso`).
  Tr-shaped→osd-list
    : ∀ (pre : ProofPreimage) (s₀ : Preprocessed) (is : List Instruction)
        (s : Preprocessed) (mem-suf pis-suf : List Fr)
    → Tr-shaped pre s₀ is s mem-suf pis-suf
    → op-side-data-list pre s₀ is mem-suf pis-suf
  Tr-shaped→osd-list pre s₀ .[] .s₀ .[] .[] tr-nil = osd-nil
  Tr-shaped→osd-list pre s₀ (i ∷ is) s-end ._ ._
      (tr-cons {mem-step = ms} {pis-step = ps} sd t) =
    osd-cons {mem-step = ms} {pis-step = ps} sd
      (Tr-shaped→osd-list pre _ is s-end _ _ t)

  -- The fold endpoint of the bridged list equals the `Tr-shaped`
  -- endpoint index `s`.  This pins D2's existential `s'` (which D2 proves
  -- `≡ osd-fold osd`) to the GIVEN state `s` in `circuit-faithful-bwd`.
  --
  -- Proof by induction on `Tr-shaped`.  In each `tr-cons` case the bridge
  -- reduces to `osd-cons sd (rec t)` (matching the instruction), so
  -- `osd-fold` reduces to `osd-fold (rec t)`, discharged by the IH on
  -- `t`.  The endpoint index `s-end` is threaded unchanged, so the IH
  -- gives `osd-fold (rec t) ≡ s-end` directly.
  Tr-shaped→osd-list-fold
    : ∀ (pre : ProofPreimage) (s₀ : Preprocessed) (is : List Instruction)
        (s : Preprocessed) (mem-suf pis-suf : List Fr)
    → (tr : Tr-shaped pre s₀ is s mem-suf pis-suf)
    → osd-fold (Tr-shaped→osd-list pre s₀ is s mem-suf pis-suf tr) ≡ s
  Tr-shaped→osd-list-fold pre s₀ .[] .s₀ .[] .[] tr-nil = refl
  Tr-shaped→osd-list-fold pre s₀ (i ∷ is) s-end ._ ._ (tr-cons sd t) =
    Tr-shaped→osd-list-fold pre _ is s-end _ _ t

  -- Per-step memory equation:  `memory (tr-next i …) ≡ memory s ++ ms`.
  -- Mirrors `tr-next`; the `sd` payload supplies `ms`'s shape, and for
  -- the transcript-active cases `consume-*-mem` shows the consumed state
  -- leaves memory unchanged.
  -- `xs ≡ xs ++ ys` for an empty suffix, and `xs ++ zs ≡ xs ++ ys` for
  -- `ys ≡ zs`.  These isolate the append plumbing that every per-step
  -- memory / pis clause shares; each clause differs only in how it
  -- projects the suffix equation out of the side-data payload `sd`.
  cat-refl : ∀ {A : Set} (xs : List A) {ys} → ys ≡ [] → xs ≡ xs ++ ys
  cat-refl xs eq = sym (trans (cong (xs ++_) eq) (++-identityʳ _))

  cat-cong : ∀ {A : Set} (xs : List A) {ys zs} → ys ≡ zs → xs ++ zs ≡ xs ++ ys
  cat-cong xs eq = cong (xs ++_) (sym eq)

  tr-step-mem
    : ∀ (i : Instruction) (pre : ProofPreimage) (s : Preprocessed)
        (ms ps : List Fr) (sd : tr-step i pre s ms ps)
    → Preprocessed.memory (tr-next i pre s ms ps sd)
        ≡ Preprocessed.memory s ++ ms
  tr-step-mem i pre s ms ps = go (shapeView i)
    where
    go : ∀ {j} (v : ShapeView j) (sd : osd-of v pre s ms ps)
       → Preprocessed.memory (nso′ v pre s ms ps sd)
           ≡ Preprocessed.memory s ++ ms
    go sv-pure0   (mn , _)           = cat-refl (Preprocessed.memory s) mn
    go sv-pure1   ((w , me) , _)     = cat-cong (Preprocessed.memory s) me
    go sv-pure2   ((x , y , me) , _) = cat-cong (Preprocessed.memory s) me
    go sv-declare (mn , _)           = cat-refl (Preprocessed.memory s) mn
    go sv-output  ((_ , _) , mn , _) = cat-refl (Preprocessed.memory s) mn
    go sv-skip (mn , _ , (true , _ , _)) = cat-refl (Preprocessed.memory s) mn
    go sv-skip (mn , _ , (false , _ , _)) = cat-refl (Preprocessed.memory s) mn
    go sv-pub-in  (w , me , _ , (true , _ , (s₁ , ce))) =
      trans (cong (_++ (w ∷ [])) (consume-pub-out-mem s ce))
            (cat-cong (Preprocessed.memory s) me)
    go sv-pub-in  (w , me , _ , (false , _ , _)) =
      cat-cong (Preprocessed.memory s) me
    go sv-priv-in (w , me , _ , (true , _ , (s₁ , ce))) =
      trans (cong (_++ (w ∷ [])) (consume-priv-mem s ce))
            (cat-cong (Preprocessed.memory s) me)
    go sv-priv-in (w , me , _ , (false , _ , _)) =
      cat-cong (Preprocessed.memory s) me

  -- Per-step pis equation:  `pis (tr-next i …) ≡ pis s ++ ps`.
  tr-step-pis
    : ∀ (i : Instruction) (pre : ProofPreimage) (s : Preprocessed)
        (ms ps : List Fr) (sd : tr-step i pre s ms ps)
    → Preprocessed.pis (tr-next i pre s ms ps sd)
        ≡ Preprocessed.pis s ++ ps
  tr-step-pis i pre s ms ps = go (shapeView i)
    where
    go : ∀ {j} (v : ShapeView j) (sd : osd-of v pre s ms ps)
       → Preprocessed.pis (nso′ v pre s ms ps sd)
           ≡ Preprocessed.pis s ++ ps
    go sv-pure0   (_ , pn)         = cat-refl (Preprocessed.pis s) pn
    go sv-pure1   (_ , pn)         = cat-refl (Preprocessed.pis s) pn
    go sv-pure2   (_ , pn)         = cat-refl (Preprocessed.pis s) pn
    go sv-declare (_ , wv , pe)    = cat-cong (Preprocessed.pis s) pe
    go sv-output  ((_ , _) , _ , pn) = cat-refl (Preprocessed.pis s) pn
    go sv-skip    (_ , pn , (true  , _ , _)) = cat-refl (Preprocessed.pis s) pn
    go sv-skip    (_ , pn , (false , _ , _)) = cat-refl (Preprocessed.pis s) pn
    go sv-pub-in  (w , _ , pn , (true , _ , (s₁ , ce))) =
      trans (consume-pub-out-pis s ce) (cat-refl (Preprocessed.pis s) pn)
    go sv-pub-in  (w , _ , pn , (false , _ , _)) =
      cat-refl (Preprocessed.pis s) pn
    go sv-priv-in (w , _ , pn , (true , _ , (s₁ , ce))) =
      trans (consume-priv-pis s ce) (cat-refl (Preprocessed.pis s) pn)
    go sv-priv-in (w , _ , pn , (false , _ , _)) =
      cat-refl (Preprocessed.pis s) pn

  -- Fold the per-step memory / pis equations along a `Tr-shaped` trace.
  Tr-shaped→mem
    : ∀ (pre : ProofPreimage) (s₀ s : Preprocessed) (is : List Instruction)
        (ms ps : List Fr)
    → Tr-shaped pre s₀ is s ms ps
    → Preprocessed.memory s ≡ Preprocessed.memory s₀ ++ ms
  Tr-shaped→mem pre s₀ .s₀ .[] .[] .[] tr-nil = sym (++-identityʳ _)
  Tr-shaped→mem pre s₀ s (i ∷ is) ._ ._
      (tr-cons {mem-step = mst} {pis-step = pst} {mem-tail = mtl} sd t) =
    let s₁ = tr-next i pre s₀ mst pst sd in
    trans (Tr-shaped→mem pre s₁ s is mtl _ t)
      (trans (cong (_++ mtl) (tr-step-mem i pre s₀ mst pst sd))
             (++-assoc (Preprocessed.memory s₀) mst mtl))

  Tr-shaped→pis
    : ∀ (pre : ProofPreimage) (s₀ s : Preprocessed) (is : List Instruction)
        (ms ps : List Fr)
    → Tr-shaped pre s₀ is s ms ps
    → Preprocessed.pis s ≡ Preprocessed.pis s₀ ++ ps
  Tr-shaped→pis pre s₀ .s₀ .[] .[] .[] tr-nil = sym (++-identityʳ _)
  Tr-shaped→pis pre s₀ s (i ∷ is) ._ ._
      (tr-cons {mem-step = mst} {pis-step = pst} {pis-tail = ptl} sd t) =
    let s₁ = tr-next i pre s₀ mst pst sd in
    trans (Tr-shaped→pis pre s₁ s is _ ptl t)
      (trans (cong (_++ ptl) (tr-step-pis i pre s₀ mst pst sd))
             (++-assoc (Preprocessed.pis s₀) pst ptl))

-- ───────────────────────────────────────────────────────────────────
-- The backward direction.
--
-- `circuit-faithful-bwd` is FALSE over an *arbitrary* `s : Preprocessed`
-- for two reasons, each addressed by an extra hypothesis:
--
--   • `satisfies` is blind to transcript-read wires (`public-input` /
--     `private-input` active emit no constraint; `pi-skip` active's
--     transcript-match has no in-circuit shadow), so an arbitrary `s`
--     can satisfy the circuit while its transcript wires hold garbage.
--     The `preprocess-shaped` hypothesis (§5.3) pins the operational
--     shape walk ending exactly at `s`.
--
--   • `T (transcripts-consumed pre s)` is a top-level conjunct of `R`
--     (Semantics.agda:632) yet is NOT entailed by `satisfies` + the
--     trace + producer-safety (no obligation constrains the transcript
--     cursors; an all-inactive walk consumes nothing).  It is folded
--     into `preprocess-shaped` (faithful: §5.3 quantifies over states
--     reached by a SUCCESSFUL `preprocess`, which passed
--     `transcripts-consumed`).
--
-- With those two and WF1 (§3.3) in hand the proof runs:
--   1. `preprocess-shaped` ⇒ its fields: the start state `s₀`, its
--      `init-state` equation, the shape walk `(ms, ps, tr)`, and `tc`.
--   2. `osd = Tr-shaped→osd-list … tr`;  memory / pis of `s` reshape as
--      `memory s₀ ++ ms` / `pis s₀ ++ ps` (`Tr-shaped→mem` / `→pis`).
--   3. invariants at the initial synth state `st₀` (mem-inv, pi-inv,
--      O2/O3-Inv via `o2/o3-inv-init`, the three traces via
--      `O2/O3-bool→Runs` ∘ `producer-safe-O2/-O3` and `wire-disc-sound`,
--      fits ≡ refl since `constraints st₀ ≡ []`).
--   4. invert `satisfies` (split off the comm constraint when hc) to feed the
--      body `satisfies-constraints` to D2 (`satisfies-constraints→R-instrs`).
--   5. D2 ⇒ `s'`, mem/pis eqs, `R-instrs pre s₀ instrs s'`, `s' ≡ osd-fold osd`.
--   6. pin `s' ≡ s`: `s' ≡ osd-fold osd ≡ s` (`Tr-shaped→osd-list-fold`),
--      so `subst` the trace to end at `s`.
--   7. `T (transcripts-consumed pre s)` = the `tc` hypothesis.
--   8. `T (comm-ok src pre s)` by inverting the comm constraint
--      (`inputs-lookup-init`, `output-wires-coincide`, `init-state-pi-1`,
--      `≡ᶠ?-refl`), mirroring the forward `circuit-faithful-fwd-true`.
-- ───────────────────────────────────────────────────────────────────

private
  -- D2 packaged for the body, returning the trace pinned to end at the
  -- GIVEN `s` (via `s' ≡ osd-fold osd ≡ s`).  Independent of `hc`'s comm
  -- constraint: it consumes the *body* constraint satisfaction only.
  bwd-body-trace
    : ∀ {hc} (pre : ProofPreimage) (src : IrSource) (s s₀ : Preprocessed)
        (ms ps : List Fr)
    → T (producer-safe src)
    → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src
    → WF2 src
    → init-state src pre ≡ just s₀
    → (tr : Tr-shaped pre s₀ (IrSource.instructions src) s ms ps)
    → IrSource.do-communications-commitment src ≡ hc
    → satisfies-constraints
        (SynthState.constraints
          (circuit-instrs hc (IrSource.instructions src) (mk-synth (IrSource.num-inputs src) [] 0 [])))
        (mk-witness (Preprocessed.memory s₀ ++ ms)
                    (Preprocessed.pis s₀ ++ ps)
                    (comm-rand-of pre))
    → R-instrs pre s₀ (IrSource.instructions src) s
  bwd-body-trace {hc} pre src s s₀ ms ps ps-safe wf1 wf2 init-eq tr hc-eq sat-body =
    let
      n   = IrSource.num-inputs src
      st₀ = mk-synth n [] 0 []
      instrs = IrSource.instructions src
      mem≡   = init-state-memory src pre s₀ init-eq
      len-eq = init-state-num-inputs src pre s₀ init-eq
      mi₀ : mem-inv s₀ st₀
      mi₀ = sym (trans (cong length mem≡) len-eq)
      pi₀-pre : length (Preprocessed.pis s₀)
                  ≡ preamble-pi-count (IrSource.do-communications-commitment src)
      pi₀-pre = init-state-pis-length src pre s₀ init-eq
      pi₀ : pi-inv hc s₀ st₀
      pi₀ = subst (λ b → length (Preprocessed.pis s₀) ≡ preamble-pi-count b + 0)
                  hc-eq
                  (trans pi₀-pre
                         (sym (+-identityʳ (preamble-pi-count
                                 (IrSource.do-communications-commitment src)))))
      -- Initial obligation invariants at `s₀` (`bk₀ = bm₀ = []`).
      o2-inv₀ : O2-Inv (n , []) s₀
      o2-inv₀ = o2-inv-init {src} {pre} {s₀} init-eq wf1
      o3-inv₀ : O3-Inv (n , []) s₀
      o3-inv₀ = o3-inv-init {src} {pre} {s₀} init-eq wf1
      -- The three producer traces at `(n , [])` / `n`.
      o2-tr : O2-Trace instrs (n , []) (O2-Runs.final (O2-bool→Runs {src} (producer-safe-O2 {src} ps-safe)))
      o2-tr = O2-Runs.trace (O2-bool→Runs {src} (producer-safe-O2 {src} ps-safe))
      o3-tr : O3-Trace instrs (n , []) (O3-Runs.final (O3-bool→Runs {src} (producer-safe-O3 {src} ps-safe)))
      o3-tr = O3-Runs.trace (O3-bool→Runs {src} (producer-safe-O3 {src} ps-safe))
      w-tr : Wire-Trace instrs n (proj₁ (wire-disc-sound {src} ps-safe))
      w-tr = proj₂ (wire-disc-sound {src} ps-safe)
      -- The osd-list and the fold endpoint = `s`.
      osd : op-side-data-list pre s₀ instrs ms ps
      osd = Tr-shaped→osd-list pre s₀ instrs s ms ps tr
      fold≡s : osd-fold osd ≡ s
      fold≡s = Tr-shaped→osd-list-fold pre s₀ instrs s ms ps tr
      -- D2.
      d2 = satisfies-constraints→R-instrs {hc} pre s₀ instrs st₀ ms ps
             mi₀ pi₀ tt tt {bk₀ = []} {bm₀ = []} o2-inv₀ o3-inv₀
             o2-tr o3-tr w-tr wf2 osd sat-body
      s' , _ , _ , Rs' , fold-eq = d2
      s'≡s : s' ≡ s
      s'≡s = trans fold-eq fold≡s
    in subst (R-instrs pre s₀ instrs) s'≡s Rs'

  -- `comm-rand-of pre ≡ just r` when `comm-commitment pre ≡ just (c, r)`.
  comm-rand-of-just : ∀ (pre : ProofPreimage) c r
    → ProofPreimage.comm-commitment pre ≡ just (c , r)
    → comm-rand-of pre ≡ just r
  comm-rand-of-just pre c r eq = cong (Data.Maybe.map (λ (_ , r) → r)) eq

  -- `circuit src` reduces to its hc-specific record shape.  (These are
  -- the backward-usable forms of the forward's `circuit-instantiate-*`,
  -- lifted out of the body's `let` since `where` is illegal there.)
  circuit-eq-false : ∀ (src : IrSource)
    → IrSource.do-communications-commitment src ≡ false
    → circuit src ≡
      mk-circuit
        (SynthState.nr-wires (circuit-instrs false (IrSource.instructions src)
                                (mk-synth (IrSource.num-inputs src) [] 0 [])))
        (SynthState.constraints (circuit-instrs false (IrSource.instructions src)
                                (mk-synth (IrSource.num-inputs src) [] 0 [])))
        (1 + SynthState.nr-declared-pi (circuit-instrs false (IrSource.instructions src)
                                (mk-synth (IrSource.num-inputs src) [] 0 [])))
        false
  circuit-eq-false src refl = refl

  circuit-eq-true : ∀ (src : IrSource)
    → IrSource.do-communications-commitment src ≡ true
    → circuit src ≡
      mk-circuit
        (SynthState.nr-wires (circuit-instrs true (IrSource.instructions src)
                                (mk-synth (IrSource.num-inputs src) [] 0 [])))
        (SynthState.constraints (circuit-instrs true (IrSource.instructions src)
                                (mk-synth (IrSource.num-inputs src) [] 0 []))
          ∷ʳ comm (nat-range (IrSource.num-inputs src))
              (SynthState.output-wires (circuit-instrs true (IrSource.instructions src)
                                (mk-synth (IrSource.num-inputs src) [] 0 []))))
        (2 + SynthState.nr-declared-pi (circuit-instrs true (IrSource.instructions src)
                                (mk-synth (IrSource.num-inputs src) [] 0 [])))
        true
  circuit-eq-true src refl = refl

  -- `comm-ok` is `true` definitionally when `do-comm ≡ false`.
  comm-ok-false : ∀ (src : IrSource) (pre : ProofPreimage) (s : Preprocessed)
    → IrSource.do-communications-commitment src ≡ false
    → T (comm-ok src pre s)
  comm-ok-false src pre s e with IrSource.do-communications-commitment src | e
  ... | false | refl = tt

  -- Invert the comm constraint to recover `T (comm-ok src pre s)`.
  -- hc=false branch is `tt`; hc=true requires the `holds` witness.
  bwd-comm-ok-true
    : ∀ (src : IrSource) (pre : ProofPreimage) (s s₀ : Preprocessed) c r
    → IrSource.do-communications-commitment src ≡ true
    → ProofPreimage.comm-commitment pre ≡ just (c , r)
    → init-state src pre ≡ just s₀
    → R-instrs pre s₀ (IrSource.instructions src) s
    → holds (witness-of s pre)
        (comm (nat-range (IrSource.num-inputs src))
          (SynthState.output-wires
            (circuit-instrs true (IrSource.instructions src)
              (mk-synth (IrSource.num-inputs src) [] 0 []))))
    → T (comm-ok src pre s)
  bwd-comm-ok-true src pre s s₀ c r hc-true cc-just init-eq Rs
      (ivs , ovs , rv , pv , ivs-lk , ovs-lk , rand≡ , pi1≡ , pv≡tc) =
    let
      n  = IrSource.num-inputs src
      st₀ = mk-synth n [] 0 []
      instrs = IrSource.instructions src
      cm-inputs = nat-range n
      out-wires = SynthState.output-wires (circuit-instrs true instrs st₀)
      -- `ivs ≡ inputs pre`.
      ivs-init : mem-lookups (Preprocessed.memory s) cm-inputs
                   ≡ just (ProofPreimage.inputs pre)
      ivs-init = mem-lookups-mono-R-instrs pre s₀ s instrs cm-inputs
                   (ProofPreimage.inputs pre) Rs (inputs-lookup-init src pre s₀ init-eq)
      ivs≡ : ivs ≡ ProofPreimage.inputs pre
      ivs≡ = just-injective (trans (sym ivs-lk) ivs-init)
      -- `ovs ≡ outputs s`.
      ovs-coin : mem-lookups (Preprocessed.memory s) out-wires
                   ≡ just (Preprocessed.outputs s)
      ovs-coin = output-wires-coincide {hc = true} pre s₀ s instrs st₀ Rs refl
                   (init-state-outputs src pre s₀ init-eq)
      ovs≡ : ovs ≡ Preprocessed.outputs s
      ovs≡ = just-injective (trans (sym ovs-lk) ovs-coin)
      -- `rv ≡ r`: `comm-rand-of pre ≡ just r` (cc-just) and `≡ just rv`.
      rof≡ : comm-rand-of pre ≡ just r
      rof≡ = comm-rand-of-just pre c r cc-just
      rv≡ : rv ≡ r
      rv≡ = just-injective (trans (sym rand≡) rof≡)
      -- `pv ≡ c`: `pi-lookup (pis s) 1 ≡ just c` and `≡ just pv`.
      pi1-init : pi-lookup (Preprocessed.pis s₀) 1 ≡ just c
      pi1-init = init-state-pi-1 src pre s₀ c r hc-true cc-just init-eq
      pi1-final : pi-lookup (Preprocessed.pis s) 1 ≡ just c
      pi1-final = pi-lookup-mono-R-instrs pre s₀ s instrs 1 c Rs pi1-init
      pv≡c : pv ≡ c
      pv≡c = just-injective (trans (sym pi1≡) pi1-final)
      -- `c ≡ transient-commit (inputs pre ++ outputs s) r`.
      c≡tc : c ≡ transient-commit (ProofPreimage.inputs pre ++ Preprocessed.outputs s) r
      c≡tc = trans (sym pv≡c)
               (trans pv≡tc
                 (cong₂ (λ vs rr → transient-commit vs rr)
                        (cong₂ _++_ ivs≡ ovs≡) rv≡))
      -- Reduce `comm-ok` under hc=true / cc=just to the `≡ᶠ?` check, true
      -- by reflexivity rewritten along `c≡tc`.
      goal : (c ≡ᶠ? transient-commit (ProofPreimage.inputs pre ++ Preprocessed.outputs s) r) ≡ true
      goal = subst (λ x → (c ≡ᶠ? x) ≡ true) c≡tc ≡ᶠ?-refl
    in comm-ok-reduce src pre s c r hc-true cc-just goal
    where
      -- `comm-ok src pre s` reduces to the `≡ᶠ?` check at hc=true/cc=just.
      comm-ok-reduce : ∀ src pre s c r
        → IrSource.do-communications-commitment src ≡ true
        → ProofPreimage.comm-commitment pre ≡ just (c , r)
        → (c ≡ᶠ? transient-commit (ProofPreimage.inputs pre ++ Preprocessed.outputs s) r) ≡ true
        → T (comm-ok src pre s)
      comm-ok-reduce src pre s c r hc-true cc-just chk
        with IrSource.do-communications-commitment src
           | ProofPreimage.comm-commitment pre
           | hc-true | cc-just
      ... | true | just .(c , r) | _ | refl = ≡→T chk

  -- hc=true with comm-commitment=nothing is ruled out by `satisfies`'s
  -- rand-shape (`Maybe-shape true nothing ≡ ⊥`).
  bwd-no-comm-contra
    : ∀ (src : IrSource) (pre : ProofPreimage) (s : Preprocessed)
    → IrSource.do-communications-commitment src ≡ true
    → ProofPreimage.comm-commitment pre ≡ nothing
    → Maybe-shape (IrSource.do-communications-commitment src) (comm-rand-of pre)
    → ⊥
  bwd-no-comm-contra src pre s hc-true cc-none msh
    with IrSource.do-communications-commitment src
       | ProofPreimage.comm-commitment pre
       | hc-true | cc-none
  ... | true | nothing | _ | refl = msh

circuit-faithful-bwd
  : ∀ (src : IrSource) (pre : ProofPreimage) (s : Preprocessed)
  → T (producer-safe src)
  → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src   -- WF1 (§3.3)
  → WF2 src                                                       -- WF2 (§3.3)
  → preprocess-shaped src pre s                                   -- §5.3
  → satisfies (circuit src) (witness-of s pre)
  → R src pre s
circuit-faithful-bwd src pre s ps-safe wf1 wf2
    (mk-shaped s₀ init-eq (mk-shape-walk ms ps tr) tc)
    (mk-sat _pi-len _mem-len rand-shape constraint-ok)
  with bool-cases (IrSource.do-communications-commitment src)
... | inj₂ hc-false =
  let
    n   = IrSource.num-inputs src
    st₀ = mk-synth n [] 0 []
    instrs = IrSource.instructions src
    mem-eq-s : Preprocessed.memory s ≡ Preprocessed.memory s₀ ++ ms
    mem-eq-s = Tr-shaped→mem pre s₀ s instrs ms ps tr
    pis-eq-s : Preprocessed.pis s ≡ Preprocessed.pis s₀ ++ ps
    pis-eq-s = Tr-shaped→pis pre s₀ s instrs ms ps tr
    -- `circuit src` reduces to its hc=false body constraints; constraint-ok is
    -- satisfaction of exactly those constraints by `witness-of s pre`.
    circuit-eq : circuit src ≡
      mk-circuit (SynthState.nr-wires (circuit-instrs false instrs st₀))
                 (SynthState.constraints (circuit-instrs false instrs st₀))
                 (1 + SynthState.nr-declared-pi (circuit-instrs false instrs st₀))
                 false
    circuit-eq = circuit-eq-false src hc-false
    sat-body-s : satisfies-constraints
      (SynthState.constraints (circuit-instrs false instrs st₀))
      (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) (comm-rand-of pre))
    sat-body-s = subst (λ c → satisfies-constraints (Circuit.constraints c)
                                 (witness-of s pre))
                       circuit-eq constraint-ok
    sat-body : satisfies-constraints
      (SynthState.constraints (circuit-instrs false instrs st₀))
      (mk-witness (Preprocessed.memory s₀ ++ ms) (Preprocessed.pis s₀ ++ ps) (comm-rand-of pre))
    sat-body = subst₂ (λ m p → satisfies-constraints
                                  (SynthState.constraints (circuit-instrs false instrs st₀))
                                  (mk-witness m p (comm-rand-of pre)))
                      mem-eq-s pis-eq-s sat-body-s
    Rs : R-instrs pre s₀ instrs s
    Rs = bwd-body-trace {false} pre src s s₀ ms ps ps-safe wf1 wf2 init-eq tr hc-false sat-body
    co : T (comm-ok src pre s)
    co = comm-ok-false src pre s hc-false
  in s₀ , init-eq , Rs , tc , co
... | inj₁ hc-true with maybe-cases (ProofPreimage.comm-commitment pre)
...   | inj₁ cc-none =
        ⊥-elim (bwd-no-comm-contra src pre s hc-true cc-none rand-shape)
...   | inj₂ (c , r , cc-just) =
  let
    n   = IrSource.num-inputs src
    st₀ = mk-synth n [] 0 []
    instrs = IrSource.instructions src
    st-end = circuit-instrs true instrs st₀
    cm-inputs = nat-range n
    out-wires = SynthState.output-wires st-end
    body-constraints = SynthState.constraints st-end
    mem-eq-s : Preprocessed.memory s ≡ Preprocessed.memory s₀ ++ ms
    mem-eq-s = Tr-shaped→mem pre s₀ s instrs ms ps tr
    pis-eq-s : Preprocessed.pis s ≡ Preprocessed.pis s₀ ++ ps
    pis-eq-s = Tr-shaped→pis pre s₀ s instrs ms ps tr
    circuit-eq : circuit src ≡
      mk-circuit (SynthState.nr-wires st-end)
                 (body-constraints ∷ʳ comm cm-inputs out-wires)
                 (2 + SynthState.nr-declared-pi st-end)
                 true
    circuit-eq = circuit-eq-true src hc-true
    -- Satisfaction of the FULL constraint list (body ++ [comm]) by `witness-of s pre`.
    sat-full : satisfies-constraints
      (body-constraints ∷ʳ comm cm-inputs out-wires)
      (mk-witness (Preprocessed.memory s) (Preprocessed.pis s) (comm-rand-of pre))
    sat-full = subst (λ c → satisfies-constraints (Circuit.constraints c) (witness-of s pre))
                     circuit-eq constraint-ok
    -- Split off the comm constraint.
    split = satisfies-constraints-split body-constraints
              (comm cm-inputs out-wires ∷ []) sat-full
    -- sat-body-s : satisfies-constraints body-constraints (witness at s)
    -- holds-comm : holds (witness at s) (comm cm-inputs out-wires)
    (sat-body-s , comm-sat) = split
    holds-comm = headᴬ comm-sat
    sat-body : satisfies-constraints body-constraints
      (mk-witness (Preprocessed.memory s₀ ++ ms) (Preprocessed.pis s₀ ++ ps) (comm-rand-of pre))
    sat-body = subst₂ (λ m p → satisfies-constraints body-constraints
                                  (mk-witness m p (comm-rand-of pre)))
                      mem-eq-s pis-eq-s sat-body-s
    Rs : R-instrs pre s₀ instrs s
    Rs = bwd-body-trace {true} pre src s s₀ ms ps ps-safe wf1 wf2 init-eq tr hc-true sat-body
    co : T (comm-ok src pre s)
    co = bwd-comm-ok-true src pre s s₀ c r hc-true cc-just init-eq Rs holds-comm
  in s₀ , init-eq , Rs , tc , co

------------------------------------------------------------------------
-- Section E.
------------------------------------------------------------------------

-- The bundled biconditional (spec §6.2, P5 — "`preprocess(S,P)=Σ` iff
-- `Σ ⊨ C_S(π_Σ)`").  The spec states P5 as an *iff* of propositions, i.e.
-- a logical equivalence, which `Function.Bundles._⇔_` captures exactly:
-- it bundles the two implications `to` / `from` with only their (trivial)
-- congruence laws — no round-trip identity is asserted.
--
-- We deliberately do NOT use the stronger `_↔_` (type isomorphism): a
-- genuine `↔` additionally demands the inverse equations `to (from x) ≡ x`
-- / `from (to r) ≡ r`, propositional equalities between *proofs* of the
-- `Set`-valued relations `R` / `satisfies`.  Those proofs are not unique,
-- so the inverse laws are not derivable without a proof-irrelevance
-- postulate — which the no-postulate discipline forbids and which the
-- spec's "iff" never required.
--
-- `to` is the forward direction; it ignores the extra `preprocess-shaped`
-- hypothesis (redundant given `R`, by `R⇒preprocess-shaped`).  `from` is
-- the backward direction (`circuit-faithful-bwd`).
--
-- WF2 (bit-count bounds, §3.3) is a hypothesis of the backward
-- direction.  Both `R` and the functional `preprocess` reject a
-- bit count outside the per-instruction bounds (`WF2-instr`), faithfully
-- to the Rust runtime, whereas the circuit's `in-range` /
-- `div-mod` / `reconstitute` stay satisfiable for an
-- excessive `bits`.  So circuit satisfaction alone cannot recover `R`
-- for an ill-formed `bits`; `WF2 src` supplies the missing bound that
-- `satisfies→R-instr-step` feeds to the bit-bound rules.  The forward
-- direction needs no WF2 hypothesis: `R` already carries the bound.
circuit-faithful
  : ∀ (src : IrSource) (pre : ProofPreimage) (s : Preprocessed)
  → T (producer-safe src)
  → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src   -- WF1 (§3.3)
  → WF2 src                                                       -- WF2 (§3.3)
  → preprocess-shaped src pre s                                   -- §5.3
  → R src pre s ⇔ satisfies (circuit src) (witness-of s pre)
circuit-faithful src pre s ps-safe wf1 wf2 pps =
  mk⇔ (circuit-faithful-fwd src pre s)
      (circuit-faithful-bwd src pre s ps-safe wf1 wf2 pps)
