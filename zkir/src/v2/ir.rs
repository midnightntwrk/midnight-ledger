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

//! This module provides zero-knowledge IR used by Compact.

use anyhow::Result;
use base_crypto::fab::Alignment;
use midnight_proofs::dev::cost_model::{CircuitModel, from_circuit_to_circuit_model};
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::io::{self, Read};
use std::sync::Arc;
use transient_crypto::curve::{
    FR_BYTES, Fr,
    outer::{self, POINT_BYTES},
};
use transient_crypto::proofs::{
    ParamsProverProvider, Proof, ProofPreimage, ProverKey, ProvingError, TranscriptHash, Zkir,
};

/// A low-level IR allowing the prover to populate circuit witnesses.
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize, Serializable)]
#[tag = "ir-source[v2]"]
pub struct IrSource {
    /// The number of inputs, the initial elements in the memory
    pub num_inputs: u32,
    /// Whether or not this IR should compile a communications commitment
    pub do_communications_commitment: bool,
    /// The sequence of instructions to run in-circuit
    pub instructions: Arc<Vec<Instruction>>,
}
tag_enforcement_test!(IrSource);
tag_enforcement_test!(ProverKey<IrSource>);

impl Zkir for IrSource {
    fn check(
        &self,
        preimage: &ProofPreimage,
    ) -> std::result::Result<Vec<Option<usize>>, transient_crypto::proofs::ProvingError> {
        Ok(self.preprocess(preimage)?.pi_skips)
    }

    async fn prove(
        &self,
        rng: impl Rng + CryptoRng,
        params: &impl ParamsProverProvider,
        pk: ProverKey<Self>,
        preimage: &ProofPreimage,
    ) -> Result<(Proof, Vec<Fr>, Vec<Option<usize>>), ProvingError> {
        use midnight_circuits::compact_std_lib::prove;

        let params_k = params.get_params(pk.init()?.k()).await?;
        let preproc = self.preprocess(preimage)?;
        let pis = preproc.pis.clone();
        let pi_skips = preproc.pi_skips.clone();

        let pk = pk
            .init()
            .map_err(|_| anyhow::anyhow!("Could not init pk"))?;

        let proof = prove::<_, TranscriptHash>(params_k.as_ref(), &pk, self, &pis, preproc, rng)?;

        Ok((Proof(proof), pis.into_iter().map(Fr).collect(), pi_skips))
    }
}

/// An index referring to the circuit memory of the IR machine
pub type Index = u32;

fn field_ser<S: serde::Serializer>(field: &Fr, serializer: S) -> Result<S::Ok, S::Error> {
    let mut repr = field.as_le_bytes();
    while repr.last() == Some(&0) && repr.len() > 1 {
        repr.pop();
    }
    serde::Serializer::serialize_str(serializer, &const_hex::encode(&repr))
}

fn field_deser<'a, D: serde::Deserializer<'a>>(deserializer: D) -> Result<Fr, D::Error> {
    let repr_str: String = serde::Deserialize::deserialize(deserializer)?;
    let mut repr = repr_str.as_bytes();
    let negate = if !repr.is_empty() && repr[0] == b'-' {
        repr = &repr[1..];
        true
    } else {
        false
    };
    let bytes = const_hex::decode(repr)
        .map_err(<D::Error as serde::de::Error>::custom)?
        .into_iter()
        .collect::<Vec<_>>();
    let field = Fr::from_le_bytes(&bytes)
        .ok_or_else(|| <D::Error as serde::de::Error>::custom("Out of range for field element"))?;
    Ok(if negate { -field } else { field })
}

