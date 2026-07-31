{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Program-level assembly of the circuit-faithfulness results.
--
-- This module ties the static well-formedness passes (`Obligations`), the
-- forward per-instruction lemmas (`CircuitFaithfulness`), and the
-- constraint-extraction interface (`CircuitBridge`) into the headline
-- theorems:
--
--   • `forward-sa`   — forward faithfulness from the decidable static
--     single-assignment check alone;
--   • `backward`     — the spine-driven converse of `forward` (a run is
--     reconstructed from the backward spine and `satisfies`);
--   • `circuit-faithful` — an ⇔ at the canonical witness `witness-of P s`;
--   • `preprocess→BwdWalk` — the spine and `Consumed` hypotheses of
--     `circuit-faithful`, projected from a successful `preprocess` (via
--     the per-instruction `step→bwd` inversions and `run→BwdWalk`).
--
-- The `defd-*` family builds the `Defd` side-conditions `holds-lower`
-- consumes; the backward spine (`BwdStep`/`BwdWalk`/`spine-⊑`) and the
-- `mutual` fold (`bwd-go`/`bwd-cont`) reconstruct the run; the commitment
-- reconstruction (`comm-from-sat`/`comm-tc2`/`run→preprocess`) closes the
-- preprocessing forward-composition.
------------------------------------------------------------------------

module zkir-v3.CircuitProof (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Semantics ⋯
  using ( Mem; State; ProofPreimage; ins; step; init; out1; run
        ; insertMany; decode-inputs; resolve; resolveᶠ
        ; resolve-all-Fr; eval-guard; collectOutputs
        ; default-val; to𝔹; preprocess; valEq?; _≟LFr_ )
open import zkir-v3.SemanticsProperties ⋯
  using ( NoDup; WF-run
        ; _⊑_; _≼_; ⊑-refl; ⊑-trans; ≼-refl; ≼-trans
        ; run-cons; run-shaped; mk-run-shaped; Consumed
        ; run-extends; init-outs; init-decode
        ; run-inv; step-extends; preprocess-walk-consumed )
open import zkir-v3.Circuit ⋯ using (satisfies; synth; satisfies-constraints
  ; holds; resolve-encode; pi-lookup
  ; gate-copy; gate-add; gate-mul; gate-neg; gate-inv; eq; boolean
  ; in-range; select; test-eq; is-not; less-than; reconstitute
  ; scalar-from-native; poseidon; sha256; keccak; ec-mul; ec-gen; h2c
  ; into-coords; from-coords; into-bytes; from-bytes; reverse-bytes
  ; bytes-into-low-high; bytes-from-low-high; div-mod
  ; Constraint; encode-eq; pi-binding; comm; impact-constraints
  ; input-operands; SynthState; synth-instr; synth-instrs; mk-synth)
open import zkir-v3.CircuitFaithfulness ⋯
  using ( forward
        -- commitment reconstruction ingredients (reused from `forward`)
        ; decode-reencode; out-go; init-pi1 )
open import zkir-v3.CircuitBackward ⋯
  using ( copy-bwd; constrain-eq-bwd; add-bwd; mul-bwd; neg-bwd; inv-bwd
        ; test-eq-bwd; assert-bwd; constrain-to-boolean-bwd; not-bwd
        ; cond-select-bwd; constrain-bits-bwd; less-than-bwd
        ; reconstitute-field-bwd; jubjub-scalar-from-native-bwd
        ; ec-mul-bwd; ec-mul-generator-bwd; hash-to-curve-bwd
        ; transient-hash-bwd; into-bytes32-bwd
        ; from-bytes32-native-bwd; from-bytes32-secp-base-bwd
        ; from-bytes32-secp-scalar-bwd
        ; reverse-bytes-bwd
        ; from-coordinates-bwd; bytes32-from-low-high-bwd
        ; into-coordinates-bwd; bytes32-into-low-high-bwd
        ; div-mod-power-of-two-bwd; persistent-hash-bwd; keccak256-bwd
        ; encode-bwd
        ; impact-true-bwd; impact-false-bwd
        ; public-input-active-bwd; public-input-inactive-bwd
        ; private-input-active-bwd; private-input-inactive-bwd
        ; circuit-output-bwd
        ; ⊢-back; out-val )
open import zkir-v3.CircuitBridge ⋯
open import zkir-v3.Obligations ⋯
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)

open import Data.Bool    using (Bool; true; false; if_then_else_)
open import Data.List    using (List; []; _∷_; _++_; _∷ʳ_; map; take; drop; length; concatMap)
open import Data.List.Properties using (++-identityʳ; ++-assoc)
open import Data.Maybe   using (Maybe; just; nothing)
open import Data.Nat     using (ℕ; _^_; _+_; _*_; _∸_; _<_; _<?_)
open import Data.Nat     using () renaming (_≟_ to _≟ℕ_)
open import Data.Product using (_×_; _,_; ∃; ∃₂; proj₁; proj₂)
open import Function.Bundles using (_⇔_; mk⇔)
open import Data.Unit    using (⊤; tt)
open import Function     using (case_of_)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; cong₂; subst)
open import Relation.Nullary using (¬_; yes; no; Dec)
open import Data.String using () renaming (_≟_ to _≟str_)

open import Data.List.Relation.Unary.All using (All; []; _∷_)
open import Data.List.Membership.Propositional using (_∈_)
open import Data.Sum using (_⊎_; inj₁; inj₂)
open import Data.Empty using (⊥)
open import Data.Maybe using () renaming (map to mapᵐ)
open import Data.Maybe.Properties using (just-injective; ≡-dec)

------------------------------------------------------------------------
-- Forward faithfulness, from the static check alone.
--
-- Combining `producer-safe→WF` (discharging the semantic `WF-run`
-- hypothesis) with the input-name distinctness carried by `producer-SA`,
-- `forward` reduces to a check that scans the source once.
------------------------------------------------------------------------

forward-sa : ∀ {S P s st0}
  → producer-SA S
  → init S P ≡ just st0
  → preprocess S P ≡ just s
  → satisfies (synth S) (witness-of P s)
forward-sa {S} {P} {s} {st0} sa ieq peq =
  forward ieq peq
    (producer-safe→WF {S} {P} {st0} sa ieq)
    (proj₁ sa)

------------------------------------------------------------------------
-- Definedness of the emitted constraint at the post-step witness.
--
-- For each instruction whose backward lemma inverts a `holds` premise,
-- `defd-<i>` builds the `Defd` side-condition `holds-lower`
-- needs: every operand the constraint reads resolves at the post-step
-- witness (its resolution at the pre-step memory, shifted across the fresh
-- output binding by `⊢-pres`/`⊢ᶠ-pres`/`⊢all-pres` and their two-output
-- variants) and every output cell is the just-written value
-- (`assign-here`/`assign-inner`/`assign-outer`).  The pre-step operand
-- resolutions come from `optype-resolve`/`optype-resolveᶠ`/`allNat-resolve`
-- (i.e. from `TyEq` + `OpTy`).  Instructions with no `holds` premise
-- (`encode`, `impact`, `public-input`, `private-input`, `circuit-output`)
-- need no `Defd`.
------------------------------------------------------------------------

-- Single-output (post-state `out1 st output v`).

defd-copy : ∀ {P st output val v w}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) val ≡ just w
  → Defd (witness-of P (out1 st output v)) (gate-copy output val)
defd-copy {P} {st} {output} {val} {v} {w} fresh rv =
    (w , ⊢-pres P st output v val fresh rv)
  , (v , assign-here P st output v)

defd-add : ∀ {P st output a b v av bv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) a ≡ just av → resolve (State.mem st) b ≡ just bv
  → Defd (witness-of P (out1 st output v)) (gate-add output a b)
defd-add {P} {st} {output} {a} {b} {v} {av} {bv} fresh ra rb =
    (av , ⊢-pres P st output v a fresh ra)
  , (bv , ⊢-pres P st output v b fresh rb)
  , (v  , assign-here P st output v)

defd-mul : ∀ {P st output a b v av bv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) a ≡ just av → resolve (State.mem st) b ≡ just bv
  → Defd (witness-of P (out1 st output v)) (gate-mul output a b)
defd-mul {P} {st} {output} {a} {b} {v} {av} {bv} fresh ra rb =
    (av , ⊢-pres P st output v a fresh ra)
  , (bv , ⊢-pres P st output v b fresh rb)
  , (v  , assign-here P st output v)

defd-neg : ∀ {P st output a v av}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) a ≡ just av
  → Defd (witness-of P (out1 st output v)) (gate-neg output a)
defd-neg {P} {st} {output} {a} {v} {av} fresh ra =
    (av , ⊢-pres P st output v a fresh ra)
  , (v  , assign-here P st output v)

defd-inv : ∀ {P st output a v av}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) a ≡ just av
  → Defd (witness-of P (out1 st output v)) (gate-inv output a)
defd-inv {P} {st} {output} {a} {v} {av} fresh ra =
    (av , ⊢-pres P st output v a fresh ra)
  , (v  , assign-here P st output v)

defd-test-eq : ∀ {P st output a b v av bv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) a ≡ just av → resolve (State.mem st) b ≡ just bv
  → Defd (witness-of P (out1 st output v)) (test-eq output a b)
defd-test-eq {P} {st} {output} {a} {b} {v} {av} {bv} fresh ra rb =
    (av , ⊢-pres P st output v a fresh ra)
  , (bv , ⊢-pres P st output v b fresh rb)
  , (v  , assign-here P st output v)

defd-scalar-from-native : ∀ {P st output a v x}
  → State.mem st output ≡ nothing
  → resolveᶠ (State.mem st) a ≡ just x
  → Defd (witness-of P (out1 st output v)) (scalar-from-native output a)
defd-scalar-from-native {P} {st} {output} {a} {v} {x} fresh ra =
    (x , ⊢ᶠ-pres P st output v a fresh ra)
  , (v , assign-here P st output v)

defd-ec-mul : ∀ {P st output a scalar v av sv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) a ≡ just av
  → resolve (State.mem st) scalar ≡ just sv
  → Defd (witness-of P (out1 st output v)) (ec-mul output a scalar)
defd-ec-mul {P} {st} {output} {a} {scalar} {v} {av} {sv} fresh ra rs =
    (av , ⊢-pres P st output v a fresh ra)
  , (sv , ⊢-pres P st output v scalar fresh rs)
  , (v  , assign-here P st output v)

defd-ec-gen : ∀ {P st output scalar v sv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) scalar ≡ just sv
  → Defd (witness-of P (out1 st output v)) (ec-gen output scalar)
defd-ec-gen {P} {st} {output} {scalar} {v} {sv} fresh rs =
    (sv , ⊢-pres P st output v scalar fresh rs)
  , (v  , assign-here P st output v)

defd-h2c : ∀ {P st output inputs v frs}
  → State.mem st output ≡ nothing
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → Defd (witness-of P (out1 st output v)) (h2c output inputs)
defd-h2c {P} {st} {output} {inputs} {v} {frs} fresh ri =
    (frs , ⊢all-pres P st output v inputs fresh ri)
  , (v   , assign-here P st output v)

defd-poseidon : ∀ {P st output inputs v frs}
  → State.mem st output ≡ nothing
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → Defd (witness-of P (out1 st output v)) (poseidon output inputs)
defd-poseidon {P} {st} {output} {inputs} {v} {frs} fresh ri =
    (frs , ⊢all-pres P st output v inputs fresh ri)
  , (v   , assign-here P st output v)

