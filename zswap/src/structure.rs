// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::ZSWAP_TREE_HEIGHT;
use crate::error::MalformedOffer;
use coin_structure::coin::{
    Commitment, Info as CoinInfo, Nullifier, PublicKey as CoinPublicKey, ShieldedTokenType,
    TokenType, UnshieldedTokenType,
};
use coin_structure::contract::ContractAddress;
use derive_where::derive_where;
use itertools::Itertools;
use rand::{CryptoRng, Rng};
use serde::Serialize;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Formatter};
use std::ops::{Add, Sub};
use std::sync::Arc;
use storage::Storable;
use storage::arena::Sp;
use storage::arena::{ArenaHash, ArenaKey};
use storage::db::DB;
#[cfg(test)]
use storage::db::InMemoryDB;
use storage::storable::Loader;
use storage::storage::Array;
use transient_crypto::commitment::{Pedersen, PedersenRandomness};
use transient_crypto::curve::{EmbeddedGroupAffine, Fr};
use transient_crypto::encryption;
use transient_crypto::merkle_tree::{MerkleTree, MerkleTreeDigest};
use transient_crypto::proofs::ProofPreimage;
use transient_crypto::repr::{FieldRepr, FromFieldRepr};

macro_rules! exptfile {
    ($name:literal, $desc:literal) => {
        (
            concat!("zswap/", midnight_ledger_static::version!(), "/", $name),
            base_crypto::data_provider::hexhash(
                &include_bytes!(concat!("../static/", $name, ".sha256"))
                    .split_at(64)
                    .0,
            ),
            $desc,
        )
    };
}

/// Files provided by Midnight's data provider for Zswap.
pub const ZSWAP_EXPECTED_FILES: &[(&str, [u8; 32], &str)] = &[
    exptfile!(
        "spend.prover",
        "zero-knowledge proving key for Zswap inputs"
    ),
    exptfile!(
        "spend.verifier",
        "zero-knowledge verifying key for Zswap inputs"
    ),
    exptfile!("spend.bzkir", "ZKIR source for Zswap inputs"),
    exptfile!(
        "output.prover",
        "zero-knowledge proving key for Zswap outputs"
    ),
    exptfile!(
        "output.verifier",
        "zero-knowledge verifying key for Zswap outputs"
    ),
    exptfile!("output.bzkir", "ZKIR source for Zswap outputs"),
    exptfile!(
        "sign.prover",
        "zero-knowledge proving key for Zswap signing operations"
    ),
    exptfile!(
        "sign.verifier",
        "zero-knowledge verifying key for Zswap signing operations"
    ),
    exptfile!("sign.bzkir", "ZKIR source for Zswap signing operations"),
];

pub(crate) const COIN_CIPHERTEXT_LEN: usize = 6;
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Storable)]
#[storable(base)]
pub struct CoinCiphertext {
    pub c: EmbeddedGroupAffine,
    pub ciph: [Fr; COIN_CIPHERTEXT_LEN],
}

impl Tagged for CoinCiphertext {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("zswap-coin-ciphertext[v1]")
    }
    fn tag_unique_factor() -> String {
        format!("(embedded-group-affine[v1],array(fr-bls,{COIN_CIPHERTEXT_LEN}))")
    }
}
tag_enforcement_test!(CoinCiphertext);

impl Serializable for CoinCiphertext {
    fn serialize(&self, writer: &mut impl std::io::Write) -> Result<(), std::io::Error> {
        <EmbeddedGroupAffine as Serializable>::serialize(&self.c, writer)?;
        // Because this is unversioned we need not send COIN_CIPHERTEXT_LEN
        for elem in self.ciph {
            <Fr as Serializable>::serialize(&elem, writer)?;
        }
        Ok(())
    }

    fn serialized_size(&self) -> usize {
        EmbeddedGroupAffine::serialized_size(&self.c)
            + self
                .ciph
                .iter()
                .map(Serializable::serialized_size)
                .sum::<usize>()
    }
}

impl Deserializable for CoinCiphertext {
    fn deserialize(
        reader: &mut impl std::io::Read,
        recursive_depth: u32,
    ) -> Result<Self, std::io::Error> {
        let c = EmbeddedGroupAffine::deserialize(reader, recursive_depth)?;
        // See note in `transient_crypto::encryption::SecretKey::decrypt` for why the point at
        // infinity is excluded.
        if c.is_infinity() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ciphertext challenge may not be the point at infinity",
            ));
        };
        let ciph = {
            let mut res = [Fr::default(); COIN_CIPHERTEXT_LEN];
            for byte in res.iter_mut() {
                *byte = Fr::deserialize(reader, recursive_depth)?;
            }
            res
        };
        Ok(Self { c, ciph })
    }
}

