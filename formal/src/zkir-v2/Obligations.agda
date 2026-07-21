{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.Obligations (⋯ : _) (open Assumptions ⋯) where

------------------------------------------------------------------------
-- Producer obligations  (spec §6.4)
--
-- Four checkable circuit well-formedness conditions — properties of a
-- circuit that guarantee the preprocess and circuit semantics coincide
-- (the backward direction of P5):
--
--   O0  Operand-index discipline    (every operand index refers to an
--                                    already-allocated cell; `wire-disc`)
--   O1  PiSkip discipline           (structural; coincides with WF3)
--   O2  Boolean-UB freedom          (operands of `assert`, `not`, and
--                                    the bit of `cond-select` lie in {0,1})
--   O3  ReconstituteField           (no field-overflow; subsumes O4
--                                    via the `less-than` constraint)
--
-- Each obligation is presented as a *checker function* `IrSource → Bool`
-- that performs a single linear scan over the instruction list, mirror-
-- ing the spec's algorithmic statement.  We keep the data structures
-- deliberately concrete: lists of indices for the boolean-known set and
-- lists of (index, ℕ) pairs for the bit-bound partial map.  No stdlib
-- set/AVL abstractions.
--
-- `producer-safe` is the conjunction of all four checks.
-- `ObligationsSoundness` threads `T (producer-safe src)` through the
-- program-level induction, supplying the per-instruction obligation
-- evidence the backward proofs require.
------------------------------------------------------------------------

-- `FR-BITS` comes from the `Assumptions` parameter.
open import zkir-v2.FieldProperties ⋯
open import zkir-v2.Syntax ⋯

open import Data.Bool    using (Bool; true; false; _∧_; if_then_else_; T)
open import Data.Bool.Properties using (T-∧)
open import Data.List    using (List; []; _∷_)
open import Data.Maybe   using (Maybe; nothing; just)
open import Data.Nat     using (ℕ; suc; _+_; _∸_; _≟_; _<_; _<?_; _≤_; _≤?_; _≡ᵇ_)
open import Data.Nat.Properties using (≡ᵇ⇒≡)
open import Data.Product using (_×_; _,_)
open import Data.Unit    using (⊤; tt)
open import Data.Empty   using (⊥; ⊥-elim)
open import Function.Bundles using (Equivalence)
open import Relation.Nullary using (Dec; yes; no)
open import Relation.Nullary.Decidable using (_×-dec_)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; subst)

-- Idiomatic decidable membership on `IndexSet = List Index` (= `List ℕ`),
-- from the standard library, instantiated at `ℕ`'s decidable equality.
open import Data.List.Membership.DecPropositional (_≟_) using (_∈_; _∈?_)
-- Decidable `All` for the list-operand wire-discipline predicate.
open import Data.List.Relation.Unary.All using (All; all?)

------------------------------------------------------------------------
-- Index sets and partial maps (small concrete encodings).
------------------------------------------------------------------------

-- Index set: a list of indices.  Membership is the standard-library
-- propositional `_∈_`, decided by `_∈?_` (see the import above).
IndexSet : Set
IndexSet = List Index

insert : Index → IndexSet → IndexSet
insert i js = i ∷ js     -- duplicates harmless; lookup is up to `_≡_`.

-- Partial map ℕ → ℕ as an association list.  First match wins under
-- `lookupᵐ`.  Inserts always shadow, keeping the invariant that older
-- bindings are still in the list but never reachable.
PartialMap : Set
PartialMap = List (Index × ℕ)

-- `lookupᵐ i ps` returns the most-recently-inserted value at `i`, or
-- `nothing` if `i` is not in the map.  Used by the obligation checks
-- below.
lookupᵐ : Index → PartialMap → Maybe ℕ
lookupᵐ _ []             = nothing
lookupᵐ i ((j , n) ∷ ps) = if (i ≡ᵇ j) then just n else lookupᵐ i ps

-- Insert (shadows any existing binding).  Used by every "record" step
-- in the spec.
insertᵐ : Index → ℕ → PartialMap → PartialMap
insertᵐ i n ps = (i , n) ∷ ps

