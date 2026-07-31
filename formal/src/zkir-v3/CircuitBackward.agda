{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Per-instruction BACKWARD faithfulness of zkir-v3.
--
-- The converse of the `*-fwd` lemmas: for each instruction, from the
-- emitted constraint holding at the immediate post-step witness (plus
-- output freshness and a minimal type-support hypothesis) reconstruct
-- the off-circuit `step`.  The backward driver (`bwd-go`,
-- CircuitProof.agda) feeds each lemma its `holds` premise via
-- `holds-lower` + the spine's monotonicity data.
------------------------------------------------------------------------

module zkir-v3.CircuitBackward (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Semantics ⋯ using (ProofPreimage; State; resolve; resolveᶠ)
open import zkir-v3.SemanticsProperties ⋯
open import zkir-v3.Circuit ⋯
open import zkir-v3.CircuitBridge ⋯

open import zkir-v3.Semantics ⋯
  using (Mem; ins; step; to𝔹; out1; valEq?; resolve-all-Fr)
open import zkir-v3.Semantics ⋯
  using (insertMany; eval-guard; collectOutputs; _≟LFr_; default-val; run)
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)

open import Data.Bool using (Bool; true; false; if_then_else_)
  renaming (not to bnot)
open import Data.Maybe using (Maybe; just; nothing; _>>=_)
open import Data.Nat using (ℕ; _+_; _*_; _<?_; _<_; _^_; _∸_)
open import Data.List using (List; []; _∷_; map; _++_; length; take; drop)
open import Data.Product using (_×_; _,_; ∃; proj₁; proj₂)
open import Data.Sum using (inj₁; inj₂; _⊎_)
open import Data.Unit using (⊤; tt)
open import Data.Empty using (⊥)
open import Data.String using () renaming (_≟_ to _≟str_)
open import Function using (case_of_)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; cong₂)
open import Relation.Nullary using (yes; no; ¬_)
open import Relation.Nullary.Decidable using (isYes)
open import Data.Maybe.Properties using (just-injective)

------------------------------------------------------------------------
-- Per-instruction BACKWARD faithfulness  (deterministic fragment).
--
-- The converse of the `*-fwd` lemmas: from the emitted constraint holding
-- at the immediate post-step witness `witness-of P (out1 st out v)` (the
-- state where `out` is freshly bound to `v`), plus output freshness and a
-- minimal type-support hypothesis, conclude the off-circuit step produces
-- exactly that state.
--
-- The type-support hypothesis is stated as an off-circuit resolution at
-- `st` itself (e.g. `resolve (State.mem st) a ≡ just (val-native x)`).  It
-- does double duty: it pins the operand type into the supported case that
-- `step` needs to reduce, AND — being a resolution in `st`, where `out` is
-- fresh — it rules out an input aliasing the output (which `holds` alone
-- at the post-witness cannot, since the post-witness binds `out`).
------------------------------------------------------------------------

-- Backward transport: an operand that resolves off-circuit at `st` and
-- resolves against the post-step witness resolves to the same value.
⊢-back : ∀ P st (out : Identifier) v op {w₀ w₁}
  → State.mem st out ≡ nothing
  → resolve (State.mem st) op ≡ just w₀
  → resolveᶜ (witness-of P (out1 st out v)) op ≡ just w₁
  → w₀ ≡ w₁
⊢-back P st out v op fresh r₀ r₁ =
  just-injective (trans (sym (⊢-pres P st out v op fresh r₀)) r₁)

⊢ᶠ-back : ∀ P st (out : Identifier) v op {x₀ x₁}
  → State.mem st out ≡ nothing
  → resolveᶠ (State.mem st) op ≡ just x₀
  → resolveᶜ-Fr (witness-of P (out1 st out v)) op ≡ just x₁
  → x₀ ≡ x₁
⊢ᶠ-back P st out v op fresh r₀ r₁ =
  just-injective (trans (sym (⊢ᶠ-pres P st out v op fresh r₀)) r₁)

-- The freshly-bound cell reads back its value: `assign w out ≡ just ov`
-- forces `v ≡ ov`.
out-val : ∀ P st (out : Identifier) v {ov}
  → CircuitWitness.assign (witness-of P (out1 st out v)) out ≡ just ov
  → v ≡ ov
out-val P st out v h = just-injective (trans (sym (assign-here P st out v)) h)

-- `assign w out ≡ just ov` together with `ov ≡ e` gives the output value
-- `e` that off-circuit `step` wrote back equal to the bound `v` — in the
-- orientation `step`'s reduced state needs (`e ≡ v`).
out-back : ∀ P st (out : Identifier) v {ov e}
  → CircuitWitness.assign (witness-of P (out1 st out v)) out ≡ just ov
  → ov ≡ e → e ≡ v
out-back P st out v h ov≡e = sym (trans (out-val P st out v h) ov≡e)

------------------------------------------------------------------------
-- copy  (single native/any-type input; type already pinned by the
-- resolution, no extra type hypothesis).
------------------------------------------------------------------------

copy-bwd : ∀ {P S st output val v w}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) val ≡ just w
  → holds (witness-of P (out1 st output v)) (gate-copy output val)
  → step P S st (copy val output) ≡ just (out1 st output v)
copy-bwd {P} {S} {st} {output} {val} {v} {w} fresh rv (av , ⊢a , ⊢out)
  rewrite rv =
    cong (λ z → just (out1 st output z))
      (trans (⊢-back P st output v val fresh rv ⊢a)
             (sym (out-val P st output v ⊢out)))

------------------------------------------------------------------------
-- constrain-eq  (no output; post-state is `st`).
--
-- Type support: the operand resolves to a value on which `valEq?` reads
-- reflexively `true` — i.e. a `valEq?`-supported type (Native / Bytes32 /
-- JubjubPoint; JubjubScalar is excluded, `valEq?` being `nothing` there).
------------------------------------------------------------------------

constrain-eq-bwd : ∀ {P S st a b}
  → (∃ λ av → resolve (State.mem st) a ≡ just av
                   × valEq? av av ≡ just true)
  → holds (witness-of P st) (eq a b)
  → step P S st (constrain-eq a b) ≡ just st
constrain-eq-bwd {P} {S} {st} {a} {b} (av , ra , veq) (v , ⊢a , ⊢b)
  with just-injective
         (trans (sym ra) (trans (sym (resolve-agree P st a)) ⊢a))
... | refl
  rewrite ra
        | trans (sym (resolve-agree P st b)) ⊢b
        | veq = refl

------------------------------------------------------------------------
-- add   (Native or JubjubPoint, type-directed).
------------------------------------------------------------------------

add-bwd : ∀ {P S st a b output v}
  → State.mem st output ≡ nothing
  → ( (∃ λ x → ∃ λ y →
         resolve (State.mem st) a ≡ just (val-native x)
       × resolve (State.mem st) b ≡ just (val-native y))
    ⊎ (∃ λ p → ∃ λ q →
         resolve (State.mem st) a ≡ just (val-jubjub-point p)
       × resolve (State.mem st) b ≡ just (val-jubjub-point q))
    ⊎ (∃ λ p → ∃ λ q →
         resolve (State.mem st) a ≡ just (val-secp-point p)
       × resolve (State.mem st) b ≡ just (val-secp-point q))
    ⊎ (∃ λ x → ∃ λ y →
         resolve (State.mem st) a ≡ just (val-secp-base x)
       × resolve (State.mem st) b ≡ just (val-secp-base y))
    ⊎ (∃ λ x → ∃ λ y →
         resolve (State.mem st) a ≡ just (val-secp-scalar x)
       × resolve (State.mem st) b ≡ just (val-secp-scalar y)))
  → holds (witness-of P (out1 st output v)) (gate-add output a b)
  → step P S st (add a b output) ≡ just (out1 st output v)
