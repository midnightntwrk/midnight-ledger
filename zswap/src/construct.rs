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

use crate::compact_slots::*;
use crate::error::OfferCreationFailed;
use crate::filter_invalid;
use crate::structure::*;
use crate::{ZSWAP_TREE_HEIGHT, ciphertext_to_field};
use base_crypto::fab::AlignedValue;
use coin_structure::coin::{
    self, Commitment, Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo,
    SecretKey as CoinSecretKey,
};
use coin_structure::contract::ContractAddress;
use coin_structure::transfer::{Recipient, SenderEvidence};
use midnight_onchain_runtime::ops::{Key, Op};
use midnight_onchain_runtime::program_fragments::*;
use midnight_onchain_runtime::result_mode::ResultMode;
use midnight_onchain_runtime::result_mode::ResultModeGather;
use midnight_onchain_runtime::result_mode::ResultModeVerify;
use midnight_onchain_runtime::state::{ContractOperation, StateValue};
use rand::{CryptoRng, Rng};
use serialize::Deserializable;
use serialize::Serializable;
use std::borrow::Cow;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Arc;
use storage::Storable;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use storage::storage::default_storage;
use transient_crypto::commitment::Pedersen;
use transient_crypto::curve::{EmbeddedFr, Fr};
use transient_crypto::encryption;
use transient_crypto::hash::transient_commit;
use transient_crypto::merkle_tree::MerkleTree;
use transient_crypto::proofs::{KeyLocation, ProofPreimage};
use transient_crypto::repr::FieldRepr;

/// Field-encodes a transcript program, dropping the ops that carry no information.
fn transcript_field_repr<M: ResultMode<D>, D: DB>(ops: impl Iterator<Item = Op<M, D>>) -> Vec<Fr>
where
    Op<M, D>: FieldRepr,
{
    let mut field_repr = Vec::new();
    for op in filter_invalid(ops) {
        op.field_repr(&mut field_repr);
    }
    field_repr
}

/// The trailing public transcript fragment shared by Zswap spends and outputs: a read of
/// the segment cell, followed by the write of the value commitment bound to that segment.
///
/// `retarget_preimage` rewrites exactly this fragment in place, so the two must stay in
/// step; emitting it from one place is what keeps them that way.
fn segment_binding_transcript<D: DB>(
    segment: u16,
    value_commitment: Pedersen,
) -> Vec<Op<ResultModeVerify, D>> {
    let mut prog = Vec::new();
    prog.extend(
        Cell_read!([Key::Value(ZSWAP_IDX_SEGMENT.into())], false, u16)
            .into_iter()
            .map(|op: Op<ResultModeGather, _>| op.translate(|()| segment.into())),
    );
    prog.extend(Cell_write!(
        [Key::Value(ZSWAP_IDX_VALUE_COM.into())],
        false,
        (Fr, Fr),
        value_commitment.0
    ));
    prog
}

/// Rebuilds the parts of a proof preimage that reference the segment: the trailing
/// transcript fragment emitted by `segment_binding_transcript`, and the transcript
/// outputs, which differs between spends and outputs and so are supplied by the caller.
fn retarget_preimage<D: DB>(
    proof: &ProofPreimage,
    new_segment: u16,
    value_commitment: Pedersen,
    public_transcript_outputs: Vec<Fr>,
) -> ProofPreimage {
    let tail = transcript_field_repr(
        segment_binding_transcript::<D>(new_segment, value_commitment).into_iter(),
    );
    let mut proof_preimage = proof.clone();
    let len = proof_preimage.public_transcript_inputs.len();
    proof_preimage.public_transcript_inputs[len - tail.len()..len].copy_from_slice(&tail);
    proof_preimage.public_transcript_outputs = public_transcript_outputs;
    proof_preimage
}

