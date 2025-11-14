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

use crate::conversions::*;
use crate::dust::Event;
use crate::intent::{Intent, IntentTypes};
use crate::state::LedgerState;
use crate::zswap_state::whitelist_from_value;
use crate::zswap_wasm::{LedgerParameters, ZswapOffer, ZswapOfferTypes, offer_segment_id};
use base_crypto::signatures;
use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use coin_structure::coin::Nonce;
use hex::ToHex;
use js_sys::{Array, BigInt, Date, Function, JsString, Map, Promise, Uint8Array};
use ledger::structure::{
    BindingKind, PedersenDowngradeable, ProofKind, ProofMarker, ProofPreimageMarker,
    ProofVersioned, SignatureKind,
};
use onchain_runtime_wasm::context::CostModel;
use onchain_runtime_wasm::conversions::token_type_to_value;
use onchain_runtime_wasm::{from_value_hex_ser, from_value_ser};
use rand::Rng;
use rand::rngs::OsRng;
use serialize::{Tagged, tagged_deserialize, tagged_serialize};
use std::collections::HashMap;
use std::ops::Deref;
use storage::Storable;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use transient_crypto::commitment::{Pedersen, PedersenRandomness, PureGeneratorPedersen};
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{ProofPreimage, ProvingProvider};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use zswap::Offer;

type PreBinding = PedersenRandomness;
type Binding = PureGeneratorPedersen;
type NoBinding = Pedersen;

// S: Signature or SignatureErased
// P: Unproven (ProofPreimage) or Proven (Proof) or ProofErased ( () )
// B: PreBinding (PedersenRandomness) or Binding (PureGeneratorPedersen) or NoBinding (Pedersen)
#[derive(Clone)]
pub enum TransactionTypes {
    UnprovenWithSignaturePreBinding(
        ledger::structure::Transaction<Signature, ProofPreimageMarker, PreBinding, InMemoryDB>,
    ),
    UnprovenWithSignatureBinding(
        ledger::structure::Transaction<Signature, ProofPreimageMarker, Binding, InMemoryDB>,
    ),
    UnprovenWithSignatureErasedPreBinding(
        ledger::structure::Transaction<(), ProofPreimageMarker, PreBinding, InMemoryDB>,
    ),
    UnprovenWithSignatureErasedBinding(
        ledger::structure::Transaction<(), ProofPreimageMarker, Binding, InMemoryDB>,
    ),
    //
    ProvenWithSignaturePreBinding(
        ledger::structure::Transaction<Signature, ProofMarker, PreBinding, InMemoryDB>,
    ),
    ProvenWithSignatureBinding(
        ledger::structure::Transaction<Signature, ProofMarker, Binding, InMemoryDB>,
    ),
    ProvenWithSignatureErasedPreBinding(
        ledger::structure::Transaction<(), ProofMarker, PreBinding, InMemoryDB>,
    ),
    ProvenWithSignatureErasedBinding(
        ledger::structure::Transaction<(), ProofMarker, Binding, InMemoryDB>,
    ),
    //
    ProofErasedWithSignatureNoBinding(
        ledger::structure::Transaction<Signature, (), NoBinding, InMemoryDB>,
    ),
    ProofErasedWithSignatureErasedNoBinding(
        ledger::structure::Transaction<(), (), NoBinding, InMemoryDB>,
    ),
}

#[derive(Clone)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct VerifiedTransaction(pub(crate) ledger::structure::VerifiedTransaction<InMemoryDB>);

#[wasm_bindgen]
impl VerifiedTransaction {
    #[wasm_bindgen(getter = "transaction")]
    pub fn transaction(&self) -> Transaction {
        Transaction(TransactionTypes::ProofErasedWithSignatureErasedNoBinding(
            (*self.0).clone(),
        ))
    }
}

#[derive(Clone)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct Transaction(pub(crate) TransactionTypes);

