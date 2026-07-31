{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Types and values of zkir-v3  (ir_types.rs)
--
-- The IR's type set, the off-circuit value domain `IrValue`, the
-- per-type encoding width, and the value→type and default-value maps.
------------------------------------------------------------------------

module zkir-v3.Types (⋯ : _) (open Assumptions ⋯) where

open import Data.Nat using (ℕ)
open import Relation.Binary.Definitions using (DecidableEquality)
open import Relation.Binary.PropositionalEquality using (_≡_; refl)
open import Relation.Nullary using (yes; no)

------------------------------------------------------------------------
-- Types  (ir_types.rs: IrType)
------------------------------------------------------------------------

data IrType : Set where
  native        : IrType   -- Scalar<BLS12-381>
  bytes32       : IrType   -- Bytes<32>
  jubjub-point  : IrType   -- Point<Jubjub>
  jubjub-scalar : IrType   -- Scalar<Jubjub>
  secp-point    : IrType   -- Point<Secp256k1>
  secp-base     : IrType   -- Base<Secp256k1>
  secp-scalar   : IrType   -- Scalar<Secp256k1>

-- Number of raw Fr elements needed to encode a value of this type
-- (ir_types.rs: IrType::encoded_len).
encoded-len : IrType → ℕ
encoded-len native        = 1
encoded-len bytes32       = 2
encoded-len jubjub-point  = 2
encoded-len jubjub-scalar = 1
encoded-len secp-point    = 5
encoded-len secp-base     = 2
encoded-len secp-scalar   = 2

------------------------------------------------------------------------
-- Off-circuit values  (ir_types.rs: IrValue)
--
-- Constructors carry the concrete carrier; they are named distinctly
-- from the `IrType` tags to avoid overloading.
------------------------------------------------------------------------

data IrValue : Set where
  val-native        : Fr           → IrValue
  val-bytes32       : Bytes32      → IrValue
  val-jubjub-point  : JubjubPoint  → IrValue
  val-jubjub-scalar : JubjubScalar → IrValue
  val-secp-point    : SecpPoint    → IrValue
  val-secp-base     : SecpBase     → IrValue
  val-secp-scalar   : SecpScalar   → IrValue

-- The type of a value  (ir_types.rs: IrValue::get_type).
typeof : IrValue → IrType
typeof (val-native        _) = native
typeof (val-bytes32       _) = bytes32
typeof (val-jubjub-point  _) = jubjub-point
typeof (val-jubjub-scalar _) = jubjub-scalar
typeof (val-secp-point    _) = secp-point
typeof (val-secp-base     _) = secp-base
typeof (val-secp-scalar   _) = secp-scalar

-- Decidable equality of types (used by the type checks of `cond-select`
-- and the `output` terminator).
_≟T_ : DecidableEquality IrType
native        ≟T native        = yes refl
native        ≟T bytes32       = no (λ ())
native        ≟T jubjub-point  = no (λ ())
native        ≟T jubjub-scalar = no (λ ())
native        ≟T secp-point    = no (λ ())
native        ≟T secp-base     = no (λ ())
native        ≟T secp-scalar   = no (λ ())
bytes32       ≟T native        = no (λ ())
bytes32       ≟T bytes32       = yes refl
bytes32       ≟T jubjub-point  = no (λ ())
bytes32       ≟T jubjub-scalar = no (λ ())
bytes32       ≟T secp-point    = no (λ ())
bytes32       ≟T secp-base     = no (λ ())
bytes32       ≟T secp-scalar   = no (λ ())
jubjub-point  ≟T native        = no (λ ())
jubjub-point  ≟T bytes32       = no (λ ())
jubjub-point  ≟T jubjub-point  = yes refl
jubjub-point  ≟T jubjub-scalar = no (λ ())
jubjub-point  ≟T secp-point    = no (λ ())
jubjub-point  ≟T secp-base     = no (λ ())
jubjub-point  ≟T secp-scalar   = no (λ ())
jubjub-scalar ≟T native        = no (λ ())
jubjub-scalar ≟T bytes32       = no (λ ())
jubjub-scalar ≟T jubjub-point  = no (λ ())
jubjub-scalar ≟T jubjub-scalar = yes refl
jubjub-scalar ≟T secp-point    = no (λ ())
jubjub-scalar ≟T secp-base     = no (λ ())
jubjub-scalar ≟T secp-scalar   = no (λ ())
secp-point    ≟T native        = no (λ ())
secp-point    ≟T bytes32       = no (λ ())
secp-point    ≟T jubjub-point  = no (λ ())
secp-point    ≟T jubjub-scalar = no (λ ())
secp-point    ≟T secp-point    = yes refl
secp-point    ≟T secp-base     = no (λ ())
secp-point    ≟T secp-scalar   = no (λ ())
secp-base     ≟T native        = no (λ ())
secp-base     ≟T bytes32       = no (λ ())
secp-base     ≟T jubjub-point  = no (λ ())
secp-base     ≟T jubjub-scalar = no (λ ())
secp-base     ≟T secp-point    = no (λ ())
secp-base     ≟T secp-base     = yes refl
secp-base     ≟T secp-scalar   = no (λ ())
secp-scalar   ≟T native        = no (λ ())
secp-scalar   ≟T bytes32       = no (λ ())
secp-scalar   ≟T jubjub-point  = no (λ ())
secp-scalar   ≟T jubjub-scalar = no (λ ())
secp-scalar   ≟T secp-point    = no (λ ())
secp-scalar   ≟T secp-base     = no (λ ())
secp-scalar   ≟T secp-scalar   = yes refl
