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

//! Tests for the `inner_proof` / `verify_proof` instruction pair: one binds a
//! prover-supplied inner proof to a name, the other verifies it in-circuit
//! against a verifying key resolved by hash.
//!
//! Each case lives in its own file under `tests/verify_proof/`, named for what
//! it asserts, and pulled in with `#[path]` — the convention
//! `tests/typed_outputs.rs` uses. Modules stay flat: `#[path]` already carries
//! the directory, and nesting would only lengthen test paths. Each file's own
//! doc comment says what it covers and why.
//!
//! # `unit/` — IR and VM logic, checked directly
//!
//! No SRS and no proving, so these run in CI in milliseconds:
//!
//! ```text
//! cargo test -p midnight-zkir-v3 --test verify_proof
//! ```
//!
//! # `e2e/` — a real inner proof, carried through to the deferred pairing
//!
//! Built, verified in-circuit, and paired. These need SRS params at a high `k`
//! (point `MIDNIGHT_PP` at a directory of `bls_midnight_2p<k>` files, or rely on
//! `~/.cache/midnight/zk-params`) and a couple of minutes of keygen each, so
//! they are all `#[ignore]`d:
//!
//! ```text
//! MIDNIGHT_PP=<dir> cargo test -p midnight-zkir-v3 --release \
//!     --test verify_proof -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `--release` matters: a debug-build keygen takes long enough to look hung.

#[path = "verify_proof/unit/text_format_round_trips.rs"]
mod text_format_round_trips;

#[path = "verify_proof/unit/ir_round_trip_preserves_vks_in_order.rs"]
mod ir_round_trip_preserves_vks_in_order;

#[path = "verify_proof/unit/missing_witness_or_vk_is_rejected.rs"]
mod missing_witness_or_vk_is_rejected;

#[path = "verify_proof/unit/surplus_witness_or_vk_is_rejected.rs"]
mod surplus_witness_or_vk_is_rejected;

#[path = "verify_proof/unit/duplicate_vk_in_side_table_is_rejected.rs"]
mod duplicate_vk_in_side_table_is_rejected;

#[path = "verify_proof/unit/duplicate_inner_proof_binding_is_rejected.rs"]
mod duplicate_inner_proof_binding_is_rejected;

#[path = "verify_proof/unit/unsynthesizable_circuit_is_reported_gracefully.rs"]
mod unsynthesizable_circuit_is_reported_gracefully;

#[path = "verify_proof/unit/unbound_proof_name_is_rejected.rs"]
mod unbound_proof_name_is_rejected;

#[path = "verify_proof/unit/vk_hash_mismatch_is_rejected.rs"]
mod vk_hash_mismatch_is_rejected;

#[path = "verify_proof/unit/harness.rs"]
mod unit_harness;

#[path = "verify_proof/e2e/accumulator_on_proof_matches_offcircuit.rs"]
mod accumulator_on_proof_matches_offcircuit;

#[path = "verify_proof/e2e/blake2b_transcript_proof_is_rejected.rs"]
mod blake2b_transcript_proof_is_rejected;

#[path = "verify_proof/e2e/corrupted_proof_is_rejected.rs"]
mod corrupted_proof_is_rejected;

#[path = "verify_proof/e2e/malformed_proof_witness_is_rejected.rs"]
mod malformed_proof_witness_is_rejected;

#[path = "verify_proof/e2e/proof_from_another_vk_is_rejected.rs"]
mod proof_from_another_vk_is_rejected;

#[path = "verify_proof/e2e/two_proofs_with_distinct_vks_are_accepted.rs"]
mod two_proofs_with_distinct_vks_are_accepted;

#[path = "verify_proof/e2e/impact_between_two_proofs_leaves_accumulators_intact.rs"]
mod impact_between_two_proofs_leaves_accumulators_intact;

#[path = "verify_proof/e2e/valid_proof_of_other_statement_is_rejected.rs"]
mod valid_proof_of_other_statement_is_rejected;

#[path = "verify_proof/e2e/same_vk_verified_twice_is_accepted.rs"]
mod same_vk_verified_twice_is_accepted;

#[path = "verify_proof/e2e/empty_instance_is_handled_gracefully.rs"]
mod empty_instance_is_handled_gracefully;

#[path = "verify_proof/e2e/harness.rs"]
mod e2e_harness;
