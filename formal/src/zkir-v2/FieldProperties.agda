{-# OPTIONS --safe #-}

------------------------------------------------------------------------
-- Field properties of the zkir-v2 formalization
--
-- Consequences of the `Assumptions` field operations:
-- the boolean field-equality test and its reflection laws, `to-bool`,
-- the `fits-in` / `bits-lt` bit predicates, `pow2-fr`, `lt-bits`, and the
-- pure list/nat bit-value machinery (`bits-to-ℕ-split`, `msbval`, …).
------------------------------------------------------------------------

open import zkir-v2.Assumptions

module zkir-v2.FieldProperties (⋯ : Assumptions) (open Assumptions ⋯) where

open import Data.Bool  using (Bool; true; false; if_then_else_; T)
open import Data.Empty using (⊥-elim)
open import Data.Unit  using (tt)
open import Data.List  using (List; []; _∷_; _++_; take; drop; reverse; length)
open import Data.Maybe using (Maybe; nothing; just)
open import Data.Nat   using (ℕ; zero; suc; _+_; _∸_; _%_; _⊔_; _⊓_; _≤_; _<_; _<?_; _*_; _^_)
open import Data.Nat.Properties  using ( m+[n∸m]≡n; m≤m+n; m≤m⊔n; ≤-trans
                                        ; ≤-<-trans; <-≤-trans; m≤n+m; m⊓n≤m
                                        ; *-monoʳ-<; *-monoʳ-≤; ^-monoʳ-≤
                                        ; *-cancelˡ-<; *-cancelʳ-<; *-identityˡ
                                        ; <ᵇ⇒<
                                        ; *-distribˡ-+; n<1⇒n≡0; 0<1+n
                                        ; +-assoc; +-comm; +-identityʳ
                                        ; *-assoc; *-comm; *-identityʳ; *-zeroʳ
                                        ; ≤-reflexive; ∸-+-assoc; ^-distribˡ-+-*
                                        ; +-monoˡ-<; +-monoʳ-<
                                        ; +-cancelˡ-<; +-cancelˡ-≤; <-asym; <-irrefl; suc-injective )
open import Data.List.Properties using (drop-drop; length-take; length-drop
                                        ; length-reverse; reverse-involutive; unfold-reverse)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; cong₂; subst; module ≡-Reasoning)
open import Relation.Nullary using (¬_; yes; no; Dec)
open import Relation.Nullary.Decidable using (does; dec-true; dec-false)
open ≡-Reasoning

-- Boolean field-equality test, derived from decidable propositional
-- equality.
_≡ᶠ?_ : Fr → Fr → Bool
x ≡ᶠ? y = does (x ≟ᶠ y)

-- Reflection of boolean field-equality into propositional equality:
-- theorems about `_≟ᶠ_`.
≡ᶠ?-refl : ∀ {x} → (x ≡ᶠ? x) ≡ true
≡ᶠ?-refl {x} = dec-true (x ≟ᶠ x) refl

≡ᶠ?-false : ∀ {x y} → ¬ (x ≡ y) → (x ≡ᶠ? y) ≡ false
≡ᶠ?-false {x} {y} = dec-false (x ≟ᶠ y)

≡ᶠ?-true : ∀ {x y} → T (x ≡ᶠ? y) → x ≡ y
≡ᶠ?-true {x} {y} t with x ≟ᶠ y
... | yes p = p
... | no  _ = ⊥-elim t

-- Negation fixes zero: −0 = 0.  (0 = 0 + (−0) = −0.)
-ᶠ-zero : (-ᶠ 0ᶠ) ≡ 0ᶠ
-ᶠ-zero = trans (sym (+-zero-l (-ᶠ 0ᶠ))) (+-inv-r 0ᶠ)

-- Right additive identity, from commutativity and the left law.
+-zero-r : ∀ x → x +ᶠ 0ᶠ ≡ x
+-zero-r x = trans (+ᶠ-comm x 0ᶠ) (+-zero-l x)