add-bwd {P} {S} {st} {a} {b} {output} {v} fresh (inj₁ (x , y , ra , rb))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₂ (inj₁ (_ , _ , () , _))
... | inj₂ (inj₂ (inj₁ (_ , _ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , _ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , _ , () , _))))
... | inj₁ (x' , y' , eax , eby , eov) with eax | eby
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
add-bwd {P} {S} {st} {a} {b} {output} {v} fresh
  (inj₂ (inj₁ (p , q , ra , rb)))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₁ (_ , _ , () , _)
... | inj₂ (inj₂ (inj₁ (_ , _ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , _ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , _ , () , _))))
... | inj₂ (inj₁ (p' , q' , eap , ebq , eov)) with eap | ebq
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
add-bwd {P} {S} {st} {a} {b} {output} {v} fresh
  (inj₂ (inj₂ (inj₁ (p , q , ra , rb))))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₁ (_ , _ , () , _)
... | inj₂ (inj₁ (_ , _ , () , _))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , _ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , _ , () , _))))
... | inj₂ (inj₂ (inj₁ (p' , q' , eap , ebq , eov))) with eap | ebq
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
add-bwd {P} {S} {st} {a} {b} {output} {v} fresh
  (inj₂ (inj₂ (inj₂ (inj₁ (x , y , ra , rb)))))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₁ (_ , _ , () , _)
... | inj₂ (inj₁ (_ , _ , () , _))
... | inj₂ (inj₂ (inj₁ (_ , _ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , _ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₁ (x' , y' , eax , eby , eov)))) with eax | eby
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
add-bwd {P} {S} {st} {a} {b} {output} {v} fresh
  (inj₂ (inj₂ (inj₂ (inj₂ (x , y , ra , rb)))))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₁ (_ , _ , () , _)
... | inj₂ (inj₁ (_ , _ , () , _))
... | inj₂ (inj₂ (inj₁ (_ , _ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , _ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (x' , y' , eax , eby , eov)))) with eax | eby
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)

------------------------------------------------------------------------
-- mul   (Native).
------------------------------------------------------------------------

mul-bwd : ∀ {P S st a b output v}
  → State.mem st output ≡ nothing
  → ( (∃ λ x → ∃ λ y →
         resolve (State.mem st) a ≡ just (val-native x)
       × resolve (State.mem st) b ≡ just (val-native y))
    ⊎ (∃ λ x → ∃ λ y →
         resolve (State.mem st) a ≡ just (val-secp-base x)
       × resolve (State.mem st) b ≡ just (val-secp-base y))
    ⊎ (∃ λ x → ∃ λ y →
         resolve (State.mem st) a ≡ just (val-secp-scalar x)
       × resolve (State.mem st) b ≡ just (val-secp-scalar y)))
  → holds (witness-of P (out1 st output v)) (gate-mul output a b)
  → step P S st (mul a b output) ≡ just (out1 st output v)
mul-bwd {P} {S} {st} {a} {b} {output} {v} fresh (inj₁ (x , y , ra , rb))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₂ (inj₁ (_ , _ , () , _))
... | inj₂ (inj₂ (_ , _ , () , _))
... | inj₁ (x' , y' , eax , eby , eov) with eax | eby
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
mul-bwd {P} {S} {st} {a} {b} {output} {v} fresh
  (inj₂ (inj₁ (x , y , ra , rb)))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₁ (_ , _ , () , _)
... | inj₂ (inj₂ (_ , _ , () , _))
... | inj₂ (inj₁ (x' , y' , eax , eby , eov)) with eax | eby
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
mul-bwd {P} {S} {st} {a} {b} {output} {v} fresh
  (inj₂ (inj₂ (x , y , ra , rb)))
  (av , bv , ov , ⊢a , ⊢b , ⊢out , disj)
  rewrite ra | rb
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl with disj
... | inj₁ (_ , _ , () , _)
... | inj₂ (inj₁ (_ , _ , () , _))
... | inj₂ (inj₂ (x' , y' , eax , eby , eov)) with eax | eby
...   | refl | refl =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)

------------------------------------------------------------------------
-- neg   (Native or JubjubPoint).
------------------------------------------------------------------------

neg-bwd : ∀ {P S st a output v}
  → State.mem st output ≡ nothing
  → ( (∃ λ x → resolve (State.mem st) a ≡ just (val-native x))
    ⊎ (∃ λ p → resolve (State.mem st) a ≡ just (val-jubjub-point p))
    ⊎ (∃ λ p → resolve (State.mem st) a ≡ just (val-secp-point p))
    ⊎ (∃ λ x → resolve (State.mem st) a ≡ just (val-secp-base x))
    ⊎ (∃ λ x → resolve (State.mem st) a ≡ just (val-secp-scalar x)))
  → holds (witness-of P (out1 st output v)) (gate-neg output a)
  → step P S st (neg a output) ≡ just (out1 st output v)
