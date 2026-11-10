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

//! `Op::Noop { n }` is the only opcode whose public-input footprint is not
//! proportional to its own encoding: `n` is a `u32` in a 9-byte op, and each
//! unit of it contributes one 32-byte field element. Fee estimation must
//! therefore count public inputs without materializing them.

use midnight_ledger_v9::error::MalformedTransaction;
use midnight_ledger_v9::structure::ContractCall;
use onchain_runtime::context::Effects;
use onchain_runtime::ops::{Key, Op};
use onchain_runtime::result_mode::ResultModeVerify;
use onchain_runtime::state::StateValue;
use onchain_runtime::transcript::Transcript;
use std::time::{Duration, Instant};
use storage::arena::Sp;
use storage::db::InMemoryDB;
use transient_crypto::repr::FieldRepr;

type Call = ContractCall<(), InMemoryDB>;
type VmOp = Op<ResultModeVerify, InMemoryDB>;

fn call_with_program(program: Vec<VmOp>) -> Call {
    let transcript = Transcript {
        gas: Default::default(),
        effects: Effects::default(),
        program: program.into(),
        version: None,
    };
    ContractCall {
        address: Default::default(),
        entry_point: b"count"[..].into(),
        guaranteed_transcript: Some(Sp::new(transcript)),
        fallible_transcript: None,
        communication_commitment: Default::default(),
        proof: (),
    }
}

/// `public_inputs_len` feeds fee calculation, so it has to agree with
/// `public_inputs().len()` for every opcode, not just for `Noop`.
#[test]
fn public_inputs_len_agrees_with_public_inputs() {
    let program: Vec<VmOp> = vec![
        Op::Noop { n: 0 },
        Op::Lt,
        Op::Eq,
        Op::Type,
        Op::Size,
        Op::New,
        Op::And,
        Op::Or,
        Op::Neg,
        Op::Log,
        Op::Root,
        Op::Pop,
        Op::Popeq {
            cached: false,
            result: 7u64.into(),
        },
        Op::Addi { immediate: 3 },
        Op::Subi { immediate: 4 },
        Op::Push {
            storage: false,
            value: StateValue::Null,
        },
        Op::Push {
            storage: true,
            value: StateValue::Cell(Sp::new(9u8.into())),
        },
        // Nested values: fee calculation now depends on `field_size` matching
        // `field_repr` all the way down, not just for the op discriminant.
        Op::Push {
            storage: false,
            value: StateValue::Array(
                vec![
                    StateValue::Null,
                    StateValue::Cell(Sp::new(1u8.into())),
                    StateValue::Array(vec![StateValue::Cell(Sp::new(2u8.into()))].into()),
                ]
                .into(),
            ),
        },
        Op::Push {
            storage: false,
            value: StateValue::Map(
                storage::storage::HashMap::new()
                    .insert(1u8.into(), StateValue::Cell(Sp::new(11u8.into())))
                    .insert(2u8.into(), StateValue::Null),
            ),
        },
        Op::Branch { skip: 2 },
        Op::Jmp { skip: 1 },
        Op::Add,
        Op::Sub,
        Op::Concat {
            cached: false,
            n: 8,
        },
        Op::Member,
        Op::Rem { cached: true },
        Op::Dup { n: 1 },
        Op::Swap { n: 2 },
        Op::Idx {
            cached: false,
            push_path: false,
            path: vec![Key::Stack, Key::Value(1u8.into())].into(),
        },
        Op::Ins {
            cached: false,
            n: 1,
        },
        Op::Ckpt,
        Op::Noop { n: 100 },
    ];

    // Per-op, so a mismatch names the opcode rather than just the total.
    for op in program.iter() {
        assert_eq!(
            op.field_vec().len(),
            op.field_size(),
            "field_size disagrees with field_repr for {op:?}"
        );
    }

    let call = call_with_program(program);
    assert_eq!(
        call.public_inputs_len(),
        call.public_inputs(Default::default()).unwrap().len()
    );
}

/// The fee path must not allocate proportionally to a declared, unverified `n`.
#[test]
fn public_inputs_len_does_not_materialize() {
    let call = call_with_program(vec![Op::Noop { n: u32::MAX }]);

    // Materializing this would demand 128 GiB.
    let t0 = Instant::now();
    let len = call.public_inputs_len();
    let elapsed = t0.elapsed();

    assert_eq!(len, u32::MAX as usize + 2);
    assert!(
        elapsed < Duration::from_millis(10),
        "counting public inputs took {elapsed:?}; it should be O(#ops)"
    );
}

/// A 9-byte opcode must not be able to name an arbitrarily large statement.
#[test]
fn declared_public_inputs_are_capped() {
    use midnight_ledger_v9::verify::MAX_CALL_PUBLIC_INPUTS;

    let over = call_with_program(vec![Op::Noop {
        n: MAX_CALL_PUBLIC_INPUTS as u32,
    }]);
    assert!(over.public_inputs_len() > MAX_CALL_PUBLIC_INPUTS);
    // `public_inputs` enforces the cap itself rather than trusting its caller.
    assert!(matches!(
        over.public_inputs(Default::default()),
        Err(MalformedTransaction::TooManyPublicInputs { .. })
    ));

    let under = call_with_program(vec![Op::Noop {
        n: MAX_CALL_PUBLIC_INPUTS as u32 - 2,
    }]);
    assert_eq!(under.public_inputs_len(), MAX_CALL_PUBLIC_INPUTS);
    assert_eq!(
        under.public_inputs(Default::default()).unwrap().len(),
        MAX_CALL_PUBLIC_INPUTS
    );
}
