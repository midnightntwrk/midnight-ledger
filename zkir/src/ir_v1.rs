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

//! Compatibility layer for the v1 (zk-stdlib v1) pipeline.
//!
//! This module implements [`midnight_zk_stdlib_v1::Relation`] directly on
//! [`IrSource`] so that v1 zk-stdlib functions (prove, keygen, etc.) can work
//! with it without depending on `zkir-old`. The v1 circuit types (from
//! `midnight-circuits` v6, `midnight-proofs` v0.7) differ from the v2 types
//! at the Rust type level even though the underlying field elements are
//! identical, so scalar conversions go through `to_bytes_le()` / `from_bytes_le()`.
//!
//! This module also provides adapter types and helper functions for bridging
//! the current (v2) proving/verification types to the v1 pipeline.

use base_crypto::fab::{Alignment, AlignmentAtom, AlignmentSegment};
use group::Group;
use midnight_circuits_v1::instructions::{
    ArithInstructions, AssertionInstructions, AssignmentInstructions, BinaryInstructions,
    ControlFlowInstructions, ConversionInstructions, DecompositionInstructions, EccInstructions,
    EqualityInstructions, PublicInputInstructions, ZeroInstructions,
};
use midnight_circuits_v1::types::{AssignedBit, AssignedByte, AssignedNative, AssignedNativePoint};
use midnight_proofs_v1::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use midnight_zk_stdlib_v1::{Relation, ZkStdLib, ZkStdLibArch};
use serialize::{Deserializable, Serializable, Tagged, VecExt, peek_tag};
use std::io::{self, Read, Seek};
use transient_crypto::curve::{FR_BITS, FR_BYTES_STORED};
use transient_crypto::fab::AlignmentExt;

use super::ir::{Instruction as I, IrSource};

/// The old outer scalar type, from midnight-curves v0.2 (via transient-crypto-old).
type OldScalar = transient_crypto_old::curve::outer::Scalar;

/// The old embedded affine type.
type OldEmbeddedAffineExt = transient_crypto_old::curve::embedded::AffineExtended;

/// The old embedded affine (non-extended).
type OldEmbeddedAffine = transient_crypto_old::curve::embedded::Affine;

/// Converts a current outer scalar to the old (v1) outer scalar.
/// Both are BLS12-381 Fq with identical byte representations.
fn cvt(s: transient_crypto::curve::outer::Scalar) -> OldScalar {
    OldScalar::from_bytes_le(&s.to_bytes_le()).expect("BLS12-381 Fq round-trip")
}

/// The v1 preprocessed witness data, using old scalar types.
#[derive(Clone, Debug)]
pub struct V1Preprocessed {
    pub memory: Vec<OldScalar>,
    pub pis: Vec<OldScalar>,
    pub pi_skips: Vec<Option<usize>>,
    pub binding_input: OldScalar,
    pub comm_comm: Option<(OldScalar, OldScalar)>,
}

impl V1Preprocessed {
    /// Converts from the current `Preprocessed` type.
    pub fn from_current(p: &super::ir_vm::Preprocessed) -> Self {
        Self {
            memory: p.memory.iter().copied().map(cvt).collect(),
            pis: p.pis.iter().copied().map(cvt).collect(),
            pi_skips: p.pi_skips.clone(),
            binding_input: cvt(p.binding_input),
            comm_comm: p.comm_comm.map(|(a, b)| (cvt(a), cvt(b))),
        }
    }
}

// --- v1 circuit helpers (using old types) ---

fn v1_lnot(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    a: &AssignedNative<OldScalar>,
) -> Result<AssignedNative<OldScalar>, Error> {
    let bit = std.is_zero(layouter, a)?;
    std.convert(layouter, &bit)
}

fn v1_fab_decode_to_bytes(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    align: &Alignment,
    mut inputs: &[AssignedNative<OldScalar>],
) -> Result<Vec<AssignedByte<OldScalar>>, Error> {
    let mut res = Vec::with_bounded_capacity(align.bin_len());
    let _ = v1_fab_decode_to_bytes_inner(std, layouter, align, &mut inputs, &mut res)?;
    Ok(res)
}

