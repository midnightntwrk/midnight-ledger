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

use crate::contract::{ContractCallPrototype, ContractDeploy, MaintenanceUpdate};
use crate::conversions::*;
use crate::dust::DustActions;
use crate::unshielded::UnshieldedOffer;
use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use js_sys::{Date, Uint8Array};
use ledger::structure::{ErasedIntent, Intent as LedgerIntent, ProofMarker, ProofPreimageMarker};
use onchain_runtime_wasm::from_value_ser;
use serialize::tagged_serialize;
use std::ops::Deref;
use storage::db::InMemoryDB;
use transient_crypto::{
    commitment::{Pedersen, PedersenRandomness, PureGeneratorPedersen},
    proofs::ProofPreimage,
};

use rand::rngs::OsRng;
use storage::arena::Sp;
use wasm_bindgen::prelude::*;

type PreBinding = PedersenRandomness;
type Binding = PureGeneratorPedersen;
type NoBinding = Pedersen;

// S: Signature or SignatureErased
// P: Unproven (ProofPreimage) or Proven (Proof) or ProofErased ( () )
// B: PreBinding (PedersenRandomness) or Binding (PureGeneratorPedersen) or NoBinding (Pedersen)
#[derive(Clone)]
pub enum IntentTypes {
    UnprovenWithSignaturePreBinding(
        LedgerIntent<Signature, ProofPreimageMarker, PreBinding, InMemoryDB>,
    ),
    UnprovenWithSignatureBinding(LedgerIntent<Signature, ProofPreimageMarker, Binding, InMemoryDB>),
    UnprovenWithSignatureErasedPreBinding(
        LedgerIntent<(), ProofPreimageMarker, PreBinding, InMemoryDB>,
    ),
    UnprovenWithSignatureErasedBinding(LedgerIntent<(), ProofPreimageMarker, Binding, InMemoryDB>),
    //
    ProvenWithSignaturePreBinding(LedgerIntent<Signature, ProofMarker, PreBinding, InMemoryDB>),
    ProvenWithSignatureBinding(LedgerIntent<Signature, ProofMarker, Binding, InMemoryDB>),
    ProvenWithSignatureErasedPreBinding(LedgerIntent<(), ProofMarker, PreBinding, InMemoryDB>),
    ProvenWithSignatureErasedBinding(LedgerIntent<(), ProofMarker, Binding, InMemoryDB>),
    //
    ProofErasedWithSignatureNoBinding(LedgerIntent<Signature, (), NoBinding, InMemoryDB>),
    ProofErasedWithSignatureErasedNoBinding(LedgerIntent<(), (), NoBinding, InMemoryDB>),
}

impl IntentTypes {
    fn as_erased(&self) -> ErasedIntent<InMemoryDB> {
        use IntentTypes::*;
        match self {
            UnprovenWithSignaturePreBinding(i) => i.erase_proofs().erase_signatures(),
            UnprovenWithSignatureBinding(i) => i.erase_proofs().erase_signatures(),
            UnprovenWithSignatureErasedPreBinding(i) => i.erase_proofs().erase_signatures(),
            UnprovenWithSignatureErasedBinding(i) => i.erase_proofs().erase_signatures(),
            ProvenWithSignaturePreBinding(i) => i.erase_proofs().erase_signatures(),
            ProvenWithSignatureBinding(i) => i.erase_proofs().erase_signatures(),
            ProvenWithSignatureErasedPreBinding(i) => i.erase_proofs().erase_signatures(),
            ProvenWithSignatureErasedBinding(i) => i.erase_proofs().erase_signatures(),
            ProofErasedWithSignatureNoBinding(i) => i.erase_proofs().erase_signatures(),
            ProofErasedWithSignatureErasedNoBinding(i) => i.erase_proofs().erase_signatures(),
        }
    }
}

#[derive(Clone)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct Intent(pub(crate) IntentTypes);

try_ref_for_exported!(Intent);

