# Midnight Transaction Format

This document specifies the **structure** of a Midnight transaction and its
**binary (wire) serialization**. The *purpose and behaviour* of each
transaction variant is catalogued in
[transaction-types.md](./transaction-types.md); the *execution semantics*
(well-formedness, balancing, sequencing, application) are specified in
[intents-transactions.md](./intents-transactions.md) and summarised here only
where they bear on structure.  Contract-call transcripts are the
[Impact opcode reference](./impact-opcodes.md).

> [!NOTE]
> [intents-transactions.md](./intents-transactions.md) describes an
> *idealised* `Transaction<S, P, B>` in pseudo-Rust. This document reconciles
> that with the **actual** `ledger/src/structure.rs` definitions (which differ
> in field names, add a wrapping enum, and add `network_id`) and adds the
> wire format. The reconciliation notes are in
> [§8](#8-discrepancies-and-gaps-vs-the-narrative-spec).

## 1. The two serialization layers

Two distinct binary formats are involved, and they should not be conflated:

| Layer | What it serialises | Format | Spec |
|---|---|---|---|
| **Container** | the `Transaction` and all its sub-structures (intents, offers, actions) | the **`serialize` crate**: a `midnight:<tag>:` prefix + a **SCALE-style** binary body (compact varints for `u32`+), with `[vN]` **type-version tags** | §7.1 |
| **Field-Aligned Binary (FAB)** | the leaf `AlignedValue`s *inside* contract state, transcripts, and keys | `Value` / `Alignment` byte form, plus a **field-element** form used for proofs | [field-aligned-binary.md](./field-aligned-binary.md), summarised §7.2 |

In short: the transaction envelope is `serialize`-crate tagged binary; FAB
appears *within* it wherever ledger data values are embedded.

## 2. Top-level structure: `Transaction`

The top-level type is an **enum** (`ledger/src/structure.rs`, tag
`transaction[v12]`):

```rust
pub enum Transaction<S: SignatureKind, P: ProofKind, B: Storable, D: DB> {
    Standard(StandardTransaction<S, P, B, D>),
    ClaimRewards(ClaimRewardsTransaction<S, D>),
}
```

The two variants are catalogued in detail in
[transaction-types.md](./transaction-types.md); structurally:

* **`Standard`** — the general transaction: shielded (Zswap) offers + intents
  (unshielded offers, contract actions, dust). §2.1.
* **`ClaimRewards`** — a minimal special transaction claiming
  block / reward Night. §2.2.

The generic parameters `S`, `P`, `B` track the transaction's **lifecycle
stage** (§5); `D` is the storage backend (not part of the wire format).

### 2.1 `StandardTransaction`

`structure.rs`, tag `standard-transaction[v12]`:

| Field | Type | Meaning |
|---|---|---|
| `network_id` | `String` | network separation tag (e.g. mainnet vs a testnet); rejected on mismatch |
| `intents` | `HashMap<Segment, Intent<S,P,B,D>>` | the intents, keyed by `segment_id` (§4). Segment `0` is reserved/guaranteed |
| `guaranteed_coins` | `Option<ZswapOffer<P::LatestProof>>` | the **guaranteed** shielded (Zswap) offer |
| `fallible_coins` | `HashMap<Segment, ZswapOffer<P::LatestProof>>` | per-segment **fallible** shielded offers |
| `binding_randomness` | `PedersenRandomness` | randomness binding the Pedersen value commitments together (§5) |

`Segment = u16`; `GUARANTEED_SEGMENT = 0`.

### 2.2 `ClaimRewardsTransaction`

`structure.rs`, tag `claim-rewards-transaction[v2]`:

| Field | Type | Meaning |
|---|---|---|
| `network_id` | `String` | as above |
| `value` | `u128` | amount of Night being claimed |
| `owner` | `SignatureVerifyingKey` | recipient's verifying key |
| `nonce` | `Nonce` | uniqueness / replay nonce |
| `signature` | `S::Signature<ErasedClaimRewardsTransaction>` | authorising signature over `value ‖ owner ‖ nonce` |
| `kind` | `ClaimKind` | which reward pool is being claimed (tag `claim-kind[v1]`) |

The signed payload (`data_to_sign`) is the tagged serialization of the
`ClaimRewardsTransactionSigningEnvelope` — the transaction with its `signature`
field erased — so it carries the domain separator
`midnight:claim-rewards-transaction-signing-envelope[v2]:`.

## 3. Intents

An **intent** groups the parts of a transaction that apply atomically within
one segment. `structure.rs`, tag `intent[v9]`:

```rust
pub struct Intent<S: SignatureKind, P: ProofKind, B: Storable, D: DB> {
    pub guaranteed_unshielded_offer: Option<Sp<UnshieldedOffer<S, D>>>,
    pub fallible_unshielded_offer:   Option<Sp<UnshieldedOffer<S, D>>>,
    pub actions:        Array<ContractAction<P, D>>,
    pub dust_actions:   Option<Sp<DustActions<S, P, D>>>,
    pub ttl:            Timestamp,
    pub binding_commitment: B,
}
```

| Field | Meaning |
|---|---|
| `guaranteed_unshielded_offer` / `fallible_unshielded_offer` | unshielded (Night/UTXO) transfers, split by phase (§4) |
| `actions` | ordered list of contract actions (calls/deploys/maintenance), §3.1 |
| `dust_actions` | DUST fee spends and registrations (fee payment), §3.3 |
| `ttl` | `Timestamp` after which the intent is invalid; also bounds replay history (§6) |
| `binding_commitment` | the intent's Pedersen binding commitment with a PoK of exponent of `g`, preventing interference with Zswap value commitments (§5) |

**Validity floor:** an intent must contain at least one offer, call, or dust
payment.

**TTL window:** valid only if `tblock ≤ ttl ≤ tblock + global_ttl`.

### 3.1 `ContractAction`

`structure.rs`, tag `contract-action[v9]`:

```rust
pub enum ContractAction<P: ProofKind, D: DB> {
    Call(Sp<ContractCall<P, D>>),
    Deploy(Sp<ContractDeploy<D>>),
    Maintain(MaintenanceUpdate<D>),
}
```

* **`ContractCall`** (tag `contract-call[v3]`): `address: ContractAddress`,
  `entry_point: EntryPointBuf`, `guaranteed_transcript: Option<Transcript>`,
  `fallible_transcript: Option<Transcript>`, `communication_commitment: Fr`,
  `proof: P::Proof`. The two transcripts are the public Impact programs for
  the guaranteed/fallible phases (see the
  [Impact opcode reference](./impact-opcodes.md)); the proof is the call's
  ZK proof.
* **`ContractDeploy`** (tag `contract-deploy[v6]`): `initial_state: ContractState`,
  `nonce: HashOutput`. The contract address is `SHA-256(tagged_serialize(deploy))`.
* **`MaintenanceUpdate`** (tag `contract-maintenance-update[v3]`): authorised
  updates to a contract's verifier keys / operations.

### 3.2 `UnshieldedOffer`

`structure.rs`, tag `unshielded-offer[v2]`:

| Field | Type | Meaning |
|---|---|---|
| `inputs` | `Array<UtxoSpend>` | unshielded UTXOs being spent |
| `outputs` | `Array<UtxoOutput>` | unshielded UTXOs being created |
| `signatures` | `Array<S::Signature<IntentSigningEnvelope>>` | signatures over the `(segment_id, erased intent)` pair (§3.4) |

### 3.3 `DustActions`

`dust.rs`, on the intent as `dust_actions`. Carries the DUST `spends` (fee
payments) and `registrations`; a canonical ordering is imposed and only that
order is valid. Fees are denominated in DUST and accumulated across all
segments when applying segment `0`. (DUST mechanics are specified in
[dust.md](./dust.md).)

### 3.4 Intent identity and signing: `IntentSigningEnvelope` / `IntentHash`

Signatures and replay protection operate over the **proof-erased** intent
paired with its segment id. The signed message is the tagged
`IntentSigningEnvelope` (tag `intent-signing-envelope[v9]`), produced by
`erased_intent.data_to_sign(segment_id)`:

```rust
type ErasedIntent = Intent<(), (), Pedersen, D>;   // structure.rs
// what unshielded-offer signatures sign (tag "intent-signing-envelope[v9]"):
struct IntentSigningEnvelope { segment: u16, intent: InnerIntentSigningEnvelope /* erased */ }
struct IntentHash(HashOutput);                     // tag "intent-hash"
```

`IntentHash = persistent_hash(erased_intent.data_to_sign(segment_id))` — i.e.
**SHA-256** (`persistent_hash`) over the canonical serialisation of the
segment-tagged, proof-erased intent. This hash is the replay key (§6) and
the message that authenticates the intent across lifecycle stages.

## 4. Segments, and guaranteed vs fallible execution

Intents and fallible Zswap offers carry a `segment_id: u16`:

* **Segment `0`** is reserved for the **guaranteed** section (fees, guaranteed
  coins, guaranteed transcripts). Intents / fallible offers must use
  `segment_id ≠ 0`.
* A segment groups parts that **apply atomically together**.

**Execution order.** The guaranteed section runs first; the remainder runs in
ascending `segment_id` order. Using a higher segment id is a deliberate
anti-frontrunning lever (at the cost of fewer coincidental merges).

**Guaranteed / fallible split & causal precedence.** A contract call may
carry *both* a guaranteed and a fallible transcript. Because the guaranteed
part of a later segment runs before the fallible part of an earlier one,
merged transactions must satisfy a **causal-precedence** constraint: if
segments `a < b` both call contract `c`, then either `a` has no fallible
transcript for `c`, or `b` has no guaranteed transcript for `c` (extended
transitively, and to intra-intent contract-to-contract calls). The full
relation and its checks are in
[intents-transactions.md](./intents-transactions.md#sequencing). The
[`ckpt` opcode](./impact-opcodes.md#control-flow) marks the boundary between
a call's guaranteed and fallible transcript segments.

**Application outcome.** The guaranteed section is all-or-nothing; each
fallible segment applies in isolation and rolls back independently on
failure (`ledger/src/semantics.rs`):

```rust
pub enum TransactionResult<D: DB> {
    Success(Vec<Event<D>>),                         // everything applied
    PartialSuccess(                                 // guaranteed (segment 0) applied;
        BTreeMap<u16, Result<(), TransactionInvalid<D>>>, // per-segment Ok/Err
        Vec<Event<D>>,
    ),
    Failure(TransactionInvalid<D>),                 // guaranteed section failed
}
```

(In `PartialSuccess` the map records, per `segment_id`, whether that fallible
segment applied (`Ok`) or rolled back (`Err`); each variant also carries the
emitted `Event`s.)

## 5. Binding: lifecycle type parameters and Pedersen commitments

The generic parameters encode the transaction's **lifecycle stage**
(`structure.rs`, `ProofKind` / `SignatureKind`;
[intents-transactions.md §Lifecycle](./intents-transactions.md#lifecycle-of-intents-and-transactions)):

| Param | Instantiations | Role |
|---|---|---|
| `S` (signature) | `Signature` / `()` | authenticating signatures, or erased |
| `P` (proof) | `ProofPreimageMarker` (`LatestProof = ProofPreimage`) → `ProofMarker` (`LatestProof = Proof`) / `()` | pre-proof construction → proven → erased |
| `B` (binding) | `PedersenRandomness` → `Pedersen` (Fiat-Shamir) | intent binding commitment, openable → finalised |

The canonical **stages** are: *construction*
`<Signature, PreProof, PedersenRandomness>` → *balancing*
`<Signature, Proof, PedersenRandomness>` → *signing / submission*
`<Signature, Proof, FiatShamirPedersen>`. The proof-erased view
`<(), (), Pedersen>` gives a stage-independent identity used for hashing.

**Pedersen binding** ties the intents and the Zswap offers into one balanced
unit: the sum of all intent binding commitments and offer value-commitments
must equal the declared per-token balance committed under
`binding_randomness` (the `pedersen_check` in
[intents-transactions.md](./intents-transactions.md)).  This is what prevents
mixing-and-matching parts across transactions.

**`TransactionIdentifier`** (`structure.rs`, tag `transcation-id[v1]` —
*sic*, see §8):

```rust
pub enum TransactionIdentifier { Merged(Pedersen), Unique(HashOutput) }
```

`Merged` identifies transactions that may have been merged (by their Pedersen
point); `Unique` is a content hash for un-merged transactions.

## 6. Replay protection and TTL (structural summary)

Each applied `IntentHash` is recorded in a time-bounded history
(`ReplayProtectionState.intent_history`, a `TimeFilterMap` keyed by `ttl`).
Re-submitting an intent whose hash is already present is rejected; entries
age out past their `ttl`. Zswap offers carry their own replay protection
(nullifiers) and are not added here. Full rules:
[intents-transactions.md §Replay Protection](./intents-transactions.md#replay-protection).

## 7. Binary serialization

### 7.1 Container format (the `serialize` crate)

A top-level value is written with `tagged_serialize`
(`serialize/src/serializable.rs`):

```text
midnight:<tag>:<Serializable body>
```

* `GLOBAL_TAG = "midnight:"` is the fixed prefix; `<tag>` is the type's
  `Tagged::tag()`, e.g. `transaction[v12]`. Deserialization checks both, so
  data tagged as one type cannot be silently read as another.
* **`<tag>` carries the version.** Square-bracketed `[vN]` suffixes version
  a type independently of its name (`tagged.rs` conventions: kebab-case,
  `[vN]` for versions, `(a,b)` for generic args). Bumping a struct's layout
  bumps its `[vN]`; a derive-time `tag_unique_factor` test
  (`.tag-decompositions/`) ensures a layout change *must* change the tag,
  preventing silent format drift.
* The **body** is the `Serializable` encoding, which is **SCALE-style**
  (`serialize/src/util.rs`): `u8`/`u16` (and the signed integers) are
  fixed-width little-endian, but `u32`/`u64`/`u128` are **SCALE compact
  varints** (1/2/4/n-byte, `ScaleBigInt`) — *not* fixed-width. `Vec` / `Array`
  and `HashMap` are length-prefixed by a compact `u32` count (maps in
  sorted-key order); `Option` is a presence byte + payload; enums are a
  discriminant + variant body; `Sp<T>` serialises as its pointed-to `T`. Note
  that `tagged_serialize` itself writes **no length field** after the
  `midnight:<tag>:` prefix — the body follows immediately.

The current top-level tags for the transaction format are indexed in
[Appendix A](#appendix-a--type--version-tag-index).

### 7.2 Field-Aligned Binary (for embedded values)

Leaf data values — contract-state cells, transcript values, map keys — are
**FAB** `AlignedValue`s (full spec:
[field-aligned-binary.md](./field-aligned-binary.md)). Summary:

* A **`Value`** is a list of **`ValueAtom`**s (each a `Uint8Array`); an
  **`Alignment`** is a list of **`AlignmentSegment`**s — either an atom
  (`compress`, `field`, or `bytes<n>`) or an "option" (sum-type) of
  alignments. An **`AlignedValue`** pairs each `ValueAtom` with its
  `AlignmentAtom`.
* **Binary encoding** uses *integers with flags* `xy`: a variable-length
  integer in 1–3 bytes (`xy0aaaaa` / `… 0bbbbbbb` / `… 1ccccccc`), capping
  lengths at `< 2^19`. The two flag bits select between
  `Value` / `ValueAtom` / `Alignment` / `AlignmentSegment` interpretations.
* **Field-element representation** (used inside proofs): `field` → the atom
  as a little-endian bigint mod the field order (1 element); `compress` →
  a hash of the atom (1 element); `bytes<n>` → `ceil(n/31)` elements,
  packing 31 bytes per field element, filled from the end. An `AlignedValue`
  prefixes its atom count and encodes each alignment atom as `<n>` (bytes),
  `-1` (compress), or `-2` (field).

The FAB field modulus is the base elliptic curve's scalar field (the native
field).

## 8. Discrepancies and gaps vs. the narrative spec

Found while reconciling
[intents-transactions.md](./intents-transactions.md) against `structure.rs`:

1. **`Transaction` is an enum, not a struct.** The narrative spec's
   `Transaction<S,P,B>` is the implementation's `StandardTransaction`; the
   real `Transaction` adds a `ClaimRewards` variant.
2. **Field names differ.** Narrative `guaranteed_offer` / `fallible_offer` →
   `guaranteed_coins` / `fallible_coins`; narrative
   `binding_randomness: Fr` → `PedersenRandomness`.
3. **`network_id` is undocumented in the narrative spec** but is a real,
   serialised field of both transaction variants (network separation).
4. **`TransactionIdentifier`'s tag is misspelled** `transcation-id[v1]` in
   `structure.rs`. Changing it is a wire-format break, so it must stay — but
   it should be documented as a known typo.
5. The narrative pseudo-code uses `Vec` / `Map`; the implementation uses
   storage-backed `Array` / `HashMap` (`Sp<…>` pointers), which serialise
   identically (length-prefixed) but differ in in-memory representation.

## Appendix A — type / version tag index

Tags as serialised by the `serialize` crate (prefix `midnight:`), from
`structure.rs` unless noted. The `[vN]` suffix is the on-wire version.

| Type | Tag |
|---|---|
| `Transaction` | `transaction[v12]` |
| `StandardTransaction` | `standard-transaction[v12]` |
| `ClaimRewardsTransaction` | `claim-rewards-transaction[v2]` |
| `Intent` | `intent[v9]` |
| `IntentHash` | `intent-hash` |
| `UnshieldedOffer` | `unshielded-offer[v2]` |
| `ContractAction` | `contract-action[v9]` |
| `ContractCall` | `contract-call[v3]` |
| `ContractDeploy` | `contract-deploy[v6]` |
| `MaintenanceUpdate` | `contract-maintenance-update[v3]` |
| `TransactionIdentifier` | `transcation-id[v1]` *(sic)* |
| `SystemTransaction` | `system-transaction[v9]` |
| `ClaimKind` | `claim-kind[v1]` |
| `TransactionCostModel` | `transaction-cost-model[v5]` |
| `TransactionLimits` | `transaction-limits[v3]` |
| `LedgerParameters` | `ledger-parameters[v8]` |
| `ReplayProtectionState` | `replay-protection-state[v1]` |
| `SignatureVerifyingKey` | `signature-verifying-key[v2]` |
| `Signature` | `signature[v2]` |
| `SigningKey` | `signing-key[v2]` |

(`SystemTransaction` is applied by the chain itself, not submitted as a user
`Transaction`; it is catalogued in
[transaction-types.md](./transaction-types.md).)
