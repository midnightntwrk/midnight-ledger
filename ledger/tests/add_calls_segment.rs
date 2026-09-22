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

//! Segment selection in [`StandardTransaction::add_calls`].
//!
//! Segment 0 is the guaranteed segment and may never be claimed by an intent:
//! `wellFormed` rejects such a transaction with
//! `MalformedTransaction::IllegallyDeclaredGuaranteed`. The `Random` and
//! `GuaranteedOnly` specifiers pick their segment from `rng`, so that draw must
//! be constrained to the documented `1..=u16::MAX` range.

use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use midnight_ledger::construct::SegmentSpecifier;
use midnight_ledger::structure::{INITIAL_PARAMETERS, ProofPreimageMarker, StandardTransaction};
use rand::{CryptoRng, RngCore};
use storage::db::InMemoryDB;
use transient_crypto::commitment::PedersenRandomness;
use transient_crypto::proofs::ProofPreimage;

/// An RNG that only ever yields zero bytes.
///
/// This pins the low end of the segment draw deterministically: the previous
/// implementation called `rng.gen()` over the full `u16` range and so returned
/// the illegal segment 0 for this RNG, whereas a correctly bounded draw cannot.
struct ZeroRng;

impl RngCore for ZeroRng {
    fn next_u32(&mut self) -> u32 {
        0
    }

    fn next_u64(&mut self) -> u64 {
        0
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.fill(0);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

// Not a claim about `ZeroRng`; `add_calls` requires the bound.
impl CryptoRng for ZeroRng {}

type Tx = StandardTransaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>;

fn empty_tx() -> Tx {
    StandardTransaction::new(
        "local-test",
        storage::storage::HashMap::new(),
        None,
        storage::storage::HashMap::new(),
    )
}

/// The segment `add_calls` selected, given no calls to partition.
fn segment_for(specifier: SegmentSpecifier) -> u16 {
    let tx = empty_tx()
        .add_calls::<ProofPreimage>(
            &mut ZeroRng,
            specifier,
            &[],
            &INITIAL_PARAMETERS,
            Timestamp::from_secs(0),
            &[],
            &[],
            &[],
        )
        .expect("add_calls should succeed for an empty call list");
    let segments: Vec<u16> = tx.intents.keys().collect();
    assert_eq!(
        segments.len(),
        1,
        "expected exactly one intent, got segments {segments:?}"
    );
    segments[0]
}

#[test]
fn random_segment_is_never_zero() {
    let segment = segment_for(SegmentSpecifier::Random);
    assert_ne!(
        segment, 0,
        "`Random` claimed segment 0, which `wellFormed` rejects as \
         IllegallyDeclaredGuaranteed"
    );
}

#[test]
fn guaranteed_only_segment_is_never_zero() {
    let segment = segment_for(SegmentSpecifier::GuaranteedOnly);
    assert_ne!(
        segment, 0,
        "`GuaranteedOnly` claimed segment 0, which `wellFormed` rejects as \
         IllegallyDeclaredGuaranteed"
    );
}

#[test]
fn first_segment_is_one() {
    assert_eq!(segment_for(SegmentSpecifier::First), 1);
}

#[test]
fn specific_segment_zero_is_rejected() {
    let err = empty_tx()
        .add_calls::<ProofPreimage>(
            &mut ZeroRng,
            SegmentSpecifier::Specific(0),
            &[],
            &INITIAL_PARAMETERS,
            Timestamp::from_secs(0),
            &[],
            &[],
            &[],
        )
        .expect_err("segment 0 must not be constructible");
    assert!(
        matches!(
            err,
            midnight_ledger::error::PartitionFailure::IllegalSegmentZero
        ),
        "unexpected failure: {err:?}"
    );
}
