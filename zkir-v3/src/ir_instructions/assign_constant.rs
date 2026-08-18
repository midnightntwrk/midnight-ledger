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

use midnight_circuits::instructions::AssignmentInstructions;
use midnight_proofs::{circuit::Layouter, plonk::Error};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Assigns a fixed (constant) in-circuit value from a concrete [`IrValue`].
///
/// Unlike [`assign_incircuit`](crate::ir_instructions::assign::assign_incircuit),
/// which assigns (possibly secret) witness values supplied by the prover, this
/// bakes `value` into the circuit as a constant via `assign_fixed`. The value is
/// known at keygen time and is therefore part of the circuit definition.
///
/// Supported on every value type.
pub fn assign_constant_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &IrValue,
) -> Result<CircuitValue, Error> {
    match value {
        IrValue::Native(x) => std_lib.assign_fixed(layouter, x.0).map(CircuitValue::Native),

        IrValue::Bool(b) => std_lib.assign_fixed(layouter, *b).map(CircuitValue::Bool),

        IrValue::Byte(b) => std_lib.assign_fixed(layouter, *b).map(CircuitValue::Byte),

        IrValue::Bytes(bs) => std_lib.assign_many_fixed(layouter, bs).map(CircuitValue::Bytes),

        IrValue::JubjubPoint(p) => std_lib
            .jubjub()
            .assign_fixed(layouter, *p)
            .map(CircuitValue::JubjubPoint),

        IrValue::JubjubScalar(s) => std_lib
            .jubjub()
            .assign_fixed(layouter, *s)
            .map(CircuitValue::JubjubScalar),

        IrValue::Secp256k1Point(p) => std_lib
            .secp256k1()
            .assign_fixed(layouter, *p)
            .map(CircuitValue::Secp256k1Point),

        IrValue::Secp256k1Base(s) => (std_lib.secp256k1().base_field_chip())
            .assign_fixed(layouter, *s)
            .map(CircuitValue::Secp256k1Base),

        IrValue::Secp256k1Scalar(s) => (std_lib.secp256k1().scalar_field_chip())
            .assign_fixed(layouter, *s)
            .map(CircuitValue::Secp256k1Scalar),
    }
}
