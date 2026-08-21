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

//! Tests for the verify side of inner-proof accumulators: reconstructing them
//! from a proof's public inputs at the offsets the `VerifierKey` records, and
//! running the deferred pairing.
//!
//! One case per file under `tests/accumulators/`, pulled in with `#[path]`.
//! Each file's doc comment says what it covers.
//!
//! These need SRS params — point `MIDNIGHT_PP` at a directory of
//! `bls_midnight_2p<k>` files, or rely on `~/.cache/midnight/zk-params`. The
//! circuits are small, so unlike the ZKIR e2e suite they are not `#[ignore]`d.

#[path = "accumulators/harness.rs"]
mod harness;

#[path = "accumulators/accumulators_verify_one_and_multiple.rs"]
mod accumulators_verify_one_and_multiple;

#[path = "accumulators/vk_round_trip_preserves_offsets.rs"]
mod vk_round_trip_preserves_offsets;

#[path = "accumulators/vk_bytes_stable_across_init.rs"]
mod vk_bytes_stable_across_init;

#[path = "accumulators/bad_accumulator_in_public_inputs_is_rejected.rs"]
mod bad_accumulator_in_public_inputs_is_rejected;

#[path = "accumulators/accumulator_at_end_of_pi_vector.rs"]
mod accumulator_at_end_of_pi_vector;

#[path = "accumulators/batch_verify_accepts_valid_batch.rs"]
mod batch_verify_accepts_valid_batch;

#[path = "accumulators/batch_verify_rejects_bad_accumulator.rs"]
mod batch_verify_rejects_bad_accumulator;

#[path = "accumulators/verifier_key_backward_compat.rs"]
mod verifier_key_backward_compat;

#[path = "accumulators/proof_preimage_backward_compat.rs"]
mod proof_preimage_backward_compat;

#[path = "accumulators/non_collapsed_accumulator_is_rejected.rs"]
mod non_collapsed_accumulator_is_rejected;

#[path = "accumulators/offsets_validated_at_construction.rs"]
mod offsets_validated_at_construction;
