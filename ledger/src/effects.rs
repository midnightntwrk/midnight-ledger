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
//! [`Offer::effects`]: midnight_zswap::structure::Offer::effects
//! [`State::try_apply_effects`]: midnight_zswap::ledger::State::try_apply_effects

use crate::structure::{IntentHash, UtxoOutput, UtxoSpend};
use base_crypto::time::Timestamp;
use coin_structure::coin::{Commitment, Nullifier};
use coin_structure::contract::ContractAddress;
use serialize::{Deserializable, Serializable};

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
    /// The replay-protection claim: this transaction has not been seen.
    pub replay: ReplayEffect,
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
    /// This segment's fallible unshielded changes, per intent.
    pub unshielded: Vec<UnshieldedDelta>,
    /// Contract transitions from this segment's fallible transcripts.
    pub contracts: Vec<ContractEffect>,
}

/// `apply_tx`'s input, reduced to what it reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayEffect {
    /// Precondition: absent from the replay set.
    pub tx_hash: IntentHash,
    /// The transaction's time-to-live.
    pub ttl: Timestamp,
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
    /// Contract transitions from the guaranteed transcripts.
    pub contracts: Vec<ContractEffect>,
}

/// Shielded changes, mirroring [`ZswapEffects`] field for field but as plain data.
///
/// ⚠︎ The merkle root is **per spend**, not per offer: each input was proven against
/// whichever root the prover held, and `apply_spend` checks each against `past_roots`
/// separately. A single root per delta would silently accept a spend proven against a root
/// the state never had.
///
/// [`ZswapEffects`]: midnight_zswap::structure::ZswapEffects
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
    pub merkle_tree_root: [u8; 32],
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnshieldedDelta {
    /// Precondition: present in `utxo`, and owned by the spender. Mutation: removed.
    pub spends: Vec<UtxoSpend>,
    /// Mutation: created.
    pub outputs: Vec<UtxoOutput>,
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
    /// The fee this spend pays.
    pub v_fee: u128,
    /// Precondition: absent from the dust nullifier set.
    pub old_nullifier: [u8; 32],
    /// Mutation: appended to the dust commitment tree.
    pub new_commitment: [u8; 32],
}

/// One dust registration, with its signature dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DustRegistrationEffect {
    /// The key being registered.
    pub key: [u8; 32],
    /// The value allowed.
    pub allow_fee_payment: u128,
}

/// One contract state transition.
///
/// ⌖ The transcript already carries its declared effects and the applier re-runs it to check
/// them. Where the producing computation is attested, the declaration can be taken on trust
/// provided the state it ran against is the state the applier holds — which is what
/// `prior_state` is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractEffect {
    /// Which contract.
    pub address: ContractAddress,
    /// Precondition: the applier's current state for `address` hashes to this.
    pub prior_state: [u8; 32],
    /// Mutation: the contract's state becomes this.
    pub new_state: Vec<u8>,
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
    UtxoSpend,
    UtxoOutput,
    Nullifier,
    Commitment,
    IntentHash,
    ContractAddress,
);

impl Flat for UnshieldedDelta {
    fn put(&self, out: &mut Vec<u8>) {
        self.spends.put(out);
        self.outputs.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(UnshieldedDelta {
            spends: Vec::get(inp)?,
            outputs: Vec::get(inp)?,
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
            merkle_tree_root: <[u8; 32]>::get(inp)?,
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

impl Flat for ContractEffect {
    fn put(&self, out: &mut Vec<u8>) {
        self.address.put(out);
        self.prior_state.put(out);
        self.new_state.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(ContractEffect {
            address: ContractAddress::get(inp)?,
            prior_state: <[u8; 32]>::get(inp)?,
            new_state: Vec::get(inp)?,
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
            contracts: Vec::get(inp)?,
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
            unshielded: Vec::get(inp)?,
            contracts: Vec::get(inp)?,
        })
    }
}

impl Flat for ReplayEffect {
    fn put(&self, out: &mut Vec<u8>) {
        self.tx_hash.put(out);
        self.ttl.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(ReplayEffect {
            tx_hash: IntentHash::get(inp)?,
            ttl: Timestamp::get(inp)?,
        })
    }
}

impl Flat for TransactionEffects {
    fn put(&self, out: &mut Vec<u8>) {
        self.replay.put(out);
        self.guaranteed.put(out);
        self.fallible.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(TransactionEffects {
            replay: ReplayEffect::get(inp)?,
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
            old_nullifier: <[u8; 32]>::get(inp)?,
            new_commitment: <[u8; 32]>::get(inp)?,
        })
    }
}

impl Flat for DustRegistrationEffect {
    fn put(&self, out: &mut Vec<u8>) {
        self.key.put(out);
        self.allow_fee_payment.put(out);
    }
    fn get(inp: &mut &[u8]) -> Option<Self> {
        Some(DustRegistrationEffect {
            key: <[u8; 32]>::get(inp)?,
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
                old_nullifier: [1; 32],
                new_commitment: [2; 32],
            }],
            registrations: vec![DustRegistrationEffect {
                key: [3; 32],
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
                old_nullifier: [4; 32],
                new_commitment: [5; 32],
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
            replay: ReplayEffect {
                tx_hash: h(9),
                ttl: Timestamp::from_secs(1000),
            },
            guaranteed: GuaranteedEffects {
                zswap: ZswapDelta {
                    spends: vec![SpendDelta {
                        merkle_tree_root: [7; 32],
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
                            old_nullifier: [5; 32],
                            new_commitment: [6; 32],
                        }],
                        registrations: vec![],
                    },
                    contracts: vec![],
                }],
            },
            fallible: vec![FallibleSegment {
                segment: 1,
                zswap: ZswapDelta::default(),
                unshielded: vec![UnshieldedDelta::default()],
                contracts: vec![],
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
