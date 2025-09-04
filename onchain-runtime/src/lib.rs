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

#![deny(unreachable_pub)]
#![deny(warnings)]
// Proptest derive triggers this.
#![allow(non_local_definitions)]

#[macro_use]
extern crate tracing;

pub mod context;
pub mod error;
#[rustfmt::skip]
#[path = "../vendored/program_fragments.rs"]
pub mod program_fragments;
pub mod contract_state_ext;
pub mod test_utilities;
pub mod transcript;

pub use onchain_runtime_state::state;

pub use onchain_vm::cost_model;
pub use onchain_vm::error as vm_error;
pub use onchain_vm::ops;
pub use onchain_vm::result_mode;
pub use onchain_vm::state_value_ext;
pub use onchain_vm::vm;
pub use onchain_vm::vm_value;

use base_crypto::fab::AlignedValue;
use transient_crypto::curve::Fr;
use transient_crypto::hash::transient_commit;

pub fn communication_commitment(input: AlignedValue, output: AlignedValue, rand: Fr) -> Fr {
    transient_commit(&AlignedValue::concat([&input, &output]), rand)
}
