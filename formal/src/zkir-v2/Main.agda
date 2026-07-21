-- Top-level module for the zkir-v2 formalization.
--
-- Importing this module type-checks the entire zkir-v2 development:
-- the syntax and semantics of the IR, the constraint-system (circuit)
-- model, the proof obligations, and the soundness / faithfulness results
-- connecting them.
--
-- The development is abstract over the trust base: it takes an
-- `Assumptions` value as a module parameter rather than postulating the
-- field/curve/hash primitives and their axioms.  No concrete BLS12-381
-- instantiation is provided yet, so the whole development typechecks
-- under `--safe`.
{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.Main (⋯ : _) (open Assumptions ⋯) where

open import zkir-v2.FieldProperties ⋯
open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
open import zkir-v2.SemanticsLemmas ⋯
open import zkir-v2.Properties ⋯
open import zkir-v2.Circuit ⋯
open import zkir-v2.CircuitFaithfulness ⋯
open import zkir-v2.CircuitProof ⋯
open import zkir-v2.Encoding ⋯
open import zkir-v2.Obligations ⋯
open import zkir-v2.ObligationsSoundness ⋯
open import zkir-v2.StatementSoundness ⋯
open import zkir-v2.StatementUniqueness ⋯
