{-# OPTIONS --safe #-}

------------------------------------------------------------------------
-- Encoding facts of the zkir-v2 formalization
--
-- Consequences of the `Assumptions` encoding / valuation laws: the
-- operational characterization of `to-bool`, the
-- `fits-from-le-bits-*` truncation facts, and the bit-arithmetic
-- identities discharged against the valuation laws
-- (`bits-decomp-split`, `reconstitute-no-overflow`, `bits-lt-pad`, …).
--
-- Parameterized by an `Assumptions` value; consumers
-- `open import zkir-v2.Encoding ⋯`.
------------------------------------------------------------------------

open import zkir-v2.Assumptions

module zkir-v2.Encoding (⋯ : Assumptions) (open Assumptions ⋯) where

open import Data.Bool  using (Bool; true; false; if_then_else_)
open import Data.Empty using (⊥-elim)
open import Data.List  using (List; []; _∷_; _++_; take; drop; reverse; length)
open import Data.Maybe using (Maybe; nothing; just)
open import Data.Nat   using (ℕ; zero; suc; _+_; _∸_; _%_; _⊓_; _≤_; _<_; _*_; _^_; NonZero)
open import Data.Nat.Properties  using ( ≤-trans; ≤-<-trans; <-≤-trans; ≤-reflexive
                                        ; <⇒≤; ≮⇒≥; m≤n⇒m∸n≡0; m∸n≤m; m≤n⇒m⊓n≡m
                                        ; n<1⇒n≡0; *-zeroʳ; +-identityʳ; +-identityˡ
                                        ; +-comm; *-comm; m≤m*n; m≤n+m; m≤m+n; m^n≢0
                                        ; *-cancelʳ-≡; +-cancelˡ-≡; _<?_; <-cmp )
open import Data.Nat.DivMod      using (m%n≤m; m<n⇒m%n≡m; %-distribˡ-+; %-distribˡ-*
                                        ; m%n%n≡m%n; m*n%n≡0)
open import Data.Product using (_×_; _,_)
open import Data.List.Properties using (length-take; length-reverse)
open import Relation.Binary.Definitions using (tri<; tri≈; tri>)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; subst; subst₂; module ≡-Reasoning)
open import Relation.Nullary using (yes; no)
open ≡-Reasoning

open import zkir-v2.FieldProperties ⋯

----------------------------------------------------------------------
-- Operational characterization of `to-bool`.
--
-- `to-bool` is the nested `with` on `_≡ᶠ?_ = does ∘ _≟ᶠ_`, so each fact
-- is discharged by case analysis on `v ≟ᶠ 0ᶠ` / `v ≟ᶠ 1ᶠ`: in the
-- `to-bool-{true,false}` directions the scrutinees are carried into the
-- `with` alongside the hypothesis (so its type reduces with the branch),
-- and the off-branches are absurd `Maybe Bool` equalities.  `to-bool 1ᶠ`
-- additionally needs `1ᶠ≢0ᶠ` to rule out the `1ᶠ ≡ᶠ? 0ᶠ` branch.
----------------------------------------------------------------------

to-bool-true : ∀ {v} → to-bool v ≡ just true → v ≡ 1ᶠ
to-bool-true {v} eq with v ≟ᶠ 0ᶠ | eq
... | yes _ | ()
... | no  _ | eq₁ with v ≟ᶠ 1ᶠ | eq₁
...   | yes q | _  = q
...   | no  _ | ()

to-bool-false : ∀ {v} → to-bool v ≡ just false → v ≡ 0ᶠ
to-bool-false {v} eq with v ≟ᶠ 0ᶠ | eq
... | yes p | _  = p
... | no  _ | eq₁ with v ≟ᶠ 1ᶠ | eq₁
...   | yes _ | ()
...   | no  _ | ()

to-bool-of-0ᶠ : to-bool 0ᶠ ≡ just false
to-bool-of-0ᶠ with 0ᶠ ≟ᶠ 0ᶠ
... | yes _  = refl
... | no ¬p = ⊥-elim (¬p refl)

