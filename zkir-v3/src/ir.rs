// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
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
#[tag = "ir-source[v3-generic]"]
pub struct IrSource {
    /// The minor version of this IR.
    pub version: IrMinorVersion,
    /// The list of input identifiers for this circuit
    pub inputs: Vec<TypedIdentifier>,
    /// The output types this circuit returns, positionally. The actual
    /// values are produced by an `Instruction::Output` terminator whose
    /// operand list is type-checked against this signature.
    pub outputs: Vec<IrType>,
    /// Whether this IR should compile a communications commitment
    pub do_communications_commitment: bool,
    /// The sequence of instructions to run in-circuit
    pub instructions: Arc<Vec<Instruction>>,
}
tag_enforcement_test!(IrSource);
tag_enforcement_test!(ProverKey<IrSource>);

#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    Serializable,
)]
#[tag = "ir-minor-version[v3]"]
#[repr(u8)]
#[non_exhaustive]
pub enum IrMinorVersion {
    #[default]
    V0,
}

impl Zkir for IrSource {
    type ProverKey = midnight_zk_stdlib::MidnightPK<IrSource>;

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

    fn k(&self) -> u8 {
        midnight_zk_stdlib::optimal_k(self) as u8
    }

    async fn keygen_vk(
        &self,
        params: &impl ParamsProverProvider,
    ) -> Result<transient_crypto::proofs::VerifierKey, anyhow::Error> {
        use midnight_zk_stdlib::setup_vk;
        Ok(transient_crypto::proofs::VerifierKey::from(setup_vk(
            params.get_params(self.k()).await?.as_ref(),
            self,
        )))
    }

    async fn keygen(
        &self,
        params: &impl ParamsProverProvider,
    ) -> Result<(ProverKey<Self>, transient_crypto::proofs::VerifierKey), anyhow::Error> {
        use midnight_zk_stdlib::{setup_pk, setup_vk};
        let vk = setup_vk(params.get_params(self.k()).await?.as_ref(), self);
        let pk = setup_pk(self, &vk);
        Ok((
            ProverKey::from_raw(pk),
            transient_crypto::proofs::VerifierKey::from(vk),
        ))
    }

    fn load_ir_from_tagged(reader: impl Read + std::io::Seek) -> std::io::Result<Self> {
        serialize::tagged_deserialize(reader)
    }

    fn load_prover_key_from_tagged(
        reader: impl Read + std::io::Seek,
    ) -> std::io::Result<ProverKey<Self>> {
        serialize::tagged_deserialize(reader)
    }

    fn read_raw_pk(reader: impl Read) -> std::io::Result<Self::ProverKey> {
        midnight_zk_stdlib::MidnightPK::<Self>::read(
            &mut { reader },
            midnight_proofs::utils::SerdeFormat::RawBytesUnchecked,
        )
    }

