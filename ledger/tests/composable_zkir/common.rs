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

//! Shared utilities for composable ZKIR integration tests.
//!
//! Contains type aliases, IR helpers, the test `ZkirProvider`, IR builders
//! for inner/outer contracts, state construction helpers, and conversion
//! helpers between zkir-v3 and ledger transcript types.

use std::collections::HashMap as StdHashMap;
use std::sync::Arc;

use base_crypto::fab::{Aligned, AlignedValue, Alignment, AlignmentAtom};
use base_crypto::hash::HashOutput;
use onchain_runtime::state::{ChargedState, StateValue};
use serialize::tagged_serialize;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use transient_crypto::curve::Fr;
use transient_crypto::repr::FieldRepr;

pub use midnight_ledger::construct::PreTranscript;
use midnight_zkir_v3::ir_execute::ExecutionError;
pub use midnight_zkir_v3::ir_execute::{PreTranscriptData, ZkirProvider};
pub use midnight_zkir_v3::Instruction;
use midnight_zkir_v3::{
    CircuitSignature, ContractTypeDescriptor, Identifier, IrSource, IrType, Operand,
    TypedIdentifier, ZkirKey, ZkirOp,
};
use onchain_runtime::ops::Op;
pub use transient_crypto::proofs::VerifierKey;

// ─── Type alias ────────────────────────────────────────────────────────────

pub type D = InMemoryDB;

// ─── IR Helpers (mirrored from zkir-v3/tests/composable_zkir.rs) ───────────

pub fn id(name: &str) -> Identifier {
    Identifier(name.to_string())
}

pub fn var(name: &str) -> Operand {
    Operand::Variable(id(name))
}

pub fn imm(v: u64) -> Operand {
    Operand::Immediate(Fr::from(v))
}

/// Signed immediate — needed for alignment atom encodings like Field (-2).
pub fn imm_neg(v: u64) -> Operand {
    Operand::Immediate(-Fr::from(v))
}

pub fn alignment_u8() -> Alignment {
    Alignment::singleton(AlignmentAtom::Bytes { length: 1 })
}

pub fn serialize_ir(ir: &IrSource) -> Vec<u8> {
    let mut buf = Vec::new();
    tagged_serialize(ir, &mut buf).expect("IrSource serialization failed");
    buf
}

pub fn make_address(seed: u64) -> coin_structure::contract::ContractAddress {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    coin_structure::contract::ContractAddress(HashOutput(bytes))
}

pub fn stub_type_descriptor() -> ContractTypeDescriptor {
    ContractTypeDescriptor {
        circuits: vec![CircuitSignature {
            name: "test".to_string(),
            param_count: 0,
            return_count: 1,
        }],
    }
}

pub fn field_aligned_value(fr: Fr) -> AlignedValue {
    fields_aligned_value(&[fr])
}

pub fn fields_aligned_value(frs: &[Fr]) -> AlignedValue {
    use base_crypto::fab::{AlignmentSegment, Value, ValueAtom};
    let segments: Vec<AlignmentSegment> = frs
        .iter()
        .map(|_| AlignmentSegment::Atom(AlignmentAtom::Field))
        .collect();
    let alignment = Alignment(segments);
    let atoms: Vec<ValueAtom> = frs
        .iter()
        .map(|fr| {
            let bytes = fr.0.to_bytes_le();
            let mut v = bytes.to_vec();
            while let Some(0) = v.last() {
                v.pop();
            }
            ValueAtom(v)
        })
        .collect();
    AlignedValue {
        value: Value(atoms),
        alignment,
    }
}

/// Convert a ContractAddress to its two-element field representation.
/// Returns `(Fr(byte31), Fr(bytes0..31))` matching `[u8; 32]::field_repr`.
pub fn addr_to_frs(addr: coin_structure::contract::ContractAddress) -> (Fr, Fr) {
    let mut fields = Vec::new();
    addr.0.field_repr(&mut fields);
    assert_eq!(
        fields.len(),
        2,
        "ContractAddress field repr must be 2 elements"
    );
    (fields[0], fields[1])
}

// ─── ZkirProvider for tests ────────────────────────────────────────────────

pub struct ContractEntry<D: DB> {
    circuits: StdHashMap<String, IrSource>,
    state: ChargedState<D>,
}

pub struct TestZkirProvider<D: DB> {
    contracts: StdHashMap<coin_structure::contract::ContractAddress, ContractEntry<D>>,
}

