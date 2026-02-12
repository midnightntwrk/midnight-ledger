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

use anyhow::Result;
use base_crypto::fab::Alignment;
use midnight_proofs::dev::cost_model::CircuitModel;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::io::{self, Read};
use std::sync::Arc;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::{
    ParamsProverProvider, Proof, ProofPreimage, ProverKey, ProvingError, TranscriptHash, Zkir,
};

use crate::ir_types::IrType;

/// A low-level IR allowing the prover to populate circuit witnesses.
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize, Serializable)]
#[tag = "ir-source[v3]"]
pub struct IrSource {
    /// The list of input identifiers for this circuit
    pub inputs: Vec<TypedIdentifier>,
    /// Whether this IR should compile a communications commitment
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
        use midnight_zk_stdlib::prove;

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

/// An identifier for a variable in the circuit memory
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Serializable)]
#[tag = "zkir-identifier[v1]"]
pub struct Identifier(pub String);

tag_enforcement_test!(Identifier);

/// A typed identifier for a variable in the circuit memory
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Serializable)]
#[tag = "zkir-typed-identifier[v1]"]
pub struct TypedIdentifier {
    pub(crate) name: Identifier,
    #[serde(rename = "type")]
    pub(crate) val_t: IrType,
}

tag_enforcement_test!(TypedIdentifier);

/// An operand that can be either a variable reference or an immediate value
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// A reference to a variable in circuit memory
    Variable(Identifier),
    /// An immediate field element value
    Immediate(Fr),
}

impl serde::Serialize for Operand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Operand::Variable(id) => serde::Serialize::serialize(&id.0, serializer),
            Operand::Immediate(imm) => {
                let mut repr = imm.as_le_bytes();
                while repr.last() == Some(&0) && repr.len() > 1 {
                    repr.pop();
                }
                serializer.serialize_str(&format!("0x{}", const_hex::encode(&repr)))
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for Operand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;

        // Check if this looks like a hex immediate (starts with "0x" or "-0x")
        let mut repr = s.as_bytes();
        let negate = if !repr.is_empty() && repr[0] == b'-' {
            repr = &repr[1..];
            true
        } else {
            false
        };

        if repr.starts_with(b"0x") || repr.starts_with(b"0X") {
            let hex_str = &repr[2..];
            if hex_str.is_empty() {
                return Err(<D::Error as serde::de::Error>::custom(
                    "Invalid operand format: hex immediate must have at least one digit after '0x'",
                ));
            }

            let bytes = const_hex::decode(hex_str)
                .map_err(<D::Error as serde::de::Error>::custom)?
                .into_iter()
                .collect::<Vec<_>>();
            let field = Fr::from_le_bytes(&bytes).ok_or_else(|| {
                <D::Error as serde::de::Error>::custom("Out of range for field element")
            })?;
            Ok(Operand::Immediate(if negate { -field } else { field }))
        } else {
            // Variables must start with '%' in v3
            if !s.starts_with('%') {
                return Err(<D::Error as serde::de::Error>::custom(format!(
                    "Invalid operand format: '{}'. Variables must start with '%', immediates must start with '0x'",
                    s
                )));
            }
            Ok(Operand::Variable(Identifier(s)))
        }
    }
}

impl Serializable for Operand {
    fn serialize(&self, sink: &mut impl std::io::Write) -> Result<(), std::io::Error> {
        match self {
            Operand::Variable(id) => {
                // Write variant tag 0 for Variable
                Serializable::serialize(&0u8, sink)?;
                Serializable::serialize(id, sink)
            }
            Operand::Immediate(imm) => {
                // Write variant tag 1 for Immediate
                Serializable::serialize(&1u8, sink)?;
                let mut repr = imm.as_le_bytes();
                while repr.last() == Some(&0) && repr.len() > 1 {
                    repr.pop();
                }
                let s = format!("0x{}", const_hex::encode(&repr));
                Serializable::serialize(&s, sink)
            }
        }
    }

    fn serialized_size(&self) -> usize {
        let variant_size = 1; // 1 byte for the variant tag
        variant_size
            + match self {
                Operand::Variable(id) => id.serialized_size(),
                Operand::Immediate(imm) => {
                    let mut repr = imm.as_le_bytes();
                    while repr.last() == Some(&0) && repr.len() > 1 {
                        repr.pop();
                    }
                    let s = format!("0x{}", const_hex::encode(&repr));
                    s.serialized_size()
                }
            }
    }
}

