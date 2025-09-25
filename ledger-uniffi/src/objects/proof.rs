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
// limitations under the License

use std::sync::Arc;

use crate::FfiError;

// ProvingKeyMaterial
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ProvingKeyMaterial {
    pub prover_key: Vec<u8>,
    pub verifier_key: Vec<u8>,
    pub ir_source: Vec<u8>,
}

impl From<transient_crypto::proofs::ProvingKeyMaterial> for ProvingKeyMaterial {
    fn from(pkm: transient_crypto::proofs::ProvingKeyMaterial) -> Self {
        Self {
            prover_key: pkm.prover_key,
            verifier_key: pkm.verifier_key,
            ir_source: pkm.ir_source,
        }
    }
}

impl From<ProvingKeyMaterial> for transient_crypto::proofs::ProvingKeyMaterial {
    fn from(pkm: ProvingKeyMaterial) -> Self {
        Self {
            prover_key: pkm.prover_key,
            verifier_key: pkm.verifier_key,
            ir_source: pkm.ir_source,
        }
    }
}

// WrappedIr
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WrappedIr {
    pub data: Vec<u8>,
}

impl From<transient_crypto::proofs::WrappedIr> for WrappedIr {
    fn from(wi: transient_crypto::proofs::WrappedIr) -> Self {
        Self { data: wi.0 }
    }
}

impl From<WrappedIr> for transient_crypto::proofs::WrappedIr {
    fn from(wi: WrappedIr) -> Self {
        Self(wi.data)
    }
}

// ProofPreimageVersioned
#[derive(uniffi::Object)]
pub struct ProofPreimageVersioned {
    inner: Arc<ledger::structure::ProofPreimageVersioned>,
}

#[uniffi::export]
impl ProofPreimageVersioned {
    pub fn serialize(&self) -> Result<Vec<u8>, FfiError> {
        use serialize::tagged_serialize;
        let mut buf = Vec::new();
        tagged_serialize(&*self.inner, &mut buf)
            .map_err(|e| FfiError::DeserializeError { details: e.to_string() })?;
        Ok(buf)
    }
}

impl ProofPreimageVersioned {
    #[allow(dead_code)]
    pub(crate) fn from_inner(inner: ledger::structure::ProofPreimageVersioned) -> Self {
        Self { inner: Arc::new(inner) }
    }
    
    #[allow(dead_code)]
    pub fn inner(&self) -> &ledger::structure::ProofPreimageVersioned { 
        &self.inner 
    }
}

// ProofVersioned
#[derive(uniffi::Object)]
pub struct ProofVersioned {
    inner: Arc<ledger::structure::ProofVersioned>,
}

#[uniffi::export]
impl ProofVersioned {
    pub fn serialize(&self) -> Result<Vec<u8>, FfiError> {
        use serialize::tagged_serialize;
        let mut buf = Vec::new();
        tagged_serialize(&*self.inner, &mut buf)
            .map_err(|e| FfiError::DeserializeError { details: e.to_string() })?;
        Ok(buf)
    }
}

impl ProofVersioned {
    #[allow(dead_code)]
    pub(crate) fn from_inner(inner: ledger::structure::ProofVersioned) -> Self {
        Self { inner: Arc::new(inner) }
    }
    
    #[allow(dead_code)]
    pub fn inner(&self) -> &ledger::structure::ProofVersioned { 
        &self.inner 
    }
}

// Helper functions
#[uniffi::export]
pub fn proving_key_material_new(
    prover_key: Vec<u8>,
    verifier_key: Vec<u8>,
    ir_source: Vec<u8>,
) -> ProvingKeyMaterial {
    ProvingKeyMaterial {
        prover_key,
        verifier_key,
        ir_source,
    }
}

#[uniffi::export]
pub fn wrapped_ir_new(data: Vec<u8>) -> WrappedIr {
    WrappedIr { data }
}

#[uniffi::export]
pub fn proof_preimage_versioned_deserialize(data: Vec<u8>) -> Result<Arc<ProofPreimageVersioned>, FfiError> {
    use std::io::Cursor;
    use serialize::tagged_deserialize;
    let cursor = Cursor::new(data);
    let val: ledger::structure::ProofPreimageVersioned = tagged_deserialize(cursor)?;
    Ok(Arc::new(ProofPreimageVersioned { inner: Arc::new(val) }))
}

#[uniffi::export]
pub fn proof_versioned_deserialize(data: Vec<u8>) -> Result<Arc<ProofVersioned>, FfiError> {
    use std::io::Cursor;
    use serialize::tagged_deserialize;
    let cursor = Cursor::new(data);
    let val: ledger::structure::ProofVersioned = tagged_deserialize(cursor)?;
    Ok(Arc::new(ProofVersioned { inner: Arc::new(val) }))
}
