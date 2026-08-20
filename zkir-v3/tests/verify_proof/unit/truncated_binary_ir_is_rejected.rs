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

//! A binary IR missing its trailing `verify_proof_vks` encoding is rejected, not
//! misparsed.
//!
//! The field was added to `IrSource` without bumping the
//! `ir-source[v3-generic]` tag, so a payload written before it exists carries a
//! tag the reader accepts over a layout it cannot read. JSON is safe — the field
//! is `#[serde(default)]` — the binary form has no such tolerance.
//!
//! This does not make old payloads loadable; it pins that they fail *loudly*.
//! Silently deserializing a short payload into a different circuit would be far
//! worse. Several truncation widths are tried so the test does not depend on how
//! wide the length prefix is.

use midnight_zkir_v3::IrSource;
use serialize::{tagged_deserialize, tagged_serialize};

use crate::unit_harness::{BIND_ONE, ir};

#[test]
fn truncated_binary_ir_is_rejected() {
    let source = ir(BIND_ONE);
    let mut bytes = Vec::new();
    tagged_serialize(&source, &mut bytes).expect("serializes");

    // Control: intact, it round-trips.
    let back: IrSource = tagged_deserialize(&bytes[..]).expect("intact payload parses");
    assert_eq!(back, source);

    for strip in 1..=4 {
        let truncated = &bytes[..bytes.len() - strip];
        let parsed = tagged_deserialize::<IrSource>(truncated);
        assert!(
            parsed.is_err(),
            "a payload {strip} byte(s) short must be refused, not read as {:?}",
            parsed.ok()
        );
    }
}