-- Left additive inverse, and additive cancellation, derived from the
-- field laws.  `+ᶠ-cancel` is the bridge that turns a polynomial gate
-- `x − y = 0` into the equation `x = y` (see `zkir-v2.Circuit`).
+ᶠ-inv-l : ∀ x → (-ᶠ x) +ᶠ x ≡ 0ᶠ
+ᶠ-inv-l x = trans (+ᶠ-comm (-ᶠ x) x) (+-inv-r x)

+ᶠ-cancel : ∀ {x y} → x +ᶠ (-ᶠ y) ≡ 0ᶠ → x ≡ y
+ᶠ-cancel {x} {y} h = begin
  x                  ≡⟨ sym (+-zero-r x) ⟩
  x +ᶠ 0ᶠ            ≡⟨ cong (x +ᶠ_) (sym (+ᶠ-inv-l y)) ⟩
  x +ᶠ ((-ᶠ y) +ᶠ y) ≡⟨ sym (+ᶠ-assoc x (-ᶠ y) y) ⟩
  (x +ᶠ (-ᶠ y)) +ᶠ y ≡⟨ cong (_+ᶠ y) h ⟩
  0ᶠ +ᶠ y            ≡⟨ +-zero-l y ⟩
  y                  ∎

-- Convert Fr to Bool; nothing if not in {0, 1} (UB in the IR)
to-bool : Fr → Maybe Bool
to-bool x with x ≟ᶠ 0ᶠ
... | yes _ = just false
... | no  _ with x ≟ᶠ 1ᶠ
...   | yes _ = just true
...   | no  _ = nothing

-- `x` fits in `n` bits: its canonical value is below 2^n.
-- A decidable predicate.
Fits-in : Fr → ℕ → Set
Fits-in x n = valFr x < 2 ^ n

fits-in? : ∀ x n → Dec (Fits-in x n)
fits-in? x n = valFr x <? 2 ^ n

-- Non-wrapping recombination guard for `div_mod_power_of_two`: the
-- integer recombination `q·2^bits + r` stays below the field order, so
-- it does not wrap modulo |Fr|.  This is the in-circuit analogue of the
-- `BitsInField` check `reconstitute-field` performs operationally; with
-- it, the field equation `v = q·2^bits + r` forces the unique Euclidean
-- decomposition of `valFr v` (see `Encoding.div-mod-constraint-unique`).
NoWrap : Fr → Fr → ℕ → Set
NoWrap q r bits = valFr q * 2 ^ bits + valFr r < FR-ORDER

noWrap? : ∀ q r bits → Dec (NoWrap q r bits)
noWrap? q r bits = (valFr q * 2 ^ bits + valFr r) <? FR-ORDER

-- Bridges to/from the Bool decision `does (fits-in? x n)`, used by the
-- operational/relational guards (which need a Bool) while the proofs
-- carry the `Fits-in` predicate.
fits-in?-true : ∀ x n → Fits-in x n → does (fits-in? x n) ≡ true
fits-in?-true x n = dec-true (fits-in? x n)

fits-in?-from-true : ∀ x n → T (does (fits-in? x n)) → Fits-in x n
fits-in?-from-true x n = <ᵇ⇒< (valFr x) (2 ^ n)

-- Same bridges for the `BitsInField` decision (used by the
-- reconstitute-field guard in the operational/relational correspondence).
bitsInField?-true : ∀ bs → BitsInField bs → does (bitsInField? bs) ≡ true
bitsInField?-true bs = dec-true (bitsInField? bs)

bitsInField?-from-true : ∀ bs → T (does (bitsInField? bs)) → BitsInField bs
bitsInField?-from-true bs = <ᵇ⇒< (bits-to-ℕ bs) FR-ORDER

-- MSB-first lexicographic comparison of two equal-length bit lists.
bits-cmp : List Bool → List Bool → Bool
bits-cmp []          _          = false
bits-cmp _           []         = false
bits-cmp (false ∷ _) (true ∷ _) = true
bits-cmp (true ∷ _)  (false ∷ _) = false
bits-cmp (_ ∷ as')   (_ ∷ bs')   = bits-cmp as' bs'

