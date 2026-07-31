{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Extraction (statement) uniqueness for zkir-v3.
--
--   statement-unique
--     : ∀ {S w} → producer-SA S → WInputs w (IrSource.inputs S)
--     → (r r′ : SubRealizer S w)
--     → (SubRealizer.P r ≡ SubRealizer.P r′)
--     × (SubRealizer.s r ≡ SubRealizer.s r′)
--
-- For a single-assignment producer and a witness `w` whose declared inputs
-- are well-shaped, every commitment-well-shaped realizer of `w` is unique:
-- any two agree on both their preimage and their preprocess state.  (Only
-- the single-assignment discipline and the input half of the witness shape
-- are needed — never operand well-typedness, never the per-step witness
-- shape.)  Combined with
-- `statement-sound` (existence) this pins EXACTLY ONE such realizer
-- (`statement-sound-unique`).
--
-- Why the whole theorem reduces to `P ≡ P′`.  Unlike v2's relational `R`,
-- v3's `run` is a deterministic FUNCTION.  Once the two preimages are
-- equal, the two initial states coincide (`init S P ≡ init S P′` by
-- `cong`, then `just`-injectivity), hence the two runs are the SAME
-- computation and the two final states coincide too — plain propositional
-- equality, no parallel two-run induction.  So the work is entirely the
-- six-field equality `P ≡ P′`, which we obtain by pinning every field of
-- each realizer's preimage to the same witness-derived value.
--
-- Commitment well-shapedness.  A preimage may carry a commitment pair only
-- when the source's flag is set.  When the flag is off, `init` seeds the
-- pis with `binding-input ∷ []` and ignores `comm-commitment` entirely, so
-- a vestigial `just (c , r)` is invisible to the run.  It is NOT wholly
-- invisible to the witness: `witness-of` reads
-- `comm-rand-of (comm-commitment P)`, so the SECOND component `r` of a
-- vestigial pair does surface as the witness's `comm-rand`.  What stays
-- invisible at `do-comm ≡ false` is the FIRST component `c` — nothing the
-- run or the witness consults depends on it — so two preimages differing
-- only in that `c` realize the same witness data.  `CommWF` removes this
-- residual slack (and, by forcing `nothing`, the `r` slack with it).
------------------------------------------------------------------------

module zkir-v3.StatementUniqueness (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Semantics ⋯
open import zkir-v3.SemanticsProperties ⋯
open import zkir-v3.Circuit ⋯
open import zkir-v3.CircuitBridge ⋯
open import zkir-v3.CircuitFaithfulness ⋯
open import zkir-v3.Obligations ⋯
open import zkir-v3.CircuitProof ⋯
open import zkir-v3.StatementSoundness ⋯
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)

open import Data.Bool    using (Bool; true; false)
open import Data.List    using (List; []; _∷_; _++_; take; drop; length; map;
                                concatMap)
open import Data.List.Properties
  using (drop-drop; take++drop≡id; length-++; length-map; ++-assoc)
open import Data.Maybe   using (Maybe; just; nothing; maybe′)
open import Data.Nat     using (ℕ; zero; suc; _+_; _*_; _∸_; _^_; _<?_)
open import Data.Nat.Properties using (+-comm)
open import Data.Product using (_×_; _,_; Σ; ∃; ∃-syntax; proj₁; proj₂)
open import Data.Sum     using (inj₁; inj₂)
open import Relation.Binary.PropositionalEquality
  using (_≡_; refl; sym; trans; cong; cong₂; subst; subst₂)
open import Data.Maybe.Properties using (just-injective)
open import Relation.Nullary using (yes; no)
open import Function using (case_of_)

-- `CommWF` and the `SubRealizer` record (the bundled `statement-sound`
-- conclusion) are defined in `StatementSoundness` and imported above.

------------------------------------------------------------------------
-- Preimage equality from field equalities (record eta).
------------------------------------------------------------------------

mk-preimage-cong : ∀ {P P′ : ProofPreimage}
  → ProofPreimage.inputs P ≡ ProofPreimage.inputs P′
  → ProofPreimage.binding-input P ≡ ProofPreimage.binding-input P′
  → ProofPreimage.comm-commitment P ≡ ProofPreimage.comm-commitment P′
  → ProofPreimage.pub-transcript-inputs P
      ≡ ProofPreimage.pub-transcript-inputs P′
  → ProofPreimage.pub-transcript-outputs P
      ≡ ProofPreimage.pub-transcript-outputs P′
  → ProofPreimage.priv-transcript P ≡ ProofPreimage.priv-transcript P′
  → P ≡ P′
mk-preimage-cong refl refl refl refl refl refl = refl

------------------------------------------------------------------------
-- Determinism: equal preimages give equal preprocess states.  This is the
-- whole content of `s ≡ s′` once `P ≡ P′` is established.
------------------------------------------------------------------------

realizer-state-cong : ∀ {S w} (r r′ : SubRealizer S w)
  → SubRealizer.P r ≡ SubRealizer.P r′
  → SubRealizer.s r ≡ SubRealizer.s r′
realizer-state-cong {S} r r′ pe =
  just-injective
    (trans (sym (run-shaped.walk (SubRealizer.shaped r)))
      (trans (cong₂ (λ Q st → run Q S st (IrSource.instructions S))
                    pe start≡)
             (run-shaped.walk (SubRealizer.shaped r′))))
  where
    start≡ : run-shaped.start (SubRealizer.shaped r)
           ≡ run-shaped.start (SubRealizer.shaped r′)
    start≡ = just-injective
      (trans (sym (run-shaped.init≡ (SubRealizer.shaped r)))
        (trans (cong (init S) pe)
               (run-shaped.init≡ (SubRealizer.shaped r′))))

------------------------------------------------------------------------
-- Per-realizer field pins.  Every field of a realizer's preimage is
-- forced by the witness `w`, so two realizers of `w` agree field by field.
------------------------------------------------------------------------

-- Shared: the initial state's store and pis sit below the final ones (the
-- run only extends the store and appends to the pis).
private
  init-⊑ : ∀ {S w} (r : SubRealizer S w) → producer-SA S
    → (State.mem (run-shaped.start (SubRealizer.shaped r))
         ⊑ State.mem (SubRealizer.s r))
    × (State.pis (run-shaped.start (SubRealizer.shaped r))
         ≼ State.pis (SubRealizer.s r))
  init-⊑ {S} {w} r sa =
    run-extends {SubRealizer.P r} {S} {SubRealizer.s r}
      (IrSource.instructions S) st0 (run-shaped.walk (SubRealizer.shaped r))
      (producer-safe→WF {S} {SubRealizer.P r} {st0} sa
        (run-shaped.init≡ (SubRealizer.shaped r)))
    where st0 = run-shaped.start (SubRealizer.shaped r)