fn v1_fab_decode_to_bytes_inner(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    align: &Alignment,
    inputs: &mut &[AssignedNative<OldScalar>],
    res: &mut Vec<AssignedByte<OldScalar>>,
) -> Result<AssignedNative<OldScalar>, Error> {
    let mut acc = std.assign_fixed(layouter, 0.into())?;
    for segment in align.0.iter() {
        match segment {
            AlignmentSegment::Atom(atom) => {
                v1_fab_decode_to_bytes_atom(std, layouter, atom, inputs, res)?;
                acc = std.add_constant(layouter, &acc, 1.into())?;
            }
            AlignmentSegment::Option(_) => {
                return Err(Error::Synthesis(
                    "in-circuit decoding of alignment options is not yet implemented!".into(),
                ));
            }
        }
    }
    Ok(acc)
}

fn v1_fab_decode_to_bytes_atom(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    align: &AlignmentAtom,
    inputs: &mut &[AssignedNative<OldScalar>],
    res: &mut Vec<AssignedByte<OldScalar>>,
) -> Result<(), Error> {
    match align {
        AlignmentAtom::Field => {
            if inputs.is_empty() {
                return Err(Error::Synthesis(
                    "Cannot decode field element from nothing".into(),
                ));
            }
            let value = &inputs[0];
            *inputs = &inputs[1..];
            res.extend(std.assigned_to_le_bytes(layouter, value, None)?);
            Ok(())
        }
        AlignmentAtom::Bytes { length } => {
            let stray = *length as usize % FR_BYTES_STORED;
            let chunks = *length as usize / FR_BYTES_STORED;
            let expected_size = chunks + (stray != 0) as usize;
            let mut bytes_from =
                |slice: &mut Vec<AssignedByte<OldScalar>>, k, f: AssignedNative<OldScalar>| {
                    let repr = std.assigned_to_le_bytes(layouter, &f, Some(k))?;
                    slice.extend(repr[..k].iter().cloned());
                    Ok::<_, Error>(())
                };
            if inputs.len() < expected_size {
                return Err(Error::Synthesis(
                    "Cannot decode byte value; not enough data provided".into(),
                ));
            }
            let mut res_vec = Vec::with_bounded_capacity(*length as usize - stray);
            if stray > 0 {
                bytes_from(&mut res_vec, stray, inputs[0].clone())?;
                *inputs = &inputs[1..];
            }
            for i in 0..chunks {
                bytes_from(res, FR_BYTES_STORED, inputs[chunks - 1 - i].clone())?;
            }
            *inputs = &inputs[chunks..];
            res.extend(res_vec);
            Ok(())
        }
        AlignmentAtom::Compress => Err(Error::Synthesis(
            "Cannot decode compressed value from field elements".into(),
        )),
    }
}

fn v1_assemble_bytes(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    bytes: &[AssignedByte<OldScalar>],
) -> Result<AssignedNative<OldScalar>, Error> {
    const BITS: usize = 8;
    let mut powers = Vec::with_bounded_capacity(bytes.len());
    powers.push(std.convert(layouter, &bytes[0])?);
    for (i, byte) in bytes.iter().enumerate().skip(1) {
        let power = (0..i * BITS)
            .fold(transient_crypto_old::curve::Fr::from(1), |acc, _| {
                acc * transient_crypto_old::curve::Fr::from(2)
            })
            .0;
        let byte = std.convert(layouter, byte)?;
        powers.push(std.mul_by_constant(layouter, &byte, power)?);
    }
    let mut acc = powers[0].clone();
    for limb in powers[1..].iter() {
        acc = std.add(layouter, &acc, limb)?;
    }
    Ok(acc)
}

