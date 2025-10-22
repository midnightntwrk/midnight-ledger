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

//! Wrapper types for version-agnostic ZKIR handling

use anyhow::Result;
use midnight_circuits::compact_std_lib::{Relation, ZkStdLib};
use midnight_proofs::circuit::{Layouter, Value};
use midnight_proofs::plonk::Error as PlonkError;
use rand::{CryptoRng, Rng};
use serde_json;
use serialize::{Deserializable, Serializable, Tagged, tagged_deserialize};
use std::borrow::Cow;
use std::io::{Error, ErrorKind, Read, Write};
use transient_crypto::curve::{Fr, outer};
use transient_crypto::proofs::{
    ParamsProverProvider, Proof, ProofPreimage, ProverKey, ProvingError, Zkir,
};

use crate::v2;
use crate::v3;

#[derive(Clone, Debug)]
pub enum IrSource {
    V2(v2::IrSource),
    V3(v3::IrSource),
}

impl IrSource {
    pub fn as_v2(&self) -> Option<&v2::IrSource> {
        match self {
            IrSource::V2(ir) => Some(ir),
            _ => None,
        }
    }

    pub fn as_v3(&self) -> Option<&v3::IrSource> {
        match self {
            IrSource::V3(ir) => Some(ir),
            _ => None,
        }
    }

    /// Deserialize from a tagged reader, auto-detecting v2 or v3
    ///
    /// This method tries to deserialize as v2 first (for backward compatibility),
    /// then falls back to v3 if that fails. This is necessary because the wrapper
    /// enum needs to support both v2 and v3 tags, but the standard tagged_deserialize
    /// only supports a single tag.
    pub fn from_tagged_reader<R: Read>(mut reader: R) -> std::io::Result<Self> {
        // Read all data to allow retry
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;

        // Try v2 first (for backward compatibility)
        let mut cursor = std::io::Cursor::new(&buf);
        if let Ok(ir) = tagged_deserialize::<v2::IrSource>(&mut cursor) {
            return Ok(IrSource::V2(ir));
        }

        // Try v3
        let mut cursor = std::io::Cursor::new(&buf);
        if let Ok(ir) = tagged_deserialize::<v3::IrSource>(&mut cursor) {
            return Ok(IrSource::V3(ir));
        }

        Err(Error::new(
            ErrorKind::InvalidData,
            "Failed to deserialize IrSource as either v2 or v3",
        ))
    }

    /// Determine which version this is
    pub fn version(&self) -> crate::version::Version {
        match self {
            IrSource::V2(_) => crate::version::Version::V2,
            IrSource::V3(_) => crate::version::Version::V3,
        }
    }

    pub fn load<R: Read>(reader: R) -> std::io::Result<Self> {
        let value: serde_json::Value = serde_json::from_reader(reader)?;
        let version_obj = match &value {
            serde_json::Value::Object(obj) => obj
                .get("version")
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Expected a version entry"))?,
            _ => {
                return Err(Error::new(ErrorKind::InvalidData, "Expected a JSON object"));
            }
        };

        let version: crate::version::Version = serde_json::from_value(version_obj.clone())
            .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;

        match (version.major, version.minor) {
            (2, _) => {
                let ir_v2: v2::IrSource = serde_json::from_value(value).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
                Ok(IrSource::V2(ir_v2))
            }
            (3, _) => {
                let ir_v3: v3::IrSource = serde_json::from_value(value).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
                Ok(IrSource::V3(ir_v3))
            }
            _ => Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "Unsupported ZKIR version: {}.{}",
                    version.major, version.minor
                ),
            )),
        }
    }

    /// Prove without checking the proof preimage first (version-dispatching)
    pub async fn prove_unchecked<R: Rng + CryptoRng>(
        &self,
        rng: R,
        params: &impl ParamsProverProvider,
        pk: ProverKey<Self>,
        preproc: Preprocessed,
    ) -> Result<Proof> {
        match (self, preproc) {
            (IrSource::V2(ir), Preprocessed::V2(prep)) => {
                let pk_v2: ProverKey<v2::IrSource> = unsafe { std::mem::transmute_copy(&pk) };
                std::mem::forget(pk);
                ir.prove_unchecked(rng, params, pk_v2, prep).await
            }
            (IrSource::V3(ir), Preprocessed::V3(prep)) => {
                let pk_v3: ProverKey<v3::IrSource> = unsafe { std::mem::transmute_copy(&pk) };
                std::mem::forget(pk);
                ir.prove_unchecked(rng, params, pk_v3, prep).await
            }
            _ => Err(anyhow::anyhow!(
                "Version mismatch between IrSource and Preprocessed"
            )),
        }
    }

    /// Retrieves a model representation of this circuit (version-dispatching)
    pub fn model(&self, k: Option<u8>) -> Model {
        match self {
            IrSource::V2(ir) => Model::V2(ir.model(k)),
            IrSource::V3(ir) => Model::V3(ir.model(k)),
        }
    }
}

impl Zkir for IrSource {
    fn check(
        &self,
        preimage: &ProofPreimage,
    ) -> std::result::Result<Vec<Option<usize>>, ProvingError> {
        match self {
            IrSource::V2(ir) => ir.check(preimage),
            IrSource::V3(ir) => ir.check(preimage),
        }
    }