neg-bwd {P} {S} {st} {a} {output} {v} fresh (inj₁ (x , ra))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₂ (inj₁ (_ , () , _))
... | inj₂ (inj₂ (inj₁ (_ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , () , _))))
... | inj₁ (x' , eax , eov) with eax
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)
neg-bwd {P} {S} {st} {a} {output} {v} fresh (inj₂ (inj₁ (p , ra)))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₁ (_ , () , _)
... | inj₂ (inj₂ (inj₁ (_ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , () , _))))
... | inj₂ (inj₁ (p' , eap , eov)) with eap
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)
neg-bwd {P} {S} {st} {a} {output} {v} fresh (inj₂ (inj₂ (inj₁ (p , ra))))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₁ (_ , () , _)
... | inj₂ (inj₁ (_ , () , _))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , () , _))))
... | inj₂ (inj₂ (inj₁ (p' , eap , eov))) with eap
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)
neg-bwd {P} {S} {st} {a} {output} {v} fresh
  (inj₂ (inj₂ (inj₂ (inj₁ (x , ra)))))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₁ (_ , () , _)
... | inj₂ (inj₁ (_ , () , _))
... | inj₂ (inj₂ (inj₁ (_ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₂ (_ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₁ (x' , eax , eov)))) with eax
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)
neg-bwd {P} {S} {st} {a} {output} {v} fresh
  (inj₂ (inj₂ (inj₂ (inj₂ (x , ra)))))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₁ (_ , () , _)
... | inj₂ (inj₁ (_ , () , _))
... | inj₂ (inj₂ (inj₁ (_ , () , _)))
... | inj₂ (inj₂ (inj₂ (inj₁ (_ , () , _))))
... | inj₂ (inj₂ (inj₂ (inj₂ (x' , eax , eov)))) with eax
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)

------------------------------------------------------------------------
-- inv   (Native; `invᶠ` is the off-circuit inverse).
------------------------------------------------------------------------

inv-bwd : ∀ {P S st a output v}
  → State.mem st output ≡ nothing
  → ( (∃ λ x → resolve (State.mem st) a ≡ just (val-native x))
    ⊎ (∃ λ x → resolve (State.mem st) a ≡ just (val-secp-base x))
    ⊎ (∃ λ x → resolve (State.mem st) a ≡ just (val-secp-scalar x)))
  → holds (witness-of P (out1 st output v)) (gate-inv output a)
  → step P S st (inv a output) ≡ just (out1 st output v)
inv-bwd {P} {S} {st} {a} {output} {v} fresh (inj₁ (x , ra))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₂ (inj₁ (_ , _ , () , _ , _))
... | inj₂ (inj₂ (_ , _ , () , _ , _))
... | inj₁ (x' , xi , eax , ei , eov) with eax
...   | refl rewrite ei =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
inv-bwd {P} {S} {st} {a} {output} {v} fresh (inj₂ (inj₁ (x , ra)))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₁ (_ , _ , () , _ , _)
... | inj₂ (inj₂ (_ , _ , () , _ , _))
... | inj₂ (inj₁ (x' , xi , eax , ei , eov)) with eax
...   | refl rewrite ei =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)
inv-bwd {P} {S} {st} {a} {output} {v} fresh (inj₂ (inj₂ (x , ra)))
  (av , ov , ⊢a , ⊢out , disj)
  rewrite ra with ⊢-back P st output v a fresh ra ⊢a
... | refl with disj
... | inj₁ (_ , _ , () , _ , _)
... | inj₂ (inj₁ (_ , _ , () , _ , _))
... | inj₂ (inj₂ (x' , xi , eax , ei , eov)) with eax
...   | refl rewrite ei =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out eov)

------------------------------------------------------------------------
-- test-eq   (output is the 0/1 equality flag; `valEq?`-supported inputs).
------------------------------------------------------------------------

test-eq-bwd : ∀ {P S st a b output v}
  → State.mem st output ≡ nothing
  → (∃ λ av → resolve (State.mem st) a ≡ just av)
  → (∃ λ bv → resolve (State.mem st) b ≡ just bv)
  → holds (witness-of P (out1 st output v)) (test-eq output a b)
  → step P S st (test-eq a b output) ≡ just (out1 st output v)
test-eq-bwd {P} {S} {st} {a} {b} {output} {v} fresh
  (av₀ , ra) (bv₀ , rb) (av , bv , e , ⊢a , ⊢b , veq , ⊢out)
  with ⊢-back P st output v a fresh ra ⊢a
     | ⊢-back P st output v b fresh rb ⊢b
... | refl | refl
  rewrite ra | rb | veq =
      cong (λ z → just (out1 st output z))
        (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- jubjub-scalar-from-native.
------------------------------------------------------------------------

jubjub-scalar-from-native-bwd : ∀ {P S st a output v}
  → State.mem st output ≡ nothing
  → (∃ λ x → resolveᶠ (State.mem st) a ≡ just x)
  → holds (witness-of P (out1 st output v)) (scalar-from-native output a)
  → step P S st (jubjub-scalar-from-native a output) ≡ just (out1 st output v)
jubjub-scalar-from-native-bwd {P} {S} {st} {a} {output} {v} fresh
  (x , ra) (x' , ⊢a , ⊢out)
  with ⊢ᶠ-back P st output v a fresh ra ⊢a
... | refl rewrite ra =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- ec-mul   (JubjubPoint × JubjubScalar).
------------------------------------------------------------------------

ec-mul-bwd : ∀ {P S st a scalar output v}
  → State.mem st output ≡ nothing
  → ( ((∃ λ p → resolve (State.mem st) a ≡ just (val-jubjub-point p))
      × (∃ λ s → resolve (State.mem st) scalar ≡ just (val-jubjub-scalar s)))
    ⊎ ((∃ λ p → resolve (State.mem st) a ≡ just (val-secp-point p))
      × (∃ λ s → resolve (State.mem st) scalar ≡ just (val-secp-scalar s))))
  → holds (witness-of P (out1 st output v)) (ec-mul output a scalar)
  → step P S st (ec-mul a scalar output) ≡ just (out1 st output v)
ec-mul-bwd {P} {S} {st} {a} {scalar} {output} {v} fresh
  (inj₁ ((p , ra) , (s , rs))) (inj₁ (p' , s' , ⊢a , ⊢s , ⊢out))
  with ⊢-back P st output v a      fresh ra ⊢a
     | ⊢-back P st output v scalar fresh rs ⊢s
... | refl | refl rewrite ra | rs =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)
ec-mul-bwd {P} {S} {st} {a} {scalar} {output} {v} fresh
  (inj₁ ((p , ra) , _)) (inj₂ (p' , s' , ⊢a , _))
  = case ⊢-back P st output v a fresh ra ⊢a of λ ()
ec-mul-bwd {P} {S} {st} {a} {scalar} {output} {v} fresh
  (inj₂ ((p , ra) , (s , rs))) (inj₂ (p' , s' , ⊢a , ⊢s , ⊢out))
  with ⊢-back P st output v a      fresh ra ⊢a
     | ⊢-back P st output v scalar fresh rs ⊢s
... | refl | refl rewrite ra | rs =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)
ec-mul-bwd {P} {S} {st} {a} {scalar} {output} {v} fresh
  (inj₂ ((p , ra) , _)) (inj₁ (p' , s' , ⊢a , _))
  = case ⊢-back P st output v a fresh ra ⊢a of λ ()

------------------------------------------------------------------------
-- ec-mul-generator   (JubjubScalar).
------------------------------------------------------------------------

ec-mul-generator-bwd : ∀ {P S st scalar output v}
  → State.mem st output ≡ nothing
  → ( (∃ λ s → resolve (State.mem st) scalar ≡ just (val-jubjub-scalar s))
    ⊎ (∃ λ s → resolve (State.mem st) scalar ≡ just (val-secp-scalar s)))
  → holds (witness-of P (out1 st output v)) (ec-gen output scalar)
  → step P S st (ec-mul-generator scalar output) ≡ just (out1 st output v)
ec-mul-generator-bwd {P} {S} {st} {scalar} {output} {v} fresh
  (inj₁ (s , rs)) (inj₁ (s' , ⊢s , ⊢out))
  with ⊢-back P st output v scalar fresh rs ⊢s
... | refl rewrite rs =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)
ec-mul-generator-bwd {P} {S} {st} {scalar} {output} {v} fresh
  (inj₁ (s , rs)) (inj₂ (s' , ⊢s , _))
  = case ⊢-back P st output v scalar fresh rs ⊢s of λ ()
ec-mul-generator-bwd {P} {S} {st} {scalar} {output} {v} fresh
  (inj₂ (s , rs)) (inj₂ (s' , ⊢s , ⊢out))
  with ⊢-back P st output v scalar fresh rs ⊢s
... | refl rewrite rs =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)
ec-mul-generator-bwd {P} {S} {st} {scalar} {output} {v} fresh
  (inj₂ (s , rs)) (inj₁ (s' , ⊢s , _))
  = case ⊢-back P st output v scalar fresh rs ⊢s of λ ()

------------------------------------------------------------------------
-- Backward list transport (converse of `⊢all-pres`).
------------------------------------------------------------------------

⊢all-back : ∀ P st (out : Identifier) v ops {frs₀ frs₁}
  → State.mem st out ≡ nothing
  → resolve-all-Fr (State.mem st) ops ≡ just frs₀
  → resolveᶜ-all-Fr (witness-of P (out1 st out v)) ops ≡ just frs₁
  → frs₀ ≡ frs₁
⊢all-back P st out v ops fresh r₀ r₁ =
  just-injective (trans (sym (⊢all-pres P st out v ops fresh r₀)) r₁)

------------------------------------------------------------------------
-- hash-to-curve   (Native inputs → JubjubPoint).
------------------------------------------------------------------------

hash-to-curve-bwd : ∀ {P S st inputs output v}
  → State.mem st output ≡ nothing
  → (∃ λ frs → resolve-all-Fr (State.mem st) inputs ≡ just frs)
  → holds (witness-of P (out1 st output v)) (h2c output inputs)
  → step P S st (hash-to-curve inputs output) ≡ just (out1 st output v)
hash-to-curve-bwd {P} {S} {st} {inputs} {output} {v} fresh
  (frs , ri) (frs' , ⊢i , ⊢out)
  with ⊢all-back P st output v inputs fresh ri ⊢i
... | refl rewrite ri =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- transient-hash   (Native inputs → Native).
------------------------------------------------------------------------

transient-hash-bwd : ∀ {P S st inputs output v}
  → State.mem st output ≡ nothing
  → (∃ λ frs → resolve-all-Fr (State.mem st) inputs ≡ just frs)
  → holds (witness-of P (out1 st output v)) (poseidon output inputs)
  → step P S st (transient-hash inputs output) ≡ just (out1 st output v)
transient-hash-bwd {P} {S} {st} {inputs} {output} {v} fresh
  (frs , ri) (frs' , ⊢i , ⊢out)
  with ⊢all-back P st output v inputs fresh ri ⊢i
... | refl rewrite ri =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- into-bytes32   (Native → Bytes32).
------------------------------------------------------------------------

into-bytes32-bwd : ∀ {P S st input output v}
  → State.mem st output ≡ nothing
  → ( (∃ λ x → resolve (State.mem st) input ≡ just (val-native x))
    ⊎ (∃ λ x → resolve (State.mem st) input ≡ just (val-secp-base x))
    ⊎ (∃ λ s → resolve (State.mem st) input ≡ just (val-secp-scalar s)))
  → holds (witness-of P (out1 st output v)) (into-bytes output input)
  → step P S st (into-bytes32 input output) ≡ just (out1 st output v)
into-bytes32-bwd {P} {S} {st} {input} {output} {v} fresh (inj₁ (x , ri))
  (av , ov , ⊢i , ⊢out , disj)
  rewrite ri with ⊢-back P st output v input fresh ri ⊢i
... | refl with disj
... | inj₂ (inj₁ (_ , () , _))
... | inj₂ (inj₂ (_ , () , _))
... | inj₁ (x' , eax , eov) with eax
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)
into-bytes32-bwd {P} {S} {st} {input} {output} {v} fresh
  (inj₂ (inj₁ (x , ri))) (av , ov , ⊢i , ⊢out , disj)
  rewrite ri with ⊢-back P st output v input fresh ri ⊢i
... | refl with disj
... | inj₁ (_ , () , _)
... | inj₂ (inj₂ (_ , () , _))
... | inj₂ (inj₁ (x' , eax , eov)) with eax
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)
into-bytes32-bwd {P} {S} {st} {input} {output} {v} fresh
  (inj₂ (inj₂ (s , ri))) (av , ov , ⊢i , ⊢out , disj)
  rewrite ri with ⊢-back P st output v input fresh ri ⊢i
... | refl with disj
... | inj₁ (_ , () , _)
... | inj₂ (inj₁ (_ , () , _))
... | inj₂ (inj₂ (s' , eax , eov)) with eax
...   | refl = cong (λ z → just (out1 st output z))
                 (out-back P st output v ⊢out eov)

------------------------------------------------------------------------
-- from-bytes32   (Bytes32 → Native / SecpBase / SecpScalar).
--
-- The target type `val-t` is part of the instruction and selects the
-- conversion chip; `step` succeeds at the three field targets and the
-- output is a total function of the resolved bytes.  Because the emitted
-- constraint `from-bytes` leaves the output type as a *disjunction*, the
-- output value is not pinned by `holds`; the run's own step determines it.
-- One lemma per succeeding `val-t`, each stating the concrete output.
------------------------------------------------------------------------

from-bytes32-native-bwd : ∀ {P S st bytes output b}
  → resolve (State.mem st) bytes ≡ just (val-bytes32 b)
  → step P S st (from-bytes32 bytes native output)
      ≡ just (out1 st output (val-native (nativeFromBytes b)))
from-bytes32-native-bwd rb rewrite rb = refl

from-bytes32-secp-base-bwd : ∀ {P S st bytes output b}
  → resolve (State.mem st) bytes ≡ just (val-bytes32 b)
  → step P S st (from-bytes32 bytes secp-base output)
      ≡ just (out1 st output (val-secp-base (secpBaseFromBytes b)))
from-bytes32-secp-base-bwd rb rewrite rb = refl

from-bytes32-secp-scalar-bwd : ∀ {P S st bytes output b}
  → resolve (State.mem st) bytes ≡ just (val-bytes32 b)
  → step P S st (from-bytes32 bytes secp-scalar output)
      ≡ just (out1 st output (val-secp-scalar (secpScalarFromBytes b)))
from-bytes32-secp-scalar-bwd rb rewrite rb = refl

------------------------------------------------------------------------
-- reverse-bytes   (Bytes32 → Bytes32).
------------------------------------------------------------------------

reverse-bytes-bwd : ∀ {P S st bytes output v}
  → State.mem st output ≡ nothing
  → (∃ λ b → resolve (State.mem st) bytes ≡ just (val-bytes32 b))
  → holds (witness-of P (out1 st output v)) (reverse-bytes output bytes)
  → step P S st (reverse-bytes bytes output) ≡ just (out1 st output v)
reverse-bytes-bwd {P} {S} {st} {bytes} {output} {v} fresh
  (b , rb) (bs , ⊢b , ⊢out)
  with ⊢-back P st output v bytes fresh rb ⊢b
... | refl rewrite rb =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- from-coordinates   (Native × Native → JubjubPoint).
------------------------------------------------------------------------

from-coordinates-bwd : ∀ {P S st xop yop output v}
  → State.mem st output ≡ nothing
  → ( ((∃ λ x → resolve (State.mem st) xop ≡ just (val-native x))
      × (∃ λ y → resolve (State.mem st) yop ≡ just (val-native y)))
    ⊎ ((∃ λ x → resolve (State.mem st) xop ≡ just (val-secp-base x))
      × (∃ λ y → resolve (State.mem st) yop ≡ just (val-secp-base y))))
  → holds (witness-of P (out1 st output v)) (from-coords output xop yop)
  → step P S st (from-coordinates (xop , yop) output) ≡ just (out1 st output v)
from-coordinates-bwd {P} {S} {st} {xop} {yop} {output} {v} fresh
  (inj₁ ((x , rx) , (y , ry))) (inj₁ (xv , yv , p , ⊢x , ⊢y , efc , ⊢out))
  with ⊢ᶠ-back P st output v xop fresh (resolveᶠ-intro (State.mem st) xop rx) ⊢x
     | ⊢ᶠ-back P st output v yop fresh (resolveᶠ-intro (State.mem st) yop ry) ⊢y
... | refl | refl rewrite rx | ry | efc =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)
from-coordinates-bwd {P} {S} {st} {xop} {yop} {output} {v} fresh
  (inj₁ ((x , rx) , _)) (inj₂ (xv , yv , p , ⊢x , _))
  = case ⊢-back P st output v xop fresh rx ⊢x of λ ()
from-coordinates-bwd {P} {S} {st} {xop} {yop} {output} {v} fresh
  (inj₂ ((x , rx) , (y , ry))) (inj₂ (xv , yv , p , ⊢x , ⊢y , efcK , ⊢out))
  with ⊢-back P st output v xop fresh rx ⊢x
     | ⊢-back P st output v yop fresh ry ⊢y
... | refl | refl rewrite rx | ry | efcK =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)
from-coordinates-bwd {P} {S} {st} {xop} {yop} {output} {v} fresh
  (inj₂ ((x , rx) , _)) (inj₁ (xv , yv , p , ⊢x , _))
  = case ⊢-back P st output v xop fresh rx
           (resolveᶜ-Fr-inv (witness-of P (out1 st output v)) xop ⊢x) of λ ()

------------------------------------------------------------------------
-- bytes32-from-low-high   (Native × Native → Bytes32).
------------------------------------------------------------------------

bytes32-from-low-high-bwd : ∀ {P S st loop hiop output v}
  → State.mem st output ≡ nothing
  → (∃ λ lo → resolveᶠ (State.mem st) loop ≡ just lo)
  → (∃ λ hi → resolveᶠ (State.mem st) hiop ≡ just hi)
  → holds (witness-of P (out1 st output v)) (bytes-from-low-high output loop hiop)
  → step P S st (bytes32-from-low-high (loop , hiop) output)
      ≡ just (out1 st output v)
bytes32-from-low-high-bwd {P} {S} {st} {loop} {hiop} {output} {v} fresh
  (lo , rl) (hi , rh) (l , h , bs , ⊢l , ⊢h , eb , ⊢out)
  with ⊢ᶠ-back P st output v loop fresh rl ⊢l
     | ⊢ᶠ-back P st output v hiop fresh rh ⊢h
... | refl | refl rewrite rl | rh | eb =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- Backward transport across two distinct fresh bindings.
------------------------------------------------------------------------

⊢-back2 : ∀ P st (i₁ : Identifier) v₁ (i₂ : Identifier) v₂ op {w₀ w₁}
  → State.mem st i₁ ≡ nothing → State.mem st i₂ ≡ nothing → ¬ (i₂ ≡ i₁)
  → resolve (State.mem st) op ≡ just w₀
  → resolveᶜ (witness-of P (out1 (out1 st i₁ v₁) i₂ v₂)) op ≡ just w₁
  → w₀ ≡ w₁
⊢-back2 P st i₁ v₁ i₂ v₂ op f₁ f₂ i₂≢i₁ r₀ r₁ =
  just-injective
    (trans (sym (⊢-pres2 P st i₁ v₁ i₂ v₂ op f₁ f₂ i₂≢i₁ r₀)) r₁)

⊢ᶠ-back2 : ∀ P st (i₁ : Identifier) v₁ (i₂ : Identifier) v₂ op {x₀ x₁}
  → State.mem st i₁ ≡ nothing → State.mem st i₂ ≡ nothing → ¬ (i₂ ≡ i₁)
  → resolveᶠ (State.mem st) op ≡ just x₀
  → resolveᶜ-Fr (witness-of P (out1 (out1 st i₁ v₁) i₂ v₂)) op ≡ just x₁
  → x₀ ≡ x₁
⊢ᶠ-back2 P st i₁ v₁ i₂ v₂ op f₁ f₂ i₂≢i₁ r₀ r₁ =
  just-injective
    (trans (sym (⊢ᶠ-pres2 P st i₁ v₁ i₂ v₂ op f₁ f₂ i₂≢i₁ r₀)) r₁)

-- The two output cells read back their bound values.
out-val-inner : ∀ P st (i₁ : Identifier) v₁ (i₂ : Identifier) v₂ {ov}
  → ¬ (i₁ ≡ i₂)
  → CircuitWitness.assign (witness-of P (out1 (out1 st i₁ v₁) i₂ v₂)) i₁
      ≡ just ov
  → ov ≡ v₁
out-val-inner P st i₁ v₁ i₂ v₂ i₁≢i₂ h =
  sym (just-injective (trans (sym (assign-inner P st i₁ v₁ i₂ v₂ i₁≢i₂)) h))

out-val-outer : ∀ P st (i₁ : Identifier) v₁ (i₂ : Identifier) v₂ {ov}
  → CircuitWitness.assign (witness-of P (out1 (out1 st i₁ v₁) i₂ v₂)) i₂
      ≡ just ov
  → ov ≡ v₂
out-val-outer P st i₁ v₁ i₂ v₂ h =
  sym (just-injective (trans (sym (assign-outer P st i₁ v₁ i₂ v₂)) h))

------------------------------------------------------------------------
-- into-coordinates   (JubjubPoint → Native × Native, two distinct outs).
------------------------------------------------------------------------

into-coordinates-bwd : ∀ {P S st point xid yid v₁ v₂}
  → State.mem st xid ≡ nothing → State.mem st yid ≡ nothing → ¬ (xid ≡ yid)
  → ( (∃ λ p → resolve (State.mem st) point ≡ just (val-jubjub-point p))
    ⊎ (∃ λ p → resolve (State.mem st) point ≡ just (val-secp-point p)))
  → holds (witness-of P (out1 (out1 st xid v₁) yid v₂)) (into-coords xid yid point)
  → step P S st (into-coordinates point (xid , yid))
      ≡ just (out1 (out1 st xid v₁) yid v₂)
into-coordinates-bwd {P} {S} {st} {point} {xid} {yid} {v₁} {v₂}
  fx fy xid≢yid (inj₁ (p , rp)) (inj₁ (p' , x , y , ⊢p , ecoords , ⊢x , ⊢y))
  with ⊢-back2 P st xid v₁ yid v₂ point fx fy (λ y≡x → xid≢yid (sym y≡x)) rp ⊢p
... | refl rewrite rp | ecoords =
      cong₂ (λ z₁ z₂ → just (out1 (out1 st xid z₁) yid z₂))
        (out-val-inner P st xid v₁ yid v₂ xid≢yid ⊢x)
        (out-val-outer P st xid v₁ yid v₂ ⊢y)
into-coordinates-bwd {P} {S} {st} {point} {xid} {yid} {v₁} {v₂}
  fx fy xid≢yid (inj₁ (p , rp)) (inj₂ (p' , x , y , ⊢p , _))
  = case ⊢-back2 P st xid v₁ yid v₂ point fx fy
           (λ y≡x → xid≢yid (sym y≡x)) rp ⊢p of λ ()
into-coordinates-bwd {P} {S} {st} {point} {xid} {yid} {v₁} {v₂}
  fx fy xid≢yid (inj₂ (p , rp)) (inj₂ (p' , x , y , ⊢p , ecoordsK , ⊢x , ⊢y))
  with ⊢-back2 P st xid v₁ yid v₂ point fx fy (λ y≡x → xid≢yid (sym y≡x)) rp ⊢p
... | refl rewrite rp | ecoordsK =
      cong₂ (λ z₁ z₂ → just (out1 (out1 st xid z₁) yid z₂))
        (out-val-inner P st xid v₁ yid v₂ xid≢yid ⊢x)
        (out-val-outer P st xid v₁ yid v₂ ⊢y)
into-coordinates-bwd {P} {S} {st} {point} {xid} {yid} {v₁} {v₂}
  fx fy xid≢yid (inj₂ (p , rp)) (inj₁ (p' , x , y , ⊢p , _))
  = case ⊢-back2 P st xid v₁ yid v₂ point fx fy
           (λ y≡x → xid≢yid (sym y≡x)) rp ⊢p of λ ()

------------------------------------------------------------------------
-- bytes32-into-low-high   (Bytes32 → Native × Native, two distinct outs).
------------------------------------------------------------------------

bytes32-into-low-high-bwd : ∀ {P S st bytes loid hiid v₁ v₂}
  → State.mem st loid ≡ nothing → State.mem st hiid ≡ nothing → ¬ (loid ≡ hiid)
  → (∃ λ b → resolve (State.mem st) bytes ≡ just (val-bytes32 b))
  → holds (witness-of P (out1 (out1 st loid v₁) hiid v₂))
          (bytes-into-low-high loid hiid bytes)
  → step P S st (bytes32-into-low-high bytes (loid , hiid))
      ≡ just (out1 (out1 st loid v₁) hiid v₂)
bytes32-into-low-high-bwd {P} {S} {st} {bytes} {loid} {hiid} {v₁} {v₂}
  fl fh loid≢hiid (b , rb) (bs , lo , hi , ⊢b , esplit , ⊢lo , ⊢hi)
  with ⊢-back2 P st loid v₁ hiid v₂ bytes fl fh
         (λ h≡l → loid≢hiid (sym h≡l)) rb ⊢b
... | refl rewrite rb | esplit =
      cong₂ (λ z₁ z₂ → just (out1 (out1 st loid z₁) hiid z₂))
        (out-val-inner P st loid v₁ hiid v₂ loid≢hiid ⊢lo)
        (out-val-outer P st loid v₁ hiid v₂ ⊢hi)

------------------------------------------------------------------------
-- div-mod-power-of-two   (two distinct outputs q, r).
------------------------------------------------------------------------

div-mod-power-of-two-bwd : ∀ {P S st val bits q r v₁ v₂}
  → State.mem st q ≡ nothing → State.mem st r ≡ nothing → ¬ (q ≡ r)
  → (∃ λ x → resolveᶠ (State.mem st) val ≡ just x)
  → holds (witness-of P (out1 (out1 st q v₁) r v₂)) (div-mod q r val bits)
  → step P S st (div-mod-power-of-two val bits (q ∷ r ∷ []))
      ≡ just (out1 (out1 st q v₁) r v₂)
div-mod-power-of-two-bwd {P} {S} {st} {val} {bits} {q} {r} {v₁} {v₂}
  fq fr q≢r (x , rf) (x' , ⊢v , ⊢q , ⊢r)
  with ⊢ᶠ-back2 P st q v₁ r v₂ val fq fr (λ r≡q → q≢r (sym r≡q)) rf ⊢v
... | refl rewrite rf =
      cong₂ (λ z₁ z₂ → just (out1 (out1 st q z₁) r z₂))
        (out-val-inner P st q v₁ r v₂ q≢r ⊢q)
        (out-val-outer P st q v₁ r v₂ ⊢r)

------------------------------------------------------------------------
-- persistent-hash   (Native inputs → single Bytes32 output).
------------------------------------------------------------------------

persistent-hash-bwd : ∀ {P S st alignment inputs output v}
  → State.mem st output ≡ nothing
  → (∃ λ frs → resolve-all-Fr (State.mem st) inputs ≡ just frs)
  → holds (witness-of P (out1 st output v)) (sha256 output alignment inputs)
  → step P S st (persistent-hash alignment inputs output)
      ≡ just (out1 st output v)
persistent-hash-bwd {P} {S} {st} {alignment} {inputs} {output} {v} fresh
  (frs , ri) (frs' , hv , ⊢i , ehp , ⊢out)
  with ⊢all-back P st output v inputs fresh ri ⊢i
... | refl rewrite ri | ehp =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- keccak256   (Native inputs → single Bytes32 output).
------------------------------------------------------------------------

keccak256-bwd : ∀ {P S st alignment inputs output v}
  → State.mem st output ≡ nothing
  → (∃ λ frs → resolve-all-Fr (State.mem st) inputs ≡ just frs)
  → holds (witness-of P (out1 st output v)) (keccak output alignment inputs)
  → step P S st (keccak256 alignment inputs output)
      ≡ just (out1 st output v)
keccak256-bwd {P} {S} {st} {alignment} {inputs} {output} {v} fresh
  (frs , ri) (frs' , hv , ⊢i , ehk , ⊢out)
  with ⊢all-back P st output v inputs fresh ri ⊢i
... | refl rewrite ri | ehk =
      cong (λ z → just (out1 st output z)) (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- encode   (input → `insertMany` of its encoding onto `outputs`).
--
-- The operational converse of `encode-fwd`.  The post-state is the
-- `insertMany` result — encode's only per-cell content is that each
-- output holds its encoding element, which `insertMany` establishes by
-- construction; `bind-each` (hence `encode-eq`) forces the output/encoding
-- lengths to agree, so `insertMany` succeeds.  Both facts are captured by
-- the `insertMany … ≡ just st'` premise, which names the post-state.
------------------------------------------------------------------------

encode-bwd : ∀ {P S st input outputs v st'}
  → resolve (State.mem st) input ≡ just v
  → insertMany st outputs (map val-native (encodeᵉ v)) ≡ just st'
  → step P S st (encode input outputs) ≡ just st'
encode-bwd {P} {S} {st} {input} {outputs} {v} {st'} ri im
  rewrite ri = im

------------------------------------------------------------------------
-- assert   (no output; post-state is `st`).
--
-- Obligation: the condition resolves to a field element whose boolean
-- reading is `true` (`to𝔹 x ≡ just true`).  `non-zero` alone gives only
-- `x ≢ 0ᶠ`; running `step` backward needs the guard to read `true`, which
-- for a booleanity-restricted `x` means `x ≡ 1ᶠ`.
------------------------------------------------------------------------

assert-bwd : ∀ {P S st cond}
  → (∃ λ x → resolveᶠ (State.mem st) cond ≡ just x × to𝔹 x ≡ just true)
  → step P S st (assert cond) ≡ just st
assert-bwd {P} {S} {st} {cond} (x , rf , tb)
  rewrite rf | tb = refl

------------------------------------------------------------------------
-- constrain-to-boolean   (no output; post-state is `st`).
--
-- Obligation: the `boolean` constraint holds at the pre-step witness.  Its
-- `is-bit` conjunct restricts the resolved value to `{0,1}`, so `to𝔹`
-- succeeds and `resolve𝔹` produces a bit; the resolution itself is read
-- off the constraint (via `resolveᶜ-Fr-agree`, the post-state being `st`).
------------------------------------------------------------------------

constrain-to-boolean-bwd : ∀ {P S st val}
  → holds (witness-of P st) (boolean val)
  → step P S st (constrain-to-boolean val) ≡ just st
constrain-to-boolean-bwd {P} {S} {st} {val} (x , ⊢val , inj₁ x≡0)
  rewrite trans (sym (resolveᶜ-Fr-agree P st val)) ⊢val | x≡0 with 0ᶠ ≟ᶠ 0ᶠ
... | yes _  = refl
... | no ¬p  = case ¬p refl of λ ()
constrain-to-boolean-bwd {P} {S} {st} {val} (x , ⊢val , inj₂ x≡1)
  rewrite trans (sym (resolveᶜ-Fr-agree P st val)) ⊢val | x≡1 with 1ᶠ ≟ᶠ 0ᶠ
... | no _   with 1ᶠ ≟ᶠ 1ᶠ
...   | yes _  = refl
...   | no ¬p  = case ¬p refl of λ ()
constrain-to-boolean-bwd {P} {S} {st} {val} (x , ⊢val , inj₂ x≡1)
    | yes 1≡0 = case 1ᶠ≢0ᶠ 1≡0 of λ ()

------------------------------------------------------------------------
-- not   (booleanity from the constraint).
--
-- Obligation: the operand resolves off-circuit (`∃ x`).  Booleanity is
-- supplied by the `is-not` constraint's own `is-bit` conjunct (at the
-- post-step witness, transported to the pre-step resolution by
-- `⊢ᶠ-back`, since the output is fresh).  Under `is-bit`, `to𝔹 x` agrees
-- with the in-circuit reading `isYes (x ≟ᶠ 1ᶠ)`, so the off-circuit output
-- `χ (bnot (to𝔹 x))` matches the value `is-not` records.
------------------------------------------------------------------------

not-bwd : ∀ {P S st a output v}
  → State.mem st output ≡ nothing
  → (∃ λ x → resolveᶠ (State.mem st) a ≡ just x)
  → holds (witness-of P (out1 st output v)) (is-not output a)
  → step P S st (not a output) ≡ just (out1 st output v)
not-bwd {P} {S} {st} {a} {output} {v} fresh (x , rf)
  (x' , ⊢a , inj₁ x'≡0 , ⊢out)
  with ⊢ᶠ-back P st output v a fresh rf ⊢a
... | refl rewrite rf | x'≡0 with 0ᶠ ≟ᶠ 0ᶠ | 0ᶠ ≟ᶠ 1ᶠ
...   | yes _ | no _   =
        cong (λ z → just (out1 st output z))
          (out-back P st output v ⊢out refl)
...   | yes _ | yes 0≡1 = case 1ᶠ≢0ᶠ (sym 0≡1) of λ ()
...   | no ¬p | _       = case ¬p refl of λ ()
not-bwd {P} {S} {st} {a} {output} {v} fresh (x , rf)
  (x' , ⊢a , inj₂ x'≡1 , ⊢out)
  with ⊢ᶠ-back P st output v a fresh rf ⊢a
... | refl rewrite rf | x'≡1 with 1ᶠ ≟ᶠ 0ᶠ
...   | no _ with 1ᶠ ≟ᶠ 1ᶠ
...     | yes _ =
          cong (λ z → just (out1 st output z))
            (out-back P st output v ⊢out refl)
...     | no ¬p = case ¬p refl of λ ()
not-bwd {P} {S} {st} {a} {output} {v} fresh (x , rf)
  (x' , ⊢a , inj₂ x'≡1 , ⊢out)
    | refl | yes 1≡0 = case 1ᶠ≢0ᶠ 1≡0 of λ ()

------------------------------------------------------------------------
-- cond-select   (booleanity of the selector + a type match).
--
-- Obligations: the selector resolves off-circuit (`∃ x`); each branch
-- resolves; the two branches share a type (`typeof av ≡ typeof bvl`, the
-- `guardD (… ≟T …)` off-circuit `step` enforces).  Booleanity of the
-- selector is supplied by the `select` constraint's own `is-bit bv`
-- conjunct (transported to the pre-step resolution by `⊢ᶠ-back`, the
-- output being fresh).  Under `is-bit`, the off-circuit `if to𝔹 bit …`
-- picks exactly the branch the two `select` implications constrain the
-- output to.
------------------------------------------------------------------------

cond-select-bwd : ∀ {P S st bit a b output v}
  → State.mem st output ≡ nothing
  → (∃ λ x → resolveᶠ (State.mem st) bit ≡ just x)
  → (∃ λ av → resolve (State.mem st) a ≡ just av
       × ∃ λ bvl → resolve (State.mem st) b ≡ just bvl
                        × typeof av ≡ typeof bvl)
  → holds (witness-of P (out1 st output v)) (select output bit a b)
  → step P S st (cond-select bit a b output) ≡ just (out1 st output v)
cond-select-bwd {P} {S} {st} {bit} {a} {b} {output} {v} fresh
  (x , rbit) (av₀ , ra , bvl₀ , rb , ta≡tb)
  (bv , av , bvl , ov , ⊢bit , ⊢a , ⊢b , ⊢out , inj₁ bv≡0 , _ , imp0)
  with ⊢ᶠ-back P st output v bit fresh rbit ⊢bit
     | ⊢-back  P st output v a   fresh ra   ⊢a
     | ⊢-back  P st output v b   fresh rb   ⊢b
... | refl | refl | refl
  rewrite rbit | ra | rb | bv≡0 with 0ᶠ ≟ᶠ 0ᶠ
...   | no ¬p = case ¬p refl of λ ()
...   | yes _ with typeof av₀ ≟T typeof bvl₀
...     | no ¬t = case ¬t ta≡tb of λ ()
...     | yes _ =
          cong (λ z → just (out1 st output z))
            (out-back P st output v ⊢out (imp0 refl))
cond-select-bwd {P} {S} {st} {bit} {a} {b} {output} {v} fresh
  (x , rbit) (av₀ , ra , bvl₀ , rb , ta≡tb)
  (bv , av , bvl , ov , ⊢bit , ⊢a , ⊢b , ⊢out , inj₂ bv≡1 , imp1 , _)
  with ⊢ᶠ-back P st output v bit fresh rbit ⊢bit
     | ⊢-back  P st output v a   fresh ra   ⊢a
     | ⊢-back  P st output v b   fresh rb   ⊢b
... | refl | refl | refl
  rewrite rbit | ra | rb | bv≡1 with 1ᶠ ≟ᶠ 0ᶠ
...   | yes 1≡0 = case 1ᶠ≢0ᶠ 1≡0 of λ ()
...   | no _ with 1ᶠ ≟ᶠ 1ᶠ
...     | no ¬p = case ¬p refl of λ ()
...     | yes _ with typeof av₀ ≟T typeof bvl₀
...       | no ¬t = case ¬t ta≡tb of λ ()
...       | yes _ =
            cong (λ z → just (out1 st output z))
              (out-back P st output v ⊢out (imp1 refl))

------------------------------------------------------------------------
-- less-than   (exact range bounds carried as side data).
--
-- Obligation: both operands resolve AND satisfy the exact `2 ^ bits`
-- bound the off-circuit step checks.  The constraint `less-than` pins
-- only the chip's evened `2 ^ even4 bits` bound, which is too weak for
-- odd or small `bits`; the exact bounds are therefore step side data
-- (`BwdStep` / `WSteps`), consumed here after the operand values are
-- pinned by `⊢ᶠ-back`.
------------------------------------------------------------------------

less-than-bwd : ∀ {P S st a b bits output v}
  → State.mem st output ≡ nothing
  → (∃ λ x → resolveᶠ (State.mem st) a ≡ just x × valFr x < 2 ^ bits)
  → (∃ λ y → resolveᶠ (State.mem st) b ≡ just y × valFr y < 2 ^ bits)
  → holds (witness-of P (out1 st output v)) (less-than output a b bits)
  → step P S st (less-than a b bits output) ≡ just (out1 st output v)
less-than-bwd {P} {S} {st} {a} {b} {bits} {output} {v} fresh
  (x₀ , ra , px) (y₀ , rb , py) (x , y , ⊢a , ⊢b , _ , _ , ⊢out)
  with ⊢ᶠ-back P st output v a fresh ra ⊢a
     | ⊢ᶠ-back P st output v b fresh rb ⊢b
... | refl | refl rewrite ra | rb with valFr x <? 2 ^ bits
...   | no ¬px = case ¬px px of λ ()
...   | yes _ with valFr y <? 2 ^ bits
...     | no ¬py = case ¬py py of λ ()
...     | yes _ =
          cong (λ z → just (out1 st output z))
            (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- reconstitute-field   (two range bounds + a no-overflow obligation).
--
-- Obligation: both operands resolve, and the recombined value does not
-- overflow the field order (`valFr mo + 2 ^ bits * valFr d < FR-ORDER`,
-- the third off-circuit guard).  The two range bounds live inside the
-- constraint `reconstitute`, so `holds` supplies them once the operand
-- values are pinned; the no-overflow fact is a producer obligation not
-- recorded by the constraint.
------------------------------------------------------------------------

reconstitute-field-bwd : ∀ {P S st divisor modulus bits output v}
  → State.mem st output ≡ nothing
  → (∃ λ d → resolveᶠ (State.mem st) divisor ≡ just d
       × ∃ λ mo → resolveᶠ (State.mem st) modulus ≡ just mo
                       × (valFr mo + 2 ^ bits * valFr d < FR-ORDER))
  → holds (witness-of P (out1 st output v))
          (reconstitute output divisor modulus bits)
  → step P S st (reconstitute-field divisor modulus bits output)
      ≡ just (out1 st output v)
reconstitute-field-bwd {P} {S} {st} {divisor} {modulus} {bits} {output} {v}
  fresh (d , rd , mo , rm , novf) (dv , mv , ⊢d , ⊢m , pd , pm , ⊢out)
  with ⊢ᶠ-back P st output v divisor fresh rd ⊢d
     | ⊢ᶠ-back P st output v modulus fresh rm ⊢m
... | refl | refl rewrite rd | rm with valFr mo <? 2 ^ bits
...   | no ¬pm = case ¬pm pm of λ ()
...   | yes _ with valFr d <? 2 ^ (FR-BITS ∸ bits)
...     | no ¬pd = case ¬pd pd of λ ()
...     | yes _ with valFr mo + 2 ^ bits * valFr d <? FR-ORDER
...       | no ¬novf = case ¬novf novf of λ ()
...       | yes _ =
            cong (λ z → just (out1 st output z))
              (out-back P st output v ⊢out refl)

------------------------------------------------------------------------
-- constrain-bits   (no output; post-state is `st`).
--
-- The range obligation `valFr x < 2 ^ bits` lives inside the constraint
-- `in-range`, so `holds` supplies both the resolution (via
-- `resolveᶜ-Fr-agree`, no output binding to invert) and the guard proof.
------------------------------------------------------------------------

constrain-bits-bwd : ∀ {P S st val bits}
  → holds (witness-of P st) (in-range val bits)
  → step P S st (constrain-bits val bits) ≡ just st
constrain-bits-bwd {P} {S} {st} {val} {bits} (x , ⊢v , px)
  rewrite trans (sym (resolveᶜ-Fr-agree P st val)) ⊢v
  with valFr x <? 2 ^ bits
... | yes _  = refl
... | no ¬px = case ¬px px of λ ()

------------------------------------------------------------------------
-- impact   (pi/transcript cluster; the only pis-appending instruction).
--
-- `step` inspects: the input list resolves (`vals`), the guard resolves
-- to a field element `gᶠ` with a bit reading `b = to𝔹 gᶠ`, and — when
-- `b = true` — the pushed values match the public transcript slice.  The
-- two branches build distinct post-states (active: append the actual
-- `vals` and advance the transcript cursor; skipped: append zeros and
-- record the skip), so each has its own converse.  These are pure `step`
-- inversions: `impact` synthesises `impact-constraints`, but running the
-- step backward needs the operational transcript facts, not the
-- constraint (whose `pi-impact` entries are a *consequence* of the push).
------------------------------------------------------------------------

impact-true-bwd : ∀ {P S st guard inputs gᶠ vals}
  → resolve-all-Fr (State.mem st) inputs ≡ just vals
  → resolveᶠ (State.mem st) guard ≡ just gᶠ
  → to𝔹 gᶠ ≡ just true
  → take (length vals)
       (drop (State.pti-idx st)
         (ProofPreimage.pub-transcript-inputs P)) ≡ vals
  → step P S st (impact guard inputs)
      ≡ just (record st { pis      = State.pis st ++ vals
                        ; pi-skips = State.pi-skips st ++ (nothing ∷ [])
                        ; pti-idx  = State.pti-idx st + length vals })
impact-true-bwd {P} {S} {st} {guard} {inputs} {gᶠ} {vals}
  ri rg tb tm
  rewrite ri | rg | tb
  with take (length vals)
         (drop (State.pti-idx st)
           (ProofPreimage.pub-transcript-inputs P)) ≟LFr vals
... | yes _   = refl
... | no ¬tm  = case ¬tm tm of λ ()

impact-false-bwd : ∀ {P S st guard inputs gᶠ vals}
  → resolve-all-Fr (State.mem st) inputs ≡ just vals
  → resolveᶠ (State.mem st) guard ≡ just gᶠ
  → to𝔹 gᶠ ≡ just false
  → step P S st (impact guard inputs)
      ≡ just (record st
                { pis      = State.pis st ++ map (λ _ → 0ᶠ) vals
                ; pi-skips = State.pi-skips st ++ (just (length vals) ∷ []) })
impact-false-bwd {P} {S} {st} {guard} {inputs} {gᶠ} {vals}
  ri rg tb
  rewrite ri | rg | tb = refl

------------------------------------------------------------------------
-- public-input   (transcript cluster; `synth-instr` emits no constraint,
-- so there is no `holds`/`satisfies` premise — the converse is a pure
-- `step` inversion driven by the guard reading and the transcript read).
--
-- Guard evaluation (`eval-guard`, which passes through `resolve𝔹` for a
-- present guard and reads `true` for an absent one) is stated as
-- `eval-guard m guard ≡ just b`, covering both guard shapes uniformly.
-- The two branches produce distinct post-states: active reads and binds a
-- decoded transcript value and advances the output cursor; inactive binds
-- the type's default value.
------------------------------------------------------------------------

public-input-active-bwd : ∀ {P S st guard val-t output v}
  → eval-guard (State.mem st) guard ≡ just true
  → decode val-t (take (encoded-len val-t) (State.pto-rem st)) ≡ just v
  → step P S st (public-input guard val-t output)
      ≡ just (record st
                { mem     = ins output v (State.mem st)
                ; pto-rem = drop (encoded-len val-t) (State.pto-rem st) })
public-input-active-bwd {P} {S} {st} {guard} {val-t} {output} {v} eg dv
  rewrite eg | dv = refl

public-input-inactive-bwd : ∀ {P S st guard val-t output}
  → eval-guard (State.mem st) guard ≡ just false
  → step P S st (public-input guard val-t output)
      ≡ just (out1 st output (default-val val-t))
public-input-inactive-bwd {P} {S} {st} {guard} {val-t} {output} eg
  rewrite eg = refl

------------------------------------------------------------------------
-- private-input   (as `public-input`, over the private transcript).
------------------------------------------------------------------------

private-input-active-bwd : ∀ {P S st guard val-t output v}
  → eval-guard (State.mem st) guard ≡ just true
  → decode val-t (take (encoded-len val-t) (State.priv-rem st)) ≡ just v
  → step P S st (private-input guard val-t output)
      ≡ just (record st
                { mem      = ins output v (State.mem st)
                ; priv-rem = drop (encoded-len val-t) (State.priv-rem st) })
private-input-active-bwd {P} {S} {st} {guard} {val-t} {output} {v} eg dv
  rewrite eg | dv = refl

private-input-inactive-bwd : ∀ {P S st guard val-t output}
  → eval-guard (State.mem st) guard ≡ just false
  → step P S st (private-input guard val-t output)
      ≡ just (out1 st output (default-val val-t))
private-input-inactive-bwd {P} {S} {st} {guard} {val-t} {output} eg
  rewrite eg = refl

------------------------------------------------------------------------
-- circuit-output   (output cluster; `synth-instr` only appends to
-- `output-ops`, emitting no constraint).  `step` collects the declared
-- source outputs into concrete values and appends them to `State.outs`.
-- The converse is a pure inversion: given that collection succeeds, the
-- step produces the extended-`outs` state.
------------------------------------------------------------------------

circuit-output-bwd : ∀ {P S st vals vs}
  → collectOutputs (State.mem st) (IrSource.outputs S) vals ≡ just vs
  → step P S st (circuit-output vals)
      ≡ just (record st { outs = State.outs st ++ vs })
circuit-output-bwd {P} {S} {st} {vals} {vs} co
  rewrite co = refl

------------------------------------------------------------------------
-- Shape machinery for the backward direction (v3 analogue of v2's
-- `preprocess-shaped` / `Tr-shaped`).
--
-- The backward direction reconstructs, from a satisfying witness, the
-- operational run that produced it.  Unlike zkir-v2 — whose per-step
-- backward lemmas yield an abstract `R-instr` relation and need a
-- `next-state-from-osd`/`op-side-data-list` layer to sequence them — the
-- v3 per-instruction `*-bwd` lemmas already conclude the *operational*
-- `step P S st i ≡ just st'`.  So the v3 shape "walk" is just v3's own
-- `run`: sequencing the reconstructed steps is `run`'s cons rule
-- (`run-inv` is its inverse).  The shape record therefore carries only
--   • the initial state (`init S P ≡ just st0`), and
--   • that the run over the source's instructions ends EXACTLY at `s`
--     (`run P S (instructions S) st0 ≡ just s`), plus
--   • the terminal transcript-consumption facts that `satisfies` is blind
--     to (`Consumed`, below) — the v3 counterpart of v2's
--     `T (transcripts-consumed pre s)`.
--
-- The per-step side data (guard evaluations, transcript reads, output
-- collection) that `satisfies` does not determine is NOT part of this
-- record: it is supplied by the backward driver at each step, threaded
-- alongside the `TyEq` context and the lowered
-- `holds` (`holds-lower` + `Defd`).  This keeps the shape record minimal
-- and the driver the single place the side data is consumed.
------------------------------------------------------------------------

