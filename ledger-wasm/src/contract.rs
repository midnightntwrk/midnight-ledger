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
use js_sys::{Array, BigInt, JsString, Uint8Array};
use ledger::structure::{ProofMarker, ProofPreimageMarker, SingleUpdate};
use onchain_runtime::state::EntryPointBuf;
use onchain_runtime_wasm::state::{
    ContractMaintenanceAuthority, ContractOperation, ContractState, from_maybe_string, maybe_string,
};
use rand::rngs::OsRng;
use serialize::Serializable;
use serialize::tagged_deserialize;
use storage::db::InMemoryDB;
use transient_crypto::proofs::KeyLocation;
use transient_crypto::proofs::ProofPreimage;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsError, JsValue};

#[wasm_bindgen]
pub struct ContractDeploy(pub(crate) ledger::structure::ContractDeploy<InMemoryDB>);

try_ref_for_exported!(ContractDeploy);

#[wasm_bindgen]
impl ContractDeploy {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state: &ContractState) -> ContractDeploy {
        ContractDeploy(ledger::structure::ContractDeploy::new(
            &mut OsRng,
            initial_state.clone().into(),
        ))
    }

    #[wasm_bindgen(getter, js_name = "initialState")]
    pub fn initial_state(&self) -> ContractState {
        self.0.initial_state.clone().into()
    }

    #[wasm_bindgen(getter)]
    pub fn address(&self) -> Result<String, JsError> {
        to_hex_ser(&self.0.address())
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
pub struct ContractCallPrototype(pub(crate) ledger::construct::ContractCallPrototype<InMemoryDB>);

#[wasm_bindgen]
impl ContractCallPrototype {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        address: &str,
        entry_point: JsValue,
        op: &ContractOperation,
        guaranteed_public_transcript: JsValue,
        fallible_public_transcript: JsValue,
        private_transcript_outputs: Vec<JsValue>,
        input: JsValue,
        output: JsValue,
        communication_commitment_rand: &str,
        key_location: &str,
    ) -> Result<ContractCallPrototype, JsError> {
        Ok(ContractCallPrototype(
            ledger::construct::ContractCallPrototype {
                address: from_hex_ser(address)?,
                entry_point: EntryPointBuf(from_maybe_string(entry_point)?),
                op: op.clone().into(),
                guaranteed_public_transcript: from_value(guaranteed_public_transcript)?,
                fallible_public_transcript: from_value(fallible_public_transcript)?,
                private_transcript_outputs: private_transcript_outputs
                    .into_iter()
                    .map(from_value)
                    .collect::<Result<Vec<_>, _>>()?,
                input: from_value(input)?,
                output: from_value(output)?,
                communication_commitment_rand: from_hex_ser(communication_commitment_rand)?,
                key_location: KeyLocation(std::borrow::Cow::Owned(key_location.to_owned())),
            },
        ))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(js_name = "intoCall")]
    pub fn into_call(&self, _parent_binding: JsValue) -> Result<ContractCall, JsError> {
        use ledger::construct::ContractCallExt;
        use ledger::structure::ContractCall as LedgerContractCall;
        Ok(LedgerContractCall::<ProofPreimageMarker, InMemoryDB>::new(
            self.0.address.clone(),
            self.0.entry_point.clone(),
            self.0.guaranteed_public_transcript.clone(),
            self.0.fallible_public_transcript.clone(),
            self.0.communication_commitment_rand.clone(),
            ProofPreimage::construct_proof(&self.0, self.0.communication_commitment_rand.clone()),
        )
        .into())
    }
}

pub enum ContractCallTypes {
    ProvenContractCall(ledger::structure::ContractCall<ProofMarker, InMemoryDB>),
    UnprovenContractCall(ledger::structure::ContractCall<ProofPreimageMarker, InMemoryDB>),
    ProofErasedContractCall(ledger::structure::ContractCall<(), InMemoryDB>),
}

#[wasm_bindgen]
#[repr(transparent)]
pub struct ContractCall(pub(crate) ContractCallTypes);

try_ref_for_exported!(ContractCall);