fn v1_ecc_from_parts(
    std: &ZkStdLib,
    layouter: &mut impl Layouter<OldScalar>,
    x: &AssignedNative<OldScalar>,
    y: &AssignedNative<OldScalar>,
) -> Result<AssignedNativePoint<OldEmbeddedAffineExt>, Error> {
    let point = x.value().zip(y.value()).map(|(x, y)| {
        transient_crypto_old::curve::EmbeddedGroupAffine::new(
            transient_crypto_old::curve::Fr(*x),
            transient_crypto_old::curve::Fr(*y),
        )
    });
    point.as_ref().error_if_known_and(|p| p.is_none())?;
    let point = point.map(|p| p.expect("After is_none check, point should exist").0);
    let point_var: AssignedNativePoint<OldEmbeddedAffineExt> =
        std.jubjub().assign(layouter, point)?;

    std.assert_equal(layouter, x, &std.jubjub().x_coordinate(&point_var))?;
    std.assert_equal(layouter, y, &std.jubjub().y_coordinate(&point_var))?;
    Ok(point_var)
}

// --- v1 Relation impl ---

impl Relation for IrSource {
    type Instance = Vec<OldScalar>;

    type Witness = V1Preprocessed;

    fn format_instance(
        instance: &Self::Instance,
    ) -> Result<Vec<OldScalar>, midnight_proofs_v1::plonk::Error> {
        Ok(instance.clone())
    }

    fn circuit(
        &self,
        std: &ZkStdLib,
        layouter: &mut impl Layouter<OldScalar>,
        _instance: Value<Self::Instance>,
        witness: Value<Self::Witness>,
    ) -> Result<(), Error> {
        let input_values = witness
            .as_ref()
            .map(|preproc| preproc.memory[..self.num_inputs as usize].to_vec());
        let binding_input_value = witness.as_ref().map(|preproc| preproc.binding_input);
        let comm_comm_value = witness.as_ref().map(|preproc| preproc.comm_comm);

        let mut memory = std.assign_many(
            layouter,
            &input_values.transpose_vec(self.num_inputs as usize),
        )?;

        let inputs = memory.clone();
        let binding_input = std.assign(layouter, binding_input_value)?;

        let mut outputs = Vec::new();

        fn idx(
            memory: &[AssignedNative<OldScalar>],
            i: u32,
        ) -> Result<&AssignedNative<OldScalar>, Error> {
            memory
                .get(i as usize)
                .ok_or(Error::Synthesis(format!("missing index {i}")))
        }
        let seq_push = |cell: AssignedNative<OldScalar>,
                        mem: &mut Vec<AssignedNative<OldScalar>>,
                        seq: for<'a> fn(&'a V1Preprocessed) -> &'a [OldScalar]|
         -> Result<(), Error> {
            let idx = mem.len();

            witness
                .as_ref()
                .zip(cell.value())
                .error_if_known_and(|(preproc, v)| {
                    if idx < seq(preproc).len() && seq(preproc)[idx] != **v {
                        error!(
                            ?idx,
                            "Misalignment between `prepare` and `synthesize` runs. This is a bug."
                        );
                        true
                    } else {
                        false
                    }
                })?;

            mem.push(cell);
            Ok(())
        };

        let mem_push =
            |cell: AssignedNative<OldScalar>,
             mem: &mut Vec<AssignedNative<OldScalar>>|
             -> Result<(), Error> { seq_push(cell, mem, |preproc| &preproc.memory) };

        let pi_push = |cell: AssignedNative<OldScalar>,
                       pis: &mut Vec<AssignedNative<OldScalar>>|
         -> Result<(), Error> { seq_push(cell, pis, |preproc| &preproc.pis) };

        let mut public_inputs = vec![];
        pi_push(binding_input, &mut public_inputs)?;