impl AuthorizedClaim<ProofPreimage> {
    #[instrument(skip(_rng))]
    pub fn new<R: Rng + CryptoRng + ?Sized, D: DB>(
        _rng: &mut R,
        coin: CoinInfo,
        sk: &CoinSecretKey,
    ) -> Result<Self, OfferCreationFailed> {
        let pk = match Recipient::from(SenderEvidence::User(Cow::Borrowed(sk))) {
            Recipient::User(pk) => pk,
            Recipient::Contract(_) => unreachable!(),
        };
        let public_transcript_prog: &[Op<ResultModeVerify, D>] = &Cell_write!(
            [Key::Value(ZSWAP_IDX_PUBLIC_KEY.into())],
            false,
            CoinPublicKey,
            pk
        );
        // Exact capacity: reallocating while appending would leave copies of the
        // secret witness behind in freed allocations, which `ProofPreimage`'s
        // zeroize-on-drop does not reach.
        let mut inputs = Vec::with_capacity(sk.field_size());
        sk.field_repr(&mut inputs);
        let mut public_transcript_inputs = Vec::new();
        for op in filter_invalid(public_transcript_prog.iter().cloned()) {
            op.field_repr(&mut public_transcript_inputs);
        }
        let proof_preimage = ProofPreimage {
            inputs,
            private_transcript: Vec::new(),
            public_transcript_inputs,
            public_transcript_outputs: Vec::new(),
            binding_input: transient_commit(&coin, 0u8.into()),
            communications_commitment: None,
            key_location: KeyLocation(Cow::Borrowed("midnight/zswap/sign")),
        };
        Ok(AuthorizedClaim {
            coin,
            recipient: pk,
            proof: Arc::new(proof_preimage),
        })
    }
}

impl<D: DB> Input<ProofPreimage, D> {
    #[instrument(skip(rng))]
    pub fn new_contract_owned<A: Debug + Storable<D>, R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: Option<u16>,
        contract: ContractAddress,
        tree: &MerkleTree<A, D>,
    ) -> Result<Self, OfferCreationFailed> {
        Self::new_from_secret_key::<A, R>(
            rng,
            coin,
            segment,
            SenderEvidence::Contract(contract),
            tree,
        )
    }

    pub fn retarget_segment(&self, new_segment: u16) -> Self {
        // We redo the last two parts of the transcript which reference the segment, as well as the
        // binding commitment.
        let delta = self.delta();
        let rc_e = self.binding_randomness();
        let value_commitment =
            Pedersen::commit(&(delta.token_type, new_segment), &delta.value.into(), &rc_e);
        Input {
            value_commitment,
            proof: Arc::new(retarget_preimage::<D>(
                self.proof.deref(),
                new_segment,
                value_commitment,
                vec![true.into(), new_segment.into()],
            )),
            ..self.clone()
        }
    }

    pub(crate) fn new_from_secret_key<A: Debug + Storable<D>, R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: Option<u16>,
        sk: SenderEvidence<'_>,
        tree: &MerkleTree<A, D>,
    ) -> Result<Self, OfferCreationFailed> {
        let rc_e: EmbeddedFr = rng.r#gen();
        let rc = Fr::try_from(rc_e).expect("Fr should be larger than EmbeddedFr");
        let nullifier = CoinInfo::from(coin).nullifier(&sk);
        let value_commitment = Pedersen::commit(
            &(coin.type_, segment.unwrap_or(0)),
            &coin.value.into(),
            &rc_e,
        );
        let merkle_tree_root = tree.root().ok_or(OfferCreationFailed::TreeNotRehashed)?;
        debug!("spending contract-owned coin");
        let mut public_transcript_prog: Vec<Op<ResultModeVerify, D>> = Vec::new();
        public_transcript_prog.extend(
            HistoricMerkleTree_check_root!(
                [Key::Value(ZSWAP_IDX_MERKLE_TREE.into())],
                false,
                32,
                [u8; 32],
                merkle_tree_root
            )
            .into_iter()
            .map(|op: Op<ResultModeGather, D>| op.translate(|()| true.into())),
        );
        public_transcript_prog.extend(Set_insert!(
            [Key::Value(ZSWAP_IDX_NULLIFIERS.into())],
            false,
            [u8; 32],
            nullifier
        ));
        if let SenderEvidence::Contract(addr) = &sk {
            public_transcript_prog.extend(Cell_write!(
                [Key::Value(ZSWAP_IDX_CONTRACT_ADDR.into())],
                false,
                ContractAddress,
                *addr
            ));
        }
        public_transcript_prog.extend(segment_binding_transcript::<D>(
            segment.unwrap_or(0),
            value_commitment,
        ));
        let coin_info = CoinInfo::from(coin);
        let Commitment(hash) = coin_info.commitment(&sk.clone().into());
        let path = tree
            .path_for_leaf(coin.mt_index, ((), hash))
            .map_err(OfferCreationFailed::InvalidIndex)?;
        let mut inputs =
            Vec::with_capacity(sk.field_size() + path.field_size() + coin_info.field_size() + 1);
        sk.field_repr(&mut inputs);
        path.field_repr(&mut inputs);
        coin_info.field_repr(&mut inputs);
        inputs.push(rc);
        let public_transcript_inputs = transcript_field_repr(public_transcript_prog.into_iter());
        let proof_preimage = ProofPreimage {
            inputs,
            private_transcript: Vec::new(),
            public_transcript_inputs,
            public_transcript_outputs: vec![true.into(), segment.unwrap_or(0).into()],
            binding_input: 0.into(),
            communications_commitment: None,
            key_location: KeyLocation(Cow::Borrowed("midnight/zswap/spend")),
        };
        let inp = Input {
            nullifier,
            value_commitment,
            contract_address: match sk {
                SenderEvidence::Contract(addr) => Some(Sp::new(addr)),
                _ => None,
            },
            merkle_tree_root,
            proof: Arc::new(proof_preimage),
        };
        //debug_assert!(inp.well_formed().is_ok());
        Ok(inp)
    }
}

