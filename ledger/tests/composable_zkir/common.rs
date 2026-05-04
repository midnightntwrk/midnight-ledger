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

use std::borrow::Cow;
use std::collections::HashMap as StdHashMap;
use std::sync::Arc;

use base_crypto::fab::{Aligned, AlignedValue, Alignment, AlignmentAtom, Value};
use base_crypto::hash::HashOutput;
use base_crypto::time::Timestamp;
use midnight_ledger::construct::{ContractCallPrototype, partition_transcripts};
use midnight_ledger::structure::{ContractDeploy, INITIAL_PARAMETERS, Transaction};
use midnight_ledger::test_utilities::{TestState, test_intents};
use midnight_ledger::verify::WellFormedStrictness;
use onchain_runtime::cost_model::INITIAL_COST_MODEL;
use onchain_runtime::state::{
    ChargedState, ContractOperation, ContractState, EntryPointBuf, StateValue,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::tagged_serialize;
use storage::arena::Sp;
use storage::db::{DB, InMemoryDB};
use storage::storage::HashMap as StorageHashMap;
use transient_crypto::curve::Fr;
use transient_crypto::proofs::KeyLocation;
use transient_crypto::repr::FieldRepr;

pub use midnight_ledger::construct::PreTranscript;
pub use midnight_zkir_v3::Instruction;
pub use midnight_zkir_v3::ir_execute::{Call, CallRole, ZkirProvider};
use midnight_zkir_v3::ir_execute::{ExecutionContext, ExecutionError, ExecutionResult};
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

/// Build a single-entry-point `ContractTypeDescriptor` for tests. The
/// callee's actual typed signature is now validated against this
/// descriptor at runtime (see `IrSource::check_conformance`), so each
/// call site must declare the entry point name and the input/output
/// types that match its corresponding callee `IrSource`.
pub fn descriptor_for(
    name: &str,
    inputs: Vec<IrType>,
    outputs: Vec<IrType>,
) -> ContractTypeDescriptor {
    ContractTypeDescriptor {
        circuits: vec![CircuitSignature {
            name: name.to_string(),
            inputs,
            outputs,
        }],
    }
}

pub fn field_aligned_value(fr: Fr) -> AlignedValue {
    fields_aligned_value(&[fr])
}

pub fn fields_aligned_value(frs: &[Fr]) -> AlignedValue {
    use base_crypto::fab::{AlignmentSegment, ValueAtom};
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

/// Convert a ContractAddress to a `Vec<Fr>` of two field elements suitable
/// for passing as the flat `(addr_hi, addr_lo)` typed inputs.
pub fn addr_to_fr_vec(addr: coin_structure::contract::ContractAddress) -> Vec<Fr> {
    let (hi, lo) = addr_to_frs(addr);
    vec![hi, lo]
}

/// Construct an `EntryPointBuf` from a string slice.
pub fn ep(name: &str) -> EntryPointBuf {
    EntryPointBuf(name.as_bytes().to_vec())
}

/// Wrap a `&[Fr]` from a `Call::input`/`Call::output` into a single
/// `AlignedValue` whose alignment is `[Field; n]`. The executor exposes
/// inputs/outputs as flat `Vec<Fr>` (the `value_only_field_repr` view), but
/// `ContractCallPrototype.input` / `.output` are typed `AlignedValue`s. The
/// preimage-side commitment is computed over `value_only_field_repr`, so
/// the choice of alignment is purely cosmetic — singleton-Field-per-Fr
/// preserves the field-element sequence exactly.
pub fn frs_to_concat_av(frs: &[Fr]) -> AlignedValue {
    fields_aligned_value(frs)
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
        self.contracts
            .insert(address, ContractEntry { circuits, state });
    }
}

impl<D: DB + Send + Sync + 'static> ZkirProvider<D> for TestZkirProvider<D> {
    async fn fetch_contract(
        &self,
        address: coin_structure::contract::ContractAddress,
        entry_point: &[u8],
    ) -> Result<(IrSource, ChargedState<D>), ExecutionError> {
        let entry = self.contracts.get(&address).ok_or_else(|| {
            ExecutionError::ProviderError(format!("contract not found: {address:?}"))
        })?;
        let ep_str = std::str::from_utf8(entry_point)
            .map_err(|e| ExecutionError::ProviderError(format!("invalid entry point: {e}")))?;
        let ir = entry.circuits.get(ep_str).ok_or_else(|| {
            ExecutionError::ProviderError(format!("entry point '{ep_str}' not found"))
        })?;
        Ok((ir.clone(), entry.state.clone()))
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
        outputs: vec![
            TypedIdentifier::new(Identifier("%read_val".to_string()), IrType::Native),
        ],
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
        do_communications_commitment: true,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%inner_addr_hi"), var("%inner_addr_lo")),
                expected_type: descriptor_for("get", vec![], vec![IrType::Native]),
                entry_point: "get".to_string(),
                args: vec![],
                outputs: vec![id("%call_result")],
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
        outputs: vec![
            TypedIdentifier::new(Identifier("%result".to_string()), IrType::Native),
        ],
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
                    imm(1),            // alignment count
                    imm_neg(2),        // AlignmentAtom::Field encoding
                    var("%state_val"), // the read value
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
        do_communications_commitment: true,
        instructions: Arc::new(vec![
            Instruction::ContractCall {
                contract_ref: (var("%inner_hi"), var("%inner_lo")),
                expected_type: descriptor_for(
                    "add_state",
                    vec![IrType::Native],
                    vec![IrType::Native],
                ),
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
        do_communications_commitment: true,
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
                expected_type: descriptor_for(
                    "add_state",
                    vec![IrType::Native],
                    vec![IrType::Native],
                ),
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
            ]),
    }
}

// ─── Conversion helpers ────────────────────────────────────────────────────

/// Convert a `Call` (zkir-v3) to a `PreTranscript` (ledger).
///
/// Both types carry the same execution-side data: `context: QueryContext<D>`,
/// `program: Vec<Op<ResultModeVerify, D>>`, and `comm_comm: Option<Fr>`.
/// `Call::role` distinguishes `Root` (no comm_comm) from `Sub` (has one); this
/// helper extracts the appropriate value.
pub fn call_to_pre_transcript<D: DB>(call: Call<D>) -> PreTranscript<D> {
    let comm_comm = match &call.role {
        CallRole::Sub { comm_comm, .. } => Some(*comm_comm),
        CallRole::Root => None,
    };
    PreTranscript {
        context: call.context,
        program: call.program,
        comm_comm,
    }
}

/// Convert an `ExecutionResult` (`Vec<Call<D>>`) into the flat
/// `Vec<PreTranscript<D>>` expected by `partition_transcripts`. Order is
/// preserved (depth-first preorder).
pub fn calls_to_pre_transcripts<D: DB>(calls: Vec<Call<D>>) -> Vec<PreTranscript<D>> {
    calls.into_iter().map(call_to_pre_transcript).collect()
}

// ─── add_state pipeline helper ─────────────────────────────────────────────
//
// Shared deploy → execute → partition → prototype-build pipeline for the
// "add_state" / "call_add" composable scenario. Used by both the
// erase-proofs e2e test and the real-proof proving test — those two only
// differ in (a) how they obtain verifier keys and (b) what they do with
// the prototypes (`erase_proofs` vs. `prove`). Everything in between is
// identical and lives here.

/// Result of running the deploy → execute → partition → prototype-build
/// pipeline. Stops short of constructing the final transaction so the
/// caller can choose between `.erase_proofs()` (for fast tests) and
/// `.prove(...)` (for real-proof tests).
pub struct AddStatePipeline {
    pub state: TestState<D>,
    pub strictness: WellFormedStrictness,
    pub call_inner_proto: ContractCallPrototype<D>,
    pub call_outer_proto: ContractCallPrototype<D>,
    /// The rng with all up-to-this-point consumption already done. Caller
    /// uses this to drive `test_intents` and (in the proving case) to
    /// split off a prover RNG.
    pub rng: StdRng,
}

/// Run the shared "add_state" / "call_add" prep pipeline.
///
/// Inner ("add_state") reads `stored_val` from cell[0], adds the input,
/// returns the sum. Outer ("call_add") takes inner's address + a value,
/// calls inner.add_state(value), then adds the value again before
/// returning. So the outer's output is `stored_val + 2*input_val`.
///
/// `inner_vk` and `outer_vk` are taken as parameters because the e2e and
/// proving tests obtain them differently (random `rng.gen()` vs. real
/// `IrSource::keygen()`).
pub async fn add_state_pipeline(
    seed: u64,
    stored_val: Fr,
    input_val: Fr,
    inner_vk: VerifierKey,
    outer_vk: VerifierKey,
) -> AddStatePipeline {
    let mut rng = StdRng::seed_from_u64(seed);

    let expected_inner_result = stored_val + input_val;
    let expected_outer_result = expected_inner_result + input_val;

    let inner_ir = build_inner_add_state_ir();
    let outer_ir = build_outer_call_add_ir();

    let inner_state: ChargedState<D> = make_cell_state(field_aligned_value(stored_val));
    let outer_state: ChargedState<D> = ChargedState::new(StateValue::Null);

    let inner_op = ContractOperation::new_with_zkir(Some(inner_vk), serialize_ir(&inner_ir));
    let outer_op = ContractOperation::new_with_zkir(Some(outer_vk), serialize_ir(&outer_ir));

    // ── Deploy ──
    let mut state: TestState<D> = TestState::new(&mut rng);
    let mut strictness = WellFormedStrictness::default();
    strictness.enforce_balancing = false;

    let inner_contract = ContractState::new(
        inner_state.get_ref().clone(),
        StorageHashMap::new().insert(b"add_state"[..].into(), inner_op.clone()),
        Default::default(),
    );

    let inner_deploy = ContractDeploy::new(&mut rng, inner_contract);
    let inner_addr = inner_deploy.address();
    let deploy_inner_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            Vec::new(),
            Vec::new(),
            vec![inner_deploy],
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();
    state.assert_apply(&deploy_inner_tx, strictness);

    let outer_contract = ContractState::new(
        outer_state.get_ref().clone(),
        StorageHashMap::new().insert(b"call_add"[..].into(), outer_op.clone()),
        Default::default(),
    );
    let outer_deploy = ContractDeploy::new(&mut rng, outer_contract);
    let outer_addr = outer_deploy.address();
    let deploy_outer_tx = Transaction::from_intents(
        "local-test",
        test_intents(
            &mut rng,
            Vec::new(),
            Vec::new(),
            vec![outer_deploy],
            Timestamp::from_secs(0),
        ),
    )
    .erase_proofs();
    state.assert_apply(&deploy_outer_tx, strictness);

    // ── Execute ──
    let mut provider = TestZkirProvider::<D>::new();
    provider.register(
        inner_addr,
        StdHashMap::from([("add_state".to_string(), inner_ir.clone())]),
        inner_state.clone(),
    );
    provider.register(
        outer_addr,
        StdHashMap::from([("call_add".to_string(), outer_ir.clone())]),
        outer_state.clone(),
    );

    let context = ExecutionContext {
        ledger_state: outer_state,
        address: outer_addr,
        entry_point: ep("call_add"),
        zkir_provider: Arc::new(provider),
        witness_provider: None,
        call_depth: 0,
        max_call_depth: 8,
        cost_model: INITIAL_COST_MODEL,
    };

    let (addr_hi, addr_lo) = addr_to_frs(inner_addr);
    let mut inputs = addr_to_fr_vec(inner_addr);
    inputs.push(input_val);

    let result: ExecutionResult<D> = outer_ir
        .execute(inputs, context, &mut rng)
        .await
        .expect("execute should succeed");

    // ── Sanity-check the computed values ──
    assert_eq!(result.len(), 2, "two calls expected (outer + inner)");
    assert_eq!(
        result[1].output,
        vec![expected_inner_result],
        "inner should return stored_val + input_val"
    );
    assert_eq!(
        result[0].output,
        vec![expected_outer_result],
        "outer should return (stored_val + input_val) + input_val"
    );

    // ── Partition ──
    let pre_transcripts: Vec<PreTranscript<D>> = calls_to_pre_transcripts(result.clone());
    let pairs = partition_transcripts(&pre_transcripts, &INITIAL_PARAMETERS)
        .expect("partition should succeed");
    assert_eq!(pairs.len(), 2);

    // ── Build prototypes ──
    let sub = &result[1];
    let sub_comm_rand = sub.comm_comm_rand().expect("sub has comm_comm_rand");

    let call_inner_proto = ContractCallPrototype {
        address: inner_addr,
        entry_point: sub.entry_point.clone(),
        op: inner_op,
        guaranteed_public_transcript: pairs[1].0.clone(),
        fallible_public_transcript: pairs[1].1.clone(),
        private_transcript_outputs: vec![],
        input: frs_to_concat_av(&sub.input),
        output: frs_to_concat_av(&sub.output),
        communication_commitment_rand: sub_comm_rand,
        key_location: KeyLocation(Cow::Borrowed("add_state")),
    };

    let call_outer_proto = ContractCallPrototype {
        address: outer_addr,
        entry_point: ep("call_add"),
        op: outer_op,
        guaranteed_public_transcript: pairs[0].0.clone(),
        fallible_public_transcript: pairs[0].1.clone(),
        private_transcript_outputs: result[0].private_transcript_outputs.clone(),
        input: fields_aligned_value(&[addr_hi, addr_lo, input_val]),
        output: field_aligned_value(expected_outer_result),
        communication_commitment_rand: rng.r#gen::<Fr>(),
        key_location: KeyLocation(Cow::Borrowed("call_add")),
    };

    AddStatePipeline {
        state,
        strictness,
        call_inner_proto,
        call_outer_proto,
        rng,
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
    use base_crypto::fab::ValueAtom;
    let addr_aligned = AlignedValue {
        value: Value(vec![ValueAtom::from(addr.0)]),
        alignment: HashOutput::alignment(),
    };
    make_cell_state(addr_aligned)
}
