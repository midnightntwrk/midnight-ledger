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
use crate::dust::DustParameters;
use crate::zswap_state::ZswapChainState;
use base_crypto::hash::HashOutput;
use coin_structure::coin::ShieldedTokenType;
use js_sys::{JsString, Map, Uint8Array};
use onchain_runtime_wasm::context::CostModel;
use onchain_runtime_wasm::from_value_ser;
use rand::rngs::OsRng;
use serialize::tagged_serialize;
use std::collections::HashSet;
use std::ops::Deref;
use storage::db::InMemoryDB;
use transient_crypto::proofs::Proof;
use transient_crypto::proofs::ProofPreimage;
use wasm_bindgen::prelude::*;
use zswap::Delta;

pub enum ZswapTransientTypes {
    ProvenTransient(zswap::Transient<Proof, InMemoryDB>),
    UnprovenTransient(zswap::Transient<ProofPreimage, InMemoryDB>),
    ProofErasedTransient(zswap::Transient<(), InMemoryDB>),
}

#[wasm_bindgen]
#[repr(transparent)]
pub struct ZswapTransient(ZswapTransientTypes);

impl From<zswap::Transient<ProofPreimage, InMemoryDB>> for ZswapTransient {
    fn from(inner: zswap::Transient<ProofPreimage, InMemoryDB>) -> ZswapTransient {
        ZswapTransient(ZswapTransientTypes::UnprovenTransient(inner))
    }
}
impl From<zswap::Transient<Proof, InMemoryDB>> for ZswapTransient {
    fn from(inner: zswap::Transient<Proof, InMemoryDB>) -> ZswapTransient {
        ZswapTransient(ZswapTransientTypes::ProvenTransient(inner))
    }
}
impl From<zswap::Transient<(), InMemoryDB>> for ZswapTransient {
    fn from(inner: zswap::Transient<(), InMemoryDB>) -> ZswapTransient {
        ZswapTransient(ZswapTransientTypes::ProofErasedTransient(inner))
    }
}

