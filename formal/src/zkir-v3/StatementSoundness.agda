{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Constructive (pis-form) statement soundness for zkir-v3.
--
-- The backward faithfulness result `backward` (in `CircuitProof`) is the
-- converse of `forward` for a GIVEN preprocess run and a GIVEN backward
-- spine.  Statement soundness strengthens this: from an ARBITRARY
-- satisfying witness `w` of `synth S` (with a well-typed producer and a
-- minimal witness-shape side condition on the free public/private-input
-- cells), one CONSTRUCTS a preimage `P'` and a final state `s` with
-- `run-shaped S P' s` and three agreements against `w`, all packaged in a
-- `SubRealizer S w` record:
--   • `pis (witness-of P' s) ≡ pis w` — the pis-form, not the extraction
--     form `witness-of P' s ≡ w`, which is false for the unconstrained
--     public/private-input cells and guards;
--   • `mem s ⊑ᵂ w` — the run's entire final memory is a sub-assignment of
--     `w` (the honest v3 analogue of the extraction form, restricted to
--     the run's domain);
--   • when `do-comm S ≡ true`, `comm-rand (witness-of P' s) ≡ comm-rand w`;
-- together with `CommWF`, which forces an absent commitment when the flag
-- is off (`do-comm S ≡ false → comm-commitment P' ≡ nothing`).
--
-- The construction:
--
--   • It rests on `preprocess→run-shaped` (SemanticsProperties):
--     `run-shaped` is exactly a successful `init` + `preprocess` (the walk
--     is the `run` inside `preprocess`, and the terminal `Consumed` side
--     conditions, both read off `preprocess-walk-consumed`).  So the
--     construction reduces to building `P'` with `preprocess S P' ≡ just s`
--     and `pis s ≡ pis w`.
--
--   • The memory-agreement invariant `m ⊑ᵂ w` (the run memory is a
--     sub-assignment of the witness) and its resolution-transport lemmas.
--     For every constraint-pinned instruction the witness's output cell is
--     exactly what `step` computes (`agree-mul` is the representative), so
--     the invariant is preserved as the run binds outputs; and on an
--     active `impact` group the value the run appends to `pis` equals the
--     witness's pinned pi entry (`impact-slice`) — the per-entry basis of
--     the pis-form conclusion, driven entirely by `satisfies w`.
--
--   • The w-only pre-passes (`mkInputs`/`mkPTI`/`mkPTO`/`mkPRV`), the
--     34-clause `build` run-realizer, the branch lemmas
--     `statement-sound-false`/`-true`, and the combined `statement-sound`.
------------------------------------------------------------------------

module zkir-v3.StatementSoundness (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Semantics ⋯
open import zkir-v3.SemanticsProperties ⋯
open import zkir-v3.Circuit ⋯
open import zkir-v3.CircuitBridge ⋯
open import zkir-v3.CircuitFaithfulness ⋯
open import zkir-v3.Obligations ⋯
open import zkir-v3.CircuitProof ⋯
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)
open import zkir-v3.Semantics ⋯ using () renaming (χ to χˢ; pow2ᶠ to pow2ᶠˢ)
open import zkir-v3.Circuit ⋯ using () renaming (χ to χᶜ)

open import Data.Bool    using (Bool; true; false; if_then_else_)
                         renaming (not to bnot)
open import Data.Bool.Properties using () renaming (_≟_ to _≟𝔹_)
open import Data.List    using (List; []; _∷_; _++_; take; drop; length; map;
                                concatMap)
open import Data.Maybe   using (Maybe; just; nothing; _>>=_; maybe′)
open import Data.Vec     using (reverse)
open import Data.Sum     using (_⊎_; inj₁; inj₂)
open import Data.Product using (_×_; _,_; ∃; ∃-syntax; uncurry; proj₁; proj₂)
open import Data.Unit    using (⊤; tt)
open import Data.String  using () renaming (_≟_ to _≟str_)
open import Data.List.Properties using (drop-drop; length-++; length-map;
                                        take-all; ++-assoc)
open import Data.Nat.Properties using (+-identityʳ; +-assoc; ≤-reflexive)
open import Data.List.Relation.Unary.All using (All; []; _∷_)
open import Data.List.Membership.Propositional using (_∈_)
open import Relation.Binary.PropositionalEquality
open import Relation.Binary.Definitions using (DecidableEquality)
open import Relation.Nullary using (¬_; yes; no; Dec)
open import Relation.Nullary.Decidable using (isYes; map′; _×-dec_)
open import Data.Maybe.Properties using (just-injective; ≡-dec)
open import Function using (case_of_)
open import Data.Nat using (ℕ; zero; suc; _+_; _*_; _^_; _∸_; _<_; _<?_)
                     renaming (_≟_ to _≟ℕ_)

------------------------------------------------------------------------
-- The memory-agreement invariant and its resolution transport.
------------------------------------------------------------------------

-- The run's memory is a sub-assignment of the given witness `w`.
_⊑ᵂ_ : Mem → CircuitWitness → Set
m ⊑ᵂ w = m ⊑ CircuitWitness.assign w

-- Transport an off-circuit resolution up to the witness under agreement.
⊑-resolve : ∀ {m w} op {v}
  → m ⊑ᵂ w → resolve m op ≡ just v → resolveᶜ w op ≡ just v
⊑-resolve (imm x)  sub e = e
⊑-resolve (var id) sub e = sub e

⊑-resolveᶠ : ∀ {m w} op {x}
  → m ⊑ᵂ w → resolveᶠ m op ≡ just x → resolveᶜ-Fr w op ≡ just x
⊑-resolveᶠ (imm x)  sub e = e
⊑-resolveᶠ {m} (var id) sub e with m id in re
... | just (val-native v) rewrite sub {id} {val-native v} re = e

-- The list companion of `⊑-resolve`, for the all-native input lists of
-- `hash-to-curve`/`transient-hash`/`impact`.
⊑-resolve-all : ∀ {m w} ops {frs}
  → m ⊑ᵂ w → resolve-all-Fr m ops ≡ just frs
  → resolveᶜ-all-Fr w ops ≡ just frs
⊑-resolve-all []       sub refl = refl
⊑-resolve-all {m} {w} (op ∷ ops) sub e
  with resolveᶠ m op in eo | resolve-all-Fr m ops in eos | e
... | just x | just xs | refl
  with resolveᶜ-Fr w op | ⊑-resolveᶠ {w = w} op sub eo
...   | .(just x) | refl
  with resolveᶜ-all-Fr w ops | ⊑-resolve-all {w = w} ops sub eos
...     | .(just xs) | refl = refl

------------------------------------------------------------------------
-- Agreement is preserved: the witness's output cell is exactly what
-- `step` computes.  `agree-mul` is the representative for the whole
-- constraint-pinned family (add/copy/neg/inv/ec-*/hash/bytes/coords/
-- div-mod/less-than/…): the gate fixes the output from operands the
-- invariant already agrees on, via `⊑-resolveᶠ`/`⊑-resolve`.
------------------------------------------------------------------------

agree-mul : ∀ {m w a b out x y}
  → m ⊑ᵂ w
  → resolveᶠ m a ≡ just x → resolveᶠ m b ≡ just y
  → holds w (gate-mul out a b)
  → CircuitWitness.assign w out ≡ just (val-native (x *ᶠ y))
agree-mul {w = w} {a = a} {b} sub ra rb
  (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (resolveᶜ-Fr-inv w a (⊑-resolveᶠ a sub ra))) wa
     | trans (sym (resolveᶜ-Fr-inv w b (⊑-resolveᶠ b sub rb))) wb
... | refl | refl with disj
...   | inj₁ (x' , y' , refl , refl , refl) = wout
...   | inj₂ (inj₁ (_ , _ , () , _))
...   | inj₂ (inj₂ (_ , _ , () , _))

-- The rest of the total-value family (output value = a total function of
-- the resolved operands).  Each lifts the run's resolutions to `w` with
-- `⊑-resolve*`, unifies them with the witness's pinned operands (`refl`),
-- and returns the witness's output cell (bridging `χˢ`/`χ` where the
-- value is a boolean reading, since `step` uses the Semantics copy and
-- `holds` the Circuit copy).

agree-add-n : ∀ {m w a b out x y}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-native x) → resolve m b ≡ just (val-native y)
  → holds w (gate-add out a b)
  → CircuitWitness.assign w out ≡ just (val-native (x +ᶠ y))
agree-add-n {a = a} {b} sub ra rb (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with disj
...   | inj₁ (x , y , refl , refl , refl) = wout
...   | inj₂ (inj₁ (p , q , () , _))
...   | inj₂ (inj₂ (inj₁ (p , q , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , y , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , y , () , _))))

agree-add-p : ∀ {m w a b out p q}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-jubjub-point p)
  → resolve m b ≡ just (val-jubjub-point q)
  → holds w (gate-add out a b)
  → CircuitWitness.assign w out ≡ just (val-jubjub-point (p +J q))
agree-add-p {a = a} {b} sub ra rb (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with disj
...   | inj₁ (x , y , () , _)
...   | inj₂ (inj₁ (p , q , refl , refl , refl)) = wout
...   | inj₂ (inj₂ (inj₁ (p , q , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , y , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , y , () , _))))

agree-neg-n : ∀ {m w a out x}
  → m ⊑ᵂ w → resolve m a ≡ just (val-native x)
  → holds w (gate-neg out a)
  → CircuitWitness.assign w out ≡ just (val-native (-ᶠ x))
agree-neg-n {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (x , refl , refl) = wout
...   | inj₂ (inj₁ (p , () , _))
...   | inj₂ (inj₂ (inj₁ (p , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , () , _))))

agree-neg-p : ∀ {m w a out p}
  → m ⊑ᵂ w → resolve m a ≡ just (val-jubjub-point p)
  → holds w (gate-neg out a)
  → CircuitWitness.assign w out ≡ just (val-jubjub-point (negJ p))
agree-neg-p {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (x , () , _)
...   | inj₂ (inj₁ (p , refl , refl)) = wout
...   | inj₂ (inj₂ (inj₁ (p , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , () , _))))

agree-copy : ∀ {m w a out v}
  → m ⊑ᵂ w → resolve m a ≡ just v
  → holds w (gate-copy out a)
  → CircuitWitness.assign w out ≡ just v
agree-copy {a = a} sub ra (av , wa , wout)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl = wout

agree-test-eq : ∀ {m w a b out av bv e}
  → m ⊑ᵂ w → resolve m a ≡ just av → resolve m b ≡ just bv
  → valEq? av bv ≡ just e
  → holds w (test-eq out a b)
  → CircuitWitness.assign w out ≡ just (val-native (χˢ e))
agree-test-eq {a = a} {b} {e = e} sub ra rb ve
  (av , bv , eqb , wa , wb , wveq , wout)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with trans (sym ve) wveq
...   | refl =
  wout

agree-scalar-from-native : ∀ {m w a out x}
  → m ⊑ᵂ w → resolveᶠ m a ≡ just x
  → holds w (scalar-from-native out a)
  → CircuitWitness.assign w out
      ≡ just (val-jubjub-scalar (native→jubjubScalar x))
agree-scalar-from-native {a = a} sub ra (x , wa , wout)
  with trans (sym (⊑-resolveᶠ a sub ra)) wa
... | refl = wout

agree-ec-mul : ∀ {m w a scalar out p s}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-jubjub-point p)
  → resolve m scalar ≡ just (val-jubjub-scalar s)
  → holds w (ec-mul out a scalar)
  → CircuitWitness.assign w out ≡ just (val-jubjub-point (s ·J p))
agree-ec-mul {a = a} {scalar} sub ra rs (inj₁ (p , s , wa , ws , wout))
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve scalar sub rs)) ws
... | refl | refl = wout
agree-ec-mul {a = a} sub ra rs (inj₂ (p , s , wa , ws , wout))
  with trans (sym (⊑-resolve a sub ra)) wa
... | ()

agree-ec-gen : ∀ {m w scalar out s}
  → m ⊑ᵂ w → resolve m scalar ≡ just (val-jubjub-scalar s)
  → holds w (ec-gen out scalar)
  → CircuitWitness.assign w out ≡ just (val-jubjub-point (s ·J genJ))
agree-ec-gen {scalar = scalar} sub rs (inj₁ (s , ws , wout))
  with trans (sym (⊑-resolve scalar sub rs)) ws
... | refl = wout
agree-ec-gen {scalar = scalar} sub rs (inj₂ (s , ws , wout))
  with trans (sym (⊑-resolve scalar sub rs)) ws
... | ()

agree-h2c : ∀ {m w inputs out frs}
  → m ⊑ᵂ w → resolve-all-Fr m inputs ≡ just frs
  → holds w (h2c out inputs)
  → CircuitWitness.assign w out
      ≡ just (val-jubjub-point (hash-to-curve-fn frs))
agree-h2c {inputs = inputs} sub rf (frs , wf , wout)
  with trans (sym (⊑-resolve-all inputs sub rf)) wf
... | refl = wout

agree-transient-hash : ∀ {m w inputs out frs}
  → m ⊑ᵂ w → resolve-all-Fr m inputs ≡ just frs
  → holds w (poseidon out inputs)
  → CircuitWitness.assign w out ≡ just (val-native (transient-hash-fn frs))
agree-transient-hash {inputs = inputs} sub rf (frs , wf , wout)
  with trans (sym (⊑-resolve-all inputs sub rf)) wf
... | refl = wout

agree-into-bytes : ∀ {m w a out x}
  → m ⊑ᵂ w → resolveᶠ m a ≡ just x
  → holds w (into-bytes out a)
  → CircuitWitness.assign w out ≡ just (val-bytes32 (nativeToBytes x))
agree-into-bytes {w = w} {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (resolveᶜ-Fr-inv w a (⊑-resolveᶠ a sub ra))) wa
... | refl with disj
...   | inj₁ (x , refl , refl) = wout
...   | inj₂ (inj₁ (_ , () , _))
...   | inj₂ (inj₂ (_ , () , _))

agree-less-than : ∀ {m w a b bits out x y}
  → m ⊑ᵂ w → resolveᶠ m a ≡ just x → resolveᶠ m b ≡ just y
  → holds w (less-than out a b bits)
  → CircuitWitness.assign w out
      ≡ just (val-native (χˢ (isYes (valFr x <? valFr y))))
agree-less-than {a = a} {b} sub ra rb (x , y , wa , wb , bx , by , wout)
  with trans (sym (⊑-resolveᶠ a sub ra)) wa
     | trans (sym (⊑-resolveᶠ b sub rb)) wb
... | refl | refl =
  wout

-- Partial-op / two-output instructions.  The output value depends on a
-- partial operation or the instruction binds two cells, so `holds`
-- carries the output cell(s) as witness existentials; each lemma unifies
-- the run's operands with the witness's and returns the semantic success
-- fact(s) (which feed the matching `*-step`) bundled with the output
-- cell(s) (which feed `ins-⊑ᵂ`).

agree-inv : ∀ {m w a out x}
  → m ⊑ᵂ w → resolveᶠ m a ≡ just x
  → holds w (gate-inv out a)
  → ∃ λ xi → invᶠ x ≡ just xi
           × CircuitWitness.assign w out ≡ just (val-native xi)
agree-inv {w = w} {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (resolveᶜ-Fr-inv w a (⊑-resolveᶠ a sub ra))) wa
... | refl with disj
...   | inj₁ (_ , xi , refl , invEq , refl) = xi , invEq , wout
...   | inj₂ (inj₁ (_ , _ , () , _))
...   | inj₂ (inj₂ (_ , _ , () , _))

-- `from-bytes`'s constraint is a 3-way disjunction over the output value; it
-- does NOT record the instruction's `val-t`.  The declared output cell value
-- `v` (from `WCell`, with `typeof v ≡ val-t`) selects the matching disjunct:
-- the two off-type disjuncts pin `v` to the other constructor, contradicting
-- `typeof v ≡ val-t`.
agree-from-bytes-native : ∀ {m w b out bs v}
  → m ⊑ᵂ w → resolve m b ≡ just (val-bytes32 bs)
  → holds w (from-bytes out b)
  → CircuitWitness.assign w out ≡ just v → typeof v ≡ native
  → CircuitWitness.assign w out ≡ just (val-native (nativeFromBytes bs))
agree-from-bytes-native {b = b} sub rb (bs , wb , disj) wo tv
  with trans (sym (⊑-resolve b sub rb)) wb
... | refl with disj
...   | inj₁ e = e
...   | inj₂ (inj₁ e) with just-injective (trans (sym wo) e)
...     | refl = case tv of λ ()
agree-from-bytes-native {b = b} sub rb (bs , wb , disj) wo tv
  | refl | inj₂ (inj₂ e) with just-injective (trans (sym wo) e)
...   | refl = case tv of λ ()

agree-from-bytes-secp-base : ∀ {m w b out bs v}
  → m ⊑ᵂ w → resolve m b ≡ just (val-bytes32 bs)
  → holds w (from-bytes out b)
  → CircuitWitness.assign w out ≡ just v → typeof v ≡ secp-base
  → CircuitWitness.assign w out ≡ just (val-secp-base (secpBaseFromBytes bs))
agree-from-bytes-secp-base {b = b} sub rb (bs , wb , disj) wo tv
  with trans (sym (⊑-resolve b sub rb)) wb
... | refl with disj
...   | inj₂ (inj₁ e) = e
...   | inj₁ e with just-injective (trans (sym wo) e)
...     | refl = case tv of λ ()
agree-from-bytes-secp-base {b = b} sub rb (bs , wb , disj) wo tv
  | refl | inj₂ (inj₂ e) with just-injective (trans (sym wo) e)
...   | refl = case tv of λ ()

agree-from-bytes-secp-scalar : ∀ {m w b out bs v}
  → m ⊑ᵂ w → resolve m b ≡ just (val-bytes32 bs)
  → holds w (from-bytes out b)
  → CircuitWitness.assign w out ≡ just v → typeof v ≡ secp-scalar
  → CircuitWitness.assign w out ≡ just (val-secp-scalar (secpScalarFromBytes bs))
agree-from-bytes-secp-scalar {b = b} sub rb (bs , wb , disj) wo tv
  with trans (sym (⊑-resolve b sub rb)) wb
... | refl with disj
...   | inj₂ (inj₂ e) = e
...   | inj₁ e with just-injective (trans (sym wo) e)
...     | refl = case tv of λ ()
agree-from-bytes-secp-scalar {b = b} sub rb (bs , wb , disj) wo tv
  | refl | inj₂ (inj₁ e) with just-injective (trans (sym wo) e)
...   | refl = case tv of λ ()

-- The `from-bytes` constraint pins the output cell to one of the three
-- supported value constructors; so the value `w` binds there has a
-- supported type.  Used to refute the unsupported declared `val-t`.
from-bytes-out-typeof : ∀ {w o bytes v}
  → holds w (from-bytes o bytes)
  → CircuitWitness.assign w o ≡ just v
  → (typeof v ≡ native) ⊎ (typeof v ≡ secp-base) ⊎ (typeof v ≡ secp-scalar)
from-bytes-out-typeof (bs , wb , inj₁ e)        wo =
  inj₁ (cong typeof (just-injective (trans (sym wo) e)))
from-bytes-out-typeof (bs , wb , inj₂ (inj₁ e)) wo =
  inj₂ (inj₁ (cong typeof (just-injective (trans (sym wo) e))))
from-bytes-out-typeof (bs , wb , inj₂ (inj₂ e)) wo =
  inj₂ (inj₂ (cong typeof (just-injective (trans (sym wo) e))))

agree-reverse-bytes : ∀ {m w b out bs}
  → m ⊑ᵂ w → resolve m b ≡ just (val-bytes32 bs)
  → holds w (reverse-bytes out b)
  → CircuitWitness.assign w out ≡ just (val-bytes32 (reverse bs))
agree-reverse-bytes {b = b} sub rb (bs , wb , wout)
  with trans (sym (⊑-resolve b sub rb)) wb
... | refl = wout

agree-from-coords : ∀ {m w xop yop out xv yv}
  → m ⊑ᵂ w → resolveᶠ m xop ≡ just xv → resolveᶠ m yop ≡ just yv
  → holds w (from-coords out xop yop)
  → ∃ λ p → fromCoordsJ xv yv ≡ just p
          × CircuitWitness.assign w out ≡ just (val-jubjub-point p)
agree-from-coords {xop = xop} {yop} sub rx ry
  (inj₁ (xv , yv , p , wx , wy , fcEq , wout))
  with trans (sym (⊑-resolveᶠ xop sub rx)) wx
     | trans (sym (⊑-resolveᶠ yop sub ry)) wy
... | refl | refl = p , fcEq , wout
agree-from-coords {w = w} {xop = xop} sub rx ry
  (inj₂ (xv , yv , p , wx , wy , fcEq , wout))
  with trans (sym (resolveᶜ-Fr-inv w xop (⊑-resolveᶠ xop sub rx))) wx
... | ()

agree-bytes-from-low-high : ∀ {m w lop hip out l h}
  → m ⊑ᵂ w → resolveᶠ m lop ≡ just l → resolveᶠ m hip ≡ just h
  → holds w (bytes-from-low-high out lop hip)
  → ∃ λ bs → low-high→bytes32 l h ≡ just bs
           × CircuitWitness.assign w out ≡ just (val-bytes32 bs)
agree-bytes-from-low-high {lop = lop} {hip} sub rl rh
  (l , h , bs , wlo , whi , lhEq , wout)
  with trans (sym (⊑-resolveᶠ lop sub rl)) wlo
     | trans (sym (⊑-resolveᶠ hip sub rh)) whi
... | refl | refl = bs , lhEq , wout

agree-reconstitute : ∀ {m w divisor modulus bits out d mo}
  → m ⊑ᵂ w → resolveᶠ m divisor ≡ just d → resolveᶠ m modulus ≡ just mo
  → holds w (reconstitute out divisor modulus bits)
  → (valFr mo < 2 ^ bits) × (valFr d < 2 ^ (FR-BITS ∸ bits))
  × CircuitWitness.assign w out
      ≡ just (val-native ((pow2ᶠˢ bits *ᶠ d) +ᶠ mo))
agree-reconstitute {divisor = divisor} {modulus} {bits = bits}
  sub rd rmo (dv , mv , wd , wm , bd , bm , wout)
  with trans (sym (⊑-resolveᶠ divisor sub rd)) wd
     | trans (sym (⊑-resolveᶠ modulus sub rmo)) wm
... | refl | refl =
  bm , bd , wout

agree-into-coords : ∀ {m w point xo yo p}
  → m ⊑ᵂ w → resolve m point ≡ just (val-jubjub-point p)
  → holds w (into-coords xo yo point)
  → ∃ λ x → ∃ λ y → coordsJ p ≡ (x , y)
          × CircuitWitness.assign w xo ≡ just (val-native x)
          × CircuitWitness.assign w yo ≡ just (val-native y)
agree-into-coords {point = point} sub rp
  (inj₁ (p , x , y , wp , coordsEq , wxo , wyo))
  with trans (sym (⊑-resolve point sub rp)) wp
... | refl = x , y , coordsEq , wxo , wyo
agree-into-coords {point = point} sub rp
  (inj₂ (p , x , y , wp , coordsEq , wxo , wyo))
  with trans (sym (⊑-resolve point sub rp)) wp
... | ()

agree-bytes-into-low-high : ∀ {m w bytes lo hi bs}
  → m ⊑ᵂ w → resolve m bytes ≡ just (val-bytes32 bs)
  → holds w (bytes-into-low-high lo hi bytes)
  → ∃ λ l → ∃ λ h → bytes32→low-high bs ≡ (l , h)
          × CircuitWitness.assign w lo ≡ just (val-native l)
          × CircuitWitness.assign w hi ≡ just (val-native h)
agree-bytes-into-low-high {bytes = bytes} sub rb
  (bs , l , h , wb , splitEq , wlo , whi)
  with trans (sym (⊑-resolve bytes sub rb)) wb
... | refl = l , h , splitEq , wlo , whi

agree-div-mod : ∀ {m w q r val bits x}
  → m ⊑ᵂ w → resolveᶠ m val ≡ just x
  → holds w (div-mod q r val bits)
  → CircuitWitness.assign w q
      ≡ just (val-native (from-le-bits (drop bits (to-le-bits x))))
  × CircuitWitness.assign w r
      ≡ just (val-native (from-le-bits (take bits (to-le-bits x))))
agree-div-mod {val = val} sub rv (x , wv , wq , wr)
  with trans (sym (⊑-resolveᶠ val sub rv)) wv
... | refl = wq , wr

agree-persistent-hash : ∀ {m w out al inputs frs}
  → m ⊑ᵂ w → resolve-all-Fr m inputs ≡ just frs
  → holds w (sha256 out al inputs)
  → ∃ λ v → persistent-hash-fn al frs ≡ just v
          × CircuitWitness.assign w out ≡ just (val-bytes32 v)
agree-persistent-hash {inputs = inputs} sub rf
  (frs , v , rall , phEq , wout)
  with trans (sym (⊑-resolve-all inputs sub rf)) rall
... | refl = v , phEq , wout

agree-keccak : ∀ {m w out al inputs frs}
  → m ⊑ᵂ w → resolve-all-Fr m inputs ≡ just frs
  → holds w (keccak out al inputs)
  → ∃ λ v → keccak-fn al frs ≡ just v
          × CircuitWitness.assign w out ≡ just (val-bytes32 v)
agree-keccak {inputs = inputs} sub rf
  (frs , v , rall , kfEq , wout)
  with trans (sym (⊑-resolve-all inputs sub rf)) rall
... | refl = v , kfEq , wout