impl<D: DB> Output<ProofPreimage, D> {
    #[instrument(skip(rng))]
    pub fn new<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        segment: Option<u16>,
        target_cpk: &coin::PublicKey,
        target_epk: Option<encryption::PublicKey>,
    ) -> Result<Self, OfferCreationFailed> {
        let ciphertext = target_epk.map(|epk| CoinCiphertext::new(rng, coin, epk));
        Self::new_with_ciphertext::<R>(rng, coin, segment, target_cpk, ciphertext)
    }

    pub fn retarget_segment(&self, new_segment: u16) -> Self {
        // We redo the last two parts of the transcript which reference the segment, as well as the
        // binding commitment.
        let delta = self.delta();
        // NOTE: both negated because `Output::delta` and `Output::binding_randomness` already
        // negate them, but we need the positive variants.
        let rc_e = -self.binding_randomness();
        let value_commitment = Pedersen::commit(
            &(delta.token_type, new_segment),
            &delta.value.saturating_neg().into(),
            &rc_e,
        );
        Output {
            value_commitment,
            proof: Arc::new(retarget_preimage::<D>(
                self.proof.deref(),
                new_segment,
                value_commitment,
                vec![new_segment.into()],
            )),
            ..self.clone()
        }
    }

    #[instrument(skip(rng))]
    pub fn new_with_ciphertext<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        segment: Option<u16>,
        target_cpk: &coin::PublicKey,
        ciph: Option<CoinCiphertext>,
    ) -> Result<Self, OfferCreationFailed> {
        Self::new_for_recipient::<R>(rng, coin, segment, Recipient::User(*target_cpk), ciph)
    }

    #[instrument(skip(rng))]
    pub fn new_contract_owned<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        segment: Option<u16>,
        contract: ContractAddress,
    ) -> Result<Self, OfferCreationFailed> {
        Self::new_for_recipient::<R>(rng, coin, segment, Recipient::Contract(contract), None)
    }

    pub(crate) fn new_for_recipient<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        segment: Option<u16>,
        recipient: Recipient,
        ciphertext: Option<CoinCiphertext>,
    ) -> Result<Self, OfferCreationFailed> {
        let rc_e: EmbeddedFr = rng.r#gen();
        let rc = Fr::try_from(rc_e).expect("Fr should be within EmbeddedFr");
        let coin_com = coin.commitment(&recipient);
        let value_commitment = Pedersen::commit(
            &(coin.type_, segment.unwrap_or(0)),
            &coin.value.into(),
            &rc_e,
        );
        debug!("creating new contract-owned output coin");
        let mut public_transcript_prog = Vec::new();
        public_transcript_prog.extend::<[Op<ResultModeVerify, InMemoryDB>; 17]>(
            HistoricMerkleTree_insert_hash!(
                [Key::Value(ZSWAP_IDX_MERKLE_TREE.into())],
                false,
                32,
                [u8; 32],
                coin_com
            ),
        );
        if let Recipient::Contract(addr) = &recipient {
            public_transcript_prog.extend(Cell_write!(
                [Key::Value(ZSWAP_IDX_CONTRACT_ADDR.into())],
                false,
                ContractAddress,
                addr
            ));
        }
        public_transcript_prog.extend(segment_binding_transcript::<InMemoryDB>(
            segment.unwrap_or(0),
            value_commitment,
        ));
        let mut inputs = Vec::with_capacity(recipient.field_size() + coin.field_size() + 1);
        recipient.field_repr(&mut inputs);
        coin.field_repr(&mut inputs);
        inputs.push(rc);
        let public_transcript_inputs = transcript_field_repr(public_transcript_prog.into_iter());
        let proof_preimage = ProofPreimage {
            inputs,
            private_transcript: Vec::new(),
            public_transcript_inputs,
            public_transcript_outputs: vec![segment.unwrap_or(0).into()],
            binding_input: match &ciphertext {
                Some(ciph) => ciphertext_to_field(ciph),
                None => 0.into(),
            },
            communications_commitment: None,
            key_location: KeyLocation(Cow::Borrowed("midnight/zswap/output")),
        };
        let outp = Output {
            coin_com,
            value_commitment,
            contract_address: match recipient {
                Recipient::Contract(addr) => Some(Sp::new(addr)),
                _ => None,
            },
            ciphertext: ciphertext.map(|x| Sp::new(x)),
            proof: Arc::new(proof_preimage),
        };
        // NOTE: rc negated because output commitments are subtracted
        Ok(outp)
    }
}

