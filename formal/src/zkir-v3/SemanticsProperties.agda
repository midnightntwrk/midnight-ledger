{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Pure properties of the off-circuit (preprocess) semantics of zkir-v3.
--
-- Structural facts about the named store, operand resolution, the
-- single-assignment freshness discipline, and monotonicity of a run —
-- none of which mention the in-circuit constraint layer.  These are the
-- semantics-level lemmas shared by the faithfulness bridge, the backward
-- driver, and the static well-formedness / typing passes.
------------------------------------------------------------------------

module zkir-v3.SemanticsProperties (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)
open import zkir-v3.Semantics ⋯

open import Data.Bool using (Bool; true; false)
open import Data.Nat using (ℕ; zero; suc; _+_; _*_; _^_; _∸_; _<?_)
open import Data.Nat using () renaming (_≟_ to _≟ℕ_)
open import Data.List using (List; []; _∷_; _++_; map; length; take; drop;
                            concatMap)
open import Data.List.Properties using (++-identityʳ; ++-assoc)
open import Data.Maybe using (Maybe; just; nothing)
open import Data.Maybe.Properties using (just-injective)
open import Data.Product using (_×_; _,_; ∃)
open import Data.Unit using (⊤; tt)
open import Data.String using () renaming (_≟_ to _≟str_)
open import Function using (case_of_)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong)
open import Relation.Nullary using (yes; no; ¬_)
open import Relation.Nullary.Decidable using (isYes)

------------------------------------------------------------------------
-- Memory / resolution helpers under a single output binding.
------------------------------------------------------------------------

-- After binding `out` to `v`, the cell `out` holds exactly `v`.
ins-here : ∀ (out : Identifier) v (m : Mem)
  → ins out v m out ≡ just v
ins-here out v m with out ≟str out
... | yes _ = refl
... | no ¬p = case ¬p refl of λ ()

-- Looking up a key other than the one just bound sees through the bind.
ins-other : ∀ {id k : Identifier} v (m : Mem)
  → ¬ (id ≡ k)
  → ins k v m id ≡ m id
ins-other {id} {k} v m id≢k with id ≟str k
... | yes p = case id≢k p of λ ()
... | no  _ = refl

-- After binding `out` to `v`, any operand that resolved in the original
-- memory still resolves to the same value — given freshness of `out`
-- (an immediate is unaffected; a variable equal to `out` could not have
-- resolved before the binding, so it is impossible here).
resolve-pres : ∀ (out : Identifier) v (m : Mem) op {w}
  → m out ≡ nothing
  → resolve m op ≡ just w
  → resolve (ins out v m) op ≡ just w
resolve-pres out v m (imm x) _ r = r
resolve-pres out v m (var id) {w} fresh r with id ≟str out
... | no  _  = r
... | yes refl rewrite fresh = case r of λ ()

-- Invert `resolveᶠ`: a Native resolution comes from a Native value.
resolveᶠ-inv : ∀ (m : Mem) op {x}
  → resolveᶠ m op ≡ just x
  → resolve m op ≡ just (val-native x)
resolveᶠ-inv m op r with resolve m op
... | just (val-native _) with refl ← r = refl

-- And the converse: a Native value resolution is a Native resolution.
resolveᶠ-intro : ∀ (m : Mem) op {x}
  → resolve m op ≡ just (val-native x)
  → resolveᶠ m op ≡ just x
resolveᶠ-intro m op r rewrite r = refl

-- The Native-resolution analogue of `resolve-pres`.
resolveᶠ-pres : ∀ (out : Identifier) v (m : Mem) op {x}
  → m out ≡ nothing
  → resolveᶠ m op ≡ just x
  → resolveᶠ (ins out v m) op ≡ just x
resolveᶠ-pres out v m op {x} fresh r
  rewrite resolve-pres out v m op fresh (resolveᶠ-inv m op r) = refl

------------------------------------------------------------------------
-- Store extension and public-input prefix orders.
--
-- A single step only *extends* the named store (inserting fresh keys)
-- and *appends* to the public-input vector.
------------------------------------------------------------------------

-- Sub-map: m′ preserves every binding of m.
_⊑_ : Mem → Mem → Set
m ⊑ m′ = ∀ {id v} → m id ≡ just v → m′ id ≡ just v

⊑-refl : ∀ {m} → m ⊑ m
⊑-refl p = p

