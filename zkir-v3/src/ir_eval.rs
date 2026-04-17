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

//! Shared off-circuit evaluation helpers for ZKIR instructions.
//!
//! These standalone functions evaluate instructions against an `IrValue` memory
//! map. They are used by both [`crate::ir_preprocess`] (proving) and
//! [`crate::ir_execute`] (rehearsal) to avoid duplicating the computational
//! instruction variants.
//!
//! All functions return `anyhow::Result`. The execute path converts via
//! `From<anyhow::Error> for ExecutionError`.

use std::collections::HashMap;

use anyhow::{anyhow, bail};
use transient_crypto::curve::{FR_BITS, FR_BYTES_STORED, Fr};
use transient_crypto::hash::{hash_to_curve, transient_hash};

use crate::ir::{Identifier, Instruction as I, Operand};
use crate::ir_instructions::add::add_offcircuit;
use crate::ir_instructions::decode::decode_offcircuit;
use crate::ir_instructions::encode::encode_offcircuit;
use crate::ir_types::IrValue;

use base_crypto::hash::persistent_hash;
use base_crypto::repr::BinaryHashRepr;
use group::Group;
use midnight_curves::{Fr as JubjubFr, JubjubSubgroup};
use transient_crypto::curve::outer;
use transient_crypto::fab::{AlignmentExt, ValueReprAlignedValue};
use transient_crypto::repr::FieldRepr;

/// Converts a BLS12-381 scalar field element into a Jubjub scalar field element.
// TODO: Remove this function when IrType supports JubjubScalar.
pub(crate) fn jubjub_scalar_from_native(native: outer::Scalar) -> Result<JubjubFr, anyhow::Error> {
    let s: Option<JubjubFr> = JubjubFr::from_bytes(&native.to_bytes_le()).into();
    s.ok_or(anyhow::Error::msg("Error converting Fr to JubjubScalar"))
}

pub(crate) fn eval_idx(
    memory: &HashMap<Identifier, IrValue>,
    id: &Identifier,
) -> anyhow::Result<IrValue> {
    let res = memory
        .get(id)
        .cloned()
        .ok_or_else(|| anyhow!("variable not found: {:?}", id));
    trace!(?res, "retrieved from {:?}", id);
    res
}

pub(crate) fn eval_operand(
    memory: &HashMap<Identifier, IrValue>,
    operand: &Operand,
) -> anyhow::Result<IrValue> {
    match operand {
        Operand::Variable(id) => eval_idx(memory, id),
        Operand::Immediate(imm) => Ok(IrValue::Native(*imm)),
    }
}

pub(crate) fn eval_operand_bool(
    memory: &HashMap<Identifier, IrValue>,
    operand: &Operand,
) -> anyhow::Result<bool> {
    eval_operand(memory, operand).and_then(|val| {
        let val: Fr = val.try_into()?;
        if val == 0.into() {
            Ok(false)
        } else if val == 1.into() {
            Ok(true)
        } else {
            bail!("Expected boolean, found: {val:?}");
        }
    })
}

pub(crate) fn eval_operand_fr(
    memory: &HashMap<Identifier, IrValue>,
    operand: &Operand,
) -> anyhow::Result<Fr> {
    eval_operand(memory, operand)?
        .try_into()
        .map_err(|e| anyhow!("expected native Fr: {e}"))
}

pub(crate) fn eval_operand_bits(
    memory: &HashMap<Identifier, IrValue>,
    operand: &Operand,
    max_bits: Option<u32>,
) -> anyhow::Result<Vec<bool>> {
    eval_operand(memory, operand).and_then(|val| {
        let val: Fr = val.try_into()?;
        let mut bits = val
            .0
            .to_bytes_le()
            .into_iter()
            .flat_map(|byte| {
                [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80]
                    .into_iter()
                    .map(move |mask| byte & mask != 0)
            })
            .collect::<Vec<_>>();
        if let Some(n) = max_bits {
            if n as usize >= FR_BITS {
                bail!("Excessive bit bound");
            }
            if bits[n as usize..].iter().any(|b| *b) {
                bail!("Bit bound failed: {val:?} is not {n}-bit");
            }
            bits.truncate(n as usize);
        }
        Ok(bits)
    })
}

pub(crate) fn eval_from_bits<I: DoubleEndedIterator<Item = bool>>(bits: I) -> Fr {
    bits.rev()
        .fold(0.into(), |acc, bit| acc * 2.into() + bit.into())
}

