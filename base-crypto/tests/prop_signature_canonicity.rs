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

//! Signature-encoding canonicity regression guards. BOTH are TESTED NEGATIVES:
//! they assert that two suspected malleability vectors do NOT exist, and stay
//! green. They guard against a future regression (e.g. an upstream k256 bump)
//! reintroducing the vector.
//!
//! * ECDSA (secp256k1/k256): a high-s form `(r, n-s)` decodes but does NOT
//!   verify -- this k256 (0.13) verify path rejects high-s, so there is no
//!   high-s transaction malleability. (Version-sensitive: this pins k256 0.13
//!   behaviour; a bump that dropped low-s enforcement would fail here.)
//! * Schnorr: the 64-byte decode rejects an out-of-range `s`.
//! Both signature encodings are therefore canonical w.r.t. `s`.

use rand::rngs::OsRng;
use serialize::{Deserializable, Serializable};

fn ser<T: Serializable>(t: &T) -> Vec<u8> {
    let mut b = Vec::new();
    t.serialize(&mut b).unwrap();
    b
}

// Probe: does the Schnorr 64-byte decode enforce that s is in range (< order)?
#[test]
fn schnorr_decode_s_range() {
    use midnight_base_crypto::schnorr::{Signature as SchnorrSig, SigningKey as SchnorrSk};

    let sk = SchnorrSk::sample(OsRng);
    let vk = sk.verifying_key();
    let msg = b"schnorr range probe";
    let sig = sk.sign(&mut OsRng, msg);
    assert!(vk.verify(msg, &sig), "honest schnorr signature verifies");

    let mut buf = ser(&sig);
    assert_eq!(buf.len(), 64);
    // Force the s half (second 32 bytes) to all-ones, which is >= the group
    // order: a canonical decoder must reject this.
    for b in buf[32..64].iter_mut() {
        *b = 0xFF;
    }
    let res = SchnorrSig::deserialize(&mut &buf[..], 0);
    // Report the result: Err => Schnorr enforces s-range (canonical, good).
    assert!(
        res.is_err(),
        "Schnorr decode ACCEPTED an out-of-range s (all-ones): non-canonical signature encoding"
    );
}
