//! What applying a transaction *does*, separated from what it *is*.
//!
//! A consumer that has already verified a transaction — proofs, signatures, balance — needs
//! none of that again to apply it. It needs the state changes and the conditions under which
//! they are legal. This module is that: every field below is either a **precondition** the
//! applier checks against its state, or a **mutation** it performs.
//!
//! [`Offer::effects`] already does this for one rule family, and
//! [`State::try_apply_effects`] applies it; this generalises the idea to a whole transaction.
//!
//! # Why the types are plain
//!
//! Deliberately **not** `Storable`, not generic over `DB`, and containing no `Sp`. The
//! motivating consumer decodes 1,422 octets and pays 4,584,171 gas to do it, against 1,303 to
//! merely read the same octets — 99.97% of the cost is rebuilding an object graph, not reading
//! the data. A `Storable` effects type would reproduce exactly that. The leaves are already
//! flat ([`UtxoSpend`] and [`UtxoOutput`] are not even generic), so the transport is largely
//! `Array<T, D>` → `Vec<T>` with the same payload.
//!
//! # ⚠︎ What the applier is trusting
//!
//! Applying effects assumes the caller has already verified the transaction they came from.
//! That is the same contract [`State::try_apply_effects`] makes when it drops proofs, and it
//! is sound only where something attests the producing computation. It must not become the
//! default path for a consumer with no such stage.
//!
//! [`Offer::effects`]: zswap::structure::Offer::effects
//! [`State::try_apply_effects`]: zswap::ledger::State::try_apply_effects

use crate::dust::{DustCommitment, DustNullifier, DustPublicKey};
use crate::structure::{IntentHash, Utxo, UtxoSpend};
// ⌖ The ledger's own sorted iteration, not `HashMap::iter`. Order reaches the effects record
// and therefore the applied state, and in a consensus system a nondeterministic order is a
// fault rather than a flaky test.
use crate::utils::SortedIter;
use base_crypto::signatures::VerifyingKey;
use base_crypto::time::Timestamp;
use coin_structure::coin::{Commitment, Nullifier};
use coin_structure::contract::ContractAddress;
use serialize::{Deserializable, Serializable, Tagged};
use transient_crypto::merkle_tree::MerkleTreeDigest;

/// Everything applying one transaction does.
///
/// ⚠︎ **The shape is `apply_section`'s, not an intuitive one.** Applying is not "per segment,
/// each doing the same thing". Segment 0 is a distinct *guaranteed pass* — replay protection,
/// the transaction's single guaranteed offer, every intent's guaranteed unshielded offer, and
/// all dust, which the implementation notes is explicitly "not processed segment-by-segment".
/// Every later segment applies only its own fallible parts.
///
/// A per-segment `guaranteed` field, which this type had first, would apply the guaranteed
/// offer once per segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionEffects {
    /// The segment-0 pass. Applied once; its failure fails the whole transaction.
    pub guaranteed: GuaranteedEffects,
    /// One entry per segment above 0, in application order. Each may fail alone.
    pub fallible: Vec<FallibleSegment>,
}

/// The guaranteed pass — everything `apply_section` does when `segment == 0`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuaranteedEffects {
    /// The transaction's single guaranteed shielded offer.
    pub zswap: ZswapDelta,
    /// Per intent, keyed by its physical segment.
    pub intents: Vec<IntentEffects>,
}

/// One fallible segment's changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallibleSegment {
    /// Which segment. Never 0.
    pub segment: u16,
    /// This segment's fallible shielded offer, if it has one.
    pub zswap: ZswapDelta,
    /// This segment's fallible unshielded changes.
    ///
    /// ⚠︎ At most **one**. The guaranteed pass iterates every intent
    /// (`tx.intents.sorted_iter()`), but a fallible segment takes only the intent at that
    /// segment (`tx.intents.get(&segment)`). A `Vec` here would admit a shape the ledger
    /// cannot produce and an applier could not faithfully consume.
    pub unshielded: Option<UnshieldedDelta>,
    /// Whether this segment's contract actions make the effects path unusable.
    pub contracts: ContractsPresent,
}

/// One intent's guaranteed changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentEffects {
    /// Which segment this intent sits in.
    pub segment: u16,
    /// Precondition: unseen. Mutation: recorded until `ttl`.
    pub intent_hash: IntentHash,
    /// When this intent stops being replayable.
    pub ttl: Timestamp,
    /// Unshielded changes from the guaranteed offer.
    pub unshielded: UnshieldedDelta,
    /// Dust changes. Not segment-scoped — see the note on [`TransactionEffects`].
    pub dust: DustDelta,
    /// Whether this intent's contract actions make the effects path unusable.
    pub contracts: ContractsPresent,
}

/// Shielded changes, mirroring [`ZswapEffects`] field for field but as plain data.
///
/// ⚠︎ The merkle root is **per spend**, not per offer: each input was proven against
/// whichever root the prover held, and `apply_spend` checks each against `past_roots`
/// separately. A single root per delta would silently accept a spend proven against a root
/// the state never had.
///
/// [`ZswapEffects`]: zswap::structure::ZswapEffects
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZswapDelta {
    /// Precondition: each root `∈ past_roots`, each nullifier absent. Mutation: inserted.
    pub spends: Vec<SpendDelta>,
    /// Mutation: appended to the commitment tree, in this order.
    pub creates: Vec<CreateDelta>,
    /// Spend and create in one, for a coin created and consumed within the same offer.
    pub transients: Vec<TransientDelta>,
}

/// One shielded spend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendDelta {
    /// Precondition: `∈ past_roots`. The root *this* input was proven against.
    pub merkle_tree_root: MerkleTreeDigest,
    /// Precondition: absent from the nullifier set. Mutation: inserted.
    pub nullifier: Nullifier,
    /// The contract this spend belongs to, if any.
    pub contract_address: Option<ContractAddress>,
}

/// One shielded create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateDelta {
    /// Mutation: appended to the commitment tree.
    pub coin_com: Commitment,
    /// The contract this create belongs to, if any.
    pub contract_address: Option<ContractAddress>,
}

/// A coin created and spent within the same offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransientDelta {
    /// Precondition: absent from the nullifier set. Mutation: inserted.
    pub nullifier: Nullifier,
    /// Mutation: appended to the commitment tree.
    pub coin_com: Commitment,
    /// The contract this belongs to, if any.
    pub contract_address: Option<ContractAddress>,
}

/// Unshielded changes: which utxos are consumed and which are created.
///
/// ⚠︎ **`creates` carries whole [`Utxo`]s, not [`UtxoOutput`]s, on purpose.** `apply_offer`
/// derives a created utxo's identity from `parent.intent_hash(segment_id)` plus its output
/// index, and that hash is *not* the one replay protection uses — replay deliberately takes
/// `intent_hash(0)`, segment-independent, so a replay cannot be moved to another segment.
/// Two different hashes off the same intent. Carrying the derived utxo means the applier
/// never has to pick, and the equivalence test compares identities directly rather than
/// re-deriving them the same wrong way on both sides.
///
/// [`Utxo`]: crate::structure::Utxo
/// [`UtxoOutput`]: crate::structure::UtxoOutput
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnshieldedDelta {
    /// Precondition: `Utxo::from(spend)` is present in `utxo`. Mutation: removed.
    pub spends: Vec<UtxoSpend>,
    /// Mutation: inserted, with `ctime` from the block context.
    pub creates: Vec<Utxo>,
}

/// Dust changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DustDelta {
    /// The time the dust actions were constructed against.
    pub ctime: Timestamp,
    /// Precondition: nullifier absent. Mutation: nullifier inserted, commitment appended.
    pub spends: Vec<DustSpendEffect>,
    /// Mutation: the registration is recorded.
    pub registrations: Vec<DustRegistrationEffect>,
}

/// One dust spend, with its proof dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DustSpendEffect {
    /// The fee this spend pays, deducted from the running fee allowance.
    pub v_fee: u128,
    /// Precondition: absent from `utxo.nullifiers`. Mutation: inserted.
    pub old_nullifier: DustNullifier,
    /// Mutation: appended at `commitments_first_free`.
    pub new_commitment: DustCommitment,
}

/// One dust registration, with only its signature dropped.
///
/// ⚠︎ **Inputs, not an outcome.** `apply_registration` threads a running `fees_remaining`
/// through the registrations in order, and the implementation applies all spends first "to
/// make sure registration outputs get the maximum dust they can". A registration's result
/// therefore depends on everything before it, so this carries what the ledger's own rule needs
/// and lets it run, rather than a precomputed answer that would have to be trusted and could
/// not be checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DustRegistrationEffect {
    /// The night key being registered; `UserAddress::from` of this is the account touched.
    pub night_key: VerifyingKey,
    /// The dust key, absent for a deregistration.
    pub dust_address: Option<DustPublicKey>,
    /// The ceiling this registration allows to be spent on fees.
    pub allow_fee_payment: u128,
}

