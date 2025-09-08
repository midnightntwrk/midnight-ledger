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

//! Hashing functions for use across Midnight.

use crate::curve::{EmbeddedGroupAffine, outer};
use crate::curve::{FR_BYTES_STORED, Fr, embedded};
use crate::repr::{FieldRepr, FromFieldRepr};
pub use base_crypto::hash::{HashOutput, PERSISTENT_HASH_BYTES};
pub use base_crypto::repr::MemWrite;
use midnight_circuits::ecc::hash_to_curve::HashToCurveGadget;
use midnight_circuits::ecc::native::EccChip;
use midnight_circuits::hash::poseidon::PoseidonChip;
use midnight_circuits::instructions::HashToCurveCPU;
use midnight_circuits::instructions::hash::HashCPU;
use midnight_circuits::types::AssignedNative;

impl FieldRepr for HashOutput {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        self.0.field_repr(writer);
    }
    fn field_size(&self) -> usize {
        self.0.field_size()
    }
}

impl FromFieldRepr for HashOutput {
    const FIELD_SIZE: usize = <[u8; PERSISTENT_HASH_BYTES] as FromFieldRepr>::FIELD_SIZE;
    fn from_field_repr(mut repr: &[Fr]) -> Option<Self> {
        let size = <[u8; PERSISTENT_HASH_BYTES] as FromFieldRepr>::FIELD_SIZE;
        if size > repr.len() {
            return None;
        }
        let field_0 = <[u8; PERSISTENT_HASH_BYTES]>::from_field_repr(&repr[..size])?;
        repr = &repr[size..];
        if repr.is_empty() {
            Some(HashOutput(field_0))
        } else {
            None
        }
    }
}

/// A hash-to-field, transforming arbitrary (binary) data into a single [Fr]
/// element.
pub fn hash_to_field(data: &[u8]) -> Fr {
    let mut preimage = vec![];
    b"midnight:field_hash".field_repr(&mut preimage);
    data.field_repr(&mut preimage);
    transient_hash(&preimage)
}

/// Transforms the output of a [`transient_hash`] to one of [`base_crypto::hash::persistent_hash`].
pub fn upgrade_from_transient(transient: Fr) -> HashOutput {
    let mut res = [0u8; PERSISTENT_HASH_BYTES];
    res[..FR_BYTES_STORED].copy_from_slice(&transient.as_le_bytes()[..FR_BYTES_STORED]);
    HashOutput(res)
}

/// Transforms the output of a [`base_crypto::hash::persistent_hash`] to one of [`transient_hash`].
pub fn degrade_to_transient(persistent: HashOutput) -> Fr {
    persistent.field_vec()[1]
}

/// An efficient hash function that may be changed on hard-forks.
pub fn transient_hash(elems: &[Fr]) -> Fr {
    let h = <PoseidonChip<outer::Scalar> as HashCPU<outer::Scalar, outer::Scalar>>::hash(
        &elems.iter().map(|x| x.0).collect::<Vec<_>>(),
    );
    Fr(h)
}

/// Commits to a value using `transient_hash`.
pub fn transient_commit<T: FieldRepr + ?Sized>(value: &T, opening: Fr) -> Fr {
    let mut preimage = vec![opening];
    value.field_repr(&mut preimage);
    transient_hash(&preimage)
}

/// Hashes a value that can be represented as field elements to the proof system's embedded curve.
pub fn hash_to_curve<T: FieldRepr + ?Sized>(value: &T) -> EmbeddedGroupAffine {
    let preimage = value
        .field_vec()
        .into_iter()
        .map(|f| f.0)
        .collect::<Vec<_>>();
    let point = <HashToCurveGadget<
        outer::Scalar,
        embedded::AffineExtended,
        AssignedNative<outer::Scalar>,
        PoseidonChip<outer::Scalar>,
        EccChip<embedded::AffineExtended>,
    > as HashToCurveCPU<embedded::AffineExtended, outer::Scalar>>::hash_to_curve(
        &preimage
    );
    EmbeddedGroupAffine(point)
}