⊑-trans : ∀ {m m′ m″} → m ⊑ m′ → m′ ⊑ m″ → m ⊑ m″
⊑-trans f g p = g (f p)

-- Inserting a fresh key extends the store.
ins-⊑ : ∀ {out v m} → m out ≡ nothing → m ⊑ ins out v m
ins-⊑ {out} fresh {id} p with id ≟str out
... | yes refl = case trans (sym fresh) p of λ ()
... | no  _    = p

-- Prefix order on the public-input vector.
_≼_ : List Fr → List Fr → Set
xs ≼ ys = ∃ λ zs → ys ≡ xs ++ zs

≼-refl : ∀ {xs} → xs ≼ xs
≼-refl {xs} = [] , sym (++-identityʳ xs)

≼-trans : ∀ {xs ys zs} → xs ≼ ys → ys ≼ zs → xs ≼ zs
≼-trans {xs} (as , refl) (bs , refl) = (as ++ bs) , ++-assoc xs as bs

≼-append : ∀ xs ys → xs ≼ (xs ++ ys)
≼-append xs ys = ys , refl

------------------------------------------------------------------------
-- Single-assignment freshness predicates.
--
-- `out-fresh i m` is the freshness/distinctness precondition the
-- per-instruction lemma of `i` needs: every output identifier is unbound
-- in `m`, and (for multi-output instructions) the outputs are pairwise
-- distinct.  It is what the program-level proofs thread through a run as
-- their single-assignment invariant.
------------------------------------------------------------------------

AllFresh : List Identifier → Mem → Set
AllFresh []         m = ⊤
AllFresh (id ∷ ids) m = (m id ≡ nothing) × AllFresh ids m

NotIn : Identifier → List Identifier → Set
NotIn id []         = ⊤
NotIn id (k ∷ ks)   = ¬ (id ≡ k) × NotIn id ks

NoDup : List Identifier → Set
NoDup []         = ⊤
NoDup (id ∷ ids) = NotIn id ids × NoDup ids

out-fresh : Instruction → Mem → Set
out-fresh (encode _ outputs)              m = AllFresh outputs m × NoDup outputs
out-fresh (assert _)                      m = ⊤
out-fresh (cond-select _ _ _ o)           m = m o ≡ nothing
out-fresh (constrain-bits _ _)            m = ⊤
out-fresh (constrain-eq _ _)              m = ⊤
out-fresh (constrain-to-boolean _)        m = ⊤
out-fresh (copy _ o)                      m = m o ≡ nothing
out-fresh (impact _ _)                    m = ⊤
out-fresh (ec-mul _ _ o)                  m = m o ≡ nothing
out-fresh (ec-mul-generator _ o)          m = m o ≡ nothing
out-fresh (hash-to-curve _ o)             m = m o ≡ nothing
out-fresh (into-coordinates _ (xo , yo))  m =
  (m xo ≡ nothing) × (m yo ≡ nothing) × ¬ (xo ≡ yo)
out-fresh (from-coordinates _ o)          m = m o ≡ nothing
out-fresh (into-bytes32 _ o)              m = m o ≡ nothing
out-fresh (from-bytes32 _ _ o)            m = m o ≡ nothing
out-fresh (reverse-bytes _ o)             m = m o ≡ nothing
out-fresh (bytes32-into-low-high _ (lo , hi)) m =
  (m lo ≡ nothing) × (m hi ≡ nothing) × ¬ (lo ≡ hi)
out-fresh (bytes32-from-low-high _ o)     m = m o ≡ nothing
out-fresh (div-mod-power-of-two _ _ outs) m = AllFresh outs m × NoDup outs
out-fresh (reconstitute-field _ _ _ o)    m = m o ≡ nothing
out-fresh (transient-hash _ o)            m = m o ≡ nothing
out-fresh (persistent-hash _ _ o)         m = m o ≡ nothing
out-fresh (keccak256 _ _ o)               m = m o ≡ nothing
out-fresh (test-eq _ _ o)                 m = m o ≡ nothing
out-fresh (add _ _ o)                     m = m o ≡ nothing
out-fresh (mul _ _ o)                     m = m o ≡ nothing
out-fresh (neg _ o)                       m = m o ≡ nothing
out-fresh (inv _ o)                       m = m o ≡ nothing
out-fresh (not _ o)                       m = m o ≡ nothing
out-fresh (less-than _ _ _ o)             m = m o ≡ nothing
out-fresh (jubjub-scalar-from-native _ o) m = m o ≡ nothing
out-fresh (public-input _ _ o)            m = m o ≡ nothing
out-fresh (private-input _ _ o)           m = m o ≡ nothing
out-fresh (circuit-output _)              m = ⊤

