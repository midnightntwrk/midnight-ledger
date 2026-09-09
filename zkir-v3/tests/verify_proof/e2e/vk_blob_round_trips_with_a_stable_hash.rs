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

//! A verifying-key blob must round-trip byte-for-byte, keeping its `vk_hash`
//! stable. The decider tag is part of that identity.

use midnight_zkir_v3::decider::{DeciderKind, deserialize_vk, serialize_vk};
use sha2::Digest;

use crate::e2e_harness::{SingleScalarRelation, inner_setup_for};

fn sha256(bytes: &[u8]) -> Vec<u8> {
    sha2::Sha256::digest(bytes).to_vec()
}

#[actix_rt::test]
#[ignore = "needs an SRS to build a genuine inner verifying key"]
async fn vk_blob_round_trips_with_a_stable_hash() {
    let (_srs, _pk, blob) = inner_setup_for::<SingleScalarRelation>("single-scalar").await;

    // It decodes, and to the kind it was written under.
    let (kind, vk) = deserialize_vk(&blob).expect("a freshly written blob must decode");
    assert_eq!(
        kind,
        DeciderKind::None,
        "the decider tag must survive the round-trip"
    );

    // And re-encoding is byte-identical, which is the property `vk_hash` rests
    // on. Asserted on the bytes rather than the hash so a failure says how the
    // encoding moved, not merely that it did.
    let again = serialize_vk(&vk, kind).expect("re-serialize");
    assert_eq!(
        again, blob,
        "re-encoding a decoded key must reproduce the original bytes"
    );

    // The tag is inside the hashed blob, so re-tagging the same key is a
    // different identity rather than the same one wearing a different label.
    let collapsed = serialize_vk(&vk, DeciderKind::Collapsed).expect("serialize as collapsed");
    assert_ne!(
        sha256(&collapsed),
        sha256(&blob),
        "a key registered under a different decider must not share a vk_hash"
    );
    assert_eq!(
        collapsed.len(),
        blob.len(),
        "the two differ in the tag byte alone, so a length check would not catch a swap"
    );
}