/// Whether a transaction's contract actions can be applied from these effects.
///
/// ⌖ **Do not carry contract state — carry the transcript.** `apply_actions` computes the new
/// state by *running* the transcript against the current one
/// (`res.update_index(addr, results.context.state, ..)`), so an applier needs the ops, not the
/// result. An earlier version of this type carried a `new_state: Vec<u8>` and concluded
/// contracts were unfit for a flat transport; that was wrong, and wrong because it assumed
/// the state had to cross.
///
/// Everything a `Call` needs flattens the same way the rest of this module does:
///
/// ```text
/// Transcript.gas        RunningCost, plain
/// Transcript.effects    HashSet/HashMap over flat leaves — Nullifier, TokenType, u128
/// Transcript.program    Array<Op, D> → Vec<Op>; 28 variants, but see the ⚠︎ below
/// Transcript.version    a small enum
/// ```
///
/// So the three variants split by *frequency*, not by whether they are representable:
///
/// - **`Call`** — the common case. Carry the transcript flat; the ledger runs it and derives
///   the state. No state blob.
/// - **`Deploy`** — carries an `initial_state`, which genuinely is a state blob, but it is
///   one-off per contract and a fresh contract's state is small.
/// - **`Maintain`** — rare, and a governance operation: it rotates the contract's
///   verification machinery for a ᴢᴋ upgrade rather than touching its data. The full path is
///   the right answer for it permanently, not a stopgap.
///
/// ⚠︎ **Corrected twice; this is the third reading and the first one that holds.**
///
/// 1. The original said the remaining work was "a tag-and-payload codec for ~28 mostly-nullary
///    variants, not a design question". Wrong: two of the variants are not small
///    (`Push { value: StateValue<D> }` carries the whole recursive arena value, `Popeq` an
///    `AlignedValue`), and it *is* a design question.
/// 2. The replacement said contracts "must run the ᴠᴍ, so a flat codec replaces the
///    transcript's encoding and never its work". Also wrong, and more usefully so.
///
/// Read `semantics.rs:1328..1378` as four steps:
///
/// ```text
/// ①  results = qcontext.run_transcript(transcript, …)          run the program
/// ②  if results.context.effects != transcript.effects → Err    declared vs actual
/// ③  new_balance from transcript.effects.unshielded_inputs     already in the effects
/// ④  res.update_index(addr, results.context.state, balance)    install
/// ```
///
/// ④ is a **plain write** (`structure.rs:3196`) — take the address, the state, the balance,
/// put them in the map. The reason ① cannot be skipped is only that ④ needs
/// `results.context.state`, and that is *not* in `transcript.effects`, which carries the
/// transcript's **declarations** (`claimed_nullifiers`, `unshielded_inputs`, …) for ② to
/// check against.
///
/// ◈ **But a transport is not limited to the transaction's declared effects.** Refine already
/// runs the transcript — it must, to verify — so refine *holds* `results.context.state` when
/// it finishes. Shipping that makes the accumulate side ④ alone: a write, not a run, which is
/// exactly the shape [`UnshieldedDelta`] has and the shape contracts were said not to have.
///
/// So the question is size, not possibility. A whole post-state per call is proportional to
/// the contract's *state*; but the arena is content-addressed, so only the changed nodes
/// differ from what the chain already holds, and the delta is proportional to its *churn*.
/// That is the witness mechanism run in reverse.
///
/// ⚠︎ Skipping ① also skips the `EffectsMismatch` check. Sound only under this module's
/// standing premise — refine verified, accumulate trusts it — but worth stating outright,
/// because here it is the difference between the chain *re-deriving* a contract's state and
/// the chain being *told* it.
///
/// ⏸︎ Still `UseFullPath` today: the post-state channel is designed, not built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractsPresent {
    /// No contract actions; the effects above are the whole transaction.
    None,
    /// Contract actions are present and not yet represented here. These effects are
    /// incomplete — apply the transaction through `LedgerState::apply` instead.
    UseFullPath,
}

impl Flat for ContractsPresent {
    fn put(&self, out: &mut Vec<u8>) {
        out.push(match self {
            ContractsPresent::None => 0,
            ContractsPresent::UseFullPath => 1,
        });
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        match take(inp, 1)?[0] {
            0 => Some(ContractsPresent::None),
            1 => Some(ContractsPresent::UseFullPath),
            _ => None,
        }
    }
}

/// A transcript's declared effects, as plain data.
///
/// Mirrors [`onchain_runtime::context::Effects`] field for field. Every leaf is already flat —
/// `Nullifier`, `CoinCommitment`, `TokenType`, `HashOutput`, and two tuple structs of the same
/// — so only the containers change.
///
/// ⚠︎ **The originals are sets and maps; these are sequences, and that makes ordering part of
/// the encoding.** Two producers projecting the same effects must emit the same octets, so
/// every sequence is **sorted** on the way out. Iterating a `HashMap` and taking whatever order
/// falls out would give a record whose bytes depend on hash iteration order — which in a
/// consensus system is a fault, not a flaky test. Equality after reconstruction is unaffected
/// either way, since the originals are sets; determinism of the *bytes* is the reason.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TranscriptEffects {
    /// Nullifiers the transcript claims to spend.
    pub claimed_nullifiers: Vec<Nullifier>,
    /// Shielded commitments it claims to receive.
    pub claimed_shielded_receives: Vec<Commitment>,
    /// Shielded commitments it claims to spend.
    pub claimed_shielded_spends: Vec<Commitment>,
    /// Contract calls it claims to have made.
    pub claimed_contract_calls: Vec<onchain_runtime::context::ClaimedContractCallsValue>,
    /// Shielded mints, by token hash.
    pub shielded_mints: Vec<(base_crypto::hash::HashOutput, u64)>,
    /// Unshielded mints, by token hash.
    pub unshielded_mints: Vec<(base_crypto::hash::HashOutput, u64)>,
    /// Unshielded value taken in, by token type.
    pub unshielded_inputs: Vec<(coin_structure::coin::TokenType, u128)>,
    /// Unshielded value paid out, by token type.
    pub unshielded_outputs: Vec<(coin_structure::coin::TokenType, u128)>,
    /// Unshielded spends it claims, by (token, address).
    pub claimed_unshielded_spends:
        Vec<(onchain_runtime::context::ClaimedUnshieldedSpendsKey, u128)>,
}

// ── producers ───────────────────────────────────────────────────────────────────────────

impl ZswapDelta {
    /// Project a shielded offer onto what applying it needs.
    ///
    /// ⌖ Deliberately routed through [`Offer::effects`] rather than reading the offer's own
    /// fields. That method is the ledger's answer to "what does applying an offer need", and
    /// duplicating the projection here is how the two would drift — the first draft of this
    /// type dropped `contract_address` and collapsed the per-spend merkle root to one per
    /// offer, both of which mirroring would have prevented.
    pub fn from_offer<P, D, C>(offer: &zswap::Offer<P, D, C>) -> Self
    where
        D: storage::db::DB,
        P: storage::storable::Storable<D>,
        C: storage::storable::Storable<D>
            + Tagged
            + transient_crypto::commitment::CommitmentRepr<D>,
    {
        let e = offer.effects();
        ZswapDelta {
            spends: e
                .spends
                .iter_deref()
                .map(|s| SpendDelta {
                    merkle_tree_root: s.merkle_tree_root,
                    nullifier: s.nullifier,
                    contract_address: s.contract_address.as_ref().map(|a| *a.clone()),
                })
                .collect(),
            creates: e
                .creates
                .iter_deref()
                .map(|c| CreateDelta {
                    coin_com: c.coin_com,
                    contract_address: c.contract_address.as_ref().map(|a| *a.clone()),
                })
                .collect(),
            transients: e
                .transients
                .iter_deref()
                .map(|t| TransientDelta {
                    nullifier: t.nullifier,
                    coin_com: t.coin_com,
                    contract_address: t.contract_address.as_ref().map(|a| *a.clone()),
                })
                .collect(),
        }
    }
}

