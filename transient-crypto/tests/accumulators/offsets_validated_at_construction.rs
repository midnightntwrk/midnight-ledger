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

//! A `VerifierKey` must not be built from an offsets list no real circuit could
//! produce.
//!
//! `from_vk_with_accumulator_offsets` records whatever integers it is handed.
//! Two accumulator regions cannot overlap, and a duplicate offset pairs one
//! region twice while leaving another unpaired — so the inner proof behind the
//! unpaired region is never checked at all. Both are decidable from exactly the
//! arguments the constructor already has.
//!
//! `verify` cannot catch either: both produce a well-formed read of a
//! well-formed region, and it has no way to know a region was meant to be
//! covered. That is what makes this a construction-time property.
//!
//! Distinct from `non_collapsed_accumulator_is_rejected`, which is about a
//! region a *prover* influences. Offsets come from `IrSource::accumulator_offsets()`,
//! so this guards against a faulty key producer instead.
//!
//! The constructor returns `Self`, so rejecting cannot be expressed as an error
//! today; these assertions pin the observable consequence — such a key must not
//! verify. Re-express them against the constructor once it can fail.

use midnight_curves::Fq;

use midnight_transient_crypto::curve::Fr;
use midnight_transient_crypto::proofs::{PARAMS_VERIFIER, VerifierKey};

use crate::harness::{
    acc_len, failing_accumulator, passing_accumulator, raw_proof_exposing, test_rng,
};

/// Proves that the statement is exactly `fields`, records `offsets`, verifies.
fn verify_with_offsets(fields: &[Fq], offsets: &[usize]) -> Result<(), String> {
    let mut rng = test_rng();
    let (raw, proof) = raw_proof_exposing(fields, &mut rng);
    let statement: Vec<Fr> = fields.iter().map(|f| Fr(*f)).collect();
    VerifierKey::from_vk_with_accumulator_offsets(raw, offsets)
        .verify(&PARAMS_VERIFIER, &proof, statement.into_iter())
        .map_err(|e| format!("{e:#}"))
}

#[test]
fn offsets_validated_at_construction() {
    // A good accumulator followed by one that does not pair.
    let good_then_bad: Vec<Fq> = passing_accumulator()
        .into_iter()
        .chain(failing_accumulator())
        .collect();

    // Control: with the offsets a real circuit would record, both regions are
    // paired and the bad one is caught.
    verify_with_offsets(&good_then_bad, &[0, acc_len()])
        .expect_err("honest offsets must pair both regions and catch the bad one");

    // A duplicate offset pairs the first region twice and never looks at the
    // second — so the accumulator that does not pair is simply skipped, and a
    // proof that must be refused is accepted.
    verify_with_offsets(&good_then_bad, &[0, 0]).expect_err(
        "a duplicate offset leaves an accumulator unchecked and must not produce a usable key",
    );

    // Overlapping regions are geometrically impossible for a real circuit.
    // Both accumulators here pair, so nothing but the offsets themselves is
    // wrong — and nothing rejects them.
    let two_good: Vec<Fq> = passing_accumulator()
        .into_iter()
        .chain(passing_accumulator())
        .collect();
    verify_with_offsets(&two_good, &[0, 1])
        .expect_err("overlapping regions must not produce a usable key");
}
