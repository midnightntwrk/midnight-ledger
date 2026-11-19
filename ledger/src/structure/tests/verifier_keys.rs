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

//! A proof takes its version from the tag of the verifier key it was made
//! against. The version names the one `ContractOperation` field whose key the
//! proof is checked against, and the fee is priced on that key. The number
//! written for a version is part of the transaction format.

use crate::structure::{
    ContractOperationVersion as Version, ContractOperationVersionedVerifierKey as VersionedKey,
    ProofKind, ProofMarker, ProofVersioned, verifier_key_size,
};
use onchain_runtime::state::ContractOperation;
use serialize::{Deserializable, Serializable};
use storage::db::InMemoryDB;
use transient_crypto::proofs::Proof;

/// A verifier key of any version holding `len` bytes, fewer than 64. Keys are
/// parsed on first use, so the bytes need not be a real key.
fn key<K: Deserializable>(len: usize) -> K {
    let mut ser = Vec::new();
    Serializable::serialize(&vec![0u8; len], &mut ser).unwrap();
    K::deserialize(&mut &ser[..], 0).unwrap()
}

fn serialized(value: &impl Serializable) -> Vec<u8> {
    let mut bytes = Vec::new();
    value.serialize(&mut bytes).unwrap();
    bytes
}

#[test]
fn versions_keep_their_numbers() {
    let keys = [
        (VersionedKey::V3(key(3)), 2u8),
        (VersionedKey::V4(key(3)), 3),
        (VersionedKey::V5(key(3)), 4),
    ];
    for (versioned, number) in keys {
        let version = versioned.as_version();
        assert_eq!(serialized(&version), [number]);
        assert_eq!(
            Version::deserialize(&mut &[number][..], 0).unwrap(),
            version
        );

        let bytes = serialized(&versioned);
        assert_eq!(bytes[0], number);
        assert_eq!(bytes.len(), versioned.serialized_size());
        assert_eq!(
            VersionedKey::deserialize(&mut &bytes[..], 0).unwrap(),
            versioned
        );
    }
}

#[test]
fn proof_is_labelled_and_priced_by_its_verifier_key() {
    // A serialized key is one length byte longer than the bytes it holds.
    let mut op = ContractOperation::new(Some(key(30)), None);
    op.v2 = Some(key(10));
    op.v3 = Some(key(20));
    let priced_size = |tag| {
        let proof = ProofVersioned::of_verifier_key_tag(tag, Proof::from_bytes(Vec::new()))?;
        let version =
            <ProofMarker as ProofKind<InMemoryDB>>::proof_version_to_operation_version(&proof);
        verifier_key_size(&op, version)
    };
    assert_eq!(priced_size("verifier-key[v6]"), Some(11));
    assert_eq!(priced_size("verifier-key[v7]"), Some(21));
    assert_eq!(priced_size("verifier-key[v8]"), Some(31));
    // A transaction not yet proven has no proof version: the newest key is priced.
    assert_eq!(verifier_key_size(&op, None), Some(31));
}
