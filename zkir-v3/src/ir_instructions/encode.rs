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

use midnight_circuits::{
    instructions::PublicInputInstructions,
    types::{AssignedNative, AssignedNativePoint, AssignedScalarOfNativeCurve, Instantiable},
};
use midnight_curves::JubjubExtended;
use midnight_proofs::{circuit::Layouter, plonk::Error};
use midnight_zk_stdlib::ZkStdLib;
use transient_crypto::curve::Fr;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Encodes the given off-circuit value as a vector of IrValue::Native.
pub fn encode_offcircuit(value: &IrValue) -> Vec<IrValue> {
    let encoded = match value {
        IrValue::Native(x) => AssignedNative::<F>::as_public_input(&x.0),
        IrValue::JubjubPoint(p) => AssignedNativePoint::<JubjubExtended>::as_public_input(p),
        IrValue::JubjubScalar(s) => {
            let encoded = AssignedScalarOfNativeCurve::<JubjubExtended>::as_public_input(s);
            // In ZKIRv3, an assigned scalar can only originate from:
            //   (i)  a circuit input, or
            //   (ii) a `decode` instruction.
            //
            // Circuit inputs yield canonical assigned scalars (whose internal
            // representation uses at most 252 bits). The `decode` path is carefully
            // implemented in [crate::ir_instructions::decode::decode_incircuit] to
            // also produce canonical assigned scalars.
            assert_eq!(encoded.len(), 1);
            encoded
        }
    };
    encoded
        .into_iter()
        .map(|s| IrValue::Native(Fr(s)))
        .collect()
}

/// Encodes the given in-circuit value as a vector of CircuitValue::Native.
pub fn encode_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<Vec<CircuitValue>, Error> {
    let encoded = match value {
        CircuitValue::Native(x) => std_lib.as_public_input(layouter, x),
        CircuitValue::JubjubPoint(p) => std_lib.jubjub().as_public_input(layouter, p),
        CircuitValue::JubjubScalar(s) => {
            let encoded = std_lib.jubjub().as_public_input(layouter, s)?;
            // In ZKIRv3, an assigned scalar can only originate from:
            //   (i)  a circuit input, or
            //   (ii) a `decode` instruction.
            //
            // Circuit inputs yield canonical assigned scalars (whose internal
            // representation uses at most 252 bits). The `decode` path is carefully
            // implemented in [crate::ir_instructions::decode::decode_incircuit] to
            // also produce canonical assigned scalars.
            assert_eq!(encoded.len(), 1);
            Ok(encoded)
        }
    }?;
    Ok(encoded.into_iter().map(CircuitValue::Native).collect())
}
