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
use base_crypto::signatures;
use base_crypto::signatures::Signature;
use ledger::structure::UnshieldedOffer as LedgerUnshieldedOffer;
use storage::db::InMemoryDB;
use wasm_bindgen::prelude::*;

#[derive(Clone, Debug)]
pub enum UnshieldedOfferTypes {
    Signature(LedgerUnshieldedOffer<Signature, InMemoryDB>),
    SignatureErased(LedgerUnshieldedOffer<(), InMemoryDB>),
}

#[derive(Clone, Debug)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct UnshieldedOffer(pub(crate) UnshieldedOfferTypes);

try_ref_for_exported!(UnshieldedOffer);

impl From<LedgerUnshieldedOffer<Signature, InMemoryDB>> for UnshieldedOffer {
    fn from(offer: LedgerUnshieldedOffer<Signature, InMemoryDB>) -> UnshieldedOffer {
        UnshieldedOffer(UnshieldedOfferTypes::Signature(offer))
    }
}
impl From<LedgerUnshieldedOffer<(), InMemoryDB>> for UnshieldedOffer {
    fn from(offer: LedgerUnshieldedOffer<(), InMemoryDB>) -> UnshieldedOffer {
        UnshieldedOffer(UnshieldedOfferTypes::SignatureErased(offer))
    }
}

impl TryFrom<UnshieldedOffer> for LedgerUnshieldedOffer<Signature, InMemoryDB> {
    type Error = JsError;
    fn try_from(
        offer: UnshieldedOffer,
    ) -> Result<LedgerUnshieldedOffer<Signature, InMemoryDB>, Self::Error> {
        match &offer.0 {
            UnshieldedOfferTypes::Signature(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported UnshieldedOffer type provided.")),
        }
    }
}
impl TryFrom<UnshieldedOffer> for LedgerUnshieldedOffer<(), InMemoryDB> {
    type Error = JsError;
    fn try_from(
        offer: UnshieldedOffer,
    ) -> Result<LedgerUnshieldedOffer<(), InMemoryDB>, Self::Error> {
        match &offer.0 {
            UnshieldedOfferTypes::SignatureErased(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported UnshieldedOffer type provided.")),
        }
    }
}

#[wasm_bindgen]
impl UnshieldedOffer {
    #[wasm_bindgen(constructor)]
    pub fn construct() -> Result<UnshieldedOffer, JsError> {
        Err(JsError::new(
            "UnshieldedOffer cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn new(
        inputs: Vec<JsValue>,
        outputs: Vec<JsValue>,
        signatures: Vec<String>,
    ) -> Result<UnshieldedOffer, JsError> {
        use UnshieldedOfferTypes::*;
        let mut inputs = inputs
            .into_iter()
            .map(value_to_utxo_spend)
            .collect::<Result<Vec<_>, _>>()?;
        let mut outputs = outputs
            .into_iter()
            .map(value_to_utxo_output)
            .collect::<Result<Vec<_>, _>>()?;
        let mut signatures = signatures
            .into_iter()
            .map(|sig| Ok::<signatures::Signature, JsError>(from_hex_ser(&sig)?))
            .collect::<Result<Vec<_>, _>>()?;
        if signatures.len() == inputs.len() {
            signatures = {
                let mut input_sigs = inputs
                    .iter()
                    .zip(signatures.into_iter())
                    .collect::<Vec<_>>();
                input_sigs.sort();
                input_sigs.into_iter().map(|(_, s)| s).collect()
            };
        }
        inputs.sort();
        outputs.sort();
        Ok(UnshieldedOffer(Signature(LedgerUnshieldedOffer {
            inputs: inputs.into_iter().collect(),
            outputs: outputs.into_iter().collect(),
            signatures: signatures.into_iter().collect(),
        })))
    }

    #[wasm_bindgen(js_name = "eraseSignatures")]
    pub fn erase_signatures(&self) -> Result<UnshieldedOffer, JsError> {
        use UnshieldedOfferTypes::*;
        Ok(match &self.0 {
            Signature(val) => UnshieldedOffer(SignatureErased(val.erase_signatures())),
            SignatureErased(val) => UnshieldedOffer(SignatureErased(val.erase_signatures())),
        })
    }

    #[wasm_bindgen(js_name = "addSignatures")]
    pub fn add_signatures(&mut self, signatures: Vec<String>) -> Result<UnshieldedOffer, JsError> {
        use UnshieldedOfferTypes::*;

        Ok(UnshieldedOffer(match &mut self.0 {
            Signature(val) => {
                let sigs = signatures
                    .into_iter()
                    .map(|sig| Ok::<signatures::Signature, JsError>(from_hex_ser(&sig)?))
                    .collect::<Result<Vec<_>, _>>()?;
                val.add_signatures(sigs);
                Signature(val.clone())
            }
            SignatureErased(val) => {
                let sigs = signatures
                    .into_iter()
                    .map(|sig| Ok::<(), JsError>(from_hex_ser(&sig)?))
                    .collect::<Result<Vec<_>, _>>()?;
                val.add_signatures(sigs);
                SignatureErased(val.clone())
            }
        }))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use UnshieldedOfferTypes::*;
        match &self.0 {
            Signature(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            SignatureErased(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(getter)]
    pub fn inputs(&self) -> Result<Vec<JsValue>, JsError> {
        use UnshieldedOfferTypes::*;
        match &self.0 {
            Signature(val) => val
                .inputs
                .clone()
                .iter()
                .map(|v| Ok(utxo_spend_to_value(&v)?))
                .collect(),
            SignatureErased(val) => val
                .inputs
                .clone()
                .iter()
                .map(|v| Ok(utxo_spend_to_value(&v)?))
                .collect(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn outputs(&self) -> Result<Vec<JsValue>, JsError> {
        use UnshieldedOfferTypes::*;
        match &self.0 {
            Signature(val) => val
                .outputs
                .clone()
                .iter()
                .map(|v| Ok(utxo_output_to_value(&v)?))
                .collect(),
            SignatureErased(val) => val
                .outputs
                .clone()
                .iter()
                .map(|v| Ok(utxo_output_to_value(&v)?))
                .collect(),
        }
    }

    #[wasm_bindgen(getter = signatures)]
    pub fn signatures(&self) -> Result<Vec<String>, JsError> {
        use UnshieldedOfferTypes::*;
        match &self.0 {
            Signature(val) => val
                .signatures
                .iter_deref()
                .map(|sig| Ok(to_hex_ser(&sig)?))
                .collect(),
            SignatureErased(val) => val
                .signatures
                .iter_deref()
                .map(|sig| Ok(to_hex_ser(&sig)?))
                .collect(),
        }
    }
}

impl UnshieldedOffer {
    pub fn input_output_matches(
        current_offer: &Option<UnshieldedOffer>,
        new_offer: &Option<UnshieldedOffer>,
    ) -> bool {
        use UnshieldedOfferTypes::*;
        match (current_offer, new_offer) {
            (None, None) => true,
            (Some(current_offer), Some(new_offer)) => match (&current_offer.0, &new_offer.0) {
                (Signature(offer1), Signature(offer2)) => {
                    do_vecs_match(&Vec::from(&offer1.inputs), &Vec::from(&offer2.inputs))
                }
                (SignatureErased(offer1), SignatureErased(offer2)) => {
                    do_vecs_match(&Vec::from(&offer1.inputs), &Vec::from(&offer2.inputs))
                }
                _ => false,
            },
            _ => false,
        }
    }
}