impl CoinCiphertext {
    pub fn new<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        pk: encryption::PublicKey,
    ) -> CoinCiphertext {
        pk.encrypt(rng, coin)
            .try_into()
            .expect("ciphertext should have ciphertext length")
    }
}

impl TryFrom<encryption::Ciphertext> for CoinCiphertext {
    type Error = ();

    fn try_from(ciph: encryption::Ciphertext) -> Result<Self, ()> {
        if ciph.ciph.len() != COIN_CIPHERTEXT_LEN {
            return Err(());
        }
        let mut arr = [0.into(); COIN_CIPHERTEXT_LEN];
        arr.copy_from_slice(&ciph.ciph);
        Ok(CoinCiphertext {
            c: ciph.c,
            ciph: arr,
        })
    }
}

impl From<CoinCiphertext> for encryption::Ciphertext {
    fn from(ciph: CoinCiphertext) -> encryption::Ciphertext {
        encryption::Ciphertext {
            c: ciph.c,
            ciph: ciph.ciph.to_vec(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serializable, Serialize)]
#[tag = "zswap-authorized-claim[v3]"]
/// A claim to a specific public key, authorized by the user's private key.
pub struct AuthorizedClaim<P> {
    pub coin: CoinInfo,
    pub recipient: CoinPublicKey,
    pub proof: Arc<P>,
}
tag_enforcement_test!(AuthorizedClaim<()>);

impl<P> AuthorizedClaim<P> {
    pub fn erase_proof(&self) -> AuthorizedClaim<()> {
        AuthorizedClaim {
            coin: self.coin,
            recipient: self.recipient,
            proof: Arc::new(()),
        }
    }
}

#[derive(Storable, Serialize)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord, Hash, Clone; P)]
#[tag = "zswap-input[v2]"]
#[storable(db = D)]
pub struct Input<P: Storable<D>, D: DB> {
    pub nullifier: Nullifier,
    pub value_commitment: Pedersen,
    pub contract_address: Option<Sp<ContractAddress, D>>,
    pub merkle_tree_root: MerkleTreeDigest,
    pub proof: Arc<P>,
}
tag_enforcement_test!(Input<(), InMemoryDB>);

impl<P> Debug for AuthorizedClaim<P> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "<claim of {} of token {:?} for recipient {:?}>",
            self.coin.value, self.coin.type_, self.recipient
        )
    }
}

impl<P: Storable<D>, D: DB> Input<P, D> {
    pub fn erase_proof(&self) -> Input<(), D> {
        Input {
            nullifier: self.nullifier,
            value_commitment: self.value_commitment,
            contract_address: self.contract_address.clone(),
            merkle_tree_root: self.merkle_tree_root,
            proof: Arc::new(()),
        }
    }
}

impl<D: DB> Input<ProofPreimage, D> {
    pub fn delta(&self) -> Delta {
        // NOTE: This is tied to the implementation in construct.rs
        // Input before last is CoinInfo
        let inputs = &self.proof.inputs;
        let coin = CoinInfo::from_field_repr(
            &inputs[inputs.len() - 1 - CoinInfo::FIELD_SIZE..inputs.len() - 1],
        )
        .expect("coin info must be correct encoded in input preimage");
        Delta {
            token_type: coin.type_,
            value: coin.value.try_into().unwrap_or(i128::MAX),
        }
    }

    pub fn binding_randomness(&self) -> PedersenRandomness {
        // NOTE: This is tied to the implementation in construct.rs
        // rc is the last input, and should be a single Fr element.
        (*self
            .proof
            .inputs
            .last()
            .expect("must have witness to extract from"))
        .try_into()
        .expect("extracted binding randomness is invalid")
    }
}

impl<P: Storable<D>, D: DB> Debug for Input<P, D> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match &self.contract_address {
            Some(addr) => write!(
                formatter,
                "<shielded input {:?} for: {:?}>",
                self.nullifier, addr
            ),
            None => write!(formatter, "<shielded input {:?}>", self.nullifier),
        }
    }
}

