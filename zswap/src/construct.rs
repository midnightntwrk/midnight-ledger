// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

impl AuthorizedClaim<ProofPreimage> {
    #[instrument(skip(_rng))]
    pub fn new<R: Rng + CryptoRng + ?Sized, D: DB>(
        _rng: &mut R,
        coin: CoinInfo,
        sk: &CoinSecretKey,
    ) -> Result<Self, OfferCreationFailed> {
        let pk = match Recipient::from(&SenderEvidence::User(*sk)) {
            Recipient::User(pk) => pk,
            Recipient::Contract(_) => unreachable!(),
        };
        let public_transcript_prog: &[Op<ResultModeVerify, D>] =
            &Cell_write!([Key::Value(4u8.into())], false, CoinPublicKey, pk);
        let mut inputs = Vec::new();
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
            proof: proof_preimage,
        })
    }
}

impl<D: DB> Input<ProofPreimage, D> {
    #[instrument(skip(rng))]
    pub fn new_contract_owned<A: Debug + Storable<D>, R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: u16,
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

    pub(crate) fn new_from_secret_key<A: Debug + Storable<D>, R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: u16,
        sk: SenderEvidence,
        tree: &MerkleTree<A, D>,
    ) -> Result<Self, OfferCreationFailed> {
        let rc_e: EmbeddedFr = rng.r#gen();
        let rc = Fr::try_from(rc_e).expect("Fr should be larger than EmbeddedFr");
        let nullifier = CoinInfo::from(coin).nullifier(&sk);
        let value_commitment = Pedersen::commit(&(coin.type_, segment), &coin.value.into(), &rc_e);
        let merkle_tree_root = tree.root().ok_or(OfferCreationFailed::TreeNotRehashed)?;
        debug!("spending contract-owned coin");
        let mut public_transcript_prog: Vec<Op<ResultModeVerify, D>> = Vec::new();
        public_transcript_prog.extend(
            HistoricMerkleTree_check_root!(
                [Key::Value(0u8.into())],
                false,
                32,
                [u8; 32],
                merkle_tree_root
            )
            .into_iter()
            .map(|op: Op<ResultModeGather, D>| op.translate(|()| true.into())),
        );
        public_transcript_prog.extend(Set_insert!(
            [Key::Value(1u8.into())],
            false,
            [u8; 32],
            nullifier
        ));
        if let SenderEvidence::Contract(addr) = &sk {
            public_transcript_prog.extend(Cell_write!(
                [Key::Value(3u8.into())],
                false,
                ContractAddress,
                addr
            ));
        }
        public_transcript_prog.extend(
            Cell_read!([Key::Value(5u8.into())], false, u16)
                .into_iter()
                .map(|op: Op<ResultModeGather, _>| op.translate(|()| segment.into())),
        );
        public_transcript_prog.extend(Cell_write!(
            [Key::Value(2u8.into())],
            false,
            (Fr, Fr),
            value_commitment.0
        ));
        let Commitment(hash) = CoinInfo::from(coin).commitment(&(&sk).into());
        let mut inputs = Vec::new();
        sk.field_repr(&mut inputs);
        tree.path_for_leaf(coin.mt_index, ((), hash))
            .map_err(OfferCreationFailed::InvalidIndex)?
            .field_repr(&mut inputs);
        CoinInfo::from(coin).field_repr(&mut inputs);
        inputs.push(rc);
        let mut public_transcript_inputs = Vec::new();
        for op in filter_invalid(public_transcript_prog.into_iter()) {
            op.field_repr(&mut public_transcript_inputs);
        }
        let proof_preimage = ProofPreimage {
            inputs,
            private_transcript: Vec::new(),
            public_transcript_inputs,
            public_transcript_outputs: vec![true.into(), segment.into()],
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
            proof: proof_preimage,
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
        segment: u16,
        target_cpk: &coin::PublicKey,
        target_epk: Option<encryption::PublicKey>,
    ) -> Result<Self, OfferCreationFailed> {
        let ciphertext = target_epk.map(|epk| CoinCiphertext::new(rng, coin, epk));
        Self::new_with_ciphertext::<R>(rng, coin, segment, target_cpk, ciphertext)
    }

    #[instrument(skip(rng))]
    pub fn new_with_ciphertext<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        segment: u16,
        target_cpk: &coin::PublicKey,
        ciph: Option<CoinCiphertext>,
    ) -> Result<Self, OfferCreationFailed> {
        Self::new_for_recipient::<R>(rng, coin, segment, Recipient::User(*target_cpk), ciph)
    }

    #[instrument(skip(rng))]
    pub fn new_contract_owned<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        segment: u16,
        contract: ContractAddress,
    ) -> Result<Self, OfferCreationFailed> {
        Self::new_for_recipient::<R>(rng, coin, segment, Recipient::Contract(contract), None)
    }

    pub(crate) fn new_for_recipient<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &CoinInfo,
        segment: u16,
        recipient: Recipient,
        ciphertext: Option<CoinCiphertext>,
    ) -> Result<Self, OfferCreationFailed> {
        let rc_e: EmbeddedFr = rng.r#gen();
        let rc = Fr::try_from(rc_e).expect("Fr should be within EmbeddedFr");
        let coin_com = coin.commitment(&recipient);
        let value_commitment = Pedersen::commit(&(coin.type_, segment), &coin.value.into(), &rc_e);
        debug!("creating new contract-owned output coin");
        let mut public_transcript_prog = Vec::new();
        public_transcript_prog.extend::<[Op<ResultModeVerify, InMemoryDB>; 17]>(
            HistoricMerkleTree_insert_hash!(
                [Key::Value(0u8.into())],
                false,
                32,
                [u8; 32],
                coin_com
            ),
        );
        if let Recipient::Contract(addr) = &recipient {
            public_transcript_prog.extend(Cell_write!(
                [Key::Value(3u8.into())],
                false,
                ContractAddress,
                addr
            ));
        }
        public_transcript_prog.extend(
            Cell_read!([Key::Value(5u8.into())], false, u16)
                .into_iter()
                .map(|op: Op<ResultModeGather, _>| op.translate(|()| segment.into())),
        );
        public_transcript_prog.extend(Cell_write!(
            [Key::Value(2u8.into())],
            false,
            (Fr, Fr),
            value_commitment.0
        ));
        let mut inputs = Vec::new();
        recipient.field_repr(&mut inputs);
        coin.field_repr(&mut inputs);
        inputs.push(rc);
        let mut public_transcript_inputs = Vec::new();
        for op in filter_invalid(public_transcript_prog.into_iter()) {
            op.field_repr(&mut public_transcript_inputs);
        }
        let proof_preimage = ProofPreimage {
            inputs,
            private_transcript: Vec::new(),
            public_transcript_inputs,
            public_transcript_outputs: vec![segment.into()],
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
            proof: proof_preimage,
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
        segment: u16,
        output: Output<ProofPreimage, D>,
    ) -> Result<Self, OfferCreationFailed> {
        let tree = MerkleTree::<(), InMemoryDB>::blank(ZSWAP_TREE_HEIGHT)
            .update_hash(0, output.coin_com.0, ())
            .rehash();
        let addr = output
            .contract_address
            .clone()
            .ok_or(OfferCreationFailed::NotContractOwned)?;
        let input = Input::new_contract_owned(rng, coin, segment, *addr.deref(), &tree)?;
        let io = Transient {
            nullifier: input.nullifier,
            coin_com: output.coin_com,
            value_commitment_input: input.value_commitment,
            value_commitment_output: output.value_commitment,
            contract_address: output.contract_address,
            ciphertext: output.ciphertext,
            proof_input: input.proof,
            proof_output: output.proof,
        };
        Ok(io)
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
            Output::new::<_>(&mut rng, &coin, 0, &pk, None).unwrap();
        let trans = Transient::new_from_contract_owned_output(&mut rng, &coin.qualify(0), 0, out);
        assert!(trans.is_err());
    }
}