impl<D: DB> TestZkirProvider<D> {
    pub fn new() -> Self {
        TestZkirProvider {
            contracts: StdHashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        address: coin_structure::contract::ContractAddress,
        circuits: StdHashMap<String, IrSource>,
        state: ChargedState<D>,
    ) {
        self.contracts.insert(
            address,
            ContractEntry {
                circuits,
                state,
            },
        );
    }
}

impl<D: DB + Send + Sync + 'static> ZkirProvider<D> for TestZkirProvider<D> {
    fn fetch_zkir(
        &self,
        address: coin_structure::contract::ContractAddress,
        entry_point: &[u8],
    ) -> Result<IrSource, ExecutionError> {
        let entry = self.contracts.get(&address).ok_or_else(|| {
            ExecutionError::ProviderError(format!("contract not found: {address:?}"))
        })?;
        let ep_str = std::str::from_utf8(entry_point)
            .map_err(|e| ExecutionError::ProviderError(format!("invalid entry point: {e}")))?;
        let ir = entry.circuits.get(ep_str).ok_or_else(|| {
            ExecutionError::ProviderError(format!("entry point '{ep_str}' not found"))
        })?;
        Ok(ir.clone())
    }

    fn fetch_state(
        &self,
        address: coin_structure::contract::ContractAddress,
    ) -> Result<ChargedState<D>, ExecutionError> {
        let entry = self.contracts.get(&address).ok_or_else(|| {
            ExecutionError::ProviderError(format!("contract not found: {address:?}"))
        })?;
        Ok(entry.state.clone())
    }
}

// ─── IR builders ───────────────────────────────────────────────────────────

/// Inner contract "get": reads cell[0] from state and outputs it.
pub fn build_inner_get_ir() -> IrSource {
    let ops: Vec<ZkirOp> = vec![
        Op::Dup { n: 0 },
        Op::Idx {
            cached: false,
            push_path: false,
            path: vec![ZkirKey::Value {
                alignment: alignment_u8(),
                operands: vec![imm(0)],
            }],
        },
        Op::Popeq {
            cached: false,
            result: (),
        },
    ];

    IrSource {
        inputs: vec![],
        do_communications_commitment: true,
        instructions: Arc::new(vec![
            Instruction::Impact {
                guard: imm(1),
                ops,
                read_results: vec![vec![]],
            },
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%read_val"),
            },
            Instruction::Output {
                val: var("%read_val"),
            },
        ]),
    }
}

/// Outer contract "call_inner": takes inner address as two field-element
/// inputs, calls inner's "get", and outputs the result.
pub fn build_outer_call_ir() -> IrSource {
    IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%inner_addr_hi"), IrType::Native),
            TypedIdentifier::new(id("%inner_addr_lo"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%inner_addr_hi"), var("%inner_addr_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "get".to_string(),
                args: vec![],
                outputs: vec![id("%call_result")],
            },
            Instruction::Output {
                val: var("%call_result"),
            },
        ]),
    }
}

/// Inner contract "add_state": takes one input, reads a stored value from
/// state[0], and returns input + stored_value.
///
/// Requires state to contain a Field-aligned cell at key 0.
/// This ensures the callee has a non-empty transcript (Impact ops).
pub fn build_inner_add_state_ir() -> IrSource {
    let ops: Vec<ZkirOp> = vec![
        Op::Dup { n: 0 },
        Op::Idx {
            cached: false,
            push_path: false,
            path: vec![ZkirKey::Value {
                alignment: alignment_u8(),
                operands: vec![imm(0)],
            }],
        },
        Op::Popeq {
            cached: false,
            result: (),
        },
    ];

    IrSource {
        inputs: vec![TypedIdentifier::new(id("%x"), IrType::Native)],
        do_communications_commitment: true,
        instructions: Arc::new(vec![
            // Impact runs first (executor needs it before PublicInput since
            // PublicInput reads Popeq results from the transcript).
            Instruction::Impact {
                guard: imm(1),
                ops,
                // The Popeq read result encodes one AlignedValue whose
                // alignment is singleton(Field). AlignedValue.field_repr()
                // produces: alignment.field_repr() ++ value, which for
                // singleton(Field) is [1 (count), -2 (Field atom), value].
                //
                // var("%state_val") is resolved from circuit memory. The
                // preprocessor and circuit pre-populate PublicInput outputs
                // before the instruction loop so they're available here.
                read_results: vec![vec![
                    imm(1),              // alignment count
                    imm_neg(2),          // AlignmentAtom::Field encoding
                    var("%state_val"),    // the read value
                ]],
            },
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%state_val"),
            },
            // %result = %x + %state_val
            Instruction::Add {
                a: var("%x"),
                b: var("%state_val"),
                output: id("%result"),
            },
            Instruction::Output {
                val: var("%result"),
            },
        ]),
    }
}