impl<D: DB> Input<ProofPreimage, D> {
    pub fn segment(&self) -> Option<u16> {
        self.proof
            .public_transcript_outputs
            .iter()
            .copied()
            .last()
            .map(TryInto::<u16>::try_into)
            .transpose()
            .unwrap_or(None)
    }
}

#[derive(Storable, Serialize)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord, Hash, Clone; P)]
#[tag = "zswap-output[v2]"]
#[storable(db = D)]
pub struct Output<P: Storable<D>, D: DB> {
    pub coin_com: Commitment,
    pub value_commitment: Pedersen,
    pub contract_address: Option<Sp<ContractAddress, D>>,
    pub ciphertext: Option<Sp<CoinCiphertext, D>>,
    pub proof: Arc<P>,
}
tag_enforcement_test!(Output<(), InMemoryDB>);

impl<P: Storable<D>, D: DB> Output<P, D> {
    pub fn erase_proof(&self) -> Output<(), D> {
        Output {
            coin_com: self.coin_com,
            value_commitment: self.value_commitment,
            contract_address: self.contract_address.clone(),
            ciphertext: self.ciphertext.clone(),
            proof: Arc::new(()),
        }
    }
}

impl<D: DB> Output<ProofPreimage, D> {
    pub fn delta(&self) -> Delta {
        // NOTE: This is tied to the implementation in construct.rs.
        // Input before last is CoinInfo
        let inputs = &self.proof.inputs;
        let coin = CoinInfo::from_field_repr(
            &inputs[inputs.len() - 1 - CoinInfo::FIELD_SIZE..inputs.len() - 1],
        )
        .expect("coin info must be correct encoded in input preimage");
        Delta {
            token_type: coin.type_,
            value: coin.value.try_into().unwrap_or(i128::MAX).saturating_neg(),
        }
    }

    pub fn binding_randomness(&self) -> PedersenRandomness {
        // NOTE: This is tied to the implementation in construct.rs.
        // rc is the last input, and should be a single Fr element.
        // NOTE: rc negated because output commitments are subtracted
        -PedersenRandomness::try_from(
            *self
                .proof
                .inputs
                .last()
                .expect("must have witness to extract from"),
        )
        .expect("extracted binding randomness is invalid")
    }
    pub fn segment(&self) -> Option<u16> {
        self.proof
            .public_transcript_outputs
            .iter()
            .copied()
            .last()
            .map(TryInto::<u16>::try_into)
            .transpose()
            .unwrap_or(None)
    }
}

impl<P: Storable<D>, D: DB> Debug for Output<P, D> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match &self.contract_address {
            Some(addr) => write!(
                formatter,
                "<shielded output {:?} for: {:?}>",
                self.coin_com, addr
            ),
            None => write!(formatter, "<shielded output {:?}>", self.coin_com),
        }
    }
}

#[derive(Storable, Serialize)]
#[derive_where(PartialOrd, Ord, PartialEq, Eq, Clone; P)]
#[tag = "zswap-transient[v2]"]
#[storable(db = D)]
pub struct Transient<P: Storable<D>, D: DB> {
    pub nullifier: Nullifier,
    pub coin_com: Commitment,
    pub value_commitment_input: Pedersen,
    pub value_commitment_output: Pedersen,
    pub contract_address: Option<Sp<ContractAddress, D>>,
    pub ciphertext: Option<Sp<CoinCiphertext, D>>,
    pub proof_input: Arc<P>,
    pub proof_output: Arc<P>,
}
tag_enforcement_test!(Transient<(), InMemoryDB>);

impl<P: Storable<D>, D: DB> Transient<P, D> {
    pub fn erase_proof(&self) -> Transient<(), D> {
        Transient {
            nullifier: self.nullifier,
            coin_com: self.coin_com,
            value_commitment_input: self.value_commitment_input,
            value_commitment_output: self.value_commitment_output,
            contract_address: self.contract_address.clone(),
            ciphertext: self.ciphertext.clone(),
            proof_input: Arc::new(()),
            proof_output: Arc::new(()),
        }
    }
}

impl<D: DB> Transient<ProofPreimage, D> {
    pub fn binding_randomness(&self) -> PedersenRandomness {
        self.as_input().binding_randomness() + self.as_output().binding_randomness()
    }
    pub fn segment(&self) -> Option<u16> {
        self.as_input().segment()
    }
}

