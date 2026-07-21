{-# OPTIONS --safe #-}

------------------------------------------------------------------------
-- Assumptions of the zkir-v2 formalization
--
-- The entire trust base of the development, collected into a single flat
-- record `Assumptions`.  Downstream modules take an `Assumptions` value
-- as a module parameter (`module M (⋯ : _) (open Assumptions ⋯) where`),
-- so the whole development typechecks under `--safe` with no
-- `postulate`s.  No concrete BLS12-381 instantiation is provided;
-- the development is intentionally abstract over this interface.
--
-- The record bundles:
--   * the primitive carrier types and field/curve/hash/commitment
--     operations, together with the field laws;
--   * the encoding / valuation axioms.
------------------------------------------------------------------------

module zkir-v2.Assumptions where

open import Data.Bool  using (Bool; if_then_else_)
open import Data.List  using (List; []; _∷_; length)
open import Data.Maybe using (Maybe)
open import Data.Nat   using (ℕ; _+_; _∸_; _%_; _≤_; _<_; _<?_; _*_; _^_; NonZero)
open import Data.Product using (_×_)
open import Relation.Binary.Definitions using (DecidableEquality)
open import Relation.Binary.PropositionalEquality using (_≡_)
open import Relation.Nullary using (¬_; Dec)

-- Integer value of a little-endian bit list.
bits-to-ℕ : List Bool → ℕ
bits-to-ℕ []       = 0
bits-to-ℕ (b ∷ bs) = (if b then 1 else 0) + 2 * bits-to-ℕ bs

record Assumptions : Set₁ where

  ----------------------------------------------------------------------
  -- Primitive carrier types and operations.
  ----------------------------------------------------------------------

  field
    -- Carrier types.
    Fr        : Set
    -- ^ BLS12-381 scalar field element (transient_crypto::curve::Fr).
    Alignment : Set
    -- ^ Byte-alignment descriptor (base_crypto::fab::Alignment).

    -- Field constants
    0ᶠ 1ᶠ : Fr

    -- Field arithmetic
    _+ᶠ_ _*ᶠ_ : Fr → Fr → Fr
    -ᶠ_       : Fr → Fr

    -- Decidable propositional equality on field elements.
    _≟ᶠ_ : DecidableEquality Fr

    -- Field laws.  The fragment of the BLS12-381 scalar-field axioms the
    -- development actually uses; together with `1ᶠ≢0ᶠ` they say `Fr` with
    -- the operations above is a non-trivial field.
    *-zero-l : ∀ x → 0ᶠ *ᶠ x ≡ 0ᶠ
    *-one-l  : ∀ x → 1ᶠ *ᶠ x ≡ x
    +-zero-l : ∀ x → 0ᶠ +ᶠ x ≡ x
    +-inv-r  : ∀ x → x +ᶠ (-ᶠ x) ≡ 0ᶠ
    +ᶠ-assoc : ∀ x y z → (x +ᶠ y) +ᶠ z ≡ x +ᶠ (y +ᶠ z)
    +ᶠ-comm  : ∀ x y → x +ᶠ y ≡ y +ᶠ x
    -- (The mirrored identities — `+-zero-r`, `+ᶠ-inv-l` — are derived
    -- from these in `FieldProperties`, not assumed.)

    -- 1 ≠ 0 — non-triviality.  Used by `not-fwd` for the `bv = 1ᶠ` case,
    -- and to characterize `to-bool 1ᶠ`.
    1ᶠ≢0ᶠ : ¬ (1ᶠ ≡ 0ᶠ)

  field
    -- Number of bits in a field element (255 for BLS12-381 scalar field)
    FR-BITS : ℕ

    -- Order of the scalar field (the prime |Fr|, ≈ 2^255 for BLS12-381).
    -- `from-le-bits` reduces modulo this value.
    FR-ORDER : ℕ

    -- Little-endian bit decomposition: to-le-bits x has exactly FR-BITS entries
    to-le-bits   : Fr → List Bool
    from-le-bits : List Bool → Fr

    -- Jubjub EC operations; nothing = invalid input point(s)
    ec-add-pts       : Fr → Fr → Fr → Fr → Maybe (Fr × Fr)
    ec-mul-pt        : Fr → Fr → Fr → Maybe (Fr × Fr)
    ec-mul-gen       : Fr → Fr × Fr
    hash-to-curve-fn : List Fr → Fr × Fr

    -- Hash functions
    transient-hash-fn  : List Fr → Fr
    persistent-hash-fn : Alignment → List Fr → Fr × Fr

    -- Communications commitment: transient_commit(inputs ++ outputs, randomness)
    transient-commit : List Fr → Fr → Fr

  ----------------------------------------------------------------------
  -- Axioms: the bit-decomposition / encoding / valuation laws relating
  -- `Fr` to its little-endian bit representation.
  ----------------------------------------------------------------------

  field
    ------------------------------------------------------------------
    -- Encoding correctness laws.
    --
    -- The defining properties of the BLS12-381 little-endian scalar
    -- encoding, stated via the pure `bits-to-ℕ`.  Together they say:
    -- `to-le-bits` returns the canonical FR_BITS-wide LE representation,
    -- and `from-le-bits` reads a bit list as an integer and reduces it
    -- modulo the field order.  Every correct implementation satisfies
    -- them.
    ------------------------------------------------------------------

    -- `to-le-bits` produces exactly FR_BITS bits.
    to-le-bits-length : ∀ x → length (to-le-bits x) ≡ FR-BITS

    -- The field order is non-zero (any non-trivial field).  An instance
    -- field, so `_%_ FR-ORDER` resolves in the laws below.
    ⦃ FR-ORDER-nonZero ⦄ : NonZero FR-ORDER

    -- Round trip: `from-le-bits` interprets the bits as an integer and
    -- reduces mod the field order; `to-le-bits` recovers the canonical
    -- representative's bit value.
    from-le-bits-roundtrip : ∀ bs
      → bits-to-ℕ (to-le-bits (from-le-bits bs)) ≡ bits-to-ℕ bs % FR-ORDER

  ------------------------------------------------------------------
  -- Valuation laws: `Fr` is the prime field ℤ/FR-ORDER via the
  -- canonical valuation `valFr = bits-to-ℕ ∘ to-le-bits`.
  ------------------------------------------------------------------

  -- Canonical integer value of a field element (in [0, FR-ORDER) by
  -- `valFr-bound`).
  valFr : Fr → ℕ
  valFr x = bits-to-ℕ (to-le-bits x)

  -- In-range predicate: a LE bit pattern denotes a valid field element
  -- iff its integer value is below the field order.
  BitsInField : List Bool → Set
  BitsInField bs = bits-to-ℕ bs < FR-ORDER

  bitsInField? : ∀ bs → Dec (BitsInField bs)
  bitsInField? bs = bits-to-ℕ bs <? FR-ORDER

  field
    -- The canonical value is in range, and distinct field elements have
    -- distinct canonical values.
    valFr-bound : ∀ x → valFr x < FR-ORDER
    valFr-inj   : ∀ {x y} → valFr x ≡ valFr y → x ≡ y

    -- The valuation is a (multiplicative-unit-preserving) semiring
    -- homomorphism into ℤ/FR-ORDER.
    valFr-1 : valFr 1ᶠ ≡ 1 % FR-ORDER
    valFr-+ : ∀ x y → valFr (x +ᶠ y) ≡ (valFr x + valFr y) % FR-ORDER
    valFr-* : ∀ x y → valFr (x *ᶠ y) ≡ (valFr x * valFr y) % FR-ORDER

    -- BLS12-381: the field order exceeds the top representable bit,
    -- 2^(FR_BITS−1) ≤ |Fr|.  (Numeric fact: |Fr| ≈ 2^254.86.)
    order-lb : 2 ^ (FR-BITS ∸ 1) ≤ FR-ORDER

    -- CAUTION: do NOT add a `div-mod-unique`-style uniqueness axiom here.
    -- With only the bounds `fits-in q (FR-BITS∸bits)`, the value
    -- `q·2^bits + r` can reach 2^FR_BITS − 1 > |Fr|, so two distinct pairs
    -- can be congruent mod |Fr| (e.g. (0,0) and the decomposition of |Fr|
    -- itself) — such an axiom is FALSE for BLS12-381.