to-bool-of-1ᶠ : to-bool 1ᶠ ≡ just true
to-bool-of-1ᶠ with 1ᶠ ≟ᶠ 0ᶠ
... | yes p = ⊥-elim (1ᶠ≢0ᶠ p)
... | no  _ with 1ᶠ ≟ᶠ 1ᶠ
...   | yes _  = refl
...   | no ¬q = ⊥-elim (¬q refl)

----------------------------------------------------------------------
-- Derived encoding facts: `fits-from-le-bits-{take,drop}`.
--
-- Both follow from `from-le-bits-roundtrip` and the bit-value bounds in
-- `FieldProperties`: the canonical value of `from-le-bits (take n bs)` is
-- `bits-to-ℕ (take n bs) % FR-ORDER ≤ bits-to-ℕ (take n bs) < 2^n`, so
-- it fits in n bits — and likewise for `drop`.  Reduction mod the field
-- order can only shrink the value (`m%n≤m`), so no bound on FR-ORDER is
-- needed.
----------------------------------------------------------------------

fits-from-le-bits-take : ∀ bs n
  → Fits-in (from-le-bits (take n bs)) n
fits-from-le-bits-take bs n =
  subst (_< 2 ^ n) (sym (from-le-bits-roundtrip (take n bs)))
    (≤-<-trans (m%n≤m (bits-to-ℕ (take n bs)) FR-ORDER)
               (take-bound n bs))

fits-from-le-bits-drop : ∀ v n
  → Fits-in (from-le-bits (drop n (to-le-bits v))) (FR-BITS ∸ n)
fits-from-le-bits-drop v n =
  subst (_< 2 ^ (FR-BITS ∸ n))
        (sym (from-le-bits-roundtrip (drop n (to-le-bits v))))
    (≤-<-trans
      (m%n≤m (bits-to-ℕ (drop n (to-le-bits v))) FR-ORDER)
      (subst (λ L → bits-to-ℕ (drop n (to-le-bits v)) < 2 ^ (L ∸ n))
             (to-le-bits-length v)
             (drop-bound n (to-le-bits v))))

----------------------------------------------------------------------
-- Arithmetic bit-facts, discharged against the valuation laws.
----------------------------------------------------------------------

-- `valFr (pow2-fr n) ≡ 2^n mod FR-ORDER`, by induction using `valFr-+`
-- and `valFr-1` (`pow2-fr` doubles at each step).
valFr-pow2 : ∀ n → valFr (pow2-fr n) ≡ 2 ^ n % FR-ORDER
valFr-pow2 zero    = valFr-1
valFr-pow2 (suc k) =
  trans (valFr-+ (pow2-fr k) (pow2-fr k))
  (trans (cong (λ z → (z + z) % FR-ORDER) (valFr-pow2 k))
  (trans (sym (%-distribˡ-+ (2 ^ k) (2 ^ k) FR-ORDER))
         (cong (_% FR-ORDER) (sym (cong (2 ^ k +_) (+-identityʳ (2 ^ k)))))))

-- Splitting a field element at `bits` and rebuilding it as
-- (high · 2^bits + low) recovers it.  By valuation injectivity: both
-- sides have value `bits-to-ℕ (to-le-bits v) mod FR-ORDER`, which is
-- `valFr v` since `valFr v < FR-ORDER`.
bits-decomp-split : ∀ v bits
  → v ≡ (from-le-bits (drop bits (to-le-bits v)) *ᶠ pow2-fr bits)
      +ᶠ from-le-bits (take bits (to-le-bits v))