impl<P: Clone + Storable<D>, D: DB> Transient<P, D> {
    pub fn as_input(&self) -> Input<P, D> {
        Input {
            nullifier: self.nullifier,
            value_commitment: self.value_commitment_input,
            contract_address: self.contract_address.clone(),
            merkle_tree_root: MerkleTree::<_>::blank(ZSWAP_TREE_HEIGHT)
                .try_update_hash(0, self.coin_com.0, ())
                .expect("updating hash on non-collapsed tree should always succeed")
                .rehash()
                .root()
                .expect("rehashed tree must have root"),
            proof: self.proof_input.clone(),
        }
    }

    pub fn as_output(&self) -> Output<P, D> {
        Output {
            coin_com: self.coin_com,
            value_commitment: self.value_commitment_output,
            contract_address: self.contract_address.clone(),
            ciphertext: self.ciphertext.clone(),
            proof: self.proof_output.clone(),
        }
    }
}

impl<P: Storable<D>, D: DB> Debug for Transient<P, D> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self.contract_address.clone() {
            Some(addr) => {
                write!(
                    formatter,
                    "<shielded transient coin {:?} {:?} for: {:?}>",
                    self.coin_com, self.nullifier, addr
                )
            }
            None => write!(
                formatter,
                "<shielded transient coin {:?} {:?}>",
                self.coin_com, self.nullifier
            ),
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serializable, Serialize, Storable)]
#[storable(base)]
#[tag = "zswap-delta"]
pub struct Delta {
    pub token_type: ShieldedTokenType,
    pub value: i128,
}
tag_enforcement_test!(Delta);

#[derive(Storable)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord, Clone; P)]
#[tag = "zswap-offer[v5]"]
#[storable(db = D)]
/// A Zswap offer consists of a potentially unbalanced set of Zswap
/// inputs/outputs.
///
/// All vectors must be sorted to be valid, and `deltas` must be key-unique
/// (i.e. not contain tuples sharing their first element `(a, b)` and `(a, c)`).
/// This is to have a canonical representation while operating on sets and maps.
pub struct Offer<P: Storable<D>, D: DB> {
    /// A set of Inputs
    pub inputs: Array<Input<P, D>, D>,
    /// A set of Outputs
    pub outputs: Array<Output<P, D>, D>,
    /// A set of "transient" Zswap coins: Coins that are created and spent in
    /// the same transaction
    pub transient: Array<Transient<P, D>, D>,
    /// A map from types (coin colors) to the offer value in this type.
    /// A positive value means more coins have been spent, a negative value
    /// means more coins were created.
    pub deltas: Array<Delta, D>,
}
tag_enforcement_test!(Offer<(), InMemoryDB>);

impl<D: DB> Offer<ProofPreimage, D> {
    pub fn binding_randomness(&self) -> PedersenRandomness {
        self.inputs
            .iter()
            .map(|i| i.binding_randomness())
            .chain(self.outputs.iter().map(|o| o.binding_randomness()))
            .chain(self.transient.iter().map(|t| t.binding_randomness()))
            .fold(0.into(), |a, b| a + b)
    }
}

impl<P: Storable<D>, D: DB> Offer<P, D> {
    pub fn erase_proofs(&self) -> Offer<(), D> {
        Offer {
            inputs: self.inputs.iter_deref().map(Input::erase_proof).collect(),
            outputs: self.outputs.iter_deref().map(Output::erase_proof).collect(),
            transient: self
                .transient
                .iter_deref()
                .map(Transient::erase_proof)
                .collect(),
            deltas: self.deltas.clone(),
        }
    }
}

impl<P: Storable<D>, D: DB> Debug for Offer<P, D> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_map()
            .entry(&Symbol("inputs"), &self.inputs)
            .entry(&Symbol("outputs"), &self.outputs)
            .entry(&Symbol("transient"), &self.transient)
            .entry(
                &Symbol("deltas"),
                &self
                    .deltas
                    .iter_deref()
                    .cloned()
                    .map(DebugDelta)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

struct DebugDelta(Delta);

impl Debug for DebugDelta {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{:?} -> {:?}", self.0.token_type, self.0.value)
    }
}

pub fn normalize_deltas<T: Ord, I: Iterator<Item = (T, i128)>>(deltas: I) -> Vec<(T, i128)> {
    let mut new_deltas: Vec<_> = deltas
        .fold(BTreeMap::new(), |mut map, (k, v)| {
            *map.entry(k).or_insert(0) += v;
            map
        })
        .into_iter()
        .collect();
    new_deltas.retain(|(_, v)| *v != 0);
    new_deltas.sort();
    new_deltas
}

