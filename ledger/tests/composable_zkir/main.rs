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

//! End-to-end tests for dynamic cross-contract composability using ZKIR v3.
//!
//! Pipeline: deploy → IrSource::execute → pre_transcripts →
//!           PreTranscript conversion → partition_transcripts →
//!           ContractCallPrototype → Intent → Transaction → well_formed → apply
//!
//! These tests bridge the ZKIR interpreter's `ExecutionResult` (from `zkir-v3`)
//! with the ledger crate's transaction construction and verification
//! infrastructure. They validate that:
//!
//! 1. Execution results from `IrSource::execute` can be converted to
//!    `PreTranscript` objects (structurally identical types).
//! 2. `partition_transcripts` correctly reconstructs the call forest from
//!    the claim ops emitted during execution.
//! 3. `ContractCallPrototype` objects can be built from the partition results
//!    and execution metadata.
//! 4. The full transaction pipeline (deploy → execute → partition →
//!    intent construction → transaction → well_formed → state application)
//!    succeeds end-to-end for cross-contract calls.

mod common;
mod e2e_pipeline;
mod linkage;
mod nontrivial_results;
mod pipeline;
#[cfg(feature = "proving")]
mod proving;
mod rejection;
