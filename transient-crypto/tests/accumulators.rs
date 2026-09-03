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

//! Tests for the verify side of inner-proof accumulators: rebuilding the blocks
//! a proof carries, running their deferred pairing, and the serialization of the
//! types that changed to carry them.
//!
//! One case per file under `tests/accumulators/`, pulled in with `#[path]`.
//! Each file's doc comment says what it covers.
//!
//! These need SRS params — point `MIDNIGHT_PP` at a directory of
//! `bls_midnight_2p<k>` files, or rely on `~/.cache/midnight/zk-params`. The
//! circuits are small, so unlike the ZKIR e2e suite they are not `#[ignore]`d.

#[path = "accumulators/bad_accumulator_block_is_rejected.rs"]
mod bad_accumulator_block_is_rejected;

#[path = "accumulators/batch_verify_accepts_proofs_with_and_without_accumulators.rs"]
mod batch_verify_accepts_proofs_with_and_without_accumulators;

#[path = "accumulators/batch_verify_rejects_bad_accumulator.rs"]
mod batch_verify_rejects_bad_accumulator;

#[path = "accumulators/each_accumulator_block_is_paired.rs"]
mod each_accumulator_block_is_paired;

#[path = "accumulators/proof_preimage_round_trip_preserves_witnesses.rs"]
mod proof_preimage_round_trip_preserves_witnesses;

#[path = "accumulators/proof_round_trip_preserves_accumulators.rs"]
mod proof_round_trip_preserves_accumulators;

#[path = "accumulators/harness.rs"]
mod harness;