------------------------------------------------------------------------
-- Single-step extension.
--
-- One step never shrinks the public-input vector nor drops a memory
-- binding: most instructions leave `pis` untouched (the only one that
-- touches it — `impact` — appends to it), and memory only grows by the
-- fresh output cells the step writes.  `step-extends` establishes both
-- `State.pis st ≼ State.pis st'` and `State.mem st ⊑ State.mem st'` from
-- one inversion of the step.  The helpers below (`insertMany-pis`,
-- `out1-⊑`, …) supply the two component facts per instruction shape.
------------------------------------------------------------------------

-- `insertMany` only ever rebinds memory cells (`out1`), so it leaves the
-- public-input vector untouched.
insertMany-pis : ∀ st ids vs {st'}
  → insertMany st ids vs ≡ just st'
  → State.pis st' ≡ State.pis st
insertMany-pis st []         []         refl = refl
insertMany-pis st (id ∷ ids) (v ∷ vs)   e    =
  insertMany-pis (out1 st id v) ids vs e
insertMany-pis st []         (_ ∷ _)    ()
insertMany-pis st (_ ∷ _)    []         ()

-- Transport a `pis`-equality to the prefix order.
pis-≡→≼ : ∀ {xs ys : List Fr} → ys ≡ xs → xs ≼ ys
pis-≡→≼ refl = ≼-refl

------------------------------------------------------------------------
-- Memory-extension helpers.
--
-- The memory component of `step-extends`: no-output instructions leave
-- memory unchanged, single-output instructions insert one fresh cell,
-- two-output instructions insert two, and the `insertMany` instructions
-- bind a whole (fresh, duplicate-free) list.  Given the
-- freshness/distinctness that `out-fresh` records, each shape yields
-- `State.mem st ⊑ State.mem st'`.
------------------------------------------------------------------------

-- Binding a key keeps the others unbound, given the key is distinct from
-- each of them.
allfresh-ins : ∀ (id : Identifier) v ids m
  → AllFresh ids m → NotIn id ids → AllFresh ids (ins id v m)
allfresh-ins id v []         m _          _            = tt
allfresh-ins id v (k ∷ ids)  m (fk , frs) (id≢k , nin) =
    trans (ins-other v m (λ k≡id → id≢k (sym k≡id))) fk
  , allfresh-ins id v ids m frs nin

-- A single fresh output binding extends the store.  Keyed on `out1` so
-- that the output identifier and value are read off the goal's state.
out1-⊑ : ∀ st (o : Identifier) v
  → State.mem st o ≡ nothing → State.mem st ⊑ State.mem (out1 st o v)
out1-⊑ st o v fresh = ins-⊑ {o} {v} {State.mem st} fresh

-- Two distinct fresh output bindings extend the store.  Keyed on the
-- nested `out1` so the identifiers and values come from the goal's state.
out2-⊑ : ∀ st (i₁ : Identifier) v₁ (i₂ : Identifier) v₂
  → State.mem st i₁ ≡ nothing → State.mem st i₂ ≡ nothing → ¬ (i₁ ≡ i₂)
  → State.mem st ⊑ State.mem (out1 (out1 st i₁ v₁) i₂ v₂)
out2-⊑ st i₁ v₁ i₂ v₂ f₁ f₂ i₁≢i₂ {id} p =
  ins-⊑ {i₂} {v₂} {ins i₁ v₁ (State.mem st)}
    (trans (ins-other v₁ (State.mem st)
             (λ i₂≡i₁ → i₁≢i₂ (sym i₂≡i₁))) f₂)
    {id} (ins-⊑ {i₁} {v₁} {State.mem st} f₁ {id} p)

-- `insertMany` over fresh, duplicate-free keys only extends the store.
insertMany-⊑ : ∀ st ids vs {st'}
  → insertMany st ids vs ≡ just st'
  → AllFresh ids (State.mem st) → NoDup ids
  → State.mem st ⊑ State.mem st'
insertMany-⊑ st []         []         refl _            _            =
  ⊑-refl {State.mem st}
insertMany-⊑ st (id ∷ ids) (v ∷ vs)   e    (fid , frs)  (nin , ndup) {k} p =
  insertMany-⊑ (out1 st id v) ids vs e
    (allfresh-ins id v ids (State.mem st) frs nin) ndup
    {k} (ins-⊑ {id} {v} {State.mem st} fid {k} p)
insertMany-⊑ st []         (_ ∷ _)    ()   _            _
insertMany-⊑ st (_ ∷ _)    []         ()   _            _

step-extends : ∀ {P S st st'} (i : Instruction)
  → step P S st i ≡ just st'
  → out-fresh i (State.mem st)
  → (State.pis st ≼ State.pis st') × (State.mem st ⊑ State.mem st')
step-extends {st = st} (encode input outputs) e (af , nd)
  with resolve (State.mem st) input
... | just v = pis-≡→≼ (insertMany-pis st outputs
                          (map val-native (encodeᵉ v)) e)
             , insertMany-⊑ st outputs (map val-native (encodeᵉ v)) e af nd
step-extends {st = st} (assert cond) e _
  with resolve𝔹 (State.mem st) cond
... | just true with refl ← e = ≼-refl , ⊑-refl {State.mem st}
step-extends {st = st} (cond-select bit a b output) e fresh
  with resolve𝔹 (State.mem st) bit
... | just bv with resolve (State.mem st) a | resolve (State.mem st) b
...   | just av | just bvl with typeof av ≟T typeof bvl
...     | yes _ with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (constrain-bits val bits) e _
  with resolveᶠ (State.mem st) val
... | just x with valFr x <? 2 ^ bits
...   | yes _ with refl ← e = ≼-refl , ⊑-refl {State.mem st}
step-extends {st = st} (constrain-eq a b) e _
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just av | just bv with valEq? av bv
...   | just true with refl ← e = ≼-refl , ⊑-refl {State.mem st}
step-extends {st = st} (constrain-to-boolean val) e _
  with resolve𝔹 (State.mem st) val
... | just _ with refl ← e = ≼-refl , ⊑-refl {State.mem st}
step-extends {st = st} (copy val output) e fresh
  with resolve (State.mem st) val
... | just v with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {P = P} {st = st} (impact guard inputs) e _
  with resolve-all-Fr (State.mem st) inputs
... | just vals with resolve𝔹 (State.mem st) guard
...   | just g with g
...     | true
          with take (length vals)
                 (drop (State.pti-idx st)
                   (ProofPreimage.pub-transcript-inputs P))
               ≟LFr vals
...       | yes _ with refl ← e =
            ≼-append (State.pis st) vals , ⊑-refl {State.mem st}
step-extends {st = st} (impact guard inputs) e _ | just vals | just g | false
  with refl ← e =
    ≼-append (State.pis st) (map (λ _ → 0ᶠ) vals) , ⊑-refl {State.mem st}
step-extends {st = st} (ec-mul a scalar output) e fresh
  with resolve (State.mem st) a | resolve (State.mem st) scalar
... | just (val-jubjub-point p) | just (val-jubjub-scalar s)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (ec-mul a scalar output) e fresh
    | just (val-secp-point p) | just (val-secp-scalar s)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (ec-mul-generator scalar output) e fresh
  with resolve (State.mem st) scalar
... | just (val-jubjub-scalar s) with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-scalar s)   with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (hash-to-curve inputs output) e fresh
  with resolve-all-Fr (State.mem st) inputs
... | just frs with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (into-coordinates point (xid , yid)) e
  (freshx , freshy , xid≢yid)
  with resolve (State.mem st) point
... | just (val-jubjub-point p) with coordsJ p
...   | (x , y) with refl ← e =
          ≼-refl
        , out2-⊑ st xid (val-native x) yid (val-native y)
            freshx freshy xid≢yid
step-extends {st = st} (into-coordinates point (xid , yid)) e
  (freshx , freshy , xid≢yid)
    | just (val-secp-point p) with coordsK p
...   | just (x , y) with refl ← e =
          ≼-refl
        , out2-⊑ st xid (val-secp-base x) yid (val-secp-base y)
            freshx freshy xid≢yid
step-extends {st = st} (from-coordinates (xop , yop) output) e fresh
  with resolve (State.mem st) xop | resolve (State.mem st) yop
... | just (val-native x) | just (val-native y) with fromCoordsJ x y
...   | just p with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (from-coordinates (xop , yop) output) e fresh
    | just (val-secp-base x) | just (val-secp-base y) with fromCoordsK x y
...   | just p with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (into-bytes32 input output) e fresh
  with resolve (State.mem st) input
... | just (val-native x)      with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-base x)   with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-scalar s) with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (from-bytes32 bytes native output) e fresh
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (from-bytes32 bytes bytes32 output) e _
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-extends {st = st} (from-bytes32 bytes jubjub-point output) e _
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-extends {st = st} (from-bytes32 bytes jubjub-scalar output) e _
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-extends {st = st} (from-bytes32 bytes secp-point output) e _
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-extends {st = st} (from-bytes32 bytes secp-base output) e fresh
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (from-bytes32 bytes secp-scalar output) e fresh
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (reverse-bytes bytes output) e fresh
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (bytes32-into-low-high bytes (loid , hiid)) e
  (freshl , freshh , loid≢hiid)
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with bytes32→low-high b
...   | (lo , hi) with refl ← e =
          ≼-refl
        , out2-⊑ st loid (val-native lo) hiid (val-native hi)
            freshl freshh loid≢hiid
