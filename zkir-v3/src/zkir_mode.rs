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

//! ZKIR-mode types for `Op`.
//!
//! These types parameterize `Op`'s value and key-path positions with symbolic
//! ZKIR operand references instead of materialized runtime values. This allows
//! the same `Op` enum to represent ImpactVM operations both in the on-chain
//! runtime (with `StateValue<D>` and `Array<Key, D>`) and in ZKIR (with
//! `ZkirPushValue` and `Vec<ZkirKey>`).

use std::collections::HashMap;

use anyhow::{anyhow, bail};
use base_crypto::fab::Alignment;
use onchain_vm::ops::Op;
use onchain_vm::result_mode::ResultModeGather;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged};
use storage::DefaultDB;
use transient_crypto::curve::Fr;
use transient_crypto::repr::FieldRepr;

use crate::ir::{Identifier, Operand};
use crate::ir_types::IrValue;

/// A Push value in ZKIR: carries alignment metadata and symbolic operand
/// references that will be resolved to field elements at execution time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Serializable)]
#[tag = "zkir-push-value[v1]"]
pub struct ZkirPushValue {
    /// Whether this is a storage push.
    pub storage: bool,
    /// The FAB alignment describing the layout of the value.
    pub alignment: Alignment,
    /// Symbolic operand references resolved from ZKIR memory during execution.
    pub operands: Vec<Operand>,
}

/// A key in a ZKIR Idx operation: either the stack sentinel or a symbolic value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Serializable)]
#[serde(tag = "key_type", content = "value")]
#[tag = "zkir-key[v1]"]
pub enum ZkirKey {
    /// The stack key — corresponds to `Key::Stack` at runtime.
    Stack,
    /// A value key — operands are resolved to field elements, then parsed via
    /// the alignment to produce an `AlignedValue` → `Key::Value(aligned)`.
    Value {
        alignment: Alignment,
        operands: Vec<Operand>,
    },
}

/// A ZKIR-mode `Op`: uses `ZkirPushValue` for Push and `Vec<ZkirKey>` for Idx,
/// with `ResultModeGather` (ReadResult = ()) since read results are unknown at
/// ZKIR definition time.
///
/// The `DefaultDB` parameter is unused — ZKIR ops carry no storage-layer types.
pub type ZkirOp = Op<ResultModeGather, DefaultDB, ZkirPushValue, Vec<ZkirKey>>;

/// Resolve a single operand to a field element.
fn resolve_operand_fr(
    memory: &HashMap<Identifier, IrValue>,
    operand: &Operand,
) -> anyhow::Result<Fr> {
    match operand {
        Operand::Variable(id) => {
            let val = memory
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow!("variable not found: {id:?}"))?;
            val.try_into()
                .map_err(|e| anyhow!("operand {id:?} is not a native field element: {e}"))
        }
        Operand::Immediate(imm) => Ok(*imm),
    }
}

/// Resolve a slice of operands to field elements.
fn resolve_operands(
    memory: &HashMap<Identifier, IrValue>,
    operands: &[Operand],
) -> anyhow::Result<Vec<Fr>> {
    operands
        .iter()
        .map(|op| resolve_operand_fr(memory, op))
        .collect()
}

/// Like [`zkir_ops_to_field_elements`] but also returns the per-op field
/// element counts. `per_op_sizes[i]` is the number of field elements that
/// `ops[i]` contributed to the output vector.
///
/// This is needed because `prove.rs` consumes one `pi_skips` entry per
/// transcript op, so the preprocessor must produce one entry per op.
///
/// # Encoding contract
///
/// The operands stored in [`ZkirPushValue`] and [`ZkirKey::Value`] encode the
/// **complete** field representation for their position (everything after the
/// variant opcode), so that no structural knowledge beyond the opcode is
/// needed here. This keeps the encoding function a direct mirror of the
/// `FieldRepr` match arms.
pub fn zkir_ops_to_field_elements_with_sizes(
    ops: Vec<ZkirOp>,
    read_results: &[Vec<Operand>],
    memory: &HashMap<Identifier, IrValue>,
) -> anyhow::Result<(Vec<Fr>, Vec<usize>)> {
    let (out, sizes) = zkir_ops_to_field_elements_inner(ops, read_results, memory)?;
    Ok((out, sizes))
}

/// Convert a sequence of structured ZKIR operations to the flat field-element
/// encoding consumed by the proving circuit as public inputs.
///
/// This produces the same byte-stream as `FieldRepr for Op<ResultModeVerify, D>`
/// but resolves symbolic ZKIR operand references from `memory` instead of
/// reading materialized runtime values.
///
/// `read_results` provides the operand references for each `Popeq`'s read
/// result, in Popeq-occurrence order within `ops`. Each inner `Vec<Operand>`
/// resolves to the field elements encoding one `AlignedValue` (alignment +
/// value), matching what `FieldRepr for AlignedValue` would produce.
pub fn zkir_ops_to_field_elements(
    ops: Vec<ZkirOp>,
    read_results: &[Vec<Operand>],
    memory: &HashMap<Identifier, IrValue>,
) -> anyhow::Result<Vec<Fr>> {
    let (out, _) = zkir_ops_to_field_elements_inner(ops, read_results, memory)?;
    Ok(out)
}

