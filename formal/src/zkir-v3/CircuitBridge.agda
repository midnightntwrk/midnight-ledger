{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- The off-circuit / in-circuit bridge core.
--
-- This module collects the definitions that mediate between the
-- off-circuit (preprocess) semantics and the in-circuit (constraint)
-- semantics, independently of the per-instruction faithfulness proofs:
--
--   • per-step constraint extraction over `synth`/`satisfies` (`csOf`
--     and friends) — the per-instruction contribution of an instruction
--     list, and the peeling/projection lemmas the backward driver uses;
--   • the in-circuit witness of a preprocess run (`witness-of`) and the
--     resolution-agreement lemmas (`resolve-agree` /
--     `resolveᶜ-Fr-agree`) through which every per-instruction lemma
--     transports its hypotheses across the boundary;
--   • the single- and two-output binding transports (`⊢-pres`,
--     `⊢ᶠ-pres`, `assign-*`, `⊢all-pres`, …);
--   • constraint-satisfaction monotonicity (`holds-mono`) and its
--     converse lowering (`holds-lower`, with the `Defd` definedness
--     side-condition and the `down-*` reverse transports).
------------------------------------------------------------------------

module zkir-v3.CircuitBridge (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Semantics ⋯
  using (ProofPreimage; State; resolve; resolveᶠ; Mem; ins; step; out1;
         resolve-all-Fr)
open import zkir-v3.SemanticsProperties ⋯
open import zkir-v3.Circuit ⋯
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)

open import Data.Maybe using (Maybe; just; nothing; _>>=_)
open import Data.Bool using (Bool; true; false; if_then_else_)
open import Data.Nat using (ℕ; zero; suc; _+_; _*_; _<?_; _<_; _^_; _∸_)
open import Data.List using (List; []; _∷_; _++_; _∷ʳ_; map)
open import Data.List.Properties using (++-identityʳ; ++-assoc)
open import Data.Product using (_×_; _,_; proj₁; proj₂; ∃)
open import Data.Sum using (inj₁; inj₂)
open import Data.Unit using (⊤; tt)
open import Data.String using () renaming (_≟_ to _≟str_)
open import Data.Maybe.Properties using (just-injective)
open import Function using (case_of_)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; subst)
open import Relation.Nullary using (yes; no; ¬_)
open import Relation.Nullary.Decidable using (isYes)

-- Splitting a satisfied constraint list (inverse of `sat-++`).
sat-++⁻ : ∀ xs {ys w}
  → satisfies-constraints (xs ++ ys) w
  → satisfies-constraints xs w × satisfies-constraints ys w
sat-++⁻ []       h         = tt , h
sat-++⁻ (c ∷ xs) (hc , hr) =
  let (hx , hy) = sat-++⁻ xs hr in (hc , hx) , hy

-- The constraints `synth-instr i` appends to the accumulator `ss` (in
-- emission order).  Single-output instructions push one gate; `impact`
-- pushes one `pi-impact` per input; the transcript-input and output
-- terminators push none.
pushed : Instruction → SynthState → List Constraint
pushed (encode input outputs)              ss = encode-eq input outputs ∷ []
pushed (assert cond)                       ss = non-zero cond ∷ []
pushed (cond-select bit a b o)             ss = select o bit a b ∷ []
pushed (constrain-bits val bits)           ss = in-range val bits ∷ []
pushed (constrain-eq a b)                  ss = eq a b ∷ []
pushed (constrain-to-boolean val)          ss = boolean val ∷ []
pushed (copy val o)                        ss = gate-copy o val ∷ []
pushed (impact guard inputs)               ss =
  impact-constraints (SynthState.next-pi ss) guard inputs
pushed (ec-mul a scalar o)                 ss = ec-mul o a scalar ∷ []
pushed (ec-mul-generator scalar o)         ss = ec-gen o scalar ∷ []
pushed (hash-to-curve inputs o)            ss = h2c o inputs ∷ []
pushed (into-coordinates point (xo , yo))  ss = into-coords xo yo point ∷ []
pushed (from-coordinates (x , y) o)        ss = from-coords o x y ∷ []
pushed (into-bytes32 input o)              ss = into-bytes o input ∷ []
pushed (from-bytes32 bytes val-t o)        ss = from-bytes o bytes ∷ []
pushed (reverse-bytes bytes o)             ss = reverse-bytes o bytes ∷ []
pushed (bytes32-into-low-high bytes (lo , hi)) ss =
  bytes-into-low-high lo hi bytes ∷ []
pushed (bytes32-from-low-high (lo , hi) o) ss = bytes-from-low-high o lo hi ∷ []
pushed (div-mod-power-of-two val bits outs) ss =
  case outs of λ { (q ∷ r ∷ []) → div-mod q r val bits ∷ [] ; _ → [] }
pushed (reconstitute-field d m bits o)     ss = reconstitute o d m bits ∷ []
pushed (transient-hash inputs o)           ss = poseidon o inputs ∷ []
pushed (persistent-hash al inputs o)       ss = sha256 o al inputs ∷ []
pushed (keccak256 al inputs o)             ss = keccak o al inputs ∷ []
pushed (test-eq a b o)                     ss = test-eq o a b ∷ []
pushed (add a b o)                         ss = gate-add o a b ∷ []
pushed (mul a b o)                         ss = gate-mul o a b ∷ []
pushed (neg a o)                           ss = gate-neg o a ∷ []
pushed (inv a o)                           ss = gate-inv o a ∷ []
pushed (not a o)                           ss = is-not o a ∷ []
pushed (less-than a b bits o)              ss = less-than o a b bits ∷ []
pushed (jubjub-scalar-from-native a o)     ss = scalar-from-native o a ∷ []
pushed (public-input guard val-t o)        ss = []
pushed (private-input guard val-t o)       ss = []
pushed (circuit-output vals)               ss = []

-- `synth-instr` appends exactly `pushed i ss` to the constraint list.
push-cs : ∀ i ss
  → SynthState.constraints (synth-instr i ss)
      ≡ SynthState.constraints ss ++ pushed i ss
push-cs (encode input outputs)              ss = refl
push-cs (assert cond)                       ss = refl
push-cs (cond-select bit a b o)             ss = refl
push-cs (constrain-bits val bits)           ss = refl
push-cs (constrain-eq a b)                  ss = refl
push-cs (constrain-to-boolean val)          ss = refl
push-cs (copy val o)                        ss = refl
push-cs (impact guard inputs)               ss = refl
push-cs (ec-mul a scalar o)                 ss = refl
push-cs (ec-mul-generator scalar o)         ss = refl
push-cs (hash-to-curve inputs o)            ss = refl
push-cs (into-coordinates point (xo , yo))  ss = refl
push-cs (from-coordinates (x , y) o)        ss = refl
push-cs (into-bytes32 input o)              ss = refl
push-cs (from-bytes32 bytes val-t o)        ss = refl
push-cs (reverse-bytes bytes o)             ss = refl
push-cs (bytes32-into-low-high bytes (lo , hi)) ss = refl
push-cs (bytes32-from-low-high (lo , hi) o) ss = refl
push-cs (div-mod-power-of-two val bits (q ∷ r ∷ [])) ss = refl
push-cs (div-mod-power-of-two val bits [])           ss = sym (++-identityʳ _)
push-cs (div-mod-power-of-two val bits (_ ∷ []))     ss = sym (++-identityʳ _)
push-cs (div-mod-power-of-two val bits (_ ∷ _ ∷ _ ∷ _)) ss = sym (++-identityʳ _)
push-cs (reconstitute-field d m bits o)     ss = refl
push-cs (transient-hash inputs o)           ss = refl
push-cs (persistent-hash al inputs o)       ss = refl
push-cs (keccak256 al inputs o)             ss = refl
push-cs (test-eq a b o)                     ss = refl
push-cs (add a b o)                         ss = refl
push-cs (mul a b o)                         ss = refl
push-cs (neg a o)                           ss = refl
push-cs (inv a o)                           ss = refl
push-cs (not a o)                           ss = refl
push-cs (less-than a b bits o)              ss = refl
push-cs (jubjub-scalar-from-native a o)     ss = refl
push-cs (public-input guard val-t o)        ss = sym (++-identityʳ _)
push-cs (private-input guard val-t o)       ss = sym (++-identityʳ _)
push-cs (circuit-output vals)               ss = sym (++-identityʳ _)