impl UnshieldedDelta {
    /// Project an unshielded offer onto what applying it needs.
    ///
    /// ⚠︎ `intent_hash` is a parameter rather than derived here, and that is deliberate. The
    /// creating hash is `parent.intent_hash(segment_id)`, while replay protection uses
    /// `intent_hash(0)` — segment-independent, so a replay cannot be relocated to another
    /// segment. Deriving it inside this function would put the choice somewhere the caller
    /// cannot see it, and picking the wrong one yields utxos under identities nothing else in
    /// the system agrees with.
    ///
    /// The `output_no` is the offer's own enumeration order, exactly as `apply_offer` assigns
    /// it. Signatures are dropped: whoever produced these effects has already checked them.
    pub fn from_offer<S, D>(
        offer: &crate::structure::UnshieldedOffer<S, D>,
        intent_hash: IntentHash,
    ) -> Self
    where
        D: storage::db::DB,
        S: crate::structure::SignatureKind<D>,
    {
        UnshieldedDelta {
            spends: offer.inputs.iter_deref().cloned().collect(),
            creates: offer
                .outputs
                .iter_deref()
                .enumerate()
                .map(|(output_no, o)| Utxo {
                    value: o.value,
                    owner: o.owner,
                    type_: o.type_,
                    intent_hash,
                    // Cast safe, as `apply_offer` assumes fewer than 4B outputs.
                    output_no: output_no as u32,
                })
                .collect(),
        }
    }
}

impl DustDelta {
    /// Project an intent's dust actions onto what applying them needs.
    ///
    /// Proofs and signatures are dropped; whoever produced these has already checked them.
    ///
    /// ⚠︎ **Order is part of the meaning.** The applier must run every spend before any
    /// registration — `apply_section` does so explicitly, "to make sure registration outputs
    /// get the maximum dust they can" — and must keep each sequence in the order given, since
    /// `apply_registration` threads a running fee allowance through them. Two `DustDelta`s
    /// with the same members in a different order are not the same effects.
    pub fn from_actions<S, P, D>(a: &crate::dust::DustActions<S, P, D>) -> Self
    where
        D: storage::db::DB,
        S: crate::structure::SignatureKind<D>,
        P: crate::structure::ProofKind<D>,
    {
        DustDelta {
            ctime: a.ctime,
            spends: a
                .spends
                .iter_deref()
                .map(|s| DustSpendEffect {
                    v_fee: s.v_fee,
                    old_nullifier: s.old_nullifier,
                    new_commitment: s.new_commitment,
                })
                .collect(),
            registrations: a
                .registrations
                .iter_deref()
                .map(|r| DustRegistrationEffect {
                    night_key: r.night_key.clone(),
                    dust_address: r.dust_address.as_ref().map(|d| *d.clone()),
                    allow_fee_payment: r.allow_fee_payment,
                })
                .collect(),
        }
    }
}

impl IntentEffects {
    /// Project one intent's *guaranteed* changes.
    ///
    /// ⚠︎ Two different hashes come off an intent and they are not interchangeable:
    /// `intent_hash(segment)` identifies the utxos this intent creates, while replay
    /// protection uses `intent_hash(0)` — segment-independent, so a replay cannot be
    /// relocated to another segment. Both are computed here, from the **erased** intent,
    /// because that is what `apply_section` hashes; hashing the unerased form gives a
    /// different value and utxos nothing else in the system agrees with.
    ///
    /// Contract actions are not projected — see [`ContractsPresent`].
    pub fn from_intent<B, D>(intent: &crate::structure::Intent<(), (), B, D>, segment: u16) -> Self
    where
        D: storage::db::DB,
        B: storage::storable::Storable<D> + serialize::Serializable,
    {
        // ⌖ `intent_hash(0)`, not `intent_hash(segment)` — and for *both* hashes here.
        //
        // Replay is segment-independent so it cannot be relocated. The creating hash is
        // `intent_hash(pass)`, and this projection is the **guaranteed pass**, whose pass is
        // 0 whatever segment the intent physically sits in: `semantics.rs:990` applies every
        // intent's guaranteed offer with `apply_offer(offer, &erased, segment, ..)` where
        // `segment == 0`, and marks the physical segment `#[allow(unused_variables)]`.
        //
        // ⚠︎ So in the guaranteed pass the two hashes coincide, and they diverge only in a
        // fallible segment (see `effects()`, which passes `i.intent_hash(segment)` there).
        // A test built only from segment-0 offers therefore cannot tell the two functions
        // apart — which is exactly how this shipped wrong: every family-level gate passed
        // while the whole-transaction comparison put the created utxo under a hash nothing
        // else in the system agrees with.
        let creating_hash = intent.intent_hash(0);
        IntentEffects {
            segment,
            intent_hash: intent.intent_hash(0),
            ttl: intent.ttl,
            unshielded: intent
                .guaranteed_unshielded_offer
                .as_ref()
                .map(|o| UnshieldedDelta::from_offer(o, creating_hash))
                .unwrap_or_default(),
            dust: intent
                .dust_actions
                .as_ref()
                .map(|d| DustDelta::from_actions(d))
                .unwrap_or_default(),
            contracts: if intent.actions.is_empty() {
                ContractsPresent::None
            } else {
                ContractsPresent::UseFullPath
            },
        }
    }
}

impl<B, D, C> crate::structure::StandardTransaction<(), (), B, D, C>
where
    D: storage::db::DB,
    B: storage::storable::Storable<D> + serialize::Serializable,
    C: storage::storable::Storable<D> + Tagged + transient_crypto::commitment::CommitmentRepr<D>,
{
    /// Project a whole transaction onto what applying it needs.
    ///
    /// The shape follows `apply_section` exactly, and the two asymmetries in it are the ones
    /// worth stating, because neither is guessable:
    ///
    /// - Segment 0 is a *guaranteed pass*, not a segment like the others. It applies the
    ///   transaction's single `guaranteed_coins` offer and **every** intent's guaranteed
    ///   unshielded offer, whatever segment those intents sit in.
    /// - A fallible segment applies its own `fallible_coins` and **at most one** intent —
    ///   `tx.intents.get(&segment)`, not an iteration.
    ///
    /// ⚠︎ There is no transaction-level replay claim. `ReplayProtectionState::apply_tx` folds
    /// over the intents calling `apply_intent`, which is
    /// `apply_member(intent.intent_hash(0), intent.ttl, ..)` — so replay is recorded **per
    /// intent**, and `IntentEffects` already carries both halves. An earlier version of this
    /// type had a `ReplayEffect { tx_hash, ttl }`, which was a concept the ledger does not
    /// have.
    pub fn effects(&self) -> TransactionEffects {
        let guaranteed = GuaranteedEffects {
            zswap: self
                .guaranteed_coins
                .as_ref()
                .map(|o| ZswapDelta::from_offer(o))
                .unwrap_or_default(),
            intents: self
                .intents
                .sorted_iter()
                .map(|(seg, intent)| IntentEffects::from_intent(&intent, *seg))
                .collect(),
        };
        let fallible = self
            .segments()
            .into_iter()
            .filter(|s| *s != 0)
            .map(|segment| {
                let intent = self.intents.get(&segment);
                FallibleSegment {
                    segment,
                    zswap: self
                        .fallible_coins
                        .get(&segment)
                        .map(|o| ZswapDelta::from_offer(&o))
                        .unwrap_or_default(),
                    unshielded: intent.as_ref().and_then(|i| {
                        i.fallible_unshielded_offer
                            .as_ref()
                            .map(|o| UnshieldedDelta::from_offer(o, i.intent_hash(segment)))
                    }),
                    contracts: match intent.as_ref() {
                        Some(i) if !i.actions.is_empty() => ContractsPresent::UseFullPath,
                        _ => ContractsPresent::None,
                    },
                }
            })
            .collect();
        TransactionEffects {
            guaranteed,
            fallible,
        }
    }
}

impl<D, B, C> crate::structure::VerifiedTransaction<D, B, C>
where
    D: storage::db::DB,
    B: storage::storable::Storable<D> + serialize::Serializable,
    C: storage::storable::Storable<D> + Tagged + transient_crypto::commitment::CommitmentRepr<D>,
{
    /// What applying this transaction does.
    ///
    /// ⌖ Deliberately on `VerifiedTransaction` rather than on `Transaction`. Applying effects
    /// assumes the transaction they came from has been checked — proofs, signatures, balance
    /// — and this type is the ledger's evidence of exactly that. Putting the projection here
    /// makes the trust contract a type rather than a doc comment: you cannot produce effects
    /// from something you have not verified.
    ///
    /// `None` for a transaction with no effects to project — a rewards claim, say — so a
    /// caller cannot mistake "nothing to do" for "an empty transaction applied".
    pub fn effects(&self) -> Option<TransactionEffects> {
        match &self.inner {
            crate::structure::Transaction::Standard(stx) => Some(stx.effects()),
            _ => None,
        }
    }
}

// ── appliers ────────────────────────────────────────────────────────────────────────────

