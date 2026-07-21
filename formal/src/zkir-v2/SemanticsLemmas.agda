{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

------------------------------------------------------------------------
-- Inversion and framing principles for the `Semantics` definitions.
--
-- This module is the single home for the lemma layer directly above
-- `Semantics`: lookup stability under memory extension, inversion of
-- the transcript consumers (`consume-pub-out` / `consume-priv`), and
-- inversion of `init-state`.  Scope discipline: only facts *about
-- definitions of `Semantics`* belong here — nothing about circuits,
-- obligations, or the proof modules.
--
-- `Semantics` itself stays a definition-only, spec-mirroring artifact
-- (spec §4); consumers import this module instead of re-proving these
-- facts privately.
------------------------------------------------------------------------

module zkir-v2.SemanticsLemmas (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯

open import Data.Bool using (Bool; true; false; T)
open import Data.List using (List; []; _∷_; _++_; length)
open import Data.List.Properties using (++-assoc)
open import Data.Maybe using (Maybe; nothing; just; _>>=_)
open import Data.Maybe.Properties using (just-injective)
open import Data.Nat using (ℕ; zero; suc; _≡ᵇ_)
open import Data.Nat.Properties using (≡ᵇ⇒≡)
open import Data.Product using (_×_; _,_; ∃; ∃₂; proj₁; proj₂)
open import Data.Sum using (_⊎_; inj₁; inj₂)
open import Data.Unit using (tt)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; subst)

------------------------------------------------------------------------
-- 1. `mem-lookup` under memory extension
--
-- Memory only ever grows by appending (spec P2), so a successful
-- lookup is stable under any suffix, and the freshly-pushed cell(s)
-- are found at the old length.  The `mem` argument is explicit to
-- make implicit-argument inference robust at use sites.
------------------------------------------------------------------------

lookup-extends : ∀ (mem suffix : List Fr) i {v}
  → mem-lookup mem i ≡ just v
  → mem-lookup (mem ++ suffix) i ≡ just v
lookup-extends []       _ _       ()
lookup-extends (x ∷ xs) _ zero    eq = eq
lookup-extends (x ∷ xs) s (suc i) eq = lookup-extends xs s i eq

lookup-new : ∀ (mem : List Fr) v
  → mem-lookup (mem ++ (v ∷ [])) (length mem) ≡ just v
lookup-new []       v = refl
lookup-new (x ∷ xs) v = lookup-new xs v

-- Two-cell variants for instructions with Δmem = 2.  The post-state's
-- memory is `(mem ++ (x ∷ [])) ++ (y ∷ [])` — the shape produced by
-- `push-mem (push-mem s x) y`.
lookup-new-fst : ∀ (mem : List Fr) x y
  → mem-lookup ((mem ++ (x ∷ [])) ++ (y ∷ [])) (length mem) ≡ just x
lookup-new-fst mem x y =
  lookup-extends (mem ++ (x ∷ [])) (y ∷ []) (length mem) (lookup-new mem x)

lookup-new-snd : ∀ (mem : List Fr) x y
  → mem-lookup ((mem ++ (x ∷ [])) ++ (y ∷ [])) (suc (length mem)) ≡ just y
lookup-new-snd []       x y = refl
lookup-new-snd (z ∷ zs) x y = lookup-new-snd zs x y

-- Extend a pre-state lookup across both pushed cells of a Δmem = 2
-- instruction (the iterated `(mem ++ x ∷ []) ++ y ∷ []` shape).
lookup-extend2 : ∀ (mem : List Fr) x y i {v}
  → mem-lookup mem i ≡ just v
  → mem-lookup ((mem ++ (x ∷ [])) ++ (y ∷ [])) i ≡ just v
lookup-extend2 mem x y i e =
  lookup-extends (mem ++ (x ∷ [])) (y ∷ []) i
    (lookup-extends mem (x ∷ []) i e)

lookup-uniq : ∀ (mem : List Fr) (i : Index) {v w}
  → mem-lookup mem i ≡ just v
  → mem-lookup mem i ≡ just w
  → v ≡ w
lookup-uniq _ _ p q = just-injective (trans (sym p) q)

-- A pre-state lookup, extended to the post-state memory, matched
-- against a second lookup at the same index: identifies the
-- operational operand value `v` with the witnessed value `w`.
extend-uniq : ∀ (mem suffix : List Fr) (i : Index) {v w}
  → mem-lookup mem i ≡ just v
  → mem-lookup (mem ++ suffix) i ≡ just w
  → v ≡ w
extend-uniq mem suffix i la la' =
  lookup-uniq (mem ++ suffix) i (lookup-extends mem suffix i la) la'

-- The freshly-pushed cell, matched against a lookup at the new index
-- `length mem`: identifies the pushed value `v` with the witnessed
-- output value `w`.
new-uniq : ∀ (mem : List Fr) v {w}
  → mem-lookup (mem ++ (v ∷ [])) (length mem) ≡ just w
  → v ≡ w
new-uniq mem v lout =
  lookup-uniq (mem ++ (v ∷ [])) (length mem) (lookup-new mem v) lout

-- Two-cell analogues of `new-uniq`.
new2-uniq-fst : ∀ (mem : List Fr) x y {w}
  → mem-lookup ((mem ++ (x ∷ [])) ++ (y ∷ [])) (length mem) ≡ just w
  → x ≡ w
new2-uniq-fst mem x y l =
  lookup-uniq ((mem ++ (x ∷ [])) ++ (y ∷ [])) (length mem)
    (lookup-new-fst mem x y) l

new2-uniq-snd : ∀ (mem : List Fr) x y {w}
  → mem-lookup ((mem ++ (x ∷ [])) ++ (y ∷ [])) (suc (length mem)) ≡ just w
  → y ≡ w
new2-uniq-snd mem x y l =
  lookup-uniq ((mem ++ (x ∷ [])) ++ (y ∷ [])) (suc (length mem))
    (lookup-new-snd mem x y) l

-- Multi-index analogue of `lookup-extends`, for instructions whose
-- premises look up an index list via `mem-lookups`.
mem-lookups-extends : ∀ (mem suffix : List Fr) (is : List Index) {vs}
  → mem-lookups mem is ≡ just vs
  → mem-lookups (mem ++ suffix) is ≡ just vs
mem-lookups-extends mem suffix []       refl = refl
mem-lookups-extends mem suffix (i ∷ is) eq   =
  aux (mem-lookup mem i)      refl
      (mem-lookups mem is)    refl
      eq
  where
    aux : ∀ (m : Maybe Fr) → mem-lookup mem i ≡ m
        → (ms : Maybe (List Fr)) → mem-lookups mem is ≡ ms
        → ∀ {vs}
        → (m >>= λ v → ms >>= λ vs' → just (v ∷ vs')) ≡ just vs
        → mem-lookups (mem ++ suffix) (i ∷ is) ≡ just vs
    aux nothing   _    _          _     ()
    aux (just _)  _    nothing    _     ()
    aux (just v)  m-eq (just vs') ms-eq refl
      rewrite lookup-extends mem suffix i {v} m-eq
            | mem-lookups-extends mem suffix is {vs'} ms-eq
      = refl

-- `push-mem2 s x y`'s memory unfolds to `mem ++ (x ∷ y ∷ [])`; the
-- iterated `push-mem` form is `(mem ++ x ∷ []) ++ y ∷ []`.  The two
-- shapes are propositionally equal by `++`-associativity.
push-mem2-assoc : ∀ (mem : List Fr) x y
  → mem ++ (x ∷ y ∷ []) ≡ (mem ++ (x ∷ [])) ++ (y ∷ [])
push-mem2-assoc mem x y = sym (++-assoc mem (x ∷ []) (y ∷ []))

------------------------------------------------------------------------
-- 2. Inversion of the transcript consumers
--
-- `consume-pub-out` / `consume-priv` touch exactly one field: they
-- pop the head of their transcript stream and leave every other
-- state component unchanged.  `consume-*-inv` is the full
-- characterization; the named corollaries below project the facts
-- the proof modules consume.
------------------------------------------------------------------------

consume-pub-out-inv : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → ∃ λ rest
      → (Preprocessed.pub-out-rem s ≡ v ∷ rest)
      × (s′ ≡ record s { pub-out-rem = rest })
consume-pub-out-inv s eq with Preprocessed.pub-out-rem s | eq
... | []       | ()
... | w ∷ rest | p =
  rest
  , cong (_∷ rest) (cong proj₁ (just-injective p))
  , sym (cong proj₂ (just-injective p))

consume-priv-inv : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → ∃ λ rest
      → (Preprocessed.priv-rem s ≡ v ∷ rest)
      × (s′ ≡ record s { priv-rem = rest })
consume-priv-inv s eq with Preprocessed.priv-rem s | eq
... | []       | ()
... | w ∷ rest | p =
  rest
  , cong (_∷ rest) (cong proj₁ (just-injective p))
  , sym (cong proj₂ (just-injective p))

-- Forward direction: a non-empty stream makes the consumer succeed
-- with exactly the head/tail split.
consume-pub-out-eq : ∀ s {v rest}
  → Preprocessed.pub-out-rem s ≡ v ∷ rest
  → consume-pub-out s ≡ just (v , record s { pub-out-rem = rest })
consume-pub-out-eq s eq with Preprocessed.pub-out-rem s | eq
... | _ ∷ _ | refl = refl

consume-priv-eq : ∀ s {v rest}
  → Preprocessed.priv-rem s ≡ v ∷ rest
  → consume-priv s ≡ just (v , record s { priv-rem = rest })
consume-priv-eq s eq with Preprocessed.priv-rem s | eq
... | _ ∷ _ | refl = refl

-- Projections: the fields a consumer leaves unchanged, and the
-- one-step stream relation on the field it pops.

consume-pub-out-mem : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → Preprocessed.memory s′ ≡ Preprocessed.memory s
consume-pub-out-mem s eq =
  let (_ , _ , s′≡) = consume-pub-out-inv s eq
  in cong Preprocessed.memory s′≡

consume-pub-out-pis : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → Preprocessed.pis s′ ≡ Preprocessed.pis s
consume-pub-out-pis s eq =
  let (_ , _ , s′≡) = consume-pub-out-inv s eq
  in cong Preprocessed.pis s′≡

consume-pub-out-skips : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → Preprocessed.pi-skips s′ ≡ Preprocessed.pi-skips s
consume-pub-out-skips s eq =
  let (_ , _ , s′≡) = consume-pub-out-inv s eq
  in cong Preprocessed.pi-skips s′≡

consume-pub-out-idx : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → Preprocessed.pub-in-idx s′ ≡ Preprocessed.pub-in-idx s
consume-pub-out-idx s eq =
  let (_ , _ , s′≡) = consume-pub-out-inv s eq
  in cong Preprocessed.pub-in-idx s′≡

consume-pub-out-outputs : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → Preprocessed.outputs s′ ≡ Preprocessed.outputs s
consume-pub-out-outputs s eq =
  let (_ , _ , s′≡) = consume-pub-out-inv s eq
  in cong Preprocessed.outputs s′≡

consume-pub-out-priv : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → Preprocessed.priv-rem s′ ≡ Preprocessed.priv-rem s
consume-pub-out-priv s eq =
  let (_ , _ , s′≡) = consume-pub-out-inv s eq
  in cong Preprocessed.priv-rem s′≡

consume-pub-out-rem : ∀ s {v s′}
  → consume-pub-out s ≡ just (v , s′)
  → Preprocessed.pub-out-rem s ≡ v ∷ Preprocessed.pub-out-rem s′
consume-pub-out-rem s {v} eq =
  let (rest , step , s′≡) = consume-pub-out-inv s eq
  in trans step (cong (v ∷_) (sym (cong Preprocessed.pub-out-rem s′≡)))

consume-priv-mem : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → Preprocessed.memory s′ ≡ Preprocessed.memory s
consume-priv-mem s eq =
  let (_ , _ , s′≡) = consume-priv-inv s eq
  in cong Preprocessed.memory s′≡

consume-priv-pis : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → Preprocessed.pis s′ ≡ Preprocessed.pis s
consume-priv-pis s eq =
  let (_ , _ , s′≡) = consume-priv-inv s eq
  in cong Preprocessed.pis s′≡

consume-priv-skips : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → Preprocessed.pi-skips s′ ≡ Preprocessed.pi-skips s
consume-priv-skips s eq =
  let (_ , _ , s′≡) = consume-priv-inv s eq
  in cong Preprocessed.pi-skips s′≡

consume-priv-idx : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → Preprocessed.pub-in-idx s′ ≡ Preprocessed.pub-in-idx s
consume-priv-idx s eq =
  let (_ , _ , s′≡) = consume-priv-inv s eq
  in cong Preprocessed.pub-in-idx s′≡

consume-priv-outputs : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → Preprocessed.outputs s′ ≡ Preprocessed.outputs s
consume-priv-outputs s eq =
  let (_ , _ , s′≡) = consume-priv-inv s eq
  in cong Preprocessed.outputs s′≡

consume-priv-pub : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → Preprocessed.pub-out-rem s′ ≡ Preprocessed.pub-out-rem s
consume-priv-pub s eq =
  let (_ , _ , s′≡) = consume-priv-inv s eq
  in cong Preprocessed.pub-out-rem s′≡

consume-priv-rem : ∀ s {v s′}
  → consume-priv s ≡ just (v , s′)
  → Preprocessed.priv-rem s ≡ v ∷ Preprocessed.priv-rem s′
consume-priv-rem s {v} eq =
  let (rest , step , s′≡) = consume-priv-inv s eq
  in trans step (cong (v ∷_) (sym (cong Preprocessed.priv-rem s′≡)))

------------------------------------------------------------------------
-- 3. Inversion of `init-state`
--
-- A successful `init-state src pre ≡ just s₀` pins every field of
-- `s₀` and enforces WF1 (input arity).  The `pis` preamble depends on
-- the communications-commitment flag, so `pis-shape` is a sum.
------------------------------------------------------------------------

record InitInv (src : IrSource) (pre : ProofPreimage)
               (s₀ : Preprocessed) : Set where
  constructor mk-init-inv
  field
    mem≡      : Preprocessed.memory s₀ ≡ ProofPreimage.inputs pre
    arity≡    : length (ProofPreimage.inputs pre)
                  ≡ IrSource.num-inputs src
    idx≡      : Preprocessed.pub-in-idx s₀ ≡ 0
    skips≡    : Preprocessed.pi-skips s₀ ≡ []
    outputs≡  : Preprocessed.outputs s₀ ≡ []
    pub-rem≡  : Preprocessed.pub-out-rem s₀
                  ≡ ProofPreimage.pub-transcript-outputs pre
    priv-rem≡ : Preprocessed.priv-rem s₀
                  ≡ ProofPreimage.priv-transcript pre
    pis-shape : (IrSource.do-communications-commitment src ≡ false
                  × Preprocessed.pis s₀
                    ≡ ProofPreimage.binding-input pre ∷ [])
              ⊎ ∃₂ λ c r
                  → (IrSource.do-communications-commitment src ≡ true)
                  × (ProofPreimage.comm-commitment pre ≡ just (c , r))
                  × (Preprocessed.pis s₀
                      ≡ ProofPreimage.binding-input pre ∷ c ∷ [])

init-state-inv : ∀ src pre {s₀}
  → init-state src pre ≡ just s₀
  → InitInv src pre s₀
init-state-inv src pre eq
  with length (ProofPreimage.inputs pre) ≡ᵇ IrSource.num-inputs src in ar
     | IrSource.do-communications-commitment src in hc
     | ProofPreimage.comm-commitment pre in cc
     | eq
... | false | _     | _            | ()
... | true  | true  | nothing      | ()
... | true  | false | _            | eq′ =
  let st≡ = just-injective eq′ in mk-init-inv
    (sym (cong Preprocessed.memory      st≡))
    (≡ᵇ⇒≡ _ _ (subst T (sym ar) tt))
    (sym (cong Preprocessed.pub-in-idx  st≡))
    (sym (cong Preprocessed.pi-skips    st≡))
    (sym (cong Preprocessed.outputs     st≡))
    (sym (cong Preprocessed.pub-out-rem st≡))
    (sym (cong Preprocessed.priv-rem    st≡))
    (inj₁ (hc , sym (cong Preprocessed.pis st≡)))
... | true  | true  | just (c , r) | eq′ =
  let st≡ = just-injective eq′ in mk-init-inv
    (sym (cong Preprocessed.memory      st≡))
    (≡ᵇ⇒≡ _ _ (subst T (sym ar) tt))
    (sym (cong Preprocessed.pub-in-idx  st≡))
    (sym (cong Preprocessed.pi-skips    st≡))
    (sym (cong Preprocessed.outputs     st≡))
    (sym (cong Preprocessed.pub-out-rem st≡))
    (sym (cong Preprocessed.priv-rem    st≡))
    (inj₂ (c , r , hc , cc , sym (cong Preprocessed.pis st≡)))

-- Named corollaries in the signatures the proof modules use.

init-state-memory : ∀ src pre s₀
  → init-state src pre ≡ just s₀
  → Preprocessed.memory s₀ ≡ ProofPreimage.inputs pre
init-state-memory src pre s₀ eq = InitInv.mem≡ (init-state-inv src pre eq)

init-state-num-inputs : ∀ src pre s₀
  → init-state src pre ≡ just s₀
  → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src
init-state-num-inputs src pre s₀ eq =
  InitInv.arity≡ (init-state-inv src pre eq)

init-state-pub-in-idx : ∀ src pre s₀
  → init-state src pre ≡ just s₀
  → Preprocessed.pub-in-idx s₀ ≡ 0
init-state-pub-in-idx src pre s₀ eq =
  InitInv.idx≡ (init-state-inv src pre eq)

init-state-pi-skips : ∀ src pre s₀
  → init-state src pre ≡ just s₀
  → Preprocessed.pi-skips s₀ ≡ []
init-state-pi-skips src pre s₀ eq =
  InitInv.skips≡ (init-state-inv src pre eq)

init-state-outputs : ∀ src pre s₀
  → init-state src pre ≡ just s₀
  → Preprocessed.outputs s₀ ≡ []
init-state-outputs src pre s₀ eq =
  InitInv.outputs≡ (init-state-inv src pre eq)

init-state-pub-rem : ∀ src pre s₀
  → init-state src pre ≡ just s₀
  → Preprocessed.pub-out-rem s₀
      ≡ ProofPreimage.pub-transcript-outputs pre
init-state-pub-rem src pre s₀ eq =
  InitInv.pub-rem≡ (init-state-inv src pre eq)

init-state-priv-rem : ∀ src pre s₀
  → init-state src pre ≡ just s₀
  → Preprocessed.priv-rem s₀ ≡ ProofPreimage.priv-transcript pre
init-state-priv-rem src pre s₀ eq =
  InitInv.priv-rem≡ (init-state-inv src pre eq)