-- The binding input is π[0], stable from `init` through the run and pinned
-- in `w` by the pis-agreement.
binding-pin : ∀ {S w} (r : SubRealizer S w) → producer-SA S
  → pi-lookup (CircuitWitness.pis w) 0
    ≡ just (ProofPreimage.binding-input (SubRealizer.P r))
binding-pin {S} {w} r sa =
  subst (λ ps → pi-lookup ps 0
                ≡ just (ProofPreimage.binding-input (SubRealizer.P r)))
        (SubRealizer.pis-agree r)
        (pi-lookup-mono (proj₂ (init-⊑ r sa))
          (init-pi0 {S} {SubRealizer.P r} {st0}
            (run-shaped.init≡ (SubRealizer.shaped r))))
  where st0 = run-shaped.start (SubRealizer.shaped r)

-- The declared inputs re-encode from the run's memory (a sub-assignment of
-- `w`) to exactly `mkInputs w`, so the raw input stream `inputs P` is
-- pinned.  Uses `decode-reencode` (needs `NoDup` of the input names, from
-- `producer-SA`) against `resolve-encode-mkInputs` (needs `WInputs`).
inputs-pin : ∀ {S w} (r : SubRealizer S w) → producer-SA S
  → WInputs w (IrSource.inputs S)
  → ProofPreimage.inputs (SubRealizer.P r) ≡ mkInputs w (IrSource.inputs S)
inputs-pin {S} {w} r sa wins =
  just-injective
    (trans (sym (decode-reencode {w} tis
                   (ProofPreimage.inputs (SubRealizer.P r))
                   (State.mem st0)
                   (init-decode {S} {SubRealizer.P r} {st0} ie)
                   (proj₁ sa) st0⊑w))
           (resolve-encode-mkInputs {w} tis wins))
  where
    tis = IrSource.inputs S
    st0 = run-shaped.start (SubRealizer.shaped r)
    ie  = run-shaped.init≡ (SubRealizer.shaped r)

    st0⊑w : State.mem st0 ⊑ᵂ w
    st0⊑w = ⊑-trans {State.mem st0} {State.mem (SubRealizer.s r)}
              {CircuitWitness.assign w}
              (proj₁ (init-⊑ r sa)) (SubRealizer.mem-agree r)

-- Under the commitment flag, the commitment pair is `just (c , rd)` with
-- `c` pinned as π[1] in `w` and `rd` pinned as `comm-rand w`.
comm-true-facts : ∀ {S w} (r : SubRealizer S w) → producer-SA S
  → IrSource.do-communications-commitment S ≡ true
  → ∃ λ c → ∃ λ rd →
       (ProofPreimage.comm-commitment (SubRealizer.P r) ≡ just (c , rd))
     × (pi-lookup (CircuitWitness.pis w) 1 ≡ just c)
     × (CircuitWitness.comm-rand w ≡ just rd)
comm-true-facts {S} {w} r sa hc =
  let (c , rd , cc) = init-comm {S} {SubRealizer.P r} {st0} hc ie
  in c , rd , cc
   , subst (λ ps → pi-lookup ps 1 ≡ just c) (SubRealizer.pis-agree r)
       (pi-lookup-mono (proj₂ (init-⊑ r sa))
         (init-pi1 {S} {SubRealizer.P r} {st0} {c} {rd} hc cc ie))
   , sym (trans (sym (cong comm-rand-of cc)) (SubRealizer.rand-agree r hc))
  where
    st0 = run-shaped.start (SubRealizer.shaped r)
    ie  = run-shaped.init≡ (SubRealizer.shaped r)

------------------------------------------------------------------------
-- The transcript-pinning walk.
--
-- Every field of a realizer's transcript is consumed by the run exactly
-- as the corresponding w-only pre-pass (`mkPTI`/`mkPTO`/`mkPRV`) computes
-- it.  The single per-step lemma `step-transcripts` peels one instruction
-- of the run, relating the three transcript invariants at the pre-state to
-- the post-state.  Only `impact` (pub-transcript-inputs), `public-input`
-- (pub-transcript-outputs) and `private-input` (priv-transcript) touch a
-- transcript; every other instruction preserves all three cursors, so its
-- clause returns the inductive hypotheses unchanged.
------------------------------------------------------------------------