step-extends {st = st} (bytes32-from-low-high (loop , hiop) output) e fresh
  with resolveᶠ (State.mem st) loop | resolveᶠ (State.mem st) hiop
... | just lo | just hi with low-high→bytes32 lo hi
...   | just b with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (div-mod-power-of-two val bits outs) e (af , nd)
  with resolveᶠ (State.mem st) val
... | just x =
        pis-≡→≼ (insertMany-pis st outs
          ( val-native (from-le-bits (drop bits (to-le-bits x)))
          ∷ val-native (from-le-bits (take bits (to-le-bits x))) ∷ []) e)
      , insertMany-⊑ st outs
          ( val-native (from-le-bits (drop bits (to-le-bits x)))
          ∷ val-native (from-le-bits (take bits (to-le-bits x))) ∷ [])
          e af nd
step-extends {st = st} (reconstitute-field divisor modulus bits output) e fresh
  with resolveᶠ (State.mem st) divisor | resolveᶠ (State.mem st) modulus
... | just d | just mo with valFr mo <? 2 ^ bits
...   | yes _ with valFr d <? 2 ^ (FR-BITS ∸ bits)
...     | yes _ with valFr mo + 2 ^ bits * valFr d <? FR-ORDER
...       | yes _ with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (transient-hash inputs output) e fresh
  with resolve-all-Fr (State.mem st) inputs