impl<D: storage::db::DB> crate::structure::LedgerState<D> {
    /// Apply a [`TransactionEffects`] without the transaction it came from.
    ///
    /// The order is `apply_section`'s: the guaranteed pass first — replay, the single
    /// guaranteed shielded offer, then every intent's unshielded changes and dust spends —
    /// followed by each fallible segment.
    ///
    /// ⚠︎ **This refuses anything it cannot apply completely.** An intent whose contract
    /// actions or dust registrations are not represented in the effects returns an error
    /// rather than applying the part it understands. Silently applying a prefix of a
    /// transaction is the worst failure available here: the state would advance, look valid,
    /// and disagree with every node that took the ordinary path.
    ///
    /// ⚠︎ And it assumes the caller has already verified the transaction — proofs,
    /// signatures, balance. That is the same contract `try_apply_effects` makes, and it holds
    /// only where something attests the producing computation.
    pub fn apply_effects(
        &self,
        fx: &TransactionEffects,
        context: &crate::semantics::TransactionContext<D>,
    ) -> Result<Self, crate::error::TransactionInvalid<D>> {
        let tblock = context.block_context.tblock;
        let global_ttl = self.parameters.global_ttl;
        let mut state = self.clone();

        // ── the guaranteed pass ─────────────────────────────────────────────────────────
        for intent in &fx.guaranteed.intents {
            Self::refuse_if_incomplete(intent)?;
        }
        for intent in &fx.guaranteed.intents {
            state.replay_protection = storage::arena::Sp::new(
                state
                    .replay_protection
                    .apply_member(intent.intent_hash, intent.ttl, tblock, global_ttl)
                    .map_err(crate::error::TransactionInvalid::ReplayProtectionViolation)?,
            );
        }
        state = state.apply_zswap_delta(&fx.guaranteed.zswap)?;
        for intent in &fx.guaranteed.intents {
            state.utxo = storage::arena::Sp::new(
                state
                    .utxo
                    .apply_unshielded_delta(&intent.unshielded, tblock)?,
            );
            state = state.apply_dust_spends(&intent.dust, tblock, context)?;
        }

        // ── each fallible segment ───────────────────────────────────────────────────────
        for seg in &fx.fallible {
            if seg.contracts != ContractsPresent::None {
                return Err(crate::error::TransactionInvalid::EffectsIncomplete);
            }
            state = state.apply_zswap_delta(&seg.zswap)?;
            if let Some(u) = &seg.unshielded {
                state.utxo = storage::arena::Sp::new(state.utxo.apply_unshielded_delta(u, tblock)?);
            }
        }
        Ok(state)
    }

    /// Refuse an intent carrying anything the effects do not represent.
    fn refuse_if_incomplete(
        intent: &IntentEffects,
    ) -> Result<(), crate::error::TransactionInvalid<D>> {
        if intent.contracts != ContractsPresent::None {
            return Err(crate::error::TransactionInvalid::EffectsIncompleteContracts);
        }
        if !intent.dust.registrations.is_empty() {
            // Registrations need the parent intent's ɴɪɢʜᴛ inputs and outputs threaded
            // through a running fee allowance; see the note on `DustRegistrationEffect`.
            return Err(crate::error::TransactionInvalid::EffectsIncomplete);
        }
        Ok(())
    }

    fn apply_zswap_delta(
        mut self,
        d: &ZswapDelta,
    ) -> Result<Self, crate::error::TransactionInvalid<D>> {
        let spends: Vec<_> = d
            .spends
            .iter()
            .map(|s| (s.merkle_tree_root, s.nullifier, s.contract_address))
            .collect();
        let creates: Vec<_> = d
            .creates
            .iter()
            .map(|c| (c.coin_com, c.contract_address))
            .collect();
        let transients: Vec<_> = d
            .transients
            .iter()
            .map(|t| (t.nullifier, t.coin_com, t.contract_address))
            .collect();
        let (zs, _idx) = self
            .zswap
            .try_apply_flat(&spends, &creates, &transients, None)?;
        self.zswap = storage::arena::Sp::new(zs);
        Ok(self)
    }

    fn apply_dust_spends(
        mut self,
        d: &DustDelta,
        _tblock: Timestamp,
        context: &crate::semantics::TransactionContext<D>,
    ) -> Result<Self, crate::error::TransactionInvalid<D>> {
        for sp in &d.spends {
            self.dust = storage::arena::Sp::new(self.dust.apply_spend_flat(
                sp.old_nullifier,
                sp.new_commitment,
                sp.v_fee,
                d.ctime,
                context,
                &self.parameters.dust,
                |_| {},
            )?);
        }
        Ok(self)
    }
}

impl<D: storage::db::DB> crate::structure::UtxoState<D> {
    /// Apply an [`UnshieldedDelta`]: check every spend is present, remove it, then insert
    /// every create.
    ///
    /// ⌖ This is `apply_offer` with the derivation already done. `apply_offer` computes each
    /// created utxo's identity from the intent hash and the output index; the delta carries
    /// the derived utxos, so this only checks and mutates. The precondition and the order are
    /// the same — all spends checked and removed before any create is inserted, so a
    /// transaction cannot spend a utxo it creates in the same delta.
    pub fn apply_unshielded_delta(
        &self,
        delta: &UnshieldedDelta,
        ctime: Timestamp,
    ) -> Result<Self, crate::error::TransactionInvalid<D>> {
        let mut res = self.clone();
        for spend in &delta.spends {
            let utxo = Utxo::from(spend.clone());
            if !res.utxos.contains_key(&utxo) {
                return Err(crate::error::TransactionInvalid::InputNotInUtxos(Box::new(
                    utxo,
                )));
            }
            res = res.remove(&utxo);
        }
        for create in &delta.creates {
            res = res.insert(create.clone(), crate::structure::UtxoMeta { ctime });
        }
        Ok(res)
    }
}

// ── the flat codec ──────────────────────────────────────────────────────────────────────
//
// Fixed-width fields and length-prefixed sequences, little-endian, no tags and no
// self-description. The point is that decoding is a walk with direct loads: no allocation
// per node, no hashing, no arena. `Serializable` is deliberately not used — it is the
// machinery this exists to avoid.

/// Anything that can be written to, and read from, the flat form.
pub trait Flat: Sized {
    /// Append `self` to `out`.
    fn put(&self, out: &mut Vec<u8>);
    /// Read one value from `inp`, advancing it. `None` if the input is short or malformed.
    fn get(inp: &mut &[u8]) -> Option<Self>;
}

fn take<'a>(inp: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if inp.len() < n {
        return None;
    }
    let (a, b) = inp.split_at(n);
    *inp = b;
    Some(a)
}

impl Flat for u16 {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(u16::from_le_bytes(take(inp, 2)?.try_into().ok()?))
    }
}

impl Flat for u32 {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(u32::from_le_bytes(take(inp, 4)?.try_into().ok()?))
    }
}

impl Flat for u64 {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(u64::from_le_bytes(take(inp, 8)?.try_into().ok()?))
    }
}

impl<A: Flat, B: Flat> Flat for (A, B) {
    fn put(&self, out: &mut Vec<u8>) {
        self.0.put(out);
        self.1.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some((A::get(inp)?, B::get(inp)?))
    }
}

impl Flat for u128 {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(u128::from_le_bytes(take(inp, 16)?.try_into().ok()?))
    }
}

impl Flat for [u8; 32] {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        take(inp, 32)?.try_into().ok()
    }
}

impl<T: Flat> Flat for Vec<T> {
    fn put(&self, out: &mut Vec<u8>) {
        (self.len() as u32).put(out);
        for x in self {
            x.put(out);
        }
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        let n = u32::get(inp)? as usize;
        // ⚠︎ No `with_capacity(n)`: `n` is attacker-controlled and would otherwise be a
        // one-word allocation bomb. The push path grows only as far as the input allows.
        let mut v = Vec::new();
        for _ in 0..n {
            v.push(T::get(inp)?);
        }
        Some(v)
    }
}

impl<T: Flat> Flat for Option<T> {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            None => out.push(0),
            Some(x) => {
                out.push(1);
                x.put(out);
            }
        }
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        match take(inp, 1)?[0] {
            0 => Some(None),
            1 => Some(Some(T::get(inp)?)),
            _ => None,
        }
    }
}

impl Flat for Timestamp {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_secs().to_le_bytes());
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(Timestamp::from_secs(u64::from_le_bytes(
            take(inp, 8)?.try_into().ok()?,
        )))
    }
}

/// ⌖ Leaves delegate to `Serializable`, which is **not** `Storable`: it is plain byte work
/// on types that carry no `Sp` and are not generic over `DB`, so it costs nothing like the
/// graph reconstruction this module exists to avoid. Delegating also keeps the wire form
/// identical to the ledger's own, so the two cannot drift.
macro_rules! flat_via_serializable {
    ($($t:ty),* $(,)?) => { $(
        impl Flat for $t {
            fn put(&self, out: &mut Vec<u8>) {
                Serializable::serialize(self, out).expect("writing to a Vec cannot fail");
            }
            fn get(inp: &mut &[u8]) -> Option<Self> {
                Deserializable::deserialize(inp, 0).ok()
            }
        }
    )* };
}

