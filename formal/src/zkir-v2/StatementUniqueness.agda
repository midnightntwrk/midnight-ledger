{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.StatementUniqueness (⋯ : _) (open Assumptions ⋯) where

------------------------------------------------------------------------
-- Extraction uniqueness for ZKIR v2.
--
--   circuit-statement-unique
--     : ∀ src {pre s pre′ s′} → T (producer-safe src)
--     → CommWF (do-comm src) (comm-commitment pre)
--     → CommWF (do-comm src) (comm-commitment pre′)
--     → R src pre s → R src pre′ s′
--     → witness-of s pre ≡ witness-of s′ pre′
--     → (pre ≡ pre′) × (s ≡ s′)
--
-- The realizing run of a witness is unique.  Combined with
-- `circuit-statement-sound` (existence) this pins, for every satisfying
-- witness of a WF2 source whose `comm-rand` respects the commitment
-- flag (`RandWF`), EXACTLY ONE commitment-well-shaped realizer — a
-- `CommWFRealizer`, the `Realizer` record of `StatementSoundness`
-- extended with `CommWF` (`circuit-statement-sound-unique`; its full
-- hypotheses are documented at its definition).
--
-- The witness `witness-of s pre` carries `(memory s , pis s , comm-rand
-- of pre)`, so witness equality hands us, by `cong`,
--   (E1) memory s ≡ memory s′,  (E2) pis s ≡ pis s′,
--   (E3) comm-rand-of pre ≡ comm-rand-of pre′.
-- The two runs execute the SAME instruction list from initial states
-- with equal memory/pis lengths, ending with equal memory (E1) and pis
-- (E2).  Memory and pis only grow by appends, so two intermediate states
-- that are prefixes of the SAME final list and have the SAME length are
-- equal — the anchor.  A single parallel induction over the instruction
-- list, carrying both `R-instrs` traces and the anchoring suffix
-- evidence, forces the two runs to agree at every step (`StateEq` of the
-- post-states), which assembles to `s ≡ s′` and, reading off the
-- preimage fields seeded into `s₀` / consumed along the walk, to
-- `pre ≡ pre′`.
------------------------------------------------------------------------

open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
  using ( ProofPreimage; mk-preimage; Preprocessed; mk-state; init-state
        ; transcripts-consumed; comm-ok; R; push-mem; push-mem2
        ; mem-lookup; eval-guard; consume-pub-out; consume-priv; from-bool
        ; _≡ᶠ-list?_; WF2
        ; R-instr; R-instrs; r-done; r-step
        ; r-assert; r-cond-select; r-constrain-bits; r-constrain-eq
        ; r-constrain-to-boolean; r-copy; r-declare-pub-input
        ; r-pi-skip-active; r-pi-skip-inactive; r-ec-add; r-ec-mul
        ; r-ec-mul-generator; r-hash-to-curve; r-load-imm
        ; r-div-mod-power-of-two; r-reconstitute-field; r-output
        ; r-transient-hash; r-persistent-hash; r-test-eq; r-add; r-mul
        ; r-neg; r-not; r-less-than; r-public-input-inactive
        ; r-public-input-active; r-private-input-inactive
        ; r-private-input-active )
open import zkir-v2.SemanticsLemmas ⋯
  using ( consume-pub-out-mem; consume-pub-out-pis; consume-pub-out-idx
        ; consume-pub-out-outputs; consume-pub-out-skips
        ; consume-pub-out-priv; consume-pub-out-rem
        ; consume-priv-mem; consume-priv-pis; consume-priv-idx
        ; consume-priv-outputs; consume-priv-skips
        ; consume-priv-pub; consume-priv-rem )
open import zkir-v2.Circuit ⋯
  using ( Witness; mk-witness; Circuit; circuit; satisfies )
open import zkir-v2.FieldProperties ⋯
  using ( to-bool; _≡ᶠ?_; ≡ᶠ?-true )
open import zkir-v2.Obligations ⋯
  using ( producer-safe; Δmem; O1-scan
        ; O1-Trace; o1-nil; o1-decl; o1-skip; o1-other
        ; o1-sound )
open import zkir-v2.CircuitProof ⋯
  using ( witness-of; comm-rand-of )
open import zkir-v2.Properties ⋯
  using ( R-instr-Δmem; Δmem-sum )
open import zkir-v2.StatementSoundness ⋯
  using ( circuit-statement-sound; Realizer; mk-realizer )

open import Data.Bool    using (Bool; true; false; T; if_then_else_)
import Data.Bool as Bool
open import Function
open import Data.Bool.Properties using (T-∧)
open import Data.List    using (List; []; _∷_; _++_; length; take; drop)
open import Data.List.Properties
  using ( ++-identityʳ; ++-assoc; ++-cancelˡ
        ; take-all; take-[]; ∷-injectiveˡ; ∷-injectiveʳ )
open import Data.Maybe   using (Maybe; just; nothing)
open import Data.Maybe.Properties using (just-injective)
open import Data.Nat     using (ℕ; zero; suc; _+_; _∸_; _≤_; _<_; s≤s; _≡ᵇ_)
open import Data.Nat.Properties
  using ( ≡ᵇ⇒≡; +-identityʳ; +-suc; +-assoc
        ; m+n∸n≡m; ≤-reflexive )
open import Data.Empty   using (⊥; ⊥-elim)
open import Data.Unit    using (⊤; tt)
open import Data.Product using (_×_; _,_; Σ; proj₁; proj₂)
open import Function.Bundles using (Equivalence)
open import Relation.Binary.PropositionalEquality
  using ( _≡_; refl; sym; trans; cong; cong₂; subst )

------------------------------------------------------------------------
-- Commitment well-shapedness.  A preimage may carry a commitment pair
-- only when the source's flag is set.  When the flag is off, `init-state`
-- and `comm-ok` ignore `comm-commitment` entirely, so a vestigial
-- `just (c , r)` is invisible to `R` and to `witness-of` except through
-- `r` — two preimages differing only in that `c` realize the same
-- witness.  `CommWF` removes this slack.
------------------------------------------------------------------------

CommWF : Bool → Maybe (Fr × Fr) → Set
CommWF false (just _) = ⊥
CommWF false nothing  = ⊤
CommWF true  _        = ⊤

-- A commitment-well-shaped realizer: a `Realizer` (StatementSoundness)
-- whose preimage is `CommWF`.  The `Realizer` fields (`preimage`,
-- `run`, `R-run`, `extracts`) are re-exported as projections.
record CommWFRealizer (src : IrSource) (w : Witness) : Set where
  constructor mk-commwf-realizer
  field
    realizer : Realizer src w
  open Realizer realizer public
  field
    comm-wf : CommWF (IrSource.do-communications-commitment src)
                     (ProofPreimage.comm-commitment preimage)

------------------------------------------------------------------------
-- Reflection and anchoring helpers.
------------------------------------------------------------------------

private
  -- `≡ᶠ-list?` reflected back to propositional list equality (the
  -- two-run analogue of `≡ᶠ?-true`, lifted element-wise).
  ≡ᶠ-list?-true : ∀ (xs ys : List Fr) → T (xs ≡ᶠ-list? ys) → xs ≡ ys
  ≡ᶠ-list?-true []       []       _ = refl
  ≡ᶠ-list?-true (x ∷ xs) (y ∷ ys) t =
    let (hx , ht) = T-∧ .Equivalence.to t
    in cong₂ _∷_ (≡ᶠ?-true {x} {y} hx) (≡ᶠ-list?-true xs ys ht)

  -- Two appends with equal left lengths that produce the same list have
  -- equal left parts and equal right parts: the anchor.  Proved by
  -- splitting each side as `take + drop` at the common left length.
  split-eq : ∀ (ys zs ysr zsr : List Fr)
    → ys ++ ysr ≡ zs ++ zsr → length ys ≡ length zs
    → (ys ≡ zs) × (ysr ≡ zsr)
  split-eq ys zs ysr zsr eq ly≡lz = ys≡zs , ysr≡zsr
    where
      take-eq : take (length ys) (ys ++ ysr) ≡ take (length zs) (zs ++ zsr)
      take-eq = cong₂ take ly≡lz eq

      ys≡zs : ys ≡ zs
      ys≡zs = trans (sym (take-len ys ysr))
                    (trans take-eq (take-len zs zsr))
        where
          take-len : ∀ (xs xr : List Fr) → take (length xs) (xs ++ xr) ≡ xs
          take-len []       xr = refl
          take-len (x ∷ xs) xr = cong (x ∷_) (take-len xs xr)

      ysr≡zsr : ysr ≡ zsr
      ysr≡zsr = ++-cancelˡ ys ysr zsr
                  (subst (λ z → ys ++ ysr ≡ z ++ zsr) (sym ys≡zs) eq)

  -- `take (m + n) xs ≡ take m xs ++ take n (drop m xs)`.
  take-split : ∀ (m n : ℕ) (xs : List Fr)
    → take (m + n) xs ≡ take m xs ++ take n (drop m xs)
  take-split zero    n xs       = refl
  take-split (suc m) n []       = sym (cong (take (suc m) [] ++_) (take-[] n))
  take-split (suc m) n (x ∷ xs) = cong (x ∷_) (take-split m n xs)

------------------------------------------------------------------------
-- The two-run parallel induction.
--
-- `StateEq s s′` records the five forward-determined fields agreeing
-- between the two runs (memory and pis grow by appends, the rest by
-- record updates); the transcript cursors `pub-out-rem` / `priv-rem`
-- are settled backward from the final emptiness.
------------------------------------------------------------------------

private
  -- A record (not a product) so that `StateEq s s′` keeps `s` / `s′` as
  -- recoverable parameters — the accessors below then infer their state
  -- arguments without leaving metas (a bare `_×_` of projections does
  -- not pin `s` / `s′` under unification).
  record StateEq (s s′ : Preprocessed) : Set where
    constructor mk-stateq
    field
      eq-mem  : Preprocessed.memory     s ≡ Preprocessed.memory     s′
      eq-pis  : Preprocessed.pis        s ≡ Preprocessed.pis        s′
      eq-idx  : Preprocessed.pub-in-idx s ≡ Preprocessed.pub-in-idx s′
      eq-out  : Preprocessed.outputs    s ≡ Preprocessed.outputs    s′
      eq-skip : Preprocessed.pi-skips   s ≡ Preprocessed.pi-skips   s′

  -- The result the parallel walk hands back.  A record (rather than a
  -- nested product) so its components carry names at use sites.  `we-af`
  -- (the final validated prefix length) is `pub-in-idx sf`, and the
  -- transcript is pinned to agree up to it.
  record WalkEq (pre pre′ : ProofPreimage)
                (s s′ sf sf′ : Preprocessed) : Set where
    constructor mk-walkeq
    field
      we-state : StateEq sf sf′
      we-pout  : Preprocessed.pub-out-rem s ≡ Preprocessed.pub-out-rem s′
      we-priv  : Preprocessed.priv-rem    s ≡ Preprocessed.priv-rem    s′
      we-af    : ℕ
      we-idx   : Preprocessed.pub-in-idx sf ≡ we-af
      we-tpin  : take we-af (ProofPreimage.pub-transcript-inputs pre)
               ≡ take we-af (ProofPreimage.pub-transcript-inputs pre′)

  -- A `mem-lookup` agrees across the two runs when their memories agree.
  lookup-cong : ∀ {m m′ : List Fr} (i : Index) → m ≡ m′
    → mem-lookup m i ≡ mem-lookup m′ i
  lookup-cong i meq = cong (λ m → mem-lookup m i) meq

  -- An unguarded transcript instruction is always active (`eval-guard m
  -- nothing = just true`, irrespective of `m`), so the inactive arm's
  -- premise reduces to `just true ≡ just false`.  The memory is pinned to
  -- `[]` — any choice reduces identically, but a fixed one avoids an
  -- uninferable implicit.
  unguarded-not-inactive : eval-guard [] nothing ≡ just false → ⊥
  unguarded-not-inactive eq = case just-injective eq of λ where ()

  -- A guarded instruction cannot be active on one run and inactive on the
  -- other when the two memories agree (the guard reads the SAME wire).
  guard-mismatch : ∀ {m m′ : List Fr} (gw : Index) → m ≡ m′
    → eval-guard m (just gw) ≡ just true
    → eval-guard m′ (just gw) ≡ just false → ⊥
  guard-mismatch gw meq gt gf = 
    case just-injective (trans (sym gt) 
                        (trans (cong (λ m → eval-guard m (just gw)) meq) 
                        gf)) 
      of λ where ()

  -- StateEq components, named for readability at use sites.
  open StateEq renaming ( eq-mem to se-mem; eq-pis to se-pis; eq-idx to se-idx
                        ; eq-out to se-out; eq-skip to se-skip )

  -- Memory only grows by appends, so the start memory is a prefix of the
  -- end memory along any `R-instr` / `R-instrs` trace.
  mem-step-prefix : ∀ {pre s i s′} → R-instr pre s i s′
    → Σ (List Fr) (λ ext →
         Preprocessed.memory s′ ≡ Preprocessed.memory s ++ ext)
  mem-step-prefix (r-assert _)               = [] , sym (++-identityʳ _)
  mem-step-prefix (r-constrain-bits _ _ _)     = [] , sym (++-identityʳ _)
  mem-step-prefix (r-constrain-eq _ _ _)     = [] , sym (++-identityʳ _)
  mem-step-prefix (r-constrain-to-boolean _) = [] , sym (++-identityʳ _)
  mem-step-prefix (r-declare-pub-input _)    = [] , sym (++-identityʳ _)
  mem-step-prefix (r-pi-skip-active _ _)     = [] , sym (++-identityʳ _)
  mem-step-prefix (r-pi-skip-inactive _)     = [] , sym (++-identityʳ _)
  mem-step-prefix (r-output _)               = [] , sym (++-identityʳ _)
  mem-step-prefix (r-cond-select {sel = sel} {av = av} {bv = bv} _ _ _) =
    (if sel then av else bv) ∷ [] , refl
  mem-step-prefix (r-copy {v = v} _)         = v ∷ [] , refl
  mem-step-prefix (r-load-imm {imm = imm})   = imm ∷ [] , refl
  mem-step-prefix (r-reconstitute-field _ _ _ _ _) = _ ∷ [] , refl
  mem-step-prefix (r-transient-hash {vs = vs} _) =
    transient-hash-fn vs ∷ [] , refl
  mem-step-prefix (r-test-eq {av = av} {bv} _ _) =
    from-bool (av ≡ᶠ? bv) ∷ [] , refl
  mem-step-prefix (r-add {av = av} {bv} _ _) = (av +ᶠ bv) ∷ [] , refl
  mem-step-prefix (r-mul {av = av} {bv} _ _) = (av *ᶠ bv) ∷ [] , refl
  mem-step-prefix (r-neg {av = av} _)        = (-ᶠ av) ∷ [] , refl
  mem-step-prefix (r-not {b = b} _)          =
    from-bool (Bool.not b) ∷ [] , refl
  mem-step-prefix (r-less-than _ _ _ _)        = _ ∷ [] , refl
  mem-step-prefix (r-ec-add {cx = cx} {cy} _ _ _ _ _) = cx ∷ cy ∷ [] , refl
  mem-step-prefix (r-ec-mul {cx = cx} {cy} _ _ _ _)   = cx ∷ cy ∷ [] , refl
  mem-step-prefix (r-ec-mul-generator {cx = cx} {cy} _ _) = cx ∷ cy ∷ [] , refl
  mem-step-prefix (r-hash-to-curve {cx = cx} {cy} _ _)    = cx ∷ cy ∷ [] , refl
  mem-step-prefix (r-persistent-hash {h₁ = h₁} {h₂} _ _)  = h₁ ∷ h₂ ∷ [] , refl
  mem-step-prefix {s = s} (r-div-mod-power-of-two {bits = bits} {v = v} _ _) =
    let dvc = from-le-bits (drop bits (to-le-bits v))
        mvc = from-le-bits (take bits (to-le-bits v))
    in dvc ∷ mvc ∷ []
     , ++-assoc (Preprocessed.memory s) (dvc ∷ []) (mvc ∷ [])
  mem-step-prefix {s = s} (r-public-input-inactive _) = 0ᶠ ∷ [] , refl
  mem-step-prefix {s = s} (r-public-input-active {v = v} {s₁} _ cp) =
    v ∷ [] , cong (_++ (v ∷ [])) (consume-pub-out-mem _ cp)
  mem-step-prefix {s = s} (r-private-input-inactive _) = 0ᶠ ∷ [] , refl
  mem-step-prefix {s = s} (r-private-input-active {v = v} {s₁} _ cp) =
    v ∷ [] , cong (_++ (v ∷ [])) (consume-priv-mem _ cp)

  mem-prefix : ∀ {pre s is sf} → R-instrs pre s is sf
    → Σ (List Fr) (λ ext →
         Preprocessed.memory sf ≡ Preprocessed.memory s ++ ext)
  mem-prefix r-done = [] , sym (++-identityʳ _)
  mem-prefix {s = s} (r-step {s₁ = s₁} step rest) =
    let (e1 , p1) = mem-step-prefix step
        (e2 , p2) = mem-prefix rest
    in e1 ++ e2
     , trans p2 (trans (cong (_++ e2) p1)
                       (++-assoc (Preprocessed.memory s) e1 e2))

  -- The analogue for pis: only `declare-pub-input` appends a pis cell.
  pis-step-prefix : ∀ {pre s i s′} → R-instr pre s i s′
    → Σ (List Fr) (λ ext →
         Preprocessed.pis s′ ≡ Preprocessed.pis s ++ ext)
  pis-step-prefix (r-declare-pub-input {v = v} _) = v ∷ [] , refl
  pis-step-prefix (r-assert _)               = [] , sym (++-identityʳ _)
  pis-step-prefix (r-cond-select _ _ _)      = [] , sym (++-identityʳ _)
  pis-step-prefix (r-constrain-bits _ _ _)     = [] , sym (++-identityʳ _)
  pis-step-prefix (r-constrain-eq _ _ _)     = [] , sym (++-identityʳ _)
  pis-step-prefix (r-constrain-to-boolean _) = [] , sym (++-identityʳ _)
  pis-step-prefix (r-copy _)                 = [] , sym (++-identityʳ _)
  pis-step-prefix (r-pi-skip-active _ _)     = [] , sym (++-identityʳ _)
  pis-step-prefix (r-pi-skip-inactive _)     = [] , sym (++-identityʳ _)
  pis-step-prefix (r-ec-add _ _ _ _ _)       = [] , sym (++-identityʳ _)
  pis-step-prefix (r-ec-mul _ _ _ _)         = [] , sym (++-identityʳ _)
  pis-step-prefix (r-ec-mul-generator _ _)   = [] , sym (++-identityʳ _)
  pis-step-prefix (r-hash-to-curve _ _)      = [] , sym (++-identityʳ _)
  pis-step-prefix r-load-imm                 = [] , sym (++-identityʳ _)
  pis-step-prefix (r-div-mod-power-of-two _ _) = [] , sym (++-identityʳ _)
  pis-step-prefix (r-reconstitute-field _ _ _ _ _) = [] , sym (++-identityʳ _)
  pis-step-prefix (r-output _)               = [] , sym (++-identityʳ _)
  pis-step-prefix (r-transient-hash _)       = [] , sym (++-identityʳ _)
  pis-step-prefix (r-persistent-hash _ _)    = [] , sym (++-identityʳ _)
  pis-step-prefix (r-test-eq _ _)            = [] , sym (++-identityʳ _)
  pis-step-prefix (r-add _ _)                = [] , sym (++-identityʳ _)
  pis-step-prefix (r-mul _ _)                = [] , sym (++-identityʳ _)
  pis-step-prefix (r-neg _)                  = [] , sym (++-identityʳ _)
  pis-step-prefix (r-not _)                  = [] , sym (++-identityʳ _)
  pis-step-prefix (r-less-than _ _ _ _)        = [] , sym (++-identityʳ _)
  pis-step-prefix (r-public-input-inactive _)  = [] , sym (++-identityʳ _)
  pis-step-prefix (r-public-input-active _ cp) =
    [] , trans (consume-pub-out-pis _ cp) (sym (++-identityʳ _))
  pis-step-prefix (r-private-input-inactive _) = [] , sym (++-identityʳ _)
  pis-step-prefix (r-private-input-active _ cp) =
    [] , trans (consume-priv-pis _ cp) (sym (++-identityʳ _))

  pis-prefix : ∀ {pre s is sf} → R-instrs pre s is sf
    → Σ (List Fr) (λ ext →
         Preprocessed.pis sf ≡ Preprocessed.pis s ++ ext)
  pis-prefix r-done = [] , sym (++-identityʳ _)
  pis-prefix {s = s} (r-step {s₁ = s₁} step rest) =
    let (e1 , p1) = pis-step-prefix step
        (e2 , p2) = pis-prefix rest
    in e1 ++ e2
     , trans p2 (trans (cong (_++ e2) p1)
                       (++-assoc (Preprocessed.pis s) e1 e2))

  -- The cell a step appends, read off the common final memory.  Given
  -- `memory sf ≡ (memory s ++ (v ∷ [])) ++ extTail` for both runs, the
  -- common final memory and the equal start memories force `v ≡ v′`.
  next-cell-eq : ∀ {ms ms′ : List Fr} {v v′ : Fr} {et et′ mf mf′ : List Fr}
    → ms ≡ ms′ → mf ≡ mf′
    → mf  ≡ (ms  ++ (v  ∷ [])) ++ et
    → mf′ ≡ (ms′ ++ (v′ ∷ [])) ++ et′
    → v ≡ v′
  next-cell-eq {ms} {ms′} {v} {v′} {et} {et′} meq mfeq pf pf′ =
    let joined : ms ++ ((v ∷ []) ++ et) ≡ ms′ ++ ((v′ ∷ []) ++ et′)
        joined = trans (sym (++-assoc ms (v ∷ []) et))
                   (trans (sym pf)
                     (trans mfeq (trans pf′ (++-assoc ms′ (v′ ∷ []) et′))))
        (_ , tails) = split-eq ms ms′ ((v ∷ []) ++ et) ((v′ ∷ []) ++ et′)
                        joined (cong length meq)
    in ∷-injectiveˡ tails

  -- Memory equality is preserved across a step of the SAME instruction:
  -- both post-states are equal-length prefixes of the common final
  -- memory.  The post-states' ext lengths agree (`R-instr-Δmem`), and
  -- their tails reach the same `mf`.
  step-mem-eq : ∀ {pre pre′ i} {s s′ s₁ s₁′ : Preprocessed}
                  {mf mf′ et et′ : List Fr}
    → R-instr pre  s  i s₁
    → R-instr pre′ s′ i s₁′
    → Preprocessed.memory s ≡ Preprocessed.memory s′
    → mf ≡ mf′
    → mf  ≡ Preprocessed.memory s₁  ++ et
    → mf′ ≡ Preprocessed.memory s₁′ ++ et′
    → Preprocessed.memory s₁ ≡ Preprocessed.memory s₁′
  step-mem-eq {i = i} {s} {s′} {s₁} {s₁′} {mf} {mf′} {et} {et′}
              step step′ meq mfeq pf pf′ =
    proj₁ (split-eq (Preprocessed.memory s₁) (Preprocessed.memory s₁′) et et′
             (trans (sym pf) (trans mfeq pf′)) len-eq)
    where
      len-eq : length (Preprocessed.memory s₁) ≡ length (Preprocessed.memory s₁′)
      len-eq =
        trans (R-instr-Δmem step)
          (trans (cong (_+ Δmem i) (cong length meq))
                 (sym (R-instr-Δmem step′)))

  ------------------------------------------------------------------------
  -- The two-run parallel induction proper.  Both runs execute the SAME
  -- instruction list `is`; `o1` is the shared O1-Trace; `mf` is the
  -- common final memory (`memory sf ≡ memory sf′`, anchored by `ma`/`ma′`
  -- against the two intermediate memories).  `a` and `d` are the
  -- validated-prefix length and the declares-since-last-skip counter; the
  -- transcript prefix is pinned to agree up to `a` (`tpin`), and
  -- `pub-in-idx s ≡ a + d` (`idx≡`).
  -- Instructions whose `R-instr` touches only memory: pis / pub-in-idx /
  -- outputs / pi-skips / both rems pass through.  `declare-pub-input`,
  -- `pi-skip`, `output`, and active `public-input` / `private-input` are
  -- excluded (they update one of those fields).
  PureMem : Instruction → Set
  PureMem (declare-pub-input _) = ⊥
  PureMem (pi-skip _ _)         = ⊥
  PureMem (output _)            = ⊥
  PureMem (public-input _)      = ⊥
  PureMem (private-input _)     = ⊥
  PureMem _                     = ⊤

  -- For a pure-memory step, the five non-memory state components are
  -- unchanged, alongside the two transcript cursors.
  -- (`public-input`/`private-input` are excluded by `PureMem` precisely
  -- because their *active* arm pops a transcript.)
  record PureMemFrame (s s′ : Preprocessed) : Set where
    constructor mk-pmf
    field
      pmf-pis     : Preprocessed.pis s′ ≡ Preprocessed.pis s
      pmf-idx     : Preprocessed.pub-in-idx s′ ≡ Preprocessed.pub-in-idx s
      pmf-out     : Preprocessed.outputs s′ ≡ Preprocessed.outputs s
      pmf-skip    : Preprocessed.pi-skips s′ ≡ Preprocessed.pi-skips s
      pmf-pubrem  : Preprocessed.pub-out-rem s′ ≡ Preprocessed.pub-out-rem s
      pmf-privrem : Preprocessed.priv-rem s′ ≡ Preprocessed.priv-rem s

  pm-frame : ∀ {pre s i s′} → R-instr pre s i s′ → PureMem i
    → PureMemFrame s s′
  pm-frame (r-assert _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-cond-select _ _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-constrain-bits _ _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-constrain-eq _ _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-constrain-to-boolean _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-copy _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-ec-add _ _ _ _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-ec-mul _ _ _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-ec-mul-generator _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-hash-to-curve _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame r-load-imm _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-div-mod-power-of-two _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-reconstitute-field _ _ _ _ _) _ =
    mk-pmf refl refl refl refl refl refl
  pm-frame (r-transient-hash _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-persistent-hash _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-test-eq _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-add _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-mul _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-neg _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-not _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-less-than _ _ _ _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-public-input-inactive _) _ = mk-pmf refl refl refl refl refl refl
  pm-frame (r-private-input-inactive _) _ =
    mk-pmf refl refl refl refl refl refl

  pm-pis : ∀ {pre s i s′} → R-instr pre s i s′ → PureMem i
    → Preprocessed.pis s′ ≡ Preprocessed.pis s
  pm-pis step pm = PureMemFrame.pmf-pis (pm-frame step pm)

  pm-idx : ∀ {pre s i s′} → R-instr pre s i s′ → PureMem i
    → Preprocessed.pub-in-idx s′ ≡ Preprocessed.pub-in-idx s
  pm-idx step pm = PureMemFrame.pmf-idx (pm-frame step pm)

  pm-out : ∀ {pre s i s′} → R-instr pre s i s′ → PureMem i
    → Preprocessed.outputs s′ ≡ Preprocessed.outputs s
  pm-out step pm = PureMemFrame.pmf-out (pm-frame step pm)

  pm-skip : ∀ {pre s i s′} → R-instr pre s i s′ → PureMem i
    → Preprocessed.pi-skips s′ ≡ Preprocessed.pi-skips s
  pm-skip step pm = PureMemFrame.pmf-skip (pm-frame step pm)

  pm-pubrem : ∀ {pre s i s′} → R-instr pre s i s′ → PureMem i
    → Preprocessed.pub-out-rem s′ ≡ Preprocessed.pub-out-rem s
  pm-pubrem step pm = PureMemFrame.pmf-pubrem (pm-frame step pm)

  pm-privrem : ∀ {pre s i s′} → R-instr pre s i s′ → PureMem i
    → Preprocessed.priv-rem s′ ≡ Preprocessed.priv-rem s
  pm-privrem step pm = PureMemFrame.pmf-privrem (pm-frame step pm)

  -- Peel the O1-Trace head past a pure-memory instruction (the counter
  -- `d` is unchanged: `PureMem` excludes `declare-pub-input` / `pi-skip`).
  o1-pure : ∀ {d i is} → PureMem i → O1-Trace d (i ∷ is) → O1-Trace d is
  o1-pure _  (o1-other _ t) = t
  o1-pure pm (o1-decl _)    = ⊥-elim pm
  o1-pure pm (o1-skip _ _)  = ⊥-elim pm

  ------------------------------------------------------------------------
  walk-eq
    : ∀ (pre pre′ : ProofPreimage) (is : List Instruction)
        {s s′ sf sf′ : Preprocessed} {a d : ℕ}
    → R-instrs pre  s  is sf
    → R-instrs pre′ s′ is sf′
    → O1-Trace d is
    → StateEq s s′
    → Preprocessed.memory sf ≡ Preprocessed.memory sf′
    → Preprocessed.pub-out-rem sf  ≡ []
    → Preprocessed.pub-out-rem sf′ ≡ []
    → Preprocessed.priv-rem    sf  ≡ []
    → Preprocessed.priv-rem    sf′ ≡ []
    → Preprocessed.pub-in-idx s ≡ a + d
    → take a (ProofPreimage.pub-transcript-inputs pre)
      ≡ take a (ProofPreimage.pub-transcript-inputs pre′)
    → WalkEq pre pre′ s s′ sf sf′

  -- A pure-memory head: peel `o1`, build the post-state `StateEq` from
  -- the inherited components, and recurse.  The backward rems pass
  -- through unchanged, so the recursion's bundle transports directly.
  pure-step
    : ∀ (pre pre′ : ProofPreimage) {i is}
        {s s′ s₁ s₁′ sf sf′ : Preprocessed} {a d : ℕ}
    → PureMem i
    → R-instr  pre  s  i s₁ → R-instr  pre′ s′ i s₁′
    → R-instrs pre  s₁ is sf → R-instrs pre′ s₁′ is sf′
    → O1-Trace d (i ∷ is)
    → StateEq s s′
    → Preprocessed.memory sf ≡ Preprocessed.memory sf′
    → Preprocessed.pub-out-rem sf  ≡ []
    → Preprocessed.pub-out-rem sf′ ≡ []
    → Preprocessed.priv-rem    sf  ≡ []
    → Preprocessed.priv-rem    sf′ ≡ []
    → Preprocessed.pub-in-idx s ≡ a + d
    → take a (ProofPreimage.pub-transcript-inputs pre)
      ≡ take a (ProofPreimage.pub-transcript-inputs pre′)
    → WalkEq pre pre′ s s′ sf sf′

  -- The empty trace: both final states are the start states themselves;
  -- the backward rem-equalities come from the supplied final emptiness,
  -- and `O1-Trace d []` forces `d ≡ 0` so `pub-in-idx ≡ a`.
  walk-eq pre pre′ [] {s} {s′} {a = a} {d} r-done r-done o1-nil se mfeq
          poe poe′ pve pve′ idx≡ tpin =
    mk-walkeq se (trans poe (sym poe′)) (trans pve (sym pve′))
              a (trans idx≡ (+-identityʳ a)) tpin

  -- Every pure-memory instruction delegates to `pure-step`.
  walk-eq pre pre′ (assert _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (cond-select _ _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (constrain-bits _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (constrain-eq _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (constrain-to-boolean _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (copy _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (ec-add _ _ _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (ec-mul _ _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (ec-mul-generator _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (hash-to-curve _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (load-imm _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (div-mod-power-of-two _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (reconstitute-field _ _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (transient-hash _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (persistent-hash _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (test-eq _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (add _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (mul _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (neg _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (not _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′
  walk-eq pre pre′ (less-than _ _ _ ∷ _) (r-step step rest) (r-step step′ rest′) =
    pure-step pre pre′ tt step step′ rest rest′

  -- `output`: the appended output cell is a memory lookup, equal across
  -- runs; memory / pis / pub-in-idx / pi-skips / rems pass through.
  walk-eq pre pre′ (output var ∷ is) {s} {s′} {a = a} {d}
          (r-step (r-output {v = v} lk) rest)
          (r-step (r-output {v = v′} lk′) rest′)
          (o1-other _ o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let v≡ : v ≡ v′
        v≡ = just-injective
               (trans (sym lk) (trans (lookup-cong var (se-mem se)) lk′))
        se₁ : StateEq (record s { outputs = Preprocessed.outputs s ++ (v ∷ []) })
                      (record s′ { outputs = Preprocessed.outputs s′ ++ (v′ ∷ []) })
        se₁ = mk-stateq (se-mem se) (se-pis se) (se-idx se)
                (cong₂ _++_ (se-out se) (cong (_∷ []) v≡)) (se-skip se)
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = d} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ idx≡ tpin
    in mk-walkeq seq po≡ pv≡ af idx tpn

  -- `declare-pub-input`: the appended pis cell is a memory lookup, equal
  -- across runs; `pub-in-idx` bumps by one in both, and the O1 counter
  -- `d` bumps to `suc d` while `a` is unchanged, so `pub-in-idx ≡ a +
  -- suc d` is restored.
  walk-eq pre pre′ (declare-pub-input var ∷ is) {s} {s′} {a = a} {d}
          (r-step (r-declare-pub-input {v = v} lk) rest)
          (r-step (r-declare-pub-input {v = v′} lk′) rest′)
          (o1-decl o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let v≡ : v ≡ v′
        v≡ = just-injective
               (trans (sym lk) (trans (lookup-cong var (se-mem se)) lk′))
        se₁ : StateEq
                (record s { pis = Preprocessed.pis s ++ (v ∷ [])
                          ; pub-in-idx = suc (Preprocessed.pub-in-idx s) })
                (record s′ { pis = Preprocessed.pis s′ ++ (v′ ∷ [])
                           ; pub-in-idx = suc (Preprocessed.pub-in-idx s′) })
        se₁ = mk-stateq (se-mem se)
                (cong₂ _++_ (se-pis se) (cong (_∷ []) v≡))
                (cong suc (se-idx se)) (se-out se) (se-skip se)
        idx₁ : suc (Preprocessed.pub-in-idx s) ≡ a + suc d
        idx₁ = trans (cong suc idx≡) (sym (+-suc a d))
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = suc d} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ idx₁ tpin
    in mk-walkeq seq po≡ pv≡ af idx tpn

  -- `pi-skip` ── ACTIVE / ACTIVE.  The validation premise pins the last
  -- `count = d` declared cells against the transcript window at
  -- `pub-in-idx ∸ d ≡ a`; the cells (`pis`) agree across runs, so the two
  -- windows agree, extending the pinned prefix from `a` to `a + d`.  The
  -- counter resets (`d := 0`, `a := a + d`); `pub-in-idx` is unchanged.
  walk-eq pre pre′ (pi-skip g count ∷ is) {s} {s′} {a = a} {d}
          (r-step (r-pi-skip-active _ chk) rest)
          (r-step (r-pi-skip-active _ chk′) rest′)
          (o1-skip n≡d o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let se₁ : StateEq
                (record s  { pi-skips = Preprocessed.pi-skips s  ++ (nothing ∷ []) })
                (record s′ { pi-skips = Preprocessed.pi-skips s′ ++ (nothing ∷ []) })
        se₁ = mk-stateq (se-mem se) (se-pis se) (se-idx se) (se-out se)
                (cong₂ _++_ (se-skip se) refl)
        pti  = ProofPreimage.pub-transcript-inputs pre
        pti′ = ProofPreimage.pub-transcript-inputs pre′
        -- The validated window, with `count` rewritten to `d` and
        -- `pub-in-idx ∸ d` to `a`.
        win-s : Preprocessed.pub-in-idx s ∸ count ≡ a
        win-s = trans (cong (_∸ count) idx≡)
                      (trans (cong (λ k → (a + d) ∸ k) n≡d) (m+n∸n≡m a d))
        win-s′ : Preprocessed.pub-in-idx s′ ∸ count ≡ a
        win-s′ = trans (cong (_∸ count) (trans (sym (se-idx se)) idx≡))
                       (trans (cong (λ k → (a + d) ∸ k) n≡d) (m+n∸n≡m a d))
        rec-eq : drop (length (Preprocessed.pis s) ∸ count) (Preprocessed.pis s)
               ≡ drop (length (Preprocessed.pis s′) ∸ count) (Preprocessed.pis s′)
        rec-eq = cong₂ (λ l xs → drop (l ∸ count) xs)
                       (cong length (se-pis se)) (se-pis se)
        exp-s  : take count (drop (Preprocessed.pub-in-idx s ∸ count) pti)
               ≡ take d (drop a pti)
        exp-s  = cong₂ (λ k st → take k (drop st pti)) n≡d win-s
        exp-s′ : take count (drop (Preprocessed.pub-in-idx s′ ∸ count) pti′)
               ≡ take d (drop a pti′)
        exp-s′ = cong₂ (λ k st → take k (drop st pti′)) n≡d win-s′
        win-eq : take d (drop a pti) ≡ take d (drop a pti′)
        win-eq = trans (sym exp-s)
                   (trans (sym (≡ᶠ-list?-true _ _ chk))
                     (trans rec-eq (trans (≡ᶠ-list?-true _ _ chk′) exp-s′)))
        tpin₁ : take (a + d) pti ≡ take (a + d) pti′
        tpin₁ = trans (take-split a d pti)
                  (trans (cong₂ _++_ tpin win-eq)
                         (sym (take-split a d pti′)))
        idx₁ : Preprocessed.pub-in-idx s ≡ (a + d) + 0
        idx₁ = trans idx≡ (sym (+-identityʳ (a + d)))
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a + d} {d = 0} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ idx₁ tpin₁
    in mk-walkeq seq po≡ pv≡ af idx tpn

  -- Mixed guard outcomes are impossible: a guard reads the same wire of
  -- equal memories.  An unguarded skip is always active.
  walk-eq pre pre′ (pi-skip nothing count ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-pi-skip-active _ _) _)
          (r-step (r-pi-skip-inactive gf′) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (unguarded-not-inactive gf′)
  walk-eq pre pre′ (pi-skip nothing count ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-pi-skip-inactive gf) _)
          (r-step (r-pi-skip-active _ _) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (unguarded-not-inactive gf)
  walk-eq pre pre′ (pi-skip (just gw) count ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-pi-skip-active gt _) _)
          (r-step (r-pi-skip-inactive gf′) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (guard-mismatch gw (se-mem se) gt gf′)
  walk-eq pre pre′ (pi-skip (just gw) count ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-pi-skip-inactive gf) _)
          (r-step (r-pi-skip-active gt′ _) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (guard-mismatch gw (sym (se-mem se)) gt′ gf)

  -- `pi-skip` ── INACTIVE / INACTIVE.  Both roll `pub-in-idx` back by
  -- `count = d` (discarding the current group), so `pub-in-idx ∸ d ≡ a`
  -- and the counter resets (`d := 0`, `a` unchanged).
  walk-eq pre pre′ (pi-skip g count ∷ is) {s} {s′} {a = a} {d}
          (r-step (r-pi-skip-inactive _) rest)
          (r-step (r-pi-skip-inactive _) rest′)
          (o1-skip n≡d o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let se₁ : StateEq
                (record s  { pi-skips   = Preprocessed.pi-skips s  ++ (just count ∷ [])
                           ; pub-in-idx = Preprocessed.pub-in-idx s  ∸ count })
                (record s′ { pi-skips   = Preprocessed.pi-skips s′ ++ (just count ∷ [])
                           ; pub-in-idx = Preprocessed.pub-in-idx s′ ∸ count })
        se₁ = mk-stateq (se-mem se) (se-pis se)
                (cong (_∸ count) (se-idx se)) (se-out se)
                (cong₂ _++_ (se-skip se) refl)
        idx₁ : Preprocessed.pub-in-idx s ∸ count ≡ a + 0
        idx₁ = trans (cong (_∸ count) idx≡)
                 (trans (cong (λ k → (a + d) ∸ k) n≡d)
                        (trans (m+n∸n≡m a d) (sym (+-identityʳ a))))
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = 0} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ idx₁ tpin
    in mk-walkeq seq po≡ pv≡ af idx tpn

  -- `public-input` ── INACTIVE / INACTIVE: pushes `0ᶠ`; pure memory.
  walk-eq pre pre′ (public-input g ∷ is) {s} {s′} {a = a} {d}
          (r-step step@(r-public-input-inactive gf) rest)
          (r-step step′@(r-public-input-inactive gf′) rest′)
          (o1-other _ o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let se₁ : StateEq (push-mem s 0ᶠ) (push-mem s′ 0ᶠ)
        se₁ = mk-stateq
                (step-mem-eq step step′ (se-mem se) mfeq
                  (proj₂ (mem-prefix rest)) (proj₂ (mem-prefix rest′)))
                (se-pis se) (se-idx se) (se-out se) (se-skip se)
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = d} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ idx≡ tpin
    in mk-walkeq seq po≡ pv≡ af idx tpn

  -- `public-input` ── ACTIVE / ACTIVE: consumes the next public-output
  -- cell.  Its value is the appended memory cell — equal across runs by
  -- the anchor — so the consumed-cursor cons reassembles the backward
  -- `pub-out-rem` equality.
  walk-eq pre pre′ (public-input g ∷ is) {s} {s′} {sf = sf} {sf′ = sf′} {a = a} {d}
          (r-step (r-public-input-active {v = v} {s₁ = c1} _ cp) rest)
          (r-step (r-public-input-active {v = v′} {s₁ = c1′} _ cp′) rest′)
          (o1-other _ o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let pf  : Preprocessed.memory sf
            ≡ (Preprocessed.memory s ++ (v ∷ [])) ++ proj₁ (mem-prefix rest)
        pf  = trans (proj₂ (mem-prefix rest))
                    (cong (λ m → (m ++ (v ∷ [])) ++ proj₁ (mem-prefix rest))
                          (consume-pub-out-mem _ cp))
        pf′ : Preprocessed.memory sf′
            ≡ (Preprocessed.memory s′ ++ (v′ ∷ [])) ++ proj₁ (mem-prefix rest′)
        pf′ = trans (proj₂ (mem-prefix rest′))
                    (cong (λ m → (m ++ (v′ ∷ [])) ++ proj₁ (mem-prefix rest′))
                          (consume-pub-out-mem _ cp′))
        v≡ : v ≡ v′
        v≡ = next-cell-eq (se-mem se) mfeq pf pf′
        memc : Preprocessed.memory (push-mem c1 v)
             ≡ Preprocessed.memory (push-mem c1′ v′)
        memc = cong₂ _++_ (trans (consume-pub-out-mem _ cp)
                            (trans (se-mem se) (sym (consume-pub-out-mem _ cp′))))
                          (cong (_∷ []) v≡)
        se₁ : StateEq (push-mem c1 v) (push-mem c1′ v′)
        se₁ = mk-stateq memc
                (trans (consume-pub-out-pis _ cp) (trans (se-pis se)
                  (sym (consume-pub-out-pis _ cp′))))
                (trans (consume-pub-out-idx _ cp) (trans (se-idx se)
                  (sym (consume-pub-out-idx _ cp′))))
                (trans (consume-pub-out-outputs _ cp) (trans (se-out se)
                  (sym (consume-pub-out-outputs _ cp′))))
                (trans (consume-pub-out-skips _ cp) (trans (se-skip se)
                  (sym (consume-pub-out-skips _ cp′))))
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = d} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ (trans (consume-pub-out-idx _ cp) idx≡) tpin
    in mk-walkeq seq
         (trans (consume-pub-out-rem _ cp)
           (trans (cong₂ _∷_ v≡ po≡) (sym (consume-pub-out-rem _ cp′))))
         (trans (sym (consume-pub-out-priv _ cp))
           (trans pv≡ (consume-pub-out-priv _ cp′)))
         af idx tpn

  -- Mixed guard outcomes for `public-input` are impossible.
  walk-eq pre pre′ (public-input nothing ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-public-input-active _ _) _)
          (r-step (r-public-input-inactive gf′) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (unguarded-not-inactive gf′)
  walk-eq pre pre′ (public-input nothing ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-public-input-inactive gf) _)
          (r-step (r-public-input-active _ _) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (unguarded-not-inactive gf)
  walk-eq pre pre′ (public-input (just gw) ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-public-input-active gt _) _)
          (r-step (r-public-input-inactive gf′) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (guard-mismatch gw (se-mem se) gt gf′)
  walk-eq pre pre′ (public-input (just gw) ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-public-input-inactive gf) _)
          (r-step (r-public-input-active gt′ _) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (guard-mismatch gw (sym (se-mem se)) gt′ gf)

  -- `private-input` ── INACTIVE / INACTIVE: pushes `0ᶠ`; pure memory.
  walk-eq pre pre′ (private-input g ∷ is) {s} {s′} {a = a} {d}
          (r-step step@(r-private-input-inactive gf) rest)
          (r-step step′@(r-private-input-inactive gf′) rest′)
          (o1-other _ o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let se₁ : StateEq (push-mem s 0ᶠ) (push-mem s′ 0ᶠ)
        se₁ = mk-stateq
                (step-mem-eq step step′ (se-mem se) mfeq
                  (proj₂ (mem-prefix rest)) (proj₂ (mem-prefix rest′)))
                (se-pis se) (se-idx se) (se-out se) (se-skip se)
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = d} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ idx≡ tpin
    in mk-walkeq seq po≡ pv≡ af idx tpn

  -- `private-input` ── ACTIVE / ACTIVE: consumes the next private cell.
  walk-eq pre pre′ (private-input g ∷ is) {s} {s′} {sf = sf} {sf′ = sf′} {a = a} {d}
          (r-step (r-private-input-active {v = v} {s₁ = c1} _ cp) rest)
          (r-step (r-private-input-active {v = v′} {s₁ = c1′} _ cp′) rest′)
          (o1-other _ o1t) se mfeq poe poe′ pve pve′ idx≡ tpin =
    let pf  : Preprocessed.memory sf
            ≡ (Preprocessed.memory s ++ (v ∷ [])) ++ proj₁ (mem-prefix rest)
        pf  = trans (proj₂ (mem-prefix rest))
                    (cong (λ m → (m ++ (v ∷ [])) ++ proj₁ (mem-prefix rest))
                          (consume-priv-mem _ cp))
        pf′ : Preprocessed.memory sf′
            ≡ (Preprocessed.memory s′ ++ (v′ ∷ [])) ++ proj₁ (mem-prefix rest′)
        pf′ = trans (proj₂ (mem-prefix rest′))
                    (cong (λ m → (m ++ (v′ ∷ [])) ++ proj₁ (mem-prefix rest′))
                          (consume-priv-mem _ cp′))
        v≡ : v ≡ v′
        v≡ = next-cell-eq (se-mem se) mfeq pf pf′
        memc : Preprocessed.memory (push-mem c1 v)
             ≡ Preprocessed.memory (push-mem c1′ v′)
        memc = cong₂ _++_ (trans (consume-priv-mem _ cp)
                            (trans (se-mem se) (sym (consume-priv-mem _ cp′))))
                          (cong (_∷ []) v≡)
        se₁ : StateEq (push-mem c1 v) (push-mem c1′ v′)
        se₁ = mk-stateq memc
                (trans (consume-priv-pis _ cp) (trans (se-pis se)
                  (sym (consume-priv-pis _ cp′))))
                (trans (consume-priv-idx _ cp) (trans (se-idx se)
                  (sym (consume-priv-idx _ cp′))))
                (trans (consume-priv-outputs _ cp) (trans (se-out se)
                  (sym (consume-priv-outputs _ cp′))))
                (trans (consume-priv-skips _ cp) (trans (se-skip se)
                  (sym (consume-priv-skips _ cp′))))
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = d} rest rest′ o1t se₁ mfeq
                  poe poe′ pve pve′ (trans (consume-priv-idx _ cp) idx≡) tpin
    in mk-walkeq seq
         (trans (sym (consume-priv-pub _ cp))
           (trans po≡ (consume-priv-pub _ cp′)))
         (trans (consume-priv-rem _ cp)
           (trans (cong₂ _∷_ v≡ pv≡) (sym (consume-priv-rem _ cp′))))
         af idx tpn

  -- Mixed guard outcomes for `private-input` are impossible.
  walk-eq pre pre′ (private-input nothing ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-private-input-active _ _) _)
          (r-step (r-private-input-inactive gf′) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (unguarded-not-inactive gf′)
  walk-eq pre pre′ (private-input nothing ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-private-input-inactive gf) _)
          (r-step (r-private-input-active _ _) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (unguarded-not-inactive gf)
  walk-eq pre pre′ (private-input (just gw) ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-private-input-active gt _) _)
          (r-step (r-private-input-inactive gf′) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (guard-mismatch gw (se-mem se) gt gf′)
  walk-eq pre pre′ (private-input (just gw) ∷ is) {s} {s′} {a = a} {d = d}
          (r-step (r-private-input-inactive gf) _)
          (r-step (r-private-input-active gt′ _) _) _ se _ _ _ _ _ _ _ =
    ⊥-elim (guard-mismatch gw (sym (se-mem se)) gt′ gf)

  pure-step pre pre′ {i} {is} {s} {s′} {s₁} {s₁′} {sf} {sf′} {a} {d}
            pm step step′ rest rest′ o1 se mfeq poe poe′ pve pve′ idx≡ tpin =
    let se₁ : StateEq s₁ s₁′
        se₁ = mk-stateq
                (step-mem-eq step step′ (se-mem se) mfeq
                  (proj₂ (mem-prefix rest)) (proj₂ (mem-prefix rest′)))
                (trans (pm-pis step pm) (trans (se-pis se) (sym (pm-pis step′ pm))))
                (trans (pm-idx step pm) (trans (se-idx se) (sym (pm-idx step′ pm))))
                (trans (pm-out step pm) (trans (se-out se) (sym (pm-out step′ pm))))
                (trans (pm-skip step pm) (trans (se-skip se) (sym (pm-skip step′ pm))))
        idx₁ : Preprocessed.pub-in-idx s₁ ≡ a + d
        idx₁ = trans (pm-idx step pm) idx≡
        (mk-walkeq seq po≡ pv≡ af idx tpn) =
          walk-eq pre pre′ is {a = a} {d = d} rest rest′ (o1-pure pm o1) se₁ mfeq
                  poe poe′ pve pve′ idx₁ tpin
    in mk-walkeq seq
         (trans (sym (pm-pubrem step pm)) (trans po≡ (pm-pubrem step′ pm)))
         (trans (sym (pm-privrem step pm)) (trans pv≡ (pm-privrem step′ pm)))
         af idx tpn

  ------------------------------------------------------------------------
  -- Record congruences (each field equal ⇒ records equal); proved by
  -- matching all components `refl`.
  ------------------------------------------------------------------------
  mk-state-cong : ∀ {m m′ p p′ k k′ i i′ po po′ pv pv′ o o′}
    → m ≡ m′ → p ≡ p′ → k ≡ k′ → i ≡ i′ → po ≡ po′ → pv ≡ pv′ → o ≡ o′
    → mk-state m p k i po pv o ≡ mk-state m′ p′ k′ i′ po′ pv′ o′
  mk-state-cong refl refl refl refl refl refl refl = refl

  mk-preimage-cong : ∀ {ins ins′ b b′ cm cm′ ti ti′ to to′ pv pv′}
    → ins ≡ ins′ → b ≡ b′ → cm ≡ cm′ → ti ≡ ti′ → to ≡ to′ → pv ≡ pv′
    → mk-preimage ins b cm ti to pv ≡ mk-preimage ins′ b′ cm′ ti′ to′ pv′
  mk-preimage-cong refl refl refl refl refl refl = refl

  ------------------------------------------------------------------------
  -- Initial-state field extraction from `init-state src pre ≡ just s₀`.
  -- `init-state` seeds memory with `inputs pre`, pis with the
  -- binding-input (plus the commitment value when the flag is set),
  -- `pub-out-rem` / `priv-rem` with the two transcripts, and the rest
  -- empty; it also enforces WF1 (`length inputs ≡ num-inputs`).
  ------------------------------------------------------------------------
  record InitFields (src : IrSource) (pre : ProofPreimage)
                    (hc : Bool) (s₀ : Preprocessed) : Set where
    constructor mk-init
    field
      if-mem  : Preprocessed.memory s₀ ≡ ProofPreimage.inputs pre
      if-pis  : Σ Fr (λ b → Σ (List Fr) (λ rest →
                  (Preprocessed.pis s₀ ≡ b ∷ rest)
                × (b ≡ ProofPreimage.binding-input pre)))
      if-idx  : Preprocessed.pub-in-idx s₀ ≡ 0
      if-pout : Preprocessed.pub-out-rem s₀ ≡ ProofPreimage.pub-transcript-outputs pre
      if-priv : Preprocessed.priv-rem s₀ ≡ ProofPreimage.priv-transcript pre
      if-out  : Preprocessed.outputs s₀ ≡ []
      if-skip : Preprocessed.pi-skips s₀ ≡ []
      if-wf1  : length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src
      -- The pis preamble length is fixed by the (shared) flag `hc`.
      if-plen : length (Preprocessed.pis s₀) ≡ (if hc then 2 else 1)

  init-fields : ∀ (src : IrSource) (pre : ProofPreimage) (hc : Bool) {s₀}
    → IrSource.do-communications-commitment src ≡ hc
    → init-state src pre ≡ just s₀ → InitFields src pre hc s₀
  init-fields src pre hc hc≡ eq
    with length (ProofPreimage.inputs pre) ≡ᵇ IrSource.num-inputs src
       in wf-eq
       | IrSource.do-communications-commitment src
       | hc≡
       | ProofPreimage.comm-commitment pre
  init-fields src pre .false refl refl | true | false | refl | _ =
    mk-init refl (_ , [] , refl , refl) refl refl refl refl refl
      (≡ᵇ⇒≡ _ _ (subst T (sym wf-eq) tt)) refl
  init-fields src pre .true refl refl | true | true | refl | just (c , r) =
    mk-init refl (_ , c ∷ [] , refl , refl) refl refl refl refl refl
      (≡ᵇ⇒≡ _ _ (subst T (sym wf-eq) tt)) refl
  init-fields src pre hc hc≡ () | false | _ | _ | _
  init-fields src pre .true refl () | true | true | refl | nothing

  ------------------------------------------------------------------------
  -- Commitment-field equality.  Flag off: `CommWF` forces both
  -- commitments `nothing`.  Flag on: `comm-ok` forces both `just (c , r)`;
  -- the value `c` sits in the (equal) pis preamble, and the randomness
  -- `r` equals `comm-rand-of`, pinned by witness equality (E3).
  ------------------------------------------------------------------------

  comm-rand-of-just : ∀ (pre : ProofPreimage) {c r}
    → ProofPreimage.comm-commitment pre ≡ just (c , r)
    → comm-rand-of pre ≡ just r
  comm-rand-of-just pre eq = cong (Data.Maybe.map (λ (_ , r) → r)) eq

  -- With the flag on, `comm-ok` rules out a missing commitment.
  comm-ok-just : ∀ (src : IrSource) (pre : ProofPreimage) (s : Preprocessed)
    → IrSource.do-communications-commitment src ≡ true
    → T (comm-ok src pre s)
    → Σ Fr (λ c → Σ Fr (λ r →
        ProofPreimage.comm-commitment pre ≡ just (c , r)))
  comm-ok-just src pre s hc≡ co
    with IrSource.do-communications-commitment src | hc≡
       | ProofPreimage.comm-commitment pre
  ... | true | refl | just (c , r) = c , r , refl

  -- With the flag off, `CommWF` rules out a present commitment.
  commwf-nothing : ∀ (pre : ProofPreimage)
    → CommWF false (ProofPreimage.comm-commitment pre)
    → ProofPreimage.comm-commitment pre ≡ nothing
  commwf-nothing pre cwf with ProofPreimage.comm-commitment pre
  ... | nothing = refl

  -- The pis preamble's commitment cell, recovered from `init-state` in
  -- the flag-on case: `pis s₀ ≡ binding ∷ c ∷ []` where `just (c , r)` is
  -- the carried commitment.
  init-comm-c : ∀ (src : IrSource) (pre : ProofPreimage) {s₀ c r}
    → IrSource.do-communications-commitment src ≡ true
    → ProofPreimage.comm-commitment pre ≡ just (c , r)
    → init-state src pre ≡ just s₀
    → Preprocessed.pis s₀ ≡ ProofPreimage.binding-input pre ∷ c ∷ []
  init-comm-c src pre hc≡ cc≡ eq
    with length (ProofPreimage.inputs pre) ≡ᵇ IrSource.num-inputs src
       | IrSource.do-communications-commitment src | hc≡
       | ProofPreimage.comm-commitment pre | cc≡
  ... | true | true | refl | just (c , r) | refl =
    sym (cong Preprocessed.pis (just-injective eq))
  init-comm-c src pre hc≡ cc≡ ()
    | false | true | refl | just (c , r) | refl

  ------------------------------------------------------------------------
  -- `transcripts-consumed` decomposed: the verifier-transcript length
  -- matches `pub-in-idx`, and the two output cursors are empty.
  ------------------------------------------------------------------------
  transcripts-fields : ∀ (pre : ProofPreimage) (s : Preprocessed)
    → T (transcripts-consumed pre s)
    → (length (ProofPreimage.pub-transcript-inputs pre)
         ≡ Preprocessed.pub-in-idx s)
    × (Preprocessed.pub-out-rem s ≡ [])
    × (Preprocessed.priv-rem s ≡ [])
  transcripts-fields pre s tc
    with Preprocessed.pub-out-rem s | Preprocessed.priv-rem s | tc
  ... | [] | [] | tc′ =
    ≡ᵇ⇒≡ _ _ (proj₁ (T-∧ .Equivalence.to tc′)) , refl , refl
  ... | _ ∷ _ | _     | tc′ = ⊥-elim (proj₂ (T-∧ .Equivalence.to tc′))
  ... | []    | _ ∷ _ | tc′ = ⊥-elim (proj₂ (T-∧ .Equivalence.to tc′))

------------------------------------------------------------------------
-- Extraction uniqueness.  For a producer-safe source, two CommWF
-- preimage/state pairs that realize the same witness are equal.
------------------------------------------------------------------------

circuit-statement-unique
  : ∀ (src : IrSource) {pre s pre′ s′}
  → T (producer-safe src)
  → CommWF (IrSource.do-communications-commitment src)
           (ProofPreimage.comm-commitment pre)
  → CommWF (IrSource.do-communications-commitment src)
           (ProofPreimage.comm-commitment pre′)
  → R src pre s → R src pre′ s′
  → witness-of s pre ≡ witness-of s′ pre′
  → (pre ≡ pre′) × (s ≡ s′)
circuit-statement-unique src {pre} {s} {pre′} {s′} ps cwf cwf′
  (s₀ , init≡ , Rs , tc , co) (s₀′ , init≡′ , Rs′ , tc′ , co′) weq =
  pre≡ , s≡
  where
    hc = IrSource.do-communications-commitment src
    instrs = IrSource.instructions src

    E1 : Preprocessed.memory s ≡ Preprocessed.memory s′
    E1 = cong Witness.mem weq
    E2 : Preprocessed.pis s ≡ Preprocessed.pis s′
    E2 = cong Witness.pis weq
    E3 : comm-rand-of pre ≡ comm-rand-of pre′
    E3 = cong Witness.comm-rand weq

    if₁ = init-fields src pre  hc refl init≡
    if₂ = init-fields src pre′ hc refl init≡′

    -- Initial memories are equal-length prefixes (length `num-inputs`) of
    -- the common final memory, hence equal.
    mem₀≡ : Preprocessed.memory s₀ ≡ Preprocessed.memory s₀′
    mem₀≡ = proj₁ (split-eq (Preprocessed.memory s₀) (Preprocessed.memory s₀′)
                     (proj₁ (mem-prefix Rs)) (proj₁ (mem-prefix Rs′))
                     (trans (sym (proj₂ (mem-prefix Rs)))
                            (trans E1 (proj₂ (mem-prefix Rs′))))
                     len₀)
      where
        len₀ : length (Preprocessed.memory s₀)
             ≡ length (Preprocessed.memory s₀′)
        len₀ = trans (cong length (InitFields.if-mem if₁))
                 (trans (InitFields.if-wf1 if₁)
                   (trans (sym (InitFields.if-wf1 if₂))
                          (sym (cong length (InitFields.if-mem if₂)))))

    -- Initial pis are equal-length prefixes (preamble length) of the
    -- common final pis, hence equal.
    pis₀≡ : Preprocessed.pis s₀ ≡ Preprocessed.pis s₀′
    pis₀≡ = proj₁ (split-eq (Preprocessed.pis s₀) (Preprocessed.pis s₀′)
                     (proj₁ (pis-prefix Rs)) (proj₁ (pis-prefix Rs′))
                     (trans (sym (proj₂ (pis-prefix Rs)))
                            (trans E2 (proj₂ (pis-prefix Rs′))))
                     (trans (InitFields.if-plen if₁)
                            (sym (InitFields.if-plen if₂))))

    se₀ : StateEq s₀ s₀′
    se₀ = mk-stateq mem₀≡ pis₀≡
            (trans (InitFields.if-idx if₁) (sym (InitFields.if-idx if₂)))
            (trans (InitFields.if-out if₁) (sym (InitFields.if-out if₂)))
            (trans (InitFields.if-skip if₁) (sym (InitFields.if-skip if₂)))

    -- The two output cursors at the final states are empty.
    tf  = transcripts-fields pre  s  tc
    tf′ = transcripts-fields pre′ s′ tc′

    walk = walk-eq pre pre′ instrs {a = 0} {d = 0} Rs Rs′ (o1-sound src ps)
             se₀ E1 (proj₁ (proj₂ tf)) (proj₁ (proj₂ tf′))
             (proj₂ (proj₂ tf)) (proj₂ (proj₂ tf′))
             (trans (InitFields.if-idx if₁) refl) refl
    se-fin   = WalkEq.we-state walk
    pout₀≡   = WalkEq.we-pout  walk
    priv₀≡   = WalkEq.we-priv  walk
    af       = WalkEq.we-af    walk
    pidx-fin = WalkEq.we-idx   walk
    tpin-fin = WalkEq.we-tpin  walk

    -- `s ≡ s′`: the five `StateEq` fields plus the (empty) cursors.
    s≡ : s ≡ s′
    s≡ = mk-state-cong E1 E2 (se-skip se-fin) (se-idx se-fin)
           (trans (proj₁ (proj₂ tf)) (sym (proj₁ (proj₂ tf′))))
           (trans (proj₂ (proj₂ tf)) (sym (proj₂ (proj₂ tf′))))
           (se-out se-fin)

    ----------------------------------------------------------------
    -- `pre ≡ pre′` field by field.
    ----------------------------------------------------------------
    inputs≡ : ProofPreimage.inputs pre ≡ ProofPreimage.inputs pre′
    inputs≡ = trans (sym (InitFields.if-mem if₁))
                (trans mem₀≡ (InitFields.if-mem if₂))

    -- The binding-input heads the (equal) pis preamble.
    bind≡ : ProofPreimage.binding-input pre ≡ ProofPreimage.binding-input pre′
    bind≡ =
      let (b , rest , pis≡ , b≡) = InitFields.if-pis if₁
          (b′ , rest′ , pis≡′ , b≡′) = InitFields.if-pis if₂
      in trans (sym b≡)
           (trans (∷-injectiveˡ (trans (sym pis≡) (trans pis₀≡ pis≡′))) b≡′)

    -- Public-output / private transcripts equal via the backward cursor
    -- equalities at the initial states.
    pout≡ : ProofPreimage.pub-transcript-outputs pre
          ≡ ProofPreimage.pub-transcript-outputs pre′
    pout≡ = trans (sym (InitFields.if-pout if₁))
              (trans pout₀≡ (InitFields.if-pout if₂))

    priv≡ : ProofPreimage.priv-transcript pre
          ≡ ProofPreimage.priv-transcript pre′
    priv≡ = trans (sym (InitFields.if-priv if₁))
              (trans priv₀≡ (InitFields.if-priv if₂))

    -- Verifier transcript: the pinned prefix is everything, since
    -- `transcripts-consumed` fixes both lengths to `pub-in-idx`.
    pti≡ : ProofPreimage.pub-transcript-inputs pre
         ≡ ProofPreimage.pub-transcript-inputs pre′
    pti≡ =
      trans (sym (take-all af (ProofPreimage.pub-transcript-inputs pre) len₁))
        (trans tpin-fin
          (take-all af (ProofPreimage.pub-transcript-inputs pre′) len₂))
      where
        len₁ : length (ProofPreimage.pub-transcript-inputs pre) ≤ af
        len₁ = ≤-reflexive (trans (proj₁ tf) pidx-fin)
        len₂ : length (ProofPreimage.pub-transcript-inputs pre′) ≤ af
        len₂ = ≤-reflexive (trans (proj₁ tf′)
                 (trans (sym (se-idx se-fin)) pidx-fin))

    -- Flag off: `CommWF` forces both commitments `nothing`.  Flag on:
    -- `comm-ok` forces both `just`; the randomness is pinned by E3 and
    -- the commitment value by the (equal) pis preambles.
    comm≡ : ProofPreimage.comm-commitment pre
          ≡ ProofPreimage.comm-commitment pre′
    comm≡ = go (IrSource.do-communications-commitment src) refl
      where
        go : ∀ b → IrSource.do-communications-commitment src ≡ b
           → ProofPreimage.comm-commitment pre
             ≡ ProofPreimage.comm-commitment pre′
        go false hc≡f =
          trans
            (commwf-nothing pre
              (subst (λ x → CommWF x (ProofPreimage.comm-commitment pre))
                     hc≡f cwf))
            (sym (commwf-nothing pre′
              (subst (λ x → CommWF x (ProofPreimage.comm-commitment pre′))
                     hc≡f cwf′)))
        go true hc≡t =
          let (c  , r  , cc≡ ) = comm-ok-just src pre  s  hc≡t co
              (c′ , r′ , cc≡′) = comm-ok-just src pre′ s′ hc≡t co′
              r≡ = just-injective
                     (trans (sym (comm-rand-of-just pre cc≡))
                            (trans E3 (comm-rand-of-just pre′ cc≡′)))
              c≡ = ∷-injectiveˡ (∷-injectiveʳ
                     (trans (sym (init-comm-c src pre hc≡t cc≡ init≡))
                       (trans pis₀≡
                              (init-comm-c src pre′ hc≡t cc≡′ init≡′))))
          in trans cc≡ (trans (cong just (cong₂ _,_ c≡ r≡)) (sym cc≡′))

    pre≡ : pre ≡ pre′
    pre≡ = mk-preimage-cong inputs≡ bind≡ comm≡ pti≡ pout≡ priv≡

------------------------------------------------------------------------
-- Exactly-one packaging.  `circuit-statement-sound` (existence) plus
-- `circuit-statement-unique` (uniqueness): every satisfying witness of
-- the allocated memory length has exactly one `CommWFRealizer` — one
-- whose `preimage`/`run` components any other agrees with.
--
-- The witness-side condition `RandWF` mirrors `CommWF`: when the
-- commitment flag is off, `satisfies` does not pin `Witness.comm-rand`
-- (`Maybe-shape false _` is trivial), and a witness carrying a junk
-- `just r` there is realized only by preimages carrying a junk
-- commitment — which `CommWF` excludes.  So a CommWF realizer exists
-- precisely when the witness does not carry the junk either.
------------------------------------------------------------------------

RandWF : Bool → Maybe Fr → Set
RandWF false (just _) = ⊥
RandWF false nothing  = ⊤
RandWF true  _        = ⊤

private
  -- A realizer of a RandWF witness is CommWF: its `comm-rand-of`
  -- projection IS the witness's `comm-rand` field.
  realizer-commwf : ∀ (src : IrSource) (pre : ProofPreimage)
      (s : Preprocessed) (w : Witness)
    → witness-of s pre ≡ w
    → RandWF (IrSource.do-communications-commitment src)
             (Witness.comm-rand w)
    → CommWF (IrSource.do-communications-commitment src)
             (ProofPreimage.comm-commitment pre)
  realizer-commwf src pre s w wo≡ rwf =
    go (IrSource.do-communications-commitment src) rwf
    where
      -- Case split on the commitment via an explicit equality (a `with`
      -- would abstract `comm-rand-of pre`, which matches on the same
      -- projection, out of the goal).
      go2 : ∀ (cc : Maybe (Fr × Fr))
        → ProofPreimage.comm-commitment pre ≡ cc
        → RandWF false (Witness.comm-rand w) → CommWF false cc
      go2 nothing        _   _ = tt
      go2 (just (c , rv)) cc≡ r =
        subst (RandWF false)
              (trans (sym (cong Witness.comm-rand wo≡))
                     (comm-rand-of-just pre cc≡))
              r

      go : ∀ b → RandWF b (Witness.comm-rand w)
         → CommWF b (ProofPreimage.comm-commitment pre)
      go true  _ = tt
      go false r = go2 (ProofPreimage.comm-commitment pre) refl r

circuit-statement-sound-unique
  : ∀ (src : IrSource) (w : Witness)
  → T (producer-safe src)
  → WF2 src
  → satisfies (circuit src) w
  → RandWF (IrSource.do-communications-commitment src)
           (Witness.comm-rand w)
  → Σ (CommWFRealizer src w) (λ can
      → ∀ (other : CommWFRealizer src w)
      → (CommWFRealizer.preimage other ≡ CommWFRealizer.preimage can)
      × (CommWFRealizer.run      other ≡ CommWFRealizer.run      can))
circuit-statement-sound-unique src w ps wf2 sat rwf =
  let (mk-realizer pre s Rs wo≡) =
        circuit-statement-sound src w ps wf2 sat
      cwf = realizer-commwf src pre s w wo≡ rwf
  in mk-commwf-realizer (mk-realizer pre s Rs wo≡) cwf
   , λ (mk-commwf-realizer (mk-realizer _ _ Rs′ wo≡′) cwf′) →
       circuit-statement-unique src ps cwf′ cwf Rs′ Rs
         (trans wo≡′ (sym wo≡))