... | just frs with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (persistent-hash alignment inputs output) e fresh
  with resolve-all-Fr (State.mem st) inputs
... | just frs with persistent-hash-fn alignment frs
...   | just h with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (keccak256 alignment inputs output) e fresh
  with resolve-all-Fr (State.mem st) inputs
... | just frs with keccak-fn alignment frs
...   | just h with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (test-eq a b output) e fresh
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just av | just bv with valEq? av bv
...   | just eqv with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (add a b output) e fresh
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just (val-native x) | just (val-native y)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
... | just (val-jubjub-point p) | just (val-jubjub-point q)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-point p) | just (val-secp-point q)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-base x) | just (val-secp-base y)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-scalar x) | just (val-secp-scalar y)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (mul a b output) e fresh
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just (val-native x) | just (val-native y)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-base x) | just (val-secp-base y)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-scalar x) | just (val-secp-scalar y)
        with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (neg a output) e fresh
  with resolve (State.mem st) a
... | just (val-native x) with refl ← e = ≼-refl , out1-⊑ st output _ fresh
... | just (val-jubjub-point p) with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-point p) with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-base x) with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
... | just (val-secp-scalar x) with refl ← e =
        ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (inv a output) e fresh
  with resolve (State.mem st) a
... | just (val-native x) with invᶠ x
...   | just xi with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (inv a output) e fresh
    | just (val-secp-base x) with invᵇ x
...   | just xi with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (inv a output) e fresh
    | just (val-secp-scalar x) with invˢ x
...   | just xi with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (not a output) e fresh
  with resolve𝔹 (State.mem st) a
... | just b with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (less-than a b bits output) e fresh
  with resolveᶠ (State.mem st) a | resolveᶠ (State.mem st) b
... | just x | just y with valFr x <? 2 ^ bits
...   | yes _ with valFr y <? 2 ^ bits
...     | yes _ with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (jubjub-scalar-from-native a output) e fresh
  with resolveᶠ (State.mem st) a