fn zkir_ops_to_field_elements_inner(
    ops: Vec<ZkirOp>,
    read_results: &[Vec<Operand>],
    memory: &HashMap<Identifier, IrValue>,
) -> anyhow::Result<(Vec<Fr>, Vec<usize>)> {
    let mut out: Vec<Fr> = Vec::new();
    let mut popeq_idx: usize = 0;
    let mut per_op_sizes: Vec<usize> = Vec::with_capacity(ops.len());

    for op in ops {
        let start = out.len();
        match op {
            // --- Fixed-opcode variants (no payload) ---
            Op::Noop { n } => {
                out.extend(std::iter::repeat_n(Fr::from(0u64), n as usize));
            }
            Op::Lt => out.push(0x01u64.into()),
            Op::Eq => out.push(0x02u64.into()),
            Op::Type => out.push(0x03u64.into()),
            Op::Size => out.push(0x04u64.into()),
            Op::New => out.push(0x05u64.into()),
            Op::And => out.push(0x06u64.into()),
            Op::Or => out.push(0x07u64.into()),
            Op::Neg => out.push(0x08u64.into()),
            Op::Log => out.push(0x09u64.into()),
            Op::Root => out.push(0x0au64.into()),
            Op::Pop => out.push(0x0bu64.into()),
            Op::Add => out.push(0x14u64.into()),
            Op::Sub => out.push(0x15u64.into()),
            Op::Member => out.push(0x18u64.into()),
            Op::Ckpt => out.push(0xffu64.into()),

            // --- Popeq: opcode + read-result field encoding from read_results ---
            Op::Popeq { cached, .. } => {
                out.push(((0x0cu8 + cached as u8) as u64).into());
                let rr = read_results.get(popeq_idx).ok_or_else(|| {
                    anyhow!(
                        "structured Impact has more Popeq ops than read_results entries \
                         (Popeq #{popeq_idx}, but only {} read_results)",
                        read_results.len()
                    )
                })?;
                out.extend(resolve_operands(memory, rr)?);
                popeq_idx += 1;
            }

            // --- Addi / Subi: opcode + immediate as single field element ---
            Op::Addi { immediate } => {
                out.push(0x0eu64.into());
                out.push((immediate as u64).into());
            }
            Op::Subi { immediate } => {
                out.push(0x0fu64.into());
                out.push((immediate as u64).into());
            }

            // --- Push: opcode + StateValue::Cell field repr ---
            // ZkirPushValue always resolves to StateValue::Cell(aligned).
            // Cell field repr = [1] + alignment.field_repr() + value operands.
            Op::Push { storage, value } => {
                out.push(((0x10u8 + storage as u8) as u64).into());
                out.push(Fr::from(1u64)); // StateValue::Cell tag
                value.alignment.field_repr(&mut out);
                out.extend(resolve_operands(memory, &value.operands)?);
            }

            // --- Branch / Jmp: opcode + skip ---
            Op::Branch { skip } => {
                out.push(0x12u64.into());
                out.push((skip as u64).into());
            }
            Op::Jmp { skip } => {
                out.push(0x13u64.into());
                out.push((skip as u64).into());
            }

            // --- Concat: opcode + n ---
            Op::Concat { cached, n } => {
                out.push(((0x16u8 + cached as u8) as u64).into());
                out.push((n as u64).into());
            }

            // --- Rem: single opcode ---
            Op::Rem { cached } => {
                out.push(((0x19u8 + cached as u8) as u64).into());
            }

            // --- Dup / Swap: opcode encodes n in lower nibble ---
            Op::Dup { n } => out.push(((0x30u8 | n) as u64).into()),
            Op::Swap { n } => out.push(((0x40u8 | n) as u64).into()),

            // --- Idx: opcode encodes cached/push_path/len, then key field reprs ---
            Op::Idx {
                cached,
                push_path,
                path,
            } => {
                if !path.is_empty() {
                    let base: u8 = match (cached, push_path) {
                        (false, false) => 0x50,
                        (true, false) => 0x60,
                        (false, true) => 0x70,
                        (true, true) => 0x80,
                    };
                    let opcode = base | (path.len() as u8 - 1);
                    out.push((opcode as u64).into());
                    for key in &path {
                        match key {
                            ZkirKey::Stack => {
                                // Key::Stack encodes as a single -1 field element.
                                out.push(-Fr::from(1u64));
                            }
                            ZkirKey::Value { alignment, operands } => {
                                // Encode alignment metadata first, then the value
                                // operands, matching Key::Value(av).field_repr() which
                                // emits alignment.field_repr() + value fields.
                                alignment.field_repr(&mut out);
                                out.extend(resolve_operands(memory, &operands)?);
                            }
                        }
                    }
                }
            }

            // --- Ins: opcode encodes cached + n ---
            Op::Ins { cached, n } => {
                let base: u8 = if cached { 0xa0 } else { 0x90 };
                out.push(((base | n) as u64).into());
            }

            // Op is #[non_exhaustive] in onchain-vm, so we must handle unknown variants.
            _ => bail!("unsupported Op variant in ZKIR field-element encoding"),
        }
        per_op_sizes.push(out.len() - start);
    }

    if popeq_idx != read_results.len() {
        bail!(
            "read_results has {} entries but only {} Popeq ops were found in the ops sequence",
            read_results.len(),
            popeq_idx
        );
    }

    Ok((out, per_op_sizes))
}