        if self.do_communications_commitment {
            let comm_comm_value = comm_comm_value.map(|c| {
                c.ok_or_else(|| {
                    Error::Synthesis("Communication commitment not present despite preproc.".into())
                })
                .unwrap()
                .0
            });
            let comm_comm = std.assign(layouter, comm_comm_value)?;
            pi_push(comm_comm, &mut public_inputs)?;
        }
        for ins in self.instructions.iter() {
            match ins {
                I::Assert { cond } => std.assert_non_zero(layouter, idx(&memory, *cond)?)?,
                I::CondSelect { bit, a, b } => {
                    let bit = std.is_zero(layouter, idx(&memory, *bit)?)?;
                    let result =
                        std.select(layouter, &bit, idx(&memory, *b)?, idx(&memory, *a)?)?;
                    mem_push(result, &mut memory)?;
                }
                I::ConstrainBits { var, bits } => drop(std.assigned_to_le_bits(
                    layouter,
                    idx(&memory, *var)?,
                    Some(*bits as usize),
                    *bits as usize >= FR_BITS,
                )?),
                I::ConstrainEq { a, b } => {
                    std.assert_equal(layouter, idx(&memory, *a)?, idx(&memory, *b)?)?
                }
                I::ConstrainToBoolean { var } => {
                    let _: AssignedBit<_> = std.convert(layouter, idx(&memory, *var)?)?;
                }
                I::Copy { var } => mem_push(idx(&memory, *var)?.clone(), &mut memory)?,
                I::DeclarePubInput { var } => {
                    pi_push(idx(&memory, *var)?.clone(), &mut public_inputs)?
                }
                I::PiSkip { .. } => {}
                I::LoadImm { imm } => {
                    mem_push(std.assign_fixed(layouter, cvt(imm.0))?, &mut memory)?
                }
                I::Output { var } => outputs.push(idx(&memory, *var)?.clone()),
                I::TransientHash { inputs } => mem_push(
                    std.poseidon(
                        layouter,
                        &inputs
                            .iter()
                            .map(|inp| idx(&memory, *inp).cloned())
                            .collect::<Result<Vec<_>, _>>()?,
                    )?,
                    &mut memory,
                )?,
                I::PersistentHash { alignment, inputs } => {
                    let inputs = inputs
                        .iter()
                        .map(|i| idx(&memory, *i).cloned())
                        .collect::<Result<Vec<_>, _>>()?;
                    let bytes = v1_fab_decode_to_bytes(std, layouter, alignment, &inputs)?;
                    let res_bytes = std.sha2_256(layouter, &bytes)?;
                    mem_push(std.convert(layouter, &res_bytes[31])?, &mut memory)?;
                    mem_push(
                        v1_assemble_bytes(std, layouter, &res_bytes[..31])?,
                        &mut memory,
                    )?;
                }
                I::TestEq { a, b } => {
                    let bit = std.is_equal(layouter, idx(&memory, *a)?, idx(&memory, *b)?)?;
                    mem_push(std.convert(layouter, &bit)?, &mut memory)?;
                }
                I::Add { a, b } => mem_push(
                    std.add(layouter, idx(&memory, *a)?, idx(&memory, *b)?)?,
                    &mut memory,
                )?,
                I::Mul { a, b } => mem_push(
                    std.mul(layouter, idx(&memory, *a)?, idx(&memory, *b)?, None)?,
                    &mut memory,
                )?,
                I::Neg { a } => mem_push(std.neg(layouter, idx(&memory, *a)?)?, &mut memory)?,
                I::Not { a } => mem_push(v1_lnot(std, layouter, idx(&memory, *a)?)?, &mut memory)?,
                I::LessThan { a, b, bits } => {
                    let bit = std.lower_than(
                        layouter,
                        idx(&memory, *a)?,
                        idx(&memory, *b)?,
                        u32::max(*bits + *bits % 2, 4),
                    )?;
                    mem_push(std.convert(layouter, &bit)?, &mut memory)?;
                }
                I::PublicInput { guard } | I::PrivateInput { guard } => {
                    let guard = guard.map(|guard| idx(&memory, guard)).transpose()?;
                    witness.error_if_known_and(|preproc| memory.len() > preproc.memory.len())?;
                    let value = witness.as_ref().map(|preproc| preproc.memory[memory.len()]);
                    let value_cell = std.assign(layouter, value)?;
                    if let Some(guard) = guard {
                        let value_is_zero = std.is_zero(layouter, &value_cell)?;
                        let guard_bit = std.convert(layouter, guard)?;
                        let is_ok = std.or(layouter, &[value_is_zero, guard_bit])?;
                        let is_ok_field = std.convert(layouter, &is_ok)?;
                        std.assert_non_zero(layouter, &is_ok_field)?;
                    }
                    mem_push(value_cell, &mut memory)?;
                }
                I::DivModPowerOfTwo { var, bits } => {
                    let var = idx(&memory, *var)?;
                    let var_bits = std.assigned_to_le_bits(layouter, var, None, true)?;
                    let modulus =
                        std.assigned_from_le_bits(layouter, &var_bits[..*bits as usize])?;
                    let divisor =
                        std.assigned_from_le_bits(layouter, &var_bits[*bits as usize..])?;
                    mem_push(divisor, &mut memory)?;
                    mem_push(modulus, &mut memory)?;
                }
                I::ReconstituteField {
                    divisor,
                    modulus,
                    bits,
                } => {
                    let divisor_bits = std.assigned_to_le_bits(
                        layouter,
                        idx(&memory, *divisor)?,
                        Some(FR_BITS - *bits as usize),
                        true,
                    )?;
                    let modulus_bits = std.assigned_to_le_bits(
                        layouter,
                        idx(&memory, *modulus)?,
                        Some(*bits as usize),
                        true,
                    )?;
                    let reconstituted = std
                        .assigned_from_le_bits(layouter, &[modulus_bits, divisor_bits].concat())?;
                    mem_push(reconstituted, &mut memory)?;
                }
                I::EcAdd { a_x, a_y, b_x, b_y } => {
                    let a =
                        v1_ecc_from_parts(std, layouter, idx(&memory, *a_x)?, idx(&memory, *a_y)?)?;
                    let b =
                        v1_ecc_from_parts(std, layouter, idx(&memory, *b_x)?, idx(&memory, *b_y)?)?;
                    let c = std.jubjub().add(layouter, &a, &b)?;
                    mem_push(std.jubjub().x_coordinate(&c), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&c), &mut memory)?;
                }
                I::EcMul { a_x, a_y, scalar } => {
                    let a =
                        v1_ecc_from_parts(std, layouter, idx(&memory, *a_x)?, idx(&memory, *a_y)?)?;
                    let scalar = std.jubjub().convert(layouter, idx(&memory, *scalar)?)?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[a])?;
                    mem_push(std.jubjub().x_coordinate(&b), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&b), &mut memory)?;
                }
                I::EcMulGenerator { scalar } => {
                    let g: AssignedNativePoint<OldEmbeddedAffineExt> = std
                        .jubjub()
                        .assign_fixed(layouter, OldEmbeddedAffine::generator())?;
                    let scalar = std.jubjub().convert(layouter, idx(&memory, *scalar)?)?;
                    let b = std.jubjub().msm(layouter, &[scalar], &[g])?;
                    mem_push(std.jubjub().x_coordinate(&b), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&b), &mut memory)?;
                }
                I::HashToCurve { inputs } => {
                    let inputs = inputs
                        .iter()
                        .map(|input| idx(&memory, *input).cloned())
                        .collect::<Result<Vec<_>, _>>()?;
                    let point = std.hash_to_curve(layouter, &inputs)?;
                    mem_push(std.jubjub().x_coordinate(&point), &mut memory)?;
                    mem_push(std.jubjub().y_coordinate(&point), &mut memory)?;
                }
            }
        }
        if self.do_communications_commitment {
            let comm_comm_rand_value = comm_comm_value.map(|c| {
                c.ok_or_else(|| {
                    Error::Synthesis("Communication commitment not present despite preproc.".into())
                })
                .unwrap()
                .1
            });
            let comm_comm_rand = std.assign(layouter, comm_comm_rand_value)?;

            let mut preimage = vec![comm_comm_rand];
            preimage.extend(inputs.iter().cloned());
            preimage.extend(outputs.iter().cloned());
            let comm_comm = std.poseidon(layouter, &preimage)?;
            std.assert_equal(layouter, &comm_comm, &public_inputs[1])?;
        }

        public_inputs
            .iter()
            .try_for_each(|x| std.constrain_as_public_input(layouter, x))
    }

    fn used_chips(&self) -> ZkStdLibArch {
        let jubjub = self.instructions.iter().any(|op| {
            matches!(
                op,
                I::EcAdd { .. }
                    | I::EcMul { .. }
                    | I::EcMulGenerator { .. }
                    | I::HashToCurve { .. }
            )
        });
        let hash_to_curve = self
            .instructions
            .iter()
            .any(|op| matches!(op, I::HashToCurve { .. }));
        let poseidon = self.do_communications_commitment
            || self
                .instructions
                .iter()
                .any(|op| matches!(op, I::TransientHash { .. }));
        let sha2_256 = self
            .instructions
            .iter()
            .any(|op| matches!(op, I::PersistentHash { .. }));
        ZkStdLibArch {
            jubjub: jubjub || hash_to_curve,
            poseidon: poseidon || hash_to_curve,
            sha2_256,
            sha2_512: false,
            sha3_256: false,
            keccak_256: false,
            blake2b: false,
            nr_pow2range_cols: 1,
            secp256k1: false,
            bls12_381: false,
            base64: false,
            automaton: false,
        }
    }

    fn write_relation<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Serialize without the version field to match the old zkir_old::IrSource
        // format that is embedded in existing prover keys.
        use super::ir::OldIrSource;
        let old = OldIrSource {
            num_inputs: self.num_inputs,
            do_communications_commitment: self.do_communications_commitment,
            instructions: self.instructions.clone(),
        };
        serialize::Serializable::serialize(&old, writer)
    }

    fn read_relation<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        use super::ir::OldIrSource;
        let old: OldIrSource = serialize::Deserializable::deserialize(reader, 0)?;
        Ok(old.into())
    }
}

