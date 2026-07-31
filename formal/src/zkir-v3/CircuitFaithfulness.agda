{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- FORWARD faithfulness of zkir-v3: every successful preprocess run
-- yields a witness satisfying the synthesized constraint system.
--
-- Contents: the per-instruction `*-fwd` lemmas (at the immediate
-- post-step witness), the program-level induction `fwd-go`, the
-- output-collection coupling `out-go`, the communications-commitment
-- case `comm-fwd` (with `decode-reencode` and `init-pi1`), and the
-- headline `forward`.  The witness/constraint bridge (`witness-of`,
-- `holds-mono`, …) lives in CircuitBridge; the static-check form
-- `forward-sa` and the iff live in CircuitProof.
------------------------------------------------------------------------

module zkir-v3.CircuitFaithfulness (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Semantics ⋯ using (ProofPreimage; State; resolve; resolveᶠ)
open import zkir-v3.SemanticsProperties ⋯
open import zkir-v3.Circuit ⋯
open import zkir-v3.CircuitBridge ⋯

open import Data.Maybe   using (Maybe; just; nothing)
open import Data.Product using (_×_; _,_; proj₁; proj₂)
open import Relation.Binary.PropositionalEquality using (_≡_; refl)

------------------------------------------------------------------------
-- Per-instruction FORWARD faithfulness  (deterministic + assert).
--
-- For each instruction `I` of the deterministic / `assert` fragment, a
-- local lemma: from a successful off-circuit step
-- (`step P S st (I …) ≡ just st'`) and freshness/distinctness of the
-- output identifier(s), the constraint that `synth-instr (I …)` appends
-- holds at the immediate post-step witness `witness-of P st'`.
--
-- These are stated at the immediate post-step witness only: no
-- program-level induction, no monotonicity, no backward direction.  The
-- hypotheses are transported across the off/in-circuit boundary by
-- `resolve-agree` / `resolveᶜ-Fr-agree`; the output binding is read off
-- the `ins` that `step` performed.
------------------------------------------------------------------------

open import zkir-v3.Semantics ⋯
  using (Mem; step; resolve𝔹; to𝔹; out1; valEq?; _≟B_;
         resolve-all-Fr)
  renaming (χ to χˢ; pow2ᶠ to pow2ᶠˢ)
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)

open import Data.Bool using (Bool; true; false; if_then_else_)
  renaming (not to bnot)
open import Data.Maybe using (_>>=_)
open import Data.Nat using (ℕ; zero; suc; _+_; _*_; _<?_; _<_; _^_; _∸_)
open import Data.List using (List; []; _∷_; map)
open import Data.Sum using (inj₁; inj₂; _⊎_)
open import Data.Unit using (⊤; tt)
open import Function using (case_of_)
open import Relation.Binary.PropositionalEquality
  using (sym; trans; cong)
open import Relation.Nullary using (Dec; yes; no; ¬_)
open import Relation.Nullary.Decidable using (isYes)

------------------------------------------------------------------------
-- assert  ⟶  non-zero
--
-- A successful step forces the condition to read as `true`, which (via
-- `to𝔹`) means it resolves to a field element distinct from `0ᶠ`.
------------------------------------------------------------------------

assert-fwd : ∀ {P S st st' cond}
  → step P S st (assert cond) ≡ just st'
  → holds (witness-of P st') (non-zero cond)
assert-fwd {P} {S} {st} {st'} {cond} st≡
  with resolveᶠ (State.mem st) cond in eqf
... | just x with x ≟ᶠ 0ᶠ
...   | no x≢0 with x ≟ᶠ 1ᶠ
...     | yes _ with refl ← st≡ =
          x , trans (resolveᶜ-Fr-agree P st cond) eqf , x≢0

------------------------------------------------------------------------
-- constrain-eq  ⟶  eq
--
-- A `valEq?` reading `true` means the two values are equal (at any of
-- the supported types), so both operands resolve to a single value.
------------------------------------------------------------------------

-- A `valEq?` that reads `true` witnesses equality of the two values.
valEq?-true : ∀ {av bv} → valEq? av bv ≡ just true → av ≡ bv
valEq?-true {val-native x}       {val-native y}       e
  with x ≟ᶠ y
... | yes refl = refl
valEq?-true {val-bytes32 a}      {val-bytes32 b}      e
  with a ≟B b
... | yes refl = refl
valEq?-true {val-jubjub-point p} {val-jubjub-point q} e
  with p ≟J q
... | yes refl = refl
valEq?-true {val-secp-point p}   {val-secp-point q}   e
  with p ≟K q
... | yes refl = refl
valEq?-true {val-secp-base x}    {val-secp-base y}    e
  with x ≟ᵇ y
... | yes refl = refl
valEq?-true {val-secp-scalar x}  {val-secp-scalar y}  e
  with x ≟ˢ y
... | yes refl = refl

constrain-eq-fwd : ∀ {P S st st' a b}
  → step P S st (constrain-eq a b) ≡ just st'
  → holds (witness-of P st') (eq a b)
constrain-eq-fwd {P} {S} {st} {st'} {a} {b} st≡
  with resolve (State.mem st) a in eqa | resolve (State.mem st) b in eqb
... | just av | just bv with valEq? av bv in eqv
...   | just true with refl ← st≡ with refl ← valEq?-true {av} {bv} eqv =
          av
        , trans (resolve-agree P st a) eqa
        , trans (resolve-agree P st b) eqb

------------------------------------------------------------------------
-- constrain-to-boolean  ⟶  boolean
--
-- A successful `resolve𝔹` forces the value to read as a boolean, i.e.
-- it equals `0ᶠ` or `1ᶠ` (the two `to𝔹` success branches).
------------------------------------------------------------------------

constrain-to-boolean-fwd : ∀ {P S st st' val}
  → step P S st (constrain-to-boolean val) ≡ just st'
  → holds (witness-of P st') (boolean val)
constrain-to-boolean-fwd {P} {S} {st} {st'} {val} st≡
  with resolveᶠ (State.mem st) val in eqf
... | just x with x ≟ᶠ 0ᶠ
...   | yes x≡0 with refl ← st≡ =
          x , trans (resolveᶜ-Fr-agree P st val) eqf , inj₁ x≡0
...   | no _ with x ≟ᶠ 1ᶠ
...     | yes x≡1 with refl ← st≡ =
            x , trans (resolveᶜ-Fr-agree P st val) eqf , inj₂ x≡1

------------------------------------------------------------------------
-- constrain-bits  ⟶  in-range
--
-- The off-circuit guard `valFr x <? 2 ^ bits` succeeds exactly with the
-- range proof that `in-range` requires.
------------------------------------------------------------------------

constrain-bits-fwd : ∀ {P S st st' val bits}
  → step P S st (constrain-bits val bits) ≡ just st'
  → holds (witness-of P st') (in-range val bits)
constrain-bits-fwd {P} {S} {st} {st'} {val} {bits} st≡
  with resolveᶠ (State.mem st) val in eqf
... | just x with valFr x <? 2 ^ bits
...   | yes p with refl ← st≡ =
          x , trans (resolveᶜ-Fr-agree P st val) eqf , p

------------------------------------------------------------------------
-- copy  ⟶  gate-copy
--
-- The output cell holds the resolved input value; freshness keeps the
-- input resolving identically across the binding.
------------------------------------------------------------------------

copy-fwd : ∀ {P S st st' val output}
  → State.mem st output ≡ nothing
  → step P S st (copy val output) ≡ just st'
  → holds (witness-of P st') (gate-copy output val)
copy-fwd {P} {S} {st} {st'} {val} {output} fresh st≡
  with resolve (State.mem st) val in eqv
... | just v with refl ← st≡ =
        v , ⊢-pres P st output v val fresh eqv , assign-here P st output v

------------------------------------------------------------------------
-- add  ⟶  gate-add        (Native or JubjubPoint, type-directed)
------------------------------------------------------------------------

add-fwd : ∀ {P S st st' a b output}
  → State.mem st output ≡ nothing
  → step P S st (add a b output) ≡ just st'
  → holds (witness-of P st') (gate-add output a b)
add-fwd {P} {S} {st} {st'} {a} {b} {output} fresh st≡
  with resolve (State.mem st) a in eqa | resolve (State.mem st) b in eqb
... | just (val-native x) | just (val-native y) with refl ← st≡ =
        val-native x , val-native y , val-native (x +ᶠ y)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₁ (x , y , refl , refl , refl)
... | just (val-jubjub-point p) | just (val-jubjub-point q) with refl ← st≡ =
        val-jubjub-point p , val-jubjub-point q , val-jubjub-point (p +J q)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₂ (inj₁ (p , q , refl , refl , refl))
... | just (val-secp-point p) | just (val-secp-point q) with refl ← st≡ =
        val-secp-point p , val-secp-point q , val-secp-point (p +K q)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₂ (inj₂ (inj₁ (p , q , refl , refl , refl)))
... | just (val-secp-base x) | just (val-secp-base y) with refl ← st≡ =
        val-secp-base x , val-secp-base y , val-secp-base (x +ᵇ y)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₂ (inj₂ (inj₂ (inj₁ (x , y , refl , refl , refl))))
... | just (val-secp-scalar x) | just (val-secp-scalar y) with refl ← st≡ =
        val-secp-scalar x , val-secp-scalar y , val-secp-scalar (x +ˢ y)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₂ (inj₂ (inj₂ (inj₂ (x , y , refl , refl , refl))))

------------------------------------------------------------------------
-- mul  ⟶  gate-mul        (Native)
------------------------------------------------------------------------

mul-fwd : ∀ {P S st st' a b output}
  → State.mem st output ≡ nothing
  → step P S st (mul a b output) ≡ just st'
  → holds (witness-of P st') (gate-mul output a b)
mul-fwd {P} {S} {st} {st'} {a} {b} {output} fresh st≡
  with resolve (State.mem st) a in eqa | resolve (State.mem st) b in eqb
... | just (val-native x) | just (val-native y) with refl ← st≡ =
        val-native x , val-native y , val-native (x *ᶠ y)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₁ (x , y , refl , refl , refl)
... | just (val-secp-base x) | just (val-secp-base y) with refl ← st≡ =
        val-secp-base x , val-secp-base y , val-secp-base (x *ᵇ y)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₂ (inj₁ (x , y , refl , refl , refl))
... | just (val-secp-scalar x) | just (val-secp-scalar y) with refl ← st≡ =
        val-secp-scalar x , val-secp-scalar y , val-secp-scalar (x *ˢ y)
      , ⊢-pres P st output _ a fresh eqa
      , ⊢-pres P st output _ b fresh eqb
      , assign-here P st output _
      , inj₂ (inj₂ (x , y , refl , refl , refl))

------------------------------------------------------------------------
-- neg  ⟶  gate-neg        (Native or JubjubPoint)
------------------------------------------------------------------------

neg-fwd : ∀ {P S st st' a output}
  → State.mem st output ≡ nothing
  → step P S st (neg a output) ≡ just st'
  → holds (witness-of P st') (gate-neg output a)
neg-fwd {P} {S} {st} {st'} {a} {output} fresh st≡
  with resolve (State.mem st) a in eqa
... | just (val-native x) with refl ← st≡ =
        val-native x , val-native (-ᶠ x)
      , ⊢-pres P st output _ a fresh eqa
      , assign-here P st output _
      , inj₁ (x , refl , refl)
... | just (val-jubjub-point p) with refl ← st≡ =
        val-jubjub-point p , val-jubjub-point (negJ p)
      , ⊢-pres P st output _ a fresh eqa
      , assign-here P st output _
      , inj₂ (inj₁ (p , refl , refl))
... | just (val-secp-point p) with refl ← st≡ =
        val-secp-point p , val-secp-point (negK p)
      , ⊢-pres P st output _ a fresh eqa
      , assign-here P st output _
      , inj₂ (inj₂ (inj₁ (p , refl , refl)))
... | just (val-secp-base x) with refl ← st≡ =
        val-secp-base x , val-secp-base (-ᵇ x)
      , ⊢-pres P st output _ a fresh eqa
      , assign-here P st output _
      , inj₂ (inj₂ (inj₂ (inj₁ (x , refl , refl))))
... | just (val-secp-scalar x) with refl ← st≡ =
        val-secp-scalar x , val-secp-scalar (-ˢ x)
      , ⊢-pres P st output _ a fresh eqa
      , assign-here P st output _
      , inj₂ (inj₂ (inj₂ (inj₂ (x , refl , refl))))

------------------------------------------------------------------------
-- inv  ⟶  gate-inv        (Native; off-circuit `invᶠ` mirrors the gate)
------------------------------------------------------------------------

inv-fwd : ∀ {P S st st' a output}
  → State.mem st output ≡ nothing
  → step P S st (inv a output) ≡ just st'
  → holds (witness-of P st') (gate-inv output a)
inv-fwd {P} {S} {st} {st'} {a} {output} fresh st≡
  with resolve (State.mem st) a in eqa
... | just (val-native x) with invᶠ x in eqi
...   | just xi with refl ← st≡ =
          val-native x , val-native xi
        , ⊢-pres P st output _ a fresh eqa
        , assign-here P st output _
        , inj₁ (x , xi , refl , eqi , refl)
inv-fwd {P} {S} {st} {st'} {a} {output} fresh st≡
    | just (val-secp-base x) with invᵇ x in eqi
...   | just xi with refl ← st≡ =
          val-secp-base x , val-secp-base xi
        , ⊢-pres P st output _ a fresh eqa
        , assign-here P st output _
        , inj₂ (inj₁ (x , xi , refl , eqi , refl))
inv-fwd {P} {S} {st} {st'} {a} {output} fresh st≡
    | just (val-secp-scalar x) with invˢ x in eqi
...   | just xi with refl ← st≡ =
          val-secp-scalar x , val-secp-scalar xi
        , ⊢-pres P st output _ a fresh eqa
        , assign-here P st output _
        , inj₂ (inj₂ (x , xi , refl , eqi , refl))

------------------------------------------------------------------------
-- not  ⟶  is-not          (booleanity from the `to𝔹` success branches)
------------------------------------------------------------------------

not-fwd : ∀ {P S st st' a output}
  → State.mem st output ≡ nothing
  → step P S st (not a output) ≡ just st'
  → holds (witness-of P st') (is-not output a)
not-fwd {P} {S} {st} {st'} {a} {output} fresh st≡
  with resolveᶠ (State.mem st) a in eqf
... | just x with x ≟ᶠ 0ᶠ
...   | yes x≡0 with x ≟ᶠ 1ᶠ in eq1
...     | no _    with refl ← st≡ =
            x , ⊢ᶠ-pres P st output _ a fresh eqf , inj₁ x≡0
          , trans (assign-here P st output (val-native 1ᶠ))
                  (cong (λ d → just (val-native (χ (bnot (isYes d)))))
                        (sym eq1))
...     | yes x≡1 = case 1ᶠ≢0ᶠ (trans (sym x≡1) x≡0) of λ ()
not-fwd {P} {S} {st} {st'} {a} {output} fresh st≡
    | just x | no _ with x ≟ᶠ 1ᶠ in eq1
...   | yes x≡1 with refl ← st≡ =
          x , ⊢ᶠ-pres P st output _ a fresh eqf , inj₂ x≡1
        , trans (assign-here P st output (val-native 0ᶠ))
                (cong (λ d → just (val-native (χ (bnot (isYes d)))))
                      (sym eq1))

------------------------------------------------------------------------
-- test-eq  ⟶  test-eq      (output is the 0/1 equality flag)
------------------------------------------------------------------------

test-eq-fwd : ∀ {P S st st' a b output}
  → State.mem st output ≡ nothing
  → step P S st (test-eq a b output) ≡ just st'
  → holds (witness-of P st') (test-eq output a b)
test-eq-fwd {P} {S} {st} {st'} {a} {b} {output} fresh st≡
  with resolve (State.mem st) a in eqa | resolve (State.mem st) b in eqb
... | just av | just bv with valEq? av bv in eqv
...   | just e with refl ← st≡ =
          av , bv , e
        , ⊢-pres P st output _ a fresh eqa
        , ⊢-pres P st output _ b fresh eqb
        , eqv
        , assign-here P st output (val-native (χˢ e))

------------------------------------------------------------------------
-- less-than  ⟶  less-than   (two range proofs + the comparison flag)
--
-- The step checks the exact `2 ^ bits` operand bounds; the constraint
-- states the chip's evened `2 ^ even4 bits` bound, so the range proofs
-- are weakened via `<-even4`.
------------------------------------------------------------------------

less-than-fwd : ∀ {P S st st' a b bits output}
  → State.mem st output ≡ nothing
  → step P S st (less-than a b bits output) ≡ just st'
  → holds (witness-of P st') (less-than output a b bits)
less-than-fwd {P} {S} {st} {st'} {a} {b} {bits} {output} fresh st≡
  with resolveᶠ (State.mem st) a in eqa | resolveᶠ (State.mem st) b in eqb
... | just x | just y with valFr x <? 2 ^ bits
...   | yes px with valFr y <? 2 ^ bits
...     | yes py with refl ← st≡ =
            x , y
          , ⊢ᶠ-pres P st output _ a fresh eqa
          , ⊢ᶠ-pres P st output _ b fresh eqb
          , <-even4 bits px , <-even4 bits py
          , assign-here P st output
              (val-native (χˢ (isYes (valFr x <? valFr y))))

------------------------------------------------------------------------
-- reconstitute-field  ⟶  reconstitute
--
-- The first two off-circuit guards supply exactly the two range proofs
-- `reconstitute` asks for; the third (no field overflow) is a producer
-- obligation not part of the constraint.
------------------------------------------------------------------------

reconstitute-field-fwd : ∀ {P S st st' divisor modulus bits output}
  → State.mem st output ≡ nothing
  → step P S st (reconstitute-field divisor modulus bits output) ≡ just st'
  → holds (witness-of P st') (reconstitute output divisor modulus bits)
reconstitute-field-fwd {P} {S} {st} {st'} {divisor} {modulus} {bits} {output}
  fresh st≡
  with resolveᶠ (State.mem st) divisor in eqd
     | resolveᶠ (State.mem st) modulus in eqm
... | just d | just mo with valFr mo <? 2 ^ bits
...   | yes pm with valFr d <? 2 ^ (FR-BITS ∸ bits)
...     | yes pd with valFr mo + 2 ^ bits * valFr d <? FR-ORDER
...       | yes _ with refl ← st≡ =
              d , mo
            , ⊢ᶠ-pres P st output _ divisor fresh eqd
            , ⊢ᶠ-pres P st output _ modulus fresh eqm
            , pd , pm
            , assign-here P st output
                (val-native ((pow2ᶠˢ bits *ᶠ d) +ᶠ mo))

------------------------------------------------------------------------
-- jubjub-scalar-from-native  ⟶  scalar-from-native
------------------------------------------------------------------------

jubjub-scalar-from-native-fwd : ∀ {P S st st' a output}
  → State.mem st output ≡ nothing
  → step P S st (jubjub-scalar-from-native a output) ≡ just st'
  → holds (witness-of P st') (scalar-from-native output a)
jubjub-scalar-from-native-fwd {P} {S} {st} {st'} {a} {output} fresh st≡
  with resolveᶠ (State.mem st) a in eqf
... | just x with refl ← st≡ =
        x , ⊢ᶠ-pres P st output _ a fresh eqf
      , assign-here P st output _

------------------------------------------------------------------------
-- ec-mul  ⟶  ec-mul        (JubjubPoint × JubjubScalar)
------------------------------------------------------------------------

ec-mul-fwd : ∀ {P S st st' a scalar output}
  → State.mem st output ≡ nothing
  → step P S st (ec-mul a scalar output) ≡ just st'
  → holds (witness-of P st') (ec-mul output a scalar)
ec-mul-fwd {P} {S} {st} {st'} {a} {scalar} {output} fresh st≡
  with resolve (State.mem st) a in eqa | resolve (State.mem st) scalar in eqs
... | just (val-jubjub-point p) | just (val-jubjub-scalar s)
        with refl ← st≡ =
          inj₁ ( p , s
               , ⊢-pres P st output _ a fresh eqa
               , ⊢-pres P st output _ scalar fresh eqs
               , assign-here P st output _ )
... | just (val-secp-point p) | just (val-secp-scalar s)
        with refl ← st≡ =
          inj₂ ( p , s
               , ⊢-pres P st output _ a fresh eqa
               , ⊢-pres P st output _ scalar fresh eqs
               , assign-here P st output _ )

------------------------------------------------------------------------
-- ec-mul-generator  ⟶  ec-gen        (JubjubScalar)
------------------------------------------------------------------------

ec-mul-generator-fwd : ∀ {P S st st' scalar output}
  → State.mem st output ≡ nothing
  → step P S st (ec-mul-generator scalar output) ≡ just st'
  → holds (witness-of P st') (ec-gen output scalar)
ec-mul-generator-fwd {P} {S} {st} {st'} {scalar} {output} fresh st≡
  with resolve (State.mem st) scalar in eqs
... | just (val-jubjub-scalar s) with refl ← st≡ =
        inj₁ ( s , ⊢-pres P st output _ scalar fresh eqs
             , assign-here P st output _ )
... | just (val-secp-scalar s) with refl ← st≡ =
        inj₂ ( s , ⊢-pres P st output _ scalar fresh eqs
             , assign-here P st output _ )

------------------------------------------------------------------------
-- hash-to-curve  ⟶  h2c       (Native inputs → JubjubPoint)
------------------------------------------------------------------------

hash-to-curve-fwd : ∀ {P S st st' inputs output}
  → State.mem st output ≡ nothing
  → step P S st (hash-to-curve inputs output) ≡ just st'
  → holds (witness-of P st') (h2c output inputs)
hash-to-curve-fwd {P} {S} {st} {st'} {inputs} {output} fresh st≡
  with resolve-all-Fr (State.mem st) inputs in eqi
... | just frs with refl ← st≡ =
        frs , ⊢all-pres P st output _ inputs fresh eqi
      , assign-here P st output _

------------------------------------------------------------------------
-- into-bytes32  ⟶  into-bytes        (Native → Bytes32)
------------------------------------------------------------------------

into-bytes32-fwd : ∀ {P S st st' input output}
  → State.mem st output ≡ nothing
  → step P S st (into-bytes32 input output) ≡ just st'
  → holds (witness-of P st') (into-bytes output input)
into-bytes32-fwd {P} {S} {st} {st'} {input} {output} fresh st≡
  with resolve (State.mem st) input in eqv
... | just (val-native x) with refl ← st≡ =
        val-native x , val-bytes32 (nativeToBytes x)
      , ⊢-pres P st output _ input fresh eqv
      , assign-here P st output _
      , inj₁ (x , refl , refl)
... | just (val-secp-base x) with refl ← st≡ =
        val-secp-base x , val-bytes32 (secpBaseToBytes x)
      , ⊢-pres P st output _ input fresh eqv
      , assign-here P st output _
      , inj₂ (inj₁ (x , refl , refl))
... | just (val-secp-scalar s) with refl ← st≡ =
        val-secp-scalar s , val-bytes32 (secpScalarToBytes s)
      , ⊢-pres P st output _ input fresh eqv
      , assign-here P st output _
      , inj₂ (inj₂ (s , refl , refl))

------------------------------------------------------------------------
-- from-bytes32  ⟶  from-bytes        (Bytes32 → Native; val-t = native)
------------------------------------------------------------------------

from-bytes32-fwd : ∀ {P S st st' bytes val-t output}
  → State.mem st output ≡ nothing
  → step P S st (from-bytes32 bytes val-t output) ≡ just st'
  → holds (witness-of P st') (from-bytes output bytes)
from-bytes32-fwd {P} {S} {st} {st'} {bytes} {native} {output} fresh st≡
  with resolve (State.mem st) bytes in eqv
... | just (val-bytes32 b) with refl ← st≡ =
        b , ⊢-pres P st output _ bytes fresh eqv
      , inj₁ (assign-here P st output _)
from-bytes32-fwd {P} {S} {st} {st'} {bytes} {secp-base} {output} fresh st≡
  with resolve (State.mem st) bytes in eqv
... | just (val-bytes32 b) with refl ← st≡ =
        b , ⊢-pres P st output _ bytes fresh eqv
      , inj₂ (inj₁ (assign-here P st output _))
from-bytes32-fwd {P} {S} {st} {st'} {bytes} {secp-scalar} {output} fresh st≡
  with resolve (State.mem st) bytes in eqv
... | just (val-bytes32 b) with refl ← st≡ =
        b , ⊢-pres P st output _ bytes fresh eqv
      , inj₂ (inj₂ (assign-here P st output _))
from-bytes32-fwd {st = st} {bytes = bytes} {bytes32} _ st≡
  with resolve (State.mem st) bytes
... | just (val-bytes32 _) = case st≡ of λ ()
from-bytes32-fwd {st = st} {bytes = bytes} {jubjub-point} _ st≡
  with resolve (State.mem st) bytes
... | just (val-bytes32 _) = case st≡ of λ ()
from-bytes32-fwd {st = st} {bytes = bytes} {jubjub-scalar} _ st≡
  with resolve (State.mem st) bytes
... | just (val-bytes32 _) = case st≡ of λ ()
from-bytes32-fwd {st = st} {bytes = bytes} {secp-point} _ st≡
  with resolve (State.mem st) bytes
... | just (val-bytes32 _) = case st≡ of λ ()

------------------------------------------------------------------------
-- reverse-bytes  ⟶  reverse-bytes        (Bytes32 → Bytes32)
------------------------------------------------------------------------

reverse-bytes-fwd : ∀ {P S st st' bytes output}
  → State.mem st output ≡ nothing
  → step P S st (reverse-bytes bytes output) ≡ just st'
  → holds (witness-of P st') (reverse-bytes output bytes)
reverse-bytes-fwd {P} {S} {st} {st'} {bytes} {output} fresh st≡
  with resolve (State.mem st) bytes in eqv
... | just (val-bytes32 b) with refl ← st≡ =
        b , ⊢-pres P st output _ bytes fresh eqv , assign-here P st output _

------------------------------------------------------------------------
-- transient-hash  ⟶  poseidon        (Native inputs → Native)
------------------------------------------------------------------------

transient-hash-fwd : ∀ {P S st st' inputs output}
  → State.mem st output ≡ nothing
  → step P S st (transient-hash inputs output) ≡ just st'
  → holds (witness-of P st') (poseidon output inputs)
transient-hash-fwd {P} {S} {st} {st'} {inputs} {output} fresh st≡
  with resolve-all-Fr (State.mem st) inputs in eqi
... | just frs with refl ← st≡ =
        frs , ⊢all-pres P st output _ inputs fresh eqi
      , assign-here P st output _

------------------------------------------------------------------------
-- from-coordinates  ⟶  from-coords        (single output, pair input)
------------------------------------------------------------------------

from-coordinates-fwd : ∀ {P S st st' xop yop output}
  → State.mem st output ≡ nothing
  → step P S st (from-coordinates (xop , yop) output) ≡ just st'
  → holds (witness-of P st') (from-coords output xop yop)
from-coordinates-fwd {P} {S} {st} {st'} {xop} {yop} {output} fresh st≡
  with resolve (State.mem st) xop in eqx | resolve (State.mem st) yop in eqy
... | just (val-native x) | just (val-native y) with fromCoordsJ x y in eqp
...   | just p with refl ← st≡ =
          inj₁ ( x , y , p
               , ⊢ᶠ-pres P st output _ xop fresh
                   (resolveᶠ-intro (State.mem st) xop eqx)
               , ⊢ᶠ-pres P st output _ yop fresh
                   (resolveᶠ-intro (State.mem st) yop eqy)
               , eqp
               , assign-here P st output _ )
from-coordinates-fwd {P} {S} {st} {st'} {xop} {yop} {output} fresh st≡
    | just (val-secp-base x) | just (val-secp-base y)
        with fromCoordsK x y in eqp
...   | just p with refl ← st≡ =
          inj₂ ( x , y , p
               , ⊢-pres P st output _ xop fresh eqx
               , ⊢-pres P st output _ yop fresh eqy
               , eqp
               , assign-here P st output _ )

------------------------------------------------------------------------
-- bytes32-from-low-high  ⟶  bytes-from-low-high
------------------------------------------------------------------------

bytes32-from-low-high-fwd : ∀ {P S st st' loop hiop output}
  → State.mem st output ≡ nothing
  → step P S st (bytes32-from-low-high (loop , hiop) output) ≡ just st'
  → holds (witness-of P st') (bytes-from-low-high output loop hiop)
bytes32-from-low-high-fwd {P} {S} {st} {st'} {loop} {hiop} {output} fresh st≡
  with resolveᶠ (State.mem st) loop in eql | resolveᶠ (State.mem st) hiop in eqh
... | just lo | just hi with low-high→bytes32 lo hi in eqb
...   | just b with refl ← st≡ =
          lo , hi , b
        , ⊢ᶠ-pres P st output _ loop fresh eql
        , ⊢ᶠ-pres P st output _ hiop fresh eqh
        , eqb
        , assign-here P st output _

------------------------------------------------------------------------
-- into-coordinates  ⟶  into-coords        (two distinct outputs)
------------------------------------------------------------------------

into-coordinates-fwd : ∀ {P S st st' point xid yid}
  → State.mem st xid ≡ nothing
  → State.mem st yid ≡ nothing
  → ¬ (xid ≡ yid)
  → step P S st (into-coordinates point (xid , yid)) ≡ just st'
  → holds (witness-of P st') (into-coords xid yid point)
into-coordinates-fwd {P} {S} {st} {st'} {point} {xid} {yid}
  freshx freshy xid≢yid st≡
  with resolve (State.mem st) point in eqp
... | just (val-jubjub-point p) with coordsJ p in eqc
...   | (x , y) with refl ← st≡ =
          inj₁ ( p , x , y
               , ⊢-pres2 P st xid (val-native x) yid (val-native y) point
                   freshx freshy (λ yid≡xid → xid≢yid (sym yid≡xid)) eqp
               , eqc
               , assign-inner P st xid (val-native x) yid (val-native y)
                   xid≢yid
               , assign-outer P st xid (val-native x) yid (val-native y) )
into-coordinates-fwd {P} {S} {st} {st'} {point} {xid} {yid}
  freshx freshy xid≢yid st≡
    | just (val-secp-point p) with coordsK p in eqc
...   | just (x , y) with refl ← st≡ =
          inj₂ ( p , x , y
               , ⊢-pres2 P st xid (val-secp-base x) yid (val-secp-base y) point
                   freshx freshy (λ yid≡xid → xid≢yid (sym yid≡xid)) eqp
               , eqc
               , assign-inner P st xid (val-secp-base x) yid (val-secp-base y)
                   xid≢yid
               , assign-outer P st xid (val-secp-base x) yid (val-secp-base y) )

------------------------------------------------------------------------
-- bytes32-into-low-high  ⟶  bytes-into-low-high   (two distinct outputs)
------------------------------------------------------------------------

bytes32-into-low-high-fwd : ∀ {P S st st' bytes loid hiid}
  → State.mem st loid ≡ nothing
  → State.mem st hiid ≡ nothing
  → ¬ (loid ≡ hiid)
  → step P S st (bytes32-into-low-high bytes (loid , hiid)) ≡ just st'
  → holds (witness-of P st') (bytes-into-low-high loid hiid bytes)
bytes32-into-low-high-fwd {P} {S} {st} {st'} {bytes} {loid} {hiid}
  freshl freshh loid≢hiid st≡
  with resolve (State.mem st) bytes in eqv
... | just (val-bytes32 b) with bytes32→low-high b in eqs
...   | (lo , hi) with refl ← st≡ =
          b , lo , hi
        , ⊢-pres2 P st loid (val-native lo) hiid (val-native hi) bytes
            freshl freshh (λ hiid≡loid → loid≢hiid (sym hiid≡loid)) eqv
        , eqs
        , assign-inner P st loid (val-native lo) hiid (val-native hi) loid≢hiid
        , assign-outer P st loid (val-native lo) hiid (val-native hi)

------------------------------------------------------------------------
-- div-mod-power-of-two  ⟶  div-mod      (two distinct outputs q, r)
--
-- Stated for the WF3-mandated two-output shape; other shapes emit no
-- constraint.
------------------------------------------------------------------------

div-mod-power-of-two-fwd : ∀ {P S st st' val bits q r}
  → State.mem st q ≡ nothing
  → State.mem st r ≡ nothing
  → ¬ (q ≡ r)
  → step P S st (div-mod-power-of-two val bits (q ∷ r ∷ [])) ≡ just st'
  → holds (witness-of P st') (div-mod q r val bits)
div-mod-power-of-two-fwd {P} {S} {st} {st'} {val} {bits} {q} {r}
  freshq freshr q≢r st≡
  with resolveᶠ (State.mem st) val in eqf
... | just x with refl ← st≡ =
        x
      , ⊢ᶠ-pres2 P st q _ r _ val freshq freshr (λ r≡q → q≢r (sym r≡q)) eqf
      , assign-inner P st q _ r _ q≢r
      , assign-outer P st q _ r _

------------------------------------------------------------------------
-- persistent-hash  ⟶  sha256       (single Bytes32 output)
------------------------------------------------------------------------

persistent-hash-fwd : ∀ {P S st st' alignment inputs output}
  → State.mem st output ≡ nothing
  → step P S st (persistent-hash alignment inputs output) ≡ just st'
  → holds (witness-of P st') (sha256 output alignment inputs)
persistent-hash-fwd {P} {S} {st} {st'} {alignment} {inputs} {output}
  fresh st≡
  with resolve-all-Fr (State.mem st) inputs in eqi
... | just frs with persistent-hash-fn alignment frs in eqhp
...   | just v with refl ← st≡ =
          frs , v
        , ⊢all-pres P st output _ inputs fresh eqi
        , eqhp
        , assign-here P st output _

------------------------------------------------------------------------
-- keccak256  ⟶  keccak        (single Bytes32 output)
------------------------------------------------------------------------

keccak256-fwd : ∀ {P S st st' alignment inputs output}
  → State.mem st output ≡ nothing
  → step P S st (keccak256 alignment inputs output) ≡ just st'
  → holds (witness-of P st') (keccak output alignment inputs)
keccak256-fwd {P} {S} {st} {st'} {alignment} {inputs} {output}
  fresh st≡
  with resolve-all-Fr (State.mem st) inputs in eqi
... | just frs with keccak-fn alignment frs in eqhk
...   | just v with refl ← st≡ =
          frs , v
        , ⊢all-pres P st output _ inputs fresh eqi
        , eqhk
        , assign-here P st output _

------------------------------------------------------------------------
-- cond-select  ⟶  select
--
-- The off-circuit selector reads the bit through `to𝔹`; booleanity comes
-- from the `to𝔹` success branches, and the two select implications hold
-- because `to𝔹 0ᶠ = false` and `to𝔹 1ᶠ = true`.
------------------------------------------------------------------------

cond-select-fwd : ∀ {P S st st' bit a b output}
  → State.mem st output ≡ nothing
  → step P S st (cond-select bit a b output) ≡ just st'
  → holds (witness-of P st') (select output bit a b)
cond-select-fwd {P} {S} {st} {st'} {bit} {a} {b} {output} fresh st≡
  with resolveᶠ (State.mem st) bit in eqf
... | just x with x ≟ᶠ 0ᶠ
...   | yes x≡0
        with resolve (State.mem st) a in eqa | resolve (State.mem st) b in eqb
...     | just av | just bvl with typeof av ≟T typeof bvl
...       | yes _ with refl ← st≡ =
              x , av , bvl , bvl
            , ⊢ᶠ-pres P st output _ bit fresh eqf
            , ⊢-pres P st output _ a fresh eqa
            , ⊢-pres P st output _ b fresh eqb
            , assign-here P st output _
            , inj₁ x≡0
            , (λ x≡1 → case 1ᶠ≢0ᶠ (trans (sym x≡1) x≡0) of λ ())
            , (λ _ → refl)
cond-select-fwd {P} {S} {st} {st'} {bit} {a} {b} {output} fresh st≡
    | just x | no _ with x ≟ᶠ 1ᶠ
...   | yes x≡1
        with resolve (State.mem st) a in eqa | resolve (State.mem st) b in eqb
...     | just av | just bvl with typeof av ≟T typeof bvl
...       | yes _ with refl ← st≡ =
              x , av , bvl , av
            , ⊢ᶠ-pres P st output _ bit fresh eqf
            , ⊢-pres P st output _ a fresh eqa
            , ⊢-pres P st output _ b fresh eqb
            , assign-here P st output _
            , inj₂ x≡1
            , (λ _ → refl)
            , (λ x≡0 → case 1ᶠ≢0ᶠ (trans (sym x≡1) x≡0) of λ ())

open import Data.List using (_++_)
open import Data.List.Properties using (++-assoc)
open import Data.Product using (∃)
open import Data.Maybe.Properties using (just-injective)

-- (`out-fresh`, `step-extends`, and the other single-step facts these
-- lemmas consume live in SemanticsProperties.)

open import zkir-v3.Semantics ⋯
  using (insertMany; eval-guard; collectOutputs; _≟LFr_; run;
         init; decode-inputs)
open import Data.List using (length; take; drop)

------------------------------------------------------------------------
-- encode  ⟶  encode-eq
--
-- A successful step resolves `input` to a value `v` and binds the raw
-- field elements `map val-native (encodeᵉ v)` to `outputs` via
-- `insertMany`.  The constraint `encode-eq input outputs` asks for that
-- same `v`, that `input` resolve to it at the post-step witness, and that
-- each output cell hold its element.  The input resolution is transported
-- through the (fresh, distinct-from-the-input) output bindings by memory
-- monotonicity; the output binding is read off `insertMany` with `NoDup`
-- ensuring earlier cells survive the later ones.
------------------------------------------------------------------------

-- Each `insertMany`-bound cell holds its value at the final state, given
-- the keys were fresh and duplicate-free.
insertMany-bind-each : ∀ P st ids vs {st'}
  → insertMany st ids vs ≡ just st'
  → AllFresh ids (State.mem st) → NoDup ids
  → bind-each (witness-of P st') vs ids
insertMany-bind-each P st []        []       refl _           _            = tt
insertMany-bind-each P st (id ∷ ids) (v ∷ vs) e (fid , frs) (nin , ndup) =
    insertMany-⊑ (out1 st id v) ids vs e
      (allfresh-ins id v ids (State.mem st) frs nin) ndup
      {id} (ins-here id v (State.mem st))
  , insertMany-bind-each P (out1 st id v) ids vs e
      (allfresh-ins id v ids (State.mem st) frs nin) ndup
insertMany-bind-each P st []        (_ ∷ _)  ()  _           _
insertMany-bind-each P st (_ ∷ _)   []       ()  _           _

encode-fwd : ∀ {P S st st' input outputs}
  → out-fresh (encode input outputs) (State.mem st)
  → step P S st (encode input outputs) ≡ just st'
  → holds (witness-of P st') (encode-eq input outputs)
encode-fwd {P} {S} {st} {st'} {input} {outputs} (af , nd) e
  with resolve (State.mem st) input in eqi
... | just v =
      v
    , trans (resolve-agree P st' input)
        (resolve-mono input
          (insertMany-⊑ st outputs (map val-native (encodeᵉ v)) e af nd) eqi)
    , insertMany-bind-each P st outputs (map val-native (encodeᵉ v)) e af nd

------------------------------------------------------------------------
-- impact  ⟶  impact-constraints
--
-- A successful `impact` step appends, for each input, one field element
-- to the public-input vector: the input's value when the guard reads
-- `true`, or `0ᶠ` when it reads `false`.  The synthesised constraint
-- list `impact-constraints start guard inputs` (with `start` the cursor
-- the synth maintains, here `length (State.pis st)`) places one
-- `pi-impact` per input, asking that the matching public-input entry be
-- the guarded value.  Memory is unchanged by the step, so every operand
-- resolves at `witness-of P st'` exactly as it did off-circuit; the
-- public-input vector is `State.pis st ++ guarded`, so the entry at
-- `length (State.pis st) + k` is the k-th element of `guarded`.
------------------------------------------------------------------------

-- Resolution against an explicit `mk-witness` depends only on its
-- `assign` field (the named store), so it agrees with `resolveᶠ`.
resolveᶜ-Fr-mem-agree : ∀ (m : Mem) pis cr op
  → resolveᶜ-Fr (mk-witness m pis cr) op ≡ resolveᶠ m op
resolveᶜ-Fr-mem-agree m pis cr (imm x) = refl
resolveᶜ-Fr-mem-agree m pis cr (var id) with m id
... | nothing                    = refl
... | just (val-native _)        = refl
... | just (val-bytes32 _)       = refl
... | just (val-jubjub-point _)  = refl
... | just (val-jubjub-scalar _) = refl
... | just (val-secp-point _)    = refl
... | just (val-secp-base _)     = refl
... | just (val-secp-scalar _)   = refl

-- "The public-input entries from `start` onward are exactly `guarded`."
-- A structured lookup hypothesis that unfolds one entry per step,
-- matching the `impact-constraints` recursion (which advances `start`).
PisAt : (full : List Fr) (start : ℕ) (guarded : List Fr) → Set
PisAt full start []          = ⊤
PisAt full start (pv ∷ rest) =
  (pi-lookup full start ≡ just pv) × PisAt full (suc start) rest

-- Prepending one entry to `full` shifts every index up by one.
PisAt-cons : ∀ z full start guarded
  → PisAt full start guarded → PisAt (z ∷ full) (suc start) guarded
PisAt-cons z full start []          p        = tt
PisAt-cons z full start (pv ∷ rest) (h , hs) =
  h , PisAt-cons z full (suc start) rest hs

-- A list reads itself off from index `0`.
PisAt-self : ∀ (guarded : List Fr) → PisAt guarded 0 guarded
PisAt-self []          = tt
PisAt-self (pv ∷ rest) =
  refl , PisAt-cons pv rest 0 rest (PisAt-self rest)

-- The guarded suffix appended after `pre` is found from index
-- `length pre` onward.
PisAt-app : ∀ (pre guarded : List Fr)
  → PisAt (pre ++ guarded) (length pre) guarded
PisAt-app []        guarded = PisAt-self guarded
PisAt-app (z ∷ pre) guarded =
  PisAt-cons z (pre ++ guarded) (length pre) guarded (PisAt-app pre guarded)

-- The guard's field reading is a bit (it is `0ᶠ` or `1ᶠ`).
to𝔹-is-bit : ∀ {gᶠ b} → to𝔹 gᶠ ≡ just b → is-bit gᶠ
to𝔹-is-bit {gᶠ} {false} e = inj₁ (to𝔹-false e)
to𝔹-is-bit {gᶠ} {true}  e = inj₂ (to𝔹-true e)

-- The select implications of `pi-impact`, given the guard's field value
-- `gᶠ`, the bit reading `b = to𝔹 gᶠ`, and the appended entry `pv`.  When
-- `b = true` the entry is the input value; when `b = false` it is `0ᶠ`.
impact-select-true : ∀ {gᶠ xv}
  → to𝔹 gᶠ ≡ just true
  → (gᶠ ≡ 1ᶠ → xv ≡ xv) × (gᶠ ≡ 0ᶠ → xv ≡ 0ᶠ)
impact-select-true {gᶠ} {xv} e =
    (λ _ → refl)
  , (λ gᶠ≡0 → case 1ᶠ≢0ᶠ (trans (sym (to𝔹-true e)) gᶠ≡0) of λ ())

impact-select-false : ∀ {gᶠ xv}
  → to𝔹 gᶠ ≡ just false
  → (gᶠ ≡ 1ᶠ → 0ᶠ ≡ xv) × (gᶠ ≡ 0ᶠ → 0ᶠ ≡ 0ᶠ)
impact-select-false {gᶠ} e =
    (λ gᶠ≡1 → case 1ᶠ≢0ᶠ (trans (sym gᶠ≡1) (to𝔹-false e)) of λ ())
  , (λ _ → refl)

-- The core induction.  The witness public-input vector is a fixed list
-- `full`; the entries from `start` onward are the guarded values
-- (`PisAt full start guarded`).  Each `pi-impact start guard xₖ` holds:
-- the guard reads the single field value `gᶠ` (its bit reading `b` fixed
-- for the whole run), `xₖ` reads `valₖ`, and the entry at `start + k` is
-- `valₖ` when `b = true`, else `0ᶠ`.  The `impact-constraints` recursion
-- advances `start`, matching the `PisAt` unfolding.
impact-sat : ∀ (m : Mem) full cr start guard {gᶠ b}
  → (inputs : List Operand) (vals : List Fr)
  → resolveᶠ m guard ≡ just gᶠ
  → to𝔹 gᶠ ≡ just b
  → resolve-all-Fr m inputs ≡ just vals
  → PisAt full start (if b then vals else map (λ _ → 0ᶠ) vals)
  → satisfies-constraints
      (impact-constraints start guard inputs)
      (mk-witness m full cr)
impact-sat m full cr start guard []       _   rg tb rv at = tt
impact-sat m full cr start guard {gᶠ} {true}  (x ∷ xs) vals rg tb rv at
  with resolveᶠ m x in eqx | resolve-all-Fr m xs in eqxs
... | nothing  | _        = case rv of λ ()
... | just vx  | nothing  = case rv of λ ()
... | just vx  | just vxs with refl ← rv =
      let (pl , at-rest) = at in
      ( gᶠ , vx , vx
      , trans (resolveᶜ-Fr-mem-agree m full cr guard) rg
      , trans (resolveᶜ-Fr-mem-agree m full cr x) eqx
      , to𝔹-is-bit tb , pl , impact-select-true {gᶠ} {vx} tb )
    , impact-sat m full cr (suc start) guard {gᶠ} {true} xs vxs rg tb eqxs
        at-rest
impact-sat m full cr start guard {gᶠ} {false} (x ∷ xs) vals rg tb rv at
  with resolveᶠ m x in eqx | resolve-all-Fr m xs in eqxs
... | nothing  | _        = case rv of λ ()
... | just vx  | nothing  = case rv of λ ()
... | just vx  | just vxs with refl ← rv =
      let (pl , at-rest) = at in
      ( gᶠ , vx , 0ᶠ
      , trans (resolveᶜ-Fr-mem-agree m full cr guard) rg
      , trans (resolveᶜ-Fr-mem-agree m full cr x) eqx
      , to𝔹-is-bit tb , pl , impact-select-false {gᶠ} {vx} tb )
    , impact-sat m full cr (suc start) guard {gᶠ} {false} xs vxs rg tb eqxs
        at-rest

impact-fwd : ∀ {P S st st' guard inputs}
  → step P S st (impact guard inputs) ≡ just st'
  → satisfies-constraints
      (impact-constraints (length (State.pis st)) guard inputs)
      (witness-of P st')
impact-fwd {P} {S} {st} {st'} {guard} {inputs} st≡
  with resolve-all-Fr (State.mem st) inputs in eqi
... | just vals with resolveᶠ (State.mem st) guard in eqg
...   | just gᶠ with to𝔹 gᶠ in eqb
...     | just true
          with take (length vals)
                 (drop (State.pti-idx st)
                   (ProofPreimage.pub-transcript-inputs P))
               ≟LFr vals
...       | yes _ with refl ← st≡ =
            impact-sat (State.mem st)
              (State.pis st ++ vals)
              (comm-rand-of (ProofPreimage.comm-commitment P))
              (length (State.pis st)) guard {gᶠ} {true} inputs vals
              eqg eqb eqi (PisAt-app (State.pis st) vals)
impact-fwd {P} {S} {st} {st'} {guard} {inputs} st≡
    | just vals | just gᶠ | just false with refl ← st≡ =
        impact-sat (State.mem st)
          (State.pis st ++ map (λ _ → 0ᶠ) vals)
          (comm-rand-of (ProofPreimage.comm-commitment P))
          (length (State.pis st)) guard {gᶠ} {false} inputs vals
          eqg eqb eqi (PisAt-app (State.pis st) (map (λ _ → 0ᶠ) vals))

------------------------------------------------------------------------
-- Lifting constraints from an intermediate witness to the final one.
--
-- The rest of a run only extends the store and the public inputs
-- (`run-extends`), and satisfaction is monotone under that — so a
-- constraint (or list) proven at an intermediate run-state still holds
-- at the run's final witness.
------------------------------------------------------------------------

sat-mono : ∀ {m m′ pis pis′ cr} cs
  → m ⊑ m′ → pis ≼ pis′
  → satisfies-constraints cs (mk-witness m pis cr)
  → satisfies-constraints cs (mk-witness m′ pis′ cr)
sat-mono []       sub pre _        = tt
sat-mono (c ∷ cs) sub pre (hc , h) =
  holds-mono c sub pre hc , sat-mono cs sub pre h

lift-cs : ∀ {P S s} cs is st-mid
  → run P S st-mid is ≡ just s → WF-run P S is st-mid
  → satisfies-constraints cs (witness-of P st-mid)
  → satisfies-constraints cs (witness-of P s)
lift-cs cs is st-mid run-rest wf h =
  let (m⊑ , p≼) = run-extends is st-mid run-rest wf
  in sat-mono cs m⊑ p≼ h

sat-++ : ∀ xs ys w
  → satisfies-constraints xs w → satisfies-constraints ys w
  → satisfies-constraints (xs ++ ys) w
sat-++ []       ys w _        hys = hys
sat-++ (x ∷ xs) ys w (hx , h) hys = hx , sat-++ xs ys w h hys

------------------------------------------------------------------------
-- The forward direction of circuit faithfulness.
--
-- A successful preprocess run, under single-assignment well-formedness
-- (`WF-run`), satisfies every synthesised constraint at its final
-- witness.  `fwd-go` folds a single per-instruction dispatch through the
-- run: for the head instruction `i`, `pushed-fwd` proves its contributed
-- constraints `pushed i ss` hold at the witness of the state *just after*
-- it; `push-cs` reassociates `synth-instr i ss` as the accumulator plus
-- that contribution; and `lift-cs` carries the fact forward to the run's
-- final witness, since the remaining run only extends the store and the
-- public inputs.  The synth cursor `next-pi` is kept in lock-step with
-- the length of the public-input vector (`pi-inv-step`), which is what
-- makes the single `impact` instruction place its `pi-impact` entries at
-- the right indices.
------------------------------------------------------------------------

open import Data.List.Properties using (length-++; length-map)
open import Data.List using (length)
open import Relation.Binary.PropositionalEquality using (subst)

-- `insertMany` only rebinds memory, so it leaves `outs` untouched.
outs-insertMany : ∀ st ids vs {st'}
  → insertMany st ids vs ≡ just st' → State.outs st' ≡ State.outs st
outs-insertMany st []         []        refl = refl
outs-insertMany st (id ∷ ids) (v ∷ vs)  e    =
  outs-insertMany (out1 st id v) ids vs e
outs-insertMany st []         (_ ∷ _)   ()
outs-insertMany st (_ ∷ _)    []        ()

-- The public-input cursor and the output stream across one step, both
-- read off a single inversion of the step.  Cursor: every instruction
-- except `impact` leaves `pis` untouched while `synth-instr` leaves
-- `next-pi` untouched, so the invariant transports unchanged; `impact`
-- grows both sides by the input count.  Output stream: every instruction
-- except `circuit-output` leaves `outs` untouched.  `pi-inv-step` and
-- `step-outs-≡` are the two projections.
step-pi-outs : ∀ {P S st st'} (i : Instruction) ss
  → step P S st i ≡ just st'
  → SynthState.next-pi ss ≡ length (State.pis st)
  → (SynthState.next-pi (synth-instr i ss) ≡ length (State.pis st'))
    × (¬ (∃ λ vals → i ≡ circuit-output vals)
        → State.outs st' ≡ State.outs st)
step-pi-outs {st = st} (encode input outputs) ss e pi
  with resolve (State.mem st) input
... | just v rewrite insertMany-pis st outputs
                       (map val-native (encodeᵉ v)) e =
        pi , λ _ → outs-insertMany st outputs (map val-native (encodeᵉ v)) e
step-pi-outs {st = st} (assert cond) ss e pi
  with resolve𝔹 (State.mem st) cond
... | just true with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (cond-select bit a b output) ss e pi
  with resolve𝔹 (State.mem st) bit
... | just bv with resolve (State.mem st) a | resolve (State.mem st) b
...   | just av | just bvl with typeof av ≟T typeof bvl
...     | yes _ with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (constrain-bits val bits) ss e pi
  with resolveᶠ (State.mem st) val
... | just x with valFr x <? 2 ^ bits
...   | yes _ with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (constrain-eq a b) ss e pi
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just av | just bv with valEq? av bv
...   | just true with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (constrain-to-boolean val) ss e pi
  with resolve𝔹 (State.mem st) val
... | just _ with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (copy val output) ss e pi
  with resolve (State.mem st) val
... | just v with refl ← e = pi , λ _ → refl
step-pi-outs {P = P} {st = st} (impact guard inputs) ss e pi
  with resolve-all-Fr (State.mem st) inputs in eqi
... | just vals with resolve𝔹 (State.mem st) guard
...   | just g with g
...     | true
          with take (length vals)
                 (drop (State.pti-idx st)
                   (ProofPreimage.pub-transcript-inputs P))
               ≟LFr vals
...       | yes _ with refl ← e =
            trans (cong (SynthState.next-pi ss +_)
                        (sym (resolve-all-Fr-length (State.mem st) inputs eqi)))
              (trans (cong (_+ length vals) pi)
                (sym (length-++ (State.pis st) {vals})))
            , λ _ → refl
step-pi-outs {st = st} (impact guard inputs) ss e pi
    | just vals | just g | false with refl ← e =
        trans (cong (SynthState.next-pi ss +_)
                    (sym (resolve-all-Fr-length (State.mem st) inputs eqi)))
          (trans (cong (_+ length vals) pi)
            (trans (cong (length (State.pis st) +_)
                         (sym (length-map (λ _ → 0ᶠ) vals)))
              (sym (length-++ (State.pis st) {map (λ _ → 0ᶠ) vals}))))
        , λ _ → refl
step-pi-outs {st = st} (ec-mul a scalar output) ss e pi
  with resolve (State.mem st) a | resolve (State.mem st) scalar
... | just (val-jubjub-point p) | just (val-jubjub-scalar s)
        with refl ← e = pi , λ _ → refl
... | just (val-secp-point p) | just (val-secp-scalar s)
        with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (ec-mul-generator scalar output) ss e pi
  with resolve (State.mem st) scalar
... | just (val-jubjub-scalar s) with refl ← e = pi , λ _ → refl
... | just (val-secp-scalar s) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (hash-to-curve inputs output) ss e pi
  with resolve-all-Fr (State.mem st) inputs
... | just frs with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (into-coordinates point (xid , yid)) ss e pi
  with resolve (State.mem st) point
... | just (val-jubjub-point p) with coordsJ p
...   | (x , y) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (into-coordinates point (xid , yid)) ss e pi
    | just (val-secp-point p) with coordsK p
...   | just (x , y) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (from-coordinates (xop , yop) output) ss e pi
  with resolve (State.mem st) xop | resolve (State.mem st) yop
... | just (val-native x) | just (val-native y) with fromCoordsJ x y
...   | just p with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (from-coordinates (xop , yop) output) ss e pi
    | just (val-secp-base x) | just (val-secp-base y) with fromCoordsK x y
...   | just p with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (into-bytes32 input output) ss e pi
  with resolve (State.mem st) input
... | just (val-native x) with refl ← e = pi , λ _ → refl
... | just (val-secp-base x) with refl ← e = pi , λ _ → refl
... | just (val-secp-scalar s) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (from-bytes32 bytes native output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (from-bytes32 bytes secp-base output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (from-bytes32 bytes secp-scalar output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (from-bytes32 bytes bytes32 output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-pi-outs {st = st} (from-bytes32 bytes jubjub-point output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-pi-outs {st = st} (from-bytes32 bytes jubjub-scalar output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-pi-outs {st = st} (from-bytes32 bytes secp-point output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step-pi-outs {st = st} (reverse-bytes bytes output) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (bytes32-into-low-high bytes (loid , hiid)) ss e pi
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with bytes32→low-high b
...   | (lo , hi) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (bytes32-from-low-high (loop , hiop) output) ss e pi
  with resolveᶠ (State.mem st) loop | resolveᶠ (State.mem st) hiop
... | just lo | just hi with low-high→bytes32 lo hi
...   | just b with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (div-mod-power-of-two val bits []) ss e pi
  with resolveᶠ (State.mem st) val
... | just x = case e of λ ()
step-pi-outs {st = st} (div-mod-power-of-two val bits (o ∷ [])) ss e pi
  with resolveᶠ (State.mem st) val
... | just x = case e of λ ()
step-pi-outs {st = st} (div-mod-power-of-two val bits (q ∷ r ∷ [])) ss e pi
  with resolveᶠ (State.mem st) val
... | just x rewrite insertMany-pis st (q ∷ r ∷ [])
                       ( val-native (from-le-bits (drop bits (to-le-bits x)))
                       ∷ val-native (from-le-bits (take bits (to-le-bits x)))
                       ∷ []) e =
        pi , λ _ → outs-insertMany st (q ∷ r ∷ [])
          ( val-native (from-le-bits (drop bits (to-le-bits x)))
          ∷ val-native (from-le-bits (take bits (to-le-bits x))) ∷ []) e
step-pi-outs {st = st} (div-mod-power-of-two val bits (q ∷ r ∷ o ∷ outs)) ss e pi
  with resolveᶠ (State.mem st) val
... | just x = case e of λ ()
step-pi-outs {st = st} (reconstitute-field divisor modulus bits output) ss e pi
  with resolveᶠ (State.mem st) divisor | resolveᶠ (State.mem st) modulus
... | just d | just mo with valFr mo <? 2 ^ bits
...   | yes _ with valFr d <? 2 ^ (FR-BITS ∸ bits)
...     | yes _ with valFr mo + 2 ^ bits * valFr d <? FR-ORDER
...       | yes _ with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (transient-hash inputs output) ss e pi
  with resolve-all-Fr (State.mem st) inputs
... | just frs with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (persistent-hash alignment inputs output) ss e pi
  with resolve-all-Fr (State.mem st) inputs
... | just frs with persistent-hash-fn alignment frs
...   | just h with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (keccak256 alignment inputs output) ss e pi
  with resolve-all-Fr (State.mem st) inputs
... | just frs with keccak-fn alignment frs
...   | just h with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (test-eq a b output) ss e pi
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just av | just bv with valEq? av bv
...   | just eqv with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (add a b output) ss e pi
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just (val-native x) | just (val-native y) with refl ← e = pi , λ _ → refl
... | just (val-jubjub-point p) | just (val-jubjub-point q)
        with refl ← e = pi , λ _ → refl
... | just (val-secp-point p) | just (val-secp-point q)
        with refl ← e = pi , λ _ → refl
... | just (val-secp-base x) | just (val-secp-base y)
        with refl ← e = pi , λ _ → refl
... | just (val-secp-scalar x) | just (val-secp-scalar y)
        with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (mul a b output) ss e pi
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just (val-native x) | just (val-native y) with refl ← e = pi , λ _ → refl
... | just (val-secp-base x) | just (val-secp-base y)
        with refl ← e = pi , λ _ → refl
... | just (val-secp-scalar x) | just (val-secp-scalar y)
        with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (neg a output) ss e pi
  with resolve (State.mem st) a
... | just (val-native x) with refl ← e = pi , λ _ → refl
... | just (val-jubjub-point p) with refl ← e = pi , λ _ → refl
... | just (val-secp-point p) with refl ← e = pi , λ _ → refl
... | just (val-secp-base x) with refl ← e = pi , λ _ → refl
... | just (val-secp-scalar x) with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (inv a output) ss e pi
  with resolve (State.mem st) a
... | just (val-native x) with invᶠ x
...   | just xi with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (inv a output) ss e pi
    | just (val-secp-base x) with invᵇ x
...   | just xi with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (inv a output) ss e pi
    | just (val-secp-scalar x) with invˢ x
...   | just xi with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (not a output) ss e pi
  with resolve𝔹 (State.mem st) a
... | just b with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (less-than a b bits output) ss e pi
  with resolveᶠ (State.mem st) a | resolveᶠ (State.mem st) b
... | just x | just y with valFr x <? 2 ^ bits
...   | yes _ with valFr y <? 2 ^ bits
...     | yes _ with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (jubjub-scalar-from-native a output) ss e pi
  with resolveᶠ (State.mem st) a
... | just x with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (public-input guard val-t output) ss e pi
  with eval-guard (State.mem st) guard
... | just g with g
...   | true with decode val-t
                    (take (encoded-len val-t) (State.pto-rem st))
...     | just v with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (public-input guard val-t output) ss e pi
    | just g | false with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (private-input guard val-t output) ss e pi
  with eval-guard (State.mem st) guard
... | just g with g
...   | true with decode val-t
                    (take (encoded-len val-t) (State.priv-rem st))
...     | just v with refl ← e = pi , λ _ → refl
step-pi-outs {st = st} (private-input guard val-t output) ss e pi
    | just g | false with refl ← e = pi , λ _ → refl
step-pi-outs {S = S} {st = st} (circuit-output vals) ss e pi
  with collectOutputs (State.mem st) (IrSource.outputs S) vals
... | just vs with refl ← e = pi , λ ¬co → case ¬co (vals , refl) of λ ()

-- The public-input cursor projection: `synth-instr`'s cursor stays in
-- lock-step with the public-input length across the step.
pi-inv-step : ∀ {P S st st'} (i : Instruction) ss
  → step P S st i ≡ just st'
  → SynthState.next-pi ss ≡ length (State.pis st)
  → SynthState.next-pi (synth-instr i ss) ≡ length (State.pis st')
pi-inv-step i ss e pi = proj₁ (step-pi-outs i ss e pi)

------------------------------------------------------------------------
-- Per-instruction forward dispatch.
--
-- The constraints an instruction contributes to the accumulator are
-- exactly `pushed i ss` (CircuitBridge); `pushed-fwd` shows they all
-- hold at the witness of the state just after the instruction.  Every
-- clause defers to that instruction's `*-fwd` lemma; the shared
-- signature carries the union of hypotheses those lemmas need — the
-- step equation, the output-freshness `out-fresh i` (consumed by the
-- output-producing gates and by `impact`/`div-mod` distinctness), and
-- the public-input cursor invariant (used by `impact` to place its
-- `pi-impact` entries at the right indices).  The transcript-input and
-- output terminators push nothing, so their proof is `tt`.
------------------------------------------------------------------------
pushed-fwd : ∀ {P S st st'} i ss
  → step P S st i ≡ just st'
  → out-fresh i (State.mem st)
  → SynthState.next-pi ss ≡ length (State.pis st)
  → satisfies-constraints (pushed i ss) (witness-of P st')
pushed-fwd {P} {S} {st} {st'} (encode input outputs) ss se of pi =
  encode-fwd {P} {S} {st} {st'} {input} {outputs} of se , tt
pushed-fwd {P} {S} {st} {st'} (assert cond) ss se of pi =
  assert-fwd {P} {S} {st} {st'} {cond} se , tt
pushed-fwd {P} {S} {st} {st'} (cond-select bit a b o) ss se of pi =
  cond-select-fwd {P} {S} {st} {st'} {bit} {a} {b} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (constrain-bits val bits) ss se of pi =
  constrain-bits-fwd {P} {S} {st} {st'} {val} {bits} se , tt
pushed-fwd {P} {S} {st} {st'} (constrain-eq a b) ss se of pi =
  constrain-eq-fwd {P} {S} {st} {st'} {a} {b} se , tt
pushed-fwd {P} {S} {st} {st'} (constrain-to-boolean val) ss se of pi =
  constrain-to-boolean-fwd {P} {S} {st} {st'} {val} se , tt
pushed-fwd {P} {S} {st} {st'} (copy val o) ss se of pi =
  copy-fwd {P} {S} {st} {st'} {val} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (impact guard inputs) ss se of pi =
  subst
    (λ n → satisfies-constraints
             (impact-constraints n guard inputs) (witness-of P st'))
    (sym pi)
    (impact-fwd {P} {S} {st} {st'} {guard} {inputs} se)
pushed-fwd {P} {S} {st} {st'} (ec-mul a scalar o) ss se of pi =
  ec-mul-fwd {P} {S} {st} {st'} {a} {scalar} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (ec-mul-generator scalar o) ss se of pi =
  ec-mul-generator-fwd {P} {S} {st} {st'} {scalar} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (hash-to-curve inputs o) ss se of pi =
  hash-to-curve-fwd {P} {S} {st} {st'} {inputs} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (into-coordinates point (xo , yo)) ss se of pi =
  let (fx , fy , x≢y) = of in
  into-coordinates-fwd {P} {S} {st} {st'} {point} {xo} {yo} fx fy x≢y se , tt
pushed-fwd {P} {S} {st} {st'} (from-coordinates (x , y) o) ss se of pi =
  from-coordinates-fwd {P} {S} {st} {st'} {x} {y} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (into-bytes32 input o) ss se of pi =
  into-bytes32-fwd {P} {S} {st} {st'} {input} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (from-bytes32 bytes val-t o) ss se of pi =
  from-bytes32-fwd {P} {S} {st} {st'} {bytes} {val-t} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (reverse-bytes bytes o) ss se of pi =
  reverse-bytes-fwd {P} {S} {st} {st'} {bytes} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (bytes32-into-low-high bytes (lo , hi)) ss se of pi =
  let (fl , fh , l≢h) = of in
  bytes32-into-low-high-fwd {P} {S} {st} {st'} {bytes} {lo} {hi}
    fl fh l≢h se , tt
pushed-fwd {P} {S} {st} {st'} (bytes32-from-low-high (lo , hi) o) ss se of pi =
  bytes32-from-low-high-fwd {P} {S} {st} {st'} {lo} {hi} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (div-mod-power-of-two val bits (q ∷ r ∷ []))
  ss se of pi =
  let ((fq , fr , _) , ((q≢r , _) , _)) = of in
  div-mod-power-of-two-fwd {P} {S} {st} {st'} {val} {bits} {q} {r}
    fq fr q≢r se , tt
pushed-fwd (div-mod-power-of-two val bits []) ss se of pi = tt
pushed-fwd (div-mod-power-of-two val bits (_ ∷ [])) ss se of pi = tt
pushed-fwd (div-mod-power-of-two val bits (_ ∷ _ ∷ _ ∷ _)) ss se of pi = tt
pushed-fwd {P} {S} {st} {st'} (reconstitute-field d m bits o) ss se of pi =
  reconstitute-field-fwd {P} {S} {st} {st'} {d} {m} {bits} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (transient-hash inputs o) ss se of pi =
  transient-hash-fwd {P} {S} {st} {st'} {inputs} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (persistent-hash al inputs o) ss se of pi =
  persistent-hash-fwd {P} {S} {st} {st'} {al} {inputs} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (keccak256 al inputs o) ss se of pi =
  keccak256-fwd {P} {S} {st} {st'} {al} {inputs} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (test-eq a b o) ss se of pi =
  test-eq-fwd {P} {S} {st} {st'} {a} {b} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (add a b o) ss se of pi =
  add-fwd {P} {S} {st} {st'} {a} {b} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (mul a b o) ss se of pi =
  mul-fwd {P} {S} {st} {st'} {a} {b} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (neg a o) ss se of pi =
  neg-fwd {P} {S} {st} {st'} {a} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (inv a o) ss se of pi =
  inv-fwd {P} {S} {st} {st'} {a} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (not a o) ss se of pi =
  not-fwd {P} {S} {st} {st'} {a} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (less-than a b bits o) ss se of pi =
  less-than-fwd {P} {S} {st} {st'} {a} {b} {bits} {o} of se , tt
pushed-fwd {P} {S} {st} {st'} (jubjub-scalar-from-native a o) ss se of pi =
  jubjub-scalar-from-native-fwd {P} {S} {st} {st'} {a} {o} of se , tt
pushed-fwd (public-input guard val-t o) ss se of pi = tt
pushed-fwd (private-input guard val-t o) ss se of pi = tt
pushed-fwd (circuit-output vals) ss se of pi = tt

fwd-go : ∀ {P S s} is st ss
  → run P S st is ≡ just s
  → WF-run P S is st
  → SynthState.next-pi ss ≡ length (State.pis st)
  → satisfies-constraints (SynthState.constraints ss) (witness-of P s)
  → satisfies-constraints
      (SynthState.constraints (synth-instrs is ss)) (witness-of P s)
    × (SynthState.next-pi (synth-instrs is ss) ≡ length (State.pis s))
fwd-go []       st ss run-eq wf pi prev
  rewrite just-injective run-eq = prev , pi
fwd-go {P} {S} {s} (i ∷ is) st ss run-eq (of , wf-rest) pi prev =
  let (st-mid , step-eq , run-rest) = run-inv i is st run-eq
      prev′ = subst (λ cs → satisfies-constraints cs (witness-of P s))
                (sym (push-cs i ss))
                (sat-++ (SynthState.constraints ss) (pushed i ss)
                  (witness-of P s) prev
                  (lift-cs {P} {S} {s} (pushed i ss) is st-mid run-rest
                    (wf-rest step-eq)
                    (pushed-fwd {P} {S} {st} {st-mid} i ss step-eq of pi)))
  in fwd-go {P} {S} {s} is st-mid (synth-instr i ss) run-rest (wf-rest step-eq)
       (pi-inv-step {P} {S} i ss step-eq pi) prev′

------------------------------------------------------------------------
-- Top-level forward faithfulness, commitment-free case.
--
-- For a single-assignment well-formed source with no communications
-- commitment, a successful preprocess run yields a witness that
-- satisfies the synthesized circuit.  (The commitment case adds
-- `comm-fwd`; the single-assignment hypothesis `WF-run` is the producer
-- obligation, to be discharged from a static check later.)
------------------------------------------------------------------------

-- Every init state's public-input vector is the concrete preamble
-- (`binding ∷ []` with no commitment, `binding ∷ c ∷ []` with one), so
-- its head is the binding input and its length is the preamble PI count.
-- One inversion of `init` yields both; `init-pis-cons`/`init-pi0`/
-- `init-pis-len` read them off.
init-pis-shape : ∀ {S P st0} → init S P ≡ just st0
  → (∃ λ rest → State.pis st0 ≡ ProofPreimage.binding-input P ∷ rest)
  × (length (State.pis st0)
       ≡ preamble-pi-count (IrSource.do-communications-commitment S))
init-pis-shape {S} {P} ieq
  with decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
... | nothing = case ieq of λ ()
... | just m with IrSource.do-communications-commitment S
...   | false = ([] , sym (cong State.pis (just-injective ieq)))
              , sym (cong (λ z → length (State.pis z)) (just-injective ieq))
...   | true with ProofPreimage.comm-commitment P
...     | just (c , _) =
            (c ∷ [] , sym (cong State.pis (just-injective ieq)))
          , sym (cong (λ z → length (State.pis z)) (just-injective ieq))
...     | nothing = case ieq of λ ()

init-pis-cons : ∀ {S P st0} → init S P ≡ just st0
  → ∃ λ rest → State.pis st0 ≡ ProofPreimage.binding-input P ∷ rest
init-pis-cons {S} {P} {st0} ieq = proj₁ (init-pis-shape {S} {P} {st0} ieq)

init-pi0 : ∀ {S P st0} → init S P ≡ just st0
  → pi-lookup (State.pis st0) 0 ≡ just (ProofPreimage.binding-input P)
init-pi0 {S} {P} {st0} ieq with init-pis-cons {S} {P} {st0} ieq
... | (rest , p) rewrite p = refl

init-pis-len : ∀ {S P st0} → init S P ≡ just st0
  → length (State.pis st0)
      ≡ preamble-pi-count (IrSource.do-communications-commitment S)
init-pis-len {S} {P} {st0} ieq = proj₂ (init-pis-shape {S} {P} {st0} ieq)

-- Package the commitment-free circuit's constraint and PI-length facts
-- (stated for the singleton-preamble fold) into `satisfies (synth S)`.
-- The `do-comm = false` case-split makes `synth S` reduce; the inputs
-- carry no `do-comm S`, so the split entangles nothing.
satisfies-synth-false : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ false
  → satisfies-constraints
      (SynthState.constraints
        (synth-instrs (IrSource.instructions S)
          (mk-synth (pi-binding 0 ∷ []) 1 []))) w
  → length (CircuitWitness.pis w)
      ≡ SynthState.next-pi
          (synth-instrs (IrSource.instructions S)
            (mk-synth (pi-binding 0 ∷ []) 1 []))
  → satisfies (synth S) w
satisfies-synth-false {S} hc cok pil
  with IrSource.do-communications-commitment S | hc
... | false | refl =
  record { pi-length = pil ; rand-shape = tt ; constraint-ok = cok }

forward-no-comm : ∀ {S P s st0}
  → IrSource.do-communications-commitment S ≡ false
  → init S P ≡ just st0
  → run P S st0 (IrSource.instructions S) ≡ just s
  → WF-run P S (IrSource.instructions S) st0
  → satisfies (synth S) (witness-of P s)
forward-no-comm {S} {P} {s} {st0} hc≡false init-eq run-eq wf =
  satisfies-synth-false {S = S} hc≡false (proj₁ result) (sym (proj₂ result))
  where
  ss₀ : SynthState
  ss₀ = mk-synth (pi-binding 0 ∷ []) 1 []

  pi-inv₀ : SynthState.next-pi ss₀ ≡ length (State.pis st0)
  pi-inv₀ = sym (trans (init-pis-len {S} {P} {st0} init-eq)
                       (cong preamble-pi-count hc≡false))

  ext : (State.mem st0 ⊑ State.mem s) × (State.pis st0 ≼ State.pis s)
  ext = run-extends {P} {S} {s} (IrSource.instructions S) st0 run-eq wf

  prev₀ : satisfies-constraints (SynthState.constraints ss₀) (witness-of P s)
  prev₀ = (ProofPreimage.binding-input P
          , pi-lookup-mono (proj₂ ext) (init-pi0 {S} {P} {st0} init-eq))
        , tt

  result : satisfies-constraints
             (SynthState.constraints
               (synth-instrs (IrSource.instructions S) ss₀))
             (witness-of P s)
         × (SynthState.next-pi
               (synth-instrs (IrSource.instructions S) ss₀)
             ≡ length (State.pis s))
  result = fwd-go (IrSource.instructions S) st0 ss₀ run-eq wf pi-inv₀ prev₀

------------------------------------------------------------------------
-- Commitment preimage round-trip and assembly.
--
-- The communications commitment (`comm`) constraint asks that
-- π[1] = transient-commit (encode(inputs) ‖ encode(outputs)) r, where the
-- in-circuit `resolve-encode` reconstructs `encode(inputs)` from the named
-- input cells and `encode(outputs)` from the terminal `Output` operands.
-- The off-circuit TC2 check (which `preprocess` enforces) commits to the
-- raw input stream `ProofPreimage.inputs P` and `concatMap encode outs`.
-- The two preimages coincide: the named input cells re-encode to the raw
-- input stream (`decode-reencode`, an inverse of `decode-inputs`), and the
-- terminal `Output` operands resolve to exactly the collected `outs`
-- (`out-go`, a run/synth induction).
------------------------------------------------------------------------

open import Data.List.Properties using (take++drop≡id; concatMap-++)
open import Data.List using (concatMap)

-- A single typed chunk that `decode` accepts re-encodes to itself.  The
-- only successful `(type, chunk)` shapes are the four supported types at
-- their exact widths; every other shape sends `decode` to `nothing`.
decode-encode-chunk : ∀ t chunk {v}
  → decode t chunk ≡ just v → encodeᵉ v ≡ chunk
decode-encode-chunk native (x ∷ []) ieq with refl ← ieq = refl
decode-encode-chunk jubjub-scalar (f ∷ []) {v} ieq
  with jubjubScalarFromFr f in e
... | just s with refl ← ieq =
        cong (_∷ []) (jubjubScalar-sound {f} {s} e)
decode-encode-chunk bytes32 (lo ∷ hi ∷ []) {v} ieq
  with low-high→bytes32 lo hi in e
... | just b with refl ← ieq
        rewrite bytes32-sound {lo} {hi} {b} e = refl
decode-encode-chunk jubjub-point (x ∷ y ∷ []) {v} ieq
  with fromCoordsJ x y in e
... | just p with refl ← ieq
        rewrite fromCoordsJ-coordsJ {x} {y} {p} e = refl
decode-encode-chunk secp-base (l ∷ h ∷ []) {v} ieq
  with limbs→secpBase l h in e
... | just x with refl ← ieq
        rewrite secpBase-sound {l} {h} {x} e = refl
decode-encode-chunk secp-scalar (l ∷ h ∷ []) {v} ieq
  with limbs→secpScalar l h in e
... | just s with refl ← ieq
        rewrite secpScalar-sound {l} {h} {s} e = refl
decode-encode-chunk secp-point (a ∷ b ∷ c ∷ d ∷ e ∷ []) {v} ieq
  with limbs→secpPoint a b c d e in pr
... | just p with refl ← ieq
        rewrite secpPoint-sound {a} {b} {c} {d} {e} {p} pr = refl
decode-encode-chunk native [] ieq = case ieq of λ ()
decode-encode-chunk native (_ ∷ _ ∷ _) ieq = case ieq of λ ()
decode-encode-chunk bytes32 [] ieq = case ieq of λ ()
decode-encode-chunk bytes32 (_ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk bytes32 (_ ∷ _ ∷ _ ∷ _) ieq = case ieq of λ ()
decode-encode-chunk jubjub-point [] ieq = case ieq of λ ()
decode-encode-chunk jubjub-point (_ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk jubjub-point (_ ∷ _ ∷ _ ∷ _) ieq = case ieq of λ ()
decode-encode-chunk jubjub-scalar [] ieq = case ieq of λ ()
decode-encode-chunk jubjub-scalar (_ ∷ _ ∷ _) ieq = case ieq of λ ()
decode-encode-chunk secp-base [] ieq = case ieq of λ ()
decode-encode-chunk secp-base (_ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk secp-base (_ ∷ _ ∷ _ ∷ _) ieq = case ieq of λ ()
decode-encode-chunk secp-scalar [] ieq = case ieq of λ ()
decode-encode-chunk secp-scalar (_ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk secp-scalar (_ ∷ _ ∷ _ ∷ _) ieq = case ieq of λ ()
decode-encode-chunk secp-point [] ieq = case ieq of λ ()
decode-encode-chunk secp-point (_ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk secp-point (_ ∷ _ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk secp-point (_ ∷ _ ∷ _ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk secp-point (_ ∷ _ ∷ _ ∷ _ ∷ []) ieq = case ieq of λ ()
decode-encode-chunk secp-point (_ ∷ _ ∷ _ ∷ _ ∷ _ ∷ _ ∷ _) ieq =
  case ieq of λ ()

-- A name absent from the declared tail is unbound in the memory the tail
-- decodes to: `decode-inputs` only ever binds the names it processes.
decode-inputs-fresh : ∀ (nm : Identifier) tis raw {m'}
  → decode-inputs tis raw ≡ just m'
  → NotIn nm (map TypedIdentifier.name tis)
  → m' nm ≡ nothing
decode-inputs-fresh nm []         []      refl nin = refl
decode-inputs-fresh nm []         (_ ∷ _) ieq  nin = case ieq of λ ()
decode-inputs-fresh nm (ti ∷ tis) raw ieq (nm≢ , nin)
  with decode (TypedIdentifier.val-t ti)
              (take (encoded-len (TypedIdentifier.val-t ti)) raw)
... | just v
  with decode-inputs tis (drop (encoded-len (TypedIdentifier.val-t ti)) raw)
         in erest
...   | just m' with refl ← ieq =
        trans (ins-other v m' nm≢)
              (decode-inputs-fresh nm tis _ erest nin)

-- Re-encoding the decoded input memory reproduces the raw input stream.
-- Inductive on the declared inputs; `decode-inputs` consumes
-- `encoded-len t` elements per input, so `take ‖ drop` rebuilds `raw`.
-- The sub-map hypothesis `m ⊑ assign w` lets each named cell resolve to
-- its decoded value; `NoDup` of the input names lets that hypothesis
-- descend to the tail (the head name is unbound in the tail's map).
decode-reencode : ∀ {w} tis raw m
  → decode-inputs tis raw ≡ just m
  → NoDup (map TypedIdentifier.name tis)
  → (∀ {id v} → m id ≡ just v → CircuitWitness.assign w id ≡ just v)
  → resolve-encode w (input-operands tis) ≡ just raw
decode-reencode []         []        m ieq nd m⊑ = refl
decode-reencode []         (_ ∷ _)   m ieq nd m⊑ = case ieq of λ ()
decode-reencode {w} (ti ∷ tis) raw m ieq (nin , ndup) m⊑
  with decode (TypedIdentifier.val-t ti)
              (take (encoded-len (TypedIdentifier.val-t ti)) raw) in ed
... | just v
  with decode-inputs tis (drop (encoded-len (TypedIdentifier.val-t ti)) raw)
         in erest
...   | just m' with refl ← ieq =
        let n  = encoded-len (TypedIdentifier.val-t ti)
            nm = TypedIdentifier.name ti
            -- the head cell resolves to its decoded value
            head-≡ : CircuitWitness.assign w nm ≡ just v
            head-≡ = m⊑ (ins-here nm v m')
            -- the tail map survives: `nm` is unbound in `m'`, so every
            -- binding of `m'` is a binding of `ins nm v m'`.
            nm-fresh : m' nm ≡ nothing
            nm-fresh = decode-inputs-fresh nm tis _ erest nin
            m'⊑ : ∀ {id u} → m' id ≡ just u
                → CircuitWitness.assign w id ≡ just u
            m'⊑ {id} {u} p = m⊑ (ins-⊑ {nm} {v} {m'} nm-fresh {id} p)
            tail-≡ : resolve-encode w (input-operands tis)
                       ≡ just (drop n raw)
            tail-≡ = decode-reencode tis (drop n raw) m' erest ndup m'⊑
        in reencode-step {w} {nm} {v} tis raw n
             head-≡ tail-≡
             (decode-encode-chunk (TypedIdentifier.val-t ti) (take n raw) ed)
  where
  -- `resolve-encode w (var nm ∷ ops)` resolves the head cell to `v`,
  -- flattens it to `encodeᵉ v = take n raw`, and prepends it to the tail
  -- (the rest of `raw`); `take ‖ drop` rebuilds `raw`.
  reencode-step : ∀ {w} {nm : Identifier} {v} tis raw n
    → CircuitWitness.assign w nm ≡ just v
    → resolve-encode w (input-operands tis) ≡ just (drop n raw)
    → encodeᵉ v ≡ take n raw
    → resolve-encode w (var nm ∷ input-operands tis) ≡ just raw
  reencode-step {w} {nm} {v} tis raw n h t c
    rewrite h | t | c = cong just (take++drop≡id n raw)

-- `resolve-encode` distributes over operand-list concatenation.
resolve-encode-++ : ∀ w xs ys {rx ry}
  → resolve-encode w xs ≡ just rx
  → resolve-encode w ys ≡ just ry
  → resolve-encode w (xs ++ ys) ≡ just (rx ++ ry)
resolve-encode-++ w []         ys {rx} {ry} ex ey with refl ← ex = ey
resolve-encode-++ w (op ∷ ops) ys ex ey
  with resolveᶜ w op in eqop
... | just v
  with resolve-encode w ops in eqrest
...   | just rest with refl ← ex
        rewrite resolve-encode-++ w ops ys eqrest ey =
          cong just (sym (++-assoc (encodeᵉ v) rest _))

-- The terminal `Output` operands resolve (at the final witness) to the
-- raw encodings of exactly the values `collectOutputs` recorded into
-- `outs`: `collectOutputs` resolves each operand at `m` and only
-- type-checks it (it does not alter the value), and those resolutions are
-- preserved to `mem s` by the rest of the run (`m ⊑ mem s`).
re-cons : ∀ {w} op {v} ops vrest
  → resolveᶜ w op ≡ just v
  → resolve-encode w ops ≡ just (concatMap encodeᵉ vrest)
  → resolve-encode w (op ∷ ops) ≡ just (concatMap encodeᵉ (v ∷ vrest))
re-cons op ops vrest h t rewrite h | t = refl

collect-resolve-encode : ∀ {P s} m ts ops {vs}
  → collectOutputs m ts ops ≡ just vs
  → m ⊑ State.mem s
  → resolve-encode (witness-of P s) ops ≡ just (concatMap encodeᵉ vs)
collect-resolve-encode m []       []         refl sub = refl
collect-resolve-encode m []       (_ ∷ _)    ce   sub = case ce of λ ()
collect-resolve-encode m (_ ∷ _)  []         ce   sub = case ce of λ ()
collect-resolve-encode {P} {s} m (t ∷ ts) (op ∷ ops) ce sub
  with resolve m op in eqop
... | just v with typeof v ≟T t
...   | yes _ with collectOutputs m ts ops in eqrest
...     | just vrest with refl ← ce =
          re-cons {witness-of P s} op ops vrest
            (trans (resolve-agree P s op) (resolve-mono op sub eqop))
            (collect-resolve-encode {P} {s} m ts ops eqrest sub)

-- Every instruction except `circuit-output` leaves the output stream
-- untouched.  This is the `outs` projection of `step-pi-outs`; the synth
-- cursor is irrelevant to the output stream, so a dummy `mk-synth`
-- (whose `next-pi` matches `length (State.pis st)`, discharging the
-- cursor hypothesis by `refl`) suffices.
step-outs-≡ : ∀ {P S st st'} (i : Instruction)
  → ¬ (∃ λ vals → i ≡ circuit-output vals)
  → step P S st i ≡ just st' → State.outs st' ≡ State.outs st
step-outs-≡ {st = st} i ¬co e =
  proj₂ (step-pi-outs i (mk-synth [] (length (State.pis st)) []) e refl) ¬co

-- Synthesis of every instruction except `circuit-output` leaves the
-- recorded `Output` operands untouched (it only pushes constraints and/or
-- advances the PI cursor).  `circuit-output` is the sole producer of
-- output operands.
synth-oo-≡ : ∀ (i : Instruction) ss
  → ¬ (∃ λ vals → i ≡ circuit-output vals)
  → SynthState.output-ops (synth-instr i ss) ≡ SynthState.output-ops ss
synth-oo-≡ (encode input outputs) ss _ = refl
synth-oo-≡ (assert cond) ss _ = refl
synth-oo-≡ (cond-select bit a b output) ss _ = refl
synth-oo-≡ (constrain-bits val bits) ss _ = refl
synth-oo-≡ (constrain-eq a b) ss _ = refl
synth-oo-≡ (constrain-to-boolean val) ss _ = refl
synth-oo-≡ (copy val output) ss _ = refl
synth-oo-≡ (impact guard inputs) ss _ = refl
synth-oo-≡ (ec-mul a scalar output) ss _ = refl
synth-oo-≡ (ec-mul-generator scalar output) ss _ = refl
synth-oo-≡ (hash-to-curve inputs output) ss _ = refl
synth-oo-≡ (into-coordinates point (xo , yo)) ss _ = refl
synth-oo-≡ (from-coordinates (x , y) output) ss _ = refl
synth-oo-≡ (into-bytes32 input output) ss _ = refl
synth-oo-≡ (from-bytes32 bytes val-t output) ss _ = refl
synth-oo-≡ (reverse-bytes bytes output) ss _ = refl
synth-oo-≡ (bytes32-into-low-high bytes (lo , hi)) ss _ = refl
synth-oo-≡ (bytes32-from-low-high (lo , hi) output) ss _ = refl
synth-oo-≡ (div-mod-power-of-two val bits []) ss _ = refl
synth-oo-≡ (div-mod-power-of-two val bits (o ∷ [])) ss _ = refl
synth-oo-≡ (div-mod-power-of-two val bits (q ∷ r ∷ [])) ss _ = refl
synth-oo-≡ (div-mod-power-of-two val bits (q ∷ r ∷ o ∷ outs)) ss _ = refl
synth-oo-≡ (reconstitute-field divisor modulus bits output) ss _ = refl
synth-oo-≡ (transient-hash inputs output) ss _ = refl
synth-oo-≡ (persistent-hash alignment inputs output) ss _ = refl
synth-oo-≡ (keccak256 alignment inputs output) ss _ = refl
synth-oo-≡ (test-eq a b output) ss _ = refl
synth-oo-≡ (add a b output) ss _ = refl
synth-oo-≡ (mul a b output) ss _ = refl
synth-oo-≡ (neg a output) ss _ = refl
synth-oo-≡ (inv a output) ss _ = refl
synth-oo-≡ (not a output) ss _ = refl
synth-oo-≡ (less-than a b bits output) ss _ = refl
synth-oo-≡ (jubjub-scalar-from-native a output) ss _ = refl
synth-oo-≡ (public-input guard val-t output) ss _ = refl
synth-oo-≡ (private-input guard val-t output) ss _ = refl
synth-oo-≡ (circuit-output vals) ss ¬co = case ¬co (vals , refl) of λ ()

------------------------------------------------------------------------
-- Output coupling: the synthesised `Output` operands resolve, at the
-- run's final witness, to the raw encodings of the values the run
-- collected into `State.outs`.  Threads the seed
--   resolve-encode w (output-ops ss) ≡ just (concatMap encode (outs st))
-- across the run.  Every instruction but `circuit-output` leaves both
-- sides untouched (`synth-oo-≡`, `step-outs-≡`); `circuit-output vals`
-- grows `output-ops` by `vals` and `outs` by `collectOutputs … vals`,
-- and the two grow in lock-step (`collect-resolve-encode`,
-- `resolve-encode-++`, `concatMap-++`).
------------------------------------------------------------------------

-- Transport the output-coupling seed across one non-`circuit-output`
-- step: both the synthesised operands (`synth-oo-≡`) and the collected
-- outputs (`step-outs-≡`) are unchanged, so the seed carries over to the
-- state and synth-state after the step.
keep-seed : ∀ {P S s} i st st-mid ss
  → ¬ (∃ λ vals → i ≡ circuit-output vals)
  → step P S st i ≡ just st-mid
  → resolve-encode (witness-of P s) (SynthState.output-ops ss)
      ≡ just (concatMap encodeᵉ (State.outs st))
  → resolve-encode (witness-of P s)
      (SynthState.output-ops (synth-instr i ss))
      ≡ just (concatMap encodeᵉ (State.outs st-mid))
keep-seed {P} {S} {s} i st st-mid ss ¬co step-eq seed =
  trans (cong (resolve-encode (witness-of P s)) (synth-oo-≡ i ss ¬co))
    (trans seed
      (cong (λ os → just (concatMap encodeᵉ os))
            (sym (step-outs-≡ i ¬co step-eq))))

-- Decide whether an instruction is a `circuit-output` (the only producer
-- of `Output` operands / `outs` entries).  Enumerated per constructor so
-- the mismatch is structural in each negative case.
circuit-output? : ∀ i → Dec (∃ λ vals → i ≡ circuit-output vals)
circuit-output? (encode _ _)                = no λ { (_ , ()) }
circuit-output? (assert _)                  = no λ { (_ , ()) }
circuit-output? (cond-select _ _ _ _)       = no λ { (_ , ()) }
circuit-output? (constrain-bits _ _)        = no λ { (_ , ()) }
circuit-output? (constrain-eq _ _)          = no λ { (_ , ()) }
circuit-output? (constrain-to-boolean _)    = no λ { (_ , ()) }
circuit-output? (copy _ _)                  = no λ { (_ , ()) }
circuit-output? (impact _ _)                = no λ { (_ , ()) }
circuit-output? (ec-mul _ _ _)              = no λ { (_ , ()) }
circuit-output? (ec-mul-generator _ _)      = no λ { (_ , ()) }
circuit-output? (hash-to-curve _ _)         = no λ { (_ , ()) }
circuit-output? (into-coordinates _ _)      = no λ { (_ , ()) }
circuit-output? (from-coordinates _ _)      = no λ { (_ , ()) }
circuit-output? (into-bytes32 _ _)          = no λ { (_ , ()) }
circuit-output? (from-bytes32 _ _ _)        = no λ { (_ , ()) }
circuit-output? (reverse-bytes _ _)         = no λ { (_ , ()) }
circuit-output? (bytes32-into-low-high _ _) = no λ { (_ , ()) }
circuit-output? (bytes32-from-low-high _ _) = no λ { (_ , ()) }
circuit-output? (div-mod-power-of-two _ _ _) = no λ { (_ , ()) }
circuit-output? (reconstitute-field _ _ _ _) = no λ { (_ , ()) }
circuit-output? (transient-hash _ _)        = no λ { (_ , ()) }
circuit-output? (persistent-hash _ _ _)     = no λ { (_ , ()) }
circuit-output? (keccak256 _ _ _)           = no λ { (_ , ()) }
circuit-output? (test-eq _ _ _)             = no λ { (_ , ()) }
circuit-output? (add _ _ _)                 = no λ { (_ , ()) }
circuit-output? (mul _ _ _)                 = no λ { (_ , ()) }
circuit-output? (neg _ _)                   = no λ { (_ , ()) }
circuit-output? (inv _ _)                   = no λ { (_ , ()) }
circuit-output? (not _ _)                   = no λ { (_ , ()) }
circuit-output? (less-than _ _ _ _)         = no λ { (_ , ()) }
circuit-output? (jubjub-scalar-from-native _ _) = no λ { (_ , ()) }
circuit-output? (public-input _ _ _)        = no λ { (_ , ()) }
circuit-output? (private-input _ _ _)       = no λ { (_ , ()) }
circuit-output? (circuit-output vals)       = yes (vals , refl)

out-go : ∀ {P S s} is st ss
  → run P S st is ≡ just s → WF-run P S is st
  → resolve-encode (witness-of P s) (SynthState.output-ops ss)
      ≡ just (concatMap encodeᵉ (State.outs st))
  → resolve-encode (witness-of P s) (SynthState.output-ops (synth-instrs is ss))
      ≡ just (concatMap encodeᵉ (State.outs s))
out-go {P} {S} {s} [] st ss run-eq wf seed
  rewrite just-injective run-eq = seed
out-go {P} {S} {s} (i ∷ is) st ss run-eq (_ , wf) seed
  with circuit-output? i
... | no ¬co =
      let (sm , se , rr) = run-inv i is st run-eq in
      out-go is sm (synth-instr i ss) rr (wf se)
        (keep-seed {P} {S} {s} i st sm ss ¬co se seed)
... | yes (vals , refl) =
      let (st-mid , step-eq , run-rest) =
            run-inv (circuit-output vals) is st run-eq
      in out-go is st-mid (synth-instr (circuit-output vals) ss)
           run-rest (wf step-eq)
           (out-step-output st st-mid step-eq run-rest (wf step-eq) seed)
  where
  -- The `circuit-output` step appends `collectOutputs (mem st) (outputs
  -- S) vals` to `outs`; synth appends `vals` to `output-ops`.  The
  -- appended operands resolve (at the final witness) to the encodings of
  -- the appended values, so the seed grows consistently.
  out-step-output : ∀ st st-mid
    → step P S st (circuit-output vals) ≡ just st-mid
    → run P S st-mid is ≡ just s → WF-run P S is st-mid
    → resolve-encode (witness-of P s) (SynthState.output-ops ss)
        ≡ just (concatMap encodeᵉ (State.outs st))
    → resolve-encode (witness-of P s)
        (SynthState.output-ops ss ++ vals)
        ≡ just (concatMap encodeᵉ (State.outs st-mid))
  out-step-output st st-mid step-eq run-rest wf-rest seed
    with collectOutputs (State.mem st) (IrSource.outputs S) vals in eqc
  ... | just vs with refl ← step-eq =
        let (m⊑ , _) = run-extends {P} {S} {s} is st-mid run-rest wf-rest
            vals-≡ : resolve-encode (witness-of P s) vals
                       ≡ just (concatMap encodeᵉ vs)
            vals-≡ = collect-resolve-encode {P} {s}
                       (State.mem st) (IrSource.outputs S) vals eqc m⊑
        in trans
             (resolve-encode-++ (witness-of P s)
               (SynthState.output-ops ss) vals seed vals-≡)
             (cong just (sym (concatMap-++ encodeᵉ (State.outs st) vs)))

------------------------------------------------------------------------
-- The communications-commitment case.
--
-- When `do-communications-commitment S = true`, `init` seeds the PI
-- vector with `binding ∷ c ∷ []` (where `comm-commitment P = just (c ,
-- r)`), and `preprocess` enforces the terminal check TC2:
--   c ≡ transient-commit (inputs P ++ concatMap encode (outs s)) r.
-- The synthesised `comm` constraint asks exactly this, with its preimage
-- the in-circuit re-encodings: `decode-reencode` reproduces `inputs P`
-- from the named input cells, `out-go` reproduces `concatMap encode (outs
-- s)` from the terminal `Output` operands, and π[1] = c carries the
-- commitment.
------------------------------------------------------------------------

-- The second PI entry of a comm-commitment init state is the commitment.
init-pi1 : ∀ {S P st0 c r}
  → IrSource.do-communications-commitment S ≡ true
  → ProofPreimage.comm-commitment P ≡ just (c , r)
  → init S P ≡ just st0
  → pi-lookup (State.pis st0) 1 ≡ just c
init-pi1 {S} {P} {st0} {c} {r} hc cc ieq
  with decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
... | nothing = case ieq of λ ()
init-pi1 {S} {P} {st0} {c} {r} hc cc ieq
  | just m
  with IrSource.do-communications-commitment S | hc
...   | true | refl with ProofPreimage.comm-commitment P | cc
...     | just (c , r) | refl rewrite sym (just-injective ieq) = refl

open import zkir-v3.Semantics ⋯ using (preprocess)
open import Data.Nat using () renaming (_≟_ to _≟ℕ_)

-- Invert `preprocess`: it begins with the very `init` it was given and
-- runs all instructions to the state it returns; the terminal checks
-- (TC1, TC2) only gate the result.  A single walk of the guard tower
-- yields both the run and TC2 — under a communications commitment, the
-- recorded value equals the transient commitment of the inputs and the
-- encoded outputs.
preprocess-inv : ∀ {S P s st0}
  → init S P ≡ just st0
  → preprocess S P ≡ just s
  → run P S st0 (IrSource.instructions S) ≡ just s
  × (∀ {c r} → IrSource.do-communications-commitment S ≡ true
       → ProofPreimage.comm-commitment P ≡ just (c , r)
       → c ≡ transient-commit
               (ProofPreimage.inputs P ++ concatMap encodeᵉ (State.outs s)) r)
preprocess-inv {S} {P} {s} {st0} ieq peq
  with init S P | ieq
... | just st0' | refl
  with run P S st0' (IrSource.instructions S)
...   | just stf
  with State.pti-idx stf ≟ℕ length (ProofPreimage.pub-transcript-inputs P)
...     | yes _
  with State.pto-rem stf
...       | (_ ∷ _) = case peq of λ ()
...       | []
  with State.priv-rem stf
...         | (_ ∷ _) = case peq of λ ()
...         | []
  with IrSource.do-communications-commitment S
...           | false with refl ← peq =
                refl , λ ht _ → case ht of λ ()
preprocess-inv {S} {P} {s} {st0} ieq peq
  | just st0' | refl | just stf | yes _ | [] | [] | true
  with ProofPreimage.comm-commitment P
...             | just (cₚ , rₚ)
  with cₚ ≟ᶠ transient-commit
              (ProofPreimage.inputs P ++ concatMap encodeᵉ (State.outs stf)) rₚ
...               | yes c≡ with refl ← peq =
                    refl
                  , λ _ cc′ → case just-injective cc′ of
                                λ where refl → c≡

-- The communications-commitment constraint holds at the run's final
-- witness.  Its preimage components are reconstructed in-circuit:
-- `decode-reencode` rebuilds the raw inputs from the named input cells,
-- `out-go` rebuilds the encoded outputs from the terminal `Output`
-- operands (seeded by the empty initial output stream), the commitment
-- randomness is `r`, and π[1] carries `c`; the final equation is TC2.
comm-fwd : ∀ {S P s st0 c r}
  → IrSource.do-communications-commitment S ≡ true
  → ProofPreimage.comm-commitment P ≡ just (c , r)
  → init S P ≡ just st0
  → preprocess S P ≡ just s
  → WF-run P S (IrSource.instructions S) st0
  → NoDup (map TypedIdentifier.name (IrSource.inputs S))
  → holds (witness-of P s)
      (comm (input-operands (IrSource.inputs S))
        (SynthState.output-ops
          (synth-instrs (IrSource.instructions S)
            (mk-synth (pi-binding 0 ∷ []) 2 []))))
comm-fwd {S} {P} {s} {st0} {c} {r} hc cc ieq peq wf nd =
    ProofPreimage.inputs P
  , concatMap encodeᵉ (State.outs s)
  , r
  , c
  , ivs-≡
  , ovs-≡
  , rand-≡
  , pi1-≡
  , proj₂ pre hc cc
  where
  pre = preprocess-inv {S} {P} {s} {st0} ieq peq

  run-eq : run P S st0 (IrSource.instructions S) ≡ just s
  run-eq = proj₁ pre

  m⊑ : State.mem st0 ⊑ State.mem s
  m⊑ = proj₁ (run-extends {P} {S} {s} (IrSource.instructions S) st0 run-eq wf)

  pis≼ : State.pis st0 ≼ State.pis s
  pis≼ = proj₂ (run-extends {P} {S} {s} (IrSource.instructions S) st0 run-eq wf)

  -- inputs: re-encode the decoded input cells
  ivs-≡ : resolve-encode (witness-of P s)
            (input-operands (IrSource.inputs S))
            ≡ just (ProofPreimage.inputs P)
  ivs-≡ = decode-reencode {witness-of P s} (IrSource.inputs S)
            (ProofPreimage.inputs P) (State.mem st0)
            (init-decode {S} {P} {st0} ieq) nd m⊑

  -- outputs: the terminal `Output` operands resolve to the encoded outs
  ovs-≡ : resolve-encode (witness-of P s)
            (SynthState.output-ops
              (synth-instrs (IrSource.instructions S)
                (mk-synth (pi-binding 0 ∷ []) 2 [])))
            ≡ just (concatMap encodeᵉ (State.outs s))
  ovs-≡ = out-go {P} {S} {s} (IrSource.instructions S) st0
            (mk-synth (pi-binding 0 ∷ []) 2 []) run-eq wf
            seed₀
    where
    -- the initial output stream is empty, and the seeded `output-ops` is
    -- empty, so both sides start at `just []`.
    seed₀ : resolve-encode (witness-of P s) []
              ≡ just (concatMap encodeᵉ (State.outs st0))
    seed₀ rewrite init-outs {S} {P} {st0} ieq = refl

  rand-≡ : CircuitWitness.comm-rand (witness-of P s) ≡ just r
  rand-≡ rewrite cc = refl

  pi1-≡ : pi-lookup (CircuitWitness.pis (witness-of P s)) 1 ≡ just c
  pi1-≡ = pi-lookup-mono pis≼ (init-pi1 {S} {P} {st0} {c} {r} hc cc ieq)

------------------------------------------------------------------------
-- Top-level forward faithfulness.
--
-- A successful preprocess run, under single-assignment well-formedness
-- and distinctness of the declared input names, yields a witness that
-- satisfies the synthesised circuit.  Splitting on the comm-commitment
-- flag selects either the commitment-free assembly (`forward-no-comm`,
-- reusing `fwd-go`) or the commitment case (appending `comm-fwd` to the
-- per-instruction constraints via the `∷ʳ` that `synth` emits).
------------------------------------------------------------------------

-- Package the comm-commitment circuit's facts (stated for the two-entry
-- preamble fold) into `satisfies (synth S)`.  The `do-comm = true`
-- case-split makes `synth S` reduce; its arguments carry no `do-comm S`,
-- so the split entangles nothing.
satisfies-synth-true : ∀ {S P s c r w}
  → IrSource.do-communications-commitment S ≡ true
  → ProofPreimage.comm-commitment P ≡ just (c , r)
  → w ≡ witness-of P s
  → satisfies-constraints
      (SynthState.constraints
        (synth-instrs (IrSource.instructions S)
          (mk-synth (pi-binding 0 ∷ []) 2 []))) w
  → holds w
      (comm (input-operands (IrSource.inputs S))
        (SynthState.output-ops
          (synth-instrs (IrSource.instructions S)
            (mk-synth (pi-binding 0 ∷ []) 2 []))))
  → length (CircuitWitness.pis w)
      ≡ SynthState.next-pi
          (synth-instrs (IrSource.instructions S)
            (mk-synth (pi-binding 0 ∷ []) 2 []))
  → satisfies (synth S) w
satisfies-synth-true {S} {P} {s} {c} {r} {w} hc cc w≡ cok comm-ok pil
  with IrSource.do-communications-commitment S | hc
... | true | refl =
  record
    { pi-length = pil
    ; rand-shape = rand-shape′
    ; constraint-ok =
        sat-++ (SynthState.constraints
                 (synth-instrs (IrSource.instructions S)
                   (mk-synth (pi-binding 0 ∷ []) 2 [])))
               (comm (input-operands (IrSource.inputs S))
                 (SynthState.output-ops
                   (synth-instrs (IrSource.instructions S)
                     (mk-synth (pi-binding 0 ∷ []) 2 []))) ∷ [])
               w cok (comm-ok , tt)
    }
  where
  -- `comm-rand w = just r`, so the comm-commitment randomness shape holds.
  rand-shape′ : Maybe-shape true (CircuitWitness.comm-rand w)
  rand-shape′ rewrite w≡ | cc = tt

forward : ∀ {S P s st0}
  → init S P ≡ just st0
  → preprocess S P ≡ just s
  → WF-run P S (IrSource.instructions S) st0
  → NoDup (map TypedIdentifier.name (IrSource.inputs S))
  → satisfies (synth S) (witness-of P s)
forward {S} {P} {s} {st0} ieq peq wf nd = aux (do-comm S) refl
  where
  do-comm = IrSource.do-communications-commitment
  run-eq = proj₁ (preprocess-inv {S} {P} {s} {st0} ieq peq)

  -- The comm-commitment branch.  Carried out under `cc`, where the
  -- commitment is present; `satisfies-synth-true` reassembles
  -- `satisfies (synth S)` (it re-splits on the flag itself).
  with-comm : ∀ {c r}
    → do-comm S ≡ true
    → ProofPreimage.comm-commitment P ≡ just (c , r)
    → satisfies (synth S) (witness-of P s)
  with-comm {c} {r} hc cc =
    satisfies-synth-true {S} {P} {s} {c} {r} {witness-of P s}
      hc cc refl (proj₁ result)
      (comm-fwd {S} {P} {s} {st0} {c} {r} hc cc ieq peq wf nd)
      (sym (proj₂ result))
    where
    ss₀ : SynthState
    ss₀ = mk-synth (pi-binding 0 ∷ []) 2 []

    pi-inv₀ : SynthState.next-pi ss₀ ≡ length (State.pis st0)
    pi-inv₀ = sym (trans (init-pis-len {S} {P} {st0} ieq)
                         (cong preamble-pi-count hc))

    prev₀ : satisfies-constraints (SynthState.constraints ss₀) (witness-of P s)
    prev₀ = (ProofPreimage.binding-input P
            , pi-lookup-mono
                (proj₂ (run-extends {P} {S} {s} (IrSource.instructions S)
                          st0 run-eq wf))
                (init-pi0 {S} {P} {st0} ieq))
          , tt

    result : satisfies-constraints
               (SynthState.constraints (synth-instrs (IrSource.instructions S) ss₀))
               (witness-of P s)
           × (SynthState.next-pi (synth-instrs (IrSource.instructions S) ss₀)
               ≡ length (State.pis s))
    result = fwd-go (IrSource.instructions S) st0 ss₀ run-eq wf pi-inv₀ prev₀

  -- With `do-comm S = true` but no commitment, `init` would have failed.
  init-no-comm : do-comm S ≡ true
    → ProofPreimage.comm-commitment P ≡ nothing → init S P ≡ nothing
  init-no-comm hc cc
    with decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
  ... | nothing = refl
  ... | just m
    with IrSource.do-communications-commitment S | hc
  ...   | true | refl
    with ProofPreimage.comm-commitment P | cc
  ...     | nothing | refl = refl

  -- Dispatch on the commitment, keeping the goal's `synth S` /
  -- `witness-of P s` abstract (the fresh `mc` is the scrutinee, not
  -- `comm-commitment P`), so the branch results unify with the goal.
  aux-cc : do-comm S ≡ true → (mc : Maybe (Fr × Fr))
    → ProofPreimage.comm-commitment P ≡ mc
    → satisfies (synth S) (witness-of P s)
  aux-cc hc (just (c , r)) cc = with-comm {c} {r} hc cc
  aux-cc hc nothing        cc = case trans (sym (init-no-comm hc cc)) ieq of λ ()

  aux : (b : Bool) → do-comm S ≡ b → satisfies (synth S) (witness-of P s)
  aux false e = forward-no-comm {S} {P} {s} {st0} e ieq run-eq wf
  aux true  e = aux-cc e (ProofPreimage.comm-commitment P) refl
