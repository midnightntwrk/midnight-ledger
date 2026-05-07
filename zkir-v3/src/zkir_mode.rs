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

//! Symbolic ZKIR-mode mirror of `onchain_vm::ops::Op`. Same variant set,
//! same opcode bytes, but operand-bearing payloads carry `Operand` references
//! that resolve from ZKIR memory at proving time.

use std::collections::HashMap;

use anyhow::anyhow;
use base_crypto::fab::Alignment;
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged};
use transient_crypto::curve::Fr;
use transient_crypto::repr::FieldRepr;

use crate::ir::{Identifier, Operand};
use crate::ir_types::IrValue;

/// Symbolic mirror of `base_crypto::fab::AlignedValue`: an alignment
/// paired with a list of symbolic operands that resolve, in order, to
/// the field-element bytes the runtime `AlignedValue::field_repr` would
/// produce. Operands resolve from circuit memory at proving time and
/// must already be `IrValue::Native`-typed by then.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Serializable)]
#[tag = "zkir-aligned-value[v1]"]
pub struct ZkirAlignedValue {
    pub alignment: Alignment,
    pub operands: Vec<Operand>,
}

/// Symbolic mirror of `runtime_state::state::StateValue`. Recursive in
/// the Cell / Map / Array / BoundedMerkleTree cases. The byte-stream
/// produced by `encode_state_value` matches the runtime
/// `StateValue::field_repr` byte-for-byte; the cross-encoder unit tests
/// pin the parity per variant.
///
/// Externally-tagged JSON shape:
///   `"null"`
///   `{"cell": {"alignment": [...], "operands": [...]}}`
///   `{"map": [[<key>, <value>], ...]}`
///   `{"array": [<value>, ...]}`
///   `{"bounded_merkle_tree": {"height": u8, "entries": [[u64, <value>], ...]}}`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Serializable)]
#[serde(rename_all = "snake_case")]
#[tag = "zkir-state-value[v1]"]
pub enum ZkirStateValue {
    Null,
    Cell(ZkirAlignedValue),
    /// Map entries must be in canonical sort order matching what the
    /// runtime `StateValue::Map.field_repr` emits (sorted by key).
    /// The compact compiler is responsible for canonical ordering.
    Map(Vec<(ZkirAlignedValue, Box<ZkirStateValue>)>),
    Array(Vec<Box<ZkirStateValue>>),
    BoundedMerkleTree {
        height: u8,
        entries: Vec<(u64, Box<ZkirStateValue>)>,
    },
}

/// Idx key: either the stack sentinel or a symbolic value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Serializable)]
#[serde(tag = "key_type", content = "value", rename_all = "snake_case")]
#[tag = "zkir-key[v1]"]
pub enum ZkirKey {
    Stack,
    Value {
        alignment: Alignment,
        operands: Vec<Operand>,
    },
}

/// Popeq's expected read result. Mirrors `AlignedValue`'s shape but with
/// symbolic operands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Serializable)]
#[tag = "zkir-read-result[v1]"]
pub struct ZkirReadResult {
    pub alignment: Alignment,
    pub operands: Vec<Operand>,
}

/// ZKIR-mode `Op`. Mirrors `onchain_vm::ops::Op`'s variant set with symbolic
/// payloads. Variant naming and serde encoding are kept consistent with the
/// runtime so tests and Compact-emitted JSON read the same shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Serializable)]
#[serde(rename_all = "lowercase", expecting = "operation")]
#[tag = "zkir-op[v1]"]
pub enum ZkirOp {
    Noop {
        n: u32,
    },
    Lt,
    Eq,
    Type,
    Size,
    New,
    And,
    Or,
    Neg,
    Log,
    Root,
    Pop,
    Popeq {
        cached: bool,
        result: ZkirReadResult,
    },
    /// Symbolic addi: `immediate` resolves to an Fr at proving time. The
    /// runtime `Op::Addi` carries a u32 literal; here we widen to `Operand`
    /// so compact-emitted IR can pass a circuit-variable reference whose
    /// value (e.g. a public-input scalar) is only known at proving time.
    /// The byte stream the prover emits for an immediate-Fr operand is
    /// identical to the runtime's `[0x0e, immediate_as_fr]`.
    Addi {
        immediate: Operand,
    },
    Subi {
        immediate: Operand,
    },
    /// Symbolic mirror of `Op::Push { storage, value: StateValue<D> }`.
    /// The byte stream is `[opcode | storage]` followed by the recursive
    /// `encode_state_value(value)` output, matching the runtime
    /// `StateValue::field_repr`.
    Push {
        storage: bool,
        value: ZkirStateValue,
    },
    Branch {
        skip: u32,
    },
    Jmp {
        skip: u32,
    },
    Add,
    Sub,
    Concat {
        cached: bool,
        n: u32,
    },
    Member,
    Rem {
        cached: bool,
    },
    Dup {
        n: u8,
    },
    Swap {
        n: u8,
    },
    Idx {
        cached: bool,
        #[serde(rename = "pushPath")]
        push_path: bool,
        path: Vec<ZkirKey>,
    },
    Ins {
        cached: bool,
        n: u8,
    },
    Ckpt,
}

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

