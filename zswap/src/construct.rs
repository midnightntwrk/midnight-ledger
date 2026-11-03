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

use crate::error::OfferCreationFailed;
use crate::filter_invalid;
use crate::structure::*;
use crate::{ZSWAP_TREE_HEIGHT, ciphertext_to_field};
use base_crypto::fab::AlignedValue;
use base_crypto::hash::HashOutput;
use coin_structure::coin::ShieldedTokenType;
use coin_structure::coin::{
    self, Commitment, Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo,
    SecretKey as CoinSecretKey,
};
use coin_structure::contract::ContractAddress;
use coin_structure::custom_spend::{ShieldedTokenPreimage, SpendRule};
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
use transient_crypto::curve::{EmbeddedFr, EmbeddedGroupAffine, Fr};
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
        let pk = match Recipient::from(SenderEvidence::User(Cow::Borrowed(sk))) {
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
            inner_proofs: vec![],
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
    /// Spends a coin a contract holds. `preimage` is the preimage of the
    /// coin's type, which the spend re-derives; `guarded` selects the guarded
    /// spend circuit, which a coin whose type has a rule requires.
    #[instrument(skip(rng, guarded))]
    pub fn new_contract_owned<A: Debug + Storable<D>, R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: Option<u16>,
        contract: ContractAddress,
        tree: &MerkleTree<A, D>,
        preimage: &ShieldedTokenPreimage,
        guarded: Option<GuardedSpend>,
    ) -> Result<Self, OfferCreationFailed> {
        Self::new_from_secret_key::<A, R>(
            rng,
            coin,
            segment,
            SenderEvidence::Contract(contract),
            tree,
            preimage,
            guarded,
        )
    }

    pub fn retarget_segment(&self, new_segment: u16) -> Self {
        // We redo the last two parts of the transcript which reference the segment, as well as the
        // binding commitment.
        let delta = self.delta();
        let rc_e = self.binding_randomness();
        let value_commitment =
            Pedersen::commit(&(delta.token_type, new_segment), &delta.value.into(), &rc_e);
        let mut public_transcript_prog = Vec::<Op<ResultModeVerify, D>>::new();
        public_transcript_prog.extend(
            Cell_read!([Key::Value(5u8.into())], false, u16)
                .into_iter()
                .map(|op: Op<ResultModeGather, _>| op.translate(|()| new_segment.into())),
        );
        public_transcript_prog.extend(Cell_write!(
            [Key::Value(2u8.into())],
            false,
            (Fr, Fr),
            value_commitment.0
        ));
        let mut public_transcript_inputs_end = Vec::new();
        for op in filter_invalid(public_transcript_prog.into_iter()) {
            op.field_repr(&mut public_transcript_inputs_end);
        }

        let mut proof_preimage = self.proof.deref().clone();
        let len = proof_preimage.public_transcript_inputs.len();
        proof_preimage.public_transcript_inputs[len - public_transcript_inputs_end.len()..len]
            .copy_from_slice(&public_transcript_inputs_end);
        // The segment is the last output; the guarded circuit's reserved
        // inputs precede it.
        if let Some(segment) = proof_preimage.public_transcript_outputs.last_mut() {
            *segment = new_segment.into();
        }

        Input {
            value_commitment,
            proof: Arc::new(proof_preimage),
            ..self.clone()
        }
    }

    pub(crate) fn new_from_secret_key<A: Debug + Storable<D>, R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: Option<u16>,
        sk: SenderEvidence<'_>,
        tree: &MerkleTree<A, D>,
        preimage: &ShieldedTokenPreimage,
        guarded: Option<GuardedSpend>,
    ) -> Result<Self, OfferCreationFailed> {
        let extra_inputs = spend_extra_inputs(&coin.type_, preimage, guarded.as_ref())?;
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
                *addr
            ));
        }
        let mut public_transcript_outputs: Vec<Fr> = vec![true.into()];
        if guarded.is_some() {
            // The ledger supplies the guard's reserved inputs, as it supplies
            // the segment; version 1 guards receive zeros.
            public_transcript_prog.extend(
                Cell_read!([Key::Value(RESERVED_SLOT.into())], false, (Fr, Fr, Fr, Fr))
                    .into_iter()
                    .map(|op: Op<ResultModeGather, _>| op.translate(|()| reserved_value())),
            );
            public_transcript_outputs.extend([Fr::from(0u64); 4]);
        }
        public_transcript_prog.extend(
            Cell_read!([Key::Value(5u8.into())], false, u16)
                .into_iter()
                .map(|op: Op<ResultModeGather, _>| op.translate(|()| segment.unwrap_or(0).into())),
        );
        public_transcript_outputs.push(segment.unwrap_or(0).into());
        public_transcript_prog.extend(Cell_write!(
            [Key::Value(2u8.into())],
            false,
            (Fr, Fr),
            value_commitment.0
        ));
        let Commitment(hash) = CoinInfo::from(coin).commitment(&sk.clone().into());
        let mut inputs = Vec::new();
        sk.field_repr(&mut inputs);
        tree.path_for_leaf(coin.mt_index, ((), hash))
            .map_err(OfferCreationFailed::InvalidIndex)?
            .field_repr(&mut inputs);
        inputs.extend(extra_inputs);
        CoinInfo::from(coin).field_repr(&mut inputs);
        inputs.push(rc);
        let mut public_transcript_inputs = Vec::new();
        for op in filter_invalid(public_transcript_prog.into_iter()) {
            op.field_repr(&mut public_transcript_inputs);
        }
        let named_output = guarded
            .as_ref()
            .and_then(|g| g.output.map(|out| out.commitment(&g.recipient.as_recipient())));
        let proof_preimage = ProofPreimage {
            inputs,
            private_transcript: Vec::new(),
            public_transcript_inputs,
            public_transcript_outputs,
            inner_proofs: guarded
                .as_ref()
                .and_then(|g| g.guard.as_ref())
                .map(GuardProof::inner_proofs)
                .unwrap_or_default(),
            binding_input: 0.into(),
            communications_commitment: None,
            key_location: KeyLocation(Cow::Borrowed(if guarded.is_some() {
                GUARDED_SPEND_KEY_LOCATION
            } else {
                PLAIN_SPEND_KEY_LOCATION
            })),
        };
        let inp = Input {
            nullifier,
            value_commitment,
            contract_address: match sk {
                SenderEvidence::Contract(addr) => Some(Sp::new(addr)),
                _ => None,
            },
            merkle_tree_root,
            named_output: named_output.map(Sp::new),
            proof: Arc::new(proof_preimage),
        };
        //debug_assert!(inp.well_formed().is_ok());
        Ok(inp)
    }
}