impl From<ledger::structure::ContractCall<ProofMarker, InMemoryDB>> for ContractCall {
    fn from(inner: ledger::structure::ContractCall<ProofMarker, InMemoryDB>) -> ContractCall {
        ContractCall(ContractCallTypes::ProvenContractCall(inner))
    }
}
impl From<ledger::structure::ContractCall<ProofPreimageMarker, InMemoryDB>> for ContractCall {
    fn from(
        inner: ledger::structure::ContractCall<ProofPreimageMarker, InMemoryDB>,
    ) -> ContractCall {
        ContractCall(ContractCallTypes::UnprovenContractCall(inner))
    }
}
impl From<ledger::structure::ContractCall<(), InMemoryDB>> for ContractCall {
    fn from(inner: ledger::structure::ContractCall<(), InMemoryDB>) -> ContractCall {
        ContractCall(ContractCallTypes::ProofErasedContractCall(inner))
    }
}

#[wasm_bindgen]
impl ContractCall {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<ContractCall, JsError> {
        Err(JsError::new(
            "ContractCall cannot be constructed directly through the WASM API.",
        ))
    }

    #[wasm_bindgen(getter)]
    pub fn address(&self) -> Result<String, JsError> {
        use ContractCallTypes::*;
        match &self.0 {
            ProvenContractCall(val) => to_hex_ser(&val.address),
            UnprovenContractCall(val) => to_hex_ser(&val.address),
            ProofErasedContractCall(val) => to_hex_ser(&val.address),
        }
    }

    #[wasm_bindgen(getter, js_name = "entryPoint")]
    pub fn entry_point(&self) -> Result<JsValue, JsError> {
        use ContractCallTypes::*;
        // TODO: something better with entry points?
        Ok(match &self.0 {
            ProvenContractCall(val) => maybe_string(&val.entry_point.0),
            UnprovenContractCall(val) => maybe_string(&val.entry_point.0),
            ProofErasedContractCall(val) => maybe_string(&val.entry_point.0),
        })
    }

    #[wasm_bindgen(getter, js_name = "guaranteedTranscript")]
    pub fn guaranteed_transcript(&self) -> Result<JsValue, JsError> {
        use ContractCallTypes::*;
        Ok(match &self.0 {
            ProvenContractCall(val) => to_value(&val.guaranteed_transcript)?,
            UnprovenContractCall(val) => to_value(&val.guaranteed_transcript)?,
            ProofErasedContractCall(val) => to_value(&val.guaranteed_transcript)?,
        })
    }

    #[wasm_bindgen(getter, js_name = "fallibleTranscript")]
    pub fn fallible_transcript(&self) -> Result<JsValue, JsError> {
        use ContractCallTypes::*;
        Ok(match &self.0 {
            ProvenContractCall(val) => to_value(&val.fallible_transcript)?,
            UnprovenContractCall(val) => to_value(&val.fallible_transcript)?,
            ProofErasedContractCall(val) => to_value(&val.fallible_transcript)?,
        })
    }

