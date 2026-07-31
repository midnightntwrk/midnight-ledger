-- Top-level module for the zkir-v3 formalization.
--
-- Importing this module type-checks the entire zkir-v3 development.  As
-- in zkir-v2, the development is abstract over the trust base: it takes
-- an `Assumptions` value as a module parameter rather than postulating
-- the field/curve/hash primitives, so the whole development typechecks
-- under `--safe` with no `postulate`s.
--
{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

module zkir-v3.Main (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Encoding ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Semantics ⋯
open import zkir-v3.SemanticsProperties ⋯
open import zkir-v3.Circuit ⋯
open import zkir-v3.CircuitBridge ⋯
open import zkir-v3.CircuitFaithfulness ⋯
open import zkir-v3.CircuitBackward ⋯
open import zkir-v3.Obligations ⋯
open import zkir-v3.CircuitProof ⋯
open import zkir-v3.StatementSoundness ⋯
open import zkir-v3.StatementUniqueness ⋯