/// The zswap ledger slot of the guard's reserved inputs.
pub(crate) const RESERVED_SLOT: u8 = 6;

/// The reserved inputs a version 1 guard receives: four zeros.
pub(crate) fn reserved_value() -> AlignedValue {
    (Fr::from(0u64), Fr::from(0u64), Fr::from(0u64), Fr::from(0u64)).into()
}

/// The spend's proof preimage inputs between the Merkle path and the coin:
/// the type's preimage, and for the guarded circuit, the descriptor's key
/// hash, the recipient and the named output.
fn spend_extra_inputs(
    token_type: &ShieldedTokenType,
    preimage: &ShieldedTokenPreimage,
    guarded: Option<&GuardedSpend>,
) -> Result<Vec<Fr>, OfferCreationFailed> {
    if preimage.token_type() != *token_type {
        return Err(OfferCreationFailed::TypeOpeningMismatch(*token_type));
    }
    if !preimage.rule.is_defined() {
        return Err(OfferCreationFailed::UndefinedSpendRule(preimage.rule));
    }
    let mut inputs = Vec::new();
    preimage.domain_sep.field_repr(&mut inputs);
    preimage.contract_address.0.field_repr(&mut inputs);
    let Some(guarded) = guarded else {
        if preimage.rule != SpendRule::Empty {
            return Err(OfferCreationFailed::GuardedSpendRequired(*token_type));
        }
        return Ok(inputs);
    };
    match (preimage.rule.descriptor(), &guarded.guard) {
        (Some(descriptor), Some(_)) => {
            let vk_hash = Fr::from_le_bytes(&descriptor.payload.0)
                .ok_or(OfferCreationFailed::NonCanonicalGuardKeyHash)?;
            inputs.extend([Fr::from(1u64), vk_hash]);
        }
        (Some(_), None) => return Err(OfferCreationFailed::GuardProofRequired(*token_type)),
        (None, None) => inputs.extend([Fr::from(0u64), Fr::from(0u64)]),
        (None, Some(_)) => return Err(OfferCreationFailed::UnexpectedGuardProof(*token_type)),
    }
    inputs.extend(recipient_fields(&guarded.recipient));
    inputs.extend(output_fields(guarded.output.as_ref()));
    Ok(inputs)
}