defd-into-bytes : ∀ {P st output input v av}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) input ≡ just av
  → Defd (witness-of P (out1 st output v)) (into-bytes output input)
defd-into-bytes {P} {st} {output} {input} {v} {av} fresh ri =
    (av , ⊢-pres P st output v input fresh ri)
  , (v  , assign-here P st output v)

defd-from-bytes : ∀ {P st output bytes v bv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) bytes ≡ just bv
  → Defd (witness-of P (out1 st output v)) (from-bytes output bytes)
defd-from-bytes {P} {st} {output} {bytes} {v} {bv} fresh rb =
    (bv , ⊢-pres P st output v bytes fresh rb)
  , (v  , assign-here P st output v)

defd-reverse-bytes : ∀ {P st output bytes v bv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) bytes ≡ just bv
  → Defd (witness-of P (out1 st output v)) (reverse-bytes output bytes)
defd-reverse-bytes {P} {st} {output} {bytes} {v} {bv} fresh rb =
    (bv , ⊢-pres P st output v bytes fresh rb)
  , (v  , assign-here P st output v)

defd-from-coords : ∀ {P st output x y v xv yv}
  → State.mem st output ≡ nothing
  → resolve (State.mem st) x ≡ just xv → resolve (State.mem st) y ≡ just yv
  → Defd (witness-of P (out1 st output v)) (from-coords output x y)
defd-from-coords {P} {st} {output} {x} {y} {v} {xv} {yv} fresh rx ry =
    (xv , ⊢-pres P st output v x fresh rx)
  , (yv , ⊢-pres P st output v y fresh ry)
  , (v  , assign-here P st output v)

defd-bytes-from-low-high : ∀ {P st output lo hi v l h}
  → State.mem st output ≡ nothing
  → resolveᶠ (State.mem st) lo ≡ just l → resolveᶠ (State.mem st) hi ≡ just h
  → Defd (witness-of P (out1 st output v)) (bytes-from-low-high output lo hi)
defd-bytes-from-low-high {P} {st} {output} {lo} {hi} {v} {l} {h} fresh rl rh =
    (l , ⊢ᶠ-pres P st output v lo fresh rl)
  , (h , ⊢ᶠ-pres P st output v hi fresh rh)
  , (v , assign-here P st output v)

defd-less-than : ∀ {P st output a b bits v x y}
  → State.mem st output ≡ nothing
  → resolveᶠ (State.mem st) a ≡ just x → resolveᶠ (State.mem st) b ≡ just y
  → Defd (witness-of P (out1 st output v)) (less-than output a b bits)
defd-less-than {P} {st} {output} {a} {b} {bits} {v} {x} {y} fresh ra rb =
    (x , ⊢ᶠ-pres P st output v a fresh ra)
  , (y , ⊢ᶠ-pres P st output v b fresh rb)
  , (v , assign-here P st output v)

defd-is-not : ∀ {P st output a v x}
  → State.mem st output ≡ nothing
  → resolveᶠ (State.mem st) a ≡ just x
  → Defd (witness-of P (out1 st output v)) (is-not output a)
defd-is-not {P} {st} {output} {a} {v} {x} fresh ra =
    (x , ⊢ᶠ-pres P st output v a fresh ra)
  , (v , assign-here P st output v)

defd-select : ∀ {P st output bit a b v bf av bv}
  → State.mem st output ≡ nothing
  → resolveᶠ (State.mem st) bit ≡ just bf
  → resolve (State.mem st) a ≡ just av → resolve (State.mem st) b ≡ just bv
  → Defd (witness-of P (out1 st output v)) (select output bit a b)
defd-select {P} {st} {output} {bit} {a} {b} {v} {bf} {av} {bv} fresh rbit ra rb =
    (bf , ⊢ᶠ-pres P st output v bit fresh rbit)
  , (av , ⊢-pres  P st output v a   fresh ra)
  , (bv , ⊢-pres  P st output v b   fresh rb)
  , (v  , assign-here P st output v)

defd-reconstitute : ∀ {P st output d m bits v dv mv}
  → State.mem st output ≡ nothing
  → resolveᶠ (State.mem st) d ≡ just dv → resolveᶠ (State.mem st) m ≡ just mv
  → Defd (witness-of P (out1 st output v)) (reconstitute output d m bits)
defd-reconstitute {P} {st} {output} {d} {m} {bits} {v} {dv} {mv} fresh rd rm =
    (dv , ⊢ᶠ-pres P st output v d fresh rd)
  , (mv , ⊢ᶠ-pres P st output v m fresh rm)
  , (v  , assign-here P st output v)

-- No output (post-state `st`).

defd-eq : ∀ {P st a b av bv}
  → resolve (State.mem st) a ≡ just av → resolve (State.mem st) b ≡ just bv
  → Defd (witness-of P st) (eq a b)
defd-eq {P} {st} {a} {b} {av} {bv} ra rb =
    (av , trans (resolve-agree P st a) ra)
  , (bv , trans (resolve-agree P st b) rb)

defd-boolean : ∀ {P st val x}
  → resolveᶠ (State.mem st) val ≡ just x
  → Defd (witness-of P st) (boolean val)
defd-boolean {P} {st} {val} {x} rv = x , trans (resolveᶜ-Fr-agree P st val) rv

defd-in-range : ∀ {P st val bits x}
  → resolveᶠ (State.mem st) val ≡ just x
  → Defd (witness-of P st) (in-range val bits)
defd-in-range {P} {st} {val} {bits} {x} rv =
  x , trans (resolveᶜ-Fr-agree P st val) rv

-- Two distinct outputs (post-state `out1 (out1 st id₁ v₁) id₂ v₂`).

defd-into-coords : ∀ {P st point xo yo v₁ v₂ pv}
  → State.mem st xo ≡ nothing → State.mem st yo ≡ nothing → ¬ (xo ≡ yo)
  → resolve (State.mem st) point ≡ just pv
  → Defd (witness-of P (out1 (out1 st xo v₁) yo v₂)) (into-coords xo yo point)
defd-into-coords {P} {st} {point} {xo} {yo} {v₁} {v₂} {pv} fx fy xo≢yo rp =
    (pv , ⊢-pres2 P st xo v₁ yo v₂ point fx fy (λ y≡x → xo≢yo (sym y≡x)) rp)
  , (v₁ , assign-inner P st xo v₁ yo v₂ xo≢yo)
  , (v₂ , assign-outer P st xo v₁ yo v₂)

defd-bytes-into-low-high : ∀ {P st bytes lo hi v₁ v₂ bv}
  → State.mem st lo ≡ nothing → State.mem st hi ≡ nothing → ¬ (lo ≡ hi)
  → resolve (State.mem st) bytes ≡ just bv
  → Defd (witness-of P (out1 (out1 st lo v₁) hi v₂)) (bytes-into-low-high lo hi bytes)
defd-bytes-into-low-high {P} {st} {bytes} {lo} {hi} {v₁} {v₂} {bv} fl fh lo≢hi rb =
    (bv , ⊢-pres2 P st lo v₁ hi v₂ bytes fl fh (λ h≡l → lo≢hi (sym h≡l)) rb)
  , (v₁ , assign-inner P st lo v₁ hi v₂ lo≢hi)
  , (v₂ , assign-outer P st lo v₁ hi v₂)

defd-div-mod : ∀ {P st q r val bits v₁ v₂ x}
  → State.mem st q ≡ nothing → State.mem st r ≡ nothing → ¬ (q ≡ r)
  → resolveᶠ (State.mem st) val ≡ just x
  → Defd (witness-of P (out1 (out1 st q v₁) r v₂)) (div-mod q r val bits)
defd-div-mod {P} {st} {q} {r} {val} {bits} {v₁} {v₂} {x} fq fr q≢r rv =
    (x  , ⊢ᶠ-pres2 P st q v₁ r v₂ val fq fr (λ r≡q → q≢r (sym r≡q)) rv)
  , (v₁ , assign-inner P st q v₁ r v₂ q≢r)
  , (v₂ , assign-outer P st q v₁ r v₂)

defd-sha256 : ∀ {P st alignment inputs output v frs}
  → State.mem st output ≡ nothing
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → Defd (witness-of P (out1 st output v)) (sha256 output alignment inputs)
defd-sha256 {P} {st} {alignment} {inputs} {output} {v} {frs} fresh ri =
    (frs , ⊢all-pres P st output v inputs fresh ri)
  , (v   , assign-here P st output v)

defd-keccak : ∀ {P st alignment inputs output v frs}
  → State.mem st output ≡ nothing
  → resolve-all-Fr (State.mem st) inputs ≡ just frs
  → Defd (witness-of P (out1 st output v)) (keccak output alignment inputs)
defd-keccak {P} {st} {alignment} {inputs} {output} {v} {frs} fresh ri =
    (frs , ⊢all-pres P st output v inputs fresh ri)
  , (v   , assign-here P st output v)

------------------------------------------------------------------------
-- The backward spine.
--
-- An endpoint-indexed reconstruction of the run from side data: per step
-- the post-state (pinned by the output value(s), or the transcript/output
-- record update) plus the semantic facts the corresponding `*-bwd` lemma
-- needs beyond freshness, operand resolutions/types, and `holds` (guard
-- readings, booleanity, the field-inverse, the no-overflow bound, the
-- transcript slice/decode, the output collection).  Each `bw-cons` also
-- carries the per-step store/PI monotonicity, so `spine-⊑` recovers
-- `mem st ⊑ mem s` / `pis st ≼ pis s` structurally, with no run.
------------------------------------------------------------------------

-- Post-state pinning for one / two fresh output cells.
O¹ : State → Identifier → State → Set
O¹ st o st' = ∃ λ v → st' ≡ out1 st o v

O² : State → Identifier → Identifier → State → Set
O² st o₁ o₂ st' = ∃ λ v₁ → ∃ λ v₂ → st' ≡ out1 (out1 st o₁ v₁) o₂ v₂

BwdStep : ProofPreimage → IrSource → State → Instruction → State → Set
BwdStep P S st (encode input outputs) st' =
  ∃ λ v → resolve (State.mem st) input ≡ just v
        × insertMany st outputs (map val-native (encodeᵉ v)) ≡ just st'
BwdStep P S st (assert cond) st' =
  st' ≡ st
  × (∃ λ x → resolveᶠ (State.mem st) cond ≡ just x × to𝔹 x ≡ just true)
BwdStep P S st (cond-select bit a b o) st' =
  O¹ st o st'
  × (∃ λ x → resolveᶠ (State.mem st) bit ≡ just x)
BwdStep P S st (constrain-bits val bits) st' = st' ≡ st
BwdStep P S st (constrain-eq a b) st' = st' ≡ st
BwdStep P S st (constrain-to-boolean val) st' = st' ≡ st
BwdStep P S st (copy val o) st' = O¹ st o st'
BwdStep P S st (impact guard inputs) st' =
  ∃ λ vals → ∃ λ gᶠ →
    resolve-all-Fr (State.mem st) inputs ≡ just vals
  × resolveᶠ (State.mem st) guard ≡ just gᶠ
  × ( (to𝔹 gᶠ ≡ just true
       × take (length vals)
           (drop (State.pti-idx st)
             (ProofPreimage.pub-transcript-inputs P)) ≡ vals
       × st' ≡ record st
                 { pis      = State.pis st ++ vals
                 ; pi-skips = State.pi-skips st ++ (nothing ∷ [])
                 ; pti-idx  = State.pti-idx st + length vals })
    ⊎ (to𝔹 gᶠ ≡ just false
       × st' ≡ record st
                 { pis      = State.pis st ++ map (λ _ → 0ᶠ) vals
                 ; pi-skips = State.pi-skips st ++ (just (length vals) ∷ []) }))