    fn write_raw_pk(writer: impl std::io::Write, pk: &Self::ProverKey) -> std::io::Result<()> {
        pk.write(
            &mut { writer },
            midnight_proofs::utils::SerdeFormat::RawBytesUnchecked,
        )
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
    ///  - Native:       1 output
    ///  - Bytes32:      2 outputs (low 31 bytes, high byte)
    ///  - JubjubPoint:  2 outputs (x and y coordinates)
    ///  - JubjubScalar: 1 output
    ///
    /// Foreign-field elements encode as 2 limbs (midnight-circuits'
    /// public-input encoding); points as x and y coordinates (2 limbs each)
    /// followed, on Weierstrass curves only, by an is-identity flag:
    ///
    ///  - Secp256k1Point:  5 outputs
    ///  - Secp256k1Base:   2 outputs
    ///  - Secp256k1Scalar: 2 outputs
    ///
    ///  - Secp256r1Point:  5 outputs
    ///  - Secp256r1Base:   2 outputs
    ///  - Secp256r1Scalar: 2 outputs
    ///
    ///  - Curve25519Point:  4 outputs
    ///  - Curve25519Base:   2 outputs
    ///  - Curve25519Scalar: 2 outputs
    Encode {
        /// The value to encode
        input: Operand,
        /// The output variable names
        outputs: Vec<Identifier>,
    },
    /// Assert that `cond` has value `1`. UB if `cond` is not `0` or `1`.
    ///
    /// No outputs
    Assert {
        /// The boolean condition being asserted
        cond: Operand,
    },
    /// Conditionally select a value. UB if `bit` is not `0` or `1`.
    /// Supported on types:
    ///  - Native
    ///  - JubjubPoint
    ///  - Secp256k1Point
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///  - Secp256r1Point
    ///  - Secp256r1Base
    ///  - Secp256r1Scalar
    ///  - Curve25519Point
    ///  - Curve25519Base
    ///  - Curve25519Scalar
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
    /// Supported on types:
    ///  - Native
    ///  - JubjubPoint
    ///  - Secp256k1Point
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///  - Secp256r1Point
    ///  - Secp256r1Base
    ///  - Secp256r1Scalar
    ///  - Curve25519Point
    ///  - Curve25519Base
    ///  - Curve25519Scalar
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
    ///
    /// In-circuit, if `guard` is `false`, instead of adding the `inputs` as public inputs,
    /// it will add `n` zeros as public inputs (where `n` is the number of `inputs`).
    /// This is enforced with in-circuit constraints.
    ///
    /// NB: Currently, we require that all `inputs` be of type `Native`.
    /// A runtime error will be raised otherwise.
    Impact {
        /// The boolean condition under which the public inputs are active
        guard: Operand,
        /// The sequence of values to declare as public inputs
        inputs: Vec<Operand>,
    },
    /// Multiplies an elliptic curve point by a scalar.
    /// Supported on types:
    ///  - `JubjubPoint x JubjubScalar`
    ///  - `Secp256k1Point x Secp256k1Scalar`
    ///  - `Secp256r1Point x Secp256r1Scalar`
    ///  - `Curve25519Point x Curve25519Scalar`
    ///
    /// This operation will result in an error if the input types are not
    /// supported.
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
    /// This operation will result in an error if the operand given as `scalar`
    /// is not of type `JubjubScalar`.
    ///
    /// Outputs 1 element, the product
    EcMulGenerator {
        /// The scalar to multiply by
        scalar: Operand,
        /// The result of multiplication
        output: Identifier,
    },
    /// Hashes a sequence of field elements to a Jubjub point.
    /// All inputs are required to be of type `Native`. Failure otherwise.
    ///
    /// Outputs 1 element, the point
    HashToCurve {
        /// The values to hash to a curve point
        inputs: Vec<Operand>,
        /// The resulting point
        output: Identifier,
    },
    /// The affine coordinates of the given elliptic curve point.
    /// On Weierstrass curves the identity has no affine coordinates, so
    /// extracting them errors off-circuit and is unsatisfiable in-circuit.
    ///
    /// Supported on types:
    /// * JubjubPoint
    /// * Secp256k1Point
    /// * Secp256r1Point
    /// * Curve25519Point
    ///
    /// Outputs 2 elements, the coordinates (x, y)
    IntoCoordinates {
        /// The point whose coordinate are extracted
        point: Operand,
        /// The output variable names (x, y)
        outputs: (Identifier, Identifier),
    },
    /// Reconstructs an elliptic curve point from the given affine coordinates.
    ///
    /// On Weierstrass curves the identity cannot be built with this instruction.
    ///
    /// Supported on types:
    /// * (Native, Native):               producing a JubjubPoint
    /// * (Secp256k1Base, Secp256k1Base): producing a Secp256k1Point
    /// * (Secp256r1Base, Secp256r1Base): producing a Secp256r1Point
    /// * (Curve25519Base, Curve25519Base): producing a Curve25519Point
    ///
    /// Outputs 1 element, the point
    FromCoordinates {
        /// The affine coordinates (x, y)
        inputs: (Operand, Operand),
        /// The output variable names
        output: Identifier,
    },
    /// Transforms the given value into its 32-byte representation.
    ///
    /// Supported on types:
    /// * Native
    /// * Secp256k1Base
    /// * Secp256k1Scalar
    /// * Secp256r1Base
    /// * Secp256r1Scalar
    /// * Curve25519Base
    /// * Curve25519Scalar
    ///
    /// In all the above prime fields, the 32-byte representation is the little-endian
    /// byte encoding of the underlying (canonical) integer.
    IntoBytes32 {
        /// The element to be converted
        input: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Constructs an element of the given type from its 32-byte representation.
    ///
    /// Supported on types:
    /// * Native
    /// * Secp256k1Base
    /// * Secp256k1Scalar
    /// * Secp256r1Base
    /// * Secp256r1Scalar
    /// * Curve25519Base
    /// * Curve25519Scalar
    ///
    /// In all the above prime fields, the 32-byte representation is the little-endian
    /// byte encoding of the underlying (canonical) integer.
    ///
    /// This operation also accepts non-canonical 32-byte representation in prime fields
    /// by applying the relevant modular reduction.
    FromBytes32 {
        /// The input bytes
        bytes: Operand,
        /// The type to be converted into
        #[serde(rename = "type")]
        val_t: IrType,
        /// The output variable name
        output: Identifier,
    },
    /// Reverses the byte order of a `Bytes32` value.
    ///
    /// The input must be of type `Bytes32`, otherwise this operation fails. The
    /// output is a `Bytes32` whose bytes are those of the input in reverse
    /// order, i.e. the first byte becomes the last and vice versa.
    ///
    /// Outputs 1 element, the reversed bytes
    ReverseBytes {
        /// The bytes to be reversed
        bytes: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Decomposes a `Bytes32` value into two `Native` field elements.
    ///
    /// The first output (`low`) encodes the first 31 bytes of the input as a
    /// little-endian native field element. The second output (`high`) encodes
    /// the 32nd (most significant) byte as a native field element.
    ///
    /// This is the inverse of `Bytes32FromLowHigh`.
    ///
    /// This instruction imposes no off-circuit errors and no in-circuit constraints.
    ///
    /// # Note
    ///
    /// This instruction is a temporary bridge for Compact, which cannot yet deal with
    /// `Bytes32` values directly. It is intended to be removed once Compact can handle
    /// `Bytes32` (or `Bytes(n)`) without decomposing it into field elements.
    Bytes32IntoLowHigh {
        /// The input bytes
        bytes: Operand,
        /// The output variables: (low, high)
        outputs: (Identifier, Identifier),
    },
    /// Constructs a `Bytes32` value from two `Native` field elements in low-high form.
    ///
    /// The first input (`low`) must encode at most 31 bytes, i.e. its value must be
    /// less than 2^248. The second input (`high`) must encode a single byte, i.e. its
    /// value must be less than 256. The result concatenates the first 31 bytes from `low`
    /// with byte `high`.
    ///
    /// This is the inverse of `Bytes32IntoLowHigh`.
    ///
    /// # Errors and constraints
    ///
    /// Off-circuit: returns an error if `low >= 2^248` or `high >= 256`.
    ///
    /// In-circuit: the constraint `low < 2^248` is enforced by asserting that the
    /// 32nd byte of the little-endian decomposition of `low` is zero, making the
    /// circuit unsatisfiable if violated. The constraint `high < 256` is enforced
    /// by a byte range check on `high`, also causing unsatisfiability if violated.
    ///
    /// # Note
    ///
    /// This instruction is a temporary bridge for Compact, which cannot yet deal with
    /// `Bytes32` values directly. It is intended to be removed once Compact can handle
    /// `Bytes32` (or `Bytes(n)`) without decomposing it into field elements.
    Bytes32FromLowHigh {
        /// The inputs: (low, high)
        inputs: (Operand, Operand),
        /// The output variable name
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
    /// Outputs a value of type Bytes32.
    PersistentHash {
        /// The alignment of the inputs being passed
        alignment: Alignment,
        /// The inputs to hash
        inputs: Vec<Operand>,
        /// The output variable names
        output: Identifier,
    },
    /// Evaluates the Keccak-256 hash function on a sequence of items with
    /// a given alignment.
    ///
    /// Outputs a value of type Bytes32.
    Keccak256 {
        /// The alignment of the inputs being passed
        alignment: Alignment,
        /// The inputs to hash
        inputs: Vec<Operand>,
        /// The output variable names
        output: Identifier,
    },
    /// Tests if `a` and `b` are equal.
    /// Supported on types:
    ///  - Native
    ///  - JubjubPoint
    ///  - Secp256k1Point
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///  - Secp256r1Point
    ///  - Secp256r1Base
    ///  - Secp256r1Scalar
    ///  - Curve25519Point
    ///  - Curve25519Base
    ///  - Curve25519Scalar
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
    /// Supported on types:
    ///  - Native
    ///  - JubjubPoint
    ///  - Secp256k1Point
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///  - Secp256r1Point
    ///  - Secp256r1Base
    ///  - Secp256r1Scalar
    ///  - Curve25519Point
    ///  - Curve25519Base
    ///  - Curve25519Scalar
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
    /// Multiplies `a` and `b`.
    /// Supported on types:
    ///  - Native
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///  - Secp256r1Base
    ///  - Secp256r1Scalar
    ///  - Curve25519Base
    ///  - Curve25519Scalar
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
    /// Negates `a`.
    /// Supported on types:
    ///  - Native
    ///  - JubjubPoint
    ///  - Secp256k1Point
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///  - Secp256r1Point
    ///  - Secp256r1Base
    ///  - Secp256r1Scalar
    ///  - Curve25519Point
    ///  - Curve25519Base
    ///  - Curve25519Scalar
    ///
    /// One output `-a`
    Neg {
        /// The value to negate
        a: Operand,
        /// The output variable name
        output: Identifier,
    },
    /// Inverts `a`, results in an error if `a` is zero.
    /// Supported on types:
    ///  - Native
    ///  - Secp256k1Base
    ///  - Secp256k1Scalar
    ///  - Secp256r1Base
    ///  - Secp256r1Scalar
    ///  - Curve25519Base
    ///  - Curve25519Scalar
    ///
    /// One output `a^(-1)`
    Inv {
        /// The value to invert
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
    /// Cast a Native value as a Jubjub scalar by reducing it modulo the Jubjub scalar
    /// field order if necessary.
    ///
    /// NB: This instruction will be removed when the BigUint type becomes available.
    JubjubScalarFromNative {
        /// The native value to be converted.
        native: Operand,
        /// The output variable name.
        output: Identifier,
    },
    /// Off-circuit (preprocessing):
    /// Retrieves an input from the public transcript outputs.
    /// Outputs one element, the next public transcript output, or a default value
    /// if the `guard` fails.
    ///
    /// In-circuit:
    /// Allows the prover to witness a free value, only constrained to respect
    /// the type `val_t`. The `guard` DOES NOT participate in in-circuit constraints.
    ///
    /// NB: This instruction is essentially identical to `PrivateInput` except that
    /// the `preprocessing` pass will consume the value from a different source
    /// (the public transcript outputs in this case).
    PublicInput {
        /// An optional condition for retrieving the next public transcript
        /// output
        guard: Option<Operand>,
        /// The type of this input
        #[serde(rename = "type")]
        val_t: IrType,
        /// The output variable name
        output: Identifier,
    },

    /// Off-circuit (preprocessing):
    /// Retrieves an input from the private transcript outputs.
    /// Outputs one element, the next private transcript output, or a default value
    /// if the `guard` fails.
    ///
    /// In-circuit:
    /// Allows the prover to witness a free value, only constrained to respect
    /// the type `val_t`. The `guard` DOES NOT participate in in-circuit constraints.
    ///
    /// NB: This instruction is essentially identical to `PublicInput` except that
    /// the `preprocessing` pass will consume the value from a different source
    /// (the private transcript outputs in this case).
    PrivateInput {
        /// An optional condition for retrieving the next private transcript
        /// output
        guard: Option<Operand>,
        /// The type of this input
        #[serde(rename = "type")]
        val_t: IrType,
        /// The output variable name
        output: Identifier,
    },
    /// Circuit terminator. Produces the circuit's return values, in
    /// signature order, and ends execution. The operand list is type-checked
    /// against `IrSource::outputs` (length and per-position type), then each
    /// operand is encoded via `encode_offcircuit` / `encode_incircuit` and
    /// pushed into the outputs accumulator.
    Output {
        /// The values returned, one per `IrSource::outputs[i]`.
        vals: Vec<Operand>,
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
            model: midnight_zk_stdlib::cost_model(self, None),
        }
    }

    /// Attempts to parse an arbitrary input as IR.
    pub fn load<R: Read>(reader: R) -> io::Result<Self> {
        let value: serde_json::Value = serde_json::from_reader(reader)?;
        match value {
            serde_json::Value::Object(mut obj) => {
                let ver = serde_json::from_value(
                    obj.get("version")
                        .ok_or(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Expected a version entry",
                        ))?
                        .clone(),
                )?;
                match ver {
                    SerdeVersion {
                        major: 3,
                        minor: 0..=0,
                    } => {
                        obj.insert(
                            "version".into(),
                            serde_json::Value::Number(ver.minor.into()),
                        );
                        Ok(serde_json::from_value(serde_json::Value::Object(obj))?)
                    }
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
