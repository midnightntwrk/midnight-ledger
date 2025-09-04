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

mod deserializable;
mod serializable;
mod tagged;
mod util;

pub use crate::deserializable::{Deserializable, RECURSION_LIMIT, tagged_deserialize};
pub use crate::serializable::{GLOBAL_TAG, Serializable, tagged_serialize, tagged_serialized_size};
pub use crate::tagged::Tagged;
#[cfg(feature = "proptest")]
pub use crate::util::{NoSearch, NoStrategy};
pub use crate::util::{
    ReadExt, ScaleBigInt, VecExt, gen_static_serialize_file, test_file_deserialize,
};
pub use macros::Serializable;
