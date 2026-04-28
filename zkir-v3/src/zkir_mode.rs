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
#[serde(tag = "key_type", content = "value", rename_all = "snake_case")]
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

/// Encode a sequence of `ZkirOp`s into the flat field-element stream that
/// the public-input vector embeds, alongside the per-op size in field
/// elements. The size vector exists because `prove.rs` consumes one
/// `pi_skips` entry per transcript op, so the preprocessor must produce
/// one entry per op.
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
                            ZkirKey::Value {
                                alignment,
                                operands,
                            } => {
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

// ── Cross-encoder consistency tests ──────────────────────────────────────
//
// `Op` is encoded into the public-input stream by *three* parallel paths
// that must agree byte-for-byte:
//
//   1. `onchain_vm::ops::impl FieldRepr for Op<ResultModeVerify, D>`
//      — the canonical runtime encoding.
//   2. `zkir_ops_to_field_elements_with_sizes` (this module)
//      — the off-circuit ZKIR encoding consumed by `ir_preprocess`.
//   3. The `I::Impact` arm in `ir_circuit.rs::Relation::synthesize_inner`
//      — the in-circuit encoding emitted as constrained public inputs.
//
// Drift between any two of these silently breaks proof binding. The
// in-circuit encoder (3) is exercised end-to-end by the existing proving
// tests (`zkir-v3/tests/proofs.rs`, `ledger/tests/composable_zkir/proving.rs`
// — both of which prove and verify against the public-input vector that
// encoder 2 produces). The tests below pin encoders 1 and 2 against each
// other on a corpus that covers every `Op` variant.
#[cfg(test)]
mod tests {
    use super::*;

    use crate::ir::{Identifier, Operand};
    use crate::ir_types::IrValue;
    use base_crypto::fab::{
        AlignedValue, Alignment, AlignmentAtom, AlignmentSegment, Value, ValueAtom,
    };
    use onchain_runtime_state::state::StateValue;
    use onchain_vm::ops::{Key, Op as RtOp};
    use onchain_vm::result_mode::ResultModeVerify;
    use std::collections::HashMap;
    use storage::arena::Sp;
    use storage::db::InMemoryDB;
    use storage::storage::Array;

    type RtOpV = RtOp<ResultModeVerify, InMemoryDB>;

    /// Encode a `Vec<RtOpV>` via the canonical runtime `Op::field_repr`.
    fn encode_runtime(ops: &[RtOpV]) -> Vec<Fr> {
        let mut out: Vec<Fr> = Vec::new();
        for op in ops {
            op.field_repr(&mut out);
        }
        out
    }

    /// Encode a `Vec<ZkirOp>` via the off-circuit ZKIR encoder. Operand
    /// resolution uses an empty memory because all operands in our tests
    /// are `Operand::Immediate`.
    fn encode_zkir(ops: Vec<ZkirOp>, read_results: &[Vec<Operand>]) -> Vec<Fr> {
        let memory: HashMap<Identifier, IrValue> = HashMap::new();
        let (out, _per_op_sizes) =
            zkir_ops_to_field_elements_with_sizes(ops, read_results, &memory)
                .expect("zkir encoding succeeds");
        out
    }

    /// Build an `AlignedValue` containing a single `Field` atom whose value
    /// is the LE-byte representation of `fr` (trailing zeros stripped — the
    /// canonical form used elsewhere in the codebase). Used so that
    /// `value_only_field_repr` of the resulting AV equals `vec![fr]`.
    fn field_av(fr: Fr) -> AlignedValue {
        let mut bytes = fr.0.to_bytes_le().to_vec();
        while let Some(0) = bytes.last() {
            bytes.pop();
        }
        AlignedValue {
            value: Value(vec![ValueAtom(bytes)]),
            alignment: Alignment(vec![AlignmentSegment::Atom(AlignmentAtom::Field)]),
        }
    }

    /// Variants with no V/K/result payload — should encode identically
    /// regardless of the V/K type parameters.
    #[test]
    fn simple_ops_runtime_matches_zkir() {
        let runtime: Vec<RtOpV> = vec![
            RtOp::Noop { n: 0 },
            RtOp::Noop { n: 3 },
            RtOp::Lt,
            RtOp::Eq,
            RtOp::Type,
            RtOp::Size,
            RtOp::New,
            RtOp::And,
            RtOp::Or,
            RtOp::Neg,
            RtOp::Log,
            RtOp::Root,
            RtOp::Pop,
            RtOp::Add,
            RtOp::Sub,
            RtOp::Member,
            RtOp::Ckpt,
            RtOp::Addi { immediate: 0 },
            RtOp::Addi { immediate: 0xdead_beefu32 },
            RtOp::Subi { immediate: 0x1234_5678u32 },
            RtOp::Branch { skip: 0 },
            RtOp::Branch { skip: 42 },
            RtOp::Jmp { skip: 0xffff_ffffu32 },
            RtOp::Concat { cached: false, n: 0 },
            RtOp::Concat { cached: false, n: 7 },
            RtOp::Concat { cached: true, n: 13 },
            RtOp::Rem { cached: false },
            RtOp::Rem { cached: true },
            RtOp::Dup { n: 0 },
            RtOp::Dup { n: 5 },
            RtOp::Dup { n: 15 },
            RtOp::Swap { n: 0 },
            RtOp::Swap { n: 9 },
            RtOp::Ins { cached: false, n: 1 },
            RtOp::Ins { cached: true, n: 15 },
        ];

        // Every op above is V/K/result-free, so `translate_full` with
        // panicking V- and K-closures and a trivial M-closure yields the
        // matching ZKIR-mode form.
        let zkir: Vec<ZkirOp> = runtime
            .iter()
            .cloned()
            .map(|op| {
                op.translate_full(
                    |_| panic!("simple ops carry no Push value"),
                    |_| panic!("simple ops carry no Idx path"),
                    |_av| (),
                )
            })
            .collect();

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir, &[]);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on simple ops",
        );
    }

    /// `Push` of a `StateValue::Cell` holding a Field-aligned AV. Encoder 1
    /// writes `[opcode, 1, alignment.field_repr, value_only_field_repr]`;
    /// encoder 2 writes `[opcode, 1, alignment.field_repr, resolved operands]`.
    /// They match iff `operands` resolve to `value_only_field_repr`.
    #[test]
    fn push_runtime_matches_zkir() {
        let value_fr = Fr::from(0xcafe_babeu64);

        let runtime_av = field_av(value_fr);
        let runtime: Vec<RtOpV> = vec![
            RtOp::Push {
                storage: false,
                value: StateValue::Cell(Sp::new(runtime_av.clone())),
            },
            RtOp::Push {
                storage: true,
                value: StateValue::Cell(Sp::new(runtime_av.clone())),
            },
        ];

        let zkir_value = ZkirPushValue {
            storage: false, // overwritten per-op below; storage flag is on the Op itself
            alignment: runtime_av.alignment.clone(),
            operands: vec![Operand::Immediate(value_fr)],
        };
        let zkir: Vec<ZkirOp> = vec![
            Op::Push {
                storage: false,
                value: ZkirPushValue {
                    storage: false,
                    ..zkir_value.clone()
                },
            },
            Op::Push {
                storage: true,
                value: ZkirPushValue {
                    storage: true,
                    ..zkir_value
                },
            },
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir, &[]);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on Push",
        );
    }

    /// `Idx` with a mix of `Key::Stack` and `Key::Value(field_av)`. Both
    /// encoders share the opcode-byte construction; this pins the per-key
    /// payload encoding (`-Fr::from(1)` for Stack, `alignment + value` for
    /// Value).
    #[test]
    fn idx_runtime_matches_zkir() {
        let key_value_fr = Fr::from(0x1234u64);
        let key_av = field_av(key_value_fr);

        // Build a runtime path: [Stack, Value(key_av), Stack].
        let runtime_path: Array<Key, InMemoryDB> = vec![
            Key::Stack,
            Key::Value(key_av.clone()),
            Key::Stack,
        ]
        .into();
        let runtime: Vec<RtOpV> = vec![
            RtOp::Idx {
                cached: false,
                push_path: false,
                path: runtime_path.clone(),
            },
            RtOp::Idx {
                cached: true,
                push_path: false,
                path: runtime_path.clone(),
            },
            RtOp::Idx {
                cached: false,
                push_path: true,
                path: runtime_path.clone(),
            },
            RtOp::Idx {
                cached: true,
                push_path: true,
                path: runtime_path,
            },
        ];

        let zkir_path: Vec<ZkirKey> = vec![
            ZkirKey::Stack,
            ZkirKey::Value {
                alignment: key_av.alignment.clone(),
                operands: vec![Operand::Immediate(key_value_fr)],
            },
            ZkirKey::Stack,
        ];
        let zkir: Vec<ZkirOp> = vec![
            Op::Idx {
                cached: false,
                push_path: false,
                path: zkir_path.clone(),
            },
            Op::Idx {
                cached: true,
                push_path: false,
                path: zkir_path.clone(),
            },
            Op::Idx {
                cached: false,
                push_path: true,
                path: zkir_path.clone(),
            },
            Op::Idx {
                cached: true,
                push_path: true,
                path: zkir_path,
            },
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir, &[]);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on Idx",
        );
    }

    /// `Popeq` carries different payload types in the two encoders: encoder
    /// 1 stores the read result as an `AlignedValue` and emits its full
    /// `field_repr` (alignment metadata + value); encoder 2 stores `()` and
    /// pulls the field elements from a parallel `read_results` operand list.
    /// They match iff the operands resolve to `result.field_repr()`.
    #[test]
    fn popeq_runtime_matches_zkir() {
        let read_value = Fr::from(0xfeed_face_dead_beefu64);
        let read_av = field_av(read_value);

        // Capture the full `result.field_repr()` so we can mirror it via
        // `Operand::Immediate` operands on the ZKIR side. This is the same
        // trick the Compact compiler uses when emitting Impact's
        // `read_results`: the operands carry the *complete* field
        // representation of the read result, including alignment metadata.
        let mut full_repr: Vec<Fr> = Vec::new();
        read_av.field_repr(&mut full_repr);
        let read_operands: Vec<Operand> =
            full_repr.iter().copied().map(Operand::Immediate).collect();

        let runtime: Vec<RtOpV> = vec![
            RtOp::Popeq {
                cached: false,
                result: read_av.clone(),
            },
            RtOp::Popeq {
                cached: true,
                result: read_av,
            },
        ];
        let zkir: Vec<ZkirOp> = vec![
            Op::Popeq {
                cached: false,
                result: (),
            },
            Op::Popeq {
                cached: true,
                result: (),
            },
        ];
        let read_results = vec![read_operands.clone(), read_operands];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir, &read_results);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on Popeq",
        );
    }

    /// A mixed program exercising Push, Idx (with empty path — both
    /// encoders skip emission), Popeq, and several scalar variants in one
    /// sequence. This pins the *interaction* between variants in the same
    /// encoded run, not just each one in isolation.
    #[test]
    fn mixed_program_runtime_matches_zkir() {
        let push_value_fr = Fr::from(7u64);
        let idx_key_fr = Fr::from(11u64);
        let popeq_value_fr = Fr::from(13u64);
        let push_av = field_av(push_value_fr);
        let key_av = field_av(idx_key_fr);
        let popeq_av = field_av(popeq_value_fr);

        let mut popeq_full_repr: Vec<Fr> = Vec::new();
        popeq_av.field_repr(&mut popeq_full_repr);

        let runtime: Vec<RtOpV> = vec![
            RtOp::Dup { n: 0 },
            RtOp::Push {
                storage: false,
                value: StateValue::Cell(Sp::new(push_av.clone())),
            },
            RtOp::Idx {
                cached: true,
                push_path: false,
                path: vec![Key::Value(key_av.clone())].into(),
            },
            RtOp::Popeq {
                cached: false,
                result: popeq_av,
            },
            RtOp::Concat { cached: true, n: 2 },
            // Empty-path Idx: both encoders skip emission entirely. Keeping
            // it in the sequence documents that they agree on the skip.
            RtOp::Idx {
                cached: false,
                push_path: false,
                path: Array::<Key, InMemoryDB>::default(),
            },
            RtOp::Ckpt,
        ];

        let zkir: Vec<ZkirOp> = vec![
            Op::Dup { n: 0 },
            Op::Push {
                storage: false,
                value: ZkirPushValue {
                    storage: false,
                    alignment: push_av.alignment.clone(),
                    operands: vec![Operand::Immediate(push_value_fr)],
                },
            },
            Op::Idx {
                cached: true,
                push_path: false,
                path: vec![ZkirKey::Value {
                    alignment: key_av.alignment.clone(),
                    operands: vec![Operand::Immediate(idx_key_fr)],
                }],
            },
            Op::Popeq {
                cached: false,
                result: (),
            },
            Op::Concat { cached: true, n: 2 },
            Op::Idx {
                cached: false,
                push_path: false,
                path: Vec::<ZkirKey>::new(),
            },
            Op::Ckpt,
        ];

        let read_results = vec![popeq_full_repr.iter().copied().map(Operand::Immediate).collect()];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir, &read_results);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on mixed program",
        );
    }
}