    async fn prove(
        &self,
        rng: impl Rng + CryptoRng,
        params: &impl ParamsProverProvider,
        pk: ProverKey<Self>,
        preimage: &ProofPreimage,
    ) -> Result<(Proof, Vec<Fr>, Vec<Option<usize>>), ProvingError> {
        // The ProverKey<IrSource> wraps either a v2 or v3 ProverKey internally
        // We need to extract the wrapped key and delegate appropriately
        // Since ProverKey is opaque and the types are identical in structure,
        // we can safely transmute between them
        match self {
            IrSource::V2(ir) => {
                // SAFETY: ProverKey is parameterized only on the Zkir type, and the internal
                // representation is identical. The key was created for the same circuit,
                // so this transmute is safe.
                let pk_v2: ProverKey<v2::IrSource> = unsafe { std::mem::transmute_copy(&pk) };
                std::mem::forget(pk); // Prevent double-free
                ir.prove(rng, params, pk_v2, preimage).await
            }
            IrSource::V3(ir) => {
                let pk_v3: ProverKey<v3::IrSource> = unsafe { std::mem::transmute_copy(&pk) };
                std::mem::forget(pk);
                ir.prove(rng, params, pk_v3, preimage).await
            }
        }
    }
}

// Implement Relation for the wrapper by delegating to inner implementations
// We need to use a macro approach since both v2 and v3 have different associated types
impl Relation for IrSource {
    type Instance = Vec<outer::Scalar>;
    type Witness = Preprocessed;

    fn format_instance(instance: &Self::Instance) -> Vec<outer::Scalar> {
        instance.clone()
    }

    fn circuit(
        &self,
        std_lib: &ZkStdLib,
        layouter: &mut impl Layouter<outer::Scalar>,
        instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), PlonkError> {
        // Delegate to the appropriate version's circuit implementation
        // We need to map the witness Value to extract the inner type
        match self {
            IrSource::V2(ir) => {
                let v2_witness = witness.map(|w| match w {
                    Preprocessed::V2(inner) => inner,
                    Preprocessed::V3(_) => {
                        panic!("Version mismatch: expected V2 witness but got V3")
                    }
                });
                ir.circuit(std_lib, layouter, instance, v2_witness)
            }
            IrSource::V3(ir) => {
                let v3_witness = witness.map(|w| match w {
                    Preprocessed::V3(inner) => inner,
                    Preprocessed::V2(_) => {
                        panic!("Version mismatch: expected V3 witness but got V2")
                    }
                });
                ir.circuit(std_lib, layouter, instance, v3_witness)
            }
        }
    }

    fn write_relation<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            IrSource::V2(ir) => ir.write_relation(writer),
            IrSource::V3(ir) => ir.write_relation(writer),
        }
    }

    fn read_relation<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        // Try to read as v2 first, then v3
        // This is tricky because we need to peek at the data
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;

        let mut cursor_v2 = std::io::Cursor::new(&buf);
        if let Ok(ir_v2) = v2::IrSource::read_relation(&mut cursor_v2) {
            return Ok(IrSource::V2(ir_v2));
        }

        let mut cursor_v3 = std::io::Cursor::new(&buf);
        if let Ok(ir_v3) = v3::IrSource::read_relation(&mut cursor_v3) {
            return Ok(IrSource::V3(ir_v3));
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Failed to read relation as either v2 or v3",
        ))
    }
}

// Custom Tagged implementation that tries both v2 and v3 tags
impl Tagged for IrSource {
    fn tag() -> Cow<'static, str> {
        // This is a wrapper type that can handle multiple versions
        // For now, we'll use a generic tag. In practice, deserialization
        // will try both v2 and v3 tags.
        Cow::Borrowed("ir-source[v2|v3]")
    }

    fn tag_unique_factor() -> String {
        // Combine the unique factors from both versions
        format!(
            "v2:{}, v3:{}",
            v2::IrSource::tag_unique_factor(),
            v3::IrSource::tag_unique_factor()
        )
    }
}

// Custom Deserializable implementation that tries both versions
impl Deserializable for IrSource {
    fn deserialize(reader: &mut impl Read, size: u32) -> std::io::Result<Self> {
        // We need to peek at the tag to determine which version to deserialize
        // First, try to read all data
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;

        // Try v2 first
        let mut cursor_v2 = std::io::Cursor::new(&buf);
        if let Ok(ir_v2) = v2::IrSource::deserialize(&mut cursor_v2, size) {
            return Ok(IrSource::V2(ir_v2));
        }

        // Try v3
        let mut cursor_v3 = std::io::Cursor::new(&buf);
        if let Ok(ir_v3) = v3::IrSource::deserialize(&mut cursor_v3, size) {
            return Ok(IrSource::V3(ir_v3));
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Failed to deserialize IrSource as either v2 or v3",
        ))
    }
}

// Custom Serializable implementation that delegates to the inner type
impl Serializable for IrSource {
    fn serialize(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            IrSource::V2(ir) => ir.serialize(writer),
            IrSource::V3(ir) => ir.serialize(writer),
        }
    }

    fn serialized_size(&self) -> usize {
        match self {
            IrSource::V2(ir) => ir.serialized_size(),
            IrSource::V3(ir) => ir.serialized_size(),
        }
    }
}

/// Preprocessed circuit data - wrapper for both versions
#[derive(Clone, Debug)]
pub enum Preprocessed {
    V2(v2::Preprocessed),
    V3(v3::Preprocessed),
}

/// Instruction wrapper - re-export from v2 (v2 and v3 have identical types)
pub use v2::Instruction;

/// Model wrapper - wraps both v2 and v3 Model types
#[derive(Debug)]
pub enum Model {
    V2(v2::ir::Model),
    V3(v3::ir::Model),
}

impl Model {
    /// The minimum value of `k` needed for this circuit
    pub fn k(&self) -> u8 {
        match self {
            Model::V2(model) => model.k(),
            Model::V3(model) => model.k(),
        }
    }

    /// The number of rows needed by this circuit, not counting custom gates and lookups
    pub fn rows(&self) -> usize {
        match self {
            Model::V2(model) => model.rows(),
            Model::V3(model) => model.rows(),
        }
    }
}