type OldProvingError = transient_crypto_old::proofs::ProvingError;

/// Converts an old `ProofPreimage` to the current type for use with `IrSource::preprocess`.
fn preimage_from_v1(
    p: &transient_crypto_old::proofs::ProofPreimage,
) -> transient_crypto::proofs::ProofPreimage {
    let cvt_fr = |f: transient_crypto_old::curve::Fr| -> transient_crypto::curve::Fr {
        transient_crypto::curve::Fr::from_le_bytes(&f.as_le_bytes()).expect("Fr round-trip")
    };
    transient_crypto::proofs::ProofPreimage {
        inputs: p.inputs.iter().copied().map(cvt_fr).collect(),
        private_transcript: p.private_transcript.iter().copied().map(cvt_fr).collect(),
        public_transcript_inputs: p
            .public_transcript_inputs
            .iter()
            .copied()
            .map(cvt_fr)
            .collect(),
        public_transcript_outputs: p
            .public_transcript_outputs
            .iter()
            .copied()
            .map(cvt_fr)
            .collect(),
        binding_input: cvt_fr(p.binding_input),
        communications_commitment: p
            .communications_commitment
            .map(|(a, b)| (cvt_fr(a), cvt_fr(b))),
        key_location: transient_crypto::proofs::KeyLocation(std::borrow::Cow::Owned(
            p.key_location.0.to_string(),
        )),
    }
}