... | just x with refl ← e = ≼-refl , out1-⊑ st output _ fresh
step-extends {st = st} (public-input guard val-t output) e fresh
  with eval-guard (State.mem st) guard
... | just g with g
...   | true with decode val-t
                    (take (encoded-len val-t) (State.pto-rem st))
...     | just v with refl ← e =
          ≼-refl , ins-⊑ {output} {v} {State.mem st} fresh
step-extends {st = st} (public-input guard val-t output) e fresh
    | just g | false with refl ← e =
        ≼-refl , out1-⊑ st output (default-val val-t) fresh
step-extends {st = st} (private-input guard val-t output) e fresh
  with eval-guard (State.mem st) guard
... | just g with g
...   | true with decode val-t
                    (take (encoded-len val-t) (State.priv-rem st))
...     | just v with refl ← e =
          ≼-refl , ins-⊑ {output} {v} {State.mem st} fresh
step-extends {st = st} (private-input guard val-t output) e fresh
    | just g | false with refl ← e =
        ≼-refl , out1-⊑ st output (default-val val-t) fresh
step-extends {S = S} {st = st} (circuit-output vals) e _
  with collectOutputs (State.mem st) (IrSource.outputs S) vals
... | just vs with refl ← e = ≼-refl , ⊑-refl {State.mem st}

-- Operand resolution is monotone in the store.
resolve-mono : ∀ {m m′} op {v} → m ⊑ m′
  → resolve m op ≡ just v → resolve m′ op ≡ just v
resolve-mono (var id) sub r = sub r
resolve-mono (imm x)  sub r = r

-- The Native-resolution, boolean-reading, guard-evaluation, and
-- output-collection variants of `resolve-mono`.
resolveᶠ-mono : ∀ {m m′} op {x} → m ⊑ m′
  → resolveᶠ m op ≡ just x → resolveᶠ m′ op ≡ just x
resolveᶠ-mono {m} {m′} op sub r =
  resolveᶠ-intro m′ op (resolve-mono op sub (resolveᶠ-inv m op r))

resolve𝔹-mono : ∀ {m m′} op {b} → m ⊑ m′
  → resolve𝔹 m op ≡ just b → resolve𝔹 m′ op ≡ just b
resolve𝔹-mono {m} {m′} op sub e
  with resolveᶠ m op in eqx
... | just x rewrite resolveᶠ-mono {m} {m′} op sub eqx = e

eval-guard-mono : ∀ {m m′} g {b} → m ⊑ m′
  → eval-guard m g ≡ just b → eval-guard m′ g ≡ just b
eval-guard-mono nothing   sub e = e
eval-guard-mono (just op) sub e = resolve𝔹-mono op sub e

collectOutputs-mono : ∀ {m m′} tys ops {vs} → m ⊑ m′
  → collectOutputs m tys ops ≡ just vs
  → collectOutputs m′ tys ops ≡ just vs
collectOutputs-mono          []       []         sub e = e
collectOutputs-mono {m} {m′} (t ∷ ts) (op ∷ ops) sub e
  with resolve m op in eqv
... | just v rewrite resolve-mono op sub eqv
  with typeof v ≟T t
...   | yes _ with collectOutputs m ts ops in eqrest
...     | just rest
          rewrite collectOutputs-mono {m} {m′} ts ops sub eqrest = e
collectOutputs-mono          []       (_ ∷ _)    sub ()
collectOutputs-mono          (_ ∷ _)  []         sub ()

------------------------------------------------------------------------
-- Run-level single-assignment well-formedness, and run extension.
--
-- `WF-run P S is st` threads the per-step output freshness (`out-fresh`)
-- through the deterministic run from `st`.  Under it, a whole run only
-- *extends* the store and *appends* to the public inputs.
------------------------------------------------------------------------

WF-run : ProofPreimage → IrSource → List Instruction → State → Set
WF-run P S []       st = ⊤
WF-run P S (i ∷ is) st =
  out-fresh i (State.mem st)
  × (∀ {st′} → step P S st i ≡ just st′ → WF-run P S is st′)

-- Invert one step of a run: a non-empty run splits into its first step
-- and the run of the rest.
run-inv : ∀ {P S s} i is st
  → run P S st (i ∷ is) ≡ just s
  → ∃ λ st-mid → (step P S st i ≡ just st-mid) × (run P S st-mid is ≡ just s)