flat_via_serializable!(
    base_crypto::hash::HashOutput,
    coin_structure::coin::TokenType,
    onchain_runtime::context::ClaimedContractCallsValue,
    onchain_runtime::context::ClaimedUnshieldedSpendsKey,
    DustNullifier,
    DustCommitment,
    DustPublicKey,
    VerifyingKey,
    MerkleTreeDigest,
    UtxoSpend,
    Utxo,
    Nullifier,
    Commitment,
    IntentHash,
    ContractAddress,
);

impl Flat for UnshieldedDelta {
    fn put(&self, out: &mut Vec<u8>) {
        self.spends.put(out);
        self.creates.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(UnshieldedDelta {
            spends: Vec::get(inp)?,
            creates: Vec::get(inp)?,
        })
    }
}

impl Flat for SpendDelta {
    fn put(&self, out: &mut Vec<u8>) {
        self.merkle_tree_root.put(out);
        self.nullifier.put(out);
        self.contract_address.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(SpendDelta {
            merkle_tree_root: MerkleTreeDigest::get(inp)?,
            nullifier: Nullifier::get(inp)?,
            contract_address: Option::get(inp)?,
        })
    }
}

impl Flat for CreateDelta {
    fn put(&self, out: &mut Vec<u8>) {
        self.coin_com.put(out);
        self.contract_address.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(CreateDelta {
            coin_com: Commitment::get(inp)?,
            contract_address: Option::get(inp)?,
        })
    }
}

impl Flat for TransientDelta {
    fn put(&self, out: &mut Vec<u8>) {
        self.nullifier.put(out);
        self.coin_com.put(out);
        self.contract_address.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(TransientDelta {
            nullifier: Nullifier::get(inp)?,
            coin_com: Commitment::get(inp)?,
            contract_address: Option::get(inp)?,
        })
    }
}

impl Flat for TranscriptEffects {
    fn put(&self, out: &mut Vec<u8>) {
        self.claimed_nullifiers.put(out);
        self.claimed_shielded_receives.put(out);
        self.claimed_shielded_spends.put(out);
        self.claimed_contract_calls.put(out);
        self.shielded_mints.put(out);
        self.unshielded_mints.put(out);
        self.unshielded_inputs.put(out);
        self.unshielded_outputs.put(out);
        self.claimed_unshielded_spends.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(TranscriptEffects {
            claimed_nullifiers: Vec::get(inp)?,
            claimed_shielded_receives: Vec::get(inp)?,
            claimed_shielded_spends: Vec::get(inp)?,
            claimed_contract_calls: Vec::get(inp)?,
            shielded_mints: Vec::get(inp)?,
            unshielded_mints: Vec::get(inp)?,
            unshielded_inputs: Vec::get(inp)?,
            unshielded_outputs: Vec::get(inp)?,
            claimed_unshielded_spends: Vec::get(inp)?,
        })
    }
}

impl Flat for ZswapDelta {
    fn put(&self, out: &mut Vec<u8>) {
        self.spends.put(out);
        self.creates.put(out);
        self.transients.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(ZswapDelta {
            spends: Vec::get(inp)?,
            creates: Vec::get(inp)?,
            transients: Vec::get(inp)?,
        })
    }
}

impl Flat for u8 {
    fn put(&self, out: &mut Vec<u8>) {
        out.push(*self);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(take(inp, 1)?[0])
    }
}

impl Flat for IntentEffects {
    fn put(&self, out: &mut Vec<u8>) {
        self.segment.put(out);
        self.intent_hash.put(out);
        self.ttl.put(out);
        self.unshielded.put(out);
        self.dust.put(out);
        self.contracts.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(IntentEffects {
            segment: u16::get(inp)?,
            intent_hash: IntentHash::get(inp)?,
            ttl: Timestamp::get(inp)?,
            unshielded: UnshieldedDelta::get(inp)?,
            dust: DustDelta::get(inp)?,
            contracts: ContractsPresent::get(inp)?,
        })
    }
}

impl Flat for GuaranteedEffects {
    fn put(&self, out: &mut Vec<u8>) {
        self.zswap.put(out);
        self.intents.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(GuaranteedEffects {
            zswap: ZswapDelta::get(inp)?,
            intents: Vec::get(inp)?,
        })
    }
}

impl Flat for FallibleSegment {
    fn put(&self, out: &mut Vec<u8>) {
        self.segment.put(out);
        self.zswap.put(out);
        self.unshielded.put(out);
        self.contracts.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(FallibleSegment {
            segment: u16::get(inp)?,
            zswap: ZswapDelta::get(inp)?,
            unshielded: Option::get(inp)?,
            contracts: ContractsPresent::get(inp)?,
        })
    }
}

impl Flat for TransactionEffects {
    fn put(&self, out: &mut Vec<u8>) {
        self.guaranteed.put(out);
        self.fallible.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(TransactionEffects {
            guaranteed: GuaranteedEffects::get(inp)?,
            fallible: Vec::get(inp)?,
        })
    }
}

impl Flat for DustSpendEffect {
    fn put(&self, out: &mut Vec<u8>) {
        self.v_fee.put(out);
        self.old_nullifier.put(out);
        self.new_commitment.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(DustSpendEffect {
            v_fee: u128::get(inp)?,
            old_nullifier: DustNullifier::get(inp)?,
            new_commitment: DustCommitment::get(inp)?,
        })
    }
}

impl Flat for DustRegistrationEffect {
    fn put(&self, out: &mut Vec<u8>) {
        self.night_key.put(out);
        self.dust_address.put(out);
        self.allow_fee_payment.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(DustRegistrationEffect {
            night_key: VerifyingKey::get(inp)?,
            dust_address: Option::get(inp)?,
            allow_fee_payment: u128::get(inp)?,
        })
    }
}