BwdStep P S st (ec-mul a scalar o) st' = O¹ st o st'
BwdStep P S st (ec-mul-generator scalar o) st' = O¹ st o st'
BwdStep P S st (hash-to-curve inputs o) st' = O¹ st o st'
BwdStep P S st (into-coordinates point (xo , yo)) st' = O² st xo yo st'
BwdStep P S st (from-coordinates (x , y) o) st' = O¹ st o st'
BwdStep P S st (into-bytes32 input o) st' = O¹ st o st'
-- The emitted `from-bytes` constraint leaves the output *type* a
-- disjunction, so the side data pins the output constructor selected by
-- `val-t` (the payload is recovered from `holds` and the concrete lemma).
BwdStep P S st (from-bytes32 bytes val-t o) st' =
  case val-t of λ { native      → ∃ λ n → st' ≡ out1 st o (val-native n)
                  ; secp-base   → ∃ λ x → st' ≡ out1 st o (val-secp-base x)
                  ; secp-scalar → ∃ λ x → st' ≡ out1 st o (val-secp-scalar x)
                  ; _           → ⊥ }
BwdStep P S st (reverse-bytes bytes o) st' = O¹ st o st'
BwdStep P S st (bytes32-into-low-high bytes (lo , hi)) st' = O² st lo hi st'
BwdStep P S st (bytes32-from-low-high (lo , hi) o) st' = O¹ st o st'
BwdStep P S st (div-mod-power-of-two val bits outs) st' =
  case outs of λ { (q ∷ r ∷ []) → O² st q r st' ; _ → ⊥ }
BwdStep P S st (reconstitute-field d m bits o) st' =
  O¹ st o st'
  × (∃ λ dv → resolveᶠ (State.mem st) d ≡ just dv
     × ∃ λ mo → resolveᶠ (State.mem st) m ≡ just mo
                     × (valFr mo + 2 ^ bits * valFr dv < FR-ORDER))
BwdStep P S st (transient-hash inputs o) st' = O¹ st o st'
BwdStep P S st (persistent-hash al inputs o) st' = O¹ st o st'
BwdStep P S st (keccak256 al inputs o) st' = O¹ st o st'
BwdStep P S st (test-eq a b o) st' = O¹ st o st'
BwdStep P S st (add a b o) st' = O¹ st o st'
BwdStep P S st (mul a b o) st' = O¹ st o st'
BwdStep P S st (neg a o) st' = O¹ st o st'
BwdStep P S st (inv a o) st' = O¹ st o st'
BwdStep P S st (not a o) st' =
  O¹ st o st'
  × (∃ λ x → resolveᶠ (State.mem st) a ≡ just x)
-- The `less-than` constraint pins only the chip's evened bound
-- (`< 2 ^ even4 bits`); the exact off-circuit `2 ^ bits` bounds are
-- side data.
BwdStep P S st (less-than a b bits o) st' =
  O¹ st o st'
  × (∃ λ x → resolveᶠ (State.mem st) a ≡ just x × valFr x < 2 ^ bits)
  × (∃ λ y → resolveᶠ (State.mem st) b ≡ just y × valFr y < 2 ^ bits)
BwdStep P S st (jubjub-scalar-from-native a o) st' = O¹ st o st'
BwdStep P S st (public-input guard val-t o) st' =
    (eval-guard (State.mem st) guard ≡ just true
     × ∃ λ v → decode val-t (take (encoded-len val-t) (State.pto-rem st)) ≡ just v
               × st' ≡ record st
                         { mem     = ins o v (State.mem st)
                         ; pto-rem = drop (encoded-len val-t) (State.pto-rem st) })
  ⊎ (eval-guard (State.mem st) guard ≡ just false
     × st' ≡ out1 st o (default-val val-t))
BwdStep P S st (private-input guard val-t o) st' =
    (eval-guard (State.mem st) guard ≡ just true
     × ∃ λ v → decode val-t (take (encoded-len val-t) (State.priv-rem st)) ≡ just v
               × st' ≡ record st
                         { mem      = ins o v (State.mem st)
                         ; priv-rem = drop (encoded-len val-t) (State.priv-rem st) })
  ⊎ (eval-guard (State.mem st) guard ≡ just false
     × st' ≡ out1 st o (default-val val-t))
BwdStep P S st (circuit-output vals) st' =
  ∃ λ vs → collectOutputs (State.mem st) (IrSource.outputs S) vals ≡ just vs
         × st' ≡ record st { outs = State.outs st ++ vs }

data BwdWalk (P : ProofPreimage) (S : IrSource)
     : State → List Instruction → State → Set where
  bw-nil  : ∀ {st} → BwdWalk P S st [] st
  bw-cons : ∀ {st st′ s} i is
          → State.mem st ⊑ State.mem st′ → State.pis st ≼ State.pis st′
          → BwdStep P S st i st′
          → BwdWalk P S st′ is s
          → BwdWalk P S st (i ∷ is) s

spine-⊑ : ∀ {P S st is s} → BwdWalk P S st is s
  → (State.mem st ⊑ State.mem s) × (State.pis st ≼ State.pis s)
spine-⊑ {st = st} bw-nil = ⊑-refl {State.mem st} , ≼-refl
spine-⊑ {st = st} {s = s} (bw-cons {st′ = st′} i is m⊑ p≼ _ tail) =
  let (m⊑′ , p≼′) = spine-⊑ tail
  in ⊑-trans {State.mem st} {State.mem st′} {State.mem s} m⊑ m⊑′
   , ≼-trans p≼ p≼′

------------------------------------------------------------------------
-- The spine of a genuine run (the v2 `R⇒preprocess-shaped` analogue).
--
-- A successful `step` determines its own `BwdStep` side data: one
-- inversion per instruction reads the resolutions, guard values, bounds,
-- and post-state shape off the step's success (`step→bwd`), and
-- `run→BwdWalk` folds them — with `step-extends` supplying the per-step
-- monotonicity — into a `BwdWalk`.  `preprocess→BwdWalk` packages the
-- projection at the `preprocess` level, so callers can discharge
-- `circuit-faithful`'s spine and `Consumed` hypotheses from a successful
-- `preprocess` and the static single-assignment check alone.
------------------------------------------------------------------------

step→bwd : ∀ {P S st st'} (i : Instruction)
  → step P S st i ≡ just st'
  → BwdStep P S st i st'
step→bwd {st = st} (encode input outputs) e
  with resolve (State.mem st) input
... | just v = v , refl , e
step→bwd {st = st} (assert cond) e
  with resolveᶠ (State.mem st) cond
... | just x with to𝔹 x in eqb
...   | just true with refl ← e = refl , x , refl , eqb
step→bwd {st = st} (cond-select bit a b output) e
  with resolveᶠ (State.mem st) bit
... | just x with to𝔹 x
...   | just bv with resolve (State.mem st) a | resolve (State.mem st) b
...     | just av | just bvl with typeof av ≟T typeof bvl
...       | yes _ with refl ← e = (_ , refl) , x , refl
step→bwd {st = st} (constrain-bits val bits) e
  with resolveᶠ (State.mem st) val
... | just x with valFr x <? 2 ^ bits
...   | yes _ with refl ← e = refl
step→bwd {st = st} (constrain-eq a b) e
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just av | just bv with valEq? av bv
...   | just true with refl ← e = refl
step→bwd {st = st} (constrain-to-boolean val) e
  with resolveᶠ (State.mem st) val
... | just x with to𝔹 x
...   | just _ with refl ← e = refl
step→bwd {st = st} (copy val output) e
  with resolve (State.mem st) val
... | just v with refl ← e = v , refl
step→bwd {P = P} {st = st} (impact guard inputs) e
  with resolve-all-Fr (State.mem st) inputs
... | just vals with resolveᶠ (State.mem st) guard
...   | just gᶠ with to𝔹 gᶠ in eqb
...     | just true
          with take (length vals)
                 (drop (State.pti-idx st)
                   (ProofPreimage.pub-transcript-inputs P))
               ≟LFr vals
...       | yes teq with refl ← e =
            vals , gᶠ , refl , refl , inj₁ (eqb , teq , refl)
step→bwd {P = P} {st = st} (impact guard inputs) e
    | just vals | just gᶠ | just false with refl ← e =
      vals , gᶠ , refl , refl , inj₂ (eqb , refl)
step→bwd {st = st} (ec-mul a scalar output) e
  with resolve (State.mem st) a | resolve (State.mem st) scalar
... | just (val-jubjub-point p) | just (val-jubjub-scalar s)
        with refl ← e = _ , refl
... | just (val-secp-point p) | just (val-secp-scalar s)
        with refl ← e = _ , refl
step→bwd {st = st} (ec-mul-generator scalar output) e
  with resolve (State.mem st) scalar
... | just (val-jubjub-scalar s) with refl ← e = _ , refl
... | just (val-secp-scalar s)   with refl ← e = _ , refl
step→bwd {st = st} (hash-to-curve inputs output) e
  with resolve-all-Fr (State.mem st) inputs
... | just frs with refl ← e = _ , refl
step→bwd {st = st} (into-coordinates point (xid , yid)) e
  with resolve (State.mem st) point
... | just (val-jubjub-point p) with coordsJ p
...   | (x , y) with refl ← e = _ , _ , refl
step→bwd {st = st} (into-coordinates point (xid , yid)) e
    | just (val-secp-point p) with coordsK p
...   | just (x , y) with refl ← e = _ , _ , refl
step→bwd {st = st} (from-coordinates (xop , yop) output) e
  with resolve (State.mem st) xop | resolve (State.mem st) yop
... | just (val-native x) | just (val-native y) with fromCoordsJ x y
...   | just p with refl ← e = _ , refl
step→bwd {st = st} (from-coordinates (xop , yop) output) e
    | just (val-secp-base x) | just (val-secp-base y) with fromCoordsK x y
...   | just p with refl ← e = _ , refl
step→bwd {st = st} (into-bytes32 input output) e
  with resolve (State.mem st) input
... | just (val-native x)      with refl ← e = _ , refl
... | just (val-secp-base x)   with refl ← e = _ , refl
... | just (val-secp-scalar s) with refl ← e = _ , refl
step→bwd {st = st} (from-bytes32 bytes native output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = _ , refl
step→bwd {st = st} (from-bytes32 bytes bytes32 output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step→bwd {st = st} (from-bytes32 bytes jubjub-point output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step→bwd {st = st} (from-bytes32 bytes jubjub-scalar output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step→bwd {st = st} (from-bytes32 bytes secp-point output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) = case e of λ ()
step→bwd {st = st} (from-bytes32 bytes secp-base output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = _ , refl
step→bwd {st = st} (from-bytes32 bytes secp-scalar output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = _ , refl
step→bwd {st = st} (reverse-bytes bytes output) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with refl ← e = _ , refl
step→bwd {st = st} (bytes32-into-low-high bytes (loid , hiid)) e
  with resolve (State.mem st) bytes
... | just (val-bytes32 b) with bytes32→low-high b
...   | (lo , hi) with refl ← e = _ , _ , refl
step→bwd {st = st} (bytes32-from-low-high (loop , hiop) output) e
  with resolveᶠ (State.mem st) loop | resolveᶠ (State.mem st) hiop
... | just lo | just hi with low-high→bytes32 lo hi
...   | just b with refl ← e = _ , refl
step→bwd {st = st} (div-mod-power-of-two val bits []) e
  with resolveᶠ (State.mem st) val
... | just x = case e of λ ()
step→bwd {st = st} (div-mod-power-of-two val bits (q ∷ [])) e
  with resolveᶠ (State.mem st) val
... | just x = case e of λ ()
step→bwd {st = st} (div-mod-power-of-two val bits (q ∷ r ∷ [])) e
  with resolveᶠ (State.mem st) val
... | just x with refl ← e = _ , _ , refl
step→bwd {st = st} (div-mod-power-of-two val bits (q ∷ r ∷ z ∷ zs)) e
  with resolveᶠ (State.mem st) val
... | just x = case e of λ ()
step→bwd {st = st} (reconstitute-field divisor modulus bits output) e
  with resolveᶠ (State.mem st) divisor
     | resolveᶠ (State.mem st) modulus
... | just d | just mo with valFr mo <? 2 ^ bits
...   | yes _ with valFr d <? 2 ^ (FR-BITS ∸ bits)
...     | yes _ with valFr mo + 2 ^ bits * valFr d <? FR-ORDER
...       | yes novf with refl ← e =
            (_ , refl) , d , refl , mo , refl , novf
step→bwd {st = st} (transient-hash inputs output) e
  with resolve-all-Fr (State.mem st) inputs
... | just frs with refl ← e = _ , refl
step→bwd {st = st} (persistent-hash alignment inputs output) e
  with resolve-all-Fr (State.mem st) inputs
... | just frs with persistent-hash-fn alignment frs
...   | just h with refl ← e = _ , refl
step→bwd {st = st} (keccak256 alignment inputs output) e
  with resolve-all-Fr (State.mem st) inputs
... | just frs with keccak-fn alignment frs
...   | just h with refl ← e = _ , refl
step→bwd {st = st} (test-eq a b output) e
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just av | just bv with valEq? av bv
...   | just eqv with refl ← e = _ , refl
step→bwd {st = st} (add a b output) e
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just (val-native x) | just (val-native y)
        with refl ← e = _ , refl
... | just (val-jubjub-point p) | just (val-jubjub-point q)
        with refl ← e = _ , refl
... | just (val-secp-point p) | just (val-secp-point q)
        with refl ← e = _ , refl
... | just (val-secp-base x) | just (val-secp-base y)
        with refl ← e = _ , refl
... | just (val-secp-scalar x) | just (val-secp-scalar y)
        with refl ← e = _ , refl
step→bwd {st = st} (mul a b output) e
  with resolve (State.mem st) a | resolve (State.mem st) b
... | just (val-native x) | just (val-native y)
        with refl ← e = _ , refl
... | just (val-secp-base x) | just (val-secp-base y)
        with refl ← e = _ , refl
... | just (val-secp-scalar x) | just (val-secp-scalar y)
        with refl ← e = _ , refl
step→bwd {st = st} (neg a output) e
  with resolve (State.mem st) a
... | just (val-native x)       with refl ← e = _ , refl
... | just (val-jubjub-point p) with refl ← e = _ , refl
... | just (val-secp-point p)   with refl ← e = _ , refl
... | just (val-secp-base x)    with refl ← e = _ , refl
... | just (val-secp-scalar x)  with refl ← e = _ , refl
step→bwd {st = st} (inv a output) e
  with resolve (State.mem st) a
... | just (val-native x) with invᶠ x
...   | just xi with refl ← e = _ , refl
step→bwd {st = st} (inv a output) e
    | just (val-secp-base x) with invᵇ x
...   | just xi with refl ← e = _ , refl
step→bwd {st = st} (inv a output) e
    | just (val-secp-scalar x) with invˢ x
...   | just xi with refl ← e = _ , refl
step→bwd {st = st} (not a output) e
  with resolveᶠ (State.mem st) a
... | just x with to𝔹 x
...   | just b with refl ← e = (_ , refl) , x , refl
step→bwd {st = st} (less-than a b bits output) e
  with resolveᶠ (State.mem st) a | resolveᶠ (State.mem st) b
... | just x | just y with valFr x <? 2 ^ bits
...   | yes px with valFr y <? 2 ^ bits
...     | yes py with refl ← e =
          (_ , refl) , (x , refl , px) , (y , refl , py)
step→bwd {st = st} (jubjub-scalar-from-native a output) e
  with resolveᶠ (State.mem st) a
... | just x with refl ← e = _ , refl
step→bwd {st = st} (public-input guard val-t output) e
  with eval-guard (State.mem st) guard
... | just true
      with decode val-t (take (encoded-len val-t) (State.pto-rem st))
...   | just v with refl ← e = inj₁ (refl , v , refl , refl)
step→bwd {st = st} (public-input guard val-t output) e | just false
  with refl ← e = inj₂ (refl , refl)
step→bwd {st = st} (private-input guard val-t output) e
  with eval-guard (State.mem st) guard
... | just true
      with decode val-t (take (encoded-len val-t) (State.priv-rem st))
...   | just v with refl ← e = inj₁ (refl , v , refl , refl)
step→bwd {st = st} (private-input guard val-t output) e | just false
  with refl ← e = inj₂ (refl , refl)
step→bwd {S = S} {st = st} (circuit-output vals) e
  with collectOutputs (State.mem st) (IrSource.outputs S) vals
... | just vs with refl ← e = vs , refl , refl

-- Fold the per-step inversions over a well-formed run.
run→BwdWalk : ∀ {P S st s} is
  → WF-run P S is st
  → run P S st is ≡ just s
  → BwdWalk P S st is s
run→BwdWalk []       _              req with refl ← req = bw-nil
run→BwdWalk {P} {S} {st} (i ∷ is) (of , wf-rest) req =
  let (st-mid , step-eq , run-rest) = run-inv i is st req
      (p≼ , m⊑) = step-extends i step-eq of
  in bw-cons i is m⊑ p≼ (step→bwd i step-eq)
       (run→BwdWalk is (wf-rest step-eq) run-rest)

-- The `preprocess`-level projection: spine and terminal consumption from
-- a successful preprocess and the static single-assignment check.
preprocess→BwdWalk : ∀ {S P s st0}
  → producer-SA S
  → init S P ≡ just st0
  → preprocess S P ≡ just s
  → BwdWalk P S st0 (IrSource.instructions S) s × Consumed P s
preprocess→BwdWalk {S} {P} {s} {st0} sa ieq peq =
  let (walk , cons) = preprocess-walk-consumed {S} {P} {s} {st0} ieq peq
  in run→BwdWalk (IrSource.instructions S)
       (producer-safe→WF {S} {P} {st0} sa ieq) walk
   , cons

------------------------------------------------------------------------
-- The backward driver: reassemble the run bottom-up from the spine.
--
-- `bwd-go` folds the spine, deriving each step operationally from the
-- per-instruction `*-bwd` lemma (fed by `spine-⊑`+`holds-lower`+`defd-*`
-- and the `*-support` discharges) and chaining with `run-cons`.
------------------------------------------------------------------------

mapᵐ-inv : ∀ {A B : Set} {f : A → B} {x : Maybe A} {y}
  → mapᵐ f x ≡ just y → ∃ λ z → x ≡ just z
mapᵐ-inv {x = just z} refl = z , refl

typeof-scalar : ∀ {v} → typeof v ≡ jubjub-scalar
  → ∃ λ s → v ≡ val-jubjub-scalar s
typeof-scalar {val-jubjub-scalar s} refl = s , refl

point-support : ∀ {m Γ a av}
  → TyEq m Γ → optype Γ a ≡ just jubjub-point → resolve m a ≡ just av
  → ∃ λ p → resolve m a ≡ just (val-jubjub-point p)
point-support {a = a} teq oa ra
  with typeof-point (tyEq-optype {op = a} teq oa ra)
... | (p , refl) = p , ra

scalar-support : ∀ {m Γ a av}
  → TyEq m Γ → optype Γ a ≡ just jubjub-scalar → resolve m a ≡ just av
  → ∃ λ s → resolve m a ≡ just (val-jubjub-scalar s)
scalar-support {a = a} teq oa ra
  with typeof-scalar (tyEq-optype {op = a} teq oa ra)
... | (s , refl) = s , ra

bytes32-support : ∀ {m Γ a av}
  → TyEq m Γ → optype Γ a ≡ just bytes32 → resolve m a ≡ just av
  → ∃ λ b → resolve m a ≡ just (val-bytes32 b)
bytes32-support {a = a} teq oa ra
  with typeof-bytes32 (tyEq-optype {op = a} teq oa ra)
... | (b , refl) = b , ra

secp-base-support : ∀ {m Γ a av}
  → TyEq m Γ → optype Γ a ≡ just secp-base → resolve m a ≡ just av
  → ∃ λ x → resolve m a ≡ just (val-secp-base x)
secp-base-support {a = a} teq oa ra
  with typeof-secp-base (tyEq-optype {op = a} teq oa ra)
... | (x , refl) = x , ra

secp-point-support : ∀ {m Γ a av}
  → TyEq m Γ → optype Γ a ≡ just secp-point → resolve m a ≡ just av
  → ∃ λ p → resolve m a ≡ just (val-secp-point p)
secp-point-support {a = a} teq oa ra
  with typeof-secp-point (tyEq-optype {op = a} teq oa ra)
... | (p , refl) = p , ra

secp-scalar-support : ∀ {m Γ a av}
  → TyEq m Γ → optype Γ a ≡ just secp-scalar → resolve m a ≡ just av
  → ∃ λ s → resolve m a ≡ just (val-secp-scalar s)
secp-scalar-support {a = a} teq oa ra
  with typeof-secp-scalar (tyEq-optype {op = a} teq oa ra)
... | (s , refl) = s , ra

mutual
  -- The backward driver: reassemble the run bottom-up from the spine.
  bwd-go : ∀ {P S s} (is : List Instruction) st Γ bound ss
    → dom-ty Γ ≡ bound
    → SA bound is → WT Γ is
    → TyEq (State.mem st) Γ → DomEq (State.mem st) bound
    → BwdWalk P S st is s
    → satisfies-constraints (csOf is ss) (witness-of P s)
    → run P S st is ≡ just s
  bwd-go [] st Γ bound ss _ _ _ _ _ bw-nil _ = refl

  bwd-go {P} {S} (mul a b output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , (t , oa , ob , fm) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (mul a b output) dom af nd
        (av , ra) = optype-resolve {op = a} teq oa
        (bv , rb) = optype-resolve {op = b} teq ob
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (mul a b output) is ss csat
        hst = holds-lower (gate-mul output a b) m⊑ p≼
                (defd-mul {P} {st} {output} {a} {b} {v} {av} {bv} fresh ra rb)
                (proj₁ hc)
        steq = mul-bwd {P} {S} {st} {a} {b} {output} {v} fresh
                 (mul-support {a = a} {b = b} teq oa ob fm ra rb) hst
    in bwd-cont (mul a b output) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (add a b output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , (t , ota , otb , fp) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (add a b output) dom af nd
        (av , ra) = optype-resolve {op = a} teq ota
        (bv , rb) = optype-resolve {op = b} teq otb
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (add a b output) is ss csat
        hst = holds-lower (gate-add output a b) m⊑ p≼
                (defd-add {P} {st} {output} {a} {b} {v} {av} {bv} fresh ra rb)
                (proj₁ hc)
        steq = add-bwd {P} {S} {st} {a} {b} {output} {v} fresh
                 (add-support {a = a} {b = b} teq ota otb fp ra rb) hst
    in bwd-cont (add a b output) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (copy val output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , (t , ov) , wt') teq dom (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (copy val output) dom af nd
        (av , rv) = optype-resolve {op = val} teq ov
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (copy val output) is ss csat
        hst = holds-lower (gate-copy output val) m⊑ p≼
                (defd-copy {P} {st} {output} {val} {v} {av} fresh rv) (proj₁ hc)
        steq = copy-bwd {P} {S} {st} {output} {val} {v} {av} fresh rv hst
    in bwd-cont (copy val output) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (constrain-eq a b ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , ((t , ota , tsup) , (tb , otb)) , wt') teq dom
    (bw-cons _ _ _ _ refl tail) csat =
    let (av , ra) = optype-resolve {op = a} teq ota
        (bv , rb) = optype-resolve {op = b} teq otb
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (constrain-eq a b) is ss csat
        hst = holds-lower (eq a b) m⊑ p≼
                (defd-eq {P} {st} {a} {b} {av} {bv} ra rb) (proj₁ hc)
        steq = constrain-eq-bwd {P} {S} {st} {a} {b}
                 (av , constrain-eq-support {a = a} teq ota tsup ra) hst
    in bwd-cont (constrain-eq a b) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (assert cond ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , _ , wt') teq dom (bw-cons _ _ _ _ (refl , (x , rf , tb)) tail)
    csat =
    let (_ , tc) = csOf-peel (assert cond) is ss csat
        steq = assert-bwd {P} {S} {st} {cond} (x , rf , tb)
    in bwd-cont (assert cond) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (encode input outputs ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , _ , wt') teq dom (bw-cons _ _ _ _ (v , ri , im) tail) csat =
    let (_ , tc) = csOf-peel (encode input outputs) is ss csat
        steq = encode-bwd {P} {S} {st} {input} {outputs} {v} ri im
    in bwd-cont (encode input outputs) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (neg a output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , (t , ota , fp) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (neg a output) dom af nd
        (av , ra) = optype-resolve {op = a} teq ota
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (neg a output) is ss csat
        hst = holds-lower (gate-neg output a) m⊑ p≼
                (defd-neg {P} {st} {output} {a} {v} {av} fresh ra) (proj₁ hc)
        steq = neg-bwd {P} {S} {st} {a} {output} {v} fresh
                 (neg-support {a = a} teq ota fp ra) hst
    in bwd-cont (neg a output) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (inv a output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , (t , oa , fm) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (inv a output) dom af nd
        (av , ra) = optype-resolve {op = a} teq oa
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (inv a output) is ss csat
        hst = holds-lower (gate-inv output a) m⊑ p≼
                (defd-inv {P} {st} {output} {a} {v} {av} fresh ra) (proj₁ hc)
        steq = inv-bwd {P} {S} {st} {a} {output} {v} fresh
                 (inv-support {a = a} teq oa fm ra) hst
    in bwd-cont (inv a output) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (not a output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ ((v , refl) , (x , rf)) tail) csat =
    let fresh    = out-fresh-of (not a output) dom af nd
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (not a output) is ss csat
        hst = holds-lower (is-not output a) m⊑ p≼
                (defd-is-not {P} {st} {output} {a} {v} {x} fresh rf) (proj₁ hc)
        steq = not-bwd {P} {S} {st} {a} {output} {v} fresh (x , rf) hst
    in bwd-cont (not a output) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (test-eq a b output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , ((_ , ota) , (_ , otb)) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (test-eq a b output) dom af nd
        (av , ra) = optype-resolve {op = a} teq ota
        (bv , rb) = optype-resolve {op = b} teq otb
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (test-eq a b output) is ss csat
        hst = holds-lower (test-eq output a b) m⊑ p≼
                (defd-test-eq {P} {st} {output} {a} {b} {v} {av} {bv} fresh ra rb)
                (proj₁ hc)
        steq = test-eq-bwd {P} {S} {st} {a} {b} {output} {v} fresh
                 (av , ra) (bv , rb) hst
    in bwd-cont (test-eq a b output) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (jubjub-scalar-from-native a output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , oa , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (jubjub-scalar-from-native a output) dom af nd
        (x , rf) = optype-resolveᶠ {op = a} teq oa
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (jubjub-scalar-from-native a output) is ss csat
        hst = holds-lower (scalar-from-native output a) m⊑ p≼
                (defd-scalar-from-native {P} {st} {output} {a} {v} {x} fresh rf)
                (proj₁ hc)
        steq = jubjub-scalar-from-native-bwd {P} {S} {st} {a} {output} {v} fresh
                 (x , rf) hst
    in bwd-cont (jubjub-scalar-from-native a output) is dΓ oty af nd sa' wt'
         teq dom steq tail tc

  bwd-go {P} {S} (ec-mul a scalar output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , inj₁ (oa , osc) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (ec-mul a scalar output) dom af nd
        (av , ra) = optype-resolve {op = a} teq oa
        (sv , rs) = optype-resolve {op = scalar} teq osc
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (ec-mul a scalar output) is ss csat
        hst = holds-lower (ec-mul output a scalar) m⊑ p≼
                (defd-ec-mul {P} {st} {output} {a} {scalar} {v} {av} {sv}
                  fresh ra rs) (proj₁ hc)
        steq = ec-mul-bwd {P} {S} {st} {a} {scalar} {output} {v} fresh
                 (inj₁ ( point-support {a = a} teq oa ra
                       , scalar-support {a = scalar} teq osc rs )) hst
    in bwd-cont (ec-mul a scalar output) is dΓ oty af nd sa' wt' teq dom
         steq tail tc
  bwd-go {P} {S} (ec-mul a scalar output ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , inj₂ (oa , osc) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (ec-mul a scalar output) dom af nd
        (av , ra) = optype-resolve {op = a} teq oa
        (sv , rs) = optype-resolve {op = scalar} teq osc
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (ec-mul a scalar output) is ss csat
        hst = holds-lower (ec-mul output a scalar) m⊑ p≼
                (defd-ec-mul {P} {st} {output} {a} {scalar} {v} {av} {sv}
                  fresh ra rs) (proj₁ hc)
        steq = ec-mul-bwd {P} {S} {st} {a} {scalar} {output} {v} fresh
                 (inj₂ ( secp-point-support {a = a} teq oa ra
                       , secp-scalar-support {a = scalar} teq osc rs )) hst
    in bwd-cont (ec-mul a scalar output) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (ec-mul-generator scalar output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , inj₁ osc , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (ec-mul-generator scalar output) dom af nd
        (sv , rs) = optype-resolve {op = scalar} teq osc
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (ec-mul-generator scalar output) is ss csat
        hst = holds-lower (ec-gen output scalar) m⊑ p≼
                (defd-ec-gen {P} {st} {output} {scalar} {v} {sv} fresh rs)
                (proj₁ hc)
        steq = ec-mul-generator-bwd {P} {S} {st} {scalar} {output} {v} fresh
                 (inj₁ (scalar-support {a = scalar} teq osc rs)) hst
    in bwd-cont (ec-mul-generator scalar output) is dΓ oty af nd sa' wt'
         teq dom steq tail tc
  bwd-go {P} {S} (ec-mul-generator scalar output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , inj₂ osc , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (ec-mul-generator scalar output) dom af nd
        (sv , rs) = optype-resolve {op = scalar} teq osc
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (ec-mul-generator scalar output) is ss csat
        hst = holds-lower (ec-gen output scalar) m⊑ p≼
                (defd-ec-gen {P} {st} {output} {scalar} {v} {sv} fresh rs)
                (proj₁ hc)
        steq = ec-mul-generator-bwd {P} {S} {st} {scalar} {output} {v} fresh
                 (inj₂ (secp-scalar-support {a = scalar} teq osc rs)) hst
    in bwd-cont (ec-mul-generator scalar output) is dΓ oty af nd sa' wt'
         teq dom steq tail tc

  bwd-go {P} {S} (hash-to-curve inputs output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , an , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (hash-to-curve inputs output) dom af nd
        (frs , ri) = allNat-resolve inputs teq an
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (hash-to-curve inputs output) is ss csat
        hst = holds-lower (h2c output inputs) m⊑ p≼
                (defd-h2c {P} {st} {output} {inputs} {v} {frs} fresh ri)
                (proj₁ hc)
        steq = hash-to-curve-bwd {P} {S} {st} {inputs} {output} {v} fresh
                 (frs , ri) hst
    in bwd-cont (hash-to-curve inputs output) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (transient-hash inputs output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , an , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (transient-hash inputs output) dom af nd
        (frs , ri) = allNat-resolve inputs teq an
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (transient-hash inputs output) is ss csat
        hst = holds-lower (poseidon output inputs) m⊑ p≼
                (defd-poseidon {P} {st} {output} {inputs} {v} {frs} fresh ri)
                (proj₁ hc)
        steq = transient-hash-bwd {P} {S} {st} {inputs} {output} {v} fresh
                 (frs , ri) hst
    in bwd-cont (transient-hash inputs output) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (into-bytes32 input output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , (t , oin , fm) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (into-bytes32 input output) dom af nd
        (av , rv) = optype-resolve {op = input} teq oin
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (into-bytes32 input output) is ss csat
        hst = holds-lower (into-bytes output input) m⊑ p≼
                (defd-into-bytes {P} {st} {output} {input} {v} {av} fresh rv)
                (proj₁ hc)
        steq = into-bytes32-bwd {P} {S} {st} {input} {output} {v} fresh
                 (inv-support {a = input} teq oin fm rv) hst
    in bwd-cont (into-bytes32 input output) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (from-bytes32 bytes native output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , ob , wt') teq dom
    (bw-cons _ _ _ _ (n , refl) tail) csat =
    bwd-cont (from-bytes32 bytes native output) is dΓ oty af nd sa' wt'
      teq dom steq tail tc
    where
    fresh = out-fresh-of (from-bytes32 bytes native output) dom af nd
    b   = proj₁ (bytes32-support {a = bytes} teq ob
                  (proj₂ (optype-resolve {op = bytes} teq ob)))
    rb  = proj₂ (bytes32-support {a = bytes} teq ob
                  (proj₂ (optype-resolve {op = bytes} teq ob)))
    tc  = proj₂ (csOf-peel (from-bytes32 bytes native output) is ss csat)
    hst = holds-lower (from-bytes output bytes)
            (proj₁ (spine-⊑ tail)) (proj₂ (spine-⊑ tail))
            (defd-from-bytes {P} {st} {output} {bytes} {val-native n}
              {val-bytes32 b} fresh rb)
            (proj₁ (proj₁ (csOf-peel (from-bytes32 bytes native output) is ss
                             csat)))
    steq : step P S st (from-bytes32 bytes native output)
             ≡ just (out1 st output (val-native n))
    steq with hst
    ... | (bs , ⊢bytes , disj)
          with ⊢-back P st output (val-native n) bytes fresh rb ⊢bytes
    ...     | refl with disj
    ...       | inj₁ dn =
                trans (from-bytes32-native-bwd {P} {S} {st} {bytes} {output} {b}
                        rb)
                  (cong (λ z → just (out1 st output z))
                    (sym (out-val P st output (val-native n) dn)))
    ...       | inj₂ (inj₁ db) =
                case out-val P st output (val-native n) db of λ ()
    ...       | inj₂ (inj₂ ds) =
                case out-val P st output (val-native n) ds of λ ()
  bwd-go {P} {S} (from-bytes32 bytes secp-base output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , ob , wt') teq dom
    (bw-cons _ _ _ _ (x , refl) tail) csat =
    bwd-cont (from-bytes32 bytes secp-base output) is dΓ oty af nd sa' wt'
      teq dom steq tail tc
    where
    fresh = out-fresh-of (from-bytes32 bytes secp-base output) dom af nd
    b   = proj₁ (bytes32-support {a = bytes} teq ob
                  (proj₂ (optype-resolve {op = bytes} teq ob)))
    rb  = proj₂ (bytes32-support {a = bytes} teq ob
                  (proj₂ (optype-resolve {op = bytes} teq ob)))
    tc  = proj₂ (csOf-peel (from-bytes32 bytes secp-base output) is ss csat)
    hst = holds-lower (from-bytes output bytes)
            (proj₁ (spine-⊑ tail)) (proj₂ (spine-⊑ tail))
            (defd-from-bytes {P} {st} {output} {bytes} {val-secp-base x}
              {val-bytes32 b} fresh rb)
            (proj₁ (proj₁ (csOf-peel (from-bytes32 bytes secp-base output) is ss
                             csat)))
    steq : step P S st (from-bytes32 bytes secp-base output)
             ≡ just (out1 st output (val-secp-base x))
    steq with hst
    ... | (bs , ⊢bytes , disj)
          with ⊢-back P st output (val-secp-base x) bytes fresh rb ⊢bytes
    ...     | refl with disj
    ...       | inj₂ (inj₁ db) =
                trans (from-bytes32-secp-base-bwd {P} {S} {st} {bytes} {output}
                        {b} rb)
                  (cong (λ z → just (out1 st output z))
                    (sym (out-val P st output (val-secp-base x) db)))
    ...       | inj₁ dn =
                case out-val P st output (val-secp-base x) dn of λ ()
    ...       | inj₂ (inj₂ ds) =
                case out-val P st output (val-secp-base x) ds of λ ()
  bwd-go {P} {S} (from-bytes32 bytes secp-scalar output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , ob , wt') teq dom
    (bw-cons _ _ _ _ (x , refl) tail) csat =
    bwd-cont (from-bytes32 bytes secp-scalar output) is dΓ oty af nd sa' wt'
      teq dom steq tail tc
    where
    fresh = out-fresh-of (from-bytes32 bytes secp-scalar output) dom af nd
    b   = proj₁ (bytes32-support {a = bytes} teq ob
                  (proj₂ (optype-resolve {op = bytes} teq ob)))
    rb  = proj₂ (bytes32-support {a = bytes} teq ob
                  (proj₂ (optype-resolve {op = bytes} teq ob)))
    tc  = proj₂ (csOf-peel (from-bytes32 bytes secp-scalar output) is ss csat)
    hst = holds-lower (from-bytes output bytes)
            (proj₁ (spine-⊑ tail)) (proj₂ (spine-⊑ tail))
            (defd-from-bytes {P} {st} {output} {bytes} {val-secp-scalar x}
              {val-bytes32 b} fresh rb)
            (proj₁ (proj₁ (csOf-peel (from-bytes32 bytes secp-scalar output) is
                             ss csat)))
    steq : step P S st (from-bytes32 bytes secp-scalar output)
             ≡ just (out1 st output (val-secp-scalar x))
    steq with hst
    ... | (bs , ⊢bytes , disj)
          with ⊢-back P st output (val-secp-scalar x) bytes fresh rb ⊢bytes
    ...     | refl with disj
    ...       | inj₂ (inj₂ ds) =
                trans (from-bytes32-secp-scalar-bwd {P} {S} {st} {bytes} {output}
                        {b} rb)
                  (cong (λ z → just (out1 st output z))
                    (sym (out-val P st output (val-secp-scalar x) ds)))
    ...       | inj₁ dn =
                case out-val P st output (val-secp-scalar x) dn of λ ()
    ...       | inj₂ (inj₁ db) =
                case out-val P st output (val-secp-scalar x) db of λ ()
  bwd-go (from-bytes32 bytes bytes32 output ∷ is) st Γ bound ss dΓ sa wt teq
    dom (bw-cons _ _ _ _ () _) csat
  bwd-go (from-bytes32 bytes jubjub-point output ∷ is) st Γ bound ss dΓ sa wt
    teq dom (bw-cons _ _ _ _ () _) csat
  bwd-go (from-bytes32 bytes jubjub-scalar output ∷ is) st Γ bound ss dΓ sa wt
    teq dom (bw-cons _ _ _ _ () _) csat
  bwd-go (from-bytes32 bytes secp-point output ∷ is) st Γ bound ss dΓ sa wt
    teq dom (bw-cons _ _ _ _ () _) csat

  bwd-go {P} {S} (reverse-bytes bytes output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , ob , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (reverse-bytes bytes output) dom af nd
        (av , rv) = optype-resolve {op = bytes} teq ob
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (reverse-bytes bytes output) is ss csat
        hst = holds-lower (reverse-bytes output bytes) m⊑ p≼
                (defd-reverse-bytes {P} {st} {output} {bytes} {v} {av} fresh rv)
                (proj₁ hc)
        steq = reverse-bytes-bwd {P} {S} {st} {bytes} {output} {v} fresh
                 (bytes32-support {a = bytes} teq ob rv) hst
    in bwd-cont (reverse-bytes bytes output) is dΓ oty af nd sa' wt'
         teq dom steq tail tc

  bwd-go {P} {S} (from-coordinates (xop , yop) output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , inj₁ (ox , oy) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (from-coordinates (xop , yop) output) dom af nd
        (xa , rxa) = optype-resolve {op = xop} teq ox
        (ya , rya) = optype-resolve {op = yop} teq oy
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (from-coordinates (xop , yop) output) is ss csat
        hst = holds-lower (from-coords output xop yop) m⊑ p≼
                (defd-from-coords {P} {st} {output} {xop} {yop} {v} {xa} {ya}
                  fresh rxa rya) (proj₁ hc)
        steq = from-coordinates-bwd {P} {S} {st} {xop} {yop} {output} {v} fresh
                 (inj₁ ( mul-support-l {a = xop} teq ox rxa
                       , mul-support-l {a = yop} teq oy rya )) hst
    in bwd-cont (from-coordinates (xop , yop) output) is dΓ oty af nd sa' wt'
         teq dom steq tail tc
  bwd-go {P} {S} (from-coordinates (xop , yop) output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , inj₂ (ox , oy) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (from-coordinates (xop , yop) output) dom af nd
        (xa , rxa) = optype-resolve {op = xop} teq ox
        (ya , rya) = optype-resolve {op = yop} teq oy
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (from-coordinates (xop , yop) output) is ss csat
        hst = holds-lower (from-coords output xop yop) m⊑ p≼
                (defd-from-coords {P} {st} {output} {xop} {yop} {v} {xa} {ya}
                  fresh rxa rya) (proj₁ hc)
        steq = from-coordinates-bwd {P} {S} {st} {xop} {yop} {output} {v} fresh
                 (inj₂ ( secp-base-support {a = xop} teq ox rxa
                       , secp-base-support {a = yop} teq oy rya )) hst
    in bwd-cont (from-coordinates (xop , yop) output) is dΓ oty af nd sa' wt'
         teq dom steq tail tc

  bwd-go {P} {S} (bytes32-from-low-high (loop , hiop) output ∷ is) st Γ bound ss
    dΓ (af , nd , sa') (Γ' , oty , (olo , ohi) , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (bytes32-from-low-high (loop , hiop) output)
                     dom af nd
        (lo , rl) = optype-resolveᶠ {op = loop} teq olo
        (hi , rh) = optype-resolveᶠ {op = hiop} teq ohi
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (bytes32-from-low-high (loop , hiop) output)
                      is ss csat
        hst = holds-lower (bytes-from-low-high output loop hiop) m⊑ p≼
                (defd-bytes-from-low-high {P} {st} {output} {loop} {hiop} {v}
                  {lo} {hi} fresh rl rh) (proj₁ hc)
        steq = bytes32-from-low-high-bwd {P} {S} {st} {loop} {hiop} {output} {v}
                 fresh (lo , rl) (hi , rh) hst
    in bwd-cont (bytes32-from-low-high (loop , hiop) output) is dΓ oty af nd
         sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (less-than a b bits output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , (oa , ob) , wt') teq dom
    (bw-cons _ _ _ _ ((v , refl) , (x , ra , px) , (y , rb , py)) tail) csat =
    let fresh    = out-fresh-of (less-than a b bits output) dom af nd
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (less-than a b bits output) is ss csat
        hst = holds-lower (less-than output a b bits) m⊑ p≼
                (defd-less-than {P} {st} {output} {a} {b} {bits} {v} {x} {y}
                  fresh ra rb) (proj₁ hc)
        steq = less-than-bwd {P} {S} {st} {a} {b} {bits} {output} {v} fresh
                 (x , ra , px) (y , rb , py) hst
    in bwd-cont (less-than a b bits output) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (cond-select bit a b output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , obit , wt') teq dom
    (bw-cons _ _ _ _ ((v , refl) , (xb , rbit)) tail) csat =
    let (t , sty) = mapᵐ-inv oty
        fresh    = out-fresh-of (cond-select bit a b output) dom af nd
        (av , ra) = optype-resolve {op = a} teq (same-ty-l {Γ} {a} {b} sty)
        (bv , rb) = optype-resolve {op = b} teq (same-ty-r {Γ} {a} {b} sty)
        tmatch   = cond-select-support {a = a} {b = b} teq
                     (same-ty-l {Γ} {a} {b} sty) (same-ty-r {Γ} {a} {b} sty)
                     ra rb
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (cond-select bit a b output) is ss csat
        hst = holds-lower (select output bit a b) m⊑ p≼
                (defd-select {P} {st} {output} {bit} {a} {b} {v} {xb} {av} {bv}
                  fresh rbit ra rb) (proj₁ hc)
        steq = cond-select-bwd {P} {S} {st} {bit} {a} {b} {output} {v} fresh
                 (xb , rbit) (av , ra , bv , rb , tmatch) hst
    in bwd-cont (cond-select bit a b output) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (reconstitute-field divisor modulus bits output ∷ is) st Γ
    bound ss dΓ (af , nd , sa') (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ ((v , refl) , (d , rd , mo , rm , novf)) tail) csat =
    let fresh    = out-fresh-of (reconstitute-field divisor modulus bits output)
                     dom af nd
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (reconstitute-field divisor modulus bits output)
                      is ss csat
        hst = holds-lower (reconstitute output divisor modulus bits) m⊑ p≼
                (defd-reconstitute {P} {st} {output} {divisor} {modulus} {bits}
                  {v} {d} {mo} fresh rd rm) (proj₁ hc)
        steq = reconstitute-field-bwd {P} {S} {st} {divisor} {modulus} {bits}
                 {output} {v} fresh (d , rd , mo , rm , novf) hst
    in bwd-cont (reconstitute-field divisor modulus bits output) is dΓ oty af
         nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (constrain-bits val bits ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , ov , wt') teq dom (bw-cons _ _ _ _ refl tail) csat =
    let (x , rf) = optype-resolveᶠ {op = val} teq ov
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (constrain-bits val bits) is ss csat
        hst = holds-lower (in-range val bits) m⊑ p≼
                (defd-in-range {P} {st} {val} {bits} {x} rf) (proj₁ hc)
        steq = constrain-bits-bwd {P} {S} {st} {val} {bits} hst
    in bwd-cont (constrain-bits val bits) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (constrain-to-boolean val ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , ov , wt') teq dom
    (bw-cons _ _ _ _ refl tail) csat =
    let (x , rf) = optype-resolveᶠ {op = val} teq ov
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (constrain-to-boolean val) is ss csat
        hst = holds-lower (boolean val) m⊑ p≼
                (defd-boolean {P} {st} {val} {x} rf) (proj₁ hc)
        steq = constrain-to-boolean-bwd {P} {S} {st} {val} hst
    in bwd-cont (constrain-to-boolean val) is dΓ oty af nd sa' wt' teq dom
         steq tail tc

  bwd-go {P} {S} (into-coordinates point (xo , yo) ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , inj₁ op , wt') teq dom
    (bw-cons _ _ _ _ (v₁ , v₂ , refl) tail) csat =
    let (fx , fy , x≢y) = out-fresh-of (into-coordinates point (xo , yo))
                            dom af nd
        (pv , rp) = optype-resolve {op = point} teq op
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (into-coordinates point (xo , yo)) is ss csat
        hst = holds-lower (into-coords xo yo point) m⊑ p≼
                (defd-into-coords {P} {st} {point} {xo} {yo} {v₁} {v₂} {pv}
                  fx fy x≢y rp) (proj₁ hc)
        steq = into-coordinates-bwd {P} {S} {st} {point} {xo} {yo} {v₁} {v₂}
                 fx fy x≢y (inj₁ (point-support {a = point} teq op rp)) hst
    in bwd-cont (into-coordinates point (xo , yo)) is dΓ oty af nd sa' wt'
         teq dom steq tail tc
  bwd-go {P} {S} (into-coordinates point (xo , yo) ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , inj₂ op , wt') teq dom
    (bw-cons _ _ _ _ (v₁ , v₂ , refl) tail) csat =
    let (fx , fy , x≢y) = out-fresh-of (into-coordinates point (xo , yo))
                            dom af nd
        (pv , rp) = optype-resolve {op = point} teq op
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (into-coordinates point (xo , yo)) is ss csat
        hst = holds-lower (into-coords xo yo point) m⊑ p≼
                (defd-into-coords {P} {st} {point} {xo} {yo} {v₁} {v₂} {pv}
                  fx fy x≢y rp) (proj₁ hc)
        steq = into-coordinates-bwd {P} {S} {st} {point} {xo} {yo} {v₁} {v₂}
                 fx fy x≢y (inj₂ (secp-point-support {a = point} teq op rp)) hst
    in bwd-cont (into-coordinates point (xo , yo)) is dΓ oty af nd sa' wt'
         teq dom steq tail tc

  bwd-go {P} {S} (bytes32-into-low-high bytes (lo , hi) ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , ob , wt') teq dom
    (bw-cons _ _ _ _ (v₁ , v₂ , refl) tail) csat =
    let (fl , fh , l≢h) = out-fresh-of (bytes32-into-low-high bytes (lo , hi))
                            dom af nd
        (bv , rb) = optype-resolve {op = bytes} teq ob
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (bytes32-into-low-high bytes (lo , hi)) is ss csat
        hst = holds-lower (bytes-into-low-high lo hi bytes) m⊑ p≼
                (defd-bytes-into-low-high {P} {st} {bytes} {lo} {hi} {v₁} {v₂}
                  {bv} fl fh l≢h rb) (proj₁ hc)
        steq = bytes32-into-low-high-bwd {P} {S} {st} {bytes} {lo} {hi} {v₁} {v₂}
                 fl fh l≢h (bytes32-support {a = bytes} teq ob rb) hst
    in bwd-cont (bytes32-into-low-high bytes (lo , hi)) is dΓ oty af nd sa' wt'
         teq dom steq tail tc

  bwd-go {P} {S} (div-mod-power-of-two val bits (q ∷ r ∷ []) ∷ is) st Γ bound ss
    dΓ (af , nd , sa') (Γ' , oty , ov , wt') teq dom
    (bw-cons _ _ _ _ (v₁ , v₂ , refl) tail) csat =
    let ((fq , fr , _) , ((q≢r , _) , _)) =
          out-fresh-of (div-mod-power-of-two val bits (q ∷ r ∷ [])) dom af nd
        (x , rf) = optype-resolveᶠ {op = val} teq ov
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (div-mod-power-of-two val bits (q ∷ r ∷ []))
                      is ss csat
        hst = holds-lower (div-mod q r val bits) m⊑ p≼
                (defd-div-mod {P} {st} {q} {r} {val} {bits} {v₁} {v₂} {x}
                  fq fr q≢r rf) (proj₁ hc)
        steq = div-mod-power-of-two-bwd {P} {S} {st} {val} {bits} {q} {r}
                 {v₁} {v₂} fq fr q≢r (x , rf) hst
    in bwd-cont (div-mod-power-of-two val bits (q ∷ r ∷ [])) is dΓ oty af nd
         sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (persistent-hash al inputs output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , an , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (persistent-hash al inputs output) dom af nd
        (frs , ri) = allNat-resolve inputs teq an
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (persistent-hash al inputs output) is ss csat
        hst = holds-lower (sha256 output al inputs) m⊑ p≼
                (defd-sha256 {P} {st} {al} {inputs} {output} {v} {frs} fresh ri)
                (proj₁ hc)
        steq = persistent-hash-bwd {P} {S} {st} {al} {inputs} {output} {v} fresh
                 (frs , ri) hst
    in bwd-cont (persistent-hash al inputs output) is dΓ oty af nd sa' wt' teq
         dom steq tail tc

  bwd-go {P} {S} (keccak256 al inputs output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , an , wt') teq dom
    (bw-cons _ _ _ _ (v , refl) tail) csat =
    let fresh    = out-fresh-of (keccak256 al inputs output) dom af nd
        (frs , ri) = allNat-resolve inputs teq an
        (m⊑ , p≼) = spine-⊑ tail
        (hc , tc) = csOf-peel (keccak256 al inputs output) is ss csat
        hst = holds-lower (keccak output al inputs) m⊑ p≼
                (defd-keccak {P} {st} {al} {inputs} {output} {v} {frs} fresh ri)
                (proj₁ hc)
        steq = keccak256-bwd {P} {S} {st} {al} {inputs} {output} {v} fresh
                 (frs , ri) hst
    in bwd-cont (keccak256 al inputs output) is dΓ oty af nd sa' wt' teq
         dom steq tail tc

  bwd-go {P} {S} (impact guard inputs ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ (vals , gᶠ , ri , rg , inj₁ (tb , tm , refl)) tail) csat =
    let (_ , tc) = csOf-peel (impact guard inputs) is ss csat
        steq = impact-true-bwd {P} {S} {st} {guard} {inputs} {gᶠ} {vals}
                 ri rg tb tm
    in bwd-cont (impact guard inputs) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (impact guard inputs ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ (vals , gᶠ , ri , rg , inj₂ (tb , refl)) tail) csat =
    let (_ , tc) = csOf-peel (impact guard inputs) is ss csat
        steq = impact-false-bwd {P} {S} {st} {guard} {inputs} {gᶠ} {vals}
                 ri rg tb
    in bwd-cont (impact guard inputs) is dΓ oty af nd sa' wt' teq dom steq tail tc

  bwd-go {P} {S} (public-input guard val-t output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ (inj₁ (eg , v , dv , refl)) tail) csat =
    let (_ , tc) = csOf-peel (public-input guard val-t output) is ss csat
        steq = public-input-active-bwd {P} {S} {st} {guard} {val-t} {output} {v}
                 eg dv
    in bwd-cont (public-input guard val-t output) is dΓ oty af nd sa' wt' teq
         dom steq tail tc

  bwd-go {P} {S} (public-input guard val-t output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ (inj₂ (eg , refl)) tail) csat =
    let (_ , tc) = csOf-peel (public-input guard val-t output) is ss csat
        steq = public-input-inactive-bwd {P} {S} {st} {guard} {val-t} {output} eg
    in bwd-cont (public-input guard val-t output) is dΓ oty af nd sa' wt' teq
         dom steq tail tc

  bwd-go {P} {S} (private-input guard val-t output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ (inj₁ (eg , v , dv , refl)) tail) csat =
    let (_ , tc) = csOf-peel (private-input guard val-t output) is ss csat
        steq = private-input-active-bwd {P} {S} {st} {guard} {val-t} {output} {v}
                 eg dv
    in bwd-cont (private-input guard val-t output) is dΓ oty af nd sa' wt' teq
         dom steq tail tc

  bwd-go {P} {S} (private-input guard val-t output ∷ is) st Γ bound ss dΓ
    (af , nd , sa') (Γ' , oty , _ , wt') teq dom
    (bw-cons _ _ _ _ (inj₂ (eg , refl)) tail) csat =
    let (_ , tc) = csOf-peel (private-input guard val-t output) is ss csat
        steq = private-input-inactive-bwd {P} {S} {st} {guard} {val-t} {output} eg
    in bwd-cont (private-input guard val-t output) is dΓ oty af nd sa' wt' teq
         dom steq tail tc

  bwd-go {P} {S} (circuit-output vals ∷ is) st Γ bound ss dΓ (af , nd , sa')
    (Γ' , oty , _ , wt') teq dom (bw-cons _ _ _ _ (vs , co , refl) tail) csat =
    let (_ , tc) = csOf-peel (circuit-output vals) is ss csat
        steq = circuit-output-bwd {P} {S} {st} {vals} {vs} co
    in bwd-cont (circuit-output vals) is dΓ oty af nd sa' wt' teq dom steq tail tc

  -- Mis-arity div-mod: `step` fails, so the spine side-data is `⊥`.
  bwd-go (div-mod-power-of-two val bits [] ∷ is) st Γ bound ss dΓ sa wt teq
    dom (bw-cons _ _ _ _ () _) csat
  bwd-go (div-mod-power-of-two val bits (_ ∷ []) ∷ is) st Γ bound ss dΓ sa wt
    teq dom (bw-cons _ _ _ _ () _) csat
  bwd-go (div-mod-power-of-two val bits (_ ∷ _ ∷ _ ∷ _) ∷ is) st Γ bound ss dΓ
    sa wt teq dom (bw-cons _ _ _ _ () _) csat

  -- The uniform tail: advance the invariants past `i` and recurse.
  bwd-cont : ∀ {P S s} i is {st st′ Γ Γ' bound ss}
    → dom-ty Γ ≡ bound
    → outtys Γ i ≡ just Γ'
    → All (λ o → ¬ (o ∈ bound)) (outs-of i) → NoDup (outs-of i)
    → SA (bound ++ outs-of i) is → WT Γ' is
    → TyEq (State.mem st) Γ → DomEq (State.mem st) bound
    → step P S st i ≡ just st′
    → BwdWalk P S st′ is s
    → satisfies-constraints (csOf is (synth-instr i ss)) (witness-of P s)
    → run P S st (i ∷ is) ≡ just s
  bwd-cont i is {st} {st′} {Γ} {Γ'} {bound} {ss} dΓ oty af nd sa' wt' teq dom
    steq tail tc =
    run-cons i is steq
      (bwd-go is st′ Γ' (bound ++ outs-of i) (synth-instr i ss)
        (trans (outtys-dom Γ i oty) (cong (_++ outs-of i) dΓ))
        sa' wt' (step-ty i oty steq teq (all∉-cast (outs-of i) dΓ af) nd)
        (step-dom i steq dom) tail tc)

------------------------------------------------------------------------
-- Backward faithfulness: the converse of `forward`.
--
-- `preprocess` enforces the terminal transcript-consumption conditions
-- (`Consumed`); `backward` reconstructs — from a well-typed producer, the
-- initial state, that terminal consumption, and the backward spine — the
-- run of a satisfying witness, assembling the `run-shaped` record.
------------------------------------------------------------------------

backward : ∀ {S P s st0}
  → producer-WT S
  → init S P ≡ just st0
  → Consumed P s
  → BwdWalk P S st0 (IrSource.instructions S) s
  → satisfies (synth S) (witness-of P s)
  → run-shaped S P s
backward {S} {P} {s} {st0} ((ndup , sa) , wt) ieq cons spine sat =
  mk-run-shaped st0 ieq (walk (IrSource.do-communications-commitment S) refl)
    cons
  where
  walk : (b : Bool) → IrSource.do-communications-commitment S ≡ b
       → run P S st0 (IrSource.instructions S) ≡ just s
  walk false hc =
    bwd-go {P} {S} {s} (IrSource.instructions S) st0
      (input-ctx (IrSource.inputs S))
      (map TypedIdentifier.name (IrSource.inputs S)) ss₁
      (dom-ty-input-ctx (IrSource.inputs S)) sa wt
      (init-ty {S} {P} {st0} ieq) (init-dom {S} {P} {st0} ieq) spine
      (csOf-from-sat-false {S} {witness-of P s} hc sat)
  walk true hc =
    bwd-go {P} {S} {s} (IrSource.instructions S) st0
      (input-ctx (IrSource.inputs S))
      (map TypedIdentifier.name (IrSource.inputs S)) ss₂
      (dom-ty-input-ctx (IrSource.inputs S)) sa wt
      (init-ty {S} {P} {st0} ieq) (init-dom {S} {P} {st0} ieq) spine
      (csOf-from-sat-true {S} {witness-of P s} hc sat)

------------------------------------------------------------------------
-- From a genuine run back to `preprocess`.
--
-- `preprocess` is `init >>= run >>=` the terminal checks TC1 (transcripts
-- consumed) and TC2 (the commitment binds the inputs and outputs).  Given
-- the run and the terminal facts, those checks all pass, so `preprocess`
-- returns exactly the run's final state.  `Consumed` supplies TC1; the
-- final argument supplies TC2 in the has-comm case (vacuous otherwise).
------------------------------------------------------------------------

run→preprocess : ∀ {S P s st0}
  → init S P ≡ just st0
  → run P S st0 (IrSource.instructions S) ≡ just s
  → Consumed P s
  → (IrSource.do-communications-commitment S ≡ true
       → ∃₂ λ c r
         → ProofPreimage.comm-commitment P ≡ just (c , r)
         × c ≡ transient-commit
                 (ProofPreimage.inputs P
                   ++ concatMap encodeᵉ (State.outs s)) r)
  → preprocess S P ≡ just s
run→preprocess {S} {P} {s} {st0} ieq req (pti≡ , pto≡ , priv≡) tc2
  rewrite ieq | req
  with State.pti-idx s ≟ℕ length (ProofPreimage.pub-transcript-inputs P)
... | no ¬p = case ¬p pti≡ of λ ()
... | yes _
  rewrite pto≡ | priv≡
  with IrSource.do-communications-commitment S
...   | false = refl
...   | true with tc2 refl
...     | (c , r , cceq , tc2eq) rewrite cceq
  with c ≟ᶠ transient-commit
             (ProofPreimage.inputs P ++ concatMap encodeᵉ (State.outs s)) r
...       | yes _ = refl
...       | no ¬q = case ¬q tc2eq of λ ()

------------------------------------------------------------------------
-- The terminal commitment check (TC2), inverted from `satisfies`.
--
-- This is the converse use of `comm-fwd`'s machinery: rather than deriving
-- the `comm` constraint from TC2, we start from the `comm` constraint that
-- `satisfies` provides (`comm-from-sat`) and recover TC2.  The commitment
-- preimage is pinned against the genuine run just as in `comm-fwd`: the
-- inputs re-encode to `ProofPreimage.inputs P` (`decode-reencode`), the
-- outputs to `concatMap encode (outs s)` (`out-go`), the randomness to `r`,
-- and π[1] to `c` (`init-pi1`, monotone along the run).
------------------------------------------------------------------------

-- Under the commitment flag, a missing commitment makes `init` fail.
init-nothing-no-comm : ∀ {S P}
  → IrSource.do-communications-commitment S ≡ true
  → ProofPreimage.comm-commitment P ≡ nothing
  → init S P ≡ nothing
init-nothing-no-comm {S} {P} hc cc
  with decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
... | nothing = refl
... | just m
  with IrSource.do-communications-commitment S | hc
...   | true | refl
  with ProofPreimage.comm-commitment P | cc
...     | nothing | refl = refl

-- Under the commitment flag, a successful `init` exposes the commitment.
-- Matching the commitment as a helper *argument* (not by `with` on the
-- projection) keeps `comm-commitment P` out of the abstracted goal, so the
-- present-case equation can be returned directly.
init-comm : ∀ {S P st0}
  → IrSource.do-communications-commitment S ≡ true
  → init S P ≡ just st0
  → ∃₂ λ c r → ProofPreimage.comm-commitment P ≡ just (c , r)
init-comm {S} {P} {st0} hc ieq = go (ProofPreimage.comm-commitment P) refl
  where
  go : (mc : _) → ProofPreimage.comm-commitment P ≡ mc
     → ∃₂ λ c r → ProofPreimage.comm-commitment P ≡ just (c , r)
  go (just (c , r)) cc = c , r , cc
  go nothing        cc =
    case trans (sym (init-nothing-no-comm {S} {P} hc cc)) ieq of λ ()

-- The `comm` constraint holds at the run's witness, extracted from
-- `satisfies` by peeling the trailing commitment constraint that `synth`
-- appends (the mirror of `csOf-from-sat-true`, which discards it).
comm-from-sat : ∀ {S w}
  → IrSource.do-communications-commitment S ≡ true
  → satisfies (synth S) w
  → holds w
      (comm (input-operands (IrSource.inputs S))
        (SynthState.output-ops
          (synth-instrs (IrSource.instructions S) ss₂)))
comm-from-sat {S} {w} hc sat =
  proj₂ (proj₂ (synth-true-split {S} {w} hc sat))

comm-tc2 : ∀ {S P s st0}
  → IrSource.do-communications-commitment S ≡ true
  → NoDup (map TypedIdentifier.name (IrSource.inputs S))
  → init S P ≡ just st0
  → run P S st0 (IrSource.instructions S) ≡ just s
  → WF-run P S (IrSource.instructions S) st0
  → satisfies (synth S) (witness-of P s)
  → ∃₂ λ c r → ProofPreimage.comm-commitment P ≡ just (c , r)
       × c ≡ transient-commit
               (ProofPreimage.inputs P
                 ++ concatMap encodeᵉ (State.outs s)) r
comm-tc2 {S} {P} {s} {st0} hc nd ieq req wf sat
  with init-comm {S} {P} {st0} hc ieq
... | (c , r , cceq) = c , r , cceq , tc2eq
  where
  w : _
  w = witness-of P s

  ext : (State.mem st0 ⊑ State.mem s) × (State.pis st0 ≼ State.pis s)
  ext = run-extends {P} {S} {s} (IrSource.instructions S) st0 req wf

  ivs-≡ : resolve-encode w (input-operands (IrSource.inputs S))
            ≡ just (ProofPreimage.inputs P)
  ivs-≡ = decode-reencode {w} (IrSource.inputs S)
            (ProofPreimage.inputs P) (State.mem st0)
            (init-decode {S} {P} {st0} ieq) nd (proj₁ ext)

  ovs-≡ : resolve-encode w
            (SynthState.output-ops
              (synth-instrs (IrSource.instructions S) ss₂))
            ≡ just (concatMap encodeᵉ (State.outs s))
  ovs-≡ = out-go {P} {S} {s} (IrSource.instructions S) st0 ss₂ req wf seed₀
    where
    seed₀ : resolve-encode w []
              ≡ just (concatMap encodeᵉ (State.outs st0))
    seed₀ rewrite init-outs {S} {P} {st0} ieq = refl

  rand-≡ : comm-rand-of (ProofPreimage.comm-commitment P) ≡ just r
  rand-≡ rewrite cceq = refl

  pi1-≡ : pi-lookup (State.pis s) 1 ≡ just c
  pi1-≡ = pi-lookup-mono (proj₂ ext)
            (init-pi1 {S} {P} {st0} {c} {r} hc cceq ieq)

  tc2eq : c ≡ transient-commit
                (ProofPreimage.inputs P
                  ++ concatMap encodeᵉ (State.outs s)) r
  tc2eq with comm-from-sat {S} {w} hc sat
  ... | (ivs , ovs , rv , pv , ri , ro , rr , rp , eqn) =
    trans (sym (just-injective (trans (sym rp) pi1-≡)))
      (trans eqn
        (cong₂ transient-commit
          (cong₂ _++_ (just-injective (trans (sym ri) ivs-≡))
                      (just-injective (trans (sym ro) ovs-≡)))
          (just-injective (trans (sym rr) rand-≡))))

------------------------------------------------------------------------
-- Canonical-witness circuit faithfulness (the v3 analogue of v2's P5
-- `circuit-faithful`).
--
-- For a well-typed producer whose backward spine and terminal consumption
-- are given, a successful `preprocess` and satisfaction of the synthesised
-- circuit at the run's canonical witness `witness-of P s` are equivalent.
--
--   ⇒  is `forward-sa` (its `producer-SA` premise is the first component
--      of `producer-WT`);
--   ⇐  runs `backward` to reconstruct the run (`init` is deterministic, so
--      the reconstructed start coincides with `st0`), then `run→preprocess`
--      re-runs the terminal checks, with `comm-tc2` supplying TC2 in the
--      has-comm case.
--
-- The spine and `Consumed` play the role of v2's `preprocess-shaped`
-- hypothesis: they are the shape data `satisfies` alone does not
-- determine.  From a successful run they are projected by
-- `preprocess→BwdWalk` (the v2 `R⇒preprocess-shaped` analogue), so a
-- caller holding a run needs no hand-built spine.  As in v2 the result
-- is a logical
-- equivalence (`_⇔_`), not a type isomorphism: the two directions are
-- proofs of distinct `Set`-valued relations, so no round-trip identity is
-- asserted.  Because `P` and `s` are fixed on both sides, this is a genuine
-- iff at a single witness — unlike the `statement-sound`/`forward-sa` pair,
-- whose witnesses differ and share only their public inputs.
------------------------------------------------------------------------

circuit-faithful : ∀ {S P s st0}
  → producer-WT S
  → init S P ≡ just st0
  → BwdWalk P S st0 (IrSource.instructions S) s
  → Consumed P s
  → (preprocess S P ≡ just s) ⇔ (satisfies (synth S) (witness-of P s))
circuit-faithful {S} {P} {s} {st0} wt ieq spine cons =
  mk⇔ (λ peq → forward-sa (proj₁ wt) ieq peq)
      (λ sat → ⇐ sat)
  where
  ⇐ : satisfies (synth S) (witness-of P s) → preprocess S P ≡ just s
  ⇐ sat =
    let rs   = backward wt ieq cons spine sat
        run≡ : run P S st0 (IrSource.instructions S) ≡ just s
        run≡ = run-shaped.walk rs
    in run→preprocess {S} {P} {s} {st0} ieq run≡ cons
         (λ hc → comm-tc2 {S} {P} {s} {st0} hc (proj₁ (proj₁ wt)) ieq run≡
                   (producer-safe→WF {S} {P} {st0} (proj₁ wt) ieq) sat)