-- bits-lt as bs: true iff the natural number represented by as (LE) is
-- < that of bs.  Assumes both lists have the same length.
bits-lt : List Bool → List Bool → Bool
bits-lt as bs = bits-cmp (reverse as) (reverse bs)

-- 2^n as a field element.
pow2-fr : ℕ → Fr
pow2-fr zero    = 1ᶠ
pow2-fr (suc n) = pow2-fr n +ᶠ pow2-fr n

-- Padded bit-count used by the range-check chip.
lt-bits : ℕ → ℕ
lt-bits n = (n + (n % 2)) ⊔ 4

-- Monotonicity of `Fits-in`: if v fits in m bits, it fits in any larger
-- bit-count.
fits-in-mono : ∀ {v m n} → Fits-in v m → m ≤ n → Fits-in v n
fits-in-mono p m≤n = <-≤-trans p (^-monoʳ-≤ 2 m≤n)

-- `lt-bits n ≥ n`, so a value fitting in n bits also fits in
-- `lt-bits n` bits.  A corollary of `fits-in-mono`: `lt-bits` only
-- ever grows the bit-count (`n ≤ (n + n % 2) ⊔ 4`).
fits-in-lt-bits : ∀ v n → Fits-in v n → Fits-in v (lt-bits n)
fits-in-lt-bits v n p =
  fits-in-mono p (≤-trans (m≤m+n n (n % 2)) (m≤m⊔n (n + (n % 2)) 4))

----------------------------------------------------------------------
-- Bounds on the integer value of a little-endian bit list.
----------------------------------------------------------------------

private
  -- A bit list of length L represents a value < 2^L.
  bits-to-ℕ-bound : ∀ bs → bits-to-ℕ bs < 2 ^ length bs
  bits-to-ℕ-bound []           = 0<1+n
  bits-to-ℕ-bound (false ∷ bs) = *-monoʳ-< 2 (bits-to-ℕ-bound bs)
  bits-to-ℕ-bound (true  ∷ bs) =
    subst (_≤ 2 * 2 ^ length bs) (*-distribˡ-+ 2 1 (bits-to-ℕ bs))
          (*-monoʳ-≤ 2 (bits-to-ℕ-bound bs))

-- `take n bs` represents a value < 2^n.
take-bound : ∀ n bs → bits-to-ℕ (take n bs) < 2 ^ n
take-bound n bs =
  <-≤-trans
    (subst (λ L → bits-to-ℕ (take n bs) < 2 ^ L) (length-take n bs)
           (bits-to-ℕ-bound (take n bs)))
    (^-monoʳ-≤ 2 (m⊓n≤m n (length bs)))

-- `drop n bs` represents a value < 2^(length bs − n).
drop-bound : ∀ n bs → bits-to-ℕ (drop n bs) < 2 ^ (length bs ∸ n)
drop-bound n bs =
  subst (λ L → bits-to-ℕ (drop n bs) < 2 ^ L) (length-drop n bs)
        (bits-to-ℕ-bound (drop n bs))

----------------------------------------------------------------------
-- The LE split/concat identities for the canonical valuation.
----------------------------------------------------------------------