/// Recursive byte-stream encoder for `ZkirStateValue`. Mirrors
/// `runtime_state::state::StateValue::field_repr` so the prover and
/// the runtime VM produce identical byte streams. All operand
/// references inside the value resolve through `memory` to Native Frs.
///
/// Tag bytes (low nibble = variant discriminator, upper bits = count):
///   `0`                                 → Null
///   `1`                                 → Cell
///   `2 | (size << 4)`                   → Map
///   `3 | (len  << 4)`                   → Array
///   `4 | (height << 4) | (entries << 12)` → BoundedMerkleTree
///
/// BoundedMerkleTree support is partial: encoding is implemented to
/// match `field_repr`, but the entry's `(u64 index, value)` field_repr
/// shape is reproduced verbatim from runtime, not yet covered by a
/// cross-encoder unit test. If a contract emits one, validate against
/// runtime before relying on the bytes.
fn encode_state_value(
    memory: &HashMap<Identifier, IrValue>,
    value: &ZkirStateValue,
    out: &mut Vec<Fr>,
) -> anyhow::Result<()> {
    use transient_crypto::repr::FieldRepr;
    match value {
        ZkirStateValue::Null => {
            out.push(Fr::from(0u64));
        }
        ZkirStateValue::Cell(av) => {
            out.push(Fr::from(1u64));
            av.alignment.field_repr(out);
            out.extend(resolve_operands(memory, &av.operands)?);
        }
        ZkirStateValue::Map(entries) => {
            // tag = 2 | (size << 4); runtime uses u128 for headroom.
            let tag = 2u128 | ((entries.len() as u128) << 4);
            out.push(u128_to_fr(tag));
            for (key, val) in entries {
                key.alignment.field_repr(out);
                out.extend(resolve_operands(memory, &key.operands)?);
                encode_state_value(memory, val, out)?;
            }
        }
        ZkirStateValue::Array(entries) => {
            let tag = 3u64 | ((entries.len() as u64) << 4);
            out.push(Fr::from(tag));
            for val in entries {
                encode_state_value(memory, val, out)?;
            }
        }
        ZkirStateValue::BoundedMerkleTree { height, entries } => {
            let tag = 4u128
                | ((*height as u128) << 4)
                | ((entries.len() as u128) << 12);
            out.push(u128_to_fr(tag));
            for (idx, val) in entries {
                // Tuple (u64, StateValue) field_repr = u64.field_repr + StateValue.field_repr
                (*idx).field_repr(out);
                encode_state_value(memory, val, out)?;
            }
        }
    }
    Ok(())
}

/// Convert a u128 to Fr by going through little-endian bytes, matching
/// the runtime's `(u128).into()` path used by `StateValue::field_repr`.
pub(crate) fn u128_to_fr(x: u128) -> Fr {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(&x.to_le_bytes());
    Fr::from_le_bytes(&bytes).expect("u128 fits in Fr")
}

fn resolve_operands(
    memory: &HashMap<Identifier, IrValue>,
    operands: &[Operand],
) -> anyhow::Result<Vec<Fr>> {
    operands
        .iter()
        .map(|op| resolve_operand_fr(memory, op))
        .collect()
}

