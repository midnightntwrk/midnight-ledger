{-# OPTIONS --safe #-}

------------------------------------------------------------------------
-- Assumptions of the zkir-v3 formalization
--
-- The trust base of the development, collected into a single flat record
-- `Assumptions`, in the style of zkir-v2.  Downstream modules take an
-- `Assumptions` value as a module parameter
-- (`module M (⋯ : _) (open Assumptions ⋯) where`), so the development
-- typechecks under `--safe` with no `postulate`s.
--
-- The assumed laws are only those the faithfulness proofs consume: the
-- field non-triviality `1ᶠ≢0ᶠ` (Group A) and the typed-encoding
-- round-trips (Group C).  Laws are admitted only when a proof needs
-- them.
--
-- Chips are modeled at the level of their *functional contract*: each
-- field below is the relation a `midnight-zk-stdlib` operation
-- guarantees, not its gate-level lowering (see docs/zkir-v3-spec.md §7.0).
------------------------------------------------------------------------

module zkir-v3.Assumptions where

open import Data.Bool    using (Bool; if_then_else_)
open import Data.Fin     using (Fin)
open import Data.List    using (List; []; _∷_)
open import Data.Maybe   using (Maybe; just)
open import Data.Nat     using (ℕ; _+_; _*_)
open import Data.Product using (_×_; _,_; uncurry)
open import Data.Vec     using (Vec)
open import Relation.Binary.Definitions using (DecidableEquality)
open import Relation.Binary.PropositionalEquality using (_≡_)
open import Relation.Nullary using (¬_)

-- Integer value of a little-endian bit list (used by the valuation `valFr`).
bits-to-ℕ : List Bool → ℕ
bits-to-ℕ []       = 0
bits-to-ℕ (b ∷ bs) = (if b then 1 else 0) + 2 * bits-to-ℕ bs

------------------------------------------------------------------------
-- Concrete byte types (ir_types.rs: Bytes32 = [u8; 32]).
--
-- A byte is an element of Fin 256; a `Bytes32` is a length-32 vector of
-- bytes.  These are concrete (they do not depend on the trust base) and
-- are shared by the record fields below and by downstream modules.
------------------------------------------------------------------------

Byte : Set
Byte = Fin 256

Bytes32 : Set
Bytes32 = Vec Byte 32

record Assumptions : Set₁ where

  ----------------------------------------------------------------------
  -- Carrier types.
  ----------------------------------------------------------------------

  field
    Fr           : Set
    -- ^ BLS12-381 scalar field element; the native field and the base
    --   field of Jubjub (transient_crypto::curve::Fr).
    Alignment    : Set
    -- ^ Byte-alignment descriptor (base_crypto::fab::Alignment).
    JubjubPoint  : Set
    -- ^ Point of the Jubjub curve (in its prime-order subgroup).
    JubjubScalar : Set
    -- ^ Element of the Jubjub scalar field.
    SecpPoint    : Set
    -- ^ Point of the Secp256k1 curve (midnight_curves::k256::K256, a
    --   Weierstrass curve of cofactor 1 over a foreign field), including
    --   the identity.
    SecpBase     : Set
    -- ^ Secp256k1 base field element (k256::Fp).
    SecpScalar   : Set
    -- ^ Secp256k1 scalar field element (k256::Fq).

  ----------------------------------------------------------------------
  -- Native field (ZkStdLib arithmetic over Fr).
  ----------------------------------------------------------------------

  field
    0ᶠ 1ᶠ : Fr
    _+ᶠ_ _*ᶠ_ : Fr → Fr → Fr
    -ᶠ_       : Fr → Fr
    -- Inversion is partial: `nothing` exactly on 0ᶠ (Inv errors on zero).
    invᶠ      : Fr → Maybe Fr
    _≟ᶠ_      : DecidableEquality Fr

    -- Bit decomposition.  `FR-BITS` is the number of bits (255), and
    -- `FR-ORDER` the field order; `to-le-bits` yields the canonical
    -- little-endian bits and `from-le-bits` reads bits as an integer
    -- reduced modulo the order.  Used by the bit/field instructions.
    FR-BITS FR-ORDER : ℕ
    to-le-bits       : Fr → List Bool
    from-le-bits     : List Bool → Fr

  ----------------------------------------------------------------------
  -- Jubjub (native embedded curve) gadget contracts.
  ----------------------------------------------------------------------

  field
    _+J_ : JubjubPoint → JubjubPoint → JubjubPoint   -- point addition
    _·J_ : JubjubScalar → JubjubPoint → JubjubPoint  -- scalar mult.
    genJ : JubjubPoint                               -- group generator
    idJ  : JubjubPoint                               -- identity (default)
    negJ : JubjubPoint → JubjubPoint                 -- point negation
    _≟J_ : DecidableEquality JubjubPoint

    -- Affine coordinates.  `coordsJ` is total (every subgroup point has
    -- affine coordinates on the Edwards model); `fromCoordsJ` is partial
    -- (the coordinates must denote a point in the prime-order subgroup).
    --
    -- `fromCoordsJ` is also the *in-circuit* contract of
    -- `from-coordinates` (`Circuit.from-coords`), and this is VERIFIED
    -- faithful (2026-07-24, midnight-circuits 7.2.2): the chip's
    -- `point_from_coordinates` enforces subgroup membership in-circuit by
    -- cofactor-clearing — a fresh point under the curve-equation gate,
    -- constrained multiply-by-8, input coordinates pinned to the result
    -- (edwards_chip.rs:783; spec §9, "Contract strength").
    coordsJ     : JubjubPoint → Fr × Fr
    fromCoordsJ : Fr → Fr → Maybe JubjubPoint

    -- Scalar encoding (canonical 252-bit) and the Native → JubjubScalar
    -- reduction (JubjubScalarFromNative).
    jubjubScalarToFr   : JubjubScalar → Fr
    jubjubScalarFromFr : Fr → Maybe JubjubScalar
    native→jubjubScalar : Fr → JubjubScalar

    -- Hash a sequence of native elements to a Jubjub point.
    hash-to-curve-fn : List Fr → JubjubPoint

  ----------------------------------------------------------------------
  -- Secp256k1 (foreign-field emulated curve) gadget contracts.
  --
  -- The in-circuit side is the `midnight-circuits` foreign-field /
  -- foreign-ECC chips (`AssignedField`, `AssignedForeignPoint`), modeled
  -- here by their functional contracts; the CRT/limb internals stay
  -- inside the trust base (spec §7.0, decision record).
  ----------------------------------------------------------------------

  field
    -- Base field Fp (k256::Fp).  Inversion is partial: `nothing`
    -- exactly on 0 (Inv errors on zero).
    _+ᵇ_ _*ᵇ_ : SecpBase → SecpBase → SecpBase
    -ᵇ_       : SecpBase → SecpBase
    invᵇ      : SecpBase → Maybe SecpBase
    _≟ᵇ_      : DecidableEquality SecpBase

    -- Scalar field Fq (k256::Fq).
    _+ˢ_ _*ˢ_ : SecpScalar → SecpScalar → SecpScalar
    -ˢ_       : SecpScalar → SecpScalar
    invˢ      : SecpScalar → Maybe SecpScalar
    _≟ˢ_      : DecidableEquality SecpScalar

    -- Curve group.
    _+K_ : SecpPoint → SecpPoint → SecpPoint    -- point addition
    negK : SecpPoint → SecpPoint                -- point negation
    _·K_ : SecpScalar → SecpPoint → SecpPoint   -- scalar mult. (msm)
    genK : SecpPoint                            -- group generator
    idK  : SecpPoint                            -- identity (default)
    _≟K_ : DecidableEquality SecpPoint

    -- Affine coordinates.  `coordsK` is partial: `nothing` exactly on
    -- the Weierstrass identity, which has no affine coordinates
    -- (IntoCoordinates errors off-circuit / asserts non-zero
    -- in-circuit).  `fromCoordsK` is partial (on-curve check); the
    -- identity is not constructible from coordinates.
    coordsK     : SecpPoint → Maybe (SecpBase × SecpBase)
    fromCoordsK : SecpBase → SecpBase → Maybe SecpPoint

    -- Limb encodings (`as_public_input` / `from_public_input` of the
    -- assigned foreign types): a 256-bit field element is two native
    -- limbs; a point is x-limbs ++ y-limbs ++ an identity flag (widths
    -- 2 / 2 / 5, ir_types.rs encoded_len).
    --
    -- TRUST NOTE (audited 2026-07-24, midnight-circuits 7.2.2): the
    -- decoders modeled here are the *canonical* partial inverses — they
    -- reject any limb vector the encoder cannot emit, as the Group C
    -- `secp*-sound` laws demand.  The deployed off-circuit
    -- `from_public_input` is laxer: it accepts every in-range limb
    -- vector, silently reducing integers ≥ the foreign modulus
    -- (field_chip.rs:141-160), short-circuits on a set identity flag
    -- without inspecting the coordinate limbs, never rejects flag
    -- values outside {0,1} (weierstrass_chip.rs:249-268), and panics
    -- (rather than erroring) on limbs ≥ 2¹⁹².  The `secp*-round` laws
    -- and both `coordsK` laws were verified TRUE of the same code.
    -- Consequently the development's theorems transfer to the deployed
    -- decoder exactly on canonically-encoded preimage/transcript data —
    -- see spec §9 ("Secp256k1 decode canonicity") and the divergence
    -- review, finding 5.
    secpBase→limbs   : SecpBase → Fr × Fr
    limbs→secpBase   : Fr → Fr → Maybe SecpBase
    secpScalar→limbs : SecpScalar → Fr × Fr
    limbs→secpScalar : Fr → Fr → Maybe SecpScalar
    secpPoint→limbs  : SecpPoint → Fr × Fr × Fr × Fr × Fr
    limbs→secpPoint  : Fr → Fr → Fr → Fr → Fr → Maybe SecpPoint

    -- Bytes32 conversions (IntoBytes32 / FromBytes32 on the foreign
    -- fields): `to_bytes_le` and the *total* little-endian reduction
    -- `from_le_bytes_with_reduction` (non-canonical bytes reduce mod
    -- the field order).
    secpBaseToBytes     : SecpBase → Bytes32
    secpBaseFromBytes   : Bytes32 → SecpBase
    secpScalarToBytes   : SecpScalar → Bytes32
    secpScalarFromBytes : Bytes32 → SecpScalar

  ----------------------------------------------------------------------
  -- Bytes32 ↔ field conversions.
  ----------------------------------------------------------------------

  field
    -- IntoBytes32 / FromBytes32 on Native.  `nativeFromBytes` is total
    -- (non-canonical inputs are reduced modulo the field order).
    nativeToBytes   : Fr → Bytes32
    nativeFromBytes : Bytes32 → Fr

    -- The (low, high) decomposition shared by the Bytes32 encoding and
    -- the Bytes32IntoLowHigh / Bytes32FromLowHigh instructions: `low` is
    -- the first 31 bytes as a field element, `high` the 32nd byte.
    -- `low-high→bytes32` is partial (requires low < 2²⁴⁸ and high < 256).
    bytes32→low-high : Bytes32 → Fr × Fr
    low-high→bytes32 : Fr → Fr → Maybe Bytes32

  ----------------------------------------------------------------------
  -- Hashing and the communications commitment.
  ----------------------------------------------------------------------

  field
    transient-hash-fn  : List Fr → Fr
    -- Persistent (SHA-256) and Keccak-256 hashes parse their inputs under
    -- the alignment, hence partial; each yields a 32-byte digest.
    -- `persistent-hash-fn` models SHA-256 (`Sha256::digest`); `keccak-fn`
    -- models Keccak-256.  Both remain opaque.
    persistent-hash-fn : Alignment → List Fr → Maybe Bytes32
    keccak-fn          : Alignment → List Fr → Maybe Bytes32
    -- Communications commitment: transient_commit(values, randomness).
    transient-commit   : List Fr → Fr → Fr

  ----------------------------------------------------------------------
  -- Field non-triviality (faithfulness Group A).
  --
  -- The only algebraic law assumed about the field operations: `1ᶠ ≢ 0ᶠ`
  -- bridges `assert` (off-circuit cond = 1ᶠ ⟹ in-circuit cond ≠ 0ᶠ) and
  -- the boolean reasoning of `not` / `cond-select` / `impact` guards.
  -- The gate atoms' `holds` clauses relate resolved values through the
  -- operations themselves, so no further field laws are consumed.
  ----------------------------------------------------------------------

  field
    1ᶠ≢0ᶠ : ¬ (1ᶠ ≡ 0ᶠ)

  ----------------------------------------------------------------------
  -- Canonical valuation (faithfulness Group B).
  --
  -- The integer value of a field element, `valFr = bits-to-ℕ ∘
  -- to-le-bits`.  The range/decomposition contracts (`in-range`,
  -- `less-than`, `div-mod`, `reconstitute`) and the reconstitute
  -- no-overflow guard state their numeric bounds directly in terms of
  -- it; no laws about the valuation are assumed — canonicity of the
  -- underlying decompositions is part of the trusted chip contracts
  -- (spec §7.0, decision record).
  ----------------------------------------------------------------------

  valFr : Fr → ℕ
  valFr x = bits-to-ℕ (to-le-bits x)

  ----------------------------------------------------------------------
  -- Typed-encoding round-trips (faithfulness Group C).
  --
  -- The decode/encode primitives for the non-native types are mutually
  -- inverse on valid data: the `*-round` laws give `decode ∘ encode = id`
  -- (for the backward direction / statement-soundness); the `*-sound`
  -- laws give `encode ∘ decode = id` on raw data (used by the
  -- communications-commitment faithfulness case, where the in-circuit
  -- preimage re-encodes the decoded inputs and must recover the raw
  -- input stream).  Native carries no law — its encoding is the identity.
  --
  -- The `secp*-sound` laws pin the decoders to the canonical partial
  -- inverses; the deployed off-circuit decoder is laxer (accepts and
  -- reduces non-canonical limb vectors) — see the TRUST NOTE at the
  -- limb-encoding fields above and spec §9.
  ----------------------------------------------------------------------

  field
    coordsJ-fromCoordsJ : ∀ p → uncurry fromCoordsJ (coordsJ p) ≡ just p
    fromCoordsJ-coordsJ : ∀ {x y p}
      → fromCoordsJ x y ≡ just p → coordsJ p ≡ (x , y)

    jubjubScalar-round : ∀ s
      → jubjubScalarFromFr (jubjubScalarToFr s) ≡ just s
    jubjubScalar-sound : ∀ {f s}
      → jubjubScalarFromFr f ≡ just s → jubjubScalarToFr s ≡ f

    bytes32-round : ∀ b
      → uncurry low-high→bytes32 (bytes32→low-high b) ≡ just b
    bytes32-sound : ∀ {lo hi b}
      → low-high→bytes32 lo hi ≡ just b → bytes32→low-high b ≡ (lo , hi)

    secpBase-round : ∀ x
      → uncurry limbs→secpBase (secpBase→limbs x) ≡ just x
    secpBase-sound : ∀ {l h x}
      → limbs→secpBase l h ≡ just x → secpBase→limbs x ≡ (l , h)

    secpScalar-round : ∀ s
      → uncurry limbs→secpScalar (secpScalar→limbs s) ≡ just s
    secpScalar-sound : ∀ {l h s}
      → limbs→secpScalar l h ≡ just s → secpScalar→limbs s ≡ (l , h)

    secpPoint-round : ∀ {p a b c d e}
      → secpPoint→limbs p ≡ (a , b , c , d , e)
      → limbs→secpPoint a b c d e ≡ just p
    secpPoint-sound : ∀ {a b c d e p}
      → limbs→secpPoint a b c d e ≡ just p
      → secpPoint→limbs p ≡ (a , b , c , d , e)

    -- Coordinate round-trips (both directions partial: `coordsK` on the
    -- identity, `fromCoordsK` off-curve).
    coordsK-fromCoordsK : ∀ {p x y}
      → coordsK p ≡ just (x , y) → fromCoordsK x y ≡ just p
    fromCoordsK-coordsK : ∀ {x y p}
      → fromCoordsK x y ≡ just p → coordsK p ≡ just (x , y)
