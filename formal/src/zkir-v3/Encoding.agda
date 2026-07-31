{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Encoding of typed values to raw Fr  (ir_instructions/encode.rs)
--
-- `encode`/`decode` are the wire format crossing every circuit boundary
-- (inputs, transcripts, outputs, commitment).  They are *derived* from
-- the per-type trust-base primitives rather than assumed, keeping them
-- out of the trusted surface; only the primitives they call are trusted.
------------------------------------------------------------------------

module zkir-v3.Encoding (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯

open import Data.List    using (List; []; _∷_)
open import Data.Maybe   using (Maybe; just; nothing; map)
open import Data.Product using (_,_)

------------------------------------------------------------------------
-- encode : flatten a typed value into raw field elements.
------------------------------------------------------------------------

encode : IrValue → List Fr
encode (val-native x)        = x ∷ []
encode (val-jubjub-scalar s) = jubjubScalarToFr s ∷ []
encode (val-bytes32 b)       = let (lo , hi) = bytes32→low-high b in lo ∷ hi ∷ []
encode (val-jubjub-point p)  = let (x , y)   = coordsJ p          in x  ∷ y  ∷ []
encode (val-secp-base x)     = let (l , h) = secpBase→limbs x   in l ∷ h ∷ []
encode (val-secp-scalar s)   = let (l , h) = secpScalar→limbs s in l ∷ h ∷ []
encode (val-secp-point p)    =
  let (a , b , c , d , e) = secpPoint→limbs p in a ∷ b ∷ c ∷ d ∷ e ∷ []

------------------------------------------------------------------------
-- decode : read raw field elements as a value of the given type.
-- Partial: the wrong number of elements, or an invalid encoding (a
-- non-subgroup point, a non-canonical Bytes32/scalar), yields `nothing`.
------------------------------------------------------------------------

decode : IrType → List Fr → Maybe IrValue
decode native        (x ∷ [])       = just (val-native x)
decode jubjub-scalar (s ∷ [])       = map val-jubjub-scalar (jubjubScalarFromFr s)
decode bytes32       (lo ∷ hi ∷ []) = map val-bytes32       (low-high→bytes32 lo hi)
decode jubjub-point  (x ∷ y ∷ [])   = map val-jubjub-point  (fromCoordsJ x y)
decode secp-base     (l ∷ h ∷ [])   = map val-secp-base     (limbs→secpBase l h)
decode secp-scalar   (l ∷ h ∷ [])   = map val-secp-scalar   (limbs→secpScalar l h)
decode secp-point    (a ∷ b ∷ c ∷ d ∷ e ∷ []) =
  map val-secp-point (limbs→secpPoint a b c d e)
decode _             _              = nothing
