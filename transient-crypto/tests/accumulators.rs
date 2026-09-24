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

//! Inner-proof accumulators through `verify` and `batch_verify`, end to end.
//! Their serialization is tested in `src/proofs.rs`.
//!
//! One case per file under `tests/accumulators/`.
//!
//! Needs SRS params from `$MIDNIGHT_PP`, or `~/.cache/midnight/zk-params`. The
//! circuits are small, so nothing here is `#[ignore]`d.

#[path = "accumulators/batch_verify_accepts_proofs_with_and_without_accumulators.rs"]
mod batch_verify_accepts_proofs_with_and_without_accumulators;

#[path = "accumulators/batch_verify_rejects_bad_accumulator.rs"]
mod batch_verify_rejects_bad_accumulator;

#[path = "accumulators/each_accumulator_block_is_paired.rs"]
mod each_accumulator_block_is_paired;

#[path = "accumulators/harness.rs"]
mod harness;