------------------------------------------------------------------------
-- The Secp256k1 companions of the total/partial-value agreements.  Each
-- selects the constructor-matching `holds` disjunct (the off-type arms
-- pin `av`/`ov` to the wrong constructor, killed by the resolution
-- equalities); the top-level ⊎ instructions (ec-mul/ec-gen/coords) cross
-- to the other arm via `⊑-resolve`/`resolveᶜ-Fr-inv`.
------------------------------------------------------------------------

agree-add-sp : ∀ {m w a b out p q}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-secp-point p)
  → resolve m b ≡ just (val-secp-point q)
  → holds w (gate-add out a b)
  → CircuitWitness.assign w out ≡ just (val-secp-point (p +K q))
agree-add-sp {a = a} {b} sub ra rb (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with disj
...   | inj₁ (x , y , () , _)
...   | inj₂ (inj₁ (p , q , () , _))
...   | inj₂ (inj₂ (inj₁ (p , q , refl , refl , refl))) = wout
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , y , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , y , () , _))))

agree-add-sb : ∀ {m w a b out x y}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-secp-base x)
  → resolve m b ≡ just (val-secp-base y)
  → holds w (gate-add out a b)
  → CircuitWitness.assign w out ≡ just (val-secp-base (x +ᵇ y))
agree-add-sb {a = a} {b} sub ra rb (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with disj
...   | inj₁ (x , y , () , _)
...   | inj₂ (inj₁ (p , q , () , _))
...   | inj₂ (inj₂ (inj₁ (p , q , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , y , refl , refl , refl)))) = wout
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , y , () , _))))

agree-add-ss : ∀ {m w a b out x y}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-secp-scalar x)
  → resolve m b ≡ just (val-secp-scalar y)
  → holds w (gate-add out a b)
  → CircuitWitness.assign w out ≡ just (val-secp-scalar (x +ˢ y))
agree-add-ss {a = a} {b} sub ra rb (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with disj
...   | inj₁ (x , y , () , _)
...   | inj₂ (inj₁ (p , q , () , _))
...   | inj₂ (inj₂ (inj₁ (p , q , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , y , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , y , refl , refl , refl)))) = wout

agree-mul-sb : ∀ {m w a b out x y}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-secp-base x)
  → resolve m b ≡ just (val-secp-base y)
  → holds w (gate-mul out a b)
  → CircuitWitness.assign w out ≡ just (val-secp-base (x *ᵇ y))
agree-mul-sb {a = a} {b} sub ra rb (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with disj
...   | inj₁ (x , y , () , _)
...   | inj₂ (inj₁ (x , y , refl , refl , refl)) = wout
...   | inj₂ (inj₂ (x , y , () , _))

agree-mul-ss : ∀ {m w a b out x y}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-secp-scalar x)
  → resolve m b ≡ just (val-secp-scalar y)
  → holds w (gate-mul out a b)
  → CircuitWitness.assign w out ≡ just (val-secp-scalar (x *ˢ y))
agree-mul-ss {a = a} {b} sub ra rb (av , bv , ov , wa , wb , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve b sub rb)) wb
... | refl | refl with disj
...   | inj₁ (x , y , () , _)
...   | inj₂ (inj₁ (x , y , () , _))
...   | inj₂ (inj₂ (x , y , refl , refl , refl)) = wout

agree-neg-sp : ∀ {m w a out p}
  → m ⊑ᵂ w → resolve m a ≡ just (val-secp-point p)
  → holds w (gate-neg out a)
  → CircuitWitness.assign w out ≡ just (val-secp-point (negK p))
agree-neg-sp {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (x , () , _)
...   | inj₂ (inj₁ (p , () , _))
...   | inj₂ (inj₂ (inj₁ (p , refl , refl))) = wout
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , () , _))))

agree-neg-sb : ∀ {m w a out x}
  → m ⊑ᵂ w → resolve m a ≡ just (val-secp-base x)
  → holds w (gate-neg out a)
  → CircuitWitness.assign w out ≡ just (val-secp-base (-ᵇ x))
agree-neg-sb {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (x , () , _)
...   | inj₂ (inj₁ (p , () , _))
...   | inj₂ (inj₂ (inj₁ (p , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , refl , refl)))) = wout
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , () , _))))

agree-neg-ss : ∀ {m w a out x}
  → m ⊑ᵂ w → resolve m a ≡ just (val-secp-scalar x)
  → holds w (gate-neg out a)
  → CircuitWitness.assign w out ≡ just (val-secp-scalar (-ˢ x))