-- The constraints an instruction list contributes to the accumulator.
csOf : List Instruction → SynthState → List Constraint
csOf []       ss = []
csOf (i ∷ is) ss = pushed i ss ++ csOf is (synth-instr i ss)

-- `synth-instrs` grows the constraint list by exactly `csOf is ss`.
synth-cs-acc : ∀ is ss
  → SynthState.constraints (synth-instrs is ss)
      ≡ SynthState.constraints ss ++ csOf is ss
synth-cs-acc []       ss = sym (++-identityʳ _)
synth-cs-acc (i ∷ is) ss =
  trans (synth-cs-acc is (synth-instr i ss))
    (trans (cong (_++ csOf is (synth-instr i ss)) (push-cs i ss))
           (++-assoc (SynthState.constraints ss) (pushed i ss)
                     (csOf is (synth-instr i ss))))

-- Peel the head instruction's contributed constraints off `csOf` — the
-- per-step interface the backward driver consumes.
csOf-peel : ∀ i is ss {w}
  → satisfies-constraints (csOf (i ∷ is) ss) w
  → satisfies-constraints (pushed i ss) w
    × satisfies-constraints (csOf is (synth-instr i ss)) w
csOf-peel i is ss h = sat-++⁻ (pushed i ss) h

-- The initial synth accumulators: the PI preamble is the input-binding
-- entry (`ss₁`), plus the commitment entry when has-comm (`ss₂`).
ss₁ ss₂ : SynthState
ss₁ = mk-synth (pi-binding 0 ∷ []) 1 []
ss₂ = mk-synth (pi-binding 0 ∷ []) 2 []

-- Top-level entry (no-comm): the source's contributed constraints all hold
-- at the witness, after dropping the leading input-binding PI entry.
csOf-from-sat-false : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ false
  → satisfies (synth S) w
  → satisfies-constraints (csOf (IrSource.instructions S) ss₁) w
csOf-from-sat-false {S} {w} hc sat
  with IrSource.do-communications-commitment S | hc
... | false | refl =
  let cok : satisfies-constraints
              (SynthState.constraints
                (synth-instrs (IrSource.instructions S) ss₁)) w
      cok = satisfies.constraint-ok sat
      cok′ : satisfies-constraints
               (SynthState.constraints ss₁
                 ++ csOf (IrSource.instructions S) ss₁) w
      cok′ = subst (λ z → satisfies-constraints z w)
               (synth-cs-acc (IrSource.instructions S) ss₁) cok
      (_ , rest) = cok′
  in rest

-- Top-level entry (comm): the source's constraint list, after the
-- `synth-cs-acc`/`++-assoc` reshaping, is `pi-binding 0` ∷ the source's
-- contributed constraints ∷ the trailing `comm`.  Split it into all three:
-- the leading input-binding PI entry, the middle contributed constraints,
-- and the trailing comm-commitment constraint.
synth-true-split : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ true
  → satisfies (synth S) w
  → holds w (pi-binding 0)
  × satisfies-constraints (csOf (IrSource.instructions S) ss₂) w
  × holds w (comm (input-operands (IrSource.inputs S))
              (SynthState.output-ops
                (synth-instrs (IrSource.instructions S) ss₂)))
synth-true-split {S} {w} hc sat
  with IrSource.do-communications-commitment S | hc
... | true | refl =
  let ins  = input-operands (IrSource.inputs S)
      st   = synth-instrs (IrSource.instructions S) ss₂
      outs = SynthState.output-ops st
      cok  : satisfies-constraints
               (SynthState.constraints st ∷ʳ comm ins outs) w
      cok  = satisfies.constraint-ok sat
      eqc  : SynthState.constraints st ∷ʳ comm ins outs
             ≡ pi-binding 0
                 ∷ (csOf (IrSource.instructions S) ss₂ ++ (comm ins outs ∷ []))
      eqc  = trans (cong (_++ (comm ins outs ∷ []))
                         (synth-cs-acc (IrSource.instructions S) ss₂))
                   (++-assoc (pi-binding 0 ∷ [])
                             (csOf (IrSource.instructions S) ss₂)
                             (comm ins outs ∷ []))
      cok′ : satisfies-constraints
               (pi-binding 0
                 ∷ (csOf (IrSource.instructions S) ss₂
                     ++ (comm ins outs ∷ []))) w
      cok′ = subst (λ z → satisfies-constraints z w) eqc cok
      (mid , commHold , _) =
        sat-++⁻ (csOf (IrSource.instructions S) ss₂) (proj₂ cok′)
  in proj₁ cok′ , mid , commHold

-- Drop the leading input-binding entry and the trailing comm-commitment
-- constraint, keeping the source's contributed constraints.
csOf-from-sat-true : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ true
  → satisfies (synth S) w
  → satisfies-constraints (csOf (IrSource.instructions S) ss₂) w
csOf-from-sat-true {S} {w} hc sat =
  proj₁ (proj₂ (synth-true-split {S} {w} hc sat))

------------------------------------------------------------------------
-- The in-circuit witness of a preprocess run.
--
-- The in-circuit `memory` is the preprocess `memory`; the public-input
-- vector is the one the run produced; the commitment randomness is the
-- second component of the preimage's commitment (present iff the circuit
-- has a communications commitment).
------------------------------------------------------------------------

comm-rand-of : Maybe (Fr × Fr) → Maybe Fr
comm-rand-of (just (_ , r)) = just r
comm-rand-of nothing        = nothing

witness-of : ProofPreimage → State → CircuitWitness
witness-of P st =
  mk-witness (State.mem st) (State.pis st)
             (comm-rand-of (ProofPreimage.comm-commitment P))

------------------------------------------------------------------------
-- Resolution agreement.
--
-- An operand resolves identically under the off-circuit `resolve` (over
-- the preprocess memory) and the in-circuit `resolveᶜ` (over the witness
-- built from the same state) — definitionally, since the witness's
-- `assign` is precisely that memory.  These are the bridge through which
-- every per-instruction faithfulness lemma transports its hypotheses.
------------------------------------------------------------------------

resolve-agree : ∀ P st op
  → resolveᶜ (witness-of P st) op ≡ resolve (State.mem st) op
resolve-agree P st (var id) = refl
resolve-agree P st (imm x)  = refl

resolveᶜ-Fr-agree : ∀ P st op
  → resolveᶜ-Fr (witness-of P st) op ≡ resolveᶠ (State.mem st) op