/// The number of field elements a guard is handed.
pub const GUARD_ARGS_LEN: usize = 30;

/// `recipient: Either<ShieldedAddress, ContractAddress>`, in the ledger's
/// field encoding: `is_left`, the shielded address (coin key, encryption key
/// as `(x, y)`), then the contract address. The branch not taken is
/// `default`: zero, and the identity point for the encryption key.
pub(crate) fn recipient_fields(recipient: &SpendRecipient) -> Vec<Fr> {
    let mut fields = Vec::with_capacity(7);
    let (is_left, coin_key, encryption_key, contract) = match recipient {
        SpendRecipient::User {
            coin_key,
            encryption_key,
        } => (
            true,
            *coin_key,
            encryption_key.point(),
            ContractAddress::default(),
        ),
        SpendRecipient::Contract(addr) => (
            false,
            Default::default(),
            EmbeddedGroupAffine::identity(),
            *addr,
        ),
    };
    fields.push(Fr::from(is_left as u64));
    coin_key.field_repr(&mut fields);
    fields.push(encryption_key.x().unwrap_or(0.into()));
    fields.push(encryption_key.y().unwrap_or(1.into()));
    contract.field_repr(&mut fields);
    fields
}

/// `output: Maybe<ShieldedCoinInfo>`, in the ledger's field encoding:
/// `is_some`, then the coin (`default` if absent).
pub(crate) fn output_fields(output: Option<&CoinInfo>) -> Vec<Fr> {
    let mut fields = vec![Fr::from(output.is_some() as u64)];
    match output {
        Some(coin) => coin.field_repr(&mut fields),
        None => CoinInfo {
            nonce: Default::default(),
            type_: Default::default(),
            value: 0,
        }
        .field_repr(&mut fields),
    }
    fields
}