agree-neg-ss {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (x , () , _)
...   | inj₂ (inj₁ (p , () , _))
...   | inj₂ (inj₂ (inj₁ (p , () , _)))
...   | inj₂ (inj₂ (inj₂ (inj₁ (x , () , _))))
...   | inj₂ (inj₂ (inj₂ (inj₂ (x , refl , refl)))) = wout

agree-inv-sb : ∀ {m w a out x}
  → m ⊑ᵂ w → resolve m a ≡ just (val-secp-base x)
  → holds w (gate-inv out a)
  → ∃ λ xi → invᵇ x ≡ just xi
           × CircuitWitness.assign w out ≡ just (val-secp-base xi)
agree-inv-sb {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (_ , _ , () , _)
...   | inj₂ (inj₁ (_ , xi , refl , invEq , refl)) = xi , invEq , wout
...   | inj₂ (inj₂ (_ , _ , () , _))

agree-inv-ss : ∀ {m w a out x}
  → m ⊑ᵂ w → resolve m a ≡ just (val-secp-scalar x)
  → holds w (gate-inv out a)
  → ∃ λ xi → invˢ x ≡ just xi
           × CircuitWitness.assign w out ≡ just (val-secp-scalar xi)
agree-inv-ss {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (_ , _ , () , _)
...   | inj₂ (inj₁ (_ , _ , () , _))
...   | inj₂ (inj₂ (_ , xi , refl , invEq , refl)) = xi , invEq , wout

agree-ec-mul-secp : ∀ {m w a scalar out p s}
  → m ⊑ᵂ w
  → resolve m a ≡ just (val-secp-point p)
  → resolve m scalar ≡ just (val-secp-scalar s)
  → holds w (ec-mul out a scalar)
  → CircuitWitness.assign w out ≡ just (val-secp-point (s ·K p))
agree-ec-mul-secp {a = a} {scalar} sub ra rs (inj₂ (p , s , wa , ws , wout))
  with trans (sym (⊑-resolve a sub ra)) wa
     | trans (sym (⊑-resolve scalar sub rs)) ws
... | refl | refl = wout
agree-ec-mul-secp {a = a} sub ra rs (inj₁ (p , s , wa , ws , wout))
  with trans (sym (⊑-resolve a sub ra)) wa
... | ()

agree-ec-gen-secp : ∀ {m w scalar out s}
  → m ⊑ᵂ w → resolve m scalar ≡ just (val-secp-scalar s)
  → holds w (ec-gen out scalar)
  → CircuitWitness.assign w out ≡ just (val-secp-point (s ·K genK))
agree-ec-gen-secp {scalar = scalar} sub rs (inj₂ (s , ws , wout))
  with trans (sym (⊑-resolve scalar sub rs)) ws
... | refl = wout
agree-ec-gen-secp {scalar = scalar} sub rs (inj₁ (s , ws , wout))
  with trans (sym (⊑-resolve scalar sub rs)) ws
... | ()

agree-into-coords-secp : ∀ {m w point xo yo p}
  → m ⊑ᵂ w → resolve m point ≡ just (val-secp-point p)
  → holds w (into-coords xo yo point)
  → ∃ λ x → ∃ λ y → coordsK p ≡ just (x , y)
          × CircuitWitness.assign w xo ≡ just (val-secp-base x)
          × CircuitWitness.assign w yo ≡ just (val-secp-base y)
agree-into-coords-secp {point = point} sub rp
  (inj₂ (p , x , y , wp , coordsEq , wxo , wyo))
  with trans (sym (⊑-resolve point sub rp)) wp
... | refl = x , y , coordsEq , wxo , wyo
agree-into-coords-secp {point = point} sub rp
  (inj₁ (p , x , y , wp , coordsEq , wxo , wyo))
  with trans (sym (⊑-resolve point sub rp)) wp
... | ()

agree-from-coords-secp : ∀ {m w xop yop out xv yv}
  → m ⊑ᵂ w
  → resolve m xop ≡ just (val-secp-base xv)
  → resolve m yop ≡ just (val-secp-base yv)
  → holds w (from-coords out xop yop)
  → ∃ λ p → fromCoordsK xv yv ≡ just p
          × CircuitWitness.assign w out ≡ just (val-secp-point p)
agree-from-coords-secp {xop = xop} {yop} sub rx ry
  (inj₂ (xv , yv , p , wx , wy , fcEq , wout))
  with trans (sym (⊑-resolve xop sub rx)) wx
     | trans (sym (⊑-resolve yop sub ry)) wy
... | refl | refl = p , fcEq , wout
agree-from-coords-secp {w = w} {xop = xop} sub rx ry
  (inj₁ (xv , yv , p , wx , wy , fcEq , wout))
  with trans (sym (⊑-resolve xop sub rx)) (resolveᶜ-Fr-inv w xop wx)
... | ()

agree-into-bytes-sb : ∀ {m w a out x}
  → m ⊑ᵂ w → resolve m a ≡ just (val-secp-base x)
  → holds w (into-bytes out a)
  → CircuitWitness.assign w out ≡ just (val-bytes32 (secpBaseToBytes x))
agree-into-bytes-sb {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (x , () , _)
...   | inj₂ (inj₁ (x , refl , refl)) = wout
...   | inj₂ (inj₂ (x , () , _))

agree-into-bytes-ss : ∀ {m w a out x}
  → m ⊑ᵂ w → resolve m a ≡ just (val-secp-scalar x)
  → holds w (into-bytes out a)
  → CircuitWitness.assign w out ≡ just (val-bytes32 (secpScalarToBytes x))
agree-into-bytes-ss {a = a} sub ra (av , ov , wa , wout , disj)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl with disj
...   | inj₁ (x , () , _)
...   | inj₂ (inj₁ (x , () , _))
...   | inj₂ (inj₂ (x , refl , refl)) = wout

-- `not` / `cond-select`: the output value's boolean reading couples
-- step-success and the witness output cell (the same `is-bit` split), so
-- each returns the run's boolean reading (feeding `not-step`/
-- `cond-select-step`) bundled with the witness output cell (feeding
-- `ins-⊑ᵂ`).  `to𝔹`/`isYes (_ ≟ᶠ 1ᶠ)` are stuck on `_≟ᶠ_`; each is read
-- off booleanity by casing the `≟ᶠ` decisions (impossible branch via the
-- Group-A law `1ᶠ≢0ᶠ`).  `resolve𝔹 m op ≡ resolveᶠ m op >>= to𝔹`.

agree-not : ∀ {m w a out x}
  → m ⊑ᵂ w → resolveᶠ m a ≡ just x
  → holds w (is-not out a)
  → ∃ λ b → resolve𝔹 m a ≡ just b
          × CircuitWitness.assign w out ≡ just (val-native (χˢ (bnot b)))
agree-not {a = a} sub ra (x , wa , inj₁ x≡0 , wout)
  with trans (sym (⊑-resolveᶠ a sub ra)) wa
... | refl =
  false , trans (cong (_>>= to𝔹) ra) (to𝔹-0 x≡0) ,
  trans wout
    (cong (λ z → just (val-native (χᶜ (bnot z)))) (isYes1-0 x≡0))
agree-not {a = a} sub ra (x , wa , inj₂ x≡1 , wout)
  with trans (sym (⊑-resolveᶠ a sub ra)) wa
... | refl =
  true , trans (cong (_>>= to𝔹) ra) (to𝔹-1 x≡1) ,
  trans wout
    (cong (λ z → just (val-native (χᶜ (bnot z)))) (isYes1-1 x≡1))

agree-cond-select : ∀ {m w bit a b out xbit av bvl}
  → m ⊑ᵂ w
  → resolveᶠ m bit ≡ just xbit
  → resolve m a ≡ just av → resolve m b ≡ just bvl
  → holds w (select out bit a b)
  → ∃ λ bb → resolve𝔹 m bit ≡ just bb
           × CircuitWitness.assign w out ≡ just (if bb then av else bvl)
agree-cond-select {bit = bit} {a} {b} sub rbit ra rb
  (bv , av , bvl , ov , ⊢bit , ⊢a , ⊢b , wout , inj₁ bv≡0 , imp1 , imp0)
  with trans (sym (⊑-resolveᶠ bit sub rbit)) ⊢bit
     | trans (sym (⊑-resolve a sub ra)) ⊢a
     | trans (sym (⊑-resolve b sub rb)) ⊢b
... | refl | refl | refl =
  false , trans (cong (_>>= to𝔹) rbit) (to𝔹-0 bv≡0) ,
  trans wout (cong just (imp0 bv≡0))
agree-cond-select {bit = bit} {a} {b} sub rbit ra rb
  (bv , av , bvl , ov , ⊢bit , ⊢a , ⊢b , wout , inj₂ bv≡1 , imp1 , imp0)
  with trans (sym (⊑-resolveᶠ bit sub rbit)) ⊢bit
     | trans (sym (⊑-resolve a sub ra)) ⊢a
     | trans (sym (⊑-resolve b sub rb)) ⊢b
... | refl | refl | refl =
  true , trans (cong (_>>= to𝔹) rbit) (to𝔹-1 bv≡1) ,
  trans wout (cong just (imp1 bv≡1))

------------------------------------------------------------------------
-- The `*-step` family: deterministic forward step-success.
--
-- Once the operands resolve at the run memory (from TyEq/OpTy) and any
-- partial semantic operation succeeds (from `holds w` via the `agree-*`
-- kernels), `step` computes and lands on a CONCRETE post-state (pinned,
-- mirroring the `*-bwd` conclusions, to sidestep the `out1`/`step`
-- non-injectivity unification stall).  `mul-step` is the template.
------------------------------------------------------------------------

mul-step : ∀ {P S st a b output x y}
  → resolve (State.mem st) a ≡ just (val-native x)
  → resolve (State.mem st) b ≡ just (val-native y)
  → step P S st (mul a b output)
      ≡ just (out1 st output (val-native (x *ᶠ y)))
mul-step ra rb rewrite ra | rb = refl

add-step-n : ∀ {P S st a b out x y}
  → resolve (State.mem st) a ≡ just (val-native x)
  → resolve (State.mem st) b ≡ just (val-native y)
  → step P S st (add a b out) ≡ just (out1 st out (val-native (x +ᶠ y)))
add-step-n ra rb rewrite ra | rb = refl

add-step-p : ∀ {P S st a b out p q}
  → resolve (State.mem st) a ≡ just (val-jubjub-point p)
  → resolve (State.mem st) b ≡ just (val-jubjub-point q)
  → step P S st (add a b out)
      ≡ just (out1 st out (val-jubjub-point (p +J q)))
add-step-p ra rb rewrite ra | rb = refl

neg-step-n : ∀ {P S st a out x}
  → resolve (State.mem st) a ≡ just (val-native x)
  → step P S st (neg a out) ≡ just (out1 st out (val-native (-ᶠ x)))
neg-step-n ra rewrite ra = refl

neg-step-p : ∀ {P S st a out p}
  → resolve (State.mem st) a ≡ just (val-jubjub-point p)
  → step P S st (neg a out) ≡ just (out1 st out (val-jubjub-point (negJ p)))
neg-step-p ra rewrite ra = refl

copy-step : ∀ {P S st a out v}
  → resolve (State.mem st) a ≡ just v
  → step P S st (copy a out) ≡ just (out1 st out v)
copy-step ra rewrite ra = refl

inv-step : ∀ {P S st a out x xi}
  → resolve (State.mem st) a ≡ just (val-native x) → invᶠ x ≡ just xi
  → step P S st (inv a out) ≡ just (out1 st out (val-native xi))
inv-step ra ri rewrite ra | ri = refl

inv-step-sb : ∀ {P S st a out x xi}
  → resolve (State.mem st) a ≡ just (val-secp-base x) → invᵇ x ≡ just xi
  → step P S st (inv a out) ≡ just (out1 st out (val-secp-base xi))
inv-step-sb ra ri rewrite ra | ri = refl

inv-step-ss : ∀ {P S st a out x xi}
  → resolve (State.mem st) a ≡ just (val-secp-scalar x) → invˢ x ≡ just xi
  → step P S st (inv a out) ≡ just (out1 st out (val-secp-scalar xi))
inv-step-ss ra ri rewrite ra | ri = refl

mul-step-sb : ∀ {P S st a b out x y}
  → resolve (State.mem st) a ≡ just (val-secp-base x)
  → resolve (State.mem st) b ≡ just (val-secp-base y)
  → step P S st (mul a b out) ≡ just (out1 st out (val-secp-base (x *ᵇ y)))
mul-step-sb ra rb rewrite ra | rb = refl

mul-step-ss : ∀ {P S st a b out x y}
  → resolve (State.mem st) a ≡ just (val-secp-scalar x)
  → resolve (State.mem st) b ≡ just (val-secp-scalar y)
  → step P S st (mul a b out) ≡ just (out1 st out (val-secp-scalar (x *ˢ y)))
mul-step-ss ra rb rewrite ra | rb = refl

add-step-sp : ∀ {P S st a b out p q}
  → resolve (State.mem st) a ≡ just (val-secp-point p)
  → resolve (State.mem st) b ≡ just (val-secp-point q)
  → step P S st (add a b out) ≡ just (out1 st out (val-secp-point (p +K q)))
add-step-sp ra rb rewrite ra | rb = refl

add-step-sb : ∀ {P S st a b out x y}
  → resolve (State.mem st) a ≡ just (val-secp-base x)
  → resolve (State.mem st) b ≡ just (val-secp-base y)
  → step P S st (add a b out) ≡ just (out1 st out (val-secp-base (x +ᵇ y)))
add-step-sb ra rb rewrite ra | rb = refl

add-step-ss : ∀ {P S st a b out x y}
  → resolve (State.mem st) a ≡ just (val-secp-scalar x)
  → resolve (State.mem st) b ≡ just (val-secp-scalar y)
  → step P S st (add a b out) ≡ just (out1 st out (val-secp-scalar (x +ˢ y)))
add-step-ss ra rb rewrite ra | rb = refl

neg-step-sp : ∀ {P S st a out p}
  → resolve (State.mem st) a ≡ just (val-secp-point p)
  → step P S st (neg a out) ≡ just (out1 st out (val-secp-point (negK p)))
neg-step-sp ra rewrite ra = refl

neg-step-sb : ∀ {P S st a out x}
  → resolve (State.mem st) a ≡ just (val-secp-base x)
  → step P S st (neg a out) ≡ just (out1 st out (val-secp-base (-ᵇ x)))
neg-step-sb ra rewrite ra = refl

neg-step-ss : ∀ {P S st a out x}
  → resolve (State.mem st) a ≡ just (val-secp-scalar x)
  → step P S st (neg a out) ≡ just (out1 st out (val-secp-scalar (-ˢ x)))
neg-step-ss ra rewrite ra = refl

not-step : ∀ {P S st a out b}
  → resolve𝔹 (State.mem st) a ≡ just b
  → step P S st (not a out) ≡ just (out1 st out (val-native (χˢ (bnot b))))
not-step ra rewrite ra = refl

test-eq-step : ∀ {P S st a b out av bv e}
  → resolve (State.mem st) a ≡ just av → resolve (State.mem st) b ≡ just bv
  → valEq? av bv ≡ just e
  → step P S st (test-eq a b out) ≡ just (out1 st out (val-native (χˢ e)))
test-eq-step ra rb ve rewrite ra | rb | ve = refl

jubjub-scalar-from-native-step : ∀ {P S st a out x}
  → resolveᶠ (State.mem st) a ≡ just x
  → step P S st (jubjub-scalar-from-native a out)
      ≡ just (out1 st out (val-jubjub-scalar (native→jubjubScalar x)))
jubjub-scalar-from-native-step ra rewrite ra = refl

ec-mul-step : ∀ {P S st a scalar out p s}
  → resolve (State.mem st) a ≡ just (val-jubjub-point p)
  → resolve (State.mem st) scalar ≡ just (val-jubjub-scalar s)
  → step P S st (ec-mul a scalar out)
      ≡ just (out1 st out (val-jubjub-point (s ·J p)))
ec-mul-step ra rs rewrite ra | rs = refl

ec-mul-generator-step : ∀ {P S st scalar out s}
  → resolve (State.mem st) scalar ≡ just (val-jubjub-scalar s)
  → step P S st (ec-mul-generator scalar out)
      ≡ just (out1 st out (val-jubjub-point (s ·J genJ)))
ec-mul-generator-step rs rewrite rs = refl

ec-mul-step-secp : ∀ {P S st a scalar out p s}
  → resolve (State.mem st) a ≡ just (val-secp-point p)
  → resolve (State.mem st) scalar ≡ just (val-secp-scalar s)
  → step P S st (ec-mul a scalar out)
      ≡ just (out1 st out (val-secp-point (s ·K p)))
ec-mul-step-secp ra rs rewrite ra | rs = refl

ec-mul-generator-step-secp : ∀ {P S st scalar out s}
  → resolve (State.mem st) scalar ≡ just (val-secp-scalar s)
  → step P S st (ec-mul-generator scalar out)
      ≡ just (out1 st out (val-secp-point (s ·K genK)))
ec-mul-generator-step-secp rs rewrite rs = refl

hash-to-curve-step : ∀ {P S st inputs out frs}
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → step P S st (hash-to-curve inputs out)
      ≡ just (out1 st out (val-jubjub-point (hash-to-curve-fn frs)))
hash-to-curve-step rf rewrite rf = refl

transient-hash-step : ∀ {P S st inputs out frs}
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → step P S st (transient-hash inputs out)
      ≡ just (out1 st out (val-native (transient-hash-fn frs)))
transient-hash-step rf rewrite rf = refl

into-bytes32-step : ∀ {P S st a out x}
  → resolve (State.mem st) a ≡ just (val-native x)
  → step P S st (into-bytes32 a out)
      ≡ just (out1 st out (val-bytes32 (nativeToBytes x)))
into-bytes32-step ra rewrite ra = refl

into-bytes32-step-sb : ∀ {P S st a out x}
  → resolve (State.mem st) a ≡ just (val-secp-base x)
  → step P S st (into-bytes32 a out)
      ≡ just (out1 st out (val-bytes32 (secpBaseToBytes x)))
into-bytes32-step-sb ra rewrite ra = refl

into-bytes32-step-ss : ∀ {P S st a out x}
  → resolve (State.mem st) a ≡ just (val-secp-scalar x)
  → step P S st (into-bytes32 a out)
      ≡ just (out1 st out (val-bytes32 (secpScalarToBytes x)))
into-bytes32-step-ss ra rewrite ra = refl

from-bytes32-step : ∀ {P S st a out b}
  → resolve (State.mem st) a ≡ just (val-bytes32 b)
  → step P S st (from-bytes32 a native out)
      ≡ just (out1 st out (val-native (nativeFromBytes b)))
from-bytes32-step ra rewrite ra = refl

from-bytes32-secp-base-step : ∀ {P S st a out b}
  → resolve (State.mem st) a ≡ just (val-bytes32 b)
  → step P S st (from-bytes32 a secp-base out)
      ≡ just (out1 st out (val-secp-base (secpBaseFromBytes b)))
from-bytes32-secp-base-step ra rewrite ra = refl

from-bytes32-secp-scalar-step : ∀ {P S st a out b}
  → resolve (State.mem st) a ≡ just (val-bytes32 b)
  → step P S st (from-bytes32 a secp-scalar out)
      ≡ just (out1 st out (val-secp-scalar (secpScalarFromBytes b)))
from-bytes32-secp-scalar-step ra rewrite ra = refl

reverse-bytes-step : ∀ {P S st a out b}
  → resolve (State.mem st) a ≡ just (val-bytes32 b)
  → step P S st (reverse-bytes a out)
      ≡ just (out1 st out (val-bytes32 (reverse b)))
reverse-bytes-step ra rewrite ra = refl

from-coordinates-step : ∀ {P S st xop yop out x y p}
  → resolve (State.mem st) xop ≡ just (val-native x)
  → resolve (State.mem st) yop ≡ just (val-native y)
  → fromCoordsJ x y ≡ just p
  → step P S st (from-coordinates (xop , yop) out)
      ≡ just (out1 st out (val-jubjub-point p))
from-coordinates-step rx ry fp rewrite rx | ry | fp = refl

from-coordinates-step-secp : ∀ {P S st xop yop out x y p}
  → resolve (State.mem st) xop ≡ just (val-secp-base x)
  → resolve (State.mem st) yop ≡ just (val-secp-base y)
  → fromCoordsK x y ≡ just p
  → step P S st (from-coordinates (xop , yop) out)
      ≡ just (out1 st out (val-secp-point p))
from-coordinates-step-secp rx ry fp rewrite rx | ry | fp = refl

into-coordinates-step : ∀ {P S st point xid yid p x y}
  → resolve (State.mem st) point ≡ just (val-jubjub-point p)
  → coordsJ p ≡ (x , y)
  → step P S st (into-coordinates point (xid , yid))
      ≡ just (out1 (out1 st xid (val-native x)) yid (val-native y))
into-coordinates-step rp cp rewrite rp | cp = refl

into-coordinates-step-secp : ∀ {P S st point xid yid p x y}
  → resolve (State.mem st) point ≡ just (val-secp-point p)
  → coordsK p ≡ just (x , y)
  → step P S st (into-coordinates point (xid , yid))
      ≡ just (out1 (out1 st xid (val-secp-base x)) yid (val-secp-base y))
into-coordinates-step-secp rp cp rewrite rp | cp = refl

bytes32-into-low-high-step : ∀ {P S st bytes loid hiid b lo hi}
  → resolve (State.mem st) bytes ≡ just (val-bytes32 b)
  → bytes32→low-high b ≡ (lo , hi)
  → step P S st (bytes32-into-low-high bytes (loid , hiid))
      ≡ just (out1 (out1 st loid (val-native lo)) hiid (val-native hi))
bytes32-into-low-high-step rb bl rewrite rb | bl = refl

bytes32-from-low-high-step : ∀ {P S st loop hiop out lo hi b}
  → resolveᶠ (State.mem st) loop ≡ just lo
  → resolveᶠ (State.mem st) hiop ≡ just hi
  → low-high→bytes32 lo hi ≡ just b
  → step P S st (bytes32-from-low-high (loop , hiop) out)
      ≡ just (out1 st out (val-bytes32 b))
bytes32-from-low-high-step rl rh lb rewrite rl | rh | lb = refl

div-mod-step : ∀ {P S st val bits outs x st'}
  → resolveᶠ (State.mem st) val ≡ just x
  → insertMany st outs
      ( val-native (from-le-bits (drop bits (to-le-bits x)))
      ∷ val-native (from-le-bits (take bits (to-le-bits x))) ∷ []) ≡ just st'
  → step P S st (div-mod-power-of-two val bits outs) ≡ just st'
div-mod-step rv im rewrite rv | im = refl

reconstitute-field-step : ∀ {P S st divisor modulus bits out d mo}
  → resolveᶠ (State.mem st) divisor ≡ just d
  → resolveᶠ (State.mem st) modulus ≡ just mo
  → valFr mo < 2 ^ bits → valFr d < 2 ^ (FR-BITS ∸ bits)
  → valFr mo + 2 ^ bits * valFr d < FR-ORDER
  → step P S st (reconstitute-field divisor modulus bits out)
      ≡ just (out1 st out (val-native ((pow2ᶠˢ bits *ᶠ d) +ᶠ mo)))
reconstitute-field-step {bits = bits} {d = d} {mo} rd rmo bm dm om
  rewrite rd | rmo with valFr mo <? 2 ^ bits
... | no ¬bm = case ¬bm bm of λ ()
... | yes _ with valFr d <? 2 ^ (FR-BITS ∸ bits)
...   | no ¬dm = case ¬dm dm of λ ()
...   | yes _ with valFr mo + 2 ^ bits * valFr d <? FR-ORDER
...     | no ¬om = case ¬om om of λ ()
...     | yes _ = refl

less-than-step : ∀ {P S st a b bits out x y}
  → resolveᶠ (State.mem st) a ≡ just x
  → resolveᶠ (State.mem st) b ≡ just y
  → valFr x < 2 ^ bits → valFr y < 2 ^ bits
  → step P S st (less-than a b bits out)
      ≡ just (out1 st out (val-native (χˢ (isYes (valFr x <? valFr y)))))
less-than-step {bits = bits} {x = x} {y} ra rb bx by
  rewrite ra | rb with valFr x <? 2 ^ bits
... | no ¬bx = case ¬bx bx of λ ()
... | yes _ with valFr y <? 2 ^ bits
...   | no ¬by = case ¬by by of λ ()
...   | yes _ = refl

persistent-hash-step : ∀ {P S st al inputs out frs h}
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → persistent-hash-fn al frs ≡ just h
  → step P S st (persistent-hash al inputs out)
      ≡ just (out1 st out (val-bytes32 h))
persistent-hash-step rf ph rewrite rf | ph = refl

keccak256-step : ∀ {P S st al inputs out frs h}
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → keccak-fn al frs ≡ just h
  → step P S st (keccak256 al inputs out)
      ≡ just (out1 st out (val-bytes32 h))
keccak256-step rf kf rewrite rf | kf = refl

encode-step : ∀ {P S st a outs v st'}
  → resolve (State.mem st) a ≡ just v
  → insertMany st outs (map val-native (encodeᵉ v)) ≡ just st'
  → step P S st (encode a outs) ≡ just st'
encode-step ra im rewrite ra | im = refl

cond-select-step : ∀ {P S st bit a b out bv av bvl}
  → resolve𝔹 (State.mem st) bit ≡ just bv
  → resolve (State.mem st) a ≡ just av
  → resolve (State.mem st) b ≡ just bvl
  → typeof av ≡ typeof bvl
  → step P S st (cond-select bit a b out)
      ≡ just (out1 st out (if bv then av else bvl))
cond-select-step {av = av} {bvl} rbit ra rb ty
  rewrite rbit | ra | rb with typeof av ≟T typeof bvl
... | yes _ = refl
... | no ¬p = case ¬p ty of λ ()

assert-step : ∀ {P S st cond}
  → resolve𝔹 (State.mem st) cond ≡ just true
  → step P S st (assert cond) ≡ just st
assert-step rc rewrite rc = refl

constrain-bits-step : ∀ {P S st val bits x}
  → resolveᶠ (State.mem st) val ≡ just x → valFr x < 2 ^ bits
  → step P S st (constrain-bits val bits) ≡ just st
constrain-bits-step {bits = bits} {x = x} rv bd
  rewrite rv with valFr x <? 2 ^ bits
... | no ¬bd = case ¬bd bd of λ ()
... | yes _ = refl

constrain-eq-step : ∀ {P S st a b av bv}
  → resolve (State.mem st) a ≡ just av → resolve (State.mem st) b ≡ just bv
  → valEq? av bv ≡ just true
  → step P S st (constrain-eq a b) ≡ just st
constrain-eq-step ra rb ve rewrite ra | rb | ve = refl

constrain-to-boolean-step : ∀ {P S st val b}
  → resolve𝔹 (State.mem st) val ≡ just b
  → step P S st (constrain-to-boolean val) ≡ just st
constrain-to-boolean-step rv rewrite rv = refl

------------------------------------------------------------------------
-- Encoding round-trip and width law.  The transcript pre-pass reproduces
-- a witness cell `v` by placing `encode v` in the transcript; the run then
-- `decode`s exactly `encode v` back to `v` (round-trip, from the Group-C
-- laws), consuming `encoded-len (typeof v)` elements (width law).
------------------------------------------------------------------------

encode-len : ∀ v → length (encodeᵉ v) ≡ encoded-len (typeof v)
encode-len (val-native _)        = refl
encode-len (val-jubjub-scalar _) = refl
encode-len (val-bytes32 b)       with bytes32→low-high b
... | (_ , _) = refl
encode-len (val-jubjub-point p)  with coordsJ p
... | (_ , _) = refl
encode-len (val-secp-base x)     with secpBase→limbs x
... | (_ , _) = refl
encode-len (val-secp-scalar s)   with secpScalar→limbs s
... | (_ , _) = refl
encode-len (val-secp-point p)    with secpPoint→limbs p
... | (_ , _ , _ , _ , _) = refl

encode-decode : ∀ v → decode (typeof v) (encodeᵉ v) ≡ just v
encode-decode (val-native x)        = refl
encode-decode (val-jubjub-scalar s) rewrite jubjubScalar-round s = refl
encode-decode (val-bytes32 b)       with bytes32→low-high b in beq
... | (lo , hi)
  rewrite trans (cong (uncurry low-high→bytes32) (sym beq)) (bytes32-round b)
  = refl
encode-decode (val-jubjub-point p)  with coordsJ p in ceq
... | (x , y)
  rewrite trans (cong (uncurry fromCoordsJ) (sym ceq)) (coordsJ-fromCoordsJ p)
  = refl
encode-decode (val-secp-base x)     with secpBase→limbs x in beq
... | (l , h)
  rewrite trans (cong (uncurry limbs→secpBase) (sym beq)) (secpBase-round x)
  = refl
encode-decode (val-secp-scalar s)   with secpScalar→limbs s in seq
... | (l , h)
  rewrite trans (cong (uncurry limbs→secpScalar) (sym seq)) (secpScalar-round s)
  = refl
encode-decode (val-secp-point p)    with secpPoint→limbs p in peq
... | (a , b , c , d , e)
  rewrite secpPoint-round peq = refl

------------------------------------------------------------------------
-- Binding a cell whose value the witness already carries preserves
-- agreement (no freshness needed).  This is how each `build` clause
-- maintains `mem st ⊑ᵂ w` after `step` binds the output to the value the
-- `agree-*` kernel pinned in `w`.
------------------------------------------------------------------------

ins-⊑ᵂ : ∀ {m w} out {v} → m ⊑ᵂ w → CircuitWitness.assign w out ≡ just v
  → ins out v m ⊑ᵂ w
ins-⊑ᵂ out sub wo {id} e with id ≟str out
... | yes refl = trans wo e
... | no  _    = sub e

------------------------------------------------------------------------
-- The witness-shape side condition.
--
-- `public-input`/`private-input` emit no constraint and ignore the guard
-- (§7.2), so `satisfies w` pins neither the guard nor the free output
-- cell.  A run reproducing `w` therefore needs, at `assign w`: the guard
-- resolves to a bit `b`; if active (`b ≡ true`) the output cell is a
-- value of the declared type `val-t` (so the transcript can carry
-- `encode` of it and the run decodes it back); if inactive (`b ≡ false`)
-- the cell is exactly `default-val val-t`, matching the run's default.
-- The declared inputs are the same shape (each a value of its type).
-- This constrains the INPUT witness only — the v3 analogue of v2's
-- `length (mem w) ≡ nr-wires` — never the conclusion.
------------------------------------------------------------------------

-- The active/inactive witness condition on one input instruction's cell.
WCell : CircuitWitness → IrType → Identifier → Bool → Set
WCell w val-t o true  =
  ∃ λ v → CircuitWitness.assign w o ≡ just v × typeof v ≡ val-t
WCell w val-t o false = CircuitWitness.assign w o ≡ just (default-val val-t)

-- `WSteps` is indexed by the source `S` too, because `circuit-output`'s
-- executability (`collectOutputs` over `IrSource.outputs S`) mentions it.
WSteps : IrSource → CircuitWitness → List Instruction → Set
WSteps S w []       = ⊤
WSteps S w (public-input guard val-t o ∷ is) =
  (∃ λ b → eval-guard (CircuitWitness.assign w) guard ≡ just b × WCell w val-t o b)
  × WSteps S w is
WSteps S w (private-input guard val-t o ∷ is) =
  (∃ λ b → eval-guard (CircuitWitness.assign w) guard ≡ just b × WCell w val-t o b)
  × WSteps S w is
-- `impact`'s guard is not forced to a bit by the constraint when `inputs`
-- is empty (`impact-constraints … [] ≡ []`), so a run reproducing `w`
-- needs the guard to read a bit at `assign w`.  (For non-empty `inputs`
-- the pi-impact constraint already gives `is-bit`, but stating it here
-- uniformly keeps the empty case in scope.)
WSteps S w (impact guard inputs ∷ is) =
  (∃ λ x → resolveᶜ-Fr w guard ≡ just x × ∃ λ b → to𝔹 x ≡ just b)
  × WSteps S w is
-- `assert` emits only `non-zero cond` (≢ 0), but the off-circuit `step`
-- reads the guard as a bit and requires it `true`; so an executable
-- witness reads `cond` as `1` (`to𝔹 … ≡ just true`).
WSteps S w (assert cond ∷ is) =
  (∃ λ x → resolveᶜ-Fr w cond ≡ just x × to𝔹 x ≡ just true)
  × WSteps S w is
-- `reconstitute-field`'s constraint carries the two range bounds but not
-- the field-no-overflow guard (`valFr mo + 2ⁿ·valFr d < FR-ORDER`), which
-- the off-circuit `step` checks — a producer/executability condition.
WSteps S w (reconstitute-field divisor modulus bits o ∷ is) =
  (∃ λ d → ∃ λ mo → resolveᶜ-Fr w divisor ≡ just d
       × resolveᶜ-Fr w modulus ≡ just mo
       × (valFr mo + 2 ^ bits * valFr d < FR-ORDER))
  × WSteps S w is
-- `div-mod` binds exactly two outputs; the off-circuit `step` `insertMany`s
-- a two-element list, and `synth` emits its constraint only at `|outs| ≡ 2`
-- — but `outtys`/producer-WT accept any length, so the two-output arity is
-- an executability condition.  (`persistent-hash`/`keccak256` now bind a
-- single Bytes32 output; their hash-function success comes from `holds`, so
-- they need no `WSteps` condition and fall to the catch-all.)
WSteps S w (div-mod-power-of-two val bits outs ∷ is) =
  (∃ λ q → ∃ λ r → outs ≡ q ∷ r ∷ []) × WSteps S w is
-- `circuit-output` emits no constraint, and its `OpTy` (`AllDefdᵒ`) gives
-- only operand boundness, not the type match against the signature — so
-- the `collectOutputs` over the declared output types must succeed at `w`.
WSteps S w (circuit-output vals ∷ is) =
  (∃ λ vs → collectOutputs (CircuitWitness.assign w) (IrSource.outputs S) vals
              ≡ just vs)
  × WSteps S w is
-- `from-bytes32` `synth`s `from-bytes` regardless of the declared `val-t`;
-- the `from-bytes` constraint pins the output cell to one of the three
-- supported values (Native/Secp256k1Base/Secp256k1Scalar) but does NOT tie
-- the disjunct to `val-t`.  Requiring the output cell to have type `val-t`
-- selects the matching disjunct and, since the pinned value's constructor
-- is one of the three, forces `val-t` into the supported set (the off-circuit
-- `step` only succeeds there).
WSteps S w (from-bytes32 bytes val-t o ∷ is) =
  WCell w val-t o true × WSteps S w is
-- `less-than`'s constraint pins only the chip's evened operand bounds
-- (`< 2 ^ even4 bits`); the off-circuit `step` checks the exact
-- `2 ^ bits` bounds, so for odd or small `bits` the exact bounds are an
-- executability condition (spec §9, "Contract strength").
WSteps S w (less-than a b bits o ∷ is) =
  (∃ λ x → ∃ λ y → resolveᶜ-Fr w a ≡ just x × resolveᶜ-Fr w b ≡ just y
       × (valFr x < 2 ^ bits) × (valFr y < 2 ^ bits))
  × WSteps S w is
WSteps S w (_ ∷ is) = WSteps S w is

-- Each declared input cell is a value of its declared type.
WInputs : CircuitWitness → List TypedIdentifier → Set
WInputs w []         = ⊤
WInputs w (ti ∷ tis) =
  (∃ λ v → CircuitWitness.assign w (TypedIdentifier.name ti) ≡ just v
         × typeof v ≡ TypedIdentifier.val-t ti)
  × WInputs w tis

WShape : IrSource → CircuitWitness → Set
WShape S w = WInputs w (IrSource.inputs S) × WSteps S w (IrSource.instructions S)

------------------------------------------------------------------------
-- `WShape?` — the decidable witness-shape check, completing the runnable
-- trio with `producer-SA?` / `producer-WT?`.  Enumerates all 34
-- instructions (a catch-all would leave `WSteps S w (i ∷ is)` stuck).
------------------------------------------------------------------------

-- Decidable equality of witness values.  `JubjubScalar` has no primitive
-- `DecidableEquality`, but `jubjubScalarToFr` is injective (its left
-- inverse is `jubjubScalarFromFr`, by `jubjubScalar-round`), so scalar
-- equality reduces to `_≟ᶠ_` on the images.
_≟V_ : DecidableEquality IrValue
val-native x ≟V val-native y =
  map′ (cong val-native) (λ { refl → refl }) (x ≟ᶠ y)
val-native _ ≟V val-bytes32 _        = no λ ()
val-native _ ≟V val-jubjub-point _   = no λ ()
val-native _ ≟V val-jubjub-scalar _  = no λ ()
val-bytes32 _ ≟V val-native _        = no λ ()
val-bytes32 a ≟V val-bytes32 b =
  map′ (cong val-bytes32) (λ { refl → refl }) (a ≟B b)
val-bytes32 _ ≟V val-jubjub-point _  = no λ ()
val-bytes32 _ ≟V val-jubjub-scalar _ = no λ ()
val-jubjub-point _ ≟V val-native _        = no λ ()
val-jubjub-point _ ≟V val-bytes32 _       = no λ ()
val-jubjub-point p ≟V val-jubjub-point q =
  map′ (cong val-jubjub-point) (λ { refl → refl }) (p ≟J q)
val-jubjub-point _ ≟V val-jubjub-scalar _ = no λ ()
val-jubjub-scalar _ ≟V val-native _       = no λ ()
val-jubjub-scalar _ ≟V val-bytes32 _      = no λ ()
val-jubjub-scalar _ ≟V val-jubjub-point _ = no λ ()
val-jubjub-scalar s ≟V val-jubjub-scalar t =
  map′ (λ e → cong val-jubjub-scalar (sc-inj e)) (λ { refl → refl })
       (jubjubScalarToFr s ≟ᶠ jubjubScalarToFr t)
  where
    sc-inj : jubjubScalarToFr s ≡ jubjubScalarToFr t → s ≡ t
    sc-inj e = just-injective
      (trans (sym (jubjubScalar-round s))
        (trans (cong jubjubScalarFromFr e) (jubjubScalar-round t)))
val-native _ ≟V val-secp-point _         = no λ ()
val-native _ ≟V val-secp-base _          = no λ ()
val-native _ ≟V val-secp-scalar _        = no λ ()
val-bytes32 _ ≟V val-secp-point _        = no λ ()
val-bytes32 _ ≟V val-secp-base _         = no λ ()
val-bytes32 _ ≟V val-secp-scalar _       = no λ ()
val-jubjub-point _ ≟V val-secp-point _   = no λ ()
val-jubjub-point _ ≟V val-secp-base _    = no λ ()
val-jubjub-point _ ≟V val-secp-scalar _  = no λ ()
val-jubjub-scalar _ ≟V val-secp-point _  = no λ ()
val-jubjub-scalar _ ≟V val-secp-base _   = no λ ()
val-jubjub-scalar _ ≟V val-secp-scalar _ = no λ ()
val-secp-point _ ≟V val-native _         = no λ ()
val-secp-point _ ≟V val-bytes32 _        = no λ ()
val-secp-point _ ≟V val-jubjub-point _   = no λ ()
val-secp-point _ ≟V val-jubjub-scalar _  = no λ ()
val-secp-point p ≟V val-secp-point q =
  map′ (cong val-secp-point) (λ { refl → refl }) (p ≟K q)
val-secp-point _ ≟V val-secp-base _      = no λ ()
val-secp-point _ ≟V val-secp-scalar _    = no λ ()
val-secp-base _ ≟V val-native _          = no λ ()
val-secp-base _ ≟V val-bytes32 _         = no λ ()
val-secp-base _ ≟V val-jubjub-point _    = no λ ()
val-secp-base _ ≟V val-jubjub-scalar _   = no λ ()
val-secp-base _ ≟V val-secp-point _      = no λ ()
val-secp-base x ≟V val-secp-base y =
  map′ (cong val-secp-base) (λ { refl → refl }) (x ≟ᵇ y)
val-secp-base _ ≟V val-secp-scalar _     = no λ ()
val-secp-scalar _ ≟V val-native _        = no λ ()
val-secp-scalar _ ≟V val-bytes32 _       = no λ ()
val-secp-scalar _ ≟V val-jubjub-point _  = no λ ()
val-secp-scalar _ ≟V val-jubjub-scalar _ = no λ ()
val-secp-scalar _ ≟V val-secp-point _    = no λ ()
val-secp-scalar _ ≟V val-secp-base _     = no λ ()
val-secp-scalar x ≟V val-secp-scalar y =
  map′ (cong val-secp-scalar) (λ { refl → refl }) (x ≟ˢ y)

-- `mx` is defined.
defined? : ∀ {A : Set} (mx : Maybe A) → Dec (∃ λ x → mx ≡ just x)
defined? nothing  = no λ { (_ , ()) }
defined? (just x) = yes (x , refl)

-- `mx` is defined and its value satisfies a decidable `Q`.
∃just? : ∀ {A : Set} {Q : A → Set} (mx : Maybe A) → (∀ x → Dec (Q x))
  → Dec (∃ λ x → mx ≡ just x × Q x)
∃just? nothing  q? = no λ { (_ , () , _) }
∃just? {Q = Q} (just x) q? =
  map′ (λ qx → x , refl , qx)
       (λ { (x' , je , qx) → subst Q (sym (just-injective je)) qx })
       (q? x)

-- `WCell w val-t o b` is decidable.
WCell? : ∀ w val-t o b → Dec (WCell w val-t o b)
WCell? w val-t o true  =
  ∃just? (CircuitWitness.assign w o) (λ v → typeof v ≟T val-t)
WCell? w val-t o false =
  ≡-dec _≟V_ (CircuitWitness.assign w o) (just (default-val val-t))

-- `reconstitute-field`'s executability condition (two resolutions + the
-- field-no-overflow bound).
reconOK? : ∀ w divisor modulus bits
  → Dec (∃ λ d → ∃ λ mo → resolveᶜ-Fr w divisor ≡ just d
          × resolveᶜ-Fr w modulus ≡ just mo
          × (valFr mo + 2 ^ bits * valFr d < FR-ORDER))
reconOK? w divisor modulus bits
  with resolveᶜ-Fr w divisor | resolveᶜ-Fr w modulus
... | nothing | _       = no λ { (_ , _ , () , _ , _) }
... | just d  | nothing = no λ { (_ , _ , _ , () , _) }
... | just d  | just mo =
      map′ (λ lt → d , mo , refl , refl , lt)
           (λ { (d' , mo' , jd , jmo , lt) →
                subst₂ (λ p q → valFr q + 2 ^ bits * valFr p < FR-ORDER)
                  (sym (just-injective jd)) (sym (just-injective jmo)) lt })
           (valFr mo + 2 ^ bits * valFr d <? FR-ORDER)

-- `less-than`'s executability condition (two resolutions + the exact
-- `2 ^ bits` operand bounds the off-circuit `step` checks).
ltOK? : ∀ w a b bits
  → Dec (∃ λ x → ∃ λ y → resolveᶜ-Fr w a ≡ just x × resolveᶜ-Fr w b ≡ just y
          × (valFr x < 2 ^ bits) × (valFr y < 2 ^ bits))
ltOK? w a b bits
  with resolveᶜ-Fr w a | resolveᶜ-Fr w b
... | nothing | _       = no λ { (_ , _ , () , _) }
... | just x  | nothing = no λ { (_ , _ , _ , () , _) }
... | just x  | just y  =
      map′ (λ (bx , by) → x , y , refl , refl , bx , by)
           (λ { (x' , y' , jx , jy , bx , by) →
                subst (λ p → valFr p < 2 ^ bits)
                  (sym (just-injective jx)) bx
              , subst (λ p → valFr p < 2 ^ bits)
                  (sym (just-injective jy)) by })
           ((valFr x <? 2 ^ bits) ×-dec (valFr y <? 2 ^ bits))

-- `div-mod`'s two-output arity.
divmodArity? : (outs : List Identifier)
  → Dec (∃ λ q → ∃ λ r → outs ≡ q ∷ r ∷ [])
divmodArity? []              = no λ { (_ , _ , ()) }
divmodArity? (q ∷ [])        = no λ { (_ , _ , ()) }
divmodArity? (q ∷ r ∷ [])    = yes (q , r , refl)
divmodArity? (q ∷ r ∷ _ ∷ _) = no λ { (_ , _ , ()) }

WSteps? : ∀ S w is → Dec (WSteps S w is)
WSteps? S w []                                      = yes tt
WSteps? S w (public-input guard val-t o ∷ is)       =
  ∃just? (eval-guard (CircuitWitness.assign w) guard) (WCell? w val-t o)
  ×-dec WSteps? S w is
WSteps? S w (private-input guard val-t o ∷ is)      =
  ∃just? (eval-guard (CircuitWitness.assign w) guard) (WCell? w val-t o)
  ×-dec WSteps? S w is
WSteps? S w (impact guard inputs ∷ is)              =
  ∃just? (resolveᶜ-Fr w guard) (λ x → defined? (to𝔹 x)) ×-dec WSteps? S w is
WSteps? S w (assert cond ∷ is)                      =
  ∃just? (resolveᶜ-Fr w cond) (λ x → ≡-dec _≟𝔹_ (to𝔹 x) (just true))
  ×-dec WSteps? S w is
WSteps? S w (reconstitute-field divisor modulus bits o ∷ is) =
  reconOK? w divisor modulus bits ×-dec WSteps? S w is
WSteps? S w (div-mod-power-of-two val bits outs ∷ is) =
  divmodArity? outs ×-dec WSteps? S w is
WSteps? S w (circuit-output vals ∷ is)              =
  defined? (collectOutputs (CircuitWitness.assign w) (IrSource.outputs S) vals)
  ×-dec WSteps? S w is
WSteps? S w (from-bytes32 bytes val-t o ∷ is)       =
  WCell? w val-t o true ×-dec WSteps? S w is
WSteps? S w (encode _ _ ∷ is)                       = WSteps? S w is
WSteps? S w (cond-select _ _ _ _ ∷ is)              = WSteps? S w is
WSteps? S w (constrain-bits _ _ ∷ is)               = WSteps? S w is
WSteps? S w (constrain-eq _ _ ∷ is)                 = WSteps? S w is
WSteps? S w (constrain-to-boolean _ ∷ is)           = WSteps? S w is
WSteps? S w (copy _ _ ∷ is)                         = WSteps? S w is
WSteps? S w (ec-mul _ _ _ ∷ is)                     = WSteps? S w is
WSteps? S w (ec-mul-generator _ _ ∷ is)             = WSteps? S w is
WSteps? S w (hash-to-curve _ _ ∷ is)                = WSteps? S w is
WSteps? S w (into-coordinates _ _ ∷ is)             = WSteps? S w is
WSteps? S w (from-coordinates _ _ ∷ is)             = WSteps? S w is
WSteps? S w (into-bytes32 _ _ ∷ is)                 = WSteps? S w is
WSteps? S w (reverse-bytes _ _ ∷ is)                = WSteps? S w is
WSteps? S w (bytes32-into-low-high _ _ ∷ is)        = WSteps? S w is
WSteps? S w (bytes32-from-low-high _ _ ∷ is)        = WSteps? S w is
WSteps? S w (transient-hash _ _ ∷ is)               = WSteps? S w is
WSteps? S w (persistent-hash _ _ _ ∷ is)            = WSteps? S w is
WSteps? S w (keccak256 _ _ _ ∷ is)                  = WSteps? S w is
WSteps? S w (test-eq _ _ _ ∷ is)                    = WSteps? S w is
WSteps? S w (add _ _ _ ∷ is)                        = WSteps? S w is
WSteps? S w (mul _ _ _ ∷ is)                        = WSteps? S w is
WSteps? S w (neg _ _ ∷ is)                          = WSteps? S w is
WSteps? S w (inv _ _ ∷ is)                          = WSteps? S w is
WSteps? S w (not _ _ ∷ is)                          = WSteps? S w is
WSteps? S w (less-than a b bits _ ∷ is)             =
  ltOK? w a b bits ×-dec WSteps? S w is
WSteps? S w (jubjub-scalar-from-native _ _ ∷ is)    = WSteps? S w is

WInputs? : ∀ w tis → Dec (WInputs w tis)
WInputs? w []         = yes tt
WInputs? w (ti ∷ tis) =
  ∃just? (CircuitWitness.assign w (TypedIdentifier.name ti))
    (λ v → typeof v ≟T TypedIdentifier.val-t ti)
  ×-dec WInputs? w tis

WShape? : ∀ S w → Dec (WShape S w)
WShape? S w =
  WInputs? w (IrSource.inputs S) ×-dec WSteps? S w (IrSource.instructions S)

------------------------------------------------------------------------
-- Transcript-cursor arithmetic and the pis-invariant seeding.
--
-- The transcript pre-pass fixes `P'`'s lists as in-order concatenations;
-- the build fold threads a cursor invariant of the form `drop cursor list
-- ≡ (future contributions)`.  These list lemmas discharge the window
-- reads/advances (`take-len-++` for the active-impact transcript check,
-- `drop-len-++` for the cursor advance).
------------------------------------------------------------------------

take-len-++ : ∀ {A : Set} (xs ys : List A) → take (length xs) (xs ++ ys) ≡ xs
take-len-++ []       ys = refl
take-len-++ (x ∷ xs) ys = cong (x ∷_) (take-len-++ xs ys)

drop-len-++ : ∀ {A : Set} (xs ys : List A) → drop (length xs) (xs ++ ys) ≡ ys
drop-len-++ []       ys = refl
drop-len-++ (x ∷ xs) ys = drop-len-++ xs ys

-- `drop k xs` exposes the `k`-th element when `pi-lookup` finds it.
drop-lookup : ∀ (xs : List Fr) k {v} → pi-lookup xs k ≡ just v
  → drop k xs ≡ v ∷ drop (suc k) xs
drop-lookup []       zero    e = case e of λ ()
drop-lookup []       (suc k) e = case e of λ ()
drop-lookup (x ∷ xs) zero    refl = refl
drop-lookup (x ∷ xs) (suc k) e    = drop-lookup xs k e

-- The pis-invariant seeding kernel (the v2 make-pti-correctness analogue).
-- On an active `impact` group the witness's pi entries at
-- `[start, start + length inputs)` are EXACTLY the resolved input values —
-- so the group the run appends to `pis` is precisely the slice of `pis w`
-- the prefix invariant `pis st ≼ pis w` requires.  `g ≡ 1ᶠ` (from the run's
-- active decision, transported to `w` by agreement) is shared across the
-- group; each `pi-impact` pins its entry to the input value.
impact-slice : ∀ w start guard inputs {vals}
  → resolveᶜ-all-Fr w inputs ≡ just vals
  → resolveᶜ-Fr w guard ≡ just 1ᶠ
  → satisfies-constraints (impact-constraints start guard inputs) w
  → take (length inputs) (drop start (CircuitWitness.pis w)) ≡ vals
impact-slice w start guard []       refl rg _ = refl
impact-slice w start guard (x ∷ xs) rall rg
  ((g , xv , pv , wg , wx , _ , wpv , g1→ , _) , rest)
  with refl ← trans (sym rg) wg
  with resolveᶜ-Fr w x | wx
... | .(just xv) | refl
  with resolveᶜ-all-Fr w xs in rxs
...   | just vs
  with refl ← rall
  rewrite drop-lookup (CircuitWitness.pis w) start wpv
        | g1→ refl
        | impact-slice w (suc start) guard xs rxs rg rest
  = refl

------------------------------------------------------------------------
-- Transcript pre-pass (partial): the pub-transcript-inputs stream.
--
-- Read from the witness's pi vector at the pi-cursor `k` (= next-pi
-- position): an active impact group (guard reads 1 in w) contributes its
-- pi slice `take (length inputs) (drop k (pis w))`; the pi-cursor advances
-- by `length inputs` for every impact (active or skipped — both advance
-- `next-pi` / the run's pis length), but only active groups feed the
-- pub-transcript-inputs stream.  (`mkPTO`/`mkPRV`/`mkInputs` below are
-- the companion pre-passes for the other preimage streams.)
------------------------------------------------------------------------

mkPTI : CircuitWitness → ℕ → List Instruction → List Fr
mkPTI w k [] = []
mkPTI w k (impact guard inputs ∷ is)
  with resolve𝔹 (CircuitWitness.assign w) guard
... | just true = take (length inputs) (drop k (CircuitWitness.pis w))
                    ++ mkPTI w (k + length inputs) is
... | just false = mkPTI w (k + length inputs) is
... | nothing    = mkPTI w (k + length inputs) is
mkPTI w k (_ ∷ is) = mkPTI w k is

-- On an active impact, the run's transcript check reads exactly `vals` from
-- the pub-transcript-inputs window (cursor invariant + `impact-slice`).
pti-check : ∀ {w k guard inputs is vals cur} {PTI : List Fr}
  → resolve𝔹 (CircuitWitness.assign w) guard ≡ just true
  → drop cur PTI ≡ mkPTI w k (impact guard inputs ∷ is)
  → take (length inputs) (drop k (CircuitWitness.pis w)) ≡ vals
  → take (length vals) (drop cur PTI) ≡ vals
pti-check {vals = vals} active ptiI slice
  rewrite active | ptiI | slice = take-len-++ vals _

-- The pub-transcript-inputs cursor advances by `length inputs` past an
-- active impact.  The slice is full-length under `satisfies` (it equals the
-- resolved `vals`, of length `length inputs`).
pti-adv : ∀ {w k guard inputs is cur} {PTI : List Fr}
  → resolve𝔹 (CircuitWitness.assign w) guard ≡ just true
  → drop cur PTI ≡ mkPTI w k (impact guard inputs ∷ is)
  → length (take (length inputs) (drop k (CircuitWitness.pis w))) ≡ length inputs
  → drop (cur + length inputs) PTI ≡ mkPTI w (k + length inputs) is
pti-adv {w} {k} {inputs = inputs} {is} {cur} {PTI} active ptiI slicefull
  rewrite active
  = trans (sym (drop-drop cur (length inputs) PTI))
          (trans (cong (drop (length inputs)) ptiI)
                 (subst (λ n → drop n (slice ++ tail) ≡ tail) slicefull
                        (drop-len-++ slice tail)))
  where slice = take (length inputs) (drop k (CircuitWitness.pis w))
        tail  = mkPTI w (k + length inputs) is

------------------------------------------------------------------------
-- The remaining w-only pre-passes: the transcript-output streams
-- (`mkPTO`/`mkPRV`) and the declared-input stream (`mkInputs`).  Each
-- fixes the corresponding `P'` list in-order from the witness.  `cellEnc w o` is the raw encoding of the witness's cell
-- `o` (under `WShape` the cell is defined, so this is `encode` of its
-- value; the default `[]` never fires).
------------------------------------------------------------------------

cellEnc : CircuitWitness → Identifier → List Fr
cellEnc w o = maybe′ encodeᵉ [] (CircuitWitness.assign w o)

-- pub-transcript-outputs: the encoding of every ACTIVE public-input cell,
-- in order (private-input and every other instruction contribute none).
mkPTO : CircuitWitness → List Instruction → List Fr
mkPTO w [] = []
mkPTO w (public-input guard val-t o ∷ is)
  with eval-guard (CircuitWitness.assign w) guard
... | just true  = cellEnc w o ++ mkPTO w is
... | just false = mkPTO w is
... | nothing    = mkPTO w is
mkPTO w (_ ∷ is) = mkPTO w is

-- priv-transcript: the encoding of every ACTIVE private-input cell.
mkPRV : CircuitWitness → List Instruction → List Fr
mkPRV w [] = []
mkPRV w (private-input guard val-t o ∷ is)
  with eval-guard (CircuitWitness.assign w) guard
... | just true  = cellEnc w o ++ mkPRV w is
... | just false = mkPRV w is
... | nothing    = mkPRV w is
mkPRV w (_ ∷ is) = mkPRV w is

-- inputs (the `decode-inputs` inverse): the encoding of every declared
-- input cell, in signature order.
mkInputs : CircuitWitness → List TypedIdentifier → List Fr
mkInputs w []         = []
mkInputs w (ti ∷ tis) =
  cellEnc w (TypedIdentifier.name ti) ++ mkInputs w tis


-- pub-transcript-outputs cursor: an active public-input reads back its
-- cell (`encode-len` + `encode-decode`) and the cursor advances by
-- `encoded-len val-t`.  Mirrors `pti-check`/`pti-adv`.
mkPTO-active : ∀ {w guard val-t o is v}
  → eval-guard (CircuitWitness.assign w) guard ≡ just true
  → CircuitWitness.assign w o ≡ just v
  → mkPTO w (public-input guard val-t o ∷ is) ≡ encodeᵉ v ++ mkPTO w is
mkPTO-active active wo rewrite active | wo = refl

pto-check : ∀ {w guard val-t o is v cur} {PTO : List Fr}
  → eval-guard (CircuitWitness.assign w) guard ≡ just true
  → CircuitWitness.assign w o ≡ just v → typeof v ≡ val-t
  → drop cur PTO ≡ mkPTO w (public-input guard val-t o ∷ is)
  → decode val-t (take (encoded-len val-t) (drop cur PTO)) ≡ just v
pto-check {w = w} {guard = guard} {val-t = val-t} {o = o} {is = is} {v = v}
  active wo tyv ptoI
  rewrite trans ptoI
            (mkPTO-active {w = w} {guard = guard} {val-t = val-t} {o = o}
                          {is = is} active wo)
        | sym tyv | sym (encode-len v)
        | take-len-++ (encodeᵉ v) (mkPTO w is) = encode-decode v

pto-adv : ∀ {w guard val-t o is v cur} {PTO : List Fr}
  → eval-guard (CircuitWitness.assign w) guard ≡ just true
  → CircuitWitness.assign w o ≡ just v → typeof v ≡ val-t
  → drop cur PTO ≡ mkPTO w (public-input guard val-t o ∷ is)
  → drop (cur + encoded-len val-t) PTO ≡ mkPTO w is
pto-adv {w = w} {guard = guard} {val-t = val-t} {o = o} {is = is} {v = v}
  {cur = cur} {PTO = PTO} active wo tyv ptoI
  rewrite sym tyv | sym (encode-len v)
  = trans (sym (drop-drop cur (length (encodeᵉ v)) PTO))
          (trans (cong (drop (length (encodeᵉ v)))
                       (trans ptoI
                         (mkPTO-active {w = w} {guard = guard} {val-t = val-t}
                                       {o = o} {is = is} active wo)))
                 (drop-len-++ (encodeᵉ v) (mkPTO w is)))

-- priv-transcript cursor (private-input): identical over `mkPRV`.
mkPRV-active : ∀ {w guard val-t o is v}
  → eval-guard (CircuitWitness.assign w) guard ≡ just true
  → CircuitWitness.assign w o ≡ just v
  → mkPRV w (private-input guard val-t o ∷ is) ≡ encodeᵉ v ++ mkPRV w is
mkPRV-active active wo rewrite active | wo = refl

prv-check : ∀ {w guard val-t o is v cur} {PRV : List Fr}
  → eval-guard (CircuitWitness.assign w) guard ≡ just true
  → CircuitWitness.assign w o ≡ just v → typeof v ≡ val-t
  → drop cur PRV ≡ mkPRV w (private-input guard val-t o ∷ is)
  → decode val-t (take (encoded-len val-t) (drop cur PRV)) ≡ just v
prv-check {w = w} {guard = guard} {val-t = val-t} {o = o} {is = is} {v = v}
  active wo tyv prvI
  rewrite trans prvI
            (mkPRV-active {w = w} {guard = guard} {val-t = val-t} {o = o}
                          {is = is} active wo)
        | sym tyv | sym (encode-len v)
        | take-len-++ (encodeᵉ v) (mkPRV w is) = encode-decode v

prv-adv : ∀ {w guard val-t o is v cur} {PRV : List Fr}
  → eval-guard (CircuitWitness.assign w) guard ≡ just true
  → CircuitWitness.assign w o ≡ just v → typeof v ≡ val-t
  → drop cur PRV ≡ mkPRV w (private-input guard val-t o ∷ is)
  → drop (cur + encoded-len val-t) PRV ≡ mkPRV w is
prv-adv {w = w} {guard = guard} {val-t = val-t} {o = o} {is = is} {v = v}
  {cur = cur} {PRV = PRV} active wo tyv prvI
  rewrite sym tyv | sym (encode-len v)
  = trans (sym (drop-drop cur (length (encodeᵉ v)) PRV))
          (trans (cong (drop (length (encodeᵉ v)))
                       (trans prvI
                         (mkPRV-active {w = w} {guard = guard} {val-t = val-t}
                                       {o = o} {is = is} active wo)))
                 (drop-len-++ (encodeᵉ v) (mkPRV w is)))

------------------------------------------------------------------------
-- `encode` agreement + the declared-input decode.
--
-- `insertMany` realises a `bind-each`: agreement on the pre-state plus
-- the witness carrying each cell's value at its position (exactly
-- `holds (encode-eq …)`) forces the multi-cell insert to SUCCEED (arity
-- from `bind-each` being inhabited) and preserves agreement — the list
-- generalisation of `ins-⊑ᵂ`, used by the `encode` `build` clause.
------------------------------------------------------------------------

insertMany-realize : ∀ {w} st ids vs
  → State.mem st ⊑ᵂ w → bind-each w vs ids
  → ∃ λ st′ → insertMany st ids vs ≡ just st′ × State.mem st′ ⊑ᵂ w
insertMany-realize st []         []       sub _          = st , refl , sub
insertMany-realize {w} st (id ∷ ids) (v ∷ vs) sub (wid , be) =
  insertMany-realize {w} (out1 st id v) ids vs (ins-⊑ᵂ {w = w} id sub wid) be
insertMany-realize st []         (_ ∷ _)  _ ()
insertMany-realize st (_ ∷ _)    []       _ ()

-- `holds (encode-eq input outputs)` yields the `bind-each` at the encoded
-- values, once the input operand is pinned to the witness value.
agree-encode : ∀ {m w a outputs v}
  → m ⊑ᵂ w → resolve m a ≡ just v
  → holds w (encode-eq a outputs)
  → bind-each w (map val-native (encodeᵉ v)) outputs
agree-encode {a = a} sub ra (v′ , wa , be)
  with trans (sym (⊑-resolve a sub ra)) wa
... | refl = be

-- `decode-inputs` inverts `mkInputs`: decoding the declared-input stream
-- built from the witness reproduces a memory agreeing with `w`, so
-- `init S P'` succeeds with `mem st0 ⊑ᵂ w`.  `WInputs` supplies each
-- cell's value + type; the recursion mirrors `pto-check`.
mkInputs-cons : ∀ {w ti tis v}
  → CircuitWitness.assign w (TypedIdentifier.name ti) ≡ just v
  → mkInputs w (ti ∷ tis) ≡ encodeᵉ v ++ mkInputs w tis
mkInputs-cons wo rewrite wo = refl

∅⊑ᵂ : ∀ {w} → ∅ ⊑ᵂ w
∅⊑ᵂ {w} ()

decode-mkInputs : ∀ {w} tis → WInputs w tis
  → ∃ λ m₀ → decode-inputs tis (mkInputs w tis) ≡ just m₀ × m₀ ⊑ᵂ w
decode-mkInputs {w} []         _ = ∅ , refl , ∅⊑ᵂ {w}
decode-mkInputs {w} (ti ∷ tis) ((v , wo , tyv) , wins)
  with decode-mkInputs {w} tis wins
... | m₀ , deq , sub
  rewrite mkInputs-cons {w = w} {ti} {tis} {v} wo
        | sym tyv | sym (encode-len v)
        | take-len-++ (encodeᵉ v) (mkInputs w tis)
        | drop-len-++ (encodeᵉ v) (mkInputs w tis)
        | encode-decode v
        | deq
  = ins (TypedIdentifier.name ti) v m₀
  , refl
  , ins-⊑ᵂ {w = w} (TypedIdentifier.name ti) sub wo

------------------------------------------------------------------------
-- Step-success for the transcript / pi / output cluster (the `*-step`
-- companions the `build` fold's impact / public-input / private-input /
-- circuit-output clauses use).  Each is a `rewrite … = refl` onto a
-- concrete record-update post-state; active/inactive are separate (the
-- post-states differ, exactly as the `*-bwd` batch-3 lemmas).
------------------------------------------------------------------------

impact-active-step : ∀ {P S st guard inputs vals}
  → resolve-all-Fr (State.mem st) inputs ≡ just vals
  → resolve𝔹 (State.mem st) guard ≡ just true
  → take (length vals)
      (drop (State.pti-idx st) (ProofPreimage.pub-transcript-inputs P)) ≡ vals
  → step P S st (impact guard inputs)
      ≡ just (record st { pis      = State.pis st ++ vals
                        ; pi-skips = State.pi-skips st ++ (nothing ∷ [])
                        ; pti-idx  = State.pti-idx st + length vals })
impact-active-step {P = P} {st = st} {vals = vals} rall rg chk
  rewrite rall | rg
  with take (length vals)
         (drop (State.pti-idx st) (ProofPreimage.pub-transcript-inputs P))
       ≟LFr vals
... | yes _  = refl
... | no ¬p  = case ¬p chk of λ ()

impact-inactive-step : ∀ {P S st guard inputs vals}
  → resolve-all-Fr (State.mem st) inputs ≡ just vals
  → resolve𝔹 (State.mem st) guard ≡ just false
  → step P S st (impact guard inputs)
      ≡ just (record st { pis      = State.pis st ++ map (λ _ → 0ᶠ) vals
                        ; pi-skips = State.pi-skips st
                                       ++ (just (length vals) ∷ []) })
impact-inactive-step rall rg rewrite rall | rg = refl

public-input-active-step : ∀ {P S st guard val-t output v}
  → eval-guard (State.mem st) guard ≡ just true
  → decode val-t (take (encoded-len val-t) (State.pto-rem st)) ≡ just v
  → step P S st (public-input guard val-t output)
      ≡ just (record st { mem     = ins output v (State.mem st)
                        ; pto-rem = drop (encoded-len val-t)
                                      (State.pto-rem st) })
public-input-active-step eg dec rewrite eg | dec = refl

public-input-inactive-step : ∀ {P S st guard val-t output}
  → eval-guard (State.mem st) guard ≡ just false
  → step P S st (public-input guard val-t output)
      ≡ just (out1 st output (default-val val-t))
public-input-inactive-step eg rewrite eg = refl

private-input-active-step : ∀ {P S st guard val-t output v}
  → eval-guard (State.mem st) guard ≡ just true
  → decode val-t (take (encoded-len val-t) (State.priv-rem st)) ≡ just v
  → step P S st (private-input guard val-t output)
      ≡ just (record st { mem      = ins output v (State.mem st)
                        ; priv-rem = drop (encoded-len val-t)
                                       (State.priv-rem st) })
private-input-active-step eg dec rewrite eg | dec = refl

private-input-inactive-step : ∀ {P S st guard val-t output}
  → eval-guard (State.mem st) guard ≡ just false
  → step P S st (private-input guard val-t output)
      ≡ just (out1 st output (default-val val-t))
private-input-inactive-step eg rewrite eg = refl

circuit-output-step : ∀ {P S st vals vs}
  → collectOutputs (State.mem st) (IrSource.outputs S) vals ≡ just vs
  → step P S st (circuit-output vals)
      ≡ just (record st { outs = State.outs st ++ vs })
circuit-output-step co rewrite co = refl

------------------------------------------------------------------------
-- `collectOutputs` transport (for the `circuit-output` `build` clause).
--
-- `circuit-output`'s operands carry no constraint, so `OpTy` was extended
-- to `AllDefdᵒ` (each operand defined); with agreement, `collectOutputs`
-- then reads the same at the run memory as at the witness, moving the
-- WShape success (`collectOutputs (assign w) …`) down to `mem st`.
------------------------------------------------------------------------

resolveᶜ≡resolveᵐ : ∀ w op
  → resolveᶜ w op ≡ resolve (CircuitWitness.assign w) op
resolveᶜ≡resolveᵐ w (var id) = refl
resolveᶜ≡resolveᵐ w (imm x)  = refl

collectOutputs-agree : ∀ {m w Γ} ts ops → TyEq m Γ → m ⊑ᵂ w → AllDefdᵒ Γ ops
  → collectOutputs (CircuitWitness.assign w) ts ops ≡ collectOutputs m ts ops
collectOutputs-agree []       []         teq agr _ = refl
collectOutputs-agree []       (op ∷ ops) teq agr _ = refl
collectOutputs-agree (t ∷ ts) []         teq agr _ = refl
collectOutputs-agree {m} {w} (t ∷ ts) (op ∷ ops) teq agr ((t' , ot') ∷ adr)
  with optype-resolve {op = op} teq ot'
... | (v₀ , r₀)
  rewrite r₀
        | trans (sym (resolveᶜ≡resolveᵐ w op)) (⊑-resolve {m} {w} op agr r₀)
  = cong (λ z → guardD (typeof v₀ ≟T t) (z >>= λ vs′ → just (v₀ ∷ vs′)))
      (collectOutputs-agree {m} {w} ts ops teq agr adr)

------------------------------------------------------------------------
-- The witness-reconstruction fold `build`.
--
-- From a satisfying witness `w` and the static well-formedness threads
-- (TyEq/OpTy/SA/WT), `build` reconstructs the forward `run` of a
-- synthesised preimage `P'` on `w`, maintaining the memory-agreement
-- invariant `mem st ⊑ᵂ w`, the pis-prefix invariant, and the three
-- transcript-cursor invariants (`mkPTI`/`mkPTO`/`mkPRV`).  Each clause
-- peels its constraint (`csOf-peel`), reads the output cell(s) off `w`
-- (`agree-*`), runs the deterministic step (`*-step`), and re-establishes
-- agreement (`ins-⊑ᵂ`/`insertMany-realize`) before recursing.
------------------------------------------------------------------------

record BuildOut {S w} (P' : ProofPreimage) (st : State)
                (is : List Instruction) : Set where
  constructor mk-build
  field
    fs    : State
    frun  : run P' S st is ≡ just fs
    fagr  : State.mem fs ⊑ᵂ w
    fpis  : State.pis fs ≡ take (length (State.pis fs)) (CircuitWitness.pis w)
    flen  : State.pti-idx fs
              ≡ length (ProofPreimage.pub-transcript-inputs P')
    fpto  : State.pto-rem fs ≡ []
    fprv  : State.priv-rem fs ≡ []

resolveᶠ-nat : ∀ {m a x} → resolve m a ≡ just (val-native x)
  → resolveᶠ m a ≡ just x
resolveᶠ-nat ra rewrite ra = refl


-- A boolean wire (`is-bit x`) reads to some `to𝔹` bit — used by
-- `constrain-to-boolean`, where the reading is the only step precondition.
is-bit→to𝔹 : ∀ {x} → is-bit x → ∃ λ b → to𝔹 x ≡ just b
is-bit→to𝔹 (inj₁ x≡0) = false , to𝔹-0 x≡0
is-bit→to𝔹 (inj₂ x≡1) = true  , to𝔹-1 x≡1

take-take-++ : ∀ (m n : ℕ) (xs : List Fr)
  → take m xs ++ take n (drop m xs) ≡ take (m + n) xs
take-take-++ zero    n xs       = refl
take-take-++ (suc m) n (x ∷ xs) = cong (x ∷_) (take-take-++ m n xs)
take-take-++ (suc m) n []       = tnil n
  where tnil : ∀ k → take k [] ≡ []
        tnil zero    = refl
        tnil (suc _) = refl

-- The Circuit `resolveᶜ-Fr` and Semantics `resolveᶠ (assign w)` agree, but
-- not definitionally (distinct inline native-filters); case the value.
resolveᶜ-Fr≡resolveᶠᵐ : ∀ w op
  → resolveᶜ-Fr w op ≡ resolveᶠ (CircuitWitness.assign w) op
resolveᶜ-Fr≡resolveᶠᵐ w (var id) with CircuitWitness.assign w id
... | nothing                    = refl
... | just (val-native _)        = refl
... | just (val-bytes32 _)       = refl
... | just (val-jubjub-point _)  = refl
... | just (val-jubjub-scalar _) = refl
... | just (val-secp-point _)    = refl
... | just (val-secp-base _)     = refl
... | just (val-secp-scalar _)   = refl
resolveᶜ-Fr≡resolveᶠᵐ w (imm x)  = refl

-- The inactive companion of `impact-slice`: on a skipped group (guard 0)
-- the witness's pi entries are all zeros (`g ≡ 0ᶠ → pv ≡ 0ᶠ`).
impact-slice-0 : ∀ w start guard inputs {vals}
  → resolveᶜ-all-Fr w inputs ≡ just vals
  → resolveᶜ-Fr w guard ≡ just 0ᶠ
  → satisfies-constraints (impact-constraints start guard inputs) w
  → take (length inputs) (drop start (CircuitWitness.pis w))
      ≡ map (λ _ → 0ᶠ) vals
impact-slice-0 w start guard []       refl rg _ = refl
impact-slice-0 w start guard (x ∷ xs) rall rg
  ((g , xv , pv , wg , wx , _ , wpv , _ , g0→) , rest)
  with refl ← trans (sym rg) wg
  with resolveᶜ-Fr w x | wx
... | .(just xv) | refl
  with resolveᶜ-all-Fr w xs in rxs
...   | just vs
  with refl ← rall
  rewrite drop-lookup (CircuitWitness.pis w) start wpv | g0→ refl
        | impact-slice-0 w (suc start) guard xs rxs rg rest = refl

-- mkPTI on a skipped impact: the pub-transcript-inputs stream is unchanged
-- (only the pi-cursor advances by `length inputs`).
mkPTI-false : ∀ {w guard inputs is} k
  → resolve𝔹 (CircuitWitness.assign w) guard ≡ just false
  → mkPTI w k (impact guard inputs ∷ is) ≡ mkPTI w (k + length inputs) is
mkPTI-false k active rewrite active = refl

-- mkPTI on an active impact: the pub-transcript-inputs stream prepends the
-- pi window `vals` before advancing the cursor.
mkPTI-true : ∀ {w guard inputs is} k
  → resolve𝔹 (CircuitWitness.assign w) guard ≡ just true
  → mkPTI w k (impact guard inputs ∷ is)
      ≡ take (length inputs) (drop k (CircuitWitness.pis w))
          ++ mkPTI w (k + length inputs) is
mkPTI-true k active rewrite active = refl

-- The run's guard reading agrees with the witness's (used by public/
-- private-input).  `nothing` guard is `just true` both sides; a present
-- guard resolves natively (from `GuardNat`) and transports via the
-- `resolveᶜ-Fr`/`resolveᶠ` bridge + `to𝔹`.
eval-guard-agree : ∀ {m w Γ} guard {b} → m ⊑ᵂ w → TyEq m Γ → GuardNat Γ guard
  → eval-guard (CircuitWitness.assign w) guard ≡ just b
  → eval-guard m guard ≡ just b
eval-guard-agree nothing        agr teq _     e = e
eval-guard-agree {m} {w} (just op) agr teq ognat e =
  let (x , rgf) = optype-resolveᶠ {m} {op = op} teq ognat
      rafw = trans (sym (resolveᶜ-Fr≡resolveᶠᵐ w op))
                   (⊑-resolveᶠ {m} {w} op agr rgf)
      tbx  = trans (sym (cong (_>>= to𝔹) rafw)) e
  in trans (cong (_>>= to𝔹) rgf) tbx

-- mkPTO/mkPRV on a skipped input (guard reads false): the transcript-output
-- stream is unchanged.
mkPTO-false : ∀ {w guard val-t o is}
  → eval-guard (CircuitWitness.assign w) guard ≡ just false
  → mkPTO w (public-input guard val-t o ∷ is) ≡ mkPTO w is
mkPTO-false e rewrite e = refl

mkPRV-false : ∀ {w guard val-t o is}
  → eval-guard (CircuitWitness.assign w) guard ≡ just false
  → mkPRV w (private-input guard val-t o ∷ is) ≡ mkPRV w is
mkPRV-false e rewrite e = refl

insertMany-shape : ∀ st ids vs {st'} → insertMany st ids vs ≡ just st'
  → (State.pis st' ≡ State.pis st) × (State.pti-idx st' ≡ State.pti-idx st)
  × (State.pto-rem st' ≡ State.pto-rem st)
  × (State.priv-rem st' ≡ State.priv-rem st)
insertMany-shape st []         []       refl = refl , refl , refl , refl
insertMany-shape st (id ∷ ids) (v ∷ vs) e =
  insertMany-shape (out1 st id v) ids vs e
insertMany-shape st []         (_ ∷ _)  ()
insertMany-shape st (_ ∷ _)    []       ()

-- `from-bytes32` packages the run's output value, its step equation, and the
-- witness agreement uniformly, so `build` need not case-split on `val-t`.
-- `WCell`'s `typeof v ≡ val-t` selects the matching `from-bytes` disjunct
-- (supported `val-t`) or refutes an unsupported `val-t`.
from-bytes32-realize : ∀ {P' S w st bytes val-t o bs}
  → State.mem st ⊑ᵂ w
  → resolve (State.mem st) bytes ≡ just (val-bytes32 bs)
  → holds w (from-bytes o bytes)
  → WCell w val-t o true
  → ∃ λ v → step P' S st (from-bytes32 bytes val-t o) ≡ just (out1 st o v)
          × CircuitWitness.assign w o ≡ just v
from-bytes32-realize {P' = P'} {S} {w} {st} {bytes} {val-t = native} {o} {bs}
  sub rb hc (v , wo , tv) =
    val-native (nativeFromBytes bs)
  , from-bytes32-step {P'} {S} {st} {bytes} {o} {bs} rb
  , agree-from-bytes-native {State.mem st} {w} {bytes} {o} {bs} {v} sub rb hc wo tv
from-bytes32-realize {P' = P'} {S} {w} {st} {bytes} {val-t = secp-base} {o} {bs}
  sub rb hc (v , wo , tv) =
    val-secp-base (secpBaseFromBytes bs)
  , from-bytes32-secp-base-step {P'} {S} {st} {bytes} {o} {bs} rb
  , agree-from-bytes-secp-base {State.mem st} {w} {bytes} {o} {bs} {v}
      sub rb hc wo tv
from-bytes32-realize {P' = P'} {S} {w} {st} {bytes} {val-t = secp-scalar} {o} {bs}
  sub rb hc (v , wo , tv) =
    val-secp-scalar (secpScalarFromBytes bs)
  , from-bytes32-secp-scalar-step {P'} {S} {st} {bytes} {o} {bs} rb
  , agree-from-bytes-secp-scalar {State.mem st} {w} {bytes} {o} {bs} {v}
      sub rb hc wo tv
from-bytes32-realize {w = w} {bytes = bytes} {val-t = bytes32} {o = o}
  sub rb hc (v , wo , tv) =
  case from-bytes-out-typeof {w} {o} {bytes} {v} hc wo of λ
    { (inj₁ tn)        → case trans (sym tn) tv of λ ()
    ; (inj₂ (inj₁ tb)) → case trans (sym tb) tv of λ ()
    ; (inj₂ (inj₂ ts)) → case trans (sym ts) tv of λ () }
from-bytes32-realize {w = w} {bytes = bytes} {val-t = jubjub-point} {o = o}
  sub rb hc (v , wo , tv) =
  case from-bytes-out-typeof {w} {o} {bytes} {v} hc wo of λ
    { (inj₁ tn)        → case trans (sym tn) tv of λ ()
    ; (inj₂ (inj₁ tb)) → case trans (sym tb) tv of λ ()
    ; (inj₂ (inj₂ ts)) → case trans (sym ts) tv of λ () }
from-bytes32-realize {w = w} {bytes = bytes} {val-t = jubjub-scalar} {o = o}
  sub rb hc (v , wo , tv) =
  case from-bytes-out-typeof {w} {o} {bytes} {v} hc wo of λ
    { (inj₁ tn)        → case trans (sym tn) tv of λ ()
    ; (inj₂ (inj₁ tb)) → case trans (sym tb) tv of λ ()
    ; (inj₂ (inj₂ ts)) → case trans (sym ts) tv of λ () }
from-bytes32-realize {w = w} {bytes = bytes} {val-t = secp-point} {o = o}
  sub rb hc (v , wo , tv) =
  case from-bytes-out-typeof {w} {o} {bytes} {v} hc wo of λ
    { (inj₁ tn)        → case trans (sym tn) tv of λ ()
    ; (inj₂ (inj₁ tb)) → case trans (sym tb) tv of λ ()
    ; (inj₂ (inj₂ ts)) → case trans (sym ts) tv of λ () }

------------------------------------------------------------------------
-- Two helpers shared by every non-nil `build` clause.  `cons-build`
-- prepends one reconstructed step to a tail `BuildOut`: the run grows by
-- `run-cons`, every other component is copied verbatim.  `adv3` advances
-- the static well-formedness thread (the domain equation, the typing
-- invariant, and the pi-count link) from the pre-state to the
-- post-instruction state, uniformly for every instruction.
------------------------------------------------------------------------

cons-build : ∀ {S w} {P' : ProofPreimage} {st st' : State} {i is}
  → step P' S st i ≡ just st'
  → BuildOut {S} {w} P' st' is
  → BuildOut {S} {w} P' st (i ∷ is)
cons-build {i = i} {is} steq rec =
  mk-build (BuildOut.fs rec) (run-cons i is steq (BuildOut.frun rec))
    (BuildOut.fagr rec) (BuildOut.fpis rec) (BuildOut.flen rec)
    (BuildOut.fpto rec) (BuildOut.fprv rec)

-- `P`/`S`/`st`/`ss` are explicit: they occur only in non-injective
-- positions (`step P S st i` reduces on a concrete `i`; `State.mem st`,
-- `State.pis st`, `SynthState.next-pi ss` are projections), so they must be
-- supplied, exactly as the per-clause `step-ty`/`pi-inv-step` calls used to
-- pin `P`/`S` and pass `ss`.  `st'`/`Γ`/`Γ'`/`bound` stay inferred (they sit
-- at injective positions of `steq`/`oty`/`dΓ`).
adv3 : ∀ {st' Γ Γ' bound} (P : ProofPreimage) (S : IrSource) (st : State)
       (ss : SynthState) (i : Instruction)
  → outtys Γ i ≡ just Γ'
  → dom-ty Γ ≡ bound
  → All (λ o → ¬ (o ∈ bound)) (outs-of i)
  → NoDup (outs-of i)
  → TyEq (State.mem st) Γ
  → SynthState.next-pi ss ≡ length (State.pis st)
  → step P S st i ≡ just st'
  → (dom-ty Γ' ≡ bound ++ outs-of i)
  × TyEq (State.mem st') Γ'
  × (SynthState.next-pi (synth-instr i ss) ≡ length (State.pis st'))
adv3 {Γ = Γ} P S st ss i oty dΓ af nd teq npi steq =
    trans (outtys-dom Γ i oty) (cong (_++ outs-of i) dΓ)
  , step-ty {P} {S} i oty steq teq (all∉-cast (outs-of i) dΓ af) nd
  , pi-inv-step {P} {S} i ss steq npi

build : ∀ {S w} (P' : ProofPreimage) is st Γ bound ss
  → dom-ty Γ ≡ bound → SA bound is → WT Γ is
  → TyEq (State.mem st) Γ
  → State.mem st ⊑ᵂ w
  → satisfies-constraints (csOf is ss) w
  → SynthState.next-pi ss ≡ length (State.pis st)
  → State.pis st ≡ take (length (State.pis st)) (CircuitWitness.pis w)
  → drop (State.pti-idx st) (ProofPreimage.pub-transcript-inputs P')
      ≡ mkPTI w (length (State.pis st)) is
  → State.pto-rem st ≡ mkPTO w is
  → State.priv-rem st ≡ mkPRV w is
  → State.pti-idx st + length (mkPTI w (length (State.pis st)) is)
      ≡ length (ProofPreimage.pub-transcript-inputs P')
  → WSteps S w is
  → BuildOut {S} {w} P' st is
build P' [] st Γ bound ss dΓ sa wt teq agr csat npi pisP ptiI ptoI prvI
  alen ws =
  mk-build st refl agr pisP
    (trans (sym (+-identityʳ (State.pti-idx st))) alen) ptoI prvI

-- mul: split on the FM disjunction (native / secp-base / secp-scalar).
build {S} {w} P' (mul a b output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (t , oa , ob , fm) , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws with fm
... | inj₁ refl =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {native} teq oa
      (x , ra)   = mul-support-l {State.mem st} {Γ} {a} {av} teq oa rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {native} teq ob
      (y , rb)   = mul-support-l {State.mem st} {Γ} {b} {bv} teq ob rbv
      (hc , tc)  = csOf-peel (mul a b output) is ss csat
      wout = agree-mul {a = a} {b} {out = output} agr
               (resolveᶠ-nat {State.mem st} {a} ra)
               (resolveᶠ-nat {State.mem st} {b} rb) (proj₁ hc)
      steq = mul-step {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (mul a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-native (x *ᶠ y)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (mul a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₁ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-base} teq oa
      (x , ra)   = secp-base-support {State.mem st} {Γ} {a} {av} teq oa rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {secp-base} teq ob
      (y , rb)   = secp-base-support {State.mem st} {Γ} {b} {bv} teq ob rbv
      (hc , tc)  = csOf-peel (mul a b output) is ss csat
      wout = agree-mul-sb {a = a} {b} {out = output} agr ra rb (proj₁ hc)
      steq = mul-step-sb {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (mul a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-base (x *ᵇ y)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (mul a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-scalar} teq oa
      (x , ra)   = secp-scalar-support {State.mem st} {Γ} {a} {av} teq oa rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {secp-scalar} teq ob
      (y , rb)   = secp-scalar-support {State.mem st} {Γ} {b} {bv} teq ob rbv
      (hc , tc)  = csOf-peel (mul a b output) is ss csat
      wout = agree-mul-ss {a = a} {b} {out = output} agr ra rb (proj₁ hc)
      steq = mul-step-ss {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (mul a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-scalar (x *ˢ y)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (mul a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- add: split on the FP disjunction (native vs jubjub-point).
build {S} {w} P' (add a b output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (t , ota , otb , fp) , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws with fp
... | inj₁ refl =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {native} teq ota
      (x , ra)   = mul-support-l {State.mem st} {Γ} {a} {av} teq ota rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {native} teq otb
      (y , rb)   = mul-support-l {State.mem st} {Γ} {b} {bv} teq otb rbv
      (hc , tc)  = csOf-peel (add a b output) is ss csat
      wout = agree-add-n {a = a} {b} {out = output} agr ra rb (proj₁ hc)
      steq = add-step-n {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (add a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-native (x +ᶠ y)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (add a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₁ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {jubjub-point} teq ota
      (p , ra)   = point-support {State.mem st} {Γ} {a} {av} teq ota rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {jubjub-point} teq otb
      (q , rb)   = point-support {State.mem st} {Γ} {b} {bv} teq otb rbv
      (hc , tc)  = csOf-peel (add a b output) is ss csat
      wout = agree-add-p {a = a} {b} {out = output} agr ra rb (proj₁ hc)
      steq = add-step-p {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (add a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-jubjub-point (p +J q)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (add a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ (inj₁ refl)) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-point} teq ota
      (p , ra)   = secp-point-support {State.mem st} {Γ} {a} {av} teq ota rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {secp-point} teq otb
      (q , rb)   = secp-point-support {State.mem st} {Γ} {b} {bv} teq otb rbv
      (hc , tc)  = csOf-peel (add a b output) is ss csat
      wout = agree-add-sp {a = a} {b} {out = output} agr ra rb (proj₁ hc)
      steq = add-step-sp {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (add a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-point (p +K q)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (add a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ (inj₂ (inj₁ refl))) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-base} teq ota
      (x , ra)   = secp-base-support {State.mem st} {Γ} {a} {av} teq ota rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {secp-base} teq otb
      (y , rb)   = secp-base-support {State.mem st} {Γ} {b} {bv} teq otb rbv
      (hc , tc)  = csOf-peel (add a b output) is ss csat
      wout = agree-add-sb {a = a} {b} {out = output} agr ra rb (proj₁ hc)
      steq = add-step-sb {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (add a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-base (x +ᵇ y)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (add a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ (inj₂ (inj₂ refl))) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-scalar} teq ota
      (x , ra)   = secp-scalar-support {State.mem st} {Γ} {a} {av} teq ota rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {secp-scalar} teq otb
      (y , rb)   = secp-scalar-support {State.mem st} {Γ} {b} {bv} teq otb rbv
      (hc , tc)  = csOf-peel (add a b output) is ss csat
      wout = agree-add-ss {a = a} {b} {out = output} agr ra rb (proj₁ hc)
      steq = add-step-ss {P'} {S} {st} {a} {b} {output} ra rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (add a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-scalar (x +ˢ y)))
               Γ' (bound ++ (output ∷ [])) (synth-instr (add a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- inv: partial-op, split on FM (native / secp-base / secp-scalar).  Each
-- `agree-inv*` returns the Σ with the field/subgroup inverse equation.
build {S} {w} P' (inv a output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (t , oa , fm) , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws
  with fm
... | inj₁ refl =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {native} teq oa
      (x , ra)   = mul-support-l {State.mem st} {Γ} {a} {av} teq oa rav
      (hc , tc)  = csOf-peel (inv a output) is ss csat
      (xi , invEq , wout) = agree-inv {a = a} {out = output} agr
                              (resolveᶠ-nat {State.mem st} {a} ra) (proj₁ hc)
      steq = inv-step {P'} {S} {st} {a} {output} ra invEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (inv a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-native xi))
               Γ' (bound ++ (output ∷ [])) (synth-instr (inv a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₁ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-base} teq oa
      (x , ra)   = secp-base-support {State.mem st} {Γ} {a} {av} teq oa rav
      (hc , tc)  = csOf-peel (inv a output) is ss csat
      (xi , invEq , wout) = agree-inv-sb {a = a} {out = output} agr ra (proj₁ hc)
      steq = inv-step-sb {P'} {S} {st} {a} {output} ra invEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (inv a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-base xi))
               Γ' (bound ++ (output ∷ [])) (synth-instr (inv a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-scalar} teq oa
      (x , ra)   = secp-scalar-support {State.mem st} {Γ} {a} {av} teq oa rav
      (hc , tc)  = csOf-peel (inv a output) is ss csat
      (xi , invEq , wout) = agree-inv-ss {a = a} {out = output} agr ra (proj₁ hc)
      steq = inv-step-ss {P'} {S} {st} {a} {output} ra invEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (inv a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-scalar xi))
               Γ' (bound ++ (output ∷ [])) (synth-instr (inv a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- constrain-bits: no output (st' = st); the bound comes from `in-range`.
build {S} {w} P' (constrain-bits val bits ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , ov , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws =
  let (x , raf) = optype-resolveᶠ {State.mem st} {Γ} {val} teq ov
      (hc , tc) = csOf-peel (constrain-bits val bits) is ss csat
      (x' , wval , bnd) = proj₁ hc
      steq = constrain-bits-step {P'} {S} {st} {val} {bits} {x} raf
               (subst (λ z → valFr z < 2 ^ bits)
                 (sym (just-injective
                   (trans (sym (⊑-resolveᶠ {State.mem st} {w} val agr raf)) wval)))
                 bnd)
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (constrain-bits val bits)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is st
               Γ' (bound ++ outs-of (constrain-bits val bits))
               (synth-instr (constrain-bits val bits) ss)
               dΓ' sa' wt' teq' agr tc npi'
               pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- copy: OpDefd operand (any type), agree-copy returns the cell directly.
build {S} {w} P' (copy val output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (t , ov) , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws =
  let (v , rv)  = optype-resolve {State.mem st} {Γ} {val} {t} teq ov
      (hc , tc) = csOf-peel (copy val output) is ss csat
      wout = agree-copy {a = val} {out = output} agr rv (proj₁ hc)
      steq = copy-step {P'} {S} {st} {val} {output} rv
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (copy val output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output v)
               Γ' (bound ++ outs-of (copy val output))
               (synth-instr (copy val output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- hash-to-curve: all-native input list (allNat-resolve + ⊑-resolve-all).
build {S} {w} P' (hash-to-curve inputs output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , oinp , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (frs , rall) = allNat-resolve {State.mem st} {Γ} inputs teq oinp
      (hc , tc) = csOf-peel (hash-to-curve inputs output) is ss csat
      wout = agree-h2c {inputs = inputs} {out = output} agr rall (proj₁ hc)
      steq = hash-to-curve-step {P'} {S} {st} {inputs} {output} rall
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (hash-to-curve inputs output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 st output (val-jubjub-point (hash-to-curve-fn frs)))
               Γ' (bound ++ outs-of (hash-to-curve inputs output))
               (synth-instr (hash-to-curve inputs output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- not: boolean-reading Σ (agree-not gives the bit + the output cell).
build {S} {w} P' (not a output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , oa , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws =
  let (x , raf) = optype-resolveᶠ {State.mem st} {Γ} {a} teq oa
      (hc , tc) = csOf-peel (not a output) is ss csat
      (b , r𝔹 , wout) = agree-not {a = a} {out = output} agr raf (proj₁ hc)
      steq = not-step {P'} {S} {st} {a} {output} {b} r𝔹
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (not a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-native (χˢ (bnot b))))
               Γ' (bound ++ outs-of (not a output))
               (synth-instr (not a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- assert: no output; the true-reading comes from the WShape condition.
build {S} {w} P' (assert cond ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , oc , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ((x , wcond , tb) , ws') =
  let (x₀ , raf) = optype-resolveᶠ {State.mem st} {Γ} {cond} teq oc
      (hc , tc) = csOf-peel (assert cond) is ss csat
      x₀≡x = just-injective
               (trans (sym (⊑-resolveᶠ {State.mem st} {w} cond agr raf)) wcond)
      steq = assert-step {P'} {S} {st} {cond}
               (trans (cong (_>>= to𝔹) raf)
                 (subst (λ z → to𝔹 z ≡ just true) (sym x₀≡x) tb))
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (assert cond)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is st
               Γ' (bound ++ outs-of (assert cond))
               (synth-instr (assert cond) ss)
               dΓ' sa' wt' teq' agr tc npi'
               pisP ptiI ptoI prvI alen ws'
  in cons-build steq rec

-- encode: list output via `insertMany-realize` (agreement) + `insertMany-
-- shape` (transport the pis/cursor invariants, since insertMany changes
-- only mem, PROPOSITIONALLY not definitionally).
build {S} {w} P' (encode input outputs ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (t , oinp) , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws =
  let (v , rv)  = optype-resolve {State.mem st} {Γ} {input} {t} teq oinp
      (hc , tc) = csOf-peel (encode input outputs) is ss csat
      be = agree-encode {a = input} {outputs = outputs} agr rv (proj₁ hc)
      (st' , im , agr') =
        insertMany-realize {w} st outputs (map val-native (encodeᵉ v)) agr be
      steq = encode-step {P'} {S} {st} {input} {outputs} {v} {st'} rv im
      (peq , pteq , poeq , preq) =
        insertMany-shape st outputs (map val-native (encodeᵉ v)) im
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (encode input outputs)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is st' Γ'
              (bound ++ outs-of (encode input outputs))
              (synth-instr (encode input outputs) ss)
              dΓ' sa' wt' teq' agr' tc npi'
              (trans peq (trans pisP
                (cong (λ n → take n (CircuitWitness.pis w))
                  (cong length (sym peq)))))
              (trans (cong (λ n → drop n
                             (ProofPreimage.pub-transcript-inputs P')) pteq)
                     (trans ptiI
                       (cong (λ n → mkPTI w n is) (cong length (sym peq)))))
              (trans poeq ptoI) (trans preq prvI)
              (subst₂ (λ p q → p + length (mkPTI w q is)
                        ≡ length (ProofPreimage.pub-transcript-inputs P'))
                (sym pteq) (sym (cong length peq)) alen) ws
  in cons-build steq rec

-- impact: the transcript cursor pattern (active/inactive on the guard bit).
build {S} {w} P' (impact guard inputs ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (og , oinp) , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ((gᶠw , wguard , (gb , tb)) , ws')
  with optype-resolveᶠ {State.mem st} {Γ} {guard} teq og
     | allNat-resolve {State.mem st} {Γ} inputs teq oinp
     | csOf-peel (impact guard inputs) is ss csat
     | gb
... | (gᶠ , rgf) | (vals , rall) | (hc , tc) | true =
  let k        = length (State.pis st)
      PTI      = ProofPreimage.pub-transcript-inputs P'
      gᶠ≡      = just-injective (trans (sym (⊑-resolveᶠ {State.mem st} {w} guard agr rgf))
                                 wguard)
      tbf      : to𝔹 gᶠ ≡ just true
      tbf      = subst (λ z → to𝔹 z ≡ just true) (sym gᶠ≡) tb
      gᶠ≡1     = to𝔹-true tbf
      r𝔹mem    : resolve𝔹 (State.mem st) guard ≡ just true
      r𝔹mem    = trans (cong (_>>= to𝔹) rgf) tbf
      r𝔹w      : resolve𝔹 (CircuitWitness.assign w) guard ≡ just true
      r𝔹w      = trans (cong (_>>= to𝔹)
                   (trans (sym (resolveᶜ-Fr≡resolveᶠᵐ w guard)) wguard)) tb
      rᶜg1     : resolveᶜ-Fr w guard ≡ just 1ᶠ
      rᶜg1     = trans (⊑-resolveᶠ {State.mem st} {w} guard agr rgf)
                       (cong just gᶠ≡1)
      rᶜall    = ⊑-resolve-all inputs agr rall
      hc'      = subst (λ κ → satisfies-constraints
                          (impact-constraints κ guard inputs) w) npi hc
      slice    = impact-slice w k guard inputs rᶜall rᶜg1 hc'
      lenvals  : length vals ≡ length inputs
      lenvals  = resolve-all-Fr-length (State.mem st) inputs rall
      chk      = pti-check {w} {k} {guard} {inputs} {is} {vals}
                   {State.pti-idx st} {PTI} r𝔹w ptiI slice
      steq     = impact-active-step {P'} {S} {st} {guard} {inputs} {vals}
                   rall r𝔹mem chk
      lenpis'  : length (State.pis st ++ vals) ≡ k + length inputs
      lenpis'  = trans (length-++ (State.pis st)) (cong (k +_) lenvals)
      slicefull : length (take (length inputs) (drop k (CircuitWitness.pis w)))
                    ≡ length inputs
      slicefull = trans (cong length slice) lenvals
      pa       = pti-adv {w} {k} {guard} {inputs} {is} {State.pti-idx st} {PTI}
                   r𝔹w ptiI slicefull
      alen'    : State.pti-idx st + length vals
                   + length (mkPTI w (length (State.pis st ++ vals)) is)
                   ≡ length PTI
      alen'    = subst (λ n → State.pti-idx st + length vals
                                + length (mkPTI w n is) ≡ length PTI)
                   (sym lenpis')
                   (trans (+-assoc (State.pti-idx st) (length vals)
                             (length (mkPTI w (k + length inputs) is)))
                     (subst (λ z → State.pti-idx st
                              + (z + length (mkPTI w (k + length inputs) is))
                              ≡ length PTI)
                       (trans slicefull (sym lenvals))
                       (subst (λ z → State.pti-idx st + z ≡ length PTI)
                         (length-++ (take (length inputs)
                                      (drop k (CircuitWitness.pis w))))
                         (subst (λ z → State.pti-idx st + length z ≡ length PTI)
                           (mkPTI-true {w} {guard} {inputs} {is} k r𝔹w)
                           alen))))
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (impact guard inputs)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is
              (record st { pis      = State.pis st ++ vals
                         ; pi-skips = State.pi-skips st ++ (nothing ∷ [])
                         ; pti-idx  = State.pti-idx st + length vals })
              Γ' (bound ++ outs-of (impact guard inputs))
              (synth-instr (impact guard inputs) ss)
              dΓ' sa' wt' teq' agr tc npi'
              (trans (cong₂ _++_ pisP (sym slice))
                     (trans (take-take-++ k (length inputs) (CircuitWitness.pis w))
                            (cong (λ n → take n (CircuitWitness.pis w))
                              (sym lenpis'))))
              (trans (cong (λ n → drop (State.pti-idx st + n) PTI) lenvals)
                     (trans pa
                       (sym (cong (λ n → mkPTI w n is) lenpis'))))
              ptoI prvI alen' ws'
  in cons-build steq rec
... | (gᶠ , rgf) | (vals , rall) | (hc , tc) | false =
  let k        = length (State.pis st)
      PTI      = ProofPreimage.pub-transcript-inputs P'
      gᶠ≡      = just-injective (trans (sym (⊑-resolveᶠ {State.mem st} {w} guard agr rgf))
                                 wguard)
      tbf      : to𝔹 gᶠ ≡ just false
      tbf      = subst (λ z → to𝔹 z ≡ just false) (sym gᶠ≡) tb
      gᶠ≡0     = to𝔹-false tbf
      r𝔹mem    : resolve𝔹 (State.mem st) guard ≡ just false
      r𝔹mem    = trans (cong (_>>= to𝔹) rgf) tbf
      r𝔹w      : resolve𝔹 (CircuitWitness.assign w) guard ≡ just false
      r𝔹w      = trans (cong (_>>= to𝔹)
                   (trans (sym (resolveᶜ-Fr≡resolveᶠᵐ w guard)) wguard)) tb
      rᶜg0     : resolveᶜ-Fr w guard ≡ just 0ᶠ
      rᶜg0     = trans (⊑-resolveᶠ {State.mem st} {w} guard agr rgf)
                       (cong just gᶠ≡0)
      rᶜall    = ⊑-resolve-all inputs agr rall
      hc'      = subst (λ κ → satisfies-constraints
                          (impact-constraints κ guard inputs) w) npi hc
      slice0   = impact-slice-0 w k guard inputs rᶜall rᶜg0 hc'
      lenvals  : length vals ≡ length inputs
      lenvals  = resolve-all-Fr-length (State.mem st) inputs rall
      steq     = impact-inactive-step {P'} {S} {st} {guard} {inputs} {vals}
                   rall r𝔹mem
      lenpis'  : length (State.pis st ++ map (λ _ → 0ᶠ) vals) ≡ k + length inputs
      lenpis'  = trans (length-++ (State.pis st))
                   (cong (k +_) (trans (length-map (λ _ → 0ᶠ) vals) lenvals))
      alen'    : State.pti-idx st
                   + length (mkPTI w (length (State.pis st
                                                ++ map (λ _ → 0ᶠ) vals)) is)
                   ≡ length PTI
      alen'    = subst (λ n → State.pti-idx st + length (mkPTI w n is)
                                ≡ length PTI)
                   (sym lenpis')
                   (subst (λ z → State.pti-idx st + length z ≡ length PTI)
                     (mkPTI-false {w} {guard} {inputs} {is} k r𝔹w) alen)
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (impact guard inputs)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is
              (record st { pis      = State.pis st ++ map (λ _ → 0ᶠ) vals
                         ; pi-skips = State.pi-skips st
                                        ++ (just (length vals) ∷ []) })
              Γ' (bound ++ outs-of (impact guard inputs))
              (synth-instr (impact guard inputs) ss)
              dΓ' sa' wt' teq' agr tc npi'
              (trans (cong₂ _++_ pisP (sym slice0))
                     (trans (take-take-++ k (length inputs) (CircuitWitness.pis w))
                            (cong (λ n → take n (CircuitWitness.pis w))
                              (sym lenpis'))))
              (trans (trans ptiI
                        (mkPTI-false {w} {guard} {inputs} {is} k r𝔹w))
                     (sym (cong (λ n → mkPTI w n is) lenpis')))
              ptoI prvI alen' ws'
  in cons-build steq rec

-- public-input: guard (eval-guard-agree) + a decoded transcript cell
-- (WCell + pto-check/pto-adv at cursor 0, since pto-rem IS the remaining).
build {S} {w} P' (public-input guard val-t o ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , og , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ((b , evw , wcell) , ws')
  with csOf-peel (public-input guard val-t o) is ss csat | b | evw | wcell
... | (_ , tc) | true | evw' | (v , wo , tyv) =
  let egm  = eval-guard-agree {State.mem st} {w} guard agr teq og evw'
      dec  = pto-check {w} {guard} {val-t} {o} {is} {v} {0} {State.pto-rem st}
               evw' wo tyv ptoI
      steq = public-input-active-step {P'} {S} {st} {guard} {val-t} {o} {v}
               egm dec
      padv = pto-adv {w} {guard} {val-t} {o} {is} {v} {0} {State.pto-rem st}
               evw' wo tyv ptoI
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (public-input guard val-t o)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is
              (record st { mem     = ins o v (State.mem st)
                         ; pto-rem = drop (encoded-len val-t) (State.pto-rem st) })
              Γ' (bound ++ outs-of (public-input guard val-t o))
              (synth-instr (public-input guard val-t o) ss)
              dΓ' sa' wt' teq'
              (λ {id} {v'} e → ins-⊑ᵂ {w = w} o agr wo {id} {v'} e) tc
              npi' pisP ptiI padv prvI alen ws'
  in cons-build steq rec
... | (_ , tc) | false | evw' | wo =
  let egm  = eval-guard-agree {State.mem st} {w} guard agr teq og evw'
      steq = public-input-inactive-step {P'} {S} {st} {guard} {val-t} {o} egm
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (public-input guard val-t o)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is (out1 st o (default-val val-t))
              Γ' (bound ++ outs-of (public-input guard val-t o))
              (synth-instr (public-input guard val-t o) ss)
              dΓ' sa' wt' teq'
              (λ {id} {v'} e → ins-⊑ᵂ {w = w} o agr wo {id} {v'} e) tc
              npi' pisP ptiI
              (trans ptoI (mkPTO-false {w} {guard} {val-t} {o} evw'))
              prvI alen ws'
  in cons-build steq rec

-- private-input: symmetric to public-input over priv-rem / mkPRV.
build {S} {w} P' (private-input guard val-t o ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , og , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ((b , evw , wcell) , ws')
  with csOf-peel (private-input guard val-t o) is ss csat | b | evw | wcell
... | (_ , tc) | true | evw' | (v , wo , tyv) =
  let egm  = eval-guard-agree {State.mem st} {w} guard agr teq og evw'
      dec  = prv-check {w} {guard} {val-t} {o} {is} {v} {0} {State.priv-rem st}
               evw' wo tyv prvI
      steq = private-input-active-step {P'} {S} {st} {guard} {val-t} {o} {v}
               egm dec
      padv = prv-adv {w} {guard} {val-t} {o} {is} {v} {0} {State.priv-rem st}
               evw' wo tyv prvI
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (private-input guard val-t o)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is
              (record st { mem      = ins o v (State.mem st)
                         ; priv-rem = drop (encoded-len val-t)
                                        (State.priv-rem st) })
              Γ' (bound ++ outs-of (private-input guard val-t o))
              (synth-instr (private-input guard val-t o) ss)
              dΓ' sa' wt' teq'
              (λ {id} {v'} e → ins-⊑ᵂ {w = w} o agr wo {id} {v'} e) tc
              npi' pisP ptiI ptoI padv alen ws'
  in cons-build steq rec
... | (_ , tc) | false | evw' | wo =
  let egm  = eval-guard-agree {State.mem st} {w} guard agr teq og evw'
      steq = private-input-inactive-step {P'} {S} {st} {guard} {val-t} {o} egm
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (private-input guard val-t o)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is (out1 st o (default-val val-t))
              Γ' (bound ++ outs-of (private-input guard val-t o))
              (synth-instr (private-input guard val-t o) ss)
              dΓ' sa' wt' teq'
              (λ {id} {v'} e → ins-⊑ᵂ {w = w} o agr wo {id} {v'} e) tc
              npi' pisP ptiI ptoI
              (trans prvI (mkPRV-false {w} {guard} {val-t} {o} evw')) alen ws'
  in cons-build steq rec

-- circuit-output: `collectOutputs-agree` moves the WShape success to mem st;
-- the state grows only `outs`, so every invariant passes unchanged.
build {S} {w} P' (circuit-output vals ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , oc , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ((vs , cow) , ws') =
  let (_ , tc) = csOf-peel (circuit-output vals) is ss csat
      co-mem = trans (sym (collectOutputs-agree {State.mem st} {w} {Γ}
                            (IrSource.outputs S) vals teq agr oc)) cow
      steq = circuit-output-step {P'} {S} {st} {vals} {vs} co-mem
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (circuit-output vals)
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is (record st { outs = State.outs st ++ vs })
              Γ' (bound ++ outs-of (circuit-output vals))
              (synth-instr (circuit-output vals) ss)
              dΓ' sa' wt' teq' agr tc npi'
              pisP ptiI ptoI prvI alen ws'
  in cons-build steq rec

build {S} {w} P' (neg a output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (t , ota , fp) , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws
  with fp
... | inj₁ refl =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {native} teq ota
      (x , ra)   = mul-support-l {State.mem st} {Γ} {a} {av} teq ota rav
      (hc , tc)  = csOf-peel (neg a output) is ss csat
      wout = agree-neg-n {a = a} {out = output} agr ra (proj₁ hc)
      steq = neg-step-n {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (neg a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-native (-ᶠ x)))
               Γ' (bound ++ outs-of (neg a output))
               (synth-instr (neg a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₁ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {jubjub-point} teq ota
      (p , ra)   = point-support {State.mem st} {Γ} {a} {av} teq ota rav
      (hc , tc)  = csOf-peel (neg a output) is ss csat
      wout = agree-neg-p {a = a} {out = output} agr ra (proj₁ hc)
      steq = neg-step-p {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (neg a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-jubjub-point (negJ p)))
               Γ' (bound ++ outs-of (neg a output))
               (synth-instr (neg a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ (inj₁ refl)) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-point} teq ota
      (p , ra)   = secp-point-support {State.mem st} {Γ} {a} {av} teq ota rav
      (hc , tc)  = csOf-peel (neg a output) is ss csat
      wout = agree-neg-sp {a = a} {out = output} agr ra (proj₁ hc)
      steq = neg-step-sp {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (neg a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-point (negK p)))
               Γ' (bound ++ outs-of (neg a output))
               (synth-instr (neg a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ (inj₂ (inj₁ refl))) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-base} teq ota
      (x , ra)   = secp-base-support {State.mem st} {Γ} {a} {av} teq ota rav
      (hc , tc)  = csOf-peel (neg a output) is ss csat
      wout = agree-neg-sb {a = a} {out = output} agr ra (proj₁ hc)
      steq = neg-step-sb {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (neg a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-base (-ᵇ x)))
               Γ' (bound ++ outs-of (neg a output))
               (synth-instr (neg a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ (inj₂ (inj₂ refl))) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-scalar} teq ota
      (x , ra)   = secp-scalar-support {State.mem st} {Γ} {a} {av} teq ota rav
      (hc , tc)  = csOf-peel (neg a output) is ss csat
      wout = agree-neg-ss {a = a} {out = output} agr ra (proj₁ hc)
      steq = neg-step-ss {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (neg a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-scalar (-ˢ x)))
               Γ' (bound ++ outs-of (neg a output))
               (synth-instr (neg a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- ec-mul: split on the OpTy ⊎ (jubjub / secp).
build {S} {w} P' (ec-mul a scalar output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , oty⊎ , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws
  with oty⊎
... | inj₁ (oa , osc) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {jubjub-point} teq oa
      (p , ra)   = point-support {State.mem st} {Γ} {a} {av} teq oa rav
      (sv , rsv) = optype-resolve {State.mem st} {Γ} {scalar} {jubjub-scalar}
                     teq osc
      (s , rs)   = scalar-support {State.mem st} {Γ} {scalar} {sv} teq osc rsv
      (hc , tc)  = csOf-peel (ec-mul a scalar output) is ss csat
      wout = agree-ec-mul {a = a} {scalar} {out = output} agr ra rs (proj₁ hc)
      steq = ec-mul-step {P'} {S} {st} {a} {scalar} {output} ra rs
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (ec-mul a scalar output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-jubjub-point (s ·J p)))
               Γ' (bound ++ outs-of (ec-mul a scalar output))
               (synth-instr (ec-mul a scalar output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (oa , osc) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-point} teq oa
      (p , ra)   = secp-point-support {State.mem st} {Γ} {a} {av} teq oa rav
      (sv , rsv) = optype-resolve {State.mem st} {Γ} {scalar} {secp-scalar}
                     teq osc
      (s , rs)   = secp-scalar-support {State.mem st} {Γ} {scalar} {sv} teq osc rsv
      (hc , tc)  = csOf-peel (ec-mul a scalar output) is ss csat
      wout = agree-ec-mul-secp {a = a} {scalar} {out = output} agr ra rs (proj₁ hc)
      steq = ec-mul-step-secp {P'} {S} {st} {a} {scalar} {output} ra rs
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (ec-mul a scalar output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-point (s ·K p)))
               Γ' (bound ++ outs-of (ec-mul a scalar output))
               (synth-instr (ec-mul a scalar output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- ec-mul-generator: split on the OpTy ⊎ (jubjub / secp).
build {S} {w} P' (ec-mul-generator scalar output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , osc⊎ , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws with osc⊎
... | inj₁ osc =
  let (sv , rsv) = optype-resolve {State.mem st} {Γ} {scalar} {jubjub-scalar}
                     teq osc
      (s , rs)   = scalar-support {State.mem st} {Γ} {scalar} {sv} teq osc rsv
      (hc , tc)  = csOf-peel (ec-mul-generator scalar output) is ss csat
      wout = agree-ec-gen {scalar = scalar} {out = output} agr rs (proj₁ hc)
      steq = ec-mul-generator-step {P'} {S} {st} {scalar} {output} rs
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (ec-mul-generator scalar output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-jubjub-point (s ·J genJ)))
               Γ' (bound ++ outs-of (ec-mul-generator scalar output))
               (synth-instr (ec-mul-generator scalar output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ osc =
  let (sv , rsv) = optype-resolve {State.mem st} {Γ} {scalar} {secp-scalar}
                     teq osc
      (s , rs)   = secp-scalar-support {State.mem st} {Γ} {scalar} {sv} teq osc rsv
      (hc , tc)  = csOf-peel (ec-mul-generator scalar output) is ss csat
      wout = agree-ec-gen-secp {scalar = scalar} {out = output} agr rs (proj₁ hc)
      steq = ec-mul-generator-step-secp {P'} {S} {st} {scalar} {output} rs
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (ec-mul-generator scalar output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-point (s ·K genK)))
               Γ' (bound ++ outs-of (ec-mul-generator scalar output))
               (synth-instr (ec-mul-generator scalar output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

build {S} {w} P' (transient-hash inputs output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , oinp , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (frs , rall) = allNat-resolve {State.mem st} {Γ} inputs teq oinp
      (hc , tc) = csOf-peel (transient-hash inputs output) is ss csat
      wout = agree-transient-hash {inputs = inputs} {out = output} agr rall
               (proj₁ hc)
      steq = transient-hash-step {P'} {S} {st} {inputs} {output} rall
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (transient-hash inputs output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 st output (val-native (transient-hash-fn frs)))
               Γ' (bound ++ outs-of (transient-hash inputs output))
               (synth-instr (transient-hash inputs output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- into-bytes32: output is always Bytes32; split on the input's FM type.
build {S} {w} P' (into-bytes32 a output ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , (t , oa , fm) , wt') teq agr csat npi pisP ptiI ptoI prvI alen ws
  with fm
... | inj₁ refl =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {native} teq oa
      (x , ra)   = mul-support-l {State.mem st} {Γ} {a} {av} teq oa rav
      (hc , tc) = csOf-peel (into-bytes32 a output) is ss csat
      wout = agree-into-bytes {a = a} {out = output} agr
               (resolveᶠ-nat {State.mem st} {a} ra) (proj₁ hc)
      steq = into-bytes32-step {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (into-bytes32 a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-bytes32 (nativeToBytes x)))
               Γ' (bound ++ outs-of (into-bytes32 a output))
               (synth-instr (into-bytes32 a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₁ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-base} teq oa
      (x , ra)   = secp-base-support {State.mem st} {Γ} {a} {av} teq oa rav
      (hc , tc) = csOf-peel (into-bytes32 a output) is ss csat
      wout = agree-into-bytes-sb {a = a} {out = output} agr ra (proj₁ hc)
      steq = into-bytes32-step-sb {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (into-bytes32 a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 st output (val-bytes32 (secpBaseToBytes x)))
               Γ' (bound ++ outs-of (into-bytes32 a output))
               (synth-instr (into-bytes32 a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (inj₂ refl) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {secp-scalar} teq oa
      (x , ra)   = secp-scalar-support {State.mem st} {Γ} {a} {av} teq oa rav
      (hc , tc) = csOf-peel (into-bytes32 a output) is ss csat
      wout = agree-into-bytes-ss {a = a} {out = output} agr ra (proj₁ hc)
      steq = into-bytes32-step-ss {P'} {S} {st} {a} {output} ra
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (into-bytes32 a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 st output (val-bytes32 (secpScalarToBytes x)))
               Γ' (bound ++ outs-of (into-bytes32 a output))
               (synth-instr (into-bytes32 a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

build {S} {w} P' (jubjub-scalar-from-native a output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , oa , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (x , raf) = optype-resolveᶠ {State.mem st} {Γ} {a} teq oa
      (hc , tc) = csOf-peel (jubjub-scalar-from-native a output) is ss csat
      wout = agree-scalar-from-native {a = a} {out = output} agr raf (proj₁ hc)
      steq = jubjub-scalar-from-native-step {P'} {S} {st} {a} {output} raf
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (jubjub-scalar-from-native a output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 st output (val-jubjub-scalar (native→jubjubScalar x)))
               Γ' (bound ++ outs-of (jubjub-scalar-from-native a output))
               (synth-instr (jubjub-scalar-from-native a output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- less-than: single output; the exact range bounds come from the `WSteps`
-- side condition (the constraint carries only the evened chip bounds),
-- unified to the run's operand values; the `<?` reading is χˢ/χ-bridged
-- by `agree-less-than`.
build {S} {w} P' (less-than a b bits output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , (oa , ob) , wt') teq agr csat npi pisP ptiI
  ptoI prvI alen ((x* , y* , rwa , rwb , bx* , by*) , ws) =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {a} {native} teq oa
      (x , ra)   = mul-support-l {State.mem st} {Γ} {a} {av} teq oa rav
      (bv , rbv) = optype-resolve {State.mem st} {Γ} {b} {native} teq ob
      (y , rb)   = mul-support-l {State.mem st} {Γ} {b} {bv} teq ob rbv
      raf = resolveᶠ-nat {State.mem st} {a} ra
      rbf = resolveᶠ-nat {State.mem st} {b} rb
      (hc , tc)  = csOf-peel (less-than a b bits output) is ss csat
      hlt : holds w (less-than output a b bits)
      hlt = proj₁ hc
      x≡x* = just-injective (trans (sym (⊑-resolveᶠ {State.mem st} {w} a agr raf)) rwa)
      y≡y* = just-injective (trans (sym (⊑-resolveᶠ {State.mem st} {w} b agr rbf)) rwb)
      bx = subst (λ z → valFr z < 2 ^ bits) (sym x≡x*) bx*
      by = subst (λ z → valFr z < 2 ^ bits) (sym y≡y*) by*
      wout = agree-less-than {a = a} {b = b} {bits = bits} {out = output}
               agr raf rbf hlt
      steq = less-than-step {P'} {S} {st} {a} {b} {bits} {output} {x} {y}
               raf rbf bx by
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (less-than a b bits output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 st output (val-native (χˢ (isYes (valFr x <? valFr y)))))
               Γ' (bound ++ outs-of (less-than a b bits output))
               (synth-instr (less-than a b bits output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- from-coordinates: split on the OpTy ⊎ (native→jubjub / secp-base→secp).
build {S} {w} P' (from-coordinates (xop , yop) output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , oty⊎ , wt') teq agr csat npi pisP ptiI
  ptoI prvI alen ws with oty⊎
... | inj₁ (ox , oy) =
  let (xav , xrav) = optype-resolve {State.mem st} {Γ} {xop} {native} teq ox
      (x , rx)     = mul-support-l {State.mem st} {Γ} {xop} {xav} teq ox xrav
      (yav , yrav) = optype-resolve {State.mem st} {Γ} {yop} {native} teq oy
      (y , ry)     = mul-support-l {State.mem st} {Γ} {yop} {yav} teq oy yrav
      (hc , tc)    = csOf-peel (from-coordinates (xop , yop) output) is ss csat
      (p , fcEq , wout) = agree-from-coords {xop = xop} {yop} {out = output} agr
                            (resolveᶠ-nat {State.mem st} {xop} rx)
                            (resolveᶠ-nat {State.mem st} {yop} ry) (proj₁ hc)
      steq = from-coordinates-step {P'} {S} {st} {xop} {yop} {output} {x} {y} {p}
               rx ry fcEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (from-coordinates (xop , yop) output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-jubjub-point p))
               Γ' (bound ++ outs-of (from-coordinates (xop , yop) output))
               (synth-instr (from-coordinates (xop , yop) output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ (ox , oy) =
  let (xav , xrav) = optype-resolve {State.mem st} {Γ} {xop} {secp-base} teq ox
      (x , rx)     = secp-base-support {State.mem st} {Γ} {xop} {xav} teq ox xrav
      (yav , yrav) = optype-resolve {State.mem st} {Γ} {yop} {secp-base} teq oy
      (y , ry)     = secp-base-support {State.mem st} {Γ} {yop} {yav} teq oy yrav
      (hc , tc)    = csOf-peel (from-coordinates (xop , yop) output) is ss csat
      (p , fcEq , wout) = agree-from-coords-secp {xop = xop} {yop} {out = output}
                            agr rx ry (proj₁ hc)
      steq = from-coordinates-step-secp {P'} {S} {st} {xop} {yop} {output}
               {x} {y} {p} rx ry fcEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (from-coordinates (xop , yop) output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-secp-point p))
               Γ' (bound ++ outs-of (from-coordinates (xop , yop) output))
               (synth-instr (from-coordinates (xop , yop) output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- bytes32-from-low-high: single output (bytes32) from two native operands.
build {S} {w} P' (bytes32-from-low-high (loop , hiop) output ∷ is) st Γ bound ss
  dΓ (af , nd , sa') (Γ' , oty , (olo , ohi) , wt') teq agr csat npi pisP
  ptiI ptoI prvI alen ws =
  let (lo , rl) = optype-resolveᶠ {State.mem st} {Γ} {loop} teq olo
      (hi , rh) = optype-resolveᶠ {State.mem st} {Γ} {hiop} teq ohi
      (hc , tc) = csOf-peel (bytes32-from-low-high (loop , hiop) output) is ss csat
      (bs , lhEq , wout) = agree-bytes-from-low-high {lop = loop} {hip = hiop}
                             {out = output} agr rl rh (proj₁ hc)
      steq = bytes32-from-low-high-step {P'} {S} {st} {loop} {hiop} {output}
               {lo} {hi} {bs} rl rh lhEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (bytes32-from-low-high (loop , hiop) output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-bytes32 bs))
               Γ' (bound ++ outs-of (bytes32-from-low-high (loop , hiop) output))
               (synth-instr (bytes32-from-low-high (loop , hiop) output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- from-bytes32: `synth` emits `from-bytes` regardless of `val-t`.
-- `from-bytes32-realize` uses the `WCell` type witness to pick the output
-- value, its step, and the witness agreement, keeping `val-t` abstract here.
build {S} {w} P' (from-bytes32 bytes val-t o ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , ob , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  (wcell , ws') =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {bytes} {bytes32} teq ob
      (bs , rb)  = bytes32-support {State.mem st} {Γ} {bytes} {av} teq ob rav
      (hc , tc)  = csOf-peel (from-bytes32 bytes val-t o) is ss csat
      (v , steq , wout) = from-bytes32-realize {P'} {S} {w} {st} {bytes} {val-t}
                            {o} {bs} agr rb (proj₁ hc) wcell
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (from-bytes32 bytes val-t o)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st o v)
               Γ' (bound ++ outs-of (from-bytes32 bytes val-t o))
               (synth-instr (from-bytes32 bytes val-t o) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} o agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws'
  in cons-build steq rec

-- reverse-bytes: single Bytes32 output; deterministic `Data.Vec.reverse`.
build {S} {w} P' (reverse-bytes bytes o ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , ob , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ws =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {bytes} {bytes32} teq ob
      (bs , rb)  = bytes32-support {State.mem st} {Γ} {bytes} {av} teq ob rav
      (hc , tc)  = csOf-peel (reverse-bytes bytes o) is ss csat
      wout = agree-reverse-bytes {b = bytes} {out = o} agr rb (proj₁ hc)
      steq = reverse-bytes-step {P'} {S} {st} {bytes} {o} {bs} rb
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (reverse-bytes bytes o)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st o (val-bytes32 (reverse bs)))
               Γ' (bound ++ outs-of (reverse-bytes bytes o))
               (synth-instr (reverse-bytes bytes o) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} o agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- test-eq: single output; the `valEq?` reading comes from `holds (test-eq …)`
-- (unified to the run's operands).
build {S} {w} P' (test-eq a b out ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , ((ta , oa) , (tb , ob)) , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (av , ra) = optype-resolve {State.mem st} {Γ} {a} {ta} teq oa
      (bv , rb) = optype-resolve {State.mem st} {Γ} {b} {tb} teq ob
      (hc , tc) = csOf-peel (test-eq a b out) is ss csat
      (av' , bv' , ee , wa , wb , wveq , _) = proj₁ hc
      av≡av' = just-injective (trans (sym (⊑-resolve {State.mem st} {w} a agr ra)) wa)
      bv≡bv' = just-injective (trans (sym (⊑-resolve {State.mem st} {w} b agr rb)) wb)
      ve : valEq? av bv ≡ just ee
      ve = subst (λ p → valEq? p bv ≡ just ee) (sym av≡av')
             (subst (λ q → valEq? av' q ≡ just ee) (sym bv≡bv') wveq)
      wout = agree-test-eq {a = a} {b} {e = ee} agr ra rb ve (proj₁ hc)
      steq = test-eq-step {P'} {S} {st} {a} {b} {out} {av} {bv} {ee} ra rb ve
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (test-eq a b out)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st out (val-native (χˢ ee)))
               Γ' (bound ++ outs-of (test-eq a b out))
               (synth-instr (test-eq a b out) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} out agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- cond-select: single output; operands `a`/`b` typed via `outtys`' `same-ty`,
-- the selector bit's boolean reading from `agree-cond-select`.
build {S} {w} P' (cond-select bit a b output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , obit , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (t , sty)    = mapᵐ-inv oty
      (xbit , rbf) = optype-resolveᶠ {State.mem st} {Γ} {bit} teq obit
      (av , ra)    = optype-resolve {State.mem st} {Γ} {a} {t}
                       teq (same-ty-l {Γ} {a} {b} sty)
      (bvl , rb)   = optype-resolve {State.mem st} {Γ} {b} {t}
                       teq (same-ty-r {Γ} {a} {b} sty)
      tmatch = cond-select-support {State.mem st} {Γ} {a} {b} teq
                 (same-ty-l {Γ} {a} {b} sty) (same-ty-r {Γ} {a} {b} sty) ra rb
      (hc , tc) = csOf-peel (cond-select bit a b output) is ss csat
      (bb , r𝔹 , wout) = agree-cond-select {bit = bit} {a = a} {b = b}
                           agr rbf ra rb (proj₁ hc)
      steq = cond-select-step {P'} {S} {st} {bit} {a} {b} {output} r𝔹 ra rb tmatch
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (cond-select bit a b output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (if bb then av else bvl))
               Γ' (bound ++ outs-of (cond-select bit a b output))
               (synth-instr (cond-select bit a b output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- constrain-eq: no output; `valEq? av bv ≡ just true` from the reflexive
-- support (`holds (eq …)` pins both operands to the same witness value).
build {S} {w} P' (constrain-eq a b ∷ is) st Γ bound ss dΓ (af , nd , sa')
  (Γ' , oty , ((t , ota , tsup) , (tb , otb)) , wt') teq agr csat npi pisP
  ptiI ptoI prvI alen ws =
  let (av , ra) = optype-resolve {State.mem st} {Γ} {a} {t}  teq ota
      (bv , rb) = optype-resolve {State.mem st} {Γ} {b} {tb} teq otb
      (hc , tc) = csOf-peel (constrain-eq a b) is ss csat
      (v , wa , wb) = proj₁ hc
      av≡v = just-injective (trans (sym (⊑-resolve {State.mem st} {w} a agr ra)) wa)
      bv≡v = just-injective (trans (sym (⊑-resolve {State.mem st} {w} b agr rb)) wb)
      (_ , ve0) = constrain-eq-support {State.mem st} {Γ} {a} {t} {av}
                    teq ota tsup ra
      ve = subst (λ z → valEq? av z ≡ just true) (trans av≡v (sym bv≡v)) ve0
      steq = constrain-eq-step {P'} {S} {st} {a} {b} {av} {bv} ra rb ve
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (constrain-eq a b)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is st
               Γ' (bound ++ outs-of (constrain-eq a b))
               (synth-instr (constrain-eq a b) ss)
               dΓ' sa' wt' teq' agr tc npi'
               pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- constrain-to-boolean: no output; the run's guard reads the boolean the
-- `boolean` constraint pins (`is-bit`).
build {S} {w} P' (constrain-to-boolean val ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , ov , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ws =
  let (x , raf) = optype-resolveᶠ {State.mem st} {Γ} {val} teq ov
      (hc , tc) = csOf-peel (constrain-to-boolean val) is ss csat
      (xw , wval , isbit) = proj₁ hc
      x≡xw = just-injective
               (trans (sym (⊑-resolveᶠ {State.mem st} {w} val agr raf)) wval)
      (bb , tbx) = is-bit→to𝔹 (subst is-bit (sym x≡xw) isbit)
      r𝔹 = trans (cong (_>>= to𝔹) raf) tbx
      steq = constrain-to-boolean-step {P'} {S} {st} {val} {bb} r𝔹
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (constrain-to-boolean val)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is st
               Γ' (bound ++ outs-of (constrain-to-boolean val))
               (synth-instr (constrain-to-boolean val) ss)
               dΓ' sa' wt' teq' agr tc npi'
               pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- into-coordinates: two outputs, double `out1`; split on the OpTy ⊎
-- (jubjub→native coords / secp→secp-base coords, the latter partial via
-- `coordsK`).  Agreement by nested `ins-⊑ᵂ`.
build {S} {w} P' (into-coordinates point (xid , yid) ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , op⊎ , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ws with op⊎
... | inj₁ op =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {point} {jubjub-point}
                     teq op
      (p , rp)   = point-support {State.mem st} {Γ} {point} {av} teq op rav
      (hc , tc)  = csOf-peel (into-coordinates point (xid , yid)) is ss csat
      (x , y , coordsEq , wxo , wyo) =
        agree-into-coords {point = point} {xo = xid} {yo = yid} {p = p} agr rp
          (proj₁ hc)
      steq = into-coordinates-step {P'} {S} {st} {point} {xid} {yid} {p} {x} {y}
               rp coordsEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (into-coordinates point (xid , yid))
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 (out1 st xid (val-native x)) yid (val-native y))
               Γ' (bound ++ outs-of (into-coordinates point (xid , yid)))
               (synth-instr (into-coordinates point (xid , yid)) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} yid
                 (ins-⊑ᵂ {w = w} xid agr wxo) wyo {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec
... | inj₂ op =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {point} {secp-point}
                     teq op
      (p , rp)   = secp-point-support {State.mem st} {Γ} {point} {av} teq op rav
      (hc , tc)  = csOf-peel (into-coordinates point (xid , yid)) is ss csat
      (x , y , coordsEq , wxo , wyo) =
        agree-into-coords-secp {point = point} {xo = xid} {yo = yid} {p = p} agr
          rp (proj₁ hc)
      steq = into-coordinates-step-secp {P'} {S} {st} {point} {xid} {yid} {p}
               {x} {y} rp coordsEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (into-coordinates point (xid , yid))
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 (out1 st xid (val-secp-base x)) yid (val-secp-base y))
               Γ' (bound ++ outs-of (into-coordinates point (xid , yid)))
               (synth-instr (into-coordinates point (xid , yid)) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} yid
                 (ins-⊑ᵂ {w = w} xid agr wxo) wyo {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- bytes32-into-low-high: two outputs (native low/high), double `out1`.
build {S} {w} P' (bytes32-into-low-high bytes (loid , hiid) ∷ is) st Γ bound ss
  dΓ (af , nd , sa') (Γ' , oty , ob , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (av , rav) = optype-resolve {State.mem st} {Γ} {bytes} {bytes32} teq ob
      (bs , rb)  = bytes32-support {State.mem st} {Γ} {bytes} {av} teq ob rav
      (hc , tc)  = csOf-peel (bytes32-into-low-high bytes (loid , hiid)) is ss
                     csat
      (l , h , splitEq , wlo , whi) =
        agree-bytes-into-low-high {bytes = bytes} {lo = loid} {hi = hiid} {bs = bs}
          agr rb (proj₁ hc)
      steq = bytes32-into-low-high-step {P'} {S} {st} {bytes} {loid} {hiid} {bs}
               {l} {h} rb splitEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (bytes32-into-low-high bytes (loid , hiid))
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 (out1 st loid (val-native l)) hiid (val-native h))
               Γ' (bound ++ outs-of (bytes32-into-low-high bytes (loid , hiid)))
               (synth-instr (bytes32-into-low-high bytes (loid , hiid)) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} hiid
                 (ins-⊑ᵂ {w = w} loid agr wlo) whi {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- div-mod-power-of-two: two outputs via `insertMany` (like `encode`); the
-- arity `outs ≡ q ∷ r ∷ []` is a WShape executability condition.
build {S} {w} P' (div-mod-power-of-two val bits outs ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , ov , wt') teq agr csat npi pisP ptiI ptoI prvI alen
  ((q , r , refl) , ws') =
  let (x , rvf) = optype-resolveᶠ {State.mem st} {Γ} {val} teq ov
      (hc , tc) = csOf-peel (div-mod-power-of-two val bits (q ∷ r ∷ [])) is ss
                    csat
      (wq , wr) = agree-div-mod {val = val} agr rvf (proj₁ hc)
      (st' , im , agr') = insertMany-realize {w} st (q ∷ r ∷ [])
        ( val-native (from-le-bits (drop bits (to-le-bits x)))
        ∷ val-native (from-le-bits (take bits (to-le-bits x))) ∷ [])
        agr (wq , wr , tt)
      steq = div-mod-step {P'} {S} {st} {val} {bits} {q ∷ r ∷ []} {x} {st'}
               rvf im
      (peq , pteq , poeq , preq) =
        insertMany-shape st (q ∷ r ∷ [])
          ( val-native (from-le-bits (drop bits (to-le-bits x)))
          ∷ val-native (from-le-bits (take bits (to-le-bits x))) ∷ []) im
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (div-mod-power-of-two val bits (q ∷ r ∷ []))
          oty dΓ af nd teq npi steq
      rec = build {S} {w} P' is st' Γ'
              (bound ++ outs-of (div-mod-power-of-two val bits (q ∷ r ∷ [])))
              (synth-instr (div-mod-power-of-two val bits (q ∷ r ∷ [])) ss)
              dΓ' sa' wt' teq' agr' tc npi'
              (trans peq (trans pisP
                (cong (λ n → take n (CircuitWitness.pis w))
                  (cong length (sym peq)))))
              (trans (cong (λ n → drop n
                             (ProofPreimage.pub-transcript-inputs P')) pteq)
                     (trans ptiI
                       (cong (λ n → mkPTI w n is) (cong length (sym peq)))))
              (trans poeq ptoI) (trans preq prvI)
              (subst₂ (λ p q → p + length (mkPTI w q is)
                        ≡ length (ProofPreimage.pub-transcript-inputs P'))
                (sym pteq) (sym (cong length peq)) alen) ws'
  in cons-build steq rec

-- reconstitute-field: single output; range bounds from `holds`, the no-overflow
-- guard from WShape (unified to the run's operands).
build {S} {w} P' (reconstitute-field divisor modulus bits out ∷ is) st Γ bound
  ss dΓ (af , nd , sa') (Γ' , oty , (od , om) , wt') teq agr csat npi pisP
  ptiI ptoI prvI alen ((d , mo , rd-w , rmo-w , novf) , ws') =
  let (dv , rd)  = optype-resolveᶠ {State.mem st} {Γ} {divisor} teq od
      (mv , rmo) = optype-resolveᶠ {State.mem st} {Γ} {modulus} teq om
      (hc , tc)  = csOf-peel (reconstitute-field divisor modulus bits out) is ss
                     csat
      (bm , bd , wout) = agree-reconstitute {divisor = divisor} {modulus}
                           {bits = bits} {out = out} agr rd rmo (proj₁ hc)
      dv≡d = just-injective
               (trans (sym (⊑-resolveᶠ {State.mem st} {w} divisor agr rd)) rd-w)
      mv≡mo = just-injective
                (trans (sym (⊑-resolveᶠ {State.mem st} {w} modulus agr rmo)) rmo-w)
      novf' = subst₂ (λ dd mm → valFr mm + 2 ^ bits * valFr dd < FR-ORDER)
                (sym dv≡d) (sym mv≡mo) novf
      steq = reconstitute-field-step {P'} {S} {st} {divisor} {modulus} {bits}
               {out} {dv} {mv} rd rmo bm bd novf'
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (reconstitute-field divisor modulus bits out)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is
               (out1 st out (val-native ((pow2ᶠˢ bits *ᶠ dv) +ᶠ mv)))
               Γ' (bound ++ outs-of (reconstitute-field divisor modulus bits out))
               (synth-instr (reconstitute-field divisor modulus bits out) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} out agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws'
  in cons-build steq rec

-- persistent-hash: single Bytes32 output; the SHA-256 success comes from
-- `holds` (via `agree-persistent-hash`).
build {S} {w} P' (persistent-hash al inputs output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , oinp , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (frs , rall) = allNat-resolve {State.mem st} {Γ} inputs teq oinp
      (hc , tc) = csOf-peel (persistent-hash al inputs output) is ss csat
      (v , phEq , wout) =
        agree-persistent-hash {inputs = inputs} agr rall (proj₁ hc)
      steq = persistent-hash-step {P'} {S} {st} {al} {inputs} {output} {frs} {v}
               rall phEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (persistent-hash al inputs output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-bytes32 v))
               Γ' (bound ++ outs-of (persistent-hash al inputs output))
               (synth-instr (persistent-hash al inputs output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

-- keccak256: single Bytes32 output; the Keccak-256 success comes from
-- `holds` (via `agree-keccak`).
build {S} {w} P' (keccak256 al inputs output ∷ is) st Γ bound ss dΓ
  (af , nd , sa') (Γ' , oty , oinp , wt') teq agr csat npi pisP ptiI ptoI
  prvI alen ws =
  let (frs , rall) = allNat-resolve {State.mem st} {Γ} inputs teq oinp
      (hc , tc) = csOf-peel (keccak256 al inputs output) is ss csat
      (v , kfEq , wout) =
        agree-keccak {inputs = inputs} agr rall (proj₁ hc)
      steq = keccak256-step {P'} {S} {st} {al} {inputs} {output} {frs} {v}
               rall kfEq
      (dΓ' , teq' , npi') =
        adv3 P' S st ss (keccak256 al inputs output)
          oty dΓ af nd teq npi steq
      rec  = build {S} {w} P' is (out1 st output (val-bytes32 v))
               Γ' (bound ++ outs-of (keccak256 al inputs output))
               (synth-instr (keccak256 al inputs output) ss)
               dΓ' sa' wt' teq'
               (λ {id} {v'} e → ins-⊑ᵂ {w = w} output agr wout {id} {v'} e) tc
               npi' pisP ptiI ptoI prvI alen ws
  in cons-build steq rec

------------------------------------------------------------------------
-- Constructive pis-form statement soundness (no-comm fragment).
--
-- From a satisfying witness of a well-typed producer circuit (and the
-- minimal witness shape), synthesise a preimage `P'` and a final state
-- `s` with `run-shaped S P' s`, `pis (witness-of P' s) ≡ pis w`, the
-- memory sub-assignment `mem s ⊑ᵂ w`, and (vacuously in the no-comm
-- branch) the comm-rand agreement.
------------------------------------------------------------------------

-- pi tracking over a whole run (fold-free; mirrors `fwd-go`'s pi component).
pi-run : ∀ {P S s} is st ss → run P S st is ≡ just s
  → SynthState.next-pi ss ≡ length (State.pis st)
  → SynthState.next-pi (synth-instrs is ss) ≡ length (State.pis s)
pi-run [] st ss run-eq pi rewrite just-injective run-eq = pi
pi-run {P} {S} (i ∷ is) st ss run-eq pi =
  let (st' , step-eq , run-rest) = run-inv i is st run-eq in
  pi-run {P} {S} is st' (synth-instr i ss) run-rest
    (pi-inv-step {P} {S} i ss step-eq pi)

-- The witness's leading pi entry (its binding input).
take1-lookup : ∀ {v} (xs : List Fr) → pi-lookup xs 0 ≡ just v → take 1 xs ≡ v ∷ []
take1-lookup xs e = cong (take 1) (drop-lookup xs 0 e)

-- The leading `pi-binding 0` constraint holds at `w` (no-comm entry).
pi-binding-false : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ false
  → satisfies (synth S) w
  → ∃ λ bv → pi-lookup (CircuitWitness.pis w) 0 ≡ just bv
pi-binding-false {S} {w} hc sat
  with IrSource.do-communications-commitment S | hc
... | false | refl =
  proj₁ (subst (λ z → satisfies-constraints z w)
           (synth-cs-acc (IrSource.instructions S) ss₁)
           (satisfies.constraint-ok sat))

-- pi-len of the no-comm circuit equals the run-final pi cursor seed.
pilen-false : ∀ {S}
  → IrSource.do-communications-commitment S ≡ false
  → Circuit.pi-len (synth S)
      ≡ SynthState.next-pi (synth-instrs (IrSource.instructions S) ss₁)
pilen-false {S} hc =
  cong (λ b → SynthState.next-pi
                (synth-instrs (IrSource.instructions S)
                  (mk-synth (pi-binding 0 ∷ []) (preamble-pi-count b) [])))
       hc

-- `init` succeeds on a no-comm preimage whose declared inputs decode.
init-false : ∀ {S} (P' : ProofPreimage) m₀
  → IrSource.do-communications-commitment S ≡ false
  → decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P') ≡ just m₀
  → init S P'
      ≡ just (record { mem = m₀ ; pis = ProofPreimage.binding-input P' ∷ []
                     ; pi-skips = [] ; pti-idx = 0
                     ; pto-rem = ProofPreimage.pub-transcript-outputs P'
                     ; priv-rem = ProofPreimage.priv-transcript P' ; outs = [] })
init-false P' m₀ hc decEq rewrite decEq | hc = refl

-- A successful `run` whose terminal cursors are exhausted (and no comm)
-- discharges `preprocess`.
preproc-false : ∀ {S} (P' : ProofPreimage) st0 fs
  → IrSource.do-communications-commitment S ≡ false
  → init S P' ≡ just st0
  → run P' S st0 (IrSource.instructions S) ≡ just fs
  → State.pti-idx fs ≡ length (ProofPreimage.pub-transcript-inputs P')
  → State.pto-rem fs ≡ [] → State.priv-rem fs ≡ []
  → preprocess S P' ≡ just fs
preproc-false P' st0 fs hc ieq run≡ flen fpto fprv
  rewrite ieq | run≡
  with State.pti-idx fs ≟ℕ length (ProofPreimage.pub-transcript-inputs P')
... | no ¬p = case ¬p flen of λ ()
... | yes _ rewrite fpto | fprv | hc = refl

------------------------------------------------------------------------
-- A commitment-well-shaped realizer.  This bundles exactly what the branch
-- lemmas and `statement-sound` return: a preimage/state pair with
-- `run-shaped` and outright `preprocess` success (the latter subsumes the
-- former, which is kept as a field for direct consumption), the
-- pis-agreement, the memory-agreement and, under the flag, the
-- commitment-randomness agreement, together with `CommWF` on the
-- preimage's commitment.  `CommWF b cc` says an absent-commitment flag
-- (`b ≡ false`) forces the absent commitment (`cc ≡ nothing`); it is
-- vacuously inhabited when the flag is on.
------------------------------------------------------------------------

CommWF : Bool → Maybe (Fr × Fr) → Set
CommWF b cc = b ≡ false → cc ≡ nothing

record SubRealizer (S : IrSource) (w : CircuitWitness) : Set where
  constructor mk-subrealizer
  field
    P          : ProofPreimage
    s          : State
    shaped     : run-shaped S P s
    preproc-ok : preprocess S P ≡ just s
    pis-agree  : CircuitWitness.pis (witness-of P s) ≡ CircuitWitness.pis w
    mem-agree  : State.mem s ⊑ᵂ w
    rand-agree : IrSource.do-communications-commitment S ≡ true
      → CircuitWitness.comm-rand (witness-of P s)
        ≡ CircuitWitness.comm-rand w
    comm-wf    : CommWF (IrSource.do-communications-commitment S)
                        (ProofPreimage.comm-commitment P)

statement-sound-false : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ false
  → producer-WT S → satisfies (synth S) w → WShape S w
  → SubRealizer S w
statement-sound-false {S} {w} hc ((saND , sa) , wt) sat (wins , wsteps) =
  mk-subrealizer P' fs (preprocess→run-shaped {S} {P'} {fs} {st0} ieq peq)
    peq pisfs (BuildOut.fagr rec) commrand commnothing
  where
    is  = IrSource.instructions S
    tis = IrSource.inputs S
    bvp = pi-binding-false {S} {w} hc sat
    bv  = proj₁ bvp
    dm  = decode-mkInputs {w} tis wins
    m₀  = proj₁ dm

    P' : ProofPreimage
    P' = record { inputs                 = mkInputs w tis
                ; binding-input          = bv
                ; comm-commitment        = nothing
                ; pub-transcript-inputs  = mkPTI w 1 is
                ; pub-transcript-outputs = mkPTO w is
                ; priv-transcript        = mkPRV w is }

    st0 : State
    st0 = record { mem = m₀ ; pis = bv ∷ [] ; pi-skips = [] ; pti-idx = 0
                 ; pto-rem = mkPTO w is ; priv-rem = mkPRV w is ; outs = [] }

    ieq : init S P' ≡ just st0
    ieq = init-false {S} P' m₀ hc (proj₁ (proj₂ dm))

    rec : BuildOut {S} {w} P' st0 is
    rec = build {S} {w} P' is st0 (input-ctx tis)
            (map TypedIdentifier.name tis) ss₁
            (dom-ty-input-ctx tis) sa wt
            (init-ty {S} {P'} {st0} ieq)            (proj₂ (proj₂ dm))
            (csOf-from-sat-false {S} {w} hc sat)
            refl (sym (take1-lookup (CircuitWitness.pis w) (proj₂ bvp)))
            refl refl refl refl wsteps

    fs = BuildOut.fs rec

    peq : preprocess S P' ≡ just fs
    peq = preproc-false {S} P' st0 fs hc ieq (BuildOut.frun rec)
            (BuildOut.flen rec) (BuildOut.fpto rec) (BuildOut.fprv rec)

    lenw : length (CircuitWitness.pis w) ≡ length (State.pis fs)
    lenw = trans (satisfies.pi-length sat)
             (trans (pilen-false {S} hc)
               (pi-run {P'} {S} is st0 ss₁ (BuildOut.frun rec) refl))

    pisfs : State.pis fs ≡ CircuitWitness.pis w
    pisfs = trans (BuildOut.fpis rec)
              (take-all (length (State.pis fs)) (CircuitWitness.pis w)
                (≤-reflexive lenw))

    -- Vacuous here: the comm-rand agreement is guarded by `do-comm ≡ true`,
    -- which contradicts this branch's `hc : do-comm ≡ false`.
    commrand : IrSource.do-communications-commitment S ≡ true
      → CircuitWitness.comm-rand (witness-of P' fs)
        ≡ CircuitWitness.comm-rand w
    commrand ht = case trans (sym hc) ht of λ ()

    -- This branch builds `P'` with `comm-commitment = nothing`.
    commnothing : IrSource.do-communications-commitment S ≡ false
      → ProofPreimage.comm-commitment P' ≡ nothing
    commnothing _ = refl

------------------------------------------------------------------------
-- Constructive pis-form statement soundness (comm fragment).
--
-- The `do-comm = true` branch.  The extra work over the no-comm branch is
-- the TC2 discharge: `preprocess` checks the binding π[1] equals
-- `transient-commit` of the encoded inputs and collected outputs.  The
-- `comm` constraint pins that at `w`; we transport its input/output
-- encodings to the run's actual inputs (`resolve-encode-mkInputs`, from
-- `WInputs`) and outputs (`out-go` at the built witness, lowered to `w`).
-- The same `comm` constraint pins `comm-rand w ≡ just rv`, which yields the
-- comm-rand agreement (`P'`'s commitment pair carries randomness `rv`).
------------------------------------------------------------------------

-- The leading `pi-binding 0` and trailing `comm` constraints of a has-comm
-- circuit hold at `w` — the two facts `csOf-from-sat-true` drops.
comm-preamble : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ true
  → satisfies (synth S) w
  → (∃ λ bv → pi-lookup (CircuitWitness.pis w) 0 ≡ just bv)
  × holds w (comm (input-operands (IrSource.inputs S))
              (SynthState.output-ops
                (synth-instrs (IrSource.instructions S) ss₂)))
comm-preamble {S} {w} hc sat =
  let (h , _ , c) = synth-true-split {S} {w} hc sat in h , c

-- `take 2` of a stream whose first two entries `pi-lookup` finds.
take2-lookup : ∀ {u v} (xs : List Fr)
  → pi-lookup xs 0 ≡ just u → pi-lookup xs 1 ≡ just v
  → take 2 xs ≡ u ∷ v ∷ []
take2-lookup xs e0 e1 =
  trans (cong (take 2) (drop-lookup xs 0 e0))
        (cong (λ l → _ ∷ take 1 l) (drop-lookup xs 1 e1))

-- pi-len of the has-comm circuit equals the run-final pi cursor seed.
pilen-true : ∀ {S}
  → IrSource.do-communications-commitment S ≡ true
  → Circuit.pi-len (synth S)
      ≡ SynthState.next-pi (synth-instrs (IrSource.instructions S) ss₂)
pilen-true {S} hc =
  cong (λ b → SynthState.next-pi
                (synth-instrs (IrSource.instructions S)
                  (mk-synth (pi-binding 0 ∷ []) (preamble-pi-count b) [])))
       hc

-- The declared inputs' operands resolve (at `w`) to exactly `mkInputs w`.
resolve-encode-mkInputs : ∀ {w} tis → WInputs w tis
  → resolve-encode w (input-operands tis) ≡ just (mkInputs w tis)
resolve-encode-mkInputs         []         _              = refl
resolve-encode-mkInputs {w} (ti ∷ tis) ((v , wo , _) , wins)
  rewrite wo | resolve-encode-mkInputs {w} tis wins = refl

-- Lower a `resolve-encode` from the built witness to `w`: every operand
-- resolves in `mem fs`, and `mem fs ⊑ᵂ w` transports each resolution.
resolve-encode-⊑ : ∀ {P' fs w} ops {r}
  → State.mem fs ⊑ᵂ w
  → resolve-encode (witness-of P' fs) ops ≡ just r
  → resolve-encode w ops ≡ just r
resolve-encode-⊑ []       fagr e = e
resolve-encode-⊑ {P'} {fs} {w} (op ∷ ops) fagr e
  with resolveᶜ (witness-of P' fs) op in eqop
... | nothing = case e of λ ()
... | just v
  with resolve-encode (witness-of P' fs) ops in eqrest
...   | nothing = case e of λ ()
...   | just rest with refl ← e
  rewrite ⊑-resolve {State.mem fs} {w} op fagr
            (trans (sym (resolve-agree P' fs op)) eqop)
        | resolve-encode-⊑ {P'} {fs} {w} ops fagr eqrest = refl

-- `init` succeeds on a has-comm preimage whose inputs decode; the π
-- preamble binds the input and the commitment.
init-true : ∀ {S} (P' : ProofPreimage) m₀ c r
  → IrSource.do-communications-commitment S ≡ true
  → ProofPreimage.comm-commitment P' ≡ just (c , r)
  → decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P') ≡ just m₀
  → init S P'
      ≡ just (record { mem = m₀
                     ; pis = ProofPreimage.binding-input P' ∷ c ∷ []
                     ; pi-skips = [] ; pti-idx = 0
                     ; pto-rem = ProofPreimage.pub-transcript-outputs P'
                     ; priv-rem = ProofPreimage.priv-transcript P' ; outs = [] })
init-true P' m₀ c r hc cceq decEq rewrite decEq | hc | cceq = refl

-- The has-comm `preprocess`, discharging the TC2 commitment check.
preproc-true : ∀ {S} (P' : ProofPreimage) st0 fs c r
  → IrSource.do-communications-commitment S ≡ true
  → ProofPreimage.comm-commitment P' ≡ just (c , r)
  → init S P' ≡ just st0
  → run P' S st0 (IrSource.instructions S) ≡ just fs
  → State.pti-idx fs ≡ length (ProofPreimage.pub-transcript-inputs P')
  → State.pto-rem fs ≡ [] → State.priv-rem fs ≡ []
  → c ≡ transient-commit
          (ProofPreimage.inputs P' ++ concatMap encodeᵉ (State.outs fs)) r
  → preprocess S P' ≡ just fs
preproc-true P' st0 fs c r hc cceq ieq run≡ flen fpto fprv tc2
  rewrite ieq | run≡
  with State.pti-idx fs ≟ℕ length (ProofPreimage.pub-transcript-inputs P')
... | no ¬p = case ¬p flen of λ ()
... | yes _ rewrite fpto | fprv | hc | cceq
  with c ≟ᶠ transient-commit
             (ProofPreimage.inputs P' ++ concatMap encodeᵉ (State.outs fs)) r
...   | yes _  = refl
...   | no ¬c = case ¬c tc2 of λ ()

statement-sound-true : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ true
  → producer-WT S → satisfies (synth S) w → WShape S w
  → SubRealizer S w
statement-sound-true {S} {w} hc ((saND , sa) , wt) sat (wins , wsteps) =
  mk-subrealizer P' fs (preprocess→run-shaped {S} {P'} {fs} {st0} ieq peq)
    peq pisfs (BuildOut.fagr rec) commrand commnothing
  where
    is  = IrSource.instructions S
    tis = IrSource.inputs S

    cpb = comm-preamble {S} {w} hc sat
    bv      = proj₁ (proj₁ cpb)
    bv-look = proj₂ (proj₁ cpb)

    ivs = proj₁ (proj₂ cpb)
    ovs = proj₁ (proj₂ (proj₂ cpb))
    rv  = proj₁ (proj₂ (proj₂ (proj₂ cpb)))
    pv  = proj₁ (proj₂ (proj₂ (proj₂ (proj₂ cpb))))
    ri  = proj₁ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ cpb)))))
    ro  = proj₁ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ cpb))))))
    rr  = proj₁ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ cpb)))))))
    rpv = proj₁ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ cpb))))))))
    pv≡ = proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ (proj₂ cpb))))))))

    dm  = decode-mkInputs {w} tis wins
    m₀  = proj₁ dm

    P' : ProofPreimage
    P' = record { inputs                 = mkInputs w tis
                ; binding-input          = bv
                ; comm-commitment        = just (pv , rv)
                ; pub-transcript-inputs  = mkPTI w 2 is
                ; pub-transcript-outputs = mkPTO w is
                ; priv-transcript        = mkPRV w is }

    st0 : State
    st0 = record { mem = m₀ ; pis = bv ∷ pv ∷ [] ; pi-skips = [] ; pti-idx = 0
                 ; pto-rem = mkPTO w is ; priv-rem = mkPRV w is ; outs = [] }

    ieq : init S P' ≡ just st0
    ieq = init-true {S} P' m₀ pv rv hc refl (proj₁ (proj₂ dm))

    rec : BuildOut {S} {w} P' st0 is
    rec = build {S} {w} P' is st0 (input-ctx tis)
            (map TypedIdentifier.name tis) ss₂
            (dom-ty-input-ctx tis) sa wt
            (init-ty {S} {P'} {st0} ieq)            (proj₂ (proj₂ dm))
            (csOf-from-sat-true {S} {w} hc sat)
            refl (sym (take2-lookup (CircuitWitness.pis w) bv-look rpv))
            refl refl refl refl wsteps

    fs = BuildOut.fs rec

    wf : WF-run P' S is st0
    wf = producer-safe→WF (saND , sa) ieq

    ovs≡ : ovs ≡ concatMap encodeᵉ (State.outs fs)
    ovs≡ = just-injective (trans (sym ro)
             (resolve-encode-⊑ {P'} {fs} {w}
               (SynthState.output-ops (synth-instrs is ss₂))
               (BuildOut.fagr rec)
               (out-go {P'} {S} {fs} is st0 ss₂ (BuildOut.frun rec) wf refl)))

    ivs≡ : ivs ≡ mkInputs w tis
    ivs≡ = just-injective (trans (sym ri) (resolve-encode-mkInputs {w} tis wins))

    tc2 : pv ≡ transient-commit
                 (mkInputs w tis ++ concatMap encodeᵉ (State.outs fs)) rv
    tc2 = subst₂ (λ a b → pv ≡ transient-commit (a ++ b) rv) ivs≡ ovs≡ pv≡

    peq : preprocess S P' ≡ just fs
    peq = preproc-true {S} P' st0 fs pv rv hc refl ieq (BuildOut.frun rec)
            (BuildOut.flen rec) (BuildOut.fpto rec) (BuildOut.fprv rec) tc2

    lenw : length (CircuitWitness.pis w) ≡ length (State.pis fs)
    lenw = trans (satisfies.pi-length sat)
             (trans (pilen-true {S} hc)
               (pi-run {P'} {S} is st0 ss₂ (BuildOut.frun rec) refl))

    pisfs : State.pis fs ≡ CircuitWitness.pis w
    pisfs = trans (BuildOut.fpis rec)
              (take-all (length (State.pis fs)) (CircuitWitness.pis w)
                (≤-reflexive lenw))

    -- `witness-of P' fs` carries `comm-rand-of (just (pv , rv)) = just rv`;
    -- the `comm` constraint (via `rr`) pins `comm-rand w ≡ just rv`.
    commrand : IrSource.do-communications-commitment S ≡ true
      → CircuitWitness.comm-rand (witness-of P' fs)
        ≡ CircuitWitness.comm-rand w
    commrand _ = sym rr

    -- Vacuous: this branch's `hc` contradicts `do-comm ≡ false`.
    commnothing : IrSource.do-communications-commitment S ≡ false
      → ProofPreimage.comm-commitment P' ≡ nothing
    commnothing hf = case trans (sym hc) hf of λ ()

-- The combined result: statement soundness for both branches.
statement-sound : ∀ {S w}
  → producer-WT S → satisfies (synth S) w → WShape S w
  → SubRealizer S w
statement-sound {S} {w} wt sat ws =
  aux (IrSource.do-communications-commitment S) refl
  where
    aux : (b : Bool) → IrSource.do-communications-commitment S ≡ b
      → SubRealizer S w
    aux false hc = statement-sound-false {S} {w} hc wt sat ws
    aux true  hc = statement-sound-true  {S} {w} hc wt sat ws

------------------------------------------------------------------------
-- Extractor completeness.
--
-- The extracted preimage actually proves: the canonical witness of a
-- `SubRealizer`'s preimage/state pair satisfies the synthesized circuit
-- — `forward-sa` applied through the realizer's `preproc-ok` field.
-- Composed with `statement-sound`, extraction is a round trip into the
-- satisfying set: extract from `w`, and the extracted run's own witness
-- satisfies the same circuit.
------------------------------------------------------------------------

extractor-complete : ∀ {S w}
  → producer-SA S
  → (r : SubRealizer S w)
  → satisfies (synth S)
      (witness-of (SubRealizer.P r) (SubRealizer.s r))
extractor-complete {S} {w} sa r =
  forward-sa {S} {SubRealizer.P r} {SubRealizer.s r}
    {run-shaped.start (SubRealizer.shaped r)} sa
    (run-shaped.init≡ (SubRealizer.shaped r))
    (SubRealizer.preproc-ok r)

------------------------------------------------------------------------
-- `WShape` non-vacuity: the canonical witness of a successful run is in
-- the extraction class.
--
-- Every `WSteps` conjunct is forced by the corresponding step's success:
-- the step's own inversion (`step→bwd`) supplies the resolutions, guard
-- readings, bounds, and post-state shape at the pre-step memory, and the
-- run's extension (`step-extends` / `run-extends`) transports them to
-- the final memory — which is exactly
-- `CircuitWitness.assign (witness-of P s)`.  `WInputs` similarly follows
-- from the input decoding (`init-decode`), each cell typed by
-- `decode-typeof` and surviving the run by single assignment.  Combined
-- with `statement-sound`/`statement-sound-unique`, extraction is
-- non-vacuous on every honestly-provable statement.
------------------------------------------------------------------------

-- The declared inputs are well-typed cells of the final memory.
winputs-of : ∀ {P s} tis {raw m₀}
  → decode-inputs tis raw ≡ just m₀
  → NoDup (map TypedIdentifier.name tis)
  → m₀ ⊑ State.mem s
  → WInputs (witness-of P s) tis
winputs-of []         {raw = []}    refl _          _   = tt
winputs-of []         {raw = _ ∷ _} ()   _          _
winputs-of {P} {s} (ti ∷ tis) {raw} e    (nin , nd) sub
  with decode (TypedIdentifier.val-t ti)
              (take (encoded-len (TypedIdentifier.val-t ti)) raw) in eqv
... | just v
  with decode-inputs tis
         (drop (encoded-len (TypedIdentifier.val-t ti)) raw) in eqm
...   | just m′ with refl ← e =
        ( v
        , sub (ins-here (TypedIdentifier.name ti) v m′)
        , decode-typeof (TypedIdentifier.val-t ti) _ eqv )
      , winputs-of {P} {s} tis eqm nd
          (⊑-trans {m′} {ins (TypedIdentifier.name ti) v m′} {State.mem s}
            (ins-⊑ {TypedIdentifier.name ti} {v} {m′}
              (decode-inputs-dom tis _ eqm
                (notIn-∉ (map TypedIdentifier.name tis) nin)))
            sub)

-- The per-instruction walk: each `WSteps` conjunct from its step's
-- inversion, transported to the final memory.
wsteps-go : ∀ {P S s} is st
  → WF-run P S is st
  → run P S st is ≡ just s
  → WSteps S (witness-of P s) is
wsteps-go [] st _ _ = tt
wsteps-go {P} {S} {s} (assert cond ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (assert cond) is st req
      (_ , m⊑₁) = step-extends {P} {S} {st} {st-mid} (assert cond) seq of
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      m⊑s = ⊑-trans {State.mem st} {State.mem st-mid} {State.mem s} m⊑₁ m⊑₂
      (steq , x , rc , tb) = step→bwd {P} {S} {st} {st-mid} (assert cond) seq
  in ( x
     , trans (resolveᶜ-Fr-agree P s cond)
         (resolveᶠ-mono {State.mem st} {State.mem s} cond m⊑s rc)
     , tb )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (impact guard inputs ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (impact guard inputs) is st req
      (_ , m⊑₁) = step-extends {P} {S} {st} {st-mid} (impact guard inputs) seq of
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      m⊑s = ⊑-trans {State.mem st} {State.mem st-mid} {State.mem s} m⊑₁ m⊑₂
      (vals , gᶠ , rvals , rg , branch) = step→bwd {P} {S} {st} {st-mid} (impact guard inputs) seq
  in ( gᶠ
     , trans (resolveᶜ-Fr-agree P s guard)
         (resolveᶠ-mono {State.mem st} {State.mem s} guard m⊑s rg)
     , (case branch of λ
         { (inj₁ (tb , _ , _)) → true  , tb
         ; (inj₂ (tb , _))     → false , tb }) )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (reconstitute-field divisor modulus bits o ∷ is) st
  (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (reconstitute-field divisor modulus bits o) is st req
      (_ , m⊑₁) = step-extends {P} {S} {st} {st-mid} (reconstitute-field divisor modulus bits o)
                    seq of
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      m⊑s = ⊑-trans {State.mem st} {State.mem st-mid} {State.mem s} m⊑₁ m⊑₂
      (o¹ , d , rd , mo , rm , novf) =
        step→bwd {P} {S} {st} {st-mid} (reconstitute-field divisor modulus bits o) seq
  in ( d , mo
     , trans (resolveᶜ-Fr-agree P s divisor)
         (resolveᶠ-mono {State.mem st} {State.mem s} divisor m⊑s rd)
     , trans (resolveᶜ-Fr-agree P s modulus)
         (resolveᶠ-mono {State.mem st} {State.mem s} modulus m⊑s rm)
     , novf )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (div-mod-power-of-two val bits [] ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (div-mod-power-of-two val bits []) is st req
  in case step→bwd {P} {S} {st} {st-mid} (div-mod-power-of-two val bits []) seq of λ ()
wsteps-go {P} {S} {s} (div-mod-power-of-two val bits (q ∷ []) ∷ is) st
  (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (div-mod-power-of-two val bits (q ∷ [])) is st req
  in case step→bwd {P} {S} {st} {st-mid} (div-mod-power-of-two val bits (q ∷ [])) seq of λ ()
wsteps-go {P} {S} {s} (div-mod-power-of-two val bits (q ∷ r ∷ []) ∷ is) st
  (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (div-mod-power-of-two val bits (q ∷ r ∷ [])) is st req
  in (q , r , refl) , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (div-mod-power-of-two val bits (q ∷ r ∷ z ∷ zs) ∷ is) st
  (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (div-mod-power-of-two val bits (q ∷ r ∷ z ∷ zs)) is st req
  in case step→bwd {P} {S} {st} {st-mid} (div-mod-power-of-two val bits (q ∷ r ∷ z ∷ zs)) seq of
       λ ()
wsteps-go {P} {S} {s} (circuit-output vals ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (circuit-output vals) is st req
      (_ , m⊑₁) = step-extends {P} {S} {st} {st-mid} (circuit-output vals) seq of
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      m⊑s = ⊑-trans {State.mem st} {State.mem st-mid} {State.mem s} m⊑₁ m⊑₂
      (vs , ceq , steq) = step→bwd {P} {S} {st} {st-mid} (circuit-output vals) seq
  in ( vs
     , collectOutputs-mono {State.mem st} {State.mem s}
         (IrSource.outputs S) vals m⊑s ceq )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (from-bytes32 bytes native o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (from-bytes32 bytes native o) is st req
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      (n , steq) = step→bwd {P} {S} {st} {st-mid} (from-bytes32 bytes native o) seq
  in ( val-native n
     , m⊑₂ (trans (cong (λ z → State.mem z o) steq)
              (ins-here o (val-native n) (State.mem st)))
     , refl )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (from-bytes32 bytes bytes32 o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (from-bytes32 bytes bytes32 o) is st req
  in case step→bwd {P} {S} {st} {st-mid} (from-bytes32 bytes bytes32 o) seq of λ ()
wsteps-go {P} {S} {s} (from-bytes32 bytes jubjub-point o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (from-bytes32 bytes jubjub-point o) is st req
  in case step→bwd {P} {S} {st} {st-mid} (from-bytes32 bytes jubjub-point o) seq of λ ()
wsteps-go {P} {S} {s} (from-bytes32 bytes jubjub-scalar o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (from-bytes32 bytes jubjub-scalar o) is st req
  in case step→bwd {P} {S} {st} {st-mid} (from-bytes32 bytes jubjub-scalar o) seq of λ ()
wsteps-go {P} {S} {s} (from-bytes32 bytes secp-point o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (from-bytes32 bytes secp-point o) is st req
  in case step→bwd {P} {S} {st} {st-mid} (from-bytes32 bytes secp-point o) seq of λ ()
wsteps-go {P} {S} {s} (from-bytes32 bytes secp-base o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (from-bytes32 bytes secp-base o) is st req
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      (x , steq) = step→bwd {P} {S} {st} {st-mid} (from-bytes32 bytes secp-base o) seq
  in ( val-secp-base x
     , m⊑₂ (trans (cong (λ z → State.mem z o) steq)
              (ins-here o (val-secp-base x) (State.mem st)))
     , refl )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (from-bytes32 bytes secp-scalar o ∷ is) st
  (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (from-bytes32 bytes secp-scalar o) is st req
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      (x , steq) = step→bwd {P} {S} {st} {st-mid} (from-bytes32 bytes secp-scalar o) seq
  in ( val-secp-scalar x
     , m⊑₂ (trans (cong (λ z → State.mem z o) steq)
              (ins-here o (val-secp-scalar x) (State.mem st)))
     , refl )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (less-than a b bits o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (less-than a b bits o) is st req
      (_ , m⊑₁) = step-extends {P} {S} {st} {st-mid} (less-than a b bits o) seq of
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      m⊑s = ⊑-trans {State.mem st} {State.mem st-mid} {State.mem s} m⊑₁ m⊑₂
      (o¹ , (x , ra , px) , (y , rb , py)) =
        step→bwd {P} {S} {st} {st-mid} (less-than a b bits o) seq
  in ( x , y
     , trans (resolveᶜ-Fr-agree P s a)
         (resolveᶠ-mono {State.mem st} {State.mem s} a m⊑s ra)
     , trans (resolveᶜ-Fr-agree P s b)
         (resolveᶠ-mono {State.mem st} {State.mem s} b m⊑s rb)
     , px , py )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (public-input guard val-t o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (public-input guard val-t o) is st req
      (_ , m⊑₁) = step-extends {P} {S} {st} {st-mid} (public-input guard val-t o) seq of
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      m⊑s = ⊑-trans {State.mem st} {State.mem st-mid} {State.mem s} m⊑₁ m⊑₂
  in ( case step→bwd {P} {S} {st} {st-mid} (public-input guard val-t o) seq of λ
       { (inj₁ (geq , v , deq , steq)) →
           true
         , eval-guard-mono {State.mem st} {State.mem s} guard m⊑s geq
         , ( v
           , m⊑₂ (trans (cong (λ z → State.mem z o) steq)
                    (ins-here o v (State.mem st)))
           , decode-typeof val-t _ deq )
       ; (inj₂ (geq , steq)) →
           false
         , eval-guard-mono {State.mem st} {State.mem s} guard m⊑s geq
         , m⊑₂ (trans (cong (λ z → State.mem z o) steq)
                  (ins-here o (default-val val-t) (State.mem st))) } )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (private-input guard val-t o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (private-input guard val-t o) is st req
      (_ , m⊑₁) = step-extends {P} {S} {st} {st-mid} (private-input guard val-t o) seq of
      (m⊑₂ , _) = run-extends {P} {S} {s} is st-mid rrest (wfr seq)
      m⊑s = ⊑-trans {State.mem st} {State.mem st-mid} {State.mem s} m⊑₁ m⊑₂
  in ( case step→bwd {P} {S} {st} {st-mid} (private-input guard val-t o) seq of λ
       { (inj₁ (geq , v , deq , steq)) →
           true
         , eval-guard-mono {State.mem st} {State.mem s} guard m⊑s geq
         , ( v
           , m⊑₂ (trans (cong (λ z → State.mem z o) steq)
                    (ins-here o v (State.mem st)))
           , decode-typeof val-t _ deq )
       ; (inj₂ (geq , steq)) →
           false
         , eval-guard-mono {State.mem st} {State.mem s} guard m⊑s geq
         , m⊑₂ (trans (cong (λ z → State.mem z o) steq)
                  (ins-here o (default-val val-t) (State.mem st))) } )
   , wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (encode input outs ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (encode input outs) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (cond-select bit a b o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (cond-select bit a b o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (constrain-bits val bits ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (constrain-bits val bits) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (constrain-eq a b ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (constrain-eq a b) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (constrain-to-boolean val ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (constrain-to-boolean val) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (copy val o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (copy val o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (ec-mul a scalar o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (ec-mul a scalar o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (ec-mul-generator scalar o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (ec-mul-generator scalar o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (hash-to-curve inputs o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (hash-to-curve inputs o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (into-coordinates point xy ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (into-coordinates point xy) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (from-coordinates xy o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (from-coordinates xy o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (into-bytes32 input o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (into-bytes32 input o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (reverse-bytes bytes o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (reverse-bytes bytes o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (bytes32-into-low-high bytes lohi ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (bytes32-into-low-high bytes lohi) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (bytes32-from-low-high lohi o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (bytes32-from-low-high lohi o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (transient-hash inputs o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (transient-hash inputs o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (persistent-hash al inputs o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (persistent-hash al inputs o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (keccak256 al inputs o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (keccak256 al inputs o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (test-eq a b o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (test-eq a b o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (add a b o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (add a b o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (mul a b o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (mul a b o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (neg a o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (neg a o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (inv a o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (inv a o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (not a o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) = run-inv {P} {S} {s} (not a o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest
wsteps-go {P} {S} {s} (jubjub-scalar-from-native a o ∷ is) st (of , wfr) req =
  let (st-mid , seq , rrest) =
        run-inv {P} {S} {s} (jubjub-scalar-from-native a o) is st req
  in wsteps-go {P} {S} {s} is st-mid (wfr seq) rrest

-- `WShape` at the canonical witness of a successful preprocess.
preprocess→WShape : ∀ {S P s st0}
  → producer-SA S
  → init S P ≡ just st0
  → preprocess S P ≡ just s
  → WShape S (witness-of P s)
preprocess→WShape {S} {P} {s} {st0} sa ieq peq =
  let (walk , _) = preprocess-walk-consumed {S} {P} {s} {st0} ieq peq
      wf = producer-safe→WF {S} {P} {st0} sa ieq
      (m⊑ , _) = run-extends {P} {S} {s} (IrSource.instructions S) st0 walk wf
  in winputs-of {P} {s} (IrSource.inputs S)
       (init-decode {S} {P} {st0} ieq) (proj₁ sa) m⊑
   , wsteps-go {P} {S} {s} (IrSource.instructions S) st0 wf walk