private
  -- Store monotonicity of the Semantics resolvers (the guard bridge: a
  -- guard resolving in the run's memory resolves the same against `w`).
  resolveᶠ-⊑ : ∀ {m m′} op {x} → m ⊑ m′
    → resolveᶠ m op ≡ just x → resolveᶠ m′ op ≡ just x
  resolveᶠ-⊑ {m} {m′} op sub e =
    resolveᶠ-intro m′ op (resolve-mono op sub (resolveᶠ-inv m op e))

  resolve𝔹-⊑ : ∀ {m m′} op {b} → m ⊑ m′
    → resolve𝔹 m op ≡ just b → resolve𝔹 m′ op ≡ just b
  resolve𝔹-⊑ {m} op sub e with resolveᶠ m op in re | e
  ... | just x  | e′ rewrite resolveᶠ-⊑ op sub re = e′
  ... | nothing | e′ = case e′ of λ ()

  eval-guard-⊑ : ∀ {m m′} guard {b} → m ⊑ m′
    → eval-guard m guard ≡ just b → eval-guard m′ guard ≡ just b
  eval-guard-⊑ nothing    sub e = e
  eval-guard-⊑ (just op)  sub e = resolve𝔹-⊑ op sub e

  drop-len : ∀ {A : Set} (xs : List A) → drop (length xs) xs ≡ []
  drop-len []       = refl
  drop-len (x ∷ xs) = drop-len xs

  -- One step of the walk: given the three transcript invariants at the
  -- post-state `st'`, re-establish them at the pre-state `st` for `i ∷ is`.
  step-transcripts : ∀ {P S w st st'} i is
    → step P S st i ≡ just st'
    → State.mem st  ⊑ᵂ w
    → State.mem st' ⊑ᵂ w
    → State.pis st' ≼ CircuitWitness.pis w
    → drop (State.pti-idx st') (ProofPreimage.pub-transcript-inputs P)
        ≡ mkPTI w (length (State.pis st')) is
    → State.pto-rem st' ≡ mkPTO w is
    → State.priv-rem st' ≡ mkPRV w is
    → (drop (State.pti-idx st) (ProofPreimage.pub-transcript-inputs P)
         ≡ mkPTI w (length (State.pis st)) (i ∷ is))
    × (State.pto-rem st ≡ mkPTO w (i ∷ is))
    × (State.priv-rem st ≡ mkPRV w (i ∷ is))

  -- encode / div-mod-power-of-two: bind via `insertMany`; only the store
  -- changes, so the cursors are preserved (`insertMany-shape`).
  step-transcripts {P = P} {w = w} {st = st} (encode input outputs) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) input
  ... | just v with insertMany-shape st outputs
                      (map val-native (encodeᵉ v)) e
  ...   | (pis≡ , pti≡ , pto≡ , prv≡) =
          subst₂ (λ a b → drop a (ProofPreimage.pub-transcript-inputs P)
                          ≡ mkPTI w (length b) is) pti≡ pis≡ ptiIH
        , subst (λ x → x ≡ mkPTO w is) pto≡ ptoIH
        , subst (λ x → x ≡ mkPRV w is) prv≡ prvIH
  step-transcripts {P = P} {w = w} {st = st} (div-mod-power-of-two val bits outs) is e
    _ _ _ ptiIH ptoIH prvIH
    with resolveᶠ (State.mem st) val
  ... | just x with insertMany-shape st outs
                      ( val-native (from-le-bits (drop bits (to-le-bits x)))
                      ∷ val-native (from-le-bits (take bits (to-le-bits x)))
                      ∷ []) e
  ...   | (pis≡ , pti≡ , pto≡ , prv≡) =
          subst₂ (λ a b → drop a (ProofPreimage.pub-transcript-inputs P)
                          ≡ mkPTI w (length b) is) pti≡ pis≡ ptiIH
        , subst (λ x₁ → x₁ ≡ mkPTO w is) pto≡ ptoIH
        , subst (λ x₁ → x₁ ≡ mkPRV w is) prv≡ prvIH

  -- assert / constrain-* leave the state unchanged.
  step-transcripts {st = st} (assert cond) is e _ _ _ ptiIH ptoIH prvIH
    with resolve𝔹 (State.mem st) cond
  ... | just true with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (constrain-bits val bits) is e _ _ _
    ptiIH ptoIH prvIH
    with resolveᶠ (State.mem st) val
  ... | just x with valFr x <? 2 ^ bits
  ...   | yes _ with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (constrain-eq a b) is e _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) a | resolve (State.mem st) b
  ... | just av | just bv with valEq? av bv
  ...   | just true with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (constrain-to-boolean val) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve𝔹 (State.mem st) val
  ... | just _ with refl ← e = ptiIH , ptoIH , prvIH

  -- Single-output arithmetic / conversion instructions bind one fresh cell
  -- (`out1`), preserving every transcript cursor.
  step-transcripts {st = st} (cond-select bit a b output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve𝔹 (State.mem st) bit
  ... | just bv with resolve (State.mem st) a | resolve (State.mem st) b
  ...   | just av | just bvl with typeof av ≟T typeof bvl
  ...     | yes _ with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (copy val output) is e _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) val
  ... | just v with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (ec-mul a scalar output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) a | resolve (State.mem st) scalar
  ... | just (val-jubjub-point p) | just (val-jubjub-scalar s)
          with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-point p) | just (val-secp-scalar s)
          with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (ec-mul-generator scalar output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) scalar
  ... | just (val-jubjub-scalar s) with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-scalar s) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (hash-to-curve inputs output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve-all-Fr (State.mem st) inputs
  ... | just frs with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (into-coordinates point (xid , yid)) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) point
  ... | just (val-jubjub-point p) with coordsJ p
  ...   | (x , y) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (into-coordinates point (xid , yid)) is e _ _ _
    ptiIH ptoIH prvIH
    | just (val-secp-point p) with coordsK p
  ...   | just (x , y) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (from-coordinates (xop , yop) output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) xop | resolve (State.mem st) yop
  ... | just (val-native x) | just (val-native y) with fromCoordsJ x y
  ...   | just p with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (from-coordinates (xop , yop) output) is e _ _ _
    ptiIH ptoIH prvIH
    | just (val-secp-base x) | just (val-secp-base y) with fromCoordsK x y
  ...   | just p with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (into-bytes32 input output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) input
  ... | just (val-native x) with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-base x) with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-scalar x) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (from-bytes32 bytes native output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (from-bytes32 bytes secp-base output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (from-bytes32 bytes secp-scalar output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (from-bytes32 bytes bytes32 output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) = case e of λ ()
  step-transcripts {st = st} (from-bytes32 bytes jubjub-point output) is e
    _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) = case e of λ ()
  step-transcripts {st = st} (from-bytes32 bytes jubjub-scalar output) is e
    _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) = case e of λ ()
  step-transcripts {st = st} (from-bytes32 bytes secp-point output) is e
    _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) = case e of λ ()
  step-transcripts {st = st} (reverse-bytes bytes output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (bytes32-into-low-high bytes (loid , hiid)) is e
    _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) bytes
  ... | just (val-bytes32 b) with bytes32→low-high b
  ...   | (lo , hi) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (bytes32-from-low-high (loop , hiop) output) is e
    _ _ _ ptiIH ptoIH prvIH
    with resolveᶠ (State.mem st) loop | resolveᶠ (State.mem st) hiop
  ... | just lo | just hi with low-high→bytes32 lo hi
  ...   | just b with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (reconstitute-field divisor modulus bits output)
    is e _ _ _ ptiIH ptoIH prvIH
    with resolveᶠ (State.mem st) divisor | resolveᶠ (State.mem st) modulus
  ... | just d | just mo with valFr mo <? 2 ^ bits
  ...   | yes _ with valFr d <? 2 ^ (FR-BITS ∸ bits)
  ...     | yes _ with valFr mo + 2 ^ bits * valFr d <? FR-ORDER
  ...       | yes _ with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (transient-hash inputs output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve-all-Fr (State.mem st) inputs
  ... | just frs with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (persistent-hash alignment inputs output) is e
    _ _ _ ptiIH ptoIH prvIH
    with resolve-all-Fr (State.mem st) inputs
  ... | just frs with persistent-hash-fn alignment frs
  ...   | just h with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (keccak256 alignment inputs output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolve-all-Fr (State.mem st) inputs
  ... | just frs with keccak-fn alignment frs
  ...   | just h with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (test-eq a b output) is e _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) a | resolve (State.mem st) b
  ... | just av | just bv with valEq? av bv
  ...   | just eqv with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (add a b output) is e _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) a | resolve (State.mem st) b
  ... | just (val-native x) | just (val-native y) with refl ← e =
          ptiIH , ptoIH , prvIH
  ... | just (val-jubjub-point p) | just (val-jubjub-point q)
          with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-point p) | just (val-secp-point q)
          with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-base x) | just (val-secp-base y)
          with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-scalar x) | just (val-secp-scalar y)
          with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (mul a b output) is e _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) a | resolve (State.mem st) b
  ... | just (val-native x) | just (val-native y) with refl ← e =
          ptiIH , ptoIH , prvIH
  ... | just (val-secp-base x) | just (val-secp-base y)
          with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-scalar x) | just (val-secp-scalar y)
          with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (neg a output) is e _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) a
  ... | just (val-native x) with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-jubjub-point p) with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-point p) with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-base x) with refl ← e = ptiIH , ptoIH , prvIH
  ... | just (val-secp-scalar x) with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (inv a output) is e _ _ _ ptiIH ptoIH prvIH
    with resolve (State.mem st) a
  ... | just (val-native x) with invᶠ x
  ...   | just xi with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (inv a output) is e _ _ _ ptiIH ptoIH prvIH
    | just (val-secp-base x) with invᵇ x
  ...   | just xi with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (inv a output) is e _ _ _ ptiIH ptoIH prvIH
    | just (val-secp-scalar x) with invˢ x
  ...   | just xi with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (not a output) is e _ _ _ ptiIH ptoIH prvIH
    with resolve𝔹 (State.mem st) a
  ... | just b with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (less-than a b bits output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolveᶠ (State.mem st) a | resolveᶠ (State.mem st) b
  ... | just x | just y with valFr x <? 2 ^ bits
  ...   | yes _ with valFr y <? 2 ^ bits
  ...     | yes _ with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {st = st} (jubjub-scalar-from-native a output) is e _ _ _
    ptiIH ptoIH prvIH
    with resolveᶠ (State.mem st) a
  ... | just x with refl ← e = ptiIH , ptoIH , prvIH
  step-transcripts {P = P} {S = S} {st = st} (circuit-output vals) is e _ _ _
    ptiIH ptoIH prvIH
    with collectOutputs (State.mem st) (IrSource.outputs S) vals
  ... | just vs with refl ← e = ptiIH , ptoIH , prvIH

  -- impact: the only pis-appending instruction.  Active guard consumes a
  -- pub-transcript-inputs window equal to the pushed values; the window in
  -- `w` matches because the run wrote those values into a pis prefix of `w`.
  step-transcripts {P = P} {w = w} {st = st} (impact guard inputs) is e agrst _ p≼w
    ptiIH ptoIH prvIH
    with resolve-all-Fr (State.mem st) inputs in rav
  ... | just vals with resolve𝔹 (State.mem st) guard in rgb
  ...   | just g with g
  ...     | true
            with take (length vals)
                   (drop (State.pti-idx st)
                     (ProofPreimage.pub-transcript-inputs P)) ≟LFr vals | e
  ...       | no ¬p       | ()
  ...       | yes slice-eq | refl = goal1 , ptoIH , prvIH
              where
                PTI = ProofPreimage.pub-transcript-inputs P
                nn  = length vals
                r𝔹w : resolve𝔹 (CircuitWitness.assign w) guard ≡ just true
                r𝔹w = resolve𝔹-⊑ guard agrst rgb
                lenvals : length vals ≡ length inputs
                lenvals = resolve-all-Fr-length (State.mem st) inputs rav
                zs   = proj₁ p≼w
                pweq : CircuitWitness.pis w ≡ (State.pis st ++ vals) ++ zs
                pweq = proj₂ p≼w
                drop-w : drop (length (State.pis st)) (CircuitWitness.pis w)
                       ≡ vals ++ zs
                drop-w =
                  trans (cong (drop (length (State.pis st))) pweq)
                    (trans (cong (drop (length (State.pis st)))
                              (++-assoc (State.pis st) vals zs))
                           (drop-len-++ (State.pis st) (vals ++ zs)))
                window≡vals :
                  take (length inputs)
                    (drop (length (State.pis st)) (CircuitWitness.pis w))
                  ≡ vals
                window≡vals =
                  trans (cong (take (length inputs)) drop-w)
                    (trans (cong (λ k → take k (vals ++ zs)) (sym lenvals))
                           (take-len-++ vals zs))
                len-pis' : length (State.pis st ++ vals)
                         ≡ length (State.pis st) + length inputs
                len-pis' = trans (length-++ (State.pis st))
                             (cong (length (State.pis st) +_) lenvals)
                drop-tail :
                  drop nn (drop (State.pti-idx st) PTI)
                  ≡ mkPTI w (length (State.pis st) + length inputs) is
                drop-tail =
                  trans (drop-drop (State.pti-idx st) nn PTI)
                    (trans ptiIH (cong (λ n → mkPTI w n is) len-pis'))
                goal1 :
                  drop (State.pti-idx st) PTI
                  ≡ mkPTI w (length (State.pis st)) (impact guard inputs ∷ is)
                goal1 =
                  trans (sym (take++drop≡id nn (drop (State.pti-idx st) PTI)))
                    (trans (cong₂ _++_ slice-eq drop-tail)
                      (trans (cong (_++ mkPTI w
                                     (length (State.pis st) + length inputs) is)
                                (sym window≡vals))
                        (sym (mkPTI-true {w} {guard} {inputs} {is}
                                (length (State.pis st)) r𝔹w))))
  step-transcripts {P = P} {w = w} {st = st} (impact guard inputs) is e agrst _ p≼w
    ptiIH ptoIH prvIH | just vals | just g | false with refl ← e =
      goal1 , ptoIH , prvIH
      where
        PTI = ProofPreimage.pub-transcript-inputs P
        r𝔹w : resolve𝔹 (CircuitWitness.assign w) guard ≡ just false
        r𝔹w = resolve𝔹-⊑ guard agrst rgb
        lenvals : length vals ≡ length inputs
        lenvals = resolve-all-Fr-length (State.mem st) inputs rav
        len-pis' : length (State.pis st ++ map (λ _ → 0ᶠ) vals)
                 ≡ length (State.pis st) + length inputs
        len-pis' = trans (length-++ (State.pis st))
                     (cong (length (State.pis st) +_)
                       (trans (length-map (λ _ → 0ᶠ) vals) lenvals))
        goal1 :
          drop (State.pti-idx st) PTI
          ≡ mkPTI w (length (State.pis st)) (impact guard inputs ∷ is)
        goal1 =
          trans ptiIH
            (trans (cong (λ n → mkPTI w n is) len-pis')
                   (sym (mkPTI-false {w} {guard} {inputs} {is}
                           (length (State.pis st)) r𝔹w)))

  -- public-input: an active guard pops the encoding of `w`'s output cell
  -- from pub-transcript-outputs; an inactive guard pops nothing.
  step-transcripts {P = P} {w = w} {st = st} (public-input guard val-t output) is e
    agrst agrst' _ ptiIH ptoIH prvIH
    with eval-guard (State.mem st) guard in egm
  ... | just g with g
  ...   | true with decode val-t (take (encoded-len val-t)
                                    (State.pto-rem st)) in ded | e
  ...     | nothing | ()
  ...     | just v  | refl = ptiIH , goal2 , prvIH
            where
              n = encoded-len val-t
              egw : eval-guard (CircuitWitness.assign w) guard ≡ just true
              egw = eval-guard-⊑ guard agrst egm
              wo : CircuitWitness.assign w output ≡ just v
              wo = agrst' (ins-here output v (State.mem st))
              chunk≡ : take n (State.pto-rem st) ≡ encodeᵉ v
              chunk≡ = sym (decode-encode-chunk val-t
                             (take n (State.pto-rem st)) ded)
              goal2 : State.pto-rem st
                    ≡ mkPTO w (public-input guard val-t output ∷ is)
              goal2 =
                trans (sym (take++drop≡id n (State.pto-rem st)))
                  (trans (cong₂ _++_ chunk≡ ptoIH)
                    (sym (mkPTO-active {w} {guard} {val-t} {output} {is} {v}
                           egw wo)))
  step-transcripts {P = P} {w = w} {st = st} (public-input guard val-t output) is e
    agrst agrst' _ ptiIH ptoIH prvIH | just g | false with refl ← e =
      ptiIH , goal2 , prvIH
      where
        egwf : eval-guard (CircuitWitness.assign w) guard ≡ just false
        egwf = eval-guard-⊑ guard agrst egm
        goal2 : State.pto-rem st
              ≡ mkPTO w (public-input guard val-t output ∷ is)
        goal2 = trans ptoIH
                  (sym (mkPTO-false {w} {guard} {val-t} {output} {is} egwf))

  -- private-input: the priv-transcript analogue of public-input.
  step-transcripts {P = P} {w = w} {st = st} (private-input guard val-t output) is e
    agrst agrst' _ ptiIH ptoIH prvIH
    with eval-guard (State.mem st) guard in egm
  ... | just g with g
  ...   | true with decode val-t (take (encoded-len val-t)
                                    (State.priv-rem st)) in ded | e
  ...     | nothing | ()
  ...     | just v  | refl = ptiIH , ptoIH , goal3
            where
              n = encoded-len val-t
              egw : eval-guard (CircuitWitness.assign w) guard ≡ just true
              egw = eval-guard-⊑ guard agrst egm
              wo : CircuitWitness.assign w output ≡ just v
              wo = agrst' (ins-here output v (State.mem st))
              chunk≡ : take n (State.priv-rem st) ≡ encodeᵉ v
              chunk≡ = sym (decode-encode-chunk val-t
                             (take n (State.priv-rem st)) ded)
              goal3 : State.priv-rem st
                    ≡ mkPRV w (private-input guard val-t output ∷ is)
              goal3 =
                trans (sym (take++drop≡id n (State.priv-rem st)))
                  (trans (cong₂ _++_ chunk≡ prvIH)
                    (sym (mkPRV-active {w} {guard} {val-t} {output} {is} {v}
                           egw wo)))
  step-transcripts {P = P} {w = w} {st = st} (private-input guard val-t output) is e
    agrst agrst' _ ptiIH ptoIH prvIH | just g | false with refl ← e =
      ptiIH , ptoIH , goal3
      where
        egwf : eval-guard (CircuitWitness.assign w) guard ≡ just false
        egwf = eval-guard-⊑ guard agrst egm
        goal3 : State.priv-rem st
              ≡ mkPRV w (private-input guard val-t output ∷ is)
        goal3 = trans prvIH
                  (sym (mkPRV-false {w} {guard} {val-t} {output} {is} egwf))

  -- The walk: fold `step-transcripts` over the whole run.  The base case
  -- reads the three pins off `Consumed` at the final state; the step case
  -- derives the per-step memory/pis agreements from `run-extends` on the
  -- tail run.
  transcripts-walk : ∀ {P S w s} st is
    → run P S st is ≡ just s
    → WF-run P S is st
    → State.mem s ⊑ᵂ w
    → State.pis s ≡ CircuitWitness.pis w
    → Consumed P s
    → (drop (State.pti-idx st) (ProofPreimage.pub-transcript-inputs P)
         ≡ mkPTI w (length (State.pis st)) is)
    × (State.pto-rem st ≡ mkPTO w is)
    × (State.priv-rem st ≡ mkPRV w is)
  transcripts-walk {P} {S} {w} {s} st [] run-eq wf magr pagr (ci , cto , cpr)
    with refl ← just-injective run-eq =
      trans (cong (λ n → drop n (ProofPreimage.pub-transcript-inputs P)) ci)
            (drop-len (ProofPreimage.pub-transcript-inputs P))
    , cto , cpr
  transcripts-walk {P} {S} {w} {s} st (i ∷ is) run-eq wf magr pagr consumed =
    let (st-mid , step-eq , run-rest) = run-inv i is st run-eq
        (of , wf-cont) = wf
        wf-mid = wf-cont step-eq
        agrst  = ⊑-trans {State.mem st} {State.mem s} {CircuitWitness.assign w}
                   (proj₁ (run-extends (i ∷ is) st run-eq wf)) magr
        agrst' = ⊑-trans {State.mem st-mid} {State.mem s}
                   {CircuitWitness.assign w}
                   (proj₁ (run-extends is st-mid run-rest wf-mid)) magr
        p≼w    = subst (State.pis st-mid ≼_) pagr
                   (proj₂ (run-extends is st-mid run-rest wf-mid))
        (ptiIH , ptoIH , prvIH) =
          transcripts-walk {P} {S} {w} {s} st-mid is run-rest wf-mid magr pagr
            consumed
    in step-transcripts {P} {S} {w} {st} {st-mid} i is step-eq agrst agrst' p≼w
         ptiIH ptoIH prvIH

------------------------------------------------------------------------
-- Initial-cursor inversions: `init` seeds `pti-idx` to 0 and the two
-- output cursors to the corresponding transcript fields of the preimage.
------------------------------------------------------------------------

-- One inversion of `init` reads off all three initial cursors; the
-- named facts are projections.
init-cursors : ∀ {S P st0} → init S P ≡ just st0
  → State.pti-idx st0 ≡ 0
  × State.pto-rem st0 ≡ ProofPreimage.pub-transcript-outputs P
  × State.priv-rem st0 ≡ ProofPreimage.priv-transcript P
init-cursors {S} {P} {st0} ieq
  with decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P)
... | just m with IrSource.do-communications-commitment S
...   | false = sym (cong State.pti-idx  (just-injective ieq))
              , sym (cong State.pto-rem  (just-injective ieq))
              , sym (cong State.priv-rem (just-injective ieq))
init-cursors {S} {P} {st0} ieq | just m | true
  with ProofPreimage.comm-commitment P
...     | just (c , _) = sym (cong State.pti-idx  (just-injective ieq))
                       , sym (cong State.pto-rem  (just-injective ieq))
                       , sym (cong State.priv-rem (just-injective ieq))

init-pti0 : ∀ {S P st0} → init S P ≡ just st0 → State.pti-idx st0 ≡ 0
init-pti0 {S} {P} {st0} ieq = proj₁ (init-cursors {S} {P} {st0} ieq)

init-pto0 : ∀ {S P st0} → init S P ≡ just st0
  → State.pto-rem st0 ≡ ProofPreimage.pub-transcript-outputs P
init-pto0 {S} {P} {st0} ieq = proj₁ (proj₂ (init-cursors {S} {P} {st0} ieq))

init-priv0 : ∀ {S P st0} → init S P ≡ just st0
  → State.priv-rem st0 ≡ ProofPreimage.priv-transcript P
init-priv0 {S} {P} {st0} ieq = proj₂ (proj₂ (init-cursors {S} {P} {st0} ieq))

------------------------------------------------------------------------
-- The three transcript pins.  Each field of a realizer's transcript
-- equals the w-only pre-pass, via `transcripts-walk` and the initial
-- cursor values.
------------------------------------------------------------------------

private
  walk-of : ∀ {S w} (r : SubRealizer S w) → producer-SA S
    → (drop (State.pti-idx (run-shaped.start (SubRealizer.shaped r)))
          (ProofPreimage.pub-transcript-inputs (SubRealizer.P r))
        ≡ mkPTI w (length (State.pis (run-shaped.start (SubRealizer.shaped r))))
                  (IrSource.instructions S))
    × (State.pto-rem (run-shaped.start (SubRealizer.shaped r))
        ≡ mkPTO w (IrSource.instructions S))
    × (State.priv-rem (run-shaped.start (SubRealizer.shaped r))
        ≡ mkPRV w (IrSource.instructions S))
  walk-of {S} {w} r sa =
    transcripts-walk {SubRealizer.P r} {S} {w} {SubRealizer.s r}
      st0 (IrSource.instructions S) (run-shaped.walk (SubRealizer.shaped r))
      (producer-safe→WF {S} {SubRealizer.P r} {st0} sa ie)
      (SubRealizer.mem-agree r) (SubRealizer.pis-agree r)
      (run-shaped.consumed (SubRealizer.shaped r))
    where
      st0 = run-shaped.start (SubRealizer.shaped r)
      ie  = run-shaped.init≡ (SubRealizer.shaped r)

pto-pin : ∀ {S w} (r : SubRealizer S w) → producer-SA S
  → ProofPreimage.pub-transcript-outputs (SubRealizer.P r)
    ≡ mkPTO w (IrSource.instructions S)
pto-pin {S} {w} r sa =
  trans (sym (init-pto0 {S} {SubRealizer.P r}
                {run-shaped.start (SubRealizer.shaped r)}
                (run-shaped.init≡ (SubRealizer.shaped r))))
        (proj₁ (proj₂ (walk-of r sa)))

priv-pin : ∀ {S w} (r : SubRealizer S w) → producer-SA S
  → ProofPreimage.priv-transcript (SubRealizer.P r)
    ≡ mkPRV w (IrSource.instructions S)
priv-pin {S} {w} r sa =
  trans (sym (init-priv0 {S} {SubRealizer.P r}
                {run-shaped.start (SubRealizer.shaped r)}
                (run-shaped.init≡ (SubRealizer.shaped r))))
        (proj₂ (proj₂ (walk-of r sa)))

pti-pin : ∀ {S w} (r : SubRealizer S w) → producer-SA S
  → ProofPreimage.pub-transcript-inputs (SubRealizer.P r)
    ≡ mkPTI w
        (length (State.pis (run-shaped.start (SubRealizer.shaped r))))
        (IrSource.instructions S)
pti-pin {S} {w} r sa =
  trans (sym (cong
                (λ n → drop n (ProofPreimage.pub-transcript-inputs
                                (SubRealizer.P r)))
                (init-pti0 {S} {SubRealizer.P r}
                  {run-shaped.start (SubRealizer.shaped r)}
                  (run-shaped.init≡ (SubRealizer.shaped r)))))
        (proj₁ (walk-of r sa))

------------------------------------------------------------------------
-- Extraction uniqueness: two realizers of the same witness agree.
------------------------------------------------------------------------

statement-unique : ∀ {S w} → producer-SA S → WInputs w (IrSource.inputs S)
  → (r r′ : SubRealizer S w)
  → (SubRealizer.P r ≡ SubRealizer.P r′)
  × (SubRealizer.s r ≡ SubRealizer.s r′)
statement-unique {S} {w} sa wins r r′ = pe , realizer-state-cong r r′ pe
  where
    is  = IrSource.instructions S
    ie  = run-shaped.init≡ (SubRealizer.shaped r)
    ie′ = run-shaped.init≡ (SubRealizer.shaped r′)

    inputs≡ : ProofPreimage.inputs (SubRealizer.P r)
            ≡ ProofPreimage.inputs (SubRealizer.P r′)
    inputs≡ = trans (inputs-pin r sa wins) (sym (inputs-pin r′ sa wins))

    bind≡ : ProofPreimage.binding-input (SubRealizer.P r)
          ≡ ProofPreimage.binding-input (SubRealizer.P r′)
    bind≡ = just-injective (trans (sym (binding-pin r sa)) (binding-pin r′ sa))

    pto≡ : ProofPreimage.pub-transcript-outputs (SubRealizer.P r)
         ≡ ProofPreimage.pub-transcript-outputs (SubRealizer.P r′)
    pto≡ = trans (pto-pin r sa) (sym (pto-pin r′ sa))

    priv≡ : ProofPreimage.priv-transcript (SubRealizer.P r)
          ≡ ProofPreimage.priv-transcript (SubRealizer.P r′)
    priv≡ = trans (priv-pin r sa) (sym (priv-pin r′ sa))

    -- The two initial-preamble pis lengths agree (both the preamble count
    -- of the shared flag), so the two `mkPTI` cursors coincide.
    len≡ : length (State.pis (run-shaped.start (SubRealizer.shaped r)))
         ≡ length (State.pis (run-shaped.start (SubRealizer.shaped r′)))
    len≡ = trans (init-pis-len {S} {SubRealizer.P r} ie)
             (sym (init-pis-len {S} {SubRealizer.P r′} ie′))

    pti≡ : ProofPreimage.pub-transcript-inputs (SubRealizer.P r)
         ≡ ProofPreimage.pub-transcript-inputs (SubRealizer.P r′)
    pti≡ = trans (pti-pin r sa)
             (trans (cong (λ k → mkPTI w k is) len≡)
                    (sym (pti-pin r′ sa)))

    comm≡ : ProofPreimage.comm-commitment (SubRealizer.P r)
          ≡ ProofPreimage.comm-commitment (SubRealizer.P r′)
    comm≡ = aux (IrSource.do-communications-commitment S) refl
      where
        aux : ∀ b → IrSource.do-communications-commitment S ≡ b
            → ProofPreimage.comm-commitment (SubRealizer.P r)
              ≡ ProofPreimage.comm-commitment (SubRealizer.P r′)
        aux false hc =
          trans (SubRealizer.comm-wf r hc) (sym (SubRealizer.comm-wf r′ hc))
        aux true hc =
          let (c  , rd  , ccr  , cl  , rr ) = comm-true-facts r  sa hc
              (c′ , rd′ , ccr′ , cl′ , rr′) = comm-true-facts r′ sa hc
              c≡  = just-injective (trans (sym cl) cl′)
              rd≡ = just-injective (trans (sym rr) rr′)
          in trans ccr (trans (cong just (cong₂ _,_ c≡ rd≡)) (sym ccr′))

    pe : SubRealizer.P r ≡ SubRealizer.P r′
    pe = mk-preimage-cong inputs≡ bind≡ comm≡ pti≡ pto≡ priv≡

------------------------------------------------------------------------
-- Exactly-one packaging.  `statement-sound` (existence) plus
-- `statement-unique` (uniqueness): for a well-typed producer and a
-- satisfying, minimally-shaped witness `w`, there is a commitment-well-
-- shaped realizer whose preimage and state any other realizer of `w`
-- agrees with.  `statement-sound`'s built `P'` is `CommWF`: at
-- `do-comm ≡ false` it carries `comm-commitment ≡ nothing`, and at
-- `do-comm ≡ true` any commitment is well-shaped.
------------------------------------------------------------------------

statement-sound-unique : ∀ {S w}
  → producer-WT S → satisfies (synth S) w → WShape S w
  → Σ (SubRealizer S w) (λ r → ∀ (r′ : SubRealizer S w)
      → (SubRealizer.P r ≡ SubRealizer.P r′)
      × (SubRealizer.s r ≡ SubRealizer.s r′))
statement-sound-unique {S} {w} wt sat ws =
  can , λ r′ → statement-unique (proj₁ wt) (proj₁ ws) can r′
  where
    can : SubRealizer S w
    can = statement-sound {S} {w} wt sat ws