run-inv {P} {S} i is st run-eq with step P S st i
... | just st-mid = st-mid , refl , run-eq
... | nothing     = case run-eq of λ ()

run-extends : ∀ {P S s} is st
  → run P S st is ≡ just s
  → WF-run P S is st
  → (State.mem st ⊑ State.mem s) × (State.pis st ≼ State.pis s)
run-extends {s = s} [] st run-eq wf rewrite just-injective run-eq =
  ⊑-refl {State.mem s} , ≼-refl {State.pis s}
run-extends (i ∷ is) st run-eq (of , wf-rest) =
  let (st-mid , step-eq , run-rest) = run-inv i is st run-eq
      (p≼₁ , m⊑₁) = step-extends i step-eq of
      (m⊑ , p≼) = run-extends is st-mid run-rest (wf-rest step-eq)
  in  ⊑-trans {State.mem st} {State.mem st-mid} m⊑₁ m⊑
   ,  ≼-trans p≼₁ p≼

------------------------------------------------------------------------
-- Boolean reading of a field element.
------------------------------------------------------------------------

-- `to𝔹` reading `false` forces the field element to `0ᶠ`.
to𝔹-false : ∀ {gᶠ} → to𝔹 gᶠ ≡ just false → gᶠ ≡ 0ᶠ
to𝔹-false {gᶠ} e with gᶠ ≟ᶠ 0ᶠ
... | yes gᶠ≡0 = gᶠ≡0
... | no _ with gᶠ ≟ᶠ 1ᶠ
...   | yes _ = case e of λ ()
...   | no  _ = case e of λ ()

-- `to𝔹` reading `true` forces the field element to `1ᶠ`.
to𝔹-true : ∀ {gᶠ} → to𝔹 gᶠ ≡ just true → gᶠ ≡ 1ᶠ
to𝔹-true {gᶠ} e with gᶠ ≟ᶠ 0ᶠ
... | yes _ = case e of λ ()
... | no _ with gᶠ ≟ᶠ 1ᶠ
...   | yes gᶠ≡1 = gᶠ≡1
...   | no  _    = case e of λ ()

-- `to𝔹` / `isYes (_ ≟ᶠ 1ᶠ)` reading forced from the bit value.
to𝔹-0 : ∀ {x} → x ≡ 0ᶠ → to𝔹 x ≡ just false
to𝔹-0 x≡0 rewrite x≡0 with 0ᶠ ≟ᶠ 0ᶠ
... | yes _ = refl
... | no ¬p = case ¬p refl of λ ()

to𝔹-1 : ∀ {x} → x ≡ 1ᶠ → to𝔹 x ≡ just true
to𝔹-1 x≡1 rewrite x≡1 with 1ᶠ ≟ᶠ 0ᶠ
... | yes 1≡0 = case 1ᶠ≢0ᶠ 1≡0 of λ ()
... | no _ with 1ᶠ ≟ᶠ 1ᶠ
...   | yes _ = refl
...   | no ¬p = case ¬p refl of λ ()

isYes1-0 : ∀ {x} → x ≡ 0ᶠ → isYes (x ≟ᶠ 1ᶠ) ≡ false
isYes1-0 x≡0 rewrite x≡0 with 0ᶠ ≟ᶠ 1ᶠ
... | yes 0≡1 = case 1ᶠ≢0ᶠ (sym 0≡1) of λ ()
... | no _ = refl

isYes1-1 : ∀ {x} → x ≡ 1ᶠ → isYes (x ≟ᶠ 1ᶠ) ≡ true
isYes1-1 x≡1 rewrite x≡1 with 1ᶠ ≟ᶠ 1ᶠ
... | yes _ = refl
... | no ¬p = case ¬p refl of λ ()

------------------------------------------------------------------------
-- Miscellaneous run / initialisation facts.
------------------------------------------------------------------------

-- The number of resolved field elements equals the number of operands.
resolve-all-Fr-length : ∀ m ops {vals}
  → resolve-all-Fr m ops ≡ just vals → length vals ≡ length ops
resolve-all-Fr-length m []         refl = refl
resolve-all-Fr-length m (op ∷ ops) e
  with resolveᶠ m op | resolve-all-Fr m ops in eqs
... | just x | just xs with refl ← e =
        cong suc (resolve-all-Fr-length m ops eqs)

