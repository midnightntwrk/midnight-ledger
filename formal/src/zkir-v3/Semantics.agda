{-# OPTIONS --safe #-}
open import zkir-v3.Assumptions

------------------------------------------------------------------------
-- Off-circuit (preprocess) semantics of zkir-v3  (ir_vm.rs: preprocess)
--
-- A big-step functional interpreter, faithful to `IrSource::preprocess`:
-- a forward run over the instruction list, threading the named value
-- store and the public-input / transcript / commitment channels, that
-- either produces the preprocessed witness or fails (`nothing`).
--
-- Per-instruction type dispatch matches the supported types of
-- the corresponding `*_offcircuit` functions; unsupported combinations
-- fail.  The purely-structural WF2 bit-count bounds (bits < FR-BITS,
-- bits ≤ 248) are treated as a well-formedness layer (as in zkir-v2) and
-- are NOT re-checked in the step (the Rust preprocess does check them
-- dynamically); value-dependent premises are.  The runnable static check
-- is `producer-WF2?` (Obligations) — theorems transfer to the Rust
-- exactly for WF2-conforming sources.
------------------------------------------------------------------------

module zkir-v3.Semantics (⋯ : _) (open Assumptions ⋯) where

open import zkir-v3.Types ⋯
open import zkir-v3.Syntax ⋯
open import zkir-v3.Encoding ⋯ renaming (encode to encodeᵉ)

open import Data.Bool using (Bool; true; false; if_then_else_)
  renaming (not to bnot)
open import Data.Nat  using (ℕ; zero; suc; _+_; _*_; _^_; _∸_; _<?_; _≟_)
open import Data.List using (List; []; _∷_; _++_; length; take; drop; map;
                             concatMap)
open import Data.Maybe using (Maybe; just; nothing; _>>=_)
open import Data.Product using (_×_; _,_)
open import Data.String using () renaming (_≟_ to _≟str_)
import Data.Fin as F
import Data.Fin.Properties  as FinP
import Data.Vec.Properties  as VecP
import Data.List.Properties as ListP
open import Data.Vec using (tabulate; reverse)
open import Function using (case_of_)
open import Relation.Binary.Definitions using (DecidableEquality)
open import Relation.Nullary using (Dec; yes; no)
open import Relation.Nullary.Decidable using (isYes)

------------------------------------------------------------------------
-- Numeric / decision helpers.
------------------------------------------------------------------------

-- A decision used as a guard: keep the continuation iff it holds.
guardD : {P A : Set} → Dec P → Maybe A → Maybe A
guardD (yes _) k = k
guardD (no  _) _ = nothing

-- `bits-to-ℕ` and `valFr` come from the trust base (`Assumptions`).

-- 2^n as a field element.
pow2ᶠ : ℕ → Fr
pow2ᶠ zero    = 1ᶠ
pow2ᶠ (suc n) = (1ᶠ +ᶠ 1ᶠ) *ᶠ pow2ᶠ n

-- Boolean → field embedding.
χ : Bool → Fr
χ true  = 1ᶠ
χ false = 0ᶠ

-- Decidable equality on Bytes32 (a vector of bytes).
_≟B_ : DecidableEquality Bytes32
_≟B_ = VecP.≡-dec FinP._≟_

-- Decidable equality on raw field-element lists (for the active-Impact
-- transcript-match check).
_≟LFr_ : DecidableEquality (List Fr)
_≟LFr_ = ListP.≡-dec _≟ᶠ_

-- Read a field element as a boolean: 0 ↦ false, 1 ↦ true, else fail.
to𝔹 : Fr → Maybe Bool
to𝔹 x with x ≟ᶠ 0ᶠ
... | yes _ = just false
... | no  _ with x ≟ᶠ 1ᶠ
...   | yes _ = just true
...   | no  _ = nothing

-- The default value of each type (guarded transcript reads).  The secp
-- field defaults are the Rust `Fp::default()` / `Fq::default()` = 0,
-- obtained here as the reduction of the all-zero byte string (keeping
-- zero out of the trust base); `K256::default()` is the identity.
default-val : IrType → IrValue
default-val native        = val-native 0ᶠ
default-val bytes32       = val-bytes32 (tabulate (λ _ → F.zero))
default-val jubjub-point  = val-jubjub-point idJ
default-val jubjub-scalar = val-jubjub-scalar (native→jubjubScalar 0ᶠ)
default-val secp-point    = val-secp-point idK
default-val secp-base     =
  val-secp-base (secpBaseFromBytes (tabulate (λ _ → F.zero)))
default-val secp-scalar   =
  val-secp-scalar (secpScalarFromBytes (tabulate (λ _ → F.zero)))

------------------------------------------------------------------------
-- The named value store.
------------------------------------------------------------------------

Mem : Set
Mem = Identifier → Maybe IrValue

∅ : Mem
∅ _ = nothing

ins : Identifier → IrValue → Mem → Mem
ins id v m i = case (i ≟str id) of λ { (yes _) → just v ; (no _) → m i }

------------------------------------------------------------------------
-- Operand resolution.
------------------------------------------------------------------------

resolve : Mem → Operand → Maybe IrValue
resolve m (var id) = m id
resolve m (imm x)  = just (val-native x)

-- Resolve, expecting a Native value (an immediate, or a Native variable).
resolveᶠ : Mem → Operand → Maybe Fr
resolveᶠ m op = resolve m op >>= λ { (val-native x) → just x ; _ → nothing }

resolve𝔹 : Mem → Operand → Maybe Bool
resolve𝔹 m op = resolveᶠ m op >>= to𝔹

eval-guard : Mem → Maybe Operand → Maybe Bool
eval-guard m nothing   = just true
eval-guard m (just op) = resolve𝔹 m op

resolve-all-Fr : Mem → List Operand → Maybe (List Fr)
resolve-all-Fr m []         = just []
resolve-all-Fr m (op ∷ ops) =
  resolveᶠ m op        >>= λ x  →
  resolve-all-Fr m ops >>= λ xs →
  just (x ∷ xs)

------------------------------------------------------------------------
-- Proof preimage and preprocess state.
------------------------------------------------------------------------

record ProofPreimage : Set where
  field
    inputs                 : List Fr
    binding-input          : Fr
    comm-commitment        : Maybe (Fr × Fr)
    pub-transcript-inputs  : List Fr
    pub-transcript-outputs : List Fr
    priv-transcript        : List Fr

record State : Set where
  field
    mem      : Mem
    pis      : List Fr
    pi-skips : List (Maybe ℕ)
    pti-idx  : ℕ            -- cursor into pub-transcript-inputs
    pto-rem  : List Fr      -- remaining pub-transcript-outputs
    priv-rem : List Fr      -- remaining private-transcript
    outs     : List IrValue

------------------------------------------------------------------------
-- Small state combinators.
------------------------------------------------------------------------

out1 : State → Identifier → IrValue → State
out1 st id v = record st { mem = ins id v (State.mem st) }

-- Bind a list of identifiers to a list of values; lengths must match.
insertMany : State → List Identifier → List IrValue → Maybe State
insertMany st []         []       = just st
insertMany st (id ∷ ids) (v ∷ vs) = insertMany (out1 st id v) ids vs
insertMany st _          _        = nothing

-- Equality of two values at a supported type (Native / Bytes32 /
-- JubjubPoint / the Secp256k1 triple), or `nothing` if the type
-- combination is unsupported.
valEq? : IrValue → IrValue → Maybe Bool
valEq? (val-native x)       (val-native y)       = just (isYes (x ≟ᶠ y))
valEq? (val-bytes32 a)      (val-bytes32 b)      = just (isYes (a ≟B b))
valEq? (val-jubjub-point p) (val-jubjub-point q) = just (isYes (p ≟J q))
valEq? (val-secp-point p)   (val-secp-point q)   = just (isYes (p ≟K q))
valEq? (val-secp-base x)    (val-secp-base y)    = just (isYes (x ≟ᵇ y))
valEq? (val-secp-scalar x)  (val-secp-scalar y)  = just (isYes (x ≟ˢ y))
valEq? _                    _                    = nothing

-- Resolve and type-check the `output` operands against a type signature.
collectOutputs : Mem → List IrType → List Operand → Maybe (List IrValue)
collectOutputs m []       []         = just []
collectOutputs m (t ∷ ts) (op ∷ ops) =
  resolve m op >>= λ v →
  guardD (typeof v ≟T t)
    (collectOutputs m ts ops >>= λ vs → just (v ∷ vs))
collectOutputs m _        _          = nothing

-- Decode the declared inputs from the raw input stream, consuming
-- `encoded-len` elements per input; the stream must be fully consumed.
decode-inputs : List TypedIdentifier → List Fr → Maybe Mem
decode-inputs []         []        = just ∅
decode-inputs []         (_ ∷ _)   = nothing
decode-inputs (ti ∷ tis) raw       =
  let t = TypedIdentifier.val-t ti in
  decode t (take (encoded-len t) raw)        >>= λ v →
  decode-inputs tis (drop (encoded-len t) raw) >>= λ m →
  just (ins (TypedIdentifier.name ti) v m)

------------------------------------------------------------------------
-- The per-instruction step  (ir_vm.rs: the `preprocess` match).
------------------------------------------------------------------------

step : ProofPreimage → IrSource → State → Instruction → Maybe State
step P S st i = go i
  where
  m : Mem
  m = State.mem st
  go : Instruction → Maybe State
  go (encode input outs) =
    resolve m input >>= λ v →
    insertMany st outs (map val-native (encodeᵉ v))

  go (assert cond) =
    resolve𝔹 m cond >>= λ b → if b then just st else nothing

  go (cond-select bit a b output) =
    resolve𝔹 m bit >>= λ bv →
    resolve m a    >>= λ av →
    resolve m b    >>= λ bvl →
    guardD (typeof av ≟T typeof bvl)
      (just (out1 st output (if bv then av else bvl)))

  go (constrain-bits val bits) =
    resolveᶠ m val >>= λ x →
    guardD (valFr x <? 2 ^ bits) (just st)

  go (constrain-eq a b) =
    resolve m a >>= λ av →
    resolve m b >>= λ bv →
    valEq? av bv >>= λ eq → if eq then just st else nothing

  go (constrain-to-boolean val) =
    resolve𝔹 m val >>= λ _ → just st

  go (copy val output) =
    resolve m val >>= λ v → just (out1 st output v)

  -- Public inputs are pushed guarded, exactly as the Rust preprocess
  -- does (ir_vm.rs, I::Impact): an active group (guard = true) pushes
  -- the resolved `vals` — also matched against the public transcript —
  -- and advances the cursor; a skipped group pushes zeros, mirroring the
  -- in-circuit `select(guard, x, 0)` (`Circuit.pi-impact`), and leaves
  -- the cursor unmoved.
  go (impact guard inputs) =
    resolve-all-Fr m inputs >>= λ vals →
    resolve𝔹 m guard        >>= λ g    →
    let n = length vals in
    if g
    then guardD (take n (drop (State.pti-idx st)
                           (ProofPreimage.pub-transcript-inputs P))
                  ≟LFr vals)
           (just (record st { pis      = State.pis st ++ vals
                            ; pi-skips = State.pi-skips st ++ (nothing ∷ [])
                            ; pti-idx  = State.pti-idx st + n }))
    else just (record st { pis      = State.pis st ++ map (λ _ → 0ᶠ) vals
                         ; pi-skips = State.pi-skips st ++ (just n ∷ []) })

  go (ec-mul a scalar output) =
    resolve m a      >>= λ pv →
    resolve m scalar >>= λ sv →
    case (pv , sv) of λ
      { (val-jubjub-point p , val-jubjub-scalar s) →
          just (out1 st output (val-jubjub-point (s ·J p)))
      ; (val-secp-point p , val-secp-scalar s) →
          just (out1 st output (val-secp-point (s ·K p)))
      ; _ → nothing }

  go (ec-mul-generator scalar output) =
    resolve m scalar >>= λ sv →
    case sv of λ
      { (val-jubjub-scalar s) →
          just (out1 st output (val-jubjub-point (s ·J genJ)))
      ; (val-secp-scalar s) →
          just (out1 st output (val-secp-point (s ·K genK)))
      ; _ → nothing }

  go (hash-to-curve inputs output) =
    resolve-all-Fr m inputs >>= λ frs →
    just (out1 st output (val-jubjub-point (hash-to-curve-fn frs)))

  go (into-coordinates point (xid , yid)) =
    resolve m point >>= λ pv →
    case pv of λ
      { (val-jubjub-point p) →
          let (x , y) = coordsJ p in
          just (out1 (out1 st xid (val-native x)) yid (val-native y))
      ; (val-secp-point p) →
          -- Errors on the Weierstrass identity (`coordsK` is `nothing`
          -- exactly there).
          coordsK p >>= λ (x , y) →
          just (out1 (out1 st xid (val-secp-base x)) yid (val-secp-base y))
      ; _ → nothing }

  go (from-coordinates (xop , yop) output) =
    resolve m xop >>= λ xv →
    resolve m yop >>= λ yv →
    case (xv , yv) of λ
      { (val-native x , val-native y) →
          fromCoordsJ x y >>= λ p →
          just (out1 st output (val-jubjub-point p))
      ; (val-secp-base x , val-secp-base y) →
          fromCoordsK x y >>= λ p →
          just (out1 st output (val-secp-point p))
      ; _ → nothing }

  go (into-bytes32 input output) =
    resolve m input >>= λ v →
    case v of λ
      { (val-native x)      → just (out1 st output (val-bytes32 (nativeToBytes x)))
      ; (val-secp-base x)   → just (out1 st output (val-bytes32 (secpBaseToBytes x)))
      ; (val-secp-scalar s) → just (out1 st output (val-bytes32 (secpScalarToBytes s)))
      ; _ → nothing }

  go (from-bytes32 bytes val-t output) =
    resolve m bytes >>= λ v →
    case v of λ
      { (val-bytes32 b) →
          -- On the foreign fields the conversion is total: non-canonical
          -- bytes reduce mod the field order
          -- (from_le_bytes_with_reduction).
          case val-t of λ
            { native      → just (out1 st output (val-native (nativeFromBytes b)))
            ; secp-base   → just (out1 st output (val-secp-base (secpBaseFromBytes b)))
            ; secp-scalar → just (out1 st output (val-secp-scalar (secpScalarFromBytes b)))
            ; _           → nothing }
      ; _ → nothing }

  go (reverse-bytes bytes output) =
    resolve m bytes >>= λ v →
    case v of λ
      { (val-bytes32 b) → just (out1 st output (val-bytes32 (reverse b)))
      ; _ → nothing }

  go (bytes32-into-low-high bytes (loid , hiid)) =
    resolve m bytes >>= λ v →
    case v of λ
      { (val-bytes32 b) →
          let (lo , hi) = bytes32→low-high b in
          just (out1 (out1 st loid (val-native lo)) hiid (val-native hi))
      ; _ → nothing }

  go (bytes32-from-low-high (loop , hiop) output) =
    resolveᶠ m loop >>= λ lo →
    resolveᶠ m hiop >>= λ hi →
    low-high→bytes32 lo hi >>= λ b →
    just (out1 st output (val-bytes32 b))

  go (div-mod-power-of-two val bits outs) =
    resolveᶠ m val >>= λ x →
    let L = to-le-bits x in
    insertMany st outs
      ( val-native (from-le-bits (drop bits L))
      ∷ val-native (from-le-bits (take bits L)) ∷ [])

  go (reconstitute-field divisor modulus bits output) =
    resolveᶠ m divisor >>= λ d  →
    resolveᶠ m modulus >>= λ mo →
    guardD (valFr mo <? 2 ^ bits)
      (guardD (valFr d <? 2 ^ (FR-BITS ∸ bits))
        (guardD (valFr mo + 2 ^ bits * valFr d <? FR-ORDER)
          (just (out1 st output (val-native ((pow2ᶠ bits *ᶠ d) +ᶠ mo))))))

  go (transient-hash inputs output) =
    resolve-all-Fr m inputs >>= λ frs →
    just (out1 st output (val-native (transient-hash-fn frs)))

  go (persistent-hash alignment inputs output) =
    resolve-all-Fr m inputs >>= λ frs →
    persistent-hash-fn alignment frs >>= λ h →
    just (out1 st output (val-bytes32 h))

  go (keccak256 alignment inputs output) =
    resolve-all-Fr m inputs >>= λ frs →
    keccak-fn alignment frs >>= λ h →
    just (out1 st output (val-bytes32 h))

  go (test-eq a b output) =
    resolve m a  >>= λ av →
    resolve m b  >>= λ bv →
    valEq? av bv >>= λ eq →
    just (out1 st output (val-native (χ eq)))

  go (add a b output) =
    resolve m a >>= λ av →
    resolve m b >>= λ bv →
    case (av , bv) of λ
      { (val-native x , val-native y) →
          just (out1 st output (val-native (x +ᶠ y)))
      ; (val-jubjub-point p , val-jubjub-point q) →
          just (out1 st output (val-jubjub-point (p +J q)))
      ; (val-secp-point p , val-secp-point q) →
          just (out1 st output (val-secp-point (p +K q)))
      ; (val-secp-base x , val-secp-base y) →
          just (out1 st output (val-secp-base (x +ᵇ y)))
      ; (val-secp-scalar x , val-secp-scalar y) →
          just (out1 st output (val-secp-scalar (x +ˢ y)))
      ; _ → nothing }

  go (mul a b output) =
    resolve m a >>= λ av →
    resolve m b >>= λ bv →
    case (av , bv) of λ
      { (val-native x , val-native y) →
          just (out1 st output (val-native (x *ᶠ y)))
      ; (val-secp-base x , val-secp-base y) →
          just (out1 st output (val-secp-base (x *ᵇ y)))
      ; (val-secp-scalar x , val-secp-scalar y) →
          just (out1 st output (val-secp-scalar (x *ˢ y)))
      ; _ → nothing }

  go (neg a output) =
    resolve m a >>= λ av →
    case av of λ
      { (val-native x)       → just (out1 st output (val-native (-ᶠ x)))
      ; (val-jubjub-point p) → just (out1 st output (val-jubjub-point (negJ p)))
      ; (val-secp-point p)   → just (out1 st output (val-secp-point (negK p)))
      ; (val-secp-base x)    → just (out1 st output (val-secp-base (-ᵇ x)))
      ; (val-secp-scalar x)  → just (out1 st output (val-secp-scalar (-ˢ x)))
      ; _ → nothing }

  go (inv a output) =
    resolve m a >>= λ av →
    case av of λ
      { (val-native x) →
          invᶠ x >>= λ xi → just (out1 st output (val-native xi))
      ; (val-secp-base x) →
          invᵇ x >>= λ xi → just (out1 st output (val-secp-base xi))
      ; (val-secp-scalar x) →
          invˢ x >>= λ xi → just (out1 st output (val-secp-scalar xi))
      ; _ → nothing }

  go (not a output) =
    resolve𝔹 m a >>= λ b →
    just (out1 st output (val-native (χ (bnot b))))

  go (less-than a b bits output) =
    resolveᶠ m a >>= λ x →
    resolveᶠ m b >>= λ y →
    guardD (valFr x <? 2 ^ bits)
      (guardD (valFr y <? 2 ^ bits)
        (just (out1 st output (val-native (χ (isYes (valFr x <? valFr y)))))))

  go (jubjub-scalar-from-native a output) =
    resolveᶠ m a >>= λ x →
    just (out1 st output (val-jubjub-scalar (native→jubjubScalar x)))

  go (public-input guard val-t output) =
    eval-guard m guard >>= λ g →
    if g
    then (let n = encoded-len val-t in
          decode val-t (take n (State.pto-rem st)) >>= λ v →
          just (record st { mem     = ins output v (State.mem st)
                          ; pto-rem = drop n (State.pto-rem st) }))
    else just (out1 st output (default-val val-t))

  go (private-input guard val-t output) =
    eval-guard m guard >>= λ g →
    if g
    then (let n = encoded-len val-t in
          decode val-t (take n (State.priv-rem st)) >>= λ v →
          just (record st { mem      = ins output v (State.mem st)
                          ; priv-rem = drop n (State.priv-rem st) }))
    else just (out1 st output (default-val val-t))

  go (circuit-output vals) =
    collectOutputs m (IrSource.outputs S) vals >>= λ vs →
    just (record st { outs = State.outs st ++ vs })

------------------------------------------------------------------------
-- Running a program and the acceptance conditions.
------------------------------------------------------------------------

run : ProofPreimage → IrSource → State → List Instruction → Maybe State
run P S st []       = just st
run P S st (i ∷ is) = step P S st i >>= λ st′ → run P S st′ is

-- Initialisation  (ir_vm.rs: input decoding + seeding the PI vector).
init : IrSource → ProofPreimage → Maybe State
init S P =
  decode-inputs (IrSource.inputs S) (ProofPreimage.inputs P) >>= λ m →
  let mk = λ pis → record { mem      = m
                          ; pis      = pis
                          ; pi-skips = []
                          ; pti-idx  = 0
                          ; pto-rem  = ProofPreimage.pub-transcript-outputs P
                          ; priv-rem = ProofPreimage.priv-transcript P
                          ; outs     = [] }
  in if IrSource.do-communications-commitment S
     then (case ProofPreimage.comm-commitment P of λ
            { (just (c , _)) →
                just (mk (ProofPreimage.binding-input P ∷ c ∷ []))
            ; nothing → nothing })
     else just (mk (ProofPreimage.binding-input P ∷ []))

-- The preprocess function: run, then check the terminal side conditions
-- TC1 (transcripts fully consumed) and TC2 (communications commitment).
preprocess : IrSource → ProofPreimage → Maybe State
preprocess S P =
  init S P >>= λ st0 →
  run P S st0 (IrSource.instructions S) >>= λ stf →
  guardD (State.pti-idx stf ≟ length (ProofPreimage.pub-transcript-inputs P))
   (case State.pto-rem stf of λ
     { (_ ∷ _) → nothing
     ; []      → case State.priv-rem stf of λ
       { (_ ∷ _) → nothing
       ; []      →
           if IrSource.do-communications-commitment S
           then (case ProofPreimage.comm-commitment P of λ
                  { (just (c , r)) →
                      guardD (c ≟ᶠ transient-commit
                                    (ProofPreimage.inputs P
                                      ++ concatMap encodeᵉ (State.outs stf)) r)
                        (just stf)
                  ; nothing → nothing })
           else just stf } })