impl Flat for DustDelta {
    fn put(&self, out: &mut Vec<u8>) {
        self.ctime.put(out);
        self.spends.put(out);
        self.registrations.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(DustDelta {
            ctime: Timestamp::get(inp)?,
            spends: Vec::get(inp)?,
            registrations: Vec::get(inp)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Flat + PartialEq + core::fmt::Debug>(v: T) {
        let mut buf = Vec::new();
        v.put(&mut buf);
        let mut inp = &buf[..];
        let back = T::get(&mut inp).expect("decodes");
        assert_eq!(v, back, "round trip");
        assert!(
            inp.is_empty(),
            "decoder consumed exactly what the encoder wrote"
        );
    }

    #[test]
    fn scalars_and_sequences_round_trip() {
        round_trip(7u32);
        round_trip(vec![1u32, 2, 3]);
        round_trip(Vec::<u32>::new());
        round_trip(Some(9u128));
        round_trip(None::<u128>);
        round_trip([3u8; 32]);
    }

    #[test]
    fn dust_round_trips() {
        round_trip(DustDelta {
            ctime: Timestamp::from_secs(1234),
            spends: vec![DustSpendEffect {
                v_fee: 5,
                old_nullifier: DustNullifier(Default::default()),
                new_commitment: DustCommitment(Default::default()),
            }],
            registrations: vec![DustRegistrationEffect {
                night_key: VerifyingKey::default(),
                dust_address: None,
                allow_fee_payment: 11,
            }],
        });
    }

    /// A truncated input must decline, not panic and not invent a value.
    #[test]
    fn a_short_input_is_refused_at_every_prefix() {
        let v = DustDelta {
            ctime: Timestamp::from_secs(1),
            spends: vec![DustSpendEffect {
                v_fee: 1,
                old_nullifier: DustNullifier(Default::default()),
                new_commitment: DustCommitment(Default::default()),
            }],
            registrations: vec![],
        };
        let mut buf = Vec::new();
        v.put(&mut buf);
        for cut in 0..buf.len() {
            let mut inp = &buf[..cut];
            assert!(
                DustDelta::get(&mut inp).is_none(),
                "a {cut}-octet prefix must not decode"
            );
        }
    }

    /// A whole record, of the shape a real transaction produces: one segment, a shielded
    /// spend and two creates, an unshielded spend and output, and a dust spend.
    ///
    /// ⌖ The size is the point as much as the round trip. The transport it replaces is 1,422
    /// octets and costs 4,584,171 gas to decode; `𝖶_R`, the report's blob budget, is 49,152.
    /// If this were to come out at tens of kilobytes the design would be in trouble.
    #[test]
    fn a_whole_record_round_trips_and_stays_small() {
        let h = |n| IntentHash(base_crypto::hash::HashOutput([n; 32]));
        let fx = TransactionEffects {
            guaranteed: GuaranteedEffects {
                zswap: ZswapDelta {
                    spends: vec![SpendDelta {
                        merkle_tree_root: MerkleTreeDigest::default(),
                        nullifier: Nullifier(base_crypto::hash::HashOutput([1; 32])),
                        contract_address: None,
                    }],
                    creates: vec![
                        CreateDelta {
                            coin_com: Commitment(base_crypto::hash::HashOutput([2; 32])),
                            contract_address: None,
                        },
                        CreateDelta {
                            coin_com: Commitment(base_crypto::hash::HashOutput([3; 32])),
                            contract_address: None,
                        },
                    ],
                    transients: vec![],
                },
                intents: vec![IntentEffects {
                    segment: 0,
                    intent_hash: h(4),
                    ttl: Timestamp::from_secs(2000),
                    unshielded: UnshieldedDelta::default(),
                    dust: DustDelta {
                        ctime: Timestamp::from_secs(1500),
                        spends: vec![DustSpendEffect {
                            v_fee: 42,
                            old_nullifier: DustNullifier(Default::default()),
                            new_commitment: DustCommitment(Default::default()),
                        }],
                        registrations: vec![],
                    },
                    contracts: ContractsPresent::None,
                }],
            },
            fallible: vec![FallibleSegment {
                segment: 1,
                zswap: ZswapDelta::default(),
                unshielded: Some(UnshieldedDelta::default()),
                contracts: ContractsPresent::None,
            }],
        };
        let mut buf = Vec::new();
        fx.put(&mut buf);
        let mut inp = &buf[..];
        assert_eq!(TransactionEffects::get(&mut inp).as_ref(), Some(&fx));
        assert!(inp.is_empty(), "decoded exactly what was encoded");
        assert!(
            buf.len() < 1024,
            "a one-segment record should be well under a kilobyte, was {}",
            buf.len()
        );
    }

    /// **The gate: the projection must lose nothing `ZswapEffects` carries.**
    ///
    /// ⌖ Written because two hand-derived shapes were wrong in ways neither the type system
    /// nor five passing round-trip tests noticed: the per-spend merkle root was collapsed to
    /// one per offer, and `contract_address` was dropped from every entry. Both are
    /// soundness-relevant — the first would accept a spend proven against a root the state
    /// never held. Field-by-field correspondence against the ledger's own projection is the
    /// only thing that catches that class.
    #[test]
    fn the_projection_loses_nothing_the_ledgers_own_effects_carry() {
        use coin_structure::coin::{Info as CoinInfo, PublicKey, ShieldedTokenType};
        use storage::db::InMemoryDB;
        use zswap::{Delta, Offer, Output};

        use coin_structure::contract::ContractAddress;
        use std::sync::Arc;
        use storage::arena::Sp;
        use zswap::Input;

        let mut rng = rand::thread_rng();
        let (mut outputs, mut deltas) = (Vec::new(), Vec::new());
        for _ in 0..4 {
            let (type_, value): (ShieldedTokenType, u128) =
                (rand::Rng::r#gen(&mut rng), rand::Rng::r#gen(&mut rng));
            let info = CoinInfo {
                nonce: rand::Rng::r#gen(&mut rng),
                type_,
                value,
            };
            let cpk = PublicKey(rand::Rng::r#gen(&mut rng));
            outputs.push(
                Output::new(&mut rng, &info, None, &cpk, None)
                    .expect("output")
                    .erase_proof(),
            );
            deltas.push(Delta {
                token_type: type_,
                value: value as i128,
            });
        }

        // ⚠︎ Inputs are what make this test mean anything. Built by hand because the fields
        // are public and the constructors want a witnessed tree: each carries a **distinct**
        // merkle root, so collapsing them to one is visible, and a **present**
        // `contract_address`, so dropping it is visible. With outputs alone — which is all
        // the fixture had at first, and all zswap's own equivalence test has — both loops
        // below iterate zero times and the test passes against a deliberately broken
        // projection. Verified: it did.
        let inputs: Vec<Input<(), InMemoryDB>> = (0u8..3)
            .map(|i| Input {
                nullifier: Nullifier(base_crypto::hash::HashOutput([i + 1; 32])),
                value_commitment: Default::default(),
                contract_address: Some(Sp::new(ContractAddress(base_crypto::hash::HashOutput(
                    [i + 100; 32],
                )))),
                merkle_tree_root: MerkleTreeDigest::from(transient_crypto::curve::Fr::from(
                    u64::from(i) + 1,
                )),
                proof: Arc::new(()),
            })
            .collect();

        let offer: Offer<(), InMemoryDB> = Offer {
            inputs: inputs.into(),
            outputs: outputs.into(),
            transient: vec![].into(),
            deltas: deltas.into(),
        };

        let reference = offer.effects();
        let ours = ZswapDelta::from_offer(&offer);

        // The fixture must actually exercise what the loops below check.
        assert!(!ours.spends.is_empty(), "fixture has no spends to compare");
        assert!(
            !ours.creates.is_empty(),
            "fixture has no creates to compare"
        );
        assert!(
            ours.spends.iter().any(|s| s.contract_address.is_some()),
            "fixture has no contract address, so dropping it would go unnoticed"
        );
        assert!(
            ours.spends
                .iter()
                .map(|s| s.merkle_tree_root)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1,
            "fixture has one distinct root, so collapsing per-spend roots would go unnoticed"
        );

        assert_eq!(ours.spends.len(), reference.spends.len(), "spend count");
        assert_eq!(ours.creates.len(), reference.creates.len(), "create count");
        assert_eq!(
            ours.transients.len(),
            reference.transients.len(),
            "transient count"
        );

        for (a, b) in ours.spends.iter().zip(reference.spends.iter_deref()) {
            assert_eq!(a.nullifier, b.nullifier, "nullifier");
            // ⚠︎ per spend, not per offer — the bug this line exists for
            assert_eq!(a.merkle_tree_root, b.merkle_tree_root, "merkle root");
            assert_eq!(
                a.contract_address,
                b.contract_address.as_ref().map(|x| *x.clone()),
                "contract address"
            );
        }
        for (a, b) in ours.creates.iter().zip(reference.creates.iter_deref()) {
            assert_eq!(a.coin_com, b.coin_com, "commitment");
            assert_eq!(
                a.contract_address,
                b.contract_address.as_ref().map(|x| *x.clone()),
                "contract address"
            );
        }

        // And it must survive the wire unchanged.
        let mut buf = Vec::new();
        ours.put(&mut buf);
        let mut inp = &buf[..];
        assert_eq!(ZswapDelta::get(&mut inp).as_ref(), Some(&ours));
        assert!(inp.is_empty());
    }

    /// **The unshielded gate.** `apply_offer` derives each created utxo from the intent hash
    /// plus the output's *enumeration index*; the expectation below is written out by hand
    /// rather than re-derived, so a producer that enumerated differently, dropped a field, or
    /// used the wrong hash cannot agree with it by construction.
    #[test]
    fn the_unshielded_projection_matches_what_apply_offer_derives() {
        use crate::structure::{UnshieldedOffer, UtxoOutput};
        use base_crypto::hash::HashOutput;
        use coin_structure::coin::UnshieldedTokenType;
        use storage::db::InMemoryDB;

        let ty = UnshieldedTokenType(HashOutput([8; 32]));
        let owner = coin_structure::coin::UserAddress(HashOutput([9; 32]));
        // A sentinel distinct from anything the producer could derive on its own, so using
        // some *other* intent hash would be visible.
        let ih = IntentHash(HashOutput([0xAB; 32]));

        let spend = UtxoSpend {
            value: 5,
            owner: Default::default(),
            type_: ty,
            intent_hash: IntentHash(HashOutput([1; 32])),
            output_no: 3,
        };
        let offer: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
            inputs: vec![spend.clone()].into(),
            outputs: vec![
                UtxoOutput {
                    value: 11,
                    owner,
                    type_: ty,
                },
                UtxoOutput {
                    value: 22,
                    owner,
                    type_: ty,
                },
            ]
            .into(),
            signatures: vec![].into(),
        };

        let d = UnshieldedDelta::from_offer(&offer, ih);

        assert_eq!(d.spends, vec![spend], "spends pass through untouched");
        assert_eq!(
            d.creates,
            vec![
                Utxo {
                    value: 11,
                    owner,
                    type_: ty,
                    intent_hash: ih,
                    output_no: 0,
                },
                Utxo {
                    value: 22,
                    owner,
                    type_: ty,
                    intent_hash: ih,
                    output_no: 1,
                },
            ],
            "creates carry the given intent hash and the offer's enumeration order"
        );

        let mut buf = Vec::new();
        d.put(&mut buf);
        let mut inp = &buf[..];
        assert_eq!(UnshieldedDelta::get(&mut inp).as_ref(), Some(&d));
        assert!(inp.is_empty());
    }

    /// **The dust gate.** Field correspondence, and — the part that matters here — that
    /// order is preserved. `apply_registration` threads a running fee allowance through the
    /// registrations, so a projection that reordered them would produce different outcomes
    /// from the same inputs while every individual field still matched.
    #[test]
    fn the_dust_projection_preserves_fields_and_order() {
        use crate::dust::{DustActions, DustRegistration, DustSpend};
        use storage::db::InMemoryDB;

        let spend = |fee: u128| DustSpend::<(), InMemoryDB> {
            v_fee: fee,
            old_nullifier: DustNullifier(Default::default()),
            new_commitment: DustCommitment(Default::default()),
            proof: (),
        };
        let reg = |allow: u128| DustRegistration::<(), InMemoryDB> {
            night_key: VerifyingKey::default(),
            dust_address: None,
            allow_fee_payment: allow,
            signature: None,
        };
        let actions = DustActions::<(), (), InMemoryDB> {
            spends: vec![spend(1), spend(2), spend(3)].into(),
            registrations: vec![reg(10), reg(20)].into(),
            ctime: Timestamp::from_secs(777),
        };

        let d = DustDelta::from_actions(&actions);

        assert_eq!(d.ctime, Timestamp::from_secs(777));
        assert_eq!(
            d.spends.iter().map(|s| s.v_fee).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "spends keep the order the fee allowance is threaded in"
        );
        assert_eq!(
            d.registrations
                .iter()
                .map(|r| r.allow_fee_payment)
                .collect::<Vec<_>>(),
            vec![10, 20],
            "registrations keep their order"
        );

        let mut buf = Vec::new();
        d.put(&mut buf);
        let mut inp = &buf[..];
        assert_eq!(
            DustDelta::get(&mut inp).as_ref(),
            Some(&d),
            "survives the wire"
        );
        assert!(inp.is_empty());
    }

    /// **The applier's preconditions and mutations, both directions.**
    #[test]
    fn applying_an_unshielded_delta_checks_then_mutates() {
        use crate::structure::{UtxoMeta, UtxoState};
        use base_crypto::hash::HashOutput;
        use coin_structure::coin::{UnshieldedTokenType, UserAddress};
        use storage::db::InMemoryDB;

        let ty = UnshieldedTokenType(HashOutput([8; 32]));
        // ⚠︎ The owner must be what `Utxo::from(UtxoSpend)` derives — it maps the spend's
        // `VerifyingKey` through `UserAddress::from`. An unrelated address makes the spend
        // name a utxo the state does not hold, and the test then proves nothing. The
        // assertion below exists because the first version of this fixture did exactly that.
        let spender = VerifyingKey::default();
        let owner = UserAddress::from(spender.clone());
        let utxo = |n: u8, v: u128| Utxo {
            value: v,
            owner,
            type_: ty,
            intent_hash: IntentHash(HashOutput([n; 32])),
            output_no: 0,
        };
        let ctime = Timestamp::from_secs(500);

        let held = utxo(1, 10);
        let state: UtxoState<InMemoryDB> =
            UtxoState::default().insert(held.clone(), UtxoMeta { ctime });

        // A spend of something absent is refused, and nothing is mutated.
        let absent = UnshieldedDelta {
            spends: vec![UtxoSpend {
                value: 99,
                owner: spender.clone(),
                type_: ty,
                intent_hash: IntentHash(HashOutput([7; 32])),
                output_no: 0,
            }],
            creates: vec![],
        };
        assert!(
            state.apply_unshielded_delta(&absent, ctime).is_err(),
            "spending a utxo the state does not hold must be refused"
        );

        // A spend of something held, plus a create.
        let made = utxo(2, 20);
        let d = UnshieldedDelta {
            spends: vec![UtxoSpend {
                value: held.value,
                owner: spender.clone(),
                type_: ty,
                intent_hash: held.intent_hash,
                output_no: held.output_no,
            }],
            creates: vec![made.clone()],
        };
        // The spend's identity must be the utxo actually held, or this tests nothing.
        assert_eq!(
            Utxo::from(d.spends[0].clone()),
            held,
            "fixture's spend must name the held utxo"
        );

        let after = state
            .apply_unshielded_delta(&d, ctime)
            .expect("the spend is present, so this applies");
        assert!(!after.utxos.contains_key(&held), "the spent utxo is gone");
        assert!(
            after.utxos.contains_key(&made),
            "the created utxo is present"
        );
    }

    /// ⚠︎ **A delta may not spend what it creates.** Spends are all checked and removed
    /// before any create is inserted, so a create cannot satisfy a spend in the same delta.
    /// Reversing those two loops leaves every other assertion in this module passing while
    /// permitting value to be conjured from nothing — verified by mutation, which is the only
    /// reason this test exists as well as the one above.
    #[test]
    fn a_delta_cannot_spend_the_utxo_it_creates() {
        use crate::structure::UtxoState;
        use base_crypto::hash::HashOutput;
        use coin_structure::coin::{UnshieldedTokenType, UserAddress};
        use storage::db::InMemoryDB;

        let ty = UnshieldedTokenType(HashOutput([8; 32]));
        let spender = VerifyingKey::default();
        let owner = UserAddress::from(spender.clone());
        let ih = IntentHash(HashOutput([3; 32]));

        // The create and the spend name the *same* utxo.
        let coin = Utxo {
            value: 50,
            owner,
            type_: ty,
            intent_hash: ih,
            output_no: 0,
        };
        let d = UnshieldedDelta {
            spends: vec![UtxoSpend {
                value: 50,
                owner: spender,
                type_: ty,
                intent_hash: ih,
                output_no: 0,
            }],
            creates: vec![coin.clone()],
        };
        assert_eq!(
            Utxo::from(d.spends[0].clone()),
            coin,
            "fixture must have the spend and the create name one utxo"
        );

        let empty: UtxoState<InMemoryDB> = UtxoState::default();
        assert!(
            empty
                .apply_unshielded_delta(&d, Timestamp::from_secs(1))
                .is_err(),
            "a delta must not be able to spend a utxo it creates in the same delta"
        );
    }

    /// ⚠︎ **The two hashes must not be swapped.** `intent_hash(segment)` identifies the
    /// utxos an intent creates; replay protection uses `intent_hash(0)`, segment-independent
    /// so a replay cannot be relocated. They differ for any non-zero segment, and swapping
    /// them yields utxos under identities nothing else agrees with — silently, since both are
    /// well-formed hashes of the same intent.
    #[test]
    fn the_creating_hash_and_the_replay_hash_are_not_the_same() {
        use crate::structure::{Intent, UnshieldedOffer, UtxoOutput};
        use base_crypto::hash::HashOutput;
        use coin_structure::coin::{UnshieldedTokenType, UserAddress};
        use storage::db::InMemoryDB;
        use transient_crypto::commitment::Pedersen;

        let ty = UnshieldedTokenType(HashOutput([8; 32]));
        let owner = UserAddress(HashOutput([9; 32]));
        let offer: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
            inputs: vec![].into(),
            outputs: vec![UtxoOutput {
                value: 7,
                owner,
                type_: ty,
            }]
            .into(),
            signatures: vec![].into(),
        };
        let intent: Intent<(), (), Pedersen, InMemoryDB> = Intent {
            guaranteed_unshielded_offer: Some(storage::arena::Sp::new(offer)),
            fallible_unshielded_offer: None,
            actions: vec![].into(),
            dust_actions: None,
            ttl: Timestamp::from_secs(1234),
            binding_commitment: Pedersen::default(),
        };

        const SEG: u16 = 3;
        let by_segment = intent.intent_hash(SEG);
        let by_zero = intent.intent_hash(0);
        assert_ne!(
            by_segment, by_zero,
            "the fixture must use a segment where the two hashes differ, or this proves nothing"
        );

        let e = IntentEffects::from_intent(&intent, SEG);

        assert_eq!(
            e.intent_hash, by_zero,
            "replay uses the segment-independent hash"
        );
        assert_eq!(e.segment, SEG);
        assert_eq!(e.ttl, Timestamp::from_secs(1234));
        assert_eq!(e.unshielded.creates.len(), 1, "fixture must create a utxo");
        // ⚠︎ **This assertion used to read `by_segment`, and it was wrong.**
        //
        // It encoded the same misreading as the code it was checking, so it passed. The
        // creating hash is `intent_hash(pass)`, and `from_intent` projects the *guaranteed
        // pass*, whose pass is 0 whatever segment the intent sits in — `semantics.rs:990`
        // applies every intent's guaranteed offer with `segment == 0`. The whole-transaction
        // comparison in `tests/effects_equivalence.rs` is what caught it: it put the created
        // utxo under a hash the full path never produces, and nothing at this level could
        // see that, because both sides of a family-level gate were derived from the same
        // wrong belief.
        assert_eq!(
            e.unshielded.creates[0].intent_hash, by_zero,
            "the guaranteed pass creates under intent_hash(0) — see semantics.rs:990"
        );
        assert_ne!(
            e.unshielded.creates[0].intent_hash, by_segment,
            "and specifically not under the physical segment's hash"
        );
        assert_eq!(
            e.contracts,
            ContractsPresent::None,
            "no actions in the fixture"
        );

        // ⚠︎ And the flag must actually flip. Without an intent that *has* an action, a
        // producer that always answered `None` passes — verified by mutation, it did. That
        // flag routes a contract transaction to the full path, so answering it wrongly means
        // silently applying incomplete effects, which is the worst failure this module has.
        let with_action = Intent::<(), (), Pedersen, InMemoryDB> {
            actions: vec![crate::structure::ContractAction::Maintain(
                crate::structure::MaintenanceUpdate {
                    address: coin_structure::contract::ContractAddress(HashOutput([5; 32])),
                    updates: vec![].into(),
                    counter: 0,
                    signatures: vec![].into(),
                },
            )]
            .into(),
            ..intent.clone()
        };
        assert_eq!(
            IntentEffects::from_intent(&with_action, SEG).contracts,
            ContractsPresent::UseFullPath,
            "an intent with actions must route to the full path"
        );
    }

    /// ⚠︎ **Incomplete effects must be refused, not partially applied.**
    ///
    /// An intent carrying contract actions or dust registrations is not fully represented
    /// here. Applying the part that *is* represented would advance the state by a prefix of
    /// the transaction — locally valid-looking, and in disagreement with every node that took
    /// the ordinary path. That is the worst failure this module can produce, so it is the one
    /// property tested directly rather than inferred.
    #[test]
    fn incomplete_effects_are_refused_rather_than_partly_applied() {
        let with_contracts = IntentEffects {
            segment: 0,
            intent_hash: IntentHash(base_crypto::hash::HashOutput([1; 32])),
            ttl: Timestamp::from_secs(10),
            unshielded: UnshieldedDelta::default(),
            dust: DustDelta::default(),
            contracts: ContractsPresent::UseFullPath,
        };
        assert!(
            matches!(
                crate::structure::LedgerState::<storage::db::InMemoryDB>::refuse_if_incomplete(
                    &with_contracts
                ),
                Err(crate::error::TransactionInvalid::EffectsIncompleteContracts)
            ),
            "an intent with contract actions must be refused"
        );

        let with_registrations = IntentEffects {
            dust: DustDelta {
                ctime: Timestamp::from_secs(1),
                spends: vec![],
                registrations: vec![DustRegistrationEffect {
                    night_key: VerifyingKey::default(),
                    dust_address: None,
                    allow_fee_payment: 1,
                }],
            },
            contracts: ContractsPresent::None,
            ..with_contracts.clone()
        };
        assert!(
            matches!(
                crate::structure::LedgerState::<storage::db::InMemoryDB>::refuse_if_incomplete(
                    &with_registrations
                ),
                Err(crate::error::TransactionInvalid::EffectsIncomplete)
            ),
            "an intent with dust registrations must be refused"
        );

        // And an intent carrying neither is accepted, or the guard would be refusing
        // everything and the two assertions above would prove nothing.
        let plain = IntentEffects {
            dust: DustDelta::default(),
            contracts: ContractsPresent::None,
            ..with_contracts
        };
        assert!(
            crate::structure::LedgerState::<storage::db::InMemoryDB>::refuse_if_incomplete(&plain)
                .is_ok(),
            "an intent with neither must be accepted"
        );
    }

    /// **The equivalence gate for the unshielded family.**
    ///
    /// The same offer applied two ways — through `UtxoState::apply_offer`, which derives each
    /// created utxo from the intent hash and the output index, and through
    /// `UnshieldedDelta::from_offer` + `apply_unshielded_delta`, where the derivation happened
    /// in the producer. The resulting states must be indistinguishable.
    ///
    /// ⌖ This is the only test here that compares the *two paths* rather than checking one
    /// against a written-out expectation. An expectation can be wrong in the same way the code
    /// is; the ledger's own applier cannot.
    #[test]
    fn the_effects_path_and_apply_offer_reach_the_same_state() {
        use crate::semantics::TransactionContext;
        use crate::structure::{Intent, UnshieldedOffer, UtxoOutput, UtxoState};
        use base_crypto::hash::HashOutput;
        use coin_structure::coin::{UnshieldedTokenType, UserAddress};
        use onchain_runtime::context::BlockContext;
        use storage::db::InMemoryDB;
        use transient_crypto::commitment::Pedersen;

        let ty = UnshieldedTokenType(HashOutput([8; 32]));
        let spender = VerifyingKey::default();
        let owner = UserAddress::from(spender.clone());
        let tblock = Timestamp::from_secs(900);

        let offer: UnshieldedOffer<(), InMemoryDB> = UnshieldedOffer {
            inputs: vec![].into(),
            outputs: vec![
                UtxoOutput {
                    value: 11,
                    owner,
                    type_: ty,
                },
                UtxoOutput {
                    value: 22,
                    owner,
                    type_: ty,
                },
            ]
            .into(),
            signatures: vec![].into(),
        };
        let intent: Intent<(), (), Pedersen, InMemoryDB> = Intent {
            guaranteed_unshielded_offer: Some(storage::arena::Sp::new(offer.clone())),
            fallible_unshielded_offer: None,
            actions: vec![].into(),
            dust_actions: None,
            ttl: tblock,
            binding_commitment: Pedersen::default(),
        };

        const SEG: u16 = 2;
        let ctx = TransactionContext::<InMemoryDB> {
            ref_state: crate::structure::LedgerState::new("test"),
            block_context: BlockContext {
                tblock,
                tblock_err: 0,
                parent_block_hash: HashOutput([0; 32]),
                last_block_time: tblock,
            },
            whitelist: None,
        };

        let base = UtxoState::<InMemoryDB>::default();
        let via_apply = base
            .apply_offer(&offer, &intent, SEG, &ctx)
            .expect("no inputs, so nothing to be missing");

        let delta = UnshieldedDelta::from_offer(&offer, intent.intent_hash(SEG));
        let via_effects = base
            .apply_unshielded_delta(&delta, tblock)
            .expect("same offer, so the same outcome");

        // Compare the utxo sets themselves, not a summary of them.
        let members = |st: &UtxoState<InMemoryDB>| {
            let mut v: Vec<Utxo> = st.utxos.keys().collect();
            v.sort();
            v
        };
        assert!(!members(&via_apply).is_empty(), "fixture created no utxos");
        assert_eq!(
            members(&via_apply),
            members(&via_effects),
            "the two paths must create the same utxos, with the same identities"
        );
    }

    /// ⚠︎ **Sorted, because the bytes must not depend on hash iteration order.**
    ///
    /// `Effects`'s fields are sets and maps; these are sequences. Reconstruction is unaffected
    /// by order — the originals are sets — but the *encoding* is not, and two producers
    /// emitting different octets for the same effects is a consensus fault rather than a
    /// flaky test. The projection sorts; this pins that it does.
    #[test]
    fn the_transcript_projection_is_ordered() {
        use base_crypto::hash::HashOutput;

        let h = |n: u8| HashOutput([n; 32]);
        // Deliberately built out of order.
        let fx = TranscriptEffects {
            shielded_mints: vec![(h(9), 1), (h(2), 2), (h(5), 3)],
            ..Default::default()
        };
        let mut sorted = fx.clone();
        sorted.shielded_mints.sort();

        let enc = |v: &TranscriptEffects| {
            let mut b = Vec::new();
            v.put(&mut b);
            b
        };
        assert_ne!(
            enc(&fx),
            enc(&sorted),
            "if these matched, order would not reach the octets and this test would be vacuous"
        );

        // And the codec itself round-trips whatever order it is given.
        let mut b = Vec::new();
        fx.put(&mut b);
        let mut inp = &b[..];
        assert_eq!(TranscriptEffects::get(&mut inp).as_ref(), Some(&fx));
        assert!(inp.is_empty());
    }

    /// ⚠︎ A huge declared count must not allocate before the octets exist. The decoder grows
    /// only as the input supplies elements, so a four-octet header cannot ask for a gigabyte.
    #[test]
    fn a_lying_length_prefix_allocates_nothing() {
        let mut buf = u32::MAX.to_le_bytes().to_vec();
        buf.extend_from_slice(&[0u8; 8]);
        let mut inp = &buf[..];
        assert!(Vec::<u128>::get(&mut inp).is_none());
    }
}
