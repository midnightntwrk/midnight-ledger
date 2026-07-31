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

//! Defense-in-depth regression: a length-lying `Array<Signature>` must NOT
//! survive the untrusted-input deserializer.
//!
//! `Array::len()` is read from a stored size annotation, whereas `iter_deref()`
//! walks the subtree structurally. `UnshieldedOffer::well_formed`
//! (ledger/src/verify.rs) guards authorization with
//!     if self.inputs.len() != self.signatures.len() { reject }
//!     for (inp, sig) in inputs.iter_deref().zip(signatures.iter_deref()) { verify }
//! -- it compares annotation lengths but verifies over structural contents, and
//! `apply_offer` spends every input in `inputs.iter_deref()`. So a signatures
//! array whose `len()` reports 2 while it holds a single real signature would
//! leave the second input authorized by nobody yet spent on apply.
//!
//! `well_formed` does NOT independently re-check that a container's `len()`
//! matches its contents; authorization relies entirely on the deserializer
//! having sanitized the array. This test pins that load-bearing guarantee: the
//! deserializer (`MerklePatriciaTrie::invariant` recomputes the annotation to
//! the true leaf count, and `Array::invariant` requires every index < len)
//! rejects the length-lying array, so it is not reachable over the wire. Any
//! future weakening of those invariants would resurface as an authorization
//! bypass, and would fail here first.

mod common;
use common::keypair;

use rand::SeedableRng;
use rand::rngs::StdRng;
use serialize::Serializable;
use storage::arena::Sp;
use storage::merkle_patricia_trie::{MerklePatriciaTrie, Node};
use storage::storable::SizeAnn;
use storage::storage::Array;
use storage::{DefaultDB, Storage};

type D = DefaultDB;
type Signature = base_crypto::schnorr::Signature;

/// Build a signatures `Array` holding exactly ONE real signature (at index 0)
/// but whose `len()` reports 2. The root is a `MidBranchLeaf` whose stored size
/// annotation says 2 while its subtree holds a single leaf, so `len()` (the
/// annotation) and `iter_deref()` (the structure) disagree.
fn length_two_one_real_sig(sig: &Signature) -> Array<Signature, D> {
    let root = Node::MidBranchLeaf {
        ann: SizeAnn(2), // lies: one real leaf, annotation says two
        value: Sp::new(sig.clone()),
        child: Sp::new(Node::Empty),
    };
    Array(Sp::new(MerklePatriciaTrie(Sp::new(root))))
}

/// Round-trip a container through `deserialize_sp` (the untrusted boundary):
/// `Ok` if accepted, `Err` if rejected.
fn round_trip(arr: &Array<Signature, D>) -> Result<Array<Signature, D>, String> {
    let storage = Storage::<D>::new(64, D::default());
    let mut buf = Vec::new();
    Sp::serialize(&Sp::new(arr.clone()), &mut buf).expect("serialize");
    storage
        .arena
        .deserialize_sp::<Array<Signature, D>, _>(&mut &buf[..], 0)
        .map(|sp| (*sp).clone())
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn length_lying_array_is_rejected_by_the_untrusted_deserializer() {
    let mut rng = StdRng::seed_from_u64(1);
    let (sk, _vk) = keypair(&mut rng);
    let sig = sk.sign(&mut rng, b"anything");

    // In memory the lie is expressible: len() reports 2, iter yields 1.
    let lying = length_two_one_real_sig(&sig);
    assert_eq!(lying.len(), 2, "len() reads the (inflated) annotation");
    assert_eq!(lying.iter_deref().count(), 1, "iter walks the real, smaller content");
    assert_eq!(lying.get(0), Some(&sig));

    // It must NOT survive the untrusted-input boundary.
    let rt = round_trip(&lying);
    assert!(
        rt.is_err(),
        "SECURITY: a length-lying array survived deserialize_sp -- authorization in \
         UnshieldedOffer::well_formed relies on this being rejected. Got: {rt:?}"
    );

    // Sanity: an honest array survives and stays honest.
    let honest: Array<Signature, D> = vec![sig.clone()].into();
    let honest_rt = round_trip(&honest).expect("honest array must round-trip");
    assert_eq!(honest_rt.len(), 1);
    assert_eq!(honest_rt.iter_deref().count(), 1);
}