-- The init state's memory is exactly the decoding of the raw inputs.
init-decode : ∀ {S P st0}
  → init S P ≡ just st0
  → decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
      ≡ just (State.mem st0)
init-decode {S} {P} {st0} ieq
  with decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
... | just m
  with IrSource.do-communications-commitment S
...   | false = cong (λ z → just (State.mem z)) (just-injective ieq)
init-decode {S} {P} {st0} ieq | just m | true
  with ProofPreimage.comm-commitment P
...     | just (c , _) = cong (λ z → just (State.mem z)) (just-injective ieq)

-- `init` always seeds an empty output stream.
init-outs : ∀ {S P st0} → init S P ≡ just st0 → State.outs st0 ≡ []
init-outs {S} {P} {st0} ieq
  with decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
... | just m
  with IrSource.do-communications-commitment S
...   | false = sym (cong State.outs (just-injective ieq))
init-outs {S} {P} {st0} ieq | just m | true
  with ProofPreimage.comm-commitment P
...     | just (c , _) = sym (cong State.outs (just-injective ieq))

------------------------------------------------------------------------
-- The user-facing run shape.
--
-- `Consumed` records the terminal transcript-exhaustion side conditions;
-- `run-shaped` packages an initial state and a run over the source's
-- instructions ending EXACTLY at `s`, with the transcripts consumed.
------------------------------------------------------------------------

Consumed : ProofPreimage → State → Set
Consumed P s =
    (State.pti-idx s ≡ length (ProofPreimage.pub-transcript-inputs P))
  × (State.pto-rem s ≡ [])
  × (State.priv-rem s ≡ [])

record run-shaped (S : IrSource) (P : ProofPreimage) (s : State) : Set where
  constructor mk-run-shaped
  field
    start    : State
    init≡    : init S P ≡ just start
    walk     : run P S start (IrSource.instructions S) ≡ just s
    consumed : Consumed P s

-- Chaining one reconstructed step onto a tail run: the cons rule of `run`
-- read as a builder.
run-cons : ∀ {P S st st' s} i is
  → step P S st i ≡ just st'
  → run P S st' is ≡ just s
  → run P S st (i ∷ is) ≡ just s
run-cons {P} {S} {st} i is step-eq run-rest rewrite step-eq = run-rest

------------------------------------------------------------------------
-- `run-shaped` from a successful preprocess.
------------------------------------------------------------------------

-- Invert a successful `preprocess`: its inner `run` reaches `s` from the
-- initial state, and the terminal transcript-exhaustion side conditions
-- hold (the terminal guards only pass `stf` through).  A single walk of
-- the 7-deep guard tower yields both facts.
preprocess-walk-consumed : ∀ {S P s st0}
  → init S P ≡ just st0 → preprocess S P ≡ just s
  → run P S st0 (IrSource.instructions S) ≡ just s × Consumed P s
preprocess-walk-consumed {S} {P} {s} {st0} ieq peq
  with init S P | ieq
... | just st0' | refl
  with run P S st0' (IrSource.instructions S)
...   | just stf
  with State.pti-idx stf ≟ℕ length (ProofPreimage.pub-transcript-inputs P)
...     | yes p
  with State.pto-rem stf in ptoeq
...       | []
  with State.priv-rem stf in preq
...         | []
  with IrSource.do-communications-commitment S
...           | false with refl ← peq = refl , p , ptoeq , preq
preprocess-walk-consumed {S} {P} {s} {st0} ieq peq
  | just st0' | refl | just stf | yes p | [] | [] | true
  with ProofPreimage.comm-commitment P
...             | just (c , r)
  with c ≟ᶠ transient-commit
              (ProofPreimage.inputs P ++ concatMap encodeᵉ (State.outs stf)) r
...               | yes _ with refl ← peq = refl , p , ptoeq , preq

-- `run-shaped` follows directly from a successful `init` + `preprocess`.
-- This is the target the witness→run construction reduces to: build `P'`
-- with a successful `preprocess`, and `run-shaped` is immediate.
preprocess→run-shaped : ∀ {S P s st0}
  → init S P ≡ just st0 → preprocess S P ≡ just s → run-shaped S P s
preprocess→run-shaped {S} {P} {s} {st0} ieq peq =
  let (walk , consumed) = preprocess-walk-consumed {S} {P} {s} {st0} ieq peq
  in mk-run-shaped st0 ieq walk consumed