bits-decomp-split v bits = valFr-inj (sym (begin
  valFr ((D *ᶠ P) +ᶠ M)
    ≡⟨ valFr-+ (D *ᶠ P) M ⟩
  (valFr (D *ᶠ P) + valFr M) % FR-ORDER
    ≡⟨ cong (λ z → (z + valFr M) % FR-ORDER) (valFr-* D P) ⟩
  ((valFr D * valFr P) % FR-ORDER + valFr M) % FR-ORDER
    ≡⟨ cong (λ z → ((valFr D * z) % FR-ORDER + valFr M) % FR-ORDER) (valFr-pow2 bits) ⟩
  ((valFr D * (2 ^ bits % FR-ORDER)) % FR-ORDER + valFr M) % FR-ORDER
    ≡⟨ cong (λ z → ((z * (2 ^ bits % FR-ORDER)) % FR-ORDER + valFr M) % FR-ORDER)
            (from-le-bits-roundtrip (drop bits (to-le-bits v))) ⟩
  ((dN % FR-ORDER * (2 ^ bits % FR-ORDER)) % FR-ORDER + valFr M) % FR-ORDER
    ≡⟨ cong (λ z → ((dN % FR-ORDER * (2 ^ bits % FR-ORDER)) % FR-ORDER + z) % FR-ORDER)
            (from-le-bits-roundtrip (take bits (to-le-bits v))) ⟩
  ((dN % FR-ORDER * (2 ^ bits % FR-ORDER)) % FR-ORDER + tN % FR-ORDER) % FR-ORDER
    ≡⟨ cong (λ z → (z + tN % FR-ORDER) % FR-ORDER) (sym (%-distribˡ-* dN (2 ^ bits) FR-ORDER)) ⟩
  ((dN * 2 ^ bits) % FR-ORDER + tN % FR-ORDER) % FR-ORDER
    ≡⟨ sym (%-distribˡ-+ (dN * 2 ^ bits) tN FR-ORDER) ⟩
  (dN * 2 ^ bits + tN) % FR-ORDER
    ≡⟨ cong (_% FR-ORDER) (sym (bits-to-ℕ-split bits (to-le-bits v))) ⟩
  valFr v % FR-ORDER
    ≡⟨ m<n⇒m%n≡m (valFr-bound v) ⟩
  valFr v ∎))
  where
    D  = from-le-bits (drop bits (to-le-bits v))
    M  = from-le-bits (take bits (to-le-bits v))
    P  = pow2-fr bits
    dN = bits-to-ℕ (drop bits (to-le-bits v))
    tN = bits-to-ℕ (take bits (to-le-bits v))

----------------------------------------------------------------------
-- The non-wrapping recombination guard `NoWrap` and the uniqueness it
-- buys for `constraint-div-mod`.
----------------------------------------------------------------------

-- The canonical decomposition of `v` recombines (in ℕ) to exactly
-- `valFr v`, with no reduction modulo the field order.  The two pieces
-- `dN`, `tN` are each ≤ `valFr v < FR-ORDER`, so `from-le-bits`'s modular
-- round trip is the identity on them.
divmod-canonical-value : ∀ v bits
  → valFr (from-le-bits (drop bits (to-le-bits v))) * 2 ^ bits
    + valFr (from-le-bits (take bits (to-le-bits v)))
    ≡ valFr v
divmod-canonical-value v bits = begin
  valFr (from-le-bits (drop bits BV)) * 2 ^ bits
    + valFr (from-le-bits (take bits BV))
    ≡⟨ cong (λ z → z * 2 ^ bits + valFr (from-le-bits (take bits BV)))
            (from-le-bits-roundtrip (drop bits BV)) ⟩
  dN % FR-ORDER * 2 ^ bits + valFr (from-le-bits (take bits BV))
    ≡⟨ cong (λ z → dN % FR-ORDER * 2 ^ bits + z)
            (from-le-bits-roundtrip (take bits BV)) ⟩
  dN % FR-ORDER * 2 ^ bits + tN % FR-ORDER
    ≡⟨ cong (λ z → z * 2 ^ bits + tN % FR-ORDER) (m<n⇒m%n≡m dN<order) ⟩
  dN * 2 ^ bits + tN % FR-ORDER
    ≡⟨ cong (dN * 2 ^ bits +_) (m<n⇒m%n≡m tN<order) ⟩
  dN * 2 ^ bits + tN
    ≡⟨ sym (bits-to-ℕ-split bits BV) ⟩
  valFr v ∎
  where
    BV = to-le-bits v
    dN = bits-to-ℕ (drop bits BV)
    tN = bits-to-ℕ (take bits BV)
    -- valFr v = dN·2^bits + tN, so both pieces are ≤ valFr v < FR-ORDER.
    dN≤val : dN * 2 ^ bits + tN ≡ valFr v
    dN≤val = sym (bits-to-ℕ-split bits BV)
    dN<order : dN < FR-ORDER
    dN<order = ≤-<-trans
      (≤-trans (m≤m*n dN (2 ^ bits) {{m^n≢0 2 bits}})
               (subst (dN * 2 ^ bits ≤_) dN≤val
                      (m≤m+n (dN * 2 ^ bits) tN)))
      (valFr-bound v)
    tN<order : tN < FR-ORDER
    tN<order = ≤-<-trans
      (subst (tN ≤_) dN≤val (m≤n+m tN (dN * 2 ^ bits)))
      (valFr-bound v)

