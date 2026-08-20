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

//! A `verify_proof` with an empty `instance` is handled, not fatal.
//!
//! The IR parses and the key resolves, then `ir.k()` aborts the process inside
//! the verifier gadget's `inner_product`, which rejects an empty input — on a
//! size query, before keygen, so no caller can handle it. A non-empty but
//! wrong-length instance is already fine: it keygens, proves, and is refused by
//! the pairing, as `valid_proof_of_other_statement_is_rejected` describes.
//!
//! Whether an empty instance should be *rejected* as invalid IR or should *work*
//! — a relation with no public inputs is meaningful — is unresolved, so this
//! asserts only what both answers share: no panic. Tighten it once decided.
//! `unsynthesizable_circuit_is_reported_gracefully` is the same missing error
//! path by another route.

use transient_crypto::proofs::Zkir;

use crate::e2e_harness::{outer_ir_for, scalar_inner_proof, test_rng};

#[actix_rt::test]
#[ignore = "needs an SRS to build a genuine inner verifying key"]
async fn empty_instance_is_handled_gracefully() {
    let mut rng = test_rng();
    let inner = scalar_inner_proof(&mut rng).await;

    // The same key and proof every other case uses; only the instance is empty.
    let ir = outer_ir_for(&inner.vk_blob, &[]);

    let k = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ir.k()));
    assert!(
        k.is_ok(),
        "an empty instance must be handled — rejected or supported — not panic"
    );
}
