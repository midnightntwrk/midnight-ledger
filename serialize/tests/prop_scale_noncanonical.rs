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

//! `ScaleBigInt` marker-class minimality regression.
//!
//! The decoder rejects trailing-zero non-minimality WITHIN each marker class,
//! but never checks that the marker CLASS itself is minimal. The N-byte marker
//! path (`first & 0b11 == 0b11`) computes `n = top6bits(first) + 4` and reads
//! `n` raw bytes, checking only that the top byte is non-zero. With
//! `top6bits(first) == 0` we get `n == 4`: a 4-byte value encoded with the
//! N-byte marker is accepted, even though the same value also has a (distinct)
//! 4-byte-marker encoding. Two wire forms, one integer -- affecting every integer
//! and field element (via `ScaleBigInt`).
//!
//! The correct behaviour asserted below is that the decoder REJECTS the
//! non-minimal marker form. Because `ScaleBigInt` backs every integer and
//! length-prefix decoder, this alias would otherwise apply to every integer on
//! the wire whose top significant byte is in `1..=63`.

use midnight_serialize::{Deserializable, ScaleBigInt, Serializable};

#[test]
fn scale_bigint_rejects_non_minimal_marker_class() {
    // A value whose most-significant significant byte is the 4th (needs 4 bytes).
    let mut a = [0u8; 67];
    a[3] = 1; // value = 0x0100_0000
    let x = ScaleBigInt(a);

    // Canonical encoding chooses the 4-byte marker.
    let mut canonical = Vec::new();
    x.serialize(&mut canonical).expect("serialize");

    // Hand-crafted N-byte-marker encoding with n = 4 (top6bits(first)=0):
    //   first byte = 0b11 (N-byte marker), then 4 raw LE bytes [0,0,0,1].
    // A distinct wire form of the SAME value.
    let n_byte = vec![0b0000_0011u8, 0, 0, 0, 1];
    assert_ne!(
        canonical, n_byte,
        "the two encodings are genuinely different wire forms"
    );

    // The canonical form must decode; the non-minimal marker form must be REJECTED.
    assert!(
        ScaleBigInt::deserialize(&mut &canonical[..], 0).is_ok(),
        "the canonical 4-byte-marker form must decode"
    );
    assert!(
        ScaleBigInt::deserialize(&mut &n_byte[..], 0).is_err(),
        "SEVERE-primitive: the decoder must reject the non-minimal N-byte(n=4) marker \
         (else two distinct wire forms decode to the same integer)"
    );
}