#[wasm_bindgen]
impl Transaction {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Transaction, JsError> {
        Err(JsError::new(
            "Transaction cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "fromParts")]
    pub fn from_parts(
        network_id: String,
        guaranteed: JsValue,
        fallible: JsValue,
        intent: JsValue,
    ) -> Result<Transaction, JsError> {
        let guaranteed = if guaranteed.is_null() || guaranteed.is_undefined() {
            None
        } else {
            ZswapOffer::try_ref(&guaranteed)?
        };
        if guaranteed.is_some()
            && !matches!(
                guaranteed.as_ref().unwrap().0,
                ZswapOfferTypes::UnprovenOffer(_)
            )
        {
            return Err(JsError::new("Guaranteed offer must be unproven."));
        }

        let fallible = if fallible.is_null() || fallible.is_undefined() {
            None
        } else {
            ZswapOffer::try_ref(&fallible)?
        };
        if fallible.is_some()
            && !matches!(
                fallible.as_ref().unwrap().0,
                ZswapOfferTypes::UnprovenOffer(_)
            )
        {
            return Err(JsError::new("Fallible offer must be unproven."));
        }

        let intent = if intent.is_null() || intent.is_undefined() {
            None
        } else {
            Intent::try_ref(&intent)?
        };
        if intent.is_some()
            && !matches!(
                intent.as_ref().unwrap().0,
                IntentTypes::UnprovenWithSignaturePreBinding(_)
                    | IntentTypes::UnprovenWithSignatureErasedPreBinding(_)
            )
        {
            return Err(JsError::new("Intent offer must be unproven."));
        }

        let mut intents = vec![];
        if intent.is_some() {
            intents.push((1, intent.unwrap().clone().try_into()?));
        }

        let fallible_items = if let Some(fallible) = fallible {
            let offer: zswap::Offer<ProofPreimage, InMemoryDB> = fallible.clone().try_into()?;
            let segment_id = offer_segment_id(&offer)?;
            match segment_id {
                Some(segment_id) => {
                    if segment_id == 0 {
                        return Err(JsError::new("Segment ID cannot be 0 in a fallible offer"));
                    }
                    HashMap::from([(segment_id, offer)])
                }
                None => HashMap::new(),
            }
        } else {
            HashMap::new()
        };

        Ok(Transaction(
            TransactionTypes::UnprovenWithSignaturePreBinding(ledger::structure::Transaction::new(
                network_id,
                intents.into_iter().collect(),
                guaranteed.map(|f| f.clone().try_into()).transpose()?,
                fallible_items,
            )),
        ))
    }

    #[wasm_bindgen(js_name = "fromPartsRandomized")]
    pub fn from_parts_randomized(
        network_id: String,
        guaranteed: JsValue,
        fallible: JsValue,
        intent: JsValue,
    ) -> Result<Transaction, JsError> {
        let guaranteed = if guaranteed.is_null() || guaranteed.is_undefined() {
            None
        } else {
            ZswapOffer::try_ref(&guaranteed)?
        };
        if guaranteed.is_some()
            && !matches!(
                guaranteed.as_ref().unwrap().0,
                ZswapOfferTypes::UnprovenOffer(_)
            )
        {
            return Err(JsError::new("Guaranteed offer must be unproven."));
        }

        let fallible = if fallible.is_null() || fallible.is_undefined() {
            None
        } else {
            ZswapOffer::try_ref(&fallible)?
        };
        if fallible.is_some()
            && !matches!(
                fallible.as_ref().unwrap().0,
                ZswapOfferTypes::UnprovenOffer(_)
            )
        {
            return Err(JsError::new("Fallible offer must be unproven."));
        }

        let intent = if intent.is_null() || intent.is_undefined() {
            None
        } else {
            Intent::try_ref(&intent)?
        };
        if intent.is_some()
            && !matches!(
                intent.as_ref().unwrap().0,
                IntentTypes::UnprovenWithSignaturePreBinding(_)
                    | IntentTypes::UnprovenWithSignatureErasedPreBinding(_)
            )
        {
            return Err(JsError::new("Intent offer must be unproven."));
        }

        let segment_id = OsRng.gen_range(1..u16::MAX);
        let mut intents = vec![];
        if intent.is_some() {
            intents.push((segment_id, intent.unwrap().clone().try_into()?));
        }

        let fallible_items = if let Some(fallible) = fallible {
            let offer: zswap::Offer<ProofPreimage, InMemoryDB> = fallible.clone().try_into()?;
            let segment_id = offer_segment_id(&offer)?;
            match segment_id {
                Some(segment_id) => {
                    if segment_id == 0 {
                        return Err(JsError::new("Segment ID cannot be 0 in a fallible offer"));
                    }
                    HashMap::from([(segment_id, offer)])
                }
                None => HashMap::new(),
            }
        } else {
            HashMap::new()
        };

        Ok(Transaction(
            TransactionTypes::UnprovenWithSignaturePreBinding(ledger::structure::Transaction::new(
                network_id,
                intents.into_iter().collect(),
                guaranteed.map(|f| f.clone().try_into()).transpose()?,
                fallible_items,
            )),
        ))
    }

    #[wasm_bindgen(js_name = "fromRewards")]
    pub fn from_rewards(rewards: &ClaimRewardsTransaction) -> Transaction {
        use ClaimRewardsTransactionTypes::*;
        use TransactionTypes::*;
        use ledger::structure::Transaction::ClaimRewards;
        match &rewards.0 {
            SignatureClaimRewards(val) => Transaction(UnprovenWithSignatureBinding(ClaimRewards(
                ledger::structure::ClaimRewardsTransaction::from(val.clone()).into(),
            ))),
            SignatureErasedClaimRewards(val) => Transaction(UnprovenWithSignatureErasedBinding(
                ClaimRewards(ledger::structure::ClaimRewardsTransaction::from(val.clone()).into()),
            )),
        }
    }

    #[wasm_bindgen(js_name = "mockProve")]
    pub fn mock_prove(&self) -> Result<Transaction, JsError> {
        use TransactionTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(tx) => {
                Ok(Transaction(ProvenWithSignatureBinding(tx.mock_prove()?)))
            }
            UnprovenWithSignatureErasedPreBinding(tx) => Ok(Transaction(
                ProvenWithSignatureErasedBinding(tx.mock_prove()?),
            )),
            ProvenWithSignaturePreBinding(_)
            | ProvenWithSignatureErasedPreBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_) => {
                Err(JsError::new("cannot prove already proven transaction"))
            }
            ProofErasedWithSignatureNoBinding(_) | ProofErasedWithSignatureErasedNoBinding(_) => {
                Err(JsError::new("cannot prove proof-erased transaction"))
            }
            UnprovenWithSignatureBinding(_) | UnprovenWithSignatureErasedBinding(_) => {
                Err(JsError::new("cannot prove bound transaction"))
            }
        }
    }

