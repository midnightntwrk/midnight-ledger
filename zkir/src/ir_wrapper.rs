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

    pub fn from_tagged_reader<R: Read>(mut reader: R) -> std::io::Result<Self> {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;

        let mut cursor = std::io::Cursor::new(&buf);
        if let Ok(ir) = tagged_deserialize::<v2::IrSource>(&mut cursor) {
            return Ok(IrSource::V2(ir));
        }

        let mut cursor = std::io::Cursor::new(&buf);
        if let Ok(ir) = tagged_deserialize::<v3::IrSource>(&mut cursor) {
            return Ok(IrSource::V3(ir));
        }

        Err(Error::new(
            ErrorKind::InvalidData,
            "Failed to deserialize IrSource as either v2 or v3",
        ))
    }

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
                let ir_v2: v2::IrSource = serde_json::from_value(value)
                    .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
                Ok(IrSource::V2(ir_v2))
            }
            (3, _) => {
                let ir_v3: v3::IrSource = serde_json::from_value(value)
                    .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
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

    pub async fn prove_unchecked<R: Rng + CryptoRng>(
        &self,
        rng: R,
        params: &impl ParamsProverProvider,
        pk: ProverKey<Self>,
        preproc: Preprocessed,
    ) -> Result<Proof> {
        match (self, preproc) {
            (IrSource::V2(ir), Preprocessed::V2(prep)) => {
                // Need to convert ProverKey<IrSource> to ProverKey<v2::IrSource>. Rust doesn't
                // allow this even though the memory layout is identical (both are Arc<Mutex<...>>).
                // transmute_copy is safe here since we're just reinterpreting the pointer and
                // immediately forgetting the original to prevent double-dropping the Arc.
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
        match self {
            IrSource::V2(ir) => {
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

        Err(Error::new(
            ErrorKind::InvalidData,
            "Failed to read relation as either v2 or v3",
        ))
    }
}

// Don't use the wrapper for serialization - use v2::IrSource or v3::IrSource directly.
// This impl only exists to satisfy the Zkir trait bound.
impl Tagged for IrSource {
    fn tag() -> Cow<'static, str> {
        panic!(
            "Don't use the wrapper type directly. Use v2::IrSource or v3::IrSource. \
             For deserialization: use IrSource::from_tagged_reader(). \
             For proving: use prove::<v2::IrSource>() or prove::<v3::IrSource>()."
        )
    }

    fn tag_unique_factor() -> String {
        panic!("Don't use the wrapper type directly. Use v2::IrSource or v3::IrSource.")
    }
}

impl Deserializable for IrSource {
    fn deserialize(_reader: &mut impl Read, _size: u32) -> std::io::Result<Self> {
        // Don't deserialize the wrapper directly. V2 and V3 have different tags, so tagged_deserialize()
        // can't handle both. Use IrSource::from_tagged_reader() instead, which tries both versions.
        // If you're seeing this from generic code like ProofPreimage::prove<Z>(),
        // make sure Z is v2::IrSource or v3::IrSource, not the wrapper.
        Err(Error::new(
            ErrorKind::InvalidData,
            "Don't deserialize the wrapper directly. Use IrSource::from_tagged_reader().",
        ))
    }
}

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

#[derive(Clone, Debug)]
pub enum Preprocessed {
    V2(v2::Preprocessed),
    V3(v3::Preprocessed),
}

pub use v2::Instruction;

#[derive(Debug)]
pub enum Model {
    V2(v2::ir::Model),
    V3(v3::ir::Model),
}

impl Model {
    pub fn k(&self) -> u8 {
        match self {
            Model::V2(model) => model.k(),
            Model::V3(model) => model.k(),
        }
    }

    pub fn rows(&self) -> usize {
        match self {
            Model::V2(model) => model.rows(),
            Model::V3(model) => model.rows(),
        }
    }
}