// --- Old Zkir impl (v1 pipeline) ---

#[derive(Serializable)]
#[tag = "prover-key[v7](ir-source[v2])"]
struct FacadeProverKey(Vec<u8>);

impl transient_crypto_old::proofs::Zkir for IrSource {
    fn check(
        &self,
        preimage: &transient_crypto_old::proofs::ProofPreimage,
    ) -> Result<Vec<Option<usize>>, OldProvingError> {
        let current = preimage_from_v1(preimage);
        Ok(self.preprocess(&current)?.pi_skips)
    }

    async fn prove(
        &self,
        rng: impl rand::Rng + rand::CryptoRng,
        params: &impl transient_crypto_old::proofs::ParamsProverProvider,
        pk: transient_crypto_old::proofs::ProverKey<Self>,
        preimage: &transient_crypto_old::proofs::ProofPreimage,
    ) -> Result<
        (
            transient_crypto_old::proofs::Proof,
            Vec<transient_crypto_old::curve::Fr>,
            Vec<Option<usize>>,
        ),
        OldProvingError,
    > {
        let current = preimage_from_v1(preimage);
        let preproc = self.preprocess(&current)?;
        let pi_skips = preproc.pi_skips.clone();
        let v1_preproc = V1Preprocessed::from_current(&preproc);
        let v1_pis: Vec<OldScalar> = v1_preproc.pis.clone();

        let pk = pk
            .init()
            .map_err(|_| anyhow::anyhow!("Could not init pk"))?;

        let params_k = params.get_params(pk.k()).await?;
        let proof =
            midnight_zk_stdlib_v1::prove::<_, transient_crypto_old::proofs::TranscriptHash>(
                params_k.as_ref(),
                &pk,
                self,
                &v1_pis,
                v1_preproc,
                rng,
            )
            .map_err(|e| anyhow::anyhow!("v1 prove: {e}"))?;

        let old_pis = v1_pis
            .into_iter()
            .map(transient_crypto_old::curve::Fr)
            .collect();

        Ok((
            transient_crypto_old::proofs::Proof(proof),
            old_pis,
            pi_skips,
        ))
    }