/// The arguments the guarded spend circuit hands a coin's guard, in the
/// ledger's field encoding, in order: `coin` (with its commitment tree
/// index), `nullifier`, `spender`, `recipient`, `output`, and `reserved`
/// (zero for version 1).
///
/// A guard proof for a spend must be made with these as its public inputs.
pub fn guard_args(
    coin: &QualifiedCoinInfo,
    sender: &SenderEvidence<'_>,
    recipient: &SpendRecipient,
    output: Option<&CoinInfo>,
) -> Vec<Fr> {
    let mut args = Vec::with_capacity(GUARD_ARGS_LEN);
    coin.field_repr(&mut args);
    CoinInfo::from(coin).nullifier(sender).field_repr(&mut args);
    let (is_left, coin_key, contract) = match Recipient::from(sender.clone()) {
        Recipient::User(pk) => (true, pk, ContractAddress::default()),
        Recipient::Contract(addr) => (false, Default::default(), addr),
    };
    args.push(Fr::from(is_left as u64));
    coin_key.field_repr(&mut args);
    contract.field_repr(&mut args);
    args.extend(recipient_fields(recipient));
    args.extend(output_fields(output));
    args.extend([Fr::from(0u64); 4]);
    debug_assert_eq!(args.len(), GUARD_ARGS_LEN);
    args
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
        // NOTE: negated because `Output::binding_randomness` already negates it, but we need the
        // positive variant.
        let rc_e = -self.binding_randomness();
        let value_commitment = Pedersen::commit(
            &(delta.token_type, new_segment),
            &delta.value.saturating_neg().into(),
            &rc_e,
        );
        let mut public_transcript_prog = Vec::<Op<ResultModeVerify, D>>::new();
        public_transcript_prog.extend(
            Cell_read!([Key::Value(5u8.into())], false, u16)
                .into_iter()
                .map(|op: Op<ResultModeGather, _>| op.translate(|()| new_segment.into())),
        );
        public_transcript_prog.extend(Cell_write!(
            [Key::Value(2u8.into())],
            false,
            (Fr, Fr),
            value_commitment.0
        ));
        let mut public_transcript_inputs_end = Vec::new();
        for op in filter_invalid(public_transcript_prog.into_iter()) {
            op.field_repr(&mut public_transcript_inputs_end);
        }

        let mut proof_preimage = self.proof.deref().clone();
        let len = proof_preimage.public_transcript_inputs.len();
        proof_preimage.public_transcript_inputs[len - public_transcript_inputs_end.len()..len]
            .copy_from_slice(&public_transcript_inputs_end);
        proof_preimage.public_transcript_outputs = vec![new_segment.into()];

        Output {
            value_commitment,
            proof: Arc::new(proof_preimage),
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
                .map(|op: Op<ResultModeGather, _>| op.translate(|()| segment.unwrap_or(0).into())),
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
            public_transcript_outputs: vec![segment.unwrap_or(0).into()],
            inner_proofs: vec![],
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
    #[instrument(skip(rng, guarded))]
    pub fn new_from_contract_owned_output<R: Rng + CryptoRng + ?Sized>(
        rng: &mut R,
        coin: &QualifiedCoinInfo,
        segment: Option<u16>,
        output: Output<ProofPreimage, D>,
        preimage: &ShieldedTokenPreimage,
        guarded: Option<GuardedSpend>,
    ) -> Result<Self, OfferCreationFailed> {
        let tree = MerkleTree::<(), InMemoryDB>::blank(ZSWAP_TREE_HEIGHT)
            .try_update_hash(0, output.coin_com.0, ())
            .map_err(OfferCreationFailed::MerkleTreeError)?
            .rehash();
        let addr = output
            .contract_address
            .clone()
            .ok_or(OfferCreationFailed::NotContractOwned)?;
        let input = Input::new_contract_owned(
            rng,
            coin,
            segment,
            *addr.deref(),
            &tree,
            preimage,
            guarded,
        )?;
        let io = Transient {
            nullifier: input.nullifier,
            coin_com: output.coin_com,
            value_commitment_input: input.value_commitment,
            value_commitment_output: output.value_commitment,
            contract_address: output.contract_address,
            ciphertext: output.ciphertext,
            named_output: input.named_output.map(|com| Sp::new(*com)),
            proof_input: input.proof,
            proof_output: output.proof,
        };
        Ok(io)
    }

    pub fn retarget_segment(&self, new_segment: u16) -> Self {
        let input = self.as_input().retarget_segment(new_segment);
        let output = self.as_output().retarget_segment(new_segment);
        Transient {
            nullifier: input.nullifier,
            coin_com: output.coin_com,
            value_commitment_input: input.value_commitment,
            value_commitment_output: output.value_commitment,
            contract_address: output.contract_address,
            ciphertext: output.ciphertext,
            named_output: input.named_output,
            proof_input: input.proof,
            proof_output: output.proof,
        }
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
        let preimage = coin_structure::custom_spend::ShieldedTokenPreimage::genesis(
            rng.r#gen(),
            Default::default(),
        );
        let trans = Transient::new_from_contract_owned_output(
            &mut rng,
            &coin.qualify(0),
            None,
            out,
            &preimage,
            None,
        );
        assert!(trans.is_err());
    }
}
