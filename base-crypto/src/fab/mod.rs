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

//! Defines the primitives of the field-aligned binary representation, where
//! values are represented as sequences of binary strings, that are tied to an
//! alignment which can be used to interpret them either as binary data, or a
//! sequence of field elements for proving.

mod alignments;
mod conversions;
mod encoding;
mod serialize;

pub use alignments::*;
pub use conversions::InvalidBuiltinDecode;
pub use encoding::*;
pub use serialize::*;