/// Encode `ops` into the public-input Fr stream, returning per-op sizes
/// alongside (prove.rs needs one pi_skips entry per op). Output must
/// match `Op<ResultModeVerify>::field_repr` byte-for-byte; the cross-encoder
/// tests below pin that.
pub fn zkir_ops_to_field_elements_with_sizes(
    ops: Vec<ZkirOp>,
    memory: &HashMap<Identifier, IrValue>,
) -> anyhow::Result<(Vec<Fr>, Vec<usize>)> {
    let mut out: Vec<Fr> = Vec::new();
    let mut per_op_sizes: Vec<usize> = Vec::with_capacity(ops.len());

    for op in ops {
        let start = out.len();
        match op {
            ZkirOp::Noop { n } => {
                out.extend(std::iter::repeat_n(Fr::from(0u64), n as usize));
            }
            ZkirOp::Lt => out.push(0x01u64.into()),
            ZkirOp::Eq => out.push(0x02u64.into()),
            ZkirOp::Type => out.push(0x03u64.into()),
            ZkirOp::Size => out.push(0x04u64.into()),
            ZkirOp::New => out.push(0x05u64.into()),
            ZkirOp::And => out.push(0x06u64.into()),
            ZkirOp::Or => out.push(0x07u64.into()),
            ZkirOp::Neg => out.push(0x08u64.into()),
            ZkirOp::Log => out.push(0x09u64.into()),
            ZkirOp::Root => out.push(0x0au64.into()),
            ZkirOp::Pop => out.push(0x0bu64.into()),
            ZkirOp::Add => out.push(0x14u64.into()),
            ZkirOp::Sub => out.push(0x15u64.into()),
            ZkirOp::Member => out.push(0x18u64.into()),
            ZkirOp::Ckpt => out.push(0xffu64.into()),

            ZkirOp::Popeq { cached, result } => {
                out.push(((0x0cu8 + cached as u8) as u64).into());
                result.alignment.field_repr(&mut out);
                out.extend(resolve_operands(memory, &result.operands)?);
            }

            ZkirOp::Addi { immediate } => {
                out.push(0x0eu64.into());
                out.push(resolve_operand_fr(memory, &immediate)?);
            }
            ZkirOp::Subi { immediate } => {
                out.push(0x0fu64.into());
                out.push(resolve_operand_fr(memory, &immediate)?);
            }

            // Push mirrors the runtime `Op::Push { storage, value }` byte
            // stream: opcode | recursive StateValue encoding
            // (see encode_state_value below).
            ZkirOp::Push { storage, value } => {
                out.push(((0x10u8 + storage as u8) as u64).into());
                encode_state_value(memory, &value, &mut out)?;
            }

            ZkirOp::Branch { skip } => {
                out.push(0x12u64.into());
                out.push((skip as u64).into());
            }
            ZkirOp::Jmp { skip } => {
                out.push(0x13u64.into());
                out.push((skip as u64).into());
            }

            ZkirOp::Concat { cached, n } => {
                out.push(((0x16u8 + cached as u8) as u64).into());
                out.push((n as u64).into());
            }

            ZkirOp::Rem { cached } => {
                out.push(((0x19u8 + cached as u8) as u64).into());
            }

            ZkirOp::Dup { n } => out.push(((0x30u8 | n) as u64).into()),
            ZkirOp::Swap { n } => out.push(((0x40u8 | n) as u64).into()),

            ZkirOp::Idx {
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
                            ZkirKey::Stack => out.push(-Fr::from(1u64)),
                            ZkirKey::Value {
                                alignment,
                                operands,
                            } => {
                                alignment.field_repr(&mut out);
                                out.extend(resolve_operands(memory, operands)?);
                            }
                        }
                    }
                }
            }

            ZkirOp::Ins { cached, n } => {
                let base: u8 = if cached { 0xa0 } else { 0x90 };
                out.push(((base | n) as u64).into());
            }
        }
        per_op_sizes.push(out.len() - start);
    }

    Ok((out, per_op_sizes))
}

// Three encoders need to agree byte-for-byte: the runtime FieldRepr
// (onchain_vm::ops), the off-circuit one above, and the in-circuit
// I::Impact arm in ir_vm.rs. Drift breaks proof binding. The in-circuit
// one is covered end-to-end by the proving tests; these unit tests pin
// runtime vs off-circuit on every variant.
#[cfg(test)]
mod tests {
    use super::*;

    use crate::ir::Operand;
    use base_crypto::fab::{
        AlignedValue, Alignment, AlignmentAtom, AlignmentSegment, Value, ValueAtom,
    };
    use onchain_vm::ops::{Key, Op as RtOp};
    use onchain_vm::result_mode::ResultModeVerify;
    use runtime_state::state::StateValue;
    use std::collections::HashMap;
    use storage::arena::Sp;
    use storage::db::InMemoryDB;
    use storage::storage::Array;

    type RtOpV = RtOp<ResultModeVerify, InMemoryDB>;

    fn encode_runtime(ops: &[RtOpV]) -> Vec<Fr> {
        let mut out: Vec<Fr> = Vec::new();
        for op in ops {
            op.field_repr(&mut out);
        }
        out
    }

