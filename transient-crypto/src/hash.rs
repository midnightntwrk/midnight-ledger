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

//! Hashing functions for use across Midnight.

use crate::curve::{EmbeddedGroupAffine, outer};
use crate::curve::{FR_BYTES_STORED, Fr, embedded};
use crate::repr::{FieldRepr, FromFieldRepr};
pub use base_crypto::hash::{HashOutput, PERSISTENT_HASH_BYTES};
pub use base_crypto::repr::MemWrite;
use midnight_circuits::ecc::hash_to_curve::HashToCurveGadget;
use midnight_circuits::ecc::native::EccChip;
use midnight_circuits::hash::poseidon::PoseidonChip;
use std::fmt;
use std::sync::OnceLock;
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

/// The in-process implementation, and the definition of what the host call must answer.
///
/// Kept as the oracle whatever else exists: an installed implementation is only ever as
/// trustworthy as the thing it is checked against, and this is that thing.
fn transient_hash_cpu(elems: &[Fr]) -> Fr {
    let h = <PoseidonChip<outer::Scalar> as HashCPU<outer::Scalar, outer::Scalar>>::hash(
        &elems.iter().map(|x| x.0).collect::<Vec<_>>(),
    );
    Fr(h)
}

/// The type of an embedder-supplied [`transient_hash`].
pub type TransientHashFn = fn(&[Fr]) -> Fr;

static TRANSIENT_HASH: OnceLock<TransientHashFn> = OnceLock::new();

/// Returned by [`set_transient_hash`] when an implementation is already installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransientHashAlreadySet;

impl fmt::Display for TransientHashAlreadySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a transient-hash implementation is already installed")
    }
}

impl std::error::Error for TransientHashAlreadySet {}

/// Install the [`transient_hash`] implementation, replacing the in-process one.
///
/// This exists because the cost of this hash is wildly uneven across the environments the
/// ledger runs in. Some embedders can compute a ᴘᴏsᴇɪᴅᴏɴ permutation far more cheaply than a
/// generic execution of [`transient_hash_reference`] — with dedicated hardware, a vectorised
/// routine, or by stepping outside a metered interpreter entirely. This crate has no way to
/// know which, and no business guessing: it defines *what* the hash is, and lets whoever
/// embeds it say *how* it is computed.
///
/// The obligation is exact agreement. An implementation that differs from
/// [`transient_hash_reference`] on any input does not compute a faster hash, it computes a
/// different ledger — every commitment, nullifier and Merkle root downstream diverges. Check
/// yours against the reference over the shapes you will see, and keep doing so.
///
/// # Ordering
///
/// Install before hashing anything. The first call to [`transient_hash`] settles which
/// implementation that process uses, and a later `set_transient_hash` cannot retract values
/// already computed — hence `Err(TransientHashAlreadySet)` rather than a silent swap, which
/// would let one process hash two ways and agree with itself about neither.
///
/// ```
/// # use midnight_transient_crypto::hash::{set_transient_hash, transient_hash_reference};
/// // An embedder that has a faster route to the same answer installs it here.
/// set_transient_hash(transient_hash_reference).expect("nothing installed yet");
/// ```
pub fn set_transient_hash(f: TransientHashFn) -> Result<(), TransientHashAlreadySet> {
    TRANSIENT_HASH.set(f).map_err(|_| TransientHashAlreadySet)
}

/// The in-process implementation, and the definition of what any installed one must answer.
///
/// Public so an embedder can check its own against it. A hash is only ever as trustworthy as
/// the thing it is checked against, and this is that thing.
pub fn transient_hash_reference(elems: &[Fr]) -> Fr {
    transient_hash_cpu(elems)
}

/// An efficient hash function that may be changed on hard-forks.
///
/// Uses the implementation an embedder installed with [`set_transient_hash`], or
/// [`transient_hash_reference`] when none was.
pub fn transient_hash(elems: &[Fr]) -> Fr {
    match TRANSIENT_HASH.get() {
        Some(f) => f(elems),
        None => transient_hash_cpu(elems),
    }
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

#[cfg(test)]
mod transient_hash_seam_tests {
    use super::*;

    /// The seam's whole contract, in one test: an installed implementation must answer exactly
    /// what the reference does, and it may not be swapped once hashing has begun.
    ///
    /// Both halves matter. An implementation that disagrees does not compute a faster hash, it
    /// computes a different ledger — so the reference stays public and this is what an embedder
    /// is expected to do with it. And a second install would let one process hash two ways and
    /// agree with itself about neither, so it is refused rather than accepted quietly.
    #[test]
    fn an_installed_hash_answers_the_reference_and_cannot_be_swapped() {
        let xs: Vec<Fr> = (0u64..5).map(Fr::from).collect();
        let before = transient_hash(&xs);
        assert_eq!(before, transient_hash_reference(&xs));

        set_transient_hash(transient_hash_reference).expect("nothing installed yet");
        assert_eq!(transient_hash(&xs), before, "installing must not change the answer");

        assert_eq!(
            set_transient_hash(transient_hash_reference),
            Err(TransientHashAlreadySet),
            "a second install must be refused, not applied"
        );
    }
}