    #[wasm_bindgen(getter, js_name = "communicationCommitment")]
    pub fn communication_commitment(&self) -> Result<JsString, JsError> {
        use ContractCallTypes::*;
        Ok(match &self.0 {
            ProvenContractCall(val) => to_hex_ser(&val.communication_commitment)?.into(),
            UnprovenContractCall(val) => to_hex_ser(&val.communication_commitment)?.into(),
            ProofErasedContractCall(val) => to_hex_ser(&val.communication_commitment)?.into(),
        })
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        use ContractCallTypes::*;
        match &self.0 {
            ProvenContractCall(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            UnprovenContractCall(val) => {
                if compact.unwrap_or(false) {
                    format!("{:?}", &val)
                } else {
                    format!("{:#?}", &val)
                }
            }
            ProofErasedContractCall(val) => {
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
        use ContractCallTypes::*;
        Ok(match &self.0 {
            ProvenContractCall(val) => JsValue::from(Proof(val.proof.clone())),
            UnprovenContractCall(val) => JsValue::from(PreProof(val.proof.clone())),
            ProofErasedContractCall(_) => JsValue::from(NoProof()),
        })
    }
}

#[wasm_bindgen]
pub struct ReplaceAuthority(onchain_runtime::state::ContractMaintenanceAuthority);

try_ref_for_exported!(ReplaceAuthority);

#[wasm_bindgen]
impl ReplaceAuthority {
    #[wasm_bindgen(constructor)]
    pub fn new(authority: &ContractMaintenanceAuthority) -> Self {
        Self(authority.clone().into())
    }

    #[wasm_bindgen(getter = authority)]
    pub fn authority(&self) -> ContractMaintenanceAuthority {
        self.0.clone().into()
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
#[derive(Clone)]
pub struct ContractOperationVersion(ledger::structure::ContractOperationVersion);

#[wasm_bindgen]
impl ContractOperationVersion {
    #[wasm_bindgen(constructor)]
    pub fn new(version: &str) -> Result<ContractOperationVersion, JsError> {
        use ledger::structure::ContractOperationVersion as V;
        Ok(ContractOperationVersion(match version {
            "v1" => {
                return Err(JsError::new(&format!(
                    "superseded contract operation version: {version}"
                )));
            }
            "v2" => V::V2,
            _ => {
                return Err(JsError::new(&format!(
                    "unknown contract operation version: {version}"
                )));
            }
        }))
    }

    #[wasm_bindgen(getter = version)]
    pub fn version(&self) -> String {
        use ledger::structure::ContractOperationVersion as V;
        match &self.0 {
            V::V2 => "v2",
            _ => unreachable!("non exhaustive pattern should be exhaustive in this scope"),
        }
        .to_owned()
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
#[derive(Clone)]
pub struct ContractOperationVersionedVerifierKey(
    ledger::structure::ContractOperationVersionedVerifierKey,
);

#[wasm_bindgen]
impl ContractOperationVersionedVerifierKey {
    #[wasm_bindgen(constructor)]
    pub fn new(
        version: &str,
        raw_vk: Uint8Array,
    ) -> Result<ContractOperationVersionedVerifierKey, JsError> {
        use ledger::structure::ContractOperationVersionedVerifierKey as V;
        let raw_vk = raw_vk.to_vec();
        Ok(ContractOperationVersionedVerifierKey(match version {
            "v1" => {
                return Err(JsError::new(&format!(
                    "superceded contract operation version: {version}"
                )));
            }
            "v2" => V::V2(tagged_deserialize(&mut &raw_vk[..])?),
            _ => {
                return Err(JsError::new(&format!(
                    "unknown contract operation version: {version}"
                )));
            }
        }))
    }

    #[wasm_bindgen(getter = version)]
    pub fn version(&self) -> String {
        use ledger::structure::ContractOperationVersionedVerifierKey as V;
        match &self.0 {
            V::V2(..) => "v2",
            _ => unreachable!("non exhaustive pattern should be exhaustive in this scope"),
        }
        .to_owned()
    }

    #[wasm_bindgen(getter = rawVk)]
    pub fn raw_vk(&self) -> Result<Uint8Array, JsError> {
        use ledger::structure::ContractOperationVersionedVerifierKey as V;
        let mut buf = Vec::new();
        match &self.0 {
            V::V2(vk) => Serializable::serialize(vk, &mut buf)?,
            _ => unreachable!("non exhaustive pattern should be exhaustive in this scope"),
        }
        Ok(Uint8Array::from(&buf[..]))
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
pub struct VerifierKeyRemove(EntryPointBuf, ContractOperationVersion);

try_ref_for_exported!(VerifierKeyRemove);

#[wasm_bindgen]
impl VerifierKeyRemove {
    #[wasm_bindgen(constructor)]
    pub fn new(
        operation: JsValue,
        version: &ContractOperationVersion,
    ) -> Result<VerifierKeyRemove, JsError> {
        let operation: EntryPointBuf = EntryPointBuf(from_maybe_string(operation)?);
        Ok(VerifierKeyRemove(operation, version.clone()))
    }

    #[wasm_bindgen(getter = operation)]
    pub fn operation(&self) -> JsValue {
        maybe_string(&self.0.0)
    }

    #[wasm_bindgen(getter = version)]
    pub fn version(&self) -> ContractOperationVersion {
        self.1.clone()
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
pub struct VerifierKeyInsert(EntryPointBuf, ContractOperationVersionedVerifierKey);

try_ref_for_exported!(VerifierKeyInsert);

#[wasm_bindgen]
impl VerifierKeyInsert {
    #[wasm_bindgen(constructor)]
    pub fn new(
        operation: JsValue,
        vk: &ContractOperationVersionedVerifierKey,
    ) -> Result<VerifierKeyInsert, JsError> {
        let operation: EntryPointBuf = EntryPointBuf(from_maybe_string(operation)?);
        Ok(VerifierKeyInsert(operation, vk.clone()))
    }

    #[wasm_bindgen(getter = operation)]
    pub fn operation(&self) -> JsValue {
        maybe_string(&self.0.0)
    }

    #[wasm_bindgen(getter = vk)]
    pub fn vk(&self) -> ContractOperationVersionedVerifierKey {
        self.1.clone()
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
pub struct MaintenanceUpdate(pub(crate) ledger::structure::MaintenanceUpdate<InMemoryDB>);

try_ref_for_exported!(MaintenanceUpdate);

#[wasm_bindgen]
impl MaintenanceUpdate {
    #[wasm_bindgen(constructor)]
    pub fn new(
        address: &str,
        updates: Vec<JsValue>,
        counter: u64,
    ) -> Result<MaintenanceUpdate, JsError> {
        let updates = updates
            .into_iter()
            .map(|su| match ReplaceAuthority::try_ref(&su)? {
                Some(ra) => Ok(SingleUpdate::ReplaceAuthority(ra.0.clone())),
                _ => match VerifierKeyRemove::try_ref(&su)? {
                    Some(rm) => Ok(SingleUpdate::VerifierKeyRemove(
                        rm.0.clone(),
                        rm.1.0.clone(),
                    )),
                    _ => match VerifierKeyInsert::try_ref(&su)? {
                        Some(ins) => Ok(SingleUpdate::VerifierKeyInsert(
                            ins.0.clone(),
                            ins.1.0.clone(),
                        )),
                        _ => Err(JsError::new("Expected SingleUpdate type")),
                    },
                },
            })
            .collect::<Result<Vec<_>, _>>()?;
        let address = from_hex_ser(address)?;
        if counter > u32::MAX as u64 {
            return Err(JsError::new("counter exceeded u32 max"));
        }
        Ok(MaintenanceUpdate(
            ledger::structure::MaintenanceUpdate::new(address, updates, counter as u32),
        ))
    }

    #[wasm_bindgen(js_name = "addSignature")]
    pub fn add_signature(&self, idx: u64, signature: &str) -> Result<MaintenanceUpdate, JsError> {
        let signature: signatures::Signature = from_hex_ser(signature)?;
        if idx > u32::MAX as u64 {
            return Err(JsError::new("idx exceeded u32 max"));
        }
        Ok(MaintenanceUpdate(
            self.0.clone().add_signature(idx as u32, signature),
        ))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter = dataToSign)]
    pub fn data_to_sign(&self) -> Uint8Array {
        self.0.data_to_sign().as_slice().into()
    }

    #[wasm_bindgen(getter = address)]
    pub fn address(&self) -> Result<String, JsError> {
        to_hex_ser(&self.0.address)
    }

    #[wasm_bindgen(getter = updates)]
    pub fn updates(&self) -> Vec<JsValue> {
        self.0
            .updates
            .iter_deref()
            .map(|su| match su.clone() {
                SingleUpdate::ReplaceAuthority(auth) => JsValue::from(ReplaceAuthority(auth)),
                SingleUpdate::VerifierKeyRemove(op, ver) => {
                    JsValue::from(VerifierKeyRemove(op, ContractOperationVersion(ver)))
                }
                SingleUpdate::VerifierKeyInsert(op, vk) => JsValue::from(VerifierKeyInsert(
                    op,
                    ContractOperationVersionedVerifierKey(vk),
                )),
            })
            .collect()
    }

    #[wasm_bindgen(getter = counter)]
    pub fn counter(&self) -> u64 {
        self.0.counter as u64
    }

    #[wasm_bindgen(getter = signatures)]
    pub fn signatures(&self) -> Result<Vec<JsValue>, JsError> {
        self.0
            .signatures
            .iter()
            .map(|sig_value| {
                let idx = sig_value.0;
                let tuple = Array::new();
                tuple.push(&BigInt::from(idx));
                let signature = to_hex_ser(&sig_value.1)?;
                tuple.push(&JsValue::from_str(&signature));
                Ok(tuple.into())
            })
            .collect()
    }
}