    // Tests use Immediate operands only, so memory is empty.
    fn encode_zkir(ops: Vec<ZkirOp>) -> Vec<Fr> {
        zkir_ops_to_field_elements_with_sizes(ops, &HashMap::new())
            .expect("zkir encoding succeeds")
            .0
    }

    // Single-Field-atom AV holding `fr`. Trailing zeros stripped.
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
            RtOp::Addi {
                immediate: 0xdead_beefu32,
            },
            RtOp::Subi {
                immediate: 0x1234_5678u32,
            },
            RtOp::Branch { skip: 0 },
            RtOp::Branch { skip: 42 },
            RtOp::Jmp {
                skip: 0xffff_ffffu32,
            },
            RtOp::Concat {
                cached: false,
                n: 0,
            },
            RtOp::Concat {
                cached: false,
                n: 7,
            },
            RtOp::Concat {
                cached: true,
                n: 13,
            },
            RtOp::Rem { cached: false },
            RtOp::Rem { cached: true },
            RtOp::Dup { n: 0 },
            RtOp::Dup { n: 5 },
            RtOp::Dup { n: 15 },
            RtOp::Swap { n: 0 },
            RtOp::Swap { n: 9 },
            RtOp::Ins {
                cached: false,
                n: 1,
            },
            RtOp::Ins {
                cached: true,
                n: 15,
            },
        ];

        let zkir: Vec<ZkirOp> = vec![
            ZkirOp::Noop { n: 0 },
            ZkirOp::Noop { n: 3 },
            ZkirOp::Lt,
            ZkirOp::Eq,
            ZkirOp::Type,
            ZkirOp::Size,
            ZkirOp::New,
            ZkirOp::And,
            ZkirOp::Or,
            ZkirOp::Neg,
            ZkirOp::Log,
            ZkirOp::Root,
            ZkirOp::Pop,
            ZkirOp::Add,
            ZkirOp::Sub,
            ZkirOp::Member,
            ZkirOp::Ckpt,
            ZkirOp::Addi {
                immediate: Operand::Immediate(Fr::from(0u64)),
            },
            ZkirOp::Addi {
                immediate: Operand::Immediate(Fr::from(0xdead_beefu64)),
            },
            ZkirOp::Subi {
                immediate: Operand::Immediate(Fr::from(0x1234_5678u64)),
            },
            ZkirOp::Branch { skip: 0 },
            ZkirOp::Branch { skip: 42 },
            ZkirOp::Jmp {
                skip: 0xffff_ffffu32,
            },
            ZkirOp::Concat {
                cached: false,
                n: 0,
            },
            ZkirOp::Concat {
                cached: false,
                n: 7,
            },
            ZkirOp::Concat {
                cached: true,
                n: 13,
            },
            ZkirOp::Rem { cached: false },
            ZkirOp::Rem { cached: true },
            ZkirOp::Dup { n: 0 },
            ZkirOp::Dup { n: 5 },
            ZkirOp::Dup { n: 15 },
            ZkirOp::Swap { n: 0 },
            ZkirOp::Swap { n: 9 },
            ZkirOp::Ins {
                cached: false,
                n: 1,
            },
            ZkirOp::Ins {
                cached: true,
                n: 15,
            },
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on simple ops"
        );
    }

    // Push of a Cell([Field], v): runtime emits opcode|1|alignment|v;
    // ZKIR emits opcode|1|alignment|resolve(operands). Operands need to
    // resolve to v.
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

        let zkir_av = ZkirAlignedValue {
            alignment: runtime_av.alignment.clone(),
            operands: vec![Operand::Immediate(value_fr)],
        };
        let zkir: Vec<ZkirOp> = vec![
            ZkirOp::Push {
                storage: false,
                value: ZkirStateValue::Cell(zkir_av.clone()),
            },
            ZkirOp::Push {
                storage: true,
                value: ZkirStateValue::Cell(zkir_av),
            },
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir);
        assert_eq!(rt_frs, zk_frs, "runtime and ZKIR encoders disagree on Push",);
    }

    // Push of StateValue::Null (tag = 0): runtime emits opcode|0; ZKIR emits
    // opcode|0 — no alignment or operands follow. Pins the byte-stream
    // fidelity of the Null special-case in zkir_ops_to_field_elements_with_sizes.
    #[test]
    fn push_null_runtime_matches_zkir() {
        let runtime: Vec<RtOpV> = vec![
            RtOp::Push {
                storage: false,
                value: StateValue::Null,
            },
            RtOp::Push {
                storage: true,
                value: StateValue::Null,
            },
        ];

        let zkir: Vec<ZkirOp> = vec![
            ZkirOp::Push {
                storage: false,
                value: ZkirStateValue::Null,
            },
            ZkirOp::Push {
                storage: true,
                value: ZkirStateValue::Null,
            },
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on Push of StateValue::Null",
        );
    }

    // Idx with [Stack, Value, Stack] across all four cached/push_path combos.
    #[test]
    fn idx_runtime_matches_zkir() {
        let key_value_fr = Fr::from(0x1234u64);
        let key_av = field_av(key_value_fr);

        // path: [Stack, Value(key_av), Stack]
        let runtime_path: Array<Key, InMemoryDB> =
            vec![Key::Stack, Key::Value(key_av.clone()), Key::Stack].into();
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
            ZkirOp::Idx {
                cached: false,
                push_path: false,
                path: zkir_path.clone(),
            },
            ZkirOp::Idx {
                cached: true,
                push_path: false,
                path: zkir_path.clone(),
            },
            ZkirOp::Idx {
                cached: false,
                push_path: true,
                path: zkir_path.clone(),
            },
            ZkirOp::Idx {
                cached: true,
                push_path: true,
                path: zkir_path,
            },
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir);
        assert_eq!(rt_frs, zk_frs, "runtime and ZKIR encoders disagree on Idx",);
    }

    // Popeq: runtime emits opcode|alignment|value via AlignedValue.field_repr;
    // ZKIR emits opcode|alignment|resolve(operands) where alignment lives on
    // the inline ZkirReadResult.
    #[test]
    fn popeq_runtime_matches_zkir() {
        let read_value = Fr::from(0xabcdu64);
        let read_av = field_av(read_value);

        // operands carry only the value; alignment is on ZkirReadResult
        let value_operands: Vec<Operand> = vec![Operand::Immediate(read_value)];

        let runtime: Vec<RtOpV> = vec![
            RtOp::Popeq {
                cached: false,
                result: read_av.clone(),
            },
            RtOp::Popeq {
                cached: true,
                result: read_av.clone(),
            },
        ];
        let zkir: Vec<ZkirOp> = vec![
            ZkirOp::Popeq {
                cached: false,
                result: ZkirReadResult {
                    alignment: read_av.alignment.clone(),
                    operands: value_operands.clone(),
                },
            },
            ZkirOp::Popeq {
                cached: true,
                result: ZkirReadResult {
                    alignment: read_av.alignment.clone(),
                    operands: value_operands,
                },
            },
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on Popeq",
        );
    }

    // Mix of Push, Idx (incl. empty-path), Popeq, and scalar ops in one run.
    #[test]
    fn mixed_program_runtime_matches_zkir() {
        let push_value_fr = Fr::from(7u64);
        let idx_key_fr = Fr::from(11u64);
        let popeq_value_fr = Fr::from(13u64);
        let push_av = field_av(push_value_fr);
        let key_av = field_av(idx_key_fr);
        let popeq_av = field_av(popeq_value_fr);

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
                result: popeq_av.clone(),
            },
            RtOp::Concat { cached: true, n: 2 },
            // empty-path Idx: both encoders emit nothing
            RtOp::Idx {
                cached: false,
                push_path: false,
                path: Array::<Key, InMemoryDB>::default(),
            },
            RtOp::Ckpt,
        ];

        let zkir: Vec<ZkirOp> = vec![
            ZkirOp::Dup { n: 0 },
            ZkirOp::Push {
                storage: false,
                value: ZkirStateValue::Cell(ZkirAlignedValue {
                    alignment: push_av.alignment.clone(),
                    operands: vec![Operand::Immediate(push_value_fr)],
                }),
            },
            ZkirOp::Idx {
                cached: true,
                push_path: false,
                path: vec![ZkirKey::Value {
                    alignment: key_av.alignment.clone(),
                    operands: vec![Operand::Immediate(idx_key_fr)],
                }],
            },
            ZkirOp::Popeq {
                cached: false,
                result: ZkirReadResult {
                    alignment: popeq_av.alignment.clone(),
                    operands: vec![Operand::Immediate(popeq_value_fr)],
                },
            },
            ZkirOp::Concat { cached: true, n: 2 },
            ZkirOp::Idx {
                cached: false,
                push_path: false,
                path: Vec::<ZkirKey>::new(),
            },
            ZkirOp::Ckpt,
        ];

        let rt_frs = encode_runtime(&runtime);
        let zk_frs = encode_zkir(zkir);
        assert_eq!(
            rt_frs, zk_frs,
            "runtime and ZKIR encoders disagree on mixed program"
        );
    }
}