impl Deserializable for Operand {
    fn deserialize(source: &mut impl Read, _max_depth: u32) -> Result<Self, io::Error> {
        // Read the variant tag
        let variant_tag = <u8 as Deserializable>::deserialize(source, _max_depth)?;

        match variant_tag {
            0 => {
                // Variable variant
                let id = <Identifier as Deserializable>::deserialize(source, _max_depth)?;
                Ok(Operand::Variable(id))
            }
            1 => {
                // Immediate variant
                let s: String = Deserializable::deserialize(source, _max_depth)?;

                // Parse the hex string
                let mut repr = s.as_bytes();
                let negate = if !repr.is_empty() && repr[0] == b'-' {
                    repr = &repr[1..];
                    true
                } else {
                    false
                };

                if !repr.starts_with(b"0x") && !repr.starts_with(b"0X") {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Expected hex immediate to start with '0x', got: {}", s),
                    ));
                }

                let hex_str = &repr[2..];
                if hex_str.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid operand format: hex immediate must have at least one digit after '0x'",
                    ));
                }

                let bytes = const_hex::decode(hex_str)
                    .map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                    })?
                    .into_iter()
                    .collect::<Vec<_>>();
                let field = Fr::from_le_bytes(&bytes).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Out of range for field element")
                })?;
                Ok(Operand::Immediate(if negate { -field } else { field }))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid Operand variant tag: {}", variant_tag),
            )),
        }
    }
}

impl Tagged for Operand {
    fn tag() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("zkir-operand[v1]")
    }

    fn tag_unique_factor() -> String {
        "[zkir-identifier,fr]".to_string()
    }
}
tag_enforcement_test!(Operand);

