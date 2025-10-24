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
use base_crypto::time::{Duration, Timestamp};
use js_sys::{Array, BigInt, Date, Uint8Array};
use ledger::dust::{
    DustActions as LedgerDustActions, DustGenerationState as LedgerDustGenerationState,
    DustLocalState as LedgerDustLocalState, DustOutput as LedgerDustOutput,
    DustParameters as LedgerDustParameters, DustPublicKey,
    DustRegistration as LedgerDustRegistration, DustSecretKey as LedgerDustSecretKey,
    DustSpend as LedgerDustSpend, DustState as LedgerDustState,
    DustUtxoState as LedgerDustUtxoState,
};
use ledger::events::Event as LedgerEvent;
use ledger::structure::{ProofMarker, ProofPreimageMarker, UtxoMeta as LedgerUtxoMeta};
use onchain_runtime_wasm::{from_value_hex_ser, from_value_ser, to_value_hex_ser};
use rand::rngs::OsRng;
use serialize::tagged_serialize;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Debug)]
pub struct Event(pub(crate) LedgerEvent<InMemoryDB>);

impl From<LedgerEvent<InMemoryDB>> for Event {
    fn from(inner: LedgerEvent<InMemoryDB>) -> Event {
        Event(inner)
    }
}

#[wasm_bindgen]
impl Event {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Event, JsError> {
        Err(JsError::new(
            "Event cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<Event, JsError> {
        Ok(Event(from_value_ser(raw, "Event")?))
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

#[derive(Clone)]
pub enum DustSpendTypes {
    ProvenDustSpend(LedgerDustSpend<ProofMarker, InMemoryDB>),
    UnprovenDustSpend(LedgerDustSpend<ProofPreimageMarker, InMemoryDB>),
    ProofErasedDustSpend(LedgerDustSpend<(), InMemoryDB>),
}

#[derive(Clone)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct DustSpend(pub(crate) DustSpendTypes);

try_ref_for_exported!(DustSpend);

impl TryFrom<DustSpend> for LedgerDustSpend<ProofMarker, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustSpend) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustSpendTypes::ProvenDustSpend(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustSpend type provided.")),
        }
    }
}
impl TryFrom<DustSpend> for LedgerDustSpend<ProofPreimageMarker, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustSpend) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustSpendTypes::UnprovenDustSpend(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustSpend type provided.")),
        }
    }
}
impl TryFrom<DustSpend> for LedgerDustSpend<(), InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustSpend) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustSpendTypes::ProofErasedDustSpend(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustSpend type provided.")),
        }
    }
}

impl From<LedgerDustSpend<ProofMarker, InMemoryDB>> for DustSpend {
    fn from(inner: LedgerDustSpend<ProofMarker, InMemoryDB>) -> DustSpend {
        DustSpend(DustSpendTypes::ProvenDustSpend(inner))
    }
}
impl From<LedgerDustSpend<ProofPreimageMarker, InMemoryDB>> for DustSpend {
    fn from(inner: LedgerDustSpend<ProofPreimageMarker, InMemoryDB>) -> DustSpend {
        DustSpend(DustSpendTypes::UnprovenDustSpend(inner))
    }
}
impl From<LedgerDustSpend<(), InMemoryDB>> for DustSpend {
    fn from(inner: LedgerDustSpend<(), InMemoryDB>) -> DustSpend {
        DustSpend(DustSpendTypes::ProofErasedDustSpend(inner))
    }
}