#[wasm_bindgen]
impl ZswapTransient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<ZswapTransient, JsError> {
        Err(JsError::new(
            "ZswapTransient cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "newFromContractOwnedOutput")]
    pub fn new_from_contract_owned_output(
        coin: JsValue,
        segment: u16,
        output: &ZswapOutput,
    ) -> Result<ZswapTransient, JsError> {
        match &output.0 {
            ZswapOutputTypes::UnprovenOutput(val) => {
                let coin = value_to_qualified_shielded_coininfo(coin)?;
                Ok(ZswapTransient(ZswapTransientTypes::UnprovenTransient(
                    zswap::Transient::new_from_contract_owned_output(
                        &mut OsRng,
                        &coin,
                        segment,
                        val.clone(),
                    )?,
                )))
            }
            _ => Err(JsError::new(
                "ZswapTransient cannot be constructed from a proven or proof-erased transient.",
            )),
        }
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use ZswapTransientTypes::*;
        let mut res = Vec::new();
        match &self.0 {
            ProvenTransient(val) => tagged_serialize(&val, &mut res)?,
            UnprovenTransient(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedTransient(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(proof_marker: &str, raw: Uint8Array) -> Result<ZswapTransient, JsError> {
        use ZswapTransientTypes::*;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;
        Ok(match proof_type {
            Proofish::Proof => {
                ZswapTransient(ProvenTransient(from_value_ser(raw, "ZswapTransient")?))
            }
            Proofish::PreProof => {
                ZswapTransient(UnprovenTransient(from_value_ser(raw, "ZswapTransient")?))
            }
            Proofish::NoProof => {
                ZswapTransient(ProofErasedTransient(from_value_ser(raw, "ZswapTransient")?))
            }
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use ZswapTransientTypes::*;
        match &self.0 {
            ProvenTransient(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenTransient(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedTransient(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(getter)]
    pub fn commitment(&self) -> Result<String, JsError> {
        use ZswapTransientTypes::*;
        match &self.0 {
            ProvenTransient(val) => to_hex_ser(&val.coin_com),
            UnprovenTransient(val) => to_hex_ser(&val.coin_com),
            ProofErasedTransient(val) => to_hex_ser(&val.coin_com),
        }
    }

    #[wasm_bindgen(getter, js_name = "contractAddress")]
    pub fn contract_address(&self) -> Result<Option<String>, JsError> {
        use ZswapTransientTypes::*;
        match &self.0 {
            ProvenTransient(val) => val.contract_address.clone().map(|x| x.deref().clone()),
            UnprovenTransient(val) => val.contract_address.clone().map(|x| x.deref().clone()),
            ProofErasedTransient(val) => val.contract_address.clone().map(|x| x.deref().clone()),
        }
        .map(|v| to_hex_ser(&v))
        .transpose()
    }

    #[wasm_bindgen(getter)]
    pub fn nullifier(&self) -> Result<String, JsError> {
        use ZswapTransientTypes::*;
        match &self.0 {
            ProvenTransient(val) => to_hex_ser(&val.nullifier),
            UnprovenTransient(val) => to_hex_ser(&val.nullifier),
            ProofErasedTransient(val) => to_hex_ser(&val.nullifier),
        }
    }

    #[wasm_bindgen(getter, js_name = "inputProof")]
    pub fn input_proof(&self) -> Result<JsValue, JsError> {
        use crate::crypto::{NoProof, PreProof, Proof};
        use ZswapTransientTypes::*;
        Ok(match &self.0 {
            ProvenTransient(val) => JsValue::from(Proof(val.proof_input.clone().into())),
            UnprovenTransient(val) => JsValue::from(PreProof(val.proof_input.clone().into())),
            ProofErasedTransient(_) => JsValue::from(NoProof()),
        })
    }

    #[wasm_bindgen(getter, js_name = "outputProof")]
    pub fn output_proof(&self) -> Result<JsValue, JsError> {
        use crate::crypto::{NoProof, PreProof, Proof};
        use ZswapTransientTypes::*;
        Ok(match &self.0 {
            ProvenTransient(val) => JsValue::from(Proof(val.proof_output.clone().into())),
            UnprovenTransient(val) => JsValue::from(PreProof(val.proof_output.clone().into())),
            ProofErasedTransient(_) => JsValue::from(NoProof()),
        })
    }
}

#[derive(Clone)]
pub enum ZswapOutputTypes {
    ProvenOutput(zswap::Output<Proof, InMemoryDB>),
    UnprovenOutput(zswap::Output<ProofPreimage, InMemoryDB>),
    ProofErasedOutput(zswap::Output<(), InMemoryDB>),
}

#[wasm_bindgen]
#[derive(Clone)]
#[repr(transparent)]
pub struct ZswapOutput(ZswapOutputTypes);

impl From<zswap::Output<ProofPreimage, InMemoryDB>> for ZswapOutput {
    fn from(inner: zswap::Output<ProofPreimage, InMemoryDB>) -> ZswapOutput {
        ZswapOutput(ZswapOutputTypes::UnprovenOutput(inner))
    }
}
impl From<zswap::Output<Proof, InMemoryDB>> for ZswapOutput {
    fn from(inner: zswap::Output<Proof, InMemoryDB>) -> ZswapOutput {
        ZswapOutput(ZswapOutputTypes::ProvenOutput(inner))
    }
}
impl From<zswap::Output<(), InMemoryDB>> for ZswapOutput {
    fn from(inner: zswap::Output<(), InMemoryDB>) -> ZswapOutput {
        ZswapOutput(ZswapOutputTypes::ProofErasedOutput(inner))
    }
}

impl TryFrom<ZswapOutput> for zswap::Output<ProofPreimage, InMemoryDB> {
    type Error = JsError;
    fn try_from(
        outer: ZswapOutput,
    ) -> Result<zswap::Output<ProofPreimage, InMemoryDB>, Self::Error> {
        match &outer.0 {
            ZswapOutputTypes::UnprovenOutput(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported ZswapOutput type provided.")),
        }
    }
}

#[wasm_bindgen]
impl ZswapOutput {
    #[wasm_bindgen(constructor)]
    pub fn construct() -> Result<ZswapOutput, JsError> {
        Err(JsError::new(
            "ZswapOutput cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn new(
        coin: JsValue,
        segment: u16,
        target_cpk: &str,
        target_epk: &str,
    ) -> Result<ZswapOutput, JsError> {
        let coin = value_to_shielded_coininfo(coin)?;
        let target_cpk = from_hex_ser(target_cpk)?;
        let target_epk = from_hex_ser(target_epk)?;
        Ok(ZswapOutput(ZswapOutputTypes::UnprovenOutput(
            zswap::Output::new(&mut OsRng, &coin, segment, &target_cpk, Some(target_epk))?,
        )))
    }

    #[wasm_bindgen(js_name = "newContractOwned")]
    pub fn new_contract_owned(
        coin: JsValue,
        segment: u16,
        contract: &str,
    ) -> Result<ZswapOutput, JsError> {
        let coin = value_to_shielded_coininfo(coin)?;
        let contract = from_hex_ser(contract)?;
        Ok(ZswapOutput(ZswapOutputTypes::UnprovenOutput(
            zswap::Output::new_contract_owned(&mut OsRng, &coin, segment, contract)?,
        )))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use ZswapOutputTypes::*;
        let mut res = Vec::new();
        match &self.0 {
            ProvenOutput(val) => tagged_serialize(&val, &mut res)?,
            UnprovenOutput(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedOutput(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(proof_marker: &str, raw: Uint8Array) -> Result<ZswapOutput, JsError> {
        use ZswapOutputTypes::*;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;
        Ok(match proof_type {
            Proofish::Proof => ZswapOutput(ProvenOutput(from_value_ser(raw, "ZswapOutput")?)),
            Proofish::PreProof => ZswapOutput(UnprovenOutput(from_value_ser(raw, "ZswapOutput")?)),
            Proofish::NoProof => {
                ZswapOutput(ProofErasedOutput(from_value_ser(raw, "ZswapOutput")?))
            }
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use ZswapOutputTypes::*;
        match &self.0 {
            ProvenOutput(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenOutput(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedOutput(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(getter)]
    pub fn commitment(&self) -> Result<String, JsError> {
        use ZswapOutputTypes::*;
        match &self.0 {
            ProvenOutput(val) => to_hex_ser(&val.coin_com),
            UnprovenOutput(val) => to_hex_ser(&val.coin_com),
            ProofErasedOutput(val) => to_hex_ser(&val.coin_com),
        }
    }

    #[wasm_bindgen(getter, js_name = "contractAddress")]
    pub fn contract_address(&self) -> Result<Option<String>, JsError> {
        use ZswapOutputTypes::*;
        match &self.0 {
            ProvenOutput(val) => val.contract_address.clone().map(|x| x.deref().clone()),
            UnprovenOutput(val) => val.contract_address.clone().map(|x| x.deref().clone()),
            ProofErasedOutput(val) => val.contract_address.clone().map(|x| x.deref().clone()),
        }
        .map(|v| to_hex_ser(&v))
        .transpose()
    }

    #[wasm_bindgen(getter)]
    pub fn proof(&self) -> Result<JsValue, JsError> {
        use crate::crypto::{NoProof, PreProof, Proof};
        use ZswapOutputTypes::*;
        Ok(match &self.0 {
            ProvenOutput(val) => JsValue::from(Proof(val.proof.clone().into())),
            UnprovenOutput(val) => JsValue::from(PreProof(val.proof.clone().into())),
            ProofErasedOutput(_) => JsValue::from(NoProof()),
        })
    }
}

pub enum ZswapInputTypes {
    ProvenInput(zswap::Input<Proof, InMemoryDB>),
    UnprovenInput(zswap::Input<ProofPreimage, InMemoryDB>),
    ProofErasedInput(zswap::Input<(), InMemoryDB>),
}

#[wasm_bindgen]
#[repr(transparent)]
pub struct ZswapInput(ZswapInputTypes);

impl From<zswap::Input<ProofPreimage, InMemoryDB>> for ZswapInput {
    fn from(inner: zswap::Input<ProofPreimage, InMemoryDB>) -> ZswapInput {
        ZswapInput(ZswapInputTypes::UnprovenInput(inner))
    }
}
impl From<zswap::Input<Proof, InMemoryDB>> for ZswapInput {
    fn from(inner: zswap::Input<Proof, InMemoryDB>) -> ZswapInput {
        ZswapInput(ZswapInputTypes::ProvenInput(inner))
    }
}
impl From<zswap::Input<(), InMemoryDB>> for ZswapInput {
    fn from(inner: zswap::Input<(), InMemoryDB>) -> ZswapInput {
        ZswapInput(ZswapInputTypes::ProofErasedInput(inner))
    }
}

#[wasm_bindgen]
impl ZswapInput {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<ZswapInput, JsError> {
        Err(JsError::new(
            "ZswapInput cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "newContractOwned")]
    pub fn new_contract_owned(
        coin: JsValue,
        segment: u16,
        contract: &str,
        state: &ZswapChainState,
    ) -> Result<ZswapInput, JsError> {
        let coin = value_to_qualified_shielded_coininfo(coin)?;
        let addr = from_hex_ser(contract)?;
        Ok(ZswapInput(ZswapInputTypes::UnprovenInput(
            zswap::Input::new_contract_owned(&mut OsRng, &coin, segment, addr, &state.0.coin_coms)?,
        )))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use ZswapInputTypes::*;
        let mut res = Vec::new();
        match &self.0 {
            ProvenInput(val) => tagged_serialize(&val, &mut res)?,
            UnprovenInput(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedInput(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(proof_marker: &str, raw: Uint8Array) -> Result<ZswapInput, JsError> {
        use ZswapInputTypes::*;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;
        Ok(match proof_type {
            Proofish::Proof => ZswapInput(ProvenInput(from_value_ser(raw, "ZswapInput")?)),
            Proofish::PreProof => ZswapInput(UnprovenInput(from_value_ser(raw, "ZswapInput")?)),
            Proofish::NoProof => ZswapInput(ProofErasedInput(from_value_ser(raw, "ZswapInput")?)),
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use ZswapInputTypes::*;
        match &self.0 {
            ProvenInput(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenInput(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedInput(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(getter, js_name = "contractAddress")]
    pub fn contract_address(&self) -> Result<Option<String>, JsError> {
        use ZswapInputTypes::*;
        match &self.0 {
            ProvenInput(val) => val.contract_address.clone().map(|x| x.deref().clone()),
            UnprovenInput(val) => val.contract_address.clone().map(|x| x.deref().clone()),
            ProofErasedInput(val) => val.contract_address.clone().map(|x| x.deref().clone()),
        }
        .map(|v| to_hex_ser(&v))
        .transpose()
    }

    #[wasm_bindgen(getter)]
    pub fn nullifier(&self) -> Result<String, JsError> {
        use ZswapInputTypes::*;
        match &self.0 {
            ProvenInput(val) => to_hex_ser(&val.nullifier),
            UnprovenInput(val) => to_hex_ser(&val.nullifier),
            ProofErasedInput(val) => to_hex_ser(&val.nullifier),
        }
    }

    #[wasm_bindgen(getter, js_name = "proof")]
    pub fn proof(&self) -> Result<JsValue, JsError> {
        use crate::crypto::{NoProof, PreProof, Proof};
        use ZswapInputTypes::*;
        Ok(match &self.0 {
            ProvenInput(val) => JsValue::from(Proof(val.proof.clone().into())),
            UnprovenInput(val) => JsValue::from(PreProof(val.proof.clone().into())),
            ProofErasedInput(_) => JsValue::from(NoProof()),
        })
    }
}

#[derive(Clone)]
pub enum ZswapOfferTypes {
    ProvenOffer(zswap::Offer<Proof, InMemoryDB>),
    UnprovenOffer(zswap::Offer<ProofPreimage, InMemoryDB>),
    ProofErasedOffer(zswap::Offer<(), InMemoryDB>),
}

#[derive(Clone)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct ZswapOffer(pub(crate) ZswapOfferTypes);

try_ref_for_exported!(ZswapOffer);

impl From<zswap::Offer<ProofPreimage, InMemoryDB>> for ZswapOffer {
    fn from(inner: zswap::Offer<ProofPreimage, InMemoryDB>) -> ZswapOffer {
        ZswapOffer(ZswapOfferTypes::UnprovenOffer(inner))
    }
}

impl From<zswap::Offer<Proof, InMemoryDB>> for ZswapOffer {
    fn from(inner: zswap::Offer<Proof, InMemoryDB>) -> ZswapOffer {
        ZswapOffer(ZswapOfferTypes::ProvenOffer(inner))
    }
}

impl From<zswap::Offer<(), InMemoryDB>> for ZswapOffer {
    fn from(inner: zswap::Offer<(), InMemoryDB>) -> ZswapOffer {
        ZswapOffer(ZswapOfferTypes::ProofErasedOffer(inner))
    }
}

impl TryFrom<ZswapOffer> for zswap::Offer<ProofPreimage, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: ZswapOffer) -> Result<zswap::Offer<ProofPreimage, InMemoryDB>, Self::Error> {
        match &outer.0 {
            ZswapOfferTypes::UnprovenOffer(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported ZswapOffer type provided.")),
        }
    }
}

impl TryFrom<ZswapOffer> for zswap::Offer<Proof, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: ZswapOffer) -> Result<zswap::Offer<Proof, InMemoryDB>, Self::Error> {
        match &outer.0 {
            ZswapOfferTypes::ProvenOffer(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported ZswapOffer type provided.")),
        }
    }
}

impl TryFrom<ZswapOffer> for zswap::Offer<(), InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: ZswapOffer) -> Result<zswap::Offer<(), InMemoryDB>, Self::Error> {
        match &outer.0 {
            ZswapOfferTypes::ProofErasedOffer(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported ZswapOffer type provided.")),
        }
    }
}

#[wasm_bindgen]
impl ZswapOffer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<ZswapOffer, JsError> {
        Err(JsError::new(
            "ZswapOffer cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "fromInput")]
    pub fn from_input(input: &ZswapInput, type_: &str, value: u128) -> Result<ZswapOffer, JsError> {
        match &input.0 {
            ZswapInputTypes::UnprovenInput(val) => {
                let type_: HashOutput = from_hex_ser(type_)?;
                Ok(ZswapOffer(ZswapOfferTypes::UnprovenOffer(zswap::Offer {
                    inputs: vec![val.clone()].into(),
                    outputs: vec![].into(),
                    transient: vec![].into(),
                    deltas: vec![Delta {
                        token_type: ShieldedTokenType(type_),
                        value: value as i128,
                    }]
                    .into(),
                })))
            }
            _ => Err(JsError::new(
                "ZswapOffer cannot be constructed from a proven or proof-erased input.",
            )),
        }
    }

    #[wasm_bindgen(js_name = "fromOutput")]
    pub fn from_output(
        output: &ZswapOutput,
        type_: &str,
        value: u128,
    ) -> Result<ZswapOffer, JsError> {
        match &output.0 {
            ZswapOutputTypes::UnprovenOutput(val) => {
                let type_: HashOutput = from_hex_ser(type_)?;
                Ok(ZswapOffer(ZswapOfferTypes::UnprovenOffer(zswap::Offer {
                    inputs: vec![].into(),
                    outputs: vec![val.clone()].into(),
                    transient: vec![].into(),
                    deltas: vec![Delta {
                        token_type: ShieldedTokenType(type_),
                        value: -(value as i128),
                    }]
                    .into(),
                })))
            }
            _ => Err(JsError::new(
                "ZswapOffer cannot be constructed from a proven or proof-erased output.",
            )),
        }
    }

    #[wasm_bindgen(js_name = "fromTransient")]
    pub fn from_transient(transient: &ZswapTransient) -> Result<ZswapOffer, JsError> {
        match &transient.0 {
            ZswapTransientTypes::UnprovenTransient(val) => {
                Ok(ZswapOffer(ZswapOfferTypes::UnprovenOffer(zswap::Offer {
                    inputs: vec![].into(),
                    outputs: vec![].into(),
                    transient: vec![val.clone()].into(),
                    deltas: vec![].into(),
                })))
            }
            _ => Err(JsError::new(
                "ZswapOffer cannot be constructed from a proven or proof-erased transient.",
            )),
        }
    }

    pub fn merge(&self, other: &ZswapOffer) -> Result<ZswapOffer, JsError> {
        use ZswapOfferTypes::*;
        match (&self.0, &other.0) {
            (ProvenOffer(self_val), ProvenOffer(other_val)) => {
                Ok(ZswapOffer(ProvenOffer(self_val.merge(&other_val)?)))
            }
            (UnprovenOffer(self_val), UnprovenOffer(other_val)) => {
                let self_segment_id = offer_segment_id(&self_val)?;
                let other_segment_id = offer_segment_id(&other_val)?;
                if self_segment_id != other_segment_id {
                    return Err(JsError::new(&format!(
                        "Mismatched output segments. Self: {:?}, Other: {:?}",
                        self_segment_id, other_segment_id
                    )));
                }
                Ok(ZswapOffer(UnprovenOffer(self_val.merge(&other_val)?)))
            }
            (ProofErasedOffer(self_val), ProofErasedOffer(other_val)) => {
                Ok(ZswapOffer(ProofErasedOffer(self_val.merge(&other_val)?)))
            }
            _ => Err(JsError::new(
                "Only ZswapOffers of the same proof type can be merged with each other.",
            )),
        }
    }
    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use ZswapOfferTypes::*;
        let mut res = Vec::new();
        match &self.0 {
            ProvenOffer(val) => tagged_serialize(&val, &mut res)?,
            UnprovenOffer(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedOffer(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(proof_marker: &str, raw: Uint8Array) -> Result<ZswapOffer, JsError> {
        use ZswapOfferTypes::*;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;
        Ok(match proof_type {
            Proofish::Proof => ZswapOffer(ProvenOffer(from_value_ser(raw, "ZswapOffer")?)),
            Proofish::PreProof => ZswapOffer(UnprovenOffer(from_value_ser(raw, "ZswapOffer")?)),
            Proofish::NoProof => ZswapOffer(ProofErasedOffer(from_value_ser(raw, "ZswapOffer")?)),
        })
    }

    #[wasm_bindgen(getter)]
    pub fn inputs(&self) -> Vec<JsValue> {
        use ZswapOfferTypes::*;
        match &self.0 {
            ProvenOffer(val) => val
                .inputs
                .iter_deref()
                .cloned()
                .map(ZswapInput::from)
                .map(JsValue::from)
                .collect(),
            UnprovenOffer(val) => val
                .inputs
                .iter_deref()
                .cloned()
                .map(ZswapInput::from)
                .map(JsValue::from)
                .collect(),
            ProofErasedOffer(val) => val
                .inputs
                .iter_deref()
                .cloned()
                .map(ZswapInput::from)
                .map(JsValue::from)
                .collect(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn outputs(&self) -> Vec<JsValue> {
        use ZswapOfferTypes::*;
        match &self.0 {
            ProvenOffer(val) => val
                .outputs
                .iter_deref()
                .cloned()
                .map(ZswapOutput::from)
                .map(JsValue::from)
                .collect(),
            UnprovenOffer(val) => val
                .outputs
                .iter_deref()
                .cloned()
                .map(ZswapOutput::from)
                .map(JsValue::from)
                .collect(),
            ProofErasedOffer(val) => val
                .outputs
                .iter_deref()
                .cloned()
                .map(ZswapOutput::from)
                .map(JsValue::from)
                .collect(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn transients(&self) -> Vec<JsValue> {
        use ZswapOfferTypes::*;
        match &self.0 {
            ProvenOffer(val) => val
                .transient
                .iter_deref()
                .cloned()
                .map(ZswapTransient::from)
                .map(JsValue::from)
                .collect(),
            UnprovenOffer(val) => val
                .transient
                .iter_deref()
                .cloned()
                .map(ZswapTransient::from)
                .map(JsValue::from)
                .collect(),
            ProofErasedOffer(val) => val
                .transient
                .iter_deref()
                .cloned()
                .map(ZswapTransient::from)
                .map(JsValue::from)
                .collect(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn deltas(&self) -> Result<Map, JsError> {
        use ZswapOfferTypes::*;
        let res = Map::new();
        let deltas: Vec<Delta> = match &self.0 {
            ProvenOffer(val) => (&val.deltas).into(),
            UnprovenOffer(val) => (&val.deltas).into(),
            ProofErasedOffer(val) => (&val.deltas).into(),
        };
        for Delta { token_type, value } in deltas.iter() {
            res.set(
                &JsString::from(to_hex_ser(&token_type.0)?),
                &to_value(&value)?,
            );
        }
        Ok(res)
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use ZswapOfferTypes::*;
        match &self.0 {
            ProvenOffer(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenOffer(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedOffer(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct LedgerParameters(ledger::structure::LedgerParameters);

impl From<ledger::structure::LedgerParameters> for LedgerParameters {
    fn from(params: ledger::structure::LedgerParameters) -> Self {
        LedgerParameters(params)
    }
}

impl From<LedgerParameters> for ledger::structure::LedgerParameters {
    fn from(params: LedgerParameters) -> Self {
        params.0
    }
}

impl Deref for LedgerParameters {
    type Target = ledger::structure::LedgerParameters;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[wasm_bindgen]
impl LedgerParameters {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<LedgerParameters, JsError> {
        Err(JsError::new(
            "LedgerParameters cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "maxPriceAdjustment")]
    pub fn max_price_adjustment(&self) -> f64 {
        self.0.max_price_adjustment().into()
    }

    #[wasm_bindgen(js_name = "initialParameters")]
    pub fn initial_parameters() -> LedgerParameters {
        LedgerParameters(ledger::structure::INITIAL_PARAMETERS)
    }

    #[wasm_bindgen(getter, js_name = "transactionCostModel")]
    pub fn transaction_cost_model(&self) -> TransactionCostModel {
        TransactionCostModel(self.0.cost_model.clone())
    }

    #[wasm_bindgen(getter)]
    pub fn dust(&self) -> Result<DustParameters, JsError> {
        Ok(DustParameters(self.0.dust))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<LedgerParameters, JsError> {
        Ok(LedgerParameters(from_value_ser(raw, "LedgerParameters")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter, js_name = "feePrices")]
    pub fn fee_prices(&self) -> Result<JsValue, JsError> {
        fee_prices_to_value(&self.0.fee_prices)
    }
}

#[wasm_bindgen]
pub struct TransactionCostModel(ledger::structure::TransactionCostModel);

#[wasm_bindgen]
impl TransactionCostModel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<TransactionCostModel, JsError> {
        Err(JsError::new(
            "TransactionCostModel cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "initialTransactionCostModel")]
    pub fn initial_transaction_cost_model() -> TransactionCostModel {
        TransactionCostModel(ledger::structure::INITIAL_TRANSACTION_COST_MODEL)
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<TransactionCostModel, JsError> {
        Ok(TransactionCostModel(from_value_ser(
            raw,
            "TransactionCostModel",
        )?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter, js_name = "runtimeCostModel")]
    pub fn runtime_cost_model(&self) -> CostModel {
        CostModel::from(self.0.runtime_cost_model.clone())
    }

    #[wasm_bindgen(getter, js_name = "baselineCost")]
    pub fn baseline_cost(&self) -> Result<JsValue, JsError> {
        Ok(to_value(&self.0.baseline_cost.clone())?)
    }
}

#[derive(Clone)]
pub enum AuthorizedClaimTypes {
    ProvenAuthorizedClaim(zswap::AuthorizedClaim<Proof>),
    UnprovenAuthorizedClaim(zswap::AuthorizedClaim<ProofPreimage>),
    ProofErasedAuthorizedClaim(zswap::AuthorizedClaim<()>),
}

#[derive(Clone)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct AuthorizedClaim(pub(crate) AuthorizedClaimTypes);

impl From<zswap::AuthorizedClaim<Proof>> for AuthorizedClaim {
    fn from(inner: zswap::AuthorizedClaim<Proof>) -> AuthorizedClaim {
        AuthorizedClaim(AuthorizedClaimTypes::ProvenAuthorizedClaim(inner))
    }
}

impl From<zswap::AuthorizedClaim<ProofPreimage>> for AuthorizedClaim {
    fn from(inner: zswap::AuthorizedClaim<ProofPreimage>) -> AuthorizedClaim {
        AuthorizedClaim(AuthorizedClaimTypes::UnprovenAuthorizedClaim(inner))
    }
}

impl From<zswap::AuthorizedClaim<()>> for AuthorizedClaim {
    fn from(inner: zswap::AuthorizedClaim<()>) -> AuthorizedClaim {
        AuthorizedClaim(AuthorizedClaimTypes::ProofErasedAuthorizedClaim(inner))
    }
}

#[wasm_bindgen]
impl AuthorizedClaim {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<AuthorizedClaim, JsError> {
        Err(JsError::new(
            "AuthorizedClaim cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use AuthorizedClaimTypes::*;
        let mut res = vec![];
        match &self.0 {
            ProvenAuthorizedClaim(val) => tagged_serialize(&val, &mut res)?,
            UnprovenAuthorizedClaim(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedAuthorizedClaim(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(proof_marker: &str, raw: Uint8Array) -> Result<AuthorizedClaim, JsError> {
        use AuthorizedClaimTypes::*;
        use Proofish::*;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;
        Ok(match proof_type {
            Proof => AuthorizedClaim(ProvenAuthorizedClaim(from_value_ser(
                raw,
                "AuthorizedClaim",
            )?)),
            PreProof => AuthorizedClaim(UnprovenAuthorizedClaim(from_value_ser(
                raw,
                "AuthorizedClaim",
            )?)),
            NoProof => AuthorizedClaim(ProofErasedAuthorizedClaim(from_value_ser(
                raw,
                "AuthorizedClaim",
            )?)),
        })
    }

    #[wasm_bindgen(js_name = "eraseProof")]
    pub fn erase_proof(&self) -> Result<AuthorizedClaim, JsError> {
        use AuthorizedClaimTypes::*;
        match &self.0 {
            ProvenAuthorizedClaim(val) => Ok(AuthorizedClaim(ProofErasedAuthorizedClaim(
                val.erase_proof(),
            ))),
            UnprovenAuthorizedClaim(val) => Ok(AuthorizedClaim(ProofErasedAuthorizedClaim(
                val.erase_proof(),
            ))),
            ProofErasedAuthorizedClaim(_) => {
                Err(JsError::new("AuthorizedClaim is already proof-erased."))
            }
        }
    }

    #[wasm_bindgen(getter)]
    pub fn coin(&self) -> Result<JsValue, JsError> {
        use AuthorizedClaimTypes::*;
        Ok(to_value(match &self.0 {
            ProvenAuthorizedClaim(val) => &val.coin,
            UnprovenAuthorizedClaim(val) => &val.coin,
            ProofErasedAuthorizedClaim(val) => &val.coin,
        })?)
    }

    #[wasm_bindgen(getter)]
    pub fn recipient(&self) -> Result<String, JsError> {
        use AuthorizedClaimTypes::*;
        match &self.0 {
            ProvenAuthorizedClaim(val) => to_hex_ser(&val.recipient),
            UnprovenAuthorizedClaim(val) => to_hex_ser(&val.recipient),
            ProofErasedAuthorizedClaim(val) => to_hex_ser(&val.recipient),
        }
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use AuthorizedClaimTypes::*;
        match &self.0 {
            ProvenAuthorizedClaim(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenAuthorizedClaim(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedAuthorizedClaim(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }
}

pub(crate) fn offer_segment_id(
    offer: &zswap::Offer<ProofPreimage, InMemoryDB>,
) -> Result<Option<u16>, JsError> {
    let output_segments = offer
        .outputs
        .iter()
        .map(|output| output.segment())
        .flatten()
        .collect::<HashSet<_>>();

    if output_segments.len() > 1 {
        return Err(JsError::new(&format!(
            "Segment ids in zswap offer outputs should be equal. Received: {:?}",
            output_segments
        )));
    }
    let output_segment_id = output_segments.into_iter().next();

    let input_segments = offer
        .inputs
        .iter()
        .map(|input| input.segment())
        .flatten()
        .collect::<HashSet<_>>();

    if input_segments.len() > 1 {
        return Err(JsError::new(&format!(
            "Segment ids in zswap offer inputs should be equal. Received: {:?}",
            input_segments
        )));
    }
    let input_segment_id = input_segments.into_iter().next();

    let transient_segments = offer
        .transient
        .iter()
        .map(|transient| transient.segment())
        .flatten()
        .collect::<HashSet<_>>();

    if transient_segments.len() > 1 {
        return Err(JsError::new(&format!(
            "Segment ids in zswap offer transient should be equal. Received: {:?}",
            transient_segments
        )));
    }
    let transient_segment_id = transient_segments.into_iter().next();

    // ensure the segments in inputs, outputs and transient are equal
    let segments: HashSet<u16> = HashSet::from_iter(
        [output_segment_id, input_segment_id, transient_segment_id]
            .into_iter()
            .flatten(),
    );
    if segments.len() > 1 {
        return Err(JsError::new(&format!(
            "Segment ids in zswap offer should be equal. Received: outputs={:?}, inputs={:?}, transient={:?}",
            output_segment_id, input_segment_id, transient_segment_id,
        )));
    }
    Ok(segments.into_iter().next())
}