    pub async fn prove(
        &self,
        provider: JsValue,
        cost_model: &CostModel,
    ) -> Result<Transaction, JsError> {
        let check = js_sys::Reflect::get(&provider, &"check".into())
            .map_err(|_| JsError::new("failed to get property 'check' on ProvingProvider"))?
            .dyn_into::<Function>()
            .map_err(|_| {
                JsError::new("expected proof provider property 'check' to be a function")
            })?;
        let prove = js_sys::Reflect::get(&provider, &"prove".into())
            .map_err(|_| JsError::new("failed to get property 'prove' on ProvingProvider"))?
            .dyn_into::<Function>()
            .map_err(|_| {
                JsError::new("expected proving provider property 'prove' to be a function")
            })?;
        #[derive(Clone)]
        struct JsProvingProvider {
            this: JsValue,
            check: Function,
            prove: Function,
        }
        impl ProvingProvider for JsProvingProvider {
            async fn check(
                &self,
                preimage: &transient_crypto::proofs::ProofPreimage,
            ) -> Result<Vec<Option<usize>>, anyhow::Error> {
                let mut ser = Vec::new();
                tagged_serialize(preimage, &mut ser)?;
                let arg_encoded_preimage = JsValue::from(Uint8Array::from(&ser[..]));
                let arg_key_location =
                    JsValue::from(JsString::from(preimage.key_location.0.as_ref()));
                let promise = self
                    .check
                    .call2(&self.this, &arg_encoded_preimage, &arg_key_location)
                    .map_err(|e| anyhow::anyhow!("failed to call 'check': {}", try_to_string(e)))?
                    .dyn_into::<Promise>()
                    .map_err(|_| anyhow::anyhow!("result of 'check' was not a promise"))?;
                let result = JsFuture::from(promise)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("'check' returned an error: {}", try_to_string(e))
                    })?
                    .dyn_into::<Array>()
                    .map_err(|_| anyhow::anyhow!("'check' did not return an array"))?;
                let mut res = Vec::new();
                for value in result {
                    if value.is_undefined() || value.is_null() {
                        res.push(None);
                    }
                    let value_bigint = value
                        .dyn_into::<BigInt>()
                        .map_err(|_| anyhow::anyhow!("'check' returned invalid type"))?;
                    let value_usize = u64::try_from(value_bigint)
                        .ok()
                        .and_then(|v| usize::try_from(v).ok())
                        .ok_or_else(|| anyhow::anyhow!("'check' returned bigint out of range"))?;
                    res.push(Some(value_usize))
                }
                Ok(res)
            }
            async fn prove(
                self,
                preimage: &transient_crypto::proofs::ProofPreimage,
                overwrite_binding_input: Option<Fr>,
            ) -> Result<transient_crypto::proofs::Proof, anyhow::Error> {
                let mut ser = Vec::new();
                tagged_serialize(preimage, &mut ser)?;
                let arg_encoded_preimage = JsValue::from(Uint8Array::from(&ser[..]));
                let arg_key_location =
                    JsValue::from(JsString::from(preimage.key_location.0.as_ref()));
                let arg_overwrite_binding_input = match overwrite_binding_input {
                    Some(input) => JsValue::from(fr_to_bigint(input)),
                    None => JsValue::UNDEFINED,
                };
                let promise = self
                    .prove
                    .call3(
                        &self.this,
                        &arg_encoded_preimage,
                        &arg_key_location,
                        &arg_overwrite_binding_input,
                    )
                    .map_err(|e| anyhow::anyhow!("failed to call 'prove': {}", try_to_string(e)))?
                    .dyn_into::<Promise>()
                    .map_err(|_| anyhow::anyhow!("result of 'check' was not a promise"))?;
                let result = JsFuture::from(promise)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("'prove' returned an error: {}", try_to_string(e))
                    })?
                    .dyn_into::<Uint8Array>()
                    .map_err(|_| anyhow::anyhow!("'prove' did not return Uint8Array"))?
                    .to_vec();
                Ok(tagged_deserialize(&mut &result[..]).or_else(|_| {
                    let ppiv = tagged_deserialize::<ProofVersioned>(&mut &result[..])?;
                    match ppiv {
                        ProofVersioned::V1(proof) => Ok::<_, std::io::Error>(proof),
                        _ => Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "expected proof[v1], got a different version",
                        )),
                    }
                })?)
            }
            fn split(&mut self) -> Self {
                self.clone()
            }
        }
        let provider = JsProvingProvider {
            this: provider,
            check,
            prove,
        };
        use TransactionTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(tx) => Ok(Transaction(ProvenWithSignaturePreBinding(
                tx.prove(provider, &cost_model.clone().into()).await?,
            ))),
            UnprovenWithSignatureErasedPreBinding(tx) => {
                Ok(Transaction(ProvenWithSignatureErasedPreBinding(
                    tx.prove(provider, &cost_model.clone().into()).await?,
                )))
            }
            ProvenWithSignaturePreBinding(_)
            | ProvenWithSignatureErasedPreBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_) => {
                Err(JsError::new("cannot prove already proven transaction"))
            }
            ProofErasedWithSignatureNoBinding(_) | ProofErasedWithSignatureErasedNoBinding(_) => {
                Err(JsError::new("cannot prove proof-erased transaction"))
            }
            UnprovenWithSignatureBinding(_) | UnprovenWithSignatureErasedBinding(_) => {
                Err(JsError::new("cannot prove bound transaction"))
            }
        }
    }

    #[wasm_bindgen(js_name = "eraseProofs")]
    pub fn erase_proofs(&self) -> Transaction {
        use TransactionTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(val) => {
                Transaction(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            UnprovenWithSignatureBinding(val) => {
                Transaction(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            UnprovenWithSignatureErasedPreBinding(val) => {
                Transaction(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            UnprovenWithSignatureErasedBinding(val) => {
                Transaction(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            ProvenWithSignaturePreBinding(val) => {
                Transaction(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            ProvenWithSignatureBinding(val) => {
                Transaction(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            ProvenWithSignatureErasedPreBinding(val) => {
                Transaction(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            ProvenWithSignatureErasedBinding(val) => {
                Transaction(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            ProofErasedWithSignatureNoBinding(val) => {
                Transaction(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            ProofErasedWithSignatureErasedNoBinding(val) => {
                Transaction(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
        }
    }

    #[wasm_bindgen(getter, js_name = "eraseSignatures")]
    pub fn erase_signatures(&self) -> Result<Transaction, JsError> {
        use TransactionTypes::*;
        Ok(match &self.0 {
            UnprovenWithSignatureErasedBinding(_)
            | UnprovenWithSignatureErasedPreBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProvenWithSignatureErasedPreBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_) => self.clone(),
            UnprovenWithSignatureBinding(val) => {
                Transaction(UnprovenWithSignatureErasedBinding(val.erase_signatures()))
            }
            ProvenWithSignatureBinding(val) => {
                Transaction(ProvenWithSignatureErasedBinding(val.erase_signatures()))
            }
            UnprovenWithSignaturePreBinding(val) => Transaction(
                UnprovenWithSignatureErasedPreBinding(val.erase_signatures()),
            ),
            ProvenWithSignaturePreBinding(val) => {
                Transaction(ProvenWithSignatureErasedPreBinding(val.erase_signatures()))
            }
            ProofErasedWithSignatureNoBinding(val) => Transaction(
                ProofErasedWithSignatureErasedNoBinding(val.erase_signatures()),
            ),
        })
    }

    pub fn bind(&self) -> Result<Transaction, JsError> {
        use TransactionTypes::*;
        Ok(match &self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureNoBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_)
            | ProvenWithSignatureBinding(_) => self.clone(),
            UnprovenWithSignaturePreBinding(val) => {
                Transaction(UnprovenWithSignatureBinding(val.seal(OsRng)))
            }
            UnprovenWithSignatureErasedPreBinding(val) => {
                Transaction(UnprovenWithSignatureErasedBinding(val.seal(OsRng)))
            }
            ProvenWithSignaturePreBinding(val) => {
                Transaction(ProvenWithSignatureBinding(val.seal(OsRng)))
            }
            ProvenWithSignatureErasedPreBinding(val) => {
                Transaction(ProvenWithSignatureErasedBinding(val.seal(OsRng)))
            }
        })
    }

    #[wasm_bindgen(js_name = "wellFormed")]
    pub fn well_formed(
        &self,
        ref_state: &LedgerState,
        strictness: &WellFormedStrictness,
        tblock: &Date,
    ) -> Result<VerifiedTransaction, JsError> {
        get_dyn_transaction(self.0.clone()).well_formed(ref_state, strictness, tblock)
    }

    #[wasm_bindgen(js_name = "transactionHash")]
    pub fn transaction_hash(&self) -> Result<String, JsError> {
        use TransactionTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(_)
            | UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedPreBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureNoBinding(_)
            | ProvenWithSignaturePreBinding(_)
            | ProvenWithSignatureErasedPreBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_) => Err(JsError::new(
                "Transaction hash is available for proven, signed and bound transactions only.",
            )),
            ProvenWithSignatureBinding(val) => to_hex_ser(&val.transaction_hash()),
        }
    }

    pub fn identifiers(&self) -> Result<Vec<JsString>, JsError> {
        get_dyn_transaction(self.0.clone()).identifiers()
    }

    pub fn merge(&self, other: &Transaction) -> Result<Transaction, JsError> {
        use TransactionTypes::*;
        Ok(Transaction(match (&self.0, &other.0) {
            (UnprovenWithSignaturePreBinding(val), UnprovenWithSignaturePreBinding(other_val)) => {
                UnprovenWithSignaturePreBinding(val.merge(&other_val)?)
            }
            (UnprovenWithSignatureBinding(val), UnprovenWithSignatureBinding(other_val)) => {
                UnprovenWithSignatureBinding(val.merge(&other_val)?)
            }
            (
                UnprovenWithSignatureErasedPreBinding(val),
                UnprovenWithSignatureErasedPreBinding(other_val),
            ) => UnprovenWithSignatureErasedPreBinding(val.merge(&other_val)?),
            (
                UnprovenWithSignatureErasedBinding(val),
                UnprovenWithSignatureErasedBinding(other_val),
            ) => UnprovenWithSignatureErasedBinding(val.merge(&other_val)?),
            (ProvenWithSignaturePreBinding(val), ProvenWithSignaturePreBinding(other_val)) => {
                ProvenWithSignaturePreBinding(val.merge(&other_val)?)
            }
            (ProvenWithSignatureBinding(val), ProvenWithSignatureBinding(other_val)) => {
                ProvenWithSignatureBinding(val.merge(&other_val)?)
            }
            (
                ProvenWithSignatureErasedPreBinding(val),
                ProvenWithSignatureErasedPreBinding(other_val),
            ) => ProvenWithSignatureErasedPreBinding(val.merge(&other_val)?),
            (
                ProvenWithSignatureErasedBinding(val),
                ProvenWithSignatureErasedBinding(other_val),
            ) => ProvenWithSignatureErasedBinding(val.merge(&other_val)?),
            (
                ProofErasedWithSignatureNoBinding(val),
                ProofErasedWithSignatureNoBinding(other_val),
            ) => ProofErasedWithSignatureNoBinding(val.merge(&other_val)?),
            (
                ProofErasedWithSignatureErasedNoBinding(val),
                ProofErasedWithSignatureErasedNoBinding(other_val),
            ) => ProofErasedWithSignatureErasedNoBinding(val.merge(&other_val)?),
            _ => Err(JsError::new(
                "Both transactions need to be of the same type.",
            ))?,
        }))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        get_dyn_transaction(self.0.clone()).serialize()
    }

    pub fn deserialize(
        signature_marker: &str,
        proof_marker: &str,
        binding_marker: &str,
        raw: Uint8Array,
    ) -> Result<Transaction, JsError> {
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;
        let binding_type: Bindingish = text_to_bindingish(binding_marker)?;

        use Bindingish::*;
        use Proofish::*;
        use Signaturish::*;
        use TransactionTypes::*;
        Ok(Transaction(
            match (signature_type, proof_type, binding_type) {
                (Signature, PreProof, PreBinding) => {
                    UnprovenWithSignaturePreBinding(from_value_ser(raw, "Transaction")?)
                }
                (Signature, PreProof, Binding) => {
                    UnprovenWithSignatureBinding(from_value_ser(raw, "Transaction")?)
                }
                (SignatureErased, PreProof, PreBinding) => {
                    UnprovenWithSignatureErasedPreBinding(from_value_ser(raw, "Transaction")?)
                }
                (SignatureErased, PreProof, Binding) => {
                    UnprovenWithSignatureErasedBinding(from_value_ser(raw, "Transaction")?)
                }
                (Signature, Proof, PreBinding) => {
                    ProvenWithSignaturePreBinding(from_value_ser(raw, "Transaction")?)
                }
                (Signature, Proof, Binding) => {
                    ProvenWithSignatureBinding(from_value_ser(raw, "Transaction")?)
                }
                (SignatureErased, Proof, PreBinding) => {
                    ProvenWithSignatureErasedPreBinding(from_value_ser(raw, "Transaction")?)
                }
                (SignatureErased, Proof, Binding) => {
                    ProvenWithSignatureErasedBinding(from_value_ser(raw, "Transaction")?)
                }
                (Signature, NoProof, NoBinding) => {
                    ProofErasedWithSignatureNoBinding(from_value_ser(raw, "Transaction")?)
                }
                (SignatureErased, NoProof, NoBinding) => {
                    ProofErasedWithSignatureErasedNoBinding(from_value_ser(raw, "Transaction")?)
                }
                _ => Err(JsError::new("Unsupported transaction type provided."))?,
            },
        ))
    }

    pub fn imbalances(&self, segment: u16, fees: Option<BigInt>) -> Result<Map, JsError> {
        get_dyn_transaction(self.0.clone()).imbalances(segment, fees)
    }

    pub fn cost(
        &self,
        params: &LedgerParameters,
        enforce_time_to_dismiss: Option<bool>,
    ) -> Result<JsValue, JsError> {
        get_dyn_transaction(self.0.clone()).cost(params, enforce_time_to_dismiss.unwrap_or(false))
    }

    pub fn fees(
        &self,
        params: &LedgerParameters,
        enforce_time_to_dismiss: Option<bool>,
    ) -> Result<BigInt, JsError> {
        get_dyn_transaction(self.0.clone()).fees(params, enforce_time_to_dismiss.unwrap_or(false))
    }

    #[wasm_bindgen(js_name = "feesWithMargin")]
    pub fn fees_with_margin(&self, params: &LedgerParameters, n: usize) -> Result<BigInt, JsError> {
        get_dyn_transaction(self.0.clone()).fees_with_margin(params, n)
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        get_dyn_transaction(self.0.clone()).to_string(compact)
    }

    #[wasm_bindgen(getter)]
    pub fn rewards(&self) -> Option<ClaimRewardsTransaction> {
        get_dyn_transaction(self.0.clone()).rewards()
    }

    #[wasm_bindgen(getter, js_name = "bindingRandomness")]
    pub fn binding_randomness(&self) -> Result<BigInt, JsError> {
        get_dyn_transaction(self.0.clone()).binding_randomness()
    }

    #[wasm_bindgen(getter, js_name = "guaranteedOffer")]
    pub fn guaranteed_offer(&self) -> Option<ZswapOffer> {
        get_dyn_transaction(self.0.clone()).guaranteed_offer()
    }

    #[wasm_bindgen(setter, js_name = "guaranteedOffer")]
    pub fn set_guaranteed_offer(&mut self, offer: JsValue) -> Result<(), JsError> {
        use TransactionTypes::*;
        use ledger::structure::Transaction::Standard;

        let offer = if offer.is_null() || offer.is_undefined() {
            None
        } else {
            ZswapOffer::try_ref(&offer)?
        };

        match &mut self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_) => {
                Err(JsError::new("Transaction is already bound."))?
            }
            UnprovenWithSignaturePreBinding(val) => match val {
                Standard(tx) => {
                    tx.guaranteed_coins = offer
                        .map(|o| o.clone().try_into())
                        .transpose()?
                        .map(Sp::new);
                }
                _ => Err(JsError::new("Not a standard transaction."))?,
            },
            UnprovenWithSignatureErasedPreBinding(val) => match val {
                Standard(tx) => {
                    tx.guaranteed_coins = offer
                        .map(|o| o.clone().try_into())
                        .transpose()?
                        .map(Sp::new);
                }
                _ => Err(JsError::new("Not a standard transaction."))?,
            },
            ProvenWithSignaturePreBinding(val) => match val {
                Standard(tx) => {
                    tx.guaranteed_coins = offer
                        .map(|o| o.clone().try_into())
                        .transpose()?
                        .map(Sp::new);
                }
                _ => Err(JsError::new("Not a standard transaction."))?,
            },
            ProvenWithSignatureErasedPreBinding(val) => match val {
                Standard(tx) => {
                    tx.guaranteed_coins = offer
                        .map(|o| o.clone().try_into())
                        .transpose()?
                        .map(Sp::new);
                }
                _ => Err(JsError::new("Not a standard transaction."))?,
            },
            ProofErasedWithSignatureNoBinding(val) => match val {
                Standard(tx) => {
                    tx.guaranteed_coins = offer
                        .map(|o| o.clone().try_into())
                        .transpose()?
                        .map(Sp::new);
                }
                _ => Err(JsError::new("Not a standard transaction."))?,
            },
            ProofErasedWithSignatureErasedNoBinding(val) => match val {
                Standard(tx) => {
                    tx.guaranteed_coins = offer
                        .map(|o| o.clone().try_into())
                        .transpose()?
                        .map(Sp::new);
                }
                _ => Err(JsError::new("Not a standard transaction."))?,
            },
        }
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleOffer")]
    pub fn fallible_offer(&self) -> Option<Map> {
        get_dyn_transaction(self.0.clone()).fallible_offer()
    }

    #[wasm_bindgen(setter, js_name = "fallibleOffer")]
    pub fn set_fallible_offer(&mut self, offers_map: Option<Map>) -> Result<(), JsError> {
        use TransactionTypes::*;
        use ledger::structure::Transaction::Standard;

        let mut offers: Vec<(u16, ZswapOffer)> = vec![];
        if let Some(offers_map) = offers_map {
            for key in offers_map.keys() {
                let key = key.unwrap();
                let value = offers_map.get(&key);
                let zswap_offer = ZswapOffer::try_ref(&value)?
                    .ok_or(JsError::new("Unable to decode ZswapOffer."))?;
                offers.push((from_value(key)?, zswap_offer.clone()));
            }
        }

        Ok(match &mut self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_) => {
                Err(JsError::new("Transaction is already bound."))?
            }
            UnprovenWithSignaturePreBinding(Standard(tx)) => {
                tx.fallible_coins = zswap_offers_to_fallible_coins::<ProofPreimageMarker>(offers)?;
            }
            UnprovenWithSignatureErasedPreBinding(Standard(tx)) => {
                tx.fallible_coins = zswap_offers_to_fallible_coins::<ProofPreimageMarker>(offers)?;
            }
            ProvenWithSignaturePreBinding(Standard(tx)) => {
                tx.fallible_coins = zswap_offers_to_fallible_coins::<ProofMarker>(offers)?;
            }
            ProvenWithSignatureErasedPreBinding(Standard(tx)) => {
                tx.fallible_coins = zswap_offers_to_fallible_coins::<ProofMarker>(offers)?;
            }
            ProofErasedWithSignatureNoBinding(Standard(tx)) => {
                tx.fallible_coins = zswap_offers_to_fallible_coins::<()>(offers)?;
            }
            ProofErasedWithSignatureErasedNoBinding(Standard(tx)) => {
                tx.fallible_coins = zswap_offers_to_fallible_coins::<()>(offers)?;
            }
            _ => Err(JsError::new("Not a standard transaction."))?,
        })
    }

    #[wasm_bindgen(getter, js_name = "intents")]
    pub fn intents(&self) -> Option<Map> {
        get_dyn_transaction(self.0.clone()).intents()
    }

    #[wasm_bindgen(setter, js_name = "intents")]
    pub fn set_intents(&mut self, intents_map: Option<Map>) -> Result<(), JsError> {
        use TransactionTypes::*;
        use ledger::structure::Transaction::Standard;

        let intents = intents_map
            .map(value_map_to_intent_vec)
            .transpose()?
            .unwrap_or_default();

        match &mut self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_) => {
                Err(JsError::new("Transaction is already bound."))?
            }
            UnprovenWithSignaturePreBinding(Standard(tx)) => {
                let mut tx_intents = vec![];
                for (segment_id, intent) in intents {
                    tx_intents.push((segment_id, intent.try_into()?));
                }
                tx.intents = tx_intents.into_iter().collect();
            }
            UnprovenWithSignatureErasedPreBinding(Standard(tx)) => {
                let mut tx_intents = vec![];
                for (segment_id, intent) in intents {
                    tx_intents.push((segment_id, intent.try_into()?));
                }
                tx.intents = tx_intents.into_iter().collect();
            }
            ProvenWithSignaturePreBinding(Standard(tx)) => {
                let mut tx_intents = vec![];
                for (segment_id, intent) in intents {
                    tx_intents.push((segment_id, intent.try_into()?));
                }
                tx.intents = tx_intents.into_iter().collect();
            }
            ProvenWithSignatureErasedPreBinding(Standard(tx)) => {
                let mut tx_intents = vec![];
                for (segment_id, intent) in intents {
                    tx_intents.push((segment_id, intent.try_into()?));
                }
                tx.intents = tx_intents.into_iter().collect();
            }
            ProofErasedWithSignatureNoBinding(Standard(tx)) => {
                let mut tx_intents = vec![];
                for (segment_id, intent) in intents {
                    tx_intents.push((segment_id, intent.try_into()?));
                }
                tx.intents = tx_intents.into_iter().collect();
            }
            ProofErasedWithSignatureErasedNoBinding(Standard(tx)) => {
                let mut tx_intents = vec![];
                for (segment_id, intent) in intents {
                    tx_intents.push((segment_id, intent.try_into()?));
                }
                tx.intents = tx_intents.into_iter().collect();
            }
            _ => Err(JsError::new("Not a standard transaction."))?,
        }

        Ok(())
    }
}

trait ClaimRewardable {
    fn to_wasm(&self) -> ClaimRewardsTransaction;
}

impl ClaimRewardable for ledger::structure::ClaimRewardsTransaction<Signature, InMemoryDB> {
    fn to_wasm(&self) -> ClaimRewardsTransaction {
        ClaimRewardsTransaction(ClaimRewardsTransactionTypes::SignatureClaimRewards(
            self.clone(),
        ))
    }
}

impl ClaimRewardable for ledger::structure::ClaimRewardsTransaction<(), InMemoryDB> {
    fn to_wasm(&self) -> ClaimRewardsTransaction {
        ClaimRewardsTransaction(ClaimRewardsTransactionTypes::SignatureErasedClaimRewards(
            self.clone(),
        ))
    }
}

trait Transactionable {
    fn imbalances(&self, segment: u16, fees: Option<BigInt>) -> Result<Map, JsError>;
    fn cost(
        &self,
        params: &LedgerParameters,
        enforce_time_to_dismiss: bool,
    ) -> Result<JsValue, JsError>;
    fn fees(
        &self,
        params: &LedgerParameters,
        enforce_time_to_dismiss: bool,
    ) -> Result<BigInt, JsError>;
    fn fees_with_margin(&self, params: &LedgerParameters, n: usize) -> Result<BigInt, JsError>;
    fn to_string(&self, compact: Option<bool>) -> String;
    fn rewards(&self) -> Option<ClaimRewardsTransaction>;
    fn binding_randomness(&self) -> Result<BigInt, JsError>;
    fn guaranteed_offer(&self) -> Option<ZswapOffer>;
    fn fallible_offer(&self) -> Option<Map>;
    fn intents(&self) -> Option<Map>;
    fn serialize(&self) -> Result<Uint8Array, JsError>;
    fn identifiers(&self) -> Result<Vec<JsString>, JsError>;
    fn well_formed(
        &self,
        ref_state: &LedgerState,
        strictness: &WellFormedStrictness,
        tblock: &Date,
    ) -> Result<VerifiedTransaction, JsError>;
}

impl<
    S: SignatureKind<InMemoryDB>,
    P: ProofKind<InMemoryDB> + std::fmt::Debug + Storable<InMemoryDB>,
    B: BindingKind<S, P, InMemoryDB>
        + Storable<InMemoryDB>
        + serialize::Serializable
        + std::fmt::Debug
        + PedersenDowngradeable<InMemoryDB>,
> Transactionable for ledger::structure::Transaction<S, P, B, InMemoryDB>
where
    Self: Tagged,
    ledger::structure::ClaimRewardsTransaction<S, InMemoryDB>: ClaimRewardable,
    ZswapOffer: From<Offer<<P as ProofKind<InMemoryDB>>::LatestProof, InMemoryDB>>,
    Intent: From<ledger::structure::Intent<S, P, B, InMemoryDB>>,
{
    fn imbalances(&self, segment: u16, fees: Option<BigInt>) -> Result<Map, JsError> {
        let fees = fees
            .map(u128::try_from)
            .transpose()
            .map_err(|_| JsError::new("fees out of range"))?;

        let res = Map::new();

        let segments = self.segments();
        if !segments.contains(&segment) {
            Err(JsError::new("segment doesn't exist: {segment:?}"))?
        }

        let imbalances = self
            .balance(fees)
            .map_err(|err| JsError::new(&String::from(err.to_string())))?;

        for ((token, imbalanced_segment), imbalance) in imbalances {
            if imbalanced_segment == segment {
                res.set(&token_type_to_value(&token)?, &to_value(&imbalance)?);
            }
        }

        Ok(res)
    }

    fn cost(
        &self,
        params: &LedgerParameters,
        enforce_time_to_dismiss: bool,
    ) -> Result<JsValue, JsError> {
        Ok(to_value(&self.cost(&params, enforce_time_to_dismiss)?)?)
    }

    fn fees(
        &self,
        params: &LedgerParameters,
        enforce_time_to_dismiss: bool,
    ) -> Result<BigInt, JsError> {
        Ok(BigInt::from(ledger::structure::Transaction::fees(
            self,
            &params,
            enforce_time_to_dismiss,
        )?))
    }

    fn fees_with_margin(&self, params: &LedgerParameters, n: usize) -> Result<BigInt, JsError> {
        Ok(BigInt::from(
            ledger::structure::Transaction::fees_with_margin(self, &params, n)?,
        ))
    }

    fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self)
        } else {
            format!("{:#?}", &self)
        }
    }

    fn rewards(&self) -> Option<ClaimRewardsTransaction> {
        match &self {
            ledger::structure::Transaction::ClaimRewards(tx) => Some(tx.clone().to_wasm()),
            ledger::structure::Transaction::Standard(_) => None,
        }
    }

    fn binding_randomness(&self) -> Result<BigInt, JsError> {
        let binding_randomness: Option<PedersenRandomness> = match &self {
            ledger::structure::Transaction::Standard(stx) => Some(stx.binding_randomness),
            ledger::structure::Transaction::ClaimRewards(_) => None,
        };
        match binding_randomness {
            Some(binding_randomness) => {
                let bytes = binding_randomness.as_le_bytes();
                let result_as_hex = format!("0x{}", bytes.encode_hex::<String>());
                BigInt::new(&result_as_hex.into())
                    .map_err(|err| JsError::new(&String::from(err.to_string())))
            }
            None => Err(JsError::new(
                "Unable to get the binding randomness from non-standard transaction.",
            )),
        }
    }

    fn guaranteed_offer(&self) -> Option<ZswapOffer> {
        match &self {
            ledger::structure::Transaction::Standard(stx) => stx
                .guaranteed_coins
                .clone()
                .map(|sp| ZswapOffer::from(sp.deref().clone())),
            _ => None,
        }
    }

    fn fallible_offer(&self) -> Option<Map> {
        use ledger::structure::Transaction::Standard;

        match &self {
            Standard(tx) => fallible_coins_to_value_map::<P>(tx.fallible_coins.clone()),
            _ => None,
        }
    }

    fn intents(&self) -> Option<Map> {
        use ledger::structure::Transaction::Standard;
        let map = match &self {
            Standard(tx) => intents_to_value_map(tx.intents.clone()),
            _ => Map::new(),
        };

        if map.size() == 0 { None } else { Some(map) }
    }

    fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    fn identifiers(&self) -> Result<Vec<JsString>, JsError> {
        self.identifiers()
            .map(|id| to_hex_ser(&id).map(JsString::from))
            .collect::<Result<Vec<_>, _>>()
    }

    fn well_formed(
        &self,
        ref_state: &LedgerState,
        strictness: &WellFormedStrictness,
        tblock: &Date,
    ) -> Result<VerifiedTransaction, JsError> {
        let tblock = Timestamp::from_secs(js_date_to_seconds(tblock));
        Ok(VerifiedTransaction(self.well_formed(
            &ref_state.0,
            strictness.0,
            tblock,
        )?))
    }
}

fn get_dyn_transaction(tx: TransactionTypes) -> Box<dyn Transactionable> {
    match tx {
        TransactionTypes::UnprovenWithSignaturePreBinding(val) => Box::new(val),
        TransactionTypes::UnprovenWithSignatureBinding(val) => Box::new(val),
        TransactionTypes::UnprovenWithSignatureErasedPreBinding(val) => Box::new(val),
        TransactionTypes::UnprovenWithSignatureErasedBinding(val) => Box::new(val),
        TransactionTypes::ProvenWithSignaturePreBinding(val) => Box::new(val),
        TransactionTypes::ProvenWithSignatureBinding(val) => Box::new(val),
        TransactionTypes::ProvenWithSignatureErasedPreBinding(val) => Box::new(val),
        TransactionTypes::ProvenWithSignatureErasedBinding(val) => Box::new(val),
        TransactionTypes::ProofErasedWithSignatureNoBinding(val) => Box::new(val),
        TransactionTypes::ProofErasedWithSignatureErasedNoBinding(val) => Box::new(val),
    }
}

#[wasm_bindgen]
pub struct WellFormedStrictness(pub(crate) ledger::verify::WellFormedStrictness);

#[wasm_bindgen]
impl WellFormedStrictness {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        WellFormedStrictness(Default::default())
    }

    #[wasm_bindgen(getter = enforceBalancing)]
    pub fn enforce_balancing(&self) -> bool {
        self.0.enforce_balancing
    }

    #[wasm_bindgen(getter = verifyNativeProofs)]
    pub fn verify_native_proofs(&self) -> bool {
        self.0.verify_native_proofs
    }

    #[wasm_bindgen(getter = verifyContractProofs)]
    pub fn verify_contract_proofs(&self) -> bool {
        self.0.verify_contract_proofs
    }

    #[wasm_bindgen(getter = enforceLimits)]
    pub fn enforce_limits(&self) -> bool {
        self.0.enforce_limits
    }

    #[wasm_bindgen(getter = verifySignatures)]
    pub fn verify_signatures(&self) -> bool {
        self.0.verify_signatures
    }

    #[wasm_bindgen(setter = enforceBalancing)]
    pub fn set_enforce_balancing(&mut self, value: bool) {
        self.0.enforce_balancing = value;
    }

    #[wasm_bindgen(setter = verifyNativeProofs)]
    pub fn set_verify_native_proofs(&mut self, value: bool) {
        self.0.verify_native_proofs = value;
    }

    #[wasm_bindgen(setter = verifyContractProofs)]
    pub fn set_verify_contract_proofs(&mut self, value: bool) {
        self.0.verify_contract_proofs = value;
    }

    #[wasm_bindgen(setter = enforceLimits)]
    pub fn set_enforce_limits(&mut self, value: bool) {
        self.0.enforce_limits = value;
    }

    #[wasm_bindgen(setter = verifySignatures)]
    pub fn set_verify_signatures(&mut self, value: bool) {
        self.0.verify_signatures = value;
    }
}

#[derive(Clone)]
pub enum ClaimRewardsTransactionTypes {
    SignatureClaimRewards(ledger::structure::ClaimRewardsTransaction<Signature, InMemoryDB>),
    SignatureErasedClaimRewards(ledger::structure::ClaimRewardsTransaction<(), InMemoryDB>),
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct ClaimRewardsTransaction(pub(crate) ClaimRewardsTransactionTypes);

impl From<ledger::structure::ClaimRewardsTransaction<Signature, InMemoryDB>>
    for ClaimRewardsTransaction
{
    fn from(
        inner: ledger::structure::ClaimRewardsTransaction<Signature, InMemoryDB>,
    ) -> ClaimRewardsTransaction {
        ClaimRewardsTransaction(ClaimRewardsTransactionTypes::SignatureClaimRewards(inner))
    }
}

impl From<ledger::structure::ClaimRewardsTransaction<(), InMemoryDB>> for ClaimRewardsTransaction {
    fn from(
        inner: ledger::structure::ClaimRewardsTransaction<(), InMemoryDB>,
    ) -> ClaimRewardsTransaction {
        ClaimRewardsTransaction(ClaimRewardsTransactionTypes::SignatureErasedClaimRewards(
            inner,
        ))
    }
}

#[wasm_bindgen]
impl ClaimRewardsTransaction {
    #[wasm_bindgen(constructor)]
    pub fn new(
        signature_marker: &str,
        network_id: String,
        value: BigInt,
        owner: &str,
        nonce: &str,
        signature: JsValue,
    ) -> Result<ClaimRewardsTransaction, JsError> {
        let owner: signatures::VerifyingKey = from_value_hex_ser(owner)?;
        let value = u128::try_from(value).map_err(|_| JsError::new("value is out of range"))?;
        let nonce = Nonce(from_hex_ser(nonce)?);

        use ClaimRewardsTransactionTypes::*;
        use Signaturish::*;
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;

        Ok(ClaimRewardsTransaction(match signature_type {
            Signature => {
                let signature = crate::crypto::SignatureEnabled::try_ref(&signature)?.ok_or(
                    JsError::new("Unable to decode Signature as SignatureEnabled."),
                )?;
                SignatureClaimRewards(ledger::structure::ClaimRewardsTransaction {
                    network_id,
                    value,
                    owner,
                    nonce,
                    signature: signature.deref().0.clone(),
                    kind: ledger::structure::ClaimKind::Reward,
                })
            }
            SignatureErased => {
                let _ = crate::crypto::SignatureErased::try_ref(&signature)?.ok_or(
                    JsError::new("Unable to decode Signature as SignatureErased."),
                )?;
                SignatureErasedClaimRewards(ledger::structure::ClaimRewardsTransaction {
                    network_id,
                    value,
                    owner,
                    nonce,
                    signature: (),
                    kind: ledger::structure::ClaimKind::Reward,
                })
            }
        }))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use ClaimRewardsTransactionTypes::*;
        let mut res = vec![];
        match &self.0 {
            SignatureClaimRewards(val) => tagged_serialize(&val, &mut res)?,
            SignatureErasedClaimRewards(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(
        signature_marker: &str,
        raw: Uint8Array,
    ) -> Result<ClaimRewardsTransaction, JsError> {
        use ClaimRewardsTransactionTypes::*;
        use Signaturish::*;
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;
        Ok(match signature_type {
            Signature => ClaimRewardsTransaction(SignatureClaimRewards(from_value_ser(
                raw,
                "ClaimRewardsTransaction",
            )?)),
            SignatureErased => ClaimRewardsTransaction(SignatureErasedClaimRewards(
                from_value_ser(raw, "ClaimRewardsTransaction")?,
            )),
        })
    }

    #[wasm_bindgen(js_name = "eraseSignatures")]
    pub fn erase_signatures(&self) -> Result<ClaimRewardsTransaction, JsError> {
        use ClaimRewardsTransactionTypes::*;
        Ok(match &self.0 {
            SignatureClaimRewards(val) => {
                ClaimRewardsTransaction(SignatureErasedClaimRewards(val.erase_signatures()))
            }
            SignatureErasedClaimRewards(val) => {
                ClaimRewardsTransaction(SignatureErasedClaimRewards(val.erase_signatures()))
            }
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use ClaimRewardsTransactionTypes::*;
        match &self.0 {
            SignatureClaimRewards(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            SignatureErasedClaimRewards(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(getter, js_name = "dataToSign")]
    pub fn data_to_sign(&self) -> Uint8Array {
        use ClaimRewardsTransactionTypes::*;
        match &self.0 {
            SignatureClaimRewards(val) => val.data_to_sign().as_slice().into(),
            SignatureErasedClaimRewards(val) => val.data_to_sign().as_slice().into(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn value(&self) -> BigInt {
        use ClaimRewardsTransactionTypes::*;
        match &self.0 {
            SignatureClaimRewards(val) => BigInt::from(val.value.clone()),
            SignatureErasedClaimRewards(val) => BigInt::from(val.value.clone()),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn owner(&self) -> Result<String, JsError> {
        use ClaimRewardsTransactionTypes::*;
        match &self.0 {
            SignatureClaimRewards(val) => to_hex_ser(&val.owner),
            SignatureErasedClaimRewards(val) => to_hex_ser(&val.owner),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn nonce(&self) -> Result<String, JsError> {
        use ClaimRewardsTransactionTypes::*;
        match &self.0 {
            SignatureClaimRewards(val) => to_hex_ser(&val.nonce.0),
            SignatureErasedClaimRewards(val) => to_hex_ser(&val.nonce.0),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        use ClaimRewardsTransactionTypes::*;
        match &self.0 {
            SignatureClaimRewards(val) => claim_kind_to_text(&val.kind),
            SignatureErasedClaimRewards(val) => claim_kind_to_text(&val.kind),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn signature(&self) -> Result<JsValue, JsError> {
        use crate::crypto::{SignatureEnabled, SignatureErased};
        use ClaimRewardsTransactionTypes::*;
        Ok(match &self.0 {
            SignatureClaimRewards(val) => JsValue::from(SignatureEnabled(val.signature.clone())),
            SignatureErasedClaimRewards(_) => JsValue::from(SignatureErased()),
        })
    }
}

#[wasm_bindgen]
pub struct SystemTransaction(pub(crate) ledger::structure::SystemTransaction);

impl AsRef<ledger::structure::SystemTransaction> for SystemTransaction {
    fn as_ref(&self) -> &ledger::structure::SystemTransaction {
        &self.0
    }
}

impl From<ledger::structure::SystemTransaction> for SystemTransaction {
    fn from(inner: ledger::structure::SystemTransaction) -> Self {
        SystemTransaction(inner)
    }
}

#[wasm_bindgen]
impl SystemTransaction {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<SystemTransaction, JsError> {
        Err(JsError::new(
            "SystemTransaction cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<SystemTransaction, JsError> {
        Ok(SystemTransaction(from_value_ser(raw, "SystemTransaction")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}

#[wasm_bindgen]
pub struct TransactionContext(pub(crate) ledger::semantics::TransactionContext<InMemoryDB>);

#[wasm_bindgen]
impl TransactionContext {
    #[wasm_bindgen(constructor)]
    pub fn new(
        ref_state: &LedgerState,
        block_context: JsValue,
        whitelist: JsValue,
    ) -> Result<TransactionContext, JsError> {
        let block_context = from_value(block_context)?;
        let whitelist = whitelist_from_value(whitelist)?;
        Ok(TransactionContext(ledger::semantics::TransactionContext {
            ref_state: ref_state.0.clone(),
            block_context,
            whitelist,
        }))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }
}

#[wasm_bindgen]
pub struct TransactionResult(pub(crate) ledger::semantics::TransactionResult<InMemoryDB>);

#[wasm_bindgen]
impl TransactionResult {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<TransactionResult, JsError> {
        Err(JsError::new(
            "TransactionResult cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(getter = type)]
    pub fn type_(&self) -> String {
        use ledger::semantics::TransactionResult::*;
        match &self.0 {
            Success(..) => "success",
            PartialSuccess(..) => "partialSuccess",
            Failure(..) => "failure",
        }
        .to_owned()
    }

    #[wasm_bindgen(getter = error)]
    pub fn error(&self) -> Option<String> {
        use ledger::semantics::TransactionResult::*;
        match &self.0 {
            Success(..) => None,
            PartialSuccess(e, ..) => e
                .values()
                .find_map(|item| match item {
                    Ok(_) => None,
                    Err(e) => Some(e),
                })
                .map(|e| format!("{e}")),
            Failure(e) => Some(format!("{e}")),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn events(&self) -> Vec<Event> {
        self.0
            .events()
            .iter()
            .map(|event| Event::from(event.clone()))
            .collect()
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter, js_name = "successfulSegments")]
    pub fn successful_segments(&self) -> Option<Map> {
        use ledger::semantics::TransactionResult::*;
        match &self.0 {
            Success(..) => None,
            PartialSuccess(e, ..) => {
                let res = Map::new();
                for (k, v) in e {
                    res.set(&JsValue::from(k.clone()), &JsValue::from(v.is_err()));
                }
                Some(res)
            }
            Failure(_) => None,
        }
    }
}