#[wasm_bindgen]
impl DustSpend {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DustSpend, JsError> {
        Err(JsError::new(
            "DustSpend cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(getter, js_name = "vFee")]
    pub fn v_fee(&self) -> BigInt {
        use DustSpendTypes::*;
        BigInt::from(match &self.0 {
            ProvenDustSpend(val) => val.v_fee,
            UnprovenDustSpend(val) => val.v_fee,
            ProofErasedDustSpend(val) => val.v_fee,
        })
    }

    #[wasm_bindgen(getter, js_name = "oldNullifier")]
    pub fn old_nullifier(&self) -> BigInt {
        use DustSpendTypes::*;
        fr_to_bigint(match &self.0 {
            ProvenDustSpend(val) => val.old_nullifier.0,
            UnprovenDustSpend(val) => val.old_nullifier.0,
            ProofErasedDustSpend(val) => val.old_nullifier.0,
        })
    }

    #[wasm_bindgen(getter, js_name = "newCommitment")]
    pub fn new_commitment(&self) -> BigInt {
        use DustSpendTypes::*;
        fr_to_bigint(match &self.0 {
            ProvenDustSpend(val) => val.new_commitment.0,
            UnprovenDustSpend(val) => val.new_commitment.0,
            ProofErasedDustSpend(val) => val.new_commitment.0,
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use DustSpendTypes::*;
        match &self.0 {
            ProvenDustSpend(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenDustSpend(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedDustSpend(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(getter)]
    pub fn proof(&self) -> Result<JsValue, JsError> {
        use crate::crypto::{NoProof, PreProof, Proof};
        use DustSpendTypes::*;
        Ok(match &self.0 {
            ProvenDustSpend(val) => JsValue::from(Proof(val.proof.clone().into())),
            UnprovenDustSpend(val) => JsValue::from(PreProof(val.proof.clone().into())),
            ProofErasedDustSpend(_) => JsValue::from(NoProof()),
        })
    }
}

#[derive(Clone, Debug)]
pub enum DustRegistrationTypes {
    Signature(LedgerDustRegistration<Signature, InMemoryDB>),
    SignatureErased(LedgerDustRegistration<(), InMemoryDB>),
}

#[derive(Clone, Debug)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct DustRegistration(pub(crate) DustRegistrationTypes);

try_ref_for_exported!(DustRegistration);

impl From<LedgerDustRegistration<Signature, InMemoryDB>> for DustRegistration {
    fn from(inner: LedgerDustRegistration<Signature, InMemoryDB>) -> DustRegistration {
        DustRegistration(DustRegistrationTypes::Signature(inner))
    }
}
impl From<LedgerDustRegistration<(), InMemoryDB>> for DustRegistration {
    fn from(inner: LedgerDustRegistration<(), InMemoryDB>) -> DustRegistration {
        DustRegistration(DustRegistrationTypes::SignatureErased(inner))
    }
}

impl TryFrom<DustRegistration> for LedgerDustRegistration<Signature, InMemoryDB> {
    type Error = JsError;
    fn try_from(
        outer: DustRegistration,
    ) -> Result<LedgerDustRegistration<Signature, InMemoryDB>, Self::Error> {
        match &outer.0 {
            DustRegistrationTypes::Signature(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustRegistration type provided.")),
        }
    }
}
impl TryFrom<DustRegistration> for LedgerDustRegistration<(), InMemoryDB> {
    type Error = JsError;
    fn try_from(
        outer: DustRegistration,
    ) -> Result<LedgerDustRegistration<(), InMemoryDB>, Self::Error> {
        match &outer.0 {
            DustRegistrationTypes::SignatureErased(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustRegistration type provided.")),
        }
    }
}

#[wasm_bindgen]
impl DustRegistration {
    #[wasm_bindgen(constructor)]
    pub fn new(
        signature_marker: &str,
        night_key: &str,
        dust_address: Option<BigInt>,
        allow_fee_payment: BigInt,
        signature: JsValue,
    ) -> Result<DustRegistration, JsError> {
        let allow_fee_payment = u128::try_from(allow_fee_payment)
            .map_err(|_| JsError::new("allow_fee_payment is out of range"))?;
        let night_key: signatures::VerifyingKey = from_value_hex_ser(night_key)?;
        let dust_address = dust_address
            .map(bigint_to_fr)
            .transpose()?
            .map(|addr| Sp::new(DustPublicKey(addr)));

        use Signaturish::*;
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;

        Ok(DustRegistration(match signature_type {
            Signature => {
                let signature = if signature.is_null() || signature.is_undefined() {
                    None
                } else {
                    crate::crypto::SignatureEnabled::try_ref(&signature)?
                };
                DustRegistrationTypes::Signature(LedgerDustRegistration {
                    night_key,
                    dust_address,
                    allow_fee_payment,
                    signature: signature.map(|sig| sig.deref().0.clone()).map(Sp::new),
                })
            }
            SignatureErased => {
                let signature = if signature.is_null() || signature.is_undefined() {
                    None
                } else {
                    crate::crypto::SignatureErased::try_ref(&signature)?
                };
                DustRegistrationTypes::SignatureErased(LedgerDustRegistration {
                    night_key,
                    dust_address,
                    allow_fee_payment,
                    signature: signature.map(|_| Sp::new(())),
                })
            }
        }))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = vec![];
        match &self.0 {
            DustRegistrationTypes::Signature(val) => tagged_serialize(&val, &mut res)?,
            DustRegistrationTypes::SignatureErased(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(
        signature_marker: &str,
        raw: Uint8Array,
    ) -> Result<DustRegistration, JsError> {
        use Signaturish::*;
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;
        Ok(match signature_type {
            Signature => DustRegistration(DustRegistrationTypes::Signature(from_value_ser(
                raw,
                "DustRegistration",
            )?)),
            SignatureErased => DustRegistration(DustRegistrationTypes::SignatureErased(
                from_value_ser(raw, "DustRegistration")?,
            )),
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use DustRegistrationTypes::*;
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

    #[wasm_bindgen(getter, js_name = "nightKey")]
    pub fn night_key(&self) -> Result<String, JsError> {
        match &self.0 {
            DustRegistrationTypes::Signature(val) => to_value_hex_ser(&val.night_key),
            DustRegistrationTypes::SignatureErased(val) => to_value_hex_ser(&val.night_key),
        }
    }

    #[wasm_bindgen(setter, js_name = "nightKey")]
    pub fn set_night_key(&mut self, night_key: &str) -> Result<(), JsError> {
        let night_key: signatures::VerifyingKey = from_value_hex_ser(night_key)?;
        match &mut self.0 {
            DustRegistrationTypes::Signature(val) => val.night_key = night_key,
            DustRegistrationTypes::SignatureErased(val) => val.night_key = night_key,
        };
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "dustAddress")]
    pub fn dust_address(&self) -> Option<BigInt> {
        match &self.0 {
            DustRegistrationTypes::Signature(val) => val
                .dust_address
                .clone()
                .map(|address| fr_to_bigint(address.deref().0)),
            DustRegistrationTypes::SignatureErased(val) => val
                .dust_address
                .clone()
                .map(|address| fr_to_bigint(address.deref().0)),
        }
    }

    #[wasm_bindgen(setter, js_name = "dustAddress")]
    pub fn set_dust_address(&mut self, dust_address: Option<BigInt>) -> Result<(), JsError> {
        let dust_address = dust_address
            .map(bigint_to_fr)
            .transpose()?
            .map(|a| Sp::new(DustPublicKey(a)));
        match &mut self.0 {
            DustRegistrationTypes::Signature(val) => val.dust_address = dust_address,
            DustRegistrationTypes::SignatureErased(val) => val.dust_address = dust_address,
        };
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "allowFeePayment")]
    pub fn allow_fee_payment(&self) -> BigInt {
        match &self.0 {
            DustRegistrationTypes::Signature(val) => val.allow_fee_payment.into(),
            DustRegistrationTypes::SignatureErased(val) => val.allow_fee_payment.into(),
        }
    }

    #[wasm_bindgen(setter, js_name = "allowFeePayment")]
    pub fn set_allow_fee_payment(&mut self, allow_fee_payment: BigInt) -> Result<(), JsError> {
        let allow_fee_payment =
            u128::try_from(allow_fee_payment).map_err(|_| JsError::new("fees are out of range"))?;
        match &mut self.0 {
            DustRegistrationTypes::Signature(val) => val.allow_fee_payment = allow_fee_payment,
            DustRegistrationTypes::SignatureErased(val) => {
                val.allow_fee_payment = allow_fee_payment
            }
        };
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn signature(&self) -> Result<JsValue, JsError> {
        use DustRegistrationTypes::*;
        Ok(match &self.0 {
            Signature(val) => val
                .clone()
                .signature
                .map(|sig| JsValue::from(crate::crypto::SignatureEnabled(sig.deref().clone())))
                .unwrap_or(JsValue::UNDEFINED),
            SignatureErased(val) => val
                .clone()
                .signature
                .map(|_| JsValue::from(crate::crypto::SignatureErased()))
                .unwrap_or(JsValue::UNDEFINED),
        })
    }

    #[wasm_bindgen(setter, js_name = "signature")]
    pub fn set_signature(&mut self, signature: JsValue) -> Result<(), JsError> {
        use DustRegistrationTypes::*;
        match &mut self.0 {
            Signature(val) => {
                let signature = if signature.is_null() || signature.is_undefined() {
                    None
                } else {
                    crate::crypto::SignatureEnabled::try_ref(&signature)?
                };
                val.signature = signature.map(|sig| sig.deref().0.clone()).map(Sp::new)
            }
            SignatureErased(val) => {
                let signature = if signature.is_null() || signature.is_undefined() {
                    None
                } else {
                    crate::crypto::SignatureErased::try_ref(&signature)?
                };
                val.signature = signature.map(|_| Sp::new(()))
            }
        };
        Ok(())
    }
}

#[derive(Clone)]
pub enum DustActionsTypes {
    UnprovenWithSignature(LedgerDustActions<Signature, ProofPreimageMarker, InMemoryDB>),
    UnprovenWithSignatureErased(LedgerDustActions<(), ProofPreimageMarker, InMemoryDB>),
    ProvenWithSignature(LedgerDustActions<Signature, ProofMarker, InMemoryDB>),
    ProvenWithSignatureErased(LedgerDustActions<(), ProofMarker, InMemoryDB>),
    ProofErasedWithSignature(LedgerDustActions<Signature, (), InMemoryDB>),
    ProofErasedWithSignatureErased(LedgerDustActions<(), (), InMemoryDB>),
}

#[derive(Clone)]
#[wasm_bindgen]
#[repr(transparent)]
pub struct DustActions(pub(crate) DustActionsTypes);

try_ref_for_exported!(DustActions);

impl From<LedgerDustActions<Signature, ProofPreimageMarker, InMemoryDB>> for DustActions {
    fn from(inner: LedgerDustActions<Signature, ProofPreimageMarker, InMemoryDB>) -> DustActions {
        DustActions(DustActionsTypes::UnprovenWithSignature(inner))
    }
}
impl From<LedgerDustActions<(), ProofPreimageMarker, InMemoryDB>> for DustActions {
    fn from(inner: LedgerDustActions<(), ProofPreimageMarker, InMemoryDB>) -> DustActions {
        DustActions(DustActionsTypes::UnprovenWithSignatureErased(inner))
    }
}
impl From<LedgerDustActions<Signature, ProofMarker, InMemoryDB>> for DustActions {
    fn from(inner: LedgerDustActions<Signature, ProofMarker, InMemoryDB>) -> DustActions {
        DustActions(DustActionsTypes::ProvenWithSignature(inner))
    }
}
impl From<LedgerDustActions<(), ProofMarker, InMemoryDB>> for DustActions {
    fn from(inner: LedgerDustActions<(), ProofMarker, InMemoryDB>) -> DustActions {
        DustActions(DustActionsTypes::ProvenWithSignatureErased(inner))
    }
}
impl From<LedgerDustActions<Signature, (), InMemoryDB>> for DustActions {
    fn from(inner: LedgerDustActions<Signature, (), InMemoryDB>) -> DustActions {
        DustActions(DustActionsTypes::ProofErasedWithSignature(inner))
    }
}
impl From<LedgerDustActions<(), (), InMemoryDB>> for DustActions {
    fn from(inner: LedgerDustActions<(), (), InMemoryDB>) -> DustActions {
        DustActions(DustActionsTypes::ProofErasedWithSignatureErased(inner))
    }
}

impl TryFrom<DustActions> for LedgerDustActions<Signature, ProofPreimageMarker, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustActions) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustActionsTypes::UnprovenWithSignature(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustActions type provided.")),
        }
    }
}
impl TryFrom<DustActions> for LedgerDustActions<(), ProofPreimageMarker, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustActions) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustActionsTypes::UnprovenWithSignatureErased(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustActions type provided.")),
        }
    }
}
impl TryFrom<DustActions> for LedgerDustActions<Signature, ProofMarker, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustActions) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustActionsTypes::ProvenWithSignature(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustActions type provided.")),
        }
    }
}
impl TryFrom<DustActions> for LedgerDustActions<(), ProofMarker, InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustActions) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustActionsTypes::ProvenWithSignatureErased(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustActions type provided.")),
        }
    }
}
impl TryFrom<DustActions> for LedgerDustActions<Signature, (), InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustActions) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustActionsTypes::ProofErasedWithSignature(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustActions type provided.")),
        }
    }
}
impl TryFrom<DustActions> for LedgerDustActions<(), (), InMemoryDB> {
    type Error = JsError;
    fn try_from(outer: DustActions) -> Result<Self, Self::Error> {
        match &outer.0 {
            DustActionsTypes::ProofErasedWithSignatureErased(val) => Ok(val.clone()),
            _ => Err(JsError::new("Unsupported DustActions type provided.")),
        }
    }
}

#[wasm_bindgen]
impl DustActions {
    #[wasm_bindgen(constructor)]
    pub fn new(
        signature_marker: &str,
        proof_marker: &str,
        ctime: &Date,
        spends: JsValue,        // spends?: DustSpend<P>[]
        registrations: JsValue, // registrations?: DustRegistration<S>[]
    ) -> Result<DustActions, JsError> {
        let ctime = Timestamp::from_secs(js_date_to_seconds(ctime));
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;

        let mut dust_spends_proof = Vec::<LedgerDustSpend<ProofMarker, InMemoryDB>>::new();
        let mut dust_spends_pre_proof =
            Vec::<LedgerDustSpend<ProofPreimageMarker, InMemoryDB>>::new();
        let mut dust_spends_no_proof = Vec::<LedgerDustSpend<(), InMemoryDB>>::new();

        let mut registrations_signature =
            Vec::<LedgerDustRegistration<Signature, InMemoryDB>>::new();
        let mut registrations_no_signature = Vec::<LedgerDustRegistration<(), InMemoryDB>>::new();

        if !spends.is_null() && !spends.is_undefined() {
            let js_array = spends
                .dyn_into::<Array>()
                .map_err(|_| JsError::new("Expected null or Array for spends"))?;

            for js_spend in js_array.iter() {
                let spend = DustSpend::try_ref(&js_spend)?.as_deref().cloned();
                if let Some(spend) = spend {
                    use Proofish::*;
                    match proof_type {
                        Proof => {
                            dust_spends_proof.push(spend.try_into()?);
                        }
                        PreProof => {
                            dust_spends_pre_proof.push(spend.try_into()?);
                        }
                        NoProof => {
                            dust_spends_no_proof.push(spend.try_into()?);
                        }
                    }
                }
            }
        }

        if !registrations.is_null() && !registrations.is_undefined() {
            let js_array = registrations
                .dyn_into::<Array>()
                .map_err(|_| JsError::new("Expected null or Array for registrations"))?;

            for js_registration in js_array.iter() {
                let registration = DustRegistration::try_ref(&js_registration)?
                    .as_deref()
                    .cloned();
                if let Some(registration) = registration {
                    use Signaturish::*;
                    match signature_type {
                        Signature => {
                            registrations_signature.push(registration.try_into()?);
                        }
                        SignatureErased => {
                            registrations_no_signature.push(registration.try_into()?);
                        }
                    }
                }
            }
        }

        use DustActionsTypes::*;
        Ok(match (proof_type, signature_type) {
            (Proofish::Proof, Signaturish::Signature) => {
                DustActions(ProvenWithSignature(LedgerDustActions {
                    spends: dust_spends_proof.into(),
                    registrations: registrations_signature.into(),
                    ctime,
                }))
            }
            (Proofish::Proof, Signaturish::SignatureErased) => {
                DustActions(ProvenWithSignatureErased(LedgerDustActions {
                    spends: dust_spends_proof.into(),
                    registrations: registrations_no_signature.into(),
                    ctime,
                }))
            }
            //
            (Proofish::PreProof, Signaturish::Signature) => {
                DustActions(UnprovenWithSignature(LedgerDustActions {
                    spends: dust_spends_pre_proof.into(),
                    registrations: registrations_signature.into(),
                    ctime,
                }))
            }
            (Proofish::PreProof, Signaturish::SignatureErased) => {
                DustActions(UnprovenWithSignatureErased(LedgerDustActions {
                    spends: dust_spends_pre_proof.into(),
                    registrations: registrations_no_signature.into(),
                    ctime,
                }))
            }
            //
            (Proofish::NoProof, Signaturish::Signature) => {
                DustActions(ProofErasedWithSignature(LedgerDustActions {
                    spends: dust_spends_no_proof.into(),
                    registrations: registrations_signature.into(),
                    ctime,
                }))
            }
            (Proofish::NoProof, Signaturish::SignatureErased) => {
                DustActions(ProofErasedWithSignatureErased(LedgerDustActions {
                    spends: dust_spends_no_proof.into(),
                    registrations: registrations_no_signature.into(),
                    ctime,
                }))
            }
        })
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        use DustActionsTypes::*;
        let mut res = Vec::new();
        match &self.0 {
            UnprovenWithSignature(val) => tagged_serialize(&val, &mut res)?,
            UnprovenWithSignatureErased(val) => tagged_serialize(&val, &mut res)?,
            ProvenWithSignature(val) => tagged_serialize(&val, &mut res)?,
            ProvenWithSignatureErased(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedWithSignature(val) => tagged_serialize(&val, &mut res)?,
            ProofErasedWithSignatureErased(val) => tagged_serialize(&val, &mut res)?,
        };
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(
        signature_marker: &str,
        proof_marker: &str,
        raw: Uint8Array,
    ) -> Result<DustActions, JsError> {
        let signature_type: Signaturish = text_to_signaturish(signature_marker)?;
        let proof_type: Proofish = text_to_proofish(proof_marker)?;

        use DustActionsTypes::*;
        use Proofish::*;
        use Signaturish::*;
        Ok(DustActions(match (signature_type, proof_type) {
            (Signature, PreProof) => UnprovenWithSignature(from_value_ser(raw, "DustActions")?),
            (SignatureErased, PreProof) => {
                UnprovenWithSignatureErased(from_value_ser(raw, "DustActions")?)
            }
            (Signature, Proof) => ProvenWithSignature(from_value_ser(raw, "DustActions")?),
            (SignatureErased, Proof) => {
                ProvenWithSignatureErased(from_value_ser(raw, "DustActions")?)
            }
            (Signature, NoProof) => ProofErasedWithSignature(from_value_ser(raw, "DustActions")?),
            (SignatureErased, NoProof) => {
                ProofErasedWithSignatureErased(from_value_ser(raw, "DustActions")?)
            }
        }))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use DustActionsTypes::*;
        match &self.0 {
            UnprovenWithSignature(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenWithSignatureErased(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProvenWithSignature(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProvenWithSignatureErased(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedWithSignature(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedWithSignatureErased(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
        }
    }

    #[wasm_bindgen(getter)]
    pub fn registrations(&self) -> Result<Vec<DustRegistration>, JsError> {
        use DustActionsTypes::*;
        Ok(match &self.0 {
            UnprovenWithSignature(val) => val
                .registrations
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            UnprovenWithSignatureErased(val) => val
                .registrations
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProvenWithSignature(val) => val
                .registrations
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProvenWithSignatureErased(val) => val
                .registrations
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProofErasedWithSignature(val) => val
                .registrations
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProofErasedWithSignatureErased(val) => val
                .registrations
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
        })
    }

    #[wasm_bindgen(setter, js_name = "registrations")]
    pub fn set_registrations(&mut self, registrations: JsValue) -> Result<(), JsError> {
        let mut dust_registrations: Vec<DustRegistration> = vec![];
        if !registrations.is_null() && !registrations.is_undefined() {
            let js_array = registrations
                .dyn_into::<Array>()
                .map_err(|_| JsError::new("Expected null or Array for registrations"))?;

            for js_registration in js_array.iter() {
                let registration = DustRegistration::try_ref(&js_registration)?
                    .as_deref()
                    .cloned();
                if let Some(registration) = registration {
                    dust_registrations.push(registration);
                }
            }
        }

        use DustActionsTypes::*;
        match &mut self.0 {
            UnprovenWithSignature(val) => {
                val.registrations = dust_registrations
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            UnprovenWithSignatureErased(val) => {
                val.registrations = dust_registrations
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProvenWithSignature(val) => {
                val.registrations = dust_registrations
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProvenWithSignatureErased(val) => {
                val.registrations = dust_registrations
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProofErasedWithSignature(val) => {
                val.registrations = dust_registrations
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProofErasedWithSignatureErased(val) => {
                val.registrations = dust_registrations
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
        };
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn spends(&self) -> Result<Vec<DustSpend>, JsError> {
        use DustActionsTypes::*;
        Ok(match &self.0 {
            UnprovenWithSignature(val) => val
                .spends
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            UnprovenWithSignatureErased(val) => val
                .spends
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProvenWithSignature(val) => val
                .spends
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProvenWithSignatureErased(val) => val
                .spends
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProofErasedWithSignature(val) => val
                .spends
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
            ProofErasedWithSignatureErased(val) => val
                .spends
                .iter()
                .map(|sp| sp.deref().clone().into())
                .collect(),
        })
    }

    #[wasm_bindgen(setter, js_name = "spends")]
    pub fn set_spends(&mut self, spends: JsValue) -> Result<(), JsError> {
        let mut dust_spends: Vec<DustSpend> = vec![];
        if !spends.is_null() && !spends.is_undefined() {
            let js_array = spends
                .dyn_into::<Array>()
                .map_err(|_| JsError::new("Expected null or Array for spends"))?;

            for js_spend in js_array.iter() {
                let spend = DustSpend::try_ref(&js_spend)?.as_deref().cloned();
                if let Some(spend) = spend {
                    dust_spends.push(spend);
                }
            }
        }

        use DustActionsTypes::*;
        match &mut self.0 {
            UnprovenWithSignature(val) => {
                val.spends = dust_spends
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            UnprovenWithSignatureErased(val) => {
                val.spends = dust_spends
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProvenWithSignature(val) => {
                val.spends = dust_spends
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProvenWithSignatureErased(val) => {
                val.spends = dust_spends
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProofErasedWithSignature(val) => {
                val.spends = dust_spends
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
            ProofErasedWithSignatureErased(val) => {
                val.spends = dust_spends
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?;
            }
        };
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn ctime(&self) -> Date {
        use DustActionsTypes::*;
        seconds_to_js_date(match &self.0 {
            UnprovenWithSignature(val) => val.ctime.to_secs(),
            UnprovenWithSignatureErased(val) => val.ctime.to_secs(),
            ProvenWithSignature(val) => val.ctime.to_secs(),
            ProvenWithSignatureErased(val) => val.ctime.to_secs(),
            ProofErasedWithSignature(val) => val.ctime.to_secs(),
            ProofErasedWithSignatureErased(val) => val.ctime.to_secs(),
        })
    }

    #[wasm_bindgen(setter, js_name = "ctime")]
    pub fn set_ctime(&mut self, ctime: &Date) -> Result<(), JsError> {
        use DustActionsTypes::*;
        let ctime = Timestamp::from_secs(js_date_to_seconds(ctime));
        match &mut self.0 {
            UnprovenWithSignature(val) => val.ctime = ctime,
            UnprovenWithSignatureErased(val) => val.ctime = ctime,
            ProvenWithSignature(val) => val.ctime = ctime,
            ProvenWithSignatureErased(val) => val.ctime = ctime,
            ProofErasedWithSignature(val) => val.ctime = ctime,
            ProofErasedWithSignatureErased(val) => val.ctime = ctime,
        }
        Ok(())
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct DustParameters(pub(crate) LedgerDustParameters);

#[wasm_bindgen]
impl DustParameters {
    #[wasm_bindgen(constructor)]
    pub fn new(
        night_dust_ratio: BigInt,
        generation_decay_rate: BigInt,
        dust_grace_period_seconds: BigInt,
    ) -> Result<DustParameters, JsError> {
        let params = construct_dust_parameters(
            night_dust_ratio,
            generation_decay_rate,
            dust_grace_period_seconds,
        )?;
        Ok(DustParameters(params))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<DustParameters, JsError> {
        Ok(DustParameters(from_value_ser(raw, "DustParameters")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter, js_name = "nightDustRatio")]
    pub fn night_dust_ratio(&self) -> BigInt {
        BigInt::from(self.0.night_dust_ratio)
    }

    #[wasm_bindgen(setter, js_name = "nightDustRatio")]
    pub fn set_night_dust_ratio(&mut self, night_dust_ratio: BigInt) -> Result<(), JsError> {
        let night_dust_ratio = u64::try_from(night_dust_ratio)
            .map_err(|_| JsError::new("night_dust_ratio is out of range"))?;
        self.0.night_dust_ratio = night_dust_ratio;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "generationDecayRate")]
    pub fn generation_decay_rate(&self) -> BigInt {
        BigInt::from(self.0.generation_decay_rate)
    }

    #[wasm_bindgen(setter, js_name = "generationDecayRate")]
    pub fn set_generation_decay_rate(
        &mut self,
        generation_decay_rate: BigInt,
    ) -> Result<(), JsError> {
        let generation_decay_rate = bigint_to_u32(generation_decay_rate)?;
        self.0.generation_decay_rate = generation_decay_rate;
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "dustGracePeriodSeconds")]
    pub fn dust_grace_period_seconds(&self) -> BigInt {
        BigInt::from(self.0.dust_grace_period.as_seconds())
    }

    #[wasm_bindgen(setter, js_name = "dustGracePeriodSeconds")]
    pub fn set_dust_grace_period_seconds(
        &mut self,
        dust_grace_period_seconds: BigInt,
    ) -> Result<(), JsError> {
        let dust_grace_period_seconds = i128::try_from(dust_grace_period_seconds)
            .map_err(|_| JsError::new("dust_grace_period_seconds is out of range"))?;
        self.0.dust_grace_period = Duration::from_secs(dust_grace_period_seconds);
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = "timeToCapSeconds")]
    pub fn time_to_cap_seconds(&self) -> BigInt {
        BigInt::from(self.0.time_to_cap().as_seconds())
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct DustUtxoState(pub(crate) LedgerDustUtxoState<InMemoryDB>);

#[wasm_bindgen]
impl DustUtxoState {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DustUtxoState, JsError> {
        Ok(DustUtxoState(LedgerDustUtxoState::default()))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<DustUtxoState, JsError> {
        Ok(DustUtxoState(from_value_ser(raw, "DustUtxoState")?))
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
#[derive(Debug)]
pub struct DustGenerationState(pub(crate) LedgerDustGenerationState<InMemoryDB>);

#[wasm_bindgen]
impl DustGenerationState {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DustGenerationState, JsError> {
        Ok(DustGenerationState(LedgerDustGenerationState::default()))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<DustGenerationState, JsError> {
        Ok(DustGenerationState(from_value_ser(
            raw,
            "DustGenerationState",
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
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct DustState(pub(crate) LedgerDustState<InMemoryDB>);

#[wasm_bindgen]
impl DustState {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DustState, JsError> {
        Ok(DustState(LedgerDustState::default()))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<DustState, JsError> {
        Ok(DustState(from_value_ser(raw, "DustState")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn utxo(&self) -> Result<DustUtxoState, JsError> {
        Ok(DustUtxoState(self.0.utxo.clone()))
    }

    #[wasm_bindgen(getter)]
    pub fn generation(&self) -> Result<DustGenerationState, JsError> {
        Ok(DustGenerationState(self.0.generation.clone()))
    }
}

#[wasm_bindgen]
pub struct DustSecretKey(pub(crate) Rc<RefCell<Option<LedgerDustSecretKey>>>);

const DUST_SK_CLEAR_MSG: &str = "Dust secret key was cleared";

impl DustSecretKey {
    pub fn wrap(key: LedgerDustSecretKey) -> Self {
        DustSecretKey(Rc::new(RefCell::new(Some(key))))
    }

    pub fn try_unwrap(&self) -> Result<LedgerDustSecretKey, JsError> {
        self.0
            .borrow()
            .as_ref()
            .cloned()
            .ok_or(JsError::new(DUST_SK_CLEAR_MSG))
    }
}

#[wasm_bindgen]
impl DustSecretKey {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DustSecretKey, JsError> {
        Err(JsError::new(
            "DustSecretKey cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(js_name = "fromBigint")]
    pub fn from_bigint(bigint: BigInt) -> Result<DustSecretKey, JsError> {
        let sk = bigint_to_fr(bigint)?;
        Ok(DustSecretKey::wrap(LedgerDustSecretKey(sk)))
    }

    #[wasm_bindgen(js_name = "fromSeed")]
    pub fn from_seed(seed: Uint8Array) -> Result<DustSecretKey, JsError> {
        let bytes: [u8; 32] = seed
            .to_vec()
            .try_into()
            .map_err(|_| JsError::new("Expected 32-byte seed"))?;
        Ok(DustSecretKey::wrap(LedgerDustSecretKey::derive_secret_key(
            &bytes,
        )))
    }

    pub fn clear(&mut self) {
        self.0.borrow_mut().take();
    }

    #[wasm_bindgen(getter, js_name = "publicKey")]
    pub fn public_key(&self) -> Result<BigInt, JsError> {
        let sk_wrap = self.0.borrow();
        let sk = sk_wrap.as_ref().ok_or(JsError::new(DUST_SK_CLEAR_MSG))?;
        Ok(fr_to_bigint(DustPublicKey::from(sk.clone()).0))
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct DustLocalState(pub(crate) LedgerDustLocalState<InMemoryDB>);

#[wasm_bindgen]
impl DustLocalState {
    #[wasm_bindgen(constructor)]
    pub fn new(params: &DustParameters) -> DustLocalState {
        DustLocalState(LedgerDustLocalState::new(params.0))
    }

    #[wasm_bindgen(js_name = "walletBalance")]
    pub fn wallet_balance(&self, time: &Date) -> BigInt {
        let time = Timestamp::from_secs(js_date_to_seconds(time));
        BigInt::from(self.0.wallet_balance(time))
    }

    #[wasm_bindgen(js_name = "generationInfo")]
    pub fn generation_info(&self, qdo: JsValue) -> Result<JsValue, JsError> {
        let qdo = value_to_qdo(qdo)?;
        let res = self
            .0
            .generation_info(&qdo)
            .as_ref()
            .map(dust_gen_info_to_value)
            .transpose()?;
        Ok(res.unwrap_or(JsValue::UNDEFINED))
    }

    pub fn spend(
        &self,
        sk: &DustSecretKey,
        utxo: JsValue,
        v_fee: BigInt,
        ctime: &Date,
    ) -> Result<Array, JsError> {
        let qdo = value_to_qdo(utxo)?;
        let sk = sk.try_unwrap()?;
        let ctime = Timestamp::from_secs(js_date_to_seconds(ctime));
        let v_fee = u128::try_from(v_fee).map_err(|_| JsError::new("v_fee is out of range"))?;
        let (local_state, dust_spend) = self.0.spend(&sk, &qdo, v_fee.into(), ctime)?;

        let res = Array::new();
        res.push(&JsValue::from(DustLocalState(local_state)));
        res.push(&JsValue::from(DustSpend(
            DustSpendTypes::UnprovenDustSpend(dust_spend),
        )));

        Ok(res)
    }

    #[wasm_bindgen(js_name = "processTtls")]
    pub fn process_ttls(&self, time: &Date) -> Result<DustLocalState, JsError> {
        let time = Timestamp::from_secs(js_date_to_seconds(time));
        Ok(DustLocalState(self.0.process_ttls(time)))
    }

    #[wasm_bindgen(js_name = "replayEvents")]
    pub fn replay_events(
        &self,
        sk: &DustSecretKey,
        events: Vec<Event>,
    ) -> Result<DustLocalState, JsError> {
        let sk = sk.try_unwrap()?;
        let events = events.iter().map(|event| &event.0);
        Ok(DustLocalState(self.0.replay_events(&sk, events)?))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<DustLocalState, JsError> {
        Ok(DustLocalState(from_value_ser(raw, "DustLocalState")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter)]
    pub fn utxos(&self) -> Result<Vec<JsValue>, JsError> {
        Ok(self
            .0
            .utxos()
            .map(|qdo| qdo_to_value(&qdo))
            .collect::<Result<_, _>>()?)
    }

    #[wasm_bindgen(getter)]
    pub fn params(&self) -> Result<DustParameters, JsError> {
        Ok(DustParameters(self.0.params))
    }
}

#[wasm_bindgen]
pub struct UtxoMeta(pub(crate) LedgerUtxoMeta);

#[wasm_bindgen]
impl UtxoMeta {
    #[wasm_bindgen(constructor)]
    pub fn new(ctime: &Date) -> UtxoMeta {
        let ctime = Timestamp::from_secs(js_date_to_seconds(ctime));
        UtxoMeta(LedgerUtxoMeta { ctime })
    }

    #[wasm_bindgen(getter)]
    pub fn ctime(&self) -> Date {
        seconds_to_js_date(self.0.ctime.to_secs())
    }

    #[wasm_bindgen(setter, js_name = "ctime")]
    pub fn set_ctime(&mut self, ctime: &Date) -> Result<(), JsError> {
        let ctime = Timestamp::from_secs(js_date_to_seconds(ctime));
        self.0.ctime = ctime;
        Ok(())
    }
}

#[wasm_bindgen(js_name = "updatedValue")]
pub fn updated_value(
    ctime: &Date,
    initial_value: BigInt,
    gen_info: JsValue,
    now: &Date,
    params: JsValue,
) -> Result<BigInt, JsError> {
    let gen_info = value_to_dust_gen_info(gen_info)?;
    let ctime = Timestamp::from_secs(js_date_to_seconds(ctime));
    let now = Timestamp::from_secs(js_date_to_seconds(now));
    let initial_value =
        u128::try_from(initial_value).map_err(|_| JsError::new("initial_value is out of range"))?;

    let dust = LedgerDustOutput {
        initial_value,
        owner: DustPublicKey(Default::default()),
        nonce: Default::default(),
        seq: Default::default(),
        ctime,
    };
    let params = value_to_dust_params(params)?;
    Ok(dust.updated_value(&gen_info, now, &params).into())
}

#[wasm_bindgen(js_name = "sampleDustSecretKey")]
pub fn sample_dust_secret_key() -> DustSecretKey {
    DustSecretKey::wrap(LedgerDustSecretKey::sample(&mut OsRng))
}