impl<P: Clone + Ord + Storable<D>, D: DB> Offer<P, D> {
    pub fn normalize(&mut self) {
        self.inputs = self.inputs.iter_deref().sorted().cloned().collect();
        self.outputs = self.outputs.iter_deref().sorted().cloned().collect();
        self.transient = self.transient.iter_deref().sorted().cloned().collect();
        self.deltas = normalize_deltas(
            self.deltas
                .iter_deref()
                .map(|delta| (delta.token_type, delta.value)),
        )
        .into_iter()
        .map(|(token_type, value)| Delta { token_type, value })
        .collect();
    }

    #[instrument(skip(self, other))]
    pub fn merge(&self, other: &Self) -> Result<Self, MalformedOffer> {
        #[allow(clippy::mutable_key_type)]
        let inputs1: BTreeSet<_> = self.inputs.iter_deref().cloned().collect();
        #[allow(clippy::mutable_key_type)]
        let inputs2: BTreeSet<_> = other.inputs.iter_deref().cloned().collect();
        #[allow(clippy::mutable_key_type)]
        let outputs1: BTreeSet<_> = self.outputs.iter_deref().cloned().collect();
        #[allow(clippy::mutable_key_type)]
        let outputs2: BTreeSet<_> = other.outputs.iter_deref().cloned().collect();
        #[allow(clippy::mutable_key_type)]
        let transient1: BTreeSet<_> = self.transient.iter_deref().cloned().collect();
        #[allow(clippy::mutable_key_type)]
        let transient2: BTreeSet<_> = other.transient.iter_deref().cloned().collect();
        if inputs1.is_disjoint(&inputs2)
            && outputs1.is_disjoint(&outputs2)
            && transient1.is_disjoint(&transient2)
        {
            let mut res = Offer {
                inputs: inputs1.into_iter().chain(inputs2.into_iter()).collect(),
                outputs: outputs1.into_iter().chain(outputs2.into_iter()).collect(),
                transient: transient1
                    .iter()
                    .chain(transient2.iter())
                    .cloned()
                    .collect(),
                deltas: self
                    .deltas
                    .iter_deref()
                    .chain(other.deltas.iter_deref())
                    .cloned()
                    .collect(),
            };
            res.normalize();
            Ok(res)
        } else {
            warn!("overlap in coins attempted to merge");
            Err(MalformedOffer::NonDisjointCoinMerge)
        }
    }
}