    fn load_ir_from_tagged(reader: impl Read + Seek) -> io::Result<Self> {
        Self::load_from_tagged(reader)
    }

    fn load_prover_key_from_tagged(
        mut reader: impl Read + Seek,
    ) -> io::Result<transient_crypto_old::proofs::ProverKey<Self>> {
        use transient_crypto_old::proofs::ProverKey;
        let tag = peek_tag(&mut reader)?;
        let expected_tag_new = <ProverKey<IrSource>>::tag();
        let expected_tag_old = FacadeProverKey::tag();
        if tag == expected_tag_new {
            serialize::tagged_deserialize(&mut reader)
        } else if tag == expected_tag_old {
            let FacadeProverKey(data) = serialize::tagged_deserialize::<FacadeProverKey>(reader)?;
            let mut header = Vec::new();
            Serializable::serialize(&(data.len() as u32), &mut header)?;
            let mut header_cursor = &header[..];
            let mut data_cursor = &data[..];
            let mut reader = Read::chain(&mut header_cursor, &mut data_cursor);
            Deserializable::deserialize(&mut reader, 0)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "expected one of '{expected_tag_new}' or '{expected_tag_old}', got '{tag}'."
                ),
            ))
        }
    }
}

// --- Adapters for bridging current types to the v1 pipeline ---

/// Converts a current `ProofPreimage` into a v1 `ProofPreimage`.
pub fn preimage_to_v1(
    p: &transient_crypto::proofs::ProofPreimage,
) -> transient_crypto_old::proofs::ProofPreimage {
    let cvt_fr = |f: transient_crypto::curve::Fr| -> transient_crypto_old::curve::Fr {
        transient_crypto_old::curve::Fr(cvt(f.0))
    };
    transient_crypto_old::proofs::ProofPreimage {
        inputs: p.inputs.iter().copied().map(cvt_fr).collect(),
        private_transcript: p.private_transcript.iter().copied().map(cvt_fr).collect(),
        public_transcript_inputs: p
            .public_transcript_inputs
            .iter()
            .copied()
            .map(cvt_fr)
            .collect(),
        public_transcript_outputs: p
            .public_transcript_outputs
            .iter()
            .copied()
            .map(cvt_fr)
            .collect(),
        binding_input: cvt_fr(p.binding_input),
        communications_commitment: p
            .communications_commitment
            .map(|(a, b)| (cvt_fr(a), cvt_fr(b))),
        key_location: transient_crypto_old::proofs::KeyLocation(std::borrow::Cow::Owned(
            p.key_location.0.to_string(),
        )),
    }
}

/// Adapter: current `ParamsProverProvider` → v1 `ParamsProverProvider`.
pub struct V1Params<'a, P: transient_crypto::proofs::ParamsProverProvider>(pub &'a P);

impl<P: transient_crypto::proofs::ParamsProverProvider>
    transient_crypto_old::proofs::ParamsProverProvider for V1Params<'_, P>
{
    async fn get_params(
        &self,
        k: u8,
    ) -> std::io::Result<transient_crypto_old::proofs::ParamsProver> {
        let current = self.0.get_params(k).await?;
        let mut buf = Vec::new();
        midnight_proofs::poly::kzg::params::ParamsKZG::write_custom(
            current.as_ref(),
            &mut buf,
            midnight_proofs::utils::SerdeFormat::RawBytesUnchecked,
        )?;
        transient_crypto_old::proofs::ParamsProver::read(&buf[..])
    }
}
