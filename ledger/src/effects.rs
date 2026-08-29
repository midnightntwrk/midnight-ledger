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

/// Everything applying one transaction does.
///
/// Segments are separate because their failure modes are: segment 0 failing aborts the whole
/// transaction, while a later segment failing leaves the others applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionEffects {
    /// The replay-protection claim: this transaction has not been seen.
    pub replay: ReplayEffect,
    /// One entry per segment, in application order.
    pub segments: Vec<SegmentEffects>,
}

/// `apply_tx`'s input, reduced to what it reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayEffect {
    /// Precondition: absent from the replay set.
    pub tx_hash: IntentHash,
    /// The transaction's time-to-live.
    pub ttl: Timestamp,
}

/// One segment's changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentEffects {
    /// Which segment. Segment 0 is the guaranteed one and may not fail alone.
    pub segment: u16,
    /// Shielded changes that must succeed for the transaction to apply at all.
    pub guaranteed: ZswapDelta,
    /// Shielded changes that may fail without failing the transaction.
    pub fallible: ZswapDelta,
    /// The intents in this segment, in order.
    pub intents: Vec<IntentEffects>,
}

/// Shielded changes: what must be absent, and what gets appended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZswapDelta {
    /// Precondition: `∈ past_roots`. The root the spends were proven against.
    pub merkle_root: Option<[u8; 32]>,
    /// Precondition: absent from the nullifier set. Mutation: inserted.
    pub nullifiers: Vec<Nullifier>,
    /// Mutation: appended to the commitment tree, in this order.
    pub commitments: Vec<Commitment>,
}

/// One intent's changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentEffects {
    /// Precondition: unseen. Mutation: recorded until `ttl`.
    pub intent_hash: IntentHash,
    /// When this intent stops being replayable.
    pub ttl: Timestamp,
    /// Unshielded changes that must succeed.
    pub guaranteed_unshielded: UnshieldedDelta,
    /// Unshielded changes that may fail alone.
    pub fallible_unshielded: UnshieldedDelta,
    /// Dust changes.
    pub dust: DustDelta,
    /// Contract state transitions, in order.
    pub contracts: Vec<ContractEffect>,
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