impl From<LedgerIntent<Signature, ProofPreimageMarker, PreBinding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<Signature, ProofPreimageMarker, PreBinding, InMemoryDB>) -> Self {
        Intent(IntentTypes::UnprovenWithSignaturePreBinding(inner))
    }
}
impl From<LedgerIntent<Signature, ProofPreimageMarker, Binding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<Signature, ProofPreimageMarker, Binding, InMemoryDB>) -> Self {
        Intent(IntentTypes::UnprovenWithSignatureBinding(inner))
    }
}
impl From<LedgerIntent<(), ProofPreimageMarker, PreBinding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<(), ProofPreimageMarker, PreBinding, InMemoryDB>) -> Self {
        Intent(IntentTypes::UnprovenWithSignatureErasedPreBinding(inner))
    }
}
impl From<LedgerIntent<(), ProofPreimageMarker, Binding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<(), ProofPreimageMarker, Binding, InMemoryDB>) -> Self {
        Intent(IntentTypes::UnprovenWithSignatureErasedBinding(inner))
    }
}
impl From<LedgerIntent<Signature, ProofMarker, PreBinding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<Signature, ProofMarker, PreBinding, InMemoryDB>) -> Self {
        Intent(IntentTypes::ProvenWithSignaturePreBinding(inner))
    }
}
impl From<LedgerIntent<Signature, ProofMarker, Binding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<Signature, ProofMarker, Binding, InMemoryDB>) -> Self {
        Intent(IntentTypes::ProvenWithSignatureBinding(inner))
    }
}
impl From<LedgerIntent<(), ProofMarker, PreBinding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<(), ProofMarker, PreBinding, InMemoryDB>) -> Self {
        Intent(IntentTypes::ProvenWithSignatureErasedPreBinding(inner))
    }
}
impl From<LedgerIntent<(), ProofMarker, Binding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<(), ProofMarker, Binding, InMemoryDB>) -> Self {
        Intent(IntentTypes::ProvenWithSignatureErasedBinding(inner))
    }
}
impl From<LedgerIntent<Signature, (), NoBinding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<Signature, (), NoBinding, InMemoryDB>) -> Self {
        Intent(IntentTypes::ProofErasedWithSignatureNoBinding(inner))
    }
}
impl From<LedgerIntent<(), (), NoBinding, InMemoryDB>> for Intent {
    fn from(inner: LedgerIntent<(), (), NoBinding, InMemoryDB>) -> Self {
        Intent(IntentTypes::ProofErasedWithSignatureErasedNoBinding(inner))
    }
}

impl TryFrom<Intent> for LedgerIntent<Signature, ProofPreimageMarker, PreBinding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::UnprovenWithSignaturePreBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<Signature, ProofPreimageMarker, Binding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::UnprovenWithSignatureBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<(), ProofPreimageMarker, PreBinding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::UnprovenWithSignatureErasedPreBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<(), ProofPreimageMarker, Binding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::UnprovenWithSignatureErasedBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<Signature, ProofMarker, PreBinding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::ProvenWithSignaturePreBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<Signature, ProofMarker, Binding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::ProvenWithSignatureBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<(), ProofMarker, PreBinding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::ProvenWithSignatureErasedPreBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<(), ProofMarker, Binding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::ProvenWithSignatureErasedBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<Signature, (), NoBinding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::ProofErasedWithSignatureNoBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}
impl TryFrom<Intent> for LedgerIntent<(), (), NoBinding, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: Intent) -> Result<Self, Self::Error> {
        match &outer.0 {
            IntentTypes::ProofErasedWithSignatureErasedNoBinding(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported Intent type provided.")),
        }
    }
}