struct Symbol(&'static str);

impl Debug for Symbol {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

pub const INPUT_PIS: usize = 68;
pub const INPUT_PROOF_SIZE: usize = 4_832;
pub const OUTPUT_PIS: usize = 77;
pub const OUTPUT_PROOF_SIZE: usize = 4_832;
pub const AUTHORIZED_CLAIM_PIS: usize = 13;

// -----------------------------------------------------------------------------
// Proptest generators.
//
// These sample proof-erased (`P = ()`) zswap structures. The `Offer` generator
// produces a value in *normal form* (sorted inputs/outputs/transient; deltas
// strictly increasing by token type with non-zero values) so it is accepted by
// `Offer::<(), D>::well_formed` by construction — the property tests then perturb
// it adversarially for the rejection cases.
// -----------------------------------------------------------------------------
#[cfg(feature = "proptest")]
mod proptest_generators {
    use super::*;
    use rand::distributions::{Distribution, Standard};

    impl Distribution<CoinCiphertext> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> CoinCiphertext {
            CoinCiphertext {
                c: rng.r#gen(),
                ciph: rng.r#gen(),
            }
        }
    }

    impl Distribution<Delta> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Delta {
            // Non-zero value: a zero delta is rejected by `well_formed`.
            let mut value: i128 = rng.r#gen();
            if value == 0 {
                value = 1;
            }
            Delta {
                token_type: rng.r#gen(),
                value,
            }
        }
    }

    impl<D: DB> Distribution<Input<(), D>> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Input<(), D> {
            Input {
                nullifier: rng.r#gen(),
                value_commitment: Pedersen(rng.r#gen()),
                contract_address: if rng.r#gen::<bool>() {
                    Some(Sp::new(rng.r#gen()))
                } else {
                    None
                },
                merkle_tree_root: rng.r#gen(),
                proof: Arc::new(()),
            }
        }
    }

    impl<D: DB> Distribution<Output<(), D>> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Output<(), D> {
            Output {
                coin_com: rng.r#gen(),
                value_commitment: Pedersen(rng.r#gen()),
                contract_address: if rng.r#gen::<bool>() {
                    Some(Sp::new(rng.r#gen()))
                } else {
                    None
                },
                ciphertext: if rng.r#gen::<bool>() {
                    Some(Sp::new(rng.r#gen()))
                } else {
                    None
                },
                proof: Arc::new(()),
            }
        }
    }

    impl<D: DB> Distribution<Transient<(), D>> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Transient<(), D> {
            Transient {
                nullifier: rng.r#gen(),
                coin_com: rng.r#gen(),
                value_commitment_input: Pedersen(rng.r#gen()),
                value_commitment_output: Pedersen(rng.r#gen()),
                contract_address: if rng.r#gen::<bool>() {
                    Some(Sp::new(rng.r#gen()))
                } else {
                    None
                },
                ciphertext: if rng.r#gen::<bool>() {
                    Some(Sp::new(rng.r#gen()))
                } else {
                    None
                },
                proof_input: Arc::new(()),
                proof_output: Arc::new(()),
            }
        }
    }

    impl<D: DB> Distribution<Offer<(), D>> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Offer<(), D> {
            let mut inputs: std::vec::Vec<Input<(), D>> =
                (0..rng.gen_range(0..=3)).map(|_| rng.r#gen()).collect();
            let mut outputs: std::vec::Vec<Output<(), D>> =
                (0..rng.gen_range(0..=3)).map(|_| rng.r#gen()).collect();
            let mut transient: std::vec::Vec<Transient<(), D>> =
                (0..rng.gen_range(0..=2)).map(|_| rng.r#gen()).collect();
            inputs.sort();
            outputs.sort();
            transient.sort();

            // Deltas must be strictly increasing by token type (unique) and
            // non-zero — build from a deduped, sorted set of token types.
            let mut tokens: std::vec::Vec<ShieldedTokenType> =
                (0..rng.gen_range(0..=3)).map(|_| rng.r#gen()).collect();
            tokens.sort();
            tokens.dedup();
            let deltas: std::vec::Vec<Delta> = tokens
                .into_iter()
                .map(|token_type| {
                    let mut value: i128 = rng.r#gen();
                    if value == 0 {
                        value = 1;
                    }
                    Delta { token_type, value }
                })
                .collect();

            Offer {
                inputs: inputs.into(),
                outputs: outputs.into(),
                transient: transient.into(),
                deltas: deltas.into(),
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Area A: zswap `Offer` / `Input` / `Output` / `Transient` + `binding_randomness`.
//
// Two property families:
//   (A-tot) Totality: decoding arbitrary bytes never panics, and derivations
//           reachable from decoded values (notably `binding_randomness`) do not
//           panic / over/underflow.
//   (A-inv) Invariant preservation: an `Offer` obtained from `deserialize`
//           satisfies the same well-formedness (`Offer::well_formed`: sorted
//           inputs/outputs/transient, strictly-increasing unique delta token
//           types, non-zero delta values) that the checked path enforces.
//
// The `Storable` derive gives these types a binary `Serializable` /
// `Deserializable` (through `Sp`), so `serialize` then `deserialize` is a
// faithful decode. Neither the `Array` decoder nor `check_invariant` (default
// `Ok`) re-establishes normal form, so a hand-built non-normal `Offer`
// round-trips unchanged and is accepted by `deserialize` while `well_formed`
// rejects it. `binding_randomness` (on `P = ProofPreimage`) reads
// `proof.inputs.last()` and `EmbeddedFr::try_from` it, both via `expect`, so an
// empty or out-of-range witness vector panics.
// -----------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod area_a_props {
    use super::*;
    use proptest::prelude::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use serialize::{Deserializable, Serializable};
    use storage::db::InMemoryDB;
    use transient_crypto::proofs::{KeyLocation, ProofPreimage};

    type D = InMemoryDB;

    fn roundtrip<T: Serializable + Deserializable>(v: &T) -> std::io::Result<T> {
        let mut bytes = std::vec::Vec::new();
        v.serialize(&mut bytes)?;
        T::deserialize(&mut &bytes[..], 0)
    }

    /// A `ProofPreimage` with a chosen `inputs` vector and otherwise-trivial
    /// fields; the other fields are irrelevant to `binding_randomness`.
    fn preimage_with_inputs(inputs: std::vec::Vec<Fr>) -> ProofPreimage {
        ProofPreimage {
            inputs,
            private_transcript: std::vec::Vec::new(),
            public_transcript_inputs: std::vec::Vec::new(),
            public_transcript_outputs: std::vec::Vec::new(),
            binding_input: Fr::from(0u64),
            communications_commitment: None,
            key_location: KeyLocation(std::borrow::Cow::Borrowed("")),
        }
    }

    fn input_with_preimage(pre: ProofPreimage) -> Input<ProofPreimage, D> {
        let mut rng = StdRng::seed_from_u64(1);
        Input {
            nullifier: rng.r#gen(),
            value_commitment: Pedersen(rng.r#gen()),
            contract_address: None,
            merkle_tree_root: rng.r#gen(),
            proof: Arc::new(pre),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        // (A-tot) Arbitrary bytes never panic the proof-erased decoders.
        #[test]
        fn offer_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
            let _ = <Offer<(), D> as Deserializable>::deserialize(&mut &bytes[..], 0);
        }
        #[test]
        fn input_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
            let _ = <Input<(), D> as Deserializable>::deserialize(&mut &bytes[..], 0);
        }
        #[test]
        fn output_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
            let _ = <Output<(), D> as Deserializable>::deserialize(&mut &bytes[..], 0);
        }
        #[test]
        fn transient_decode_total(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
            let _ = <Transient<(), D> as Deserializable>::deserialize(&mut &bytes[..], 0);
        }

        // Decoded witnesses are untrusted: extraction must never panic (an
        // unusable witness yields the default instead of aborting).
        #[test]
        fn input_binding_randomness_never_panics(pre in any::<ProofPreimage>()) {
            let decoded = roundtrip(&pre).expect("ProofPreimage round-trips");
            let input = input_with_preimage(decoded);
            let _ = input.binding_randomness();
        }

        // The default is only a failure fallback: an honest witness (whose last
        // element is the binding randomness) must recover it exactly, so the
        // fallback stays unreachable on honestly-produced inputs.
        #[test]
        fn input_binding_randomness_recovers_honest_scalar(
            rc in any::<PedersenRandomness>(),
            prefix_len in 0usize..4,
        ) {
            let mut inputs: std::vec::Vec<Fr> =
                (0..prefix_len).map(|i| Fr::from(i as u64 + 1)).collect();
            inputs.push(Fr::try_from(rc).expect("embedded scalar embeds into the base field"));
            let input = input_with_preimage(preimage_with_inputs(inputs));
            prop_assert_eq!(input.binding_randomness(), rc);
        }
    }

}

// -----------------------------------------------------------------------------
// Area B: `Offer::merge` and delta normalization.
//
// `merge` (structure.rs:579) concatenates the two offers' `deltas` and then
// calls `normalize` -> `normalize_deltas` (structure.rs:551), which sums the
// `i128` values of same-`token_type` deltas via an unchecked
//     *map.entry(k).or_insert(0) += v;   (structure.rs:554)
// There is no checked/saturating guard, so summing two same-token deltas whose
// magnitudes exceed the `i128` range overflows: **panic in debug builds**
// (`attempt to add with overflow`), **silent wraparound in release** — the
// latter yields a merged `Offer` whose delta (and hence value-balance
// commitment) is arithmetically wrong, yet still passes `well_formed`
// (verify.rs:320-326 checks only sortedness, strict token ordering, and
// non-zero).
//
// Properties asserted here:
//   * (B-tot)  merge never panics for deltas within a non-overflowing range.
//   * (B-inv)  the merged offer's deltas are in normal form: strictly
//              increasing by `token_type` (=> sorted + key-unique) and all
//              non-zero, matching what `normalize_deltas` guarantees.
// The regressions below pin the overflow (B-tot violated at the i128
// boundary).
// -----------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod area_b_merge_props {
    use super::*;
    use proptest::prelude::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use storage::db::InMemoryDB;

    type D = InMemoryDB;

    /// A small fixed pool of distinct token types, so generated deltas collide
    /// on `token_type` often enough to exercise the summation path in
    /// `normalize_deltas`.
    fn token_pool() -> Vec<ShieldedTokenType> {
        let mut rng = StdRng::seed_from_u64(0xB0B0);
        (0..4).map(|_| rng.r#gen()).collect()
    }

    /// Build a deltas-only offer (empty input/output/transient sets, so `merge`
    /// never trips the disjointness check).
    fn deltas_offer(deltas: Vec<Delta>) -> Offer<(), D> {
        Offer {
            inputs: std::vec::Vec::new().into(),
            outputs: std::vec::Vec::new().into(),
            transient: std::vec::Vec::new().into(),
            deltas: deltas.into(),
        }
    }

    /// Turn a list of `(pool_index, value)` into `Delta`s (dropping zeros so the
    /// inputs are themselves plausible partial deltas).
    fn make_deltas(pool: &[ShieldedTokenType], raw: &[(u8, i64)]) -> Vec<Delta> {
        raw.iter()
            .filter(|(_, v)| *v != 0)
            .map(|(i, v)| Delta {
                token_type: pool[(*i as usize) % pool.len()],
                value: *v as i128,
            })
            .collect()
    }

    fn deltas_vec(offer: &Offer<(), D>) -> Vec<Delta> {
        offer.deltas.iter_deref().cloned().collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        // (B-tot)+(B-inv): merging two deltas-only offers whose values live in
        // the i64 range never panics, and the result is in delta normal form.
        #[test]
        fn merge_preserves_delta_normal_form(
            a in prop::collection::vec((any::<u8>(), any::<i64>()), 0..6),
            b in prop::collection::vec((any::<u8>(), any::<i64>()), 0..6),
        ) {
            let pool = token_pool();
            let oa = deltas_offer(make_deltas(&pool, &a));
            let ob = deltas_offer(make_deltas(&pool, &b));
            let merged = oa.merge(&ob).expect("disjoint (empty) coin sets merge");
            let ds = deltas_vec(&merged);
            // strictly increasing by token_type => sorted AND key-unique.
            for w in ds.windows(2) {
                prop_assert!(
                    w[0].token_type < w[1].token_type,
                    "merged deltas not strictly increasing by token_type"
                );
            }
            // all non-zero.
            prop_assert!(ds.iter().all(|d| d.value != 0), "merged deltas contain a zero");
            // and it agrees with the reference normalization of the concatenation.
            let mut concat: Vec<(ShieldedTokenType, i128)> =
                make_deltas(&pool, &a).into_iter().chain(make_deltas(&pool, &b))
                    .map(|d| (d.token_type, d.value)).collect();
            concat = normalize_deltas(concat.into_iter());
            let got: Vec<(ShieldedTokenType, i128)> =
                ds.iter().map(|d| (d.token_type, d.value)).collect();
            prop_assert_eq!(got, concat, "merge disagrees with normalize_deltas");
        }

        // (B-tot): merge is commutative on the delta set (normal form is order
        // independent) — a sanity property that holds today.
        #[test]
        fn merge_delta_set_is_commutative(
            a in prop::collection::vec((any::<u8>(), any::<i64>()), 0..6),
            b in prop::collection::vec((any::<u8>(), any::<i64>()), 0..6),
        ) {
            let pool = token_pool();
            let oa = deltas_offer(make_deltas(&pool, &a));
            let ob = deltas_offer(make_deltas(&pool, &b));
            let as_pairs = |o: &Offer<(), D>| -> Vec<(ShieldedTokenType, i128)> {
                deltas_vec(o).iter().map(|d| (d.token_type, d.value)).collect()
            };
            let ab = as_pairs(&oa.merge(&ob).unwrap());
            let ba = as_pairs(&ob.merge(&oa).unwrap());
            prop_assert_eq!(ab, ba);
        }
    }

    // Merging same-token deltas at the i128 extremes saturates rather than
    // wrapping to the opposite sign.

    #[test]
    fn regression_merge_delta_overflow() {
        let tt = token_pool()[0];
        let a = deltas_offer(vec![Delta { token_type: tt, value: i128::MAX }]);
        let b = deltas_offer(vec![Delta { token_type: tt, value: i128::MAX }]);
        let merged = a.merge(&b).expect("empty coin sets merge");
        let vals: Vec<i128> = deltas_vec(&merged).iter().map(|d| d.value).collect();
        assert_eq!(vals, vec![i128::MAX]);
    }

    #[test]
    fn regression_merge_delta_underflow() {
        let tt = token_pool()[0];
        let a = deltas_offer(vec![Delta { token_type: tt, value: i128::MIN }]);
        let b = deltas_offer(vec![Delta { token_type: tt, value: i128::MIN }]);
        let merged = a.merge(&b).expect("empty coin sets merge");
        let vals: Vec<i128> = deltas_vec(&merged).iter().map(|d| d.value).collect();
        assert_eq!(vals, vec![i128::MIN]);
    }
}