/// Evaluate a single computational instruction off-circuit.
/// Returns `Ok(Some(()))` if the instruction was handled, `Ok(None)` if it is a
/// mode-specific instruction (Impact, Output, PublicInput, PrivateInput, ContractCall).
pub(crate) fn eval_computational_instruction(
    ins: &I,
    memory: &mut HashMap<Identifier, IrValue>,
) -> anyhow::Result<Option<()>> {
    match ins {
        I::Encode { input, outputs } => {
            let val = eval_operand(memory, input)?;
            let encoded = encode_offcircuit(&val);
            if encoded.len() != outputs.len() {
                bail!("Unexpected output length of encode instruction");
            }
            for (out_id, enc_val) in outputs.iter().zip(encoded.into_iter()) {
                memory.insert(out_id.clone(), enc_val);
            }
            Ok(Some(()))
        }
        I::Decode {
            inputs,
            val_t,
            output,
        } => {
            let raw_inputs = inputs
                .iter()
                .map(|inp| eval_operand(memory, inp)?.try_into())
                .collect::<Result<Vec<Fr>, _>>()?;
            let decoded = decode_offcircuit(&raw_inputs, val_t)?;
            memory.insert(output.clone(), decoded);
            Ok(Some(()))
        }
        I::Add { a, b, output } => {
            let a = eval_operand(memory, a)?;
            let b = eval_operand(memory, b)?;
            let result = add_offcircuit(&a, &b)?;
            memory.insert(output.clone(), result);
            Ok(Some(()))
        }
        I::Mul { a, b, output } => {
            let a: Fr = eval_operand(memory, a)?.try_into()?;
            let b: Fr = eval_operand(memory, b)?.try_into()?;
            let result = IrValue::Native(a * b);
            memory.insert(output.clone(), result);
            Ok(Some(()))
        }
        I::Neg { a, output } => {
            let a: Fr = eval_operand(memory, a)?.try_into()?;
            let result = IrValue::Native(-a);
            memory.insert(output.clone(), result);
            Ok(Some(()))
        }
        I::Not { a, output } => {
            let result = IrValue::Native((!eval_operand_bool(memory, a)?).into());
            memory.insert(output.clone(), result);
            Ok(Some(()))
        }
        I::ConstrainEq { a, b } => {
            let va = eval_operand(memory, a)?;
            let vb = eval_operand(memory, b)?;
            if va != vb {
                bail!("Failed equality constraint: {va:?} != {vb:?}");
            }
            Ok(Some(()))
        }
        I::CondSelect { bit, a, b, output } => {
            let (bit_val, a_val, b_val) = (
                eval_operand_bool(memory, bit)?,
                eval_operand(memory, a)?,
                eval_operand(memory, b)?,
            );
            memory.insert(output.clone(), if bit_val { a_val } else { b_val });
            Ok(Some(()))
        }
        I::Assert { cond } => {
            if !eval_operand_bool(memory, cond)? {
                bail!("Failed direct assertion");
            }
            Ok(Some(()))
        }
        I::TestEq { a, b, output } => {
            let result =
                IrValue::Native((eval_operand(memory, a)? == eval_operand(memory, b)?).into());
            memory.insert(output.clone(), result);
            Ok(Some(()))
        }
        I::Copy { val, output } => {
            let val = eval_operand(memory, val)?;
            memory.insert(output.clone(), val);
            Ok(Some(()))
        }
        I::ConstrainToBoolean { val } => {
            let _ = eval_operand_bool(memory, val)?;
            Ok(Some(()))
        }
        I::ConstrainBits { val, bits } => {
            eval_operand_bits(memory, val, Some(*bits))?;
            Ok(Some(()))
        }
        I::DivModPowerOfTwo { val, bits, outputs } => {
            if outputs.len() != 2 {
                bail!("DivModPowerOfTwo requires exactly 2 outputs");
            }
            if *bits as usize > FR_BYTES_STORED * 8 {
                bail!("Excessive bit count");
            }
            let val_bits = eval_operand_bits(memory, val, None)?;
            memory.insert(
                outputs[0].clone(),
                IrValue::Native(eval_from_bits(val_bits[*bits as usize..].iter().copied())),
            );
            memory.insert(
                outputs[1].clone(),
                IrValue::Native(eval_from_bits(val_bits[..*bits as usize].iter().copied())),
            );
            Ok(Some(()))
        }
        I::ReconstituteField {
            divisor,
            modulus,
            bits,
            output,
        } => {
            if *bits as usize > FR_BYTES_STORED * 8 {
                bail!("Excessive bit count");
            }
            let fr_max = Fr::from(-1);
            let max_bits: Vec<bool> = fr_max
                .0
                .to_bytes_le()
                .into_iter()
                .flat_map(|byte| {
                    [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80]
                        .into_iter()
                        .map(move |mask| byte & mask != 0)
                })
                .collect();
            let modulus_bits = eval_operand_bits(memory, modulus, Some(*bits))?;
            let divisor_bits = eval_operand_bits(memory, divisor, Some(FR_BITS as u32 - *bits))?;
            let cmp = modulus_bits
                .iter()
                .chain(divisor_bits.iter())
                .rev()
                .zip(max_bits[..FR_BITS].iter().rev())
                .map(|(ab, max)| ab.cmp(max))
                .fold(std::cmp::Ordering::Equal, |prefix, local| {
                    if prefix.is_eq() { local } else { prefix }
                });
            if cmp.is_gt() {
                bail!("Reconstituted element overflows field");
            }
            let power = (0..*bits).fold(Fr::from(1), |acc, _| Fr::from(2) * acc);
            let modulus_val: Fr = eval_operand(memory, modulus)?.try_into()?;
            let divisor_val: Fr = eval_operand(memory, divisor)?.try_into()?;
            let result = IrValue::Native(power * divisor_val + modulus_val);
            memory.insert(output.clone(), result);
            Ok(Some(()))
        }
        I::LessThan { a, b, bits, output } => {
            let result = (eval_from_bits(eval_operand_bits(memory, a, Some(*bits))?.into_iter())
                < eval_from_bits(eval_operand_bits(memory, b, Some(*bits))?.into_iter()))
            .into();
            memory.insert(output.clone(), IrValue::Native(result));
            Ok(Some(()))
        }
        I::TransientHash { inputs, output } => {
            let result = transient_hash(
                &inputs
                    .iter()
                    .map(|i| eval_operand(memory, i))
                    .map(|r| r.and_then(|v| v.try_into()))
                    .collect::<Result<Vec<Fr>, _>>()?,
            );
            memory.insert(output.clone(), IrValue::Native(result));
            Ok(Some(()))
        }
        I::PersistentHash {
            alignment,
            inputs,
            outputs,
        } => {
            if outputs.len() != 2 {
                bail!("PersistentHash requires exactly 2 outputs");
            }
            let inputs_fr = inputs
                .iter()
                .map(|i| eval_operand(memory, i))
                .map(|r| r.and_then(|v| v.try_into()))
                .collect::<Result<Vec<_>, _>>()?;
            let value = alignment.parse_field_repr(&inputs_fr).ok_or_else(|| {
                anyhow!("Inputs did not match alignment (inputs: {inputs_fr:?}, alignment: {alignment:?})")
            })?;
            let mut repr = Vec::new();
            ValueReprAlignedValue(value).binary_repr(&mut repr);
            trace!(bytes = ?repr, "bytes decoded out-of-circuit");
            let hash = persistent_hash(&repr);
            let hash_fields = hash.field_vec();
            if hash_fields.len() >= 2 {
                memory.insert(outputs[0].clone(), IrValue::Native(hash_fields[0]));
                memory.insert(outputs[1].clone(), IrValue::Native(hash_fields[1]));
            } else {
                bail!("PersistentHash did not produce expected output");
            }
            Ok(Some(()))
        }
        I::HashToCurve { inputs, output } => {
            let inputs_fr = inputs
                .iter()
                .map(|var| eval_operand(memory, var))
                .map(|r| r.and_then(|v| v.try_into()))
                .collect::<Result<Vec<Fr>, _>>()?;
            let point = hash_to_curve(&inputs_fr);
            memory.insert(output.clone(), IrValue::JubjubPoint(point.0));
            Ok(Some(()))
        }
        I::EcMul { a, scalar, output } => {
            let a_val: JubjubSubgroup = eval_operand(memory, a)?.try_into()?;
            let s: Fr = eval_operand_fr(memory, scalar)?;
            let c = IrValue::JubjubPoint(a_val * jubjub_scalar_from_native(s.0)?);
            memory.insert(output.clone(), c);
            Ok(Some(()))
        }
        I::EcMulGenerator { scalar, output } => {
            let s: Fr = eval_operand_fr(memory, scalar)?;
            let p = JubjubSubgroup::generator() * jubjub_scalar_from_native(s.0)?;
            memory.insert(output.clone(), IrValue::JubjubPoint(p));
            Ok(Some(()))
        }
        // Mode-specific instructions — not handled here
        _ => Ok(None),
    }
}