/// Outer contract "call_add": takes inner address (2 fields) + a value,
/// calls inner.add_state(value), then outputs call_result + value.
///
/// If inner's state holds S and input is V:
///   inner returns V + S
///   outer returns (V + S) + V = 2V + S
pub fn build_outer_call_add_ir() -> IrSource {
    IrSource {
        inputs: vec![
            TypedIdentifier::new(id("%inner_hi"), IrType::Native),
            TypedIdentifier::new(id("%inner_lo"), IrType::Native),
            TypedIdentifier::new(id("%val"), IrType::Native),
        ],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%inner_hi"), var("%inner_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "add_state".to_string(),
                args: vec![var("%val")],
                outputs: vec![id("%from_inner")],
            },
            // %result = %from_inner + %val = (val + state_val) + val
            Instruction::Add {
                a: var("%from_inner"),
                b: var("%val"),
                output: id("%result"),
            },
            Instruction::Output {
                val: var("%result"),
            },
        ]),
    }
}

/// Outer contract "call_from_state": reads a contract address from ledger
/// state at key 0, calls that contract's "add_state" with a provided value,
/// then outputs call_result + caller_val.
///
/// If inner's state holds S and input is V:
///   inner returns V + S
///   outer returns (V + S) + V = 2V + S
pub fn build_outer_call_from_state_ir() -> IrSource {
    IrSource {
        inputs: vec![TypedIdentifier::new(id("%caller_val"), IrType::Native)],
        do_communications_commitment: false,
        instructions: Arc::new(vec![
            Instruction::Impact {
                guard: imm(1),
                ops: vec![
                    Op::Dup { n: 0 },
                    Op::Idx {
                        cached: false,
                        push_path: false,
                        path: vec![ZkirKey::Value {
                            alignment: alignment_u8(),
                            operands: vec![imm(0)],
                        }],
                    },
                    Op::Popeq {
                        cached: false,
                        result: (),
                    },
                ],
                read_results: vec![vec![]],
            },
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%addr_hi"),
            },
            Instruction::PublicInput {
                guard: None,
                val_t: IrType::Native,
                output: id("%addr_lo"),
            },
            Instruction::ContractCall {
                contract_ref: (var("%addr_hi"), var("%addr_lo")),
                expected_type: stub_type_descriptor(),
                entry_point: "add_state".to_string(),
                args: vec![var("%caller_val")],
                outputs: vec![id("%call_result")],
            },
            // %final = %call_result + %caller_val = (V + S) + V
            Instruction::Add {
                a: var("%call_result"),
                b: var("%caller_val"),
                output: id("%final"),
            },
            Instruction::Output {
                val: var("%final"),
            },
        ]),
    }
}

// ─── Conversion helpers ────────────────────────────────────────────────────

/// Convert a `PreTranscriptData` (zkir-v3) to a `PreTranscript` (ledger).
///
/// These types are structurally identical: both have `context: QueryContext<D>`,
/// `program: Vec<Op<ResultModeVerify, D>>`, and `comm_comm: Option<Fr>`.
pub fn to_pre_transcript<D: DB>(ptd: PreTranscriptData<D>) -> PreTranscript<D> {
    PreTranscript {
        context: ptd.context,
        program: ptd.program,
        comm_comm: ptd.comm_comm,
    }
}

// ─── State helpers ─────────────────────────────────────────────────────────

pub fn make_cell_state(value: AlignedValue) -> ChargedState<D> {
    let cell = StateValue::Cell(Sp::new(value));
    let arr = StateValue::Array(vec![cell].into());
    ChargedState::new(arr)
}

/// Build a `ChargedState` with a single cell containing a contract address
/// (Bytes<32>) at key 0.
pub fn make_address_state(addr: coin_structure::contract::ContractAddress) -> ChargedState<D> {
    use base_crypto::fab::Value;
    use base_crypto::fab::ValueAtom;
    let addr_aligned = AlignedValue {
        value: Value(vec![ValueAtom::from(addr.0)]),
        alignment: HashOutput::alignment(),
    };
    make_cell_state(addr_aligned)
}
