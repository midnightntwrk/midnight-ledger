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

use midnight_circuits::instructions::AssignmentInstructions;
use midnight_curves::{Fr as JubjubFr, JubjubSubgroup};
use midnight_proofs::{
    circuit::{Layouter, Value},
    plonk::Error,
};
use midnight_zk_stdlib::ZkStdLib;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrType, IrValue},
};

/// Initializes fresh in-circuit (potentially secret) values of the given type.
/// The prover is allowed to fill these values freely, but is constrained to
/// respect the type.
///
/// # Error
///
/// This function returns an error if one of the provided values is not of the
/// declared type `t`.
pub fn assign_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    t: &IrType,
    values: &[Value<IrValue>],
) -> Result<Vec<CircuitValue>, Error> {
    fn convert_values<T: TryFrom<IrValue>>(
        values: &[Value<IrValue>],
    ) -> Result<Vec<Value<T>>, Error>
    where
        T::Error: std::fmt::Display,
    {
        values
            .iter()
            .map(|v| {
                v.as_ref().map_with_result(|x| {
                    x.clone()
                        .try_into()
                        .map_err(|e| Error::Synthesis(format!("{}", e)))
                })
            })
            .collect()
    }

    match t {
        IrType::Native => {
            let fr_values = convert_values::<Fr>(values)?;
            let field_values: Vec<Value<_>> =
                fr_values.into_iter().map(|v| v.map(|fr| fr.0)).collect();
            std_lib
                .assign_many(layouter, &field_values)
                .map(|xs| xs.into_iter().map(CircuitValue::Native).collect())
        }

        IrType::JubjubPoint => std_lib
            .jubjub()
            .assign_many(layouter, &convert_values::<JubjubSubgroup>(values)?)
            .map(|xs| xs.into_iter().map(CircuitValue::JubjubPoint).collect()),

        IrType::JubjubScalar => std_lib
            .jubjub()
            .assign_many(layouter, &convert_values::<JubjubFr>(values)?)
            .map(|xs| xs.into_iter().map(CircuitValue::JubjubScalar).collect()),
    }
}