impl<D: DB> Transient<ProofPreimage, D> {
    #[instrument(skip(rng))]
    pub fn new_from_contract_owned_output<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: Option<u16>,
        output: Output<ProofPreimage, D>,
    ) -> Result<Self, OfferCreationFailed> {
        let tree = MerkleTree::<(), InMemoryDB>::blank(ZSWAP_TREE_HEIGHT)
            .try_update_hash(0, output.coin_com.0, ())
            .map_err(OfferCreationFailed::MerkleTreeError)?
            .rehash();
        let addr = output
            .contract_address
            .clone()
            .ok_or(OfferCreationFailed::NotContractOwned)?;
        let input = Input::new_contract_owned(rng, coin, segment, *addr.deref(), &tree)?;
        Ok(Transient::from_parts(input, output))
    }

    pub fn retarget_segment(&self, new_segment: u16) -> Self {
        let input = self.as_input().retarget_segment(new_segment);
        let output = self.as_output().retarget_segment(new_segment);
        Transient::from_parts(input, output)
    }
}

impl<D: DB> Offer<ProofPreimage, D> {
    pub fn new(
        inputs: Vec<Input<ProofPreimage, D>>,
        outputs: Vec<Output<ProofPreimage, D>>,
        transient: Vec<Transient<ProofPreimage, D>>,
    ) -> Option<Self> {
        if inputs.is_empty() && outputs.is_empty() && transient.is_empty() {
            return None;
        }
        let deltas = inputs
            .iter()
            .map(Input::delta)
            .chain(outputs.iter().map(Output::delta))
            .collect();
        let mut res = Offer {
            inputs: inputs.into_iter().collect(),
            outputs: outputs.into_iter().collect(),
            transient: transient.into_iter().collect(),
            deltas,
        };
        res.normalize();
        Some(res)
    }

    pub fn retarget_segment(&self, new_segment: u16) -> Self {
        Offer {
            inputs: self
                .inputs
                .iter()
                .map(|i| i.retarget_segment(new_segment))
                .collect(),
            outputs: self
                .outputs
                .iter()
                .map(|o| o.retarget_segment(new_segment))
                .collect(),
            transient: self
                .transient
                .iter()
                .map(|t| t.retarget_segment(new_segment))
                .collect(),
            deltas: self.deltas.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use coin_structure::coin::{Info as CoinInfo, PublicKey as CoinPublicKey, ShieldedTokenType};
    use rand::Rng;
    use rand::rngs::ThreadRng;
    use storage::db::InMemoryDB;
    use transient_crypto::proofs::ProofPreimage;

    use super::{Output, Transient};

    #[test]
    fn bad_transient() {
        let mut rng = rand::thread_rng();
        let coin = CoinInfo {
            type_: rng.r#gen(),
            nonce: rng.r#gen(),
            value: 10_000,
        };
        let pk = CoinPublicKey(rng.r#gen());
        let out: Output<ProofPreimage, InMemoryDB> =
            Output::new::<_>(&mut rng, &coin, None, &pk, None).unwrap();
        let trans =
            Transient::new_from_contract_owned_output(&mut rng, &coin.qualify(0), None, out);
        assert!(trans.is_err());
    }
}