resolveᶜ-Fr-agree P st (imm x)  = refl
resolveᶜ-Fr-agree P st (var id) with State.mem st id
... | nothing                    = refl
... | just (val-native _)        = refl
... | just (val-bytes32 _)       = refl
... | just (val-jubjub-point _)  = refl
... | just (val-jubjub-scalar _) = refl
... | just (val-secp-point _)    = refl
... | just (val-secp-base _)     = refl
... | just (val-secp-scalar _)   = refl

------------------------------------------------------------------------
-- Transport a resolution at `st` to one against the post-step witness.
--
-- After a single output binding `st' = out1 st out v`, an input operand
-- that resolved in `st` resolves identically against `witness-of P st'`
-- (freshness of `out` rules out aliasing).
------------------------------------------------------------------------

⊢-pres : ∀ P st (out : Identifier) v op {w}
  → State.mem st out ≡ nothing
  → resolve (State.mem st) op ≡ just w
  → resolveᶜ (witness-of P (out1 st out v)) op ≡ just w
⊢-pres P st out v op fresh r =
  trans (resolve-agree P (out1 st out v) op)
        (resolve-pres out v (State.mem st) op fresh r)

⊢ᶠ-pres : ∀ P st (out : Identifier) v op {x}
  → State.mem st out ≡ nothing
  → resolveᶠ (State.mem st) op ≡ just x
  → resolveᶜ-Fr (witness-of P (out1 st out v)) op ≡ just x
⊢ᶠ-pres P st out v op fresh r =
  trans (resolveᶜ-Fr-agree P (out1 st out v) op)
        (resolveᶠ-pres out v (State.mem st) op fresh r)

-- The freshly-bound cell `out` holds `v` in the post-step witness.
assign-here : ∀ P st (out : Identifier) v
  → CircuitWitness.assign (witness-of P (out1 st out v)) out ≡ just v
assign-here P st out v = ins-here out v (State.mem st)

-- List-resolution agreement across the off/in-circuit boundary.
resolve-all-agree : ∀ P st ops
  → resolveᶜ-all-Fr (witness-of P st) ops ≡ resolve-all-Fr (State.mem st) ops
resolve-all-agree P st []         = refl
resolve-all-agree P st (op ∷ ops)
  rewrite resolveᶜ-Fr-agree P st op
        | resolve-all-agree P st ops = refl

-- List resolution is preserved by a fresh output binding (each element
-- is preserved by `resolveᶠ-pres`).
resolve-all-pres : ∀ (out : Identifier) v (m : Mem) ops {frs}
  → m out ≡ nothing
  → resolve-all-Fr m ops ≡ just frs
  → resolve-all-Fr (ins out v m) ops ≡ just frs
resolve-all-pres out v m []         _     r = r
resolve-all-pres out v m (op ∷ ops) fresh r
  with resolveᶠ m op in eqop
... | just x
        rewrite resolveᶠ-pres out v m op fresh eqop
  with resolve-all-Fr m ops in eqrest
...   | just xs rewrite resolve-all-pres out v m ops fresh eqrest = r