/// An individual ZK IR instruction
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Serializable)]
#[serde(rename_all = "snake_case", tag = "op")]
#[tag = "ir-instruction[v3]"]
pub enum Instruction {
    /// Encodes the given value as a vector of raw Fr elements.
    ///
    /// This operation will result in an error if the number of outputs
    /// is not the exact number of raw Fr elements required to represent a
    /// value of the input type:
    ///
    ///  - Native:      1 output
    ///  - JubjubPoint: 2 outputs (x and y coordinates)
    Encode {
        /// The value to encode
        input: Operand,
        /// The output variable names
        outputs: Vec<Identifier>,
    },
    /// Decodes the given raw Fr elements as a value of the given type.
    ///
    /// This operation will result in an error if the number of inputs
    /// is not the exact number of raw Fr elements required to represent a
    /// value of the given type:
    ///
    ///  - Native:      1 input
    ///  - JubjubPoint: 2 inputs (x and y coordinates)
    ///
    /// It will also result in an error if the operands are not of type
    /// `Native`.
    ///
    /// The circuit may become unsatisfiable if the inputs do not encode
    /// a valid value of the given type.
    Decode {
        /// The inputs to decode
        inputs: Vec<Operand>,
        /// The type to decode as
        #[serde(rename = "type")]
        val_t: IrType,
        /// The output variable name
        output: Identifier,
    },
    /// Assert that `cond` has value `1`. UB if `cond` is not `0` or `1`.
    ///
    /// No outputs
    Assert {
        /// The boolean condition being asserted
        cond: Operand,
    },
    /// Conditionally select a value. UB if `bit` is not `0` or `1`.
    ///
    /// Outputs one element, identical to `a` or `b`
    CondSelect {
        /// A boolean selector, if `1`, select `a`, else `b`
        bit: Operand,
        /// The value to select for `1`
        a: Operand,
        /// The value to select for `0`
        b: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Constrains `val` to a set number of bits.
    ///
    /// No outputs
    ConstrainBits {
        /// The value to constrain
        val: Operand,
        /// The number of bits to constrain it to
        bits: u32,
    },
    /// Constrains two values `a` and `b` to be equal.
    ///
    /// No outputs
    ConstrainEq {
        /// The first value to constrain
        a: Operand,
        /// The second value to constrain
        b: Operand,
    },
    /// Constrains a value `val` to be a boolean (`0` or `1`).
    ///
    /// No outputs
    ConstrainToBoolean {
        /// The value to constrain
        val: Operand,
    },
    /// Creates a copy of a value `val`. Superfluous, but potentially useful
    /// in some settings, and does not extend the actual circuit.
    ///
    /// Outputs one element, identical to `val`
    Copy {
        /// The variable or immediate to copy
        val: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Conditional impact instruction - declares multiple public inputs under a guard condition.
    ///
    /// No outputs, but adds the inputs as public inputs and activity information to
    /// [`IrSource::prove`] and [`IrSource::check`].
    Impact {
        /// The boolean condition under which the public inputs are active
        guard: Operand,
        /// The sequence of values to declare as public inputs
        inputs: Vec<Operand>,
    },
    /// Multiplies an elliptic curve point by a scalar.
    /// curve point.
    ///
    /// Outputs 1 element, the product
    EcMul {
        /// The point to be multiplied
        a: Operand,
        /// The scalar to multiply by
        scalar: Operand,
        /// The result of multiplication
        output: Identifier,
    },
    /// Multiplies the group generator by a scalar.
    ///
    /// Outputs 1 element, the product
    EcMulGenerator {
        /// The scalar to multiply by
        scalar: Operand,
        /// The result of multiplication
        output: Identifier,
    },
    /// Hashes a sequence of field elements to an embedded curve point.
    /// All inputs are required to be of type `Native`. Failure otherwise.
    ///
    /// Outputs 1 element, the point
    HashToCurve {
        /// The values to hash to a curve point
        inputs: Vec<Operand>,
        /// The resulting point
        output: Identifier,
    },
    /// Divides with remainder by a power of two (number of bits).
    ///
    /// Two outputs, `val >> bits`, and `val & ((1 << bits) - 1)`
    DivModPowerOfTwo {
        /// The variable to divide
        val: Operand,
        /// The number of bits to divide by
        bits: u32,
        /// The outputs: [division result, modulus result]
        outputs: Vec<Identifier>,
    },
    /// Takes two inputs, `divisor` and `modulus`, and outputs
    /// `divisor << bits | modulus`, guaranteeing that the result does not
    /// overflow the field size, and that `modulus < (1 << bits)`. Inverse of
    /// `DivModPowerOfTwo`.
    ReconstituteField {
        /// The divisor of the reconstituted field element
        divisor: Operand,
        /// The modulus of the reconstituted field element
        modulus: Operand,
        /// The number of bits for `modulus`
        bits: u32,
        /// The output variable name
        output: Identifier,
    },
    /// Outputs `val` from the circuit, including it in the communications
    /// commitment.
    ///
    /// No outputs (at the level of the IR VM), despite the name
    Output {
        /// The variable or immediate to output
        val: Operand,
    },
    /// Calls a circuit-friendly hash function on a sequence of items.
    ///
    /// One output, `H(inputs)`
    TransientHash {
        /// The values to hash
        inputs: Vec<Operand>,
        /// The output variable name
        output: Identifier,
    },
    /// Calls a long-term hash function on a sequence of items with a given
    /// alignment.
    ///
    /// Outputs 2 elements for binary format
    PersistentHash {
        /// The alignment of the inputs being passed
        alignment: Alignment,
        /// The inputs to hash
        inputs: Vec<Operand>,
        /// The output variable names
        outputs: Vec<Identifier>,
    },
    /// Tests if `a` and `b` are equal.
    ///
    /// One boolean output, `a == b`
    TestEq {
        /// The first value to check for equality
        a: Operand,
        /// The second value to check for equality
        b: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Adds `a` and `b`.
    /// Supported on types: `Native, `JubjubPoint`.
    ///
    /// One output `a + b`
    Add {
        /// The first value to add
        a: Operand,
        /// The second value to add
        b: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Multiplies `a` and `b` in the prime field.
    ///
    /// One output `a * b`
    Mul {
        /// The first value to multiply
        a: Operand,
        /// The second value to multiply
        b: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Negates `a` in the prime field.
    ///
    /// One output `-a`
    Neg {
        /// The value to negate
        a: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Boolean not gate.
    ///
    /// One output `!a`
    Not {
        /// The value to negate
        a: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Checks if `a` < `b`, interpreting both as `bits`-bit unsigned
    /// integers. UB if `a` or `b` exceed `bits`.
    ///
    /// One boolean output `a < b`
    LessThan {
        /// The first value to compare
        a: Operand,
        /// The second value to compare
        b: Operand,
        /// The number of bits to compare
        bits: u32,
        /// The output variable name
        output: Identifier,
    },
    /// Retrieves a public input from the public transcript outputs.
    ///
    /// Outputs one element, the next public transcript output, or `0` if the
    /// guard fails
    PublicInput {
        /// An optional condition for retrieving the next public transcript
        /// output
        guard: Option<Operand>,
        /// The output variable name
        output: Identifier,
    },
    /// Retrieves a private input from the private transcript outputs.
    ///
    /// Outputs one element, the next private transcript output, or `0` if the
    /// guard fails
    PrivateInput {
        /// An optional condition for retrieving the next private transcript
        /// output
        guard: Option<Operand>,
        /// The output variable name
        output: Identifier,
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
    pub fn model(&self) -> Model {
        Model {
            model: midnight_zk_stdlib::cost_model(self),
        }
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
                    SerdeVersion { major: 3, minor: 0 } => Ok(serde_json::from_value(value)?),
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
        use midnight_zk_stdlib::prove;

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
