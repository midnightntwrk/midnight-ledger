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

//! Integration tests for the typed `outputs` field and `Output` terminator
//! instruction added to ZKIR v3.
//!
//! Layout:
//!   - `common`: shared `TestResolver`, `TestParams`, and the assertion
//!     helpers each test calls. Lives at `tests/common/mod.rs` so it can
//!     also be imported by `tests/proofs.rs` (Cargo's standard convention
//!     for sharing code between integration tests).
//!   - `roundtrip`: positive tests that prove and verify a small circuit
//!     for each `IrType` (Native, JubjubPoint, JubjubScalar) and a
//!     multi-output case.
//!   - `conformance`: negative tests that exercise the per-position
//!     arity and type checks performed by the `Output` arm in
//!     `IrSource::preprocess`.
//!
//! The `roundtrip` and `conformance` files live under `tests/typed_outputs/`
//! and are pulled in via explicit `#[path]` attributes; this avoids
//! depending on the implicit crate-root submodule resolution rules and
//! keeps related tests grouped on disk.

#[path = "common/mod.rs"]
mod common;

#[path = "typed_outputs/conformance.rs"]
mod conformance;

#[path = "typed_outputs/roundtrip.rs"]
mod roundtrip;
