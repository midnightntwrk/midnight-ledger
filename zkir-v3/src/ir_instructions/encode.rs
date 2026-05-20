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
///
/// For `IrValue::Opaque { bytes, .. }` the output is the canonical
/// runtime input-vector encoding `[byte_len, preimage_fr_0, ...,
/// preimage_fr_{N-1}]` (each entry wrapped in `IrValue::Native`). This
/// matches what the JS bridge flattens into `ProofPreimage.inputs` for
/// an Opaque-typed input and what `decode_offcircuit(_, &IrType::Opaque)`
/// reconstructs.
pub fn encode_offcircuit(value: &IrValue) -> Vec<IrValue> {
    use transient_crypto::curve::FR_BYTES_STORED;
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
        IrValue::Opaque { bytes, .. } => {
            // Canonical input-vector encoding for an Opaque preimage:  [byte_len, fr_0, ..., fr_{N-1}]
            // where the preimage Frs are packed via the same chunk-and-reverse layout 
            // `AlignmentAtom::Bytes { length: bytes.len() }` uses (see `transient_crypto/src/fab.rs`): 
            // chunks of `FR_BYTES_STORED` bytes, with the last (possibly partial) chunk first.
            let byte_len = bytes.len();
            let mut frs: Vec<F> = Vec::with_capacity(1 + byte_len.div_ceil(FR_BYTES_STORED));
            frs.push(Fr::from(byte_len as u64).0);
            let packed: Vec<F> = bytes
                .chunks(FR_BYTES_STORED)
                .map(|chunk| {
                    Fr::from_le_bytes(chunk)
                        .expect("Opaque preimage chunk must fit into Fr")
                        .0
                })
                .rev()
                .collect();
            frs.extend(packed);
            frs
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
        CircuitValue::Opaque { .. } => {
            return Err(Error::Synthesis(
                "encode_incircuit: CircuitValue::Opaque encoding is not implemented; \
                 the in-circuit projection currently carries only the commit, not \
                 the preimage Frs that the off-circuit `encode_offcircuit` emits."
                    .into(),
            ));
        }
    }?;
    Ok(encoded.into_iter().map(CircuitValue::Native).collect())
}

/// Commit-bearing off-circuit encoding for `IrValue`s, used to derive the
/// communications-commitment preimage on the IR side so it matches the
/// commit-bearing flatten that the JS bridge feeds into
/// `transient_hash` for `comm_comm`.
///
/// For `Native`, `JubjubPoint`, and `JubjubScalar` this is identical to
/// [`encode_offcircuit`] (those types have no preimage/commit duality —
/// their encoding is a single fixed-width sequence of `IrValue::Native`
/// Frs already).
///
/// For [`IrValue::Opaque`] this emits exactly the cached `commit` Fr,
/// matching the runtime's `<ValueAtom as ValueAtomExt>::field_repr_unchecked`
/// `Compress` arm at `transient-crypto/src/fab.rs` (and its
/// empty-AV special case that writes `Fr::from(0u64)`). Because
/// `IrValue::opaque` precomputes `commit` from the same `transient_commit(
/// bytes, Fr::from(byte_len as u64))` formula, the IR-side comm_comm
/// preimage built from these emissions matches the JS bridge's
/// commit-bearing flatten over the same `AlignedValue` Compress slots.
pub fn encode_offcircuit_for_commit(value: &IrValue) -> Vec<IrValue> {
    let encoded = match value {
        IrValue::Native(x) => AssignedNative::<F>::as_public_input(&x.0),
        IrValue::JubjubPoint(p) => AssignedNativePoint::<JubjubExtended>::as_public_input(p),
        IrValue::JubjubScalar(s) => {
            let encoded = AssignedScalarOfNativeCurve::<JubjubExtended>::as_public_input(s);
            // Same canonicality argument as in `encode_offcircuit`.
            assert_eq!(encoded.len(), 1);
            encoded
        }
        IrValue::Opaque { commit, .. } => {
            vec![commit.0]
        }
    };
    encoded.into_iter().map(|s| IrValue::Native(Fr(s))).collect()
}

/// Commit-bearing in-circuit encoding for `CircuitValue`s, the in-circuit
/// counterpart of [`encode_offcircuit_for_commit`]. Used by
/// [`crate::ir_vm::IrSource::circuit`] to build the comm_comm preimage
/// that the SNARK Poseidon-hashes against the public-input commitment.
///
/// For `Native`, `JubjubPoint`, and `JubjubScalar` this is identical to
/// [`encode_incircuit`] (no preimage/commit distinction).
///
/// For [`CircuitValue::Opaque`] this emits the cached `commit`
/// `AssignedNative`, which is the same Fr that the off-circuit
/// `encode_offcircuit_for_commit` pushes for `IrValue::Opaque` — keeping
/// the two preimage builds in lockstep.
pub fn encode_incircuit_for_commit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    value: &CircuitValue,
) -> Result<Vec<CircuitValue>, Error> {
    let encoded = match value {
        CircuitValue::Native(x) => std_lib.as_public_input(layouter, x),
        CircuitValue::JubjubPoint(p) => std_lib.jubjub().as_public_input(layouter, p),
        CircuitValue::JubjubScalar(s) => {
            let encoded = std_lib.jubjub().as_public_input(layouter, s)?;
            assert_eq!(encoded.len(), 1);
            Ok(encoded)
        }
        CircuitValue::Opaque { commit } => {
            // Single AssignedNative: the cached commit. We don't run it
            // through `as_public_input` because there's no PI gating
            // here — the comm_comm is hashed against PI[1] separately
            // by the caller.
            Ok(vec![commit.clone()])
        }
    }?;
    Ok(encoded.into_iter().map(CircuitValue::Native).collect())
}