#[wasm_bindgen]
impl Intent {
    #[wasm_bindgen(constructor)]
    pub fn construct() -> Result<Intent, JsError> {
        Err(JsError::new(
            "Intent cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn new(ttl: &Date) -> Result<Intent, JsError> {
        let ttl = Timestamp::from_secs(js_date_to_seconds(ttl));
        Ok(Intent(IntentTypes::UnprovenWithSignaturePreBinding(
            LedgerIntent::new(&mut OsRng, None, None, vec![], vec![], vec![], None, ttl),
        )))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use IntentTypes::*;
        let mut res = Vec::new();
        match &self.0 {
            UnprovenWithSignaturePreBinding(val) => tagged_serialize(&val, &mut res)?,
            UnprovenWithSignatureBinding(val) => tagged_serialize(&val, &mut res)?,
            UnprovenWithSignatureErasedPreBinding(val) => tagged_serialize(&val, &mut res)?,
            UnprovenWithSignatureErasedBinding(val) => tagged_serialize(&val, &mut res)?,
            ProvenWithSignaturePreBinding(val) => tagged_serialize(&val, &mut res)?,
            ProvenWithSignatureBinding(val) => tagged_serialize(&val, &mut res)?,
            ProvenWithSignatureErasedPreBinding(val) => tagged_serialize(&val, &mut res)?,
            ProvenWithSignatureErasedBinding(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedWithSignatureNoBinding(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedWithSignatureErasedNoBinding(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(
        signature_marker: &str,
        proof_marker: &str,
        binding_marker: &str,
        raw: Uint8Array,
    ) -> Result<Intent, JsError> {
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;
        let binding_type: Bindingish = text_to_bindingish(binding_marker)?;

        use Bindingish::*;
        use IntentTypes::*;
        use Proofish::*;
        use Signaturish::*;
        Ok(Intent(match (signature_type, proof_type, binding_type) {
            (Signature, PreProof, PreBinding) => {
                UnprovenWithSignaturePreBinding(from_value_ser(raw, "Intent")?)
            }
            (Signature, PreProof, Binding) => {
                UnprovenWithSignatureBinding(from_value_ser(raw, "Intent")?)
            }
            (SignatureErased, PreProof, PreBinding) => {
                UnprovenWithSignatureErasedPreBinding(from_value_ser(raw, "Intent")?)
            }
            (SignatureErased, PreProof, Binding) => {
                UnprovenWithSignatureErasedBinding(from_value_ser(raw, "Intent")?)
            }
            (Signature, Proof, PreBinding) => {
                ProvenWithSignaturePreBinding(from_value_ser(raw, "Intent")?)
            }
            (Signature, Proof, Binding) => {
                ProvenWithSignatureBinding(from_value_ser(raw, "Intent")?)
            }
            (SignatureErased, Proof, PreBinding) => {
                ProvenWithSignatureErasedPreBinding(from_value_ser(raw, "Intent")?)
            }
            (SignatureErased, Proof, Binding) => {
                ProvenWithSignatureErasedBinding(from_value_ser(raw, "Intent")?)
            }
            (Signature, NoProof, NoBinding) => {
                ProofErasedWithSignatureNoBinding(from_value_ser(raw, "Intent")?)
            }
            (SignatureErased, NoProof, NoBinding) => {
                ProofErasedWithSignatureErasedNoBinding(from_value_ser(raw, "Intent")?)
            }
            _ => Err(JsError::new("Unsupported intent type provided."))?,
        }))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenWithSignatureBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenWithSignatureErasedPreBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenWithSignatureErasedBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProvenWithSignaturePreBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProvenWithSignatureBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProvenWithSignatureErasedPreBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProvenWithSignatureErasedBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedWithSignatureNoBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedWithSignatureErasedNoBinding(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(js_name = "intentHash")]
    pub fn intent_hash(&self, segment_id: u16) -> Result<String, JsError> {
        to_hex_ser(&self.0.as_erased().intent_hash(segment_id))
    }

    #[wasm_bindgen(js_name = "addCall")]
    pub fn add_call(&self, call: &ContractCallPrototype) -> Result<Intent, JsError> {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureNoBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_) => {
                Err(JsError::new("Intent is already bound."))
            }
            ProvenWithSignaturePreBinding(_) | ProvenWithSignatureErasedPreBinding(_) => {
                Err(JsError::new("Intent should be unproven."))
            }
            UnprovenWithSignaturePreBinding(val) => Ok(Intent(UnprovenWithSignaturePreBinding(
                val.add_call::<ProofPreimage>(call.0.clone()),
            ))),
            UnprovenWithSignatureErasedPreBinding(val) => {
                Ok(Intent(UnprovenWithSignatureErasedPreBinding(
                    val.add_call::<ProofPreimage>(call.0.clone()),
                )))
            }
        }
    }

    #[wasm_bindgen(js_name = "addDeploy")]
    pub fn add_deploy(&self, deploy: &ContractDeploy) -> Result<Intent, JsError> {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureNoBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_) => {
                Err(JsError::new("Intent is already bound."))
            }
            ProvenWithSignaturePreBinding(_) | ProvenWithSignatureErasedPreBinding(_) => {
                Err(JsError::new("Intent should be unproven."))
            }
            UnprovenWithSignaturePreBinding(val) => Ok(Intent(UnprovenWithSignaturePreBinding(
                val.add_deploy(deploy.0.clone()),
            ))),
            UnprovenWithSignatureErasedPreBinding(val) => Ok(Intent(
                UnprovenWithSignatureErasedPreBinding(val.add_deploy(deploy.0.clone())),
            )),
        }
    }

    #[wasm_bindgen(js_name = "addMaintenanceUpdate")]
    pub fn add_maintenance_update(&self, update: &MaintenanceUpdate) -> Result<Intent, JsError> {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureNoBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_) => {
                Err(JsError::new("Intent is already bound."))
            }
            ProvenWithSignaturePreBinding(_) | ProvenWithSignatureErasedPreBinding(_) => {
                Err(JsError::new("Intent should be unproven."))
            }
            UnprovenWithSignaturePreBinding(val) => Ok(Intent(UnprovenWithSignaturePreBinding(
                val.add_maintenance_update(update.0.clone()),
            ))),
            UnprovenWithSignatureErasedPreBinding(val) => Ok(Intent(
                UnprovenWithSignatureErasedPreBinding(val.add_maintenance_update(update.0.clone())),
            )),
        }
    }

    pub fn bind(&self, segment_id: u16) -> Result<Intent, JsError> {
        use IntentTypes::*;

        if segment_id == 0 {
            return Err(JsError::new("Segment ID cannot be 0"));
        }

        match &self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureNoBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_) => {
                Err(JsError::new("Intent cannot be bound."))
            }
            UnprovenWithSignaturePreBinding(val) => Ok(Intent(UnprovenWithSignatureBinding(
                val.seal(OsRng, segment_id),
            ))),
            UnprovenWithSignatureErasedPreBinding(val) => Ok(Intent(
                UnprovenWithSignatureErasedBinding(val.seal(OsRng, segment_id)),
            )),
            ProvenWithSignaturePreBinding(val) => Ok(Intent(ProvenWithSignatureBinding(
                val.seal(OsRng, segment_id),
            ))),
            ProvenWithSignatureErasedPreBinding(val) => Ok(Intent(
                ProvenWithSignatureErasedBinding(val.seal(OsRng, segment_id)),
            )),
        }
    }

