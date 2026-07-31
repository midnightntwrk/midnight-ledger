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

//! Ledger-level regression tests for the non-canonical MPT encodings, driven
//! end-to-end through the real `tagged_deserialize` path on the actual ledger
//! types (`Transaction`, `UtxoState`, the zswap nullifier set). They confirm
//! that no ledger type bypasses the storage boundary: each crafted non-canonical
//! encoding must be REJECTED at deserialization.
//!
//! The generic storage-level versions live in
//! `storage/tests/canonicality_regression.rs`; this file pins the reachable
//! ledger consequences and asserts the same correct behaviour (rejection).
//!
//! Two vector classes: NON-INJECTIVE KEYS (a canonical key path plus trailing
//! nibbles that alias the same logical key) and a SOUND-BUT-NON-CANONICAL SHAPE
//! (an empty-path `Extension` wrapping a container root).

mod common;
use common::{D, SEG, keypair};

use base_crypto::hash::HashOutput;
use base_crypto::time::Timestamp;
use coin_structure::coin::{NIGHT, Nullifier, UnshieldedTokenType, UserAddress};
use base_crypto::schnorr::Signature;
use midnight_ledger::structure::{
    Intent, IntentHash, ProofPreimageMarker, StandardTransaction, Transaction,
    UnshieldedOffer, Utxo, UtxoMeta, UtxoOutput, UtxoSpend, UtxoState,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serialize::{Deserializable, Serializable, Tagged, tagged_deserialize, tagged_serialize};
use std::marker::PhantomData;
use std::ops::Deref;
use storage::arena::Sp;
use storage::merkle_patricia_trie::{MerklePatriciaTrie, Node};
use storage::storable::SizeAnn;
use storage::storage::{HashMap as StoreHashMap, Map};
use transient_crypto::commitment::PedersenRandomness;

type Tx = Transaction<Signature, ProofPreimageMarker, PedersenRandomness, D>;
type Itnt = Intent<Signature, ProofPreimageMarker, PedersenRandomness, D>;

/// The shared oracle: the canonical encoding must deserialize, the crafted
/// non-canonical one must be a distinct wire form and must be REJECTED by the
/// untrusted `tagged_deserialize` path.
fn assert_only_canonical_accepted<T>(canonical: &T, noncanonical: &T, what: &str)
where
    T: Serializable + Deserializable + Tagged,
{
    let mut canon_bytes = Vec::new();
    tagged_serialize(canonical, &mut canon_bytes).expect("serialize canonical");
    let mut noncanon_bytes = Vec::new();
    tagged_serialize(noncanonical, &mut noncanon_bytes).expect("serialize non-canonical");
    assert_ne!(
        canon_bytes, noncanon_bytes,
        "{what}: the crafted encoding must be a genuinely distinct wire form"
    );
    assert!(
        tagged_deserialize::<T>(&mut &canon_bytes[..]).is_ok(),
        "{what}: the canonical encoding must still deserialize"
    );
    assert!(
        tagged_deserialize::<T>(&mut &noncanon_bytes[..]).is_err(),
        "{what}: the untrusted deserializer must REJECT the non-canonical encoding"
    );
}

fn mk_intent(rng: &mut StdRng, ttl_secs: u64) -> Itnt {
    Intent::new(
        rng,
        None,
        None,
        vec![],
        vec![],
        vec![],
        None,
        Timestamp::from_secs(ttl_secs),
    )
}

fn std_tx(intents: StoreHashMap<u16, Itnt, D>) -> Tx {
    Transaction::Standard(StandardTransaction {
        network_id: "local-test".into(),
        intents,
        guaranteed_coins: None,
        fallible_coins: StoreHashMap::new(),
        binding_randomness: PedersenRandomness::default(),
    })
}

// ===========================================================================
// Non-injective key: a duplicate segment key in `intents`.
//
// If accepted, `Transaction::well_formed`/`balance` iterate and account for BOTH
// intents, while `erase_proofs`/`erase_signatures` rebuild the map via
// `.iter()...collect()`, which collapses the two same-key entries to ONE. The
// transaction is then validated against two intents but applied with one -- a
// validate-vs-apply divergence (GHSA-vhp6-px6f-jv94).
// ===========================================================================
#[test]
fn duplicate_segment_key_intents_rejected() {
    let mut rng = StdRng::seed_from_u64(0x0D0E);
    let intent_a = mk_intent(&mut rng, 1_000_000);
    let intent_b = mk_intent(&mut rng, 2_000_000);
    assert_ne!(intent_a.ttl, intent_b.ttl, "intents must differ");

    // Canonical: intent_a at key 1.
    let canonical_intents: StoreHashMap<u16, Itnt, D> =
        StoreHashMap::new().insert(1, intent_a.clone());

    // Non-canonical: splice a second inner leaf (intent_b) at an over-long path
    // that also decodes to key 1's ArenaHash.
    let (p, existing) = canonical_intents
        .0
        .mpt
        .iter()
        .next()
        .map(|(p, v)| (p, (*v).clone()))
        .expect("one leaf");
    let mut p2 = p.clone();
    p2.extend_from_slice(&[0, 0]); // ignored trailing byte -> aliases key 1
    let dup_value = (existing.0.clone(), Sp::new(intent_b.clone()));
    let dup_inner_mpt = canonical_intents.0.mpt.deref().insert(&p2, dup_value);
    let dup_intents: StoreHashMap<u16, Itnt, D> = StoreHashMap(Map {
        mpt: Sp::new(dup_inner_mpt),
        key_type: PhantomData,
    });
    assert_eq!(dup_intents.size(), 2, "two inner leaves, both under key 1");

    assert_only_canonical_accepted(
        &std_tx(canonical_intents),
        &std_tx(dup_intents),
        "Transaction with duplicate segment-key intents",
    );
}

// ===========================================================================
// Non-injective key: a duplicate UTXO in `UtxoState`.
//
// If accepted, the O(1) `NightAnn` total double-counts the duplicated leaf, so a
// crafted `LedgerState` (this `UtxoState` plus `reserve_pool -= 2V`) passes
// `check_night_balance_invariant` while the true distinct-UTXO NIGHT total is
// short by V -- the NIGHT conservation safety-net is defeated. The same defect
// applies to the `unclaimed_block_rewards`/`bridge_receiving`/`contract` pools
// (`Map<UserAddress,u128,NightAnn>`), by the identical mechanism.
// ===========================================================================
#[test]
fn duplicate_key_utxo_state_rejected() {
    let mut rng = StdRng::seed_from_u64(0x419A7);
    let utxo = Utxo {
        value: 1_000_000_000,
        owner: UserAddress(rng.r#gen()),
        type_: NIGHT,
        intent_hash: IntentHash(rng.r#gen()),
        output_no: 0,
    };
    let meta = UtxoMeta {
        ctime: Timestamp::from_secs(0),
    };
    let canonical = UtxoState::<D>::default().insert(utxo.clone(), meta.clone());

    // Splice a second inner leaf for the SAME utxo at an over-long path.
    let (p, existing) = canonical
        .utxos
        .0
        .mpt
        .iter()
        .next()
        .map(|(p, v)| (p, (*v).clone()))
        .expect("one leaf");
    let mut p2 = p.clone();
    p2.extend_from_slice(&[0, 0]);
    let dup_value = (existing.0.clone(), existing.1.clone());
    let dup_mpt = canonical.utxos.0.mpt.deref().insert(&p2, dup_value);
    let dup_utxos = StoreHashMap(Map {
        mpt: Sp::new(dup_mpt),
        key_type: PhantomData,
    });
    let dup_state = UtxoState { utxos: dup_utxos };
    assert_eq!(dup_state.utxos.ann().value, 2 * utxo.value, "annotation double-counts");

    assert_only_canonical_accepted(
        &canonical,
        &dup_state,
        "UtxoState with a duplicate-key UTXO",
    );
}

// ===========================================================================
// Non-injective key: a nullifier held only at a non-canonical path.
//
// If accepted, the nullifier is present by `size()`/`iter()` but invisible to
// the `contains_key` double-spend / faerie-gold checks (which use the canonical
// path only), so a coin logically in the spent-set reads as ABSENT and can be
// spent again. Also affects `coin_coms_set` and the dust uniqueness sets.
// ===========================================================================
#[test]
fn noncanonical_nullifier_set_rejected() {
    let mut rng = StdRng::seed_from_u64(0x2D5C);
    let n = Nullifier(HashOutput(rng.r#gen()));

    // Canonical: nullifier at its canonical inner path.
    let canonical: StoreHashMap<Nullifier, (), D> = StoreHashMap::new().insert(n, ());
    let (p, val) = canonical
        .0
        .mpt
        .iter()
        .next()
        .map(|(p, v)| (p, (*v).clone()))
        .expect("one leaf");

    // Non-canonical: the same nullifier held ONLY at an over-long path.
    let mut p2 = p.clone();
    p2.extend_from_slice(&[0, 0]);
    let desynced_mpt =
        MerklePatriciaTrie::<(Sp<Nullifier, D>, Sp<()>), D>::new().insert(&p2, val);
    let desynced: StoreHashMap<Nullifier, (), D> = StoreHashMap(Map {
        mpt: Sp::new(desynced_mpt),
        key_type: PhantomData,
    });
    assert!(!desynced.contains_key(&n), "sanity: contains_key misses the non-canonical path");

    assert_only_canonical_accepted(
        &canonical,
        &desynced,
        "nullifier set with the key at a non-canonical path",
    );
}

// ===========================================================================
// Sound-but-non-canonical SHAPE: empty-path `Extension` wrapping the
// outer `intents` HashMap of an already-signed transaction.
//
// If accepted, the transaction has a second valid wire form with a DIFFERENT
// `transaction_hash` while `data_to_sign`/`intent_hash` (and every signature)
// are unchanged -- transaction-id malleability (mempool/dedup/tracking
// confusion). Bounded: replay protection keys on `intent_hash`, not the txid.
// ===========================================================================
#[test]
fn noncanonical_intents_shape_rejected() {
    let mut rng = StdRng::seed_from_u64(0x27A11);
    let tt = UnshieldedTokenType(rng.r#gen());
    let (sk, vk) = keypair(&mut rng);

    let input = UtxoSpend {
        value: 1000,
        owner: vk.clone(),
        type_: tt,
        intent_hash: IntentHash(rng.r#gen()),
        output_no: 0,
    };
    let out = UtxoOutput {
        value: 1000,
        owner: UserAddress::from(vk.clone()),
        type_: tt,
    };
    let unsigned: UnshieldedOffer<Signature, D> = UnshieldedOffer {
        inputs: vec![input].into(),
        outputs: vec![out].into(),
        signatures: storage::storage::Array::new(),
    };
    let intent: Itnt = Intent::new(
        &mut rng,
        Some(unsigned),
        None,
        vec![],
        vec![],
        vec![],
        None,
        Timestamp::from_secs(1_000_000),
    );
    let signed_intent = intent.sign(&mut rng, SEG, &[sk.clone()], &[], &[]).expect("sign");
    let canonical_intents = StoreHashMap::<u16, Itnt, D>::new().insert(SEG, signed_intent);

    // Non-canonical: wrap the intents' inner trie root in an empty-path
    // Extension (correct annotation -> sound; distinct bytes -> distinct txid).
    let wrapped_root = Node::Extension {
        ann: SizeAnn(canonical_intents.size() as u64),
        compressed_path: Vec::new(),
        child: canonical_intents.0.mpt.0.clone(),
    };
    let wrapped_intents: StoreHashMap<u16, Itnt, D> = StoreHashMap(Map {
        mpt: Sp::new(MerklePatriciaTrie(Sp::new(wrapped_root))),
        key_type: PhantomData,
    });

    assert_only_canonical_accepted(
        &std_tx(canonical_intents),
        &std_tx(wrapped_intents),
        "Transaction with an empty-path-Extension-wrapped intents map",
    );
}
