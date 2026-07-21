{-# OPTIONS --safe #-}
open import zkir-v2.Assumptions

module zkir-v2.StatementSoundness (⋯ : _) (open Assumptions ⋯) where

------------------------------------------------------------------------
-- Statement soundness for ZKIR v2.
--
--   circuit-statement-sound
--     : ∀ src w → T (producer-safe src)
--     → WF2 src
--     → satisfies (circuit src) w
--     → Realizer src w
--
-- where a `Realizer src w` (a record, defined before the theorem)
-- packages a preprocess run together with proofs that it is genuine
-- (`R src pre s`) and that its canonical witness is exactly `w`.
--
-- P5 (`circuit-faithful`, CircuitProof.agda) is an iff for the SINGLE
-- canonical witness `witness-of s pre`; it does not rule out *other*
-- satisfying witnesses.  Statement soundness is the universal property
-- P5 lacks: EVERY satisfying witness `w` of the synthesized circuit
-- (with the allocated memory length — see the note at the end of this
-- file) is *realizable*, i.e. the canonical witness of a genuine
-- preprocess run with the same public inputs.  Combined with P5's
-- forward direction this yields the exact characterization
-- `circuit-witness-characterization` (end of file): the satisfying
-- witnesses of the allocated length are EXACTLY the canonical witnesses
-- of preprocess runs.
--
-- Strategy.  Exhibit a preimage / state pair `(pre , s)` with
-- `witness-of s pre ≡ w` and `preprocess-shaped src pre s`; P5's
-- backward direction (`circuit-faithful-bwd`, packaged as
-- `sstmt-from-realize`) then turns `satisfies (circuit src) w` into
-- `R src pre s`.  The work is building the `Tr-shaped` shape walk from
-- `w`: two pre-passes read off `w` the transcripts `pre` must carry
-- (`make-trfacts` for the guarded `public-input` / `private-input`
-- reads, `make-pti` for the `pi-skip` verifier transcript), and `build`
-- walks the instruction stream, tiling the memory and pis suffixes of
-- `w` cell-by-cell and consuming those transcripts.  The construction
-- is assembled, generically in the communications-commitment flag, by
-- `realize` at the end of the file.
------------------------------------------------------------------------

open import zkir-v2.Syntax ⋯
open import zkir-v2.Semantics ⋯
  using ( ProofPreimage; Preprocessed; mk-state; init-state
        ; transcripts-consumed; R; push-mem; push-mem2; mem-lookup
        ; eval-guard; consume-pub-out; consume-priv; from-bool
        ; _≡ᶠ-list?_; WF2 )
open import zkir-v2.SemanticsLemmas ⋯
  using ( consume-pub-out-eq; consume-priv-eq )
open import zkir-v2.Circuit ⋯
  using ( Witness; satisfies; circuit; Circuit
        ; SynthState; mk-synth; circuit-instr; circuit-instrs
        ; is-bit; holds; satisfies-constraints; Constraint; guard-disj
        ; test-eq; less-than; is-zero; select
        ; boolean; Maybe-shape; wire; _≑_
        ; ≑wire-inv
        ; _at_↦_ )
open import zkir-v2.FieldProperties ⋯
  using ( to-bool; _≡ᶠ?_; bits-lt; lt-bits; ≡ᶠ?-refl; -ᶠ-zero
        ; +-zero-r )
open import zkir-v2.Encoding ⋯
  using ( to-bool-of-0ᶠ; to-bool-of-1ᶠ )
open import zkir-v2.Obligations ⋯
  using ( producer-safe; Δmem
        ; Wire-Trace; wire-done; wire-cons; wire-step; wire-scan
        ; WireOK; wireOK?; wire-disc
        ; O1-scan
        ; O1-Trace; o1-nil; o1-decl; o1-skip; o1-other
        ; NotDeclSkip; o1-tail
        ; IndexSet; insert; require-∈; O2-check; O2-record
        ; O2-Trace; o2-step; O2-Runs
        ; o1-sound; O2-bool→Runs )
open import zkir-v2.ObligationsSoundness ⋯
  using ( producer-safe-wire-disc; producer-safe-O1
        ; producer-safe-O2
        ; from-bool-is-bit )
open import zkir-v2.CircuitProof ⋯
  using ( witness-of; comm-rand-of; preprocess-shaped; mk-shaped
        ; mk-shape-walk; circuit-faithful-bwd
        ; circuit-faithful-fwd; Tr-shaped; tr-cons; tr-nil
        ; tr-step; tr-next )
open import zkir-v2.Properties ⋯
  using ( Δmem-sum; R-memory-length )

open import Data.List.Relation.Unary.Any using (here; there)
open import Data.List.Relation.Unary.All using (All)
  renaming ([] to []ᴬ; _∷_ to _∷ᴬ_; head to headᴬ)
open import Data.List.Relation.Unary.All.Properties using (++⁻)
import Data.Nat as ℕ
open import Data.List.Membership.DecPropositional (ℕ._≟_) using (_∈_; _∈?_)

open import Data.Bool    using (Bool; true; false; T; if_then_else_)
open import Data.Sum     using (_⊎_; inj₁; inj₂)
open import Data.List    using (List; []; _∷_; _++_; length; take; drop)
open import Data.List.Properties
  using ( ++-assoc; ++-identityʳ; take++drop≡id; length-take; length-drop
        ; length-++ )
open import Data.Maybe   using (Maybe; just; nothing; fromMaybe)
open import Data.Maybe.Properties using (just-injective)
open import Data.Nat     using (ℕ; zero; suc; _+_; _∸_; _≤_; _<_; s≤s; _≡ᵇ_)
open import Data.Nat.Properties
  using (≡⇒≡ᵇ; ≡ᵇ⇒≡; +-identityʳ; +-suc; +-comm; +-assoc; suc-injective
        ; m+n∸m≡n; m+n∸n≡m; m≤n⇒m⊓n≡m; m≤m+n)
open import Data.Bool.Properties using (T-≡; T-∧)
open import Data.Empty   using (⊥; ⊥-elim)
open import Data.Unit    using (⊤; tt)
open import Relation.Nullary using (yes; no)
open import Function.Bundles     using (Equivalence; _⇔_; mk⇔)
open import Data.Product using (_×_; _,_; proj₁; proj₂; Σ)
open import Relation.Binary.PropositionalEquality
  using ( _≡_; refl; sym; trans; cong; cong₂; subst; subst₂
        ; module ≡-Reasoning )

------------------------------------------------------------------------
-- Wire accounting.  Synthesis allocates exactly `Δmem i` fresh wires per
-- instruction, so the circuit's wire count is `num-inputs` plus the
-- per-instruction `Δmem` sum (`Δmem-sum`, from `Properties`).  This
-- fixes the length of the memory suffix the builder must tile from
-- `Witness.mem w`.
------------------------------------------------------------------------

-- Declared-PI delta: `declare-pub-input` appends one pis cell (and bumps
-- `pub-in-idx`); every other instruction appends none.
Δpis : Instruction → ℕ
Δpis (declare-pub-input _) = 1
Δpis _                     = 0

Δpis-sum : List Instruction → ℕ
Δpis-sum []       = 0
Δpis-sum (i ∷ is) = Δpis i + Δpis-sum is

private
  nr-wires-step : ∀ (hc : Bool) (i : Instruction) (st : SynthState)
    → SynthState.nr-wires (circuit-instr hc i st)
      ≡ SynthState.nr-wires st + Δmem i
  nr-wires-step hc (assert _)               st = sym (+-identityʳ _)
  nr-wires-step hc (constrain-bits _ _)     st = sym (+-identityʳ _)
  nr-wires-step hc (constrain-eq _ _)       st = sym (+-identityʳ _)
  nr-wires-step hc (constrain-to-boolean _) st = sym (+-identityʳ _)
  nr-wires-step hc (declare-pub-input _)    st = sym (+-identityʳ _)
  nr-wires-step hc (pi-skip _ _)            st = sym (+-identityʳ _)
  nr-wires-step hc (output _)               st = sym (+-identityʳ _)
  nr-wires-step hc (cond-select _ _ _)      st = refl
  nr-wires-step hc (copy _)                 st = refl
  nr-wires-step hc (load-imm _)             st = refl
  nr-wires-step hc (reconstitute-field _ _ _) st = refl
  nr-wires-step hc (transient-hash _)       st = refl
  nr-wires-step hc (test-eq _ _)            st = refl
  nr-wires-step hc (add _ _)                st = refl
  nr-wires-step hc (mul _ _)                st = refl
  nr-wires-step hc (neg _)                  st = refl
  nr-wires-step hc (not _)                  st = refl
  nr-wires-step hc (less-than _ _ _)        st = refl
  nr-wires-step hc (public-input nothing)   st = refl
  nr-wires-step hc (public-input (just _))  st = refl
  nr-wires-step hc (private-input nothing)  st = refl
  nr-wires-step hc (private-input (just _)) st = refl
  nr-wires-step hc (ec-add _ _ _ _)         st = refl
  nr-wires-step hc (ec-mul _ _ _)           st = refl
  nr-wires-step hc (ec-mul-generator _)     st = refl
  nr-wires-step hc (hash-to-curve _)        st = refl
  nr-wires-step hc (div-mod-power-of-two _ _) st = refl
  nr-wires-step hc (persistent-hash _ _)    st = refl

nr-wires-acc : ∀ (hc : Bool) (is : List Instruction) (st : SynthState)
  → SynthState.nr-wires (circuit-instrs hc is st)
    ≡ SynthState.nr-wires st + Δmem-sum is
nr-wires-acc hc []       st = sym (+-identityʳ _)
nr-wires-acc hc (i ∷ is) st =
  let open ≡-Reasoning in
  begin
    SynthState.nr-wires (circuit-instrs hc is (circuit-instr hc i st))
  ≡⟨ nr-wires-acc hc is (circuit-instr hc i st) ⟩
    SynthState.nr-wires (circuit-instr hc i st) + Δmem-sum is
  ≡⟨ cong (_+ Δmem-sum is) (nr-wires-step hc i st) ⟩
    (SynthState.nr-wires st + Δmem i) + Δmem-sum is
  ≡⟨ +-assoc (SynthState.nr-wires st) (Δmem i) (Δmem-sum is) ⟩
    SynthState.nr-wires st + (Δmem i + Δmem-sum is)
  ∎

-- P2's exact-Δmem form (`R-memory-length`) matched against the
-- synthesis-side wire count: a run's memory length is pinned to the
-- circuit's `nr-wires` — the witness-shape fact `Witness.mem` must
-- respect (the `mem-length` conjunct of `satisfies`).
--
-- Stated for the spec (§6.1, P2): the proofs below use
-- `satisfies.mem-length` + `nr-wires-acc` directly rather than this
-- packaging.
R-mem-nr-wires : ∀ src pre s → R src pre s
  → length (Preprocessed.memory s) ≡ Circuit.nr-wires (circuit src)
R-mem-nr-wires src pre s r =
  trans (R-memory-length src pre s r)
        (sym (nr-wires-acc (IrSource.do-communications-commitment src)
               (IrSource.instructions src)
               (mk-synth (IrSource.num-inputs src) [] 0 [])))

private
  -- `declare-pub-input` is the only instruction that allocates a declared
  -- PI; synthesis bumps `nr-declared-pi` by `Δpis i` per instruction.
  nr-decl-step : ∀ (hc : Bool) (i : Instruction) (st : SynthState)
    → SynthState.nr-declared-pi (circuit-instr hc i st)
      ≡ SynthState.nr-declared-pi st + Δpis i
  nr-decl-step hc (declare-pub-input _)    st = sym (+-comm (SynthState.nr-declared-pi st) 1)
  nr-decl-step hc (assert _)               st = sym (+-identityʳ _)
  nr-decl-step hc (constrain-bits _ _)     st = sym (+-identityʳ _)
  nr-decl-step hc (constrain-eq _ _)       st = sym (+-identityʳ _)
  nr-decl-step hc (constrain-to-boolean _) st = sym (+-identityʳ _)
  nr-decl-step hc (copy _)                 st = sym (+-identityʳ _)
  nr-decl-step hc (load-imm _)             st = sym (+-identityʳ _)
  nr-decl-step hc (transient-hash _)       st = sym (+-identityʳ _)
  nr-decl-step hc (cond-select _ _ _)      st = sym (+-identityʳ _)
  nr-decl-step hc (not _)                  st = sym (+-identityʳ _)
  nr-decl-step hc (less-than _ _ _)        st = sym (+-identityʳ _)
  nr-decl-step hc (neg _)                  st = sym (+-identityʳ _)
  nr-decl-step hc (reconstitute-field _ _ _) st = sym (+-identityʳ _)
  nr-decl-step hc (test-eq _ _)            st = sym (+-identityʳ _)
  nr-decl-step hc (add _ _)                st = sym (+-identityʳ _)
  nr-decl-step hc (mul _ _)                st = sym (+-identityʳ _)
  nr-decl-step hc (ec-add _ _ _ _)         st = sym (+-identityʳ _)
  nr-decl-step hc (ec-mul _ _ _)           st = sym (+-identityʳ _)
  nr-decl-step hc (ec-mul-generator _)     st = sym (+-identityʳ _)
  nr-decl-step hc (hash-to-curve _)        st = sym (+-identityʳ _)
  nr-decl-step hc (persistent-hash _ _)    st = sym (+-identityʳ _)
  nr-decl-step hc (div-mod-power-of-two _ _) st = sym (+-identityʳ _)
  nr-decl-step hc (output _)               st = sym (+-identityʳ _)
  nr-decl-step hc (public-input nothing)   st = sym (+-identityʳ _)
  nr-decl-step hc (public-input (just _))  st = sym (+-identityʳ _)
  nr-decl-step hc (private-input nothing)  st = sym (+-identityʳ _)
  nr-decl-step hc (private-input (just _)) st = sym (+-identityʳ _)
  nr-decl-step hc (pi-skip _ _)            st = sym (+-identityʳ _)

  nr-decl-acc : ∀ (hc : Bool) (is : List Instruction) (st : SynthState)
    → SynthState.nr-declared-pi (circuit-instrs hc is st)
      ≡ SynthState.nr-declared-pi st + Δpis-sum is
  nr-decl-acc hc []       st = sym (+-identityʳ _)
  nr-decl-acc hc (i ∷ is) st =
    let open ≡-Reasoning in
    begin
      SynthState.nr-declared-pi (circuit-instrs hc is (circuit-instr hc i st))
    ≡⟨ nr-decl-acc hc is (circuit-instr hc i st) ⟩
      SynthState.nr-declared-pi (circuit-instr hc i st) + Δpis-sum is
    ≡⟨ cong (_+ Δpis-sum is) (nr-decl-step hc i st) ⟩
      (SynthState.nr-declared-pi st + Δpis i) + Δpis-sum is
    ≡⟨ +-assoc (SynthState.nr-declared-pi st) (Δpis i) (Δpis-sum is) ⟩
      SynthState.nr-declared-pi st + (Δpis i + Δpis-sum is)
    ∎

------------------------------------------------------------------------
-- The walk builder.
--
-- `build` walks the instruction stream, consuming the memory suffix
-- `mem-rest` and pis suffix `pis-rest` of `Witness.mem w` /
-- `Witness.pis w` cell-by-cell (one `tr-cons` per instruction), and
-- returns the endpoint state with its memory and pis equal to `w`'s, its
-- `pub-in-idx` landing on the committed transcript length, and its
-- public-output / private transcripts fully consumed.  It handles every
-- instruction, threading four certificates:
--   • a `Wire-Trace` (the `wire-disc` bound, indexed by the wire count
--     `length (memory s)`) — so `output` / guarded inputs / skip guards
--     can look their operands up in the prefix built so far;
--   • a `TrFacts` (from `make-trfacts`) — the per-transcript-instruction
--     guard decision and consumed value, and the skip-guard booleanity;
--   • `pub-out-rem ≡ po` / `priv-rem ≡ pv` invariants, consumed to `[]`
--     by the active public/private-input steps; and
--   • a `PiInv` — the pi-skip transcript bookkeeping (O1's bracketing
--     trace, the committed/current-group split of `pub-in-idx`, and the
--     `make-pti` shape of `pre`'s verifier transcript).
------------------------------------------------------------------------

private
  split1 : ∀ {n} (xs : List Fr) → length xs ≡ suc n
    → Σ Fr (λ c → Σ (List Fr) (λ xs′ →
         (xs ≡ c ∷ xs′) × (length xs′ ≡ n)))
  split1 (c ∷ xs′) eq = c , xs′ , refl , suc-injective eq

  split2 : ∀ {n} (xs : List Fr) → length xs ≡ suc (suc n)
    → Σ Fr (λ a → Σ Fr (λ b → Σ (List Fr) (λ xs′ →
         (xs ≡ a ∷ b ∷ xs′) × (length xs′ ≡ n))))
  split2 (a ∷ b ∷ xs′) eq =
    a , b , xs′ , refl , suc-injective (suc-injective eq)

  -- A `mem-lookup` below the memory length always succeeds.
  mem-lookup-exists : ∀ (xs : List Fr) (i : ℕ) → i < length xs
    → Σ Fr (λ v → mem-lookup xs i ≡ just v)
  mem-lookup-exists (x ∷ _)  zero    _           = x , refl
  mem-lookup-exists (_ ∷ xs) (suc i) (s≤s i<len) =
    mem-lookup-exists xs i i<len

  -- The producer's `wire-disc` obligation, reflected into a `Wire-Trace`
  -- (the witness-bearing form of the linear wire scan).  The bound
  -- `WireOK i n` it threads at each step is exactly `i`'s operands
  -- bounded by the wire count `n` allocated before `i`.
  wire-scan→trace : ∀ is n {final} → wire-scan is n ≡ just final
    → Wire-Trace is n final
  wire-scan→trace []       n refl = wire-done
  wire-scan→trace (i ∷ is) n eq with wire-step i n in step-eq
  ... | just n′ = wire-cons step-eq (wire-scan→trace is n′ eq)

  wire-bool→trace : ∀ {src} → T (wire-disc src)
    → Σ ℕ (λ final →
        Wire-Trace (IrSource.instructions src) (IrSource.num-inputs src) final)
  wire-bool→trace {src} eq
    with wire-scan (IrSource.instructions src) (IrSource.num-inputs src)
         in scan-eq
  ... | just final =
        final , wire-scan→trace (IrSource.instructions src)
                                (IrSource.num-inputs src) scan-eq

  wire-disc-sound : ∀ {src} → T (producer-safe src)
    → Σ ℕ (λ final →
        Wire-Trace (IrSource.instructions src) (IrSource.num-inputs src) final)
  wire-disc-sound {src} ps = wire-bool→trace {src} (producer-safe-wire-disc {src} ps)

  -- Peel the head step's `WireOK` premise and the residual trace at the
  -- bumped counter from a `Wire-Trace` over `i ∷ is`.
  wire-trace-head-ok : ∀ {i is n final}
    → Wire-Trace (i ∷ is) n final → WireOK i n
  wire-trace-head-ok {i} {n = n} (wire-cons step-eq _)
    with wireOK? i n | step-eq
  ... | yes ok | refl = ok

  wire-trace-tail : ∀ {i is n final}
    → Wire-Trace (i ∷ is) n final → Wire-Trace is (n + Δmem i) final
  wire-trace-tail {i} {n = n} (wire-cons step-eq tail)
    with wireOK? i n | step-eq
  ... | yes _ | refl = tail

  -- Retag the residual trace to index the *next* state's memory length.
  -- `wt-stay` covers a `Δmem i ≡ 0` step (memory unchanged); `wt-grow`
  -- covers a step that appends `cells` (with `length cells ≡ Δmem i`),
  -- so the next memory is `memory s ++ cells`.
  wt-stay : ∀ {i is final} (s : Preprocessed) → Δmem i ≡ 0
    → Wire-Trace (i ∷ is) (length (Preprocessed.memory s)) final
    → Wire-Trace is (length (Preprocessed.memory s)) final
  wt-stay s eq wt =
    subst (λ k → Wire-Trace _ k _)
      (trans (cong (length (Preprocessed.memory s) +_) eq) (+-identityʳ _))
      (wire-trace-tail wt)

  wt-grow : ∀ {i is final} (s : Preprocessed) (cells : List Fr)
    → length cells ≡ Δmem i
    → Wire-Trace (i ∷ is) (length (Preprocessed.memory s)) final
    → Wire-Trace is (length (Preprocessed.memory s ++ cells)) final
  wt-grow s cells eq wt =
    subst (λ k → Wire-Trace _ k _)
      (sym (trans (length-++ (Preprocessed.memory s) {cells})
                  (cong (length (Preprocessed.memory s) +_) eq)))
      (wire-trace-tail wt)

  ------------------------------------------------------------------------
  -- Transcript facts.  The transcript-reading instructions append a
  -- memory cell whose value is constrained relative to the verifier
  -- transcripts: an *active* `public-input` consumes the next
  -- `pub-transcript-outputs` entry (= the cell `w` carries there), and an
  -- *inactive* one is forced to `0ᶠ` by its guard-disj constraint.  `TrFacts`
  -- bundles, per transcript instruction, the guard decision and the
  -- consumed value, so the public-output / private supplies `po` / `pv`
  -- that `pre` must carry are read off as the walk's *index*.  Built by
  -- `make-trfacts` from `satisfies` (the guard-disj constraints) and consumed
  -- by `build`.
  ------------------------------------------------------------------------

  GuardActive : Witness → Maybe Index → Set
  GuardActive _ nothing   = ⊤
  GuardActive w (just g) =
    Σ Fr (λ iv → (Witness.mem w at g ↦ iv) × (iv ≡ 1ᶠ))

  GuardInactive : Witness → Maybe Index → Set
  GuardInactive _ nothing   = ⊥
  GuardInactive w (just g) =
    Σ Fr (λ iv → (Witness.mem w at g ↦ iv) × (iv ≡ 0ᶠ))

  -- `NotTranscript i` marks the instructions `build` already tiles from
  -- `mem-rest` (everything but the two transcript reads and `pi-skip`).
  NotTranscript : Instruction → Set
  NotTranscript (public-input _)  = ⊥
  NotTranscript (private-input _) = ⊥
  NotTranscript (pi-skip _ _)     = ⊥
  NotTranscript _                 = ⊤

  -- A guard wire's booleanity (trivial when unguarded).  The witness-level
  -- O2 machinery below (`BoolKnown` / `bk-step` / the `O2-Trace` peelers)
  -- produces this for a guarded `pi-skip`'s guard wire — the skip emits no
  -- constraint, so its `is-bit` cannot be read off `satisfies`; it comes from
  -- O2's `bool-known` set instead.
  IsBitGuard : Witness → Maybe Index → Set
  IsBitGuard _ nothing  = ⊤
  IsBitGuard w (just g) =
    Σ Fr (λ v → (Witness.mem w at g ↦ v) × is-bit v)

  data TrFacts (w : Witness)
       : List Instruction → ℕ → List Fr → List Fr → Set where
    tf-nil  : ∀ {n} → TrFacts w [] n [] []
    tf-other : ∀ {i is n po pv} → NotTranscript i
      → TrFacts w is (n + Δmem i) po pv → TrFacts w (i ∷ is) n po pv
    tf-pub-act : ∀ {g is n c po pv}
      → GuardActive w g → (Witness.mem w at n ↦ c)
      → TrFacts w is (suc n) po pv
      → TrFacts w (public-input g ∷ is) n (c ∷ po) pv
    tf-pub-inact : ∀ {g is n po pv}
      → GuardInactive w g → (Witness.mem w at n ↦ 0ᶠ)
      → TrFacts w is (suc n) po pv
      → TrFacts w (public-input g ∷ is) n po pv
    tf-priv-act : ∀ {g is n c po pv}
      → GuardActive w g → (Witness.mem w at n ↦ c)
      → TrFacts w is (suc n) po pv
      → TrFacts w (private-input g ∷ is) n po (c ∷ pv)
    tf-priv-inact : ∀ {g is n po pv}
      → GuardInactive w g → (Witness.mem w at n ↦ 0ᶠ)
      → TrFacts w is (suc n) po pv
      → TrFacts w (private-input g ∷ is) n po pv
    -- `pi-skip` appends no cell and consumes no transcript, but its guard
    -- (if present) must be `is-bit` for the operational guard evaluation
    -- to succeed (`to-bool` is `nothing` off {0,1}); the skip emits no
    -- constraint, so this comes from O2's `bool-known` set, not `satisfies`.
    tf-skip : ∀ {g count is n po pv}
      → IsBitGuard w g
      → TrFacts w is n po pv
      → TrFacts w (pi-skip g count ∷ is) n po pv

  -- The transcript supplies the preimage must carry, read off `w`: the
  -- public-output / private supplies together with the `TrFacts` walk
  -- they index.
  record TrSupply (w : Witness) (is : List Instruction) (n : ℕ) : Set where
    constructor mk-tr-supply
    field
      pub-out : List Fr
      priv    : List Fr
      facts   : TrFacts w is n pub-out priv

  -- Peel a satisfies-constraints over an appended list.
  sat-split : ∀ {w} (xs ys : List Constraint)
    → satisfies-constraints (xs ++ ys) w
    → satisfies-constraints xs w × satisfies-constraints ys w
  sat-split xs _ = ++⁻ xs

  -- One synthesis step appends a (possibly empty) constraint list.
  constraint-extend : ∀ {hc} (i : Instruction) (st : SynthState)
    → Σ (List Constraint) (λ new →
        SynthState.constraints (circuit-instr hc i st)
          ≡ SynthState.constraints st ++ new)
  constraint-extend (assert _)               st = _ , refl
  constraint-extend (cond-select _ _ _)      st = _ , refl
  constraint-extend (constrain-bits _ _)     st = _ , refl
  constraint-extend (constrain-eq _ _)       st = _ , refl
  constraint-extend (constrain-to-boolean _) st = _ , refl
  constraint-extend (copy _)                 st = _ , refl
  constraint-extend (declare-pub-input _)    st = _ , refl
  constraint-extend (pi-skip _ _)            st = _ , sym (++-identityʳ _)
  constraint-extend (ec-add _ _ _ _)         st = _ , refl
  constraint-extend (ec-mul _ _ _)           st = _ , refl
  constraint-extend (ec-mul-generator _)     st = _ , refl
  constraint-extend (hash-to-curve _)        st = _ , refl
  constraint-extend (load-imm _)             st = _ , refl
  constraint-extend (div-mod-power-of-two _ _) st = _ , refl
  constraint-extend (reconstitute-field _ _ _) st = _ , refl
  constraint-extend (output _)               st = _ , sym (++-identityʳ _)
  constraint-extend (transient-hash _)       st = _ , refl
  constraint-extend (persistent-hash _ _)    st = _ , refl
  constraint-extend (test-eq _ _)            st = _ , refl
  constraint-extend (add _ _)                st = _ , refl
  constraint-extend (mul _ _)                st = _ , refl
  constraint-extend (neg _)                  st = _ , refl
  constraint-extend (not _)                  st = _ , refl
  constraint-extend (less-than _ _ _)        st = _ , refl
  constraint-extend (public-input nothing)   st = _ , sym (++-identityʳ _)
  constraint-extend (public-input (just _))  st = _ , refl
  constraint-extend (private-input nothing)  st = _ , sym (++-identityʳ _)
  constraint-extend (private-input (just _)) st = _ , refl

  -- Iterated: the whole instruction stream appends a tail.
  constraints-extend-list : ∀ {hc} (is : List Instruction) (st : SynthState)
    → Σ (List Constraint) (λ tail →
        SynthState.constraints (circuit-instrs hc is st)
          ≡ SynthState.constraints st ++ tail)
  constraints-extend-list []       st = [] , sym (++-identityʳ _)
  constraints-extend-list {hc} (i ∷ is) st =
    let (new  , eq)  = constraint-extend {hc} i st
        (tail , teq) = constraints-extend-list {hc} is (circuit-instr hc i st)
    in new ++ tail
     , trans teq (trans (cong (_++ tail) eq)
                        (++-assoc (SynthState.constraints st) new tail))

  -- The guard decision for a transcript instruction at wire `n`: either
  -- active (consuming the cell `c` it carries there) or inactive (its
  -- cell forced to `0ᶠ`).
  GuardOutcome : Witness → Maybe Index → ℕ → Set
  GuardOutcome w g n =
      (Σ Fr (λ c → GuardActive w g × (Witness.mem w at n ↦ c)))
    ⊎ (GuardInactive w g × (Witness.mem w at n ↦ 0ᶠ))

  -- A guarded transcript instruction's guard-disj constraint decides the
  -- outcome: a `1`-guard is active; a `0`-guard is inactive and the
  -- disjunction `ov≡0 ∨ iv≡1` then forces the cell to `0ᶠ` (the `iv≡1`
  -- alternative would make `1ᶠ ≡ 0ᶠ`).
  classify-bit : ∀ (w : Witness) (gw n : Index) {ov iv : Fr}
    → (Witness.mem w at gw ↦ iv) → (Witness.mem w at n ↦ ov)
    → is-bit iv → (ov ≡ 0ᶠ) ⊎ (iv ≡ 1ᶠ)
    → GuardOutcome w (just gw) n
  classify-bit w gw n {ov} at-gw at-n (inj₂ iv≡1) _ =
    inj₁ (ov , (_ , at-gw , iv≡1) , at-n)
  classify-bit w gw n at-gw at-n (inj₁ iv≡0) (inj₁ ov≡0) =
    inj₂ ((_ , at-gw , iv≡0)
         , subst (λ z → Witness.mem w at n ↦ z) ov≡0 at-n)
  classify-bit w gw n at-gw at-n (inj₁ iv≡0) (inj₂ iv≡1) =
    ⊥-elim (1ᶠ≢0ᶠ (trans (sym iv≡1) iv≡0))

  -- Peel the head instruction's appended constraint list from `satisfies`:
  -- the `circuit-instr` constraints, dropping the tail.
  head-constraints-sat : ∀ {hc} (w : Witness) (i : Instruction)
      (is : List Instruction) (st : SynthState)
    → satisfies-constraints
        (SynthState.constraints (circuit-instrs hc (i ∷ is) st)) w
    → satisfies-constraints (SynthState.constraints (circuit-instr hc i st)) w
  head-constraints-sat {hc} w i is st sat =
    let st₁          = circuit-instr hc i st
        (tail , ceq) = constraints-extend-list {hc} is st₁
    in proj₁ (sat-split (SynthState.constraints st₁) tail
               (subst (λ cs → satisfies-constraints cs w) ceq sat))

  -- Extract a guarded transcript instruction's guard-disj constraint from
  -- `satisfies` and classify it.  `public-input (just gw)` and
  -- `private-input (just gw)` synthesise the *same* constraint, so this
  -- serves both.
  gd-outcome : ∀ {hc} (w : Witness) (gw : Index)
      (is : List Instruction) (st : SynthState) (n : ℕ)
    → n ≡ SynthState.nr-wires st
    → satisfies-constraints
        (SynthState.constraints (circuit-instrs hc (public-input (just gw) ∷ is) st)) w
    → GuardOutcome w (just gw) n
  gd-outcome {hc} w gw is st n n≡ sat =
    let sat-st₁ =
          head-constraints-sat {hc} w (public-input (just gw)) is st sat
        sat-gd  = proj₂ (sat-split (SynthState.constraints st)
                   (guard-disj (SynthState.nr-wires st) gw ∷ []) sat-st₁)
        (ov , iv , at-out , at-gw , bit , disj) = headᴬ sat-gd
    in classify-bit w gw n at-gw
         (subst (λ k → Witness.mem w at k ↦ ov) (sym n≡) at-out) bit disj

  -- `public-input g` head: decide the outcome and emit the matching
  -- `tf-pub-*`, given the tail facts.
  pub-decide : ∀ {hc} (w : Witness) (g : Maybe Index)
      (is : List Instruction) (st : SynthState) (n : ℕ)
    → n ≡ SynthState.nr-wires st
    → length (Witness.mem w) ≡ n + Δmem-sum (public-input g ∷ is)
    → satisfies-constraints
        (SynthState.constraints (circuit-instrs hc (public-input g ∷ is) st)) w
    → TrSupply w is (suc n)
    → TrSupply w (public-input g ∷ is) n
  pub-decide w nothing is st n n≡ l sat (mk-tr-supply po pv tf-tail) =
    let n<len : n < length (Witness.mem w)
        n<len = subst (n <_) (sym l)
                  (subst (n <_) (sym (+-suc n (Δmem-sum is)))
                    (s≤s (m≤m+n n (Δmem-sum is))))
        (c , at-c) = mem-lookup-exists (Witness.mem w) n n<len
    in mk-tr-supply (c ∷ po) pv (tf-pub-act tt at-c tf-tail)
  pub-decide {hc} w (just gw) is st n n≡ l sat (mk-tr-supply po pv tf-tail)
    with gd-outcome {hc} w gw is st n n≡ sat
  ... | inj₁ (c , ga , at-c) =
        mk-tr-supply (c ∷ po) pv (tf-pub-act ga at-c tf-tail)
  ... | inj₂ (gi , at-0)     =
        mk-tr-supply po pv (tf-pub-inact gi at-0 tf-tail)

  priv-decide : ∀ {hc} (w : Witness) (g : Maybe Index)
      (is : List Instruction) (st : SynthState) (n : ℕ)
    → n ≡ SynthState.nr-wires st
    → length (Witness.mem w) ≡ n + Δmem-sum (private-input g ∷ is)
    → satisfies-constraints
        (SynthState.constraints (circuit-instrs hc (private-input g ∷ is) st)) w
    → TrSupply w is (suc n)
    → TrSupply w (private-input g ∷ is) n
  priv-decide w nothing is st n n≡ l sat (mk-tr-supply po pv tf-tail) =
    let n<len : n < length (Witness.mem w)
        n<len = subst (n <_) (sym l)
                  (subst (n <_) (sym (+-suc n (Δmem-sum is)))
                    (s≤s (m≤m+n n (Δmem-sum is))))
        (c , at-c) = mem-lookup-exists (Witness.mem w) n n<len
    in mk-tr-supply po (c ∷ pv) (tf-priv-act tt at-c tf-tail)
  priv-decide {hc} w (just gw) is st n n≡ l sat (mk-tr-supply po pv tf-tail)
    with gd-outcome {hc} w gw is st n n≡ sat
  ... | inj₁ (c , ga , at-c) =
        mk-tr-supply po (c ∷ pv) (tf-priv-act ga at-c tf-tail)
  ... | inj₂ (gi , at-0)     =
        mk-tr-supply po pv (tf-priv-inact gi at-0 tf-tail)

  ------------------------------------------------------------------------
  -- Witness-level boolean-known invariant.  `pi-skip` emits no constraint, so
  -- the `is-bit` of a guarded skip's guard wire cannot be read off
  -- `satisfies` directly — it follows from O2, which requires the guard
  -- in its `bool-known` set, and that set's members are all `is-bit` in
  -- `w`.  `BoolKnown w bk` is the witness-level analogue of `O2-Inv`: it
  -- keys the bit-property to `Witness.mem w` (the full memory) rather than
  -- a growing operational state, so its preservation across `insert`
  -- needs no positional reasoning — a membership in `i ∷ bk` is either the
  -- fresh head (its `is-bit` supplied by the establishing constraint) or an
  -- old member (its `is-bit` inherited).
  ------------------------------------------------------------------------

  BoolKnown : Witness → IndexSet → Set
  BoolKnown w bk =
    ∀ {j} → j ∈ bk
    → Σ Fr (λ v → (Witness.mem w at j ↦ v) × is-bit v)

  bk-empty : ∀ {w} → BoolKnown w []
  bk-empty ()

  -- Extend by a fresh wire whose value is `is-bit`.
  bk-insert : ∀ {bk i v} (w : Witness) → (Witness.mem w at i ↦ v) → is-bit v
    → BoolKnown w bk → BoolKnown w (insert i bk)
  bk-insert w at-i bit bk (here refl)  = _ , at-i , bit
  bk-insert w at-i bit bk (there j∈)   = bk j∈

  -- Peel the single new constraint of a one-constraint instruction at wire `n`.
  -- Used for `test-eq` / `less-than` / `not` / `cond-select` (output wire
  -- `n`) and `constrain-to-boolean` / `copy` (operand wire).
  single-constraint-holds : ∀ {hc} (w : Witness) (i : Instruction)
      (is : List Instruction) (st : SynthState) (cl : Constraint)
    → SynthState.constraints (circuit-instr hc i st)
        ≡ SynthState.constraints st ++ (cl ∷ [])
    → satisfies-constraints
        (SynthState.constraints (circuit-instrs hc (i ∷ is) st)) w
    → holds w cl
  single-constraint-holds {hc} w i is st cl ceq sat =
    headᴬ (proj₂ (sat-split (SynthState.constraints st) (cl ∷ [])
            (subst (λ cs → satisfies-constraints cs w)
                   ceq (head-constraints-sat {hc} w i is st sat))))

  -- A wire equal to `from-bool b` is `is-bit`.  The Bool `b` is supplied
  -- explicitly because `from-bool` is opaque to unification.
  from-bool-eq-is-bit : ∀ {ov : Fr} (b : Bool) → ov ≡ from-bool b → is-bit ov
  from-bool-eq-is-bit b eq = subst is-bit (sym eq) (from-bool-is-bit b)

  -- The `cond-select` constraint output `(bv·av)+(1+(-bv))·cv` is `is-bit`
  -- when the bit `bv` and both branches `av`, `cv` are.  When `bv ≡ 0ᶠ`
  -- the expression reduces (by the field laws) to `cv`; when `bv ≡ 1ᶠ`,
  -- to `av`.
  mux-is-bit : ∀ {bv av cv ov : Fr} → is-bit bv → is-bit av → is-bit cv
    → ov ≡ (bv *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ bv)) *ᶠ cv) → is-bit ov
  mux-is-bit {bv} {av} {cv} (inj₁ bv≡0) _ cv-bit ov≡ =
    subst is-bit (sym (trans ov≡ reduce)) cv-bit
    where
      reduce : (bv *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ bv)) *ᶠ cv) ≡ cv
      reduce rewrite bv≡0 | -ᶠ-zero =
        trans (cong₂ _+ᶠ_ (*-zero-l av)
                  (trans (cong (_*ᶠ cv) (+-zero-r 1ᶠ)) (*-one-l cv)))
              (+-zero-l cv)
  mux-is-bit {bv} {av} {cv} (inj₂ bv≡1) av-bit _ ov≡ =
    subst is-bit (sym (trans ov≡ reduce)) av-bit
    where
      reduce : (bv *ᶠ av) +ᶠ ((1ᶠ +ᶠ (-ᶠ bv)) *ᶠ cv) ≡ av
      reduce rewrite bv≡1 =
        trans (cong₂ _+ᶠ_ (*-one-l av)
                  (trans (cong (_*ᶠ cv) (+-inv-r 1ᶠ)) (*-zero-l cv)))
              (+-zero-r av)

  -- `O2-record i n bk` preserves `BoolKnown`: each instruction that
  -- extends the set adds a wire whose `is-bit` is read off the head
  -- constraint from `satisfies` (`test-eq` / `less-than` / `not` /
  -- `cond-select` outputs are `from-bool` / mux; `constrain-to-boolean`
  -- pins the operand; `copy` inherits).  The boolean producers write to
  -- `n ≡ nr-wires st`; the membership-conditioned cases (`copy`,
  -- `cond-select`) branch on `_∈?_` exactly as `O2-record` does, so the
  -- two stay in lockstep definitionally.
  bk-step : ∀ {hc} (w : Witness) (i : Instruction) (bk : IndexSet)
      (is : List Instruction) (st : SynthState) (n : ℕ)
    → n ≡ SynthState.nr-wires st
    → satisfies-constraints
        (SynthState.constraints (circuit-instrs hc (i ∷ is) st)) w
    → BoolKnown w bk → BoolKnown w (O2-record i n bk)
  bk-step {hc} w (test-eq a b) bk is st n n≡ sat bkw =
    let (av , bv , ov , _ , _ , at-out , ov≡) =
          single-constraint-holds {hc} w (test-eq a b) is st
            (test-eq n a b)
            (cong (λ k → SynthState.constraints st ++ (test-eq k a b ∷ []))
                  (sym n≡))
            sat
    in bk-insert w at-out (from-bool-eq-is-bit (av ≡ᶠ? bv) ov≡) bkw
  bk-step {hc} w (less-than a b bits) bk is st n n≡ sat bkw =
    let (av , bv , ov , _ , _ , at-out , _ , _ , ov≡) =
          single-constraint-holds {hc} w (less-than a b bits) is st
            (less-than n a b bits)
            (cong (λ k → SynthState.constraints st
                          ++ (less-than k a b bits ∷ [])) (sym n≡))
            sat
    in bk-insert w at-out
         (from-bool-eq-is-bit
            (bits-lt (take (lt-bits bits) (to-le-bits av))
                     (take (lt-bits bits) (to-le-bits bv))) ov≡) bkw
  bk-step {hc} w (not a) bk is st n n≡ sat bkw =
    let (av , ov , _ , at-out , ov≡) =
          single-constraint-holds {hc} w (not a) is st
            (is-zero n a)
            (cong (λ k → SynthState.constraints st ++ (is-zero k a ∷ []))
                  (sym n≡))
            sat
    in bk-insert w at-out (from-bool-eq-is-bit (av ≡ᶠ? 0ᶠ) ov≡) bkw
  bk-step {hc} w (constrain-to-boolean v) bk is st n n≡ sat bkw =
    let (vv , at-v , vv-bit) =
          single-constraint-holds {hc} w (constrain-to-boolean v) is st
            (boolean v) refl sat
    in bk-insert w at-v vv-bit bkw
  bk-step {hc} w (copy v) bk is st n n≡ sat bkw with v ∈? bk
  ... | no  _ = bkw
  ... | yes v∈ =
    let gate-h =
          single-constraint-holds {hc} w (copy v) is st
            (wire n ≑ wire v)
            (cong (λ k → SynthState.constraints st
                          ++ ((wire k ≑ wire v) ∷ []))
                  (sym n≡))
            sat
        (vv , ov , at-v , at-out , ov≡) =
          ≑wire-inv (Witness.mem w) {n} {v} gate-h
        (_ , at-v′ , vv-bit) = bkw v∈
        vv≡ = just-injective (trans (sym at-v′) at-v)
    in bk-insert w at-out
         (subst is-bit (trans vv≡ (sym ov≡)) vv-bit) bkw
  bk-step {hc} w (cond-select b a c) bk is st n n≡ sat bkw
    with a ∈? bk | c ∈? bk
  ... | no  _ | _     = bkw
  ... | yes _ | no  _ = bkw
  ... | yes a∈ | yes c∈ =
    let (bv , av , cv , ov , _ , at-a , at-c , at-out , bv-bit , ov≡) =
          single-constraint-holds {hc} w (cond-select b a c) is st
            (select n b a c)
            (cong (λ k → SynthState.constraints st
                          ++ (select k b a c ∷ [])) (sym n≡))
            sat
        (_ , at-a′ , av-bit) = bkw a∈
        (_ , at-c′ , cv-bit) = bkw c∈
        av-bit′ = subst is-bit (just-injective (trans (sym at-a′) at-a)) av-bit
        cv-bit′ = subst is-bit (just-injective (trans (sym at-c′) at-c)) cv-bit
    in bk-insert w at-out (mux-is-bit bv-bit av-bit′ cv-bit′ ov≡) bkw
  -- Non-extending instructions: `O2-record _ _ bk = bk`.
  bk-step w (assert _)               bk is st n _ _ bkw = bkw
  bk-step w (constrain-bits _ _)     bk is st n _ _ bkw = bkw
  bk-step w (constrain-eq _ _)       bk is st n _ _ bkw = bkw
  bk-step w (declare-pub-input _)    bk is st n _ _ bkw = bkw
  bk-step w (pi-skip _ _)            bk is st n _ _ bkw = bkw
  bk-step w (ec-add _ _ _ _)         bk is st n _ _ bkw = bkw
  bk-step w (ec-mul _ _ _)           bk is st n _ _ bkw = bkw
  bk-step w (ec-mul-generator _)     bk is st n _ _ bkw = bkw
  bk-step w (hash-to-curve _)        bk is st n _ _ bkw = bkw
  bk-step w (load-imm _)             bk is st n _ _ bkw = bkw
  bk-step w (div-mod-power-of-two _ _) bk is st n _ _ bkw = bkw
  bk-step w (reconstitute-field _ _ _) bk is st n _ _ bkw = bkw
  bk-step w (output _)               bk is st n _ _ bkw = bkw
  bk-step w (transient-hash _)       bk is st n _ _ bkw = bkw
  bk-step w (persistent-hash _ _)    bk is st n _ _ bkw = bkw
  bk-step w (add _ _)                bk is st n _ _ bkw = bkw
  bk-step w (mul _ _)                bk is st n _ _ bkw = bkw
  bk-step w (neg _)                  bk is st n _ _ bkw = bkw
  bk-step w (public-input _)         bk is st n _ _ bkw = bkw
  bk-step w (private-input _)        bk is st n _ _ bkw = bkw

  -- O2's `require-∈` succeeding (read off the threaded `O2-Trace`) gives
  -- the guard's set membership, which `BoolKnown` turns into `is-bit`.
  require-∈-∈ : ∀ (g : Index) (bk bk′ : IndexSet)
    → require-∈ g bk ≡ just bk′ → g ∈ bk
  require-∈-∈ g bk bk′ eq with g ∈? bk
  ... | yes g∈ = g∈

  require-∈-stable : ∀ (g : Index) (bk bk′ : IndexSet)
    → require-∈ g bk ≡ just bk′ → bk′ ≡ bk
  require-∈-stable g bk bk′ eq with g ∈? bk
  ... | yes _ = sym (just-injective eq)

  -- `O2-check` never grows its set (every branch returns the input `bk`,
  -- via `require-∈`'s success or the trivial `just bk`).
  o2-check-stable : ∀ (i : Instruction) (bk bk′ : IndexSet)
    → O2-check i bk ≡ just bk′ → bk′ ≡ bk
  o2-check-stable (assert c)          bk bk′ eq = require-∈-stable c bk bk′ eq
  o2-check-stable (not a)             bk bk′ eq = require-∈-stable a bk bk′ eq
  o2-check-stable (cond-select b _ _) bk bk′ eq = require-∈-stable b bk bk′ eq
  o2-check-stable (pi-skip (just g) _) bk bk′ eq = require-∈-stable g bk bk′ eq
  o2-check-stable (pi-skip nothing _) bk bk′ refl = refl
  o2-check-stable (constrain-bits _ _) bk bk′ refl = refl
  o2-check-stable (constrain-eq _ _)   bk bk′ refl = refl
  o2-check-stable (constrain-to-boolean _) bk bk′ refl = refl
  o2-check-stable (copy _)             bk bk′ refl = refl
  o2-check-stable (declare-pub-input _) bk bk′ refl = refl
  o2-check-stable (ec-add _ _ _ _)     bk bk′ refl = refl
  o2-check-stable (ec-mul _ _ _)       bk bk′ refl = refl
  o2-check-stable (ec-mul-generator _) bk bk′ refl = refl
  o2-check-stable (hash-to-curve _)    bk bk′ refl = refl
  o2-check-stable (load-imm _)         bk bk′ refl = refl
  o2-check-stable (div-mod-power-of-two _ _) bk bk′ refl = refl
  o2-check-stable (reconstitute-field _ _ _) bk bk′ refl = refl
  o2-check-stable (output _)           bk bk′ refl = refl
  o2-check-stable (transient-hash _)   bk bk′ refl = refl
  o2-check-stable (persistent-hash _ _) bk bk′ refl = refl
  o2-check-stable (test-eq _ _)        bk bk′ refl = refl
  o2-check-stable (add _ _)            bk bk′ refl = refl
  o2-check-stable (mul _ _)            bk bk′ refl = refl
  o2-check-stable (neg _)              bk bk′ refl = refl
  o2-check-stable (less-than _ _ _)    bk bk′ refl = refl
  o2-check-stable (public-input _)     bk bk′ refl = refl
  o2-check-stable (private-input _)    bk bk′ refl = refl

  -- Peel one `O2-Trace` step: a successful `O2-step i (n, bk)` advances
  -- the counter to `n + Δmem i` and leaves `bk` to `O2-record i n bk`
  -- (the check never changes `bk`), so retag the tail.
  o2-trace-tail : ∀ {i is n bk final}
    → O2-Trace (i ∷ is) (n , bk) final
    → O2-Trace is (n + Δmem i , O2-record i n bk) final
  o2-trace-tail {i} {n = n} {bk} (o2-step {acc' = acc'} step-eq tail)
    with O2-check i bk in chk
  ... | just bk₁ =
    subst (λ b → O2-Trace _ (n + Δmem i , O2-record i n b) _)
          (o2-check-stable i bk bk₁ chk)
          (subst (λ a → O2-Trace _ a _) (just-injective (sym step-eq)) tail)

  -- A guarded `pi-skip` reaching this point has its guard in `bk`.
  o2-trace-skip-∈ : ∀ {g count is n bk final}
    → O2-Trace (pi-skip (just g) count ∷ is) (n , bk) final → g ∈ bk
  o2-trace-skip-∈ {g} {n = n} {bk} (o2-step step-eq _)
    with require-∈ g bk in chk
  ... | just _ = require-∈-∈ g bk _ chk

  -- A `pi-skip`'s guard wire is `is-bit` in `w`: O2 placed it in the
  -- bool-known set, and `BoolKnown` carries that set's bit facts.
  skip-guard-bit : ∀ {w bk n count is final} (g : Maybe Index)
    → BoolKnown w bk
    → O2-Trace (pi-skip g count ∷ is) (n , bk) final
    → IsBitGuard w g
  skip-guard-bit nothing   _   _   = tt
  skip-guard-bit (just gw) bkw o2t = bkw (o2-trace-skip-∈ o2t)

  ------------------------------------------------------------------------
  -- The PI-transcript pre-pass.  `pi-skip` is non-monotone on
  -- `pub-in-idx`: an *active* skip leaves it; an *inactive* one subtracts
  -- the group size.  So the `pub-transcript-inputs` (`pti`) `pre` must
  -- carry is the in-order concatenation of the *active* declare-groups,
  -- and `pre`'s `pti` must be fixed *before* the walk (the active-skip
  -- prefix-match reads it).  `make-pti` computes it from `w` alone: it
  -- walks the instructions with the declared-value stream `decls` (the
  -- pis tail of `w`) and the current group buffer `grp`, deciding each
  -- skip's activity by `to-bool (mem w at guard)`.  An active skip
  -- commits its group; an inactive one and the trailing (uncommitted)
  -- group are dropped.
  --
  -- O1's full-bracketing (each skip's `count` equals the declares since
  -- the previous skip, threaded as the `O1-Trace` in `PiInv`) is what
  -- makes this transcript canonical: the active windows are exactly the
  -- groups, in order.  Under the weaker pool-based O1, two active skips
  -- can pin the same transcript position to different declared values,
  -- and statement soundness FAILS for `pi-skip` (see the O1 header in
  -- Obligations.agda).
  ------------------------------------------------------------------------

  -- Take / drop at a list's own length: the structural prefix lemmas the
  -- active-skip window-match reduces to.
  take-len-++ : ∀ (xs ys : List Fr) → take (length xs) (xs ++ ys) ≡ xs
  take-len-++ []       ys = refl
  take-len-++ (x ∷ xs) ys = cong (x ∷_) (take-len-++ xs ys)

  drop-len-++ : ∀ (xs ys : List Fr) → drop (length xs) (xs ++ ys) ≡ ys
  drop-len-++ []       ys = refl
  drop-len-++ (x ∷ xs) ys = drop-len-++ xs ys

  -- `≡ᶠ-list?` is reflexive (so an active skip whose `recent ≡ expected`
  -- propositionally passes the boolean prefix-match).
  ≡ᶠ-list?-refl : ∀ (xs : List Fr) → T (xs ≡ᶠ-list? xs)
  ≡ᶠ-list?-refl []       = tt
  ≡ᶠ-list?-refl (x ∷ xs) =
    T-∧ .Equivalence.from
      (subst T (sym (≡ᶠ?-refl {x})) tt , ≡ᶠ-list?-refl xs)

  -- The guard's truth value, as a boolean (`true` iff active): the guard
  -- evaluates to `just true`, everything else (`just false` / `nothing`)
  -- to `false`.
  pti-active : List Fr → Maybe Index → Bool
  pti-active mem g = fromMaybe false (eval-guard mem g)

  -- The active-group transcript.  `decls` is the declared-PI value stream
  -- (the pis tail of `w`); `grp` the current uncommitted group.
  make-pti : List Fr → List Instruction → (grp decls : List Fr) → List Fr
  make-pti mem [] grp decls = []
  make-pti mem (declare-pub-input _ ∷ is) grp (d ∷ ds) =
    make-pti mem is (grp ++ (d ∷ [])) ds
  make-pti mem (declare-pub-input _ ∷ is) grp [] = []
  make-pti mem (pi-skip g _ ∷ is) grp decls =
    if pti-active mem g
      then grp ++ make-pti mem is [] decls
      else make-pti mem is [] decls
  make-pti mem (assert _ ∷ is)               grp decls = make-pti mem is grp decls
  make-pti mem (cond-select _ _ _ ∷ is)      grp decls = make-pti mem is grp decls
  make-pti mem (constrain-bits _ _ ∷ is)     grp decls = make-pti mem is grp decls
  make-pti mem (constrain-eq _ _ ∷ is)       grp decls = make-pti mem is grp decls
  make-pti mem (constrain-to-boolean _ ∷ is) grp decls = make-pti mem is grp decls
  make-pti mem (copy _ ∷ is)                 grp decls = make-pti mem is grp decls
  make-pti mem (ec-add _ _ _ _ ∷ is)         grp decls = make-pti mem is grp decls
  make-pti mem (ec-mul _ _ _ ∷ is)           grp decls = make-pti mem is grp decls
  make-pti mem (ec-mul-generator _ ∷ is)     grp decls = make-pti mem is grp decls
  make-pti mem (hash-to-curve _ ∷ is)        grp decls = make-pti mem is grp decls
  make-pti mem (load-imm _ ∷ is)             grp decls = make-pti mem is grp decls
  make-pti mem (div-mod-power-of-two _ _ ∷ is) grp decls = make-pti mem is grp decls
  make-pti mem (reconstitute-field _ _ _ ∷ is) grp decls = make-pti mem is grp decls
  make-pti mem (output _ ∷ is)               grp decls = make-pti mem is grp decls
  make-pti mem (transient-hash _ ∷ is)       grp decls = make-pti mem is grp decls
  make-pti mem (persistent-hash _ _ ∷ is)    grp decls = make-pti mem is grp decls
  make-pti mem (test-eq _ _ ∷ is)            grp decls = make-pti mem is grp decls
  make-pti mem (add _ _ ∷ is)                grp decls = make-pti mem is grp decls
  make-pti mem (mul _ _ ∷ is)                grp decls = make-pti mem is grp decls
  make-pti mem (neg _ ∷ is)                  grp decls = make-pti mem is grp decls
  make-pti mem (not _ ∷ is)                  grp decls = make-pti mem is grp decls
  make-pti mem (less-than _ _ _ ∷ is)        grp decls = make-pti mem is grp decls
  make-pti mem (public-input _ ∷ is)         grp decls = make-pti mem is grp decls
  make-pti mem (private-input _ ∷ is)        grp decls = make-pti mem is grp decls

  -- The pre-pass: from `satisfies` (the guard-disj constraints) read off, for
  -- every transcript instruction, its guard decision and consumed value,
  -- and hence the public-output / private supplies `pre` must carry.
  -- `n` is the running wire count (= `nr-wires st`); the length premise
  -- `length (mem w) ≡ n + Δmem-sum is` bounds nothing-guard lookups.
  -- Alongside, it threads the witness-level O2 state: `bk` (with its
  -- `BoolKnown` facts) and the producer's `O2-Trace`, which together
  -- supply the `IsBitGuard` a `pi-skip` head's `tf-skip` needs.
  mutual
    make-trfacts : ∀ {hc} (w : Witness) (is : List Instruction)
        (st : SynthState) (n : ℕ) (bk : IndexSet) {o2f : ℕ × IndexSet}
      → n ≡ SynthState.nr-wires st
      → length (Witness.mem w) ≡ n + Δmem-sum is
      → BoolKnown w bk
      → O2-Trace is (n , bk) o2f
      → satisfies-constraints (SynthState.constraints (circuit-instrs hc is st)) w
      → TrSupply w is n
    make-trfacts w [] st n _ _ _ _ _ _ = mk-tr-supply [] [] tf-nil
    make-trfacts w (assert c ∷ is) st n bk e l bkw o2t sat =
      step-other w (assert c) is st n bk e l bkw o2t sat tt
    make-trfacts w (cond-select b a c ∷ is) st n bk e l bkw o2t sat =
      step-other w (cond-select b a c) is st n bk e l bkw o2t sat tt
    make-trfacts w (constrain-bits v bits ∷ is) st n bk e l bkw o2t sat =
      step-other w (constrain-bits v bits) is st n bk e l bkw o2t sat tt
    make-trfacts w (constrain-eq a b ∷ is) st n bk e l bkw o2t sat =
      step-other w (constrain-eq a b) is st n bk e l bkw o2t sat tt
    make-trfacts w (constrain-to-boolean v ∷ is) st n bk e l bkw o2t sat =
      step-other w (constrain-to-boolean v) is st n bk e l bkw o2t sat tt
    make-trfacts w (copy v ∷ is) st n bk e l bkw o2t sat =
      step-other w (copy v) is st n bk e l bkw o2t sat tt
    make-trfacts w (declare-pub-input v ∷ is) st n bk e l bkw o2t sat =
      step-other w (declare-pub-input v) is st n bk e l bkw o2t sat tt
    make-trfacts w (ec-add ax ay bx by ∷ is) st n bk e l bkw o2t sat =
      step-other w (ec-add ax ay bx by) is st n bk e l bkw o2t sat tt
    make-trfacts w (ec-mul ax ay sc ∷ is) st n bk e l bkw o2t sat =
      step-other w (ec-mul ax ay sc) is st n bk e l bkw o2t sat tt
    make-trfacts w (ec-mul-generator sc ∷ is) st n bk e l bkw o2t sat =
      step-other w (ec-mul-generator sc) is st n bk e l bkw o2t sat tt
    make-trfacts w (hash-to-curve ins ∷ is) st n bk e l bkw o2t sat =
      step-other w (hash-to-curve ins) is st n bk e l bkw o2t sat tt
    make-trfacts w (load-imm imm ∷ is) st n bk e l bkw o2t sat =
      step-other w (load-imm imm) is st n bk e l bkw o2t sat tt
    make-trfacts w (div-mod-power-of-two v bits ∷ is) st n bk e l bkw o2t sat =
      step-other w (div-mod-power-of-two v bits) is st n bk e l bkw o2t sat tt
    make-trfacts w (reconstitute-field d m bits ∷ is) st n bk e l bkw o2t sat =
      step-other w (reconstitute-field d m bits) is st n bk e l bkw o2t sat tt
    make-trfacts w (output v ∷ is) st n bk e l bkw o2t sat =
      step-other w (output v) is st n bk e l bkw o2t sat tt
    make-trfacts w (transient-hash ins ∷ is) st n bk e l bkw o2t sat =
      step-other w (transient-hash ins) is st n bk e l bkw o2t sat tt
    make-trfacts w (persistent-hash α ins ∷ is) st n bk e l bkw o2t sat =
      step-other w (persistent-hash α ins) is st n bk e l bkw o2t sat tt
    make-trfacts w (test-eq a b ∷ is) st n bk e l bkw o2t sat =
      step-other w (test-eq a b) is st n bk e l bkw o2t sat tt
    make-trfacts w (add a b ∷ is) st n bk e l bkw o2t sat =
      step-other w (add a b) is st n bk e l bkw o2t sat tt
    make-trfacts w (mul a b ∷ is) st n bk e l bkw o2t sat =
      step-other w (mul a b) is st n bk e l bkw o2t sat tt
    make-trfacts w (neg a ∷ is) st n bk e l bkw o2t sat =
      step-other w (neg a) is st n bk e l bkw o2t sat tt
    make-trfacts w (not a ∷ is) st n bk e l bkw o2t sat =
      step-other w (not a) is st n bk e l bkw o2t sat tt
    make-trfacts w (less-than a b bits ∷ is) st n bk e l bkw o2t sat =
      step-other w (less-than a b bits) is st n bk e l bkw o2t sat tt
    -- `pi-skip` synthesises nothing (`circuit-instr` is the identity and
    -- `Δmem` is 0), so everything passes through; the head's `tf-skip`
    -- carries the guard booleanity read off `bk` / the `O2-Trace`.
    make-trfacts {hc} w (pi-skip g count ∷ is) st n bk {o2f} n≡ l bkw o2t sat =
      let o2t₁ : O2-Trace is (n , bk) o2f
          o2t₁ = subst (λ k → O2-Trace is (k , bk) o2f) (+-identityʳ n)
                       (o2-trace-tail o2t)
          (mk-tr-supply po pv tf) =
            make-trfacts {hc} w is st n bk n≡ l bkw o2t₁ sat
      in mk-tr-supply po pv (tf-skip (skip-guard-bit g bkw o2t) tf)
    make-trfacts {hc} w (public-input g ∷ is) st n bk n≡ l bkw o2t sat =
      pub-decision {hc} w g is st n bk n≡ l bkw o2t sat
    make-trfacts {hc} w (private-input g ∷ is) st n bk n≡ l bkw o2t sat =
      priv-decision {hc} w g is st n bk n≡ l bkw o2t sat

    -- Non-transcript head: tile via `tf-other`, recurse on the tail,
    -- advancing the boolean-known set by `bk-step` and peeling the
    -- `O2-Trace`.
    step-other : ∀ {hc} (w : Witness) (i : Instruction)
        (is : List Instruction) (st : SynthState) (n : ℕ) (bk : IndexSet)
        {o2f : ℕ × IndexSet}
      → n ≡ SynthState.nr-wires st
      → length (Witness.mem w) ≡ n + Δmem-sum (i ∷ is)
      → BoolKnown w bk
      → O2-Trace (i ∷ is) (n , bk) o2f
      → satisfies-constraints (SynthState.constraints (circuit-instrs hc (i ∷ is) st)) w
      → NotTranscript i
      → TrSupply w (i ∷ is) n
    step-other {hc} w i is st n bk n≡ l bkw o2t sat nt =
      let n≡′ : n + Δmem i ≡ SynthState.nr-wires (circuit-instr hc i st)
          n≡′ = trans (cong (_+ Δmem i) n≡) (sym (nr-wires-step hc i st))
          l′ : length (Witness.mem w) ≡ (n + Δmem i) + Δmem-sum is
          l′ = trans l (sym (+-assoc n (Δmem i) (Δmem-sum is)))
          (mk-tr-supply po pv tf) =
            make-trfacts {hc} w is (circuit-instr hc i st) (n + Δmem i)
              (O2-record i n bk) n≡′ l′
              (bk-step {hc} w i bk is st n n≡ sat bkw)
              (o2-trace-tail o2t) sat
      in mk-tr-supply po pv (tf-other nt tf)

    -- `public-input g`: decode the guard-disj constraint (if guarded), decide
    -- active/inactive, recurse on the tail at cursor `suc n`.
    pub-decision : ∀ {hc} (w : Witness) (g : Maybe Index)
        (is : List Instruction) (st : SynthState) (n : ℕ) (bk : IndexSet)
        {o2f : ℕ × IndexSet}
      → n ≡ SynthState.nr-wires st
      → length (Witness.mem w) ≡ n + Δmem-sum (public-input g ∷ is)
      → BoolKnown w bk
      → O2-Trace (public-input g ∷ is) (n , bk) o2f
      → satisfies-constraints
          (SynthState.constraints (circuit-instrs hc (public-input g ∷ is) st)) w
      → TrSupply w (public-input g ∷ is) n
    pub-decision {hc} w g is st n bk {o2f} n≡ l bkw o2t sat =
      let st₁  = circuit-instr hc (public-input g) st
          n≡₁ : suc n ≡ SynthState.nr-wires st₁
          n≡₁ = trans (cong suc n≡)
                 (trans (sym (+-comm (SynthState.nr-wires st) 1))
                        (sym (nr-wires-step hc (public-input g) st)))
          l₁ : length (Witness.mem w) ≡ suc n + Δmem-sum is
          l₁ = trans l (+-suc n (Δmem-sum is))
          o2t₁ : O2-Trace is (suc n , bk) o2f
          o2t₁ = subst (λ k → O2-Trace is (k , bk) o2f) (+-comm n 1)
                       (o2-trace-tail o2t)
      in pub-decide {hc} w g is st n n≡ l sat
           (make-trfacts {hc} w is st₁ (suc n) bk n≡₁ l₁ bkw o2t₁ sat)

    priv-decision : ∀ {hc} (w : Witness) (g : Maybe Index)
        (is : List Instruction) (st : SynthState) (n : ℕ) (bk : IndexSet)
        {o2f : ℕ × IndexSet}
      → n ≡ SynthState.nr-wires st
      → length (Witness.mem w) ≡ n + Δmem-sum (private-input g ∷ is)
      → BoolKnown w bk
      → O2-Trace (private-input g ∷ is) (n , bk) o2f
      → satisfies-constraints
          (SynthState.constraints (circuit-instrs hc (private-input g ∷ is) st)) w
      → TrSupply w (private-input g ∷ is) n
    priv-decision {hc} w g is st n bk {o2f} n≡ l bkw o2t sat =
      let st₁  = circuit-instr hc (private-input g) st
          n≡₁ : suc n ≡ SynthState.nr-wires st₁
          n≡₁ = trans (cong suc n≡)
                 (trans (sym (+-comm (SynthState.nr-wires st) 1))
                        (sym (nr-wires-step hc (private-input g) st)))
          l₁ : length (Witness.mem w) ≡ suc n + Δmem-sum is
          l₁ = trans l (+-suc n (Δmem-sum is))
          o2t₁ : O2-Trace is (suc n , bk) o2f
          o2t₁ = subst (λ k → O2-Trace is (k , bk) o2f) (+-comm n 1)
                       (o2-trace-tail o2t)
      in priv-decide {hc} w g is st n n≡ l sat
           (make-trfacts {hc} w is st₁ (suc n) bk n≡₁ l₁ bkw o2t₁ sat)

  -- Lookup at the boundary of a tiling: the cell just past `xs`.
  lookup-bound : ∀ (xs : List Fr) (y : Fr) (ys : List Fr)
    → mem-lookup (xs ++ (y ∷ ys)) (length xs) ≡ just y
  lookup-bound []       y ys = refl
  lookup-bound (x ∷ xs) y ys = lookup-bound xs y ys

  -- Lookup below the prefix length is unaffected by an appended suffix.
  lookup-prefix : ∀ (xs ys : List Fr) (i : ℕ) → i < length xs
    → mem-lookup (xs ++ ys) i ≡ mem-lookup xs i
  lookup-prefix (x ∷ xs) ys zero    _         = refl
  lookup-prefix (x ∷ xs) ys (suc i) (s≤s lt) = lookup-prefix xs ys i lt

  -- A guard's evaluation, given the guard wire's value.
  eval-guard-eq : ∀ (mem : List Fr) (gw : Index) {v : Fr}
    → mem-lookup mem gw ≡ just v → eval-guard mem (just gw) ≡ to-bool v
  eval-guard-eq mem gw eq rewrite eq = refl

  -- Retag a `TrFacts` tail (peeled from `tf-other`) to the next state's
  -- memory length.  The tail is indexed at `length (memory s) + Δmem i`,
  -- which for a concrete `i` reduces to `+ 0` / `+ 1` / `+ 2`; the three
  -- helpers handle those, avoiding an uninferable `Δmem i`.
  tf-stay : ∀ {w is po pv} (s : Preprocessed)
    → TrFacts w is (length (Preprocessed.memory s) + 0) po pv
    → TrFacts w is (length (Preprocessed.memory s)) po pv
  tf-stay s tl = subst (λ k → TrFacts _ _ k _ _) (+-identityʳ _) tl

  tf-grow1 : ∀ {w is po pv} (s : Preprocessed) (c : Fr)
    → TrFacts w is (length (Preprocessed.memory s) + 1) po pv
    → TrFacts w is (length (Preprocessed.memory s ++ (c ∷ []))) po pv
  tf-grow1 s c tl =
    subst (λ k → TrFacts _ _ k _ _)
      (sym (length-++ (Preprocessed.memory s) {c ∷ []})) tl

  tf-grow2 : ∀ {w is po pv} (s : Preprocessed) (a b : Fr)
    → TrFacts w is (length (Preprocessed.memory s) + 2) po pv
    → TrFacts w is (length (Preprocessed.memory s ++ (a ∷ b ∷ []))) po pv
  tf-grow2 s a b tl =
    subst (λ k → TrFacts _ _ k _ _)
      (sym (length-++ (Preprocessed.memory s) {a ∷ b ∷ []})) tl

  -- A transcript step appends one cell; its `TrFacts` tail is indexed at
  -- `suc n` (not `n + Δmem`), so retag it to the next memory length.
  tf-suc : ∀ {w is po pv} (s : Preprocessed) (cell : Fr)
    → TrFacts w is (suc (length (Preprocessed.memory s))) po pv
    → TrFacts w is (length (Preprocessed.memory s ++ (cell ∷ []))) po pv
  tf-suc s cell tl =
    subst (λ k → TrFacts _ _ k _ _)
      (trans (sym (+-comm (length (Preprocessed.memory s)) 1))
             (sym (length-++ (Preprocessed.memory s) {cell ∷ []}))) tl

  -- Invert the `O1-Trace` constructors the walk's pi-skip / declare /
  -- nil cases consume.
  o1-nil-zero : ∀ {d} → O1-Trace d [] → d ≡ 0
  o1-nil-zero o1-nil = refl

  o1-decl-inv : ∀ {d v is} → O1-Trace d (declare-pub-input v ∷ is)
    → O1-Trace (suc d) is
  o1-decl-inv (o1-decl t) = t

  o1-skip-inv : ∀ {d g n is} → O1-Trace d (pi-skip g n ∷ is)
    → (n ≡ d) × O1-Trace 0 is
  o1-skip-inv (o1-skip n≡d t) = n≡d , t

  -- The pi-skip / public-input transcript invariants threaded by `build`:
  -- the bracketing trace; that `pub-in-idx` is the committed-plus-current
  -- group size; that the current group `grp` is a suffix of `pis`; and
  -- that the verifier transcript `pre` carries is the committed groups
  -- followed by the active groups still to come (`make-pti`).
  record PiInv (pre : ProofPreimage) (w : Witness) (is : List Instruction)
               (s : Preprocessed) (committed grp pis-rest : List Fr) : Set where
    constructor mk-piinv
    field
      o1pii  : O1-Trace (length grp) is
      pidx   : Preprocessed.pub-in-idx s ≡ length committed + length grp
      pis-sf : Σ (List Fr) (λ ph → Preprocessed.pis s ≡ ph ++ grp)
      pti-eq : ProofPreimage.pub-transcript-inputs pre
             ≡ committed ++ make-pti (Witness.mem w) is grp pis-rest

  -- Thread `PiInv` past a non-declare/skip head `i` to the next state
  -- `s'` (which preserves `pub-in-idx`/`pis`); `make-pti` ignores such an
  -- `i`, so the transcript invariant is unchanged.
  pi-pass : ∀ {pre w i is committed grp pis-rest} (s s′ : Preprocessed)
    → NotDeclSkip i
    → Preprocessed.pub-in-idx s′ ≡ Preprocessed.pub-in-idx s
    → Preprocessed.pis        s′ ≡ Preprocessed.pis        s
    → make-pti (Witness.mem w) (i ∷ is) grp pis-rest
        ≡ make-pti (Witness.mem w) is grp pis-rest
    → PiInv pre w (i ∷ is) s  committed grp pis-rest
    → PiInv pre w is        s′ committed grp pis-rest
  pi-pass s s′ nt pidx≡ pis≡ mk≡ pii =
    let (ph , pe) = PiInv.pis-sf pii in
    mk-piinv (o1-tail nt (PiInv.o1pii pii))
             (trans pidx≡ (PiInv.pidx pii))
             (ph , trans pis≡ pe)
             (trans (PiInv.pti-eq pii) (cong (_ ++_) mk≡))

  -- The prefix-match an ACTIVE `pi-skip` must exhibit.  The last `count =
  -- length grp` pis cells are the current group `grp` (the pis suffix
  -- `pe`), and the verifier transcript reads `committed ++ (grp ++ mkrest)`
  -- with the matching window landing on `grp` (from `pidx` and `pti≡`), so
  -- both windows are `grp` and the check is reflexivity.
  skip-match : ∀ (pre : ProofPreimage) (s : Preprocessed)
      (committed grp mkrest ph : List Fr) {count : ℕ}
    → count ≡ length grp
    → Preprocessed.pis s ≡ ph ++ grp
    → Preprocessed.pub-in-idx s ≡ length committed + length grp
    → ProofPreimage.pub-transcript-inputs pre ≡ committed ++ (grp ++ mkrest)
    → T (drop (length (Preprocessed.pis s) ∸ count) (Preprocessed.pis s)
          ≡ᶠ-list?
         take count (drop (Preprocessed.pub-in-idx s ∸ count)
                          (ProofPreimage.pub-transcript-inputs pre)))
  skip-match pre s committed grp mkrest ph {count} count≡ pe pidx pti≡ =
    let recent≡ : drop (length (Preprocessed.pis s) ∸ count)
                       (Preprocessed.pis s) ≡ grp
        recent≡ =
          trans (cong₂ drop
                  (trans (cong₂ _∸_ (trans (cong length pe) (length-++ ph))
                                    count≡)
                         (m+n∸n≡m (length ph) (length grp)))
                  pe)
                (drop-len-++ ph grp)
        expected≡ : take count
                      (drop (Preprocessed.pub-in-idx s ∸ count)
                            (ProofPreimage.pub-transcript-inputs pre)) ≡ grp
        expected≡ =
          trans (cong₂ (λ a xs → take count (drop a xs))
                  (trans (cong₂ _∸_ pidx count≡)
                         (m+n∸n≡m (length committed) (length grp)))
                  pti≡)
            (trans (cong (take count) (drop-len-++ committed (grp ++ mkrest)))
              (trans (cong (λ k → take k (grp ++ mkrest)) count≡)
                     (take-len-++ grp mkrest)))
    in subst₂ (λ a b → T (a ≡ᶠ-list? b))
              (sym recent≡) (sym expected≡) (≡ᶠ-list?-refl grp)

  -- The end state `wstate` of the walk over `is`, packaged with proofs
  -- that it tiles the memory / pis suffixes of `w`, has consumed all its
  -- public-output / private transcripts, and validated exactly
  -- `length committed + length (make-pti …)` public inputs.  A record (not
  -- a nested Σ/×) so the fields are named at every use site.
  record WalkResult (pre : ProofPreimage) (w : Witness)
                    (is : List Instruction) (s : Preprocessed)
                    (committed grp mem-rest pis-rest : List Fr) : Set where
    constructor mk-walk
    field
      wstate : Preprocessed
      wtr    : Tr-shaped pre s is wstate mem-rest pis-rest
      wmem   : Preprocessed.memory     wstate ≡ Witness.mem w
      wpis   : Preprocessed.pis        wstate ≡ Witness.pis w
      widx   : Preprocessed.pub-in-idx wstate
                 ≡ length committed
                 + length (make-pti (Witness.mem w) is grp pis-rest)
      wpout  : Preprocessed.pub-out-rem wstate ≡ []
      wpriv  : Preprocessed.priv-rem    wstate ≡ []

  build : ∀ (pre : ProofPreimage) (w : Witness) (is : List Instruction)
            (s : Preprocessed) (mem-rest pis-rest : List Fr)
            {final : ℕ} {po pv : List Fr}
        → TrFacts w is (length (Preprocessed.memory s)) po pv
        → Wire-Trace is (length (Preprocessed.memory s)) final
        → Preprocessed.pub-out-rem s ≡ po
        → Preprocessed.priv-rem    s ≡ pv
        → Preprocessed.memory s ++ mem-rest ≡ Witness.mem w
        → Preprocessed.pis    s ++ pis-rest ≡ Witness.pis w
        → length mem-rest ≡ Δmem-sum is
        → length pis-rest ≡ Δpis-sum is
        → (committed grp : List Fr)
        → PiInv pre w is s committed grp pis-rest
        → WalkResult pre w is s committed grp mem-rest pis-rest

  -- The three per-shape walk steps, one per non-transcript instruction
  -- Δmem class (0 / 1 / 2 cells appended).  Each peels its head's
  -- `Tr-shaped` `tr-step` payload `sd` (built at the call site, where the
  -- instruction is concrete so `op-side-data` / `next-state-from-osd`
  -- compute), recurses over the tail, and prepends `tr-cons sd`.  Being
  -- generic in the head `i` — with `tr-next … sd ≡ <next state>`,
  -- `Δmem i ≡ k`, `NotDeclSkip i`, and the `make-pti` identity passed in —
  -- collapses the otherwise-identical per-instruction clauses to a
  -- one-line dispatch.
  step0 : ∀ (pre : ProofPreimage) (w : Witness) {i : Instruction}
            (is : List Instruction) (s : Preprocessed)
            (mem-rest pis-rest committed grp : List Fr)
            {final : ℕ} {po pv : List Fr}
        → (sd : tr-step i pre s [] [])
        → tr-next i pre s [] [] sd ≡ s
        → Δmem i ≡ 0
        → NotDeclSkip i
        → make-pti (Witness.mem w) (i ∷ is) grp pis-rest
            ≡ make-pti (Witness.mem w) is grp pis-rest
        → TrFacts w is (length (Preprocessed.memory s) + 0) po pv
        → Wire-Trace (i ∷ is) (length (Preprocessed.memory s)) final
        → Preprocessed.pub-out-rem s ≡ po
        → Preprocessed.priv-rem    s ≡ pv
        → Preprocessed.memory s ++ mem-rest ≡ Witness.mem w
        → Preprocessed.pis    s ++ pis-rest ≡ Witness.pis w
        → length mem-rest ≡ Δmem-sum is
        → length pis-rest ≡ Δpis-sum is
        → PiInv pre w (i ∷ is) s committed grp pis-rest
        → WalkResult pre w (i ∷ is) s committed grp mem-rest pis-rest

  step1 : ∀ (pre : ProofPreimage) (w : Witness) {i : Instruction}
            (is : List Instruction) (s : Preprocessed) (c : Fr)
            (rest′ pis-rest committed grp : List Fr)
            {final : ℕ} {po pv : List Fr}
        → (sd : tr-step i pre s (c ∷ []) [])
        → tr-next i pre s (c ∷ []) [] sd ≡ push-mem s c
        → Δmem i ≡ 1
        → NotDeclSkip i
        → make-pti (Witness.mem w) (i ∷ is) grp pis-rest
            ≡ make-pti (Witness.mem w) is grp pis-rest
        → TrFacts w is (length (Preprocessed.memory s) + 1) po pv
        → Wire-Trace (i ∷ is) (length (Preprocessed.memory s)) final
        → Preprocessed.pub-out-rem s ≡ po
        → Preprocessed.priv-rem    s ≡ pv
        → Preprocessed.memory s ++ (c ∷ rest′) ≡ Witness.mem w
        → Preprocessed.pis    s ++ pis-rest ≡ Witness.pis w
        → length rest′ ≡ Δmem-sum is
        → length pis-rest ≡ Δpis-sum is
        → PiInv pre w (i ∷ is) s committed grp pis-rest
        → WalkResult pre w (i ∷ is) s committed grp (c ∷ rest′) pis-rest

  step2 : ∀ (pre : ProofPreimage) (w : Witness) {i : Instruction}
            (is : List Instruction) (s : Preprocessed) (a b : Fr)
            (rest′ pis-rest committed grp : List Fr)
            {final : ℕ} {po pv : List Fr}
        → (sd : tr-step i pre s (a ∷ b ∷ []) [])
        → tr-next i pre s (a ∷ b ∷ []) [] sd ≡ push-mem2 s a b
        → Δmem i ≡ 2
        → NotDeclSkip i
        → make-pti (Witness.mem w) (i ∷ is) grp pis-rest
            ≡ make-pti (Witness.mem w) is grp pis-rest
        → TrFacts w is (length (Preprocessed.memory s) + 2) po pv
        → Wire-Trace (i ∷ is) (length (Preprocessed.memory s)) final
        → Preprocessed.pub-out-rem s ≡ po
        → Preprocessed.priv-rem    s ≡ pv
        → Preprocessed.memory s ++ (a ∷ b ∷ rest′) ≡ Witness.mem w
        → Preprocessed.pis    s ++ pis-rest ≡ Witness.pis w
        → length rest′ ≡ Δmem-sum is
        → length pis-rest ≡ Δpis-sum is
        → PiInv pre w (i ∷ is) s committed grp pis-rest
        → WalkResult pre w (i ∷ is) s committed grp
            (a ∷ b ∷ rest′) pis-rest

  build pre w [] s []       []      tf-nil _ poeq pveq memeq piseq lmem lpis
        committed grp pii =
    mk-walk s tr-nil
      (trans (sym (++-identityʳ _)) memeq)
      (trans (sym (++-identityʳ _)) piseq)
      (trans (PiInv.pidx pii)
             (cong (length committed +_) (o1-nil-zero (PiInv.o1pii pii))))
      poeq pveq
  build pre w [] s (_ ∷ _) _        tf-nil _ _ _ memeq piseq () lpis _ _ _
  build pre w [] s []      (_ ∷ _)  tf-nil _ _ _ memeq piseq lmem () _ _ _
  build pre w (assert _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii =
    step0 pre w is s mem-rest pis-rest committed grp
          (refl , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lenp lpis pii
  build pre w (constrain-bits _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii =
    step0 pre w is s mem-rest pis-rest committed grp
          (refl , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lenp lpis pii
  build pre w (constrain-eq _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii =
    step0 pre w is s mem-rest pis-rest committed grp
          (refl , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lenp lpis pii
  build pre w (constrain-to-boolean _ ∷ is)
        s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii =
    step0 pre w is s mem-rest pis-rest committed grp
          (refl , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lenp lpis pii
  build pre w (copy _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (load-imm _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (transient-hash _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (cond-select _ _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (not _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (less-than _ _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (neg _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (reconstitute-field _ _ _ ∷ is)
        s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (test-eq _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (add _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (mul _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lenp lpis committed grp pii
    with split1 mem-rest lenp
  ... | c , rest′ , refl , lreq =
    step1 pre w is s c rest′ pis-rest committed grp
          ((c , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lreq lpis pii
  build pre w (ec-add _ _ _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split2 mem-rest lmem
  ... | a , b , rest′ , refl , lmem′ =
    step2 pre w is s a b rest′ pis-rest committed grp
          ((a , b , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lmem′ lpis pii
  build pre w (ec-mul _ _ _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split2 mem-rest lmem
  ... | a , b , rest′ , refl , lmem′ =
    step2 pre w is s a b rest′ pis-rest committed grp
          ((a , b , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lmem′ lpis pii
  build pre w (ec-mul-generator _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split2 mem-rest lmem
  ... | a , b , rest′ , refl , lmem′ =
    step2 pre w is s a b rest′ pis-rest committed grp
          ((a , b , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lmem′ lpis pii
  build pre w (hash-to-curve _ ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split2 mem-rest lmem
  ... | a , b , rest′ , refl , lmem′ =
    step2 pre w is s a b rest′ pis-rest committed grp
          ((a , b , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lmem′ lpis pii
  build pre w (persistent-hash _ _ ∷ is)
        s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split2 mem-rest lmem
  ... | a , b , rest′ , refl , lmem′ =
    step2 pre w is s a b rest′ pis-rest committed grp
          ((a , b , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lmem′ lpis pii
  build pre w (div-mod-power-of-two _ _ ∷ is)
        s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split2 mem-rest lmem
  ... | a , b , rest′ , refl , lmem′ =
    step2 pre w is s a b rest′ pis-rest committed grp
          ((a , b , refl) , refl) refl refl tt refl
          tf wt poeq pveq memeq piseq lmem′ lpis pii
  build pre w (output v ∷ is) s mem-rest pis-rest (tf-other _ tf) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with mem-lookup-exists (Preprocessed.memory s) v (wire-trace-head-ok wt)
  ... | val , lk =
    let (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { outputs = Preprocessed.outputs s ++ (val ∷ []) })
            mem-rest pis-rest (tf-stay s tf)
            (wt-stay s refl wt) poeq pveq memeq piseq lmem lpis
            committed grp (pi-pass s _ tt refl refl refl pii)
    in mk-walk s′ (tr-cons ((val , lk) , refl , refl) tr) m≡ p≡ i≡ o≡ v≡
  build pre w (declare-pub-input v ∷ is)
        s mem-rest pis-rest (tf-other _ tf) wt poeq pveq
        memeq piseq lmem lpis committed grp pii
    with split1 pis-rest lpis
  ... | wv , prest′ , refl , lpis′ =
    let (ph , pe) = PiInv.pis-sf pii
        -- The declared cell joins the current group: `grp ++ (wv ∷ [])`,
        -- whose length is `suc (length grp)`.
        lg≡ : length (grp ++ (wv ∷ [])) ≡ suc (length grp)
        lg≡ = trans (length-++ grp) (+-comm (length grp) 1)
        pii′ : PiInv pre w is
                 (record s { pis = Preprocessed.pis s ++ (wv ∷ [])
                           ; pub-in-idx = suc (Preprocessed.pub-in-idx s) })
                 committed (grp ++ (wv ∷ [])) prest′
        pii′ = mk-piinv
                 (subst (λ k → O1-Trace k is) (sym lg≡)
                        (o1-decl-inv (PiInv.o1pii pii)))
                 (trans (cong suc (PiInv.pidx pii))
                        (trans (sym (+-suc (length committed) (length grp)))
                               (cong (length committed +_) (sym lg≡))))
                 (ph , trans (cong (_++ (wv ∷ [])) pe)
                             (++-assoc ph grp (wv ∷ [])))
                 (PiInv.pti-eq pii)
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { pis = Preprocessed.pis s ++ (wv ∷ [])
                      ; pub-in-idx = suc (Preprocessed.pub-in-idx s) })
            mem-rest prest′ (tf-stay s tf)
            (wt-stay s refl wt) poeq pveq memeq
            (trans (++-assoc (Preprocessed.pis s) (wv ∷ []) prest′) piseq)
            lmem lpis′ committed (grp ++ (wv ∷ [])) pii′
    in mk-walk s′ (tr-cons (refl , wv , refl) tr) m≡ p≡ i≡ o≡ v≡
  -- ── pi-skip ───────────────────────────────────────────────────────
  -- An ACTIVE skip checks the last `count` pis cells against the
  -- transcript window at `pub-in-idx ∸ count` and commits the group:
  -- `PiInv` pins both sides to exactly the current group `grp` (`count ≡
  -- length grp` by O1's full-bracketing; `grp` is the pis suffix by
  -- `pis-sf`; the window is the next active group of `make-pti` by
  -- `pidx` + `pti-eq`), so the prefix-match is reflexivity.  An INACTIVE
  -- skip rolls `pub-in-idx` back by `count`, discarding the group.  The
  -- guard's truth value comes off the `tf-skip` bit fact: `is-bit`
  -- splits it into the 1ᶠ (active) and 0ᶠ (inactive) cases.
  build pre w (pi-skip nothing count ∷ is) s mem-rest pis-rest
        (tf-skip _ tf-tl) wt poeq pveq memeq piseq lmem lpis
        committed grp pii =
    let (count≡ , o1tl) = o1-skip-inv (PiInv.o1pii pii)
        (ph , pe)       = PiInv.pis-sf pii
        mkrest          = make-pti (Witness.mem w) is [] pis-rest
        match = skip-match pre s committed grp mkrest ph count≡ pe
                           (PiInv.pidx pii) (PiInv.pti-eq pii)
        s₁ = record s { pi-skips = Preprocessed.pi-skips s ++ (nothing ∷ []) }
        pii′ : PiInv pre w is s₁ (committed ++ grp) [] pis-rest
        pii′ = mk-piinv o1tl
                 (trans (PiInv.pidx pii)
                        (sym (trans (+-identityʳ _) (length-++ committed))))
                 (Preprocessed.pis s , sym (++-identityʳ _))
                 (trans (PiInv.pti-eq pii)
                        (sym (++-assoc committed grp mkrest)))
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is s₁ mem-rest pis-rest tf-tl (wt-stay s refl wt)
            poeq pveq memeq piseq lmem lpis (committed ++ grp) [] pii′
    in mk-walk s′ (tr-cons (refl , refl , true , refl , match) tr) m≡ p≡
      (trans i≡
          (trans (cong (_+ length mkrest) (length-++ committed))
            (trans (+-assoc (length committed) (length grp) (length mkrest))
                   (cong (length committed +_) (sym (length-++ grp))))))
      o≡ v≡
  build pre w (pi-skip (just gw) count ∷ is) s mem-rest pis-rest
        (tf-skip (gv , at-gw , inj₂ gv≡1) tf-tl) wt poeq pveq memeq piseq
        lmem lpis committed grp pii =
    let (count≡ , o1tl) = o1-skip-inv (PiInv.o1pii pii)
        (ph , pe)       = PiInv.pis-sf pii
        mkrest          = make-pti (Witness.mem w) is [] pis-rest
        gw<len = wire-trace-head-ok wt
        look-s-gw : mem-lookup (Preprocessed.memory s) gw ≡ just gv
        look-s-gw =
          trans (sym (lookup-prefix (Preprocessed.memory s) mem-rest gw gw<len))
                (trans (cong (λ m → mem-lookup m gw) memeq) at-gw)
        eval-s≡ : eval-guard (Preprocessed.memory s) (just gw) ≡ just true
        eval-s≡ = trans (eval-guard-eq (Preprocessed.memory s) gw look-s-gw)
                        (trans (cong to-bool gv≡1) to-bool-of-1ᶠ)
        eval-w≡ : eval-guard (Witness.mem w) (just gw) ≡ just true
        eval-w≡ = trans (eval-guard-eq (Witness.mem w) gw at-gw)
                        (trans (cong to-bool gv≡1) to-bool-of-1ᶠ)
        mk≡ : make-pti (Witness.mem w) (pi-skip (just gw) count ∷ is)
                grp pis-rest
            ≡ grp ++ mkrest
        mk≡ = cong (λ b → if b then grp ++ mkrest else mkrest)
                   (cong (fromMaybe false) eval-w≡)
        pti≡ : ProofPreimage.pub-transcript-inputs pre
             ≡ committed ++ (grp ++ mkrest)
        pti≡ = trans (PiInv.pti-eq pii) (cong (committed ++_) mk≡)
        match = skip-match pre s committed grp mkrest ph count≡ pe
                           (PiInv.pidx pii) pti≡
        s₁ = record s { pi-skips = Preprocessed.pi-skips s ++ (nothing ∷ []) }
        pii′ : PiInv pre w is s₁ (committed ++ grp) [] pis-rest
        pii′ = mk-piinv o1tl
                 (trans (PiInv.pidx pii)
                        (sym (trans (+-identityʳ _) (length-++ committed))))
                 (Preprocessed.pis s , sym (++-identityʳ _))
                 (trans pti≡ (sym (++-assoc committed grp mkrest)))
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is s₁ mem-rest pis-rest tf-tl (wt-stay s refl wt)
            poeq pveq memeq piseq lmem lpis (committed ++ grp) [] pii′
    in mk-walk s′ (tr-cons (refl , refl , true , eval-s≡ , match) tr) m≡ p≡
      (trans i≡
          (trans (cong (_+ length mkrest) (length-++ committed))
            (trans (+-assoc (length committed) (length grp) (length mkrest))
              (trans (cong (length committed +_) (sym (length-++ grp)))
                     (cong (λ z → length committed + length z) (sym mk≡))))))
      o≡ v≡
  build pre w (pi-skip (just gw) count ∷ is) s mem-rest pis-rest
        (tf-skip (gv , at-gw , inj₁ gv≡0) tf-tl) wt poeq pveq memeq piseq
        lmem lpis committed grp pii =
    let (count≡ , o1tl) = o1-skip-inv (PiInv.o1pii pii)
        mkrest = make-pti (Witness.mem w) is [] pis-rest
        gw<len = wire-trace-head-ok wt
        look-s-gw : mem-lookup (Preprocessed.memory s) gw ≡ just gv
        look-s-gw =
          trans (sym (lookup-prefix (Preprocessed.memory s) mem-rest gw gw<len))
                (trans (cong (λ m → mem-lookup m gw) memeq) at-gw)
        eval-s≡ : eval-guard (Preprocessed.memory s) (just gw) ≡ just false
        eval-s≡ = trans (eval-guard-eq (Preprocessed.memory s) gw look-s-gw)
                        (trans (cong to-bool gv≡0) to-bool-of-0ᶠ)
        eval-w≡ : eval-guard (Witness.mem w) (just gw) ≡ just false
        eval-w≡ = trans (eval-guard-eq (Witness.mem w) gw at-gw)
                        (trans (cong to-bool gv≡0) to-bool-of-0ᶠ)
        mk≡ : make-pti (Witness.mem w) (pi-skip (just gw) count ∷ is)
                grp pis-rest
            ≡ mkrest
        mk≡ = cong (λ b → if b then grp ++ mkrest else mkrest)
                   (cong (fromMaybe false) eval-w≡)
        s₁ = record s
               { pi-skips   = Preprocessed.pi-skips s ++ (just count ∷ [])
               ; pub-in-idx = Preprocessed.pub-in-idx s ∸ count }
        pii′ : PiInv pre w is s₁ committed [] pis-rest
        pii′ = mk-piinv o1tl
                 (trans (cong₂ _∸_ (PiInv.pidx pii) count≡)
                        (trans (m+n∸n≡m (length committed) (length grp))
                               (sym (+-identityʳ _))))
                 (Preprocessed.pis s , sym (++-identityʳ _))
                 (trans (PiInv.pti-eq pii) (cong (committed ++_) mk≡))
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is s₁ mem-rest pis-rest tf-tl (wt-stay s refl wt)
            poeq pveq memeq piseq lmem lpis committed [] pii′
    in mk-walk s′ (tr-cons (refl , refl , false , eval-s≡ , tt) tr) m≡ p≡
      (trans i≡ (cong (λ z → length committed + length z) (sym mk≡)))
      o≡ v≡
  -- ── public-input ──────────────────────────────────────────────────
  build pre w (public-input nothing ∷ is) s mem-rest pis-rest
        (tf-pub-act _ at-c tf-tl) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split1 mem-rest lmem
  ... | cell , rest′ , refl , lreq =
    let cell≡c =
          just-injective
            (trans (sym (trans (sym (cong (λ m → mem-lookup m _) memeq))
                               (lookup-bound (Preprocessed.memory s) cell rest′)))
                   at-c)
        s₁ = record s { pub-out-rem = _ }
        consume-eq = consume-pub-out-eq s
                       (subst (λ z → Preprocessed.pub-out-rem s ≡ z ∷ _)
                              (sym cell≡c) poeq)
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { pub-out-rem = _
                      ; memory = Preprocessed.memory s ++ (cell ∷ []) })
            rest′ pis-rest (tf-suc s cell tf-tl)
            (wt-grow s (cell ∷ []) refl wt) refl pveq
            (trans (++-assoc (Preprocessed.memory s) (cell ∷ []) rest′) memeq)
            piseq lreq lpis committed grp
            (pi-pass s _ tt refl refl refl pii)
    in mk-walk s′
         (tr-cons (cell , refl , refl , true , refl , (s₁ , consume-eq)) tr)
         m≡ p≡ i≡ o≡ v≡
  build pre w (public-input nothing ∷ is) s mem-rest pis-rest
        (tf-pub-inact () _ _) wt poeq pveq memeq piseq lmem lpis
  build pre w (public-input (just gw) ∷ is) s mem-rest pis-rest
        (tf-pub-act (iv , at-gw , iv≡1) at-c tf-tl) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split1 mem-rest lmem
  ... | cell , rest′ , refl , lreq =
    let gw<len = wire-trace-head-ok wt
        cell≡c =
          just-injective
            (trans (sym (trans (sym (cong (λ m → mem-lookup m _) memeq))
                               (lookup-bound (Preprocessed.memory s) cell rest′)))
                   at-c)
        look-s-gw : mem-lookup (Preprocessed.memory s) gw ≡ just iv
        look-s-gw = trans (sym (lookup-prefix (Preprocessed.memory s) (cell ∷ rest′) gw gw<len))
                          (trans (cong (λ m → mem-lookup m gw) memeq) at-gw)
        eval-eq = trans (eval-guard-eq (Preprocessed.memory s) gw look-s-gw)
                        (trans (cong to-bool iv≡1) to-bool-of-1ᶠ)
        s₁ = record s { pub-out-rem = _ }
        consume-eq = consume-pub-out-eq s
                       (subst (λ z → Preprocessed.pub-out-rem s ≡ z ∷ _)
                              (sym cell≡c) poeq)
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { pub-out-rem = _
                      ; memory = Preprocessed.memory s ++ (cell ∷ []) })
            rest′ pis-rest (tf-suc s cell tf-tl)
            (wt-grow s (cell ∷ []) refl wt) refl pveq
            (trans (++-assoc (Preprocessed.memory s) (cell ∷ []) rest′) memeq)
            piseq lreq lpis committed grp
            (pi-pass s _ tt refl refl refl pii)
    in mk-walk s′
         (tr-cons (cell , refl , refl , true , eval-eq , (s₁ , consume-eq)) tr)
         m≡ p≡ i≡ o≡ v≡
  build pre w (public-input (just gw) ∷ is) s mem-rest pis-rest
        (tf-pub-inact (iv , at-gw , iv≡0) at-0 tf-tl) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split1 mem-rest lmem
  ... | cell , rest′ , refl , lreq =
    let gw<len = wire-trace-head-ok wt
        cell≡0 =
          just-injective
            (trans (sym (trans (sym (cong (λ m → mem-lookup m _) memeq))
                               (lookup-bound (Preprocessed.memory s) cell rest′)))
                   at-0)
        look-s-gw : mem-lookup (Preprocessed.memory s) gw ≡ just iv
        look-s-gw = trans (sym (lookup-prefix (Preprocessed.memory s) (cell ∷ rest′) gw gw<len))
                          (trans (cong (λ m → mem-lookup m gw) memeq) at-gw)
        eval-eq = trans (eval-guard-eq (Preprocessed.memory s) gw look-s-gw)
                        (trans (cong to-bool iv≡0) to-bool-of-0ᶠ)
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { memory = Preprocessed.memory s ++ (cell ∷ []) })
            rest′ pis-rest (tf-suc s cell tf-tl)
            (wt-grow s (cell ∷ []) refl wt) poeq pveq
            (trans (++-assoc (Preprocessed.memory s) (cell ∷ []) rest′) memeq)
            piseq lreq lpis committed grp
            (pi-pass s _ tt refl refl refl pii)
    in mk-walk s′
         (tr-cons (cell , refl , refl , false , eval-eq , cell≡0) tr)
         m≡ p≡ i≡ o≡ v≡
  -- ── private-input ─────────────────────────────────────────────────
  build pre w (private-input nothing ∷ is) s mem-rest pis-rest
        (tf-priv-act _ at-c tf-tl) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split1 mem-rest lmem
  ... | cell , rest′ , refl , lreq =
    let cell≡c =
          just-injective
            (trans (sym (trans (sym (cong (λ m → mem-lookup m _) memeq))
                               (lookup-bound (Preprocessed.memory s) cell rest′)))
                   at-c)
        s₁ = record s { priv-rem = _ }
        consume-eq = consume-priv-eq s
                       (subst (λ z → Preprocessed.priv-rem s ≡ z ∷ _)
                              (sym cell≡c) pveq)
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { priv-rem = _
                      ; memory = Preprocessed.memory s ++ (cell ∷ []) })
            rest′ pis-rest (tf-suc s cell tf-tl)
            (wt-grow s (cell ∷ []) refl wt) poeq refl
            (trans (++-assoc (Preprocessed.memory s) (cell ∷ []) rest′) memeq)
            piseq lreq lpis committed grp
            (pi-pass s _ tt refl refl refl pii)
    in mk-walk s′
         (tr-cons (cell , refl , refl , true , refl , (s₁ , consume-eq)) tr)
         m≡ p≡ i≡ o≡ v≡
  build pre w (private-input nothing ∷ is) s mem-rest pis-rest
        (tf-priv-inact () _ _) wt poeq pveq memeq piseq lmem lpis
  build pre w (private-input (just gw) ∷ is) s mem-rest pis-rest
        (tf-priv-act (iv , at-gw , iv≡1) at-c tf-tl) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split1 mem-rest lmem
  ... | cell , rest′ , refl , lreq =
    let gw<len = wire-trace-head-ok wt
        cell≡c =
          just-injective
            (trans (sym (trans (sym (cong (λ m → mem-lookup m _) memeq))
                               (lookup-bound (Preprocessed.memory s) cell rest′)))
                   at-c)
        look-s-gw : mem-lookup (Preprocessed.memory s) gw ≡ just iv
        look-s-gw = trans (sym (lookup-prefix (Preprocessed.memory s) (cell ∷ rest′) gw gw<len))
                          (trans (cong (λ m → mem-lookup m gw) memeq) at-gw)
        eval-eq = trans (eval-guard-eq (Preprocessed.memory s) gw look-s-gw)
                        (trans (cong to-bool iv≡1) to-bool-of-1ᶠ)
        s₁ = record s { priv-rem = _ }
        consume-eq = consume-priv-eq s
                       (subst (λ z → Preprocessed.priv-rem s ≡ z ∷ _)
                              (sym cell≡c) pveq)
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { priv-rem = _
                      ; memory = Preprocessed.memory s ++ (cell ∷ []) })
            rest′ pis-rest (tf-suc s cell tf-tl)
            (wt-grow s (cell ∷ []) refl wt) poeq refl
            (trans (++-assoc (Preprocessed.memory s) (cell ∷ []) rest′) memeq)
            piseq lreq lpis committed grp
            (pi-pass s _ tt refl refl refl pii)
    in mk-walk s′
         (tr-cons (cell , refl , refl , true , eval-eq , (s₁ , consume-eq)) tr)
         m≡ p≡ i≡ o≡ v≡
  build pre w (private-input (just gw) ∷ is) s mem-rest pis-rest
        (tf-priv-inact (iv , at-gw , iv≡0) at-0 tf-tl) wt
        poeq pveq memeq piseq lmem lpis committed grp pii
    with split1 mem-rest lmem
  ... | cell , rest′ , refl , lreq =
    let gw<len = wire-trace-head-ok wt
        cell≡0 =
          just-injective
            (trans (sym (trans (sym (cong (λ m → mem-lookup m _) memeq))
                               (lookup-bound (Preprocessed.memory s) cell rest′)))
                   at-0)
        look-s-gw : mem-lookup (Preprocessed.memory s) gw ≡ just iv
        look-s-gw = trans (sym (lookup-prefix (Preprocessed.memory s) (cell ∷ rest′) gw gw<len))
                          (trans (cong (λ m → mem-lookup m gw) memeq) at-gw)
        eval-eq = trans (eval-guard-eq (Preprocessed.memory s) gw look-s-gw)
                        (trans (cong to-bool iv≡0) to-bool-of-0ᶠ)
        (mk-walk s′ tr m≡ p≡ i≡ o≡ v≡) =
          build pre w is
            (record s { memory = Preprocessed.memory s ++ (cell ∷ []) })
            rest′ pis-rest (tf-suc s cell tf-tl)
            (wt-grow s (cell ∷ []) refl wt) poeq pveq
            (trans (++-assoc (Preprocessed.memory s) (cell ∷ []) rest′) memeq)
            piseq lreq lpis committed grp
            (pi-pass s _ tt refl refl refl pii)
    in mk-walk s′
         (tr-cons (cell , refl , refl , false , eval-eq , cell≡0) tr)
         m≡ p≡ i≡ o≡ v≡

  step0 pre w is s mem-rest pis-rest committed grp sd tne Δ0 nds mkeq
        tf wt poeq pveq memeq piseq lenp lpis pii =
    let rec = build pre w is s mem-rest pis-rest (tf-stay s tf)
                (wt-stay s Δ0 wt) poeq pveq memeq piseq lenp lpis
                committed grp (pi-pass s s nds refl refl mkeq pii)
    in mk-walk (WalkResult.wstate rec)
         (tr-cons sd
           (subst (λ st → Tr-shaped pre st is (WalkResult.wstate rec)
                            mem-rest pis-rest)
                  (sym tne) (WalkResult.wtr rec)))
         (WalkResult.wmem rec) (WalkResult.wpis rec)
         (trans (WalkResult.widx rec)
                (cong (λ z → length committed + length z) (sym mkeq)))
         (WalkResult.wpout rec) (WalkResult.wpriv rec)

  step1 pre w is s c rest′ pis-rest committed grp sd tne Δ1 nds mkeq
        tf wt poeq pveq memeq piseq lreq lpis pii =
    let rec = build pre w is (push-mem s c) rest′ pis-rest
                (tf-grow1 s c tf) (wt-grow s (c ∷ []) (sym Δ1) wt) poeq pveq
                (trans (++-assoc (Preprocessed.memory s) (c ∷ []) rest′)
                       memeq)
                piseq lreq lpis committed grp
                (pi-pass s (push-mem s c) nds refl refl mkeq pii)
    in mk-walk (WalkResult.wstate rec)
         (tr-cons sd
           (subst (λ st → Tr-shaped pre st is (WalkResult.wstate rec)
                            rest′ pis-rest)
                  (sym tne) (WalkResult.wtr rec)))
         (WalkResult.wmem rec) (WalkResult.wpis rec)
         (trans (WalkResult.widx rec)
                (cong (λ z → length committed + length z) (sym mkeq)))
         (WalkResult.wpout rec) (WalkResult.wpriv rec)

  step2 pre w is s a b rest′ pis-rest committed grp sd tne Δ2 nds mkeq
        tf wt poeq pveq memeq piseq lreq lpis pii =
    let rec = build pre w is (push-mem2 s a b) rest′ pis-rest
                (tf-grow2 s a b tf) (wt-grow s (a ∷ b ∷ []) (sym Δ2) wt)
                poeq pveq
                (trans (++-assoc (Preprocessed.memory s) (a ∷ b ∷ []) rest′)
                       memeq)
                piseq lreq lpis committed grp
                (pi-pass s (push-mem2 s a b) nds refl refl mkeq pii)
    in mk-walk (WalkResult.wstate rec)
         (tr-cons sd
           (subst (λ st → Tr-shaped pre st is (WalkResult.wstate rec)
                            rest′ pis-rest)
                  (sym tne) (WalkResult.wtr rec)))
         (WalkResult.wmem rec) (WalkResult.wpis rec)
         (trans (WalkResult.widx rec)
                (cong (λ z → length committed + length z) (sym mkeq)))
         (WalkResult.wpout rec) (WalkResult.wpriv rec)

------------------------------------------------------------------------
-- The realization data.
--
-- `Realizes src w pre s` says the pair `(pre , s)` *realizes* the
-- witness `w`: the canonical operational witness of `s` under `pre`
-- equals `w`, and `s` is reachable by a (shape-faithful) preprocess run
-- of `src` on `pre`.  These are exactly the two facts
-- `circuit-faithful-bwd` consumes once `satisfies` is in hand.
------------------------------------------------------------------------

Realizes : IrSource → Witness → ProofPreimage → Preprocessed → Set
Realizes src w pre s =
  (witness-of s pre ≡ w) × preprocess-shaped src pre s

------------------------------------------------------------------------
-- The reduction step.  Given a *concrete* realization of `w` — a pair
-- `(pre , s)` with the WF1 arity fact and a `Realizes` proof — S-stmt for
-- `w` is immediate from P5's backward direction.  `realize` (below)
-- discharges the realization and applies this.
------------------------------------------------------------------------

sstmt-from-realize
  : ∀ (src : IrSource) (w : Witness)
      (pre : ProofPreimage) (s : Preprocessed)
  → T (producer-safe src)
  → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src
  → WF2 src
  → Realizes src w pre s
  → satisfies (circuit src) w
  → R src pre s × (witness-of s pre ≡ w)
sstmt-from-realize src w pre s ps-safe wf1 wf2 (weq , pps) sat =
  let sat' : satisfies (circuit src) (witness-of s pre)
      sat' = subst (satisfies (circuit src)) (sym weq) sat
      Rsrc : R src pre s
      Rsrc = circuit-faithful-bwd src pre s ps-safe wf1 wf2 pps sat'
  in Rsrc , weq

------------------------------------------------------------------------
-- Explicit init state.  Recovers the concrete `mk-state` (and hence all
-- its projections) that `init-state` returns when the input arity
-- matches and the commitment payload has the shape the `has-comm` flag
-- demands: present when the flag is set (its commitment value `c` joins
-- the pis preamble), arbitrary otherwise.
------------------------------------------------------------------------

-- The randomness component of a commitment payload (cf. `comm-rand-of`,
-- which projects it out of a whole preimage).
cm-rand : Maybe (Fr × Fr) → Maybe Fr
cm-rand (just (_ , r)) = just r
cm-rand nothing        = nothing

comm-rand-of-cm : ∀ (pre : ProofPreimage)
  → comm-rand-of pre ≡ cm-rand (ProofPreimage.comm-commitment pre)
comm-rand-of-cm pre with ProofPreimage.comm-commitment pre
... | just (_ , _) = refl
... | nothing      = refl

-- A commitment payload acceptable to `init-state`: required when the
-- flag is set, unconstrained otherwise.
CommShape : Bool → Maybe (Fr × Fr) → Set
CommShape false _        = ⊤
CommShape true  (just _) = ⊤
CommShape true  nothing  = ⊥

-- The pis preamble `init-state` seeds: the binding input, plus the
-- commitment value when the flag is set.  (The `true`/`nothing` branch
-- is arbitrary — `CommShape` rules it out.)
init-pis : Bool → Fr → Maybe (Fr × Fr) → List Fr
init-pis false b _              = b ∷ []
init-pis true  b (just (c , _)) = b ∷ c ∷ []
init-pis true  b nothing        = b ∷ []

init-explicit : ∀ (src : IrSource) (pre : ProofPreimage)
  → CommShape (IrSource.do-communications-commitment src)
              (ProofPreimage.comm-commitment pre)
  → length (ProofPreimage.inputs pre) ≡ IrSource.num-inputs src
  → init-state src pre
    ≡ just (mk-state (ProofPreimage.inputs pre)
                     (init-pis (IrSource.do-communications-commitment src)
                               (ProofPreimage.binding-input pre)
                               (ProofPreimage.comm-commitment pre))
                     [] 0
                     (ProofPreimage.pub-transcript-outputs pre)
                     (ProofPreimage.priv-transcript pre)
                     [])
init-explicit src pre cs wf1
  with length (ProofPreimage.inputs pre) ≡ᵇ IrSource.num-inputs src
     | T-≡ .Equivalence.to (≡⇒≡ᵇ _ _ wf1)
     | IrSource.do-communications-commitment src
     | ProofPreimage.comm-commitment pre
     | cs
... | .true | refl | false | _            | _  = refl
... | .true | refl | true  | just (_ , _) | _  = refl
... | .true | refl | true  | nothing      | ()

------------------------------------------------------------------------
-- The realization core, generic in the communications-commitment flag.
--
-- For a producer-safe source, every satisfying witness `w` (of the
-- allocated memory length) is the canonical witness of a genuine
-- preprocess run with the same public inputs.  The candidate preimage
-- takes its inputs from the first `num-inputs` cells of `w`, its
-- `pub-transcript-inputs` from `make-pti` (the active skip groups read
-- off `w`), and its public-output / private transcripts (`po` / `pv`)
-- from `make-trfacts` — the values the active `public-input` /
-- `private-input` instructions read off `w`.  The walk (`build`) tiles
-- the memory / pis suffixes and consumes those transcripts.
--
-- The hc-dependent data is supplied by the caller (the per-flag cases
-- of `circuit-statement-sound` below): the pis preamble split of
-- `Witness.pis w`, the commitment payload `cm` the preimage carries
-- (with `cm-rand cm` matching `w`'s comm-rand, so `witness-of` agrees),
-- and the satisfaction of the *synthesized* constraint list (for `hc = true`
-- the caller peels the trailing comm-commitment constraint; it is consumed
-- only by `circuit-faithful-bwd`, which recovers `comm-ok` from it).
------------------------------------------------------------------------

private
  realize
    : ∀ (src : IrSource) (w : Witness)
        (bind : Fr) (prest : List Fr) (cm : Maybe (Fr × Fr))
    → T (producer-safe src)
    → WF2 src
    → length (Witness.mem w) ≡ Circuit.nr-wires (circuit src)
    → satisfies (circuit src) w
    → satisfies-constraints
        (SynthState.constraints
          (circuit-instrs (IrSource.do-communications-commitment src)
            (IrSource.instructions src)
            (mk-synth (IrSource.num-inputs src) [] 0 []))) w
    → CommShape (IrSource.do-communications-commitment src) cm
    → cm-rand cm ≡ Witness.comm-rand w
    → Witness.pis w
      ≡ init-pis (IrSource.do-communications-commitment src) bind cm
        ++ prest
    → length prest ≡ Δpis-sum (IrSource.instructions src)
    → Σ ProofPreimage (λ pre → Σ Preprocessed (λ s →
          R src pre s × (witness-of s pre ≡ w)))
  realize src w bind prest cm ps wf2 mlen sat sat-cls cshape creq pis-shape
          lprest =
    pre , s′ , Rsrc , weq
    where
      hc = IrSource.do-communications-commitment src

      n : ℕ
      n = IrSource.num-inputs src

      instrs = IrSource.instructions src

      preamble = init-pis hc bind cm

      -- The circuit's wire count is `n + Δmem-sum instrs`.
      nrw≡ : Circuit.nr-wires (circuit src) ≡ n + Δmem-sum instrs
      nrw≡ = nr-wires-acc hc instrs (mk-synth n [] 0 [])

      len≡ : length (Witness.mem w) ≡ n + Δmem-sum instrs
      len≡ = trans mlen nrw≡

      n≤len : n ≤ length (Witness.mem w)
      n≤len = subst (n ≤_) (sym len≡) (m≤m+n n (Δmem-sum instrs))

      -- The producer's O2 run; its trace supplies the guard booleanity
      -- the `pi-skip` transcript facts need.
      o2run = O2-bool→Runs {src} (producer-safe-O2 {src} ps)

      -- The transcript supplies `pre` must carry, read off `w` via the
      -- guard-disj constraints.
      trf = make-trfacts {hc} w instrs (mk-synth n [] 0 []) n [] refl len≡
              (bk-empty {w}) (O2-Runs.trace o2run) sat-cls
      po  = TrSupply.pub-out trf
      pv  = TrSupply.priv trf
      tf  : TrFacts w instrs n po pv
      tf  = TrSupply.facts trf

      -- The verifier transcript: the in-order concatenation of the
      -- active skip groups, computed from `w` alone.
      pti : List Fr
      pti = make-pti (Witness.mem w) instrs [] prest

      pre : ProofPreimage
      pre = record { inputs = take n (Witness.mem w)
                   ; binding-input = bind ; comm-commitment = cm
                   ; pub-transcript-inputs = pti
                   ; pub-transcript-outputs = po
                   ; priv-transcript = pv }

      wf1 : length (ProofPreimage.inputs pre) ≡ n
      wf1 = trans (length-take n (Witness.mem w)) (m≤n⇒m⊓n≡m n≤len)

      s₀ : Preprocessed
      s₀ = mk-state (take n (Witness.mem w)) preamble [] 0 po pv []

      init≡ : init-state src pre ≡ just s₀
      init≡ = init-explicit src pre cshape wf1

      memeq : take n (Witness.mem w) ++ drop n (Witness.mem w)
            ≡ Witness.mem w
      memeq = take++drop≡id n (Witness.mem w)

      piseq : Preprocessed.pis s₀ ++ prest ≡ Witness.pis w
      piseq = sym pis-shape

      lenp : length (drop n (Witness.mem w)) ≡ Δmem-sum instrs
      lenp = trans (length-drop n (Witness.mem w))
               (trans (cong (_∸ n) len≡) (m+n∸m≡n n (Δmem-sum instrs)))

      -- The producer's wire discipline, as a `Wire-Trace` indexed by the
      -- starting wire count `n` = `num-inputs` = `length (memory s₀)`.
      wt-final : ℕ
      wt-final = proj₁ (wire-disc-sound {src} ps)
      wt-init : Wire-Trace instrs (length (Preprocessed.memory s₀)) wt-final
      wt-init = subst (λ k → Wire-Trace instrs k wt-final) (sym wf1)
                      (proj₂ (wire-disc-sound {src} ps))

      tf-init : TrFacts w instrs (length (Preprocessed.memory s₀)) po pv
      tf-init = subst (λ k → TrFacts w instrs k po pv) (sym wf1) tf

      -- The initial transcript invariant: nothing committed, empty
      -- group, O1's full-bracketing trace, and `pre`'s transcript is
      -- `make-pti` in full.
      pii₀ : PiInv pre w instrs s₀ [] [] prest
      pii₀ = mk-piinv (o1-sound src ps) refl
                      (preamble , sym (++-identityʳ _)) refl

      walk = build pre w instrs s₀ (drop n (Witness.mem w)) prest
                   tf-init wt-init refl refl memeq piseq lenp lprest
                   [] [] pii₀
      s′       = WalkResult.wstate walk
      tr       = WalkResult.wtr    walk
      mem≡     = WalkResult.wmem   walk
      pis≡     = WalkResult.wpis   walk
      pidx≡    = WalkResult.widx   walk
      pout≡    = WalkResult.wpout  walk
      priv≡    = WalkResult.wpriv  walk

      tc : T (transcripts-consumed pre s′)
      tc rewrite pout≡ | priv≡ =
        T-∧ .Equivalence.from
          ( ≡⇒≡ᵇ (length pti) (Preprocessed.pub-in-idx s′) (sym pidx≡)
          , T-∧ .Equivalence.from (tt , tt) )

      weq : witness-of s′ pre ≡ w
      weq rewrite mem≡ | pis≡ =
        cong (λ m → record { mem = Witness.mem w ; pis = Witness.pis w
                           ; comm-rand = m })
             (trans (comm-rand-of-cm pre) creq)

      pps : preprocess-shaped src pre s′
      pps = mk-shaped s₀ init≡
              (mk-shape-walk (drop n (Witness.mem w)) prest tr) tc

      Rsrc : R src pre s′
      Rsrc = proj₁ (sstmt-from-realize src w pre s′ ps wf1 wf2 (weq , pps) sat)

  -- A `Maybe-shape true` forces the comm-rand to be present.
  rand-just : ∀ {mr : Maybe Fr} → Maybe-shape true mr
    → Σ Fr (λ r → mr ≡ just r)
  rand-just {just r}  _ = r , refl
  rand-just {nothing} ()

  -- The commitment payload for an hc = false preimage: `init-state`
  -- ignores it, so it only has to reproduce `w`'s comm-rand (which the
  -- circuit leaves unconstrained — the commitment value is arbitrary).
  mk-cm-f : Maybe Fr → Maybe (Fr × Fr)
  mk-cm-f (just r) = just (0ᶠ , r)
  mk-cm-f nothing  = nothing

  cm-rand-mk-f : ∀ (mr : Maybe Fr) → cm-rand (mk-cm-f mr) ≡ mr
  cm-rand-mk-f (just _) = refl
  cm-rand-mk-f nothing  = refl

  -- hc = false: pis = binding ∷ declared tail; the synthesized constraints
  -- are the whole constraint list.
  sound-no-comm
    : ∀ (src : IrSource) (w : Witness)
    → IrSource.do-communications-commitment src ≡ false
    → T (producer-safe src)
    → WF2 src
    → length (Witness.mem w) ≡ Circuit.nr-wires (circuit src)
    → satisfies (circuit src) w
    → Σ ProofPreimage (λ pre → Σ Preprocessed (λ s →
          R src pre s × (witness-of s pre ≡ w)))
  sound-no-comm src w hc≡f ps wf2 mlen sat =
    realize src w bind prest (mk-cm-f (Witness.comm-rand w))
      ps wf2 mlen sat sat-cls cshape (cm-rand-mk-f (Witness.comm-rand w))
      pis-shape lprest
    where
      n : ℕ
      n = IrSource.num-inputs src

      instrs = IrSource.instructions src

      -- pi-len is the preamble (`1`) plus the declared-PI count.
      pilen≡ : Circuit.pi-len (circuit src) ≡ suc (Δpis-sum instrs)
      pilen≡ rewrite hc≡f =
        cong (1 +_) (nr-decl-acc false instrs (mk-synth n [] 0 []))

      pislen : length (Witness.pis w) ≡ suc (Δpis-sum instrs)
      pislen = trans (satisfies.pi-length sat) pilen≡

      spl = split1 (Witness.pis w) pislen
      bind   = proj₁ spl
      prest  = proj₁ (proj₂ spl)
      pisw≡ : Witness.pis w ≡ bind ∷ prest
      pisw≡  = proj₁ (proj₂ (proj₂ spl))
      lprest = proj₂ (proj₂ (proj₂ spl))

      pis-shape : Witness.pis w
        ≡ init-pis (IrSource.do-communications-commitment src) bind
                   (mk-cm-f (Witness.comm-rand w)) ++ prest
      pis-shape =
        subst (λ b → Witness.pis w
                   ≡ init-pis b bind (mk-cm-f (Witness.comm-rand w))
                     ++ prest)
              (sym hc≡f) pisw≡

      cshape : CommShape (IrSource.do-communications-commitment src)
                         (mk-cm-f (Witness.comm-rand w))
      cshape = subst (λ b → CommShape b (mk-cm-f (Witness.comm-rand w)))
                     (sym hc≡f) tt

      -- No comm constraint is appended, so the circuit's constraints ARE the
      -- synthesized ones.
      sat-cls : satisfies-constraints
                  (SynthState.constraints
                    (circuit-instrs
                      (IrSource.do-communications-commitment src)
                      instrs (mk-synth n [] 0 []))) w
      sat-cls = subst (λ cs → satisfies-constraints cs w) cls-eq
                      (satisfies.constraint-ok sat)
        where
          cls-eq : Circuit.constraints (circuit src)
                 ≡ SynthState.constraints
                     (circuit-instrs
                       (IrSource.do-communications-commitment src)
                       instrs (mk-synth n [] 0 []))
          cls-eq rewrite hc≡f = refl

  -- hc = true: pis = binding ∷ commitment ∷ declared tail; the witness
  -- carries the randomness (`rand-shape`); the trailing comm-commitment
  -- constraint is peeled off before the walk (it constrains no walked wire —
  -- `circuit-faithful-bwd` consumes it to recover `comm-ok`).
  sound-comm
    : ∀ (src : IrSource) (w : Witness)
    → IrSource.do-communications-commitment src ≡ true
    → T (producer-safe src)
    → WF2 src
    → length (Witness.mem w) ≡ Circuit.nr-wires (circuit src)
    → satisfies (circuit src) w
    → Σ ProofPreimage (λ pre → Σ Preprocessed (λ s →
          R src pre s × (witness-of s pre ≡ w)))
  sound-comm src w hc≡t ps wf2 mlen sat =
    realize src w bind prest (just (c , rv))
      ps wf2 mlen sat sat-cls cshape (sym rv≡) pis-shape lprest
    where
      n : ℕ
      n = IrSource.num-inputs src

      instrs = IrSource.instructions src

      -- pi-len is the preamble (`2`) plus the declared-PI count.
      pilen≡ : Circuit.pi-len (circuit src) ≡ suc (suc (Δpis-sum instrs))
      pilen≡ rewrite hc≡t =
        cong (2 +_) (nr-decl-acc true instrs (mk-synth n [] 0 []))

      pislen : length (Witness.pis w) ≡ suc (suc (Δpis-sum instrs))
      pislen = trans (satisfies.pi-length sat) pilen≡

      spl = split2 (Witness.pis w) pislen
      bind   = proj₁ spl
      c      = proj₁ (proj₂ spl)
      prest  = proj₁ (proj₂ (proj₂ spl))
      pisw≡ : Witness.pis w ≡ bind ∷ c ∷ prest
      pisw≡  = proj₁ (proj₂ (proj₂ (proj₂ spl)))
      lprest = proj₂ (proj₂ (proj₂ (proj₂ spl)))

      -- The witness carries the commitment randomness.
      rv-just = rand-just
        (subst (λ b → Maybe-shape b (Witness.comm-rand w)) hc≡t
               (satisfies.rand-shape sat))
      rv  = proj₁ rv-just
      rv≡ : Witness.comm-rand w ≡ just rv
      rv≡ = proj₂ rv-just

      pis-shape : Witness.pis w
        ≡ init-pis (IrSource.do-communications-commitment src) bind
                   (just (c , rv)) ++ prest
      pis-shape =
        subst (λ b → Witness.pis w
                   ≡ init-pis b bind (just (c , rv)) ++ prest)
              (sym hc≡t) pisw≡

      cshape : CommShape (IrSource.do-communications-commitment src)
                         (just (c , rv))
      cshape = subst (λ b → CommShape b (just (c , rv))) (sym hc≡t) tt

      -- The circuit appends the comm-commitment constraint after the
      -- synthesized ones; peel it.
      sat-cls : satisfies-constraints
                  (SynthState.constraints
                    (circuit-instrs
                      (IrSource.do-communications-commitment src)
                      instrs (mk-synth n [] 0 []))) w
      sat-cls =
        let (extra , ceq) = cls-split in
        proj₁ (sat-split
                (SynthState.constraints
                  (circuit-instrs
                    (IrSource.do-communications-commitment src)
                    instrs (mk-synth n [] 0 [])))
                extra
                (subst (λ cs → satisfies-constraints cs w) ceq
                       (satisfies.constraint-ok sat)))
        where
          cls-split : Σ (List Constraint) (λ extra →
              Circuit.constraints (circuit src)
              ≡ SynthState.constraints
                  (circuit-instrs
                    (IrSource.do-communications-commitment src)
                    instrs (mk-synth n [] 0 []))
                ++ extra)
          cls-split rewrite hc≡t = _ , refl

------------------------------------------------------------------------
-- A realizer of a witness: a preprocess run whose canonical witness is
-- exactly `w`.  Statement soundness produces one; extraction uniqueness
-- (`StatementUniqueness.agda`) shows the well-shaped one is unique.
------------------------------------------------------------------------

record Realizer (src : IrSource) (w : Witness) : Set where
  constructor mk-realizer
  field
    preimage : ProofPreimage
    run      : Preprocessed
    R-run    : R src preimage run
    extracts : witness-of run preimage ≡ w

------------------------------------------------------------------------
-- Statement soundness.  Dispatch on the communications-commitment flag.
------------------------------------------------------------------------

circuit-statement-sound
  : ∀ (src : IrSource) (w : Witness)
  → T (producer-safe src)
  → WF2 src
  → satisfies (circuit src) w
  → Realizer src w
circuit-statement-sound src w ps wf2 sat =
  go (IrSource.do-communications-commitment src) refl
  where
    mlen = satisfies.mem-length sat
    go : ∀ b → IrSource.do-communications-commitment src ≡ b
       → Realizer src w
    go false hc≡f = let (pre , s , Rs , wo≡) =
                          sound-no-comm src w hc≡f ps wf2 mlen sat
                    in mk-realizer pre s Rs wo≡
    go true  hc≡t = let (pre , s , Rs , wo≡) =
                          sound-comm src w hc≡t ps wf2 mlen sat
                    in mk-realizer pre s Rs wo≡

------------------------------------------------------------------------
-- The memory-shape condition `length (Witness.mem w) ≡ nr-wires` is the
-- `mem-length` field of `satisfies`: a satisfying assignment to a circuit
-- over `nr-wires` wires is a memory vector of exactly that length.
-- Without it `satisfies-constraints` alone would admit witnesses carrying
-- trailing junk cells beyond the allocated wires, which equal no
-- `witness-of s pre` (whose memory has exactly `nr-wires` cells along an
-- honest walk) and so would not be realizable.
------------------------------------------------------------------------

------------------------------------------------------------------------
-- Witness characterization.  `circuit-statement-sound` combined with
-- P5's forward direction (`circuit-faithful-fwd`): for a producer-safe
-- WF2 source, the satisfying witnesses are EXACTLY the canonical
-- witnesses of preprocess runs.
------------------------------------------------------------------------

circuit-witness-characterization
  : ∀ (src : IrSource) (w : Witness)
  → T (producer-safe src)
  → WF2 src
  → satisfies (circuit src) w ⇔ Realizer src w
circuit-witness-characterization src w ps wf2 = mk⇔
  (circuit-statement-sound src w ps wf2)
  (λ (mk-realizer pre s Rs wo≡) →
      subst (satisfies (circuit src)) wo≡
            (circuit-faithful-fwd src pre s Rs))