    #[wasm_bindgen(js_name = "eraseProofs")]
    pub fn erase_proofs(&self) -> Result<Intent, JsError> {
        use IntentTypes::*;
        Ok(match &self.0 {
            UnprovenWithSignaturePreBinding(val) => {
                Intent(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            UnprovenWithSignatureBinding(val) => {
                Intent(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            UnprovenWithSignatureErasedPreBinding(val) => {
                Intent(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            UnprovenWithSignatureErasedBinding(val) => {
                Intent(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            ProvenWithSignaturePreBinding(val) => {
                Intent(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            ProvenWithSignatureBinding(val) => {
                Intent(ProofErasedWithSignatureNoBinding(val.erase_proofs()))
            }
            ProvenWithSignatureErasedPreBinding(val) => {
                Intent(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            ProvenWithSignatureErasedBinding(val) => {
                Intent(ProofErasedWithSignatureErasedNoBinding(val.erase_proofs()))
            }
            ProofErasedWithSignatureNoBinding(_) | ProofErasedWithSignatureErasedNoBinding(_) => {
                self.clone()
            }
        })
    }

    #[wasm_bindgen(js_name = "eraseSignatures")]
    pub fn erase_signatures(&self) -> Result<Intent, JsError> {
        use IntentTypes::*;
        Ok(match &self.0 {
            UnprovenWithSignatureErasedPreBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureErasedPreBinding(_)
            | ProvenWithSignatureErasedBinding(_)
            | ProofErasedWithSignatureErasedNoBinding(_) => self.clone(),
            UnprovenWithSignaturePreBinding(val) => Intent(UnprovenWithSignatureErasedPreBinding(
                val.erase_signatures(),
            )),
            UnprovenWithSignatureBinding(val) => {
                Intent(UnprovenWithSignatureErasedBinding(val.erase_signatures()))
            }
            ProvenWithSignaturePreBinding(val) => {
                Intent(ProvenWithSignatureErasedPreBinding(val.erase_signatures()))
            }
            ProvenWithSignatureBinding(val) => {
                Intent(ProvenWithSignatureErasedBinding(val.erase_signatures()))
            }
            ProofErasedWithSignatureNoBinding(val) => Intent(
                ProofErasedWithSignatureErasedNoBinding(val.erase_signatures()),
            ),
        })
    }

    #[wasm_bindgen(js_name = "signatureData")]
    pub fn signature_data(&self, segment_id: u16) -> Uint8Array {
        self.0
            .as_erased()
            .data_to_sign(segment_id)
            .as_slice()
            .into()
    }

    #[wasm_bindgen(getter)]
    pub fn actions(&self) -> Vec<JsValue> {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            UnprovenWithSignatureBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            UnprovenWithSignatureErasedPreBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            UnprovenWithSignatureErasedBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            ProvenWithSignaturePreBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            ProvenWithSignatureBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            ProvenWithSignatureErasedPreBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            ProvenWithSignatureErasedBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            ProofErasedWithSignatureNoBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
            ProofErasedWithSignatureErasedNoBinding(val) => val
                .actions
                .iter_deref()
                .map(contract_action_to_value)
                .collect(),
        }
    }

    #[wasm_bindgen(setter, js_name = "actions")]
    pub fn set_actions(&mut self, actions: Vec<JsValue>) -> Result<(), JsError> {
        use IntentTypes::*;

        match &mut self.0 {
            UnprovenWithSignatureBinding(_)
            | UnprovenWithSignatureErasedBinding(_)
            | ProvenWithSignatureBinding(_)
            | ProvenWithSignatureErasedBinding(_) => Err(JsError::new("Intent is already bound."))?,
            UnprovenWithSignaturePreBinding(intent) => {
                intent.actions = actions
                    .iter()
                    .map(ContractActionConverter::<ProofPreimageMarker>::try_from_value)
                    .collect::<Result<_, _>>()?;
            }
            UnprovenWithSignatureErasedPreBinding(intent) => {
                intent.actions = actions
                    .iter()
                    .map(ContractActionConverter::<ProofPreimageMarker>::try_from_value)
                    .collect::<Result<_, _>>()?;
            }
            ProvenWithSignaturePreBinding(intent) => {
                intent.actions = actions
                    .iter()
                    .map(ContractActionConverter::<ProofMarker>::try_from_value)
                    .collect::<Result<_, _>>()?;
            }
            ProvenWithSignatureErasedPreBinding(intent) => {
                intent.actions = actions
                    .iter()
                    .map(ContractActionConverter::<ProofMarker>::try_from_value)
                    .collect::<Result<_, _>>()?;
            }
            ProofErasedWithSignatureNoBinding(intent) => {
                intent.actions = actions
                    .iter()
                    .map(ContractActionConverter::<()>::try_from_value)
                    .collect::<Result<_, _>>()?;
            }
            ProofErasedWithSignatureErasedNoBinding(intent) => {
                intent.actions = actions
                    .iter()
                    .map(ContractActionConverter::<()>::try_from_value)
                    .collect::<Result<_, _>>()?;
            }
        };
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "dustActions")]
    pub fn dust_actions(&self) -> Result<JsValue, JsError> {
        use IntentTypes::*;
        Ok((match &self.0 {
            UnprovenWithSignaturePreBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            UnprovenWithSignatureBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            UnprovenWithSignatureErasedPreBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            UnprovenWithSignatureErasedBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            ProvenWithSignaturePreBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            ProvenWithSignatureBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            ProvenWithSignatureErasedPreBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            ProvenWithSignatureErasedBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            ProofErasedWithSignatureNoBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
            ProofErasedWithSignatureErasedNoBinding(val) => val
                .dust_actions
                .clone()
                .map(|dust_actions| DustActions::from(dust_actions.deref().clone())),
        })
        .map(JsValue::from)
        .unwrap_or(JsValue::UNDEFINED))
    }

    #[wasm_bindgen(setter, js_name = "dustActions")]
    pub fn set_dust_actions(&mut self, dust_actions: JsValue) -> Result<(), JsError> {
        let dust_actions = if dust_actions.is_null() || dust_actions.is_undefined() {
            None
        } else {
            DustActions::try_ref(&dust_actions)?
        };

        use IntentTypes::*;
        match &mut self.0 {
            UnprovenWithSignaturePreBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            UnprovenWithSignatureBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            UnprovenWithSignatureErasedPreBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            UnprovenWithSignatureErasedBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            ProvenWithSignaturePreBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            ProvenWithSignatureBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            ProvenWithSignatureErasedPreBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            ProvenWithSignatureErasedBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            ProofErasedWithSignatureNoBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
            ProofErasedWithSignatureErasedNoBinding(val) => {
                val.dust_actions = dust_actions
                    .map(|da| da.clone().try_into())
                    .transpose()?
                    .map(Sp::new)
            }
        };
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "guaranteedUnshieldedOffer")]
    pub fn guaranteed_unshielded_offer(&self) -> Option<UnshieldedOffer> {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            UnprovenWithSignatureBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            UnprovenWithSignatureErasedPreBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            UnprovenWithSignatureErasedBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignaturePreBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignatureBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignatureErasedPreBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignatureErasedBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProofErasedWithSignatureNoBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProofErasedWithSignatureErasedNoBinding(val) => val
                .guaranteed_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
        }
    }

    #[wasm_bindgen(setter, js_name = "guaranteedUnshieldedOffer")]
    pub fn set_guaranteed_unshielded_offer(&mut self, offer: JsValue) -> Result<(), JsError> {
        use IntentTypes::*;
        let offer = if offer.is_null() || offer.is_undefined() {
            None
        } else {
            UnshieldedOffer::try_ref(&offer)?
        };

        let bound_error = JsError::new("In bound intent only signatures can be different.");
        match &mut self.0 {
            UnprovenWithSignatureBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .guaranteed_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.guaranteed_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            UnprovenWithSignatureErasedBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .guaranteed_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.guaranteed_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            ProvenWithSignatureBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .guaranteed_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.guaranteed_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            ProvenWithSignatureErasedBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .guaranteed_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.guaranteed_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            UnprovenWithSignaturePreBinding(intent) => {
                intent.guaranteed_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            UnprovenWithSignatureErasedPreBinding(intent) => {
                intent.guaranteed_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProvenWithSignaturePreBinding(intent) => {
                intent.guaranteed_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProvenWithSignatureErasedPreBinding(intent) => {
                intent.guaranteed_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProofErasedWithSignatureNoBinding(intent) => {
                intent.guaranteed_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProofErasedWithSignatureErasedNoBinding(intent) => {
                intent.guaranteed_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
        };
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "fallibleUnshieldedOffer")]
    pub fn fallible_unshielded_offer(&self) -> Option<UnshieldedOffer> {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            UnprovenWithSignatureBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            UnprovenWithSignatureErasedPreBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            UnprovenWithSignatureErasedBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignaturePreBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignatureBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignatureErasedPreBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProvenWithSignatureErasedBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProofErasedWithSignatureNoBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
            ProofErasedWithSignatureErasedNoBinding(val) => val
                .fallible_unshielded_offer
                .clone()
                .map(|sp| sp.deref().clone().into()),
        }
    }

    #[wasm_bindgen(setter, js_name = "fallibleUnshieldedOffer")]
    pub fn set_fallible_unshielded_offer(&mut self, offer: JsValue) -> Result<(), JsError> {
        use IntentTypes::*;
        let offer = if offer.is_null() || offer.is_undefined() {
            None
        } else {
            UnshieldedOffer::try_ref(&offer)?
        };

        let bound_error = JsError::new("In bound intent only signatures can be different.");
        match &mut self.0 {
            UnprovenWithSignatureBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .fallible_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.fallible_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            UnprovenWithSignatureErasedBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .fallible_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.fallible_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            ProvenWithSignatureBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .fallible_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.fallible_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            ProvenWithSignatureErasedBinding(intent) => {
                let new_offer = offer.as_deref().cloned();
                if UnshieldedOffer::input_output_matches(
                    &intent
                        .fallible_unshielded_offer
                        .clone()
                        .map(|sp| sp.deref().clone().into()),
                    &new_offer,
                ) {
                    intent.fallible_unshielded_offer =
                        new_offer.map(TryInto::try_into).transpose()?.map(Sp::new);
                } else {
                    Err(bound_error)?
                }
            }
            UnprovenWithSignaturePreBinding(intent) => {
                intent.fallible_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            UnprovenWithSignatureErasedPreBinding(intent) => {
                intent.fallible_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProvenWithSignaturePreBinding(intent) => {
                intent.fallible_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProvenWithSignatureErasedPreBinding(intent) => {
                intent.fallible_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProofErasedWithSignatureNoBinding(intent) => {
                intent.fallible_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
            ProofErasedWithSignatureErasedNoBinding(intent) => {
                intent.fallible_unshielded_offer = offer
                    .map(|o| o.clone().try_into())
                    .transpose()?
                    .map(Sp::new);
            }
        };
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn ttl(&self) -> Date {
        use IntentTypes::*;
        match &self.0 {
            UnprovenWithSignaturePreBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            UnprovenWithSignatureBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            UnprovenWithSignatureErasedPreBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            UnprovenWithSignatureErasedBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            ProvenWithSignaturePreBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            ProvenWithSignatureBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            ProvenWithSignatureErasedPreBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            ProvenWithSignatureErasedBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            ProofErasedWithSignatureNoBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
            ProofErasedWithSignatureErasedNoBinding(val) => seconds_to_js_date(val.ttl.to_secs()),
        }
    }

    #[wasm_bindgen(setter, js_name = "ttl")]
    pub fn set_ttl(&mut self, ttl: &Date) -> () {
        let ttl = Timestamp::from_secs(js_date_to_seconds(ttl));
        use IntentTypes::*;
        match &mut self.0 {
            UnprovenWithSignaturePreBinding(val) => val.ttl = ttl,
            UnprovenWithSignatureBinding(val) => val.ttl = ttl,
            UnprovenWithSignatureErasedPreBinding(val) => val.ttl = ttl,
            UnprovenWithSignatureErasedBinding(val) => val.ttl = ttl,
            ProvenWithSignaturePreBinding(val) => val.ttl = ttl,
            ProvenWithSignatureBinding(val) => val.ttl = ttl,
            ProvenWithSignatureErasedPreBinding(val) => val.ttl = ttl,
            ProvenWithSignatureErasedBinding(val) => val.ttl = ttl,
            ProofErasedWithSignatureNoBinding(val) => val.ttl = ttl,
            ProofErasedWithSignatureErasedNoBinding(val) => val.ttl = ttl,
        }
        ()
    }

    #[wasm_bindgen(getter)]
    pub fn binding(&self) -> Result<JsValue, JsError> {
        use crate::crypto::{Binding, NoBinding, PreBinding};
        use IntentTypes::*;
        Ok(match &self.0 {
            UnprovenWithSignaturePreBinding(val) => {
                JsValue::from(PreBinding(val.binding_commitment.clone()))
            }
            UnprovenWithSignatureBinding(val) => {
                JsValue::from(Binding(val.binding_commitment.clone()))
            }
            UnprovenWithSignatureErasedPreBinding(val) => {
                JsValue::from(PreBinding(val.binding_commitment.clone()))
            }
            UnprovenWithSignatureErasedBinding(val) => {
                JsValue::from(Binding(val.binding_commitment.clone()))
            }
            ProvenWithSignaturePreBinding(val) => {
                JsValue::from(PreBinding(val.binding_commitment.clone()))
            }
            ProvenWithSignatureBinding(val) => {
                JsValue::from(Binding(val.binding_commitment.clone()))
            }
            ProvenWithSignatureErasedPreBinding(val) => {
                JsValue::from(PreBinding(val.binding_commitment.clone()))
            }
            ProvenWithSignatureErasedBinding(val) => {
                JsValue::from(Binding(val.binding_commitment.clone()))
            }
            ProofErasedWithSignatureNoBinding(val) => {
                JsValue::from(NoBinding(val.binding_commitment.clone()))
            }
            ProofErasedWithSignatureErasedNoBinding(val) => {
                JsValue::from(NoBinding(val.binding_commitment.clone()))
            }
        })
    }
}