------------------------------------------------------------------------
-- Instruction bookkeeping shape.
--
-- Every instruction contributes a fixed amount of bookkeeping to the
-- circuit-faithfulness development: how many memory cells it pushes
-- (Δmem ∈ {0,1,2}), how many public-input entries it appends
-- (Δpis ∈ {0,1}), and — for the four instructions that read a side
-- channel — which channel.  `InstrShape` names those bookkeeping
-- classes; `shapeView` is the single place that enumerates the
-- instruction constructors and assigns each its shape.
--
-- Bookkeeping tables downstream dispatch on the view instead of
-- re-enumerating the instructions.  The genuine per-instruction
-- mathematics (the faithfulness lemmas, the emitted constraints, the
-- obligation checks) stays keyed on the constructors directly — those
-- are real content, not bookkeeping, and do not factor through a shape.
--
-- `sh-pure0/1/2` push {0,1,2} cells and touch no side channel;
-- `sh-declare` appends one public input; `sh-output`, `sh-skip`,
-- `sh-pub-in`, `sh-priv-in` are the four side-channel instructions.
------------------------------------------------------------------------

data InstrShape : Set where
  sh-pure0 sh-pure1 sh-pure2 sh-declare : InstrShape
  sh-output sh-skip sh-pub-in sh-priv-in : InstrShape

shape-Δmem : InstrShape → ℕ
shape-Δmem sh-pure0   = 0
shape-Δmem sh-pure1   = 1
shape-Δmem sh-pure2   = 2
shape-Δmem sh-declare = 0
shape-Δmem sh-output  = 0
shape-Δmem sh-skip    = 0
shape-Δmem sh-pub-in  = 1
shape-Δmem sh-priv-in = 1

shape-Δpis : InstrShape → ℕ
shape-Δpis sh-declare = 1
shape-Δpis _          = 0

-- The view: for each instruction the shape it belongs to.  The pure
-- constructors leave the instruction abstract (their bookkeeping does
-- not read any operand); the side-channel constructors refine the
-- instruction so a consumer can reach the operand the channel needs.
data ShapeView : Instruction → Set where
  sv-pure0   : ∀ {i}   → ShapeView i
  sv-pure1   : ∀ {i}   → ShapeView i
  sv-pure2   : ∀ {i}   → ShapeView i
  sv-declare : ∀ {v}   → ShapeView (declare-pub-input v)
  sv-output  : ∀ {v}   → ShapeView (output v)
  sv-skip    : ∀ {g c} → ShapeView (pi-skip g c)
  sv-pub-in  : ∀ {g}   → ShapeView (public-input g)
  sv-priv-in : ∀ {g}   → ShapeView (private-input g)

shape-of : ∀ {i} → ShapeView i → InstrShape
shape-of sv-pure0   = sh-pure0
shape-of sv-pure1   = sh-pure1
shape-of sv-pure2   = sh-pure2
shape-of sv-declare = sh-declare
shape-of sv-output  = sh-output
shape-of sv-skip    = sh-skip
shape-of sv-pub-in  = sh-pub-in
shape-of sv-priv-in = sh-priv-in

-- The single 26-way enumeration of the instruction constructors.
shapeView : (i : Instruction) → ShapeView i
shapeView (assert _)                 = sv-pure0
shapeView (constrain-bits _ _)       = sv-pure0
shapeView (constrain-eq _ _)         = sv-pure0
shapeView (constrain-to-boolean _)   = sv-pure0
shapeView (add _ _)                  = sv-pure1
shapeView (mul _ _)                  = sv-pure1
shapeView (neg _)                    = sv-pure1
shapeView (copy _)                   = sv-pure1
shapeView (load-imm _)               = sv-pure1
shapeView (test-eq _ _)              = sv-pure1
shapeView (transient-hash _)         = sv-pure1
shapeView (cond-select _ _ _)        = sv-pure1
shapeView (not _)                    = sv-pure1
shapeView (less-than _ _ _)          = sv-pure1
shapeView (reconstitute-field _ _ _) = sv-pure1
shapeView (ec-add _ _ _ _)           = sv-pure2
shapeView (ec-mul _ _ _)             = sv-pure2
shapeView (ec-mul-generator _)       = sv-pure2
shapeView (hash-to-curve _)          = sv-pure2
shapeView (persistent-hash _ _)      = sv-pure2
shapeView (div-mod-power-of-two _ _) = sv-pure2
shapeView (declare-pub-input _)      = sv-declare
shapeView (output _)                 = sv-output
shapeView (pi-skip _ _)              = sv-skip
shapeView (public-input _)           = sv-pub-in
shapeView (private-input _)          = sv-priv-in

------------------------------------------------------------------------
-- Δmem  (output arity per instruction; matches `circuit-instr`),
-- derived from the shape so it reduces through `shapeView`.
------------------------------------------------------------------------

Δmem : Instruction → ℕ
Δmem i = shape-Δmem (shape-of (shapeView i))