/// An individual ZK IR instruction
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Serializable)]
#[serde(rename_all = "snake_case", tag = "op")]
#[tag = "ir-instruction[v2]"]
pub enum Instruction {
    /// Assert that `index` has value `1`. UB if `index` is not `0` or `1`.
    ///
    /// No outputs
    Assert {
        /// The boolean condition being asserted
        cond: Index,
    },
    /// Conditionally select a value. UB if `bit` is not `0` or `1`.
    ///
    /// Outputs one element, identical to `a` or `b`
    CondSelect {
        /// A boolean selector, if `1`, select `a`, else `b`
        bit: Index,
        /// The value to select for `1`
        a: Index,
        /// The value to select for `0`
        b: Index,
    },
    /// Constrains a value to a set number of bits.
    ///
    /// No outputs
    ConstrainBits {
        /// The value to constrain
        var: Index,
        /// The number of bits to constrain it to
        bits: u32,
    },
    /// Constrains two values `a` and `b` to be equal.
    ///
    /// No outputs
    ConstrainEq {
        /// The first value to constrain
        a: Index,
        /// The second value to constrain
        b: Index,
    },
    /// Constrains a value `var` to be a boolean (`0` or `1`).
    ///
    /// No outputs
    ConstrainToBoolean {
        /// The value to constrain
        var: Index,
    },
    /// Creates a copy of a value `var`. Superfluous, but potentially useful
    /// in some settings, and does not extend the actual circuit.
    ///
    /// Outputs one element, identical to `var`
    Copy {
        /// The variable to copy
        var: Index,
    },
    /// Declares a variable as the next public input.
    ///
    /// No outputs
    DeclarePubInput {
        /// The variable to use for the public input
        var: Index,
    },
    /// A marker informing the proof assembler that a set of preceding public
    /// inputs belong together (typically as an instruction), and whether they
    /// are active or not.
    ///
    /// Every `DeclarePubInput` should be *followed* by a `PiSkip` covering it.
    ///
    /// No outputs, but adds activity information to [`IrSource::prove`] and
    /// [`IrSource::check`].
    PiSkip {
        /// The boolean condition under which the public input is *not* skipped
        ///
        /// This is only used to inform transcript processing, serving as a marker
        /// for which public inputs comprise an instruction.
        guard: Option<Index>,
        /// The number of public inputs to skip in this group
        count: u32,
    },
    /// Adds two elliptic curve points. UB if either is not a valid curve point.
    ///
    /// Outputs 2 elements, `c_x`, `c_y`
    EcAdd {
        /// The affine x coordinate of `a`
        a_x: Index,
        /// The affine y coordinate of `a`
        a_y: Index,
        /// The affine x coordinate of `b`
        b_x: Index,
        /// The affine y coordinate of `b`
        b_y: Index,
    },
    /// Multiplies an elliptic curve point by a scalar. UB if it is not a valid
    /// curve point.
    ///
    /// Outputs 2 elements, `c_x`, `c_y`
    EcMul {
        /// The affine x coordinate of `a`
        a_x: Index,
        /// The affine y coordinate of `a`
        a_y: Index,
        /// The scalar to multiply by
        scalar: Index,
    },
    /// Multiplies the group generator by a scalar.
    ///
    /// Outputs 2 elements, `c_x`, `c_y`
    EcMulGenerator {
        /// The scalar to multiply by
        scalar: Index,
    },
    /// Hashes a sequence of field elements to an embedded curve point.
    ///
    /// Outputs 2 elements, `c_x`, `c_y`
    HashToCurve {
        /// The values to hash to a curve point
        inputs: Vec<Index>,
    },
    /// Loads a constant into the circuit.
    ///
    /// One output, `imm`
    LoadImm {
        /// The constant to include
        #[serde(serialize_with = "field_ser", deserialize_with = "field_deser")]
        imm: Fr,
    },
    /// Divides with remainder by a power of two (number of bits).
    ///
    /// Two outputs, `var >> bits`, and `var & ((1 << bits) - 1)`
    DivModPowerOfTwo {
        /// The variable to divide
        var: Index,
        /// The number of bits to divide by
        bits: u32,
    },
    /// Takes two inputs, `divisor` and `modulus`, and outputs
    /// `divisor << bits | modulus`, guaranteeing that the result does not
    /// overflow the field size, and that `modulus < (1 << bits)`. Inverse of
    /// `DivModPowerOfTwo`.
    ReconstituteField {
        /// The divisor of the reconstituted field element
        divisor: Index,
        /// The modulus of the reconstituted field element
        modulus: Index,
        /// The number of bits for `modulus`
        bits: u32,
    },
    /// Outputs a `var` from the circuit, including it in the communications
    /// commitment.
    ///
    /// No outputs (at the level of the IR VM), despite the name
    Output {
        /// The variable to output
        var: Index,
    },
    /// Calls a circuit-friendly hash function on a sequence of items.
    ///
    /// One output, `H(inputs)`
    TransientHash {
        /// The values to hash
        inputs: Vec<Index>,
    },
    /// Calls a long-term hash function on a sequence of items with a given
    /// alignment.
    ///
    /// One output, `H(inputs)`, in the binary format
    PersistentHash {
        /// The alignment of the inputs being passed
        alignment: Alignment,
        /// The inputs to hash
        inputs: Vec<Index>,
    },
    /// Tests if `a` and `b` are equal.
    ///
    /// One boolean output, `a == b`
    TestEq {
        /// The first value to check for equality
        a: Index,
        /// The second value to check for equality
        b: Index,
    },
    /// Adds `a` and `b` in the prime field.
    ///
    /// One output `a + b`
    Add {
        /// The first value to add
        a: Index,
        /// The second value to add
        b: Index,
    },
    /// Multiplies `a` and `b` in the prime field.
    ///
    /// One output `a * b`
    Mul {
        /// The first value to multiply
        a: Index,
        /// The second value to multiply
        b: Index,
    },
    /// Negates `a` in the prime field.
    ///
    /// One output `-a`
    Neg {
        /// The value to negate
        a: Index,
    },
    /// Boolean not gate.
    ///
    /// One output `!a`
    Not {
        /// The value to negate
        a: Index,
    },
    /// Checks if `a` < `b`, interpreting both as `bits`-bit unsigned
    /// integers. UB if `a` or `b` exceed `bits`.
    ///
    /// One boolean output `a < b`
    LessThan {
        /// The first value to compare
        a: Index,
        /// The second value to compare
        b: Index,
        /// The number of bits to compare
        bits: u32,
    },
    /// Retrieves a public input from the public transcript outputs.
    ///
    /// Outputs one element, the next public transcript output, or `0` if the
    /// guard fails
    PublicInput {
        /// An optional condition for retrieving the next public transcript
        /// output
        guard: Option<Index>,
    },
    /// Retrieves a private input from the private transcript outputs.
    ///
    /// Outputs one element, the next private transcript output, or `0` if the
    /// guard fails
    PrivateInput {
        /// An optional condition for retrieving the next private transcript
        /// output
        guard: Option<Index>,
    },
}
tag_enforcement_test!(Instruction);