private
  -- Ring rearrangements (proved by hand to avoid solver-API coupling).
  2*[dp] : ∀ d p → 2 * (d * p) ≡ d * (2 * p)
  2*[dp] d p = trans (sym (*-assoc 2 d p))
                     (trans (cong (_* p) (*-comm 2 d)) (*-assoc d 2 p))

  split-rearrange : ∀ i d t p → i + 2 * (d * p + t) ≡ d * (2 * p) + (i + 2 * t)
  split-rearrange i d t p = begin
    i + 2 * (d * p + t)            ≡⟨ cong (i +_) (*-distribˡ-+ 2 (d * p) t) ⟩
    i + (2 * (d * p) + 2 * t)      ≡⟨ cong (λ z → i + (z + 2 * t)) (2*[dp] d p) ⟩
    i + (d * (2 * p) + 2 * t)      ≡⟨ sym (+-assoc i (d * (2 * p)) (2 * t)) ⟩
    (i + d * (2 * p)) + 2 * t      ≡⟨ cong (_+ 2 * t) (+-comm i (d * (2 * p))) ⟩
    (d * (2 * p) + i) + 2 * t      ≡⟨ +-assoc (d * (2 * p)) i (2 * t) ⟩
    d * (2 * p) + (i + 2 * t)      ∎

  ++-rearrange : ∀ i x p y → i + 2 * (x + p * y) ≡ (i + 2 * x) + (2 * p) * y
  ++-rearrange i x p y = begin
    i + 2 * (x + p * y)            ≡⟨ cong (i +_) (*-distribˡ-+ 2 x (p * y)) ⟩
    i + (2 * x + 2 * (p * y))      ≡⟨ cong (λ z → i + (2 * x + z)) (2*[dp] p y) ⟩
    i + (2 * x + p * (2 * y))      ≡⟨ sym (+-assoc i (2 * x) (p * (2 * y))) ⟩
    (i + 2 * x) + p * (2 * y)      ≡⟨ cong ((i + 2 * x) +_) (sym (2*[dp] p y)) ⟩
    (i + 2 * x) + 2 * (p * y)      ≡⟨ cong (λ z → (i + 2 * x) + z) (sym (*-assoc 2 p y)) ⟩
    (i + 2 * x) + (2 * p) * y      ∎

-- LE split: dropping/​taking at position n recovers the value as
-- high·2^n + low.  Holds for every n (drop/take saturate past the end).
bits-to-ℕ-split : ∀ n bs
  → bits-to-ℕ bs ≡ bits-to-ℕ (drop n bs) * 2 ^ n + bits-to-ℕ (take n bs)
bits-to-ℕ-split zero    bs       =
  sym (trans (+-identityʳ (bits-to-ℕ bs * 1)) (*-identityʳ (bits-to-ℕ bs)))
bits-to-ℕ-split (suc k) []       = refl
bits-to-ℕ-split (suc k) (b ∷ bs) =
  trans (cong (λ z → (if b then 1 else 0) + 2 * z) (bits-to-ℕ-split k bs))
        (split-rearrange (if b then 1 else 0)
                         (bits-to-ℕ (drop k bs)) (bits-to-ℕ (take k bs)) (2 ^ k))

-- LE concatenation: value of xs ++ ys = value xs + 2^|xs|·value ys.
bits-to-ℕ-++ : ∀ xs ys
  → bits-to-ℕ (xs ++ ys) ≡ bits-to-ℕ xs + 2 ^ length xs * bits-to-ℕ ys
bits-to-ℕ-++ []       ys = sym (+-identityʳ (bits-to-ℕ ys))
bits-to-ℕ-++ (b ∷ xs) ys =
  trans (cong (λ z → (if b then 1 else 0) + 2 * z) (bits-to-ℕ-++ xs ys))
        (++-rearrange (if b then 1 else 0)
                      (bits-to-ℕ xs) (2 ^ length xs) (bits-to-ℕ ys))

-- `fits-in x n` ⇒ the dropped high part has value 0, hence the low n
-- bits already carry the full value.
-- `Fits-in x n` ⇒ the dropped high part has value 0.  From the split
-- `valFr x ≡ q·2^n + r`, the bound `valFr x < 2^n` forces `q ≡ 0`.
fits→drop0 : ∀ x n → Fits-in x n → bits-to-ℕ (drop n (to-le-bits x)) ≡ 0
fits→drop0 x n fit =
  n<1⇒n≡0 (*-cancelʳ-< (2 ^ n) (bits-to-ℕ (drop n (to-le-bits x))) 1
    (subst (bits-to-ℕ (drop n (to-le-bits x)) * 2 ^ n <_)
           (sym (*-identityˡ (2 ^ n)))
           (≤-<-trans
             (subst (bits-to-ℕ (drop n (to-le-bits x)) * 2 ^ n ≤_)
                    (sym (bits-to-ℕ-split n (to-le-bits x)))
                    (m≤m+n (bits-to-ℕ (drop n (to-le-bits x)) * 2 ^ n)
                           (bits-to-ℕ (take n (to-le-bits x)))))
             fit)))