-- The canonical decomposition satisfies the in-circuit `NoWrap` guard.
divmod-canonical-noWrap : ∀ v bits
  → NoWrap (from-le-bits (drop bits (to-le-bits v)))
           (from-le-bits (take bits (to-le-bits v))) bits
divmod-canonical-noWrap v bits =
  subst (_< FR-ORDER) (sym (divmod-canonical-value v bits)) (valFr-bound v)

-- ℕ Euclidean uniqueness: with `r, r' < d` a common recombination
-- `a·d + r ≡ a'·d + r'` forces `a ≡ a'` and `r ≡ r'`.
private
  mod-recombine : ∀ a r d .{{_ : NonZero d}} → r < d → (a * d + r) % d ≡ r
  mod-recombine a r d r<d = begin
    (a * d + r) % d
      ≡⟨ %-distribˡ-+ (a * d) r d ⟩
    ((a * d) % d + r % d) % d
      ≡⟨ cong (λ z → (z + r % d) % d) (m*n%n≡0 a d) ⟩
    (0 + r % d) % d
      ≡⟨ cong (_% d) (+-identityˡ (r % d)) ⟩
    (r % d) % d
      ≡⟨ m%n%n≡m%n r d ⟩
    r % d
      ≡⟨ m<n⇒m%n≡m r<d ⟩
    r ∎

  euclid-unique : ∀ a r a' r' d .{{_ : NonZero d}}
    → r < d → r' < d → a * d + r ≡ a' * d + r' → (a ≡ a') × (r ≡ r')
  euclid-unique a r a' r' d r<d r'<d eq = aeq , req
    where
      req : r ≡ r'
      req = trans (sym (mod-recombine a r d r<d))
            (trans (cong (_% d) eq) (mod-recombine a' r' d r'<d))
      eq2 : a * d + r ≡ a' * d + r
      eq2 = trans eq (cong (a' * d +_) (sym req))
      eq3 : r + a * d ≡ r + a' * d
      eq3 = trans (+-comm r (a * d)) (trans eq2 (+-comm (a' * d) r))
      aeq : a ≡ a'
      aeq = *-cancelʳ-≡ a a' d (+-cancelˡ-≡ r (a * d) (a' * d) eq3)

-- Value of the reconstitution concatenation under the fits premises.
concat-value : ∀ mv dv n
  → Fits-in mv n
  → Fits-in dv (FR-BITS ∸ n)
  → bits-to-ℕ (take n (to-le-bits mv) ++ take (FR-BITS ∸ n) (to-le-bits dv))
    ≡ valFr mv + 2 ^ (n ⊓ FR-BITS) * valFr dv
concat-value mv dv n fmv fdv = begin
  bits-to-ℕ (take n (to-le-bits mv) ++ take (FR-BITS ∸ n) (to-le-bits dv))
    ≡⟨ bits-to-ℕ-++ (take n (to-le-bits mv)) (take (FR-BITS ∸ n) (to-le-bits dv)) ⟩
  bits-to-ℕ (take n (to-le-bits mv))
    + 2 ^ length (take n (to-le-bits mv))
        * bits-to-ℕ (take (FR-BITS ∸ n) (to-le-bits dv))
    ≡⟨ cong (λ z → z + 2 ^ length (take n (to-le-bits mv))
                       * bits-to-ℕ (take (FR-BITS ∸ n) (to-le-bits dv)))
            (fits→take-full mv n fmv) ⟩
  valFr mv + 2 ^ length (take n (to-le-bits mv))
               * bits-to-ℕ (take (FR-BITS ∸ n) (to-le-bits dv))
    ≡⟨ cong (λ z → valFr mv + 2 ^ length (take n (to-le-bits mv)) * z)
            (fits→take-full dv (FR-BITS ∸ n) fdv) ⟩
  valFr mv + 2 ^ length (take n (to-le-bits mv)) * valFr dv
    ≡⟨ cong (λ L → valFr mv + 2 ^ L * valFr dv)
            (trans (length-take n (to-le-bits mv)) (cong (n ⊓_) (to-le-bits-length mv))) ⟩
  valFr mv + 2 ^ (n ⊓ FR-BITS) * valFr dv ∎

-- Strict divisor bound ⇒ the concatenated value is < FR-ORDER, hence
-- in field.  When n < FR_BITS the value is < 2^(FR_BITS−1) ≤ |Fr|;
-- when n ≥ FR_BITS the divisor is forced to 0 and the value is `valFr mv`.
bits-in-field-from-strict-bound : ∀ {dv mv n}
  → Fits-in mv n
  → Fits-in dv (FR-BITS ∸ n ∸ 1)
  → BitsInField
      (take n (to-le-bits mv) ++ take (FR-BITS ∸ n) (to-le-bits dv))
bits-in-field-from-strict-bound {dv} {mv} {n} fmv fdv =
  -- `BitsInField bs` is definitionally `bits-to-ℕ bs < FR-ORDER`, so the
  -- value bound *is* the witness.
  subst (_< FR-ORDER) (sym (concat-value mv dv n fmv fdv')) value-bound
  where
    fdv' : Fits-in dv (FR-BITS ∸ n)
    fdv' = fits-in-mono fdv (m∸n≤m (FR-BITS ∸ n) 1)

    value-bound : valFr mv + 2 ^ (n ⊓ FR-BITS) * valFr dv < FR-ORDER
    value-bound with n <? FR-BITS
    ... | yes n<FB =
      subst (λ e → valFr mv + 2 ^ e * valFr dv < FR-ORDER)
            (sym (m≤n⇒m⊓n≡m (<⇒≤ n<FB)))
            (<-≤-trans
              (combine-bound (valFr mv) (valFr dv) n (FR-BITS ∸ n ∸ 1)
                 fmv
                 fdv)
              (≤-trans (≤-reflexive (cong (2 ^_) (exp-sum n n<FB))) order-lb))
    ... | no n≥FB =
      subst (λ z → valFr mv + 2 ^ (n ⊓ FR-BITS) * z < FR-ORDER) (sym dv≡0)
        (subst (_< FR-ORDER) eq2 (valFr-bound mv))
      where
        FBn1≡0 : FR-BITS ∸ n ∸ 1 ≡ 0
        FBn1≡0 = cong (_∸ 1) (m≤n⇒m∸n≡0 (≮⇒≥ n≥FB))
        dv≡0 : valFr dv ≡ 0
        dv≡0 = n<1⇒n≡0 (subst (λ z → valFr dv < 2 ^ z) FBn1≡0
                              fdv)
        eq2 : valFr mv ≡ valFr mv + 2 ^ (n ⊓ FR-BITS) * 0
        eq2 = sym (trans (cong (valFr mv +_) (*-zeroʳ (2 ^ (n ⊓ FR-BITS))))
                         (+-identityʳ (valFr mv)))

-- Modular helpers: reducing a factor / summand mod FR-ORDER before the
-- outer mod is a no-op.
private
  mul-mod-r : ∀ a b → (a * (b % FR-ORDER)) % FR-ORDER ≡ (a * b) % FR-ORDER
  mul-mod-r a b =
    trans (%-distribˡ-* a (b % FR-ORDER) FR-ORDER)
    (trans (cong (λ z → (a % FR-ORDER * z) % FR-ORDER) (m%n%n≡m%n b FR-ORDER))
           (sym (%-distribˡ-* a b FR-ORDER)))

  add-mod-l : ∀ a b → (a % FR-ORDER + b) % FR-ORDER ≡ (a + b) % FR-ORDER
  add-mod-l a b =
    trans (%-distribˡ-+ (a % FR-ORDER) b FR-ORDER)
    (trans (cong (λ z → (z + b % FR-ORDER) % FR-ORDER) (m%n%n≡m%n a FR-ORDER))
           (sym (%-distribˡ-+ a b FR-ORDER)))

-- Reconstitution: the field value of the concatenated pattern is
-- exactly `dv·2^bits + mv`.  By valuation injectivity, both sides have
-- value `(valFr dv · 2^bits + valFr mv) mod FR-ORDER` — the equality
-- holds modulo FR-ORDER unconditionally, with no in-field (no-overflow)
-- hypothesis.
reconstitute-no-overflow : ∀ dv mv bits
  → Fits-in mv bits
  → Fits-in dv (FR-BITS ∸ bits)
  → from-le-bits
      (take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv))
    ≡ (dv *ᶠ pow2-fr bits) +ᶠ mv
reconstitute-no-overflow dv mv bits fmv fdv = valFr-inj (begin
  valFr (from-le-bits C)
    ≡⟨ from-le-bits-roundtrip C ⟩
  bits-to-ℕ C % FR-ORDER
    ≡⟨ cong (_% FR-ORDER) (concat-value mv dv bits fmv fdv) ⟩
  (valFr mv + 2 ^ (bits ⊓ FR-BITS) * valFr dv) % FR-ORDER
    ≡⟨ value-eq ⟩
  (valFr dv * 2 ^ bits + valFr mv) % FR-ORDER
    ≡⟨ sym rhs-val ⟩
  valFr ((dv *ᶠ pow2-fr bits) +ᶠ mv) ∎)
  where
    C = take bits (to-le-bits mv) ++ take (FR-BITS ∸ bits) (to-le-bits dv)

    rhs-val : valFr ((dv *ᶠ pow2-fr bits) +ᶠ mv)
            ≡ (valFr dv * 2 ^ bits + valFr mv) % FR-ORDER
    rhs-val = begin
      valFr ((dv *ᶠ pow2-fr bits) +ᶠ mv)
        ≡⟨ valFr-+ (dv *ᶠ pow2-fr bits) mv ⟩
      (valFr (dv *ᶠ pow2-fr bits) + valFr mv) % FR-ORDER
        ≡⟨ cong (λ z → (z + valFr mv) % FR-ORDER) (valFr-* dv (pow2-fr bits)) ⟩
      ((valFr dv * valFr (pow2-fr bits)) % FR-ORDER + valFr mv) % FR-ORDER
        ≡⟨ cong (λ z → ((valFr dv * z) % FR-ORDER + valFr mv) % FR-ORDER) (valFr-pow2 bits) ⟩
      ((valFr dv * (2 ^ bits % FR-ORDER)) % FR-ORDER + valFr mv) % FR-ORDER
        ≡⟨ cong (λ z → (z + valFr mv) % FR-ORDER) (mul-mod-r (valFr dv) (2 ^ bits)) ⟩
      ((valFr dv * 2 ^ bits) % FR-ORDER + valFr mv) % FR-ORDER
        ≡⟨ add-mod-l (valFr dv * 2 ^ bits) (valFr mv) ⟩
      (valFr dv * 2 ^ bits + valFr mv) % FR-ORDER ∎

    value-eq : (valFr mv + 2 ^ (bits ⊓ FR-BITS) * valFr dv) % FR-ORDER
             ≡ (valFr dv * 2 ^ bits + valFr mv) % FR-ORDER
    value-eq with bits <? FR-BITS
    ... | yes bits<FB =
      subst (λ e → (valFr mv + 2 ^ e * valFr dv) % FR-ORDER
                 ≡ (valFr dv * 2 ^ bits + valFr mv) % FR-ORDER)
            (sym (m≤n⇒m⊓n≡m (<⇒≤ bits<FB)))
            (cong (_% FR-ORDER)
                  (trans (+-comm (valFr mv) (2 ^ bits * valFr dv))
                         (cong (_+ valFr mv) (*-comm (2 ^ bits) (valFr dv)))))
    ... | no bits≥FB =
      subst (λ z → (valFr mv + 2 ^ (bits ⊓ FR-BITS) * z) % FR-ORDER
                 ≡ (z * 2 ^ bits + valFr mv) % FR-ORDER)
            (sym dv≡0)
            (cong (_% FR-ORDER)
                  (trans (cong (valFr mv +_) (*-zeroʳ (2 ^ (bits ⊓ FR-BITS))))
                         (+-identityʳ (valFr mv))))
      where
        dv≡0 : valFr dv ≡ 0
        dv≡0 = n<1⇒n≡0 (subst (λ z → valFr dv < 2 ^ z)
                              (m≤n⇒m∸n≡0 (≮⇒≥ bits≥FB))
                              fdv)

----------------------------------------------------------------------
-- Padding a `bits-lt` comparison from n to lt-bits n bits (≥ n) leaves
-- it unchanged when both operands fit in n bits.  `bits-lt` reduces to
-- numeric `<` on `valFr`, and `take m (to-le-bits x) = valFr x` for any
-- m ≥ n when x fits in n bits, so both comparisons compare `valFr av`
-- with `valFr bv`.
----------------------------------------------------------------------
private
  -- Length of the reversed truncation, common to both operands.
  tklen : ∀ m x → length (reverse (take m (to-le-bits x))) ≡ m ⊓ FR-BITS
  tklen m x = trans (length-reverse (take m (to-le-bits x)))
                    (trans (length-take m (to-le-bits x)) (cong (m ⊓_) (to-le-bits-length x)))

  -- `msbval` of the reversed truncation = `valFr x`, when x fits in m.
  tkval : ∀ m x → Fits-in x m → msbval (reverse (take m (to-le-bits x))) ≡ valFr x
  tkval m x fx = trans (msbval-reverse (take m (to-le-bits x))) (fits→take-full x m fx)

  bits-lt-true-side : ∀ m av bv → Fits-in av m → Fits-in bv m
    → valFr av < valFr bv
    → bits-lt (take m (to-le-bits av)) (take m (to-le-bits bv)) ≡ true
  bits-lt-true-side m av bv fav fbv lt =
    bits-cmp-true (reverse (take m (to-le-bits av))) (reverse (take m (to-le-bits bv)))
      (trans (tklen m av) (sym (tklen m bv)))
      (subst₂ _<_ (sym (tkval m av fav)) (sym (tkval m bv fbv)) lt)

  bits-lt-false-side : ∀ m av bv → Fits-in av m → Fits-in bv m
    → valFr bv ≤ valFr av
    → bits-lt (take m (to-le-bits av)) (take m (to-le-bits bv)) ≡ false
  bits-lt-false-side m av bv fav fbv le =
    bits-cmp-false (reverse (take m (to-le-bits av))) (reverse (take m (to-le-bits bv)))
      (trans (tklen m av) (sym (tklen m bv)))
      (subst₂ _≤_ (sym (tkval m bv fbv)) (sym (tkval m av fav)) le)

bits-lt-pad : ∀ av bv n
  → Fits-in av n
  → Fits-in bv n
  → bits-lt (take (lt-bits n) (to-le-bits av)) (take (lt-bits n) (to-le-bits bv))
    ≡ bits-lt (take n (to-le-bits av)) (take n (to-le-bits bv))
bits-lt-pad av bv n fav fbv with <-cmp (valFr av) (valFr bv)
... | tri< a<b _ _ =
  trans (bits-lt-true-side (lt-bits n) av bv (fits-in-lt-bits av n fav) (fits-in-lt-bits bv n fbv) a<b)
        (sym (bits-lt-true-side n av bv fav fbv a<b))
... | tri≈ _ a≡b _ =
  trans (bits-lt-false-side (lt-bits n) av bv (fits-in-lt-bits av n fav) (fits-in-lt-bits bv n fbv)
           (≤-reflexive (sym a≡b)))
        (sym (bits-lt-false-side n av bv fav fbv (≤-reflexive (sym a≡b))))
... | tri> _ _ b<a =
  trans (bits-lt-false-side (lt-bits n) av bv (fits-in-lt-bits av n fav) (fits-in-lt-bits bv n fbv)
           (<⇒≤ b<a))
        (sym (bits-lt-false-side n av bv fav fbv (<⇒≤ b<a)))

----------------------------------------------------------------------
-- Uniqueness of the guarded `constraint-div-mod` decomposition.
----------------------------------------------------------------------

-- Under `NoWrap`, the field equation `v = q·2^bits + r` lifts to a
-- non-wrapping ℕ equation `valFr v = valFr q · 2^bits + valFr r`.
private
  recombine-value : ∀ qv rv bits
    → NoWrap qv rv bits
    → valFr ((qv *ᶠ pow2-fr bits) +ᶠ rv)
      ≡ valFr qv * 2 ^ bits + valFr rv
  recombine-value qv rv bits nw = begin
    valFr ((qv *ᶠ pow2-fr bits) +ᶠ rv)
      ≡⟨ valFr-+ (qv *ᶠ pow2-fr bits) rv ⟩
    (valFr (qv *ᶠ pow2-fr bits) + valFr rv) % FR-ORDER
      ≡⟨ cong (λ z → (z + valFr rv) % FR-ORDER) (valFr-* qv (pow2-fr bits)) ⟩
    ((valFr qv * valFr (pow2-fr bits)) % FR-ORDER + valFr rv) % FR-ORDER
      ≡⟨ cong (λ z → ((valFr qv * z) % FR-ORDER + valFr rv) % FR-ORDER)
              (valFr-pow2 bits) ⟩
    ((valFr qv * (2 ^ bits % FR-ORDER)) % FR-ORDER + valFr rv) % FR-ORDER
      ≡⟨ cong (λ z → (z + valFr rv) % FR-ORDER)
              (mul-mod-r (valFr qv) (2 ^ bits)) ⟩
    ((valFr qv * 2 ^ bits) % FR-ORDER + valFr rv) % FR-ORDER
      ≡⟨ add-mod-l (valFr qv * 2 ^ bits) (valFr rv) ⟩
    (valFr qv * 2 ^ bits + valFr rv) % FR-ORDER
      ≡⟨ m<n⇒m%n≡m nw ⟩
    valFr qv * 2 ^ bits + valFr rv ∎

-- For a fixed `v`, any two (q, r) satisfying the constraint's value
-- equation, the range bound on r, and the `NoWrap` guard are equal:
-- both recombine without wrapping to `valFr v`, so they are the same
-- Euclidean split of `valFr v`; `valFr-inj` lifts the value equalities
-- back to `Fr`.
div-mod-constraint-unique : ∀ {qv rv qv' rv' v bits}
  → Fits-in rv bits
  → Fits-in rv' bits
  → NoWrap qv rv bits
  → NoWrap qv' rv' bits
  → v ≡ (qv *ᶠ pow2-fr bits) +ᶠ rv
  → v ≡ (qv' *ᶠ pow2-fr bits) +ᶠ rv'
  → (qv ≡ qv') × (rv ≡ rv')
div-mod-constraint-unique {qv} {rv} {qv'} {rv'} {v} {bits}
  fr fr' nw nw' eq eq' =
  let value-eq : valFr qv * 2 ^ bits + valFr rv
               ≡ valFr qv' * 2 ^ bits + valFr rv'
      value-eq = trans (sym (recombine-value qv rv bits nw))
                 (trans (cong valFr (trans (sym eq) eq'))
                        (recombine-value qv' rv' bits nw'))
      (vqeq , vreq) = euclid-unique (valFr qv) (valFr rv)
                        (valFr qv') (valFr rv') (2 ^ bits)
                        {{m^n≢0 2 bits}} fr fr' value-eq
  in valFr-inj vqeq , valFr-inj vreq

-- A satisfying `constraint-div-mod` value equation, with the range bound on
-- the remainder and the `NoWrap` guard, pins (q, r) to the canonical
-- (non-wrapping) bit-decomposition of `vv`.  This is the payload the
-- div-mod backward step and the statement-soundness walk both read off
-- the constraint.
divmod-canon : ∀ {qv rv vv : Fr} {bits : ℕ}
  → Fits-in rv bits
  → NoWrap qv rv bits
  → vv ≡ (qv *ᶠ pow2-fr bits) +ᶠ rv
  → (qv ≡ from-le-bits (drop bits (to-le-bits vv)))
  × (rv ≡ from-le-bits (take bits (to-le-bits vv)))
divmod-canon {vv = vv} {bits = bits} frv nw veq =
  div-mod-constraint-unique {bits = bits} frv
                        (fits-from-le-bits-take (to-le-bits vv) bits)
                        nw (divmod-canonical-noWrap vv bits)
                        veq (bits-decomp-split vv bits)