------------------------------------------------------------------------
-- Two-output bindings  (st' = out1 (out1 st id₁ v₁) id₂ v₂).
------------------------------------------------------------------------

-- An input operand survives two distinct fresh bindings.
⊢-pres2 : ∀ P st (id₁ : Identifier) v₁ (id₂ : Identifier) v₂ op {w}
  → State.mem st id₁ ≡ nothing
  → State.mem st id₂ ≡ nothing
  → ¬ (id₂ ≡ id₁)
  → resolve (State.mem st) op ≡ just w
  → resolveᶜ (witness-of P (out1 (out1 st id₁ v₁) id₂ v₂)) op ≡ just w
⊢-pres2 P st id₁ v₁ id₂ v₂ op fresh₁ fresh₂ id₂≢id₁ r =
  trans (resolve-agree P (out1 (out1 st id₁ v₁) id₂ v₂) op)
    (resolve-pres id₂ v₂ (ins id₁ v₁ (State.mem st)) op
      (trans (ins-other v₁ (State.mem st) id₂≢id₁) fresh₂)
      (resolve-pres id₁ v₁ (State.mem st) op fresh₁ r))

-- The first (inner) of two distinct bindings is still readable.
assign-inner : ∀ P st (id₁ : Identifier) v₁ (id₂ : Identifier) v₂
  → ¬ (id₁ ≡ id₂)
  → CircuitWitness.assign (witness-of P (out1 (out1 st id₁ v₁) id₂ v₂)) id₁
      ≡ just v₁
assign-inner P st id₁ v₁ id₂ v₂ id₁≢id₂ =
  trans (ins-other v₂ (ins id₁ v₁ (State.mem st)) id₁≢id₂)
        (ins-here id₁ v₁ (State.mem st))

-- The second (outer) of two bindings.
assign-outer : ∀ P st (id₁ : Identifier) v₁ (id₂ : Identifier) v₂
  → CircuitWitness.assign (witness-of P (out1 (out1 st id₁ v₁) id₂ v₂)) id₂
      ≡ just v₂
assign-outer P st id₁ v₁ id₂ v₂ =
  ins-here id₂ v₂ (ins id₁ v₁ (State.mem st))

-- Native-resolution survives two distinct fresh bindings.
⊢ᶠ-pres2 : ∀ P st (id₁ : Identifier) v₁ (id₂ : Identifier) v₂ op {x}
  → State.mem st id₁ ≡ nothing
  → State.mem st id₂ ≡ nothing
  → ¬ (id₂ ≡ id₁)
  → resolveᶠ (State.mem st) op ≡ just x
  → resolveᶜ-Fr (witness-of P (out1 (out1 st id₁ v₁) id₂ v₂)) op ≡ just x
⊢ᶠ-pres2 P st id₁ v₁ id₂ v₂ op fresh₁ fresh₂ id₂≢id₁ r =
  trans (resolveᶜ-Fr-agree P (out1 (out1 st id₁ v₁) id₂ v₂) op)
    (resolveᶠ-pres id₂ v₂ (ins id₁ v₁ (State.mem st)) op
      (trans (ins-other v₁ (State.mem st) id₂≢id₁) fresh₂)
      (resolveᶠ-pres id₁ v₁ (State.mem st) op fresh₁ r))

-- Transport list resolution to the post-step witness.
⊢all-pres : ∀ P st (out : Identifier) v ops {frs}
  → State.mem st out ≡ nothing
  → resolve-all-Fr (State.mem st) ops ≡ just frs
  → resolveᶜ-all-Fr (witness-of P (out1 st out v)) ops ≡ just frs
⊢all-pres P st out v ops fresh r =
  trans (resolve-all-agree P (out1 st out v) ops)
        (resolve-all-pres out v (State.mem st) ops fresh r)

-- List resolution survives two distinct fresh bindings.
⊢all-pres2 : ∀ P st (id₁ : Identifier) v₁ (id₂ : Identifier) v₂ ops {frs}
  → State.mem st id₁ ≡ nothing
  → State.mem st id₂ ≡ nothing
  → ¬ (id₂ ≡ id₁)
  → resolve-all-Fr (State.mem st) ops ≡ just frs
  → resolveᶜ-all-Fr (witness-of P (out1 (out1 st id₁ v₁) id₂ v₂)) ops
      ≡ just frs
⊢all-pres2 P st id₁ v₁ id₂ v₂ ops fresh₁ fresh₂ id₂≢id₁ r =
  trans (resolve-all-agree P (out1 (out1 st id₁ v₁) id₂ v₂) ops)
    (resolve-all-pres id₂ v₂ (ins id₁ v₁ (State.mem st)) ops
      (trans (ins-other v₁ (State.mem st) id₂≢id₁) fresh₂)
      (resolve-all-pres id₁ v₁ (State.mem st) ops fresh₁ r))

------------------------------------------------------------------------
-- Memory and public-input monotonicity.
--
-- A single step only *extends* the named store (inserting fresh keys)
-- and *appends* to the public-input vector.  A constraint that holds at
-- the immediate post-step witness therefore still holds at any later
-- witness — the mechanism by which the local forward lemmas lift to the
-- program's final witness.
------------------------------------------------------------------------

-- A present public-input entry survives extension of the vector.
pi-lookup-++ : ∀ xs {i v} zs
  → pi-lookup xs i ≡ just v → pi-lookup (xs ++ zs) i ≡ just v
pi-lookup-++ []       zs p = case p of λ ()
pi-lookup-++ (x ∷ xs) {zero}  zs p = p
pi-lookup-++ (x ∷ xs) {suc i} zs p = pi-lookup-++ xs zs p

pi-lookup-mono : ∀ {xs ys i v}
  → xs ≼ ys → pi-lookup xs i ≡ just v → pi-lookup ys i ≡ just v
pi-lookup-mono {xs} (zs , refl) p = pi-lookup-++ xs zs p

-- Resolution is monotone in the store.
resolveᶜ-mono : ∀ {m m′ pis pis′ cr cr′} op {v}
  → m ⊑ m′
  → resolveᶜ (mk-witness m pis cr) op ≡ just v
  → resolveᶜ (mk-witness m′ pis′ cr′) op ≡ just v
resolveᶜ-mono (var id) sub p = sub p
resolveᶜ-mono (imm x)  sub p = p

resolveᶜ-Fr-inv : ∀ w op {x}
  → resolveᶜ-Fr w op ≡ just x → resolveᶜ w op ≡ just (val-native x)
resolveᶜ-Fr-inv w op r with resolveᶜ w op
... | just (val-native _) with refl ← r = refl

resolveᶜ-Fr-intro : ∀ w op {x}
  → resolveᶜ w op ≡ just (val-native x) → resolveᶜ-Fr w op ≡ just x
resolveᶜ-Fr-intro w op r rewrite r = refl

resolveᶜ-Fr-mono : ∀ {m m′ pis pis′ cr cr′} op {x}
  → m ⊑ m′
  → resolveᶜ-Fr (mk-witness m pis cr) op ≡ just x
  → resolveᶜ-Fr (mk-witness m′ pis′ cr′) op ≡ just x
resolveᶜ-Fr-mono {m} {m′} {pis} {pis′} {cr} {cr′} op sub p =
  resolveᶜ-Fr-intro (mk-witness m′ pis′ cr′) op
    (resolveᶜ-mono op sub (resolveᶜ-Fr-inv (mk-witness m pis cr) op p))

resolveᶜ-all-Fr-mono : ∀ {m m′ pis pis′ cr cr′} ops {frs}
  → m ⊑ m′
  → resolveᶜ-all-Fr (mk-witness m pis cr) ops ≡ just frs
  → resolveᶜ-all-Fr (mk-witness m′ pis′ cr′) ops ≡ just frs
resolveᶜ-all-Fr-mono []         sub p = p
resolveᶜ-all-Fr-mono {m}{m′}{pis}{pis′}{cr}{cr′} (op ∷ ops) sub p
  with resolveᶜ-Fr (mk-witness m pis cr) op in eqop
... | just x
  with resolveᶜ-all-Fr (mk-witness m pis cr) ops in eqrest
...   | just xs
      rewrite resolveᶜ-Fr-mono {m}{m′}{pis}{pis′}{cr}{cr′} op sub eqop
            | resolveᶜ-all-Fr-mono {m}{m′}{pis}{pis′}{cr}{cr′} ops sub eqrest = p

-- The `comm`-commitment preimage (raw encodings) survives extension too.
resolve-encode-mono : ∀ {m m′ pis pis′ cr cr′} ops {vs}
  → m ⊑ m′
  → resolve-encode (mk-witness m pis cr) ops ≡ just vs
  → resolve-encode (mk-witness m′ pis′ cr′) ops ≡ just vs
resolve-encode-mono []         sub p = p
resolve-encode-mono {m}{m′}{pis}{pis′}{cr}{cr′} (op ∷ ops) sub p
  with resolveᶜ (mk-witness m pis cr) op in eqop
... | just v
  with resolve-encode (mk-witness m pis cr) ops in eqrest
...   | just rest
      rewrite resolveᶜ-mono {m}{m′}{pis}{pis′}{cr}{cr′} op sub eqop
            | resolve-encode-mono {m}{m′}{pis}{pis′}{cr}{cr′} ops sub eqrest
            = p

------------------------------------------------------------------------
-- Constraint satisfaction is monotone.
--
-- Extending the named store (`m ⊑ m′`) and the public-input vector
-- (`pis ≼ pis′`), with the commitment randomness unchanged, preserves
-- every constraint: each atom is transported by the corresponding
-- monotonicity lemma above, while the pure-`Set` side-conditions (bit,
-- range, coordinate round-trips, the `select`/`pi-impact` implications)
-- carry over untouched.
------------------------------------------------------------------------

-- `bind-each` (the `encode-eq` output binding) transports cell-by-cell.
bind-each-mono : ∀ {m m′ pis pis′ cr} vs ids
  → m ⊑ m′
  → bind-each (mk-witness m pis cr) vs ids
  → bind-each (mk-witness m′ pis′ cr) vs ids
bind-each-mono []       []         sub be       = be
bind-each-mono (v ∷ vs) (id ∷ ids) sub (b , bs) =
  sub b , bind-each-mono vs ids sub bs
bind-each-mono []       (_ ∷ _)    sub ()
bind-each-mono (_ ∷ _)  []         sub ()

holds-mono : ∀ {m m′ pis pis′ cr} (c : Constraint)
  → m ⊑ m′
  → pis ≼ pis′
  → holds (mk-witness m pis cr) c
  → holds (mk-witness m′ pis′ cr) c

holds-mono (gate-add out a b) sub pre
  (av , bv , ov , ra , rb , ro , rest) =
    av , bv , ov
  , resolveᶜ-mono a sub ra , resolveᶜ-mono b sub rb , sub ro , rest

holds-mono (gate-mul out a b) sub pre
  (av , bv , ov , ra , rb , ro , rest) =
    av , bv , ov
  , resolveᶜ-mono a sub ra , resolveᶜ-mono b sub rb , sub ro , rest

holds-mono (gate-neg out a) sub pre (av , ov , ra , ro , rest) =
    av , ov , resolveᶜ-mono a sub ra , sub ro , rest

holds-mono (gate-inv out a) sub pre (av , ov , ra , ro , rest) =
    av , ov , resolveᶜ-mono a sub ra , sub ro , rest

holds-mono (gate-copy out a) sub pre (av , ra , ro) =
    av , resolveᶜ-mono a sub ra , sub ro

holds-mono (encode-eq input outputs) sub pre (v , ri , be) =
    v , resolveᶜ-mono input sub ri
  , bind-each-mono (map val-native (encodeᵉ v)) outputs sub be


holds-mono (eq a b) sub pre (v , ra , rb) =
    v , resolveᶜ-mono a sub ra , resolveᶜ-mono b sub rb

holds-mono (boolean v) sub pre (x , rv , bit) =
    x , resolveᶜ-Fr-mono v sub rv , bit

holds-mono (non-zero c) sub pre (x , rc , nz) =
    x , resolveᶜ-Fr-mono c sub rc , nz

holds-mono (in-range v bits) sub pre (x , rv , lt) =
    x , resolveᶜ-Fr-mono v sub rv , lt

holds-mono (select out bit a b) sub pre
  (bv , av , bvl , ov , rbit , ra , rb , ro , bitp , imps) =
    bv , av , bvl , ov
  , resolveᶜ-Fr-mono bit sub rbit
  , resolveᶜ-mono a sub ra , resolveᶜ-mono b sub rb
  , sub ro , bitp , imps

holds-mono (test-eq out a b) sub pre (av , bv , eqv , ra , rb , veq , ro) =
    av , bv , eqv
  , resolveᶜ-mono a sub ra , resolveᶜ-mono b sub rb , veq , sub ro

holds-mono (is-not out a) sub pre (x , ra , bit , ro) =
    x , resolveᶜ-Fr-mono a sub ra , bit , sub ro

holds-mono (less-than out a b bits) sub pre
  (x , y , ra , rb , lx , ly , ro) =
    x , y , resolveᶜ-Fr-mono a sub ra , resolveᶜ-Fr-mono b sub rb
  , lx , ly , sub ro

holds-mono (div-mod q r v bits) sub pre (x , rv , rq , rr) =
    x , resolveᶜ-Fr-mono v sub rv , sub rq , sub rr

holds-mono (reconstitute out d m bits) sub pre
  (dv , mv , rd , rm , ld , lm , ro) =
    dv , mv , resolveᶜ-Fr-mono d sub rd , resolveᶜ-Fr-mono m sub rm
  , ld , lm , sub ro

holds-mono (scalar-from-native out a) sub pre (x , ra , ro) =
    x , resolveᶜ-Fr-mono a sub ra , sub ro

holds-mono (poseidon out inputs) sub pre (frs , ri , ro) =
    frs , resolveᶜ-all-Fr-mono inputs sub ri , sub ro

holds-mono (sha256 out alignment inputs) sub pre
  (frs , v , ri , hp , ro) =
    frs , v , resolveᶜ-all-Fr-mono inputs sub ri , hp , sub ro

holds-mono (keccak out alignment inputs) sub pre
  (frs , v , ri , hp , ro) =
    frs , v , resolveᶜ-all-Fr-mono inputs sub ri , hp , sub ro

holds-mono (ec-mul out a scalar) sub pre (inj₁ (p , s , ra , rs , ro)) =
    inj₁ (p , s , resolveᶜ-mono a sub ra , resolveᶜ-mono scalar sub rs , sub ro)
holds-mono (ec-mul out a scalar) sub pre (inj₂ (p , s , ra , rs , ro)) =
    inj₂ (p , s , resolveᶜ-mono a sub ra , resolveᶜ-mono scalar sub rs , sub ro)

holds-mono (ec-gen out scalar) sub pre (inj₁ (s , rs , ro)) =
    inj₁ (s , resolveᶜ-mono scalar sub rs , sub ro)
holds-mono (ec-gen out scalar) sub pre (inj₂ (s , rs , ro)) =
    inj₂ (s , resolveᶜ-mono scalar sub rs , sub ro)

holds-mono (h2c out inputs) sub pre (frs , ri , ro) =
    frs , resolveᶜ-all-Fr-mono inputs sub ri , sub ro

holds-mono (into-coords xo yo point) sub pre
  (inj₁ (p , x , y , rp , cp , rx , ry)) =
    inj₁ (p , x , y , resolveᶜ-mono point sub rp , cp , sub rx , sub ry)
holds-mono (into-coords xo yo point) sub pre
  (inj₂ (p , x , y , rp , cp , rx , ry)) =
    inj₂ (p , x , y , resolveᶜ-mono point sub rp , cp , sub rx , sub ry)

holds-mono (from-coords out x y) sub pre
  (inj₁ (xv , yv , p , rx , ry , fc , ro)) =
    inj₁ (xv , yv , p , resolveᶜ-Fr-mono x sub rx , resolveᶜ-Fr-mono y sub ry
         , fc , sub ro)
holds-mono (from-coords out x y) sub pre
  (inj₂ (xv , yv , p , rx , ry , fc , ro)) =
    inj₂ (xv , yv , p , resolveᶜ-mono x sub rx , resolveᶜ-mono y sub ry
         , fc , sub ro)

holds-mono (into-bytes out a) sub pre (av , ov , ra , ro , rest) =
    av , ov , resolveᶜ-mono a sub ra , sub ro , rest

holds-mono (from-bytes out b) sub pre (bs , rb , inj₁ ro) =
    bs , resolveᶜ-mono b sub rb , inj₁ (sub ro)
holds-mono (from-bytes out b) sub pre (bs , rb , inj₂ (inj₁ ro)) =
    bs , resolveᶜ-mono b sub rb , inj₂ (inj₁ (sub ro))
holds-mono (from-bytes out b) sub pre (bs , rb , inj₂ (inj₂ ro)) =
    bs , resolveᶜ-mono b sub rb , inj₂ (inj₂ (sub ro))

holds-mono (reverse-bytes out b) sub pre (bs , rb , ro) =
    bs , resolveᶜ-mono b sub rb , sub ro

holds-mono (bytes-into-low-high lo hi b) sub pre (bs , l , h , rb , sp , rl , rh) =
    bs , l , h , resolveᶜ-mono b sub rb , sp , sub rl , sub rh

holds-mono (bytes-from-low-high out lo hi) sub pre (l , h , bs , rl , rh , as , ro) =
    l , h , bs , resolveᶜ-Fr-mono lo sub rl , resolveᶜ-Fr-mono hi sub rh
  , as , sub ro

holds-mono (pi-binding entry) sub pre (bv , pl) =
    bv , pi-lookup-mono pre pl

holds-mono (pi-impact entry guard x) sub pre
  (g , xv , pv , rg , rx , bit , pl , imps) =
    g , xv , pv , resolveᶜ-Fr-mono guard sub rg , resolveᶜ-Fr-mono x sub rx
  , bit , pi-lookup-mono pre pl , imps

holds-mono (comm inputs outputs) sub pre
  (ivs , ovs , rv , pv , ri , ro , cr≡ , pl , c≡) =
    ivs , ovs , rv , pv
  , resolve-encode-mono inputs sub ri , resolve-encode-mono outputs sub ro
  , cr≡ , pi-lookup-mono pre pl , c≡

------------------------------------------------------------------------
-- `holds`-lowering  (the converse of `holds-mono`).
--
-- `holds-mono` lifts a constraint from a small witness up to a larger one
-- (more store bindings, a longer PI vector).  Backward faithfulness needs
-- the opposite: to run a step in reverse we must recover the constraint
-- it appended at the *immediate* post-step witness from the constraint
-- holding at the *final* witness.  This is NOT unconditional — a value
-- may resolve at the final witness yet be unbound at the post-step one.
--
-- The side-condition is that every wire the constraint references is
-- already *defined* at the small witness (`Defd`, below): the operands
-- resolved before the step and the outputs were just written.  Under
-- single assignment this holds structurally.  Given definedness at the
-- small witness `w` and the forward sub-map `m ⊑ m′`, each wire resolves
-- at `w` to *some* value which — pushed up by `resolveᶜ-mono` — must
-- equal the value `holds w′` uses; so it resolves at `w` to exactly that
-- value, and the pure side-conditions (bit, range, round-trips, the
-- select/pi-impact implications) carry down untouched.
------------------------------------------------------------------------

-- Reverse transport of an output-cell binding, given the small binding
-- `d`, its upward image `up = sub d`, and the large binding `big`.  The
-- two large facts pin `u₀ ≡ u₁`, so the small binding reads the large
-- value.  Stated generically over the looked-up cell `X` (a `Maybe`
-- value) so no store/witness implicit needs solving — the caller supplies
-- `up = sub d` where the identifier is in scope, sidestepping the `_⊑_`
-- Π-implicit stall, and `X` is fixed by the small binding `d`.
down-out : ∀ {A : Set} {X Y : Maybe A} {u₀ u₁}
  → X ≡ just u₀ → Y ≡ just u₀ → Y ≡ just u₁ → X ≡ just u₁
down-out d up big with just-injective (trans (sym up) big)
... | refl = d

-- Reverse transport of a single resolution: a wire defined at the small
-- witness resolves there to whatever value it resolves to at the large
-- one (the two agree by `⊑`).  Each specialises `down-out` with the
-- matching monotonicity lemma as the upward image.
down-r : ∀ {m m′ pis pis′ cr cr′} op {u₀ u₁}
  → m ⊑ m′
  → resolveᶜ (mk-witness m pis cr) op ≡ just u₀
  → resolveᶜ (mk-witness m′ pis′ cr′) op ≡ just u₁
  → resolveᶜ (mk-witness m pis cr) op ≡ just u₁
down-r {pis′ = pis′}{cr′ = cr′} op sub d big =
  down-out d (resolveᶜ-mono {pis′ = pis′}{cr′ = cr′} op sub d) big

down-fr : ∀ {m m′ pis pis′ cr cr′} op {x₀ x₁}
  → m ⊑ m′
  → resolveᶜ-Fr (mk-witness m pis cr) op ≡ just x₀
  → resolveᶜ-Fr (mk-witness m′ pis′ cr′) op ≡ just x₁
  → resolveᶜ-Fr (mk-witness m pis cr) op ≡ just x₁
down-fr {pis′ = pis′}{cr′ = cr′} op sub d big =
  down-out d (resolveᶜ-Fr-mono {pis′ = pis′}{cr′ = cr′} op sub d) big

down-all : ∀ {m m′ pis pis′ cr cr′} ops {frs₀ frs₁}
  → m ⊑ m′
  → resolveᶜ-all-Fr (mk-witness m pis cr) ops ≡ just frs₀
  → resolveᶜ-all-Fr (mk-witness m′ pis′ cr′) ops ≡ just frs₁
  → resolveᶜ-all-Fr (mk-witness m pis cr) ops ≡ just frs₁
down-all {pis′ = pis′}{cr′ = cr′} ops sub d big =
  down-out d (resolveᶜ-all-Fr-mono {pis′ = pis′}{cr′ = cr′} ops sub d) big

-- Each output identifier is defined at the witness.
AllDefd : CircuitWitness → List Identifier → Set
AllDefd w []         = ⊤
AllDefd w (id ∷ ids) =
  (∃ λ ov → CircuitWitness.assign w id ≡ just ov) × AllDefd w ids

-- Reverse transport of a `bind-each` binding: with each cell defined at
-- the small witness, the large-witness binding lowers cell-by-cell.
down-bind : ∀ {m m′ pis pis′ cr cr′} vs ids
  → m ⊑ m′
  → AllDefd (mk-witness m pis cr) ids
  → bind-each (mk-witness m′ pis′ cr′) vs ids
  → bind-each (mk-witness m pis cr) vs ids
down-bind []       []         sub _              _        = tt
down-bind (v ∷ vs) (id ∷ ids) sub ((ov , d) , ds) (b , bs) =
  down-out d (sub {id} d) b , down-bind vs ids sub ds bs
down-bind []       (_ ∷ _)    sub _              ()
down-bind (_ ∷ _)  []         sub _              ()

-- Definedness at the small witness: every wire the constraint references
-- resolves.  One clause per constructor, mirroring the wires `holds`
-- reads (operands via `resolveᶜ`/`resolveᶜ-Fr`/lists, outputs via
-- `assign`).  The pure side-conditions are omitted — only definedness.
Defd : CircuitWitness → Constraint → Set
Defd w (gate-add out a b) =
    (∃ λ av → resolveᶜ w a ≡ just av) × (∃ λ bv → resolveᶜ w b ≡ just bv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (gate-mul out a b) =
    (∃ λ av → resolveᶜ w a ≡ just av) × (∃ λ bv → resolveᶜ w b ≡ just bv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (gate-neg out a) =
    (∃ λ av → resolveᶜ w a ≡ just av)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (gate-inv out a) =
    (∃ λ av → resolveᶜ w a ≡ just av)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (gate-copy out a) =
    (∃ λ av → resolveᶜ w a ≡ just av)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (encode-eq input outputs) =
    (∃ λ v → resolveᶜ w input ≡ just v)
  × AllDefd w outputs
Defd w (eq a b) =
    (∃ λ av → resolveᶜ w a ≡ just av) × (∃ λ bv → resolveᶜ w b ≡ just bv)
Defd w (boolean v) = ∃ λ x → resolveᶜ-Fr w v ≡ just x
Defd w (non-zero c) = ∃ λ x → resolveᶜ-Fr w c ≡ just x
Defd w (in-range v bits) = ∃ λ x → resolveᶜ-Fr w v ≡ just x
Defd w (select out bit a b) =
    (∃ λ bv → resolveᶜ-Fr w bit ≡ just bv)
  × (∃ λ av → resolveᶜ w a ≡ just av) × (∃ λ bvl → resolveᶜ w b ≡ just bvl)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (test-eq out a b) =
    (∃ λ av → resolveᶜ w a ≡ just av) × (∃ λ bv → resolveᶜ w b ≡ just bv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (is-not out a) =
    (∃ λ x → resolveᶜ-Fr w a ≡ just x)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (less-than out a b bits) =
    (∃ λ x → resolveᶜ-Fr w a ≡ just x) × (∃ λ y → resolveᶜ-Fr w b ≡ just y)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (div-mod q r v bits) =
    (∃ λ x → resolveᶜ-Fr w v ≡ just x)
  × (∃ λ qv → CircuitWitness.assign w q ≡ just qv)
  × (∃ λ rv → CircuitWitness.assign w r ≡ just rv)
Defd w (reconstitute out d m bits) =
    (∃ λ dv → resolveᶜ-Fr w d ≡ just dv)
  × (∃ λ mv → resolveᶜ-Fr w m ≡ just mv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (scalar-from-native out a) =
    (∃ λ x → resolveᶜ-Fr w a ≡ just x)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (poseidon out inputs) =
    (∃ λ frs → resolveᶜ-all-Fr w inputs ≡ just frs)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (sha256 out alignment inputs) =
    (∃ λ frs → resolveᶜ-all-Fr w inputs ≡ just frs)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (keccak out alignment inputs) =
    (∃ λ frs → resolveᶜ-all-Fr w inputs ≡ just frs)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (ec-mul out a scalar) =
    (∃ λ av → resolveᶜ w a ≡ just av)
  × (∃ λ sv → resolveᶜ w scalar ≡ just sv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (ec-gen out scalar) =
    (∃ λ sv → resolveᶜ w scalar ≡ just sv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (h2c out inputs) =
    (∃ λ frs → resolveᶜ-all-Fr w inputs ≡ just frs)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (into-coords xo yo point) =
    (∃ λ pv → resolveᶜ w point ≡ just pv)
  × (∃ λ xv → CircuitWitness.assign w xo ≡ just xv)
  × (∃ λ yv → CircuitWitness.assign w yo ≡ just yv)
Defd w (from-coords out x y) =
    (∃ λ xv → resolveᶜ w x ≡ just xv)
  × (∃ λ yv → resolveᶜ w y ≡ just yv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (into-bytes out a) =
    (∃ λ av → resolveᶜ w a ≡ just av)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (from-bytes out b) =
    (∃ λ bv → resolveᶜ w b ≡ just bv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (reverse-bytes out b) =
    (∃ λ bv → resolveᶜ w b ≡ just bv)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (bytes-into-low-high lo hi b) =
    (∃ λ bv → resolveᶜ w b ≡ just bv)
  × (∃ λ lv → CircuitWitness.assign w lo ≡ just lv)
  × (∃ λ hv → CircuitWitness.assign w hi ≡ just hv)
Defd w (bytes-from-low-high out lo hi) =
    (∃ λ l → resolveᶜ-Fr w lo ≡ just l)
  × (∃ λ h → resolveᶜ-Fr w hi ≡ just h)
  × (∃ λ ov → CircuitWitness.assign w out ≡ just ov)
Defd w (pi-binding entry) =
  ∃ λ bv → pi-lookup (CircuitWitness.pis w) entry ≡ just bv
Defd w (pi-impact entry guard x) =
    (∃ λ g → resolveᶜ-Fr w guard ≡ just g)
  × (∃ λ xv → resolveᶜ-Fr w x ≡ just xv)
  × (∃ λ pv → pi-lookup (CircuitWitness.pis w) entry ≡ just pv)
Defd w (comm inputs outputs) =
    (∃ λ ivs → resolve-encode w inputs ≡ just ivs)
  × (∃ λ ovs → resolve-encode w outputs ≡ just ovs)
  × (∃ λ pv → pi-lookup (CircuitWitness.pis w) 1 ≡ just pv)

-- Reverse transport of a PI entry: an index present at the small vector
-- reads there whatever it reads at the large one (a prefix-preserved
-- entry).
down-pi : ∀ {pis pis′ entry pv₀ pv₁}
  → pis ≼ pis′
  → pi-lookup pis entry ≡ just pv₀
  → pi-lookup pis′ entry ≡ just pv₁
  → pi-lookup pis entry ≡ just pv₁
down-pi pre d big = down-out d (pi-lookup-mono pre d) big

-- Reverse transport of an `encode`-flattened resolution list.
down-enc : ∀ {m m′ pis pis′ cr cr′} ops {vs₀ vs₁}
  → m ⊑ m′
  → resolve-encode (mk-witness m pis cr) ops ≡ just vs₀
  → resolve-encode (mk-witness m′ pis′ cr′) ops ≡ just vs₁
  → resolve-encode (mk-witness m pis cr) ops ≡ just vs₁
down-enc {pis′ = pis′}{cr′ = cr′} ops sub d big =
  down-out d (resolve-encode-mono {pis′ = pis′}{cr′ = cr′} ops sub d) big

-- Lowering: from `holds` at the large witness, plus definedness at the
-- small one, recover `holds` at the small witness.  Structurally dual to
-- `holds-mono`: each resolution is pulled down by the matching `down-*`
-- helper; the pure side-conditions and (for gadget atoms with a fixed
-- output value) the output binding transport unchanged from the large
-- witness once its wire is pinned by definedness.
holds-lower : ∀ {m m′ pis pis′ cr} (c : Constraint)
  → m ⊑ m′
  → pis ≼ pis′
  → Defd (mk-witness m pis cr) c
  → holds (mk-witness m′ pis′ cr) c
  → holds (mk-witness m pis cr) c

holds-lower (gate-add out a b) sub pre
  ((av₀ , da) , (bv₀ , db) , (ov₀ , do′))
  (av , bv , ov , ra , rb , ro , rest) =
    av , bv , ov
  , down-r a sub da ra , down-r b sub db rb , down-out do′ (sub {out} do′) ro , rest

holds-lower (gate-mul out a b) sub pre
  ((av₀ , da) , (bv₀ , db) , (ov₀ , do′))
  (av , bv , ov , ra , rb , ro , rest) =
    av , bv , ov
  , down-r a sub da ra , down-r b sub db rb
  , down-out do′ (sub {out} do′) ro , rest

holds-lower (gate-neg out a) sub pre ((av₀ , da) , (ov₀ , do′))
  (av , ov , ra , ro , rest) =
    av , ov , down-r a sub da ra , down-out do′ (sub {out} do′) ro , rest

holds-lower (gate-inv out a) sub pre ((av₀ , da) , (ov₀ , do′))
  (av , ov , ra , ro , rest) =
    av , ov , down-r a sub da ra , down-out do′ (sub {out} do′) ro , rest

holds-lower (gate-copy out a) sub pre ((av₀ , da) , (ov₀ , do′))
  (av , ra , ro) =
    av , down-r a sub da ra , down-out do′ (sub {out} do′) ro

holds-lower (encode-eq input outputs) sub pre ((v₀ , di) , alldefd)
  (v , ri , be) =
    v , down-r input sub di ri
  , down-bind (map val-native (encodeᵉ v)) outputs sub alldefd be

holds-lower (eq a b) sub pre ((av₀ , da) , (bv₀ , db)) (v , ra , rb) =
    v , down-r a sub da ra , down-r b sub db rb

holds-lower (boolean v) sub pre (x₀ , dv) (x , rv , bit) =
    x , down-fr v sub dv rv , bit

holds-lower (non-zero c) sub pre (x₀ , dc) (x , rc , nz) =
    x , down-fr c sub dc rc , nz

holds-lower (in-range v bits) sub pre (x₀ , dv) (x , rv , lt) =
    x , down-fr v sub dv rv , lt

holds-lower (select out bit a b) sub pre
  ((bv₀ , dbit) , (av₀ , da) , (bvl₀ , db) , (ov₀ , do′))
  (bv , av , bvl , ov , rbit , ra , rb , ro , bitp , imps) =
    bv , av , bvl , ov
  , down-fr bit sub dbit rbit
  , down-r a sub da ra , down-r b sub db rb
  , down-out do′ (sub {out} do′) ro , bitp , imps

holds-lower (test-eq out a b) sub pre
  ((av₀ , da) , (bv₀ , db) , (ov₀ , do′))
  (av , bv , eqv , ra , rb , veq , ro) =
    av , bv , eqv
  , down-r a sub da ra , down-r b sub db rb , veq , down-out do′ (sub {out} do′) ro

holds-lower (is-not out a) sub pre ((x₀ , da) , (ov₀ , do′))
  (x , ra , bit , ro) =
    x , down-fr a sub da ra , bit , down-out do′ (sub {out} do′) ro

holds-lower (less-than out a b bits) sub pre
  ((x₀ , da) , (y₀ , db) , (ov₀ , do′))
  (x , y , ra , rb , lx , ly , ro) =
    x , y , down-fr a sub da ra , down-fr b sub db rb
  , lx , ly , down-out do′ (sub {out} do′) ro

holds-lower (div-mod q r v bits) sub pre
  ((x₀ , dv) , (qv₀ , dq) , (rv₀ , dr))
  (x , rvw , rq , rr) =
    x , down-fr v sub dv rvw
  , down-out dq (sub {q} dq) rq , down-out dr (sub {r} dr) rr

holds-lower (reconstitute out d m bits) sub pre
  ((dv₀ , dd) , (mv₀ , dm) , (ov₀ , do′))
  (dv , mv , rd , rm , ld , lm , ro) =
    dv , mv , down-fr d sub dd rd , down-fr m sub dm rm
  , ld , lm , down-out do′ (sub {out} do′) ro

holds-lower (scalar-from-native out a) sub pre ((x₀ , da) , (ov₀ , do′))
  (x , ra , ro) =
    x , down-fr a sub da ra , down-out do′ (sub {out} do′) ro

holds-lower (poseidon out inputs) sub pre ((frs₀ , di) , (ov₀ , do′))
  (frs , ri , ro) =
    frs , down-all inputs sub di ri , down-out do′ (sub {out} do′) ro

holds-lower (sha256 out alignment inputs) sub pre
  ((frs₀ , di) , (ov₀ , do′))
  (frs , v , ri , hp , ro) =
    frs , v , down-all inputs sub di ri , hp
  , down-out do′ (sub {out} do′) ro

holds-lower (keccak out alignment inputs) sub pre
  ((frs₀ , di) , (ov₀ , do′))
  (frs , v , ri , hp , ro) =
    frs , v , down-all inputs sub di ri , hp
  , down-out do′ (sub {out} do′) ro

holds-lower (ec-mul out a scalar) sub pre
  ((av₀ , da) , (sv₀ , ds) , (ov₀ , do′))
  (inj₁ (p , s , ra , rs , ro)) =
    inj₁ (p , s , down-r a sub da ra , down-r scalar sub ds rs
         , down-out do′ (sub {out} do′) ro)
holds-lower (ec-mul out a scalar) sub pre
  ((av₀ , da) , (sv₀ , ds) , (ov₀ , do′))
  (inj₂ (p , s , ra , rs , ro)) =
    inj₂ (p , s , down-r a sub da ra , down-r scalar sub ds rs
         , down-out do′ (sub {out} do′) ro)

holds-lower (ec-gen out scalar) sub pre ((sv₀ , ds) , (ov₀ , do′))
  (inj₁ (s , rs , ro)) =
    inj₁ (s , down-r scalar sub ds rs , down-out do′ (sub {out} do′) ro)
holds-lower (ec-gen out scalar) sub pre ((sv₀ , ds) , (ov₀ , do′))
  (inj₂ (s , rs , ro)) =
    inj₂ (s , down-r scalar sub ds rs , down-out do′ (sub {out} do′) ro)

holds-lower (h2c out inputs) sub pre ((frs₀ , di) , (ov₀ , do′))
  (frs , ri , ro) =
    frs , down-all inputs sub di ri , down-out do′ (sub {out} do′) ro

holds-lower (into-coords xo yo point) sub pre
  ((pv₀ , dp) , (xv₀ , dx) , (yv₀ , dy))
  (inj₁ (p , x , y , rp , cp , rx , ry)) =
    inj₁ (p , x , y , down-r point sub dp rp , cp
         , down-out dx (sub {xo} dx) rx , down-out dy (sub {yo} dy) ry)
holds-lower (into-coords xo yo point) sub pre
  ((pv₀ , dp) , (xv₀ , dx) , (yv₀ , dy))
  (inj₂ (p , x , y , rp , cp , rx , ry)) =
    inj₂ (p , x , y , down-r point sub dp rp , cp
         , down-out dx (sub {xo} dx) rx , down-out dy (sub {yo} dy) ry)

holds-lower (from-coords out x y) sub pre
  ((xv₀ , dx) , (yv₀ , dy) , (ov₀ , do′))
  (inj₁ (xv , yv , p , rx , ry , fc , ro)) =
    inj₁ (xv , yv , p
         , resolveᶜ-Fr-intro _ x (down-r x sub dx (resolveᶜ-Fr-inv _ x rx))
         , resolveᶜ-Fr-intro _ y (down-r y sub dy (resolveᶜ-Fr-inv _ y ry))
         , fc , down-out do′ (sub {out} do′) ro)
holds-lower (from-coords out x y) sub pre
  ((xv₀ , dx) , (yv₀ , dy) , (ov₀ , do′))
  (inj₂ (xv , yv , p , rx , ry , fc , ro)) =
    inj₂ (xv , yv , p , down-r x sub dx rx , down-r y sub dy ry
         , fc , down-out do′ (sub {out} do′) ro)

holds-lower (into-bytes out a) sub pre ((av₀ , da) , (ov₀ , do′))
  (av , ov , ra , ro , rest) =
    av , ov , down-r a sub da ra , down-out do′ (sub {out} do′) ro , rest

holds-lower (from-bytes out b) sub pre ((bv₀ , db) , (ov₀ , do′))
  (bs , rb , inj₁ ro) =
    bs , down-r b sub db rb , inj₁ (down-out do′ (sub {out} do′) ro)
holds-lower (from-bytes out b) sub pre ((bv₀ , db) , (ov₀ , do′))
  (bs , rb , inj₂ (inj₁ ro)) =
    bs , down-r b sub db rb , inj₂ (inj₁ (down-out do′ (sub {out} do′) ro))
holds-lower (from-bytes out b) sub pre ((bv₀ , db) , (ov₀ , do′))
  (bs , rb , inj₂ (inj₂ ro)) =
    bs , down-r b sub db rb , inj₂ (inj₂ (down-out do′ (sub {out} do′) ro))

holds-lower (reverse-bytes out b) sub pre ((bv₀ , db) , (ov₀ , do′))
  (bs , rb , ro) =
    bs , down-r b sub db rb , down-out do′ (sub {out} do′) ro

holds-lower (bytes-into-low-high lo hi b) sub pre
  ((bv₀ , db) , (lv₀ , dl) , (hv₀ , dh))
  (bs , l , h , rb , sp , rl , rh) =
    bs , l , h , down-r b sub db rb , sp
  , down-out dl (sub {lo} dl) rl , down-out dh (sub {hi} dh) rh

holds-lower (bytes-from-low-high out lo hi) sub pre
  ((l₀ , dl) , (h₀ , dh) , (ov₀ , do′))
  (l , h , bs , rl , rh , as , ro) =
    l , h , bs , down-fr lo sub dl rl , down-fr hi sub dh rh
  , as , down-out do′ (sub {out} do′) ro

holds-lower (pi-binding entry) sub pre (bv₀ , dp) (bv , pl) =
    bv , down-pi pre dp pl

holds-lower (pi-impact entry guard x) sub pre
  ((g₀ , dg) , (xv₀ , dx) , (pv₀ , dp))
  (g , xv , pv , rg , rx , bit , pl , imps) =
    g , xv , pv , down-fr guard sub dg rg , down-fr x sub dx rx
  , bit , down-pi pre dp pl , imps

holds-lower (comm inputs outputs) sub pre
  ((ivs₀ , di) , (ovs₀ , do′) , (pv₀ , dp))
  (ivs , ovs , rv , pv , ri , ro , cr≡ , pl , c≡) =
    ivs , ovs , rv , pv
  , down-enc inputs sub di ri , down-enc outputs sub do′ ro
  , cr≡ , down-pi pre dp pl , c≡