fits→take-full : ∀ x n → Fits-in x n
  → bits-to-ℕ (take n (to-le-bits x)) ≡ valFr x
fits→take-full x n p =
  sym (trans (bits-to-ℕ-split n (to-le-bits x))
             (cong (λ z → z * 2 ^ n + bits-to-ℕ (take n (to-le-bits x)))
                   (fits→drop0 x n p)))

-- If a < 2^e and b < 2^d then a + 2^e·b < 2^(e+d).
combine-bound : ∀ a b e d → a < 2 ^ e → b < 2 ^ d → a + 2 ^ e * b < 2 ^ (e + d)
combine-bound a b e d a<2e b<2d =
  <-≤-trans (+-monoˡ-< (2 ^ e * b) a<2e)
    (≤-trans (≤-reflexive (sym eq1))
    (≤-trans (*-monoʳ-≤ (2 ^ e) b<2d)
             (≤-reflexive (sym (^-distribˡ-+-* 2 e d)))))
  where eq1 : 2 ^ e * suc b ≡ 2 ^ e + 2 ^ e * b
        eq1 = trans (*-distribˡ-+ (2 ^ e) 1 b) (cong (_+ 2 ^ e * b) (*-identityʳ (2 ^ e)))

-- For n < FR_BITS: n + (FR_BITS − n − 1) = FR_BITS − 1.
exp-sum : ∀ n → n < FR-BITS → n + (FR-BITS ∸ n ∸ 1) ≡ FR-BITS ∸ 1
exp-sum n n<FB =
  trans (cong (n +_)
              (trans (∸-+-assoc FR-BITS n 1) (cong (FR-BITS ∸_) (+-comm n 1))))
        (cong (_∸ 1) (m+[n∸m]≡n n<FB))

----------------------------------------------------------------------
-- `bits-lt` as a numeric comparison (used to discharge `bits-lt-pad`).
--
-- `msbval` is the MSB-first value; `bits-cmp` (MSB-first lexicographic)
-- agrees with numeric `<` on equal-length lists.  `msbval ∘ reverse`
-- recovers the LE value `bits-to-ℕ`, so `bits-lt` is numeric `<`.
----------------------------------------------------------------------

msbval : List Bool → ℕ
msbval []       = 0
msbval (b ∷ bs) = (if b then 2 ^ length bs else 0) + msbval bs

msbval-bound : ∀ bs → msbval bs < 2 ^ length bs
msbval-bound []           = 0<1+n
msbval-bound (false ∷ bs) =
  <-≤-trans (msbval-bound bs) (^-monoʳ-≤ 2 (m≤n+m (length bs) 1))
msbval-bound (true ∷ bs)  =
  <-≤-trans (+-monoʳ-< (2 ^ length bs) (msbval-bound bs))
            (≤-reflexive (cong (2 ^ length bs +_) (sym (+-identityʳ (2 ^ length bs)))))

-- `msbval (false ∷ xs) < msbval (true ∷ ys)` when the tails match length.
private
  msbval-f<t : ∀ xs ys → length xs ≡ length ys → msbval xs < 2 ^ length ys + msbval ys
  msbval-f<t xs ys len =
    <-≤-trans (subst (λ L → msbval xs < 2 ^ L) len (msbval-bound xs))
              (m≤m+n (2 ^ length ys) (msbval ys))

-- MSB-first comparison agrees with numeric `<` on equal-length lists.
bits-cmp-true : ∀ xs ys → length xs ≡ length ys
  → msbval xs < msbval ys → bits-cmp xs ys ≡ true
bits-cmp-true []           []           _   ()
bits-cmp-true []           (_ ∷ _)      ()  _
bits-cmp-true (_ ∷ _)      []           ()  _
bits-cmp-true (false ∷ _)  (true ∷ _)   _   _  = refl
bits-cmp-true (true ∷ xs)  (false ∷ ys) len lt =
  ⊥-elim (<-asym lt (msbval-f<t ys xs (sym (suc-injective len))))