------------------------------------------------------------------------
-- O1  —  PiSkip discipline (WF3, full-bracketing)
--
-- Linear scan tracking `d`, the number of `declare-pub-input`s emitted
-- *since the previous* `pi-skip` (or program start).  Each `pi-skip g n`
-- must cover exactly that group (`n ≡ d`), then resets the counter; after
-- the scan no declares may be left uncovered (`d ≡ 0`).  This is WF3 as
-- stated in spec §3.3 ("no PiSkip may cover more than emitted since the
-- previous one", with the counts summing to the declare total): the two
-- conditions together force `n = d` at every skip and no trailing
-- declares.  It is faithful to the Rust producer, which emits a `PiSkip`
-- "covering" each preceding declare group "as an instruction"
-- (`zkir/src/ir.rs`).  A running-pool scan (`n ≤ pool`, final
-- `pool = 0`) would be strictly weaker — it admits sources whose
-- declared public inputs cannot be reconciled with any verifier
-- transcript (see `StatementSoundness`), so statement soundness fails
-- for them; full bracketing is required.
------------------------------------------------------------------------

-- Instructions that neither declare a public input nor mark a skip
-- group; the `O1-Trace` group counter `d` is unchanged across them.
-- Purely syntactic, so it lives here beside `O1-Trace`.
NotDeclSkip : Instruction → Set
NotDeclSkip (declare-pub-input _) = ⊥
NotDeclSkip (pi-skip _ _)         = ⊥
NotDeclSkip _                     = ⊤

-- A view classifying an instruction as a declare, a skip (exposing its
-- covered count `n`), or "other" (carrying its `NotDeclSkip` witness).
-- The single point where the instruction constructors are enumerated;
-- both `O1-scan` and `o1-scan→trace` dispatch on it, so the bracketing
-- scan and its witness reconstruction stay in lock-step.
data DeclSkipView : Instruction → Set where
  dsv-decl  : ∀ {v}   → DeclSkipView (declare-pub-input v)
  dsv-skip  : ∀ {g n} → DeclSkipView (pi-skip g n)
  dsv-other : ∀ {i}   → NotDeclSkip i → DeclSkipView i

declSkipView : (i : Instruction) → DeclSkipView i
declSkipView (declare-pub-input _)     = dsv-decl
declSkipView (pi-skip _ _)             = dsv-skip
declSkipView (assert _)                = dsv-other tt
declSkipView (cond-select _ _ _)       = dsv-other tt
declSkipView (constrain-bits _ _)      = dsv-other tt
declSkipView (constrain-eq _ _)        = dsv-other tt
declSkipView (constrain-to-boolean _)  = dsv-other tt
declSkipView (copy _)                  = dsv-other tt
declSkipView (ec-add _ _ _ _)          = dsv-other tt
declSkipView (ec-mul _ _ _)            = dsv-other tt
declSkipView (ec-mul-generator _)      = dsv-other tt
declSkipView (hash-to-curve _)         = dsv-other tt
declSkipView (load-imm _)              = dsv-other tt
declSkipView (div-mod-power-of-two _ _)= dsv-other tt
declSkipView (reconstitute-field _ _ _)= dsv-other tt
declSkipView (output _)                = dsv-other tt
declSkipView (transient-hash _)        = dsv-other tt
declSkipView (persistent-hash _ _)     = dsv-other tt
declSkipView (test-eq _ _)             = dsv-other tt
declSkipView (add _ _)                 = dsv-other tt
declSkipView (mul _ _)                 = dsv-other tt
declSkipView (neg _)                   = dsv-other tt
declSkipView (not _)                   = dsv-other tt
declSkipView (less-than _ _ _)         = dsv-other tt
declSkipView (public-input _)          = dsv-other tt
declSkipView (private-input _)         = dsv-other tt

O1-scan : ℕ → List Instruction → Bool
O1-scan d [] = d ≡ᵇ 0
O1-scan d (i ∷ is) with declSkipView i
... | dsv-decl         = O1-scan (suc d) is
... | dsv-skip {n = n} = (n ≡ᵇ d) ∧ O1-scan 0 is
... | dsv-other _      = O1-scan d is

O1 : IrSource → Bool
O1 src = O1-scan 0 (IrSource.instructions src)

-- Witness-bearing form of the scan (parallel to `Wire-Trace`), threaded
-- by the statement-soundness builder.  `O1-Trace d is` certifies that
-- `is` is well-bracketed starting from a group with `d` declares already
-- emitted.
data O1-Trace : ℕ → List Instruction → Set where
  o1-nil   : O1-Trace 0 []
  o1-decl  : ∀ {d v is} → O1-Trace (suc d) is
           → O1-Trace d (declare-pub-input v ∷ is)
  o1-skip  : ∀ {d g n is} → n ≡ d → O1-Trace 0 is
           → O1-Trace d (pi-skip g n ∷ is)
  o1-other : ∀ {d i is} → NotDeclSkip i → O1-Trace d is
           → O1-Trace d (i ∷ is)

-- The PiSkip discipline O1, reflected into its `O1-Trace` witness.
o1-scan→trace : ∀ (is : List Instruction) (d : ℕ)
  → T (O1-scan d is) → O1-Trace d is
o1-scan→trace [] d ok =
  subst (λ k → O1-Trace k []) (sym (≡ᵇ⇒≡ d 0 ok)) o1-nil
o1-scan→trace (i ∷ is) d ok with declSkipView i
... | dsv-decl         = o1-decl (o1-scan→trace is (suc d) ok)
... | dsv-skip {n = n} =
        let (n≡ , rest) = T-∧ .Equivalence.to ok
        in o1-skip (≡ᵇ⇒≡ n d n≡) (o1-scan→trace is 0 rest)
... | dsv-other nds    = o1-other nds (o1-scan→trace is d ok)

-- Peel the residual `O1-Trace` past a non-declare/skip head; the group
-- counter `d` is unchanged.  Consumers with a `NotDeclSkip` head (or a
-- stronger purity predicate that reduces to `⊥` on declare/skip) reuse
-- this single peeler.
o1-tail : ∀ {d i is} → NotDeclSkip i → O1-Trace d (i ∷ is) → O1-Trace d is
o1-tail _  (o1-other _ t) = t
o1-tail nt (o1-decl _)    = ⊥-elim nt
o1-tail nt (o1-skip _ _)  = ⊥-elim nt

------------------------------------------------------------------------
-- O2  —  Boolean-UB freedom
--
-- Linear scan with a `bool-known` set and a wire counter `i`.  Spec is
-- "check obligation, then record": `O2-check` decides the obligation
-- (`Maybe IndexSet`, `nothing` = violated), `O2-step` composes it with
-- the record step and the wire-counter bump (`Maybe (ℕ × IndexSet)`).
--
-- Records (spec §6.4): boolean-producing instructions add `i` (the
-- next wire index) to the set; `ConstrainToBoolean(v)` adds `v` (the
-- *operand*) — a constraint pinning an existing wire's value to {0,1}.
-- The spec's further `ConstrainBits(v, 1)` record case is deliberately
-- dropped (see the comment on `O2-record` below).
--
-- Note (spec): `public-input` and `private-input` outputs are NOT added
-- even when contextually boolean — the producer must `ConstrainToBoolean`.
------------------------------------------------------------------------

-- Is the constant `k` boolean?  Deliberately the constant `false` — a
-- sound *under-approximation*: we may miss boolean-known wires but
-- never falsely claim one is boolean.  (`_≟ᶠ_` is in scope, so the real
-- test is implementable; this checker is the restriction the spec's
-- §6.4 mechanisation caveat documents, under which `load-imm` results
-- are never marked.  Producers mark boolean constants with
-- `constrain-to-boolean` instead.)
is-bool-imm? : Fr → Bool
is-bool-imm? _ = false   -- conservative; see note above.

-- Obligation-check for one instruction.  `i` is the next wire index
-- (only used for "record" cases; the obligation cases only need `bk`).
-- Returns `nothing` if the obligation is violated.
-- Membership obligation: succeed (leaving `bk` unchanged) iff `c ∈ bk`,
-- decided by `_∈?_`.  Factored out so the four obligation-bearing
-- instructions (`assert`, `not`, `cond-select`, guarded `pi-skip`)
-- share one reduct, hence one extraction lemma downstream.
require-∈ : Index → IndexSet → Maybe IndexSet
require-∈ c bk with c ∈? bk
... | yes _ = just bk
... | no  _ = nothing

O2-check : Instruction → IndexSet → Maybe IndexSet
O2-check (assert c)           bk = require-∈ c bk
O2-check (not a)              bk = require-∈ a bk
O2-check (cond-select b _ _)  bk = require-∈ b bk
O2-check (pi-skip (just g) _) bk = require-∈ g bk
O2-check _                    bk = just bk

-- Record-step: extend `bk` based on the instruction's outputs.  `i` is
-- the wire index of the first output.
O2-record : Instruction → ℕ → IndexSet → IndexSet
O2-record (test-eq _ _)        i bk = insert i bk
O2-record (less-than _ _ _)    i bk = insert i bk
O2-record (not _)              i bk = insert i bk
O2-record (load-imm k)         i bk = if is-bool-imm? k then insert i bk else bk
O2-record (copy v)             i bk with v ∈? bk
... | yes _ = insert i bk
... | no  _ = bk
O2-record (cond-select _ a c)  i bk with a ∈? bk | c ∈? bk
... | yes _ | yes _ = insert i bk
... | _     | _     = bk
O2-record (constrain-to-boolean v) _ bk = insert v bk
-- `constrain-bits v 1` morally pins v to {0, 1}, but
-- `Fits-in v 1 ↔ v ∈ {0, 1}` is not in the bit-arithmetic trust base,
-- so `constrain-bits` never adds to bool-known (a sound under-
-- approximation).  Producers must use `constrain-to-boolean` to mark
-- a wire boolean.
O2-record (constrain-bits _ _) _ bk = bk
O2-record _ _ bk = bk

-- One step of the scan.  `nothing` = obligation violated.
O2-step : Instruction → ℕ × IndexSet → Maybe (ℕ × IndexSet)
O2-step instr (i , bk) with O2-check instr bk
... | nothing  = nothing
... | just bk₁ = just (i + Δmem instr , O2-record instr i bk₁)

-- Full scan.
O2-scan : List Instruction → ℕ × IndexSet → Maybe (ℕ × IndexSet)
O2-scan []       acc = just acc
O2-scan (i ∷ is) acc with O2-step i acc
... | nothing  = nothing
... | just acc' = O2-scan is acc'

O2 : IrSource → Bool
O2 src with O2-scan (IrSource.instructions src) (IrSource.num-inputs src , [])
... | just _  = true
... | nothing = false

------------------------------------------------------------------------
-- O3  —  ReconstituteField no-overflow  (also covers O4)
--
-- Linear scan with a `bits-known` partial map and wire counter `i`.
-- `ReconstituteField(d, m, n)` requires:
--   d ∈ dom(bits-known)  ∧  bits-known[d] ≤ FR_BITS - n - 1
--   m ∈ dom(bits-known)  ∧  bits-known[m] ≤ n
-- `LessThan(a, b, n)` (O4 folded in):
--   a, b ∈ dom(bits-known)  ∧  bits-known[a] ≤ n  ∧  bits-known[b] ≤ n
--
-- Records: `ConstrainBits`, `DivModPowerOfTwo`, and `Copy` — the sound
-- restriction of the spec's §6.4 map that this checker implements (the
-- spec's further `TestEq`/`LessThan`/`Not`, `LoadImm`, `CondSelect`,
-- `ReconstituteField` record cases are dropped; see the comment on
-- `O3-record` below and the spec's mechanisation caveat).
------------------------------------------------------------------------

-- The per-instruction O3 obligation as a decidable predicate: the
-- operands' recorded bit-bounds (looked up in `bm`) are small enough
-- to rule out field overflow.  `reconstitute-field`/`less-than` are the
-- only constrained instructions; all others hold trivially.
O3OK : Instruction → PartialMap → Set
O3OK (reconstitute-field d m n) bm with lookupᵐ d bm | lookupᵐ m bm
... | just kd | just km = (kd ≤ (FR-BITS ∸ n ∸ 1)) × (km ≤ n)
... | _       | _       = ⊥
O3OK (less-than a b n) bm with lookupᵐ a bm | lookupᵐ b bm
... | just ka | just kb = (ka ≤ n) × (kb ≤ n)
... | _       | _       = ⊥
O3OK _ _ = ⊤

o3OK? : ∀ instr bm → Dec (O3OK instr bm)
o3OK? (reconstitute-field d m n) bm with lookupᵐ d bm | lookupᵐ m bm
... | just kd | just km = (kd ≤? (FR-BITS ∸ n ∸ 1)) ×-dec (km ≤? n)
... | just _  | nothing = no (λ ())
... | nothing | _       = no (λ ())
o3OK? (less-than a b n) bm with lookupᵐ a bm | lookupᵐ b bm
... | just ka | just kb = (ka ≤? n) ×-dec (kb ≤? n)
... | just _  | nothing = no (λ ())
... | nothing | _       = no (λ ())
o3OK? (assert _)               bm = yes tt
o3OK? (cond-select _ _ _)      bm = yes tt
o3OK? (constrain-bits _ _)     bm = yes tt
o3OK? (constrain-eq _ _)       bm = yes tt
o3OK? (constrain-to-boolean _) bm = yes tt
o3OK? (copy _)                 bm = yes tt
o3OK? (declare-pub-input _)    bm = yes tt
o3OK? (pi-skip _ _)            bm = yes tt
o3OK? (ec-add _ _ _ _)         bm = yes tt
o3OK? (ec-mul _ _ _)           bm = yes tt
o3OK? (ec-mul-generator _)     bm = yes tt
o3OK? (hash-to-curve _)        bm = yes tt
o3OK? (load-imm _)             bm = yes tt
o3OK? (div-mod-power-of-two _ _) bm = yes tt
o3OK? (output _)               bm = yes tt
o3OK? (transient-hash _)       bm = yes tt
o3OK? (persistent-hash _ _)    bm = yes tt
o3OK? (test-eq _ _)            bm = yes tt
o3OK? (add _ _)                bm = yes tt
o3OK? (mul _ _)                bm = yes tt
o3OK? (neg _)                  bm = yes tt
o3OK? (not _)                  bm = yes tt
o3OK? (public-input _)         bm = yes tt
o3OK? (private-input _)        bm = yes tt

-- Record-step.  `i` is the wire index of the first output.
--
-- Some "natural" record entries in the spec (from-bool of
-- test-eq/less-than/not, a bit-length bound for
-- load-imm/reconstitute-field, ka⊔kc for cond-select) would require
-- `fits-in` facts outside the
-- bit-arithmetic trust base, so `O3-record` records only the subset
-- whose justification *is* in-base:
--
--   • `constrain-bits v n`       (premise `Fits-in v n`)
--   • `div-mod-power-of-two _ n` (via `fits-from-le-bits-{take,drop}`)
--   • `copy v`                   (inherits from v)
--
-- The other record cases are no-ops.  Backward soundness for
-- `reconstitute-field` and `less-than` does not use the O3-recorded
-- map — both `*-bwd` lemmas take their `fits-in` premises directly
-- from satisfies-constraints data.  We record with plain `insertᵐ`, so the
-- stored value is exactly `n`, justified by the `r-constrain-bits`
-- premise.
O3-record : Instruction → ℕ → PartialMap → PartialMap
O3-record (constrain-bits v n) _ bm = insertᵐ v n bm
O3-record (div-mod-power-of-two _ n) i bm =
  insertᵐ (suc i) n (insertᵐ i (FR-BITS ∸ n) bm)
O3-record (copy v)             i bm with lookupᵐ v bm
... | just k  = insertᵐ i k bm
... | nothing = bm
O3-record _ _ bm = bm

O3-step : Instruction → ℕ × PartialMap → Maybe (ℕ × PartialMap)
O3-step instr (i , bm) with o3OK? instr bm
... | yes _ = just (i + Δmem instr , O3-record instr i bm)
... | no  _ = nothing

O3-scan : List Instruction → ℕ × PartialMap → Maybe (ℕ × PartialMap)
O3-scan []       acc = just acc
O3-scan (i ∷ is) acc with O3-step i acc
... | nothing  = nothing
... | just acc' = O3-scan is acc'

O3 : IrSource → Bool
O3 src with O3-scan (IrSource.instructions src) (IrSource.num-inputs src , [])
... | just _  = true
... | nothing = false

------------------------------------------------------------------------
-- Wire-discipline (O0)  —  every operand index < nr-wires at emission.
--
-- The spec (§3.3) phrases this as a structural well-formedness invariant
-- producers maintain: when an instruction emits, all index operands must
-- be < the current wire count.  The backward (`satisfies → R-instr`)
-- per-step dispatcher needs this to pull `mem-lookup mem a ≡ just av`
-- back from `mem-lookup (mem ++ suf) a ≡ just av'`.
--
-- We encode it idiomatically as a decidable predicate `WireOK instr n`
-- (each operand index `< n`), with `wireOK?` the decision procedure.  A
-- linear scan tracks the current wire count `n`, requires `WireOK instr
-- n` at each instruction, then bumps `n := n + Δmem instr`.
------------------------------------------------------------------------

-- Guard-operand discipline (Maybe Index).  `nothing` is always OK;
-- `just g` requires `g < n`.
GuardOK : Maybe Index → ℕ → Set
GuardOK nothing  _ = ⊤
GuardOK (just g) n = g < n

guardOK? : ∀ g n → Dec (GuardOK g n)
guardOK? nothing  _ = yes tt
guardOK? (just g) n = g <? n

-- Per-instruction operand-discipline predicate: every wire-index operand
-- of `instr` is `< n` (the current wire count).
WireOK : Instruction → ℕ → Set
WireOK (assert c)                 n = c < n
WireOK (cond-select b a c)        n = (b < n) × (a < n) × (c < n)
WireOK (constrain-bits v _)       n = v < n
WireOK (constrain-eq a b)         n = (a < n) × (b < n)
WireOK (constrain-to-boolean v)   n = v < n
WireOK (copy v)                   n = v < n
WireOK (declare-pub-input v)      n = v < n
WireOK (pi-skip g _)              n = GuardOK g n
WireOK (ec-add ax ay bx by)       n = (ax < n) × (ay < n) × (bx < n) × (by < n)
WireOK (ec-mul ax ay s)           n = (ax < n) × (ay < n) × (s < n)
WireOK (ec-mul-generator s)       n = s < n
WireOK (hash-to-curve is)         n = All (_< n) is
WireOK (load-imm _)               _ = ⊤
WireOK (div-mod-power-of-two v _) n = v < n
WireOK (reconstitute-field d m _) n = (d < n) × (m < n)
WireOK (output v)                 n = v < n
WireOK (transient-hash is)        n = All (_< n) is
WireOK (persistent-hash _ is)     n = All (_< n) is
WireOK (test-eq a b)              n = (a < n) × (b < n)
WireOK (add a b)                  n = (a < n) × (b < n)
WireOK (mul a b)                  n = (a < n) × (b < n)
WireOK (neg a)                    n = a < n
WireOK (not a)                    n = a < n
WireOK (less-than a b _)          n = (a < n) × (b < n)
WireOK (public-input g)           n = GuardOK g n
WireOK (private-input g)          n = GuardOK g n

wireOK? : ∀ instr n → Dec (WireOK instr n)
wireOK? (assert c)                 n = c <? n
wireOK? (cond-select b a c)        n = (b <? n) ×-dec (a <? n) ×-dec (c <? n)
wireOK? (constrain-bits v _)       n = v <? n
wireOK? (constrain-eq a b)         n = (a <? n) ×-dec (b <? n)
wireOK? (constrain-to-boolean v)   n = v <? n
wireOK? (copy v)                   n = v <? n
wireOK? (declare-pub-input v)      n = v <? n
wireOK? (pi-skip g _)              n = guardOK? g n
wireOK? (ec-add ax ay bx by)       n = (ax <? n) ×-dec (ay <? n) ×-dec (bx <? n) ×-dec (by <? n)
wireOK? (ec-mul ax ay s)           n = (ax <? n) ×-dec (ay <? n) ×-dec (s <? n)
wireOK? (ec-mul-generator s)       n = s <? n
wireOK? (hash-to-curve is)         n = all? (_<? n) is
wireOK? (load-imm _)               _ = yes tt
wireOK? (div-mod-power-of-two v _) n = v <? n
wireOK? (reconstitute-field d m _) n = (d <? n) ×-dec (m <? n)
wireOK? (output v)                 n = v <? n
wireOK? (transient-hash is)        n = all? (_<? n) is
wireOK? (persistent-hash _ is)     n = all? (_<? n) is
wireOK? (test-eq a b)              n = (a <? n) ×-dec (b <? n)
wireOK? (add a b)                  n = (a <? n) ×-dec (b <? n)
wireOK? (mul a b)                  n = (a <? n) ×-dec (b <? n)
wireOK? (neg a)                    n = a <? n
wireOK? (not a)                    n = a <? n
wireOK? (less-than a b _)          n = (a <? n) ×-dec (b <? n)
wireOK? (public-input g)           n = guardOK? g n
wireOK? (private-input g)          n = guardOK? g n

-- One step: `nothing` = obligation violated; otherwise bump count.
wire-step : Instruction → ℕ → Maybe ℕ
wire-step instr n with wireOK? instr n
... | yes _ = just (n + Δmem instr)
... | no  _ = nothing

wire-scan : List Instruction → ℕ → Maybe ℕ
wire-scan []       n = just n
wire-scan (i ∷ is) n with wire-step i n
... | nothing  = nothing
... | just n'  = wire-scan is n'

wire-disc : IrSource → Bool
wire-disc src with wire-scan (IrSource.instructions src) (IrSource.num-inputs src)
... | just _  = true
... | nothing = false

-- Witness-bearing trace, parallel to O2-Trace / O3-Trace.
data Wire-Trace : List Instruction → ℕ → ℕ → Set where
  wire-done : ∀ {n} → Wire-Trace [] n n
  wire-cons : ∀ {i is n n' final}
    → wire-step i n ≡ just n'
    → Wire-Trace is n' final
    → Wire-Trace (i ∷ is) n final

------------------------------------------------------------------------
-- Producer safety: all four obligations hold.
------------------------------------------------------------------------

producer-safe : IrSource → Bool
producer-safe src = O1 src ∧ O2 src ∧ O3 src ∧ wire-disc src

-- Producer safety reflects the PiSkip discipline into an `O1-Trace`
-- over the whole instruction list.  `producer-safe` leads with `O1`, so
-- its `T` splits off the O1 conjunct directly.
o1-sound : ∀ (src : IrSource) → T (producer-safe src)
  → O1-Trace 0 (IrSource.instructions src)
o1-sound src ps =
  let (o1 , _) = T-∧ .Equivalence.to ps
  in o1-scan→trace (IrSource.instructions src) 0 o1

------------------------------------------------------------------------
-- Witness-bearing predicates (Set form)
--
-- Two ways to use these obligations downstream:
--
--   • As Bool checkers, via `O1`, `O2`, `O3` above (decidable by
--     construction — they are functions to Bool).
--
--   • As witness-bearing predicates that record the trace of
--     `bool-known` / `bits-known` along the scan, used to thread the
--     invariant through the program-level induction.
--
-- The Set forms are inductive predicates that exactly mirror the
-- scans.  Decidability for any specific `IrSource` follows from the
-- corresponding Bool form: `T (O2 src) ↔ O2-Runs src`.  Only the
-- Bool ⇒ Set direction is proved (`O2-bool→Runs` / `O3-bool→Runs`
-- below); the reverse is not needed.
------------------------------------------------------------------------

-- O2 trace: at each step, the obligation check returned `just`.
data O2-Trace : List Instruction → ℕ × IndexSet → ℕ × IndexSet → Set where
  o2-done : ∀ {acc} → O2-Trace [] acc acc
  o2-step : ∀ {i is acc acc' final}
    → O2-step i acc ≡ just acc'
    → O2-Trace is acc' final
    → O2-Trace (i ∷ is) acc final

-- Convenience: existence of a trace.
record O2-Runs (src : IrSource) : Set where
  constructor mk-o2-runs
  field
    final : ℕ × IndexSet
    trace : O2-Trace (IrSource.instructions src)
                     (IrSource.num-inputs src , []) final

-- O3 trace, analogous.
data O3-Trace : List Instruction → ℕ × PartialMap → ℕ × PartialMap → Set where
  o3-done : ∀ {acc} → O3-Trace [] acc acc
  o3-step : ∀ {i is acc acc' final}
    → O3-step i acc ≡ just acc'
    → O3-Trace is acc' final
    → O3-Trace (i ∷ is) acc final

record O3-Runs (src : IrSource) : Set where
  constructor mk-o3-runs
  field
    final : ℕ × PartialMap
    trace : O3-Trace (IrSource.instructions src)
                     (IrSource.num-inputs src , []) final

------------------------------------------------------------------------
-- Bool ⇒ witness extractors: a successful scan reconstructs its trace.
------------------------------------------------------------------------

private
  O2-scan→trace : ∀ is acc {final}
    → O2-scan is acc ≡ just final
    → O2-Trace is acc final
  O2-scan→trace []       acc refl = o2-done
  O2-scan→trace (i ∷ is) acc eq
    with O2-step i acc in step-eq
  ... | just acc' = o2-step step-eq (O2-scan→trace is acc' eq)

  O3-scan→trace : ∀ is acc {final}
    → O3-scan is acc ≡ just final
    → O3-Trace is acc final
  O3-scan→trace []       acc refl = o3-done
  O3-scan→trace (i ∷ is) acc eq
    with O3-step i acc in step-eq
  ... | just acc' = o3-step step-eq (O3-scan→trace is acc' eq)

O2-bool→Runs : ∀ {src} → T (O2 src) → O2-Runs src
O2-bool→Runs {src} eq
  with O2-scan (IrSource.instructions src) (IrSource.num-inputs src , [])
       in scan-eq
... | just final = mk-o2-runs final
                     (O2-scan→trace (IrSource.instructions src)
                                     (IrSource.num-inputs src , [])
                                     scan-eq)

O3-bool→Runs : ∀ {src} → T (O3 src) → O3-Runs src
O3-bool→Runs {src} eq
  with O3-scan (IrSource.instructions src) (IrSource.num-inputs src , [])
       in scan-eq
... | just final = mk-o3-runs final
                     (O3-scan→trace (IrSource.instructions src)
                                     (IrSource.num-inputs src , [])
                                     scan-eq)