#[derive(Deserialize)]
struct SerdeVersion {
    major: u8,
    minor: u8,
}

#[derive(Debug)]
/// A model containing data about a specific constructed circuit
pub struct Model {
    model: CircuitModel,
}

impl Model {
    /// The minimum value of `k` needed for this circuit
    pub fn k(&self) -> u8 {
        self.model.k as u8
    }

    /// The number of rows needed by this circuit, not counting custom gates and lookups
    pub fn rows(&self) -> usize {
        self.model.rows
    }
}

impl IrSource {
    /// Retrieves a model representation of this circuit.
    pub fn model(&self, k: Option<u8>) -> Model {
        use midnight_circuits::compact_std_lib::MidnightCircuit;
        let model = from_circuit_to_circuit_model::<
            outer::Scalar,
            MidnightCircuit<Self>,
            POINT_BYTES,
            FR_BYTES,
        >(
            k.map(|k| k as u32),
            &MidnightCircuit::from_relation(self),
            self.instructions
                .iter()
                .filter(|op| matches!(op, Instruction::DeclarePubInput { .. }))
                .count(),
        );

        Model { model }
    }

    /// Attempts to parse an arbitrary input as IR.
    pub fn load<R: Read>(reader: R) -> io::Result<Self> {
        let value: serde_json::Value = serde_json::from_reader(reader)?;
        match &value {
            serde_json::Value::Object(obj) => {
                let ver = serde_json::from_value(
                    obj.get("version")
                        .ok_or(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Expected a version entry",
                        ))?
                        .clone(),
                )?;
                match ver {
                    SerdeVersion { major: 2, minor: 0 } => Ok(serde_json::from_value(value)?),
                    SerdeVersion { major, minor } => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Unhandled version: {major}.{minor}"),
                    )),
                }
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Expected a JSON object",
            )),
        }
    }

    /// Intended for testing only. This method enables fully controlling the inputs passed to
    /// proving, to test malicious prover behavior.
    pub async fn prove_unchecked<R: Rng + CryptoRng>(
        &self,
        rng: R,
        params: &impl ParamsProverProvider,
        pk: ProverKey<IrSource>,
        preproc: super::ir_vm::Preprocessed,
    ) -> Result<Proof> {
        use midnight_circuits::compact_std_lib::prove;

        let params_k = params.get_params(pk.init()?.k()).await?;
        let pis = preproc.pis.clone();

        let pk = pk
            .init()
            .map_err(|_| anyhow::anyhow!("Could not init pk"))?;

        let proof = prove::<_, TranscriptHash>(params_k.as_ref(), &pk, self, &pis, preproc, rng)?;

        Ok(Proof(proof))
    }
}

#[cfg(feature = "proptest")]
randomised_serialization_test!(IrSource);

#[cfg(feature = "proptest")]
randomised_serialization_test!(Instruction);