bits-cmp-true (false ∷ xs) (false ∷ ys) len lt = bits-cmp-true xs ys (suc-injective len) lt
bits-cmp-true (true ∷ xs)  (true ∷ ys)  len lt =
  bits-cmp-true xs ys (suc-injective len)
    (+-cancelˡ-< (2 ^ length xs) (msbval xs) (msbval ys)
       (subst (λ L → 2 ^ length xs + msbval xs < 2 ^ L + msbval ys)
              (sym (suc-injective len)) lt))

bits-cmp-false : ∀ xs ys → length xs ≡ length ys
  → msbval ys ≤ msbval xs → bits-cmp xs ys ≡ false
bits-cmp-false []           []           _   _  = refl
bits-cmp-false []           (_ ∷ _)      ()  _
bits-cmp-false (_ ∷ _)      []           ()  _
bits-cmp-false (true ∷ _)   (false ∷ _)  _   _  = refl
bits-cmp-false (false ∷ xs) (true ∷ ys)  len le =
  ⊥-elim (<-irrefl refl (≤-<-trans le (msbval-f<t xs ys (suc-injective len))))
bits-cmp-false (false ∷ xs) (false ∷ ys) len le = bits-cmp-false xs ys (suc-injective len) le
bits-cmp-false (true ∷ xs)  (true ∷ ys)  len le =
  bits-cmp-false xs ys (suc-injective len)
    (+-cancelˡ-≤ (2 ^ length xs) (msbval ys) (msbval xs)
       (subst (λ L → 2 ^ L + msbval ys ≤ 2 ^ length xs + msbval xs)
              (sym (suc-injective len)) le))

-- `msbval` of a list equals the LE value of its reverse.
msbval-rev : ∀ xs → msbval xs ≡ bits-to-ℕ (reverse xs)
msbval-rev []       = refl
msbval-rev (b ∷ xs) = begin
  (if b then 2 ^ length xs else 0) + msbval xs
    ≡⟨ cong ((if b then 2 ^ length xs else 0) +_) (msbval-rev xs) ⟩
  (if b then 2 ^ length xs else 0) + bits-to-ℕ (reverse xs)
    ≡⟨ rearr b ⟩
  bits-to-ℕ (reverse xs) + 2 ^ length xs * (if b then 1 else 0)
    ≡⟨ cong₂ (λ E F → bits-to-ℕ (reverse xs) + E * F)
             (cong (2 ^_) (sym (length-reverse xs)))
             (sym (+-identityʳ (if b then 1 else 0))) ⟩
  bits-to-ℕ (reverse xs) + 2 ^ length (reverse xs) * bits-to-ℕ (b ∷ [])
    ≡⟨ sym (bits-to-ℕ-++ (reverse xs) (b ∷ [])) ⟩
  bits-to-ℕ (reverse xs ++ (b ∷ []))
    ≡⟨ cong bits-to-ℕ (sym (unfold-reverse b xs)) ⟩
  bits-to-ℕ (reverse (b ∷ xs)) ∎
  where
    rearr : ∀ b → (if b then 2 ^ length xs else 0) + bits-to-ℕ (reverse xs)
          ≡ bits-to-ℕ (reverse xs) + 2 ^ length xs * (if b then 1 else 0)
    rearr true  = trans (+-comm (2 ^ length xs) (bits-to-ℕ (reverse xs)))
                        (cong (bits-to-ℕ (reverse xs) +_) (sym (*-identityʳ (2 ^ length xs))))
    rearr false = sym (trans (cong (bits-to-ℕ (reverse xs) +_) (*-zeroʳ (2 ^ length xs)))
                             (+-identityʳ (bits-to-ℕ (reverse xs))))

msbval-reverse : ∀ cs → msbval (reverse cs) ≡ bits-to-ℕ cs
msbval-reverse cs = trans (msbval-rev (reverse cs)) (cong bits-to-ℕ (reverse-involutive cs))
